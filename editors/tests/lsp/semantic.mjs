import assert from 'node:assert/strict';
import { copyFile, mkdir, mkdtemp, readFile, realpath, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import {
  CompletionRequest, DefinitionRequest, DidChangeTextDocumentNotification,
  DidCloseTextDocumentNotification, DidOpenTextDocumentNotification, HoverRequest,
  ImplementationRequest, PublishDiagnosticsNotification,
} from 'vscode-languageserver-protocol/node';

const repository = fileURLToPath(new URL('../../../', import.meta.url));
const fixture = path.join(repository, 'crates/kagari-analyzer/tests/fixtures/editor');

function position(text, needle) {
  const offset = text.indexOf(needle);
  assert.ok(offset >= 0, `Fixture site missing: ${needle}`);
  const prefix = text.slice(0, offset);
  // JavaScript string offsets are the UTF-16 code units negotiated on the wire.
  return { line: prefix.split('\n').length - 1, character: prefix.length - prefix.lastIndexOf('\n') - 1 };
}

function site(uri, text, needle) {
  return { textDocument: { uri }, position: position(text, needle) };
}

export async function createSemanticFixture() {
  const base = path.join(repository, 'target/lsp-tests');
  await mkdir(base, { recursive: true });
  const directory = await mkdtemp(path.join(base, 'node-雪 #'));
  for (const file of ['Kagari.toml', 'src/lib.kgr', 'src/api.kgr']) {
    const destination = path.join(directory, file);
    await mkdir(path.dirname(destination), { recursive: true });
    await copyFile(path.join(fixture, file), destination);
  }
  return {
    uri: pathToFileURL(directory).toString(),
    lib: pathToFileURL(path.join(directory, 'src/lib.kgr')).toString(),
    api: pathToFileURL(path.join(directory, 'src/api.kgr')).toString(),
    libText: await readFile(path.join(directory, 'src/lib.kgr'), 'utf8'),
    apiText: await readFile(path.join(directory, 'src/api.kgr'), 'utf8'),
    async cleanup() {
      const relative = path.relative(await realpath(base), await realpath(directory));
      assert.ok(relative && !relative.startsWith('..') && !path.isAbsolute(relative));
      await rm(directory, { recursive: true });
    },
  };
}

export async function semanticRoundtrip(connection, fixture, bounded) {
  const { lib, api, libText, apiText } = fixture;
  let waiting;
  const diagnostics = [];
  const subscription = connection.onNotification(PublishDiagnosticsNotification.type, (params) => {
    assert.ok(diagnostics.length < 128, 'Semantic diagnostics notification bound');
    diagnostics.push(params);
    if (waiting?.predicate(params)) {
      waiting.resolve(params);
      waiting = undefined;
    }
  });
  function expectDiagnostics(version, nonempty) {
    const predicate = (params) => params.uri === lib && params.version === version
      && (nonempty
        ? params.diagnostics.some((diagnostic) => diagnostic.code === 'KG_TYPE_RETURN_TYPE_MISMATCH')
        : params.diagnostics.length === 0);
    return new Promise((resolve) => {
      const existing = diagnostics.find(predicate);
      if (existing) resolve(existing);
      else waiting = { predicate, resolve };
    });
  }
  try {
    for (const [uri, text] of [[lib, libText], [api, apiText]]) {
      await connection.sendNotification(DidOpenTextDocumentNotification.type, {
        textDocument: { uri, languageId: 'kagari', version: 1, text },
      });
    }
    const hover = await bounded(connection.sendRequest(HoverRequest.type,
      site(lib, libText, 'answer\n')), 'semantic hover');
    assert.ok(Array.isArray(hover.contents) && hover.contents.some((content) =>
      typeof content.value === 'string' && content.value.includes('answer: i32')), JSON.stringify(hover));
    const definition = await bounded(connection.sendRequest(DefinitionRequest.type,
      site(lib, libText, 'value();')), 'cross-file definition');
    assert.equal(definition.uri, api);
    assert.deepEqual(definition.range.start, position(apiText, 'value()'));
    const implementations = await bounded(connection.sendRequest(ImplementationRequest.type,
      site(api, apiText, 'Read {')), 'complete implementation lookup');
    assert.equal(implementations.length, 1);
    assert.equal(implementations[0].uri, api);
    assert.deepEqual(implementations[0].range.start, position(apiText, 'Reading {\n    fn'));
    const completion = await bounded(connection.sendRequest(CompletionRequest.type,
      site(lib, libText, 'answer\n')), 'lexical completion');
    assert.equal(completion.isIncomplete, false);
    const answer = completion.items.find((item) => item.label === 'answer');
    assert.ok(answer, JSON.stringify(completion));
    assert.equal(answer.detail, 'i32');
    assert.equal(answer.textEdit.newText, 'answer');
    assert.deepEqual(answer.textEdit.range.start, position(libText, 'answer\n'));
    assert.equal(answer.textEdit.range.end.character - answer.textEdit.range.start.character, 6);

    const error = expectDiagnostics(2, true);
    await connection.sendNotification(DidChangeTextDocumentNotification.type, {
      textDocument: { uri: lib, version: 2 },
      contentChanges: [{ text: libText.replace('    answer\n', '    true\n') }],
    });
    const published = await bounded(error, 'versioned unsaved semantic diagnostic');
    assert.equal(published.diagnostics.find((diagnostic) => diagnostic.code === 'KG_TYPE_RETURN_TYPE_MISMATCH').severity, 1);
    const repair = expectDiagnostics(3, false);
    await connection.sendNotification(DidChangeTextDocumentNotification.type, {
      textDocument: { uri: lib, version: 3 }, contentChanges: [{ text: libText }],
    });
    await bounded(repair, 'repaired diagnostic clear');
    for (const uri of [lib, api]) {
      await connection.sendNotification(DidCloseTextDocumentNotification.type, { textDocument: { uri } });
    }
    return { hover: true, definition: true, implementation: true, completion: true,
      diagnostics: true, diagnosticVersions: [2, 3], diagnosticNotifications: diagnostics.length };
  } finally {
    waiting = undefined;
    subscription.dispose();
  }
}
