//! Debounced context work returns owned diagnostics through one typed completion.
use crate::{
    analysis::{AnalysisFailure, diagnostics::DiagnosticsResult},
    documents::DocumentInput,
    session::{
        Lifecycle, SessionActor, analysis::Environment, publications::ContributionOwner,
        timers::Deadline,
    },
    workspace::{ContextKey, WorkspaceCapture},
};
use kagari_common::cancellation::CancellationToken;
use lattice_actor::{
    context::{ActorContext, HandlerContext},
    traits::Handler,
};
use std::{
    collections::VecDeque,
    io::{self, Error},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, oneshot::error::RecvError};

#[derive(Clone)]
struct DiagnosticTask {
    capture: Arc<WorkspaceCapture>,
    context: Option<ContextKey>,
    documents: Arc<[DocumentInput]>,
}

#[derive(Default)]
pub(crate) struct DiagnosticScheduler {
    pending: VecDeque<DiagnosticTask>,
    pub due: bool,
}

impl DiagnosticScheduler {
    pub fn shutdown(&mut self) {
        self.pending.clear();
        self.due = false;
    }
}

#[derive(lattice_actor::Message)]
struct DiagnosticsFinished {
    job: u64,
    epoch: u64,
    generation: u64,
    context: Option<ContextKey>,
    label: String,
    retry: DiagnosticTask,
    result: Result<Result<DiagnosticsResult, AnalysisFailure>, RecvError>,
}

impl SessionActor {
    pub(super) fn schedule_diagnostics(&mut self, ctx: &mut ActorContext<Self>) -> io::Result<()> {
        if !self.diagnostics.due {
            return Ok(());
        }
        let Some(task) = self.diagnostics.pending.pop_front() else {
            return Ok(());
        };
        let token = CancellationToken::default();
        let job = self.analysis.credit.reserve(token.clone())?;
        let epoch = self.analysis.epoch;
        let generation = task.capture.generation;
        let context = task.context.clone();
        let semantic_ready = matches!(self.analysis.environment, Environment::Ready);
        #[cfg(test)]
        let gate = self.analysis.gate.clone();
        let label = context
            .as_ref()
            .and_then(|key| {
                task.capture
                    .contexts
                    .iter()
                    .find(|context| &context.key == key)
            })
            .and_then(|context| {
                context
                    .sources
                    .module(&context.root)
                    .and_then(|source| context.origins.get(&source.id()))
            })
            .and_then(|origin| origin.uri.as_ref())
            .map(|uri| uri.as_str().to_owned())
            .unwrap_or_else(|| "workspace inputs".into());
        let (sender, receiver) = oneshot::channel();
        let retry = task.clone();
        ctx.pipe_to_self(receiver, move |result| DiagnosticsFinished {
            job,
            epoch,
            generation,
            context,
            label,
            retry,
            result,
        })
        .map_err(Error::other)?;
        let live_serials = self.requests.unresolved().into_iter().collect();
        let latest_capture = self.captures.latest.clone();
        let mut retained = self.operations.retained_contexts();
        retained.extend(self.analysis.live_contexts.iter().cloned());
        self.analysis.maintenance = false;
        retained.extend(
            task.capture
                .contexts
                .iter()
                .map(|context| context.key.clone()),
        );
        self.analysis
            .port
            .as_ref()
            .ok_or_else(|| Error::other("analysis worker unavailable"))?
            .submit(
                move |state, cancel| {
                    if !semantic_ready {
                        state.discard_environment();
                    }
                    state.retain_contexts(&retained);
                    state.retain_requests(&live_serials);
                    if let Some(capture) = &latest_capture {
                        state.retain_capture_inputs(capture);
                    }
                    let result = state.diagnostics(
                        &task.capture,
                        task.context.as_ref(),
                        &task.documents,
                        cancel,
                    );
                    #[cfg(test)]
                    if result.is_ok()
                        && task.context.is_some()
                        && let Some(gate) = gate
                    {
                        gate.hold(job, generation);
                    }
                    result
                },
                token,
                sender,
            )
            .map_err(Error::other)?;
        self.analysis.preemptible = true;
        Ok(())
    }
}

impl Handler<DiagnosticsFinished> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: DiagnosticsFinished,
    ) -> io::Result<()> {
        self.analysis.credit.complete(message.job)?;
        self.analysis.preemptible = false;
        #[cfg(test)]
        self.analysis
            .completed
            .send_replace((message.job, message.generation));
        let result = message
            .result
            .map_err(|_| Error::other("analysis worker lost diagnostic result"))?;
        if self.state != Lifecycle::Running
            || message.epoch != self.analysis.epoch
            || message.generation != self.captures.generation
        {
            return Ok(());
        }
        match result {
            Ok(result) => {
                let owner = match message.context {
                    Some(context) => ContributionOwner::Context(context),
                    None => ContributionOwner::Acquisition,
                };
                self.publications.replace(
                    message.generation,
                    owner,
                    message.label,
                    result.groups,
                )?;
                self.logger.write(format!(
                    "diagnostics {}: {} retained publication bytes",
                    message.generation,
                    self.publications.retained_bytes()
                ));
                for issue in result.issues.into_iter().take(8) {
                    self.report_problem(issue)?;
                }
            }
            Err(AnalysisFailure::Cancelled) => self.diagnostics.pending.push_front(message.retry),
            Err(AnalysisFailure::Capacity(message)) => return Err(Error::other(message)),
            Err(error) => self.report_problem(format!("diagnostic analysis failed: {error}"))?,
        }
        Ok(())
    }
}

impl SessionActor {
    pub fn invalidate_diagnostics(&mut self) -> io::Result<()> {
        self.diagnostics.pending.clear();
        self.diagnostics.due = false;
        self.analysis.maintenance = true;
        if self.analysis.preemptible {
            self.analysis.credit.cancel();
        }
        self.publications
            .invalidate(self.captures.generation, &self.documents.inputs())?;
        self.timers.set(
            Deadline::Diagnostics,
            Instant::now() + Duration::from_millis(150),
        );
        Ok(())
    }

    pub fn accept_diagnostic_capture(&mut self, capture: Arc<WorkspaceCapture>) {
        let documents: Arc<[DocumentInput]> = self.documents.inputs().into();
        self.diagnostics.pending.clear();
        // Acquisition errors have their own contribution, including an empty workspace.
        self.diagnostics.pending.push_back(DiagnosticTask {
            capture: capture.clone(),
            context: None,
            documents: documents.clone(),
        });
        for context in &capture.contexts {
            self.diagnostics.pending.push_back(DiagnosticTask {
                capture: capture.clone(),
                context: Some(context.key.clone()),
                documents: documents.clone(),
            });
        }
    }
}
