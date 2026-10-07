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

### Name resolution (SA8, SA2, SA3, active)

The [execution plan](name-resolution-plan.md) owns the two-space lookup/import
contract and subsequent dependency-driven solver migration. Status: NR01-NR02 complete; NR03 active.
Execute NR01-NR05 in order, with `Resolution-Phase: NR01` through `Resolution-Phase: NR05` on
implementation commits. SA1 inline AST reuse and SA9 compact module handles remain
outside this track.

- [x] NR01: Specify type/value namespaces; replace binding, directive and exported
  alias storage and lookup APIs, including portable registration consumers.
- [x] NR02: Migrate semantic/tooling/cache consumers and derive HIR public members.
- [ ] NR03: Complete focused SA8 local acceptance before changing solver scheduling.
- [ ] NR04: Introduce dependency-driven import work and explicit convergence,
  unresolved-cycle, exhaustion and cancellation outcomes.
- [ ] NR05: Complete integration/measurements and resolve local failures; record
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
| [Async execution](async-execution-design.md) | Host-driven awaited native IO and owned execution lifetimes. Task/completion contracts need review; synchronous acceptance does not require async. |
| [Host task scopes](host-task-scope-design.md) | Scope-owned work launched by synchronous handlers, host/Actor dispatch and bounded driving; no Actor/Tokio policy inside the VM. |

Ordinary callback reentry remains synchronous. Async designs must later share
the same quiescence, cancellation, late-completion and pinned-generation rules.
No new restoration checklist for retired standard-library APIs is active.

## Completed milestones

These tracks are complete and require no replay. Current architecture/specifications
supersede historical designs; use `git show <checkpoint>:docs/implementation-roadmap.md`
for the detailed acceptance record at each final checkpoint.

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

The [review document](review.md) records inline module AST reuse, import-resolution
scheduling, iteration exhaustion and mixed import/namespace records. Append future
review findings there. SA4 is resolved by the completed IR01-IR03 track above.
SA1 remains a separate unactivated HIR analysis follow-up. SA2/SA3 are planned
after SA8 in the [NR01-NR05 track](name-resolution-plan.md); implementation is
not yet activated.

[SA8](review.md#sa8-separate-typevalue-lookup-and-unify-export-information) records
the proposed type/value namespace split and unified export derivation, including
per-space import outcomes, host/tooling/cache consumers and focused acceptance.
The [execution plan](name-resolution-plan.md) formalizes this design, excluding
macros. Its NR03 local gate stabilizes the semantic model before NR04 changes
SA2/SA3 solver scheduling and convergence handling.

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
