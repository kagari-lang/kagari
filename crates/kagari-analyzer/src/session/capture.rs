//! Input generation barriers and the single acquisition-worker credit.
use crate::{
    protocol::{Notification, source_name},
    session::{Lifecycle, SessionActor, jobs::JobCredit},
    workspace::{
        CaptureFailure, CaptureRequest, WorkspaceCapture,
        acquisition::{self, AcquisitionPort},
        budget::InputBudget,
    },
};
use kagari_common::cancellation::CancellationToken;
use lattice_actor::{
    context::{ActorContext, HandlerContext},
    traits::{Handler, Message},
};
use lsp_types::{InitializeParams, LogMessageParams, MessageType, Uri};
use serde_json::to_value;
use std::{
    collections::HashSet,
    io::{self, Error},
    sync::Arc,
};
#[cfg(test)]
use tokio::sync::watch;
use tokio::sync::{oneshot, oneshot::error::RecvError};

pub(crate) struct CaptureState {
    pub normalization_root: String,
    pub budget: InputBudget,
    pub roots: Vec<Uri>,
    pub generation: u64,
    pending: Option<CaptureRequest>,
    active: Option<(u64, CancellationToken)>,
    pub port: Option<AcquisitionPort>,
    pub credit: JobCredit,
    pub latest: Option<Arc<WorkspaceCapture>>,
    #[cfg(test)]
    observed: watch::Sender<Option<Arc<WorkspaceCapture>>>,
}

impl CaptureState {
    pub fn new(normalization_root: String, retained_input_bytes: usize) -> Self {
        Self {
            normalization_root,
            roots: Vec::new(),
            generation: 0,
            budget: InputBudget::new(retained_input_bytes),
            pending: None,
            active: None,
            port: None,
            credit: JobCredit::default(),
            latest: None,
            #[cfg(test)]
            observed: watch::channel(None).0,
        }
    }

