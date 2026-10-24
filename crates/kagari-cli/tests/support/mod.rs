use std::{
    fs,
    io::{self, ErrorKind, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const TIMEOUT: Duration = Duration::from_secs(60);
const STREAM_BYTES: usize = 1024 * 1024;

pub(super) struct Project(pub(super) PathBuf);

impl Project {
    pub(super) fn new() -> Self {
        Self::new_in(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/toolchain-contracts"),
            &NEXT,
        )
    }

    fn new_in(parent: &Path, sequence: &AtomicUsize) -> Self {
        fs::create_dir_all(parent).unwrap();
        for _ in 0..128 {
            let root = parent.join(format!(
                "{}-{}",
                std::process::id(),
                sequence.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&root) {
                Ok(()) => return Self(root.canonicalize().unwrap()),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => panic!("claim project directory: {error}"),
            }
        }
        panic!("could not claim a fresh project directory");
    }

    pub(super) fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    pub(super) fn run(&self, args: &[&str]) -> Output {
        run(
            Command::new(env!("CARGO_BIN_EXE_kg"))
                .current_dir(&self.0)
                .args(args),
            TIMEOUT,
            STREAM_BYTES,
        )
        .unwrap_or_else(|error| panic!("kg {args:?}: {error}"))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            if thread::panicking() {
                eprintln!("project cleanup failed: {error}");
            } else {
                panic!("project cleanup failed: {error}");
            }
        }
    }
}

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        // Also runs on timeout, output overflow, reader failure and assertion unwinding.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn drain(
    reader: impl Read + Send + 'static,
    limit: usize,
    overflow: Arc<AtomicBool>,
) -> Receiver<io::Result<Vec<u8>>> {
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let result = (|| {
            let mut reader = reader;
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = reader.read(&mut buffer)?;
                if count == 0 {
                    return Ok(bytes);
                }
                let keep = count.min(limit - bytes.len());
                bytes.extend_from_slice(&buffer[..keep]);
                if keep != count {
                    overflow.store(true, Ordering::Relaxed);
                }
                // Keep draining after overflow until the supervisor terminates the child.
            }
        })();
        let _ = send.send(result);
    });
    receive
}

fn run(command: &mut Command, timeout: Duration, limit: usize) -> Result<Output, String> {
    let mut running = Running(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout = drain(running.0.stdout.take().unwrap(), limit, overflow.clone());
    let stderr = drain(running.0.stderr.take().unwrap(), limit, overflow.clone());
    let deadline = Instant::now() + timeout;
    let status = loop {
        if overflow.load(Ordering::Relaxed) {
            return Err("output exceeded capture limit; child killed and reaped".into());
        }
        if let Some(status) = running.0.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            return Err("process timed out; child killed and reaped".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let receive = |receiver: Receiver<io::Result<Vec<u8>>>| {
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| "pipe drain timed out".to_owned())?
            .map_err(|e| e.to_string())
    };
    let stdout = receive(stdout)?;
    let stderr = receive(stderr)?;
    if overflow.load(Ordering::Relaxed) {
        return Err("output exceeded capture limit; child killed and reaped".into());
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[test]
fn project_namespace_collision_and_unwind_preserve_existing_files() {
    let parent = Project::new();
    let first = Project::new_in(&parent.0, &AtomicUsize::new(0));
    first.write("retained", "before");
    let second = Project::new_in(&parent.0, &AtomicUsize::new(0));
    assert_ne!(first.0, second.0);
    assert_eq!(fs::read(first.0.join("retained")).unwrap(), b"before");
    let path = second.0.clone();
    assert!(
        std::panic::catch_unwind(move || {
            let _owned = second;
            panic!("exercise fixture cleanup");
        })
        .is_err()
    );
    assert!(!path.exists());
}

#[rstest::rstest]
#[case::timeout("timeout", Duration::from_millis(100), "timed out")]
#[case::overflow("overflow", Duration::from_secs(10), "capture limit")]
fn process_failures_are_bounded(
    #[case] mode: &str,
    #[case] timeout: Duration,
    #[case] message: &str,
) {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "support::process_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("KAGARI_TOOLCHAIN_PROCESS_FIXTURE", mode);
    let error = run(&mut child, timeout, 1024).unwrap_err();
    assert!(error.contains(message), "{error}");
    assert!(error.contains("killed and reaped"), "{error}");
}

#[test]
#[ignore = "child fixture launched only by the bounded process supervisor tests"]
fn process_fixture() {
    match std::env::var("KAGARI_TOOLCHAIN_PROCESS_FIXTURE")
        .unwrap()
        .as_str()
    {
        "timeout" => thread::sleep(Duration::from_secs(30)),
        "overflow" => {
            // Both streams exceed pipe capacity, detecting sequential/undrained capture.
            let writer = thread::spawn(|| io::stdout().write_all(&vec![b'x'; 128 * 1024]).unwrap());
            io::stderr().write_all(&vec![b'y'; 128 * 1024]).unwrap();
            writer.join().unwrap();
        }
        _ => panic!("unknown process fixture mode"),
    }
}
