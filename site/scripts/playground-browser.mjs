import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { cpus, release, totalmem, type } from 'node:os';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { brotliCompress, constants, gzip } from 'node:zlib';
import { base, page as route } from '../src/links.mjs';
import { actionTimeout, bounded, launchBrowser, startServer } from './browser-support.mjs';
import { verifyBrowserAssets } from './build-provenance.mjs';
import { distRoot, inputDigest, manifestName, outputDigest, walk } from './files.mjs';
import { productsRoot, repositoryRoot, runCommand, sha256 } from './wasm-products.mjs';

// Acceptance only: never rebuild dist/products, install tools, or download a browser.
// The supervising process bounds and kills the entire child tree, including Chrome.
const usage = 'Usage: node site/scripts/playground-browser.mjs [--executable-path PATH] [--samples 1..5]';
const playground = '[data-playground]';
const sourceSelector = `${playground} [data-source]`;
const simpleSource = 'fn main() -> i32 { 42 }';
const loopSource = 'use web::print; fn main() { print("before stop 😀"); loop {} }';
const measurementSource = 'fn main() -> i32 { var sum = 0; for i in 0..1000 { sum += i; } sum }';
const compressGzip = promisify(gzip);
const compressBrotli = promisify(brotliCompress);
const button = (page, action) => page.locator(`${playground} [data-action="${action}"]`);
const text = (page, name) => page.locator(`${playground} [data-${name}]`).textContent();

async function idle(page) {
  await page.waitForFunction((selector) => document.querySelector(selector)?.dataset.state === 'idle', playground);
  assert(await button(page, 'run').isEnabled(), 'Idle Run must be enabled');
  assert(await button(page, 'cancel').isDisabled(), 'Idle Cancel must be disabled');
  assert(await button(page, 'reset').isEnabled(), 'Idle Reset must be enabled');
}

async function openPlayground(page) {
  assert.equal((await page.goto(route('playground'), { waitUntil: 'networkidle' }))?.status(), 200);
  assert.equal(new URL(page.url()).pathname, '/kagari/playground/');
  await page.waitForFunction((selector) => document.querySelector(selector)?.dataset.mounted === 'true', playground);
  await idle(page);
}

async function run(page, source, activation = 'click') {
  await page.locator(sourceSelector).fill(source);
  if (activation === 'shortcut') await page.locator(sourceSelector).press('Control+Enter');
  else if (activation === 'tap') await button(page, 'run').tap();
  else await button(page, 'run').click();
  await idle(page);
}

async function success(page, value = { tag: 'i32', value: '42' }) {
  assert.equal(await text(page, 'status'), 'Run complete.');
  assert.deepEqual(JSON.parse(await text(page, 'result')), value);
  assert.equal(await page.locator(`${playground} [data-diagnostics] > li`).count(), 0);
}

async function liveLoop(page, activation = 'click') {
  await page.locator(sourceSelector).fill(loopSource);
  await button(page, 'run')[activation]();
  await page.waitForFunction((selector) => document.querySelector(`${selector} [data-output]`)?.textContent === 'before stop 😀\n', playground);
  assert.equal(await page.locator(playground).getAttribute('data-state'), 'running');
  assert(await button(page, 'run').isDisabled());
  for (const action of ['cancel', 'hard-stop', 'reset']) assert(await button(page, action).isEnabled(), `Live ${action} disabled`);
}

async function assertReset(page, source) {
  await idle(page);
  assert.match(await text(page, 'status'), /Reset complete/u);
  assert.equal(await page.locator(sourceSelector).inputValue(), source, 'Reset must retain source');
  assert.equal(await text(page, 'output'), '');
  assert.equal(await text(page, 'result'), 'No result yet.');
  assert.equal(await page.locator(`${playground} [data-diagnostics] > li`).count(), 0);
  assert(await page.locator(`${playground} [data-output-truncated]`).isHidden());
}

