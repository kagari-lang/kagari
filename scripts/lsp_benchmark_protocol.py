"""Bounded raw-wire process peer, independent of the analyzer's codec."""
from __future__ import annotations

from collections import deque
import json
import os
from pathlib import Path
import queue
import socket
import subprocess
import threading
import time

MAX_HEADER = 8192
MAX_BODY = 8 * 1024 * 1024
MAX_NOTIFICATION_HISTORY_BYTES = 16 * 1024 * 1024
TIMEOUT = 10.0


def read_frame(stream):
    total = 0
    length = None
    while True:
        line = stream.readline(MAX_HEADER + 1)
        if not line and total == 0:
            return None
        total += len(line)
        if total > MAX_HEADER or not line.endswith(b"\r\n"):
            raise ValueError("truncated, non-CRLF or oversized protocol header")
        if line == b"\r\n":
            break
        name, separator, value = line[:-2].partition(b":")
        if not separator:
            raise ValueError("non-header bytes on protocol output")
        if name.lower() == b"content-length":
            value = value.strip()
            if length is not None or not value or not value.isdigit():
                raise ValueError("invalid or duplicate Content-Length")
            length = int(value)
        elif name.lower() != b"content-type":
            raise ValueError("unexpected output header")
    if length is None or length > MAX_BODY:
        raise ValueError("missing or oversized Content-Length")
    parts = []
    remaining = length
    while remaining:
        part = stream.read(remaining)
        if not part:
            raise ValueError("truncated protocol body")
        parts.append(part)
        remaining -= len(part)
    value = json.loads(b"".join(parts).decode("utf-8"))
    if not isinstance(value, dict) or value.get("jsonrpc") != "2.0":
        raise ValueError("invalid JSON-RPC output envelope")
    return value


