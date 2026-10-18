import { spawnSync } from 'node:child_process';
import { rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { distRoot, inputDigest, manifestName, outputDigest, siteRoot } from './files.mjs';
import { copyBrowserAssets, verifyBrowserAssets } from './build-provenance.mjs';
import { snapshotProducts } from './wasm-products.mjs';

// Never leave old output looking current after failed input/product/type checks.
await rm(join(distRoot, manifestName), { force: true });
const before = await inputDigest();
// Products are explicitly built elsewhere; Astro rendering never invokes Cargo.
const snapshot = await snapshotProducts();
for (const command of ['check', 'build']) {
  if (command === 'build') await rm(distRoot, { recursive: true, force: true });
  const result = spawnSync(process.execPath, [join(siteRoot, 'node_modules/astro/bin/astro.mjs'), command], {
    cwd: siteRoot,
    env: { ...process.env, ASTRO_TELEMETRY_DISABLED: '1' },
    stdio: 'inherit',
    timeout: 180_000,
  });
  if (result.error || result.status !== 0) {
    throw new Error(`Astro ${command} failed: ${result.error?.message ?? `exit ${result.status}`}`);
  }
}
await copyBrowserAssets(snapshot);
await verifyBrowserAssets(snapshot.record);
if (before !== await inputDigest()) {
  throw new Error('Build inputs changed during the build; run npm run build again.');
}
await writeFile(join(distRoot, manifestName), JSON.stringify({
  inputs: before,
  output: await outputDigest(),
  wasm: snapshot.record,
}, null, 2) + '\n');
console.log('Fresh build recorded; run npm run check to validate this exact output.');
