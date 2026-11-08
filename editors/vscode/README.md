# Kagari for VS Code

Syntax highlighting, editing configuration and a thin language-server client for
`.kgr` files. Language ID: `kagari`; TextMate scope: `source.kagari`. The bundled
client launches an explicitly configured `kagari-analyzer` over stdio. It registers
diagnostics, hover, definition, implementation and completion through the server's
advertised capabilities. This extension is not published.

The extension also carries fixed standard-library declarations in `dist/stdlib`
and passes their location as `KAGARI_STDLIB_SOURCES` to its analyzer child. An
explicit inherited value takes precedence. The analyzer verifies exact bytes
against its embedded package, so a separately configured binary cannot silently
use incompatible declarations. Missing or mismatched assets produce a warning
and use embedded sources with a local cache fallback. Standard navigation requires
no user Host build; analysis still works if fallback files cannot be published.

## Configure the analyzer

Build `cargo build --locked -p kagari-analyzer`, then set an absolute executable path:

```json
{
  "kagari.server.path": "F:/path/to/kagari/target/debug/kagari-analyzer.exe",
  "kagari.server.enabled": true
}
```

The executable is never downloaded or searched on `PATH`. A missing or invalid path
appears in the Kagari status item and output channel. Use **Kagari: Show Language
Server Output** for startup failures and **Kagari: Restart Language Server** after
repairing a configuration. Setting `kagari.server.enabled` to `false` stops the owned
server; grammar highlighting and editing remain available.

One controller owns startup, replacement, shutdown and recovery. An unexpected exit
first finishes client cleanup, then retries after 250 ms, 1 s and 4 s, with at most
three automatic restarts in sixty seconds. Further failures require an explicit
restart. Configuration changes and disabling invalidate queued recovery. Reopening
the protocol connection resends current editor buffers, including unsaved text.

## Develop and install locally

Use Node 22.21.1 and the pinned tools installed from the parent `editors/` package
(`npm ci` there). From `editors/`, run:

```sh
npm --workspace kagari-language test
npm run build:vscode
npm run package:vscode
```

The packaging command creates the ignored repository `target/editors/` directory
and writes `target/editors/kagari-language-0.1.0.vsix`; it does not write a VSIX into
this extension. Install that file with **Extensions: Install from VSIX...**, or:

```sh
code --install-extension ../target/editors/kagari-language-0.1.0.vsix
```

For an Extension Development Host, run from the repository root:

```sh
code --new-window --extensionDevelopmentPath=editors/vscode examples/syntax/string-interpolation.kgr
```

The minimum engine is VS Code 1.142.0. `build:vscode` bundles the pinned
`vscode-languageclient` 11.0.0 and its runtime dependencies with esbuild; the VSIX
includes the client, standard declaration sources and dependency license notices,
without `node_modules`, build tools or an analyzer binary. Packaging always builds first. Follow the
[isolated host guide](../tests/vscode-host/README.md) for H01–H05 client acceptance
against an existing editor and the actual installed VSIX.

## Scope and maintenance

Nested block/doc comments use recursive contexts. Cooked and raw Strings retain
physical LF/CRLF across lines; only the matching quote/hash delimiter ends them.
Raw strings use 0..255 hashes and do not interpret escapes or interpolation.
Cooked holes are exactly `{name}` or `{name:?}`; malformed holes are marked
invalid instead of entering an arbitrary code context. Unicode/ASCII escapes,
continuations and doubled braces stay string text. The compiler validates decoded
Unicode scalars and standalone CR. Byte characters (`b'A'`), byte strings
(`b"..."`) and raw byte strings (`br#"..."#`) have dedicated string scopes.
Direct non-ASCII contents and Unicode escapes in cooked bytes are marked invalid;
byte hexadecimal escapes accept x00..xFF. Byte braces never start holes and raw
byte backslashes stay literal. The old `f` prefix is not accepted.

The grammar uses theme scopes, not fixed colors. Cooked text segments, rather
than their enclosing context, carry string scopes so identifier colors do not
inherit the outer string. Declaration names and simple explicit type positions
are contextual captures; comments/newlines between declaration tokens may prevent
those captures. Arbitrary capitalized identifiers, constructors, generic arguments,
parameter types and generic/comparison angle brackets are not semantically resolved.
Binding patterns reuse lexical identifier, separator, operator and recursive brace
scopes: `val`/`var` destructuring, grouped/nested alternatives, renamed/shorthand
struct fields and final `..` (with or without a trailing comma) need no pattern
parser. Pattern names remain variables, not guessed types or constructors. The
highlighter does not validate pattern legality, including the ban on bare top-level
alternatives and tuple/slice rest; the language parser/checker owns those rules.

`///` and `/** ... */` receive documentation comment presentation, not additional
language semantics. Unsupported Rust strings/lifetimes/keywords are not added.

Tests live in `../tests/vscode/` and run with native `node --test`, actual
`vscode-textmate` and Oniguruma. They assert scopes, encoded token categories,
string-theme isolation and incremental versus fresh rule stacks, reading selected
repository examples directly. `tokenize.mjs` exports `async tokenize(text)` for
shared adapter contracts: `{start, end, scopes, type}` spans use UTF-16 offsets;
`type` is `code`, `string` or `comment`. LF separators are omitted from spans (CR is
retained for CRLF), and no synthetic TextMate newline token escapes into the result.
The VSIX allowlist excludes tests, dependency directories and tooling.

Licensed under MIT OR Apache-2.0; license texts are included in the VSIX.