async function sizeEvidence(record) {
  const modes = {};
  for (const mode of ['source', 'artifact']) {
    const files = [];
    for (const name of ['kagari_web.js', 'kagari_web_bg.wasm']) {
      const path = `${mode}/${name}`;
      const bytes = await readFile(join(productsRoot, path));
      assert.deepEqual({ sha256: sha256(bytes), bytes: bytes.length }, record.products[path], `Size input differs from verified product: ${path}`);
      const [gz, br] = await Promise.all([
        compressGzip(bytes, { level: 9 }),
        compressBrotli(bytes, { params: { [constants.BROTLI_PARAM_QUALITY]: 5 } }),
      ]);
      files.push({ path, sha256: sha256(bytes), rawBytes: bytes.length, gzipBytes: gz.length, brotliBytes: br.length });
    }
    modes[mode] = { files, total: Object.fromEntries(['rawBytes', 'gzipBytes', 'brotliBytes'].map((key) => [key, files.reduce((sum, file) => sum + file[key], 0)])) };
  }
  return { modes, compression: 'Node zlib gzip level 9 / Brotli quality 5; each JS/WASM file compressed separately; totals are sums. Not HTTP transfer measurements. Declarations and fixture excluded. Only source mode is shipped by the site.' };
}

// Runs in a separate fresh browser context, after UI cases, against the same
// deployed source adapter. These are adapter-boundary timings, not UI latency.
async function measureAdapter(source) {
  const check = (condition, label) => { if (!condition) throw new Error(label); };
  const started = performance.now();
  const adapter = await import('/kagari/wasm/source/kagari_web.js');
  await adapter.default();
  const initializationMs = performance.now() - started;
  const constructStarted = performance.now();
  const session = new adapter.BrowserSession();
  const sessionConstructionMs = performance.now() - constructStarted;
  try {
    const compileStarted = performance.now();
    const compiledText = session.compile(source);
    const compilationMs = performance.now() - compileStarted;
    const compiled = JSON.parse(compiledText);
    check(compiled.status === 'ready', `Compile failed: ${compiledText}`);
    const executeStarted = performance.now();
    let response = JSON.parse(session.start());
    check(response.status === 'runnable', `Start failed: ${JSON.stringify(response)}`);
    let slices = 0;
    while (response.status === 'runnable' && slices < 1000) {
      response = JSON.parse(session.drive(1000));
      slices += 1;
      check(response.output.length === 0, 'Measurement workload must not print');
    }
    const executionMs = performance.now() - executeStarted;
    check(response.status === 'complete', `Execution failed: ${JSON.stringify(response)}`);
    check(response.result?.tag === 'i32' && response.result.value === '499500', 'Wrong measured result');
    return { initializationMs, sessionConstructionMs, compilationMs, executionMs, slices, result: response.result };
  } finally { session.free(); }
}

