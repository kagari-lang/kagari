import assert from 'node:assert/strict';
import { constants } from 'node:fs';
import { access, readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { homedir } from 'node:os';
import { delimiter, extname, join, relative } from 'node:path';
import { chromium } from 'playwright';
import { base } from '../src/links.mjs';
import { distRoot, manifestName, walk } from './files.mjs';

export const actionTimeout = 8_000;

export async function bounded(operation, milliseconds, label) {
  let timer;
  try {
    return await Promise.race([
      Promise.resolve().then(operation),
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`${label} timed out after ${milliseconds}ms`)), milliseconds); }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

const mimeTypes = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.xml': 'application/xml; charset=utf-8',
  '.wasm': 'application/wasm',
};

/** Serve an immutable, symlink-free snapshot, never a request-derived filesystem path. */
export async function startServer() {
  const files = new Map();
  for (const path of await walk(distRoot, new Set([manifestName]))) {
    files.set(relative(distRoot, path).replaceAll('\\', '/'), await readFile(path));
  }
  assert(files.has('index.html') && files.has('404.html'), 'Missing built homepage/404; run npm run build.');
  const server = createServer({ headersTimeout: 5_000, requestTimeout: 10_000, keepAliveTimeout: 1_000 }, (request, response) => {
    response.setTimeout(10_000, () => response.destroy());
    response.setHeader('Cache-Control', 'no-store');
    response.setHeader('X-Content-Type-Options', 'nosniff');
    function send(status, name = '404.html') {
      const body = files.get(name);
      response.writeHead(status, {
        'Content-Type': mimeTypes[extname(name)] ?? 'application/octet-stream',
        'Content-Length': body.length,
      });
      response.end(request.method === 'HEAD' ? undefined : body);
    }
    if (!['GET', 'HEAD'].includes(request.method)) {
      response.setHeader('Allow', 'GET, HEAD');
      send(405);
      return;
    }
    let pathname;
    try { pathname = decodeURIComponent(request.url.split('?')[0]); }
    catch { send(400); return; }
    // Inspect before URL normalization, including Windows separators/ADS and double encoding.
    if (!pathname.startsWith('/') || /[\\%:#\u0000-\u001f\u007f]/u.test(pathname) || pathname.split('/').some((part) => part === '.' || part === '..')) {
      send(400);
      return;
    }
    function redirect(location) {
      response.writeHead(308, { Location: location, 'Content-Length': 0 });
      response.end();
    }
    if (pathname === base) { redirect(`${base}/`); return; }
    if (!pathname.startsWith(`${base}/`)) { send(404); return; }
    const name = pathname.slice(base.length + 1);
    if (name.endsWith('/') || name === '') {
      const index = `${name}index.html`;
      send(files.has(index) ? 200 : 404, files.has(index) ? index : '404.html');
    } else if (files.has(name)) {
      send(name === '404.html' ? 404 : 200, name);
    } else if (files.has(`${name}/index.html`)) {
      redirect(`${pathname}/`);
    } else {
      send(404);
    }
  });
  server.maxRequestsPerSocket = 100;
  server.on('clientError', (_error, socket) => socket.destroy());
  async function close() {
    const closed = new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
    server.closeAllConnections();
    await bounded(() => closed, 5_000, 'HTTP server teardown');
  }
  try {
    await bounded(() => new Promise((resolve, reject) => {
      server.once('error', reject);
      server.listen(0, '127.0.0.1', resolve);
    }), 5_000, 'HTTP server startup');
    return { origin: `http://127.0.0.1:${server.address().port}`, close };
  } catch (error) {
    if (server.listening) await close();
    throw error;
  }
}

/** Prefer Playwright's managed Chromium, then installed Chrome/Edge/Chromium. No downloads. */
export async function launchBrowser(explicitPath, attempts) {
  const candidates = [chromium.executablePath()];
  if (process.platform === 'win32') {
    for (const root of [process.env.PROGRAMFILES, process.env['PROGRAMFILES(X86)'], process.env.LOCALAPPDATA]) {
      if (root) candidates.push(join(root, 'Google/Chrome/Application/chrome.exe'), join(root, 'Microsoft/Edge/Application/msedge.exe'));
    }
  } else if (process.platform === 'darwin') {
    for (const root of ['/Applications', join(homedir(), 'Applications')]) {
      candidates.push(join(root, 'Google Chrome.app/Contents/MacOS/Google Chrome'), join(root, 'Microsoft Edge.app/Contents/MacOS/Microsoft Edge'), join(root, 'Chromium.app/Contents/MacOS/Chromium'));
    }
  } else {
    candidates.push('/opt/google/chrome/chrome', '/opt/microsoft/msedge/msedge', '/usr/bin/chromium', '/usr/bin/chromium-browser');
  }
  for (const directory of (process.env.PATH ?? '').split(delimiter).filter(Boolean)) {
    for (const name of process.platform === 'win32' ? ['chrome.exe', 'msedge.exe', 'chromium.exe'] : ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser', 'microsoft-edge', 'microsoft-edge-stable']) {
      candidates.push(join(directory, name));
    }
  }
  for (const executablePath of new Set(explicitPath ? [explicitPath] : candidates)) {
    try { await access(executablePath, process.platform === 'win32' ? constants.F_OK : constants.X_OK); }
    catch { attempts.push({ executablePath, result: 'not available' }); continue; }
    try {
      const browser = await chromium.launch({ executablePath, headless: true, timeout: 15_000 });
      attempts.push({ executablePath, result: 'launched' });
      return { browser, executablePath };
    } catch (error) {
      attempts.push({ executablePath, result: error.message });
    }
  }
  throw new Error('No usable Chromium browser. Install Chrome/Edge/Chromium or Playwright Chromium, or pass --executable-path PATH. See launchAttempts in report.json.');
}
