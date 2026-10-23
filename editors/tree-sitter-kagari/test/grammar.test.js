'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const Parser = require('tree-sitter');
const Kagari = require('..');

function parse(source, oldTree) {
  const parser = new Parser();
  parser.setLanguage(Kagari);
  return parser.parse(source, oldTree);
}
function nodes(root, type) {
  const found = [];
  function visit(node) {
    if (node.type === type) found.push(node);
    for (const child of node.children) visit(child);
  }
  visit(root);
  return found;
}
function valid(source) {
  const tree = parse(source);
  assert.equal(tree.rootNode.hasError, false, tree.rootNode.toString());
  for (const type of ['unterminated_string_end', 'unterminated_raw_string', 'unterminated_block_comment']) {
    assert.equal(nodes(tree.rootNode, type).length, 0, tree.rootNode.toString());
  }
  return tree;
}

test('native binding supports default ESM imports, parsing and queries', async () => {
  const { default: ImportedParser } = await import('tree-sitter');
  const { default: ImportedKagari } = await import('tree-sitter-kagari');
  const parser = new ImportedParser();
  parser.setLanguage(ImportedKagari);
  const tree = parser.parse('fn main() { 42 }');
  const query = new ImportedParser.Query(ImportedKagari, '(function_item name: (identifier) @function)');
  assert.deepEqual(query.captures(tree.rootNode).map(c => c.node.text), ['main']);
  assert.ok(Kagari.nodeTypeInfo.some(node => node.type === 'interpolation'));
});

// Shared source fixtures, not copies. The complete inventory is deliberately CI-only.
const selected = ['literals-and-comments', 'char-literals', 'string-interpolation', 'type-inference',
  'generic-associated-types', 'bitwise', 'attributes', 'async'];
const representative = ['grammar-witnesses', 'pattern-alternatives', 'callables',
  'where-bounds', 'inline-modules', 'data-model', 'control-flow', 'associated-constants', 'ranges'];
for (const name of [...selected, ...representative]) {
  test(`shared syntax fixture: ${name}`, () => {
    const file = path.join(__dirname, '../../../examples/syntax', `${name}.kgr`);
    const tree = valid(fs.readFileSync(file, 'utf8'));
    if (name === 'callables') {
      const locals = nodes(tree.rootNode, 'function_item').filter(n => n.parent.type === 'block');
      assert.deepEqual(locals.map(n => n.childForFieldName('name').text),
        ['identity', 'make_increment', 'increment']);
    }
  });
}

test('character shapes reject multiple scalars and trivia without losing following declarations', () => {
  for (const literal of ["''", "'ab'", "'e\u0301'", "'\t'", "'\n'", "'\r'", "'a '", "'/*x*/a'", "'a/*x*/'",
    String.raw`'\x80'`, String.raw`'\u{_41}'`, String.raw`'\u{0000000}'`]) {
    const source = `fn broken() { val c = ${literal}; }\nfn after() { 42 }`;
    const tree = parse(source);
    assert.ok(tree.rootNode.hasError, literal);
    assert.ok(nodes(tree.rootNode, 'function_item').some(node => node.childForFieldName('name')?.text === 'after'),
      tree.rootNode.toString());
  }
  // Escaped scalar-domain checks belong to kagari-source, not this editing grammar.
  valid(String.raw`fn chars() { '\u{D800}'; '\u{110000}'; }`);
});

test('async blocks are distinct cold expressions with ordinary postfix and recovery', () => {
  const tree = valid('async fn run() { val make = async || 1; async /* body */ { make().await }.await? }');
  const blocks = nodes(tree.rootNode, 'async_block_expression');
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].childForFieldName('body').type, 'block');
  assert.equal(blocks[0].parent.type, 'await_expression');
  assert.equal(nodes(tree.rootNode, 'closure_expression').length, 1);
  valid('fn make() { async { async { 42 } } }');
  const broken = parse('fn make() { val pending = async; val after = 42; after }');
  assert.ok(broken.rootNode.hasError);
  assert.ok(nodes(broken.rootNode, 'binding_statement').some(n => n.childForFieldName('pattern')?.text === 'after'));
});

