import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, readdir, realpath, symlink } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { parse } from 'smol-toml';
import { artifactsDirectory, checkoutGrammar, grammarAssets, grammarSubpath,
  productDirectory, runGit, snapshotGrammar, stageZed } from '../../scripts/stage-zed.mjs';
import { buildZed } from '../../scripts/test-zed-build.mjs';

const repository = fileURLToPath(new URL('../../../', import.meta.url));
const sourceExtension = path.join(repository, 'editors', 'zed');

async function files(directory, prefix = '') {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (entry.name === '.git') continue;
    const relative = prefix + entry.name;
    if (entry.isDirectory()) result.push(...await files(path.join(directory, entry.name), relative + '/'));
    else result.push(relative);
  }
  return result.sort();
}

test('current grammar snapshot has a reproducible SHA and supports Zed-style shallow file URL fetch',
  { timeout: 120000 }, async () => {
    await mkdir(artifactsDirectory, { recursive: true });
    const directory = await mkdtemp(path.join(artifactsDirectory, 'zed staging 雪 #%-'));
    const snapshot = await snapshotGrammar({ directory });
    assert.match(snapshot.repositoryUrl, /^file:\/\//);
    assert.ok(snapshot.repositoryUrl.includes('%20') && snapshot.repositoryUrl.includes('%23'), 'Escape local Git URLs');
    assert.equal(await realpath(fileURLToPath(snapshot.repositoryUrl)), snapshot.grammarRepository);
    assert.match(snapshot.revision, /^[a-f0-9]{40}$/);
    assert.deepEqual(await files(snapshot.grammarRepository), grammarAssets.map((asset) => `${grammarSubpath}/${asset}`).sort(),
      'Snapshot only generated grammar assets, scanner and metadata: no dependencies, binaries or second query copy');
    assert.equal(runGit(snapshot.grammarRepository, ['rev-parse', 'HEAD']), snapshot.revision);
    assert.equal(runGit(snapshot.grammarRepository, ['log', '--format=%P', '-1']), '', 'Synthetic snapshot has no workspace history');
    const extensionDirectory = path.join(snapshot.directory, 'extension');
    const checkout = await checkoutGrammar({ ...snapshot, extensionDirectory });
    for (const asset of grammarAssets) {
      assert.deepEqual(await readFile(path.join(checkout, asset)),
        await readFile(path.join(repository, grammarSubpath, asset)), `Zed-style fetch checks out current ${asset}`);
    }
    const checkoutRepository = path.join(extensionDirectory, 'grammars', 'kagari');
    assert.equal(runGit(checkoutRepository, ['rev-parse', 'HEAD']), snapshot.revision);
    assert.equal(runGit(checkoutRepository, ['rev-parse', '--is-shallow-repository']), 'true');
    assert.equal(runGit(checkoutRepository, ['remote', 'get-url', 'origin']), snapshot.repositoryUrl);
    const second = await snapshotGrammar();
    assert.equal(second.revision, snapshot.revision, 'Identical bytes yield the same snapshot SHA regardless of output directory');
  });

test('staged extension preserves its monorepo path and copies queries from their single owner',
  { timeout: 60000 }, async () => {
    const before = await readFile(path.join(sourceExtension, 'extension.toml'));
    const sourceManifest = parse(before.toString());
    const stage = await stageZed();
    const manifest = parse(await readFile(stage.manifestPath, 'utf8'));
    const grammar = manifest.grammars.kagari;
    assert.equal(grammar.repository, stage.repositoryUrl);
    assert.equal(grammar.path, grammarSubpath);
    assert.equal(grammar.rev, stage.revision);
    assert.match(grammar.rev, /^[a-f0-9]{40}$/);
    assert.deepEqual({ ...grammar, repository: sourceManifest.grammars.kagari.repository,
      rev: sourceManifest.grammars.kagari.rev }, { ...sourceManifest.grammars.kagari });
    assert.deepEqual({ ...manifest, grammars: sourceManifest.grammars }, { ...sourceManifest },
      'Staging only rewrites the grammar repository and revision');
    assert.deepEqual(await readFile(path.join(sourceExtension, 'extension.toml')), before,
      'Do not put local paths or synthetic revisions in the maintained manifest');
    await readFile(path.join(stage.grammarRepository, grammar.path, 'src', 'parser.c'));
    const sourceLanguages = path.join(sourceExtension, 'languages', 'kagari');
    const expectedLanguageFiles = (await readdir(sourceLanguages))
      .filter((name) => name === 'config.toml' || name.endsWith('.scm')).sort();
    assert.ok(expectedLanguageFiles.includes('highlights.scm') && expectedLanguageFiles.includes('brackets.scm'));
    assert.deepEqual(await files(path.join(stage.extensionDirectory, 'languages', 'kagari')), expectedLanguageFiles);
    for (const name of expectedLanguageFiles) {
      assert.deepEqual(await readFile(path.join(stage.extensionDirectory, 'languages', 'kagari', name)),
        await readFile(path.join(sourceLanguages, name)), `Copy ${name} byte-for-byte from its single source`);
    }
  });

test('staging cannot overwrite source, escape through symlinks, or replace an existing build product', async () => {
  await assert.rejects(productDirectory(sourceExtension, 'unused-'), /below/);
  await assert.rejects(productDirectory(artifactsDirectory, 'unused-'), /below/);
  const directory = await productDirectory(undefined, 'zed-stage-guard-');
  await mkdir(path.join(directory, 'existing'));
  await assert.rejects(productDirectory(directory, 'unused-'), /empty/);
  // A Windows directory junction needs no elevated symlink privilege. The link
  // itself stays in target/editors; rejecting it must not create source children.
  const link = path.join(directory, 'source-link');
  await symlink(sourceExtension, link, process.platform === 'win32' ? 'junction' : 'dir');
  await assert.rejects(productDirectory(path.join(link, 'must-not-be-created'), 'unused-'), /below/);
});

test('optional real Zed-style WASM build (requires WASI SDK 34)', {
  skip: process.env.KAGARI_TEST_ZED_WASM !== '1' ? 'Set KAGARI_TEST_ZED_WASM=1 and WASI_SDK_PATH to opt in; no automatic download' : false,
  timeout: 180000,
}, async () => {
  const wsl = process.env.KAGARI_TEST_ZED_WSL || undefined;
  const result = await buildZed({ wsl });
  assert.ok(result.wasmBytes > 8);
  const bytes = await readFile(result.wasmPath);
  assert.equal(bytes.length, result.wasmBytes);
  const metadata = JSON.parse(await readFile(path.join(result.directory, 'build.json'), 'utf8'));
  assert.deepEqual(metadata.provenance, { hostPlatform: process.platform,
    compilerPlatform: wsl ? 'linux' : process.platform, wslDistribution: wsl ?? null });
  assert.equal(metadata.wasmSha256, createHash('sha256').update(bytes).digest('hex'));
  if (wsl) {
    assert.equal(metadata.compilerInvocation.command, 'wsl.exe');
    assert.deepEqual(metadata.compilerInvocation.args.slice(0, 3), ['--distribution', wsl, '--exec']);
    assert.ok(metadata.compilerInvocation.args[3].endsWith('/bin/clang'));
  }
});
