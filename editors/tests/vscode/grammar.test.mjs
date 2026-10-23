import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { loadGrammar, scanLines, tokenize, tokenizeDocument, tokenType } from './tokenize.mjs';

function occurrenceOffset(text, needle, occurrence = 1) {
  let offset = -1;
  for (let count = 0; count < occurrence; count++) {
    offset = text.indexOf(needle, offset + 1);
    assert.notEqual(offset, -1, `Missing occurrence ${occurrence} of ${JSON.stringify(needle)}`);
  }
  return offset;
}

function region(text, spans, needle, type, scope, occurrence = 1) {
  const start = occurrenceOffset(text, needle, occurrence);
  const end = start + needle.length;
  const selected = spans.filter((span) => span.start < end && span.end > start);
  assert.ok(selected.length, `No tokens for ${needle}`);
  for (let offset = start; offset < end; offset++) {
    if (text[offset] === '\n') continue;
    assert.ok(selected.some((span) => span.start <= offset && offset < span.end),
      `Missing UTF-16 offset ${offset} in ${needle}`);
  }
  for (const span of selected) {
    assert.equal(span.type, type, `${needle}: ${JSON.stringify(span)}`);
    if (scope) assert.ok(span.scopes.includes(scope), `${needle}: ${span.scopes.join(' ')}`);
  }
  return selected;
}

function regionsIn(text, spans, context, expected) {
  const start = occurrenceOffset(text, context);
  const local = spans.filter((span) => span.start < start + context.length && span.end > start)
    .map((span) => ({ ...span, start: span.start - start, end: span.end - start }));
  for (const [needle, scope, occurrence] of expected) {
    region(context, local, needle, 'code', scope, occurrence);
  }
}

test('character literals retain scalar spelling, escape scopes and interpolation boundaries', async () => {
  const source = String.raw`fn main() { val chars = ['a', 'é', '🙂', ' ', '\'', '\\', '\"', '\0', '\n', '\r', '\t', '\x00', '\x7F', '\u{10_FF_FF_}']; val text = f"{'}'} {'\''} tail"; }`;
  const spans = await tokenize(source);
  for (const literal of ["'a'", "'é'", "'🙂'", "' '", String.raw`'\''`, String.raw`'\\'`, String.raw`'\"'`,
    String.raw`'\0'`, String.raw`'\n'`, String.raw`'\r'`, String.raw`'\t'`, String.raw`'\x00'`, String.raw`'\x7F'`, String.raw`'\u{10_FF_FF_}'`]) {
    const found = region(source, spans, literal, 'string', 'string.quoted.single.kagari');
    assert.ok(found.every(span => !span.scopes.some(scope => scope.startsWith('invalid.'))), literal);
  }
  for (const escape of [String.raw`\'`, String.raw`\x7F`, String.raw`\u{10_FF_FF_}`]) {
    region(source, spans, escape, 'string', 'constant.character.escape.kagari');
  }
  region(source, spans, 'tail', 'string');
  const example = await readFile(new URL('../../../examples/syntax/char-literals.kgr', import.meta.url), 'utf8');
  region(example, await tokenize(example), "'🙂'", 'string', 'string.quoted.single.kagari');
});

test('invalid character shapes are marked and incomplete characters recover at LF and CRLF', async () => {
  for (const content of ['ab', 'e\u0301', '\t', String.raw`\x80`, String.raw`\u{_41}`, String.raw`\u{0000000}`]) {
    const text = `fn main() { val c = '${content}'; val after = 42; }`;
    const spans = await tokenize(text);
    assert.ok(spans.some(span => span.scopes.includes('invalid.illegal.character.kagari')), content);
    region(text, spans, 'after', 'code');
  }
  for (const eol of ['\n', '\r\n']) {
    const text = `fn main() { val c = 'unfinished${eol}val after = 42; }`;
    const spans = await tokenize(text);
    region(text, spans, 'unfinished', 'string');
    region(text, spans, 'after', 'code');
  }
});