test('persistent statics have named typed initializers only at module scope', () => {
  const file = path.join(__dirname, '../../../examples/syntax/grammar-witnesses.kgr');
  const tree = valid(fs.readFileSync(file, 'utf8'));
  const declarations = nodes(tree.rootNode, 'static_item');
  assert.deepEqual(declarations.map(node => [node.childForFieldName('name').text,
    node.childForFieldName('type').text, node.childForFieldName('value').text, node.parent.type]), [
    ['requests', 'i64', 'BASELINE', 'source_file'],
    ['callback', 'fn(i32) -> i32', '|value: i32| value + 1', 'source_file'],
    ['cached', '[i64]', '[1, 2]', 'module_body'],
  ]);
  assert.deepEqual(declarations.map(node => node.namedChildren.find(child => child.type === 'visibility').text),
    ['pub(crate)', 'pub', 'pub(super)']);
  assert.ok(declarations[2].namedChildren.some(node => node.type === 'attribute'));

  const initialized = valid('static val cache: Map<String, Vec<i64>> = { val initial = make(); initial };');
  assert.equal(nodes(initialized.rootNode, 'static_item')[0].childForFieldName('value').type, 'block');
});

test('static declarations require a single name, explicit type and initializer', () => {
  for (const source of [
    'static count: i64 = 0;', 'static val count = 0;', 'static var count: i64;',
    'static val count: = 0;', 'static var count: i64 = ;',
    'static val (left, right): (i64, i64) = (1, 2);',
    'static val count<T>: T = make();', 'static var count: i64 = 0',
  ]) {
    assert.ok(parse(source).rootNode.hasError, source);
  }
});

test('local and associated statics are not valid module items', () => {
  for (const binding of ['val', 'var']) {
    const declaration = `static ${binding} count: i64 = 0;`;
    for (const source of [
      `fn run() { ${declaration} }`, `fn run() { val callback = || { ${declaration} }; }`,
      `trait Counter { ${declaration} }`, `impl Counter { ${declaration} }`,
      `impl Trait for Counter { ${declaration} }`, `impl<T> Counter<T> { ${declaration} }`,
      `struct Counter { ${declaration} }`, `enum Counter { ${declaration} }`,
    ]) assert.ok(parse(source).rootNode.hasError, source);
  }
  assert.ok(parse('fn static() {}').rootNode.hasError, 'static is reserved, not a function name');
  valid('fn static_value() { val static_count = 1; static_count }');
});

test('ordinary declarations retain complete patterns from the shared binding example', () => {
  const file = path.join(__dirname, '../../../examples/syntax/binding-patterns.kgr');
  const tree = valid(fs.readFileSync(file, 'utf8'));
  const declarations = nodes(tree.rootNode, 'binding_statement');
  assert.ok(declarations.every(node => node.childForFieldName('pattern')));
  const annotated = declarations.find(node => node.childForFieldName('pattern').text === '(reader, wide)');
  assert.equal(annotated.childForFieldName('pattern').type, 'tuple_pattern');
  assert.equal(annotated.namedChildren.find(node => node.type === 'type_annotation').text, ': (Read, i64)');
  assert.ok(declarations.some(node => node.childForFieldName('pattern').type === 'wildcard_pattern'));
  assert.ok(declarations.some(node => node.childForFieldName('pattern').type === 'parenthesized_pattern'));
  const struct = nodes(tree.rootNode, 'struct_pattern').find(node => node.text.startsWith('Pair'));
  assert.equal(struct.childForFieldName('type').text, 'Pair');
  assert.equal(nodes(struct, 'field_pattern')[0].childForFieldName('pattern').text, 'alias');
  assert.equal(nodes(struct, 'rest_pattern')[0].text, '..');
});

