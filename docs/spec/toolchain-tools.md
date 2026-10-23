# Toolchain tools

`fmt`, `check` and `lint` are source tools. `disasm` inspects a validated artifact
without source files; see [KBC disassembly](#kbc-disassembly). None executes scripts.
`check` and `lint` use the same sealed native environment and required language
checks as CLI compilation;
neither emits MIR, bytecode or native products. See [CLI loading](module-loading.md#cli-loading)
for dependency resolution, filesystem boundaries and execution commands.

## Commands and selection

```sh
kg fmt                         # format selected project members
kg fmt --workspace --check     # report differences without writing
kg fmt -p app --max-width 80
kg fmt script.kgr              # standalone file; no project discovery
kg fmt - < script.kgr > formatted.kgr
kg check --workspace --message-format json
kg check script.kgr
kg lint -p app -D warnings
kg lint script.kgr -A unused_binding
kg lint --list-rules
kg lint --explain unnecessary_var
```

Without a file, discovery starts at the nearest `Kagari.toml`, or the manifest
selected by `--manifest-path`. `--input-root` narrows the approved read boundary.
`-p NAME` selects packages; `--workspace` selects all members. These selectors
are mutually exclusive. Otherwise the existing invoking/default-member rules
apply. Explicit files cannot be combined with project/resolution selectors.

Project formatting uses the workspace's **declared members** and their declared
inline/external modules. A local path dependency becomes a formatting candidate
only when explicitly declared as a workspace member. Formatting does not resolve
dependency contents, require a lockfile, read generated native declarations, or
scan arbitrary `.kgr` files. `--locked`, `--offline` and `--frozen` do not apply to
fmt. Check/lint retain ordinary package resolution and lock flags, including
implicit local dependency membership. Optional lint findings are restricted to
authored files of selected roots; dependencies still receive mandatory checks.

A malformed module retains all captured syntax diagnostics and prevents discovering
its children. Independent siblings and roots continue where possible. Missing,
unreadable, ambiguous or aliased modules never yield complete compilation inputs.
Check/lint report the affected root as incomplete and analyze other complete roots.
Manifest/resolution failures remain request-wide. Cancellation stops further work.
A lock candidate is published only after every selected root is acquired completely,
before semantic checking. Check/lint may materialize the existing declaration cache;
these side effects are distinct from source writes and executable emission.

Only fmt supports stdin (`-`). It writes formatted text to stdout and diagnostics
to stderr; `--check` is rejected with stdin. Check/lint support `--message-format
human|json`; fmt reports to stderr. Rule help loads no project or engine. Tool
commands reject execution/JIT/output options. There is no autofix mode.

Exit codes are 0 for success (including warnings), 1 for language errors, denied
findings, invalid source suppression, formatting differences under `--check`, or
input/write failures, and 2 for CLI/project-tool configuration errors. Invalid
suppression cannot be allowed away. Unknown CLI rule IDs or levels are errors.

## Configuration and precedence

Read `Kagari.tools.toml` only at the owning workspace root, or the standalone
package root outside a workspace. Explicit file/stdin mode uses defaults and flags
without ancestor configuration discovery. Tables and fields are strict:

```toml
[format]
indent_width = 4
max_width = 100
newline = "preserve"

[lint]
unused_binding = "warn"
unused_parameter = "warn"
unnecessary_var = "warn"
non_snake_case = "warn"
non_upper_camel_case = "warn"
non_upper_case_globals = "warn"
```

Indent width accepts 1–16; maximum width accepts 20–1000. Newline accepts
`preserve`, `lf` or `crlf`. Preserve selects CRLF only if every existing newline
is CRLF; mixed, empty and newline-free input selects LF. CLI `--indent-width`,
`--max-width` and `--newline` override configuration. Width is a target: literals,
comments and other indivisible text may exceed it. Style settings are not
executable inputs and do not affect compiler fingerprints.

Each rule accepts `allow`, `warn` or `deny`. Levels apply in this order:

1. Registry default, then project configuration.
2. Source attributes from outer to inner declaration scope.
3. Explicit `-A RULE`, `-W RULE` or `-D RULE` (last explicit level for a rule wins).
4. `-D warnings` promotes remaining warnings; allowed findings remain suppressed.

```text
#[tool::allow("unused_parameter")]
mod child;
```

The module annotation applies to `child` and descendants, including external
files. A declaration inside `child` can override it with
`#[tool::warn("unused_parameter")]`. Multiple positional string IDs are accepted.
Empty lists, unknown IDs, named arguments, non-string arguments and assigned-value
forms are invalid. Invalid policy is reported as `KG_LINT_POLICY` and is not applied.
Targets are existing supported declarations (functions/methods, types/traits/impls,
constants, modules and uses); no statement attributes or new syntax are introduced.

The linter owns this validation. Ordinary check/build and direct SDK checking
preserve `tool::` metadata opaquely, so malformed optional policy does not become a
language error in those paths. Other tool metadata remains opaque. Required parser,
resolution, typing, declaration and boundary diagnostics never enter lint policy.

## Six initial rules

All six default to **warn**. Findings include source locations and explanatory
notes, with no executable edits. The static [registry](../../crates/kagari-lint/src/registry.rs)
is also exposed by `--list-rules` and `--explain RULE`.

| Rule | Scope and rationale | Warn / accepted example | Limits and exemptions |
| --- | --- | --- | --- |
| `unused_binding` | Local and pattern bindings with no resolved reads; distinct shadowed identities stay distinct | `val unused = record_event();` / `val _unused = record_event();` | Closure reads count; writes alone do not. An initializer may have effects, so deletion is never suggested. |
| `unused_parameter` | Unread parameters of source function and closure bodies | `fn f(value:i32) {}` / `fn f(_value:i32) {}` | Excludes bodyless signatures, implicit `self`, and underscore names. |
| `unnecessary_var` | Initialized local `var` with no rebinding | `var count=1; consume(count);` / `var count=1; count+=1;` | Direct/compound writes and nested closure writes count. Member mutation is not rebinding. Captured variables and deferred initialization are conservatively excluded. |
| `non_snake_case` | Functions/methods, modules, fields, locals and parameters | `fn BadName() {}` / `fn good_name() {}` | Trait implementation methods follow the declaration and are exempt independently; the trait declaration itself can warn. |
| `non_upper_camel_case` | Named types, traits, generic type parameters, associated types and enum variants | `struct bad_name {}` / `struct GoodName {}` | Acronyms such as `HTTPServer` are accepted; internal underscores are discouraged. |
| `non_upper_case_globals` | Top-level and associated constants | `const limit:i32=1;` / `const LIMIT:i32=1;` | Ordinary `val` bindings are not constants. |

Naming uses Unicode case predicates without normalizing identifiers or proposing
ASCII conversions. The current identifier grammar is ASCII. Leading underscores
are allowed, digits are neutral, snake case rejects uppercase, UpperCamelCase
requires an uppercase first non-underscore character and no internal underscores,
and global constant names reject lowercase.

The two type/constant naming rules use syntax. The other rules require matching
HIR evidence, including trait ownership for method exemptions. Local facts are
available only for bodies that completed under valid shared inputs. A local type
error leaves other healthy bodies eligible; parser/declaration/import or shared
signature errors conservatively disable semantic rules across the snapshot.
Diagnostic truncation cannot turn incomplete facts into valid facts. Syntax rules
can still run on recovered snapshots; required errors always remain errors.

For example, lint reports both the required error in `broken` and the optional
unused binding in `healthy`, while strict compilation still rejects the program:

```text
fn broken() -> i32 { true }
fn healthy() -> i32 { val unused = 10; 42 }
```

## Formatting guarantees and writeback

The formatter covers ordinary declarations, generic/bounded types, use trees,
attributes, statements, places, operators, lists/chains, closures, control flow,
patterns, async/await, propagation, literals and interpolation. Syntax constructs
layout documents, comment attachment is tracked separately, and a dedicated printer
chooses line breaks. See [syntax architecture](../architecture/syntax.md#formatting).

Formatting changes whitespace only. It preserves non-whitespace tokens **including
comments**, literal spelling and interpolation text; it performs no import sorting,
parenthesis removal or comma insertion. Comments retain their text/order and
leading/trailing/dangling attachment; prose is not reflowed. Complete interpolated
strings are preserved verbatim, including physical newlines, continuation whitespace
and delimiter spelling, even when layout requests a different newline style.
Holes accept only identifiers with optional `:?`, not expressions or comments.
Output has a final newline. Reformatting with the same options is idempotent.

Before returning output, the library reparses it and verifies the token sequence
and normalized CST structure. Both checks matter: equal lexer tokens alone do not
preserve adjacency-sensitive shifts, generic closers or closures. Malformed input
or a preservation failure leaves the file unchanged with diagnostics. Semantic
errors alone do not block formatting, so `fn answer()->i32{true}` is formattable.

Only changed files are written. Reads retain captured contents, metadata and file
identity; writeback rechecks all three. A sibling temporary file is written,
permissions preserved, contents synced, and the destination revalidated immediately
before replacement. Failed staging/replacement cleans up the temporary file and
preserves the existing source. `--check` stages nothing.

Changed symbolic links (including directory links/junctions/reparse points), files
with multiple hard links and read-only files are rejected without altering their
relationships or permissions. The requested path is retained rather than silently
switching to its referent. Unchanged files need no writeback and succeed even when
linked/read-only. Approved-root read containment still applies. Unix and Windows
have identity-specific implementations; other platforms reject unsupported identity
acquisition. Standalone capture is bounded to 4 MiB per file.

This is per-file replacement, not a project transaction: earlier successful files
can remain written if a later file fails. Concurrent edits observed during capture
or before replacement are errors. A host-controlled filesystem is assumed; there
is an unavoidable interval between final validation and rename, and this is not a
transaction against hostile concurrent mutation. Ownership/ACL metadata beyond
applicable filesystem permissions is not promised.

## Output contracts

Source-tool human diagnostics use retained source excerpts and one-based positions; tabs use
four-column stops and Unicode text is preserved. The presentation record and
renderer require neither HIR objects nor a runtime. See [diagnostic ownership](error-reporting.md#source-tool-diagnostics).

JSON output is newline-delimited objects on stdout, ending with one summary.
Diagnostics have `type`, `context` (selected root), `code`, `severity`, `message`,
nullable `location`, `labels` and `notes`. Labels contain nullable `location` and
`message`. Locations contain slash-normalized display `path`, `revision`,
`byte_start`, `byte_end`, and `start`/`end` objects with `line` and `column`.
Ranges are end-exclusive; lines and UTF-8 byte columns are zero-based. No
process-local file ID is serialized. A missing/stale revision or invalid range
produces a null location with a note, never an excerpt from unrelated source.

Records sort by display path, byte offset, severity, code, root context and message.
Root-dependent diagnostics remain separate. Spanless IO/cancellation errors use the
same envelope. Summary counts selected roots for check/lint and captured files for
fmt; incomplete formatting operations/discovery are separate from the file count,
not a census of unknown children. Configuration failures may have no selected root.
Messages and excerpts are human text; codes and structured positions identify the
machine-readable finding. For a successful standalone check:

```json
{"type":"summary","command":"check","succeeded":1,"failed":0,"incomplete":0,"exit_code":0}
```

The summary's `exit_code` matches the process result. Argument parsing errors occur
before a report exists and use the ordinary stderr usage/error path. A failing
reported command may additionally print its short final error on stderr.

## Static initializer tooling

Formatting preserves dedicated module-level static declarations rather than
rewriting them as constants. Checking includes every initializer in the checked
closure even when execution never accesses its cell. Eligible initializer bodies
participate in ordinary local/parameter usage lints, including captured reads and
writes. HIR navigation, reference-to-definition facts and lexical/method completion
use `BodyOwner::Static` and resolved static declarations/import aliases; these facts
do not imply that the analyzer implements new semantic LSP handlers.

## KBC disassembly

```sh
kg disasm artifact.kbc
kg disasm artifact.kbc --list-functions
kg disasm artifact.kbc --function 'game::add_one'
kg disasm artifact.kbc --function '@0:3' > function.txt
```

`disasm` takes one explicit regular local file. It performs no source compilation,
project/configuration discovery, package resolution, directory recursion, source
lookup, provider installation or execution. It does not accept stdin or fetch URLs.
All modes use the same [complete offline artifact validation](artifacts.md#offline-disassembly-validation)
before emitting normal output; selecting one function cannot hide invalid code in
another function.

- The default prints every script bytecode function in stored module/function
  order, including private and generated bodies.
- `--list-functions` prints the artifact summary and compact function headers:
  numeric selector, qualified identity, retained concrete instance arguments,
  signature, instruction count, `may_suspend` and recorded effects. It omits
  instruction bodies, descriptors/constants and detailed slot/debug tables.
- `--function` prints one body and its referenced descriptors. The selector is an
  exact underlying qualified name or `@<module-index>:<function-index>`. Both
  indices are zero-based buffer slots in this artifact, not persistent identities
  across rebuilds or reloads. Quote selectors for shell portability.

Static descriptors include declaration identity, semantic and physical type,
visibility, mutability, supplier module and the supplier-local initializer selector.
An imported descriptor resolves its initializer against the supplier, not the
importer's function table. Selecting an otherwise-unused initializer still shows
its owning descriptor and referenced layouts. `state=runtime-only` explicitly
means no live cell state/value exists in the artifact; disassembly never initializes
state. Retained initializer source metadata remains diagnostic, not type authority.

Qualified names use package/module/declaration components joined with `::`, with
nonzero declaration occurrences retained as `#N`. Functions without a declaration
identity use their module and stored function name. Matching uses raw text, **not**
the quoted/escaped display spelling, and excludes displayed generic instance
arguments. There is no substring or regex matching. A name must match exactly one
function; ambiguity (including multiple generic instances) reports candidate
headers with numeric selectors and retained type arguments in stored order. Use
`--list-functions` to discover slots and numeric selection for ambiguous or awkward
generated names. Missing names and malformed/out-of-range numeric slots fail.
Native imports expose contracts, not KBC function bodies.

Output is plain UTF-8 text on stdout without terminal styling; diagnostics go to
stderr. Redirect stdout for a file; `-o` is not supported. Repeated selection
switches, combining `--function` with `--list-functions`, extra operands, and
unrelated execution/JIT, project, formatting, lint or message-format flags are
usage errors. No `--offline` flag is needed or accepted: this command is always
offline.

| Status | Meaning |
| --- | --- |
| `0` | Successful output, including final flush |
| `1` | File/read/write/flush failure (including a closed pipe), decoding/identity import/validation failure, or function-selection failure |
| `2` | Invalid command arguments |

Validation and selector resolution precede normal output. A later write failure
can leave partial text; rendering stops at the first IO error rather than retrying
discarded buffered output. See [bytecode disassembly output](bytecode.md#disassembly-output)
for instruction indices, references, types and nonauthoritative debug annotations.
The text is deterministic for the same artifact/options, but is **not a stable
machine protocol**. There is no JSON mode, source decompilation, assembler/text
round trip, bytecode editing, MIR/native-code browsing or raw-invalid-artifact mode.
