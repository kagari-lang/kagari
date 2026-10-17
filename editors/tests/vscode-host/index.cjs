const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

const extensionId = 'kagari-lang.kagari-language';
const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

async function record(message) {
  console.log(message);
  await fs.appendFile(path.join(process.env.KAGARI_HOST_RUN_DIRECTORY, 'assertions.log'), message + '\n');
}

async function waitFor(description, predicate) {
  const deadline = Date.now() + 10000;
  while (!predicate()) {
    assert.ok(Date.now() < deadline, `Timed out waiting for ${description}`);
    await delay(50);
  }
}

async function replace(editor, text, cursor = text.length) {
  const document = editor.document;
  const changed = await editor.edit((edit) => edit.replace(
    new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), text));
  assert.equal(changed, true, 'Editor replacement must succeed');
  const position = document.positionAt(cursor);
  editor.selection = new vscode.Selection(position, position);
  await vscode.commands.executeCommand('workbench.action.focusActiveEditorGroup');
}

async function checkComments(editor) {
  const text = 'val answer = 42;';
  await replace(editor, text, 0);
  await vscode.commands.executeCommand('editor.action.commentLine');
  assert.equal(editor.document.getText(), `// ${text}`);
  await vscode.commands.executeCommand('editor.action.commentLine');
  assert.equal(editor.document.getText(), text);

  editor.selection = new vscode.Selection(0, 0, 0, text.length);
  await vscode.commands.executeCommand('editor.action.blockComment');
  assert.equal(editor.document.getText(), `/* ${text} */`);
  await vscode.commands.executeCommand('editor.action.blockComment');
  assert.equal(editor.document.getText(), text);
}

async function checkPairs(editor) {
  const prefix = 'val value = ';
  for (const [open, close] of [['(', ')'], ['[', ']'], ['{', '}'], ['"', '"']]) {
    await replace(editor, prefix);
    await vscode.commands.executeCommand('type', { text: open });
    assert.equal(editor.document.getText(), prefix + open + close, `Code pairing for ${open}`);
    assert.equal(editor.selection.active.character, prefix.length + 1);
  }
  for (const open of ['<', "'"]) {
    await replace(editor, prefix);
    await vscode.commands.executeCommand('type', { text: open });
    assert.equal(editor.document.getText(), prefix + open, `No unsupported pairing for ${open}`);
  }

  for (const [text, cursor] of [
    ['val value = "text ";', 'val value = "text '.length],
    ['// comment ', '// comment '.length],
    ['/* comment  */', '/* comment '.length],
    ['val text = f"text ";', 'val text = f"text '.length],
  ]) {
    await replace(editor, text, cursor);
    await vscode.commands.executeCommand('type', { text: '(' });
    assert.equal(editor.document.getText(), text.slice(0, cursor) + '(' + text.slice(cursor),
      `No bracket autoclose inside string/comment: ${text}`);
  }

  const embedded = 'val text = f"{ }";';
  await replace(editor, embedded, embedded.indexOf('{') + 1);
  await vscode.commands.executeCommand('type', { text: '(' });
  assert.equal(editor.document.getText(), 'val text = f"{() }";',
    'An embedded expression must support code bracket pairing, not inherit string category');

  const brackets = 'val value = (1 + (2 * 3));';
  const open = brackets.indexOf('(');
  const close = brackets.lastIndexOf(')');
  await replace(editor, brackets, open + 1);
  await vscode.commands.executeCommand('editor.action.jumpToBracket');
  assert.equal(editor.selection.active.character, close, 'Jump to matching outer closing bracket');
}

async function checkTheme(editor, name, kind) {
  await vscode.workspace.getConfiguration('workbench').update(
    'colorTheme', name, vscode.ConfigurationTarget.Global);
  await waitFor(`${name} to become the active color theme`, () => vscode.window.activeColorTheme.kind === kind);
  assert.equal(vscode.workspace.getConfiguration('workbench').get('colorTheme'), name);
  await checkComments(editor);
  await checkPairs(editor);
  await record(`[kagari-host] ${name}: comments, code/string/comment/embedded pairing, bracket jump passed`);
  return { name, kind, comments: true, pairs: true, bracketJump: true };
}

