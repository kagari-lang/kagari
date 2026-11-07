'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');
const cases = require('./cases.cjs');
const { checkTheme } = require('./grammar.cjs');

async function boundedCase(action, milliseconds) {
  let timer;
  try {
    return await Promise.race([Promise.resolve().then(action), new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`Case deadline exceeded ${milliseconds} ms`)), milliseconds);
    })]);
  } finally { clearTimeout(timer); }
}

exports.run = async function run() {
  const mode = process.env.KAGARI_HOST_MODE;
  const runDirectory = process.env.KAGARI_HOST_RUN_DIRECTORY;
  const selection = JSON.parse(process.env.KAGARI_HOST_CASES);
  assert.ok(selection.length > 0, 'Empty host case selection');
  const extension = vscode.extensions.getExtension('kagari-lang.kagari-language');
  assert.ok(extension);
  const loadedPath = await fs.realpath(extension.extensionPath);
  const sourcePath = await fs.realpath(process.env.KAGARI_HOST_EXTENSION_SOURCE);
  const extensionDirectory = await fs.realpath(process.env.KAGARI_HOST_EXTENSIONS_DIRECTORY);
  if (mode === 'dev') assert.equal(loadedPath, sourcePath);
  else {
    const relative = path.relative(extensionDirectory, loadedPath);
    assert.ok(relative && !relative.startsWith('..') && !path.isAbsolute(relative));
    assert.notEqual(loadedPath, sourcePath);
  }
  const report = { mode, vscode: vscode.version, extensionPath: loadedPath,
    loadOrigin: mode === 'dev' ? 'development-source' : 'isolated-installed-vsix',
    selected: selection, executed: [], failed: [], unrun: [...selection], cases: [] };
  let failure;
  try {
    for (const id of selection) {
      const started = performance.now();
      const timeoutMs = id === 'H04' ? 30000 : 10000;
      report.unrun.shift();
      report.executed.push(id);
      try {
        const directory = path.join(runDirectory, 'fixtures', `${id}-雪 #`);
        const evidence = await boundedCase(async () => {
          if (id !== 'grammar') return cases[id]({ extension, directory });
          const document = await vscode.workspace.openTextDocument(vscode.Uri.file(path.join(directory, 'standalone.kgr')));
          const editor = await vscode.window.showTextDocument(document, { preview: false });
          const api = await extension.activate();
          await cases.status(api, (value) => value.state === 'disabled', 'grammar without analyzer');
          const evidence = { themes: [await checkTheme(editor, 'Default Light Modern', vscode.ColorThemeKind.Light),
            await checkTheme(editor, 'Default Dark Modern', vscode.ColorThemeKind.Dark)] };
          await vscode.commands.executeCommand('workbench.action.revertAndCloseActiveEditor');
          return evidence;
        }, timeoutMs);
        const row = { id, passed: true, milliseconds: performance.now() - started, timeoutMs, evidence };
        report.cases.push(row);
        await fs.appendFile(path.join(runDirectory, 'assertions.log'), `${JSON.stringify(row)}\n`);
      } catch (error) {
        report.failed.push(id);
        report.cases.push({ id, passed: false, milliseconds: performance.now() - started, timeoutMs, error: error.message });
        throw error;
      }
    }
  } catch (error) { failure = error; }
  finally {
    try {
      if (extension.isActive) await cases.disable(extension.exports);
      await vscode.commands.executeCommand('workbench.action.closeAllEditors');
    } catch (error) {
      report.cleanupError = error.message;
      failure ??= error;
    }
    report.client = extension.isActive ? extension.exports.status : undefined;
    report.openDocuments = vscode.workspace.textDocuments.map((document) => ({ uri: document.uri.toString(), dirty: document.isDirty }));
    await fs.writeFile(path.join(runDirectory, 'result.json'), JSON.stringify(report, null, 2) + '\n');
  }
  if (failure) throw failure;
};
