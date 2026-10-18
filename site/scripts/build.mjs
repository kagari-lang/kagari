import { spawnSync } from 'node:child_process';
import { rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { distRoot, inputDigest, manifestName, outputDigest, siteRoot } from './files.mjs';

const before = await inputDigest();
// Never leave old output looking current after a failed type/content check.
await rm(join(distRoot, manifestName), { force: true });
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
if (before !== await inputDigest()) {
  throw new Error('Build inputs changed during the build; run npm run build again.');
}
await writeFile(join(distRoot, manifestName), JSON.stringify({
  inputs: before,
  output: await outputDigest(),
}, null, 2) + '\n');
console.log('Fresh build recorded; run npm run check to validate this exact output.');
