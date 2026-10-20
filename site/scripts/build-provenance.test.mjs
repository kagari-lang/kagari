import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { basename, dirname, join } from 'node:path';
import test from 'node:test';
import { copyBrowserAssets, verifyBrowserAssets } from './build-provenance.mjs';
import { buildPages } from './build-pages.mjs';
import { digest, manifestName, outputDigest, rustInputFiles, siteRoot, walk } from './files.mjs';
import { assertSourceFreeGraph, repositoryRoot, sha256 } from './wasm-products.mjs';

async function temporary(t) {
  const target = join(repositoryRoot, 'target');
  await mkdir(target, { recursive: true });
  const root = await mkdtemp(join(target, 'site-build-test-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  return root;
}

async function put(root, name, bytes) {
  const path = join(root, name);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, bytes);
}

async function fixture(t) {
  const root = await temporary(t);
  const paths = {
    destination: join(root, 'dist'),
    products: join(root, 'products'),
    fixture: join(root, 'browser-basic.kbc'),
    clientRoot: join(root, 'client'),
  };
  const files = new Map();
  for (const mode of ['source', 'artifact']) {
    for (const name of ['kagari_web.js', 'kagari_web_bg.wasm', 'kagari_web.d.ts', 'kagari_web_bg.wasm.d.ts']) {
      const key = `${mode}/${name}`;
      files.set(key, Buffer.from(`fixture:${key}`));
      await put(paths.products, key, files.get(key));
    }
  }
  files.set('browser-basic.kbc', Buffer.from('native artifact fixture'));
  await writeFile(paths.fixture, files.get('browser-basic.kbc'));
  for (const name of ['client.js', 'protocol.js', 'worker.js']) await put(paths.clientRoot, name, `// ${name}\n`);
  const record = {
    recipe: { target: 'wasm32-unknown-unknown', profile: 'release', features: { source: ['default', 'source'], artifact: ['--no-default-features'] } },
    tools: { rustc: 'fixture rustc', cargo: 'fixture cargo', bindgen: 'wasm-bindgen 0.2.129', environment: {} },
    inputs: { sha256: 'fixture input identity', files: {} },
    sourceFreeGraph: 'kagari-web v0.0.0\nkagari-runtime v0.0.0\n',
    products: Object.fromEntries([...files].map(([name, bytes]) => [name, { sha256: sha256(bytes), bytes: bytes.length }])),
  };
  await put(paths.products, 'products.json', JSON.stringify(record));
  await copyBrowserAssets({ record, files }, paths.destination, paths.clientRoot);
  return { paths, record, files };
}

test('verified snapshot deploys only exact source runtime and raw Worker modules; checking is tool-free', async (t) => {
  const { paths, record } = await fixture(t);
  await verifyBrowserAssets(record, paths);
  assert.equal((await walk(paths.destination)).length, 5);
  const script = `import { verifyBrowserAssets } from ${JSON.stringify(new URL('./build-provenance.mjs', import.meta.url).href)}; await verifyBrowserAssets(${JSON.stringify(record)}, ${JSON.stringify(paths)});`;
  const result = spawnSync(process.execPath, ['--input-type=module', '--eval', script], {
    env: { ...process.env, PATH: '', Path: '' }, encoding: 'utf8', timeout: 10_000,
  });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stderr);
});

test('checked product contract rejects missing, stale, tampered and excess assets', async (t) => {
  const cases = [
    ['missing provenance', async ({ paths }) => rm(join(paths.products, 'products.json'))],
    ['changed tool identity', async ({ record }) => { record.tools.bindgen = 'wasm-bindgen other'; }],
    ['changed target/features', async ({ record }) => { record.recipe.features.source = []; }],
    ['missing source wasm', async ({ paths }) => rm(join(paths.destination, 'wasm/source/kagari_web_bg.wasm'))],
    ['tampered deployed glue', async ({ paths }) => put(paths.destination, 'wasm/source/kagari_web.js', 'tampered')],
    ['tampered canonical artifact', async ({ paths }) => put(paths.products, 'artifact/kagari_web_bg.wasm', 'tampered')],
    ['missing canonical source', async ({ paths }) => rm(join(paths.products, 'source/kagari_web.js'))],
    ['tampered native fixture', async ({ paths }) => writeFile(paths.fixture, 'tampered')],
    ['stale Worker source', async ({ paths }) => put(paths.clientRoot, 'worker.js', 'changed source')],
    ['tampered deployed client', async ({ paths }) => put(paths.destination, 'wasm/playground/client.js', 'tampered')],
    ['unexpected deployed artifact', async ({ paths }) => put(paths.destination, 'wasm/artifact/extra.wasm', 'extra')],
  ];
  for (const [name, mutate] of cases) {
    await t.test(name, async (child) => {
      const state = await fixture(child);
      await mutate(state);
      await assert.rejects(verifyBrowserAssets(state.record, state.paths));
    });
  }
});

