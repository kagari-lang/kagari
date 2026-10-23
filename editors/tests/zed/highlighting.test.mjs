import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { parse as parseToml } from 'smol-toml';
import { cases, region } from '../contracts.mjs';
import { captures, effectiveCapture, languageDirectory, overrideAt, parse, query } from './tokenize.mjs';

const config = parseToml(readFileSync(new URL('config.toml', languageDirectory), 'utf8'));
const manifest = parseToml(readFileSync(new URL('../../zed/extension.toml', import.meta.url), 'utf8'));

test('all shipped queries compile against the shipped grammar', () => {
  for (const file of ['highlights.scm', 'brackets.scm', 'overrides.scm']) assert.ok(query(file));
  assert.equal(manifest.grammars.kagari.path, 'editors/tree-sitter-kagari');
  assert.equal(manifest.language_servers, undefined);
  assert.equal(manifest.lib, undefined);
  assert.deepEqual(config.path_suffixes, ['kgr']);
  assert.equal(config.grammar, 'kagari');
  assert.deepEqual(config.line_comments, ['// ']);
  assert.deepEqual(config.block_comment, ['/*', '*/']);
  assert.equal(config.autoclose_before, ')]}', 'Allow pairing before existing closers, including an interpolation boundary');
  assert.deepEqual(config.brackets.map(pair => pair.start), ['(', '[', '{', '"', "'"]);
  for (const pair of config.brackets) assert.deepEqual(pair.not_in, ['string', 'comment']);
});

for (const fixture of cases) {
  test(`theme captures: ${fixture.name}`, () => {
    const found = captures(fixture.text);
    for (const expected of fixture.expected.filter(item => item.capture)) {
      const { start, end } = region(fixture.text, expected);
      for (let at = start; at < end; at++) {
        assert.equal(effectiveCapture(found, at), expected.capture, `${expected.needle}: expected winning @${expected.capture}`);
      }
    }
    assert.ok(found.every(({ name }) => supportedCaptures.has(name)), 'Only Zed-supported theme captures');
  });
}

const supportedCaptures = new Set(['variable', 'variable.parameter', 'type', 'function', 'enum',
  'label', 'constant', 'variant', 'property', 'attribute', 'keyword', 'boolean', 'number',
  'comment', 'comment.doc', 'string', 'string.escape', 'operator', 'punctuation.bracket',
  'punctuation.delimiter', 'punctuation.special']);

test('declaration/type captures and bracket pairs use syntax, not name guesses', () => {
  const text = '#[meta(tag = "demo")] struct Point { pub var x: i32 } impl Point { pub async fn run(self, n: i32) { f"x={n}" } }';
  const tree = parse(text);
  assert.equal(tree.rootNode.hasError, false, tree.rootNode.toString());
  const found = captures(text);
  for (const [name, value] of [['attribute', 'meta'], ['type', 'Point'], ['property', 'x'],
    ['function', 'run'], ['variable.parameter', 'n:']]) {
    assert.equal(effectiveCapture(found, text.indexOf(value)), name, `${name}: ${value}`);
  }
  const names = 'fn main() { val isize = 1i64; val usize = 1u64; isize + usize }';
  const nameCaptures = captures(names);
  for (const name of ['isize', 'usize']) {
    assert.equal(effectiveCapture(nameCaptures, names.indexOf(name)), 'variable');
    assert.equal(effectiveCapture(nameCaptures, names.lastIndexOf(name)), 'variable');
  }
  const matches = query('brackets.scm').matches(tree.rootNode);
  assert.ok(matches.length > 4);
  for (const match of matches) {
    const open = match.captures.find(capture => capture.name === 'open');
    const close = match.captures.find(capture => capture.name === 'close');
    assert.ok(open && close);
    assert.ok(open.node.startIndex < close.node.startIndex);
  }
});

test('character quote pairs and overrides remain inside interpolation code', () => {
  const text = String.raw`fn main() { f"{'🙂'} {'\u{7B_}'} tail" }`;
  const tree = parse(text);
  assert.equal(tree.rootNode.hasError, false, tree.rootNode.toString());
  const overrides = query('overrides.scm').captures(tree.rootNode);
  assert.equal(overrideAt(overrides, text.indexOf('🙂')), 'string');
  assert.equal(overrideAt(overrides, text.indexOf('7B_')), 'string');
  assert.equal(overrideAt(overrides, text.indexOf("'🙂'")), 'interpolation');
  const pairs = query('brackets.scm').matches(tree.rootNode).filter(match =>
    match.captures.some(capture => capture.node.parent.type === 'char_literal'));
  assert.equal(pairs.length, 2);
  for (const match of pairs) {
    assert.deepEqual(match.captures.map(capture => capture.node.text), ["'", "'"]);
  }
});