test('struct rest is final and shared by declarations, matches, loops and conditions', () => {
  valid(`fn main() {
    val Point { x, child: Point { y: renamed, .., }, .. } = value;
    var ((left, _) | (_, left), unit): ((i32, i32), ()) = input;
    val () = unit; val (single,) = input; val (grouped) = value;
    if val Point { .. } = value {}
    while val Point { x, .., } = next() {}
    for Point { x, .. } in points {}
    match value { Point { child: Point { .. }, .. } => x, _ => 0 }
  }`);
  for (const pattern of ['Point { .., x }', 'Point { .., .. }', 'Point { x, ..= }',
    '(first, ..)', '[first, ..]', 'left | right']) {
    assert.ok(parse(`fn main() { val ${pattern} = value; }`).rootNode.hasError, pattern);
  }
});

test('struct updates keep explicit fields and one terminal ordinary base expression', () => {
  for (const [operand, type] of [
    ['base', 'path'], ['Default::default()', 'call_expression'],
    ['make()?.items[0]', 'index_expression'], ['source.await', 'await_expression'],
    ['{ val next = make(); next }', 'block'],
    ['if ready { first } else { second }', 'if_expression'],
    ['match tag { _ => base }', 'match_expression'], ['loop { break base; }', 'loop_expression'],
    ['Point { ..base }', 'struct_expression'], ['|x: i32| x', 'closure_expression'],
    ['left + right * 2', 'binary_expression'], ['base as Point', 'cast_expression'],
    ['1..2', 'range_expression'], ['..end', 'range_expression'], ['..', 'range_expression'],
  ]) {
    const tree = valid(`fn main() { model::Point<i32> { x: 1, label, ..${operand} } }`);
    const struct = nodes(tree.rootNode, 'struct_expression')[0];
    assert.equal(struct.childForFieldName('type').text, 'model::Point');
    assert.deepEqual(struct.namedChildren.filter(n => n.type === 'field_initializer').map(n => n.text),
      ['x: 1', 'label']);
    const base = struct.childForFieldName('base');
    assert.equal(base.type, 'struct_update_base');
    assert.equal(base.children[0].type, '..');
    assert.equal(base.childForFieldName('value').type, type, operand);
    assert.equal(base.childForFieldName('value').text, operand);
    assert.equal(nodes(tree.rootNode, 'rest_pattern').length, 0);
  }
  const tree = valid(`fn main() {
    Point {}; Point { x, }; Point { ..base }; Point { x: ..end, ..base };
    val Point { .., } = base;
    ..base;
  }`);
  assert.equal(nodes(tree.rootNode, 'struct_update_base').length, 2);
  assert.equal(nodes(tree.rootNode, 'range_expression').length, 2,
    'Only the explicit field value and standalone expression are ranges, never the update marker');
  assert.equal(nodes(tree.rootNode, 'rest_pattern').length, 1);
});

test('malformed update placement and missing operands preserve following source', () => {
  for (const expression of ['Point { .. }', 'Point { x, .. }', 'Point { ..base, }',
    'Point { ..base, x: 1 }', 'Point { ..base, ..other }', 'Point { ..=base }',
    'Point { x ..base }']) {
    for (const eol of ['\n', '\r\n']) {
      const tree = parse(`fn main() { val broken = ${expression};${eol}val after = 42; }${eol}fn later() { 7 }`);
      assert.ok(tree.rootNode.hasError, expression);
      assert.ok(nodes(tree.rootNode, 'binding_statement').some(n => n.childForFieldName('pattern')?.text === 'after'),
        tree.rootNode.toString());
      assert.ok(nodes(tree.rootNode, 'function_item').some(n => n.childForFieldName('name')?.text === 'later'),
        tree.rootNode.toString());
    }
  }
});

test('struct update bindings precede identifier interpolation', () => {
  for (const eol of ['\n', '\r\n']) {
    const source = ['fn main() { val point = Point {',
      'label: "inner {score}", ..{ /* } .. */ Point { ..make(r"}")? } }',
      '}; val rendered = "before {point:?} tail"; val after = 42; }'].join(eol);
    const tree = valid(source);
    assert.equal(nodes(tree.rootNode, 'struct_update_base').length, 2);
    assert.equal(nodes(tree.rootNode, 'range_expression').length, 0);
    assert.equal(nodes(tree.rootNode, 'interpolation').length, 2);
  }
});

