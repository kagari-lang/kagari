import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createWriteStream } from 'node:fs';
import { access, copyFile, mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveCliArgsFromVSCodeExecutablePath, runTests } from '@vscode/test-electron';

const script = fileURLToPath(import.meta.url);
const repository = fileURLToPath(new URL('../../', import.meta.url));
const extensionSource = path.join(repository, 'editors', 'vscode');
const hostTests = path.join(repository, 'editors', 'tests', 'vscode-host', 'index.cjs');
const artifacts = path.join(repository, 'target', 'editors');

function cleanEnvironment() {
  const environment = { ...process.env };
  for (const name of ['ELECTRON_RUN_AS_NODE', 'VSCODE_IPC_HOOK_CLI', 'VSCODE_CWD', 'VSCODE_DEV',
    'VSCODE_PORTABLE', 'VSCODE_LOGS', 'VSCODE_AMD_ENTRYPOINT', 'VSCODE_APPDATA', 'VSCODE_EXTENSIONS']) delete environment[name];
  return environment;
}

async function terminateOwnedTree(child) {
  if (!child.pid || child.exitCode !== null || child.signalCode !== null) return;
  if (process.platform === 'win32') {
    // Target only this runner's child and its descendants, never an image name.
    await new Promise((resolve, reject) => {
      const killer = spawn('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true });
      const timeout = setTimeout(() => { killer.kill(); reject(new Error('taskkill exceeded 5 seconds')); }, 5000);
      killer.once('error', (error) => { clearTimeout(timeout); reject(error); });
      killer.once('close', (code) => {
        clearTimeout(timeout);
        if (code === 0 || child.exitCode !== null) resolve();
        else reject(new Error(`taskkill for owned PID ${child.pid} exited ${code}`));
      });
    });
  } else {
    // Every subprocess below owns a newly detached process group on POSIX.
    try { process.kill(-child.pid, 'SIGKILL'); }
    catch (error) { if (error.code !== 'ESRCH') throw error; }
  }
}

async function boundedProcess(command, args, { timeoutMs, environment = cleanEnvironment(), logPath } = {}) {
  assert.ok(Number.isFinite(timeoutMs) && timeoutMs > 0, 'Every subprocess needs a finite timeout');
  const log = logPath ? createWriteStream(logPath) : undefined;
  const invocation = `[kagari-host] exec ${JSON.stringify([command, ...args])}\n`;
  if (log) log.write(invocation);
  let output = '';
  const child = spawn(command, args, {
    cwd: repository, env: environment, windowsHide: true, detached: process.platform !== 'win32',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const relay = (stream) => (data) => {
    stream.write(data);
    log?.write(data);
    output = (output + data.toString()).slice(-1024 * 1024);
  };
  child.stdout.on('data', relay(process.stdout));
  child.stderr.on('data', relay(process.stderr));
  try {
    await new Promise((resolve, reject) => {
      let finished = false;
      let timedOut = false;
      const finish = (error) => {
        if (finished) return;
        finished = true;
        clearTimeout(timer);
        error ? reject(error) : resolve();
      };
      const timer = setTimeout(async () => {
        timedOut = true;
        const notice = `[kagari-host] timeout: terminating only owned process tree PID ${child.pid}\n`;
        process.stderr.write(notice);
        log?.write(notice);
        let cleanup;
        try { await terminateOwnedTree(child); }
        catch (error) { cleanup = error; }
        child.stdout.destroy();
        child.stderr.destroy();
        child.unref();
        finish(new Error(`Timed out after ${timeoutMs} ms: ${command}${cleanup ? `; cleanup failed: ${cleanup.message}` : ''}`));
      }, timeoutMs);
      child.once('error', finish);
      child.once('close', (code, signal) => {
        if (!timedOut) finish(code === 0 ? undefined : new Error(
          `Subprocess failed (${code ?? signal}): ${command}; ${logPath ? `see ${logPath}` : output}`));
      });
    });
    return output;
  } finally {
    if (log) await new Promise((resolve) => log.end(resolve));
  }
}

async function executablePath(explicit) {
  if (explicit) {
    const executable = path.resolve(explicit);
    await access(executable);
    return executable;
  }
  if (process.platform !== 'win32') {
    throw new Error('Pass --vscode=/absolute/path/to/the/existing/editor on this platform; no editor is downloaded');
  }
  const found = await boundedProcess('where.exe', ['code'], { timeoutMs: 10000 });
  const launcher = found.split(/\r?\n/).find((line) => line.toLowerCase().endsWith('code.cmd'));
  assert.ok(launcher, 'Cannot discover code.cmd; pass --vscode=/absolute/path/to/Code.exe');
  const executable = path.resolve(path.dirname(launcher), '..', 'Code.exe');
  await access(executable);
  return executable;
}

async function cliInvocation(executable) {
  const [launcher, ...args] = resolveCliArgsFromVSCodeExecutablePath(executable, { reuseMachineInstall: true });
  if (process.platform !== 'win32') return { command: launcher, args, environment: cleanEnvironment() };
  // Follow the shipped code.cmd rather than guessing resources/app: current Windows
  // installs place resources under a versioned directory. Direct exec avoids shell quoting.
  const batch = await readFile(launcher, 'utf8');
  const match = batch.match(/"%~dp0([^"\r\n]*cli\.js)"/i);
  assert.ok(match, `Cannot resolve the CLI entrypoint from ${launcher}`);
  const cli = path.resolve(path.dirname(launcher), match[1]);
  await access(cli);
  return { command: executable, args: [cli, ...args],
    environment: { ...cleanEnvironment(), ELECTRON_RUN_AS_NODE: '1' } };
}

function hostArguments(directory, extensions, sample) {
  return [sample, '--new-window', '--disable-gpu', '--disable-updates', '--skip-welcome',
    '--skip-release-notes', '--disable-workspace-trust',
    '--disable-extension=github.copilot', '--disable-extension=github.copilot-chat',
    `--user-data-dir=${path.join(directory, 'user-data')}`, `--extensions-dir=${extensions}`,
    `--shared-data-dir=${path.join(directory, 'shared-data')}`];
}

async function worker() {
  const mode = process.env.KAGARI_HOST_MODE;
  assert.equal(mode, 'dev');
  const directory = process.env.KAGARI_HOST_RUN_DIRECTORY;
  const launchArgs = hostArguments(directory, process.env.KAGARI_HOST_EXTENSIONS_DIRECTORY,
    process.env.KAGARI_HOST_SAMPLE);
  const options = {
    vscodeExecutablePath: process.env.KAGARI_HOST_EXECUTABLE,
    extensionDevelopmentPath: extensionSource,
    extensionTestsPath: hostTests,
    launchArgs,
    extensionTestsEnv: { ...cleanEnvironment(), VSCODE_PORTABLE: directory,
      KAGARI_HOST_MODE: mode, KAGARI_HOST_RUN_DIRECTORY: directory,
      KAGARI_HOST_EXTENSION_SOURCE: extensionSource,
      KAGARI_HOST_EXTENSIONS_DIRECTORY: process.env.KAGARI_HOST_EXTENSIONS_DIRECTORY,
      KAGARI_HOST_SAMPLE: process.env.KAGARI_HOST_SAMPLE },
  };
  await writeFile(path.join(directory, 'launch.json'), JSON.stringify({
    vscodeExecutablePath: options.vscodeExecutablePath,
    extensionDevelopmentPath: options.extensionDevelopmentPath,
    extensionTestsPath: options.extensionTestsPath, launchArgs,
  }, null, 2) + '\n');
  await runTests(options);
}

async function runMode(mode, executable, cli, vsix, timeoutMs) {
  const directory = await mkdtemp(path.join(artifacts, `vscode-host-${mode}-`));
  const userData = path.join(directory, 'user-data');
  const extensions = path.join(directory, 'extensions');
  const sample = path.join(directory, 'standalone.kgr');
  await mkdir(path.join(userData, 'User'), { recursive: true });
  await mkdir(extensions);
  await writeFile(sample, '// Isolated standalone editor acceptance; no analyzer is launched.\nval answer = 42;\n');
  await writeFile(path.join(userData, 'User', 'settings.json'), JSON.stringify({
    'telemetry.telemetryLevel': 'off',
    'update.mode': 'none',
    'extensions.autoUpdate': false,
    'extensions.autoCheckUpdates': false,
    'extensions.ignoreRecommendations': true,
    'security.workspace.trust.enabled': false,
    'workbench.startupEditor': 'none',
    'workbench.colorTheme': 'Default Dark Modern',
    'window.restoreWindows': 'none',
    'editor.autoClosingBrackets': 'languageDefined',
    'editor.autoClosingQuotes': 'languageDefined',
    'editor.autoSurround': 'languageDefined',
    'editor.comments.insertSpace': true,
    'files.autoSave': 'off',
    'git.enabled': false,
    'chat.disableAIFeatures': true,
    'chat.agent.enabled': false,
  }, null, 2) + '\n');
  console.log(`[kagari-host] ${mode} isolated artifacts: ${directory}`);
  if (mode === 'installed') {
    await access(vsix);
    const isolation = [`--user-data-dir=${userData}`, `--extensions-dir=${extensions}`,
      `--shared-data-dir=${path.join(directory, 'shared-data')}`];
    const cliEnvironment = { ...cli.environment, VSCODE_PORTABLE: directory };
    await boundedProcess(cli.command, [...cli.args, ...isolation, '--install-extension', vsix, '--force'], {
      timeoutMs: 60000, environment: cliEnvironment, logPath: path.join(directory, 'install.log'),
    });
    const installed = await boundedProcess(cli.command,
      [...cli.args, ...isolation, '--list-extensions', '--show-versions'], {
        timeoutMs: 30000, environment: cliEnvironment, logPath: path.join(directory, 'installed-extensions.log'),
      });
    assert.ok(installed.split(/\r?\n/).some((line) => /^kagari-lang\.kagari-language@\d/.test(line)),
      'The local VSIX must be listed in the isolated extension directory');
  }
  const environment = {
    ...cleanEnvironment(), VSCODE_PORTABLE: directory,
    KAGARI_HOST_MODE: mode, KAGARI_HOST_RUN_DIRECTORY: directory,
    KAGARI_HOST_EXECUTABLE: executable, KAGARI_HOST_EXTENSION_SOURCE: extensionSource,
    KAGARI_HOST_EXTENSIONS_DIRECTORY: extensions, KAGARI_HOST_SAMPLE: sample,
  };
  if (mode === 'dev') {
    await boundedProcess(process.execPath, [script, '--worker'], {
      timeoutMs, logPath: path.join(directory, 'host.log'), environment,
    });
  } else {
    // Code 1.142.0 ignores --extensionTestsPath without a development extension.
    // A production-mode test-only helper invokes the same suite instead; Kagari
    // remains the CLI-installed VSIX and the launch contains no development flags.
    const staged = path.join(directory, 'harness-package');
    await mkdir(staged);
    await Promise.all(['package.json', 'harness.cjs', 'index.cjs', 'README.md'].map((name) =>
      copyFile(path.join(path.dirname(hostTests), name), path.join(staged, name))));
    const licenses = await Promise.all(['LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE'].map((name) =>
      readFile(path.join(extensionSource, name), 'utf8')));
    await writeFile(path.join(staged, 'LICENSE'), licenses.join('\n'));
    await boundedProcess(process.execPath, [script, '--package-harness'], {
      timeoutMs: 60000, logPath: path.join(directory, 'harness-package.log'), environment,
    });
    await boundedProcess(cli.command, [...cli.args,
      `--user-data-dir=${userData}`, `--extensions-dir=${extensions}`,
      `--shared-data-dir=${path.join(directory, 'shared-data')}`,
      '--install-extension', path.join(directory, 'kagari-host-acceptance-0.0.0.vsix'), '--force'], {
      timeoutMs: 60000, environment: { ...cli.environment, VSCODE_PORTABLE: directory },
      logPath: path.join(directory, 'install-harness.log'),
    });
    const launchArgs = hostArguments(directory, extensions, sample);
    await writeFile(path.join(directory, 'launch.json'), JSON.stringify({
      vscodeExecutablePath: executable, extensionDevelopmentPath: [], launchArgs,
    }, null, 2) + '\n');
    await boundedProcess(executable, launchArgs, {
      timeoutMs, logPath: path.join(directory, 'host.log'), environment,
    });
    const failure = await readFile(path.join(directory, 'failure.json'), 'utf8').catch((error) => {
      if (error.code === 'ENOENT') return undefined;
      throw error;
    });
    assert.equal(failure, undefined, `Installed host assertions failed: ${failure}`);
    const harness = JSON.parse(await readFile(path.join(directory, 'harness.json'), 'utf8'));
    assert.equal(harness.extensionMode, 1, 'The installed harness must have a Production ExtensionContext');
    process.stdout.write(await readFile(path.join(directory, 'assertions.log'), 'utf8'));
  }
  const launch = JSON.parse(await readFile(path.join(directory, 'launch.json'), 'utf8'));
  assert.deepEqual(launch.extensionDevelopmentPath, mode === 'dev' ? extensionSource : []);
  if (mode === 'installed') {
    assert.ok(launch.launchArgs.every((arg) => !arg.startsWith('--extensionDevelopmentPath')
      && !arg.startsWith('--extensionTestsPath')));
  }
  const result = JSON.parse(await readFile(path.join(directory, 'result.json'), 'utf8'));
  assert.equal(result.mode, mode, 'The host must actually execute the expected test runner');
  assert.equal(result.loadOrigin, mode === 'dev' ? 'development-source' : 'isolated-installed-vsix');
  assert.equal(result.standaloneLanguageId, 'kagari');
  assert.equal(result.extensionActive, false);
  assert.equal(result.themes.length, 2);
  assert.ok(result.themes.every((theme) => theme.comments && theme.pairs && theme.bracketJump));
  console.log(`[kagari-host] verified ${mode}: ${path.join(directory, 'result.json')}`);
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes('--worker')) return worker();
  if (args.includes('--package-harness')) {
    const { createVSIX } = await import('@vscode/vsce');
    const directory = process.env.KAGARI_HOST_RUN_DIRECTORY;
    return createVSIX({
      cwd: path.join(directory, 'harness-package'),
      packagePath: path.join(directory, 'kagari-host-acceptance-0.0.0.vsix'),
      dependencies: false, githubBranch: 'main',
    });
  }
  if (args.includes('--help')) {
    console.log('node editors/scripts/test-vscode-host.mjs [both|dev|installed] [--vscode=/absolute/executable] [--vsix=/absolute/package.vsix] [--timeout-ms=120000]');
    return;
  }
  const mode = args.find((arg) => !arg.startsWith('--')) ?? 'both';
  assert.ok(['both', 'dev', 'installed'].includes(mode), `Unknown host mode: ${mode}`);
  const known = ['--vscode=', '--vsix=', '--timeout-ms='];
  for (const arg of args.filter((arg) => arg.startsWith('--'))) {
    assert.ok(known.some((option) => arg.startsWith(option)), `Unknown option: ${arg}`);
  }
  const option = (name) => args.find((arg) => arg.startsWith(`--${name}=`))?.slice(name.length + 3);
  const timeoutMs = Number(option('timeout-ms') ?? 120000);
  assert.ok(Number.isSafeInteger(timeoutMs) && timeoutMs >= 1000 && timeoutMs <= 300000,
    'Host timeout must be 1000..300000 milliseconds');
  const manifest = JSON.parse(await readFile(path.join(extensionSource, 'package.json'), 'utf8'));
  const vsix = path.resolve(option('vsix') ?? path.join(artifacts, `${manifest.name}-${manifest.version}.vsix`));
  const executable = await executablePath(option('vscode'));
  const cli = await cliInvocation(executable);
  const version = await boundedProcess(cli.command, [...cli.args, '--version'], {
    timeoutMs: 15000, environment: cli.environment,
  });
  console.log(`[kagari-host] existing executable: ${executable}; version=${version.trim().split(/\r?\n/)[0]}; no download`);
  await mkdir(artifacts, { recursive: true });
  for (const selected of mode === 'both' ? ['dev', 'installed'] : [mode]) {
    await runMode(selected, executable, cli, vsix, timeoutMs);
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
