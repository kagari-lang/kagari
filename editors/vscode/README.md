# Kagari for VS Code

Declarative syntax highlighting, comment toggling and basic bracket/quote pairing
for `.kgr` files. Language ID: `kagari`; TextMate scope: `source.kagari`.
There is no activation code, language server, analyzer launch or runtime dependency.
`kagari-lang.kagari-language` names the language contribution rather than implying
semantic language services. This extension is not published.

## Develop and install locally

Use Node 22.21.1 and the pinned tools installed from the parent `editors/` package
(`npm ci` there). From `editors/`, run:

```sh
npm --workspace kagari-language test
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

The minimum engine is VS Code 1.142.0 (the installed local version, not evidence of
GUI acceptance). In the host and installed VSIX, verify the Kagari language mode,
comment toggling, bracket and quote pairing in code versus strings/comments, and
the selected examples in light
and dark themes. **Developer: Inspect Editor Tokens and Scopes** should show code
inside `meta.embedded.expression.kagari` for identifier holes, string text around
it, and inert braces/backslashes in raw Strings. Verify this without `kg`
or an analyzer running. Automated tokenization is not a substitute for this GUI
acceptance; see the parent editor tooling documentation for recorded results.

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
The VSIX allowlist excludes tests, dependencies and tooling.

Licensed under MIT OR Apache-2.0; license texts are included in the VSIX.