test('output digest covers additions, removal and bytes, excluding only the root build record', async (t) => {
  const root = await temporary(t);
  await put(root, 'index.html', 'first');
  const original = await outputDigest(root);
  await put(root, manifestName, 'record');
  assert.equal(await outputDigest(root), original);
  await put(root, 'index.html', 'other');
  assert.notEqual(await outputDigest(root), original);
  await put(root, 'index.html', 'first');
  await put(root, `nested/${manifestName}`, 'unexpected file');
  assert.notEqual(await outputDigest(root), original);
  await rm(join(root, 'nested'), { recursive: true });
  assert.equal(await outputDigest(root), original);
  await rm(join(root, 'index.html'));
  assert.notEqual(await outputDigest(root), original);
});

test('Rust provenance closure tracks new/removed sources, lock/config/toolchain files without Cargo', async (t) => {
  const root = await temporary(t);
  await put(root, 'crates/example/src/lib.rs', 'pub fn example() {}');
  await put(root, 'Cargo.toml', '[workspace]');
  await put(root, 'Cargo.lock', '# locked');
  const current = async () => digest(await rustInputFiles(root), root);
  const original = await current();
  for (const name of ['crates/example/src/new.rs', 'crates/example/input.kgr', 'crates/new/Cargo.toml', '.cargo/config.toml', 'rust-toolchain.toml']) {
    await put(root, name, 'new input');
    assert.notEqual(await current(), original, name);
    await rm(join(root, name));
    assert.equal(await current(), original, name);
  }
  await put(root, 'Cargo.lock', '# changed');
  assert.notEqual(await current(), original);
  await put(root, 'Cargo.lock', '# locked');
  await rm(join(root, 'crates/example/src/lib.rs'));
  assert.notEqual(await current(), original);
});

test('bounded Pages orchestration builds fresh products then site then checks 25 routes once', async () => {
  const calls = [];
  await buildPages(async (...args) => { calls.push(args); });
  assert.deepEqual(calls, [
    [process.execPath, [join(siteRoot, 'scripts/wasm-products.mjs')], { timeout: 960_000, mirror: true }],
    [process.execPath, [join(siteRoot, 'scripts/build.mjs')], { timeout: 420_000, mirror: true }],
    [process.execPath, [join(siteRoot, 'scripts/check.mjs'), '--expected-pages', '25'], { timeout: 180_000, mirror: true }],
  ]);
  for (let failing = 0; failing < calls.length; failing += 1) {
    const attempted = [];
    await assert.rejects(buildPages(async (command, args) => {
      attempted.push(basename(args[0]));
      if (attempted.length === failing + 1) throw new Error('stage failed');
    }), /stage failed/u);
    assert.deepEqual(attempted, calls.slice(0, failing + 1).map(([, args]) => basename(args[0])));
  }
});

test('product orchestration retains fresh source-free graph guards, not just cached verification', async () => {
  const source = await readFile(new URL('./wasm-products.mjs', import.meta.url), 'utf8');
  assert.match(source.slice(source.indexOf('export async function buildProducts()')), /await buildProductFiles\(discovered\.bindgen\.command\)/u);
  const builder = source.slice(source.indexOf('export async function buildProductFiles('));
  assert.match(builder, /run\('cargo', \['tree', '-p', 'kagari-web', '--no-default-features', '--target', target, '--edges', 'normal', '--prefix', 'none', '--locked'\]/u);
  const guard = builder.indexOf('assertSourceFreeGraph(tree)');
  assert(guard >= 0 && guard < builder.indexOf("['build', '-p', 'kagari-web'"), 'Check the fresh graph before compiling');
  const valid = 'kagari-web v0.0.0\nkagari-runtime v0.0.0\n';
  assertSourceFreeGraph(valid);
  for (const dependency of ['kagari-source', 'kagari-syntax', 'kagari-hir', 'kagari-package', 'kagari-compiler', 'kagari-mir', 'kagari-codegen-cranelift', 'cranelift-codegen', 'libloading']) {
    assert.throws(() => assertSourceFreeGraph(`${valid}${dependency} v0.0.0\n`), /frontend\/native tooling/u);
  }
  assert.throws(() => assertSourceFreeGraph('kagari-web v0.0.0'), /Incomplete/u);
});