test('recursive comments, documentation and Unicode survive LF and CRLF', async () => {
  for (const newline of ['\n', '\r\n']) {
    const text = ['/// 中文😀', '/** doc /* nested /** deeper */ nested */ doc */',
      'val code = 1; // tail', '/* open', '/* nested */', 'still comment'].join(newline);
    const spans = await tokenize(text);
    region(text, spans, '中文😀', 'comment', 'comment.line.documentation.kagari');
    region(text, spans, 'deeper', 'comment', 'comment.block.documentation.kagari');
    region(text, spans, 'doc */', 'comment', 'comment.block.documentation.kagari');
    region(text, spans, 'val', 'code', 'storage.type.kagari');
    region(text, spans, 'tail', 'comment', 'comment.line.double-slash.kagari');
    region(text, spans, 'still comment', 'comment', 'comment.block.kagari');
  }
  const text = '/**/ val after = 1; /* ordinary /** doc */ outer */';
  const spans = await tokenize(text);
  region(text, spans, 'after', 'code');
  region(text, spans, 'outer', 'comment', 'comment.block.kagari');
});

test('nested interpolation balances blocks, strings, comments and nested f-strings', async () => {
  const text = String.raw`val s = f"text {{literal}} \u{7b} { { val x = "}"; /* } /* { */ } */ f"inner {value}" } } tail";`;
  const spans = await tokenize(text);
  region(text, spans, 'text ', 'string');
  region(text, spans, '{{literal}}', 'string');
  region(text, spans, String.raw`\u{7b}`, 'string', 'constant.character.escape.unicode.kagari');
  region(text, spans, 'val x', 'code', 'meta.embedded.expression.kagari');
  region(text, spans, '"}"', 'string', 'string.quoted.double.kagari');
  region(text, spans, '/* } /* { */ } */', 'comment');
  region(text, spans, 'inner ', 'string');
  region(text, spans, 'value', 'code', 'meta.embedded.expression.kagari');
  region(text, spans, ' tail', 'string');
  for (const span of spans.filter((span) => span.type === 'code')) {
    assert.ok(!span.scopes.some((scope) => scope.startsWith('string.')), span.scopes.join(' '));
  }
});

test('debug colon/trivia/question and closure annotations/propagation remain code', async () => {
  const text = 'f"{value: /* debug /* nested */ end */ ?} {(|x: i32| x)(value)?} done"';
  const spans = await tokenize(text);
  region(text, spans, ':', 'code', 'punctuation.separator.kagari');
  region(text, spans, 'debug', 'comment');
  region(text, spans, '?', 'code', 'keyword.operator.kagari');
  region(text, spans, 'x: i32', 'code', 'meta.embedded.expression.kagari');
  region(text, spans, '?', 'code', 'keyword.operator.kagari', 2);
  region(text, spans, ' done', 'string');
});

test('ordinary and interpolated text recover at physical LF/CRLF, holes may span lines', async () => {
  for (const newline of ['\n', '\r\n']) {
    for (const opening of ['"', 'f"']) {
      for (const trailing of ['', '\\']) {
        const text = `val s = ${opening}unfinished${trailing}${newline}val after = 1;`;
        const spans = await tokenize(text);
        region(text, spans, 'unfinished', 'string');
        region(text, spans, 'val after', 'code');
      }
    }
    const text = ['val s = f"before {', '  /* multiline', '     } */',
      '  f"inner {', '    "}"', '  } text"', '} after";', 'val next = 2;'].join(newline);
    const spans = await tokenize(text);
    region(text, spans, 'before ', 'string');
    region(text, spans, 'multiline', 'comment');
    region(text, spans, '"}"', 'string');
    region(text, spans, ' text', 'string');
    region(text, spans, ' after', 'string');
    region(text, spans, 'val next', 'code');
    const debug = ['f"{value:', '  // a } and a " do not end the hole',
      '  /* suffix trivia */ ?', '} tail"'].join(newline);
    const debugSpans = await tokenize(debug);
    region(debug, debugSpans, 'a } and a "', 'comment');
    region(debug, debugSpans, 'suffix trivia', 'comment');
    region(debug, debugSpans, '?', 'code', 'meta.embedded.expression.kagari');
    region(debug, debugSpans, ' tail', 'string');
    const recovery = ['f"{', 'f"broken', 'value', '} tail"', 'val end = 1;'].join(newline);
    const recovered = await tokenize(recovery);
    region(recovery, recovered, 'broken', 'string');
    region(recovery, recovered, 'value', 'code', 'meta.embedded.expression.kagari');
    region(recovery, recovered, ' tail', 'string');
    region(recovery, recovered, 'val end', 'code');
  }
});

