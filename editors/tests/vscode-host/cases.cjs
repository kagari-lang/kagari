'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');
const { replace, checkTheme } = require('./grammar.cjs');

function observe(event, predicate, description, action = async () => {}) {
  let subscription;
  let timer;
  let check;
  const result = new Promise((resolve, reject) => {
    check = () => {
      try { const value = predicate(); if (value) resolve(value); }
      catch (error) { reject(error); }
    };
    subscription = event(check);
    timer = setTimeout(() => reject(new Error(`Timed out: ${description}`)), 20000);
  });
  return Promise.resolve().then(action).then(() => { check(); return result; })
    .finally(() => { clearTimeout(timer); subscription.dispose(); });
}

function status(api, predicate, description, action) {
  return observe(api.onDidChangeStatus, () => predicate(api.status) && api.status, description, action);
}

function diagnostics(document, hasError, action) {
  return observe(vscode.languages.onDidChangeDiagnostics, () => {
    const values = vscode.languages.getDiagnostics(document.uri);
    return (hasError ? values.some((value) => (typeof value.code === 'object' ? value.code.value : value.code)
      === 'KG_TYPE_RETURN_TYPE_MISMATCH') : values.length === 0) && { values };
  }, `${document.uri.toString()} diagnostics ${hasError ? 'type error' : 'clear'}`, action);
}

function position(document, needle, extra = 0, last = false) {
  const text = document.getText();
  const offset = last ? text.lastIndexOf(needle) : text.indexOf(needle);
  assert.ok(offset >= 0, `Missing fixture marker ${needle}`);
  return document.positionAt(offset + extra);
}

async function open(directory, relative) {
  const document = await vscode.workspace.openTextDocument(vscode.Uri.file(path.join(directory, relative)));
  const editor = await vscode.window.showTextDocument(document, { preview: false });
  assert.equal(document.languageId, 'kagari');
  return editor;
}

function enable(api) {
  return status(api, (value) => value.state === 'running', 'client running', () =>
    vscode.workspace.getConfiguration('kagari.server').update('enabled', true, vscode.ConfigurationTarget.Global));
}

function disable(api) {
  return status(api, (value) => value.state === 'disabled', 'client disabled', () =>
    vscode.workspace.getConfiguration('kagari.server').update('enabled', false, vscode.ConfigurationTarget.Global));
}

async function clean(editor, original) {
  if (editor.document.getText() !== original) await replace(editor, original);
  if (editor.document.isDirty) assert.equal(await editor.document.save(), true);
  await vscode.commands.executeCommand('workbench.action.closeAllEditors');
}

async function hover(document, marker) {
  const values = await vscode.commands.executeCommand('vscode.executeHoverProvider',
    document.uri, position(document, marker, 1, true));
  assert.equal(values.length, 1, 'Exactly one registered Kagari hover provider result');
  assert.match(values[0].contents.map((content) => content.value).join('\n'), /answer: i32/);
}

async function activation({ extension, directory }) {
  const plaintext = await vscode.workspace.openTextDocument(vscode.Uri.file(path.join(directory, 'plain.txt')));
  await vscode.window.showTextDocument(plaintext);
  assert.equal(extension.isActive, false, 'Plain text must not activate Kagari');
  const editor = await open(directory, 'standalone.kgr');
  const original = editor.document.getText();
  const api = await extension.activate();
  await status(api, (value) => value.state === 'disabled', 'disabled activation');
  await checkTheme(editor, 'Default Dark Modern', vscode.ColorThemeKind.Dark);
  await replace(editor, original);
  const connected = await enable(api);
  assert.ok(Number.isSafeInteger(connected.pid));
  assert.equal(connected.build.name, 'kagari-analyzer');
  assert.ok(connected.capabilities.textDocumentSync);
  const providers = await vscode.commands.executeCommand('vscode.executeHoverProvider', plaintext.uri, new vscode.Position(0, 0));
  assert.deepEqual(providers, []);
  await clean(editor, original);
  return { pid: connected.pid, language: 'kagari', plaintextExcluded: true, disabledEditing: true };
}

async function roundTrip({ extension, directory }) {
  let editor = await open(directory, 'src/lib.kgr');
  const original = editor.document.getText();
  const api = await extension.activate();
  await enable(api);
  try {
    await hover(editor.document, 'answer');
    const targets = await vscode.commands.executeCommand('vscode.executeDefinitionProvider', editor.document.uri,
      position(editor.document, 'api::value', 6));
    assert.equal(targets.length, 1);
    assert.ok(targets[0].uri.fsPath.endsWith(`${path.sep}api.kgr`));
    const completion = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', editor.document.uri,
      position(editor.document, 'answer', 3, true));
    const answer = completion.items.find((item) => (typeof item.label === 'string' ? item.label : item.label.label) === 'answer');
    assert.ok(answer, 'Local answer completion');
    assert.equal(answer.insertText, 'answer');
    assert.ok(answer.range instanceof vscode.Range);
    const apiEditor = await open(directory, 'src/api.kgr');
    const implementations = await vscode.commands.executeCommand('vscode.executeImplementationProvider', apiEditor.document.uri,
      position(apiEditor.document, 'trait Read', 7));
    assert.equal(implementations.length, 1);
    assert.equal(implementations[0].uri.toString(), apiEditor.document.uri.toString());
    editor = await vscode.window.showTextDocument(editor.document, { preview: false });
    const changed = original.replace('    answer\n', '    true\n');
    assert.notEqual(changed, original);
    await diagnostics(editor.document, true, () => replace(editor, changed));
    assert.equal(editor.document.isDirty, true);
    await diagnostics(editor.document, false, () => replace(editor, original));
    return { hover: true, definition: true, implementation: true, completion: true, diagnostics: true };
  } finally { await clean(editor, original); }
}