async function main({ executablePath, samples }) {
  const started = Date.now();
  const directory = join(repositoryRoot, 'target/site-playground-browser', `${new Date().toISOString().replaceAll(/[:.]/gu, '-')}-${process.pid}`);
  await mkdir(directory, { recursive: true });
  const reportPath = join(directory, 'report.json');
  const cpu = cpus();
  const report = {
    startedAt: new Date(started).toISOString(), status: 'running', base,
    machine: { os: type(), release: release(), platform: process.platform, arch: process.arch, cpu: cpu[0]?.model, logicalCpus: cpu.length, memoryBytes: totalmem(), node: process.version, zlib: process.versions.zlib },
    launchAttempts: [], checks: [],
    measurementMethod: {
      source: measurementSource, expectedResult: { tag: 'i32', value: '499500' }, sampleCount: samples,
      initialization: 'performance.now around dynamic source-adapter import plus await default(): local JS/WASM fetch, module evaluation and WASM initialization; excludes BrowserSession construction (reported separately).',
      compilation: 'Synchronous BrowserSession.compile(source) including adapter serialization; response JSON decoding excluded.',
      execution: 'BrowserSession.start plus drive(1000) until complete, including JS bridge/JSON decoding and slice loop; excludes compilation, UI, Worker messaging and scheduling delays.',
      cache: 'Fresh context and BrowserSession per sample; routing disables HTTP cache and server sends no-store. Same browser process after UI tests; OS filesystem and browser/WASM code caches uncontrolled, not claimed cold. No discarded warmup samples.',
      parallelism: 'Sequential browser cases/samples. Product Cargo recipe uses default parallelism unless provenance environment overrides it (recorded below).',
      limitations: 'Small explicit workload, observational timings only; no speedup, release-profile, cross-browser or benchmark claims.',
    },
    limits: ['Local exact dist at /kagari/ only; no public deployment acceptance.', 'Chromium headless desktop 1280x900 / mobile emulation 390x844; not a full accessibility or physical-device audit.', 'Real production UI/Worker/adapter without test hooks. No synthetic non-yielding compilation or fatal fault injection (owned by WB04).', 'External HTTP and all WebSocket requests blocked. No browser downloads, rebuilds, or Rust commands.'],
  };
  const save = async () => {
    report.elapsedMs = Date.now() - started;
    await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  };
  async function check(name, operation) {
    assert(Date.now() - started < 240_000, '240s admission deadline reached; remaining checks not run');
    const item = { name, status: 'running' };
    report.checks.push(item);
    await save();
    try {
      item.evidence = await operation(item);
      item.status = 'passed';
      console.log(`PASS ${name}`);
    } catch (error) {
      item.status = 'failed';
      item.error = error.stack ?? String(error);
      console.error(`FAIL ${name}: ${error.message}`);
    }
    await save();
  }
  let browser;
  let server;
  let manifest;
  let allowedPaths;
  async function browserCheck(name, operation, mobile = false) {
    await check(name, async (item) => {
      const context = await browser.newContext({ baseURL: server.origin, viewport: mobile ? { width: 390, height: 844 } : { width: 1280, height: 900 }, isMobile: mobile, hasTouch: mobile, reducedMotion: 'reduce', serviceWorkers: 'block' });
      context.setDefaultTimeout(actionTimeout);
      context.setDefaultNavigationTimeout(12_000);
      const errors = [];
      const error = (message) => { if (errors.length < 32) errors.push(message); };
      item.network = [];
      item.workers = [];
      let requests = 0;
      let page;
      try {
        await context.route('**/*', async (requestRoute) => {
          const request = requestRoute.request();
          const url = new URL(request.url());
          requests += 1;
          if (requests > 512 || url.origin !== server.origin || !allowedPaths.has(url.pathname) || !['GET', 'HEAD'].includes(request.method()) || url.search) {
            error(`Unexpected request: ${request.method()} ${url.href}`);
            await requestRoute.abort('blockedbyclient');
          } else await requestRoute.continue();
        });
        await context.routeWebSocket('**/*', (socket) => { error(`Unexpected WebSocket: ${socket.url()}`); socket.close(); });
        context.on('request', (request) => {
          if (item.network.length < 512) item.network.push({ method: request.method(), url: request.url(), resourceType: request.resourceType() });
          const url = new URL(request.url());
          if (url.origin !== server.origin || !allowedPaths.has(url.pathname)) error(`Request outside checked inventory: ${url.href}`);
        });
        context.on('response', (response) => { if (response.status() >= 400) error(`HTTP ${response.status()}: ${response.url()}`); });
        context.on('requestfailed', (request) => { error(`Request failed: ${request.url()}: ${request.failure()?.errorText}`); });
        context.on('console', (message) => { if (message.type() === 'error') error(message.text().slice(0, 2048)); });
        page = await context.newPage();
        page.on('pageerror', (failure) => error(failure.message));
        page.on('worker', (worker) => { item.workers.push(worker.url()); });
        const evidence = await bounded(() => operation(page, item), 45_000, name);
        assert.deepEqual(errors, [], 'Unexpected browser/network errors');
        return evidence;
      } catch (failure) {
        if (page) {
          try {
            await page.screenshot({ path: join(directory, `${name}.png`), timeout: 2_000 });
            item.screenshot = `${name}.png`;
          } catch (captureError) { item.screenshotError = captureError.message; }
        }
        throw failure;
      } finally {
        item.browserErrors = errors;
        await bounded(() => context.close(), 5_000, `${name} context teardown`);
      }
    });
  }
  try {
    await save();
    assert.equal(base, '/kagari', 'Acceptance requires the project base /kagari/');
    manifest = JSON.parse(await readFile(join(distRoot, manifestName), 'utf8'));
    assert.equal(await inputDigest(), manifest.inputs, 'Stale site inputs; finish edits, build and check before running.');
    assert.equal(await outputDigest(), manifest.output, 'Changed dist; build and check before running.');
    await verifyBrowserAssets(manifest.wasm);
    report.buildRecord = manifest;
    report.productBuild = { builtAt: manifest.wasm.builtAt, recipe: manifest.wasm.recipe, tools: manifest.wasm.tools,
      buildCache: 'Existing verified products reused, not rebuilt here. Original compiler/incremental cache state is not recorded by the product record.',
      parallelism: manifest.wasm.tools.environment.CARGO_BUILD_JOBS ?? 'Cargo default (no jobs override recorded)' };
    allowedPaths = new Set();
    for (const path of await walk(distRoot, new Set([manifestName]))) {
      const name = relative(distRoot, path).replaceAll('\\', '/');
      allowedPaths.add(`${base}/${name}`);
      if (name.endsWith('index.html')) allowedPaths.add(`${base}/${name.slice(0, -'index.html'.length)}`);
    }
    server = await startServer();
    // Confirm the immutable preview snapshot was captured while the output stayed current.
    assert.equal(await outputDigest(), manifest.output, 'Output changed during server snapshot');
    report.origin = server.origin;
    await check('verified-product-sizes', async () => bounded(() => sizeEvidence(manifest.wasm), 45_000, 'Product compression'));
    const launched = await bounded(() => launchBrowser(executablePath, report.launchAttempts), 45_000, 'Browser launch');
    browser = launched.browser;
    report.browser = { executablePath: launched.executablePath, version: browser.version() };
    await save();

    await browserCheck('lazy-guide-and-playground', async (page, item) => {
      assert.equal((await page.goto(route('start/quickstart'), { waitUntil: 'networkidle' }))?.status(), 200);
      assert.equal(await page.locator(playground).count(), 0);
      const guideRequests = item.network.length;
      await openPlayground(page);
      assert(await button(page, 'hard-stop').isDisabled());
      await button(page, 'reset').click();
      assert.match(await text(page, 'status'), /compiler has not been started/u);
      assert.equal(item.workers.length, 0, 'No Worker before Run');
      const runtimeRequests = item.network.filter(({ url }) => /\/wasm\/|\.wasm(?:\?|$)|\/(?:client|worker|protocol)\.js(?:\?|$)/u.test(new URL(url).pathname));
      assert.deepEqual(runtimeRequests, [], 'Guide/initial Playground must not fetch compiler, client or Worker');
      return { guideRequests, totalRequests: item.network.length, runtimeRequests, workersBeforeRun: item.workers.length };
    });

    await browserCheck('source-text-diagnostics-keyboard', async (page, item) => {
      await openPlayground(page);
      const editor = page.locator(sourceSelector);
      await editor.focus();
      await page.keyboard.press('Tab');
      assert(await button(page, 'run').evaluate((element) => document.activeElement === element), 'Tab must leave editor for Run');
      await page.keyboard.press('Enter');
      await idle(page);
      await success(page);
      const printed = '<img src="https://example.invalid/probe" onerror="globalThis.playgroundInjected=true"> & 😀';
      await run(page, `use web::print; fn main() -> u64 { print(${JSON.stringify(printed)}); 18446744073709551615u64 }`, 'shortcut');
      await success(page, { tag: 'u64', value: '18446744073709551615' });
      assert.equal(await text(page, 'output'), `${printed}\n`);
      assert.equal(await page.locator(`${playground} [data-output] *`).count(), 0, 'Print must create text, not HTML');
      assert.equal(await page.evaluate(() => globalThis.playgroundInjected), undefined);
      for (const value of ['41', '43']) {
        await run(page, `fn main() -> i32 { ${value} }`);
        await success(page, { tag: 'i32', value });
        assert.equal(await text(page, 'output'), '', 'Repeated runs must clear previous output');
      }
      const pasted = '// 😀\r\nfn main() -> i32 { missing }';
      await run(page, pasted);
      assert.equal(await editor.inputValue(), pasted.replaceAll('\r\n', '\n'));
      assert.match(await text(page, 'status'), /Operation failed/u);
      const location = page.locator(`${playground} [data-diagnostic-location][data-start="25"][data-end="32"]`).first();
      await location.click();
      const selection = await editor.evaluate((element) => ({ start: element.selectionStart, end: element.selectionEnd, selected: element.value.slice(element.selectionStart, element.selectionEnd), focused: document.activeElement === element }));
      assert.deepEqual(selection, { start: 25, end: 32, selected: 'missing', focused: true });
      await editor.fill(simpleSource);
      assert(await location.isDisabled(), 'Stale diagnostic location must be disabled');
      await page.locator(`${playground} [data-diagnostics-stale]`).waitFor({ state: 'visible' });
      await button(page, 'run').click();
      await idle(page);
      await success(page);
      for (const path of ['playground/client.js', 'playground/worker.js', 'source/kagari_web.js', 'source/kagari_web_bg.wasm']) {
        assert(item.network.some(({ url }) => new URL(url).pathname === `${base}/wasm/${path}`), `Run did not load ${path}`);
      }
      assert.equal(item.workers.length, 1, 'Repeated runs reuse the healthy Worker');
      return { selection, maximumU64: '18446744073709551615', textSafePrint: printed, workers: item.workers.length };
    });

    await browserCheck('cancel-hard-stop-restart-reset', async (page, item) => {
      await openPlayground(page);
      await liveLoop(page);
      const generation = await page.locator(playground).getAttribute('data-worker-generation');
      await page.locator(sourceSelector).focus();
      for (const action of ['cancel', 'hard-stop', 'reset']) {
        await page.keyboard.press('Tab');
        assert(await button(page, action).evaluate((element) => document.activeElement === element), `Keyboard cannot reach ${action}`);
      }
      await button(page, 'cancel').press('Space');
      await idle(page);
      assert.match(await text(page, 'status'), /Cancelled with normal cleanup/u);
      assert.match(await text(page, 'result'), /KG_RUNTIME_CANCELLED/u);
      assert.equal(await text(page, 'output'), 'before stop 😀\n');
      await run(page, simpleSource);
      await success(page);
      assert.equal(await page.locator(playground).getAttribute('data-worker-generation'), generation);
      assert.equal(item.workers.length, 1, 'Cancel must reuse healthy Worker');
      await liveLoop(page);
      await button(page, 'hard-stop').press('Enter');
      await idle(page);
      assert.match(await text(page, 'status'), /Hard stopped/u);
      assert.equal(await text(page, 'output'), 'before stop 😀\n');
      await run(page, simpleSource);
      await success(page);
      assert.notEqual(await page.locator(playground).getAttribute('data-worker-generation'), generation);
      assert.equal(item.workers.length, 2, 'Hard stop must create a fresh Worker on next Run');
      await liveLoop(page);
      await button(page, 'reset').press('Enter');
      await assertReset(page, loopSource);
      await run(page, simpleSource);
      await success(page);
      await button(page, 'reset').click();
      await assertReset(page, simpleSource);
      assert.equal(item.workers.length, 2, 'Reset must not destroy healthy Worker');
      return { workers: item.workers.length, initialGeneration: generation, finalGeneration: await page.locator(playground).getAttribute('data-worker-generation'), cancelledEffects: 'before stop 😀\n', activeAndIdleReset: true };
    });

    await browserCheck('bounded-output-visible-truncation', async (page) => {
      await openPlayground(page);
      await run(page, 'use web::print; fn main() -> i32 { for i in 0..300 { print("x"); } 42 }');
      await success(page);
      assert.equal(await text(page, 'output'), 'x\n'.repeat(256));
      await page.locator(`${playground} [data-output-truncated]`).waitFor({ state: 'visible' });
      await run(page, `use web::print; fn main() -> i32 { for i in 0..20 { print("${'😀'.repeat(1025)}"); } 42 }`);
      await success(page);
      assert.equal(await text(page, 'output'), `${'😀'.repeat(1024)}\n`.repeat(16));
      await page.locator(`${playground} [data-output-truncated]`).waitFor({ state: 'visible' });
      await run(page, simpleSource);
      await success(page);
      assert.equal(await text(page, 'output'), '');
      assert(await page.locator(`${playground} [data-output-truncated]`).isHidden());
      return { messages: 256, messageUtf8Bytes: 4096, aggregateUtf8Bytes: 65536, quotaResetsBetweenRuns: true };
    });

    await browserCheck('mobile-layout-and-controls', async (page) => {
      await openPlayground(page);
      const layout = await page.locator(playground).evaluate((root) => {
        const box = (element) => { const { x, width, height } = element.getBoundingClientRect(); return { x, width, height }; };
        return { viewport: innerWidth, documentWidth: document.documentElement.scrollWidth, editor: box(root.querySelector('[data-source]')), buttons: [...root.querySelectorAll('[data-action]')].map((element) => ({ action: element.dataset.action, ...box(element) })) };
      });
      assert(layout.documentWidth <= layout.viewport + 1, 'Mobile page overflows horizontally');
      for (const box of [layout.editor, ...layout.buttons]) assert(box.x >= 0 && box.x + box.width <= layout.viewport + 1, 'Mobile control is clipped');
      for (const box of layout.buttons) assert(box.height >= 44, `${box.action} touch target is too short`);
      await run(page, simpleSource, 'tap');
      await success(page);
      await liveLoop(page, 'tap');
      await button(page, 'cancel').tap();
      await idle(page);
      assert.match(await text(page, 'result'), /KG_RUNTIME_CANCELLED/u);
      await liveLoop(page, 'tap');
      await button(page, 'hard-stop').tap();
      await idle(page);
      assert.match(await text(page, 'status'), /Hard stopped/u);
      await run(page, simpleSource, 'tap');
      await success(page);
      await button(page, 'reset').tap();
      await assertReset(page, simpleSource);
      return layout;
    }, true);

    for (let sample = 1; sample <= samples; sample += 1) {
      await browserCheck(`adapter-measurement-${sample}`, async (page, item) => {
        assert.equal((await page.goto(route('start/quickstart'), { waitUntil: 'networkidle' }))?.status(), 200);
        const result = await page.evaluate(measureAdapter, measurementSource);
        assert.equal(item.workers.length, 0, 'Measurements must not include UI Worker orchestration');
        return { sample, ...result };
      });
    }
    await check('inputs-output-and-provenance-unchanged', async () => {
      assert.equal(await inputDigest(), manifest.inputs, 'Inputs changed during acceptance; rebuild/check and rerun');
      assert.equal(await outputDigest(), manifest.output, 'Output changed during acceptance');
      assert.deepEqual(JSON.parse(await readFile(join(distRoot, manifestName), 'utf8')), manifest, 'Build record changed');
      await verifyBrowserAssets(manifest.wasm);
      return { inputs: manifest.inputs, output: manifest.output };
    });
  } catch (error) {
    report.error = error.stack ?? String(error);
    console.error(error);
  } finally {
    for (const [name, cleanup] of [['browser', () => browser?.close()], ['server', () => server?.close()]]) {
      try { await bounded(cleanup, 8_000, `${name} teardown`); }
      catch (error) { (report.teardownErrors ??= []).push(`${name}: ${error.message}`); }
    }
    const expectedChecks = 7 + samples;
    report.status = !report.error && !report.teardownErrors && report.checks.length === expectedChecks && report.checks.every((item) => item.status === 'passed') ? 'passed' : 'failed';
    await save();
    console.log(`Playground browser acceptance ${report.status}: ${reportPath}`);
  }
  return report.status === 'passed';
}