test('numbers, adjacency, identity and generic angles remain distinct', () => {
  const tree = valid(`fn main() {
    val range = 1..2;
    val float = 1f64;
    val byte = 0xff_u8;
    val hex = 0xff32;
    val bases = [0b_1010_u8, 0o_755, 1_2.5e+2_f32, 10e-2];
    val token = model::Token<Map<i32, [bool]>>::Empty;
    val marker = model::Marker<Map<i32, [bool]>> { value: 7 };
    val same = a === b || a !== b;
    a < b; a << b; a >> b; f::<Vec<Vec<i32>>>();
  }`);
  assert.deepEqual(nodes(tree.rootNode, 'integer_literal').slice(0, 4).map(n => n.text),
    ['1', '2', '0xff_u8', '0xff32']);
  assert.ok(nodes(tree.rootNode, 'float_literal').some(n => n.text === '1f64'));
  const fixed = ['1i8', '1i16', '1i32', '1i64', '1u8', '1u16', '1u32', '1u64',
    '18446744073709551615u64'];
  const integers = valid(`fn main() { val values = [${fixed.join(', ')}]; }`);
  assert.deepEqual(nodes(integers.rootNode, 'integer_literal').map(n => n.text), fixed);
  for (const literal of ['1isize', '1usize', '0o755usize', '0xff_isize']) {
    const invalid = parse(`fn main() { val value = ${literal}; }`);
    assert.ok(invalid.rootNode.hasError, `unsupported integer suffix: ${literal}`);
    assert.ok(!nodes(invalid.rootNode, 'integer_literal').some(n => n.text === literal));
  }
  assert.deepEqual(nodes(tree.rootNode, 'generic_path').map(n => [n.text,
    n.childForFieldName('name')?.text ?? null]), [
    ['model::Token<Map<i32, [bool]>>::Empty', 'Empty'],
    ['f::<Vec<Vec<i32>>>', null],
  ]);
  for (const op of ['> >', '> /* comment */ >', '< <', '< /* comment */ <']) {
    const invalid = parse(`fn main() { a ${op} b; }`);
    assert.ok(invalid.rootNode.hasError, `must not merge ${op}`);
  }
});

test('nested comments and documentation comments have separate node classes', () => {
  const tree = valid(`/// Documentation 文
//! Module docs
//// ordinary
/** documentation /* nested */ end */
/*! module docs */
/**/ /* plain /* nested */ end */
fn main() { "文" }`);
  assert.equal(nodes(tree.rootNode, 'doc_line_comment').length, 2);
  assert.equal(nodes(tree.rootNode, 'line_comment').length, 1);
  assert.equal(nodes(tree.rootNode, 'doc_block_comment').length, 2);
  assert.equal(nodes(tree.rootNode, 'block_comment').length, 2);
  const unfinished = parse('/* outer /* inner */ fn hidden() {}');
  assert.equal(nodes(unfinished.rootNode, 'unterminated_block_comment').length, 1);
  assert.equal(nodes(unfinished.rootNode, 'function_item').length, 0);
});

test('source whitespace remains trivia before nested comments', () => {
  valid('\u0085\u00a0/* nested /* inner */ end */\u2003fn main() {\u3000val x = 42;\u202f/* c */ x }');
});

test('ordinary interpolation contains names, not expressions or trivia', () => {
  const source = String.raw`fn main() { "text\u{41} {{}} {name} {name:?}"; }`;
  const tree = valid(source);
  assert.deepEqual(nodes(tree.rootNode, 'interpolation').map(n => n.childForFieldName('name').text), ['name', 'name']);
  assert.equal(nodes(tree.rootNode, 'interpolation_format').length, 1);
  for (const hole of ['name + 1', 'name.field', 'read()', 'module::VALUE', ' name', 'name ', 'name: ?', 'name:/*c*/?', 'name:x', 'true']) {
    assert.ok(parse('fn main() { "{' + hole + '}"; }').rootNode.hasError, hole);
  }
  for (const literal of ['f"old"', 'fr"old"', '"}"', String.raw`"\x80"`, String.raw`"\u{_41}"`, String.raw`"\u{0000000}"`]) {
    assert.ok(parse('fn main() { ' + literal + '; }').rootNode.hasError, literal);
  }
});