test('Unicode escapes never open holes, including malformed escapes', async () => {
  const text = String.raw`f"\u{41} \u{not{a}hole} \q {{}} {value} end"`;
  const spans = await tokenize(text);
  region(text, spans, String.raw`\u{41}`, 'string', 'constant.character.escape.unicode.kagari');
  region(text, spans, String.raw`\u{not{a}`, 'string', 'invalid.illegal.escape.kagari');
  region(text, spans, 'hole', 'string');
  region(text, spans, String.raw`\q`, 'string', 'invalid.illegal.escape.kagari');
  region(text, spans, '{{}}', 'string', 'constant.character.escape.brace.kagari');
  region(text, spans, 'value', 'code');
});

test('numeric bases, suffixes, separators and ranges follow lexer boundaries', async () => {
  const integers = ['0', '1_000', '255_u8', '128i8', '0b_10_', '0o755u64', '0xff_u8', '0xff32',
    '0xff64', '18446744073709551615u64'];
  const floats = ['1f64', '1.25_f32', '6.02e23', '1e+2', '1_2.5e0', '1_0e-2_f64'];
  const invalid = ['0xgg', '0b2', '1u128', 'version1', '1isize', '1usize', '0o755usize', '0xff_isize'];
  const text = [...integers, ...floats, '1..2', '1..=2', '1.foo', ...invalid].join(' ');
  const spans = await tokenize(text);
  for (const [numbers, scope] of [[integers, 'constant.numeric.integer.kagari'],
    [floats, 'constant.numeric.float.kagari']]) {
    for (const number of numbers) {
      const start = text.indexOf(number);
      const token = spans.find((span) => span.start === start && span.end === start + number.length);
      assert.ok(token?.scopes.includes(scope), `${number}: ${JSON.stringify(token)}`);
      assert.equal(token.type, 'code');
    }
  }
  region(text, spans, '..', 'code', 'keyword.operator.kagari');
  region(text, spans, '..=', 'code', 'keyword.operator.kagari');
  for (const bad of invalid) {
    const start = text.indexOf(bad);
    assert.ok(!spans.some((span) => span.start < start + bad.length && span.end > start
      && span.scopes.some((scope) => scope.startsWith('constant.numeric.'))), bad);
  }
});