async function navigation({ extension, directory }) {
  const editor = await open(directory, 'src/lib.kgr');
  const api = await extension.activate();
  await enable(api);
  assert.match(editor.document.uri.toString(), /%23/);
  assert.match(editor.document.uri.fsPath, /雪/);
  const result = {};
  for (const [name, marker, extra, expected] of [
    ['crossFile', 'api::value', 6, 'value'], ['native', 'create()', 2, 'new'],
  ]) {
    const targets = await vscode.commands.executeCommand('vscode.executeDefinitionProvider', editor.document.uri,
      position(editor.document, marker, extra, true));
    assert.equal(targets.length, 1, `${name} destination`);
    const target = targets[0];
    assert.equal(target.uri.scheme, 'file');
    const document = await vscode.workspace.openTextDocument(target.uri);
    await vscode.window.showTextDocument(document, { selection: target.range, preview: false });
    assert.equal(document.getText(target.range), expected);
    assert.equal(await fs.readFile(target.uri.fsPath, 'utf8'), document.getText());
    if (name === 'native') assert.notEqual(path.dirname(target.uri.fsPath), path.join(directory, 'src'));
    result[name] = { uri: target.uri.toString(), range: target.range };
  }
  await vscode.commands.executeCommand('workbench.action.closeAllEditors');
  return result;
}

function assertDead(pid) {
  assert.throws(() => process.kill(pid, 0), (error) => error.code === 'ESRCH', `Owned PID ${pid} must exit`);
}

async function recovery({ extension, directory }) {
  const editor = await open(directory, 'standalone.kgr');
  const original = editor.document.getText();
  const bad = original.replace('    7\n', '    true\n');
  assert.notEqual(bad, original);
  const api = await extension.activate();
  await enable(api);
  const owned = [];
  try {
    await diagnostics(editor.document, true, () => replace(editor, bad));
    for (let retry = 1; retry <= 3; retry += 1) {
      const pid = api.status.pid;
      assert.ok(pid);
      owned.push(pid);
      const current = await status(api, (value) => value.state === 'running' && value.pid !== pid,
        `automatic restart ${retry}`, async () => { process.kill(pid); });
      assert.equal(current.restartCount, retry);
      assertDead(pid);
      assert.equal(editor.document.getText(), bad);
      assert.equal(editor.document.isDirty, true);
      assert.equal(await fs.readFile(editor.document.uri.fsPath, 'utf8'), original);
      await diagnostics(editor.document, true);
    }
    const pid = api.status.pid;
    owned.push(pid);
    const stopped = await status(api, (value) => value.state === 'error', 'restart ceiling', async () => { process.kill(pid); });
    assert.equal(stopped.pendingRestart, false);
    assert.equal(stopped.pid, undefined);
    assert.match(stopped.error, /three automatic restarts/);
    assertDead(pid);
    assert.deepEqual(vscode.languages.getDiagnostics(editor.document.uri), []);
    const restarted = await status(api, (value) => value.state === 'running', 'manual restart', () =>
      vscode.commands.executeCommand('kagari.restartServer'));
    assert.equal(restarted.restartCount, 0);
    await diagnostics(editor.document, true);
    await disable(api);
    assert.equal(api.status.pendingRestart, false);
    return { terminatedOwnedPids: owned, restoredUnsaved: true, retryCeiling: 3 };
  } finally { await clean(editor, original); }
}

async function installed({ extension, directory }) {
  const editor = await open(directory, 'src/lib.kgr');
  const api = await extension.activate();
  await enable(api);
  await hover(editor.document, 'answer');
  assert.ok(extension.extensionPath.includes(`${path.sep}extensions${path.sep}`));
  await fs.access(path.join(extension.extensionPath, 'dist/extension.cjs'));
  await assert.rejects(fs.access(path.join(extension.extensionPath, 'node_modules')));
  await disable(api);
  await vscode.commands.executeCommand('workbench.action.closeAllEditors');
  return { bundledLoad: true, registeredHover: true, nodeModulesAbsent: true };
}

module.exports = { H01: activation, H02: roundTrip, H03: navigation, H04: recovery, H05: installed, status, disable };
