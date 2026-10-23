// A small independent oracle for lexical regions. Adapters keep their own scopes/trees.
const lexical = {
  name: 'nested comments versus ordinary string text',
  text: 'fn main() { val text = r"comment // { not code }"; /* outer /* nested */ still */ val after = 0xff32; }',
  expected: [
    { needle: 'fn', category: 'code', capture: 'keyword' },
    { needle: 'comment // { not code }', category: 'string' },
    { needle: '/* nested */', category: 'comment' },
    { needle: 'still', category: 'comment' },
    { needle: 'after', category: 'code', capture: 'variable' },
    { needle: '0xff32', category: 'code', capture: 'number' },
  ],
};
const interpolation = {
  name: 'identifier interpolation, doubled braces and escaped Unicode braces',
  text: String.raw`fn main() { val result = "你好 \u{41} {{}} {score} {score:?} tail"; val end = true; }`,
  expected: [
    { needle: '你好', category: 'string' },
    { needle: String.raw`\u{41}`, category: 'string', capture: 'string.escape' },
    { needle: '{{}}', category: 'string' },
    { needle: 'score', category: 'code', capture: 'variable' },
    { needle: 'score', occurrence: 1, category: 'code', capture: 'variable' },
    { needle: 'tail', category: 'string' },
    { needle: 'true', category: 'code', capture: 'boolean' },
  ],
};
const casesForEol = (eol, label) => [
  {
    name: 'multiline cooked text (' + label + ')',
    text: 'fn main() { val s = "first' + eol + '  {name} last' + eol + '"; val after = 42; }',
    expected: [
      { needle: 'first', category: 'string' },
      { needle: 'name', category: 'code', capture: 'variable' },
      { needle: 'last', category: 'string' },
      { needle: 'after', category: 'code', capture: 'variable' },
    ],
  },
  {
    name: 'multiline raw text (' + label + ')',
    text: 'fn main() { val s = r##"first' + eol + '  {name} \\n "# last' + eol + '"##; val after = 42; }',
    expected: [
      { needle: '{name}', category: 'string', capture: 'string' },
      { needle: String.raw`\n`, category: 'string', capture: 'string' },
      { needle: 'last', category: 'string' },
      { needle: 'after', category: 'code', capture: 'variable' },
    ],
  },
  {
    name: 'unclosed multiline raw text (' + label + ')',
    text: 'fn main() { val s = r#"unfinished' + eol + 'val after = 42;',
    expected: [
      { needle: 'unfinished', category: 'string' },
      { needle: 'val after', category: 'string' },
    ],
  },
];
const bindingPatterns = {
  name: 'destructuring and struct rest before identifier interpolation',
  text: `fn main() { val rendered = {
    val ((0, left) | (left, _), model::Point { x: renamed, .., }) = pair;
    /* rest .. and braces { } stay comments */ "inner {renamed}"
  }; "before {rendered} after"; val next = 42; }`,
  expected: [
    { needle: 'before', category: 'string' },
    { needle: 'val ((0', category: 'code' },
    { needle: 'left', category: 'code', capture: 'variable' },
    { needle: '|', category: 'code', capture: 'operator' },
    { needle: '_', category: 'code', capture: 'variable' },
    { needle: 'Point', category: 'code', capture: 'type' },
    { needle: 'x', category: 'code', capture: 'property' },
    { needle: 'renamed', category: 'code', capture: 'variable' },
    { needle: '..', category: 'code', capture: 'operator' },
    { needle: 'rest .. and braces { }', category: 'comment' },
    { needle: 'inner ', category: 'string' },
    { needle: 'renamed', occurrence: 1, category: 'code', capture: 'variable' },
    { needle: 'after', category: 'string' },
    { needle: 'next', category: 'code', capture: 'variable' },
  ],
};
const structUpdate = {
  name: 'struct update base blocks precede identifier interpolation',
  text: `fn main() { val point = Point {
    label: "inner {score}", ..{ /* } .. /* nested */ */ Point { ..make(r"}")? } }
  }; val rendered = "before {point:?} tail"; val after = 42; }`,
  expected: [
    { needle: 'before', category: 'string' },
    { needle: 'label', category: 'code', capture: 'property' },
    { needle: 'inner ', category: 'string' },
    { needle: 'score', category: 'code', capture: 'variable' },
    { needle: '..', category: 'code', capture: 'operator' },
    { needle: '/* nested */', category: 'comment' },
    { needle: 'make', category: 'code', capture: 'variable' },
    { needle: 'r"}"', category: 'string' },
    { needle: '?', category: 'code', capture: 'operator' },
    { needle: 'tail', category: 'string' },
    { needle: 'after', category: 'code', capture: 'variable' },
  ],
};
const asyncBlock = {
  name: 'async blocks retain keyword and nested lexical regions',
  text: 'async fn run() { val pending = async /* body */ { val label = r"}"; read().await }; pending.await }',
  expected: [
    { needle: 'async', occurrence: 1, category: 'code', capture: 'keyword' },
    { needle: '/* body */', category: 'comment' },
    { needle: 'r"}"', category: 'string' },
    { needle: 'await', category: 'code', capture: 'keyword' },
    { needle: 'pending', occurrence: 1, category: 'code', capture: 'variable' },
  ],
};
const characters = {
  name: 'character bindings keep braces and escaped quotes out of interpolation code',
  text: String.raw`fn main() { val c: char = '🙂'; val close = '}'; val quote = '\''; val smile = '\u{1_F642_}'; val text = "{close} {quote} {smile} tail"; val after = 42; }`,
  expected: [
    { needle: 'char', category: 'code', capture: 'type' },
    { needle: "'🙂'", category: 'string', capture: 'string' },
    { needle: "'}'", category: 'string', capture: 'string' },
    { needle: String.raw`\'`, category: 'string', capture: 'string.escape' },
    { needle: String.raw`\u{1_F642_}`, category: 'string', capture: 'string.escape' },
    { needle: 'tail', category: 'string', capture: 'string' },
    { needle: 'after', category: 'code', capture: 'variable' },
  ],
};
export const cases = [lexical, interpolation, bindingPatterns, structUpdate, asyncBlock, characters, ...casesForEol('\n', 'LF'), ...casesForEol('\r\n', 'CRLF'), {
  name: 'unterminated block comment really extends to EOF',
  text: 'fn main() {} /* outer /* inner */ fn hidden() {}',
  expected: [
    { needle: 'main', category: 'code', capture: 'function' },
    { needle: 'fn hidden', category: 'comment' },
  ],
}, {
  name: 'numeric spelling and generic/shift boundaries',
  text: 'fn main() { val xs = [1..2, 1f64, 0xff_u8, 0xff32]; val x = Token<Map<i32, [bool]>>::Empty; a >> b; }',
  expected: [
    { needle: '1f64', category: 'code', capture: 'number' },
    { needle: '0xff_u8', category: 'code', capture: 'number' },
    { needle: '0xff32', category: 'code', capture: 'number' },
    { needle: 'i32', category: 'code', capture: 'type' },
    { needle: '>>', occurrence: 1, category: 'code', capture: 'operator' },
  ],
}];

export function region(text, expected) {
  let start = -1;
  for (let occurrence = 0; occurrence <= (expected.occurrence ?? 0); occurrence++) {
    start = text.indexOf(expected.needle, start + 1);
    if (start < 0) throw new Error(`Missing oracle needle ${JSON.stringify(expected.needle)}`);
  }
  return { start, end: start + expected.needle.length };
}
