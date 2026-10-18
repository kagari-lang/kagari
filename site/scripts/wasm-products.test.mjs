import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import test from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { buildProductFiles, productsRoot, repositoryRoot, runCommand } from './wasm-products.mjs';

const validTree = 'kagari-web v0.0.0\nkagari-runtime v0.0.0\n';

test('product commands have explicit budgets inside the 16-minute aggregate, unchanged profiles and parallelism', async () => {
  const calls = [];
  assert.equal(await buildProductFiles('fixture-bindgen', async (...args) => {
    calls.push(args);
    return { stdout: validTree };
  }), validTree);
  const target = 'wasm32-unknown-unknown';
  const binding = (mode) => ['fixture-bindgen', [join(repositoryRoot, `target/${target}/debug/kagari_web.wasm`), '--target', 'web', '--out-dir', join(productsRoot, mode), '--out-name', 'kagari_web'], { timeout: 30_000, mirror: true }];
  assert.deepEqual(calls, [
    ['cargo', ['tree', '-p', 'kagari-web', '--no-default-features', '--target', target, '--edges', 'normal', '--prefix', 'none', '--locked'], { timeout: 30_000 }],
    ['cargo', ['build', '-p', 'kagari-web', '--target', target, '--locked'], { timeout: 240_000, mirror: true }],
    binding('source'),
    ['cargo', ['build', '-p', 'kagari-web', '--no-default-features', '--target', target, '--locked'], { timeout: 240_000, mirror: true }],
    binding('artifact'),
    ['cargo', ['test', '-p', 'kagari-web', '--lib', '--locked', 'tests::generate_browser_artifact', '--', '--exact'], { timeout: 240_000, mirror: true }],
  ]);
  // Two tool discoveries: at most two bindgen probes + rustc + Cargo each;
  // one target probe. Reserve one further minute for input/product IO and cleanup.
  assert.equal(calls.reduce((sum, [, , { timeout }]) => sum + timeout, 0) + 9 * 10_000 + 60_000, 960_000);
  let attempted = 0;
  await assert.rejects(buildProductFiles('fixture-bindgen', async () => {
    attempted += 1;
    return { stdout: `${validTree}kagari-source v0.0.0\n` };
  }), /frontend\/native tooling/u);
  assert.equal(attempted, 1, 'An invalid fresh graph must stop before compilation');
});

test('command bounds reject disabled, overflowing and non-integer timers before spawning', async () => {
  for (const timeout of [0, -1, 0.5, NaN, Infinity, 2_147_483_648, '1000', null]) {
    await assert.rejects(runCommand(process.execPath, ['--eval', 'process.exit(0)'], { timeout }), /positive integer/u);
  }
  const result = await runCommand(process.execPath, ['--eval', 'console.log("bounded child")'], { timeout: 10_000 });
  assert.equal(result.stdout.trim(), 'bounded child');
});

async function readPid(path) {
  try { return Number(await readFile(path, 'utf8')); }
  catch (error) { if (error.code === 'ENOENT') return undefined; throw error; }
}

function running(pid) {
  try { process.kill(pid, 0); }
  catch (error) { if (error.code === 'ESRCH') return false; throw error; }
  // Reparented zombies may await init's reap in containers; they cannot execute.
  const state = spawnSync('ps', ['-p', String(pid), '-o', 'stat='], { encoding: 'utf8', timeout: 1_000 });
  assert.equal(state.error, undefined);
  return state.status === 0 && !state.stdout.trim().startsWith('Z');
}

test('Unix nested helpers inherit one group on outer timeout, inner timeout and interrupt', {
  skip: process.platform === 'win32' ? 'Unix process-group contract; Windows uses taskkill /T /F' : false,
  timeout: 25_000,
}, async (t) => {
  for (const mode of ['outer-timeout', 'inner-timeout', 'interrupt']) {
    await t.test(mode, { timeout: 8_000 }, async (childTest) => {
      const target = join(repositoryRoot, 'target');
      await mkdir(target, { recursive: true });
      const directory = await mkdtemp(join(target, 'command-group-test-'));
      const middlePid = join(directory, 'middle.pid');
      const leafPid = join(directory, 'leaf.pid');
      const leaf = join(directory, 'leaf.mjs');
      const middle = join(directory, 'middle.mjs');
      const supervisor = join(directory, 'supervisor.mjs');
      const helper = JSON.stringify(new URL('./wasm-products.mjs', import.meta.url).href);
      await writeFile(leaf, `import { writeFileSync } from 'node:fs';
writeFileSync(${JSON.stringify(leafPid)}, String(process.pid));
setInterval(() => {}, 1000);
setTimeout(() => process.exit(3), 30000);
`);
      await writeFile(middle, `import { writeFileSync } from 'node:fs';
import { runCommand } from ${helper};
writeFileSync(${JSON.stringify(middlePid)}, String(process.pid));
await runCommand(process.execPath, [${JSON.stringify(leaf)}], { timeout: ${mode === 'inner-timeout' ? 1_000 : 15_000} });
`);
      await writeFile(supervisor, `import { runCommand } from ${helper};
try {
  await runCommand(process.execPath, [${JSON.stringify(middle)}], { timeout: ${mode === 'outer-timeout' ? 2_000 : 5_000} });
  process.exitCode = 2;
} catch (error) { console.error(error.message); }
`);
      // The fixture is an independent supervision boundary even when these tests
      // themselves run inside build:pages' supervised check process.
      const env = { ...process.env };
      delete env.KAGARI_COMMAND_GROUP;
      const child = spawn(process.execPath, [supervisor], { env, detached: true, stdio: ['ignore', 'ignore', 'pipe'] });
      let stderr = '';
      child.stderr.on('data', (bytes) => { stderr += bytes; });
      const closed = new Promise((accept, reject) => {
        child.once('error', reject);
        child.once('close', (code, signal) => accept({ code, signal }));
      });
      childTest.after(async () => {
        // Also clean up under the old broken implementation, where leaf detaches.
        for (const pid of [await readPid(leafPid), await readPid(middlePid), child.pid]) {
          if (!pid) continue;
          try { process.kill(-pid, 'SIGKILL'); } catch { /* Group already gone or inherited. */ }
          try { process.kill(pid, 'SIGKILL'); } catch { /* Process already gone. */ }
        }
        await rm(directory, { recursive: true, force: true });
      });
      if (mode === 'interrupt') {
        for (let attempt = 0; attempt < 100 && !await readPid(leafPid); attempt += 1) await delay(20);
        assert(await readPid(leafPid), 'Leaf must be running before interrupt');
        child.kill('SIGTERM');
      }
      assert.deepEqual(await closed, { code: 0, signal: null }, stderr);
      assert.match(stderr, mode === 'interrupt' ? /interrupted/u : /timed out after (1000|2000)ms/u);
      for (const path of [middlePid, leafPid]) {
        const pid = await readPid(path);
        assert(Number.isSafeInteger(pid) && pid > 0, `Fixture did not start: ${path}`);
        for (let attempt = 0; attempt < 50 && running(pid); attempt += 1) await delay(20);
        assert.equal(running(pid), false, `Supervised descendant survived: ${pid}`);
      }
    });
  }
});
