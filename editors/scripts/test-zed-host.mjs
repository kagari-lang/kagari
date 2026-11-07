import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { access, copyFile, cp, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { parse } from 'smol-toml';
import { buildZed, run, zedCommit } from './test-zed-build.mjs';
import { productDirectory } from './stage-zed.mjs';

const repository = fileURLToPath(new URL('../../', import.meta.url));

async function readOptional(file) {
  try { return await readFile(file, 'utf8'); }
  catch (error) { if (error.code === 'ENOENT') return ''; throw error; }
}

async function terminateOwnedTree(child) {
  if (!child.pid || child.exitCode !== null || child.signalCode !== null) return;
  if (process.platform === 'win32') run('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { timeoutMs: 5000 });
  else {
    try { process.kill(-child.pid, 'SIGKILL'); }
    catch (error) { if (error.code !== 'ESRCH') throw error; }
  }
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes('--help')) {
    console.log('node editors/scripts/test-zed-host.mjs --analyzer=/absolute/analyzer --cli=/absolute/Zed-CLI '
      + '[--build-json=build.json] [--sdk=WASI-SDK-34] [--download-sdk] [--timeout-ms=60000]');
    return;
  }
  assert.notEqual(process.platform, 'darwin', 'This isolated runner does not support the pinned macOS log layout');
  for (const arg of args) assert.ok(arg === '--download-sdk'
    || ['--analyzer=', '--cli=', '--build-json=', '--sdk=', '--timeout-ms='].some((prefix) => arg.startsWith(prefix)), arg);
  const option = (name) => args.find((arg) => arg.startsWith(`--${name}=`))?.slice(name.length + 3);
  const analyzer = option('analyzer');
  assert.ok(analyzer && path.isAbsolute(analyzer), 'An explicit absolute analyzer path is required');
  await access(analyzer);
  const timeoutMs = Number(option('timeout-ms') ?? 60000);
  assert.ok(Number.isSafeInteger(timeoutMs) && timeoutMs >= 1000 && timeoutMs <= 120000);
  const version = run(option('cli') ?? 'zed', ['--version'], { timeoutMs: 10000 });
  assert.ok(version.includes('1.27.0') && version.includes(zedCommit), 'Host evidence is pinned to installed Zed1.27.0');
  const executable = version.split('–').at(-1).trim().replace(/^\\\\\?\\/, '');
  await access(executable);
  const stage = option('build-json') ? JSON.parse(await readFile(option('build-json'), 'utf8'))
    : await buildZed({ sdkPath: option('sdk'), download: args.includes('--download-sdk') });
  await access(path.join(stage.extensionDirectory, 'grammars', 'kagari.wasm'));
  await access(path.join(stage.extensionDirectory, 'extension.wasm'));
  const manifest = parse(await readFile(stage.manifestPath, 'utf8'));
  assert.deepEqual(manifest.language_servers.kagari.languages, ['Kagari']);
  assert.equal(manifest.language_servers.kagari.language_ids.Kagari, 'kagari');
  assert.equal(manifest.lib.version, '0.7.0');
  const directory = await productDirectory(undefined, 'zed-host-');
  const userData = path.join(directory, 'user-data');
  const installed = path.join(userData, 'extensions', 'installed', manifest.id);
  await mkdir(path.dirname(installed), { recursive: true });
  await cp(stage.extensionDirectory, installed, { recursive: true });
  const config = path.join(userData, 'config');
  const temporary = path.join(directory, 'tmp');
  await mkdir(config, { recursive: true });
  await mkdir(temporary);
  const sample = path.join(directory, 'standalone-雪 #.kgr');
  await copyFile(path.join(repository, 'crates/kagari-analyzer/tests/fixtures/workspace/standalone_error.kgr'), sample);
  const initial = await readFile(sample);
  const relay = path.join(directory, 'relay.json');
  const relayConfig = path.join(directory, 'relay-config.json');
  await writeFile(relayConfig, JSON.stringify({ analyzer, sample, result: relay, timeoutMs }) + '\n');
  await writeFile(path.join(config, 'settings.json'), JSON.stringify({
    telemetry: { metrics: false, diagnostics: false }, auto_update: false, disable_ai: true,
    // This profile opens only our disposable fixture; no trust preference is
    // written to the user's profile and no interactive trust click is required.
    session: { trust_all_worktrees: true, restore_unsaved_buffers: false },
    auto_install_extensions: { html: false }, auto_update_extensions: { [manifest.id]: false },
    languages: { Kagari: { enable_language_server: true, language_servers: ['kagari'] } },
    lsp: { kagari: { binary: { path: process.execPath,
      arguments: [path.join(repository, 'editors/tests/zed/relay.mjs'), relayConfig] } } },
    autosave: 'off', theme: 'One Dark', restore_on_startup: 'none',
  }, null, 2) + '\n');
  const launchArgs = [...(process.platform === 'win32' ? ['--foreground'] : []), '--user-data-dir', userData, sample];
  const launch = { executable, version: version.trim(), launchArgs, directory, userData, installed,
    analyzer, analyzerSha256: createHash('sha256').update(await readFile(analyzer)).digest('hex'),
    grammarSha256: stage.wasmSha256, adapterSha256: stage.adapterSha256, timeoutMs };
  await writeFile(path.join(directory, 'launch.json'), JSON.stringify(launch, null, 2) + '\n');
  console.log(`[zed-host] isolated artifacts: ${directory}`);
  const output = createWriteStream(path.join(directory, 'host.log'));
  const child = spawn(executable, launchArgs, {
    cwd: directory, env: { ...process.env, ZED_STATELESS: '1', TMP: temporary, TEMP: temporary,
      TMPDIR: temporary, ZED_LOG: 'info,language=trace,extension_host=debug' },
    detached: process.platform !== 'win32', stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
  });
  let spawnError;
  child.once('error', (error) => { spawnError = error; });
  let logBytes = 0;
  for (const stream of [child.stdout, child.stderr]) stream.on('data', (bytes) => {
    logBytes += bytes.length;
    if (logBytes <= 8 * 1024 * 1024) output.write(bytes);
    else spawnError ??= new Error('Zed host log exceeded 8 MiB');
  });
  let result;
  let accepted = false;
  const started = performance.now();
  try {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (spawnError) throw spawnError;
      if (child.exitCode !== null) throw new Error(`Zed exited early: ${child.exitCode}`);
      const content = await readOptional(relay);
      if (content) { result = JSON.parse(content); break; }
      const log = await readOptional(path.join(userData, 'logs', 'Zed.log'));
      if (log.includes('Failed to start language server "kagari"')) {
        throw new Error(`Real Zed could not start Kagari; inspect ${path.join(directory, 'Zed.log')}`);
      }
      await delay(100);
    }
    assert.ok(result, 'Real Zed did not complete the protocol transcript before its deadline');
    assert.equal(result.success, true, result.error);
    assert.ok(result.initialized && result.opened && result.changed && result.restored);
    assert.deepEqual(result.edits.map(({ name }) => name), ['change', 'restore']);
    assert.deepEqual(await readFile(sample), initial, 'Only the isolated unsaved editor buffer was edited');
    accepted = true;
  } finally {
    try { await terminateOwnedTree(child); }
    finally {
      child.stdout.destroy(); child.stderr.destroy(); child.unref();
      await new Promise((resolve) => output.end(resolve));
    }
    const log = await readOptional(path.join(userData, 'logs', 'Zed.log'));
    if (log) await writeFile(path.join(directory, 'Zed.log'), log);
    await writeFile(path.join(directory, 'result.json'), JSON.stringify({ ...launch,
      hostMilliseconds: performance.now() - started, pid: child.pid, protocol: result,
      success: accepted,
      scope: 'Real Zed launch/full-sync/changed semantic diagnostics; richer providers remain outside this checkpoint.',
    }, null, 2) + '\n');
  }
  console.log(`[zed-host] PASS actual initialize/open/applyEdit/change/diagnostics/restore: ${directory}`);
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
