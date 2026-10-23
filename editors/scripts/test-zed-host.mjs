import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createWriteStream } from 'node:fs';
import { access, cp, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { parse } from 'smol-toml';
import { buildZed, run, zedCommit } from './test-zed-build.mjs';
import { productDirectory, stageZed } from './stage-zed.mjs';

async function readOptional(file) {
  try { return await readFile(file, 'utf8'); }
  catch (error) { if (error.code === 'ENOENT') return ''; throw error; }
}

async function terminateOwnedTree(child) {
  if (!child.pid || child.exitCode !== null || child.signalCode !== null) return;
  if (process.platform === 'win32') {
    // PID-scoped only: never terminate other Zed windows or use an image name.
    run('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { timeoutMs: 5000 });
  } else {
    try { process.kill(-child.pid, 'SIGKILL'); }
    catch (error) { if (error.code !== 'ESRCH') throw error; }
  }
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes('--help')) {
    console.log('node scripts/test-zed-host.mjs [--index-only | --build-json=<build.json>] '
      + '[--sdk=<SDK host root>] [--wsl=<installed distribution>] [--download-sdk] [--cli=<Zed CLI executable>] [--duration-ms=20000]');
    return;
  }
  assert.notEqual(process.platform, 'darwin',
    'Pinned Zed writes macOS logs outside --user-data-dir; this runner cannot isolate that profile safely');
  for (const arg of args) assert.ok(['--index-only', '--download-sdk'].includes(arg)
    || ['--build-json=', '--sdk=', '--wsl=', '--cli=', '--duration-ms='].some((name) => arg.startsWith(name)),
  `Unknown option ${arg}`);
  const option = (name) => args.find((arg) => arg.startsWith(`--${name}=`))?.slice(name.length + 3);
  const durationMs = Number(option('duration-ms') ?? 20000);
  assert.ok(Number.isSafeInteger(durationMs) && durationMs >= 1000 && durationMs <= 120000);
  const indexOnly = args.includes('--index-only');
  assert.ok(!(indexOnly && option('build-json')), '--index-only cannot consume a WASM build');
  // The CLI is used only for discovery/version reporting, not launch: Windows'
  // normal CLI routes through a release-channel-wide singleton in this version.
  const version = run(option('cli') ?? (process.platform === 'win32' ? 'zed.exe' : 'zed'),
    ['--version'], { timeoutMs: 10000 });
  assert.ok(version.includes('1.24.2') && version.includes(zedCommit), 'Host probe is pinned to Zed 1.24.2');
  const executable = version.split('–').at(-1).trim().replace(/^\\\\\?\\/, '');
  await access(executable);
  const stage = option('build-json') ? JSON.parse(await readFile(option('build-json'), 'utf8'))
    : indexOnly ? await stageZed() : await buildZed({ sdkPath: option('sdk'), wsl: option('wsl'), download: args.includes('--download-sdk') });
  if (!indexOnly) await access(path.join(stage.extensionDirectory, 'grammars', 'kagari.wasm'));
  const manifest = parse(await readFile(stage.manifestPath, 'utf8'));
  assert.match(manifest.id, /^[a-z0-9][a-z0-9_-]*$/);
  assert.ok(!Object.keys(manifest.language_servers ?? {}).length, 'Syntax-only extension must not register an analyzer');
  const directory = await productDirectory(undefined, 'zed-host-');
  const userData = path.join(directory, 'user-data');
  const installed = path.join(userData, 'extensions', 'installed', manifest.id);
  await mkdir(path.dirname(installed), { recursive: true });
  // ExtensionStore::rebuild_extension_index discovers directories here and reads
  // their manifests/configs/queries itself. Never manufacture its index.json.
  await cp(stage.extensionDirectory, installed, { recursive: true });
  const config = path.join(userData, 'config');
  const temporary = path.join(directory, 'tmp');
  await mkdir(config, { recursive: true });
  await mkdir(temporary);
  await writeFile(path.join(config, 'settings.json'), JSON.stringify({
    telemetry: { metrics: false, diagnostics: false }, auto_update: false, disable_ai: true,
    auto_install_extensions: { html: false }, auto_update_extensions: { [manifest.id]: false },
    languages: { Kagari: { enable_language_server: false } },
    theme: 'One Dark', restore_on_startup: 'none',
  }, null, 2) + '\n');
  const sample = path.join(directory, 'standalone-雪.kgr');
  await writeFile(sample, '// Standalone isolated Zed probe; no analyzer.\r\n'
    + 'fn main() {\r\n    val answer = 42;\r\n    val text = "value={answer}";\r\n'
    + '    /* outer /* nested */ end */\r\n}\r\n');
  const launchArgs = [...(process.platform === 'win32' ? ['--foreground'] : []), '--user-data-dir', userData, sample];
  const launch = { executable, version: version.trim(), launchArgs, directory, userData, installed,
    indexOnly, stateless: true, durationMs };
  await writeFile(path.join(directory, 'launch.json'), JSON.stringify(launch, null, 2) + '\n');
  console.log(`[zed-host] ${JSON.stringify(launch, null, 2)}`);
  const output = createWriteStream(path.join(directory, 'host.log'));
  // ZED_STATELESS is a real Zed mode: in-memory DBs and no singleton handoff.
  // The custom data directory still owns extensions/config/logs. This avoids
  // routing acceptance input to the user's running editor on Windows/macOS.
  const child = spawn(executable, launchArgs, {
    cwd: directory, env: { ...process.env, ZED_STATELESS: '1', TMP: temporary, TEMP: temporary,
      TMPDIR: temporary, ZED_LOG: 'info,language=trace,extension_host=debug' },
    detached: process.platform !== 'win32', stdio: ['ignore', 'pipe', 'pipe'], windowsHide: false,
  });
  let spawnError;
  child.once('error', (error) => { spawnError = error; });
  for (const stream of [child.stdout, child.stderr]) stream.on('data', (bytes) => {
    output.write(bytes); process.stdout.write(bytes);
  });
  try {
    const deadline = Date.now() + durationMs;
    while (Date.now() < deadline && child.exitCode === null && child.signalCode === null && !spawnError) {
      await delay(Math.min(250, deadline - Date.now()));
    }
    if (spawnError) throw spawnError;
  } finally {
    try { await terminateOwnedTree(child); }
    finally {
      child.stdout.destroy(); child.stderr.destroy(); child.unref();
      await new Promise((resolve) => output.end(resolve));
    }
  }
  const log = await readOptional(path.join(userData, 'logs', 'Zed.log'));
  if (log) await writeFile(path.join(directory, 'Zed.log'), log);
  const indexPath = path.join(userData, 'extensions', 'index.json');
  const indexContent = await readOptional(indexPath);
  const index = indexContent ? JSON.parse(indexContent) : undefined;
  const language = index?.languages?.Kagari;
  const combinedLog = `${log}\n${await readOptional(path.join(directory, 'host.log'))}`;
  const grammarLoadStarted = combinedLog.includes('start loading grammar "kagari"');
  const grammarLoadFinished = combinedLog.includes('finish loading grammar "kagari"');
  const loadErrors = combinedLog.split(/\r?\n/)
    .filter((line) => /error|fail/i.test(line) && /kagari|grammar|query/i.test(line));
  const result = { ...launch, pid: child.pid, exitCode: child.exitCode, indexCreated: Boolean(index),
    languageIndexed: language?.extension === manifest.id,
    suffixRegistered: language?.matcher?.path_suffixes?.includes('kgr') ?? false,
    grammarBuilt: !indexOnly, grammarLoadStarted, grammarLoadFinished, loadErrors,
    grammarLoadEvidence: grammarLoadStarted && grammarLoadFinished && loadErrors.length === 0
      ? 'Real Zed WASM loader ran to completion without reported language/grammar/query errors'
      : 'No successful real Zed grammar load established',
    visualAndEditingAcceptance: 'manual, not performed',
    acceptance: 'incomplete',
    limitation: indexOnly ? 'Metadata-only probe: no WASM grammar was built or loaded.'
      : 'An extension index and absence of errors do not prove rendering, language selection or editing behavior.',
  };
  await writeFile(path.join(directory, 'result.json'), JSON.stringify(result, null, 2) + '\n');
  console.log(`[zed-host] ${JSON.stringify(result, null, 2)}`);
  assert.ok(result.languageIndexed && result.suffixRegistered,
    `Actual editor did not index the Kagari .kgr association; inspect ${directory}`);
  if (!indexOnly) {
    assert.deepEqual(loadErrors, [], 'Actual editor reported grammar/query loading errors');
    assert.ok(grammarLoadStarted && grammarLoadFinished,
      'Indexing alone is insufficient: require positive trace events from the actual Zed WASM loader');
  }
  // There is no supported external test API to inspect Zed buffer language,
  // rendering, theme capture boundaries or key edits. Do not turn logs into a pass.
  process.exitCode = 2;
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
