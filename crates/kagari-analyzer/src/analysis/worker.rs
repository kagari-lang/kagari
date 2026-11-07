//! A bounded bridge carrying typed closures, never semantic database payloads.
use crate::analysis::{AnalysisState, diagnostics::ProjectionLimits};
use crate::workspace::budget::InputBudget;
use kagari_common::cancellation::CancellationToken;
use std::{
    error::Error,
    fmt, io,
    sync::mpsc::{SyncSender, TrySendError, sync_channel},
    thread::{Builder, JoinHandle},
};
use tokio::sync::oneshot;

type Task = Box<dyn FnOnce(&mut AnalysisState) + Send>;

pub(crate) struct AnalysisPort(SyncSender<Task>);

pub(crate) struct AnalysisWorker {
    pub port: AnalysisPort,
    pub thread: JoinHandle<()>,
    pub terminated: oneshot::Receiver<Result<(), String>>,
}

#[derive(Debug)]
pub(crate) enum SubmitError {
    Full,
    Closed,
}

impl fmt::Display for SubmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Full => "analysis submission without a free worker credit",
            Self::Closed => "analysis worker has stopped",
        })
    }
}

impl Error for SubmitError {}

impl AnalysisPort {
    /// Register the typed deferred completion before submitting. Cancellation is
    /// interpreted by the concrete operation; its terminal reply is never skipped.
    pub fn submit<R, F>(
        &self,
        task: F,
        cancel: CancellationToken,
        reply: oneshot::Sender<R>,
    ) -> Result<(), SubmitError>
    where
        R: Send + 'static,
        F: FnOnce(&mut AnalysisState, &CancellationToken) -> R + Send + 'static,
    {
        self.0
            .try_send(Box::new(move |state| {
                let result = task(state, &cancel);
                let _ = reply.send(result);
            }))
            .map_err(|error| match error {
                TrySendError::Full(_) => SubmitError::Full,
                TrySendError::Disconnected(_) => SubmitError::Closed,
            })
    }
}

struct Termination {
    sender: Option<oneshot::Sender<Result<(), String>>>,
    orderly: bool,
}

impl Drop for Termination {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let result = if self.orderly {
                Ok(())
            } else {
                Err("analysis worker exited unexpectedly".into())
            };
            let _ = sender.send(result);
        }
    }
}

pub(crate) fn start(
    source_root: String,
    budget: InputBudget,
    projection_limits: ProjectionLimits,
) -> io::Result<AnalysisWorker> {
    let (sender, receiver) = sync_channel::<Task>(1);
    let (terminated, status) = oneshot::channel();
    let thread = Builder::new()
        .name("kagari-lsp-analysis".into())
        .spawn(move || {
            let mut termination = Termination {
                sender: Some(terminated),
                orderly: false,
            };
            let mut state = AnalysisState::new(source_root, budget, projection_limits);
            for task in receiver {
                task(&mut state);
            }
            termination.orderly = true;
        })?;
    Ok(AnalysisWorker {
        port: AnalysisPort(sender),
        thread,
        terminated: status,
    })
}
