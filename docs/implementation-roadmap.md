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

### Import and namespace resolution (IR01-IR03, planned)

The user requested an execution plan for [SA4](review.md#sa4-import-records-also-represent-namespace-lookup-state).
[The import-resolution plan](import-resolution-plan.md) owns its detailed contract
and acceptance matrix. This roadmap owns phase order and the progress ledger.
Plan preparation is authorized; implementation has not started. SA1, SA2 and SA3
remain independent findings, including the current iteration-limit concern.

- [ ] **IR01:** Publish shared namespace lookup and qualified declaration targets;
  migrate name/signature/compiler consumers and remove internal namespace imports.
- [ ] **IR02:** Separate import directives, named scope bindings and provenance;
  preserve diagnostics, re-exports, navigation and direct dependency edges.
- [ ] **IR03:** Verify snapshot invalidation, tooling and source/artifact/native
  consumers; complete final checks and update implemented architecture.

Ledger: plan prepared; all implementation phases pending. No implementation checks
have been attempted for this track. Phase commits use `Import-Phase: IR01` through
`Import-Phase: IR03` as specified in the plan.

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
review findings there. SA4 has the planned IR01-IR03 track above; implementation
has not started. SA1-SA3 remain unactivated HIR analysis/import follow-ups.

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

Use focused checks during development. Final architecture integration runs the
track's behavioral/feature matrix, source-free/backend consumers and:

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
