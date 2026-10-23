use std::{
    io::{self, BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use lsp_server::{Message, Notification, Request};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_HEADER_BYTES: usize = 8 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Receiver<io::Result<Option<Value>>>,
    stderr: Receiver<io::Result<Vec<u8>>>,
}

impl Server {
    fn spawn() -> Self {
        let (stdout_tx, stdout) = mpsc::channel();
        let (stderr_tx, stderr) = mpsc::channel();
        let mut server = Self {
            child: Command::new(env!("CARGO_BIN_EXE_kagari-analyzer"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn kagari-analyzer"),
            stdin: None,
            stdout,
            stderr,
        };
        server.stdin = server.child.stdin.take();
        let stdout = server.child.stdout.take().expect("child stdout");
        let stderr = server.child.stderr.take().expect("child stderr");
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let frame = read_frame(&mut reader);
                let finished = !matches!(&frame, Ok(Some(_)));
                if stdout_tx.send(frame).is_err() || finished {
                    break;
                }
            }
        });
        thread::spawn(move || {
            let _ = stderr_tx.send(drain_stderr(stderr));
        });
        server
    }

    fn send(&mut self, message: Message) {
        let mut bytes = Vec::new();
        message.write(&mut bytes).expect("encode LSP message");
        // Keep writes below pipe capacity; responses/barriers bound queued input.
        assert!(bytes.len() < 4096, "test input must remain small");
        self.write_raw(&bytes);
    }

    fn write_raw(&mut self, bytes: &[u8]) {
        let stdin = self.stdin.as_mut().expect("stdin is still open");
        stdin.write_all(bytes).expect("write child stdin");
        stdin.flush().expect("flush child stdin");
    }

    fn request(&mut self, id: i32, method: &str, params: Value) {
        self.send(Message::Request(Request::new(
            id.into(),
            method.to_owned(),
            params,
        )));
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(Message::Notification(Notification::new(
            method.to_owned(),
            params,
        )));
    }

    fn output(&self) -> Option<Value> {
        self.stdout
            .recv_timeout(TIMEOUT)
            .expect("timed out or reader stopped while awaiting stdout")
            .expect("stdout must contain only complete Content-Length-framed JSON")
    }

    fn result(&self, id: i32) -> Value {
        let response = self.output().expect("response before stdout EOF");
        let result = response
            .get("result")
            .expect("successful response has a result")
            .clone();
        assert_eq!(
            response,
            json!({"jsonrpc": "2.0", "id": id, "result": result}),
            "expected exactly this response, not a notification or another request's reply"
        );
        result
    }

    fn error(&self, id: i32, code: i32) {
        let response = self.output().expect("error response before stdout EOF");
        let message = response["error"]["message"]
            .as_str()
            .filter(|message| !message.is_empty())
            .expect("error response has a nonempty message");
        assert_eq!(
            response,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": code, "message": message}
            })
        );
    }

    fn initialize(&mut self, id: i32) {
        self.request(id, "initialize", initialize_params());
        let result = self.result(id);
        assert_eq!(
            result["capabilities"],
            json!({
                "positionEncoding": "utf-16",
                "textDocumentSync": {"openClose": true, "change": 1}
            }),
            "advertise only UTF-16 positions and implemented full-document synchronization"
        );
        self.notify("initialized", json!({}));
    }

    fn shutdown(&mut self, id: i32) {
        self.request(id, "shutdown", Value::Null);
        assert_eq!(self.result(id), Value::Null);
    }

    fn finish(&mut self, expected_code: Option<i32>) {
        let status = self.wait();
        let stderr = self
            .stderr
            .recv_timeout(TIMEOUT)
            .expect("timed out or reader stopped while draining stderr")
            .expect("read child stderr");
        assert_eq!(
            self.output(),
            None,
            "unexpected trailing protocol output; stderr: {}",
            String::from_utf8_lossy(&stderr)
        );
        match expected_code {
            Some(code) => assert_eq!(
                status.code(),
                Some(code),
                "unexpected exit status; stderr: {}",
                String::from_utf8_lossy(&stderr)
            ),
            None => assert!(
                !status.success(),
                "unexpected successful exit; stderr: {}",
                String::from_utf8_lossy(&stderr)
            ),
        }
    }

    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("poll child status") {
                return status;
            }
            assert!(Instant::now() < deadline, "analyzer did not exit in time");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Also runs during assertion failures and timeouts; never leave a child behind.
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn initialize_params() -> Value {
    json!({
        "processId": null,
        "rootUri": null,
        "capabilities": {
            "general": {"positionEncodings": ["utf-8", "utf-16", "utf-32"]}
        }
    })
}

