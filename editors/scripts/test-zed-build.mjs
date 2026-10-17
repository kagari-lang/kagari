import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { access, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { artifactsDirectory, checkoutGrammar, productDirectory, stageZed } from './stage-zed.mjs';

export const zedCommit = 'fb69067729cb16a7f6135e2c1600c26a28c59659';
const sdkVersion = '34.0';

export function run(command, args, { cwd, timeoutMs = 120000, env = process.env } = {}) {
  assert.ok(Number.isSafeInteger(timeoutMs) && timeoutMs > 0 && timeoutMs <= 300000);
  console.log(`[zed-build] ${JSON.stringify([command, ...args])}`);
  const result = spawnSync(command, args, {
    cwd, env, encoding: 'utf8', timeout: timeoutMs, windowsHide: true, maxBuffer: 8 * 1024 * 1024,
  });
  if (result.stdout) process.stdout.write(result.stdout);
  if (result.stderr) process.stderr.write(result.stderr);
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${command} failed (${result.signal ?? result.status})`);
  return result.stdout;
}

function sdkPlatform() {
  const platform = { win32: 'windows', darwin: 'macos', linux: 'linux' }[process.platform];
  const arch = { x64: 'x86_64', arm64: 'arm64' }[process.arch];
  assert.ok(platform && arch, `No wasi-sdk 34 binary for ${process.platform}/${process.arch}`);
  return `${arch}-${platform}`;
}

export async function resolveSdk({ sdkPath, download = false, wsl } = {}) {
  const explicitDirectory = sdkPath || process.env.WASI_SDK_PATH;
  if (wsl !== undefined) {
    assert.equal(process.platform, 'win32', 'WSL compilation is supported only on Windows');
    assert.ok(typeof wsl === 'string' && wsl.trim(), '--wsl requires an installed distribution name, for example --wsl=Ubuntu');
    assert.ok(explicitDirectory, 'WSL requires an explicit host SDK directory through --sdk or WASI_SDK_PATH');
    assert.ok(!download, 'WSL mode never downloads an SDK; supply an extracted Linux WASI SDK 34 root through --sdk or WASI_SDK_PATH');
  }
  const name = wsl ? undefined : `wasi-sdk-${sdkVersion}-${sdkPlatform()}`;
  const directory = path.resolve(explicitDirectory || path.join(artifactsDirectory, name));
  const clang = path.join(directory, 'bin', !wsl && process.platform === 'win32' ? 'clang.exe' : 'clang');
  try {
    await access(clang);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    if (wsl) {
      throw new Error(`Linux WASI SDK 34 compiler is missing at ${clang}. Pass its host-visible SDK root, containing bin/clang, VERSION and share/wasi-sysroot; WSL mode will not download a replacement.`);
    }
    if (sdkPath || process.env.WASI_SDK_PATH || !download) {
      throw new Error(`WASI SDK 34 is missing at ${directory}. Set WASI_SDK_PATH, pass --sdk=<SDK root>, `
        + 'or use --download-sdk to download the fixed official release into target/editors. Native clang is not a substitute.');
    }
    const temporary = await productDirectory(undefined, 'wasi-sdk-download-');
    const archive = path.join(temporary, `${name}.tar.gz`);
    const url = `https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-34/${name}.tar.gz`;
    // Use the official public release URL without API tokens or alternate mirrors.
    run('curl', ['--fail', '--location', '--connect-timeout', '20', '--max-time', '180',
      '--output', archive, url], { timeoutMs: 190000 });
    run('tar', ['-xzf', archive, '-C', temporary], { timeoutMs: 120000 });
    await rename(path.join(temporary, name), directory);
    await access(clang);
  }
  const version = (await readFile(path.join(directory, 'VERSION'), 'utf8')).split(/\r?\n/)[0];
  assert.equal(version, sdkVersion, 'Use the same WASI SDK version as pinned Zed 1.24.2');
  // SDK 34 ships target-specific libc headers; clang defaults to wasip1.
  await access(path.join(directory, 'share', 'wasi-sysroot', 'include', 'wasm32-wasip1', 'stdio.h'));
  return { directory, clang, version };
}

function runWsl(distribution, args, options = {}) {
  try {
    return run('wsl.exe', ['--distribution', distribution, '--exec', ...args], { timeoutMs: 10000, ...options });
  } catch (error) {
    throw new Error(`WSL distribution ${JSON.stringify(distribution)} could not run ${JSON.stringify(args[0])}. Verify that the distribution is installed and can access/execute the supplied Linux SDK; no download fallback is attempted. ${error.message}`, { cause: error });
  }
}

function wslPath(distribution, hostPath) {
  const converted = runWsl(distribution, ['wslpath', '-a', '-u', hostPath]).trim();
  assert.ok(converted.startsWith('/') && !/[\r\n\0]/.test(converted), `WSL wslpath did not return one absolute Linux path for ${hostPath}`);
  return converted;
}

/** Build with Zed's exact C arguments, optionally using a host-visible Linux SDK through WSL. */
export async function buildZed({ stage, sdkPath, download = false, timeoutMs = 120000, wsl } = {}) {
  const sdk = await resolveSdk({ sdkPath, download, wsl });
  stage ??= await stageZed();
  const checkedOutGrammar = await checkoutGrammar(stage);
  const src = path.join(checkedOutGrammar, 'src');
  const wasmPath = path.join(stage.extensionDirectory, 'grammars', 'kagari.wasm');
  const compiler = wsl ? wslPath(wsl, sdk.clang) : sdk.clang;
  const compilerPaths = [wasmPath, src, path.join(src, 'parser.c'), path.join(src, 'scanner.c')]
    .map((hostPath) => wsl ? wslPath(wsl, hostPath) : hostPath);
  const [output, include, parser, scanner] = compilerPaths;
  const args = ['-fPIC', '-shared', '-Os', '-Wl,--export=tree_sitter_kagari', '-o', output,
    '-I', include, parser, scanner];
  const clangVersion = wsl ? runWsl(wsl, [compiler, '--version']) : run(compiler, ['--version'], { timeoutMs: 10000 });
  const options = { cwd: stage.extensionDirectory, timeoutMs };
  if (wsl) runWsl(wsl, [compiler, ...args], options);
  else run(compiler, args, options);
  const bytes = await readFile(wasmPath);
  const module = new WebAssembly.Module(bytes);
  assert.ok(WebAssembly.Module.exports(module).some((entry) => entry.kind === 'function'
    && entry.name === 'tree_sitter_kagari'), 'WASM must export the grammar entrypoint');
  assert.equal(WebAssembly.Module.customSections(module, 'dylink.0').length, 1,
    'Zed loads the grammar as a Tree-sitter WASM dynamic library');
  const result = { ...stage, wasmPath, wasmBytes: bytes.length,
    sdkDirectory: sdk.directory, sdkVersion: sdk.version, clangVersion: clangVersion.trim(),
    zedCommit, compilerArguments: args, wasmSha256: createHash('sha256').update(bytes).digest('hex'),
    compilerInvocation: { command: wsl ? 'wsl.exe' : compiler,
      args: wsl ? ['--distribution', wsl, '--exec', compiler, ...args] : args, timeoutMs },
    provenance: { hostPlatform: process.platform, compilerPlatform: wsl ? 'linux' : process.platform,
      wslDistribution: wsl ?? null } };
  await writeFile(path.join(stage.directory, 'build.json'), JSON.stringify(result, null, 2) + '\n');
  console.log(`[zed-build] Real WASM grammar built and validated (${bytes.length} bytes): ${wasmPath}`);
  return result;
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  const args = process.argv.slice(2);
  if (args.includes('--help')) {
    console.log('node scripts/test-zed-build.mjs [--sdk=<WASI SDK 34 host root>] [--wsl=<installed distribution>] [--download-sdk] [--timeout-ms=120000]');
  } else {
    const unknown = args.filter((arg) => arg !== '--download-sdk' && !arg.startsWith('--sdk=')
      && !arg.startsWith('--timeout-ms=') && !arg.startsWith('--wsl='));
    if (unknown.length) { console.error(`Unknown arguments: ${unknown.join(' ')}`); process.exitCode = 1; }
    else buildZed({ sdkPath: args.find((arg) => arg.startsWith('--sdk='))?.slice(6),
      wsl: args.find((arg) => arg.startsWith('--wsl='))?.slice(6),
      download: args.includes('--download-sdk'),
      timeoutMs: Number(args.find((arg) => arg.startsWith('--timeout-ms='))?.slice(13) ?? 120000),
    }).catch((error) => { console.error(error); process.exitCode = 1; });
  }
}
