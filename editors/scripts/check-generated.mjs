import { createHash } from 'node:crypto';
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const editors = fileURLToPath(new URL('../', import.meta.url));
const source = new URL('../tree-sitter-kagari/src/', import.meta.url);
const headers = ['alloc.h', 'array.h', 'parser.h'];
const expected = [
  'parser.c',
  'grammar.json',
  'node-types.json',
  ...headers.map(name => `tree_sitter/${name}`),
];

function snapshot() {
  const actualHeaders = readdirSync(new URL('tree_sitter/', source))
    .filter(name => name.endsWith('.h')).sort();
  if (JSON.stringify(actualHeaders) !== JSON.stringify(headers)) {
    throw new Error(`Unexpected generated header inventory: ${actualHeaders.join(', ')}; expected ${headers.join(', ')}`);
  }
  return new Map(expected.map(name => [name, createHash('sha256')
    .update(readFileSync(new URL(name, source))).digest('hex')]));
}

try {
  // Hash the on-disk assets, including new files not yet tracked by Git.
  const before = snapshot();
  const npm = process.env.npm_execpath;
  const result = spawnSync(npm ? process.execPath : 'npm',
    [...(npm ? [npm] : []), 'run', 'generate', '--offline'], {
      cwd: editors,
      stdio: 'inherit',
      shell: !npm && process.platform === 'win32',
      env: { ...process.env, npm_config_offline: 'true' },
      timeout: 120_000,
      killSignal: 'SIGKILL',
    });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`Parser generation failed (${result.signal ?? result.status})`);
  }
  const after = snapshot();
  const changed = expected.filter(name => before.get(name) !== after.get(name));
  if (changed.length > 0) {
    throw new Error(`Generated parser assets are stale: ${changed.join(', ')}. Review and commit the regenerated files.`);
  }
  console.log(`Generated parser assets match (${expected.length} SHA-256 hashes; exact header inventory).`);
} catch (error) {
  console.error(`Generation consistency check failed: ${error.message}`);
  process.exitCode = 1;
}
