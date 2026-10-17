# Editor tooling: LSP foundation and syntax highlighting

Status: execution plan requested on 2026-10-17; implementation is unstarted.
The agreed order is a working in-repository LSP foundation, then basic Zed and
VS Code syntax highlighting. This document owns ET01-ET05, acceptance and the
progress ledger. It does not claim that an analyzer or editor extension exists.

## Goal and finite scope

Maintain language implementation and editor tooling in this repository, with
separate crate/module responsibilities and independently distributable products.
A language change should be able to update its frontend, editor adapters and
focused tests in one change without coordinating Git dependency revisions.

Deliver:

- A `kagari-analyzer` executable with real stdio LSP lifecycle and document sync.
- One source identity/text/revision foundation, reusing existing source services.
- Zed Tree-sitter highlighting and a VS Code TextMate language extension for `.kgr`.
- Shared representative source inputs, adapter-specific highlighting assertions,
  reproducible local installation instructions and focused CI coverage.

Not in this iteration: semantic diagnostics, completion, hover, navigation,
references, rename, semantic tokens, formatting integration, workspace/package
analysis, filesystem watching, DAP, run buttons, snippets, language-server download
management, marketplace publication or a new incremental analysis engine.
Do not advertise these capabilities or return placeholder results for them.
An IDE query crate is not required until there is a concrete query consumer.

Basic highlighting must work with no `kg` or analyzer installed and no project
manifest. The extensions do not need to start the skeleton language server;
editor-to-server launch integration belongs to the first useful IDE feature.

## Baseline and migration evidence

The main repository already owns:

- [Syntax and error recovery](architecture/syntax.md), backed by the handwritten
  parser and Rowan CST. This remains the compiler/formatter syntax implementation.
- [HIR analysis and tooling facts](architecture/hir.md), including analysis
  databases and snapshots; no `kagari-sema` crate currently exists.
- [SourceDatabase](../crates/kagari-source/src/source_database.rs), with base and
  overlay layers, stable file identities and retained source snapshots.
- [LineIndex](../crates/kagari-source/src/line_index.rs), with UTF-8/UTF-16 position
  conversion and CRLF handling.
- [Syntax examples](../examples/syntax), lexer/parser tests and grammar coverage.