class Peer:
    def __init__(self, executable: Path, transport: str, root: Path, cache_root: Path | None = None, history_limit=512):
        self.root = root
        self.frames = queue.Queue(64)
        self.stop = threading.Event()
        self.history = deque(maxlen=32)
        self.notifications = deque(maxlen=128)
        self.notification_history = []
        self.history_limit = history_limit
        self.notification_bytes = 0
        self.responses = {}
        self.pending = set()
        self.stderr = bytearray()
        self.tcp_stdout = bytearray()
        self.threads = []
        self.socket = None
        self.stdin = None
        self.reader = None
        self.process = None
        self.read_delay_seconds = 0
        environment = os.environ.copy()
        if cache_root is not None:
            cache_root.mkdir(parents=True, exist_ok=False)
            environment.update(LOCALAPPDATA=str(cache_root), XDG_CACHE_HOME=str(cache_root))
        listener = None
        args = [str(executable), "--stdio"]
        if transport == "tcp":
            listener = socket.socket()
            listener.bind(("127.0.0.1", 0))
            listener.listen(1)
            listener.settimeout(TIMEOUT)
            args = [str(executable), "--tcp-connect", f"127.0.0.1:{listener.getsockname()[1]}"]
        elif transport != "stdio":
            raise ValueError(f"unsupported transport: {transport}")
        self.spawn_ns = time.perf_counter_ns()
        try:
            self.process = subprocess.Popen(args, cwd=root, stdin=subprocess.PIPE,
                                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0, env=environment)
            self._thread(self._drain, self.process.stderr, self.stderr)
            if listener:
                self.socket, _ = listener.accept()
                self.socket.settimeout(TIMEOUT)
                self.socket.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                self.stdin = self.socket.makefile("wb", buffering=0)
                self.reader = self.socket.makefile("rb")
                self.process.stdin.close()
                self._thread(self._drain, self.process.stdout, self.tcp_stdout)
            else:
                self.stdin = self.process.stdin
                self.reader = self.process.stdout
            self._thread(self._read)
        except BaseException:
            self.close()
            raise
        finally:
            if listener:
                listener.close()

    def _thread(self, target, *args):
        worker = threading.Thread(target=target, args=args, daemon=True)
        self.threads.append(worker)
        worker.start()

    def _put(self, value):
        while not self.stop.is_set():
            try:
                self.frames.put(value, timeout=0.1)
                return
            except queue.Full:
                pass

    def _read(self):
        try:
            while not self.stop.is_set():
                if self.stop.wait(self.read_delay_seconds):
                    return
                frame = read_frame(self.reader)
                self._put((frame, time.perf_counter_ns()))
                if frame is None:
                    return
        except Exception as error:
            self._put(error)

    @staticmethod
    def _drain(stream, retained):
        try:
            while data := stream.read(4096):
                retained.extend(data[:max(0, 65536 - len(retained))])
        finally:
            stream.close()

    def send(self, value):
        body = json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode()
        if len(body) > MAX_BODY:
            raise ValueError("peer input body budget exceeded")
        self.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
        self.stdin.flush()

    def notify(self, method, params):
        self.send({"jsonrpc": "2.0", "method": method, "params": params})

    def _receive(self, deadline):
        try:
            frame = self.frames.get(timeout=max(0, deadline - time.monotonic()))
        except queue.Empty as error:
            raise ValueError(f"protocol timeout; recent frames={list(self.history)!r}") from error
        if isinstance(frame, Exception):
            raise ValueError(f"protocol reader failed: {frame}") from frame
        value, received_ns = frame
        if value is not None:
            self.history.append(value)
        return value, received_ns

    def _dispatch(self, value, received_ns):
        if "method" in value:
            if "result" in value or "error" in value:
                raise ValueError("ambiguous server method envelope")
            if "id" in value:
                if value["method"] == "window/workDoneProgress/create":
                    result = None
                elif value["method"] == "workspace/configuration":
                    result = [None] * len(value["params"]["items"])
                elif value["method"] == "workspace/workspaceFolders":
                    result = [{"uri": self.root.as_uri(), "name": "benchmark"}]
                else:
                    raise ValueError(f"unexpected server request: {value!r}")
                self.send({"jsonrpc": "2.0", "id": value["id"], "result": result})
            else:
                if len(self.notifications) == self.notifications.maxlen:
                    raise ValueError("unconsumed notification budget exceeded")
                if len(self.notification_history) >= self.history_limit:
                    raise ValueError("notification history budget exceeded")
                retained = len(json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode())
                if retained > MAX_NOTIFICATION_HISTORY_BYTES - self.notification_bytes:
                    raise ValueError("notification history byte budget exceeded")
                self.notification_bytes += retained
                self.notifications.append((value, received_ns))
                self.notification_history.append((value, received_ns))
            return
        key = json.dumps(value.get("id"), separators=(",", ":"))
        if key not in self.pending or key in self.responses:
            raise ValueError(f"unsolicited or duplicated response: {value!r}")
        if ("result" in value) == ("error" in value):
            raise ValueError("response must have exactly one of result/error")
        self.responses[key] = (value, received_ns)

    def begin_request(self, identifier, method, params):
        key = json.dumps(identifier, separators=(",", ":"))
        if key in self.pending or len(self.pending) >= 64:
            raise ValueError("duplicate ID or request credit exhausted")
        self.pending.add(key)
        started = time.perf_counter_ns()
        self.send({"jsonrpc": "2.0", "id": identifier, "method": method, "params": params})
        return started

    def response(self, identifier, started, allow_error=False):
        key = json.dumps(identifier, separators=(",", ":"))
        if key not in self.pending:
            raise ValueError("response wait has no admitted request")
        deadline = time.monotonic() + TIMEOUT
        while key not in self.responses:
            frame, received = self._receive(deadline)
            if frame is None:
                raise ValueError("analyzer EOF before terminal response")
            self._dispatch(frame, received)
        value, received = self.responses.pop(key)
        self.pending.remove(key)
        if "error" in value and not allow_error:
            raise ValueError(f"request failed: {value['error']!r}")
        return (value if allow_error else value["result"]), received - started, received

    def request(self, identifier, method, params):
        started = self.begin_request(identifier, method, params)
        return self.response(identifier, started)

    def wait_notification(self, method, predicate, after_ns=0, timeout=TIMEOUT):
        deadline = time.monotonic() + timeout
        while True:
            # A caller-supplied operation barrier retires older pending notices;
            # the bounded raw history still preserves every received notification.
            while self.notifications and self.notifications[0][1] < after_ns:
                self.notifications.popleft()
            for index, (value, received) in enumerate(self.notifications):
                if received >= after_ns and value.get("method") == method and predicate(value.get("params")):
                    del self.notifications[index]
                    return value["params"], received
            frame, received = self._receive(deadline)
            if frame is None:
                raise ValueError(f"analyzer EOF before expected {method} notification")
            self._dispatch(frame, received)

    def finish(self):
        deadline = time.monotonic() + TIMEOUT
        while True:
            frame, received = self._receive(deadline)
            if frame is None:
                break
            self._dispatch(frame, received)
        code = self.process.wait(timeout=max(0.01, deadline - time.monotonic()))
        if code != 0 or self.pending or self.responses or self.tcp_stdout:
            raise ValueError(f"invalid analyzer shutdown: code={code}, stderr={bytes(self.stderr)!r}")

    def close(self):
        self.stop.set()
        if self.process is not None and self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=TIMEOUT)
        if self.socket is not None:
            try:
                self.socket.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        for worker in self.threads:
            worker.join(timeout=1)
        for stream in (self.stdin, self.reader):
            if stream is not None:
                stream.close()
        if self.socket is not None:
            self.socket.close()
        if any(worker.is_alive() for worker in self.threads):
            raise ValueError("protocol peer failed to retire reader threads")

    def __enter__(self):
        return self

    def __exit__(self, kind, value, traceback):
        self.close()
