use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use rstest::rstest;
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_HEADER_BYTES: usize = 8 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;
const MAX_PENDING: usize = 64;
const MAX_HISTORY: usize = 32;

#[derive(Clone, Copy, Debug)]
enum Transport {
    Stdio,
    TcpConnect,
    TcpListen,
}

struct PendingChild(Option<Child>);

impl Drop for PendingChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct Server {
    child: Child,
    input: Option<Box<dyn Write + Send>>,
    socket: Option<TcpStream>,
    output: Receiver<io::Result<Option<Value>>>,
    read_grant: Option<SyncSender<()>>,
    stderr: Receiver<io::Result<Vec<u8>>>,
    auxiliary_stdout: Option<Receiver<io::Result<Vec<u8>>>>,
    threads: Vec<JoinHandle<()>>,
    outstanding: BTreeSet<String>,
    responses: BTreeMap<String, Value>,
    notifications: VecDeque<Value>,
    history: VecDeque<Value>,
}

impl Server {
    fn spawn(transport: Transport) -> Self {
        Self::spawn_configured(transport, None, None)
    }

    fn spawn_configured(
        transport: Transport,
        cache: Option<&Path>,
        sources: Option<&Path>,
    ) -> Self {
        let listener = match transport {
            Transport::Stdio => None,
            Transport::TcpConnect | Transport::TcpListen => {
                Some(TcpListener::bind("127.0.0.1:0").expect("reserve loopback address"))
            }
        };
        let address = listener
            .as_ref()
            .map(|listener| listener.local_addr().unwrap());
        let mut command = Command::new(env!("CARGO_BIN_EXE_kagari-analyzer"));
        let default_cache =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/lsp-process-native-sources");
        let cache = cache.unwrap_or(&default_cache);
        command
            .env("LOCALAPPDATA", cache)
            .env("XDG_CACHE_HOME", cache);
        if let Some(sources) = sources {
            command.env("KAGARI_STDLIB_SOURCES", sources);
        } else {
            command.env_remove("KAGARI_STDLIB_SOURCES");
        }
        match transport {
            Transport::Stdio => {
                command.arg("--stdio");
            }
            Transport::TcpConnect => {
                command.args(["--tcp-connect", &address.unwrap().to_string()]);
            }
            Transport::TcpListen => {
                command.args(["--tcp-listen", &address.unwrap().to_string()]);
            }
        }
        // The listener adapter needs to bind the selected address itself.
        let listener = if matches!(transport, Transport::TcpListen) {
            drop(listener);
            None
        } else {
            listener
        };
        let mut pending = PendingChild(Some(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn kagari-analyzer"),
        ));
        let child = pending.0.as_mut().unwrap();
        let stdout = child.stdout.take().expect("child stdout");
        let stderr = child.stderr.take().expect("child stderr");
        let mut threads = Vec::new();
        let (stderr_tx, stderr_rx) = mpsc::sync_channel(1);
        threads.push(thread::spawn(move || {
            let _ = stderr_tx.send(drain_output(stderr));
        }));
        let mut auxiliary_stdout = None;
        let mut socket = None;
        let (input, reader): (Box<dyn Write + Send>, Box<dyn Read + Send>) = match transport {
            Transport::Stdio => (Box::new(child.stdin.take().unwrap()), Box::new(stdout)),
            Transport::TcpConnect | Transport::TcpListen => {
                drop(child.stdin.take());
                let stream = match listener {
                    Some(listener) => accept(&listener, child),
                    None => connect(address.unwrap(), child),
                };
                // Windows can inherit the listener's nonblocking mode on accept.
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(TIMEOUT)).unwrap();
                stream.set_write_timeout(Some(TIMEOUT)).unwrap();
                stream.set_nodelay(true).unwrap();
                socket = Some(stream.try_clone().unwrap());
                let (tx, rx) = mpsc::sync_channel(1);
                auxiliary_stdout = Some(rx);
                threads.push(thread::spawn(move || {
                    let _ = tx.send(drain_output(stdout));
                }));
                (Box::new(stream.try_clone().unwrap()), Box::new(stream))
            }
        };
        let (output_tx, output) = mpsc::sync_channel(32);
        let (read_grant, permits) = mpsc::sync_channel(1);
        threads.push(thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            while permits.recv().is_ok() {
                let frame = read_frame(&mut reader);
                let finished = !matches!(&frame, Ok(Some(_)));
                if output_tx.send(frame).is_err() || finished {
                    break;
                }
            }
        }));
        Self {
            child: pending.0.take().unwrap(),
            input: Some(input),
            socket,
            output,
            read_grant: Some(read_grant),
            stderr: stderr_rx,
            auxiliary_stdout,
            threads,
            outstanding: BTreeSet::new(),
            responses: BTreeMap::new(),
            notifications: VecDeque::new(),
            history: VecDeque::new(),
        }
    }

    fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let bytes = [header.as_bytes(), &body].concat();
        assert!(
            bytes.len() < 4096,
            "test input must remain below pipe capacity"
        );
        self.write_raw(&bytes);
    }

    fn write_raw(&mut self, bytes: &[u8]) {
        let input = self.input.as_mut().expect("input is still open");
        input.write_all(bytes).expect("write analyzer input");
        input.flush().expect("flush analyzer input");
    }

    fn request(&mut self, id: Value, method: &str, params: Value) {
        assert!(self.outstanding.len() < MAX_PENDING);
        assert!(
            self.outstanding.insert(id.to_string()),
            "duplicate client request ID"
        );
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn receive(&mut self, deadline: Instant) -> Option<Value> {
        self.read_grant
            .as_ref()
            .unwrap()
            .send(())
            .expect("peer reader is live");
        let timeout = deadline.saturating_duration_since(Instant::now());
        let frame = self
            .output
            .recv_timeout(timeout)
            .unwrap_or_else(|error| {
                panic!(
                    "waiting for analyzer output: {error}; recent frames: {:?}",
                    self.history
                )
            })
            .expect("complete Content-Length-framed JSON output");
        if let Some(frame) = &frame {
            assert_eq!(frame["jsonrpc"], "2.0", "{frame}");
            if self.history.len() == MAX_HISTORY {
                self.history.pop_front();
            }
            self.history.push_back(frame.clone());
        }
        frame
    }

    fn dispatch(&mut self, frame: Value) {
        if let Some(method) = frame.get("method") {
            assert!(method.is_string(), "method is a string: {frame}");
            assert!(frame.get("result").is_none() && frame.get("error").is_none());
            if let Some(id) = frame.get("id") {
                let result = match method.as_str().unwrap() {
                    "window/workDoneProgress/create" => Value::Null,
                    "workspace/configuration" => {
                        let items = frame["params"]["items"].as_array().unwrap();
                        Value::Array(items.iter().map(|_| Value::Null).collect())
                    }
                    "workspace/workspaceFolders" => Value::Null,
                    unexpected => panic!("unconfigured server request {unexpected}: {frame}"),
                };
                self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
            } else {
                assert!(
                    self.notifications.len() < 128,
                    "notification budget exceeded"
                );
                self.notifications.push_back(frame);
            }
            return;
        }
        let id = frame.get("id").expect("response has an ID").to_string();
        assert!(
            self.outstanding.contains(&id),
            "unsolicited or duplicate reply: {frame}"
        );
        assert_ne!(frame.get("result").is_some(), frame.get("error").is_some());
        assert!(
            self.responses.insert(id, frame).is_none(),
            "duplicate pending reply"
        );
    }

    fn response(&mut self, id: Value) -> Value {
        let key = id.to_string();
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(response) = self.responses.remove(&key) {
                assert!(self.outstanding.remove(&key));
                return response;
            }
            let frame = self.receive(deadline).expect("response before output EOF");
            self.dispatch(frame);
        }
    }

    fn result(&mut self, id: Value) -> Value {
        let response = self.response(id.clone());
        let result = response
            .get("result")
            .expect("successful response has a result")
            .clone();
        assert_eq!(
            response,
            json!({"jsonrpc": "2.0", "id": id, "result": result})
        );
        result
    }

    fn error(&mut self, id: Value, code: i32) {
        let response = self.response(id.clone());
        assert_eq!(response["id"], id);
        assert_eq!(response["error"]["code"], code, "{response}");
        assert!(
            response["error"]["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty())
        );
        assert!(response.get("result").is_none());
    }

    fn initialize(&mut self, id: Value) {
        self.request(id.clone(), "initialize", initialize_params());
        let result = self.result(id);
        assert_eq!(result["capabilities"]["positionEncoding"], "utf-16");
        assert_eq!(
            result["capabilities"]["textDocumentSync"]["openClose"],
            true
        );
        assert_eq!(result["capabilities"]["textDocumentSync"]["change"], 1);
        assert_eq!(result["serverInfo"]["name"], "kagari-analyzer");
        assert!(
            result["serverInfo"]["version"]
                .as_str()
                .is_some_and(|version| !version.is_empty())
        );
        self.notify("initialized", json!({}));
    }

    fn shutdown(&mut self, id: Value) {
        self.request(id.clone(), "shutdown", Value::Null);
        assert_eq!(self.result(id), Value::Null);
    }

    fn finish(&mut self, expected_code: Option<i32>) {
        let deadline = Instant::now() + TIMEOUT;
        while let Some(frame) = self.receive(deadline) {
            self.dispatch(frame);
        }
        assert!(
            self.outstanding.is_empty(),
            "requests without terminal replies"
        );
        assert!(self.responses.is_empty(), "unconsumed replies");
        let status = wait(&mut self.child, deadline);
        let stderr = self.stderr.recv_timeout(TIMEOUT).unwrap().unwrap();
        if let Some(stdout) = &self.auxiliary_stdout {
            assert!(
                stdout.recv_timeout(TIMEOUT).unwrap().unwrap().is_empty(),
                "TCP mode wrote stdout"
            );
        }
        let stderr = String::from_utf8_lossy(&stderr);
        match expected_code {
            Some(code) => assert_eq!(status.code(), Some(code), "stderr: {stderr}"),
            None => assert!(
                !status.success(),
                "unexpected successful exit; stderr: {stderr}"
            ),
        }
        drop(self.input.take());
        for worker in self.threads.drain(..) {
            worker.join().expect("peer reader must not panic");
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.read_grant.take());
        drop(self.input.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        // A failed assertion can leave the bounded reader parked on send. Dropping
        // the receiver during unwind releases it; never block Drop on that sender.
    }
}

fn accept(listener: &TcpListener, child: &mut Child) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("accept analyzer connection: {error}"),
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "analyzer exited before connecting"
        );
        assert!(Instant::now() < deadline, "analyzer did not connect");
        thread::sleep(Duration::from_millis(5));
    }
}

