# Structured HIR import paths execution plan

## Status and authority

**Status: active; SA15-01/SA15-02 complete, SA15-03 next.** On 2026-10-19 the user authorized
phased implementation of [SA15](review.md#sa15-hir-import-paths-should-retain-segments-instead-of-joined-text)
with one commit per phase. The
[roadmap](implementation-roadmap.md#review-follow-up-order) owns activation and queue
placement. This document owns the finite SA15-01 through SA15-03 sequence, decisions,
acceptance and progress ledger while the task is active.

Planning inspected `5654dd8e` with an initially clean worktree. No behavior tests,
Rust builds or performance measurements were run during preparation. Source facts
below are observations, not newly implemented contracts or proof of a bottleneck.
Recheck affected files and existing user changes before activating implementation.

## Outcome and scope

Replace `Import.path: String` with owned `segments: Vec<String>`, retaining those
segments through `ImportDirective`, package/relative-path normalization, catalog
traversal and syntactic dependency-prefix matching. Join text only for diagnostic
payloads or a genuine existing text API boundary, not between internal traversal
steps. Preserve current accepted programs, diagnostics, source attribution,
resolution outcomes, provenance and snapshot validity.

This is a representation correction, not a language or resolver redesign. It does
not implement SA9 module handles, symbol interning, SA1 inline AST reuse, SA11 self-ID
cleanup, SA14 local functions, artifact pruning, new import syntax or a new solver.
Do not migrate all expression/type path storage, change `ModuleIdentity`, add a
second namespace index, or broaden host lookup and package selection policies.
Executable contracts/backends continue to consume checked targets, not import syntax.
No format/runtime ABI bump or compatibility reader is required for this unpublished
HIR API change; do not add aliases/accessors to preserve the obsolete string model.

### Ownership and one concrete example

For the following source inside a nested module:

```text
use super::api::{sum as add, ops::*};
```

Proposed HIR leaves are:

```text
Import { segments: ["super", "api", "sum"], kind: Named { alias: Some("add") }, ... }
Import { segments: ["super", "api", "ops"], kind: Glob, ... }
```

- **Written paths:** HIR and directives own these flattened spellings, including
  relative roots and package aliases. The glob's final `*` and the alias are not
  path segments. Preserve leaf order and the existing import slot identity.
- **Normalized lookup inputs:** the catalog interprets `super` relative to the
  importing module and selects candidate packages/components. These may still name
  members, not just physical modules. Do not overwrite the written path with them.
- **Resolved facts:** existing `ModuleIdentity`, `NamespaceId`, `SourceDeclRef`,
  `ResolvedTarget` and binding origins retain their current owners and validity.
  A re-export may resolve somewhere different from the written facade.
- **Physical sites:** SourceMap retains each segment token's original byte range,
  separately from semantic path data. Grouped leaves may share ancestor sites;
  `root_span` and terminal `span` retain their existing distinct meanings.

This removes the required joined spelling and complete-prefix string copies from
this import path pipeline. `Vec<String>` can still allocate/clone, normalization
may still own components, and `ModuleIdentity` still contains owned paths. No net
allocation, retained-byte or elapsed improvement follows without measurement.

### Mature reference and applicability

Rust HIR's [Path](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_hir/hir/struct.Path.html)
and [PathSegment](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_hir/hir/struct.PathSegment.html)
retain segment structure rather than reparsing a joined spelling. These rolling
nightly API documents were consulted on 2026-10-19; they are not pinned source or
performance evidence. Borrow this representation separation only. Kagari keeps
owned strings, its existing resolution facts, source snapshots and definition
contexts; Rust's arena lifetimes, interned Symbol and complete HIR model are not
requirements. No new path framework is needed for this concrete migration.

## Affected owners and migration boundary

Paths below are relative to `crates/kagari-hir/src/` unless linked elsewhere.

| Owner | Required work |
| --- | --- |
| [HIR import records](../crates/kagari-hir/src/hir/item/module.rs), [module examples](../crates/kagari-hir/src/hir/item/mod.rs) | Replace the stored spelling with segments; derive unaliased local names from the terminal segment; update source-to-field docs. Preserve kind, visibility, leaf/root spans and import ordering. |
| [Item lowering](../crates/kagari-hir/src/lower/item.rs) | Replace the temporary `ImportPath.text` accumulation and complete-prefix strings. Flatten groups with branch-local segment/site state; preserve recovery, aliases and the correspondence used by native declaration imports. |
| [SourceMap](../crates/kagari-hir/src/source_map.rs) | Replace import-only `(String, Span)` prefixes with structural prefix correspondence. Prefer an exclusive segment-end index plus the existing physical Span; resolve `segments[..end]`. Do not store another full prefix vector per site or change expression/type path maps. |
| [Directive facts](../crates/kagari-hir/src/imports/mod.rs), [resolution orchestration](../crates/kagari-hir/src/imports/resolve.rs) | Retain written segments in directives; pass them to lookup/dependency preparation. Determine terminal prefix by written segment count, not normalized length; join only when building textual diagnostics. Update glob-target diagnostics as well. |
| [Namespace catalog](../crates/kagari-hir/src/imports/catalog.rs) | Change `package_path`, `absolute`, `resolve_path` and `path_dependencies` to consume structure. Remove `absolute` joining suffixes only for `resolve_path` to split them. Keep one traversal implementation and existing lookup observations. |
| [Import reuse](../crates/kagari-hir/src/imports/cache.rs) | Compare written segments instead of `a.path == b.path`; retain kind, visibility, dependencies, resolution outcomes and existing arena-aware target comparisons. This does not activate R8's reuse redesign. |
| Text-based lookup consumers | Keep the required boundary of `resolve_name` for [body resolution](../crates/kagari-hir/src/resolver/resolve.rs), [declarations](../crates/kagari-hir/src/declarations.rs) and [target queries](../crates/kagari-hir/src/analysis/target_queries.rs). Adapt text once into the shared traversal; do not add a second resolver or keep obsolete import APIs as forwarding aliases. |
| Navigation, documentation and native declarations | Check [namespace queries](../crates/kagari-hir/src/analysis/namespace_queries.rs), [analysis queries](../crates/kagari-hir/src/analysis/mod.rs), [documentation queries](../crates/kagari-hir/src/analysis/documentation_queries.rs) and [native import_source](../crates/kagari-hir/src/native/api.rs). Preserve smallest physical-site selection, terminal categories, alias/provenance and trusted native category assignment by import slot. |

`imports/resolve.rs` orchestrates lookups; relative/package normalization currently
belongs to `NamespaceCatalog::package_path`. Solver observations and direct module
dependencies are distinct: failed/pending lookups still wake reverse watchers, while
`path_dependencies` retains syntactic module/facade edges even if the terminal is
missing. Keep both; terminal-target-only dependencies are not equivalent.

## Contracts to preserve

| Boundary | Required behavior and focused checks |
| --- | --- |
| Flattening and names | Preserve grouped leaf order, repeated ancestor prefixes, aliases, unaliased terminals and glob paths without `*`. One real leaf remains one directive; traversing namespaces does not create synthetic imports/bindings. |
| Relative roots/packages | Preserve `crate`, `self`, repeated `super`, root underflow, package dependency aliases, installed/native aliases, ambiguity and physical module collisions. Normalize borrowed written segments without mutating their owner. |
| Namespace/access | Intermediate prefixes use Type; named terminal sites retain all eligible Type/Value results. Preserve native category restrictions, strong/glob/implicit precedence, self-avoidance proofs, visibility and re-export limits. No arbitrary winner for dual/ambiguous results. |
| Source positions | Prefix end counts refer to written segments, never normalized components. Each site remains the original segment-token range, not a reconstructed whole-prefix range. Preserve alias/leaf/root ranges, UTF-8/CRLF bytes and inline-module physical offsets. |
| Recovery | Missing tree/name/alias, empty recovered paths, trailing separators and empty groups must not acquire unchecked `.last().unwrap()` or new successful bindings. Characterize current lowerings/diagnostics before editing; do not silently fix parser/recovery behavior in this task. |
| Dependencies/publication | Preserve syntactic facade edges, canonical target/provenance edges, namespace/InputUniverse observations, cancellation, work limits, cycle support and failure atomicity. Failed analysis must not replace a published graph/cache. |
| Reuse/retention | Segment equality replaces string equality only at the same predicate. Preserve source file/revision/arena checks and retained snapshots; do not reuse a target in a newer HIR arena or weaken invalidation based only on canonical target equality. |
| Text boundaries | Existing diagnostics retain their spelling and physical sites. Existing text APIs keep their own parsing/fallback rules; do not apply catalog empty-component filtering to stricter public export selection, or change qualified expression/type recovery incidentally. |

For an empty recovered named path with no alias, the proposed `local_name()` result
is `None`, rather than fabricating an empty segment to reproduce the old inert
`Some("")`. Confirm all consumers in SA15-01: import binding currently filters names
through `LocalName::new`, so no empty binding should appear. Record this explicit
internal helper change while keeping observable diagnostics/recovery unchanged;
an explicit usable alias still takes precedence. Stop for a scope decision if the
replacement requires a language-visible recovery change.

For SourceMap, define and document the prefix-end invariant before implementation:
nonempty sites identify an existing written prefix, aliases add no segment and
star adds no segment. Empty recovered leaves may have no sites. Validate production
of these indices and handle missing recovery data without unchecked slicing;
do not silently omit valid navigation to hide a mismatch. No second semantic source
of path strings belongs in SourceMap.

## Execution sequence

### SA15-01 — Confirm the baseline and freeze the contract selection

After explicit implementation authorization:

1. Inspect status/relevant diffs and refresh the owner inventory, including public
   Rust consumers, generated native declaration input, source-map accessors and
   reuse equality. Use compilation/search to find consumers, not guessed wrappers.
2. Characterize valid grouped/named/glob paths and malformed-source recovery with
   existing parsing/lowering helpers. In particular check empty leaves, missing
   aliases, `use a::;` and empty groups. Record actual syntax diagnostics, lowered
   segments-to-be, local names and physical sites; do not assume they are accepted.
3. Confirm prefix-end representation, textual entry boundaries and their existing
   empty-component/fallback policies. Choose one shared structured traversal with
   borrowed inputs; keep normalized outputs transient, not a persistent path cache.
4. Run the small core baseline selection below. Map the contract rows to existing
   cases, add only distinct missing cases at their owners and record exact filters,
   counts and any pre-existing failures here before replacement.

Exit: concrete representation/source-map decisions and a finite consumer/test list,
not a new language design or performance promise. Do not expand into SA9/SA1 if their
current owners are sufficient. No performance harness is required absent a claim.

### SA15-02 — Replace the representation coherently

Migrate HIR `Import`, lowering's temporary path, import SourceMap sites, directive
facts, catalog normalization/traversal/dependencies and cache comparisons as one
buildable checkpoint. Adapt shared text consumers to the same traversal and update
relevant assertions/source-to-field documentation in the same slice.

Remove obsolete string fields and internal join/split round trips directly. Do not
retain a parallel resolver, compatibility field/getter, public forwarding entrypoint,
broad re-export or duplicated complete-path cache. Preserve required existing text
APIs as real boundaries, not compatibility bridges. Do not change solver scheduling,
query ownership, import grammar or independent export-selector validation.

Run the core contract selection after the migration. On failures, batch related
repairs and rerun failing/affected cases only. Temporary cross-file edit breakage
is not a completed checkpoint: no disabled validation, fake success or production
stubs may be committed. If the intended boundary cannot build, record diagnostics
and their owner here and finish the coherent integration before claiming completion.

Exit: production imports retain segments through structural consumers; diagnostics
and actual text boundaries are the deliberate remaining string materializations.
All existing consumers compile, core contracts pass and no mandatory internal
join-then-split loop remains in the migrated import pipeline.

### SA15-03 — Validate dependents and hand off

Run the selected downstream cases below for navigation, retained snapshots/cache
invalidation, native declaration imports and compiler source-free export behavior.
Reuse passing unaffected core results rather than rerunning the sequence for a new
all-green transcript. Check every contract row, including relative roots, native
categories, unresolved-prefix dependencies and malformed source; close only actual
coverage gaps, not alternate-spelling smoke cases.

Review changed handwritten Rust for ownership, imports, visibility, re-exports,
module boundaries and effective LOC. Do not split unrelated large files or add a
new path abstraction merely to satisfy style. Run the required local checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Move the lasting source/segment/prefix-site contract into the HIR model rustdoc and
[HIR guide](architecture/hir.md), with an architecture link if needed. Remove the
completed SA15 review entry, update the roadmap's remaining order and hand off SA9
without activating it. Git owns completed phase logs; retire this plan once required
implementation/local acceptance and documentation handoff are complete, with pending
CI scope recorded in the roadmap. Do not call unrun CI passed.

Use Conventional Commits with `Plan-Phase: SA15-01`, `Plan-Phase: SA15-02` or
`Plan-Phase: SA15-03` trailers for the authorized phase checkpoints.

## Focused validation and acceptance

### Core baseline and replacement checks

Run from the repository root with bounded command deadlines. These filters select
import contracts, not the entire HIR crate. Confirm nonzero test counts; an early
build failure or timeout does not count the selected tests as passed.

```text
cargo test -p kagari-hir --lib imports::provenance_tests::
cargo test -p kagari-hir --lib wildcard_imports_detect_conflicts_and_reject_nonmodule_targets
cargo test -p kagari-hir --lib scoped_visibility_allows_parent_tree_and_rejects_outsiders
cargo test -p kagari-hir --lib reexports_cannot_widen_private_items_or_modules
cargo test -p kagari-hir --lib failed_solves_preserve_published_snapshots_and_allow_recovery
```

Existing owners are [provenance](../crates/kagari-hir/src/imports/provenance_tests.rs),
[import semantics](../crates/kagari-hir/src/imports/tests.rs),
[solver failure/publication](../crates/kagari-hir/src/imports/solver_tests.rs) and
[lowering](../crates/kagari-hir/src/tests/lower.rs). SA15-01 must select or extend
focused relative-root, recovery and import-prefix navigation cases there if no
existing case establishes the exact boundary; record their final filters here.
Inspect every written prefix, not only successful final name resolution. Checking
the new `segments` model is appropriate at the HIR representation owner, but does
not replace observable semantic/navigation assertions in dependent tests.

### Downstream regression selection

```text
cargo test -p kagari-hir --lib analysis::namespace_tests::
cargo test -p kagari-hir --lib imports::cache_tests::
cargo test -p kagari-hir --lib inline_module_queries_use_physical_offsets_and_stable_child_identity
cargo test -p kagari-hir --lib dependency_overlay_invalidates_cached_imports_and_preserves_old_snapshot
cargo test -p kagari-hir --lib explicit_variant_imports_shadow_prelude_in_calls_patterns_and_navigation
cargo test -p kagari-hir --lib native_view_must_match_authoritative_signatures_bounds_members_and_visibility
cargo test -p kagari-compiler --lib source_free_exports_preserve_aliases_cycles_and_separate_entry
```

These cover shared qualified lookup callers, reuse across edits, physical child
routing, old snapshots, relative/aliased variant lookup, generated native views and
checked export handoff. The compiler case is a directly affected dependent, not an
artifact format migration. Preserve its meaningful source-free assertions rather
than replacing them with a source-only compile test.

Full-workspace tests and complete feature/backend/platform validation remain CI-only.
Do not run every HIR/compiler test or a package-by-package equivalent workspace
sweep locally. Broaden the small selection only when a concrete changed consumer
or failure demonstrates additional impact. Documentation-only plan preparation
uses link/content/diff checks and does not need Rust builds or Clippy.

### Structural and performance evidence

Acceptance requires one segment representation through the migrated structural
pipeline, no obsolete `Import.path`/`ImportDirective.path` storage, no copied full
prefix strings in the import SourceMap, and unchanged observable contracts. Review
remaining join/split uses by owner; unrelated expression/type paths and required
text entrypoints are not automatically failures or permission to expand scope.

There is no predeclared speed or memory improvement gate: this task corrects model
ownership without a demonstrated bottleneck. Do not label fewer conversions as a
measured speedup. If making a performance claim, freeze the pre-edit revision and
use the same bounded import workload/toolchain/profile/features/parallelism/cache
state; measure source preparation, import analysis, allocations and retained bytes
separately. Reuse the existing ignored solver workload where suitable, keep probes
under `target/`, and retain methods/results/limitations in the performance document.
A performance claim is optional; inventing a new benchmark framework or starting
SA9 to manufacture a win is outside scope.

## Progress and decision ledger

- [x] Planning: inspect current owner chain and prepare this finite plan.
- [x] Obtain explicit implementation authorization; inspect then-current user work.
- [x] SA15-01: characterize recovery/source sites, choose shared interfaces and record core baseline/coverage gaps.
- [x] SA15-02: replace the import representation coherently; compile and pass selected core contracts.
- [ ] SA15-03: complete dependent checks, structural review and lasting documentation handoff.

### Planning record — 2026-10-19

Source inspection confirms joined HIR/directive paths, accumulated SourceMap prefix
strings, catalog join/split round trips and path-sensitive reuse. It does not
establish a correctness defect or measured cost share. The user requested this
plan only; no implementation decisions were represented as completed code, no
behavior tests/benchmarks were run and no carried build/test failures were inferred.
Known integration errors at activation must be recorded with reproduction and owner
here rather than silently folded into SA15 or waived.

### SA15-01 — 2026-10-19

Activated by the user's phased-execution/one-commit-per-phase request. The initial
changes were the prepared SA15 plan and its review/roadmap links; all are part of
this authorized task. No unrelated work was present.

Decisions: HIR/directives own `Vec<String>`; import SourceMap sites hold exclusive
written-segment end counts plus physical spans. Catalog structural entrypoints
consume borrowed `&[String]`; the existing `resolve_name(&str)` boundary preserves
its root/suffix fallback rules and converts once for the shared traversal. It is
not a compatibility alias. Normalized package components stay transient; expression/
type path storage and strict export selectors remain unchanged. Missing source-map
sites yield no navigation; an impossible produced prefix end must fail as an internal
invariant, never silently omit otherwise valid navigation.

Recovery baseline: `use ;` and `use {};` produce an empty named leaf; `use a as ;`,
`use a::;` and `use a::{};` retain `a`, with no explicit alias. The first/trailing
separator inputs report ExpectedUseTree, missing alias reports ExpectedUseAlias,
and empty groups have no parser diagnostic. In isolated import graphs all report
UnknownName at the original leaf range and create no binding. Empty paths currently
return inert `Some("")` from local_name; SA15-02 deliberately changes that helper to
None without changing diagnostics/bindings. Prefix ranges address segment tokens;
package-root navigation can be absent or fall back to an enclosing directive.
Repeated super prefixes preserve their written sites even when normalization reaches
an absent package root or underflows. These observations are frozen by owner tests,
not patched as new language behavior.

Validation: the five pre-edit provenance tests and four named core commands above
passed (nine tests). Extended the grouped provenance case for all prefix sites and
UTF-8/CRLF, and added two distinct recovery/relative-root boundary cases. Initial
new navigation expectations failed; source behavior was inspected and expectations
corrected before accepting the baseline. Focused reruns passed all three changed/new
cases, so all seven provenance cases and four other core cases have passing evidence.
No selected test remains skipped. Structure (1,377 files, zero violations), fmt,
workspace all-target Clippy with denied warnings and diff checks passed. The IDE
build endpoint timed out; Cargo test compilation and workspace Clippy succeeded.
No product build failure or performance claim is carried into SA15-02.

### SA15-02 — 2026-10-19

Replaced HIR/directive path strings with owned segments and import SourceMap prefix
strings with exclusive segment-end indices plus token spans. Lowering produces both
in lockstep; navigation checks those ends against the matching written segments.
Catalog normalization, traversal and syntactic dependencies consume borrowed slices;
only diagnostics join the written import segments. The retained text lookup boundary
parses into this shared traversal, keeping literal scope roots, raw suffix-emptiness
category selection and fallback only on Missing. Unqualified successful lookups do
not allocate a copied root/path vector. Expression/type path storage, module identity,
solver policy and native import slot order are unchanged.

The five core commands above passed all 11 cases (seven provenance plus four named
contracts). `cargo test -p kagari-hir --lib analysis::namespace_tests::` passed five
shared-caller cases after the final text-boundary adjustment; reuse this evidence
in SA15-03. All changed consumers compile through tests and workspace all-target
Clippy. Structure (1,377 files, zero violations), fmt and diff checks passed.
Independent read-only review found no correctness or ownership issue. The planned
empty-path helper change is covered while diagnostics/no-binding assertions remain.
No compatibility field, new re-export/dependency, format bump, measured speed claim
or integration failure is carried. Final downstream acceptance/handoff is SA15-03.
