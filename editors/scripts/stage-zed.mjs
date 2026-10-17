import { spawnSync } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, readdir, realpath, writeFile, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parse, stringify } from 'smol-toml';

const repository = fileURLToPath(new URL('../../', import.meta.url));
export const artifactsDirectory = path.join(repository, 'target', 'editors');
export const grammarSubpath = 'editors/tree-sitter-kagari';
export const grammarAssets = [
  'tree-sitter.json',
  'src/parser.c', 'src/scanner.c', 'src/grammar.json', 'src/node-types.json',
  'src/tree_sitter/alloc.h', 'src/tree_sitter/array.h', 'src/tree_sitter/parser.h',
];

// Snapshot Git must neither inherit the worktree's index nor invoke user hooks,
// signing, line-ending conversion, or configuration-dependent clean filters.
export function runGit(directory, args) {
  const env = { ...process.env };
  for (const name of Object.keys(env)) if (name.startsWith('GIT_')) delete env[name];
  Object.assign(env, {
    GIT_CONFIG_NOSYSTEM: '1', GIT_ATTR_NOSYSTEM: '1',
    GIT_CONFIG_GLOBAL: process.platform === 'win32' ? 'NUL' : '/dev/null',
    GIT_TERMINAL_PROMPT: '0', GIT_EDITOR: 'true',
    GIT_AUTHOR_NAME: 'Kagari grammar snapshot', GIT_AUTHOR_EMAIL: 'snapshot@kagari.invalid',
    GIT_COMMITTER_NAME: 'Kagari grammar snapshot', GIT_COMMITTER_EMAIL: 'snapshot@kagari.invalid',
    GIT_AUTHOR_DATE: '2000-01-01T00:00:00Z', GIT_COMMITTER_DATE: '2000-01-01T00:00:00Z',
  });
  const result = spawnSync('git', ['--no-pager', '-c', 'core.autocrlf=false',
    '-c', 'core.safecrlf=false', '-c', 'core.filemode=false', '-c', 'core.hooksPath=', '-c', 'commit.gpgsign=false', ...args], {
    cwd: directory, env, encoding: 'utf8', timeout: 30000, windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`git ${args.join(' ')} failed: ${result.stderr}`);
  return result.stdout.trim();
}

/** Reserve an empty build-product directory, never a source or user-profile path. */
export async function productDirectory(directory, prefix) {
  await mkdir(artifactsDirectory, { recursive: true });
  const base = await realpath(artifactsDirectory);
  if (!directory) return mkdtemp(path.join(base, prefix));
  const resolved = path.resolve(directory);
  const relative = path.relative(base, resolved);
  if (!relative || relative.startsWith(`..${path.sep}`) || relative === '..' || path.isAbsolute(relative)) {
    throw new Error(`Build products must be below ${base}`);
  }
  // Resolve the closest existing ancestor before creating any descendants, so
  // a directory symlink cannot redirect writes outside the artifact tree.
  let ancestor = resolved;
  for (;;) {
    try {
      const ancestorRelative = path.relative(base, await realpath(ancestor));
      if (ancestorRelative === '..' || ancestorRelative.startsWith(`..${path.sep}`)
        || path.isAbsolute(ancestorRelative)) throw new Error('Build-product symlinks must stay below target/editors');
      break;
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      ancestor = path.dirname(ancestor);
    }
  }
  await mkdir(resolved, { recursive: true });
  const actual = await realpath(resolved);
  const actualRelative = path.relative(base, actual);
  if (!actualRelative || actualRelative.startsWith(`..${path.sep}`) || actualRelative === '..'
    || path.isAbsolute(actualRelative)) throw new Error('Build-product symlinks must stay below target/editors');
  if ((await readdir(actual)).length) throw new Error(`Build-product directory must be empty: ${actual}`);
  return actual;
}

/** Create the disposable grammar Git input independently of the editor adapter. */
export async function snapshotGrammar({ directory } = {}) {
  const output = await productDirectory(directory, 'zed-stage-');
  const grammarRepository = path.join(output, 'grammar-repository');
  const grammarDirectory = path.join(grammarRepository, grammarSubpath);
  for (const asset of grammarAssets) {
    const destination = path.join(grammarDirectory, asset);
    await mkdir(path.dirname(destination), { recursive: true });
    await copyFile(path.join(repository, grammarSubpath, asset), destination);
  }
  runGit(grammarRepository, ['init', '--initial-branch=staged', '--object-format=sha1', '--template=']);
  runGit(grammarRepository, ['add', '--', grammarSubpath]);
  const tree = runGit(grammarRepository, ['write-tree']);
  // This parentless synthetic commit exists ONLY in the disposable build product.
  const revision = runGit(grammarRepository, ['commit-tree', tree, '-m', 'Snapshot current Kagari grammar assets']);
  runGit(grammarRepository, ['update-ref', 'refs/heads/staged', revision]);
  if (!/^[0-9a-f]{40,64}$/.test(revision)) throw new Error(`Invalid resolved Git revision: ${revision}`);
  const repositoryUrl = pathToFileURL(grammarRepository).href;
  return { directory: output, grammarRepository, grammarDirectory, repositoryUrl, revision };
}

/** Stage current on-disk assets, including uncommitted generated C, for a dev install. */
export async function stageZed({ directory } = {}) {
  const source = path.join(repository, 'editors', 'zed');
  const manifest = parse(await readFile(path.join(source, 'extension.toml'), 'utf8'));
  if (manifest.grammars?.kagari?.path !== grammarSubpath) {
    throw new Error(`Expected the Kagari grammar monorepo path ${grammarSubpath}`);
  }
  const snapshot = await snapshotGrammar({ directory });
  const { directory: output, grammarRepository, grammarDirectory, repositoryUrl, revision } = snapshot;
  manifest.grammars.kagari.repository = repositoryUrl;
  manifest.grammars.kagari.rev = revision;

  const extensionDirectory = path.join(output, 'extension');
  const languageSource = path.join(source, 'languages', 'kagari');
  const languageDestination = path.join(extensionDirectory, 'languages', 'kagari');
  await mkdir(languageDestination, { recursive: true });
  const queries = (await readdir(languageSource)).filter((name) => name.endsWith('.scm')).sort();
  for (const required of ['highlights.scm', 'brackets.scm']) {
    if (!queries.includes(required)) throw new Error(`Missing source query ${required}`);
  }
  for (const name of ['config.toml', ...queries]) {
    await copyFile(path.join(languageSource, name), path.join(languageDestination, name));
  }
  const manifestPath = path.join(extensionDirectory, 'extension.toml');
  await writeFile(manifestPath, stringify(manifest));
  const result = { directory: output, extensionDirectory, grammarRepository, grammarDirectory,
    manifestPath, repositoryUrl, revision };
  await writeFile(path.join(output, 'stage.json'), JSON.stringify(result, null, 2) + '\n');
  return result;
}

// Match Zed 1.24.2's checkout_repo, including a shallow fetch of the literal SHA.
export async function checkoutGrammar(stage) {
  const checkoutDirectory = path.join(stage.extensionDirectory, 'grammars', 'kagari');
  await mkdir(path.dirname(checkoutDirectory), { recursive: true });
  await mkdir(checkoutDirectory);
  runGit(checkoutDirectory, ['init', '--template=']);
  const gitDir = path.join(checkoutDirectory, '.git');
  runGit(checkoutDirectory, ['--git-dir', gitDir, 'remote', 'add', 'origin', stage.repositoryUrl]);
  runGit(checkoutDirectory, ['--git-dir', gitDir, 'fetch', '--depth', '1', 'origin', stage.revision]);
  runGit(checkoutDirectory, ['--git-dir', gitDir, 'checkout', stage.revision]);
  return path.join(checkoutDirectory, grammarSubpath);
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  const args = process.argv.slice(2);
  if (args.length > 1 || (args[0] && !args[0].startsWith('--directory='))) {
    console.error('Usage: node scripts/stage-zed.mjs [--directory=<empty path below target/editors>]');
    process.exitCode = 1;
  } else {
    stageZed({ directory: args[0]?.slice('--directory='.length) }).then((stage) => {
      console.log(JSON.stringify(stage, null, 2));
      console.log(`Install the staged development extension: ${stage.extensionDirectory}`);
    }).catch((error) => { console.error(error); process.exitCode = 1; });
  }
}