test('reserved Kagari words, conservative declarations/types, attributes and no Rust rules', async () => {
  const reserved = ['as', 'async', 'await', 'break', 'const', 'continue', 'crate', 'else', 'enum',
    'false', 'fn', 'for', 'trait', 'if', 'impl', 'in', 'loop', 'match', 'mod', 'pub', 'return',
    'self', 'static', 'struct', 'super', 'true', 'use', 'val', 'var', 'type', 'where', 'while'];
  for (const word of reserved) {
    const spans = await tokenize(word);
    assert.ok(spans.some((span) => span.scopes.some((scope) =>
      /^(keyword\.|storage\.|variable\.language\.|constant\.language\.)/.test(scope))), word);
  }
  const plain = "let mut dyn ref static_value r b SomeType Constructor Vec String isize usize";
  for (const span of await tokenize(plain)) {
    assert.equal(span.type, 'code');
    assert.ok(!span.scopes.some((scope) => /^(keyword|storage|entity\.name\.type|support\.type)\./.test(scope)),
      span.scopes.join(' '));
  }
  const lifetime = await tokenize("'life");
  assert.ok(lifetime.every(span => !span.scopes.some(scope => scope.includes('lifetime'))));
  assert.ok(lifetime.some(span => span.scopes.includes('invalid.illegal.character.kagari')));
  const text = '#[tool::meta(tags = ["text", true])] struct Model<T> {}\n'
    + 'trait Family { type Item<T>; fn make(x: T) -> Item<T>; }\n'
    + 'mod child; val local: Model<i32> = Model {}; var callable: fn(i32) -> i32;';
  const spans = await tokenize(text);
  region(text, spans, 'tool::meta', 'code', 'entity.other.attribute-name.kagari');
  region(text, spans, 'text', 'string');
  region(text, spans, 'Model', 'code', 'entity.name.type.kagari');
  region(text, spans, 'Family', 'code', 'entity.name.type.kagari');
  region(text, spans, 'Item', 'code', 'entity.name.type.kagari');
  region(text, spans, 'make', 'code', 'entity.name.function.kagari');
  region(text, spans, 'child', 'code', 'entity.name.namespace.kagari');
  region(text, spans, 'Model', 'code', 'entity.name.type.kagari', 2);
  region(text, spans, 'Model', 'code', 'variable.other.kagari', 3);
});

test('persistent statics use lexical storage scopes without treating val objects as constants', async () => {
  const text = await readFile(new URL('../../../examples/syntax/grammar-witnesses.kgr', import.meta.url), 'utf8');
  const spans = await tokenize(text);
  for (const [occurrence, binding, name] of [[1, 'var', 'requests'], [2, 'val', 'callback'], [3, 'val', 'cached']]) {
    region(text, spans, 'static', 'code', 'storage.modifier.kagari', occurrence);
    regionsIn(text, spans, `static ${binding} ${name}`, [
      ['static', 'storage.modifier.kagari'], [binding, 'storage.type.kagari'], [name, 'variable.other.kagari'],
    ]);
  }
  regionsIn(text, spans, 'pub(crate) static var requests', [
    ['pub', 'storage.modifier.kagari'], ['crate', 'variable.language.kagari'],
    ['(', 'punctuation.separator.kagari'], [')', 'punctuation.separator.kagari'],
  ]);
  const quoted = '// static var count: i64 = 0;\nstatic val label: String = "static val";';
  const quotedSpans = await tokenize(quoted);
  region(quoted, quotedSpans, 'static', 'comment', undefined, 1);
  region(quoted, quotedSpans, 'static', 'code', 'storage.modifier.kagari', 2);
  region(quoted, quotedSpans, 'static', 'string', undefined, 3);
});

test('runnable binding patterns retain lexical scopes without guessing types or constructors', async () => {
  const text = await readFile(new URL('../../../examples/syntax/binding-patterns.kgr', import.meta.url), 'utf8');
  const spans = await tokenize(text);
  const storage = 'storage.type.kagari';
  const variable = 'variable.other.kagari';
  const separator = 'punctuation.separator.kagari';
  const operator = 'keyword.operator.kagari';
  const bindings = [
    ['val (x, Pair { first: alias, .. })', [
      ['val', storage], ['(', separator], ['x', variable], [',', separator], ['Pair', variable],
      ['{', 'punctuation.section.block.begin.kagari'], ['first', variable], [':', separator],
      ['alias', variable], ['..', operator], ['}', 'punctuation.section.block.end.kagari'], [')', separator],
    ]],
    ['var (total, step)', [ ['var', storage], ['total', variable], [',', separator], ['step', variable] ]],
    ['val ((0, first) | (first, _))', [
      ['val', storage], ['((', separator], ['0', 'constant.numeric.integer.kagari'],
      ['first', variable], ['|', operator], ['first', variable, 2], ['_', variable], ['))', separator],
    ]],
    ['var ((Choice { first: chosen, second: 0 } | Choice { first: _, second: chosen }), _)', [
      ['var', storage], ['Choice', variable], ['chosen', variable], ['|', operator],
      ['Choice', variable, 2], ['_', variable], ['chosen', variable, 2], ['_', variable, 2],
    ]],
    ['val (reader, wide): (Read, i64)', [
      ['reader', variable], ['wide', variable], [':', separator], ['Read', variable], ['i64', variable],
    ]],
    ['val _ =', [ ['val', storage], ['_', variable], ['=', operator] ]],
    ['val (None, LIMIT)', [ ['None', variable], ['LIMIT', variable] ]],
  ];
  for (const [binding, expected] of bindings) regionsIn(text, spans, binding, expected);
});

