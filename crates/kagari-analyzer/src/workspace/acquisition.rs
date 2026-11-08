//! One synchronous acquisition owner behind a bounded, typed job bridge.
use crate::workspace::{
    CaptureFailure, CaptureRequest, WorkspaceCapture, budget::InputBudget, capture::CaptureState,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::generated::{GeneratedDocument, GeneratedDocuments};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
    thread::{Builder, JoinHandle},
};
use tokio::sync::oneshot;

type Task = Box<dyn FnOnce(&mut CaptureState) + Send>;

pub(crate) struct AcquisitionPort(SyncSender<Task>);

pub(crate) struct AcquisitionWorker {
    pub port: AcquisitionPort,
    pub thread: JoinHandle<()>,
    pub terminated: oneshot::Receiver<Result<(), String>>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SubmitError {
    #[error("acquisition submission without a free worker credit")]
    Full,
    #[error("acquisition worker has stopped")]
    Closed,
}

impl AcquisitionPort {
    /// Register the matching actor deferred wait before calling this method.
    /// Failure drops the unsent job and its reply sender; no work has started.
    pub fn submit(
        &self,
        request: CaptureRequest,
        cancel: CancellationToken,
        reply: oneshot::Sender<Result<WorkspaceCapture, CaptureFailure>>,
    ) -> Result<(), SubmitError> {
        self.send(Box::new(move |state| {
            let result = state.capture(request, &cancel);
            let _ = reply.send(result);
        }))
    }

    /// Open the native document store on the existing IO owner.
    pub fn open_generated(
        &self,
        root: PathBuf,
        namespace: String,
        cancel: CancellationToken,
        reply: oneshot::Sender<Result<GeneratedDocuments, String>>,
    ) -> Result<(), SubmitError> {
        self.send(Box::new(move |_| {
            let result = (|| {
                check(&cancel)?;
                let documents = GeneratedDocuments::open(&root, &namespace)
                    .map_err(|error| error.to_string())?;
                check(&cancel)?;
                Ok(documents)
            })();
            let _ = reply.send(result);
        }))
    }

    /// Materialize only the batch already validated by the analysis owner.
    pub fn publish(
        &self,
        documents: Arc<[GeneratedDocument]>,
        cancel: CancellationToken,
        reply: oneshot::Sender<Result<(), String>>,
    ) -> Result<(), SubmitError> {
        self.send(Box::new(move |_| {
            let result = (|| {
                for document in documents.iter() {
                    check(&cancel)?;
                    document.publish().map_err(|error| error.to_string())?;
                }
                check(&cancel)
            })();
            let _ = reply.send(result);
        }))
    }

    fn send(&self, task: Task) -> Result<(), SubmitError> {
        self.0.try_send(task).map_err(|error| match error {
            TrySendError::Full(_) => SubmitError::Full,
            TrySendError::Disconnected(_) => SubmitError::Closed,
        })
    }
}

fn check(cancel: &CancellationToken) -> Result<(), String> {
    cancel
        .check()
        .map_err(|_| "source acquisition cancelled".into())
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
    let (sender, receiver) = sync_channel::<Task>(1);
    let (terminated, status) = oneshot::channel();
    let thread = Builder::new()
        .name("kagari-lsp-acquisition".into())
        .spawn(move || {
            let mut termination = Termination {
                sender: Some(terminated),
                orderly: false,
            };
            let mut state = CaptureState::new(normalization_root, budget);
            for task in receiver {
                task(&mut state);
            }
            termination.orderly = true;
        })?;
    Ok(AcquisitionWorker {
        port: AcquisitionPort(sender),
        thread,
        terminated: status,
    })
}
