// A small independent oracle for lexical regions. Adapters keep their own scopes/trees.
const lexical = {
  name: 'nested comments versus ordinary string text',
  text: 'fn main() { val text = "comment // { not code }"; /* outer /* nested */ still */ val after = 0xff32; }',
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
  name: 'recursive interpolation and debug suffix trivia',
  text: String.raw`fn main() { val result = f"你好 \u{41} {{}} { {
    val s = "}"; /* } /* inner */ */ f"inner {score + 2}"
  }: /* debug */ ?} tail"; val end = true; }`,
  expected: [
    { needle: '你好', category: 'string' },
    { needle: String.raw`\u{41}`, category: 'string', capture: 'string.escape' },
    { needle: '{{}}', category: 'string' },
    { needle: 'val s', category: 'code' },
    { needle: '"}"', category: 'string' },
    { needle: '/* inner */', category: 'comment' },
    { needle: 'inner ', occurrence: 1, category: 'string' },
    { needle: 'score', category: 'code', capture: 'variable' },
    { needle: '2', category: 'code', capture: 'number' },
    { needle: '/* debug */', category: 'comment' },
    { needle: 'tail', category: 'string' },
    { needle: 'true', category: 'code', capture: 'boolean' },
  ],
};
const casesForEol = (eol, label) => [
  {
    name: `unterminated ordinary text recovers (${label})`,
    text: `fn main() {${eol}val broken = "unfinished${eol}val after = 42;${eol}}`,
    expected: [
      { needle: 'unfinished', category: 'string' },
      { needle: 'val after', category: 'code' },
      { needle: '42', category: 'code', capture: 'number' },
    ],
  },
  {
    name: `unterminated interpolated text recovers (${label})`,
    text: `fn main() {${eol}val broken = f"unfinished${eol}val after = 42;${eol}}`,
    expected: [
      { needle: 'unfinished', category: 'string' },
      { needle: 'val after', category: 'code' },
      { needle: '42', category: 'code', capture: 'number' },
    ],
  },
  {
    name: `multiline expression hole stays code (${label})`,
    text: `fn main() { val s = f"before { {${eol} val code = 1;${eol} code${eol}} } after"; }`,
    expected: [
      { needle: 'before', category: 'string' },
      { needle: 'val code', category: 'code' },
      { needle: 'code', occurrence: 1, category: 'code' },
      { needle: 'after', category: 'string' },
    ],
  },
];
export const cases = [lexical, interpolation, ...casesForEol('\n', 'LF'), ...casesForEol('\r\n', 'CRLF'), {
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