fn connect(address: SocketAddr, child: &mut Child) -> TcpStream {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match TcpStream::connect_timeout(&address, Duration::from_millis(50)) {
            Ok(stream) => return stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => panic!("connect to analyzer: {error}"),
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "analyzer exited before listening"
        );
        assert!(Instant::now() < deadline, "analyzer did not listen");
        thread::sleep(Duration::from_millis(5));
    }
}

fn wait(child: &mut Child, deadline: Instant) -> ExitStatus {
    loop {
        if let Some(status) = child.try_wait().expect("poll child status") {
            return status;
        }
        assert!(Instant::now() < deadline, "analyzer did not exit in time");
        thread::sleep(Duration::from_millis(5));
    }
}

fn initialize_params() -> Value {
    json!({
        "processId": null,
        "rootUri": null,
        "capabilities": {"general": {"positionEncodings": ["utf-8", "utf-16", "utf-32"]}}
    })
}

fn invalid_output(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

// Independent literal framing oracle: this intentionally does not call the
// production codec, so shared encoder/decoder mistakes cannot validate themselves.
fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut header_bytes = 0;
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let count = (&mut *reader)
            .take(MAX_HEADER_BYTES as u64 + 1)
            .read_line(&mut line)?;
        if count == 0 {
            return if header_bytes == 0 {
                Ok(None)
            } else {
                Err(invalid_output("EOF inside header"))
            };
        }
        header_bytes += count;
        if header_bytes > MAX_HEADER_BYTES || !line.ends_with("\r\n") {
            return Err(invalid_output("invalid or oversized LSP header"));
        }
        if line == "\r\n" {
            break;
        }
        let (name, value) = line[..line.len() - 2]
            .split_once(':')
            .ok_or_else(|| invalid_output("non-header text on protocol stream"))?;
        if name.eq_ignore_ascii_case("Content-Length") {
            let value = value.trim();
            if content_length.is_some()
                || value.is_empty()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(invalid_output("invalid or duplicate Content-Length"));
            }
            content_length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| invalid_output("invalid Content-Length"))?,
            );
        } else if !name.eq_ignore_ascii_case("Content-Type") {
            return Err(invalid_output(
                "unexpected header or log on protocol stream",
            ));
        }
    }
    let length = content_length.ok_or_else(|| invalid_output("missing Content-Length"))?;
    if length > MAX_FRAME_BYTES {
        return Err(invalid_output("oversized LSP body"));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn drain_output(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut retained = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(retained);
        }
        let keep = count.min(MAX_STDERR_BYTES - retained.len());
        retained.extend_from_slice(&buffer[..keep]);
    }
}

