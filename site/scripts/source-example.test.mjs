import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdir, mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { readExample } from '../src/source-example.mjs';
import { siteRoot } from './files.mjs';

// Keep temporary fixtures within ignored site output, never in canonical example roots.
test('source include reads canonical sources and rejects unsafe paths', async () => {
  const example = await readExample('examples/syntax/control-flow.kgr');
  assert.equal(example.language, 'kagari');
  assert.match(example.code, /fn main\(\) -> i32/u);
  for (const path of ['../README.md', '/examples/main.kgr', 'examples/../secret.kgr', 'examples\\main.kgr', 'examples/%2e%2e/secret.kgr', 'docs/example.kgr', 'examples/README.md', 'crates/kagari/src/main.rs']) {
    await assert.rejects(readExample(path), /Disallowed source example path/u);
  }
  await assert.rejects(readExample('examples/does-not-exist.kgr'), /ENOENT/u);
});

test('source include rejects a symlink even when its target stays inside the allowed root', async () => {
  await mkdir(join(siteRoot, '.astro'), { recursive: true });
  const root = await mkdtemp(join(siteRoot, '.astro/source-include-'));
  try {
    await mkdir(join(root, 'examples/actual'), { recursive: true });
    await writeFile(join(root, 'examples/actual/main.kgr'), 'fn main() {}');
    await symlink(join(root, 'examples/actual'), join(root, 'examples/link'), process.platform === 'win32' ? 'junction' : 'dir');
    await assert.rejects(readExample('examples/link/main.kgr', root), /symlinks/u);
    assert.equal((await readExample('examples/actual/main.kgr', root)).code, 'fn main() {}');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
