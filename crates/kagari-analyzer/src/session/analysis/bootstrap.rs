//! Validated native declarations become usable only after publication acknowledgement.
use crate::{
    analysis::{AnalysisFailure, diagnostics::ProjectionLimits, worker},
    session::{Lifecycle, SessionActor, analysis::Environment},
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::native::render::NATIVE_RENDERER_NAMESPACE;
use kagari_source::generated::{GeneratedDocument, GeneratedDocuments};
use lattice_actor::{
    context::{ActorContext, HandlerContext},
    traits::{Handler, Message},
};
use std::{
    env,
    io::{self, Error},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::{oneshot, oneshot::error::RecvError};

struct StoreOpened {
    job: u64,
    epoch: u64,
    result: Result<Result<GeneratedDocuments, String>, RecvError>,
}

struct EnvironmentPrepared {
    job: u64,
    epoch: u64,
    result: Result<Result<Arc<[GeneratedDocument]>, AnalysisFailure>, RecvError>,
}

struct EnvironmentPublished {
    job: u64,
    epoch: u64,
    result: Result<Result<(), String>, RecvError>,
}

struct AnalysisExited(Result<Result<(), String>, RecvError>);

impl Message for StoreOpened {}

impl Message for EnvironmentPrepared {}

impl Message for EnvironmentPublished {}

impl Message for AnalysisExited {}

fn cache_root() -> PathBuf {
    #[cfg(windows)]
    if let Some(root) = env::var_os("LOCALAPPDATA") {
        return PathBuf::from(root).join("kagari");
    }
    if let Some(root) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(root).join("kagari");
    }
    if let Some(root) = env::var_os("HOME") {
        return PathBuf::from(root).join(".cache").join("kagari");
    }
    env::temp_dir().join("kagari")
}

impl SessionActor {
    pub fn start_analysis(&mut self, ctx: &mut ActorContext<Self>) -> io::Result<()> {
        let limits = ProjectionLimits {
            uris: self.limits.publication_uris,
            diagnostics: self.limits.publication_diagnostics,
            bytes: self.limits.publication_bytes,
        };
        let worker = worker::start(
            self.captures.normalization_root.clone(),
            self.captures.budget.clone(),
            limits,
        )?;
        ctx.pipe_to_self(worker.terminated, AnalysisExited)
            .map_err(Error::other)?;
        self.threads
            .lock()
            .map_err(|_| Error::other("thread owner poisoned"))?
            .push(worker.thread);
        self.analysis.port = Some(worker.port);
        Ok(())
    }

    pub fn schedule_environment_acquisition(
        &mut self,
        ctx: &mut ActorContext<Self>,
    ) -> io::Result<bool> {
        if !self.captures.credit.is_idle() {
            return Ok(false);
        }
        let epoch = self.analysis.epoch;
        match &self.analysis.environment {
            Environment::NeedStore => {
                let token = CancellationToken::default();
                let job = self.captures.credit.reserve(token.clone())?;
                let (sender, receiver) = oneshot::channel();
                ctx.pipe_to_self(receiver, move |result| StoreOpened { job, epoch, result })
                    .map_err(Error::other)?;
                self.captures
                    .port
                    .as_ref()
                    .ok_or_else(|| Error::other("acquisition worker unavailable"))?
                    .open_generated(
                        cache_root(),
                        NATIVE_RENDERER_NAMESPACE.to_owned(),
                        token,
                        sender,
                    )
                    .map_err(Error::other)?;
                self.analysis.environment = Environment::OpeningStore;
                Ok(true)
            }
            Environment::NeedPublication(documents) => {
                let token = CancellationToken::default();
                let job = self.captures.credit.reserve(token.clone())?;
                let (sender, receiver) = oneshot::channel();
                ctx.pipe_to_self(receiver, move |result| EnvironmentPublished {
                    job,
                    epoch,
                    result,
                })
                .map_err(Error::other)?;
                self.captures
                    .port
                    .as_ref()
                    .ok_or_else(|| Error::other("acquisition worker unavailable"))?
                    .publish(documents.clone(), token, sender)
                    .map_err(Error::other)?;
                self.analysis.environment = Environment::Publishing;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub(super) fn schedule_environment_analysis(
        &mut self,
        ctx: &mut ActorContext<Self>,
    ) -> io::Result<bool> {
        let Environment::NeedPreparation(cache) = &self.analysis.environment else {
            return Ok(false);
        };
        let cache = cache.clone();
        let epoch = self.analysis.epoch;
        let token = CancellationToken::default();
        let job = self.analysis.credit.reserve(token.clone())?;
        let (sender, receiver) = oneshot::channel();
        ctx.pipe_to_self(receiver, move |result| EnvironmentPrepared {
            job,
            epoch,
            result,
        })
        .map_err(Error::other)?;
        self.analysis
            .port
            .as_ref()
            .ok_or_else(|| Error::other("analysis worker unavailable"))?
            .submit(
                move |state, cancel| state.prepare(&cache, cancel),
                token,
                sender,
            )
            .map_err(Error::other)?;
        self.analysis.environment = Environment::Preparing;
        Ok(true)
    }

    fn environment_failed(&mut self, message: String) -> io::Result<()> {
        self.analysis.environment = Environment::Failed;
        self.report_problem(format!(
            "standard declaration environment unavailable: {message}"
        ))
    }
}

impl Handler<StoreOpened> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: StoreOpened,
    ) -> io::Result<()> {
        self.captures.credit.complete(message.job)?;
        if self.state != Lifecycle::Running || message.epoch != self.analysis.epoch {
            return Ok(());
        }
        match message.result {
            Ok(Ok(store)) => self.analysis.environment = Environment::NeedPreparation(store),
            Ok(Err(error)) => self.environment_failed(error)?,
            Err(_) => return Err(Error::other("acquisition worker lost store result")),
        }
        Ok(())
    }
}

impl Handler<EnvironmentPrepared> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: EnvironmentPrepared,
    ) -> io::Result<()> {
        self.analysis.credit.complete(message.job)?;
        if self.state != Lifecycle::Running || message.epoch != self.analysis.epoch {
            return Ok(());
        }
        match message.result {
            Ok(Ok(documents)) => {
                self.analysis.environment = Environment::NeedPublication(documents)
            }
            Ok(Err(error)) => self.environment_failed(error.to_string())?,
            Err(_) => return Err(Error::other("analysis worker lost environment result")),
        }
        Ok(())
    }
}

impl Handler<EnvironmentPublished> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: EnvironmentPublished,
    ) -> io::Result<()> {
        self.captures.credit.complete(message.job)?;
        if self.state != Lifecycle::Running || message.epoch != self.analysis.epoch {
            return Ok(());
        }
        match message.result {
            Ok(Ok(())) => self.analysis.environment = Environment::Ready,
            Ok(Err(error)) => self.environment_failed(error)?,
            Err(_) => return Err(Error::other("acquisition worker lost publication result")),
        }
        Ok(())
    }
}

impl Handler<AnalysisExited> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: AnalysisExited,
    ) -> io::Result<()> {
        match message.0 {
            Ok(Ok(())) if self.state != Lifecycle::Running => {}
            result => self.fail(ctx, format!("analysis worker terminated: {result:?}")),
        }
        Ok(())
    }
}
