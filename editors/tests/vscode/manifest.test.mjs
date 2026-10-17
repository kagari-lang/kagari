import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const extension = new URL('../../vscode/', import.meta.url);
const json = async (name) => JSON.parse(await readFile(new URL(name, extension), 'utf8'));

test('manifest is a declarative Kagari language contribution without an executable client', async () => {
  const manifest = await json('package.json');
  assert.equal(manifest.name, 'kagari-language');
  assert.equal(manifest.publisher, 'kagari-lang');
  assert.equal(manifest.license, 'MIT OR Apache-2.0');
  assert.equal(manifest.engines.vscode, '^1.142.0');
  for (const field of ['main', 'browser', 'activationEvents', 'dependencies', 'devDependencies']) {
    assert.equal(manifest[field], undefined, field);
  }
  assert.deepEqual(manifest.contributes.languages.map(({ id, extensions }) => ({ id, extensions })),
    [{ id: 'kagari', extensions: ['.kgr'] }]);
  const [contribution] = manifest.contributes.grammars;
  assert.equal(contribution.language, 'kagari');
  assert.equal(contribution.scopeName, 'source.kagari');
  // A blanket tokenTypes override would also override nested strings/comments in holes.
  assert.equal(contribution.tokenTypes, undefined);
  const grammar = await json(contribution.path);
  assert.equal(grammar.scopeName, contribution.scopeName);
  await json(manifest.contributes.languages[0].configuration);
  assert.deepEqual(Object.keys(manifest.contributes).sort(), ['grammars', 'languages']);
});

test('editing configuration pairs only supported delimiters and respects lexical categories', async () => {
  const configuration = await json('language-configuration.json');
  assert.deepEqual(configuration.comments, { lineComment: '//', blockComment: ['/*', '*/'] });
  assert.deepEqual(configuration.brackets, [['{', '}'], ['[', ']'], ['(', ')']]);
  assert.deepEqual(configuration.autoClosingPairs.map(({ open, close }) => [open, close]),
    [['{', '}'], ['[', ']'], ['(', ')'], ['"', '"']]);
  for (const pair of configuration.autoClosingPairs) {
    assert.deepEqual(pair.notIn, ['string', 'comment']);
  }
  assert.deepEqual(configuration.surroundingPairs,
    [['{', '}'], ['[', ']'], ['(', ')'], ['"', '"']]);
});

test('packaging uses a payload allowlist and repository-consistent license texts', async () => {
  const ignore = await readFile(new URL('.vscodeignore', extension), 'utf8');
  const rules = ignore.split(/\r?\n/).filter((line) => line && !line.startsWith('#'));
  assert.deepEqual(rules, ['**', '!package.json', '!README.md', '!LICENSE', '!LICENSE-MIT', '!LICENSE-APACHE',
    '!language-configuration.json', '!syntaxes/', '!syntaxes/kagari.tmLanguage.json']);
  const notice = await readFile(new URL('LICENSE', extension), 'utf8');
  assert.ok(notice.includes('SPDX-License-Identifier: MIT OR Apache-2.0'));
  for (const name of ['LICENSE-MIT', 'LICENSE-APACHE']) {
    assert.ok(notice.includes(name));
    assert.equal(await readFile(new URL(name, extension), 'utf8'),
      await readFile(new URL(`../../../${name}`, import.meta.url), 'utf8'));
  }
});
