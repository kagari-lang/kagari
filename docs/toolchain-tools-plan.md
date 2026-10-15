# Formatter, checker and linter implementation plan

Status: planning authorized on 2026-10-15; implementation has not started.
This plan owns the first usable `kg fmt`, `kg check` and `kg lint` milestone.
Command spellings and configuration examples below describe planned behavior,
not commands already supported by the current binary.

## Objective and boundaries

Provide project-wide formatting that preserves source meaning, a dependable CI
language checker, and a small configurable lint set with explicit false-positive
boundaries. Reuse the existing parser, HIR, package inputs and CLI native environment.

| Command | Responsibility | Source writes |
| --- | --- | --- |
| `kg fmt` | Whitespace and layout using lossless syntax; no type checking | Yes, except `--check` and stdin mode |
| `kg check` | Required syntax, resolution, type and language rules | None |
| `kg lint` | Required language checks plus optional code-quality rules | None |

For `fn answer()->i32{true}`, formatting succeeds and preserves the tokens;
check and lint report the return-type error. Disabling every optional lint must
not suppress that error. No command executes script code, and checking/linting
does not emit MIR, bytecode or native executable products.

The contracts in [syntax](spec/syntax.md), [packages](spec/packages.md),
[CLI loading](spec/module-loading.md#cli-loading) and
[source architecture](architecture.md#source-package-inputs) remain authoritative.
This work does not activate the Host API refactor, REPL, LSP, debugger, native
backend expansion, package publication or persistent compiler-cache work.

## Baseline and ownership

Baseline inspection at 5b801eee found:

- [The parser](../crates/kagari-syntax/src/parser/mod.rs) produces a recoverable,
  lossless Rowan tree. CST tokens retain whitespace and comments; typed AST child
  access alone does not enumerate every token or establish valid syntax.
- [SDK source checking](../crates/kagari/src/engine/source.rs) already reaches HIR
  `check_program`. Current CLI check does not need executable lowering.
- [Project acquisition](../crates/kagari-package/src/project.rs) resolves dependencies,
  constructs a lock candidate and captures complete root closures. CLI project
  commands separately publish the lock and materialize native declaration views.
- [Workspace inputs](../crates/kagari-package/src/workspace.rs) own discovery and
  member selection before graph resolution. This is the starting boundary for fmt.
- [Source diagnostics](../crates/kagari-source/src/diagnostic.rs) have severity,
  kind and byte spans. SDK diagnostics additionally carry file spans, labels and
  notes. CLI rendering currently prints codes and byte ranges without excerpts.
- [Resolved names](../crates/kagari-hir/src/resolver/resolved.rs) already distinguish
  local/parameter identities, lexical scopes, assignment places and closure
  captures. Lints must consume these facts rather than search identifier text.

| Owner | Responsibility in this milestone |
| --- | --- |
| New `kagari-format` | Pure CST-to-layout library and printer; depends on syntax/source, never HIR or runtime |
| New `kagari-lint` | Concrete rule registry, severity policy and syntax/semantic rules consuming existing analysis |
| `kagari-source` | Shared source identity, revision, ranges and presentation-neutral diagnostic data |
| `kagari-syntax` | Existing parsing, CST and AST; fix concrete formatter blockers here |
| `kagari-hir` | Existing resolution/type facts and narrowly required usage queries |
| `kagari-package` | Existing manifest/member selection and shared module-source discovery |
| SDK source integration | Analysis handoff using the same sealed native declarations as compilation |
| `kagari-cli` | Arguments, config discovery, filesystem IO, terminal/JSON rendering and writeback |

Create each new crate with its first concrete implementation, not as an empty
scaffold. Keep optional-rule policy out of type checking and rendering out of HIR.
Do not add a generic tooling facade or duplicate name/type resolution. Shared
diagnostic data must remain usable without the SDK/runtime. Adapt existing SDK
diagnostics coherently; avoid parallel diagnostic implementations or speculative
text-edit frameworks before this milestone has a consumer.

## Architecture contracts

The reference designs inform these boundaries, not a requirement to reproduce
their crate graphs or infrastructure:

| Reference | Adopt for Kagari | Adaptation boundary |
| --- | --- | --- |
| [Clippy lint passes](https://doc.rust-lang.org/nightly/clippy/development/lint_passes.html) | Syntax rules and semantic rules have different required inputs | Reuse Kagari's own HIR facts and shared-object/capture semantics |
| [rust-analyzer architecture](https://rust-analyzer.github.io/book/contributing/architecture.html) | IO-free analysis queries, retained snapshots and presentation-oriented output | Use existing Kagari snapshots; no second semantic engine, incremental database or IDE facade |
| [Prettier printer](https://prettier.io/docs/technical-details) | Syntax rules construct a layout document; a printer measures it and chooses breaks | Preserve Kagari CST tokens/comments and interpolation semantics |
| [Biome linter](https://biomejs.dev/linter/) | Formatting policy belongs to the formatter | Lint rules must not duplicate whitespace, indentation or line-width checks |

The intended data flow is:

```text
CLI input/configuration -> captured sources -> lossless CST -> formatter -> text
                                              |
                                              +-> HIR analysis snapshot
CST + analysis snapshot -> lint findings -> level/suppression policy -> diagnostics
analysis snapshot -> required language diagnostics
analysis snapshot -> check_program -> CheckedProgram -> existing compilation
diagnostics + retained source revisions -> terminal/JSON renderers
```

### Recoverable analysis and strict compilation

Project inputs must reach this boundary before recovery can help. Currently
[`ModuleTree::load`](../crates/kagari-package/src/modules.rs) returns on the first
syntax diagnostic, and [`ProjectInputs::load`](../crates/kagari-package/src/project.rs)
aborts the request if any selected root fails capture. TT01 must address this
handoff before promising multi-error reporting or independent-root continuation.

Keep source discovery/capture at `kagari-package`, with an explicit distinction
between unavailable input, captured text with syntax diagnostics, and incomplete
module discovery. Share module discovery rules rather than implement a second
loader in CLI. Tooling may consume discovered source and diagnostics; incomplete
discovery is never a complete `ProgramInputs` or checked compilation result.
Whole-workspace manifest/resolution failures remain request failures, while a
root-local source failure must not prevent processing other independently available
roots. Do not silently change lock publication on partial failure: specify and
test the timing against existing behavior in TT01. TT03 reuses this discovery
boundary for fmt without activating dependency resolution.

Lint consumes `AnalysisSnapshot` and the relevant file/body facts, not only a
successfully constructed `CheckedProgram`. The former can retain diagnostics and
partial facts; the latter remains evidence of an error-free dependency closure.
Do not weaken `check_program` or manufacture checked evidence for erroneous input.

```text
fn broken() -> i32 { true }

fn healthy() -> i32 {
    val unused = 10;
    42
}
```

Check reports the return-type error. Lint also reports the unused binding in
`healthy` when its required facts are valid. Build cannot obtain a `CheckedProgram`.
HIR owns the validity of recovered facts; a missing binding/type is unknown, not
proof of absence. Establish the affected file/body boundary before running a rule;
an error that invalidates shared signatures may affect more than one body.

The current [`AnalysisResult`](../crates/kagari-hir/src/lib.rs) pairs facts with
diagnostics but does not by itself certify each body's required facts; diagnostic
lists can also be truncated. Absence of a diagnostic within a function's span
therefore cannot prove complete resolution or valid type facts. TT01 must validate
a narrow HIR-owned eligibility/query contract for complete binding facts, required
type facts and shared-signature failures; TT04 implements and consumes it. Reuse
existing evidence where available, adding only what these rules require. Do not
recheck each function through a second analysis path to establish eligibility.
Cover truncated diagnostics and errors outside the body as well as ordinary local
errors. Parser/analysis exhaustion or cancellation must not become a clean result.

### Diagnostic and policy ownership

Language checking keeps its typed language-error kinds; lint owns its `LintId` and
rule findings. Do not add every optional rule to the language `DiagnosticKind` enum.
Both adapt to one source-owned presentation record containing code, final severity,
revision-bound ranges, labels and notes. Terminal/JSON rendering consumes that
record and retained source text, without querying HIR or keeping HIR objects in
the output model. Never render a revision-A span against revision-B source text.

Rule evaluation does not read configuration or print messages. The lint policy
layer applies levels and declaration suppression to optional findings; mandatory
language diagnostics bypass optional-rule policy. Keep native declaration setup
at its existing owner. Shared diagnostic/policy integration must not introduce
a production dependency from HIR to the linter or from source-free runtime to HIR.

### Reuse and scheduling

Use one coherent analysis handoff within a lint invocation. Query existing facts
and share a body usage walk where several rules need the same read/write/capture
classification. Do not analyze the project separately for each rule. Changing
external lint severity on unchanged inputs must not invalidate name/type facts;
source edits, provider changes and package-environment changes still obey existing
invalidation rules. This does not promise reuse between separate CLI processes.

Start with static rule registration and ordinary modules. Current SDK analysis
state uses `RefCell`; immutability of a retained result alone is not proof that the
whole engine or snapshot is safe to share across threads. Do not add per-rule
threads, a daemon, a second query database or persistent caches. If measured costs
later justify concurrency, first establish the ownership/Send/Sync and cancellation
contracts for independent files, bodies or source sessions.

## Common command and input contract

First-release invocations:

```text
kg fmt [file.kgr] [--check]
kg fmt --workspace --check
kg fmt -p app
kg fmt -
kg check [file.kgr] [--message-format human|json]
kg check --workspace --locked --message-format json
kg lint [file.kgr] [--message-format human|json]
kg lint -p app -D warnings
```

- No file selects the current project using existing invoking/default-member
  semantics. Support `--manifest-path`, `--input-root`, repeated `-p` and
  `--workspace`; preserve the existing conflict between `-p` and `--workspace`.
- Explicit files retain standalone source semantics; do not silently reinterpret
  them as package roots or combine them with project-selection flags. First
  release accepts one explicit file per invocation, matching the current CLI.
- Check/lint share the existing standard and CLI host declarations. No implicit
  remote acquisition, host package catalog or new host-plugin loader is introduced.
- Check/lint retain `--locked`, `--offline`, `--frozen` and existing shared lock
  publication. They may materialize declaration views, but do not write executable
  products. Fmt rejects resolution-only flags and never reads/writes a lock,
  resolves dependency contents or materializes declarations.
- `kg fmt -` reads stdin and writes formatted source to stdout without project
  discovery. Initial stdin mode uses defaults plus explicit CLI formatting options;
  project configuration and check/lint stdin inputs are deferred.
- Validate conflicting/irrelevant options before processing inputs. Reject JIT,
  executable-output and unsupported fix options for these modes.
- Fatal workspace discovery/resolution/configuration errors stop that request.
  Root-local source acquisition and syntax failures follow the recoverable-input
  contract above: report incomplete roots and continue independently available
  inputs. Cancellation stops further work; summaries distinguish incomplete work
  from successfully checked/formatted inputs.

Exit status proposal: 0 for success (including warning-only lint); 1 for language
errors, denied lints, fmt differences under `--check`, acquisition/IO failures or
cancellation; 2 for invalid arguments/configuration. Diagnostics distinguish the
failure categories. Keep existing check exit behavior where already defined.

Terminal diagnostics include code, severity, path, line/column, source excerpt,
primary/secondary labels and actionable notes. JSON is a documented stream with
diagnostic records and one final summary; stdout contains no progress chatter or
ANSI escapes. Operational failures also have structured records. Use stable
ordering by normalized path, source offset and code, with deterministic tie breaks.
Identify shared-dependency diagnostics by source revision and semantic context;
do not merge distinct root-dependent errors just because their text matches.

Keep byte ranges and source revisions internally. Specify UTF-8 offset and
line/column conventions, end-exclusive ranges, Unicode/tab rendering and spanless
diagnostics before implementing JSON; do not expose process-local IDs as durable
file identities. This is a tooling-output contract, not an artifact ABI change.

## Configuration and diagnostics decisions

TT01 must resolve the following in this document before consumer implementation:

| Decision | Starting proposal and required evidence |
| --- | --- |
| Config location | Prefer one dedicated tool config at the owning workspace/package root, avoiding executable-input fingerprint changes for style-only edits; choose its filename and explicit-file discovery boundary |
| Config precedence | Defaults, project config, enclosing declaration policy, CLI overrides; unknown fields/rules/levels are errors |
| Format options | Indent width 4, maximum width 100, newline mode `preserve` by default; specify mixed-newline normalization and stdin defaults |
| Lint levels | Six named rules default to `warn`; allow individual `allow`, `warn`, `deny`; `-D warnings` promotes remaining warnings without reviving explicitly allowed rules |
| Suppression | Reuse supported outer attributes on modules/functions/declarations, with string rule identifiers if required by the existing attribute grammar; no inner `#![...]` or new local-statement attributes |
| Suppression validation | Prefer the existing `tool::` namespace; specify which of lint, CLI check/build and direct SDK checking validate rule names/arguments, and the dependency owner for each |
| Suppression inheritance | Define outer/inner declaration precedence and whether a module declaration's policy follows external module files; do not infer cross-file ancestry from byte spans |
| JSON | Freeze the initial record fields, position conventions and summary semantics using concrete success/error examples |
| Fmt source selection | Reuse member and declared-module discovery without dependency resolution; define recovery when an invalid file prevents discovering child modules |
| Recoverable input | Define per-root discovery results, completeness evidence, retained syntax diagnostics and lock publication on partial failure before completing check reporting |

No environment-global config, nested config cascade, plugin system, arbitrary
rule groups or per-path override language is required initially. Source suppression
must be recognized by ordinary check/build as tooling metadata; it cannot become
an unknown-attribute error outside lint or suppress required language diagnostics.
Specify permitted targets, inheritance and invalid arguments alongside the syntax.

HIR already preserves structured attributes under `tool::` without treating them
as unknown language attributes. Prefer that extension point over adding bare
`allow`/`warn`/`deny` attributes to the language. For a proposed spelling such as
`#[tool::allow("unused_parameter")] mod child;`, TT01 must specify whether the
policy reaches `child.kgr`, how nested overrides work and where rule-name validation
runs. The existing attribute `target_span` alone cannot represent that ancestry;
use declaration/module identity and the source graph for inherited policy. HIR
owns those facts, and the tooling policy owner interprets them. Checking names in
additional commands requires an explicit tool-layer integration, not HIR importing
the rule registry. No command may accidentally reject valid tooling metadata as a
language error or silently treat an invalid policy as an applied suppression.

## Formatter acceptance

Use CST-aware formatting rules that produce a small layout representation
(text, indentation, groups and break opportunities), followed by one printer.
Width decisions belong to the printer; comment attachment belongs to the formatter.
Do not build another parser or print by concatenating AST accessor results.

Keep syntax rules, comment attachment and layout printing in separate ordinary
modules within `kagari-format`. Syntax rules emit layout elements, comment handling
assigns leading/trailing/dangling comments, and the printer measures groups and
chooses breaks. A call rule must not reimplement the printer's line-width algorithm.
Test comment attachment independently of line width, and test the printer with
small layout documents independently of Kagari parsing. The library returns text
or diagnostics; CLI owns writeback and concurrent-edit checks.

First-release coverage includes all currently supported ordinary source forms:

- Functions, structs, enums, traits, impls, constants, type forms, modules, use trees
  and attributes; generic parameters/arguments, bounds and where clauses.
- Bindings, calls/chains, operators, assignment places, collections, closures,
  control flow, patterns/match, async/await and propagation forms.
- Width-aware lists and expressions, consistent blank lines and a final newline.
- Line and nested block comments, including dangling comments and trailing comments.
  Preserve their text and semantic attachment; do not reflow prose.
- Literal spelling, string escapes and interpolation text. Format interpolation
  expressions without changing surrounding string contents or inserting line breaks
  that become literal data.

Preserve non-whitespace tokens in the initial implementation: no import sorting,
declaration reordering, redundant-parenthesis removal, trailing-comma insertion or
other token rewrites. Add grammar-specific separators only as whitespace where
re-lexing proves the original tokens remain distinct. Width is a target, not an
instruction to split indivisible literals/comments.

Malformed syntax leaves that file unchanged and emits diagnostics. A semantic
error alone does not block fmt. Unsupported valid syntax must produce an explicit
diagnostic during development, never silently drop text or claim full coverage.
Do not mark TT03 complete while ordinary grammar forms are unsupported.

At TT02 start, record a bounded grammar coverage inventory in this plan against
the current syntax kinds, grammar and existing fixtures. Each form needs a rule,
representative coverage and an explicit completion state; update the inventory
deliberately when language changes affect this milestone. TT03 closes that fixed
inventory. Do not let "all syntax" expand silently during implementation, or count
an unchanged-text fallback as support for a form's formatting rules.

Project fmt owns only files of selected members, including their declared modules.
It does not follow their dependency closures, scan arbitrary `.kgr` trees, or edit
generated native declarations. Deduplicate canonical files and retain existing
filesystem containment/alias rules. When recovery prevents discovering a child,
report incomplete discovery rather than pretending every file was checked.

Write only changed files. Stage output and use a supported per-file replacement
operation so a write failure cannot leave a truncated source. Check that the source
still matches the captured input before replacement; report concurrent edits.
Preserve applicable file permissions. Document the host-controlled filesystem
assumption and lack of a whole-project transaction; successful files may already
be written when a later file fails. `--check` never writes even temporary source
replacements. Verify replacement behavior on Windows and Unix in their CI jobs.

Before writeback implementation, TT03 must fix the policy for symbolic links,
directory links/junctions, hard links and read-only files, including whether the
write target is the requested path or canonical referent. Existing approved-root
reads permit internal symbolic links; replacing a link path can destroy the link,
and replacing a hard-linked file can sever its shared identity. Preserve the
documented relationship or explicitly reject unsupported writeback before changing
it. Do not silently turn links into ordinary files or change permissions to force
a write. Cover applicable link behavior and unchanged-file cases on each platform.

Acceptance combines expected-output fixtures with idempotence, reparse success,
non-whitespace token preservation, comment order/attachment and normalized CST
structure equivalence. Reuse representative grammar fixtures without rewriting
the entire example corpus in the implementation commit. CRLF, Unicode, malformed
input, empty files and cancellation belong in the selected boundary coverage.

Token preservation checks must cover grammar-sensitive adjacency. The current
parser combines adjacent angle tokens into shift operators: an unchanged sequence
of lexer kinds after whitespace removal does not prove equivalent syntax. Include
`>>`/`<<`, nested generic closers, line-comment termination and interpolation
boundaries in reparse/CST-equivalence cases. Preserve literal text and compare
non-whitespace tokens explicitly; do not drop comments using a generic trivia filter.

## Checker acceptance

Keep existing syntax, module/name/visibility, type, generic/trait, mutation,
return and other required language checks under their current owners. Checking
library roots requires no `main`. Semantic acceptance must agree with the same
checking stage used by build; later lowering/backend failures remain build errors.

The implementation work is common diagnostics, machine output, command validation
and independent-root reporting. Preserve lock timing, declaration environments,
capture validation and cancellation. A failed root does not invalidate previously
reported diagnostics or justify skipping other independently captured roots.

Do not create persistent caches or add a separate fast checker. Investigate
concrete repeated analysis before introducing reuse beyond existing snapshots.

## Linter acceptance

Run required checks and optional rules against one coherent source/analysis
snapshot per selected root. Lint must not invoke a complete check command and then
parse/type-check the same input again. Source-invalid regions may still support
syntax rules, but missing/recovered semantic facts never count as proof of a lint.
Skip affected semantic rules and retain the actual language diagnostics.

Use a static registry with rule ID, default level, documentation and required
stage. The dispatcher supplies one of two input contexts:

| Stage | Inputs | Initial consumers |
| --- | --- | --- |
| Syntax | CST, declaration shape and source locations | Naming checks that require no resolved contract |
| Semantic | Matching analysis snapshot, resolved bindings, references, places, captures and types as needed | Unused bindings/parameters, unnecessary `var`, and naming checks requiring trait-implementation context |

Select the stage for the complete rule, including exemptions: a method-naming
check that respects an implemented trait cannot infer that relationship from text.
Syntax-stage eligibility does not exempt the lint command from required language
checking. Facts and queries remain at HIR; rule selection and judgments remain
at the linter. No extensible pass framework or dynamic rule loader is required.

| Rule ID | Scope and safeguards |
| --- | --- |
| `unused_binding` | Unused local/pattern bindings by resolved identity; handle shadowing and closure uses; distinguish reads/writes and exempt intentional underscore names |
| `unused_parameter` | Parameters of source bodies, including closures where applicable; exclude bodyless signatures and implicit receiver bindings; support underscore intent and declaration suppression |
| `unnecessary_var` | Local `var` that can be `val`; account for initialization, direct/compound assignment and writes from nested closures, not just lexical occurrences |
| `non_snake_case` | Functions/methods, modules, fields, locals and parameters; do not recommend renaming a trait-implementation member independently of its declaration |
| `non_upper_camel_case` | Named types, traits, generic type parameters and enum variants; define acronym and Unicode policy before implementation |
| `non_upper_case_globals` | Declared constants, including associated constants; ordinary `val` bindings are not constants |

Rules inspect user-authored selected members by default. Dependencies still receive
mandatory language checking; generated/provider declarations receive no optional
style diagnostics. Cross-root reuse must preserve the analyzed package environment.

For example:

```text
val unused = service.record_event();
```

Report the unused binding without suggesting deletion of the call or its side
effects. First release supplies explanatory notes, not executable edits or `--fix`.

For `unnecessary_var`, changing an object's member is not rebinding the variable:
Kagari `val` does not impose deep immutability. Conversely, a closure that rebinds
the variable prevents this suggestion. Deferred initialization and capture-mode
effects require evidence from the current semantics; conservatively skip shapes
whose conversion has not been established safe. Naming conventions must never
change identifier resolution, normalize source names, or demand ASCII-only names.

Each rule needs a stable ID, rationale, defaults, supported scope, positive and
negative examples, diagnostic spans and documented limitations. Provide rule
listing/explanation through the CLI without loading a project. Unknown suppression
names and malformed policy are errors in lint, not silent no-ops. TT01 specifies
the validation behavior of other commands and SDK consumers as described above.

## Execution phases

Implement in order; each phase ends with a coherent buildable checkpoint. Record
concrete diagnostics and an owning repair phase if a boundary cannot build. Do not
continue by weakening checks, stubbing production behavior or hiding test failures.

| Phase | Deliverable | Focused acceptance |
| --- | --- | --- |
| TT01 | Resolve the decision table, recoverable acquisition handoff and HIR eligibility interface; source-owned diagnostics/renderers, strict config and complete check reporting | Existing CLI check/project cases, independent roots after source/syntax failure, lock timing, HIR eligibility feasibility, revision-correct JSON and HIR-independent rendering |
| TT02 | Fix the grammar inventory; `kagari-format` syntax/comment/layout modules and single-file/stdin commands | Independent comment/printer tests, initial grammar forms, adjacency/interpolation, idempotence, malformed-file preservation and no HIR dependency |
| TT03 | Close the grammar inventory; reuse recoverable discovery for project fmt, implement explicit link/writeback policy and `--check` | Selected members, unresolved dependency independence, incomplete discovery, platform link/read-only behavior, alias/concurrent-edit/write failures |
| TT04 | `kagari-lint`, static registry, HIR-owned eligibility, policy/suppression, six rules and shared analysis | Rule cases, captures/writes, mixed valid/invalid bodies, truncated diagnostics, shared-signature errors, cross-file suppression, selected-source scope and analysis reuse |
| TT05 | Focused cross-tool acceptance, user docs/examples and CI wiring | fmt/check/lint on one representative workspace, direct SDK/check agreement, JSON/exit contracts and production dependency guards |

TT01 owns specification decisions and the concrete acquisition/eligibility handoff,
not speculative lint or formatter scaffolding. Record eligibility feasibility
evidence and the selected API before TT01 closes; a larger required analysis
refactor needs an explicit scope update rather than being discovered during TT04.
TT02 may have explicitly diagnosed incomplete syntax coverage; TT03 closes it.
TT04 must finish all six rules or explicitly revise this plan's accepted scope.
Use `Toolchain-Tools-Phase: TTxx` commit trailers for implementation checkpoints.

## Verification and completion

Follow [repository verification policy](../AGENTS.md#verification-and-tooling).
Place unit/contract tests beside their source owners; retain CLI tests for actual
arguments, output and filesystem behavior. Do not add one integration executable
per rule or grammar form. Reuse existing fixtures and contract tests before adding
distinct boundary cases.

Architectural acceptance checks are mandatory in the relevant phases:

1. TT04: the mixed valid/invalid function example retains its language error and
   reliable lint, while the same input still fails strict program checking.
2. TT04: within one session, applying different lint levels to unchanged source and
   provider inputs reuses semantic facts. Verify the shared check/lint handoff does
   not repeat parsing/type checking, using existing query-reuse evidence or a focused
   test probe; do not introduce a production telemetry framework for this test.
3. TT01/TT02: render diagnostics from presentation records and retained text without
   HIR objects; test comment attachment and layout printing at their separate owners.
4. TT01/TT03: a syntax error in package A retains diagnostics and incomplete status
   where appropriate; independent package B still runs. Missing/ambiguous modules
   never become complete compilation inputs. Assert lock behavior on partial failure.
5. TT04: diagnostic truncation or a shared-signature error cannot make unavailable
   facts appear valid. Test the agreed suppression behavior across `mod child;`
   and its external source, including invalid names and nested overrides.
6. TT02/TT03: grammar-sensitive token adjacency survives formatting, and writeback
   honors the agreed symbolic-link/hard-link/read-only policy without altering
   unsupported inputs. Complete the local platform's selected cases; CI covers the rest.

These checks validate ownership and behavior, not internal struct layouts or a
performance claim. Source revision mismatch must not silently produce a misleading
excerpt; unrelated valid-body facts must not be inferred from recovery placeholders.

At each phase select only affected package/target/test filters and a small directly
related regression set. Record the selection rationale and failures here. Examples
of command shapes once their corresponding crates/modules exist:

```text
cargo test -p kagari-cli --bin kg -- project::tests::
cargo test -p kagari-format --lib -- <affected-module-filter>
cargo test -p kagari-lint --lib -- <affected-rule-filter>
cargo clippy -p <affected-package> --lib --tests -- -D warnings
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
git diff --check
```

Choose the CLI binary target instead of `--lib` for CLI Clippy. Do not combine
package runs into a disguised full-workspace sweep. Review new imports, visibility,
re-exports and dependency direction; update existing feature graph guards where
new dependencies need enforcement. Production runtime/source-free consumers must
not acquire formatter/linter/HIR dependencies through this work.

CI owns complete workspace tests/Clippy, grammar coverage matrices, full
feature/backend/platform validation and platform-specific file replacement cases.
Focused local evidence and pending CI must be reported separately. Performance
checks, if a concrete issue requires them, separate parsing, analysis, rule/layout
work and IO and record machine, toolchain, profile, features, cache state and
parallelism. No cache or speedup claim is an acceptance substitute.

On completion, move lasting CLI/config/JSON/rule contracts to specifications and
guides, architectural ownership to architecture, and unresolved follow-ups to the
roadmap. Remove this completed plan; Git owns its ledger. Pending CI alone is not
a reason to keep a completed implementation plan.

## Deferred scope

Range/on-type formatting, comment reflow, import sorting, formatter skip regions,
check/lint stdin overlays, watch/daemon/LSP, per-path config, custom rule plugins,
autofix and persistent caches are deferred. Later lint candidates include unused
imports, dead declarations, ignored Result values, unreachable code, dead stores,
expression simplification and measured performance advice. Each requires its own
semantic justification, especially host-callable exports, trait lookup, traps and
side effects. None is silently added to this milestone.

## Progress ledger

- [x] Record first-release scope, ownership, phase order and validation policy.
- [x] Incorporate reference-informed architecture: recoverable snapshots, staged
  lint rules, diagnostic/policy ownership, formatter modules and focused acceptance.
- [x] Incorporate execution review: acquisition recovery, fact eligibility,
  suppression ownership/inheritance, link writeback, token adjacency and grammar scope.
- [ ] TT01: resolve command/config/diagnostic decisions, acquisition and eligibility
  contracts; complete check tooling.
- [ ] TT02: single-file formatter foundation and stdin integration.
- [ ] TT03: complete syntax coverage and project formatting.
- [ ] TT04: lint policy and six initial rules.
- [ ] TT05: focused acceptance, documentation migration and CI integration.

Planning checkpoint: implementation unstarted; no carried implementation failures.
Documentation validation covers relative links, content consistency, line endings
and diff hygiene. No Cargo builds/tests are needed for this documentation-only change.
