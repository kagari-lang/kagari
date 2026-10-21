# Inline module AST reuse execution plan

Status: activated on 2026-10-21; IA01-IA02 complete, IA03 next. This plan owns
IA01-IA05, design decisions, acceptance and the progress ledger for
[SA1](review.md#sa1-inline-modules-are-parsed-again). The
[roadmap](implementation-roadmap.md) owns queue placement. The user authorized goal execution and phase commits. SA14 and SA11 remain queued.

## Outcome and bounded scope

Parse each changed physical source once per preparation attempt and lower its
inline modules from the retained AST. Each logical module keeps its own source
identity, HIR arena, declarations and checked facts. Share immutable physical
text, line indexing and parse storage across those modules. Eliminate full-file
masking, child lexing/parsing and child line-index reconstruction.

Reuse SA9's context-owned module handles and retained namespace catalogs.
Preserve physical UTF-8 byte offsets, CRLF and editor position conversion,
visibility, import provenance, stable synthetic child file IDs within an analysis
database, parser recovery, cancellation and old snapshots. Declaration, signature
and body queries remain separate; backends continue consuming checked facts.

This does not implement incremental parsing, minimal dependency invalidation (R8),
specialization scheduling (R7), identity retention changes (R13), named local
functions (SA14), declaration ID removal (SA11), module syntax changes or runtime
changes. No artifact/ABI version bump or compatibility facade is required for
this unpublished implementation.

## Baseline and diagnosis

Planning baseline: `9e5113f8`, inspected with a clean working tree. SA9 is locally
complete; its outstanding CI matrix remains recorded separately in the roadmap.

| Owner | Current behavior and intended change |
| --- | --- |
| [Declaration preparation](../crates/kagari-hir/src/analysis/declaration_queries.rs) | Copies enclosing text, masks bytes outside each child body, creates a synthetic SourceFile and queues another parse/lower. Replace this with physical parse ownership and logical module work. |
| [SourceFile](../crates/kagari-source/src/source.rs) | Owns a String and LineIndex per source; inline_module accepts masked text. Separate immutable content from logical identity/range; preserve native authored-copy mappings. |
| [Parse](../crates/kagari-syntax/src/parser/mod.rs) | Owns a shared green tree and diagnostics, without file identity, revision or parse mode. Analysis must retain that association. |
| [AST nodes](../crates/kagari-syntax/src/ast/item.rs), [lowering](../crates/kagari-hir/src/lower/item.rs) | ModuleBlock already supplies items; lowering currently accepts AstSourceFile and records child headers only. Use one item loop and an independent Lowerer for each module. |
| [Analysis results](../crates/kagari-hir/src/analysis/mod.rs), [documentation](../crates/kagari-hir/src/analysis/documentation_queries.rs), [lint](../crates/kagari-lint/src/rules.rs) | Assume each result has an AstSourceFile root. Introduce explicit logical syntax views and migrate consumers. |
| [Attributes](../crates/kagari-hir/src/analysis/attributes.rs) | Filters child contents from recursively collected attributes. Collect at the logical owner and remove superseded filtering while preserving module policy inheritance. |
| [Signature reuse](../crates/kagari-hir/src/typeck/signature_reuse.rs) | Projects a body-elided surface from source.text(), currently masked for children. Use logical extents and explicit physical/projected coordinate translation. |

For example, consider one physical file:

```text
mod outer {
    pub fn value() -> i32 { 42 }
    mod inner { pub fn call() -> i32 { super::value() } }
}
```

Today preparation parses the physical file, a full-length masked copy for outer,
and another for inner. The proposed path parses once and lowers three item lists
into three arenas. Parents still own child declaration headers; children own their
contents. The value reference in inner still navigates to the original physical
file and byte range. Sharing syntax neither makes HIR IDs interchangeable nor
grants namespace access.

The architectural defect is treating a logical module as another source buffer
to parse. Per-module HIR allocation, imports and checking remain necessary. No
linear-time frontend or end-to-end compilation latency result is implied.

## Reference mechanisms

- [rust-analyzer syntax design](https://rust-analyzer.github.io/book/contributing/syntax.html),
  explicitly dated 2020-01-09, describes immutable green storage, typed AST views,
  absolute offsets on cursors, and retaining a root plus locator instead of a
  thread-local cursor. Apply immutable storage and attached views. Kagari retains
  the exact parse for old snapshots rather than reparsing a newer file by ID.
- [rust-analyzer architecture](https://rust-analyzer.github.io/book/contributing/architecture.html),
  living documentation consulted 2026-10-21, separates syntax from semantic
  services and retains recovered trees alongside errors. Keep that boundary;
  introducing Salsa or another query engine is unnecessary.
- [Rust compiler HIR guide](https://rustc-dev-guide.rust-lang.org/hir.html), living
  documentation consulted 2026-10-21, separates nested item contents and
  owner-local IDs. Apply that separation of containment and ownership while
  retaining Kagari's arenas, SA9 identities and existing resolution order.

The workspace uses Rowan 0.17.0. IA01 verifies locator and thread-safety details
against that pinned implementation; the historical article is not its API
reference. Kagari needs a small logical syntax view because analysis handles both
source roots and inline blocks while parsing correctly produces one physical root.
These references justify ownership choices, not unmeasured speed claims.

## Ownership and migration contract

### Shared content and retained syntax

SourceFile shares immutable text and LineIndex storage. An inline source retains
that content plus its original revision, physical origin, synthetic FileId,
logical module identity and body extent. Its text() exposes original physical
text with unchanged coordinates. Consumers use a logical extent when they mean
only that module. Replace the old inline_module signature and its callers directly.

Keep the existing `(parent FileId, child name)` ID owner. Revisions and arenas
distinguish edited or relowered children; DefinitionContext owns ModuleHandle
validity. Preserve duplicate-module and external/inline conflict diagnostics,
rename/removal behavior and exact-copy validation for native declarations.
Do not use byte ranges as persistent module IDs or treat generated text as authored.

Analysis retains one physical parse owner for a file/revision/policy and a logical
selector: root or a specific ModuleBlock. Prefer a typed locator tied to that
retained root over storing Rowan red nodes in published records. Resolve it only
against the exact owning parse and validate node kind/location. IA01 fixes the
representation and preserves existing snapshot Send/Sync requirements.

Transient AST views retain original root ancestry and offsets. Do not detach a
child green node into a zero-offset root, synthesize padded roots or parse on
demand to recover views. Avoid scanning the entire tree independently for every
child. Syntax owns AST operations; analysis owns source/parse/module association;
source gains no syntax or HIR dependency.

Replace the misleading AstSourceFile-only result API with an explicit logical
root-or-inline view and scoped traversal. Keep physical-root access explicit
where needed. Migrate callers together rather than retaining a second public API
with ambiguous behavior.

### Lowering and consumers

Use one item-lowering implementation for root items and ModuleBlock::items(), with
a fresh Lowerer and SourceMap arena per module. Discover children from immediate
items. Parent modules own child headers, header attributes and visibility; children
own their contents. Traverse ordinary function/trait/impl contents while stopping
at nested module bodies. Do not duplicate semantic lowering.

Documentation, lint rules and lint policy use the same logical boundary. Preserve
inner module documentation, item doc attachment, inherited module policy, written
signatures and physical navigation. Module documentation locations use the
appropriate logical extent. Audit descendant traversal and raw text consumers,
including signature surfaces and source/debug origins. Adapt projection arithmetic
without recreating full-file whitespace buffers. Remove old attribute filtering
only where the new traversal establishes the same ownership guarantee.

### Diagnostics and recovery

The physical parse is authoritative, including recovery and ParseLimits. Never
restart budgets for a child or reinterpret text in error nodes as successfully
parsed declarations. Lower only recovered AST items.

Store parse diagnostics once with that parse and create a deterministic logical
ownership projection for presentation. Body errors belong to the innermost
discovered module containing the primary site; child header/delimiter errors stay
with the declaring parent; file-wide budget errors stay with the physical unit.
Keep original kinds, spans and labels. Cross-boundary errors stay with their
declaring/physical owner. Preserve distinct errors at equal spans and never
deduplicate by message text.

IA01 settles zero-width EOF and missing-delimiter ownership against actual parser
output. Derive body extents from present brace tokens and recovery ranges, not
unconditional `start + 1 .. end - 1`. An unterminated body retains its last valid
token and useful declarations. A malformed node without a usable body must not
create a phantom valid child.

All query stages consume the same projection. Separately preserve physical parse
validity at checking and reuse gates: distributing display diagnostics must not
authorize code from an invalid source closure. Audit analyze_parsed, selected-body,
full-snapshot and synthetic child entrypoints together. Errorful tools remain
queryable; errorful compilation is rejected. Legacy duplicates or artifacts of
masked reparsing need not survive; explain changed recovery against the physical
parse and preserve meaningful coverage.

### Reuse and publication

Unchanged physical source and parse policy reuse the retained parse. A changed
revision supplies a new parse and child arenas. Stable child FileIds do not bypass
file/revision/arena, context-prefix, imports or namespace checks. Preserve current
signature/body reuse guarantees without promising finer sibling invalidation.

Poll cancellation during parsing, discovery, lowering, diagnostic projection,
graph preparation and before publication. Cancelled/older work cannot replace
newer caches. Old snapshots retain their original text, parse, source maps and
definition tables after edits or database destruction.

## Phases and commits

Run phases in order after activation. Each implementation checkpoint carries
`Plan-Phase: IA0N`; mark public API changes with `!` and explain their impact.
All phases should build. Unexpected failures must record command, diagnostic,
cause and owning follow-up here and in the commit body, without fake success.

| Phase | Work and acceptance | Proposed commit |
| --- | --- | --- |
| IA01 | Inventory consumers/tests; fix locator, diagnostic ownership, recovery extent and signature projection decisions. Capture the bounded baseline before removing masking. Record exact test filters and revisions. | `docs(hir): Establish inline AST reuse baseline` |
| IA02 | Share immutable text/line indexing; define retained views and the common root/block lowering input. Migrate source constructor callers. Verify physical mappings and direct child lowering. | `refactor(source)!: Share physical content for inline modules` |
| IA03 | Switch declaration preparation to physical parse plus logical views. Integrate diagnostics, attributes, docs/lint, snapshot APIs and signature projections. Remove masking, child reparsing and obsolete APIs; verify affected consumers. | `refactor(hir)!: Lower inline modules from retained syntax` |
| IA04 | Fill distinct missing recovery/identity/cancellation coverage at existing owners. Verify a representative source compiler path and source/debug attribution. Resolve carried errors and complete local quality checks. | `test(hir): Verify inline syntax ownership and recovery` |
| IA05 | Repeat matched measurements and record limits. Move lasting contracts to architecture/rustdoc, resolve SA1 in review/roadmap, and retire this plan. Keep unrun CI explicitly pending. | `docs(hir): Record inline AST reuse results and retire plan` |

IA02 may temporarily retain the old analysis path to keep its checkpoint buildable;
record that internal bridge here and remove it completely in IA03. It must not
create duplicate semantic lowering or compatibility entrypoints. Substantial scope
changes need explicit plan updates; unrelated findings return to review/roadmap.

## Focused validation

Reuse existing contract owners and add cases only for uncovered boundaries:

- HIR imports: `inline_module_queries_use_physical_offsets_and_stable_child_identity`
  and `private_inline_children_are_not_importable_from_other_modules`. Cover nested
  siblings, self/super, visibility and Unicode/CRLF physical locations.
- Declaration queries: stage separation, recovery/stale publication,
  `changing_parser_budget_invalidates_queries_without_changing_old_snapshots`
  and retained native declarations. Extend missing-delimiter, EOF and budget cases
  here or at the existing syntax recovery owner.
- Documentation: `same_named_declarations_and_inline_modules_keep_their_own_documentation`;
  add module inner-doc/retained-snapshot assertions only where missing.
- Lint: `policy_inherits_external_and_inline_modules_and_validates_even_when_allowed`;
  preserve inheritance and process child findings/attributes once.
- Source: existing exact-copy mapping coverage plus shared inline content,
  UTF-8/UTF-16 position conversion, revisions and native mapping boundaries.
- Signature/body reuse: selected body-edit remapping and cancellation contracts;
  extend an inline case for physical/logical projection where needed.
- Compiler: selected inline import/call or source-program behavior and source-origin
  coverage. Check unchanged execution/export/debug behavior without a backend sweep.

IA01 records exact package/target/test filters after checking coverage. Use
`cargo test -p <package> --lib <filter>` or the appropriate integration target;
use `--no-fail-fast` for multiple selected targets. After repairs rerun affected
cases only. Passing checks remain evidence for unchanged code.

At implementation checkpoints manually review changed Rust ownership, imports,
visibility, re-exports, functions and effective LOC, then run:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Documentation-only checkpoints require content, local-link and diff checks.
Full-workspace tests and the complete feature/backend/platform matrix remain
CI-only; targeted local results do not establish complete CI acceptance.

## Measurement and completion

Use a bounded user-source fixture family without native registration: a file
without inline modules as control, 32 sibling modules, and an eight-level nested
chain, with small valid functions/imports. Record exact generation, byte counts
and identities before baseline collection. Separate physical preparation from
graph resolution and, where practical, measure the complete declarations query.
Do not attribute SA9 graph savings to SA1.

Baseline/candidate use the same machine, toolchain, workspace profile/features,
default target directory and default parallelism. Separate compilation from timed
work; record warmup, cache state, samples/distribution and background load. Include
fresh preparation, unchanged-query reuse, one physical edit and retention of an
old snapshot while publishing a new one. Measure parse calls, full source-buffer
and line-index construction, allocation requests/bytes and elapsed time. Distinguish
requested retained bytes from RSS.

The mechanism gate is one physical parse on a cache miss and no inline masking,
child parser call or per-child full line index. Shared retention must not make
copies proportional to module count. Baseline/candidate valid semantic outputs
must match. Measure malformed fixtures separately because authoritative recovery
can remove legacy duplicates. Require correct contracts, passing selected tests
and quality checks, removed bridges and recorded measurements before completion.
Investigate material timing/allocation regressions before adding optimizations;
record variance and limitations. No percentage speedup is promised.

Temporary probes/logs belong under ignored target/. Keep reproducible methods and
results here until IA05 moves lasting evidence to
[performance-baseline.md](performance-baseline.md). Update
[HIR architecture](architecture/hir.md), relevant syntax/source rustdoc, review,
roadmap and README, then delete this completed plan. Git history owns its ledger.
Runtime performance tests are unnecessary unless implementation expands runtime
scope, which would require an explicit scope decision.

## Progress ledger

- [x] Execution plan recorded against `9e5113f8`; execution baseline `e7135c16`.
- [x] IA01 Baseline and contract decisions.
- [x] IA02 Shared physical content and logical lowering input.
- [ ] IA03 Analysis/consumer migration and bridge removal.
- [ ] IA04 Focused integration and local implementation acceptance.
- [ ] IA05 Measurements, durable documentation and plan retirement.

Planning validation: repository/source inspection, primary design references,
322 local file links across the four changed documents and diff whitespace checks.
No Cargo build/tests or performance measurements were run during planning.
Known implementation failures: none established; implementation has not started.
CI acceptance: unrun for this task. Record actual commands/results and carried
errors under the owning phase when execution starts.


### IA01 Baseline and decisions

Rowan 0.17.0 src/ast.rs implements SyntaxNodePtr as kind plus TextRange;
try_to_node follows containing children from the root, checking both kind/range.
Use that locator behind the syntax owner and retain an Arc<Parse> in analysis.
Transient logical views expose root/block items and a traversal that stops at
nested ModuleBlock nodes. Published records contain no red cursor. Native parse
mode remains attached through its existing preparation path.

Diagnostic projection uses actual body token extents. Missing closing braces at
EOF belong to the declaring parent; ordinary zero-width EOF body errors can
belong to the innermost open body. Parser-limit errors remain physical. Conservative
physical validity still guards optional facts/reuse and checked closure acceptance.
IA03 must verify synthetic-child entry behavior, not just root program checks.
Signature surfaces begin at the logical body extent, with an explicit base offset
for projection/expansion. No padding buffer is introduced for cache comparisons.

IA02 keeps the existing masked constructor call until IA03 switches analysis;
only immutable storage/common lowering and syntax capabilities change first.
The old constructor argument and masking are removed in IA03. This is the sole
migration bridge and avoids a temporary second constructor/API.

Baseline probe: cargo test -p kagari-hir --test sa1_probe --no-run, then the copied
test executable with --ignored --nocapture --test-threads=1, three processes.
The temporary probe is removed before commit. A forwarding System allocator counts
successful allocation/reallocation requests, gross requested bytes and net live
requested bytes on the executing thread; net excludes already retained inputs and
can be negative when old cache storage is released. One warmup and seven timing
samples per case/process, plus a separate allocation pass; destruction excluded.
Fresh queries include AnalysisDatabase construction. Unchanged queries retain the
first snapshot/database; edit queries build/retain an old snapshot outside timing,
apply an overlay outside timing, then measure declarations publication. Source
acquisition is excluded. No native modules are installed.

Fixtures are `// 中文😀\r\nfn entry() {}` (28 bytes), concatenated
`mod m{i} { pub fn f{i}() {} }\r\n` for i=0..31 (908 bytes), and eight
`mod m{i} { ` prefixes around `pub fn leaf() {}` with eight ` }` suffixes
(104 bytes). Logical module counts are 1/33/9; diagnostics are empty. Edits replace
each `() {}` with `() { val changed = 1; }`. Baseline source preparation parses and
builds line indexes 1/33/9 times by the existing queue; unchanged queries reuse
parses but still construct masked source buffers/line indexes. The probe measures
complete declarations, so graph and declaration resolution remain included; it
cannot attribute all time to parsing. IA05 compares the same boundary.

Windows x86_64 MSVC, Intel i7-10700F (8 cores/16 logical), rustc 1.99.1
(58cf8f9c3, 2026-10-13), LLVM 23.1.1; workspace test profile opt-level 1 with
DebugInfo, default HIR features/dev dependencies, default target and parallelism.
Compilation is excluded. Samples use warm process/workload caches; system load,
frequency and affinity are uncontrolled. Final paired measurements will check
for concurrent Cargo/rustc. Initial timing medians below use 21 samples each;
allocation columns show the third run and are not RSS or end-to-end compilation.

| Workload | Median microseconds | Requests | Requested bytes | Net bytes |
| --- | ---: | ---: | ---: | ---: |
| control/fresh | 31.1 | 180 | 35267 | 17191 |
| control/unchanged | 11.6 | 56 | 21087 | 0 |
| control/retained_edit | 38.8 | 234 | 39499 | 18232 |
| siblings/fresh | 1759.4 | 9523 | 1775252 | 1036359 |
| siblings/unchanged | 4141.2 | 21657 | 9445638 | -368 |
| siblings/retained_edit | 2433.5 | 10195 | 1530785 | 780254 |
| nested/fresh | 379.6 | 2280 | 298002 | 156037 |
| nested/unchanged | 424.8 | 2139 | 832272 | 0 |
| nested/retained_edit | 308.6 | 1818 | 279648 | 135356 |

Selected filters: HIR inline_module_queries, private_inline_children,
declaration_queries::tests, documentation_queries::tests::same_named_declarations,
analysis::tests::body_edits_reuse_other_bodies, analysis::tests::snapshots_reuse,
and oversized_source_module_identities; syntax module-view/recovery tests;
source source::tests; lint policy_inherits_external_and_inline_modules;
compiler source_program_keeps_module_identity and source-origin unit coverage.
Use narrower added test names for new boundaries. Final matrix remains CI-owned.
IA01 checks: probe outputs meet expected module/diagnostic shape; document links
and diff checks. Initial probe compilation rejected SourceDatabase::clone (not
implemented); the harness now creates each edit setup independently outside timing.
No production failure is carried. Separate preparation-only timing is deferred
unless needed to diagnose a material regression in the matched query boundary.


### IA02 Shared storage and logical syntax

SourceFile now retains immutable text/LineIndex in one shared content allocation.
ModuleSyntax exposes attached root/inline items, scoped descendant traversal,
inner docs and recovery-aware content extents. ModuleLocation wraps Rowan's checked
kind/range lookup; it is Send + Sync and must remain paired with its original parse.
One lowering loop accepts either view and collects only owned attributes.

The existing masked inline constructor remains the documented IA03 bridge; it
still constructs child content until analysis switches. No public constructor
signature changed in IA02, so this checkpoint is not marked breaking. Tests cover
Unicode/CRLF locations, unterminated bodies, missing open braces, scoped attributes,
shared clone storage and unchanged native copied-source mapping.

Passed: cargo test -p kagari-syntax --lib attached_module_views;
cargo test -p kagari-source --lib source::tests;
cargo test -p kagari-hir --lib inline_ast_lowering;
structure (1402 files, zero findings), formatting, workspace all-target Clippy and
diff checks. The checker initially found a std::iter use-site path; imported iter
explicitly and reran it successfully. No carried build/test failures. Manual review:
no production globs/reexports, no new source dependency, no LOC exception.
