import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bounded, launchBrowser } from './browser-support.mjs';
import { startProductServer } from './wasm-browser.mjs';
import { repositoryRoot, runCommand, sha256, snapshotProducts, verifyProducts } from './wasm-products.mjs';
import { workerCase, workerCases } from './wasm-worker-cases.mjs';
import { workerFixture } from './wasm-worker-fixtures.mjs';

const usage = 'Usage: node site/scripts/wasm-worker.mjs [--executable-path PATH] [--case NAME]';
const inputs = ['site/src/playground/client.js', 'site/src/playground/worker.js', 'site/src/playground/protocol.js',
  'site/scripts/wasm-worker.mjs', 'site/scripts/wasm-worker.test.mjs', 'site/scripts/wasm-worker-cases.mjs', 'site/scripts/wasm-worker-fixtures.mjs'];

export async function runWorkerTests({ executablePath, selectedCase } = {}) {
  const directory = join(repositoryRoot, 'target/wasm-browser/reports', `worker-${new Date().toISOString().replaceAll(/[:.]/gu, '-')}-${process.pid}`);
  await mkdir(directory, { recursive: true });
  const reportPath = join(directory, 'report.json');
  const started = Date.now();
  const report = { startedAt: new Date().toISOString(), node: process.version, platform: process.platform,
    selectedCases: workerCases.filter((name) => !selectedCase || name === selectedCase), launchAttempts: [], checks: [],
    limits: ['Local Chrome/Chromium only; full backend/platform CI and WB05 UI acceptance remain separate.',
      'Waiting/non-yielding compile/fatal faults use explicitly labelled test-only wrappers around the actual adapter.',
      'No external async providers, arbitrary script JS callbacks, Worker RPC or WASM threads.',
      'No exact browser GC/loan counters or panic-unwind claims; SDK owns normal cleanup.'] };
  const save = async () => { report.elapsedMs = Date.now() - started; await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`); };
  async function check(name, action) {
    const item = { name, status: 'running' };
    report.checks.push(item);
    await save();
    const start = Date.now();
    try { item.evidence = await action(item); item.status = 'passed'; console.log(`PASS ${name}`); }
    catch (error) { item.status = 'failed'; item.error = error.stack ?? String(error); console.error(`FAIL ${name}: ${error.message}`); }
    item.elapsedMs = Date.now() - start;
    await save();
  }
  let server;
  let browser;
  let snapshot;
  try {
    if (selectedCase) assert(workerCases.includes(selectedCase), `Unknown case: ${selectedCase}`);
    snapshot = await snapshotProducts();
    report.products = snapshot.record;
    report.inputs = {};
    for (const path of inputs) {
      const bytes = await readFile(join(repositoryRoot, path));
      report.inputs[path] = sha256(bytes);
      if (path.startsWith('site/src/playground/')) snapshot.files.set(`playground/${path.split('/').at(-1)}`, bytes);
    }
    for (const kind of ['compile-block', 'fatal-drive', 'waiting', 'malformed-response', 'stale-replies']) snapshot.files.set(`fixtures/${kind}.js`, Buffer.from(workerFixture(kind)));
    await check('pure-node-protocol-client-contracts', async () => {
      const result = await runCommand(process.execPath, ['--test', 'site/scripts/wasm-worker.test.mjs'], { timeout: 20_000 });
      await writeFile(join(directory, 'node-tests.txt'), result.stdout + result.stderr);
      return result.stdout;
    });
    server = await startProductServer(snapshot);
    report.origin = server.origin;
    const launched = await bounded(() => launchBrowser(executablePath, report.launchAttempts), 45_000, 'Browser launch');
    browser = launched.browser;
    report.browser = { version: browser.version(), executablePath: launched.executablePath };
    console.log(`Browser: ${report.browser.version} (${report.browser.executablePath})`);
    for (const name of report.selectedCases) {
      assert(Date.now() - started < 150_000, '150s admission deadline reached; remaining cases unrun');
      await check(name, async (item) => {
        const context = await browser.newContext({ serviceWorkers: 'block' });
        const errors = [];
        item.network = [];
        context.setDefaultTimeout(5_000);
        context.setDefaultNavigationTimeout(8_000);
        await context.route('**/*', async (route) => {
          if (new URL(route.request().url()).origin !== server.origin) {
            errors.push(`External request: ${route.request().url()}`);
            await route.abort();
          } else await route.continue();
        });
        context.on('request', (request) => {
          if (item.network.length < 64) item.network.push(new URL(request.url()).pathname);
          if (new URL(request.url()).origin !== server.origin) errors.push('Unexpected request origin');
        });
        const page = await context.newPage();
        page.on('pageerror', (error) => { if (errors.length < 16) errors.push(error.message); });
        page.on('console', (message) => { if (message.type() === 'error' && errors.length < 16) errors.push(message.text().slice(0, 2048)); });
        try {
          assert.equal((await page.goto(`${server.origin}/kagari/wasm/`)).status(), 200);
          const result = await bounded(() => page.evaluate(workerCase, name), 25_000, name);
          assert.deepEqual(errors, [], 'Browser/network errors');
          return result;
        } finally {
          try { item.trace = await bounded(() => page.evaluate(() => window.wb04Trace), 1_000, 'Trace capture'); }
          catch (error) { item.traceError = error.message; }
          item.browserErrors = errors;
          await bounded(() => context.close(), 5_000, 'Case teardown');
        }
      });
    }
  } catch (error) { report.fatal = error.stack ?? String(error); console.error(error); }
  finally {
    if (browser) await check('browser-teardown', () => bounded(() => browser.close(), 5_000, 'Browser teardown'));
    if (server) await check('server-teardown', () => server.close());
    if (snapshot) await check('products-and-inputs-unchanged', async () => {
      assert.deepEqual(await verifyProducts(), snapshot.record, 'Product provenance changed during tests');
      for (const [path, hash] of Object.entries(report.inputs ?? {})) assert.equal(sha256(await readFile(join(repositoryRoot, path))), hash, `Input changed: ${path}`);
      return { verified: true };
    });
    report.success = !report.fatal && report.checks.every((item) => item.status === 'passed')
      && report.selectedCases.every((name) => report.checks.some((item) => item.name === name && item.status === 'passed'));
    await save();
    console.log(`WB04 report: ${reportPath}`);
  }
  return report.success;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    if (args.length === 1 && args[0] === '--help') console.log(`${usage}\nRequires refreshed verified WASM products. No installs/builds.\nNode contracts plus actual browser cases, each 25s; total child tree bounded to 180s.\nReports: target/wasm-browser/reports/worker-*/report.json\nCases: ${workerCases.join(', ')}`);
    else {
      const child = args[0] === '--internal-child';
      const options = child ? args.slice(1) : args;
      assert.equal(options.length % 2, 0, usage);
      const parsed = {};
      for (let index = 0; index < options.length; index += 2) {
        assert(['--executable-path', '--case'].includes(options[index]) && options[index + 1] && !Object.hasOwn(parsed, options[index]), usage);
        parsed[options[index]] = options[index + 1];
      }
      if (child) {
        if (!await runWorkerTests({ executablePath: parsed['--executable-path'] ? resolve(parsed['--executable-path']) : undefined, selectedCase: parsed['--case'] })) process.exitCode = 1;
      } else await runCommand(process.execPath, [fileURLToPath(import.meta.url), '--internal-child', ...options], { timeout: 180_000, mirror: true });
    }
  } catch (error) { console.error(error); process.exitCode = 1; }
}
