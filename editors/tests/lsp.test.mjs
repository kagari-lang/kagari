import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { access } from "node:fs/promises";
import { createServer } from "node:net";
import { isAbsolute } from "node:path";
import test from "node:test";
import { createSemanticFixture, semanticRoundtrip } from "./lsp/semantic.mjs";
import {
  createProtocolConnection,
  InitializeRequest,
  InitializedNotification,
  ShutdownRequest,
  ExitNotification,
  DidOpenTextDocumentNotification,
  DidChangeTextDocumentNotification,
  DidCloseTextDocumentNotification,
  WorkDoneProgressCreateRequest,
  ConfigurationRequest,
  WorkspaceFoldersRequest,
} from "vscode-languageserver-protocol/node";

const timeoutMs = 10_000;
const analyzer = process.env.KAGARI_TEST_ANALYZER;

async function bounded(operation, label) {
  let timer;
  try {
    return await Promise.race([
      operation,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label} exceeded ${timeoutMs} ms`)), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

for (const transport of ["stdio", "tcp"]) {
  test(`independent Node protocol client: ${transport}`, { timeout: 30_000 }, async (context) => {
    assert(analyzer && isAbsolute(analyzer), "set KAGARI_TEST_ANALYZER to an explicit built analyzer absolute path");
    await access(analyzer);
    const started = performance.now();
    const server = transport === "tcp" ? createServer() : undefined;
    let socket;
    let connection;
    let child;
    let exited;
    let stderr = "";
    let tcpStdout = "";
    const failures = [];
    const notifications = [];
    const fixture = transport === "stdio" ? await createSemanticFixture() : undefined;
    try {
      let incoming;
      let args = ["--stdio"];
      if (server) {
        const listening = once(server, "listening");
        server.listen(0, "127.0.0.1");
        await bounded(listening, "TCP listener setup");
        incoming = once(server, "connection");
        args = ["--tcp-connect", `127.0.0.1:${server.address().port}`];
      }
      child = spawn(analyzer, args, { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
      exited = once(child, "exit");
      child.stderr.setEncoding("utf8").on("data", (data) => {
        stderr += data.slice(0, Math.max(0, 65_536 - stderr.length));
      });
      if (server) {
        child.stdin.end();
        child.stdout.setEncoding("utf8").on("data", (data) => {
          tcpStdout += data.slice(0, Math.max(0, 65_536 - tcpStdout.length));
        });
        [socket] = await bounded(Promise.race([
          incoming,
          exited.then(([code]) => { throw new Error(`analyzer exited before TCP connection: ${code}; ${stderr}`); }),
        ]), "TCP connection");
        socket.setNoDelay(true);
        server.close();
      }
      connection = createProtocolConnection(socket ?? child.stdout, socket ?? child.stdin);
      connection.onError((error) => failures.push(String(error)));
      connection.onRequest(WorkDoneProgressCreateRequest.type, () => null);
      connection.onRequest(ConfigurationRequest.type, ({ items }) => items.map(() => null));
      connection.onRequest(WorkspaceFoldersRequest.type, () => null);
      connection.onNotification((method, params) => {
        if (notifications.length === 128) failures.push("notification budget exceeded");
        else notifications.push({ method, params });
      });
      connection.listen();
      const initialized = await bounded(connection.sendRequest(InitializeRequest.type, {
        processId: process.pid,
        rootUri: fixture?.uri ?? null,
        workspaceFolders: fixture ? [{ uri: fixture.uri, name: "headless-semantic" }] : null,
        capabilities: { general: { positionEncodings: ["utf-8", "utf-16"] },
          textDocument: { publishDiagnostics: { versionSupport: true } } },
      }), "initialize");
      assert.equal(initialized.capabilities.positionEncoding, "utf-16");
      assert.equal(initialized.capabilities.textDocumentSync.openClose, true);
      assert.equal(initialized.capabilities.textDocumentSync.change, 1);
      assert.equal(initialized.serverInfo.name, "kagari-analyzer");
      assert(initialized.serverInfo.version.length > 0);
      await connection.sendNotification(InitializedNotification.type, {});
      let semantic;
      if (fixture) {
        for (const provider of ["hoverProvider", "definitionProvider", "implementationProvider", "completionProvider"]) {
          assert.ok(initialized.capabilities[provider], `missing ${provider}`);
        }
        semantic = await semanticRoundtrip(connection, fixture, bounded);
      }
      const uri = "untitled:Node%20独立-client.kgr";
      await connection.sendNotification(DidOpenTextDocumentNotification.type, {
        textDocument: { uri, languageId: "kagari", version: 1, text: 'val text = "😀";\r\n' },
      });
      await connection.sendNotification(DidChangeTextDocumentNotification.type, {
        textDocument: { uri, version: 2 }, contentChanges: [{ text: "" }],
      });
      await connection.sendNotification(DidCloseTextDocumentNotification.type, { textDocument: { uri } });
      const requests = ["kagari/unsupported-one", "kagari/unsupported-two"].map((method) =>
        assert.rejects(bounded(connection.sendRequest(method, {}), method), (error) => error.code === -32601));
      await Promise.all(requests);
      assert.equal(await bounded(connection.sendRequest(ShutdownRequest.type), "shutdown"), null);
      await connection.sendNotification(ExitNotification.type);
      // Keep the writer open: the exit notification must end the server itself.
      const [code, signal] = await bounded(exited, "process exit");
      assert.equal(code, 0, stderr);
      assert.equal(signal, null);
      assert.equal(tcpStdout, "", "TCP server must not emit protocol bytes on stdout");
      assert.deepEqual(failures, []);
      context.diagnostic(JSON.stringify({ transport, durationMs: performance.now() - started,
        notifications: notifications.length + (semantic?.diagnosticNotifications ?? 0),
        serverVersion: initialized.serverInfo.version, semantic }));
    } finally {
      connection?.dispose();
      socket?.destroy();
      if (server?.listening) server.close();
      if (child && child.exitCode === null && child.signalCode === null) {
        child.kill();
        await bounded(exited, "owned child cleanup");
      }
      child?.stdin.destroy();
      child?.stdout.destroy();
      child?.stderr.destroy();
      await fixture?.cleanup();
    }
  });
}