fn invalid_output(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

// Parse framing strictly so logs, partial headers/bodies and trailing bytes cannot
// disappear behind a permissive line reader or a JSON-only response assertion.
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
                Err(invalid_output("stdout ended inside an LSP header"))
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
            .ok_or_else(|| invalid_output("non-header text on stdout"))?;
        if name.eq_ignore_ascii_case("Content-Length") {
            let value = value.trim();
            if content_length.is_some() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid_output("invalid or duplicate Content-Length"));
            }
            content_length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| invalid_output("invalid Content-Length"))?,
            );
        } else if !name.eq_ignore_ascii_case("Content-Type") {
            return Err(invalid_output("unexpected header or log on stdout"));
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

fn drain_stderr(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut retained = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(retained);
        }
        // Keep draining even if a failing server logs excessively.
        let keep = count.min(MAX_STDERR_BYTES - retained.len());
        retained.extend_from_slice(&buffer[..keep]);
    }
}

#[test]
fn stdio_initializes_synchronizes_and_exits_with_stdin_open() {
    let mut server = Server::spawn();
    server.initialize(1);
    let uri = "untitled:Unicode-stdio.kgr";
    server.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri,
            "languageId": "kagari",
            "version": 1,
            "text": "val greeting = \"你好😀\"; val scalar: char = '🙂'; val escaped = '\\u{1F642_}';\r\n"
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
            "textDocument": {"uri": uri, "version": 3},
            "contentChanges": [{"text": ""}]
        }),
    );
    server.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": uri}}),
    );
    server.notify("kagari/unsupportedNotification", json!({}));

    // This ordered reply must be the next frame: no fake diagnostics or replies
    // to any preceding notification may be filtered out by the harness.
    server.request(2, "kagari/unsupportedRequest", json!({}));
    server.error(2, -32601);
    server.shutdown(3);
    server.notify("exit", Value::Null);
    assert!(server.stdin.is_some());
    // In particular, do not close stdin here: exit must not wait for transport EOF.
    server.finish(Some(0));
}

#[test]
fn stdio_rejects_invalid_requests_without_corrupting_lifecycle() {
    let mut server = Server::spawn();
    server.request(1, "shutdown", Value::Null);
    server.error(1, -32002);
    server.request(2, "initialize", json!({"capabilities": 42}));
    server.error(2, -32602);
    server.initialize(3);
    server.request(4, "initialize", initialize_params());
    server.error(4, -32600);
    server.request(5, "shutdown", json!({}));
    server.error(5, -32602);
    server.request(6, "textDocument/hover", Value::Null);
    server.error(6, -32601);
    server.shutdown(7);
    server.request(8, "shutdown", Value::Null);
    server.error(8, -32600);
    server.request(9, "kagari/unsupportedRequest", json!({}));
    server.error(9, -32600);
    server.notify("exit", Value::Null);
    server.finish(Some(0));
}

#[test]
fn stdio_exit_without_shutdown_is_failure() {
    let mut server = Server::spawn();
    server.initialize(1);
    server.notify("exit", Value::Null);
    // The non-graceful exit also has to terminate with the input pipe still open.
    server.finish(Some(1));
}

#[test]
fn stdio_unexpected_eof_and_malformed_framing_are_failures() {
    let mut server = Server::spawn();
    server.initialize(1);
    drop(server.stdin.take());
    server.finish(None);

    let mut server = Server::spawn();
    server.write_raw(b"Content-Length: not-a-number\r\n\r\n");
    server.finish(None);
}