#[rstest]
#[case::stdio(Transport::Stdio)]
#[case::tcp_connect(Transport::TcpConnect)]
#[case::tcp_listen(Transport::TcpListen)]
fn transports_share_ordered_lifecycle_and_exit_without_input_eof(#[case] transport: Transport) {
    let mut server = Server::spawn(transport);
    server.initialize(json!(1));
    let uri = "untitled:Unicode-transport.kgr";
    server.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "kagari", "version": 1,
            "text": "val greeting = \"你好😀\"; val scalar: char = '🙂';\r\n"
        }}),
    );
    server.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": uri, "version": 2},
            "contentChanges": [{"text": "fn broken( 😀\r\n"}]
        }),
    );
    server.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": uri, "version": 3}, "contentChanges": [{"text": ""}]
        }),
    );
    server.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": uri}}),
    );
    server.notify("kagari/unsupportedNotification", json!({}));
    // Ask for responses in reverse order: notification and response arrival order
    // is independent of which client operation currently waits for its result.
    server.request(json!(2), "kagari/unsupportedRequest", json!({}));
    server.request(json!("2"), "kagari/otherRequest", json!({}));
    server.error(json!("2"), -32601);
    server.error(json!(2), -32601);
    server.shutdown(json!(3));
    server.notify("exit", Value::Null);
    assert!(server.input.is_some());
    server.finish(Some(0));
}