try {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--help') {
    console.log(`${usage}\nRequires a current successful site build and check; serves exact dist at /kagari/.\nUses an existing Chrome/Edge/Chromium only; no downloads or rebuilds.\nDefault: 3 separate fresh-context timing samples (1..5 allowed).\nUI/measurement cases bounded to 45s; full child tree bounded to 300s.\nReports/screenshots: target/site-playground-browser/<timestamp>-<pid>/.`);
  } else {
    const child = args[0] === '--internal-child';
    const options = child ? args.slice(1) : args;
    assert.equal(options.length % 2, 0, usage);
    const parsed = {};
    for (let index = 0; index < options.length; index += 2) {
      assert(['--executable-path', '--samples'].includes(options[index]) && options[index + 1] && !Object.hasOwn(parsed, options[index]), usage);
      parsed[options[index]] = options[index + 1];
    }
    assert(!parsed['--samples'] || /^[1-5]$/u.test(parsed['--samples']), usage);
    // Resolve relative browser paths before the supervisor changes cwd to the repository.
    const executablePath = parsed['--executable-path'] ? resolve(parsed['--executable-path']) : undefined;
    const samples = Number(parsed['--samples'] ?? 3);
    if (child) {
      if (!await main({ executablePath, samples })) process.exitCode = 1;
    } else {
      await runCommand(process.execPath, [fileURLToPath(import.meta.url), '--internal-child', '--samples', String(samples), ...(executablePath ? ['--executable-path', executablePath] : [])], { timeout: 300_000, mirror: true });
    }
  }
} catch (error) { console.error(error); process.exitCode = 1; }