test('grouped/unit/singleton and struct rest patterns balance inside nested interpolation', async () => {
  const source = [
    'val s = f"before { {',
    '  val (grouped) = source;',
    '  var (single,) = (grouped,);',
    '  val () = ();',
    '  val Point { x, nested: Pair { first: renamed, .., }, .. } = source;',
    '  var Point { /* } | /* { */ .. */ .., } = source;',
    '  f"inner {renamed}"',
    '} } after";',
    'val after = 1;',
  ];
  for (const newline of ['\n', '\r\n']) {
    const text = source.join(newline);
    const spans = await tokenize(text);
    region(text, spans, 'before ', 'string');
    const variable = 'variable.other.kagari';
    const separator = 'punctuation.separator.kagari';
    const operator = 'keyword.operator.kagari';
    for (const [context, expected] of [
      ['val (grouped)', [ ['val', 'storage.type.kagari'], ['(', separator], ['grouped', variable], [')', separator] ]],
      ['var (single,)', [ ['var', 'storage.type.kagari'], ['single', variable], [',)', separator] ]],
      ['val ()', [ ['()', separator] ]],
      ['val Point { x, nested: Pair { first: renamed, .., }, .. }', [
        ['Point', variable], ['x', variable], [',', separator], ['nested', variable], [':', separator],
        ['Pair', variable], ['renamed', variable], ['..', operator], [',', separator, 3],
        ['}', 'punctuation.section.block.end.kagari'], ['..', operator, 2],
      ]],
      ['.., } = source;', [ ['..', operator], [',', separator], ['}', 'punctuation.section.block.end.kagari'],
        ['=', operator], ['source', variable], [';', separator] ]],
    ]) regionsIn(text, spans, context, expected);
    region(text, spans, '/* } | /* { */ .. */', 'comment', 'comment.block.kagari');
    region(text, spans, 'inner ', 'string');
    region(text, spans, 'renamed', 'code', 'meta.embedded.expression.kagari', 2);
    region(text, spans, ' after', 'string');
    region(text, spans, 'val after', 'code');
    for (const span of spans.filter((span) => span.type === 'code')) {
      assert.ok(!span.scopes.some((scope) => scope.startsWith('string.')), span.scopes.join(' '));
    }
  }
});

test('struct updates stay lexical in valid and incomplete nested interpolation', async () => {
  for (const newline of ['\n', '\r\n']) {
    for (const operand of ['base', 'Default::default()', '', 'base,', '=base']) {
      const text = `fn main() { val s = f"before {Point {
        label: f"inner {score}", ..${operand}
      }} tail"; val after = 42; }`.replaceAll('\n', newline);
      const spans = await tokenize(text);
      region(text, spans, 'before ', 'string');
      region(text, spans, 'inner ', 'string');
      region(text, spans, 'score', 'code', 'variable.other.kagari');
      region(text, spans, 'Point', 'code', 'variable.other.kagari');
      region(text, spans, 'label', 'code', 'variable.other.kagari');
      region(text, spans, operand === '=base' ? '..=' : '..', 'code', 'keyword.operator.kagari');
      region(text, spans, ' tail', 'string');
      region(text, spans, 'val after', 'code');
      if (operand === 'base' || operand === 'Default::default()') {
        region(text, spans, operand === 'base' ? 'base' : 'Default', 'code', 'variable.other.kagari');
        if (operand !== 'base') region(text, spans, 'default', 'code', 'variable.other.kagari');
      }
      for (const span of spans.filter(span => span.type === 'code')) {
        assert.ok(!span.scopes.some(scope => scope.startsWith('string.')), span.scopes.join(' '));
      }
    }
  }
});

