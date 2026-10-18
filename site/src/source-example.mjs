import { lstat, readFile, realpath } from 'node:fs/promises';
import { extname, isAbsolute, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

// Vite substitutes the config-derived root before moving this module into prerender chunks.
// Plain Node scripts resolve the same checkout directly from this source module.
export const repositoryRoot = import.meta.env?.KAGARI_REPOSITORY_ROOT ?? fileURLToPath(new URL('../../', import.meta.url));
export const exampleRoots = ['examples', 'crates/kagari/examples'];

function contained(root, path) {
  const remainder = relative(root, path);
  return remainder !== '' && !remainder.startsWith(`..${sep}`) && remainder !== '..' && !isAbsolute(remainder);
}

/** Read only regular .kgr/.rs examples, rejecting traversal and every symlink component. */
export async function readExample(path, root = repositoryRoot) {
  if (typeof path !== 'string' || !/^[\w-]+(?:\/[\w.-]+)+$/u.test(path) ||
      path.split('/').some((part) => part === '.' || part === '..') ||
      !exampleRoots.some((allowed) => path.startsWith(`${allowed}/`)) ||
      !['.kgr', '.rs'].includes(extname(path))) {
    throw new Error(`Disallowed source example path: ${path}`);
  }
  const canonicalRoot = await realpath(root);
  let cursor = canonicalRoot;
  const segments = path.split('/');
  for (const [index, segment] of segments.entries()) {
    cursor = resolve(cursor, segment);
    const stat = await lstat(cursor);
    if (stat.isSymbolicLink() || (index < segments.length - 1 ? !stat.isDirectory() : !stat.isFile())) {
      throw new Error(`Source examples cannot traverse symlinks or non-files: ${path}`);
    }
  }
  const canonicalFile = await realpath(cursor);
  const allowed = exampleRoots.find((prefix) => path.startsWith(`${prefix}/`));
  if (!contained(resolve(canonicalRoot, allowed), canonicalFile)) {
    throw new Error(`Source example escaped its allowed root: ${path}`);
  }
  return {
    path,
    code: (await readFile(canonicalFile, 'utf8')).replace(/\r\n/gu, '\n').trimEnd(),
    language: extname(path) === '.rs' ? 'rust' : 'kagari',
  };
}
