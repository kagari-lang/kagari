import assert from 'node:assert/strict';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { siteRoot } from './files.mjs';
import { runCommand } from './wasm-products.mjs';

/** One explicit, finite products -> render -> exact-output check path. The product
 * builder always refreshes the source-free dependency graph before compiling both
 * release-profile modes. Tool installation remains an explicit CI/maintainer step.
 */
export async function buildPages(run = runCommand) {
  for (const [script, args, timeout] of [
    ['wasm-products.mjs', [], 960_000],
    ['build.mjs', [], 420_000],
    ['check.mjs', ['--expected-pages', '25'], 180_000],
  ]) {
    await run(process.execPath, [join(siteRoot, 'scripts', script), ...args], { timeout, mirror: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    assert.equal(process.argv.length, 2, 'Usage: npm run build:pages');
    await buildPages();
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  }
}
