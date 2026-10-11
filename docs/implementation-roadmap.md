# Kagari Implementation Roadmap

This is the queue for unfinished work and open acceptance. Detailed execution
plans own their finite implementation contracts; current behavior belongs in
[architecture](architecture.md) and [specifications](README.md#language-and-execution-specifications).
Completed phases and command logs belong in Git history, not this queue.

## Current baseline

The checked source-to-MIR/bytecode pipeline, source-free loading, explicit native
registration, nominal enums/propagation, runtime-owned roots, pinned reload,
typed host access and host-driven async execution are implemented. Builtin `[T]`
arrays have construction-time fixed length; Vec is distinct. Iterable defaults
provide lazy adapters, terminal operations and explicit FromIterator collection.
The interpreter uses compact values and prepared execution/call/layout contracts.
These capabilities are foundations to reuse, not migrations to replay.

## Library-owned native collections

[NC01-NC06](native-collections-plan.md) are queued and unstarted. Baseline 9cd90557
includes the completed Iterable APIs, retained selected-call slots, consumer scopes
and Vec/HashMap/HashSet FromIterator implementations.

Libraries should own concrete storage and algorithms; runtime should supply checked
dynamic access, accounting, GC, source protection and pinned calls. NC01 identifies
missing public storage and constructor/host evidence capabilities before migration.
The plan covers Vec/HashMap/HashSet, their base cursors, host adapters and source-free
consumers, with an external collection proof. Builtin Array and the full SA21
host-registration migration remain separate.

## Test execution cost

[TP01-TP03](test-preparation-plan.md) have implemented reusable native preparation,
isolated source sessions and three SDK test-owner migrations, with focused
correctness checks passing. Idle-machine timing and retained-memory acceptance
remain open because another test project competed for CPU during validation.
[A subsequent reduced-load sample](performance-baseline.md#focused-preparation-recheck-2026-10-11)
passed nine selected tests twice and confirmed cheaper runtime creation; it does
not establish whole-suite speedup or retained-memory reduction.
[The testing guide](testing.md) defines the reusable runner and case ownership. The
[test-cost audit](performance-baseline.md#workspace-test-cost-audit-2026-10-11)
is historical evidence at 35de1818; this task measures a fresh focused baseline.
Preserve fresh mutable state, source-free/backend/GC boundaries and assertions.

## Interpreter performance follow-up

The finite compact-value and execution-architecture migrations are complete.
[The latest retained evaluation](performance-baseline.md#execution-architecture-evaluation-hp06-2026-10-10)
reports 2.57-51.11x Lua time across 16 matched workloads; parity remains unmet.
Acceptance requires interpreter/Lua median <=1.0 for every unchanged matched
workload on one machine, with uncertainty analysis near parity. Entry, adapters,
alternate numeric diagnostics and JIT results cannot replace that gate.

The proposed, unactivated next scope is a bounded call-lifecycle review: prepared
invocation contracts, active-session admission, parameter/frame-window setup and
return. Preserve changing receiver/type/version, roots, cancellation, host/reentry
and trap order; measure unchanged workloads plus setup and retained memory. Native
collection optimization and kernel/bytecode design are separate decisions.

Shared generic Add failed MIR lowering with
`MissingBinding("checked callable requirement")` at the measured revision. Its
[reproduction](performance-baseline.md#correctness-and-reproduction) remains an
open compiler follow-up; shared-identity benchmarks do not prove that capability.

## Other queued designs

These are proposals, not active execution plans. Activation and exact sequencing
require a concrete task; implemented foundations are not prerequisites to replay.

| Track | Scope and dependencies |
| --- | --- |
| [Rust interoperability](rust-interop-design.md) | DTO derives, optional schema-backed Serde and retained host-owned opaque objects; reuse existing conversion/root contracts. |
| [Host API](host-api-refactor.md) | Preparation/load/reload facade and logical entry policy; reuse typed calls/roots. Freeze affected package/update contracts before dependent phases. |
| [Packages](package-design.md) | Manifests, exact dependency graphs and source/module identities; first source kinds and selection/default policies need decisions. |
| [Update model](update-model-design.md) | Compatible publication versus explicit state replacement; depends on the affected identity, value and host facade contracts. |

## Open review and documentation work

[The review](review.md) owns unresolved SA findings, including inline AST reuse,
compact module handles, HIR model cleanup, local functions, patterns, collection
interfaces, definition identities and host-registration unification. Resolved
namespace/import/default-body/Array work is described by the current specifications
and reading guides, not retained as review tasks.

[Architecture findings](architecture-review-2026-10-03.md#outstanding-findings)
R1/R2/R5/R7/R8 require current-code confirmation: raw-kind safety, prepared-JIT
policy, trait-search cancellation/limits, repeated specialization and conservative
semantic invalidation. Native expansion requires checked call/root/effect contracts
and actual-native conformance; LLVM remains a separate choice.

Earlier identity measurements identified transient authoring path projections,
table-index copies on retained-prefix append and normalized metadata copies across
independent runtimes. Re-measure before optimization; compact IDs alone do not
establish whole-module sharing or universal speedups.

HIR model documentation is complete within its scope. The whole-crate
`cargo rustdoc -p kagari-hir --lib -- --document-private-items -D missing_docs`
audit had unsuppressed gaps in imports/namespace/solver APIs, including
`PerNamespace`, `LookupOutcome` variants and work/limit records. Complete those
docs at their owning documentation checkpoint; model examples do not establish
whole-crate missing-docs acceptance.

## Verification and open CI acceptance

Completed implementations have recorded local acceptance with no carried local
integration failures. Complete GitHub CI feature/backend acceptance remains
unverified; local validation does not establish that matrix passed.

[AGENTS.md](../AGENTS.md) owns verification policy. Local implementation checks
select affected contracts and a small dependent regression set; full workspace
and feature/backend matrices belong to CI. Documentation-only changes need
content/link/diff checks. Performance reports retain revision, workload, machine,
toolchain, profile/features, parallelism and cache state, separating compilation,
preparation and execution.