#[test]
fn invalid_requests_do_not_corrupt_lifecycle_or_request_identity() {
    let mut server = Server::spawn(Transport::Stdio);
    server.request(json!(1), "shutdown", Value::Null);
    server.error(json!(1), -32002);
    server.request(json!(2), "initialize", json!({"capabilities": 42}));
    server.error(json!(2), -32602);
    server.initialize(json!(3));
    server.request(json!(4), "initialize", initialize_params());
    server.error(json!(4), -32600);
    server.request(json!(5), "shutdown", json!({}));
    server.error(json!(5), -32602);
    server.request(json!(6), "kagari/unimplemented", Value::Null);
    server.error(json!(6), -32601);
    // Reuse is legal after the previous terminal reply, including after errors.
    server.shutdown(json!(6));
    server.request(json!(7), "shutdown", Value::Null);
    server.error(json!(7), -32600);
    server.notify("exit", Value::Null);
    server.finish(Some(0));
}

#[test]
fn raw_invalid_envelopes_receive_null_id_errors_then_recover() {
    let mut server = Server::spawn(Transport::Stdio);
    assert!(server.outstanding.insert("null".into()));
    server.write_raw(b"Content-Length: 1\r\n\r\n{");
    server.error(Value::Null, -32700);
    assert!(server.outstanding.insert("null".into()));
    server.send(json!({"jsonrpc": "2.0", "id": false, "method": "initialize"}));
    server.error(Value::Null, -32600);
    server.initialize(json!("ready"));
    server.shutdown(json!("done"));
    server.notify("exit", Value::Null);
    server.finish(Some(0));
}

#[test]
fn unordered_exit_and_tcp_disconnect_fail() {
    let mut server = Server::spawn(Transport::Stdio);
    server.initialize(json!(1));
    server.notify("exit", Value::Null);
    server.finish(Some(1));

    let mut server = Server::spawn(Transport::TcpConnect);
    server.initialize(json!(1));
    server
        .socket
        .as_ref()
        .unwrap()
        .shutdown(Shutdown::Both)
        .unwrap();
    drop(server.input.take());
    server.finish(None);
}

#[test]
fn truncated_input_and_malformed_headers_are_fatal() {
    let mut server = Server::spawn(Transport::Stdio);
    server.initialize(json!(1));
    drop(server.input.take());
    server.finish(None);

    let mut server = Server::spawn(Transport::Stdio);
    server.write_raw(b"Content-Length: not-a-number\r\n\r\n");
    server.finish(None);
}

#[test]
fn stalled_stdout_expires_write_deadline_with_stdin_still_open() {
    let mut server = Server::spawn(Transport::Stdio);
    server.initialize(json!("ready"));
    // The peer grants reads only through receive(). Keep stdout open but unread
    // after initialize; 64 escaped IDs generate > 96 KiB, exceeding supported
    // anonymous-pipe capacities without overflowing the server's 128-slot outbox.
    let mut bytes = Vec::new();
    for index in 0..64 {
        let id = format!("{}-{index}", "\0".repeat(250));
        let body = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": id, "method": "kagari/unsupported", "params": {}
        }))
        .unwrap();
        write!(bytes, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        bytes.extend_from_slice(&body);
    }
    let mut input = server.input.take().unwrap();
    let (sent, completed) = mpsc::sync_channel(1);
    let sender = thread::spawn(move || {
        let result = input.write_all(&bytes).and_then(|()| input.flush());
        let _ = sent.send((result, input));
    });
    let (result, input) = completed
        .recv_timeout(TIMEOUT)
        .expect("server must consume bounded input");
    server.input = Some(input);
    result.expect("write all requests before server expiry");
    sender.join().unwrap();
    let status = wait(&mut server.child, Instant::now() + TIMEOUT);
    assert_eq!(status.code(), Some(1));
    let stderr = server.stderr.recv_timeout(TIMEOUT).unwrap().unwrap();
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(
        stderr.contains("Write deadline expired"),
        "wrong failure owner: {stderr}"
    );
    assert!(
        server.input.is_some(),
        "input EOF must not rescue a blocked writer"
    );
    drop(server.read_grant.take());
    for worker in server.threads.drain(..) {
        worker.join().expect("peer cleanup after stalled output");
    }
}

