//! Process composition; Lattice owns the sole asynchronous executor.
use std::io::Error;
pub(crate) mod options;

use crate::{
    logging::LogSink,
    server::options::{Endpoint, SessionLimits},
    session::SessionActor,
};
use lattice_actor::{
    mailbox::MailboxConfig,
    observation::{ActorLifecycleEvent, ActorMetadata, ActorObserver, ActorObserverHandle},
    runtime::{ActorRuntime, ActorRuntimeConfig, ActorSpawnOptions},
};
use std::{
    env, io,
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    thread::JoinHandle,
    time::Instant,
};

pub(crate) enum ProcessNotice {
    Cleanup,
    Stopped,
}

struct Observer(SyncSender<ProcessNotice>);

impl ActorObserver for Observer {
    fn lifecycle(&self, _: &ActorMetadata, event: ActorLifecycleEvent) {
        if matches!(
            event,
            ActorLifecycleEvent::Stopped(_)
                | ActorLifecycleEvent::StartFailed
                | ActorLifecycleEvent::Panicked
                | ActorLifecycleEvent::StopFailed(_)
        ) {
            let _ = self.0.try_send(ProcessNotice::Stopped);
        }
    }
}

pub(crate) type ThreadOwners = Arc<Mutex<Vec<JoinHandle<()>>>>;

pub(crate) fn run() -> io::Result<ExitCode> {
    let Some(endpoint) = options::command_line()? else {
        return Ok(ExitCode::SUCCESS);
    };
    run_endpoint(endpoint, SessionLimits::default())
}

pub(crate) fn run_endpoint(endpoint: Endpoint, limits: SessionLimits) -> io::Result<ExitCode> {
    limits.validate()?;
    let root = env::current_dir()?;
    let root = root
        .to_str()
        .ok_or_else(|| Error::other("working directory is not UTF-8"))?;
    let (logger, log_thread) = LogSink::start()?;
    let threads = Arc::new(Mutex::new(vec![log_thread]));
    let (notices, receiver) = sync_channel(8);
    let status = Arc::new(AtomicU8::new(1));
    let runtime = ActorRuntime::new(ActorRuntimeConfig {
        observer: ActorObserverHandle::new(Observer(notices.clone())),
        ..Default::default()
    });
    let actor = SessionActor::new(
        endpoint,
        root,
        limits.clone(),
        logger,
        threads.clone(),
        notices,
        status.clone(),
    )?;
    let handle = runtime
        .spawn_actor(
            actor,
            ActorSpawnOptions {
                mailbox: MailboxConfig::with_lanes(32, 8)
                    .with_deferred_capacity(8)
                    .with_turn_budget(8),
                ..Default::default()
            },
        )
        .map_err(Error::other)?;
    let mut deadline: Option<Instant> = None;
    loop {
        let notice = match deadline {
            Some(deadline) => receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .ok(),
            None => receiver.recv().ok(),
        };
        match notice {
            Some(ProcessNotice::Cleanup) => {
                deadline.get_or_insert_with(|| Instant::now() + limits.cleanup);
            }
            Some(ProcessNotice::Stopped) | None => break,
        }
    }
    drop(handle);
    runtime.shutdown();
    for thread in threads
        .lock()
        .map_err(|_| Error::other("thread owner poisoned"))?
        .drain(..)
    {
        // Blocking stdio is process-owned; an uncooperative peer cannot delay exit.
        if thread.is_finished() {
            let _ = thread.join();
        }
    }
    Ok(ExitCode::from(status.load(Ordering::Acquire)))
}
