import { build } from 'esbuild';
import { cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { createRequire, isBuiltin } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const extension = fileURLToPath(new URL('../vscode/', import.meta.url));
const declarations = fileURLToPath(new URL('../../crates/kagari-stdlib/declarations/', import.meta.url));
const bundledSources = path.resolve(extension, 'dist/stdlib');
if (!bundledSources.startsWith(path.resolve(extension, 'dist') + path.sep)) {
  throw new Error('Standard source destination escaped extension dist');
}
await rm(bundledSources, { recursive: true, force: true });
await cp(declarations, bundledSources, { recursive: true, filter: (source) =>
  !path.extname(source) || source.endsWith('.kgr') });
const result = await build({
  entryPoints: [path.join(extension, 'src/extension.cjs')],
  outfile: path.join(extension, 'dist/extension.cjs'),
  bundle: true, platform: 'node', format: 'cjs', target: 'node22',
  external: ['vscode'], legalComments: 'eof', metafile: true,
});
for (const output of Object.values(result.metafile.outputs)) {
  for (const imported of output.imports) {
    if (imported.external && imported.path !== 'vscode' && !isBuiltin(imported.path)) {
      throw new Error(`Runtime dependency escaped the VSIX bundle: ${imported.path}`);
    }
  }
}

const packages = new Set();
for (const input of Object.keys(result.metafile.inputs)) {
  const normalized = `/${input.replaceAll('\\', '/')}`;
  if (!normalized.includes('/node_modules/')) continue;
  const segments = normalized.split('/node_modules/').at(-1).split('/');
  packages.add(segments[0].startsWith('@') ? segments.slice(0, 2).join('/') : segments[0]);
}
const notices = [];
for (const name of [...packages].sort()) {
  let directory = path.dirname(require.resolve(name));
  while (path.basename(directory) !== name.split('/').at(-1)) directory = path.dirname(directory);
  const manifest = JSON.parse(await readFile(path.join(directory, 'package.json'), 'utf8'));
  let license;
  for (const filename of ['LICENSE', 'LICENSE.md', 'License.txt', 'LICENSE.txt']) {
    try { license = await readFile(path.join(directory, filename), 'utf8'); break; }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  if (!license) throw new Error(`Missing bundled dependency license: ${name}`);
  notices.push(`${name}@${manifest.version} (${manifest.license})\n${license.trim()}\n`);
}
await writeFile(path.join(extension, 'dist/THIRD-PARTY-NOTICES.txt'), notices.join('\n'));
const artifacts = new URL('../../target/editors/', import.meta.url);
await mkdir(artifacts, { recursive: true });
await writeFile(new URL('vscode-bundle.json', artifacts), JSON.stringify(result.metafile, null, 2) + '\n');
console.log(`Bundled Kagari client and ${packages.size} dependency packages.`);
