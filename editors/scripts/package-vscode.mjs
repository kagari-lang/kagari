import { spawnSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const extension = fileURLToPath(new URL('../vscode/', import.meta.url));
const artifacts = new URL('../../target/editors/', import.meta.url);
mkdirSync(artifacts, { recursive: true });
const result = spawnSync(process.execPath, [require.resolve('@vscode/vsce/vsce'),
  'package', '--no-dependencies', '--out', fileURLToPath(new URL('kagari-language-0.1.0.vsix', artifacts))],
{ cwd: extension, stdio: 'inherit', timeout: 120000, windowsHide: true });
if (result.error) throw result.error;
if (result.status !== 0) throw new Error(`VSIX packaging failed (${result.signal ?? result.status})`);
