// Test-only transparent relay: real Zed and analyzer own all ordinary messages.
// Only our unique workspace/applyEdit request IDs are intercepted on their reply.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { StreamMessageReader, StreamMessageWriter } from 'vscode-languageserver-protocol/node';

const configuration = JSON.parse(await readFile(process.argv[2], 'utf8'));
const initial = await readFile(configuration.sample, 'utf8');
const changed = initial.replace('true', 'missing');
assert.notEqual(changed, initial);
const prefix = `kagari-zed-probe:${randomUUID()}:`;
const server = spawn(configuration.analyzer, ['--stdio'], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
const clientReader = new StreamMessageReader(process.stdin);
const serverReader = new StreamMessageReader(server.stdout);
const clientWriter = new StreamMessageWriter(process.stdout);
const serverWriter = new StreamMessageWriter(server.stdin);
let queue = Promise.resolve();
let done = false;
let uri;
let version;
let initializeId;
let phase;
let traceBytes = 0;
let pending = 0;
const trace = [];
const report = { pid: process.pid, analyzerPid: server.pid, initialized: false, opened: false,
  changed: false, restored: false, edits: [], capabilities: undefined };
const timer = setTimeout(() => void finish(new Error('Zed protocol transcript exceeded its deadline')), configuration.timeoutMs);

function sameFile(candidate) {
  try {
    const actual = path.resolve(fileURLToPath(candidate));
    const expected = path.resolve(configuration.sample);
    return process.platform === 'win32' ? actual.toLowerCase() === expected.toLowerCase() : actual === expected;
  } catch { return false; }
}

async function finish(error) {
  if (done) return;
  done = true;
  clearTimeout(timer);
  const result = { ...report, success: !error, error: error?.message, trace };
  const temporary = `${configuration.result}.tmp`;
  await writeFile(temporary, JSON.stringify(result, null, 2) + '\n');
  await rename(temporary, configuration.result);
  if (error) {
    process.stderr.write(`Kagari Zed probe failed: ${error.message}\n`);
    server.kill();
    process.exitCode = 1;
  }
}

function remember(direction, message) {
  const bytes = Buffer.byteLength(JSON.stringify(message));
  assert.ok(bytes <= 256 * 1024 && trace.length < 256 && traceBytes + bytes <= 2 * 1024 * 1024,
    'Bounded protocol transcript');
  traceBytes += bytes;
  trace.push({ direction, message });
}

async function edit(name, text) {
  const currentText = name === 'change' ? initial : changed;
  const lines = currentText.split('\n');
  const id = `${prefix}${name}`;
  phase = { name, id, text, applied: false, synchronized: false, diagnostic: false };
  const request = { jsonrpc: '2.0', id, method: 'workspace/applyEdit', params: {
    label: 'Kagari isolated interoperability acceptance',
    edit: { documentChanges: [{ textDocument: { uri, version }, edits: [{
      range: { start: { line: 0, character: 0 }, end: { line: lines.length - 1, character: lines.at(-1).length } },
      newText: text,
    }] }] },
  } };
  remember('probe->zed', request);
  await clientWriter.write(request);
}

async function advance() {
  if (!phase?.applied || !phase.synchronized || !phase.diagnostic) return;
  report.edits.push({ name: phase.name, version, applied: true, synchronized: true, diagnostic: true });
  if (phase.name === 'change') {
    report.changed = true;
    await edit('restore', initial);
  } else {
    report.restored = true;
    await finish();
  }
}

async function fromClient(message) {
  remember('zed->analyzer', message);
  if (typeof message.id === 'string' && message.id.startsWith(prefix)) {
    assert.equal(message.id, phase?.id, 'Only the outstanding owned request ID may be intercepted');
    assert.equal(message.result?.applied, true, `Zed rejected applyEdit: ${JSON.stringify(message)}`);
    phase.applied = true;
    await advance();
    return;
  }
  if (message.method === 'initialize') {
    initializeId = message.id;
    report.capabilities = message.params.capabilities;
    assert.equal(report.capabilities.workspace?.applyEdit, true, 'Real Zed must support workspace/applyEdit');
  }
  if (message.method === 'textDocument/didOpen' && sameFile(message.params.textDocument.uri)) {
    const document = message.params.textDocument;
    assert.equal(document.text, initial);
    assert.equal(document.languageId, 'kagari');
    uri = document.uri;
    version = document.version;
    report.opened = true;
  }
  if (message.method === 'textDocument/didChange' && sameFile(message.params.textDocument.uri)) {
    assert.ok(phase, 'No unsolicited fixture edit');
    assert.ok(message.params.textDocument.version > version);
    assert.equal(message.params.contentChanges.length, 1);
    assert.equal(message.params.contentChanges[0].range, undefined, 'Analyzer negotiated full synchronization');
    assert.equal(message.params.contentChanges[0].text, phase.text);
    version = message.params.textDocument.version;
    phase.synchronized = true;
  }
  await serverWriter.write(message);
  await advance();
}

async function fromServer(message) {
  remember('analyzer->zed', message);
  assert.ok(!(typeof message.id === 'string' && message.id.startsWith(prefix)), 'Analyzer collided with probe IDs');
  if (initializeId !== undefined && message.id === initializeId && message.result) {
    report.initialized = true;
    report.serverInfo = message.result.serverInfo;
    report.serverCapabilities = message.result.capabilities;
  }
  await clientWriter.write(message);
  if (message.method !== 'textDocument/publishDiagnostics' || message.params.uri !== uri) return;
  const diagnostics = message.params.diagnostics;
  if (!phase) {
    if (diagnostics.some((diagnostic) => diagnostic.code === 'KG_TYPE_RETURN_TYPE_MISMATCH')) await edit('change', changed);
    return;
  }
  if (!phase.synchronized) return;
  const expected = phase.name === 'change' ? 'KG_RESOLVE_UNKNOWN_NAME' : 'KG_TYPE_RETURN_TYPE_MISMATCH';
  if (diagnostics.some((diagnostic) => diagnostic.code === expected)) {
    if (message.params.version !== undefined) assert.equal(message.params.version, version);
    phase.diagnostic = true;
  }
  await advance();
}

function dispatch(handler, message) {
  if (done) {
    void (handler === fromClient ? serverWriter : clientWriter).write(message).catch(() => {});
    return;
  }
  if (++pending > 64) return void finish(new Error('Relay queue capacity exceeded'));
  queue = queue.then(() => handler(message)).catch(finish).finally(() => { --pending; });
}

clientReader.listen((message) => dispatch(fromClient, message));
serverReader.listen((message) => dispatch(fromServer, message));
clientReader.onError((error) => void finish(error));
serverReader.onError((error) => void finish(error));
server.once('error', (error) => void finish(error));
server.once('exit', (code, signal) => { if (!done) void finish(new Error(`Analyzer exited early: ${code ?? signal}`)); });
server.stderr.on('data', (bytes) => process.stderr.write(bytes));
process.stdin.on('end', () => { server.stdin.end(); if (!done) void finish(new Error('Zed input closed before transcript completed')); });
