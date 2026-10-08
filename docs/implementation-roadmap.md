# Kagari Implementation Roadmap

This is the single queue and progress owner for pending work. Detailed execution
plans own their implementation contracts; this roadmap owns activation, phase
order and progress. Implemented behavior belongs in [architecture](architecture.md)
and [specifications](README.md#language-and-execution-specifications).
Completed checklists, intermediate failures and execution logs remain in Git
history; the milestones below retain only outcomes and final checkpoints.

## Current baseline

The compiler-to-MIR/bytecode pipeline, source-free artifact validation, synchronous
native calls, registered GC storage, generic interface methods, collections/String,
scoped definition identities, installation-based access and cooperative
cancellation are implemented. Semantic declarations, executable contracts and
physical ABI have separate owners. Explicit standard/application registrations
serve analysis, tooling and source-free installation through one library flow.
Ordinary nominal enums and checked library-authored Try/FromResidual propagation
are implemented. Runtime-owned stores and checked IDs support automatic host
retention, exclusive thread transfer, typed conversion, object editing and prepared
member calls. Interpreter execution uses reusable register windows, prepared scalar
kernels and scalar/managed frame banks with explicit execution boundaries.

Immutable preparation, foundation registration reuse and shared interface metadata
are implemented. Measured costs and reproduction commands live in
[performance measurements](performance-baseline.md). Completed tracks have no
carried build/test failures; their completion does not establish performance gains.

## Pending work and open acceptance

### Name resolution (SA8, SA2, SA3, CI pending)

The [execution plan](name-resolution-plan.md) owns the two-space lookup/import
contract and subsequent dependency-driven solver migration. Status: NR01-NR05 implemented and locally accepted; full CI acceptance pending.
The completed phases carry `Resolution-Phase: NR01` through `Resolution-Phase: NR05` on
implementation commits. SA1 inline AST reuse and SA9 compact module handles remain
outside this track.

- [x] NR01: Specify type/value namespaces; replace binding, directive and exported
  alias storage and lookup APIs, including portable registration consumers.
- [x] NR02: Migrate semantic/tooling/cache consumers and derive HIR public members.
- [x] NR03: Complete focused SA8 local acceptance before changing solver scheduling.
- [x] NR04: Introduce dependency-driven import work and explicit convergence,
  unresolved-cycle, exhaustion and cancellation outcomes.
- [x] NR05: Complete integration/measurements and resolve local failures; record
  GitHub CI acceptance independently.

Only NR01 may carry documented mechanical consumer build failures to NR02. The
NR03 local gate must pass before NR04. Full workspace/feature/backend checks stay
in GitHub CI; unrun CI remains pending. Reuse core contract fixtures, adding cases
only for genuinely distinct missing semantics or boundaries.

NR01 checkpoint: replaced the sole binding model with per-category entries,
named/glob syntax, per-category directive outcomes and explicit lookup parameters.
Portable aliases now use `ExportName`; registration validates category agreement
and independent duplicate rules. Split per-module import contribution rebuilding
from graph orchestration; whole-round scheduling remains until NR04. No internal
compatibility adapters or version bump were added.

Validation: `DEVELOPER_DIR=/Library/Developer/CommandLineTools cargo check -p
kagari-types -p kagari-stdlib` passed. `cargo check -p kagari-hir` exposed mechanical
consumer migration errors. The focused runtime registration command
`cargo test -p kagari-runtime
installed_reexports_require_declared_and_installed_canonical_targets --lib`
is blocked by its HIR development dependency (20 compiler errors); no test ran.
Representative errors: E0061 missing `NameNamespace` in target/declaration/body/
pattern/constraint queries; E0599 old `DirectiveResolution::Resolved` in cache
comparison; E0609 old single-space fields in cache/declaration collection. NR02
owns all these errors and test caller migration. Logs: `target/nr01-hir.log`,
`target/nr01-registration.log`. Structure check passed (925 files, zero findings),
formatting and diff checks passed. Manual review retained existing visibility,
qualified identities and module boundaries; no structural exception was added.
Full CI acceptance remains pending.

NR02 checkpoint: all NR01 production and test-caller compilation errors are
resolved. Semantic queries select their syntactic category; lexical Value names
no longer block Type path roots. Cache comparisons cover both category slots and
native alias categories. Navigation records the selected category and exposes
`source_targets_at` for dual imports. Removed HIR `Module.exports`/`ExportItem`;
public names come from resolved bindings and registered variant use leaves.
Native authored aliases retain their validated category in generated views (see
the plan refinement), without changing ordinary source use syntax or native ABI
symbol checks.

Focused validation passed: HIR `wildcard_imports_detect_conflicts_and_reject_nonmodule_targets`,
`resolves_native_constructor_imports_facade_exports_and_function_calls`,
`type_alias_swap_invalidates_signature_reuse`; runtime
`installed_reexports_require_declared_and_installed_canonical_targets` (including
dual-category portable aliases and rejection of mismatched metadata). The initial
glob diagnostic mismatch was fixed and its check passed. Runtime test compilation
also built the affected compiler/MIR dependency path. Structure check: 926 files,
zero findings; formatting/diff checks passed. NR03 will adapt the existing broad
single-space collision/shadowing matrices to the new contract and exercise dual
source/native aliases, navigation and incremental behavior. No full suite or CI
run has been performed; CI remains pending.

NR03 local SA8 gate: reused the existing namespace, collision, import/native,
cache and source-module contract owners; no new test function was added. The
collision matrix now checks same-category conflicts (20 meaningful pairs instead
of 36 including obsolete cross-category failures). Dual source/native aliases,
Type-prefix lookup under lexical Value shadowing, prelude types, independent
strong/glob tiers, partial access failure and category-specific navigation are
covered. Transitive facade edits retarget/remove the Value category while retaining
the Type identity; incremental diagnostics/locations match fresh analysis and
old snapshots remain queryable. Grouped import prefixes coalesce equal canonical
targets while preserving their distinct origins.

Local commands/results (all Cargo commands use
`DEVELOPER_DIR=/Library/Developer/CommandLineTools`):

- `cargo test -p kagari-hir analysis::namespace_tests --lib`: 5 passed.
- `cargo test -p kagari-hir analysis::type_name_tests --lib`: 2 passed.
- Exact HIR filters: `module_facade_bindings_share_source_type_call_and_navigation_targets`,
  `resolves_language_builtin_type_annotations`,
  `wildcard_imports_detect_conflicts_and_reject_nonmodule_targets`,
  `reexports_cannot_widen_private_items_or_modules`,
  `source_host_and_module_item_ambiguities_are_rejected`,
  `host_catalog_rejects_ambiguous_or_unspellable_paths`,
  `equal_glob_targets_keep_origins_and_absent_categories_release_fallback`,
  `grouped_directives_keep_leaf_root_ranges_and_explicit_aliases`,
  `transitive_alias_rename_invalidates_unchanged_caller`, and
  `alias_target_swap_invalidates_body_reuse_after_trivia_edit`: each passed.
- `cargo test -p kagari-embed --test source_modules
  public_source_glob_reexports_members_through_artifacts -- --nocapture`: passed;
  direct and encoded source-free products return 42. The selected Cranelift
  preparation reported `InterpreterFallback`, and execution returned 42.
- `cargo clippy -p kagari-hir --lib -- -D warnings`, structure (926 files, zero
  findings), formatting and diff checks passed.

The first runs exposed obsolete single-space assertions, a test import moved to
an incorrect scope during import grouping, and duplicate grouped-prefix query
results. All were repaired and the affected checks passed. Clippy's argument-count
findings were resolved by an import context object, without lint allowances.
No local failure is carried. Generated artifacts were rebuilt/round-tripped by the
focused SDK fixture; no tracked artifact layout or version refresh was required.
NR04 still owns whole-graph rounds, pending reservations and exhaustion handling.
Full workspace/backend CI acceptance remains pending; no remote push was made.

NR04 baseline (NR03 algorithm plus work counters): Apple M1 Max, 32 GiB,
macOS 26.6.2 arm64, rustc 1.98.1 / Cargo 1.98.1; workspace test/dev profile
(opt-level 1), default features/parallelism and `target/`. Run
`cargo test -p kagari-hir solver_workload_measurements --lib -- --ignored --nocapture`.
Each graph has a 48-module active shape; unrelated adds 96 stable modules.
Candidate work counts produced binding contributions per module evaluation.
The bounded measurement fixture lives in `imports/solver_tests.rs` (ignored by
normal test runs). Source preparation and solving exclude Rust compilation.

| Shape | Prepare µs | Solve µs | Visits | Changed scopes | Candidate work |
| --- | ---: | ---: | ---: | ---: | ---: |
| Named chain | 1603 | 43792 | 2304 | 47 | 1223 |
| Glob chain | 303 | 18565 | 2304 | 47 | 1223 |
| Diamond | 396 | 2110 | 144 | 92 | 240 |
| Seeded cycle | 237 | 16891 | 2352 | 48 | 1273 |
| Unrelated + chain | 633 | 47845 | 6912 | 47 | 5831 |
| Removed seed, new revision | 245 | 526 | 48 | 0 | 0 |

The first measurement link failed with missing Rust object symbols; package-only
`cargo clean -p kagari-hir` followed by the same bounded build/run succeeded.
Rebuild time was 29.03 s, recorded separately. Dependencies remained cached;
source/graph inputs are recreated per shape. The editor's rust-analyzer background
checks caused build-lock contention, so these single-run timings are descriptive,
not a controlled speed claim. Durable work counts are the primary comparison.

NR04 checkpoint: whole-graph rounds are replaced by stable per-module work and
reverse namespace watchers, including unsuccessful lookups and glob membership.
Pending categories/globs reserve draft slots. Closed pending groups and finite
acyclic derivations prevent temporary weak fallbacks from creating self-supporting
aliases. Candidate failures preserve ambiguity/access outcomes. An exact bounded
history and explicit work limits distinguish non-convergence/exhaustion from
cancellation and ordinary unresolved imports. HIR preparation, standalone name
resolution and SDK callers propagate failure without publishing/cache-installing
drafts; errors include bounded physical import sites. The plan documents why
closing the entire quiescent pending group satisfies the closure contract.

Focused validation (same Cargo environment as NR03):

- HIR `cycles` filter: 2 passed, including seeded glob circulation, reversed source
  input order, cross-category alias dependencies, unresolved alias cycles and
  retained stale-target rejection.
- Exact HIR filters `failed_solves_preserve_published_snapshots_and_allow_recovery`,
  `diamond_has_deterministic_reachable_order`,
  `equal_glob_targets_keep_origins_and_absent_categories_release_fallback`,
  `duplicate_logical_modules_keep_source_units_and_reject_absolute_lookup`,
  `duplicate_type_imports_invalidate_all_alias_targets`, and
  `transitive_alias_rename_invalidates_unchanged_caller`: all passed. The single
  new normal test owns the distinct failed-solve/publication/recovery boundary;
  existing language fixtures own the other cases.
- SDK `public_source_glob_reexports_members_through_artifacts`: passed after error
  boundary migration, compiling HIR/compiler/SDK and executing direct/encoded
  products. Selected Cranelift execution reported `InterpreterFallback`.
- `cargo clippy -p kagari-hir --lib --tests -- -D warnings`, structure (928 files,
  zero findings), formatting and diff checks passed. Manual review found no new
  re-export/visibility facade or structural exception.

During focused test preparation an iterator function needed an explicit `Arc`
coercion closure, and import grouping briefly omitted `ImportSolveError`; both
compile errors were fixed before the successful cycle run. Final review added
category-qualified derivation keys and delayed conflicts involving pending slots:
otherwise a valid Value import through its same-leaf Type alias could be rejected.
The existing cycle fixture now covers that boundary and passes. No local failure
is carried. NR05 owns the final same-workload comparison and CI-status review;
full CI remains pending and no push was made.

NR05 local acceptance: no carried build/test failures. The final audit confirms
removal of single-space lookup/export storage and whole-graph round publication;
all callers propagate solver failure. Diagnostic site samples now come from the
active/repeating or unsettled modules rather than unrelated inputs. No added
compatibility surface, version bump, structural exception or scope expansion.
SA1 and SA9 remain unactivated. Checkpoints: NR01 `67e00240`, NR02 `be76015f`,
NR03 `ca672860`, NR04 `9739a7d7`; the final commit carries `Resolution-Phase: NR05`.

Final measurements use the same six inputs, machine/toolchain/profile/features,
default parallelism and `target/` as the baseline above. Command:
`cargo test -p kagari-hir solver_workload_measurements --lib -- --ignored --nocapture`.
Rust test compilation took 11.36 s, separately from graph work; dependencies were
cached. Logs: `target/nr05-worklist.log`. The fixture also checks successful input
shapes remain diagnostic-free and seed removal remains diagnosed.

| Shape | Prepare µs | Solve µs | Visits before → after | Changed scopes | Candidate work before → after |
| --- | ---: | ---: | ---: | ---: | ---: |
| Named chain | 1708 | 5455 | 2304 → 86 | 47 | 1223 → 124 |
| Glob chain | 486 | 2731 | 2304 → 86 | 47 | 1223 → 48 |
| Diamond | 765 | 3114 | 144 → 82 | 81 | 240 → 129 |
| Seeded cycle | 416 | 3749 | 2352 → 188 | 95 | 1273 → 54 |
| Unrelated + chain | 1114 | 4998 | 6912 → 182 | 47 | 5831 → 220 |
| Removed seed, new revision | 397 | 1822 | 48 → 86 | 47 | 0 → 76 |

The required work reduction is established: 96 added unrelated modules add exactly
96 visits, rather than participating in every propagation round. Timings are single
samples with possible editor activity, not controlled universal speed claims.
Diamond solving increased from 2,110 to 3,114 µs despite fewer evaluations; each
new visit additionally maintains observations/acyclic proof sets and completion
state. Seed removal increased from 526 to 1,822 µs: explicit initial reservations
must drain through the chain, producing 38 extra visits before absence is proved.
These bounded overheads preserve the required pending/closure contract. Further
identity/collection optimization is not activated; future performance work should
measure representative project inputs before changing those boundaries.

Final focused checks passed (Cargo environment as above): the measurement helper,
`failed_solves_preserve_published_snapshots_and_allow_recovery`, and SDK
`public_source_glob_reexports_members_through_artifacts`. The SDK build took 21.96 s
and the test 3.09 s; direct/encoded execution returns 42, and selected Cranelift
preparation again reported `InterpreterFallback`. Strict HIR library/test Clippy,
structure (928 Rust files, zero findings), formatting, local documentation links
and `git diff --check` passed. Earlier successful NR03/NR04 contract evidence is
retained above, not replaced by the measurement fixture.

CI status: **pending rerun after the user-reported example compilation failure
described below**. The original local checkpoints were not pushed by the agent. Reviewed `.github/workflows/ci.yml`: structure/self-test, formatting,
strict workspace/all-target Clippy, workspace tests, SDK feature consumers and CLI
JIT are configured there. Its workspace tests own the complete backend/artifact
matrices; none was run locally or simulated with split package commands. Local
NR05 is complete; full architecture acceptance requires those CI jobs on the
final implementation. This remaining acceptance item does not activate SA1/SA9
or authorize a remote push.

NR05 CI follow-up: the user supplied E0061 from
`crates/kagari-embed/examples/source_queries.rs`: its `NameTable::lookup("Point")`
call was missed in the category migration. The earlier focused HIR and SDK test
targets did not compile this example. The example now explicitly selects
`NameNamespace::Type`; no compatibility overload or new regression test was added.
A source/documentation scan found no remaining one-argument string-literal lookup
calls. Focused verification passed:
`DEVELOPER_DIR=/Library/Developer/CommandLineTools cargo clippy -p kagari-embed
--example source_queries -- -D warnings`, structure (928 files, zero findings),
formatting and `git diff --check`. This resolves the reported compile error locally;
full CI still requires a rerun. No workflow or remote branch was changed.

### Explicit native default bodies

The [execution plan](native-default-bodies-plan.md) makes registered trait defaults
visible as checked forwarding bodies in generated declarations. Private native
helpers remain typed call targets; source compilation consumes checked calls and
source-free linking retains validated symbolic registration recipes.
Status: ND01-ND04 complete locally. Focused source/default/cache tests and
structure checks pass; GitHub CI owns pending full-suite and source-free feature
acceptance. The plan records the checkpoint and validation evidence.

### HIR documentation completion

The [execution plan](hir-documentation-plan.md) covers `kagari-hir` storage and ID
relationships, lowering, imports/resolution, semantic facts, registered inputs,
query caches and the checked compiler boundary. Documentation follows two concrete
source examples with annotated storage/lookup diagrams and Rustdoc contracts.
HD01-HD07 are complete. The [reading guide](architecture/hir.md) connects both
examples to the owning code. All 125 production modules have orientations; strict
Rustdoc, three runnable examples, rendered-page review, changed-file formatting,
structure (921 files, zero findings), local links and diff checks pass. HIR behavior
is unchanged; the sole code change forwards ID macro documentation attributes.
Full suites remain with CI; no CI run was observed for this local checkpoint.

- [x] HD01: Document orientation, storage, IDs and source-map ownership.
- [x] HD02: Document HIR node families and AST-to-HIR lowering.
- [x] HD03: Document imports, namespaces and lexical resolution.
- [x] HD04: Document declarations, semantic types and type-checking facts.
- [x] HD05: Document host/native inputs, language roles and builtin bridges.
- [x] HD06: Document queries, caches, snapshots and checked-program handoff.
- [x] HD07: Audit coverage and perform focused documentation validation.

### Syntax documentation completion

The [execution plan](syntax-documentation-plan.md) covers the entire `kagari-syntax`
crate using Rustdoc/standard-library conventions: visible token spellings, concrete
AST shapes, accessor contracts, parser state and examples. SD01-SD05 are complete:
80 AST wrappers have annotated trees; strict Rustdoc, three documentation examples,
structure/formatting and link/diff checks pass. Runtime/parser behavior is unchanged;
the sole code change forwards AST macro documentation attributes.

- [x] SD01: Document orientation/storage and forward AST macro documentation.
- [x] SD02: Document kinds, tokens and Rowan vocabulary.
- [x] SD03: Document all AST categories with annotated trees and accessor mappings.
- [x] SD04: Document lexer/parser APIs, internal state and grammar flow.
- [x] SD05: Audit coverage and run focused documentation validation.

### Definition spacing enforcement

- [x] Require a blank line between adjacent Rust functions/methods, before the
  following function's comments/attributes. Cover traits, local functions and tests;
  retain the checker's macro/literal boundary.
- [x] Extend separation to structs, enums, unions, traits, impl blocks, inline
  modules and extern blocks, including mixed pairs with functions. Preserve compact
  import, out-of-line module, type alias and constant groups.
- [x] Insert missing separators without changing behavior; validate the checker
  regression suite, full structure audit, formatting and diffs. No Cargo test run.

Ledger: `function-spacing` and `item-spacing` reject missing separators in parsed
Rust item scopes, including tests. The initial pass inserted 81 blank lines in 27
Rust files; the type/implementation extension adds 53 in 13 files, with no code changes.
Checker self-tests pass (42); the audit passes (921 Rust files, zero violations or
exceptions), as do formatting and diff checks. Existing user edits are preserved.

### Test consolidation (contract ownership, complete)

- [x] Inventory Rust test entries and inspect grammar, HIR, VM and SDK contract owners.
- [x] Replace 23 per-position early-return tests in SDK instantiation with four
  evaluation-contract matrices in `tests/never.rs`; prune equivalent variants
  rather than transferring every historical reproduction. Preserve observable
  order, short-circuiting, skipped specialization and uncommitted writes.
- [x] Remove duplicate example wrappers and the identical enum equality fixture.
  Remove four standalone tests of the obsolete permission model; retain ordinary
  function resolution in the resolver suite and merge generic-name shadowing into
  the existing trait-reference contract.
- [x] Make contract coverage, rather than feature/bug history, govern test growth.
- [x] Run focused checks and review the final diff.

Audit:

| Crate | Rust test attributes before | After |
| --- | ---: | ---: |
| embed | 443 | 421 |
| HIR | 438 | 434 |
| VM | 302 | 302 |
| runtime | 301 | 301 |
| compiler | 187 | 187 |
| syntax | 80 | 80 |
| contract | 49 | 49 |
| types | 42 | 42 |
| bytecode | 29 | 29 |
| common | 14 | 14 |
| source | 7 | 7 |
| Cranelift | 7 | 7 |
| CLI | 5 | 5 |
| MIR | 1 | 1 |
| stdlib | 1 | 1 |
| **Total** | **1906** | **1880** |

Counts are a static inventory of Rust test attributes in 307 files, not Cargo's
feature-dependent runtime count or the number of fixture rows; after this pass
306 files contain these attributes. The major suites own core language and runtime
contracts; individual redundant variants still require review. No blanket deletion
by crate or age is appropriate. Grammar inventories and numeric-width matrices remain canonical.
HIR tooling and snapshot queries, bounded executable rejection, GC/host safety,
cancellation, source-free consumers and pinned reload are also core contracts.

The bounded cleanup removes two standalone wrappers for examples already owned by
`syntax_examples` (`bitwise` and `generic-trait-methods`); focused numeric/static
dispatch and generic argument coverage remains. The four return matrices own
control flow, calls, aggregate construction and accesses/writes: 43 named entries
in four compilations instead of 62 independent compilations, with direct, decoded
and JIT/fallback routes. Equivalent repetitions (such as typed/untyped constructors,
repeated member chains and overlapping native-call positions) are removed. Each entry
observes its own counter and array state; route-local runtimes retain root/depth
cleanup checks. The removed enum fixture is byte-for-byte identical to the retained
`enum_value` fixture apart from its label. No production behavior or compatibility
contract changes.

Remaining suite ownership is recorded here rather than opening a parallel cleanup
queue. This pass does not claim that every other fixture is minimal. Further
iteration should prune redundant variants within its existing contract owner;
it must not introduce new permanent cases solely to record a fix.

Validation: the four `returning_expressions_` matrices pass (43 entries, 129
route executions). Five selected surviving tests also pass: never closure/loop
joins, contextual generic calls, fixed-width bit operations, ordinary `type_of`
function resolution and generic/trait name shadowing. The shared observable suite
compiles with `cargo test -p kagari-embed --lib runtime::language_contract:: --no-run`;
its full matrix was not executed locally. Strict Clippy passes for the three
affected SDK test targets. Structure checks pass for 921 Rust files with no
violations/exceptions; formatting, added local links and `git diff --check` pass.
No local failures are carried. Full workspace and feature/backend acceptance
remain owned by GitHub CI and were not run or claimed passing in this cleanup.

### Focused test-harness optimization (SA7, numeric fixtures complete)

- [x] Batch VM numeric fixtures into one compilation per test, preserving every
  integer width, operation and conversion. Direct and serialized routes keep
  separate fresh runtimes; a successful call after every scalar entry verifies
  trap cleanup. Source-free validation remains active.
- Scope: numeric fixtures only. SDK engine preparation, other aggregate runners,
  production caches and broad acceptance matrices remain separate work.
- Validation: compare the exact float test before/after, run the four numeric tests,
  and check structure, formatting and diffs. Record compilation separately from
  test execution; do not rerun the workspace or claim a whole-suite speedup.

Ledger:

- The four tests retain 90 scalar cases and 180 direct/decoded executions.
  Compilations and artifact round-trips decrease from 90 to 4; standard-library
  installation and loading decrease from 180 to 8. Each artifact route keeps a
  separate runtime. A successful probe after every case verifies subsequent calls
  work, including after traps. No source analysis or loader validation is bypassed.
- `cargo test -p kagari-vm --lib tests::numeric::` passes all four tests in 4.51s
  (Cargo preparation 0.13s, no rebuild). The intermediate compilation-only batch
  still used a fresh runtime/load per case and took 82.32s with the same filter.
  These are single-run observations with lightweight checks running concurrently,
  not isolated benchmarks or a whole-workspace comparison.
- The exact `tests::numeric::floats_use_their_source_precision_and_ieee_comparisons`
  test (`cargo test -p kagari-vm --lib <name> -- --exact`) took 4.97s before,
  3.87s with compilation-only batching, and 6.53s in the final version. No stable
  standalone float speedup is established. Cargo build/preparation times were
  65s, 4.61s and 8.25s respectively; the first package feature graph required
  dependency rebuilds, while later builds reused them and rebuilt the VM test.
- Environment: Windows x86_64 MSVC, Rust 1.99.0 (`b940084d7`), i9-12900K
  (16 cores/24 logical processors), default optimized/debug test profile,
  default target directory and Cargo/test parallelism, no extra VM features
  (the compiler source dev-dependency remains enabled). Cached build products
  were retained; process-local standard registration initializes per invocation.
- Focused VM Clippy, structure (922 files, zero violations/exceptions), formatting
  and diff checks pass. No workspace tests or backend matrix were rerun.

### Import and namespace resolution (IR01-IR03, complete)

[SA4](review.md#sa4-import-records-also-represent-namespace-lookup-state) separates
import directives, local bindings and namespace lookup state.
[The import-resolution plan](import-resolution-plan.md) owns its detailed contract
and acceptance matrix. This roadmap owns phase order and the progress ledger.
IR01-IR03 are complete. SA1, SA2 and SA3
remain independent findings, including the current iteration-limit concern.

- [x] **IR01:** Publish shared namespace lookup and qualified declaration targets;
  migrate name/signature/compiler consumers and remove internal namespace imports.
- [x] **IR02:** Separate import directives, named scope bindings and provenance;
  preserve diagnostics, re-exports, navigation and direct dependency edges.
- [x] **IR03:** Verify snapshot invalidation, tooling and source/artifact/native
  consumers; complete final checks and update implemented architecture.
- [x] **SA5 follow-up:** Invalidate signature, file and function-body reuse on
  transitive namespace binding changes; retain local arena remapping and old snapshots.

Ledger:

- IR01 implementation replaces import-index identities with qualified source units,
  canonical targets and a snapshot-owned catalog. Candidate tiers preserve blocked
  strong names, glob equality/ambiguity and implicit precedence. Directive model
  types are introduced with the identity cutover so consumers can compile together;
  IR02 owns syntax ranges, explicit aliases and complete use-site provenance.
- Native declaration preparation previously fabricated qualified-name and dependency
  imports. Qualified names now enter the same catalog directly, while registered
  dependency metadata contributes graph edges without fabricated scope bindings.
- Signature projections use canonical declarations reachable from the local scope,
  rather than all snapshot declarations. Existing arena remappers remain the only
  reuse path across local lowerings; body and signature cache comparisons have
  distinct input contracts, and retained catalogs require matching reachable tables.
- IR01 validation passed: `cargo test -p kagari-hir imports::` (34 tests),
  `cargo test -p kagari-hir analysis::` (238 tests),
  `cargo test -p kagari-compiler --test source_programs` (10 tests),
  `uv run --locked scripts/check_structure.py` (no violations), and
  `git diff --check`. Deep aliases/direct leaves share canonical declarations;
  lexical blocking, strong collisions, host/source ambiguity, scoped access,
  transitive invalidation and retained snapshots pass. All intermediate failures
  listed above are resolved; no carried build/test errors remain.

- IR02 retains one directive per real import leaf, explicit alias presence, grouped
  leaf/root ranges and selected provenance for import, expression and type path
  prefixes. Equal glob targets retain both origins. Dependency collection records
  syntactic module paths before terminal lookup, preserving unused imports and
  unresolved facade edges. Foreign directive origins are qualified by their source
  unit before local directive indexing.
- IR02 validation passed: import tests (37), analysis tests (238), compiler
  `source_programs` (10), the structure checker (no violations), and diff checks.
  Initial focused failures exposed whitespace in syntax ranges and foreign origin
  indexing; both were corrected and the affected suites passed. No carried errors.

- IR03 identity review preserves per-source-unit import facts when logical module
  identities collide, validates complete source units before local call lowering,
  and keeps strong collisions ambiguous before visibility filtering. Same-revision
  re-lowering tests reject stale arenas; duplicate-module tests retain independent
  scopes and reject executable reachability. These refine implementation of the
  existing SA4 identity contract without changing SA1-SA3 scope or solver policy.

- IR03 first workspace Clippy attempt found five migration lint failures in
  HIR analysis, declaration mapping and a stale-target test (discarded enumerate,
  unit-variant pattern, two Copy clones and a redundant borrow). IR03 owns their
  correction; all five are resolved and strict workspace Clippy passes. Cleanup
  attempts briefly failed `cargo test -p kagari-embed --test syntax_examples
  nested_inline_modules_resolve_qualified_members` with E0507 (a non-Copy
  declaration key) and E0425 (a removed enumerate index). Explicit per-owner
  mapping and direct directive access fixed both; the focused test now passes.

- IR03 first `cargo test --workspace` failed the embed language-contract route:
  `ambiguous-module-name` expected `KG_RESOLVE_DUPLICATE_DECLARATION` but received
  `Graph(InvalidImports)`. Pure declaration conflicts had been incorrectly placed
  in import facts. IR03 restores declaration-stage diagnostic ownership while
  retaining graph rejection for import conflicts and duplicate logical modules;
  the original conformance assertion remains unchanged. The focused
  `cargo test -p kagari-embed --lib runtime::language_contract::language_contract_routes_preserve_values_diagnostics_and_effects`
  now passes, including in the final workspace run. Strict Clippy, format,
  structure and all artifact feature checks also pass.

- IR03 second workspace attempt passed language-contract routes but failed
  `source_modules::imports::duplicate_inline_and_external_module_identity_is_rejected`:
  a colliding child identity became a missing-module candidate, hiding the
  duplicate-declaration diagnostic. Module headers now retain every colliding
  source-unit candidate and its dependency edge, so lookup remains ambiguous and
  graph rejection reports the real identity collision. The focused test and all
  30 `source_modules` tests pass in the final workspace run; the original assertion
  is preserved.

- IR03 full `cargo test -p kagari-hir` exposed two remaining old-model test
  expectations: a missing module header was expected to resolve to a local
  `ModuleId`, and a module alias was expected to be a source declaration item.
  Tests now assert a retained unresolved blocking name and a valid source module
  namespace respectively, preserving missing-name diagnostics, owner identity,
  constructor calls and checked type arguments. Both focused tests pass, as do all
  425 HIR unit tests and its language-contract integration tests in the final
  workspace run.

- IR03 final acceptance passed on 2026-10-07. Focused import tests (39), analysis
  tests (238), compiler `source_programs` (10), embed `source_snapshots` (13),
  `syntax_examples` (9, including 45 standalone source/artifact examples),
  `cranelift_preparation` (3 real native/fallback/reload tests) and CLI JIT (5) pass.
  The nested inline-module artifact test now covers grouped facade aliases, direct
  leaf calls, encoding/decoding and execution after Engine drop.
- Final integration passed: `cargo test --workspace` (including doc tests),
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, `uv run --locked scripts/check_structure.py`
  (920 Rust files, zero violations), and `git diff --check`.
- All `artifact_features` configurations pass with `--no-default-features`:
  no extra features (9 tests), `--features source` (10), and `--features native`
  (11). The native-only run executes
  `real_cranelift_compiles_portable_artifact_without_source`; source-enabled
  fixture emission still matches the checked-in artifact. No artifact regeneration
  or format/ABI bump was needed.
- Architecture and review now document the implemented boundary; local links in
  all four changed documents pass. All intermediate failures above are resolved;
  no SA4 build/test failures or structural exceptions remain. SA1-SA3 stay
  unactivated, with fixed-point scheduling and round bounds unchanged. Import
  resolution performance remains unmeasured.

Phase commits use `Import-Phase: IR01` through `Import-Phase: IR03`.

SA5 follow-up ledger:

- The user activated the [SA5 finding](review.md#sa5-incremental-reuse-misses-transitive-namespace-changes-p1).
  Three incremental-versus-fresh reproductions failed before the fix: alias rename,
  function alias retargeting after a trivia edit, and nominal alias retargeting in
  signature queries. Reachable namespace comparison now gates all affected reuse
  paths, rebasing only the current module's source unit for existing arena remappers.
  Regression coverage also exercises single-function reuse and local nominal
  namespaces. All five regression tests pass; SA1-SA3 remain outside this fix.
- The first structure check found 1207 effective LOC in `analysis/mod.rs`.
  Complete-file cache predicates now belong to `analysis/cache.rs`; the structure
  check passes without exceptions. The first HIR run exposed local enum member
  IDs retaining their arenas in namespace comparisons; local variant comparison
  now validates each arena and compares owner/slot before the existing remappers.
- Strict workspace Clippy on Rust 1.99.0 rejected two existing runtime
  `AtomicU64::fetch_update` calls as deprecated. Both use the renamed `try_update`
  API with unchanged ordering and checked increment behavior. No warnings were
  suppressed. Strict workspace Clippy and runtime tests now pass.
- The workspace run reached `registration_sources` and failed four existing
  Windows path assertions: canonical cache names retained `\\?\` and did not
  match the normalized analyzed names. The SDK publication boundary now normalizes
  canonical drive paths using the source database's naming rules. The unchanged
  six-test registration suite now passes without weakening assertions.
- Final checks pass: HIR (430 unit + 8 integration tests), structure (922 Rust
  files, zero violations/exceptions), formatting, strict workspace Clippy and
  `git diff --check`. All carried HIR, Clippy and Windows failures are resolved.
- Workspace tests completed in segments after the initial Windows failure:
  retain the successful unchanged prefix, rerun `registration_sources`, run the
  remaining 13 SDK integration targets (`result_option` through `typed_calls`),
  run `cargo test --workspace` excluding the nine completed packages (`kagari-abi`,
  `kagari-bytecode`, `kagari-cli`, `kagari-codegen`, `kagari-codegen-cranelift`,
  `kagari-common`, `kagari-compiler`, `kagari-contract`, `kagari-embed`), and run
  `cargo test --doc` for the eight excluded libraries. No failing suite is carried.
- The plan's `artifact_features` matrix passes with `--no-default-features`
  (9 tests), `--features source` (10) and `--features native` (11).
  `cargo test -p kagari-cli --features jit` passes (5); source snapshots, syntax
  examples, native preparation and source-free consumers also pass.

### Interpreter performance follow-up

IP00-IP04 and NE01-NE05 implementation/correctness integration are complete;
**Lua parity acceptance remains open**. The latest NE05 paired report records
nontrivial interpreter/Lua ratios of 4.85-212.01. Detailed environment, workloads,
commands, preparation scopes and results live in
[measurements](performance-baseline.md#typed-numeric-execution-result-ne05-2026-10-06);
the earlier [IP04 report](performance-baseline.md#prepared-native-facts-and-integration-ip04-2026-10-06)
retains its distinct checkpoint evidence.

Acceptance remains interpreter/Lua median <= 1.0 for every genuinely matched
nontrivial workload in repeated same-machine release runs, with uncertainty
analysis near parity. Report entry/setup separately and add representative strings,
objects, closures, traits and host callbacks before a general language parity claim.
Lua-incompatible numeric domains use checked reference results without invented
parity ratios. The byte-oriented fixture does not establish NES emulation acceptance.

Remaining costs include script/generic/interface boundaries, collection retention,
module/type lookup and allocation. A shared generic Add default-method fixture
still fails source analysis with `MissingBinding("checked callable requirement")`;
the supported shared-identity fixture does not close that capability gap.
Further optimization/frontend work requires a separately bounded activation.
Collector replacement, enum unboxing, general collection lease redesign, JIT
expansion and new numeric APIs remain outside these completed tracks.

### Async execution (AX00-AX06, active)

The [execution plan](async-execution-plan.md) owns phase scope, dependencies,
checkpoint policy and focused acceptance. The [async design](async-execution-design.md)
and [host task scopes](host-task-scope-design.md) own language/lifetime behavior.
Status: implementation and focused local acceptance complete; full GitHub CI is
pending. On 2026-10-08 the user authorized goal execution of AX00-AX06,
including ordinary `for` traversal with `.await` in the body. Current specifications
cover owned native/script waits and scope-owned Tasks; implementation and local/CI
acceptance are recorded separately below.

- [x] AX00: Finalize concrete semantic/executable, state, host and verification contracts (local documentation gate).
- [x] AX01: Introduce owned execution lifetime and bounded driving, preserving synchronous reentry (local gate).
- [x] AX02: Prove typed native Future completion and checked source-free wait/resume (local gate).
- [x] AX03: Implement async functions, explicit async closures and await through source and artifacts (local gate).
- [x] AX04: Add scope spawn, shared Task results, waiters and directional cancellation (focused local acceptance).
- [x] AX05: Complete lifecycle, reload, cleanup and diagnostic integration (focused local acceptance).
- [x] AX06: Deliver embedding/example products and local integration evidence (focused local acceptance; full CI unrun).

Dependency order is AX00 through AX06. Each implementation checkpoint must pass
its focused checks without carrying known compilation/test failures; unsupported
intermediate async operations reject before execution. Full suites and complete
feature/backend matrices remain GitHub CI work. Commits carry `Async-Phase: AX00`
through `Async-Phase: AX06`; the planning commit has no implementation trailer.

Async design checkpoint (2026-10-08): selected explicit `.await` over implicit
waiting. Calling an async function creates a cold Future, not a scheduled Task;
unstarted Futures independently retain captures beyond their creating handler.
First driving binds a Future to an execution; shared task results instead use
scope-owned Tasks created by `spawn`. This replaces creator-root-bound cold Tasks
and separate launch/async APIs. Async closures use explicit `async |args| body`,
extending the existing closure form rather than inferring suspension from a body
or receiving API. Invocation returns a fresh Future without executing the body;
captures retain ordinary value/shared-slot semantics across repeated calls. Callable
types and generic bounds reuse `fn(A) -> Future<T>` and `Fn(A) -> Future<T>`;
no separate AsyncFn protocol or async function-type syntax is introduced. Spawn
accepts `Fn() -> Future<T>`, invokes it once on first drive and drives one Future
layer, without implicit wrapping/flattening. Futures are driven once; repeated
await through any alias traps. Tasks retain completed results/terminal metadata
for reachable handles without retaining execution frames. Spawn reports admission
through `Result<Task<T>, SpawnError>`. Both awaits return T, with business Result
unchanged: cancellation/traps terminate execution, not an outer Result wrapper.
Cancelling waiter A leaves target B running; cancelling B terminates dependent A
with source-task provenance. Nested spawns belong to the selected scope; scope
close cancels unfinished work. Host reports identify task/scope/cause and distinguish
cancel request acceptance from completed cleanup. Wait cycles are rejected.
Local structured scopes, parent-child task trees, supervisor modes and script-level
async cleanup remain deferred. AX00 now owns concrete error/report and state
contracts; AX01-AX06 own their implementation and acceptance without reopening
these language choices.
Updated both proposals without activating implementation or changing current specs.
Documentation validation passed: 58 local links/anchors, content review and
`git diff --check`; CRLF line endings are preserved. No Rust
build/test or CI acceptance is claimed by this design checkpoint.

Ordinary callback reentry remains synchronous. Async designs must later share
the same quiescence, cancellation, late-completion and pinned-generation rules.
No new restoration checklist for retired standard-library APIs is active.

Planning ledger (2026-10-08): added AX00-AX06 with current code/test owners, explicit
producer/consumer exits, a no-carried-failure checkpoint policy and bounded local
checks. Runtime sessions/frames/native registration, VM executor, HIR closures and
SDK entry/features were inspected; no implementation, build or test was run.
No Rust validation was attempted and no newly observed build/test failure is
carried. Runtime correctness and full async CI acceptance are unrun. Planning
validation passed: 116 local links/anchors, code-fence/content consistency, CRLF
preservation and `git diff --check`.

AX00 execution ledger (2026-10-08): activated by the user's goal request.
Reviewed owned iteration/lease records, frame cleanup, session storage and native
callbacks. Required ordinary for-body await without snapshot conversion; preserve
structural-write exclusion through waits and nonstructural replacement visibility.
Documented semantic roles, factory/resume validation, owned drive state transitions,
bounded durable readiness/admission, endpoint races, reporting and candidate graphs.
Added explicitly scheduled specification handoffs. AX01 next owns execution
lifetime and safe slicing; AX03 must prove real deferred for-body await under
interleaving/GC and cleanup, including encoded artifacts. Validation passed for
15 changed Markdown files: 172 local links/anchors, fences, CRLF and
`git diff --check`. No Rust build/test or CI was run; no carried build failures.

AX01 execution ledger (2026-10-08): implemented owned runtime session tokens,
short frame activation, VM start/drive and safe instruction slices on the existing
interpreter/store. Queued start executes no script. Owned drop/cancel signals a
host waker; explicit drain or owned start/drive completes retirement. Parked roots
retain GC windows, pins and iteration leases, with independent restored call depth.
Synchronous callbacks/reentry finish before a slice exit; frame mutation guards,
scoped host values and borrows defer slicing. Independent/recursive driver entry
rejects, while checked synchronous reentry shares its root.

Independent roots exposed a concrete storage gap: tail-only operand-window release
could retain historical holes during alternating completions. Reuse retired window
slots and compact backing banks while retaining checked live identities; expanded
the existing out-of-order window contract rather than adding a duplicate test.
No unrelated collector/lease redesign or performance claim was made. Manual review
found no new forwarding/re-export surface, deep paths or mixed module ownership.

AX01 validation passed:
- `cargo check -p kagari-vm` (initial producer/consumer build).
- `cargo test -p kagari-runtime --lib frame::values::tests`: 5 affected storage
  contracts, including alternating retirement, scalar bits and stale identities.
- `cargo test -p kagari-vm --lib async_owned_drive_contract`: 2 contracts; interleaved
  roots/GC, once-only effects, queued/running cancellation, owner drop, synchronous
  reentry and call-depth preservation; encoded for-loop lease retention, another
  handler during a slice, structural-write rejection/nonstructural visibility and
  normal/cancel/drop cleanup. Repeated only after changing their affected code.
- `cargo test -p kagari-vm --lib host_reentry_cannot_swallow_root_termination_and_releases_borrows`:
  1 existing cancellation/cleanup boundary contract passed.
- `cargo clippy -p kagari-vm --lib -- -D warnings`, structure (932 files, zero
  findings/exceptions), formatting and diff checks passed. CRLF preserved.

An intermediate test compile reported E0599 for the test's `ResourceLimit` spelling;
corrected it to existing `ResourceLimitExceeded` and reran the selected contract.
No build/test failure is carried. Full suites, feature/backend matrices and GitHub
CI were not run. External Waiting/Future completion remains AX02; source async and
actual deferred `for { ... .await }` remain AX03. The successful slice tests do not
claim those later capabilities are implemented. AX02 is the next unfinished phase.

AX02 progress ledger (2026-10-08, completion transport checkpoint): added
`native::completion` bounded typed reservations, durable one-result publication,
registration/publication wake handshake, duplicate/stale status and generational
slot reuse. Consumer poll transfers owned host data; it does not run script or
convert GC values. Cancel/drop discards queued payload and invokes provider cleanup
at most once; cleanup panic faults admission, and exhausted generations retire.
Producer endpoints hold weak state and cannot retain old result payloads/capacity.

Added scoped weak wake subscriptions to the existing CancellationToken and connected
them to owned execution readiness. Cancelling the host-supplied token now wakes the
same control path as owner cancellation; slice exits republish readiness and
terminal/session destruction invalidates it. Host wakers execute outside locks;
notifications only request scheduling. Extended the existing owned-driver contract
for external-token wakeup and runtime-destruction retirement.

AX02 native wait checkpoint (2026-10-08): connected the transport to sealed traced
Future storage, `ModuleBuilder::add_async_function`, per-runtime operation capacity,
session-owned pending consumers and bytecode Await. Cold calls retain script
arguments and exact type provenance without converting/submitting; first await
claims once, roots captures and reserves before submission. Conversion and
continuation publication occur only on the driver. VM drive now returns Waiting;
pending activations poll without resubmission or a busy runnable state. Completion,
cancellation, owner retirement and shutdown release operation state. Cleanup panic
quarantines the runtime. Ordinary entry/native installation reject resume bodies
before their body executes.

Portable validation checks the Future storage role, invariant output/input semantic
slots, physical representations and resume-body capability. Identity mapping,
artifact round trips and register allocation consume the new instruction. Direct
ordinary calls/closures cannot enter resume bodies. Runtime checks the actual
nominal Future/output and active resource chain even for immediate completions;
`may_suspend` is not accepted as a resource-safety proof. The fixture deliberately
supplies a native Future declaration while foundation export is pending.

Focused validation passed:

- `cargo test -p kagari-vm --lib async_native_completion_contract`: 3 contracts.
  Encoded source-free loading; cold discard/captures across GC; deferred cross-thread
  result; immediate and completion-during-start results; single await/alias rejection;
  capacity before submission; submission/provider failure; queued-result cancellation; owner
  drop/runtime destruction; stale endpoints and cleanup quarantine.
- `cargo test -p kagari-bytecode --lib async_artifact_validation_contract`: 1 contract.
  Encoded round trip plus forged storage role, missing resume capability/input type,
  incompatible managed output/input and synchronous resume-body call rejection.
- `cargo test -p kagari-vm --lib async_owned_drive_contract`: 2 contracts, preserving
  interleaved execution, reentry, cancellation and encoded iteration lease behavior.
- `cargo clippy -p kagari-vm --lib -- -D warnings`, structure (940 Rust files,
  zero findings/exceptions), formatting and diff checks passed. Manual review kept
  producer/GC claim/pending-session/VM responsibilities separate and added no
  re-export whitelist or structural exceptions. CRLF is preserved.

Initial test compilation exposed an explicit RuntimeConfig literal missing the
new async limits, and a stale compiled dependency while the wait API was being
updated. The callers and dependency were rebuilt; no compilation/test failure is
carried. No full suite or GitHub CI run. Tracked artifact regeneration remains at
the plan's coherent acceptance checkpoint; no unpublished format number bump.

AX02 remains unchecked. Next finish the portable MIR Await producer/lowering and
independent CFG initialization/resource proofs, publish core::future and its prelude
entry, connect SDK owned driving, and exercise pre-entry backend fallback through
that surface. The completed VM/bytecode focused gates prove the native wait path,
not the entire AX02 checklist. Source async functions/closures and actual deferred
`for { ... .await }` remain AX03; Task scopes remain AX04. No extra collection rule,
implicit async callback or user-defined awaitable was introduced.

AX02 suspension verification checkpoint (2026-10-08): implemented portable MIR
Await, identity/operand/effect mapping and bytecode lowering. MIR verification
checks the native Future role and output contract, definite initialization,
live host capabilities and iteration-stack joins/backedges. Bytecode independently
rebuilds bounded initialization, liveness and iteration-resource facts; serialized
resume flags cannot substitute for those checks. Direct synchronous MIR calls and
closure construction reject resume targets, and Cranelift rejects resume bodies
before compilation/entry.

Sealed bytecode retains computed await liveness through identity normalization and
artifact adoption. Runtime preparation maps those logical facts through physical
slot reuse, preserving every location with a live alias. Dead managed slots are
discarded before suspension checks, avoiding false rejection from stale temporary
host capabilities and unnecessary GC retention. Debug-visible locals remain live;
iteration leases remain independently owned and are not dropped/reacquired at await.
Register operand roles now belong to bytecode and are consumed by both verification
and runtime allocation; the old runtime copy was removed directly.

Focused validation passed:

- `cargo test -p kagari-bytecode -p kagari-compiler --lib async_` initially passed
  the two bytecode contracts and exposed invalid identity/effect metadata in the
  newly constructed MIR fixture. The fixture was corrected; the final
  `cargo test -p kagari-compiler --lib async_mir_suspension_contract` passed MIR
  encode/decode, checked lowering, artifact adoption and four malformed-MIR cases.
- `cargo test -p kagari-bytecode --lib async_flow_validation_contract` passed after
  the final debugger-liveness case. The existing artifact contract plus CFG cases
  cover initialization, premature destination reads, iteration underflow/joins,
  host liveness, dead host slots, debugger visibility and a valid loop backedge.
- `cargo test -p kagari-vm --lib async_native_completion_contract`: three existing
  cold/deferred completion, admission/retirement and cleanup contracts passed.
- `cargo test -p kagari-vm --lib async_wait_live_storage_and_iteration_contract`:
  encoded source-free actual Waiting, dead-slot collection, live physical aliases,
  iteration structural-write exclusion, nonstructural visibility and normal,
  cancellation and owner-drop cleanup passed. This is not the source for-await gate.
- `cargo clippy -p kagari-vm -p kagari-compiler -p kagari-codegen-cranelift --lib -- -D warnings`
  passed after replacing one range-index loop with iteration. Structure review
  passed for 944 Rust files with zero findings/exceptions; manual review found no
  forwarding/re-export growth or unrelated module migration. Formatting, CRLF and
  diff/content checks passed. No full suite or GitHub CI acceptance was run.

The initial bytecode test had a wrong Branch field name; corrected before its
successful focused run. No build/test failures are carried. AX02 remains unchecked:
next publish the canonical `core::future`/prelude declaration, connect SDK owned
driving and verify pre-entry native fallback through that surface. AX03 still owns
source async functions/closures and the required ordinary for-body await matrix;
AX04 owns Task scopes. No format/ABI number bump or historical artifact reader.

AX02 SDK/foundation exit checkpoint (2026-10-08): published the canonical
`core::future::Future<T>` declaration/storage, identity-preserving `std::future`
and prelude exports, and generated documentation. Separately authored native types
can bind storage through `ModuleBuilder::bind_storage`; exact local declaration,
layout, parameter and duplicate checks remain mandatory.

Added SDK `start`, `drive` and explicit retirement drain over the existing runtime
owner/VM implementation. These APIs work without source or native compilation
features, retain no Runtime borrow across waits, and map terminal errors to the
SDK error taxonomy. Native preparation returns Unsupported for resume bodies before
calling a backend; synchronous entry still rejects them without submitting IO.
Added the async contract target to the existing standalone CI feature consumer;
the complete consumer matrix was not run locally.

Integration review found a concrete cancellation leak: cancelling one owner also
cancelled other executions sharing its host ExecutionContext token. A failing SDK
case reproduced this. Each execution now has its own cancellation token and a weak
subscription forwarding the external host signal inward. Local cancellation never
propagates back to that shared signal; external cancellation still wakes pending
executions. The corrected case covers two simultaneous waits and independent results.

Focused validation passed:

- `cargo test -p kagari-embed --no-default-features --test async_execution sdk_owned_native_wait_contract`:
  encoded source-free input, canonical Future binding, cold creation, delayed and
  immediate completion, completion during start, durable host wakeups, duplicate and
  stale publication, interleaved roots/GC, external and owner-local cancellation,
  owner-drop drain, terminal SDK errors and stale owner rejection.
- `cargo test -p kagari-embed --test async_execution sdk_native_preparation_declines_resume_before_entry`:
  no compiler invocation or IO before the explicit owned interpreter route, followed
  by a real wait/resume. This is a selected native boundary, not a backend matrix.
- `cargo test -p kagari-embed --test library_namespaces explicit_prelude_requires_imports_and_local_names_shadow_defaults`
  and `cargo test -p kagari-stdlib --test documentation standard_documentation_covers_the_registered_api_and_examples_analyze`:
  canonical aliases/prelude and generated standard examples passed.
- `cargo test -p kagari-runtime --test native_builder declaration_storage_binding_preserves_local_contract`:
  rejects missing, mismatched, duplicate and foreign storage bindings.
- `cargo test -p kagari-vm --lib async_owned_drive_contract`: both owned execution
  contracts passed after the cancellation change, including iteration and reentry.
  `cargo test -p kagari-compiler --lib async_mir_suspension_contract` passed with
  the new standard declaration installed.
- `cargo clippy -p kagari-embed --lib --test async_execution -- -D warnings`,
  structure (947 Rust files, zero findings/exceptions), formatting and diff/content
  checks passed. Manual review retained separate registration, runtime lifetime and
  SDK orchestration owners; no re-exports, forwarding modules or exceptions added.

The initial SDK fixture omitted required parameter names and a constant-pool entry;
both were corrected before its passing run. No compilation/test errors are carried.
The local AX02 exit is satisfied together with its preceding transport, VM and MIR
checkpoints. Full CI acceptance remains unrun. Next is AX03: source async functions,
explicit async closures, script Future factories/await and the ordinary for-body
await contract, including real deferred interleaving and cleanup. Task/scope capacity,
sharing and reports remain AX04, lifecycle integration AX05 and final products/CI AX06.

AX03 frontend checkpoint (2026-10-08): reserved `async`/`await`, retained explicit
async markers in callable syntax/HIR and added postfix Await nodes. Parsing reuses
ordinary closure parameters/bodies and postfix precedence; async blocks remain
invalid. Checked signatures expose the installed nominal `Future<T>` independently
of a local type named Future. Async bodies check their completed T, including
`return`, `?`, generic Fn contexts and unflattened nested Future outputs. Ordinary
closures reset the async context, so synchronous retain callbacks reject await.
The lexer/parser, EBNF inventories, syntax specification and syntax witness agree.

Local evidence: `async_callable_syntax_contract` (kagari-syntax) and
`async_callable_typing_contract` (kagari-hir) passed. The latter includes ordinary
for-body await, contextual/direct generic Fn closures, business Result propagation,
shadowed Future spelling, call-signature queries and signature invalidation after
removing async. Grammar inventory checks passed after grouping the optional async
terminal to match the inventory parser. `async_mir_suspension_contract`
(kagari-compiler), structure (949 files, no findings/exceptions), formatting,
diff checks and 74 local documentation links/anchors passed. Focused Clippy for
compiler/HIR/syntax libraries passed; initial unnecessary string conversions were
removed. Manual review retained parser, HIR role/context checking and compiler
lowering boundaries with explicit imports and no new re-exports. No build/test
failures are carried.

AX03 executable checkpoint (2026-10-08): source async functions and explicit async
closures now emit synchronous cold factories and private resume bodies. The new
portable MakeFuture instruction captures checked arguments and existing closure
cells exactly once; nested script awaits share the owned stack. Future payloads
retain private closure metadata edges, preserving the existing GC/type environment
mechanism. Independent MIR/bytecode validation checks factory target, signature,
Future role, output and definite initialization. No format/ABI identifier bump is
needed for this unpublished internal opcode change.

Runtime/VM/SDK start_future explicitly queues one Future layer; ordinary function
entry still returns the cold value. Native Future roots use the same endpoint
path without a synthetic caller. Captures are traced with cycle detection at
construction, first drive and safe parking, under the native tracing/reentry guard.
This checks actual mutable cells, interfaces and native edges as well as static
outer types. Known direct host captures remain rejected by executable validation.

Local evidence: `cargo test -p kagari-embed --test async_execution sdk_` passed
the three SDK contracts. The source contract compiles and round-trips bytecode,
parks a Vec loop on real deferred IO, interleaves element replacement and rejected
structural mutation, forces GC, resumes at the saved cursor and releases the lease
on completion/cancellation/owner drop. It also exercises generic nested awaits,
async closures with shared writable captures, unflattened Future results and
business Err propagation. Its final focused rerun additionally passed no-await
body laziness, cancellation before claim and repeated-Future rejection. Native
root immediate/pending/cancel/drop behavior and pre-entry backend rejection passed.

`async_factory_artifact_contract` (bytecode) passed the encoded valid factory and
malformed target/signature/initialization cases. `async_retention_graph_contract`
(runtime) passed cycle handling, hidden invalid edges after mutation, foreign
values and stale identities. Focused compiler/runtime/VM library and SDK test
Clippy passed after replacing two unnecessary map lookups. The initial SDK test
harness was corrected to use owned start for arguments because synchronous SDK
execute does not implement nonempty argument lists; that unrelated facade limit
was not changed. No compilation or behavioral failures are carried.

Manual review kept factory lowering, executable validation, sealed payload/graph
checks and host orchestration with their existing owners. New Rust modules remain
below the effective-LOC threshold, with explicit imports and no re-exports/globs.
The test-only path attribute groups the source fixture under its integration-test
owner. Structure checked 952 files with no findings/exceptions; formatting and
documentation/diff checks complete the local checkpoint. Full CI remains unrun.

AX03 local acceptance (2026-10-08): `sdk_for_await_iterator_control_contract` adds
custom source/iter/next counts and nested adapter/erased Iterator traversal across
real native waits. Interleaved replacement remains visible; structural writes trap
while parked. Continue, break, return and post-resume division failure preserve
cursor progress and release all loop leases. The trap retains its original source
URI/line after bytecode encoding. The existing source contract now also proves
generic Fn dispatch of async closures, two-RPC success/early business Err, and
left-to-right once-only argument effects before drive.

Known scoped host async parameters now report KG_TYPE_INVALID_ASYNC_CAPTURE at
their declaration, including tuple and annotated/contextual closure parameters.
Generic/mutable graph safety retains independent executable/runtime enforcement.
The expanded `async_callable_typing_contract` and both SDK `source::` contracts
passed. The initial source-line assertion needed usize-to-u32 conversion in the
Rust fixture; it is fixed with no carried failure. HIR library and SDK test Clippy,
structure (952 files, no findings/exceptions), formatting and local documentation
link/diff checks passed. Manual review found no new ownership/import/LOC debt.
Current specifications now describe source async as implemented. AX03 is complete
locally; next is AX04 scope spawn, shared Tasks and directional cancellation.
AX04-AX06 and full GitHub CI acceptance remain outstanding.

AX04 factory-driver checkpoint (2026-10-08): Runtime start_owned_factory validates
a zero-argument synchronous Future-producing closure and queues it without running
its body. First drive invokes it once, then drives exactly one returned Future
layer in the same owned session. Capture roots, closed output types, cancellation
and call-depth accounting share the established path. Private resume bodies cannot
be admitted as ordinary factories. There is no new instruction or ABI change.

The focused `sdk_owned_future_factory_contract` passed ordinary/explicit factories,
slice-by-slice GC after dropping the host factory root, cancellation before start,
factory trap, invalid output shape, repeated use of a captured single-drive Future
and unflattened Future-valued output. A new retain-reentry case first failed:
callback-local stack completion incorrectly consumed the factory continuation.
The return handoff now checks the complete session stack, and the case passes.
Synchronous callbacks still return to their native caller without suspension.

AX04 remains open. Next wire bounded scope admission/dispatch and canonical Task
storage into this factory driver, with cached outputs traced from Task values,
reserved reports, waiter dependencies and directional cancellation. This checkpoint
does not expose script spawn or claim Task/scope support. The existing VM
`host_reentry_cannot_swallow_root_termination_and_releases_borrows` contract also
passed. Focused SDK test Clippy, structure (953 Rust files, no findings/exceptions),
formatting and local documentation/diff checks passed. Manual review kept factory
handoff in the frame owner, with no feature policy added to VM instruction loops,
no new re-exports or structural debt, and no carried errors. Full CI remains pending.

AX04 host-scope checkpoint (2026-10-08): canonical core::task Task<T>/TaskScope
storage roles and prelude names now support host-created scopes. Bounded generational
registries reserve each task's terminal report at admission; unclaimed reports retain
capacity. Runtime spawn_task validates and roots a factory without running it, rolls
back failed publication, and returns the agreed admission errors. SDK create_task_scope
and drive_task use the same owned factory/session and bounded VM interpreter. An
activation guard restores ownership on unwind; script execution remains serialized.

Ready notices contain only scope/task identities. Coalesced ready bits remain durable
when a dispatcher fails. First activation consumes initial waker registration so a
subsequent provider wake cannot be lost. Replacing a dispatcher replays readiness;
a late failure from the previous dispatcher generation cannot poison the replacement.
Closed scopes refuse admission. Explicit cancellation, owner drop, dispatcher failure,
quarantine and Runtime drop clean up through the owning runtime without resuming script.
Close acknowledgement follows cleanup, and failure reports retain task/scope identity,
failure class, cancellation cause and available stack traces.

Successful results live in traced Task payloads, with independent internal retention
roots while running or awaiting report consumption. Terminal reports also reserve an
independent result root at publication, so an unrelated quarantine cannot prevent
later report transfer or change an already completed outcome. Mutable host roots cannot replace
those private roots. Consuming a report retires the generational slot; remaining Task
handles retain their result, and dropping the last handle permits collection. Sealed
failure publication/report reads remain available after quarantine without running
provider hooks, exposing borrows, allocating heap objects or adding GC edges.

Focused `cargo test -p kagari-embed --test async_execution scoped_task_` passed the
scope contract and shutdown/dispatcher contract. The source-to-encoded-artifact fixture
drives a real for-await task in one-instruction slices with interleaved GC, verifies
element replacement visibility and structural-edit rejection, runs an independent task,
and checks lease release on completion/cancellation. It covers deferred starts, report
backpressure, scope capacity, stale generations, wrong-scope driving, completed result
retention/reclamation, factory traps, failed admission, notification-only completion,
scope cancellation, provider cancellation, runtime quarantine and destruction.
A channel-coordinated producer verifies in-flight dispatcher replacement without sleeps.

The existing standard documentation/example-analysis contract also passed for the new
type/module names. Focused SDK-test Clippy, structure (966 Rust files, no violations
or exceptions), formatting, documentation links and diff checks passed. Manual review
kept registry, sealed heap payloads, notification control and backend orchestration in
their owning modules, with no new re-exports or structural debt. No errors are carried.

AX04 remains open: wire generic script spawn and Task cancellation, extend checked
await contracts and runtime waiting to shared Task caches, bound waiter registrations,
reject dependency cycles and preserve originating-task provenance through waiters.
The current script `.await` still accepts Future only. AX05/AX06 and full GitHub CI
acceptance remain pending. No local full suite or feature/backend matrix was run.

AX04 shared-wait checkpoint (2026-10-08): source `.await` now accepts the installed
Task<T> role as well as Future<T>. MIR and independently verified bytecode check
the exact nominal role, arity and semantic input/output types. MakeFuture remains
Future-only. Expected result types constrain await output without forcing a Future
operand; generic Future inference and nested Future-valued output remain intact.

A bounded runtime dependency registry associates scope tasks with their owned
sessions and tracks unresolved waits, including ordinary owned Future roots. Wait
leases detach on completion/cancellation/retirement without cancelling the target.
Self and transitive cycles trap before insertion, including cycles reaching a task
that was queued when earlier edges were registered. Completion removes incoming
registrations before waking; cached Task payloads remain usable after report and
slot retirement. No waiter retains a borrowed Runtime, frame or native call.
The new max_task_waiters bound defaults to 4096 and has checked identity exhaustion.

Task reports retain reported-task versus source-task identity. RuntimeError and
EmbeddingError carry TaskFailureOrigin through propagation, retaining the initial
cancellation cause while dependent task reports use Dependency. Business Err is an
ordinary output. Host wake panics quarantine the runtime after publication, and
activation unwind cannot overwrite a committed terminal report with retired state.

Focused SDK `scoped_task_shared_wait_contract` and
`scoped_task_dependency_failure_contract` passed through encoded artifacts. Coverage
includes multiple/repeated Task awaits inside for, one-instruction slices plus GC,
mutation guard retention/release, waiter capacity/reuse, report consumption and task
slot reuse before resumption, local/owned-root/cross-scope cancellation, multi-hop
failure identity, cached SDK errors, 1/2/3-task cycles and wake-panic cleanup. Nested
Future outputs remain cold and preserve single-drive alias behavior. Existing HIR
`async_callable_typing_contract` and bytecode `async_artifact_validation_contract` /
`async_factory_artifact_contract` also passed, including Task role validation and
rejection of Task as a cold Future factory role. No local full suite was run.

Focused SDK-test Clippy and structure checks passed (969 Rust files, no violations
or exceptions). Manual review kept Task dependency policy in task/frame owners,
outside generic VM instruction loops; no new re-exports or structural debt. The
current checkpoint has no carried errors. AX04 remains open for generic registered
script spawn and cancellation methods plus the synchronous-handler workflow; host
admission alone is not that acceptance. AX05/AX06 and full GitHub CI remain pending.

AX04 script-handler checkpoint (2026-10-09): installed ordinary generic
TaskScope.spawn and Task.cancel methods, plus the canonical core::task SpawnError
enum and prelude/std exports. Spawn accepts fn() -> Future<T>; existing checked
callable coercion covers generic F: Fn() -> Future<T>, closures and user Fn objects.
No syntax, provider-specific VM branch or alternate admission implementation was
added. CallContext exposes scoped admission/cancellation without driver authority.
Native construction roots both accepted Task handles and rejected enum payloads.

The encoded-artifact synchronous-handler contract passed: the handler returns
before its ordinary factory and IO start; an independent handler runs while its
for loop awaits; structural mutation traps and a later replacement is observed.
An async factory captures an existing Task, waits and cancels without cancelling
the producer. Generic Fn objects run once on first drive. Task<Future<T>> stays
cold at its inner layer. Capacity, scope closure and failed dispatch produce exact
SpawnError cases without admitted work; a trap after successful spawn preserves
the admitted task. Existing callable inference was missing NativeObject structural
constraints; correcting that nominal case enables nested Future<T> output inference
without weakening type identity or bound checks.

Validation: scoped_task_handler_contract and async_callable_typing_contract passed,
including rejection of bare Futures and Unit-returning factories. All standard-library
registered documentation examples analyzed. Focused SDK-test Clippy, fmt, diff and
structure checks passed (972 Rust files, no violations or exceptions). Manual review
confirmed ordinary native registration, explicit imports/module ownership and no
new re-exports or structural debt. No carried errors. AX04 local workflow acceptance is now
complete; AX05 lifecycle/reload/diagnostics, AX06 products and full GitHub CI remain
pending. No full local workspace suite was run.

AX05 reload checkpoint (2026-10-09): the encoded-artifact
`lifecycle_reload_contract` now exercises compatible publication while an old Task
is parked at its first native request. Its second request, generic retained object
method and final result continue using the old generation. A queued factory with
an already-created Future, a retained callable creating a Future after publication,
and a cold owned Future preserve the same pins. Fresh root resolution uses the new
generation. One-instruction slices and allocation/explicit GC stress retained
captures; terminal report consumption and handle retirement release all test roots
and old runtime-value leases. Publication and ready notifications do not run jobs.

The same contract enters staged candidate initialization while ordinary Tasks are
parked. Script/native cold construction and scope admission reject with
ExecutionPhaseViolation before submission, leave the ready set intact and do not
quarantine unrelated work. Existing production generation/phase checks required no
changes. The initial cleanup assertion exposed a test-local shadowed root; explicit
factory-handle drop fixed the test without weakening the zero-root assertion.

Validation: `cargo test -p kagari-embed --test async_execution lifecycle_reload_contract`
passed. Focused SDK-test Clippy, formatting and diff checks passed; structure checked
973 Rust files with no violations/exceptions. Manual review retained test-only
module boundaries and explicit imports; no production ownership changes or new
structural debt. AX05 remains open for the remaining lifecycle audit, detached
logical spawn/await diagnostic sites and debugger driving integration; Task failure
identity alone does not fulfill logical-site provenance. AX06 and full GitHub CI
acceptance remain outstanding. No local full suite or backend matrix was run.

AX05 diagnostic checkpoint (2026-10-09): TaskReport now carries detached SpawnOrigin
for success and failure, including a portable factory target even for direct host
admission. Script admission records its call site; failed Task waits append their
own sites without mutating the cached failure or sibling observers. ErrorTrace
retains the original stack and at most 32 causal Spawn/Await boundaries, with
explicit truncation counts. A queued cancellation has admission provenance before
any body frame exists; later stack capture preserves that earlier provenance.
ErrorFrame and factory origins include the module slot because function indices
alone are not unique within a multi-module fingerprinted program. No source lookup,
script objects, roots, program handles or execution-version leases enter snapshots.

The encoded-artifact async_failure_provenance_contract passed with retained and
stripped source maps: native deferred failure, two independent waiters, exact
portable await instruction locations, consumed report slots, SDK propagation,
before-start cancellation, bounded deep chains and host-created success reports.
The retained reports/errors also survive final GC with zero roots, script objects
and runtime-value version leases. Existing scoped_task_dependency_failure_contract
and VM host_created_err_captures_script_site_and_reentry_traps_keep_inner_origin
passed. Focused SDK-test Clippy, fmt and diff checks passed; structure checked 975
Rust files with no violations/exceptions. Manual ownership/import review kept
diagnostic snapshots separate from GC/execution records with no widened visibility,
new re-exports or structural debt. ErrorFrame/TaskReport gain public fields, an
unpublished internal API replacement without an ABI/format number bump. No carried
errors or full local suite. AX05 remains open for lifecycle audit and debugger
driving integration; AX06 and full GitHub CI acceptance remain outstanding.

AX05 debugger checkpoint (2026-10-09): independent owned roots now expose detached
generational ExecutionId values, and DebugPause identifies its observed execution.
Step requests bind to the most recent snapshot's root instead of being consumed by
another interleaved execution. Synchronous host reentry keeps the same identity and
complete stack. Removing/replacing an observer between bounded drives clears parked
attachment flags; the replacement resolves each retained program on its next drive.
No blocking debugger transport, second execution driver or additional GC owner was
introduced. The public DebugPause field is an unpublished API change without an
ABI/format identifier bump.

The encoded-artifact async_debugger_drive_contract passed: two parked roots,
isolated breakpoint stacks, root-specific stepping, GC between instruction slices,
readiness without execution, detach/reattach while waiting and cancellation after
reattachment with stale late completion and zero remaining roots. The initial
breakpoint test used an unnormalized filename; using the loaded source identity
fixed its setup without changing breakpoint matching. Existing VM
nested_breakpoints_and_traps_include_the_suspended_host_caller also passed.
Focused SDK-test Clippy, formatting and diff checks passed; structure checked 976
Rust files with no violations/exceptions. Manual review retained explicit imports,
existing ownership boundaries and no new re-exports or structural debt. No carried
local errors or full local suite. AX05 remains open for its final lifecycle audit;
AX06 products and full GitHub CI acceptance remain outstanding.

AX05 lifecycle acceptance (2026-10-09): audited the existing contract owners rather
than adding duplicate lifecycle cases. Native completion contracts cover reservation,
duplicate/stale replies, cross-thread delivery/cancellation races, cancellation-hook
failure and generation exhaustion. Session identity and async capture graph contracts
reject foreign/stale state. Source Future/for-await contracts cover cold ownership,
before-start cancellation, nested cursor cleanup, alias mutation, trap and owner drop.
Scope/shared-wait/dependency contracts cover rejected admission, dispatcher replacement,
close/restart, directional cancellation, cached output retirement and shutdown without
a remote reply. Reload, detached diagnostics and debugger evidence is recorded above.

The remaining output-publication gap is now covered by
async_output_publication_contract using the shared SDK fixture and an independently
registered native provider. A worker publishes owned Rust data; conversion runs only
on the driver. Cancellation before conversion never enters the converter; cancellation
after allocating the converted collection prevents continuation effects and releases
temporary roots and the ordinary for-loop lease. A native converter invariant fault
produces one EngineFault report and preserves quarantine. Successful conversion
publishes once. Payload drop and cancellation-hook counts distinguish unclaimed
replies from consumed IO, and late completion is stale. Normal terminal paths leave
zero GC roots/objects; the fault path leaves zero roots and explicitly rejects GC
under quarantine. The initial test incorrectly requested GC after quarantine; its
assertion now checks that restriction rather than bypassing it.

Validation: the focused publication contract and no-default-features
sdk_owned_native_wait_contract both passed. Focused SDK-test Clippy, fmt and diff
checks passed; structure checked 977 Rust files with no violations/exceptions.
Manual review found no new production API, ownership changes, re-exports or structural
debt. Fixture extension only installs an additional public provider and reuses artifact
preparation. No carried local error or full local suite. AX05 is locally complete;
AX06 owns the runnable example, final product/spec updates, source-free acceptance
and CI wiring. Required full GitHub CI remains unrun and is not implied by this gate.

AX06 product/local acceptance (2026-10-09): the runnable `async_tasks` example
installs independent RPC/database providers through the same typed native API.
A synchronous handler admits a scope-owned async closure and returns before IO.
Its ordinary for loop preserves progress across four deferred requests; another
handler runs during each wait. A non-Actor notification counter never drives code.
The host uses bounded slices, consumes success/cancellation reports, closes the
scope without a remote reply and rejects late completion, with no remaining roots
or objects. An optional output path emits the exact portable artifact under target.

The source-free artifact contract shares the example's provider and host modules
through documented cross-target test paths. `scripts/check_features.py --async-only`
checks a standalone artifact-only consumer and its production dependency graph;
the default CI invocation still runs all feature lanes. The async artifact is
generated before those lanes, and GitHub CI also runs the source-enabled example.
Current architecture, embedding/execution specifications, grammar coverage and
example documentation now describe the implemented async behavior. No ABI/format
version change, compatibility reader, Actor framework or async JIT was added.

Focused validation passed:

- `cargo run -p kagari-embed --example async_tasks --no-default-features --features source`;
  optional artifact emission also passed. Initial example compile errors concerned
  the SDK diagnostic's lack of std::error::Error and script module-call spelling;
  explicit diagnostic rendering and `module::request` resolved them.
- `cargo test -p kagari-embed --no-default-features --test artifact_features source_free_async_execution_contract`.
- `uv run python scripts/check_features.py --async-only`: one standalone async
  contract passed, all 13 production crate boundary checks passed, and the host
  dependency graph excluded source analysis and compilation crates.
- `cargo test -p kagari-embed --test async_execution sdk_native_preparation_declines_resume_before_entry`:
  unsupported suspension rejects before backend entry/IO; explicit interpreter
  driving then waits and completes without repeating effects.
- `cargo test -p kagari-embed --test artifact_features real_cranelift_compiles_portable_artifact_without_source`:
  existing synchronous actual-native execution remains supported; its source-enabled
  setup refreshed the existing disposable feature fixture.
- Targeted example/artifact-test Clippy, formatting, local documentation links and
  diff checks passed. Structure checked 981 Rust files with no violations/exceptions.

Manual review retained ordinary module boundaries, explicit imports, small example
responsibilities and justified shared test sources; no new structural debt or
re-exports. The final requirement audit maps syntax/cold factories/single drive to
AX03 callable and source contracts; suspension liveness/guards to AX02 independent
MIR/bytecode and runtime contracts; for-await evaluation, cursor and mutation behavior
to source iterator controls; admission/sharing/cancellation/cycles to AX04 scope and
waiter contracts; reload/cleanup/provenance/debugging to AX05; and public embedding,
source-free products and pre-entry fallback to the AX06 evidence above. There are
no carried local errors or omitted implementation phases. No full local suite or
complete backend/feature matrix was run. Full workspace/all-target Clippy, complete
tests and the entire feature/backend matrix remain unrun GitHub CI acceptance;
no passing remote run or remote publication is claimed.

### Other proposals

These are design documents, not additional active execution plans. Activation and
exact scheduling must be agreed within the user's scope. Completed migrations are
not prerequisites to replay.

| Track | Scope and dependencies |
| --- | --- |
| [Rust interoperability](rust-interop-design.md) — RI00-RI05 | DTO derives, optional schema-backed Serde and independently retained opaque objects; reuse GO conversion/root contracts. External host mutation remains distinct from GO managed-payload editing. |
| [Host API](host-api-refactor.md) — HA00-HA05 | Preparation/load/reload facade and logical entry policy; reuse GO typed calls/roots and RI value extensions. Package identity and update compatibility must be frozen before affected phases, without requiring both entire tracks first. |
| [Packages](package-design.md) — PK00-PK04 | Cargo-style manifests, exact dependency graphs and source/module identities. First source kinds, single-selection policy and defaults remain review choices. |
| [Update model](update-model-design.md) — UP00-UP05 | Compatible publication versus explicit state replacement. GO supplies calls/roots, RI value extensions, PK identities and HA the facade; lower-level cutover belongs to UP. |

## Completed milestones

These tracks are complete and require no replay. Current architecture/specifications
supersede historical designs; use `git show <checkpoint>:docs/implementation-roadmap.md`
for the detailed acceptance record at each final checkpoint.

### Definition visitor cancellation cleanup

Removed 35 consecutive duplicate `check_cancel(cancel)?` calls in 15 definition
mapping modules. Each pair retains one check; distinct traversal/callback checks
remain in place. The Rust diff contains only those 35 deletions. The existing
`kagari-types --test host_metadata` case
`resolved_host_types_still_validate_identity_kind_and_mapping_observes_cancellation`
passed, as did structure (928 files, zero findings), formatting and diff checks.
No new test, full-suite run or cancellation contract change was needed.

### Contract and common responsibility cleanup

**AC01-AC05 complete**, final checkpoint `15fbd5df`. Separated physical ABI and
executable contracts, checked language roles and native declaration analysis,
and completed source/tooling and installation integration. Later CR/LR tracks
refined semantic ownership and library authority; follow the
[current boundaries](architecture.md#crate-responsibility-target).

### Rust-style library namespaces (NS01, complete)

**NS01 complete**, checkpoint `b93a08b3`. Adopted canonical `core`/`alloc`/`std`
modules, identity-preserving re-exports, an explicit prelude and Vec naming.
Collection interfaces remain outside the implicit prelude; GC/shared-object
semantics and Iterable are retained. See [standard declarations](spec/standard-declarations.md).

### Crate responsibility migration (CR01-CR02, complete)

**CR01-CR02 complete**, final checkpoint `2815ba06`. `kagari-types` owns shared
semantic models; HIR consumes explicit semantic providers without execution
contracts. Compiler/contract own representation lowering; runtime/VM remain
frontend-free. See [crate responsibilities](architecture.md#crate-responsibility-target).

### Unified library registration (LR01-LR03, complete)

**LR01-LR03 complete**, final checkpoint `9d083fa8`. Standard and application
libraries use explicit declarations and Rust bindings through Engine registration.
Generated source serves analysis/navigation; source-free installation uses the
same declarations without a binary declaration product or separate handwritten
bootstrap. See [registration architecture](architecture.md#unified-library-registration).

### Nominal enums and propagation (EN01-EN05, complete)

**EN01-EN05 complete**, final checkpoint `c2f87c89`. Source/library/native enums
share ordinary nominal declarations and checked layouts. `?` selects library
Try/FromResidual calls, preserving static checks, effects, roots and generation
pins. Final behavioral, feature, backend and structural checks passed.
See [the execution design](enum-propagation-plan.md) and [trait semantics](spec/traits.md).

### Runtime ownership and host objects (GO01-GO06, complete)

**GO01-GO06 complete**, final checkpoint `97804fe7`. Central runtime stores and
checked identities replace the surrounding Rc/Weak graph; host handles retain
automatically. Exclusive thread transfer, typed registration/results, checked
object/member calls and controlled traced-field edits are integrated. The existing
nonmoving mark-sweep collector is retained. See [the design](runtime-ownership-and-host-api-design.md).

### Scalar fact cleanup (SV01, complete)

**SV01 complete**, checkpoint `ba170cc7`. HIR uses one checked integer record
with integer-only type identity; shared semantics cover all ten integer domains.
Full-width values, casts, arithmetic/traps and physical lowering retain coverage.
No numeric API or physical encoding migration was activated.

### Interpreter execution (IP00-IP04, implemented)

**IP00-IP04 implemented**, final checkpoint `3d3fb624`. Established the interpreter
baseline, reusable execution windows, compact prepared code, physical register
allocation/call windows and reusable native type/layout facts. Correctness,
feature/backend and structural integration passed; performance acceptance is open
under [the follow-up above](#interpreter-performance-follow-up).

### Typed numeric execution (NE01-NE05, implemented; parity open)

**NE01-NE05 implemented**, final checkpoint `da2a2d5c`. Checked scalar facts,
closed execution regions, scalar frame banks, prepared arithmetic/transfers and
CFG-aware value reuse cover the supported numeric matrix. Final correctness,
feature/backend and structural checks passed with no carried build/test failure.
[Runtime architecture](architecture.md#runtime-model) owns implemented boundaries;
Lua parity and the shared-bound source-analysis capability remain open above.

## Outstanding review and performance questions

The [review document](review.md) owns findings and follow-up boundaries. SA4 is
resolved by IR01-IR03; SA8 and SA2/SA3 are implemented and locally accepted by
[NR01-NR05](name-resolution-plan.md), with full CI acceptance pending. SA1 inline
AST reuse remains a separate unactivated HIR analysis follow-up. The completed
namespace/solver migration does not activate macros or other excluded work.

[SA9](review.md#sa9-module-paths-are-copied-into-internal-graph-keys-and-references)
records compact module handles backed by shared portable identities as a separate,
unactivated follow-up. Evaluate reuse of the existing identity context and measure
path-copy/key costs before claiming a performance improvement.

[The architecture review](architecture-review-2026-10-03.md#outstanding-findings)
retains R1/R2/R5/R7/R8 for current-code confirmation: raw-kind safety, prepared-JIT
policy, trait-search cancellation/bounds, repeated whole-program specialization
and semantic cache invalidation. R3/R4/R6 have subsequent implementations; their
remaining costs are measurement questions, not instructions to replay those fixes.
Native expansion requires checked call/root/effect contracts and actual-native
conformance. LLVM and broader interpreter optimization remain separate choices.

The identity measurements retain three finite follow-up costs: transient authoring
path projections, table-index copies on retained-prefix append, and normalized
metadata copies across independent runtimes. Compact IDs do not prove whole-module
sharing or universal speedup. These observations do not activate another migration.

## Execution and verification policy

At activation, use the selected track's finite phases and commit trailers. Keep
unfinished decisions and genuinely carried errors here or in its owning design;
record a command, cause and bounded follow-up. Replace obsolete internals directly,
preserving static typing, bounds, roots, declared access and generation validation.
Do not accumulate successful command logs or completed per-commit narratives.

Use focused checks during development. GitHub CI owns final architecture
integration, the track's behavioral/feature matrix, source-free/backend consumers
and the commands below. Do not run local full suites or split them into package
runs; the repository policy takes precedence over older execution-plan lists.

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Documentation-only edits need content/local-link and diff checks. Measurements
must identify workload, machine, toolchain, profile/features/parallelism and cache
state, separating compilation, preparation and execution. Repository-wide workflow
and exception rules live in [AGENTS.md](../AGENTS.md).