/** Called by the real VS Code extension test host; deliberately no test-framework activation. */
exports.run = async function run() {
  const mode = process.env.KAGARI_HOST_MODE;
  assert.ok(mode === 'dev' || mode === 'installed', `Unexpected test mode: ${mode}`);
  const runDirectory = process.env.KAGARI_HOST_RUN_DIRECTORY;
  const sourcePath = await fs.realpath(process.env.KAGARI_HOST_EXTENSION_SOURCE);
  const extensionDirectory = await fs.realpath(process.env.KAGARI_HOST_EXTENSIONS_DIRECTORY);
  const samplePath = await fs.realpath(process.env.KAGARI_HOST_SAMPLE);
  const extension = vscode.extensions.getExtension(extensionId);
  assert.ok(extension, `Missing extension ${extensionId}`);
  const loadedPath = await fs.realpath(extension.extensionPath);
  if (mode === 'dev') {
    assert.equal(loadedPath, sourcePath);

  } else {
    const relative = path.relative(extensionDirectory, loadedPath);
    assert.ok(relative && !relative.startsWith('..') && !path.isAbsolute(relative), loadedPath);
    assert.notEqual(loadedPath, sourcePath);

  }
  const manifest = extension.packageJSON;
  assert.equal(manifest.publisher, 'kagari-lang');
  assert.equal(manifest.name, 'kagari-language');
  for (const field of ['main', 'browser', 'activationEvents', 'dependencies', 'devDependencies']) {
    assert.equal(manifest[field], undefined, `No runtime/server entrypoint or dependency: ${field}`);
  }
  assert.deepEqual(Object.keys(manifest.contributes).sort(), ['grammars', 'languages']);
  assert.equal(manifest.contributes.grammars[0].scopeName, 'source.kagari');
  assert.equal(extension.isActive, false, 'A declarative extension must not activate');
  assert.equal(vscode.workspace.workspaceFolders?.length ?? 0, 0, 'Open a standalone file, not a workspace');
  assert.ok((await vscode.languages.getLanguages()).includes('kagari'));

  const document = await vscode.workspace.openTextDocument(vscode.Uri.file(samplePath));
  assert.equal(document.languageId, 'kagari', 'Recognize .kgr without overriding its language');
  const editor = await vscode.window.showTextDocument(document, { preview: false });
  const original = document.getText();
  await record(`[kagari-host] mode=${mode} vscode=${vscode.version} loadOrigin=${mode === 'dev' ? 'development-source' : 'isolated-installed-vsix'}`);
  await record(`[kagari-host] extensionPath=${loadedPath}`);
  await record('[kagari-host] standalone .kgr recognized; declarative manifest; extension inactive; no analyzer launched');
  const themes = [];
  try {
    themes.push(await checkTheme(editor, 'Default Light Modern', vscode.ColorThemeKind.Light));
    themes.push(await checkTheme(editor, 'Default Dark Modern', vscode.ColorThemeKind.Dark));
    assert.equal(extension.isActive, false, 'Editing must not activate a runtime or server');
    await replace(editor, original);
    assert.equal(await document.save(), true);
    const report = {
      mode, vscode: vscode.version, extensionId, extensionPath: loadedPath,
      loadOrigin: mode === 'dev' ? 'development-source' : 'isolated-installed-vsix',
      standaloneLanguageId: document.languageId,
      extensionActive: extension.isActive, themes,
      limits: 'No public TextMate token-scope API. Automated editing checks do not establish visual or token-inspector acceptance.',
    };
    await fs.writeFile(path.join(runDirectory, 'result.json'), JSON.stringify(report, null, 2) + '\n');
    await record(`[kagari-host] PASS ${mode}; light/dark host assertions passed; visual inspection remains manual`);
  } finally {
    await replace(editor, original);
    await document.save();
    await vscode.commands.executeCommand('workbench.action.closeAllEditors');
  }
};