The separate [analyzer repository](https://github.com/kagari-lang/kagari-analyzer)
was inspected at commit `4a05bc564dd56c5e32f68c7f0138ddc83840cec2`. Its workspace
contains analyzer, IDE, base-db and VFS skeletons. The inspected server bootstrap dispatches synthetic
messages and prints a startup line; it is not a stdio LSP transport. Its diagnostic
helper reports an empty file as an error rather than invoking Kagari analysis.
Its base-db stores analyzer-side VFS inputs rather than integrating the existing
source database. These are migration inputs, not behavior to preserve.

ET01 must inspect the remaining code and repository metadata before retiring it.
Transfer useful code/designs with applicable attribution/license information;
do not copy all four crates merely to preserve their names. Preserve any unique
issues or decisions in their appropriate owner here. No permanent compatibility
crate, Git submodule, split progress ledger or external source dependency remains.
The agreed old-repository removal is an administrative follow-up after that audit
and transfer, not an automated side effect of a build or this planning change.
This document records the baseline so resumption does not require the old URL.

## Ownership and design decisions

| Owner | Responsibility |
| --- | --- |
| `kagari-source` | Source names/IDs, text layers, immutable snapshots, revisions and position conversion |
| Existing syntax/HIR/package owners | Parsing, semantic facts and project inputs; future IDE features consume these rather than reimplementing them |
| `kagari-analyzer` | LSP transport/lifecycle, URI adaptation, open-document metadata and protocol versions, synchronization and error reporting |
| Future IDE query layer | Editor-oriented results derived from shared semantic facts, without LSP types; create only with real features |
| Tree-sitter grammar | Recoverable editor syntax structure, not static validation or compiler lowering |
| Zed/TextMate adapters | Theme classifications and declarative editing behavior |

Use ordinary analyzer modules for transport, session/document handling and
protocol conversion. Start with one new workspace package; do not create empty
`kagari-ide`, `kagari-base-db` or `kagari-vfs` crates. Extend existing source APIs
only where the real document lifecycle reveals a gap. Protocol document versions
and source revisions are different identities and must be mapped explicitly.

SDK/runtime crates must not depend on the analyzer, LSP types or editor grammars.
Workspace membership alone does not create a product dependency. Do not introduce
LSP types into source/HIR or make the compiler consume Tree-sitter output.

### Reference mechanisms and differing assumptions

Primary references for the design decisions recorded on 2026-10-17:

- [rust-analyzer architecture](https://rust-analyzer.github.io/book/contributing/architecture.html):
  separate IDE queries from protocol conversion, retain immutable analysis views,
  and keep language semantics below LSP. Its IDE API is explicitly unstable.
  Kagari already shares its frontend with tools; copying rust-analyzer's complete
  database/VFS/HIR organization or its relationship with rustc is not justified.
- [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/):
  use standard stdio framing, lifecycle, capability and synchronization contracts,
  rather than the old skeleton's private message enums. Select maintained Rust
  protocol/transport dependencies in ET01 and record exact locked versions.
- [Zed languages](https://zed.dev/docs/extensions/languages) and
  [extension development](https://zed.dev/docs/extensions/developing-extensions):
  Tree-sitter grammars and queries provide basic highlighting without custom Rust
  extension code. Local grammar repositories can use `file://` references.
  The [manifest model at Zed 1.24.2 commit fb690677](https://github.com/zed-industries/zed/blob/fb69067729cb16a7f6135e2c1600c26a28c59659/crates/extension/src/extension_manifest.rs)
  has `repository`, `rev` and optional `path` fields for grammar entries, allowing
  a monorepo subdirectory reference. This source inspection is not evidence of a
  successful grammar build or development installation.
- [VS Code syntax highlighting](https://code.visualstudio.com/api/language-extensions/syntax-highlight-guide)
  and [language configuration](https://code.visualstudio.com/api/language-extensions/language-configuration-guide):
  TextMate/Oniguruma and declarative configuration provide baseline coloring and
  editing behavior without runtime extension code or a server.

Web documentation is a design reference, not a pinned build input. Record tested
editor versions, Tree-sitter CLI/ABI and Node/test dependency versions when each
adapter is implemented. Do not invent a common grammar generator or deploy a
custom VS Code Tree-sitter integration merely to unify implementations. Share
inputs and classification contracts, not incompatible parsing machinery.

## Suggested crate and file layout

The following layout is guidance, not a mandatory scaffold or a claim that these
files exist. It applies to the proposed paths in the phase descriptions as well.
If implementation reveals a conflict with actual ownership, existing APIs, build
requirements or test organization, the verified implementation needs take
precedence over this sketch. Adapt names and boundaries rather than forcing the
code into the tree. Record material adjustments and their reasons in this plan's
ledger and update the affected guidance. This flexibility does not waive semantic
invariants, acceptance checks or the agreed scope; substantial scope changes must
still be made explicit.

### Repository placement

Keep language infrastructure and Rust tools under `crates/`; keep editor grammars
and extension packaging under `editors/`. The relevant existing owners remain:

```text
crates/
  kagari-source/              # Source identities, overlays, snapshots, positions
  kagari-syntax/              # Compiler lexer/parser and Rowan CST
  kagari-hir/                 # Shared semantic analysis and query facts
  kagari-package/             # Package/module inputs
  kagari-format/              # Formatting
  kagari-lint/                # Static lint
  kagari-cli/                 # kg executable
  kagari/                     # Embedding SDK
```

The initial new Rust package is `crates/kagari-analyzer/`. Do not add an IDE,
base-db or VFS package solely to match the old repository. Tree-sitter assets
belong to editor tooling, not the compiler dependency graph.

### Analyzer modules and tests

```text
crates/kagari-analyzer/
  Cargo.toml
  src/
    main.rs
    lib.rs
    server.rs
    session.rs
    documents.rs
    protocol.rs
    tests/
      mod.rs
      lifecycle.rs
      documents.rs
      protocol.rs
  tests/
    stdio.rs
```

| Suggested file | Responsibility |
| --- | --- |
| `main.rs` | Process entry, startup failures and exit status; no document or language policy |
| `lib.rs` | Minimal entrypoints and module declarations, not a broad re-export facade |
| `server.rs` | Stdio transport setup, message dispatch and driving termination |
| `session.rs` | Initialization/shutdown state, capabilities and session configuration |
| `documents.rs` | Open/change/close handling, protocol document metadata and calls into existing source services |
| `protocol.rs` | URI and protocol/internal type conversion, protocol error construction |
| `src/tests/` | Focused module behavior tests without widening production visibility |
| `tests/stdio.rs` | Bounded real-process framing/lifecycle test and stdout cleanliness |

Create modules only when they contain real responsibilities. Small cohesive parts
may initially share a file; growing transport, dispatch or conversion code can be
split at meaningful boundaries. Protocol types should come from the selected LSP
library, not a second handwritten protocol model in `protocol.rs`.

For example, `didChange(uri, version=12, text=...)` first checks the open document
and client version, then applies text through `SourceDatabase`'s overlay API and
records the resulting source state. Analyzer owns URI/open-state/LSP-version
metadata; source owns text, FileId, source revisions and retained snapshots.
`documents.rs` is not another VFS, and `protocol.rs` reuses `LineIndex` rather than
implementing another coordinate system. Protocol versions need not equal source
revisions, including when an edit leaves the text unchanged.

### Future IDE layer, only when needed

A later concrete hover/completion/navigation feature may justify
`crates/kagari-ide/`, with `analysis.rs` and feature modules such as `hover.rs`,
`completion.rs` and `navigation.rs`. Do not create them in this iteration merely
as placeholders. The intended dependency direction is:

```text
kagari-analyzer: protocol requests and responses
  -> kagari-ide: editor-oriented query results
     -> existing source/syntax/HIR/package capabilities
```

For hover, HIR supplies the resolved type/signature facts, IDE selects the display
content and source range, and analyzer converts that result to LSP. Reuse existing
queries first; move presentation policy out of a semantic owner only when an
actual feature exposes that misplaced responsibility. Do not retain two query
implementations or create an independent cache/analysis database for the IDE.

### Editor adapters and highlighting tests

```text
editors/
  README.md
  tree-sitter-kagari/
    grammar.js
    tree-sitter.json
    package.json
    src/
      parser.c                    # Generated
      grammar.json                # Generated
      node-types.json             # Generated
    test/
      corpus/
  zed/
    extension.toml
    languages/
      kagari/
        config.toml
        highlights.scm
        brackets.scm
  vscode/
    package.json
    language-configuration.json
    syntaxes/
      kagari.tmLanguage.json
  tests/
    fixtures/
    zed/
    vscode/
```

This sketch omits tool-required headers, lockfiles and packaging metadata; add
those required by the selected versions. Keep one owner for each handwritten
rule: grammar structure in `tree-sitter-kagari`, Zed theme captures in the Zed
extension, and TextMate rules in the VS Code extension. Do not maintain a second
copy of `highlights.scm` merely for a test runner; generate/stage disposable inputs
when a tool requires a particular layout.

No Zed Rust extension crate or VS Code `src/extension.ts` is required for basic
declarative highlighting. Add procedural code only for an implemented capability.
The shared test area owns editor-specific incomplete inputs and classification
checks; parser corpus tests own tree shape and recovery. Read representative
`examples/syntax/` files directly instead of copying their sources. Keep expected
Zed captures and TextMate scopes adapter-specific, and do not duplicate compiler
semantic execution tests under `editors/`.

## ET01 — Consolidate and implement the LSP foundation

Scope:

1. Audit the old repository, record transferred/discarded responsibilities, and
   add `crates/kagari-analyzer` to the existing Cargo workspace and lockfile.
2. Implement the `kagari-analyzer` stdio executable using a maintained LSP transport
   and typed protocol model. Keep stdout exclusively for framed protocol messages;
   logs and startup failures go to stderr. Do not add an unused async runtime.
3. Implement `initialize`, `initialized`, `shutdown` and `exit`, including invalid
   lifecycle requests, unsupported requests and termination/EOF behavior according
   to LSP. Advertise only implemented capabilities.
4. Advertise full-document synchronization initially: `didOpen`, full `didChange`
   and `didClose`. Apply client text as overlays using the existing source owner;
   closing removes the overlay and restores a retained base or removes an
   overlay-only document from the current snapshot. Retain its source identity
   for reopen; closing does not require deleting the database identity record.
   Do not eagerly crawl projects or load dependency closures. Base restoration
   tests may seed retained base text without introducing disk loading.
5. Track URI, language ID and document version at the protocol boundary. Define
   behavior for duplicate opens, changes to unopened documents, stale versions and
   ranged changes inconsistent with full sync: no partial update or state corruption;
   invalid notifications are logged, not answered as requests. Handle reopen with
   a fresh protocol version lifecycle. Preserve previously retained snapshots.
6. Use standard URI parsing for local file URIs, including Windows drive paths
   and escaping. In this phase, explicitly reject non-local file authorities and
   UNC paths before source normalization; do not collapse them into local paths.
   UNC support remains deferred, not implicitly covered by Windows drive support.
   Support explicitly supplied non-file document text, including non-hierarchical
   URIs such as `untitled:Untitled-1`, without treating its URI as a disk path.
   Preserve scheme-specific identity without collisions with filesystem keys.
   Reuse and extend source normalization at its existing owner rather than
   inventing a second file ID system. Start with UTF-16 positions; reuse the
   existing line index when converting ranges, without adding a speculative
   second conversion implementation.

Known source boundary gaps to resolve in ET01: the existing
[normalizer](../crates/kagari-source/src/source_database.rs) recognizes virtual
sources only through `://`, so `untitled:Untitled-1` is currently treated as a
relative path. Its path normalization also loses UNC's leading double separator.
A standard URI parser at the protocol boundary alone does not repair either
source-key issue. Keep URI adaptation in analyzer and source-key normalization in
source; do not add another VFS or route document sync through complete-input
replacement, which clears overlays and removes identities outside its batch.

Acceptance:

- A bounded subprocess test exchanges real Content-Length-framed LSP messages,
  completes initialization/shutdown/exit and verifies stdout is protocol-only.
- Focused session tests cover open/change/close/reopen, version rejection,
  base-versus-overlay behavior, retained snapshots and URI normalization. Include
  an overlay-only reopen retaining its FileId with a fresh protocol-version
  lifecycle, a non-hierarchical virtual URI, escaped Windows drive URIs and
  explicit UNC/non-local-authority rejection without source-state mutation.
- Empty and malformed Kagari source can be synchronized without fake diagnostics.
- Unsupported language features are absent from capabilities; unknown requests
  receive the appropriate protocol error, not a fabricated empty success.
- No second frontend/database or runtime dependency on analyzer is introduced.
- Package builds and focused tests pass before ET02. Do not carry a deliberately
  broken workspace forward to make the skeleton appear complete.

No background analysis is needed in this phase. When future queries add jobs,
request cancellation and revision-checked result publication are mandatory; this
skeleton does not establish those unimplemented guarantees.

## ET02 — Define highlighting coverage and implement Tree-sitter

Proposed new owner: `editors/tree-sitter-kagari/`, containing `grammar.js`,
Tree-sitter metadata, generated parser assets and corpus tests. Add an external
scanner only if concrete lexical state requires it; document its state and recovery
contract. Pin generation tools and track assets needed for editor grammar builds.

Use the [syntax specification](spec/syntax.md), [EBNF](kagari.ebnf), actual lexer/
parser and conformance tests together. Resolve discrepancies at their owner instead
of silently treating an editor grammar as a new language specification. The current
Markdown operator overview is not an exhaustive token list.

Known syntax-document discrepancies to resolve at the start of ET02:

- EBNF `primary_expr` omits the explicit generic enum/member path represented by
  `Token<i32>::Empty`, although the syntax specification and parser support it.
  Correct the EBNF and reuse representative nested-generic path/constructor inputs
  from [existing structure tests](../crates/kagari-syntax/src/tests/parser/struct_expr.rs).
- EBNF and Markdown `method_decl` omit optional `async`, although the parser and
  specification prose support inherent async methods. Correct both productions
  and reuse the [async callable contract](../crates/kagari-syntax/src/tests/parser/functions.rs),
  including inherent methods, explicit closures and postfix await chains. Do not
  import Rust async blocks or prefix await; do not confuse syntax recovery for
  unsupported async trait methods with executable support.

These are existing documentation gaps, not authorization to change language
semantics or make editor grammars enforce static legality.

Coverage includes declarations, types/generics, statements, expressions, patterns,
attributes, closures and current async syntax. Do not implement type checking or
static legality constraints. Avoid catch-all rules that conceal missing grammar.

Required lexical/context boundaries:

- Reserved Kagari words, including `val`/`var`; no imported Rust `let`/`mut`/`dyn`,
  lifetime, character, byte-string or raw-string rules.
- ASCII identifiers, Unicode text in strings/comments and CRLF input.
- Nested block comments and line/documentation comment classification.
- Integer bases, separators, integer/float suffixes and exponents; distinguish
  `1..2`, `1f64`, `0xff_u8` and hexadecimal `0xff32`.
- Ordinary strings versus `f"..."`, escapes including `\u{...}`, `{{`/`}}`,
  expression holes and `:?`; blocks, comments, strings and nested interpolation
  inside holes must not terminate the outer context early. Ordinary string text
  and interpolated text segments cannot cross physical line breaks; expression
  holes may contain multiline code under ordinary whitespace/comment rules.
- The debug suffix consists of separate `:` and `?` tokens with trivia permitted
  between them, as in `f"{value: /* debug */ ?}"`. Do not mistake a type-annotation
  colon inside a closure or propagation `?` inside an expression for the suffix.
- Generic angle brackets versus comparison/shift operators, `===`/`!==`, bitwise
  and compound operators, propagation, closure bars and pattern alternatives.
  Include nested generic closers and explicit generic enum paths. Shift tokens
  require character adjacency: do not merge `> >` or `> /* comment */ >`.

For example, in `f"score={score + 2}"`, `score=` is string text, `score + 2`
receives code classifications, and the closing delimiter returns to string mode.
Do not tokenize a hole by searching for its first closing brace.

Reuse selected files under `examples/syntax/` as shared inputs, especially
`literals-and-comments.kgr`, `string-interpolation.kgr`, `type-inference.kgr`,
`generic-associated-types.kgr`, `bitwise.kgr`, `attributes.kgr` and `async.kgr`.
Reuse relevant lexer/parser cases for boundary selection. Keep only distinct
editor-specific incomplete-input fixtures under `editors/tests/`; semantic tests
are evidence of language behavior, not substitutes for capture assertions.

Acceptance: selected valid fixtures parse without unexpected ERROR/MISSING nodes;
corpus cases establish ambiguous structure and lexical boundaries; incomplete
inputs recover at documented boundaries. Include unterminated ordinary strings,
unterminated interpolated text and valid multiline expression holes, with LF and
CRLF, and verify the expected lexical context after each line boundary. For selected
edit sequences, incremental parses agree with fresh parses after adding/removing
quotes, braces and comment terminators. Reuse these boundary cases and selected
edit sequences for adapter classification tests. Do not require an actually
unterminated block comment to stop at an invented boundary simply to avoid
coloring the rest of a file.

## ET03 — Add the Zed language extension

Proposed owner: `editors/zed/`, with `extension.toml` and
`languages/kagari/{config.toml,highlights.scm,brackets.scm}`. Add scope overrides
only where needed for correct pairing in strings/comments. No Rust extension
crate or language-server registration is needed for this phase.

- Associate `.kgr` with Kagari and use the ET02 grammar.
- Highlight keywords, booleans, numbers, comments, strings/escapes, interpolation,
  operators, punctuation, attributes and reliably identifiable declaration names
  and type positions. Use Zed's supported theme captures, not fixed colors.
- Configure comment toggling and basic parentheses/braces/brackets/quote pairing.
  Do not globally auto-close `<` as `>` or add unsupported single-quote strings.
- Verify the monorepo grammar loading/build path with a development extension.
  First try a repository/revision reference with `path` set to
  `editors/tree-sitter-kagari`; do not assume a separate grammar repository is
  required. Supply a reproducible local setup with a resolved repository location
  and Git revision, without committing a machine-specific absolute path. If the
  selected Zed version actually requires staging a grammar repository/root, record
  the observed limitation and use a disposable build product from this tree, not
  a second maintained source repository.
- Record generation/WASM build prerequisites, tested Zed version and install steps.
  Marketplace registration and automated release packaging remain deferred.

Acceptance: queries compile and selected capture assertions pass; development
installation recognizes `.kgr`, renders representative fixtures and provides the
configured editing behavior without an analyzer process. Check light/dark themes
and incomplete-source recovery in the actual editor. Outline, text objects and
advanced indentation queries are not prerequisites.

## ET04 — Add the VS Code language extension

Proposed owner: `editors/vscode/`, with `package.json`,
`language-configuration.json` and `syntaxes/kagari.tmLanguage.json`.
Use language ID `kagari`, scope `source.kagari` and extension `.kgr`.

- Implement TextMate rules for the same baseline categories. Use recursive
  begin/end contexts for nested comments/interpolation, not a single giant regex.
- Give interpolation expressions a code context such as `meta.embedded.*` so
  bracket behavior is not incorrectly inherited from the surrounding string.
- Use standard TextMate scopes and conservative contextual classification. Do not
  pretend lexical guesses resolve symbols, constructors or arbitrary type names.
- Add declarative comments and basic bracket/quote behavior consistent with ET03.
  No runtime TypeScript activation code or language-client dependency is required.
- Test with the actual `vscode-textmate`/Oniguruma engine, carrying rule stacks
  across lines and asserting scopes and string/comment/code token categories.
  Reuse shared source inputs, but keep TextMate-specific expected scopes separate
  from Tree-sitter captures. For selected ET02 edit sequences, retokenize from the
  affected line using the valid preceding rule stack and compare with tokenization
  from the beginning of the edited document. Verify that quote, brace and comment
  edits do not leave stale string/comment state on later lines. Pin Node
  dependencies and use a lockfile.

Acceptance: targeted tokenization tests pass; an Extension Development Host and a
locally packaged VSIX recognize `.kgr`, handle the selected interpolation/comment
boundaries and work without `kg` or analyzer. Use the token/scope inspector and
light/dark themes for manual verification; screenshots alone are not evidence of
correct scope boundaries. Package no unnecessary test tooling or local paths.

## ET05 — Integration, documentation and retirement

- Run shared classification cases through both adapters. Define explicit expected
  code/string/comment regions for a small set of critical boundaries, including
  nested interpolation and newline recovery, and assert each adapter against
  those expectations; agreement between two incorrect outputs is insufficient.
  Require agreement on critical boundaries and baseline categories, not identical
  parse trees, token segmentation, scope names or RGB colors. Keep detailed capture
  and scope expectations adapter-specific. Record deliberate contextual limits.
- Add focused editor-tooling CI jobs for parser generation consistency, corpus/
  capture tests, TextMate tests and extension build/package checks. Keep Node and
  grammar tooling out of the runtime dependency graph. Add representative Windows
  analyzer coverage for URI and stdio boundaries; leave full matrices to CI.
- Document analyzer launch/protocol scope, local extension installation and the
  workflow for changing syntax plus both adapters. Record tested tool versions
  and any pending editor/platform verification without claiming it passed.
- Complete the old-repository preservation/reference audit and record readiness
  for its separately performed removal. Do not require that URL for builds/tests.
- Move lasting implemented ownership/contracts into architecture and tooling docs,
  move deferred IDE work to the roadmap, then remove this completed plan according
  to the documentation lifecycle. Git history retains the ledger.

## Verification policy

Each phase is a buildable checkpoint; no intermediate build failures are planned.
If one occurs, record the command, representative error, cause and owning repair
here. Do not repeat unchanged failures or equate skipped cases with passing tests.

Local selection:

- ET01: analyzer lifecycle/document-sync tests plus only directly affected source
  overlay/position/identity tests if source code changes.
- ET02-ET04: selected grammar, capture and TextMate boundary tests and actual
  development installs. Existing semantic execution suites need not be rerun when
  no compiler behavior changes. Complete grammar inventories belong to CI.
- ET05: missing cross-adapter and integration cases; reuse earlier passing evidence
  for unaffected code instead of rerunning everything for a fresh transcript.

Record exact test commands once targets exist; do not invent runnable target names
in advance. Use timeouts for protocol subprocesses and install/build checks. On
Windows, explicitly include escaped file URIs, CRLF and Unicode source text.

For implementation checkpoints run the repository's required local checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Full-workspace tests and complete feature/backend/platform matrices remain CI-only.
Do not reproduce them locally through many package commands. Documentation-only
changes need content/link/diff checks, not workspace builds or Clippy. Changes to
Rust public re-exports require reviewed whitelist entries; migration is not an
exception to structure/ownership rules. No format/runtime ABI bump is required.
Authorized implementation commits use phase trailers such as `Plan-Phase: ET01`;
this plan is not authorization to commit, publish or delete remote resources.

## Progress ledger

- [ ] ET01: consolidate useful skeleton material; working stdio LSP and document sync.
- [ ] ET02: shared coverage contract and Tree-sitter grammar.
- [ ] ET03: Zed development extension and capture/editing verification.
- [ ] ET04: VS Code grammar/configuration, tokenization tests and local VSIX.
- [ ] ET05: cross-adapter checks, CI integration, lasting docs and retirement audit.

Planning baseline: no implementation, runtime validation or editor installation
has been performed for this plan. No known intermediate implementation failures
are carried. Full CI and manual platform/editor acceptance remain pending.

Pre-implementation review on 2026-10-17: retain ET01-ET05 order and ownership.
Recorded ET01 source-key gaps and local-file/virtual-URI boundaries, ET02 syntax
production discrepancies and string/newline/debug-suffix boundaries, ET03 direct
monorepo grammar loading as the first path to verify, and ET04-ET05 edit-state and
independent classification expectations. Existing source overlays and retained
snapshots remain the foundation; no replacement database or broader IDE scope is
needed. These findings are implementation obligations in the phases above, not
completed implementation or installation evidence.

During execution, extend this ledger with decisions, focused validation results,
selected-test rationale, open failures and remaining CI/manual scope. Do not create
a parallel progress document. Completion requires actual delivery and required
checks; the existence of skeleton files or passing syntax-only checks is insufficient.
