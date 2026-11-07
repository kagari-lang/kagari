//! Bounded in-memory peer around the actual actor runtime and acquisition worker.
use crate::{
    logging::LogSink,
    server::{
        ProcessNotice,
        options::{Endpoint, SessionLimits},
    },
    session::SessionActor,
    workspace::WorkspaceCapture,
};
use lattice_actor::{
    mailbox::MailboxConfig,
    observation::{ActorLifecycleEvent, ActorMetadata, ActorObserver, ActorObserverHandle},
    runtime::{ActorRuntime, ActorRuntimeConfig, ActorSpawnOptions},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex},
    sync::watch,
    time::timeout,
};

const WAIT: Duration = Duration::from_secs(5);

pub(super) async fn send(peer: &mut DuplexStream, message: Value) {
    timeout(WAIT, async {
        let body = serde_json::to_vec(&message).unwrap();
        assert!(body.len() <= 64 * 1024);
        peer.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .await
            .unwrap();
        peer.write_all(&body).await.unwrap();
    })
    .await
    .expect("bounded peer write");
}

pub(super) async fn read(peer: &mut DuplexStream) -> Value {
    timeout(WAIT, async {
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            assert!(header.len() < 1024);
            header.push(peer.read_u8().await.unwrap());
        }
        let header = String::from_utf8(header).unwrap();
        let length: usize = header
            .strip_prefix("Content-Length: ")
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(length <= 64 * 1024);
        let mut body = vec![0; length];
        peer.read_exact(&mut body).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    })
    .await
    .expect("bounded peer read")
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

fn serve(
    endpoint: DuplexStream,
    root: String,
    limits: SessionLimits,
    ready: SyncSender<watch::Receiver<Option<Arc<WorkspaceCapture>>>>,
) -> Result<u8, String> {
    let (logger, log_thread) = LogSink::start().map_err(|error| error.to_string())?;
    let threads = Arc::new(Mutex::new(vec![log_thread]));
    let (notices, receiver) = sync_channel(8);
    let status = Arc::new(AtomicU8::new(1));
    let runtime = ActorRuntime::new(ActorRuntimeConfig {
        observer: ActorObserverHandle::new(Observer(notices.clone())),
        ..Default::default()
    });
    let actor = SessionActor::new(
        Endpoint::Memory(endpoint),
        &root,
        limits.clone(),
        logger,
        threads.clone(),
        notices,
        status.clone(),
    )
    .map_err(|error| error.to_string())?;
    ready
        .send(actor.captures.subscribe())
        .map_err(|error| error.to_string())?;
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
        .map_err(|error| error.to_string())?;
    let mut deadline = Instant::now() + Duration::from_secs(20);
    let stopped = loop {
        match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(ProcessNotice::Cleanup) => deadline = deadline.min(Instant::now() + limits.cleanup),
            Ok(ProcessNotice::Stopped) => break true,
            Err(_) => break false,
        }
    };
    drop(handle);
    runtime.shutdown();
    // This owned helper thread can wait for actual cleanup; the caller bounds its
    // completion channel, so a broken worker cannot hang the test harness.
    for thread in threads.lock().unwrap().drain(..) {
        thread
            .join()
            .map_err(|_| "owned server thread panicked".to_owned())?;
    }
    if !stopped {
        return Err("actor did not stop within its cleanup bound".into());
    }
    Ok(status.load(Ordering::Acquire))
}

pub(super) struct Peer {
    io: Option<DuplexStream>,
    captures: watch::Receiver<Option<Arc<WorkspaceCapture>>>,
    done: Receiver<Result<u8, String>>,
    task: Option<JoinHandle<()>>,
    responses: BTreeMap<i64, Value>,
    pending: BTreeSet<i64>,
    notifications: usize,
}

impl Peer {
    pub fn new(root: String, limits: SessionLimits) -> Self {
        let (io, endpoint) = duplex(64 * 1024);
        let (ready, capture) = sync_channel(1);
        let (finished, done) = sync_channel(1);
        let task = thread::spawn(move || {
            let _ = finished.send(serve(endpoint, root, limits, ready));
        });
        let captures = match capture.recv_timeout(WAIT) {
            Ok(captures) => captures,
            Err(error) => {
                drop(io);
                if done.recv_timeout(WAIT).is_ok() || task.is_finished() {
                    let _ = task.join();
                }
                panic!("actor subscription startup failed: {error}");
            }
        };
        Self {
            io: Some(io),
            captures,
            done,
            task: Some(task),
            responses: BTreeMap::new(),
            pending: BTreeSet::new(),
            notifications: 0,
        }
    }

    pub async fn notify(&mut self, method: &str, params: Value) {
        send(
            self.io.as_mut().unwrap(),
            json!({"jsonrpc":"2.0","method":method,"params":params}),
        )
        .await;
    }

    pub async fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        assert!(self.pending.insert(id));
        send(
            self.io.as_mut().unwrap(),
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )
        .await;
        loop {
            if let Some(response) = self.responses.remove(&id) {
                assert!(self.pending.remove(&id));
                return response;
            }
            let message = read(self.io.as_mut().unwrap()).await;
            assert_eq!(message["jsonrpc"], "2.0");
            if let Some(id) = message.get("id") {
                assert!(
                    message.get("method").is_none(),
                    "unexpected server request: {message}"
                );
                assert!(self.responses.len() < 16);
                assert!(
                    self.pending.contains(&id.as_i64().unwrap()),
                    "unsolicited or duplicate response: {message}"
                );
                assert!(
                    self.responses
                        .insert(id.as_i64().unwrap(), message)
                        .is_none(),
                    "duplicate reply"
                );
            } else {
                assert!(
                    message.get("method").is_some(),
                    "invalid notification: {message}"
                );
                self.notifications += 1;
                assert!(self.notifications <= 256);
            }
        }
    }

    pub async fn capture(&mut self, generation: u64) -> Arc<WorkspaceCapture> {
        timeout(WAIT, async {
            loop {
                let current = self.captures.borrow_and_update().clone();
                if let Some(capture) = current
                    && capture.generation >= generation
                {
                    assert_eq!(
                        capture.generation, generation,
                        "unexpected input generation"
                    );
                    return capture;
                }
                self.captures
                    .changed()
                    .await
                    .expect("capture owner stopped before publication");
            }
        })
        .await
        .expect("capture generation deadline")
    }

    pub async fn shutdown(mut self) {
        let response = self.request(99, "shutdown", Value::Null).await;
        assert_eq!(response.get("result"), Some(&Value::Null));
        self.notify("exit", Value::Null).await;
        self.finish(0).await;
    }

    pub async fn expect_failure(mut self) {
        self.finish(1).await;
    }

    async fn finish(&mut self, status: u8) {
        let eof = timeout(WAIT, self.io.as_mut().unwrap().read_u8())
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(eof.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(
            self.done
                .recv_timeout(WAIT)
                .expect("owned server cleanup")
                .unwrap(),
            status
        );
        self.task.take().unwrap().join().unwrap();
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.io.take();
        if let Some(task) = self.task.take() {
            let cleaned = self.done.recv_timeout(WAIT).is_ok();
            if cleaned {
                let _ = task.join();
            }
            if !thread::panicking() {
                assert!(cleaned, "failure cleanup exceeded its bound");
            }
        }
    }
}
