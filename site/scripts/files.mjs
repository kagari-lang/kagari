import { createHash } from 'node:crypto';
import { lstat, readFile, readdir } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { exampleRoots, repositoryRoot } from '../src/source-example.mjs';

export const siteRoot = fileURLToPath(new URL('../', import.meta.url));
export const distRoot = join(siteRoot, 'dist');
export const manifestName = '.checked-build.json';

export async function walk(root, excluded = new Set()) {
  const files = [];
  async function visit(directory) {
    for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
      if (excluded.has(entry.name)) continue;
      const path = join(directory, entry.name);
      if (entry.isSymbolicLink() || (await lstat(path)).isSymbolicLink()) {
        throw new Error(`Unexpected symlink in build inputs/output: ${path}`);
      }
      if (entry.isDirectory()) await visit(path);
      else if (entry.isFile()) files.push(path);
    }
  }
  await visit(root);
  return files;
}

export async function digest(files, root) {
  const hash = createHash('sha256');
  for (const path of [...files].sort()) {
    const name = relative(root, path).replaceAll('\\', '/');
    const content = await readFile(path);
    hash.update(`${name.length}:${name}:${content.length}:`);
    hash.update(content);
  }
  return hash.digest('hex');
}

export async function inputDigest() {
  const files = await walk(siteRoot, new Set(['dist', 'node_modules', '.astro', '.git']));
  files.push(join(repositoryRoot, 'editors/vscode/syntaxes/kagari.tmLanguage.json'));
  for (const root of exampleRoots) {
    files.push(...(await walk(join(repositoryRoot, root))).filter((path) => /\.(kgr|rs)$/u.test(path)));
  }
  return digest(files, repositoryRoot);
}

export async function outputDigest() {
  return digest(await walk(distRoot, new Set([manifestName])), distRoot);
}
