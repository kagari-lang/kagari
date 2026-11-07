//! One synchronous acquisition owner behind a bounded, typed job bridge.
use crate::workspace::{
    CaptureFailure, CaptureRequest, WorkspaceCapture, budget::InputBudget, capture::CaptureState,
};
use kagari_common::cancellation::CancellationToken;
use std::{
    error::Error,
    fmt, io,
    sync::mpsc::{SyncSender, TrySendError, sync_channel},
    thread::{Builder, JoinHandle},
};
use tokio::sync::oneshot;

struct CaptureJob {
    request: CaptureRequest,
    cancel: CancellationToken,
    reply: oneshot::Sender<Result<WorkspaceCapture, CaptureFailure>>,
}

pub(crate) struct AcquisitionPort(SyncSender<CaptureJob>);

pub(crate) struct AcquisitionWorker {
    pub port: AcquisitionPort,
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
            Self::Full => "acquisition submission without a free worker credit",
            Self::Closed => "acquisition worker has stopped",
        })
    }
}

impl Error for SubmitError {}

impl AcquisitionPort {
    /// Register the matching actor deferred wait before calling this method.
    /// Failure drops the unsent job and its reply sender; no work has started.
    pub fn submit(
        &self,
        request: CaptureRequest,
        cancel: CancellationToken,
        reply: oneshot::Sender<Result<WorkspaceCapture, CaptureFailure>>,
    ) -> Result<(), SubmitError> {
        self.0
            .try_send(CaptureJob {
                request,
                cancel,
                reply,
            })
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
                Err("acquisition worker exited unexpectedly".into())
            };
            let _ = sender.send(result);
        }
    }
}

/// Start the sole acquisition thread. Dropping its port closes admission;
/// the current operation still owns its cancellation and terminal reply.
pub(crate) fn start(
    normalization_root: String,
    budget: InputBudget,
) -> io::Result<AcquisitionWorker> {
    let (sender, receiver) = sync_channel::<CaptureJob>(1);
    let (terminated, status) = oneshot::channel();
    let thread = Builder::new()
        .name("kagari-lsp-acquisition".into())
        .spawn(move || {
            let mut termination = Termination {
                sender: Some(terminated),
                orderly: false,
            };
            let mut state = CaptureState::new(normalization_root, budget);
            for job in receiver {
                let result = state.capture(job.request, &job.cancel);
                let _ = job.reply.send(result);
            }
            termination.orderly = true;
        })?;
    Ok(AcquisitionWorker {
        port: AcquisitionPort(sender),
        thread,
        terminated: status,
    })
}