test('operators preserve adjacency without claiming generic/comparison resolution', async () => {
  const operators = ['===', '!==', '<<', '>>', '<<=', '>>=', '&=', '|=', '^=', '%=', '?', '|', '||',
    '&&', '=>', '->', '::', '..', '..=', '+', '-', '*', '/', '%', '<=', '>='];
  for (const operator of operators) {
    const text = `left ${operator} right`;
    const spans = await tokenize(text);
    const tokens = region(text, spans, operator, 'code', 'keyword.operator.kagari');
    assert.equal(tokens.length, 1, operator);
  }
  for (const text of ['A<B<C>>::Empty', 'a > > b', 'a > /* gap */ > b']) {
    const spans = await tokenize(text);
    for (const span of spans) {
      if (span.type === 'code' && text.slice(span.start, span.end).includes('>')) {
        assert.ok(!/\s|\/\*/.test(text.slice(span.start, span.end)));
      }
    }
  }
});

test('embedded expressions reset both encoded categories and string theme inheritance', async () => {
  const { grammar, registry } = await loadGrammar({ settings: [
    { settings: { foreground: '#101010', background: '#ffffff' } },
    { scope: 'string', settings: { foreground: '#aa0000' } },
    { scope: 'comment', settings: { foreground: '#008800' } },
  ] });
  try {
    const text = 'f"text {unresolved + "inner" /* note */ + f"nested {other}"} tail"';
    const { spans, rows } = await tokenizeDocument(text, grammar);
    const metadataAt = (needle) => {
      const offset = text.indexOf(needle);
      const encoded = rows[0].encoded;
      let index = 0;
      while (index + 2 < encoded.length && encoded[index + 2] <= offset) index += 2;
      return encoded[index + 1];
    };
    const foreground = (needle) => registry.getColorMap()[(metadataAt(needle) >>> 15) & 511];
    for (const needle of ['unresolved', 'other', '+']) {
      region(text, spans, needle, 'code', 'meta.embedded.expression.kagari');
      assert.equal(tokenType(metadataAt(needle)), 'code');
      assert.equal(foreground(needle), '#101010', needle);
    }
    for (const needle of ['text', 'inner', 'nested', 'tail']) {
      assert.equal(tokenType(metadataAt(needle)), 'string');
      assert.equal(foreground(needle), '#AA0000', needle);
    }
    assert.equal(tokenType(metadataAt('note')), 'comment');
    assert.equal(foreground('note'), '#008800');
  } finally {
    registry.dispose();
  }
});