test('raw delimiter bounds and multiline ranges retain physical source spelling', () => {
  for (const eol of ['\n', '\r\n']) {
    for (const count of [0, 1, 2, 255]) {
      const h = '#'.repeat(count);
      const literal = 'r' + h + '"first' + eol + '  {name} \\n' + eol + '"' + h;
      const source = 'fn main() { ' + literal + '; val after = 42; }';
      const raw = nodes(valid(source).rootNode, 'raw_string_literal')[0];
      assert.equal(raw.text, literal);
      assert.equal(raw.startIndex, source.indexOf(literal));
      assert.equal(raw.endIndex, raw.startIndex + literal.length);
    }
    const cooked = '"first' + eol + '{name}' + eol + ' tail"';
    const tree = valid('fn main() { ' + cooked + '; }');
    assert.equal(nodes(tree.rootNode, 'interpolated_string')[0].text, cooked);
    const continued = 'fn main() { "first\\' + eol + ' \t' + eol + ' second"; }';
    valid(continued);
    const unclosed = parse('fn main() { r#"first' + eol + 'val hidden = 1;');
    assert.equal(nodes(unclosed.rootNode, 'unterminated_raw_string').length, 1);
    assert.equal(nodes(unclosed.rootNode, 'binding_statement').length, 0);
  }
  const h = '#'.repeat(256);
  assert.ok(parse('fn main() { r' + h + '"text"' + h + '; }').rootNode.hasError);
  const extra = parse('fn main() { r#"text"##; }');
  assert.equal(nodes(extra.rootNode, 'raw_string_literal')[0].text, 'r#"text"#');
  assert.ok(extra.rootNode.hasError);
  for (const literal of ['"bare\rCR"', 'r"bare\rCR"']) {
    assert.ok(parse('fn main() { ' + literal + '; }').rootNode.hasError);
  }
});

test('block-like tails and range endpoints preserve their source structure', () => {
  for (const tail of ['if true { 1 } else { 2 }', '{ 42 }', 'loop { break 42; }']) {
    const tree = valid(`fn main() { if false {} val value = 1; ${tail} }`);
    const body = tree.rootNode.namedChildren[0].childForFieldName('body');
    assert.equal(body.childForFieldName('tail')?.text, tail);
  }
  valid('fn main() { a..(b..c); (a..b)..c; a + b..c * d; 0..{42}; }');
  for (const range of ['0..', '..']) {
    const tree = valid(`fn main() { for value in ${range} { break; } }`);
    const expr = nodes(tree.rootNode, 'for_statement')[0].childForFieldName('value');
    assert.equal(expr.type, 'range_expression');
    assert.equal(expr.childForFieldName('end'), null, 'the following block belongs to the loop');
  }
  for (const expr of ['a..b..c', 'a..=b..c', 'a + ..b', 'if a..b..c {}']) {
    assert.ok(parse(`fn main() { ${expr}; }`).rootNode.hasError, expr);
  }
});

test('async methods, typed closures and pattern bars use their owning syntax', () => {
  const tree = valid(`struct S {} impl S { pub async fn run(self) { self.read().await?.field[0].await } }
    fn main() {
      val callback: fn(i32) -> i32 = |x: i32| x | 2;
      val pending = async || callback(1).await;
      match (1, 2) { (1, x) | (2, x) if x > 0 => x, _ => 0 }
    }`);
  assert.equal(nodes(tree.rootNode, 'method_item').length, 1);
  assert.equal(nodes(tree.rootNode, 'or_pattern').length, 1);
  assert.equal(nodes(tree.rootNode, 'closure_expression').length, 2);
});

