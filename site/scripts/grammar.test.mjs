import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { createHighlighter } from 'shiki';
import { grammarFile, kagariGrammar } from '../src/grammar.mjs';

const highlighter = await createHighlighter({ themes: ['github-dark'], langs: [kagariGrammar] });

function tokens(code, language = 'kagari') {
  return highlighter.codeToTokens(code, { lang: language, theme: 'github-dark', includeExplanation: true })
      .tokens.flat().flatMap((token) => token.explanation);
}

function hasScope(token, scope) {
  return token.scopes.some(({ scopeName }) => scopeName === scope);
}

function scoped(all, content, scope) {
  assert(all.some((token) => token.content === content && hasScope(token, scope)), `Expected ${JSON.stringify(content)} in ${scope}`);
}

test('registration adapts only the maintained grammar name and aliases', () => {
  const maintained = JSON.parse(readFileSync(grammarFile, 'utf8'));
  assert.deepEqual(kagariGrammar, { ...maintained, name: 'kagari', aliases: ['kgr'] });
  assert.deepEqual(tokens('val count = 42;', 'kgr'), tokens('val count = 42;'));
});

test('shared grammar captures comments, strings and interpolation holes lexically', () => {
  const all = tokens('// line comment\n/* outer /* inner */ end */\nval text = "hello";\nval value = 1 + 2;\nval message = "sum {value} {{literal}}";');
  assert(all.some((token) => token.content.includes('line comment') && hasScope(token, 'comment.line.double-slash.kagari')));
  assert(all.some((token) => token.content.includes('inner') && hasScope(token, 'comment.block.kagari')));
  scoped(all, 'hello', 'string.quoted.double.kagari');
  scoped(all, '{', 'punctuation.section.interpolation.begin.kagari');
  scoped(all, 'value', 'meta.embedded.expression.kagari');
  scoped(all, 'value', 'variable.other.kagari');
  assert(all.filter((token) => token.content === 'value').every((token) => !hasScope(token, 'string.quoted.double.kagari')));
  scoped(all, '{{', 'constant.character.escape.brace.kagari');
});

test('rest patterns and struct update use lexical operators, not guessed type names', () => {
  const all = tokens('val Pair { left, .. } = value;\nval next = Pair { left: 2, ..base };\ntotal += 10;');
  assert.equal(all.filter((token) => token.content === '..' && hasScope(token, 'keyword.operator.kagari')).length, 2);
  scoped(all, '+=', 'keyword.operator.kagari');
  scoped(all, 'Pair', 'variable.other.kagari');
  assert(all.filter((token) => token.content === 'Pair').every((token) => !hasScope(token, 'entity.name.type.kagari')));
});

test('module aliases share maintained declaration highlighting with associated types', () => {
  const all = tokens('pub type Pair<T> where T: Show = (T, T);\ntrait Source { type Item; }');
  scoped(all, 'Pair', 'entity.name.type.kagari');
  scoped(all, 'Item', 'entity.name.type.kagari');
  scoped(all, 'type', 'storage.type.kagari');
  scoped(all, 'where', 'keyword.control.kagari');
  scoped(all, '=', 'keyword.operator.kagari');
});

test.after(() => highlighter.dispose());
