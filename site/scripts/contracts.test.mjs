import assert from 'node:assert/strict';
import test from 'node:test';
import { page, source } from '../src/links.mjs';
import { validateFences } from './fences.mjs';

test('link helpers centralize project-base and canonical source URLs', () => {
  assert.equal(page(''), '/kagari/');
  assert.equal(page('start/quickstart#read-the-complete-program'), '/kagari/start/quickstart/#read-the-complete-program');
  assert.equal(source('examples/syntax/control-flow.kgr'), 'https://github.com/kagari-lang/kagari/blob/main/examples/syntax/control-flow.kgr');
  assert.throws(() => page('../escape'));
  assert.throws(() => source('/absolute'));
});

test('parsed fence checker rejects missing/unknown languages and accepts shared aliases', () => {
  const check = validateFences();
  const file = { fail(message) { throw new Error(message); } };
  for (const lang of [undefined, 'kagarri', 'constructor']) {
    assert.throws(() => check({ type: 'root', children: [{ type: 'code', lang }] }, file), /known language/u);
  }
  for (const lang of ['kagari', 'kgr', 'rust', 'sh', 'text']) {
    check({ type: 'root', children: [{ type: 'code', lang }] }, file);
  }
});