    pub fn configure(&mut self, params: &InitializeParams) -> Result<(), String> {
        // LSP retains rootUri as the fallback for clients without workspaceFolders.
        #[allow(deprecated)]
        let supplied = params
            .workspace_folders
            .as_ref()
            .map(|folders| {
                folders
                    .iter()
                    .map(|folder| folder.uri.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| params.root_uri.clone().into_iter().collect());
        if supplied.len() > 32
            || supplied.iter().map(|uri| uri.as_str().len()).sum::<usize>() > 64 * 1024
        {
            return Err("workspace roots exceed metadata limits".into());
        }
        let mut names = HashSet::new();
        let mut roots = Vec::new();
        for uri in supplied {
            if !uri
                .scheme()
                .is_some_and(|scheme| scheme.as_str().eq_ignore_ascii_case("file"))
            {
                return Err("workspace roots require local file URIs".into());
            }
            let name = source_name(&uri)?;
            if names.insert(name) {
                roots.push(uri);
            }
        }
        self.roots = roots;
        Ok(())
    }

    pub fn shutdown(&mut self) {
        self.pending = None;
        self.latest = None;
        self.credit.cancel();
        if let Some((_, cancel)) = &self.active {
            cancel.cancel();
        }
        self.port = None;
    }

    pub fn is_idle(&self) -> bool {
        self.credit.is_idle()
    }

    #[cfg(test)]
    pub fn subscribe(&self) -> watch::Receiver<Option<Arc<WorkspaceCapture>>> {
        self.observed.subscribe()
    }
}

pub(crate) struct CaptureFinished {
    job: u64,
    generation: u64,
    result: Result<Result<WorkspaceCapture, CaptureFailure>, RecvError>,
}

impl Message for CaptureFinished {}

pub(crate) struct AcquisitionExited(Result<Result<(), String>, RecvError>);

impl Message for AcquisitionExited {}

impl SessionActor {
    pub fn start_acquisition(&mut self, ctx: &mut ActorContext<Self>) -> io::Result<()> {
        let worker = acquisition::start(
            self.captures.normalization_root.clone(),
            self.captures.budget.clone(),
        )?;
        ctx.pipe_to_self(worker.terminated, AcquisitionExited)
            .map_err(Error::other)?;
        self.threads
            .lock()
            .map_err(|_| Error::other("thread owner poisoned"))?
            .push(worker.thread);
        self.captures.port = Some(worker.port);
        self.invalidate_inputs()
    }

    pub fn invalidate_inputs(&mut self) -> io::Result<()> {
        self.captures.generation = self
            .captures
            .generation
            .checked_add(1)
            .ok_or_else(|| Error::other("input generation exhausted"))?;
        self.captures.latest = None;
        self.invalidate_analysis()?;
        if let Some((_, cancel)) = &self.captures.active {
            cancel.cancel();
        }
        self.captures.pending = None;
        match CaptureRequest::new(
            self.captures.generation,
            self.captures.roots.clone(),
            self.documents.inputs(),
            &self.captures.budget,
        ) {
            Ok(request) => self.captures.pending = Some(request),
            Err(error) => self.report_problem(error.to_string())?,
        }
        Ok(())
    }

    pub fn schedule_capture(&mut self, ctx: &mut ActorContext<Self>) -> io::Result<()> {
        if self.state != Lifecycle::Running || !self.captures.credit.is_idle() {
            return Ok(());
        }
        if self.schedule_environment_acquisition(ctx)? {
            return Ok(());
        }
        let Some(request) = self.captures.pending.take() else {
            return Ok(());
        };
        let generation = request.generation;
        let cancel = CancellationToken::default();
        let job = self.captures.credit.reserve(cancel.clone())?;
        let (reply, receiver) = oneshot::channel();
        // Reserve deferred completion before handing a job to the worker.
        ctx.pipe_to_self(receiver, move |result| CaptureFinished {
            job,
            generation,
            result,
        })
        .map_err(Error::other)?;
        self.captures
            .port
            .as_ref()
            .ok_or_else(|| Error::other("acquisition worker unavailable"))?
            .submit(request, cancel.clone(), reply)
            .map_err(Error::other)?;
        self.captures.active = Some((generation, cancel));
        Ok(())
    }

    pub(super) fn report_problem(&mut self, message: String) -> io::Result<()> {
        self.logger.write(message.clone());
        // Unlocated input/environment failures remain visible in the client log.
        let text: String = message.chars().take(512).collect();
        let params = to_value(LogMessageParams {
            typ: MessageType::ERROR,
            message: text,
        })
        .map_err(Error::other)?;
        self.outbox.push(
            Notification::new("window/logMessage".into(), params).into(),
            None,
            4096,
        )
    }

    fn validate_current_capture(&self, capture: &WorkspaceCapture) -> io::Result<()> {
        if capture.generation != self.captures.generation
            || capture.retained_bytes() > self.limits.retained_input_bytes
        {
            return Err(Error::other(
                "capture generation or retained-input accounting mismatch",
            ));
        }
        let documents = self.documents.inputs();
        for context in &capture.contexts {
            if context
                .origins
                .values()
                .any(|origin| origin.uri.is_none() && !origin.read_only)
            {
                return Err(Error::other("editable captured source has no document URI"));
            }
        }
        for (name, selected) in &capture.document_contexts {
            let document = documents
                .iter()
                .find(|document| &document.source_name == name)
                .ok_or_else(|| Error::other("capture refers to a closed document"))?;
            let context = capture
                .contexts
                .iter()
                .find(|context| context.key == selected.context)
                .ok_or_else(|| Error::other("capture refers to an absent context"))?;
            if document.version != selected.version
                || document.incarnation != selected.incarnation
                || context.sources.file(selected.file).is_none()
                || !context.origins.contains_key(&selected.file)
            {
                return Err(Error::other("capture document identity mismatch"));
            }
        }
        Ok(())
    }
}

impl Handler<CaptureFinished> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: CaptureFinished,
    ) -> io::Result<()> {
        self.captures.credit.complete(message.job)?;
        let Some((generation, _)) = self.captures.active.take() else {
            return Err(Error::other("capture completion without worker credit"));
        };
        if generation != message.generation {
            return Err(Error::other(
                "capture generation does not own worker credit",
            ));
        }
        let result = match message.result {
            Ok(result) => result,
            Err(_) => {
                self.fail(ctx, "acquisition worker lost its result".into());
                return Ok(());
            }
        };
        if self.state == Lifecycle::Running && generation == self.captures.generation {
            match result {
                Ok(capture) => {
                    self.validate_current_capture(&capture)?;
                    let complete = capture
                        .contexts
                        .iter()
                        .filter(|context| context.complete)
                        .count();
                    let semantic = capture
                        .contexts
                        .iter()
                        .filter(|context| context.semantic_available)
                        .count();
                    self.logger.write(format!(
                        "capture {generation}: {} contexts, {complete} complete, {semantic} available for semantics, {} retained bytes",
                        capture.contexts.len(), capture.retained_bytes(),
                    ));
                    let capture = Arc::new(capture);
                    self.captures.latest = Some(capture.clone());
                    self.accept_analysis_capture(capture.clone());
                    #[cfg(test)]
                    self.captures.observed.send_replace(Some(capture));
                }
                Err(error) => self.report_problem(error.to_string())?,
            }
        }
        Ok(())
    }
}

impl Handler<AcquisitionExited> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: AcquisitionExited,
    ) -> io::Result<()> {
        match message.0 {
            Ok(Ok(())) if self.state != Lifecycle::Running => {}
            result => self.fail(ctx, format!("acquisition worker terminated: {result:?}")),
        }
        Ok(())
    }
}