test('persistent statics keep variable names and runtime initializer captures distinct from const', () => {
  const text = readFileSync(new URL('../../../examples/syntax/grammar-witnesses.kgr', import.meta.url), 'utf8');
  const tree = parse(text);
  assert.equal(tree.rootNode.hasError, false, tree.rootNode.toString());
  const found = captures(text);
  for (const declaration of tree.rootNode.descendantsOfType('static_item')) {
    const keyword = declaration.children.find(node => node.type === 'static');
    assert.equal(effectiveCapture(found, keyword.startIndex), 'keyword');
    assert.equal(effectiveCapture(found, declaration.childForFieldName('name').startIndex), 'variable');
  }
  for (const [needle, capture] of [['pub(crate)', 'keyword'], ['crate)', 'keyword'],
    ['super)', 'keyword'], ['BASELINE:', 'constant'], ['i64', 'type'],
    ['value +', 'variable'], ['cached:', 'variable'], ['[1, 2]', 'punctuation.bracket']]) {
    assert.equal(effectiveCapture(found, text.indexOf(needle)), capture, needle);
  }
});

test('string/comment override ranges do not disable pairing inside interpolation code', () => {
  const text = 'fn main() { val s = f"outer {call(1)} tail"; /* comment */ }';
  const overrides = query('overrides.scm').captures(parse(text).rootNode);
  const inScope = (needle, scope) => overrideAt(overrides, text.indexOf(needle)) === scope;
  assert.ok(inScope('outer', 'string'));
  assert.ok(inScope('tail', 'string'));
  assert.ok(inScope('comment', 'comment'));
  assert.equal(inScope('call', 'string'), false);
  assert.equal(overrideAt(overrides, text.indexOf('call')), 'interpolation');
});

test('empty strings, one-character text and hole boundaries follow actual Zed cursor rules', () => {
  for (const source of ['f""', 'f"x"', 'f"{x}"', 'f"a{x}b"', 'f"{f""}"']) {
    const text = `fn main() { ${source} }`;
    const tree = parse(text);
    const found = query('overrides.scm').captures(tree.rootNode);
    const strings = tree.rootNode.descendantsOfType('interpolated_string');
    for (const node of strings) {
      assert.equal(overrideAt(found, node.startIndex + 2), 'string', source);
      assert.equal(overrideAt(found, node.endIndex - 1), 'string', source);
      assert.notEqual(overrideAt(found, node.endIndex), 'string', 'Do not extend the closed string scope outside its quote');
    }
    for (const hole of tree.rootNode.descendantsOfType('interpolation')) {
      const at = hole.startIndex + 1;
      // A nested string may start at this position, but its exclusive start does
      // not shadow the enclosing code hole until the cursor moves inside it.
      assert.equal(overrideAt(found, at), 'interpolation', source);
    }
  }
});

test('update bases retain ordinary captures and code pairing through incomplete interpolation edits', () => {
  for (const eol of ['\n', '\r\n']) {
    for (const operand of ['base', 'Default::default()', '', 'base,', '=base']) {
      const text = `fn main() { val s = f"before {Point { x: 1, ..${operand} }} tail";${eol}val after = 42; }`;
      const tree = parse(text);
      assert.equal(tree.rootNode.hasError, ['', 'base,', '=base'].includes(operand), tree.rootNode.toString());
      const found = captures(text);
      const overrides = query('overrides.scm').captures(tree.rootNode);
      for (const [needle, name] of [['x:', 'property'], ['..', 'operator'], ['tail', 'string'],
        ['after', 'variable'], ['42', 'number']]) {
        assert.equal(effectiveCapture(found, text.indexOf(needle)), name, `${operand}: ${needle}`);
      }
      if (operand === 'base' || operand === 'Default::default()') {
        const start = text.indexOf(operand);
        assert.equal(effectiveCapture(found, start), 'variable', 'An update operand is not a field name or resolved type');
        assert.equal(overrideAt(overrides, start), 'interpolation');
      }
      const brackets = query('brackets.scm').matches(tree.rootNode);
      const struct = tree.rootNode.descendantsOfType('struct_expression')[0];
      assert.ok(brackets.some(({ captures }) => captures.some(({ name, node }) => name === 'close'
        && node.endIndex === struct.endIndex)), 'The struct closer must not be mistaken for the hole closer');
    }
  }
});

test('attribute paths and interpolation delimiters win over generic names and punctuation', () => {
  const text = '#[tool::meta] fn main() { f"{x: /* debug */ ?}" }';
  const found = captures(text);
  for (const name of ['tool', 'meta']) assert.equal(effectiveCapture(found, text.indexOf(name)), 'attribute');
  for (const needle of ['{x', ': /*', '?}', '}"']) {
    assert.equal(effectiveCapture(found, text.indexOf(needle)), 'punctuation.special', needle);
  }
});
