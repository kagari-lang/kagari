import assert from 'node:assert/strict';
import test from 'node:test';
import { cases, region } from './contracts.mjs';
import { tokenize as zed } from './zed/tokenize.mjs';
import { tokenize as vscode } from './vscode/tokenize.mjs';

for (const fixture of cases) {
  test(fixture.name, async () => {
    const adapters = { Zed: zed(fixture.text), TextMate: await vscode(fixture.text) };
    for (const expected of fixture.expected) {
      const { start, end } = region(fixture.text, expected);
      for (const [adapter, spans] of Object.entries(adapters)) {
        for (let at = start; at < end; at++) {
          const covering = spans.filter(span => span.start <= at && span.end > at);
          const lexical = covering.filter(span => span.type !== 'code');
          const actual = lexical.length ? lexical.at(-1).type : 'code';
          assert.equal(actual, expected.category, `${adapter}: ${JSON.stringify(expected.needle)} at ${at}`);
          if (adapter === 'TextMate') assert.ok(covering.length, `TextMate did not classify offset ${at}`);
        }
      }
    }
  });
}
