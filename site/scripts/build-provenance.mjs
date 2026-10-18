import assert from 'node:assert/strict';
import { lstat, mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, relative } from 'node:path';
import { distRoot, siteRoot, walk } from './files.mjs';
import { fixturePath, productsRoot, sha256 } from './wasm-products.mjs';

const sourceAssets = ['source/kagari_web.js', 'source/kagari_web_bg.wasm'];
const clientAssets = ['client.js', 'protocol.js', 'worker.js'];
const defaultClientRoot = join(siteRoot, 'src/playground');

async function regularFile(path) {
  const stat = await lstat(path);
  assert(stat.isFile() && !stat.isSymbolicLink(), `Not a regular browser product: ${path}`);
  return readFile(path);
}

function assertBytes(bytes, expected, name) {
  assert.deepEqual({ sha256: sha256(bytes), bytes: bytes.length }, expected, `Browser product changed: ${name}`);
}

/** Publish only the source runtime and raw Worker modules, not the source-free
 * acceptance fixture or declarations. The caller owns the verified snapshot.
 */
export async function copyBrowserAssets({ record, files }, destination = distRoot, clientRoot = defaultClientRoot) {
  for (const name of sourceAssets) {
    const bytes = files.get(name);
    assert(bytes, `Missing browser snapshot asset: ${name}`);
    assertBytes(bytes, record.products[name], name);
    const path = join(destination, 'wasm', name);
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, bytes);
  }
  await mkdir(join(destination, 'wasm/playground'), { recursive: true });
  for (const name of clientAssets) {
    await writeFile(join(destination, 'wasm/playground', name), await regularFile(join(clientRoot, name)));
  }
}

/** Offline validation of a previously verified build, not a product certification
 * entrypoint. The site's input digest covers current Rust/JS/configuration inputs;
 * this binds their build-time tool/recipe/graph identity to canonical and shipped
 * bytes. No Cargo, rustup, binding tool, network or product rebuild runs here.
 */
export async function verifyBrowserAssets(record, {
  destination = distRoot,
  products = productsRoot,
  fixture = fixturePath,
  clientRoot = defaultClientRoot,
} = {}) {
  assert(record?.products && record.tools && record.recipe && record.inputs && record.sourceFreeGraph,
    'Missing verified WASM provenance; run npm run build:pages.');
  assert.deepEqual(JSON.parse(await regularFile(join(products, 'products.json'))), record,
    'WASM provenance changed since the site build; run npm run build:pages.');
  // Validate both canonical modes and the native fixture: a checked site cannot
  // silently substitute a stale source-free acceptance product after the build.
  for (const [name, expected] of Object.entries(record.products)) {
    assert(name === 'browser-basic.kbc' || /^(source|artifact)\/kagari_web(?:\.js|\.d\.ts|_bg\.wasm(?:\.d\.ts)?)$/u.test(name),
      `Unexpected canonical browser product: ${name}`);
    const path = name === 'browser-basic.kbc' ? fixture : join(products, name);
    assertBytes(await regularFile(path), expected, name);
  }
  const wasm = join(destination, 'wasm');
  const names = (await walk(wasm)).map((path) => relative(wasm, path).replaceAll('\\', '/')).sort();
  assert.deepEqual(names, [...sourceAssets, ...clientAssets.map((name) => `playground/${name}`)].sort(),
    'Unexpected deployed browser asset inventory');
  for (const name of sourceAssets) assertBytes(await regularFile(join(wasm, name)), record.products[name], name);
  for (const name of clientAssets) {
    assert.deepEqual(await regularFile(join(wasm, 'playground', name)), await regularFile(join(clientRoot, name)),
      `Deployed Worker module differs from source: ${name}`);
  }
}
