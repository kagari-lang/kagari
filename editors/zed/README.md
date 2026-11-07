# Kagari for Zed

This extension owns the Kagari language configuration, Tree-sitter queries and a
small Rust launch adapter. Highlighting needs no analyzer. Semantic services launch
an explicitly configured executable over stdio; the adapter never downloads an
analyzer or searches `PATH`.

```json
{
  "lsp": {
    "kagari": {
      "binary": {
        "path": "F:/path/to/kagari/target/debug/kagari-analyzer.exe",
        "arguments": ["--stdio"]
      }
    }
  }
}
```

The current interoperability target is Zed **1.27.0**, commit
`c84af5b99cbc366d675064e52b23664a015b6e35`. The adapter uses extension API **0.7.0**
and Rust's `wasm32-wasip2` target. The ABI 15 grammar uses **WASI SDK 34**; a native
Node binding or system clang does not replace this SDK.

## Development and staging

After `npm ci` in `editors/`:

```sh
npm run check:generated
npm run test:zed
npm run stage:zed
```

Native capture tests require Python and a C/C++ toolchain. The staging suite's
optional WASM test requires `KAGARI_TEST_ZED_WASM=1` and `WASI_SDK_PATH`; otherwise
that case is explicitly skipped. Native capture tests need no running editor.

**Install the staged extension, not `editors/zed/` directly.** The maintained
manifest references the public monorepo's `main`; staging replaces that reference
with a resolved local snapshot, including uncommitted grammar changes:

```text
target/editors/zed-stage-<unique>/
  grammar-repository/.git/
  grammar-repository/editors/tree-sitter-kagari/
  extension/
    extension.toml
    Cargo.toml
    Cargo.lock
    src/lib.rs
    languages/kagari/{config.toml,*.scm}
  stage.json
```

The disposable grammar repository uses a parentless synthetic commit with fixed
identity, timestamp and Git configuration. Identical grammar inputs yield the same
SHA. Staging never changes the main worktree's index, commits or refs. Its manifest
uses an escaped absolute `file://` repository URL, the exact snapshot revision and
the grammar subdirectory. Machine paths stay out of maintained configuration.

Choose **Extensions → Install Dev Extension** and select the printed extension
directory. Keep the complete staging tree while installed. Restage after adapter,
grammar or query changes. Queries remain maintained only in
[languages/kagari](languages/kagari/); staged copies are disposable.

## Build the actual grammar and adapter

Install the Rust target with `rustup target add wasm32-wasip2`, then from `editors/`:

```sh
npm run build:zed -- --sdk=<WASI-SDK-34-root>
# Explicitly acquire the official SDK under ignored target/editors:
npm run build:zed -- --download-sdk
```

`WASI_SDK_PATH` can supply an existing read-only SDK. Windows can use an existing
Linux SDK through `--wsl=Ubuntu` plus a host-visible `--sdk` path; WSL builds never
download an SDK. Downloads use the fixed
[official WASI SDK 34 release](https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-34),
with finite transfer/extraction/build deadlines. The Windows archive is about
590 MiB; incomplete downloads cannot produce a successful build.

The builder reproduces Zed's literal-revision shallow checkout and grammar command:

```text
clang -fPIC -shared -Os -Wl,--max-memory=4294967296
  -Wl,--export=tree_sitter_kagari -o <extension>/grammars/kagari.wasm
  -I <grammar>/src <grammar>/src/parser.c <grammar>/src/scanner.c
```

It verifies the grammar module, exported entrypoint and `dylink.0`, and builds the
locked Rust adapter for `wasm32-wasip2` in release mode. `build.json` records grammar
and adapter hashes and actual invocations. Missing tools fail visibly; no placeholder
WASM or indexing-only result counts as interoperability.

## Automated real-client interoperability

Build `kagari-analyzer` separately. Run with the existing pinned Zed installation:

```sh
npm run test:zed-host -- --analyzer=/absolute/kagari-analyzer --sdk=<WASI-SDK-34-root>
npm run test:zed-host -- --analyzer=/absolute/kagari-analyzer --build-json=/absolute/build.json
```

Use `--cli=/absolute/zed` for an explicit installation and `--timeout-ms=60000`
to bound the host (1–120 seconds). No editor is downloaded. The runner launches
the actual app with `ZED_STATELESS=1` and fresh user/config/extension/temp storage
under `target/editors/`. It explicitly trusts only that disposable test profile's
worktree, disables autosave and never changes the user's profile. Windows uses
`--foreground` to avoid singleton routing. macOS is rejected because the inspected
version's log path does not honor the isolated directory; Linux acceptance is not
inferred from a Windows run.

A test-only transparent protocol relay forwards the real Zed client to the real
analyzer. After initialize/open and an expected semantic diagnostic, it sends a
standard `workspace/applyEdit` for the disposable unsaved buffer. Acceptance needs
Zed's successful edit reply, its actual full-sync `didChange`, the corresponding
changed analyzer diagnostic, and the same chain after restoring the original text.
Unsupported `applyEdit`, missing capabilities, wrong versions, startup failures and
deadline expiration fail the probe. The disk fixture must remain unchanged.

The report records client/analyzer versions and image hashes, protocol evidence and
host duration. Cleanup targets only the owned process tree. This establishes real
launch, synchronization and diagnostics; richer Zed navigation/completion UI coverage
belongs to the later Zed feature checkpoint. Exact query captures and incremental
recovery remain at the native grammar owner. No manual clicks are required for this
protocol acceptance, and it does not claim visual rendering for every theme.

## Editing configuration

Pairing has two gates: the smallest matching editing scope and the following
character. `autoclose_before = ")]}"` allows code pairing immediately before an interpolation
`}`. A hole accepts only a name and optional adjacent `:?`; entering an arbitrary
expression makes it invalid. Strings/comments suppress pairing, including before
whitespace. Empty interpolation overrides restore code pairing without inheriting
the surrounding string's disabled pairs. Character literals use single quotes.

## Primary implementation references

- [Pinned builder, grammar checkout, WASI SDK and Rust target](https://github.com/zed-industries/zed/blob/c84af5b99cbc366d675064e52b23664a015b6e35/crates/extension/src/extension_builder.rs)
- [Pinned manifest and extension library contract](https://github.com/zed-industries/zed/blob/c84af5b99cbc366d675064e52b23664a015b6e35/crates/extension/src/extension_manifest.rs)
- [Worktree trust](https://zed.dev/docs/worktree-trust)
- [Settings reference](https://zed.dev/docs/reference/all-settings)