test('unsupported Rust syntax and non-ASCII identifiers are not silently accepted', () => {
  for (const source of [
    'fn main() { let x = 1; }', 'fn main() { var mut x = 1; }',
    'fn main(x: dyn Trait) {}', "fn main(x: &'a T) {}",
    'fn main() { b"bytes" }', 'fn main() { f"old" }',
    'fn main() { async move { 1 } }', 'fn main() { await value; }',
    'fn 文() {}', 'fn async() {}',
  ]) assert.ok(parse(source).rootNode.hasError, source);
});

function point(source, index) {
  const lines = source.slice(0, index).split('\n');
  return { row: lines.length - 1, column: lines.at(-1).length };
}
function snapshot(node) {
  return [node.type, node.isNamed, node.isMissing, node.isError, node.startIndex, node.endIndex,
    node.startPosition, node.endPosition,
    node.children.map((child, index) => [node.fieldNameForChild(index), snapshot(child)])];
}
function editSequence(initial, edits) {
  let source = initial;
  let tree = parse(source);
  for (const [needle, replacement] of edits) {
    const startIndex = source.indexOf(needle);
    assert.notEqual(startIndex, -1, needle);
    const oldEndIndex = startIndex + needle.length;
    const updated = source.slice(0, startIndex) + replacement + source.slice(oldEndIndex);
    const newEndIndex = startIndex + replacement.length;
    tree.edit({ startIndex, oldEndIndex, newEndIndex, startPosition: point(source, startIndex),
      oldEndPosition: point(source, oldEndIndex), newEndPosition: point(updated, newEndIndex) });
    tree = parse(updated, tree);
    assert.deepEqual(snapshot(tree.rootNode), snapshot(parse(updated).rootNode), updated);
    source = updated;
  }
}

test('incremental quotes, hole/block braces and nested comment terminators agree with fresh parsing', () => {
  for (const eol of ['\n', '\r\n']) {
    editSequence(`mod state {${eol}static var count: i64 = 0;${eol}fn after() { count }${eol}}`, [
      ['static var', 'static val'], [': i64', ':'], [': =', ': i64 ='],
      ['= 0;', '= ;'], ['= ;', '= { val initial = 1; initial };'],
      ['static val', 'val'], ['val count', 'static val count'],
    ]);
    editSequence(['fn main() {', 'val s = "文🙂";', 'val after = 42;', '}'].join(eol), [
      ['文🙂"', '文🙂'], ['文🙂', '文🙂"'], ['"文🙂"', '"文🙂 {after}"'],
      ['{after}', '{{after}'], ['{{after}', '{after}'],
      ['{after}', '{after:?}'], ['{after:?}', '{after: ?}'], ['{after: ?}', '{after:?}'],
    ]);
    editSequence(`fn main() {${eol}val (left, Point { x: renamed, .. }) = input;${eol}val after = 42; }`, [
      ['.. }', '. }'], ['. }', '.. }'], ['.. })', '.. )'], ['.. )', '.. })'],
      ['(left, Point', '((left, _) | (_, left), Point'],
    ]);
    editSequence(['fn main() { val point = Point { label: "inner {score}", ..base };',
      '"outer {point} tail"; val after = 42; }'].join(eol), [
      ['..base', '..'], ['.. };', '..base };'],
      ['..base', '..base,'], ['..base,', '..base'],
      ['inner {score}"', 'inner {score}'], ['inner {score}', 'inner {score}"'],
    ]);
    editSequence(`/* outer /* inner */ end */${eol}fn main() { 42 }`, [
      ['inner */', 'inner'], ['inner', 'inner */'], ['end */', 'end'], ['end', 'end */'],
    ]);
    editSequence(['fn main() { val raw = r##"first', String.raw`{name} \n "# last`, '"##; val after = 42; }'].join(eol), [
      ['r##"', 'r#"'], ['r#"', 'r##"'],
      ['"##;', '"#;'], ['"#;', '"##;'],
      ['{name}', '{name:?}'], ['{name:?}', '{name}'],
    ]);
  }
});
