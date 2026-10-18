import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { request } from 'node:http';
import { join, relative, resolve } from 'node:path';
import { base, page as route } from '../src/links.mjs';
import { pages } from '../src/navigation.mjs';
import { repositoryRoot } from '../src/source-example.mjs';
import { distRoot, inputDigest, manifestName, outputDigest } from './files.mjs';
import { actionTimeout, bounded, launchBrowser, startServer } from './browser-support.mjs';

// Run build + check first. This runner never rebuilds, downloads browsers, or writes dist.
// Independent browser diagnostics are retained on stale inputs, but acceptance still fails.
const args = process.argv.slice(2);
const usage = 'Usage: node scripts/browser.mjs [--executable-path PATH]';
if (args.length === 1 && args[0] === '--help') {
  console.log(`${usage}\nUses managed Chromium or an installed Chrome/Edge/Chromium. Evidence: ../target/site-browser/.\nRequires a successful, current npm run build and npm run check.`);
} else {
  assert(args.length === 0 || (args.length === 2 && args[0] === '--executable-path' && args[1]), usage);
  await main(args.length ? resolve(args[1]) : undefined);
}

async function main(explicitPath) {
  const started = Date.now();
  const directory = join(repositoryRoot, 'target', 'site-browser', `${new Date().toISOString().replaceAll(/[:.]/gu, '-')}-${process.pid}`);
  await mkdir(directory, { recursive: true });
  const reportPath = join(directory, 'report.json');
  const report = {
    startedAt: new Date(started).toISOString(),
    platform: process.platform,
    node: process.version,
    base,
    launchAttempts: [],
    checks: [],
    screenshots: [],
    limits: [
      'Local static output only; no public deployment or cross-browser/platform acceptance.',
      'Chromium-based headless browser; desktop 1280x900 and mobile 390x844.',
      'Keyboard smoke checks, not a full accessibility audit or visual-regression comparison.',
      'External network requests are blocked; no external link validation.',
    ],
  };
  let browser;
  let server;
  let stopped = false;
  // Last-resort process bound also covers a wedged browser launch/teardown.
  const watchdog = setTimeout(() => {
    console.error(`Browser runner exceeded 180s; forced exit. Partial evidence: ${reportPath}`);
    process.exit(1);
  }, 180_000);
  async function saveReport() {
    report.elapsedMs = Date.now() - started;
    await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  }
  async function check(name, operation) {
    if (stopped) return;
    const result = { name, status: 'running' };
    report.checks.push(result);
    const start = Date.now();
    try {
      await operation(result);
      result.status = 'passed';
      console.log(`PASS ${name}`);
    } catch (error) {
      result.status = 'failed';
      result.error = error.stack ?? String(error);
      console.error(`FAIL ${name}: ${error.message}`);
    }
    result.elapsedMs = Date.now() - start;
    await saveReport();
  }
  async function screenshot(page, name, fullPage = false) {
    await page.screenshot({ path: join(directory, name), fullPage, timeout: actionTimeout, animations: 'disabled' });
    report.screenshots.push(relative(repositoryRoot, join(directory, name)).replaceAll('\\', '/'));
  }
  async function browserCheck(name, operation, mobile = false) {
    await check(name, async (result) => {
      const context = await browser.newContext({
        baseURL: server.origin,
        viewport: mobile ? { width: 390, height: 844 } : { width: 1280, height: 900 },
        isMobile: mobile,
        hasTouch: mobile,
        colorScheme: 'light',
        reducedMotion: 'reduce',
        serviceWorkers: 'block',
      });
      context.setDefaultTimeout(actionTimeout);
      context.setDefaultNavigationTimeout(12_000);
      const errors = [];
      result.pagefindResponses = [];
      result.consoleErrors = [];
      await context.route('**/*', async (requestRoute) => {
        const url = new URL(requestRoute.request().url());
        if (url.origin !== server.origin) {
          errors.push(`External request: ${url.href}`);
          await requestRoute.abort('blockedbyclient');
        } else {
          if (!url.pathname.startsWith(`${base}/`)) errors.push(`Request escaped project base: ${url.href}`);
          await requestRoute.continue();
        }
      });
      const page = await context.newPage();
      page.on('pageerror', (error) => errors.push(`Uncaught browser error: ${error.message}`));
      page.on('console', (message) => { if (message.type() === 'error') result.consoleErrors.push(message.text()); });
      page.on('response', (response) => {
        const url = new URL(response.url());
        if (url.pathname.startsWith(`${base}/pagefind/`)) result.pagefindResponses.push({ path: url.pathname, status: response.status() });
        if (response.status() >= 400 && ![route('browser-acceptance-missing'), `${base}/404.html`].includes(url.pathname)) errors.push(`HTTP ${response.status()}: ${url.href}`);
      });
      page.on('requestfailed', (request) => {
        // A result click can legitimately cancel in-flight search requests.
        if (request.failure()?.errorText !== 'net::ERR_ABORTED') errors.push(`${request.failure()?.errorText}: ${request.url()}`);
      });
      try {
        await bounded(() => operation({ page, context, result }), 30_000, name);
        assert.deepEqual(errors, [], errors.join('\n'));
      } catch (error) {
        try { await screenshot(page, `failure-${name}.png`); }
        catch (captureError) { result.screenshotError = captureError.message; }
        throw error;
      } finally {
        result.browserErrors = errors;
        await bounded(() => context.close(), 5_000, 'Browser context teardown');
      }
    });
  }
  async function visit(page, path, status = 200) {
    const response = await page.goto(route(path), { waitUntil: 'networkidle' });
    assert.equal(response?.status(), status, `Unexpected navigation status for ${route(path)}`);
    assert.equal(new URL(page.url()).pathname, route(path));
    const entry = pages.find((candidate) => candidate.path === path);
    if (entry) {
      assert.equal((await page.locator('h1').innerText()).trim(), entry.title);
      assert((await page.title()).includes(entry.title), 'Document title differs from route title');
    }
    await page.locator('main').waitFor({ state: 'visible' });
  }
  try {
    await bounded(async () => {
      let manifest;
      try { manifest = JSON.parse(await readFile(join(distRoot, manifestName), 'utf8')); }
      catch { throw new Error('Missing successful build record. Run npm run build, then npm run check.'); }
      assert.equal(manifest.output, await outputDigest(), 'Build output changed; run npm run build, then npm run check.');
      report.buildRecord = manifest;
      const initialInputs = await inputDigest();
      report.currentInputs = initialInputs;
      await check('build-freshness', async () => {
        assert(initialInputs === manifest.inputs, 'Stale build inputs; run npm run build, then npm run check before accepting browser results.');
      });
      server = await startServer();
      report.origin = server.origin;
      const launched = await launchBrowser(explicitPath, report.launchAttempts);
      browser = launched.browser;
      report.browser = { executablePath: launched.executablePath, version: browser.version() };
      console.log(`Browser: ${report.browser.executablePath} (${report.browser.version})`);
      await saveReport();

      await check('server-boundary', async () => {
        // Raw HTTP preserves dot segments that fetch/URL would normalize before sending.
        for (const [path, expected] of [
          [route(), 200], [base, 308], [`${base}/host/native`, 308],
          [route('browser-acceptance-missing'), 404], [`${base}/404.html`, 404],
          ['/', 404], ['/index.html', 404], [`${base}/.checked-build.json`, 404],
          [`${base}/../package.json`, 400], [`${base}/%2e%2e/package.json`, 400],
          [`${base}/%2e%2e%5cpackage.json`, 400], [`${base}/%252e%252e/package.json`, 400],
          [`${base}/C:%5cWindows`, 400], [`${base}/%00`, 400], [`${base}/%ZZ`, 400],
        ]) {
          const result = await rawRequest(server.origin, path);
          assert.equal(result.status, expected, `${path}: wrong HTTP status`);
          if (expected === 404) assert(result.body.includes('Page not found'), `${path}: missing custom 404`);
          if (expected === 308) assert(result.location.startsWith(`${base}/`), 'Redirect escaped base');
        }
        assert.equal((await rawRequest(server.origin, route(), 'HEAD')).body, '');
        assert.equal((await rawRequest(server.origin, route(), 'POST')).status, 405);
      });

      await browserCheck('homepage-keyboard', async ({ page }) => {
        await visit(page, '');
        await page.keyboard.press('Tab');
        const skip = page.getByRole('link', { name: 'Skip to content', exact: true });
        assert(await skip.evaluate((element) => element === document.activeElement), 'First Tab did not reach skip link');
        const box = await skip.boundingBox();
        assert(box && box.y >= 0 && box.y < 900, 'Focused skip link is not onscreen');
        await screenshot(page, 'homepage-keyboard.png');
        const target = await skip.getAttribute('href');
        assert(target?.startsWith('#'));
        await page.keyboard.press('Enter');
        await page.waitForFunction((hash) => location.hash === hash, target);
        assert(await page.locator(target).isVisible(), 'Skip link target is missing');
        await page.keyboard.press('Tab');
        assert(await page.evaluate(() => document.querySelector('main').contains(document.activeElement)), 'Skip link did not move sequential keyboard navigation into main content');
      });

      await browserCheck('language-long', async ({ page }) => {
        await visit(page, 'language/traits');
        assert(await page.evaluate(() => document.documentElement.scrollHeight > 2 * innerHeight), 'Selected language page is not long');
        assert(await page.locator('main h2').count() >= 5, 'Missing chapter sections');
        await highlighted(page, 'kagari');
        await screenshot(page, 'language-long.png', true);
        const heading = page.locator('main h2[id]').last();
        const id = await heading.getAttribute('id');
        await page.locator(`starlight-toc a[href="#${id}"]`).click();
        await page.waitForFunction((hash) => location.hash === hash, `#${id}`);
        const box = await heading.boundingBox();
        assert(box && box.y >= 0 && box.y < 900, 'Table of contents did not scroll to heading');
      });

      await browserCheck('host-code-copy', async ({ page, context }) => {
        await visit(page, 'host/native');
        await highlighted(page, 'rust');
        await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: server.origin });
        await page.evaluate(() => navigator.clipboard.writeText('browser-acceptance-sentinel'));
        const block = page.locator('.expressive-code').filter({ has: page.locator('pre[data-language="rust"]') }).first();
        const expected = await block.locator('pre code').innerText();
        const copy = block.getByRole('button', { name: 'Copy to clipboard', exact: true });
        await block.hover();
        await copy.click();
        await page.waitForFunction(async (text) => (await navigator.clipboard.readText()).trimEnd() === text.trimEnd(), expected);
        await block.locator('.feedback').waitFor({ state: 'visible' });
        assert((await block.locator('.feedback').innerText()).includes('Copied'), 'No copy success feedback');
        await screenshot(page, 'host-code-copy.png');
      });

      await browserCheck('nested-route', async ({ page }) => {
        await visit(page, 'host/overview');
        await page.locator('main').getByRole('link', { name: 'native registration', exact: true }).click();
        await page.waitForURL(`${server.origin}${route('host/native')}`);
        assert.equal((await page.locator('h1').innerText()).trim(), 'Native functions and types');
        await page.reload({ waitUntil: 'networkidle' });
        assert.equal(new URL(page.url()).pathname, route('host/native'));
        await screenshot(page, 'nested-route.png');
      });

      await browserCheck('pagefind-search', async ({ page, result }) => {
        await visit(page, '');
        assert.equal(result.pagefindResponses.length, 0, 'Pagefind index/API loaded before search interaction');
        await page.keyboard.press('ControlOrMeta+k');
        const dialog = page.getByRole('dialog', { name: 'Search', exact: true });
        await dialog.waitFor({ state: 'visible' });
        const input = dialog.getByRole('textbox');
        await input.fill('Generics');
        const link = dialog.locator('.pagefind-ui__result-link').filter({ hasText: 'Generics, traits and interfaces' }).first();
        await link.waitFor({ state: 'visible', timeout: 12_000 });
        const href = new URL(await link.getAttribute('href'), server.origin);
        assert.equal(href.origin, server.origin);
        assert.equal(href.pathname, route('language/traits'));
        assert(result.pagefindResponses.some(({ path, status }) => path.endsWith('/pagefind.js') && status === 200), 'Real Pagefind API did not load');
        assert(result.pagefindResponses.some(({ path, status }) => path.endsWith('.pf_index') && status === 200), 'Real Pagefind index did not load');
        await screenshot(page, 'pagefind-search.png');
        await page.keyboard.press('Escape');
        await dialog.waitFor({ state: 'hidden' });
        await page.keyboard.press('ControlOrMeta+k');
        await dialog.waitFor({ state: 'visible' });
        // Pagefind clears the query when its dialog closes.
        await input.fill('Generics');
        await link.waitFor({ state: 'visible' });
        await link.focus();
        await page.keyboard.press('Enter');
        await page.waitForURL((url) => url.pathname === route('language/traits'));
        assert.equal((await page.locator('h1').innerText()).trim(), 'Generics, traits and interfaces');
      });

      await browserCheck('themes', async ({ page }) => {
        await visit(page, '');
        const theme = page.getByRole('combobox', { name: 'Select theme', exact: true }).filter({ visible: true });
        const colors = [];
        for (const value of ['dark', 'light']) {
          await theme.selectOption(value);
          await page.waitForFunction((expected) => document.documentElement.dataset.theme === expected, value);
          colors.push(await page.locator('body').evaluate((element) => getComputedStyle(element).backgroundColor));
          await highlighted(page, 'kagari');
          await screenshot(page, `homepage-${value}.png`);
          await page.reload({ waitUntil: 'networkidle' });
          assert.equal(await page.locator('html').getAttribute('data-theme'), value, 'Theme did not survive reload');
        }
        assert.notEqual(colors[0], colors[1], 'Theme attribute changed without changing rendered background');
      });

      await browserCheck('mobile-sidebar', async ({ page }) => {
        await visit(page, 'language/traits');
        const menu = page.getByRole('button', { name: 'Menu', exact: true });
        const sidebar = page.locator('#starlight__sidebar');
        assert(await menu.isVisible(), 'Mobile menu toggle is missing');
        assert(!(await sidebar.isVisible()), 'Mobile sidebar starts open');
        await menu.click();
        await sidebar.waitFor({ state: 'visible' });
        await screenshot(page, 'mobile-sidebar-open.png');
        await sidebar.getByRole('link', { name: 'Variables and constants', exact: true }).click();
        await page.waitForURL(`${server.origin}${route('language/variables')}`);
        assert.equal((await page.locator('h1').innerText()).trim(), 'Variables and constants');
        await highlighted(page, 'kagari');
        assert(!(await sidebar.isVisible()), 'Mobile sidebar remained open after navigation');
        assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'Page overflows mobile viewport horizontally');
        await screenshot(page, 'mobile-code-page.png');
        await menu.click();
        await sidebar.waitFor({ state: 'visible' });
        await page.keyboard.press('Escape');
        await sidebar.waitFor({ state: 'hidden' });
      }, true);

      await browserCheck('not-found', async ({ page }) => {
        await visit(page, 'browser-acceptance-missing', 404);
        assert.equal((await page.locator('h1').innerText()).trim(), '404');
        assert((await page.locator('main').innerText()).includes('Page not found'));
        await screenshot(page, '404.png');
        const home = page.getByRole('link', { name: 'Kagari', exact: true }).first();
        assert.equal(await home.getAttribute('href'), route());
        await home.click();
        await page.waitForURL(`${server.origin}${route()}`);
        assert.equal((await page.locator('h1').innerText()).trim(), 'Kagari');
      });

      await check('build-unchanged', async () => {
        assert.equal(await inputDigest(), initialInputs, 'Build inputs changed during browser checks. Rebuild/check and rerun.');
        assert.equal(await outputDigest(), manifest.output, 'Build output changed during browser checks. Rebuild/check and rerun.');
      });
    }, 150_000, 'Browser acceptance matrix');
  } catch (error) {
    report.error = error.stack ?? String(error);
    console.error(error.message);
  } finally {
    stopped = true;
    for (const [label, cleanup] of [
      ['browser', async () => { if (browser) await browser.close(); }],
      ['server', async () => { if (server) await server.close(); }],
    ]) {
      try { await bounded(cleanup, 8_000, `${label} teardown`); }
      catch (error) { (report.teardownErrors ??= []).push(error.message); }
    }
    report.status = !report.error && !report.teardownErrors && report.checks.length > 0 && report.checks.every(({ status }) => status === 'passed') ? 'passed' : 'failed';
    try { await saveReport(); }
    finally { clearTimeout(watchdog); }
    console.log(`Browser acceptance ${report.status}: ${reportPath}`);
    if (report.status !== 'passed') process.exitCode = 1;
  }
}

async function highlighted(page, language) {
  const code = page.locator(`pre[data-language="${language}"] code`).first();
  assert(await code.isVisible(), `Missing visible ${language} code`);
  const colors = await code.locator('span[style]').evaluateAll((spans) => [...new Set(spans.filter((span) => span.textContent.trim()).map((span) => getComputedStyle(span).color))]);
  assert(colors.length >= 2, `${language} code lacks rendered token-color variation`);
}

async function rawRequest(origin, path, method = 'GET') {
  return bounded(() => new Promise((resolve, reject) => {
    const outgoing = request(origin, { path, method, timeout: 5_000, agent: false }, (response) => {
      let body = '';
      response.setEncoding('utf8');
      response.on('data', (chunk) => { body += chunk; });
      response.on('end', () => resolve({ status: response.statusCode, location: response.headers.location, body }));
      response.on('error', reject);
    });
    outgoing.on('timeout', () => outgoing.destroy(new Error(`HTTP request timed out: ${path}`)));
    outgoing.on('error', reject);
    outgoing.end();
  }), 6_000, `HTTP ${method} ${path}`);
}