test('incremental retokenization from valid preceding stacks agrees with fresh tokenization', async () => {
  const { grammar, registry } = await loadGrammar();
  const documents = [
    { text: 'val s = f"{\n "closed";\n value\n} tail";\nval after = 1;',
      from: '"closed";', to: '"open', needle: 'value', type: 'code' },
    { text: 'val s = f"{\n val Point { x, .. } = point;\n} tail";\nval after = 1;',
      from: '.. }', to: '..', needle: 'tail', type: 'code' },
    { text: 'val s = f"{\n /* outer\n /* inner */\n end */ value\n} tail";\nval after = 1;',
      from: 'end */ value', to: 'end value', needle: 'val after', type: 'comment' },
    { text: 'val s = f"{Point {\n label: f"inner {score}",\n ..base\n}} tail";\nval after = 1;',
      from: '..base', to: '..', needle: ' tail', type: 'string' },
    { text: 'val s = f"{Point {\n label: f"inner {score}",\n ..base }\n} tail";\nval after = 1;',
      from: '..base }', to: '..base', needle: ' tail', type: 'code' },
    { text: "val s = f\"{\n '\\u{7B_}';\n value\n} tail\";\nval after = 1;",
      from: "'\\u{7B_}';", to: "'\\u{7B_}", needle: 'value', type: 'code' },
    { text: '/* header\n end */\nval s = "closed";\nval after = 1;',
      from: '"closed";', to: '"open', needle: 'val after', type: 'code' },
  ];
  try {
    for (const newline of ['\n', '\r\n']) {
      for (const document of documents) {
        const original = document.text.replaceAll('\n', newline);
        const edited = original.replace(document.from, document.to);
        for (const [before, after] of [[original, edited], [edited, original]]) {
          const previous = scanLines(before.split('\n'), grammar);
          const fresh = scanLines(after.split('\n'), grammar);
          const first = previous.rows.findIndex((row, index) => row.text !== fresh.rows[index].text);
          assert.ok(first > 0, 'Edit must exercise a preceding stack, not restart at line zero');
          const prefix = previous.rows[first];
          const incremental = scanLines(after.split('\n').slice(first), grammar,
            prefix.stackBefore, prefix.offset);
          assert.deepEqual(incremental.spans, fresh.spans.filter((span) => span.start >= prefix.offset));
          for (const [index, row] of incremental.rows.entries()) {
            assert.deepEqual(row.encoded, fresh.rows[first + index].encoded);
            assert.ok(row.stackAfter.equals(fresh.rows[first + index].stackAfter));
          }
          if (after === edited) region(after, fresh.spans, document.needle, document.type);
          else region(after, fresh.spans, 'val after', 'code');
        }
      }
    }
  } finally {
    registry.dispose();
  }
});

test('selected repository examples are read directly and have meaningful capture assertions', async () => {
  const examples = [
    ['literals-and-comments.kgr', [ ['still outer', 'comment'], ['0b1010', 'code', 'constant.numeric.integer.kagari'],
      [String.raw`\u{41}`, 'string', 'constant.character.escape.unicode.kagari'] ]],
    ['string-interpolation.kgr', [ ['Hello ', 'string'], ['score + 2', 'code', 'meta.embedded.expression.kagari'],
      ['name', 'code', 'meta.embedded.expression.kagari', 2] ]],
    ['type-inference.kgr', [ ['Marker', 'code', 'entity.name.type.kagari'],
      ['0xff_u8', 'code', 'constant.numeric.integer.kagari'], ['1.25f32', 'code', 'constant.numeric.float.kagari'] ]],
    ['generic-associated-types.kgr', [ ['Family', 'code', 'entity.name.type.kagari'],
      ['Item', 'code', 'entity.name.type.kagari'], ['make', 'code', 'entity.name.function.kagari'] ]],
    ['bitwise.kgr', [ ['<<=', 'code', 'keyword.operator.kagari'], ['>>=', 'code', 'keyword.operator.kagari'] ]],
    ['attributes.kgr', [ ['meta', 'code', 'entity.other.attribute-name.kagari', 2], ['answer', 'string'],
      ['Tagged', 'code', 'entity.name.type.kagari'] ]],
    ['async.kgr', [ ['async', 'code', 'keyword.control.kagari'], ['read', 'code', 'entity.name.function.kagari'],
      ['await', 'code', 'keyword.control.kagari'] ]],
  ];
  for (const [name, expected] of examples) {
    const text = await readFile(new URL(`../../../examples/syntax/${name}`, import.meta.url), 'utf8');
    const spans = await tokenize(text);
    for (const [needle, type, scope, occurrence] of expected) region(text, spans, needle, type, scope, occurrence);
  }
});
