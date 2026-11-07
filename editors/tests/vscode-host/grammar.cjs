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
  for (const [open, close] of [['(', ')'], ['[', ']'], ['{', '}'], ['"', '"'], ["'", "'"]]) {
    await replace(editor, prefix);
    await vscode.commands.executeCommand('type', { text: open });
    assert.equal(editor.document.getText(), prefix + open + close, `Code pairing for ${open}`);
    assert.equal(editor.selection.active.character, prefix.length + 1);
  }
  for (const open of ['<']) {
    await replace(editor, prefix);
    await vscode.commands.executeCommand('type', { text: open });
    assert.equal(editor.document.getText(), prefix + open, `No unsupported pairing for ${open}`);
  }

  for (const [text, cursor] of [
    ['val value = "text ";', 'val value = "text '.length],
    ['// comment ', '// comment '.length],
    ['/* comment  */', '/* comment '.length],
    ['val text = r"text ";', 'val text = r"text '.length],
  ]) {
    await replace(editor, text, cursor);
    await vscode.commands.executeCommand('type', { text: '(' });
    assert.equal(editor.document.getText(), text.slice(0, cursor) + '(' + text.slice(cursor),
      `No bracket autoclose inside string/comment: ${text}`);
  }

  const embedded = 'val text = "{name}";';
  await replace(editor, embedded, embedded.indexOf('}'));
  await vscode.commands.executeCommand('type', { text: '(' });
  assert.equal(editor.document.getText(), 'val text = "{name()}";',
    'An identifier hole must initially support code bracket pairing, not inherit string category');

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

module.exports = { checkTheme, replace };
