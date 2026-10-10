# Kagari Implementation Roadmap

This is the single queue and progress owner for pending work. Detailed execution
plans own their implementation contracts; this roadmap owns activation, phase
order and progress. Implemented behavior belongs in [architecture](architecture.md)
and [specifications](README.md#language-and-execution-specifications).
Completed checklists, intermediate failures and execution logs remain in Git
history; the milestones below retain only outcomes and final checkpoints. The
pre-cleanup record is available with `git show 9dfeba3c:docs/implementation-roadmap.md`.

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
kernels and scalar/managed frame banks with explicit execution boundaries. Internal
values are 16-byte Copy, strings share traced storage, and owned async execution
supports cold Futures and host-scoped Tasks.

Immutable preparation, foundation registration reuse and shared interface metadata
are implemented. Measured costs and reproduction commands live in
[performance measurements](performance-baseline.md). Completed tracks have no
carried build/test failures; their completion does not establish performance gains.

## Pending work and open acceptance

### Name resolution (SA8, SA2, SA3, CI pending)

NR01-NR05 are implemented and locally accepted. Type/Value namespaces, canonical
source/native aliases, snapshot/tooling consumers and dependency-driven import
solving replace single-space storage and repeated whole-graph rounds. Failed solves
do not publish partial snapshots; cycles, exhaustion and cancellation are explicit.
The [contract](name-resolution-plan.md) retains the detailed semantic requirements.

Focused HIR, registration, snapshot and source/artifact SDK checks passed. The
subsequent `source_queries` example migration was fixed and passed strict Clippy;
there are no carried local errors. Full GitHub CI still requires acceptance.
The recorded workload comparison establishes less repeated work, not a universal
speedup: diamond and seed-removal timings increased. Detailed measurements and
resolved diagnostics are in the historical record. SA1/SA9 remain unactivated.

### Explicit native default bodies

ND01-ND04 are locally complete. Generated trait defaults are checked forwarding
bodies; lowering consumes checked call evidence, while source-free linking validates
the portable registration recipe. Native algorithms and explicit overrides retain
one implementation. Focused tests/structure passed; full CI remains pending.
See [the contract and acceptance record](native-default-bodies-plan.md).

### Interpreter performance follow-up

VE00-VE09 implementation and local acceptance are complete. The
[compact execution record](interpreter-value-execution-plan.md) owns representation,
safety boundaries, checkpoints and remaining acceptance. `Value` is 16-byte Copy;
strings/constants share storage, concrete fields/calls use prepared facts, scalar
segments borrow code/banks, and enum results reuse complete layouts within a pinned
program. No new collector, general enum unboxing or JIT expansion was introduced.

[VE08](performance-baseline.md#compact-value-and-interpreter-final-local-evaluation-ve08-2026-10-10)
passed local workspace integration. The separately authorized
[VE09](performance-baseline.md#native-enum-result-layout-reuse-ve09-2026-10-10)
passed 38 focused contracts and reduces map time to 0.489x VE08, with Map::get
requests falling 500,280 -> 50,280 and unchanged GC counts. Complete CI remains unrun.

Lua parity remains **unmet**: all 16 matched nontrivial workloads exceed Lua,
currently 3.66-201.94x median time. Acceptance requires interpreter/Lua median <=1.0
for every matched workload, repeated on one machine with uncertainty analysis near
parity. Entry, host adapters and bounded numeric diagnostics remain separate; no
average, changed semantics or JIT result can replace that gate. Further work needs
a bounded activation, not replay of completed phases.

Remaining measured costs include script/generic/interface boundaries, collection
retention, type/layout work and allocation. Shared generic Add default-method
lowering still has `MissingBinding("checked callable requirement")`; the supported
shared-identity workload does not close that capability gap. Byte-state benchmarks
do not establish NES emulation acceptance.

The [HP00-HP06 execution architecture plan](interpreter-hotpath-execution-plan.md)
is active from 2026-10-10, with HP00 complete and HP01 in progress. At the user's direction it replaces
the unstarted hotspot-by-hotspot approach: audit architecture and earlier IP/NE/VE
optimizations, establish runtime-linked executable identities, separate active
execution ownership from host admission, unify call/return and prepared operations,
then unify layout admission and retire old paths. Prior benchmark wins do not exempt
obsolete mechanisms from replacement. The plan owns the retrospective dispositions,
migration ledger and finite scope; the Lua gate remains unchanged and separate from
local correctness, architectural acceptance, measured benefit and complete CI.

### Async execution (AX00-AX06, CI pending)

AX00-AX06 implementation, local workspace sweep and focused repairs are complete;
checkpoint `f521b2f0`. [The acceptance record](async-execution-plan.md),
[async contracts](async-execution-design.md) and
[host task scopes](host-task-scope-design.md) retain the implemented behavior.
Cold Futures, explicit async closures, ordinary for-body await, host-scoped Tasks,
multiple waiters, directional cancellation and pinned resume are implemented.

The AX sweep found nine fixture failures; all passed focused repairs, with no
carried local errors. That checkpoint did not claim a single all-green rerun.
The later VE08 workspace invocation passed in full. Neither observation establishes
GitHub CI or its complete feature/backend matrix, which remains unrun. The host
example and source-free contracts remain available; no async JIT suspension,
structured task trees or additional async combinator APIs are activated.

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

These implementation tracks are complete and require no replay. Local and CI
acceptance remain separate where noted above. Current architecture/specifications
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

### HIR documentation completion

HD01-HD07 are complete. [The reading guide](architecture/hir.md) and Rustdoc cover
storage/IDs, lowering, resolution, semantic facts, provider inputs and query reuse.
Strict Rustdoc, runnable examples, rendered-page review and lightweight checks
passed. The [documentation contract](hir-documentation-plan.md) remains useful.

### Syntax documentation completion

SD01-SD05 are complete. [Syntax architecture](architecture/syntax.md), AST Rustdoc,
grammar and coverage audit are the current reading material. Annotated wrapper
examples, strict Rustdoc and lightweight checks passed; parser behavior did not
change. See [the documentation contract](syntax-documentation-plan.md).

### Definition spacing enforcement

The structure checker enforces separation of adjacent Rust definitions, including
traits, local items and tests, while preserving compact import/type/constant groups.
Checker self-tests, repository audit, formatting and diff checks passed.

### Test consolidation (contract ownership, complete)

Equivalent early-return cases were consolidated into four evaluation-contract
matrices; duplicate example, enum and obsolete permission fixtures were removed.
Observable order, skipped writes, source/artifact/backend routes and cleanup remain
covered. Focused checks passed; this does not claim every remaining suite is minimal.
[Repository policy](../AGENTS.md) governs further test growth and consolidation.

### Focused test-harness optimization (SA7, numeric fixtures complete)

The four VM numeric contracts retain all 90 cases and 180 direct/decoded executions,
with one compilation per test and separate route-local runtimes. Successful calls
after traps verify cleanup. Focused numeric tests, Clippy and lightweight checks
passed; no stable standalone float or whole-workspace speedup was established.
Other harness/SDK preparation work remains outside this completed scope.

### Import and namespace resolution (IR01-IR03, complete)

IR01-IR03 and the SA5 transitive-reuse follow-up are complete. Qualified source-unit
identities, real import directives, shared namespace catalogs and use-site provenance
replace mixed import/lookup storage. Reuse validates reachable namespace bindings
before existing arena remapping; ambiguous owners and stale arenas reject.

Focused imports, tooling, compiler, source/artifact/native consumers and final local
integration passed; SA5's Windows-path and retained-enum-identity failures were
resolved. Historical commands and intermediate failures are in Git history.
[The contract](import-resolution-plan.md) retains semantic boundaries. NR01-NR05
subsequently replaced the solver and lookup categories; do not treat SA2/SA3 as
unimplemented or replay the old fixed-point scheduling.

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

Use the selected track's finite scope and record genuinely unresolved decisions or
failures at its owner. Completed narratives and successful command logs belong in
Git history. Preserve correctness, static typing, bounds, roots, declared access,
cleanup and generation validation; do not reopen completed migrations implicitly.

[AGENTS.md](../AGENTS.md) owns engineering/checkpoint policy: focused checks during
iteration, a batched full local run only at completed large-task acceptance, and
complete feature/backend matrices in GitHub CI. Report actual local and CI results
separately. Documentation-only changes need content/link and diff checks. Performance
claims retain workload, machine, toolchain, profile/features/parallelism and cache
state, with compilation, preparation and execution measured separately.
