import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { writeSync } from 'node:fs';
import { lstat, mkdir, readFile, readdir, rename, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const repositoryRoot = fileURLToPath(new URL('../../', import.meta.url));
export const productsRoot = join(repositoryRoot, 'target/wasm-browser');
export const recordPath = join(productsRoot, 'products.json');
export const fixturePath = join(repositoryRoot, 'target/fixtures/browser-basic.kbc');
const target = 'wasm32-unknown-unknown';
const productNames = ['kagari_web.js', 'kagari_web_bg.wasm', 'kagari_web.d.ts', 'kagari_web_bg.wasm.d.ts'];
const modes = ['source', 'artifact'];
const recipe = { version: 1, target, profile: 'release', cargoDirectory: 'release', features: { source: ['default', 'source'], artifact: ['--no-default-features'] }, bindgenTarget: 'web' };
export const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

// Internal inherited contract, not a user setting: only the outer helper creates
// a Unix process group. Descendants must not detach or strip this marker.
const inheritedGroup = 'KAGARI_COMMAND_GROUP';

/** No shell, finite output/time, and process-tree termination. On Unix, the outer
 * helper owns a group; nested helpers join it. A nested timeout/output overflow
 * terminates that entire operation (including the nested caller), not just a leaf.
 * Windows retains taskkill /T /F. Callers must await children; no tools install here.
 */
export async function runCommand(command, args, { timeout = 180_000, mirror = false } = {}) {
  assert(Number.isSafeInteger(timeout) && timeout > 0 && timeout <= 2_147_483_647,
    'Command timeout must be a positive integer within the Node timer range');
  const unix = process.platform !== 'win32';
  const ownsGroup = unix && process.env[inheritedGroup] !== '1';
  return new Promise((accept, reject) => {
    const child = spawn(command, args, {
      cwd: repositoryRoot, shell: false, windowsHide: true, detached: ownsGroup,
      env: unix ? { ...process.env, [inheritedGroup]: '1' } : process.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    const stdout = [];
    const stderr = [];
    let size = 0;
    let failure;
    let killing;
    const kill = (reason) => {
      if (killing) return;
      failure = new Error(reason);
      killing = new Promise((done) => {
        if (!child.pid) { done(); return; }
        if (process.platform === 'win32') {
          const killer = spawn('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { shell: false, windowsHide: true, stdio: 'ignore' });
          const limit = setTimeout(() => { killer.kill(); child.kill('SIGKILL'); done(); }, 5_000);
          killer.once('error', () => { clearTimeout(limit); child.kill('SIGKILL'); done(); });
          killer.once('close', () => { clearTimeout(limit); done(); });
        } else {
          if (!ownsGroup) {
            // This caller is inside the supervised group. Fail the whole operation;
            // killing only child.pid would strand its descendants. Write before
            // SIGKILL because this process cannot flush or run finally afterward.
            try { writeSync(2, `${failure.message}\n`); }
            finally { process.kill(0, 'SIGKILL'); }
          }
          try { process.kill(-child.pid, 'SIGKILL'); } catch { child.kill('SIGKILL'); }
          done();
        }
      });
    };
    const timer = setTimeout(() => kill(`${command} timed out after ${timeout}ms; child tree terminated`), timeout);
    const interrupt = () => kill(`${command} interrupted; child tree terminated`);
    process.once('SIGINT', interrupt);
    process.once('SIGTERM', interrupt);
    function cleanup() {
      clearTimeout(timer);
      process.removeListener('SIGINT', interrupt);
      process.removeListener('SIGTERM', interrupt);
    }
    for (const [stream, chunks, output] of [[child.stdout, stdout, process.stdout], [child.stderr, stderr, process.stderr]]) {
      stream.on('data', (chunk) => {
        size += chunk.length;
        if (size > 16 * 1024 * 1024) { kill(`${command} exceeded 16 MiB captured output`); return; }
        chunks.push(chunk);
        if (mirror) output.write(chunk);
      });
    }
    child.once('error', (error) => { cleanup(); reject(error); });
    child.once('close', async (code, signal) => {
      await killing;
      cleanup();
      const result = { stdout: Buffer.concat(stdout).toString('utf8'), stderr: Buffer.concat(stderr).toString('utf8') };
      if (failure || code !== 0) reject(failure ?? new Error(`${command} ${args.join(' ')} failed (${signal ?? code})${mirror ? '; see output above' : `\n${result.stderr}\n${result.stdout}`}`));
      else accept(result);
    });
  });
}

async function optionalFile(path) {
  try { return await readFile(path); }
  catch (error) { if (error.code === 'ENOENT') return null; throw error; }
}

async function tools() {
  const lock = await readFile(join(repositoryRoot, 'Cargo.lock'), 'utf8');
  const versions = [...lock.matchAll(/\[\[package\]\]\s+name = "wasm-bindgen"\s+version = "([^"]+)"/gu)].map((match) => match[1]);
  assert.equal(versions.length, 1, 'Expected one locked wasm-bindgen version');
  const version = versions[0];
  const local = join(repositoryRoot, 'target/wasm-tools/bin', process.platform === 'win32' ? 'wasm-bindgen.exe' : 'wasm-bindgen');
  const candidates = [local, 'wasm-bindgen'];
  const attempts = [];
  let bindgen;
  for (const command of candidates) {
    try {
      const actual = (await runCommand(command, ['--version'], { timeout: 10_000 })).stdout.trim();
      assert.equal(actual, `wasm-bindgen ${version}`, 'CLI must match Cargo.lock exactly');
      bindgen = { command, version: actual };
      break;
    } catch (error) { attempts.push(`${command}: ${error.message}`); }
  }
  assert(bindgen, `${attempts.join('\n')}\nInstall explicitly from the repository root (not performed by this script):\ncargo install wasm-bindgen-cli --version ${version} --locked --root target/wasm-tools`);
  assert(!process.env.CARGO_TARGET_DIR && !process.env.CARGO_BUILD_TARGET_DIR, 'Unset CARGO_TARGET_DIR/CARGO_BUILD_TARGET_DIR; this recipe uses the default target directory');
  const environment = Object.fromEntries(Object.entries(process.env).filter(([name]) => /^(RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|RUSTUP_TOOLCHAIN|RUSTC|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|CARGO_BUILD_.*|CARGO_TARGET_.*|CARGO_PROFILE_.*)$/u.test(name)).sort(([a], [b]) => a.localeCompare(b)));
  return {
    bindgen,
    identity: {
      rustc: (await runCommand('rustc', ['-Vv'], { timeout: 10_000 })).stdout.trim(),
      cargo: (await runCommand('cargo', ['-V'], { timeout: 10_000 })).stdout.trim(),
      bindgen: bindgen.version,
      environment,
    },
  };
}

/** Conservative closure: all crate Rust/manifests/Kagari inputs, including new untracked
 * adapter files. Test inputs can invalidate products; production inputs cannot be missed
 * merely because a new file has not yet been staged. No dependency on git dirtiness.
 */
async function inputs(identity) {
  const files = {};
  async function visit(directory, prefix) {
    for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
      const path = join(directory, entry.name);
      const name = `${prefix}/${entry.name}`;
      assert(!entry.isSymbolicLink(), `Symlink in Rust inputs: ${name}`);
      if (entry.isDirectory()) {
        if (!['target', '.git'].includes(entry.name)) await visit(path, name);
      } else if (entry.isFile() && (/\.(rs|kgr)$/u.test(name) || entry.name === 'Cargo.toml')) files[name] = sha256(await readFile(path));
    }
  }
  await visit(join(repositoryRoot, 'crates'), 'crates');
  for (const name of ['Cargo.toml', 'Cargo.lock', 'rust-toolchain', 'rust-toolchain.toml', '.cargo/config', '.cargo/config.toml', 'site/scripts/wasm-products.mjs']) {
    const bytes = await optionalFile(join(repositoryRoot, name));
    files[name] = bytes === null ? null : sha256(bytes);
  }
  return { sha256: sha256(JSON.stringify({ files, identity, recipe })), files };
}

function productPaths() {
  return new Map([
    ...modes.flatMap((mode) => productNames.map((name) => [`${mode}/${name}`, join(productsRoot, mode, name)])),
    ['browser-basic.kbc', fixturePath],
  ]);
}

async function readProducts() {
  const files = new Map();
  for (const [name, path] of productPaths()) {
    const stat = await lstat(path);
    assert(stat.isFile() && !stat.isSymbolicLink(), `Not a regular product: ${path}`);
    assert(stat.size <= 256 * 1024 * 1024, `Product exceeds snapshot bound: ${path}`);
    files.set(name, await readFile(path));
  }
  return files;
}

const productHashes = (files) => Object.fromEntries([...files].map(([name, bytes]) => [name, { sha256: sha256(bytes), bytes: bytes.length }]));

export function assertSourceFreeGraph(tree) {
  const forbidden = /^(?:kagari-(?:source|syntax|hir|package|compiler|mir|codegen(?:-cranelift)?|analyzer|format|lint|cli)|cranelift(?:-.*)?|libloading)$/u;
  const names = tree.trim().split(/\r?\n/u).map((line) => line.trim().split(/\s/u)[0]);
  assert(names.includes('kagari-web') && names.includes('kagari-runtime'), 'Incomplete source-free cargo tree');
  assert.deepEqual(names.filter((name) => forbidden.test(name)), [], 'Source-free production graph contains frontend/native tooling');
}

/** Verify only: never rebuild/install/write products. Throws on stale inputs or bytes. */
export async function verifyProducts() {
  let record;
  try { record = JSON.parse(await readFile(recordPath, 'utf8')); }
  catch (error) { throw new Error('Missing/invalid WASM products record. Run node scripts/wasm-products.mjs from site.', { cause: error }); }
  assert.deepEqual(record.recipe, recipe, 'WASM recipe changed; rebuild products');
  const currentTools = await tools();
  assert.deepEqual(record.tools, currentTools.identity, 'WASM toolchain/environment changed; rebuild products');
  assert.deepEqual(record.inputs, await inputs(currentTools.identity), 'Stale WASM Rust/script inputs; rebuild products');
  assertSourceFreeGraph(record.sourceFreeGraph);
  assert.deepEqual(record.products, productHashes(await readProducts()), 'WASM product bytes changed; rebuild products');
  return record;
}

/** Immutable verified bytes for a loopback server or site build; never serve paths. */
export async function snapshotProducts() {
  const record = await verifyProducts();
  const files = await readProducts();
  assert.deepEqual(productHashes(files), record.products, 'Products changed during snapshot');
  return { record, files };
}

/** Copy into the caller's WASM asset directory (e.g. dist/wasm), not its site root.
 * Returns the verified provenance and copied relative names. Does not invoke Cargo.
 */
export async function copyProducts(destination, { modes: selected = modes, includeFixture = false } = {}) {
  assert(selected.length > 0 && selected.every((mode) => modes.includes(mode)), 'Unknown WASM mode');
  const { record, files } = await snapshotProducts();
  const copied = [];
  for (const [name, bytes] of files) {
    if (!(name === 'browser-basic.kbc' ? includeFixture : selected.includes(name.split('/')[0]))) continue;
    const path = join(resolve(destination), name);
    assert(![...productPaths().values()].includes(path), 'Copy destination must not overwrite canonical products');
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, bytes);
    copied.push(name);
  }
  return { record, copied };
}

/** Finite product command recipe, separated from provenance publication. Three
 * Cargo compilations: 4m each; graph: 30s; two bindings: 30s each. Together with
 * up to nine 10s tool/target probes, commands consume at most 15m of the Pages
 * 16m products stage, leaving 1m for IO/validation/teardown. These are budgets,
 * not measured cold-host timings. Products use the standard workspace release profile and default parallelism.
 */
export async function buildProductFiles(bindgen, run = runCommand) {
  const tree = (await run('cargo', ['tree', '-p', 'kagari-web', '--no-default-features', '--target', target, '--edges', 'normal', '--prefix', 'none', '--locked'], { timeout: 30_000 })).stdout;
  assertSourceFreeGraph(tree);
  for (const mode of modes) {
    await run('cargo', ['build', '-p', 'kagari-web', ...(mode === 'artifact' ? ['--no-default-features'] : []), '--target', target, '--release', '--locked'], { timeout: 240_000, mirror: true });
    await run(bindgen, [join(repositoryRoot, `target/${target}/release/kagari_web.wasm`), '--target', 'web', '--out-dir', join(productsRoot, mode), '--out-name', 'kagari_web'], { timeout: 30_000, mirror: true });
  }
  await run('cargo', ['test', '-p', 'kagari-web', '--lib', '--locked', 'tests::generate_browser_artifact', '--', '--exact'], { timeout: 240_000, mirror: true });
  return tree;
}

export async function buildProducts() {
  await mkdir(productsRoot, { recursive: true });
  // A failed/partial build cannot leave a successful freshness claim behind.
  await rm(recordPath, { force: true });
  const discovered = await tools();
  const before = await inputs(discovered.identity);
  const installed = await runCommand('rustup', ['target', 'list', '--installed'], { timeout: 10_000 });
  assert(installed.stdout.split(/\s+/u).includes(target), `Missing target. Run explicitly: rustup target add ${target}`);
  const tree = await buildProductFiles(discovered.bindgen.command);
  const files = await readProducts();
  const sourceModule = new WebAssembly.Module(files.get('source/kagari_web_bg.wasm'));
  const artifactModule = new WebAssembly.Module(files.get('artifact/kagari_web_bg.wasm'));
  assert(WebAssembly.Module.exports(sourceModule).some(({ name }) => name === 'browsersession_compile'), 'Source product has no compile export');
  assert(!WebAssembly.Module.exports(artifactModule).some(({ name }) => /compile/iu.test(name)), 'Artifact product contains compile exports');
  assert(!/\bcompile\s*\(/u.test(files.get('artifact/kagari_web.js').toString('utf8')), 'Artifact JS exposes compile');
  assert.deepEqual(await inputs(discovered.identity), before, 'Inputs changed while building; rerun after edits settle');
  assert.deepEqual((await tools()).identity, discovered.identity, 'Toolchain changed during build');
  const record = { recipe, tools: discovered.identity, inputs: before, products: productHashes(files), sourceFreeGraph: tree, builtAt: new Date().toISOString() };
  const temporary = `${recordPath}.${process.pid}.tmp`;
  await writeFile(temporary, `${JSON.stringify(record, null, 2)}\n`);
  await rename(temporary, recordPath);
  console.log(`Verified WASM products: ${recordPath}`);
  for (const [name, product] of Object.entries(record.products)) console.log(`${name}: ${product.bytes} bytes (sha256 ${product.sha256})`);
  return record;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    if (args.length === 1 && args[0] === '--help') console.log('Usage: node scripts/wasm-products.mjs [--verify]\nBuilds both release-profile WASM modes and native fixture. No installs.\nBounds: each Cargo build/fixture 240s; graph/bindings 30s; tool/target probes 10s.\nPages orchestration gives products 16m total; nested commands inherit its supervised process group.\n--verify checks freshness without rebuilding. Products/provenance: ../target/wasm-browser/.');
    else if (args.length === 1 && args[0] === '--verify') { await verifyProducts(); console.log('WASM products are current'); }
    else { assert.equal(args.length, 0, 'Usage: node scripts/wasm-products.mjs [--verify]'); await buildProducts(); }
  } catch (error) { console.error(error); process.exitCode = 1; }
}
