'use strict';

const path = require('node:path');
const { spawn } = require('node:child_process');
const { realpath, stat } = require('node:fs/promises');
const { LanguageClient, CloseAction, ErrorAction, State } = require('vscode-languageclient/node');

function configurationError(message) {
  const error = new Error(message);
  error.configuration = true;
  return error;
}

async function executable(configured) {
  if (!configured || !path.isAbsolute(configured)) {
    throw configurationError('Set kagari.server.path to the absolute path of kagari-analyzer, or disable kagari.server.enabled.');
  }
  try {
    const resolved = await realpath(configured);
    if (!(await stat(resolved)).isFile()) throw new Error('not a file');
    return resolved;
  } catch (error) {
    throw configurationError(`Cannot open configured Kagari analyzer ${configured}: ${error.message}`);
  }
}

function deadline(operation, milliseconds, description, expired = () => {}) {
  let timer;
  return Promise.race([operation, new Promise((_, reject) => {
    timer = setTimeout(() => { expired(); reject(new Error(`${description} exceeded ${milliseconds} ms`)); }, milliseconds);
  })]).finally(() => clearTimeout(timer));
}

function stopped(client) {
  return client.state === State.Stopped || client.state === State.StartFailed;
}

function waitStopped(client) {
  if (stopped(client)) return Promise.resolve();
  let subscription;
  const ready = new Promise((resolve) => {
    subscription = client.onDidChangeState(() => { if (stopped(client)) resolve(); });
    if (stopped(client)) resolve();
  });
  return deadline(ready, 5000, 'Kagari client cleanup').finally(() => subscription.dispose());
}

async function createClient(configuration, closed, output) {
  const command = await executable(configuration.path);
  output.appendLine(`Starting Kagari analyzer: ${command}`);
  let child;
  const server = () => new Promise((resolve, reject) => {
    const env = { ...process.env,
      KAGARI_STDLIB_SOURCES: process.env.KAGARI_STDLIB_SOURCES ?? configuration.standardSources };
    child = spawn(command, ['--stdio'], { env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
    child.once('error', reject);
    child.once('spawn', () => resolve(child));
  });
  const client = new LanguageClient('kagari', 'Kagari', server, {
    documentSelector: [{ language: 'kagari', scheme: 'file' }, { language: 'kagari', scheme: 'untitled' }],
    outputChannel: output,
    revealOutputChannelOn: 4,
    initializationFailedHandler: () => false,
    errorHandler: {
      error: (_error, _message, count) => {
        if (count >= 3) { closed(); return { action: ErrorAction.Shutdown, handled: true }; }
        return { action: ErrorAction.Continue, handled: true };
      },
      closed: () => { closed(); return { action: CloseAction.DoNotRestart, handled: true }; },
    },
  });
  let start;
  return {
    async start() {
      start = client.start();
      try {
        await deadline(start, 15000, 'Kagari initialization', () => child?.kill());
        output.appendLine(`Kagari serverInfo: ${JSON.stringify(client.initializeResult?.serverInfo)}`);
        output.appendLine(`Kagari capabilities: ${JSON.stringify(client.initializeResult?.capabilities)}`);
      } catch (error) {
        if (['ENOENT', 'EACCES', 'ENOEXEC'].includes(error.code)) error.configuration = true;
        throw error;
      }
    },
    waitStopped: () => waitStopped(client),
    status: () => ({ pid: child?.exitCode === null && child?.signalCode === null ? child.pid : undefined, path: command,
      build: client.initializeResult?.serverInfo,
      capabilities: client.initializeResult?.capabilities }),
    async stop() {
      if (client.state === State.Starting) {
        child?.kill();
        await deadline(start.catch(() => {}), 5000, 'Kagari startup cancellation');
      }
      try {
        if (client.state === State.Running) await client.stop(2000);
        await waitStopped(client);
      } finally {
        if (child?.pid && child.exitCode === null && child.signalCode === null) {
          child.kill();
          await deadline(new Promise((resolve) => {
            child.once('exit', resolve);
            if (child.exitCode !== null || child.signalCode !== null) resolve();
          }), 5000, 'Kagari process exit');
        }
      }
      if (client.state === State.Stopped) await client.dispose();
    },
  };
}

module.exports = { createClient };
