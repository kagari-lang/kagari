const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');
const { run } = require('./index.cjs');

exports.activate = async function activate(context) {
  // Never quit a real editor if someone accidentally installs this test helper.
  // Only the orchestrator's disposable extension directory is allowed to run it.
  assert.equal(process.env.KAGARI_HOST_MODE, 'installed');
  const directory = await fs.realpath(process.env.KAGARI_HOST_RUN_DIRECTORY);
  const staged = await fs.realpath(path.join(directory, 'extensions', 'kagari-lang.kagari-host-acceptance-0.0.0'));
  assert.equal(await fs.realpath(context.extensionPath), staged);
  assert.equal(context.extensionMode, vscode.ExtensionMode.Production,
    'The test-only harness itself must run without development flags');
  await fs.writeFile(path.join(directory, 'harness.json'), JSON.stringify({
    extensionPath: staged, extensionMode: context.extensionMode,
  }, null, 2) + '\n');
  try {
    await run();
  } catch (error) {
    console.error('[kagari-host] installed harness failed', error);
    await fs.writeFile(path.join(directory, 'failure.json'), JSON.stringify({
      message: error.message, stack: error.stack,
    }, null, 2) + '\n');
  } finally {
    await vscode.commands.executeCommand('workbench.action.quit');
  }
};
