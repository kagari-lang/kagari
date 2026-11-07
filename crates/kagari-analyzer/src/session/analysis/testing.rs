//! Test-only completion hold after real analysis, outside the actor turn.
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    time::Duration,
};

#[derive(Clone)]
pub(crate) struct CompletionGate {
    armed: Arc<AtomicBool>,
    ready: SyncSender<HeldCompletion>,
}

#[derive(Debug)]
pub(crate) struct HeldCompletion {
    pub job: u64,
    pub generation: u64,
    /// Dropping this sender also releases the worker during assertion unwinding.
    pub release: SyncSender<()>,
}

impl CompletionGate {
    pub fn new() -> (Self, Receiver<HeldCompletion>) {
        let (ready, receiver) = sync_channel(1);
        (
            Self {
                armed: Arc::new(AtomicBool::new(true)),
                ready,
            },
            receiver,
        )
    }

    pub fn hold(&self, job: u64, generation: u64) {
        if !self.armed.swap(false, Ordering::AcqRel) {
            return;
        }
        let (release, receiver) = sync_channel(1);
        self.ready
            .try_send(HeldCompletion {
                job,
                generation,
                release,
            })
            .expect("one test completion hold");
        match receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
            Err(error) => panic!("diagnostic completion hold expired: {error}"),
        }
    }
}
