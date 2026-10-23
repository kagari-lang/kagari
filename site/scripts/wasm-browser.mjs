import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createServer, request } from 'node:http';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bounded, launchBrowser } from './browser-support.mjs';
import { repositoryRoot, runCommand, sha256, snapshotProducts, verifyProducts } from './wasm-products.mjs';

const base = '/kagari/wasm';
const usage = 'Usage: node scripts/wasm-browser.mjs [--executable-path PATH] [--mode source|artifact]';
const pageHtml = Buffer.from('<!doctype html><html lang="en"><meta charset="utf-8"><title>Kagari WB03 WASM contracts</title><link rel="icon" href="data:,"><body><h1>Kagari WB03 WASM contracts</h1><pre id="result">Running bounded browser contract…</pre></body></html>');

/** Own a loopback-only immutable snapshot; request text never becomes a file path.
 * Shared with later browser integration tests, without requiring or touching dist.
 */
export async function startProductServer(snapshot) {
  const files = new Map([[`${base}/`, { bytes: pageHtml, type: 'text/html; charset=utf-8' }]]);
  for (const [name, bytes] of snapshot.files) {
    if (!/\.(js|wasm|kbc)$/u.test(name)) continue;
    files.set(`${base}/${name}`, { bytes: Buffer.from(bytes), type: name.endsWith('.js') ? 'text/javascript; charset=utf-8' : name.endsWith('.wasm') ? 'application/wasm' : 'application/octet-stream' });
  }
  const server = createServer({ headersTimeout: 5_000, requestTimeout: 10_000, keepAliveTimeout: 1_000 }, (incoming, response) => {
    response.setTimeout(10_000, () => response.destroy());
    response.setHeader('Cache-Control', 'no-store');
    response.setHeader('X-Content-Type-Options', 'nosniff');
    function send(status, bytes = Buffer.alloc(0), type = 'text/plain; charset=utf-8') {
      response.writeHead(status, { 'Content-Type': type, 'Content-Length': bytes.length });
      response.end(incoming.method === 'HEAD' ? undefined : bytes);
    }
    if (!['GET', 'HEAD'].includes(incoming.method)) { response.setHeader('Allow', 'GET, HEAD'); send(405); return; }
    let pathname;
    try { pathname = decodeURIComponent(incoming.url.split('?')[0]); }
    catch { send(400); return; }
    if (!pathname.startsWith('/') || /[\\%:#\u0000-\u001f\u007f]/u.test(pathname) || pathname.split('/').some((part) => part === '.' || part === '..')) { send(400); return; }
    const file = files.get(pathname);
    if (!file) { send(404); return; }
    send(200, file.bytes, file.type);
  });
  server.maxRequestsPerSocket = 100;
  server.on('clientError', (_error, socket) => socket.destroy());
  async function close() {
    const closed = new Promise((accept, reject) => server.close((error) => error ? reject(error) : accept()));
    server.closeAllConnections();
    await bounded(() => closed, 5_000, 'WASM HTTP teardown');
  }
  try {
    await bounded(() => new Promise((accept, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', accept); }), 5_000, 'WASM HTTP startup');
    return { origin: `http://127.0.0.1:${server.address().port}`, close };
  } catch (error) { if (server.listening) await close(); throw error; }
}

function rawRequest(origin, path, method = 'GET') {
  return new Promise((accept, reject) => {
    const pending = request(`${origin}/`, { path, method, timeout: 5_000 }, (response) => {
      let bytes = 0;
      response.on('data', (chunk) => { bytes += chunk.length; });
      response.once('error', reject);
      response.once('end', () => accept({ status: response.statusCode, type: response.headers['content-type'], bytes }));
    });
    pending.on('timeout', () => pending.destroy(new Error('HTTP boundary probe timed out')));
    pending.once('error', reject);
    pending.end();
  });
}

const cases = [
  { name: 'source-fixed64-output', mode: 'source', kind: 'execute', source: 'use web::print; fn main() -> u64 { val minimum = -9223372036854775808i64; val maximum = 9223372036854775807i64; if minimum >= 0i64 || maximum <= 0i64 { return 0u64; } print("hello 😀"); 18446744073709551615u64 }', result: { tag: 'u64', value: '18446744073709551615' }, output: ['hello 😀'] },
  { name: 'source-wide-collections-parsing-take', mode: 'source', kind: 'execute', source: `use std::collections::HashSet;
fn main() -> u64 {
    val maximum = match u64::from_str("18446744073709551615") { Ok(value) => value, Err(_) => { return 0u64; } };
    if "{maximum}" != "18446744073709551615" { return 0u64; }
    val keys = HashSet::new(); keys.insert(maximum);
    if !keys.contains(maximum) || keys.len() != 1u64 { return 0u64; }
    val values = Vec::from([maximum]); values.sort();
    if values.take(4294967296u64).count() != 1u64 { return 0u64; }
    if values.skip(4294967296u64).count() != 0u64 { return 0u64; }
    values.take(18446744073709551615u64).sum()
}`, result: { tag: 'u64', value: '18446744073709551615' }, output: [] },
  { name: 'source-wide-read-no-wrap-effects', mode: 'source', kind: 'trap', source: 'use web::print; fn index() -> u64 { print("index"); 4294967296u64 } fn main() -> i32 { val values = [7]; print("before"); val result = values[index()]; print("after"); result }', output: ['before', 'index'], indexDiagnostic: 'array index 4294967296 is out of bounds' },
  { name: 'source-wide-write-no-wrap-effects', mode: 'source', kind: 'trap', source: 'use web::print; fn base(values: [i32]) -> [i32] { print("base"); values } fn index() -> u64 { print("index"); 18446744073709551615u64 } fn replacement() -> i32 { print("replacement"); 9 } fn main() { val values = [7]; base(values)[index()] = replacement(); print("after"); }', output: ['base', 'index', 'replacement'], indexDiagnostic: 'InvalidIndex(18446744073709551615)' },
  { name: 'source-gc-repeated-rooted-results', mode: 'source', kind: 'gc', source: 'fn main() -> String { val retained = ["rooted 😀"]; for i in 0..4096 { val garbage = ["{i}😀"]; } retained[0] }' },
  { name: 'source-unicode-crlf-diagnostics', mode: 'source', kind: 'diagnostics' },
  { name: 'source-removed-names-and-suffixes', mode: 'source', kind: 'removed' },
  { name: 'source-sliced-cancel-reset', mode: 'source', kind: 'cancel' },
  { name: 'source-input-output-bounds', mode: 'source', kind: 'bounds' },
  { name: 'artifact-native-product', mode: 'artifact', kind: 'artifact', result: { tag: 'u64', value: '18446744073709551615' }, output: ['artifact hello 😀'] },
  { name: 'artifact-corruption-and-admission', mode: 'artifact', kind: 'corruption' },
];

// Serialized by Playwright into the page. No closure dependencies or Worker protocol.
async function browserCase(test) {
  const trace = [];
  window.wb03Trace = trace;
  const equal = (actual, expected, label) => {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  };
  const check = (condition, message) => { if (!condition) throw new Error(message); };
  const step = (name) => { window.wb03Step = name; };
  const response = (text, label) => {
    const value = JSON.parse(text);
    equal(value.version, 1, `${label} protocol`);
    if (trace.length < 32) trace.push({ label, status: value.status, error: value.error, diagnostics: value.diagnostics, result: value.result, outputCount: value.output.length, output_truncated: value.output_truncated });
    return value;
  };
  const expectStatus = (value, status, label) => {
    check(value.status === status, `${label}: ${JSON.stringify(value)}`);
    return value;
  };
  step('import/init');
  const initializedAt = performance.now();
  const module = await import(`/kagari/wasm/${test.mode}/kagari_web.js`);
  await module.default();
  const initializationMs = performance.now() - initializedAt;
  equal(typeof module.BrowserSession.prototype.compile, test.mode === 'source' ? 'function' : 'undefined', 'feature surface');
  step('construct BrowserSession');
  const session = new module.BrowserSession();
  const timings = { initializationMs, compilationMs: [], executionMs: [] };
  function compile(source) {
    step('compile');
    const started = performance.now();
    const result = response(session.compile(source), 'compile');
    timings.compilationMs.push(performance.now() - started);
    return expectStatus(result, 'ready', 'compile');
  }
  async function finish({ start = true, slice = 1000 } = {}) {
    step('start/drive');
    const started = performance.now();
    const output = [];
    let truncated = false;
    let outputBytes = 0;
    function collect(value) {
      output.push(...value.output);
      outputBytes += value.output.reduce((size, text) => size + new TextEncoder().encode(text).length, 0);
      check(output.length <= 512 && outputBytes <= 1024 * 1024, 'Harness cumulative output bound exceeded');
      truncated ||= value.output_truncated;
    }
    if (start) collect(expectStatus(response(session.start(), 'start'), 'runnable', 'start'));
    for (let count = 0; count < 500; count++) {
      const value = response(session.drive(slice), 'drive');
      collect(value);
      if (value.status !== 'runnable') {
        check(value.status === 'complete' || value.status === 'error', `Unexpected ${value.status}; no async providers installed`);
        timings.executionMs.push(performance.now() - started);
        return { ...value, output, output_truncated: truncated, slices: count + 1 };
      }
      // Browser tasks, not an endlessly replenished microtask queue.
      await new Promise((accept) => setTimeout(accept, 0));
    }
    throw new Error('Finite case exceeded 500 drive slices');
  }
  const artifactBytes = async () => {
    const result = await fetch('/kagari/wasm/browser-basic.kbc');
    check(result.ok, `Fixture HTTP ${result.status}`);
    return new Uint8Array(await result.arrayBuffer());
  };
  let evidence = {};
  if (test.kind === 'execute' || test.kind === 'trap') {
    compile(test.source);
    const result = await finish();
    equal(result.output, test.output, 'once-only ordered output');
    equal(result.output_truncated, false, 'unexpected truncation');
    if (test.kind === 'trap') {
      expectStatus(result, 'error', 'large index must trap, not wrap');
      // EmbeddingError::vm maps both runtime bounds errors and InvalidIndex to
      // ScriptTrap. Match the exact underlying cause, not digits in trace metadata.
      equal(result.error.code, 'KG_RUNTIME_SCRIPT_TRAP', 'SDK index trap');
      equal(result.error.truncated, false, 'index diagnostic must be complete');
      check(result.error.message.includes(`message: "${test.indexDiagnostic}"`), `Wrong index cause or lost precision: ${result.error.message}`);
      equal(result.result, null, 'trap has no success result');
      const retired = response(session.drive(1), 'drive after trap');
      equal(retired.error?.code, 'WEB_NO_RUN', 'trap retires execution rather than continuing after invalid access');
      equal(retired.output, [], 'no post-trap effects');
    } else {
      expectStatus(result, 'complete', 'execution');
      equal(result.result, test.result, 'lossless result');
    }
    evidence = result;
  } else if (test.kind === 'gc') {
    compile(test.source);
    for (let run = 0; run < 4; run++) {
      const result = expectStatus(await finish({ slice: 10000 }), 'complete', 'allocation stress');
      equal(result.result, { tag: 'string', value: 'rooted 😀', truncated: false }, 'retained string across allocations/repeated calls');
      equal(result.output, [], 'GC case output');
    }
    evidence = { calls: 4, allocationsPerCall: 4096, limitation: 'Automatic-GC allocation stress and owned result conversion; adapter does not expose collection/root counters.' };
  } else if (test.kind === 'diagnostics') {
    step('Unicode/CRLF compile');
    const source = '// 😀\r\nfn main() -> i32 { missing }';
    const value = expectStatus(response(session.compile(source), 'diagnostics'), 'error', 'unknown name');
    const diagnostic = value.diagnostics.find((item) => item.location?.byte_start === 28);
    check(diagnostic, `Missing expected diagnostic: ${JSON.stringify(value)}`);
    equal(diagnostic.severity, 'error', 'diagnostic severity');
    check(diagnostic.code.startsWith('KG_'), 'Lost diagnostic code');
    equal(diagnostic.location.origin, 'memory://main.kgr', 'diagnostic origin');
    equal(diagnostic.location.start, { offset: 26, line: 1, character: 19 }, 'UTF-16 start');
    equal(diagnostic.location.end, { offset: 33, line: 1, character: 26 }, 'UTF-16 end');
    equal(diagnostic.location.byte_end, 35, 'UTF-8 end');
    check(/^\d+$/u.test(diagnostic.location.revision), 'Revision must remain lossless');
    evidence = value;
  } else if (test.kind === 'removed') {
    for (const source of ['fn main() -> usize { 1 }', 'fn main() -> isize { 1 }', 'fn main() { val n = 1usize; }', 'fn main() { val n = 1isize; }']) {
      step(`reject ${source}`);
      const value = expectStatus(response(session.compile(source), 'removed name/suffix'), 'error', 'removed spelling');
      check(value.diagnostics.some((item) => item.severity === 'error'), 'Missing removed-spelling diagnostic');
    }
  } else if (test.kind === 'cancel') {
    compile('use web::print; fn main() { print("before cancellation"); loop {} }');
    const output = [];
    equal(response(session.start(), 'start').status, 'runnable', 'loop start');
    const slice = response(session.drive(100), 'loop slice');
    equal(slice.status, 'runnable', 'loop yields');
    output.push(...slice.output);
    equal(response(session.start(), 'busy').error.code, 'WEB_BUSY', 'exclusive execution');
    await new Promise((accept) => setTimeout(accept, 0));
    session.cancel();
    const cancelled = await finish({ start: false });
    equal(cancelled.error?.code, 'KG_RUNTIME_CANCELLED', 'cooperative cancellation');
    output.push(...cancelled.output);
    equal(output, ['before cancellation'], 'completed effects survive cancellation');
    equal(response(session.start(), 'restart').status, 'runnable', 'restart cancelled program');
    equal(response(session.reset(), 'active reset').status, 'idle', 'active reset');
    equal(response(session.start(), 'no program').error.code, 'WEB_NO_PROGRAM', 'reset unloads program');
    compile('fn main() -> i32 { 42 }');
    equal((await finish()).result, { tag: 'i32', value: '42' }, 'reuse after cancellation/reset');
  } else if (test.kind === 'bounds') {
    step('source admission');
    // UTF-8 bytes, not JS UTF-16 length; no compiler invocation should occur.
    equal(response(session.compile('😀'.repeat(65537)), 'source limit').error?.code, 'WEB_SOURCE_LIMIT', 'source byte bound');
    compile('use web::print; fn main() { for i in 0..300 { print("x"); } }');
    const count = expectStatus(await finish({ slice: 100000 }), 'complete', 'output count bound');
    equal(count.output.length, 256, 'bounded message count');
    check(count.output.every((text) => text === 'x'), 'Output contents changed');
    equal(count.output_truncated, true, 'visible count truncation');
    equal(response(session.drain_output(), 'empty drain').output, [], 'drain is destructive');
    compile(`use web::print; fn main() { for i in 0..20 { print("${'😀'.repeat(1025)}"); } }`);
    const bytes = expectStatus(await finish({ slice: 100000 }), 'complete', 'output byte bound');
    equal(bytes.output.length, 16, '64 KiB aggregate output');
    check(bytes.output.every((text) => text === '😀'.repeat(1024)), '4 KiB per-message UTF-8 truncation');
    equal(bytes.output_truncated, true, 'visible byte truncation');
    evidence = { count: count.output.length, messagesAtByteLimit: bytes.output.length, bytesPerMessage: 4096 };
  } else if (test.kind === 'artifact') {
    step('load native artifact');
    expectStatus(response(session.load_artifact(await artifactBytes()), 'artifact load'), 'ready', 'native artifact');
    const result = expectStatus(await finish(), 'complete', 'source-free execution');
    equal(result.output, test.output, 'native/browser output agreement');
    equal(result.result, test.result, 'native/browser fixed64 agreement');
    equal(result.output_truncated, false, 'artifact output truncation');
    evidence = result;
  } else if (test.kind === 'corruption') {
    step('corrupt artifact validation');
    const bytes = await artifactBytes();
    expectStatus(response(session.load_artifact(bytes), 'valid baseline'), 'ready', 'baseline load');
    const magic = bytes.slice(); magic[0] ^= 255;
    const payload = bytes.slice();
    const marker = new TextEncoder().encode('artifact hello');
    const offset = payload.findIndex((_byte, index) => marker.every((byte, position) => payload[index + position] === byte));
    check(offset >= 0, 'Fixture payload marker missing');
    payload[offset] ^= 1;
    const rejected = [];
    for (const [label, input] of [['magic', magic], ['truncated', bytes.subarray(0, 32)], ['payload fingerprint', payload]]) {
      const value = expectStatus(response(session.load_artifact(input), label), 'error', label);
      check(['WEB_ARTIFACT_DECODE', 'WEB_ARTIFACT_VALIDATION'].includes(value.error?.code), `${label}: ${JSON.stringify(value)}`);
      rejected.push({ label, error: value.error });
    }
    equal(response(session.load_artifact(new Uint8Array(16 * 1024 * 1024 + 1)), 'artifact admission').error?.code, 'WEB_ARTIFACT_LIMIT', 'artifact byte bound');
    equal((await finish()).result, { tag: 'u64', value: '18446744073709551615' }, 'invalid candidates preserve loaded program');
    evidence = rejected;
  } else throw new Error(`Unknown case ${test.kind}`);
  step('normal cleanup');
  equal(response(session.reset(), 'reset').status, 'idle', 'normal cleanup');
  equal(response(session.drain_output(), 'drain').output, [], 'no pending output after reset');
  session.free();
  step('complete');
  return { timings, evidence, trace };
}

async function main(explicitPath, mode) {
  const started = Date.now();
  const directory = join(repositoryRoot, 'target/wasm-browser/reports', `${new Date().toISOString().replaceAll(/[:.]/gu, '-')}-${process.pid}`);
  await mkdir(directory, { recursive: true });
  const reportPath = join(directory, 'report.json');
  const report = {
    startedAt: new Date(started).toISOString(), node: process.version, platform: process.platform,
    selectedCases: cases.filter((test) => !mode || test.mode === mode).map((test) => test.name),
    launchAttempts: [], checks: [], screenshots: [],
    limits: ['WB03 direct-page WASM only; Worker/hard-stop/stale-message tests belong to WB04.', 'Default workspace dev profile, default Cargo parallelism; timings are diagnostic, not performance acceptance.', 'Allocation stress does not expose exact GC collection/root counts.', 'Local Chromium only; no CI/backend/platform matrix or public deployment acceptance.'],
  };
  const save = async () => { report.elapsedMs = Date.now() - started; await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`); };
  let browser;
  let server;
  let snapshot;
  async function check(name, operation) {
    const item = { name, status: 'running' };
    report.checks.push(item);
    await save();
    const start = Date.now();
    try { await operation(item); item.status = 'passed'; console.log(`PASS ${name}`); }
    catch (error) { item.status = 'failed'; item.error = error.stack ?? String(error); console.error(`FAIL ${name}: ${error.message ?? error}`); }
    item.elapsedMs = Date.now() - start;
    await save();
  }
  try {
    snapshot = await snapshotProducts();
    report.products = snapshot.record;
    report.harnessSha256 = sha256(await readFile(fileURLToPath(import.meta.url)));
    server = await startProductServer(snapshot);
    report.origin = server.origin;
    const launched = await bounded(() => launchBrowser(explicitPath, report.launchAttempts), 45_000, 'Browser discovery/launch');
    browser = launched.browser;
    report.browser = { version: browser.version(), executablePath: launched.executablePath };
    console.log(`Browser: ${report.browser.version} (${report.browser.executablePath})`);
    await check('snapshot-http-boundary', async () => {
      for (const [path, status] of [[`${base}/`, 200], ['/', 404], ['/kagari/../Cargo.toml', 400], [`${base}/%2e%2e/Cargo.toml`, 400], [`${base}/%252e%252e/Cargo.toml`, 400], [`${base}/..%5cCargo.toml`, 400], [`${base}/C:%5cWindows`, 400], [`${base}/%00`, 400], [`${base}/%ZZ`, 400], [`${base}/products.json`, 404]]) assert.equal((await rawRequest(server.origin, path)).status, status, path);
      assert.equal((await rawRequest(server.origin, `${base}/`, 'POST')).status, 405);
      assert.equal((await rawRequest(server.origin, `${base}/`, 'HEAD')).bytes, 0);
      for (const mode of ['source', 'artifact']) {
        const served = await rawRequest(server.origin, `${base}/${mode}/kagari_web_bg.wasm`);
        assert.equal(served.status, 200);
        assert.equal(served.type, 'application/wasm');
        assert.equal(served.bytes, snapshot.files.get(`${mode}/kagari_web_bg.wasm`).length);
      }
    });
    for (const test of cases.filter((test) => !mode || test.mode === mode)) {
      assert(Date.now() - started < 145_000, 'Harness reached 145s admission deadline; remaining cases unrun');
      await check(test.name, async (item) => {
        const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, serviceWorkers: 'block' });
        const errors = [];
        item.consoleErrors = [];
        context.setDefaultTimeout(5_000);
        context.setDefaultNavigationTimeout(8_000);
        await context.route('**/*', async (route) => {
          if (new URL(route.request().url()).origin !== server.origin) { errors.push(`External request: ${route.request().url()}`); await route.abort(); }
          else await route.continue();
        });
        const page = await context.newPage();
        page.on('pageerror', (error) => { if (errors.length < 16) errors.push(error.stack ?? error.message); });
        page.on('console', (message) => { if (message.type() === 'error' && item.consoleErrors.length < 16) item.consoleErrors.push(message.text().slice(0, 8192)); });
        try {
          const navigation = await page.goto(`${server.origin}${base}/`);
          assert.equal(navigation.status(), 200);
          item.result = await bounded(() => page.evaluate(browserCase, test), 25_000, test.name);
          assert.deepEqual(errors, [], 'Uncaught browser/network errors');
          assert.deepEqual(item.consoleErrors, [], 'Browser console errors');
        } catch (error) {
          try { item.failureState = await bounded(() => page.evaluate(() => ({ step: window.wb03Step, trace: window.wb03Trace })), 1_000, 'Failure state'); }
          catch (captureError) { item.captureError = captureError.message; }
          throw error;
        } finally {
          try {
            await bounded(() => page.locator('#result').evaluate((element, text) => { element.textContent = text; }, JSON.stringify({ name: test.name, result: item.result ?? item.failureState, consoleErrors: item.consoleErrors }, null, 2)), 1_000, 'Evidence text');
            const name = `${test.name}.png`;
            await page.screenshot({ path: join(directory, name), timeout: 2_000 });
            report.screenshots.push(relative(repositoryRoot, join(directory, name)).replaceAll('\\', '/'));
          } catch (error) { item.screenshotError = error.message; }
          item.browserErrors = errors;
          // A fatal WASM exception never reuses this instance. Destroy its whole context.
          await bounded(() => context.close(), 5_000, 'Case context teardown');
        }
      });
    }
  } catch (error) {
    report.fatal = error.stack ?? String(error);
    console.error(error);
  } finally {
    if (browser) await check('browser-teardown', () => bounded(() => browser.close(), 5_000, 'Browser teardown'));
    if (server) await check('server-teardown', () => server.close());
    if (snapshot) await check('source-products-unchanged', async () => {
      assert.deepEqual(await verifyProducts(), snapshot.record, 'Product provenance changed during browser run');
      assert.equal(sha256(await readFile(fileURLToPath(import.meta.url))), report.harnessSha256, 'Harness changed during run');
    });
    report.success = !report.fatal && report.checks.every((item) => item.status === 'passed');
    await save();
    console.log(`WB03 report: ${reportPath}`);
    if (!report.success) process.exitCode = 1;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  try {
    if (args.length === 1 && args[0] === '--help') console.log(`${usage}\nRequires current products; never builds, installs browsers, or uses dist.\nRuns installed Playwright with managed Chromium or installed Chrome/Edge.\nEach case: 25s, 500 slices; runner: 180s plus bounded process-tree teardown.\nReports/screenshots: ../target/wasm-browser/reports/.`);
    else {
      const child = args[0] === '--internal-child';
      const options = child ? args.slice(1) : args;
      const parsed = {};
      assert.equal(options.length % 2, 0, usage);
      for (let index = 0; index < options.length; index += 2) {
        const key = options[index];
        assert(['--executable-path', '--mode'].includes(key) && options[index + 1] && !Object.hasOwn(parsed, key), usage);
        parsed[key] = options[index + 1];
      }
      assert(!parsed['--mode'] || ['source', 'artifact'].includes(parsed['--mode']), usage);
      if (child) await main(parsed['--executable-path'] ? resolve(parsed['--executable-path']) : undefined, parsed['--mode']);
      // Supervisor owns Chrome's process tree even if a synchronous WASM call or
      // Playwright teardown wedges. This is not a browser Worker implementation.
      else await runCommand(process.execPath, [fileURLToPath(import.meta.url), '--internal-child', ...options], { timeout: 180_000, mirror: true });
    }
  } catch (error) { console.error(error); process.exitCode = 1; }
}