#[rstest]
#[case::embedded_cache_unavailable(false)]
#[case::installed_sources_need_no_cache(true)]
fn native_sources_preserve_semantics_with_blocked_cache(#[case] installed: bool) {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/lsp-blocked-cache")
        .join(format!("{}-{installed}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let blocked = directory.join("cache-file");
    fs::write(&blocked, "cache is unavailable").unwrap();
    let source_root = directory.join("stdlib");
    if installed {
        for source in kagari_stdlib::sources::DECLARATIONS {
            let path = source_root.join(source.path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source.text).unwrap();
        }
    }
    let mut server = Server::spawn_configured(
        Transport::Stdio,
        Some(&blocked),
        installed.then_some(source_root.as_path()),
    );
    server.initialize(json!(1));
    let uri = "untitled:stdlib-navigation";
    let text = include_str!("fixtures/analysis/stdlib-navigation.kgr");
    server.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"kagari","version":1,"text":text}}),
    );
    let site = |line, character| json!({"textDocument":{"uri":uri},"position":{"line":line,"character":character}});
    server.request(json!(2), "textDocument/definition", site(1, 17));
    if installed {
        let native = server.result(json!(2));
        assert!(
            native["uri"].as_str().unwrap().ends_with("/alloc/vec.kgr"),
            "{native}"
        );
    } else {
        server.error(json!(2), -32803);
    }
    server.request(json!(3), "textDocument/hover", site(2, 5));
    let hover = server.result(json!(3));
    assert!(hover.to_string().contains("Vec"), "{hover}");
    server.request(json!(4), "textDocument/definition", site(2, 5));
    let local = server.result(json!(4));
    assert_eq!(local["uri"], uri);
    assert_eq!(local["range"]["start"], json!({"line":1,"character":8}));
    server.request(json!(5), "textDocument/completion", site(2, 11));
    let completion = server.result(json!(5));
    assert!(
        completion["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "len"),
        "{completion}"
    );
    server.shutdown(json!(6));
    server.notify("exit", Value::Null);
    server.finish(Some(0));
    assert_eq!(fs::read_to_string(blocked).unwrap(), "cache is unavailable");
}

#[rstest]
#[case::version(&["--version"], 0, "kagari-analyzer ")]
#[case::help(&["--help"], 0, "Usage:")]
#[case::stdio_conflict(&["--stdio", "--tcp-connect", "127.0.0.1:9257"], 2, "cannot be used with")]
#[case::tcp_conflict(&["--tcp-listen", "127.0.0.1:9257", "--tcp-connect", "127.0.0.1:9257"], 2, "cannot be used with")]
#[case::non_loopback(&["--tcp-listen", "0.0.0.0:9257"], 2, "numeric loopback address")]
#[case::hostname(&["--tcp-connect", "localhost:9257"], 2, "invalid socket address")]
fn command_line_operations_exit_before_starting_a_protocol_session(
    #[case] args: &[&str],
    #[case] exit_code: i32,
    #[case] expected: &str,
) {
    let mut pending = PendingChild(Some(
        Command::new(env!("CARGO_BIN_EXE_kagari-analyzer"))
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    assert_eq!(
        wait(pending.0.as_mut().unwrap(), Instant::now() + TIMEOUT).code(),
        Some(exit_code)
    );
    let output = pending.0.take().unwrap().wait_with_output().unwrap();
    let text = if exit_code == 0 {
        assert!(output.stderr.is_empty());
        String::from_utf8(output.stdout).unwrap()
    } else {
        assert!(
            output.stdout.is_empty(),
            "argument errors must not pollute protocol output"
        );
        String::from_utf8(output.stderr).unwrap()
    };
    assert!(text.contains(expected) && text.ends_with('\n'), "{text}");
    if args == ["--version"] {
        assert!(text.starts_with(expected) && text.len() < 256, "{text}");
    }
}
