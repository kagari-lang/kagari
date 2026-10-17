# Kagari syntax support for Zed

This declarative extension needs no Rust extension crate, `kg`, analyzer, project
manifest or language-server registration. Maintain its configuration and queries
only in [languages/kagari](languages/kagari/); staging copies disposable products
from that owner, never a second maintained query implementation.

## Prerequisites

Use the parent npm workspace's pinned Node dependencies and generated **ABI 15**
parser. Native capture tests need Python and a C/C++ toolchain (VS 2022 on Windows).
The inspected host is **Zed 1.24.2**, commit
`fb69067729cb16a7f6135e2c1600c26a28c59659`; its grammar builder uses **WASI SDK 34.0**.
Native system clang and the native Node binding do not replace that SDK. No
Emscripten or Rust WASM extension target is needed for this syntax-only extension.

From `editors/`, after `npm ci`:

```sh
npm run check:generated
npm run test:zed
npm run stage:zed
```

The staging suite includes an opt-in WASM test: set `KAGARI_TEST_ZED_WASM=1` and
`WASI_SDK_PATH` before running it. Without those settings the build test is
explicitly skipped, not passed. To use a Linux SDK from Windows, additionally set
`KAGARI_TEST_ZED_WSL=Ubuntu`; `WASI_SDK_PATH` remains the host-visible SDK root.
The non-WASM capture/staging/fetch tests require
neither an SDK nor a running editor.

## Reproducible development installation

The maintained manifest references the public monorepo's `main` and grammar
subdirectory. Before these assets are merged there, that remote cannot supply
the working tree. **Install the staged extension, not `editors/zed/` directly.**

`npm run stage:zed` prints `extensionDirectory` and writes an ignored `stage.json`:

```text
target/editors/zed-stage-<unique>/
  grammar-repository/.git/
  grammar-repository/editors/tree-sitter-kagari/
    tree-sitter.json
    src/{parser.c,scanner.c,grammar.json,node-types.json,tree_sitter/*.h}
  extension/
    extension.toml
    languages/kagari/{config.toml,*.scm}
  stage.json
```

The disposable repository snapshots only current grammar inputs, including
uncommitted generated C and scanner changes. A parentless synthetic commit uses
fixed identity, timestamp and Git configuration, so identical inputs yield the
same SHA. The script never stages, commits or changes refs in the main worktree.
The staged manifest's `repository` becomes an escaped absolute `file://` URL and
`rev` becomes that resolved SHA; `path` stays `editors/tree-sitter-kagari`.
Machine-specific paths are never committed to maintained configuration.

In the intended editor instance, choose **Extensions → Install Dev Extension**
and select the printed directory. Zed fetches the literal revision, appends the
monorepo subpath and builds `grammars/kagari.wasm`. Keep the whole staging directory
while installed; the manifest references its sibling grammar repository. Restage
and reinstall after grammar or query changes. A separate maintained grammar
repository is not required: the pinned builder supports subdirectory `path`.

For scripting, `stageZed({ directory })` is exported by
[stage-zed.mjs](../scripts/stage-zed.mjs). An explicit output directory must be an
empty descendant of this checkout's `target/editors/`; overwrite and symlink-
escape guards protect source and user directories.

## Real WASM build

```sh
npm run build:zed -- --sdk=<WASI-SDK-34-root>
# Or explicitly download the fixed official release into target/editors:
npm run build:zed -- --download-sdk
```

On Windows, an already extracted Linux SDK can be used with
`npm run build:zed -- --sdk=<host-visible-Linux-SDK-root> --wsl=Ubuntu`, or
`buildZed({ sdkPath, wsl: 'Ubuntu' })`. This requires the named WSL distribution,
not Linux Node. Supply the explicit host SDK root via `--sdk` or `WASI_SDK_PATH`;
WSL mode never downloads an SDK. The same VERSION/header checks apply, paths are
converted by bounded `wslpath` calls, and Linux `bin/clang` is invoked directly
with `--exec`. `build.json` records separate host/compiler platforms, distribution
and the actual invocation. The host probe also accepts `--wsl=Ubuntu` for source
builds; `--build-json` continues to reuse a prior build without compiling.

An installed SDK can also be supplied read-only through `WASI_SDK_PATH`. Explicit
downloads use [official WASI SDK release 34](https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-34),
without tokens, alternate mirrors or disabled TLS verification. The Windows
archive is about 590 MiB; network transfers, extraction and builds have finite
timeouts. A partial archive is not an SDK. The Windows release asset is
`528366541`, size `619003408`, SHA-256
`cccb5c323a9b34f0349a9b09e8804a0a7632c68c3310f4b5f437ed57d7e71d8f`.
Verify a complete manually acquired archive before extraction.

The script reproduces Zed's literal-SHA shallow Git checkout and exact compiler
arguments for both parser and scanner:

```text
clang -fPIC -shared -Os -Wl,--export=tree_sitter_kagari
  -o <extension>/grammars/kagari.wasm -I <checked-out grammar>/src
  <checked-out grammar>/src/parser.c <checked-out grammar>/src/scanner.c
```

It checks the WebAssembly module, exported grammar entrypoint and `dylink.0`
section, then writes `build.json`. This proves a real build, not UI installation.
Missing SDKs or failed downloads fail the check instead of producing a placeholder.

## Isolated editor probe and manual acceptance

```sh
npm run test:zed-host -- --sdk=<WASI-SDK-34-root> --duration-ms=20000
# Reuse an actual build rather than compiling again:
npm run test:zed-host -- --build-json=<absolute-path-to-build.json>
# Metadata-only diagnostic; this intentionally cannot pass full acceptance:
npm run test:zed-host -- --index-only --duration-ms=15000
```

The probe requires the pinned editor, supports explicit `--cli=<executable>`, and
never downloads an editor. It launches the actual app binary, not the singleton-
routing CLI, with `ZED_STATELESS=1` and a fresh `--user-data-dir` below
`target/editors/`. On Windows it adds `--foreground`. This matters because the
pinned Windows singleton is release-channel-wide, not keyed by the data directory.
The probe refuses macOS because that version's log path ignores the custom data
directory. Linux host acceptance is not claimed from Windows runs.

The probe copies the staged extension into the actual installed-extension layout;
Zed itself rebuilds the index. Configuration, logs, temporary files and a standalone
Unicode/CRLF sample stay in the isolated tree. Updates, AI, telemetry and the Kagari
language server are disabled. Cleanup targets only the spawned PID tree, never
all Zed processes. The probe records loader start/finish events and rejects actual
language/grammar/query errors in WASM mode; finish events alone are not success
because Zed also emits them on failure.

The runner returns **2 for incomplete/manual acceptance**, even when indexing and
loading succeed, or **1 for an execution failure**. No external Zed API exposes
buffer language, rendered colors or key-edit assertions. Inspect the isolated
instance and selected shared examples manually:

1. `.kgr` selects Kagari without an analyzer process.
2. Light/dark themes render representative and incomplete inputs correctly.
3. Toggle line/block comments and exercise `()`, `{}`, `[]` and double quotes in
   code, strings, nested comments and interpolation holes. No global `<`/`>` or
   single-quote string pairing is configured.
4. Delete/restore quotes, interpolation braces and comment delimiters; later lines
   recover without stale string/comment classification.
5. Check actual loader logs for query, ABI, scanner or WASM failures. Screenshots
   alone do not prove capture boundaries; native query tests check those separately.

The parent guide records [validation scope](../README.md#validation-scope), and the
[roadmap](../../docs/implementation-roadmap.md#toolchain-distribution-and-modes)
owns pending CI/platform acceptance. Successful metadata indexing alone is not a
claim of successful grammar loading or editor acceptance.

Pairing has two independent gates in the pinned host: the smallest matching editing
scope must allow a pair, and the following character must allow insertion. Kagari
sets `autoclose_before = ")]}"` explicitly so `f"{value|}"` can become
`f"{value(|)}"` after typing `(` (`|` denotes the caret). Strings/comments still
suppress pairing, including before whitespace. An empty interpolation override
restores code pairing; it does not inherit the surrounding string's disabled pairs.

## Pinned implementation references

These primary sources use Zed commit `fb69067729cb16a7f6135e2c1600c26a28c59659`:

- [Builder, literal revision fetch, grammar subpath and SDK/compiler arguments](https://github.com/zed-industries/zed/blob/fb69067729cb16a7f6135e2c1600c26a28c59659/crates/extension/src/extension_builder.rs)
- [Installed layout, index rebuilding and dev-extension installation](https://github.com/zed-industries/zed/blob/fb69067729cb16a7f6135e2c1600c26a28c59659/crates/extension_host/src/extension_host.rs)
- [Input handling and the independent following-character autoclose gate](https://github.com/zed-industries/zed/blob/fb69067729cb16a7f6135e2c1600c26a28c59659/crates/editor/src/input.rs)
- [Language configuration and default-empty autoclose policy](https://github.com/zed-industries/zed/blob/fb69067729cb16a7f6135e2c1600c26a28c59659/crates/language_core/src/language_config.rs)
- [Smallest matching override range and inclusive/exclusive cursor boundaries](https://github.com/zed-industries/zed/blob/fb69067729cb16a7f6135e2c1600c26a28c59659/crates/language/src/syntax_map.rs)
- [Data/config/extensions/log paths](https://github.com/zed-industries/zed/blob/fb69067729cb16a7f6135e2c1600c26a28c59659/crates/paths/src/paths.rs)
- [Stateless startup and singleton bypass](https://github.com/zed-industries/zed/blob/fb69067729cb16a7f6135e2c1600c26a28c59659/crates/zed/src/main.rs)
