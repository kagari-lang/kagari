# Interpreter performance investigation and execution plan

## Status and authority

**Status: active, authorized on 2026-10-17; IP08-IP10 complete, IP11 next.** The user
requested autonomous execution and coherent phase commits. The
[roadmap](implementation-roadmap.md#interpreter-performance-follow-up) owns activation
and dependencies. This document owns the finite phase order, decisions, acceptance
and progress ledger; it does not reopen completed IP01-IP07 work.

[Performance measurements](performance-baseline.md#interpreter-attribution-windows-2026-10-17)
own the measured revision, machine/toolchain, raw-data methodology, results and
reproduction commands. [Architecture](architecture.md#runtime-model) and the
[execution specification](spec/execution.md) own current behavior. Proposed models
below are not implemented contracts.

## Objective and non-goals

Reduce the actual work required by pure interpretation, rather than accumulating
workload-specific shortcuts. Compare Kagari's interpreter with PUC Lua 5.4, not
LuaJIT, native helpers substituted for script bodies, or either language's JIT.

The existing long-term parity objective remains: every unchanged matched workload
must take no more than Lua time on the same machine, with uncertainty investigated
near parity. A successful bounded implementation checkpoint is not proof of parity.
There is no promised speedup from the source findings alone.

This follow-up owns internal call admission/return, prepared instruction and operand
transport, and bounded collection check/commit ownership. It does not activate:

- JIT/AOT expansion, automatic tiering or benchmark substitution.
- A new GC, universal eight-byte Value, enum unboxing or special Option semantics.
- Per-session storage segmentation or abandoned-owner notification queues.
- The separate [object storage](object-storage-overhead-plan.md),
  [native collection migration](native-collections-plan.md) or
  [host API](host-api-refactor.md) plans.
- Unrelated compiler/language changes, including the roadmap's shared-generic Add
  lowering follow-up. The generic identity benchmark does not prove that feature.

If attribution identifies a substantial change outside these boundaries, record it
here as a decision requiring separate authorization; do not silently expand a phase.

## Evidence and diagnosis

The Windows investigation at `88e87fd4` found 16 matched workloads taking
2.48-41.94 times PUC Lua 5.4.9 time. These are warmed complete workloads, excluding
compilation, verification, preparation and linking, not isolated operation costs.
The preceding macOS measurements are separate historical evidence.

| Finding | Confirmed evidence | What remains unknown |
| --- | --- | --- |
| Excess logical work | Arithmetic executes 550,015 Kagari operations versus 250,007 Lua operations; Kagari performs five local reads/writes per successful iteration. | Time attributable to transport versus dispatch, polling and arithmetic. |
| Heavy small calls | Moving the same recurrence into a helper increases Kagari elapsed time about 9.34x while logical operations grow about 1.40x. Calls and recursion have no per-call allocator traffic. | Non-overlapping time shares of admission, transfer, initialization, ownership and return. |
| Surviving scalar machinery | Release-object assembly contains a jump-table dispatcher, operand-state checks, an out-of-line cancellation check and indirect numeric-kernel Result transport. | Branch/cache effects and the gain from any particular representation change. |
| Dynamic invocation costs | Warm interface/shared-generic runs allocate no new environments or applications per call; application-key lookup and semantic admission still execute. | Relative cost of warm selection, application lookup, generic transport and return checks. |
| Repeated collection work | Map insertion has two contains_key calls followed by insert; sealed Vec set overlaps preflight and commit checks. | Elapsed benefit of a unified transaction and impact across custom-key/reentrant paths. |
| Map result protocol | The fixture's 2,000 successful gets construct 2,000 heap enum objects, then execute 2,000 variant tests and 2,000 payload reads through VM boundaries. | Separate contributions of allocation, native transition, enum handling and GC. |

These are not proof that every check is redundant. Nor do two-process diagnostic
measurements establish small-change significance. No current-HEAD CPU-stack or
hardware-counter profile was collected. The historical approximately 81% broader
scalar-loop sample share must not be relabeled as current dispatch cost.

### Example: a scalar loop

For the successful iteration of `sum += i % 97; i += 1`, current Kagari execution
can be summarized as follows. This is a logical sketch, not an instruction dump:

```text
load i; load n; compare; branch
load sum; remainder; add; store sum
increment; store i; jump
```

Lua's matching while loop uses comparison/control flow, constant remainder,
addition, immediate increment and a backedge: five dispatched handlers on the
successful numeric path. Both fixtures use while; specialized Lua numeric-for
instructions do not explain this comparison.

Kagari already hoists constants and reuses temporary storage. The remaining model
keeps named locals separate from temporaries and prepares their reads/writes as
moves. Reducing physical slot count therefore does not remove those operations.
A better model must explain how debugger-visible local values and canonical trap
positions remain available when physical transport changes.

### Example: a tiny script call

For `state = step(state)`, the current useful body is only a few arithmetic
operations, but the runtime performs:

```text
end caller cursor borrow
select prepared target
admit arguments and executable dependencies
initialize callee window; transfer arguments; publish roots
enter callee region
return; retire callee window; publish result
re-enter caller region
```

The proposed direction is one authoritative internal invocation transition carrying
already-established selection and source-window evidence through transactional
admission and return. It must still validate actual values, unavailable candidates,
changed dynamic receivers/environments, depth and destination authority. It is not
an unchecked public call API or a second fast-call implementation.

### Example: a map result

For `match values.get(key) { Some(value) => value, None => -1 }`, the integer-key
fixture performs a builtin-key lookup, allocates an ordinary enum result, then
separately tests and projects that enum. It does not call script Hash/Eq or allocate
a vector for each integer key. Lua returns the existing value or nil directly.

Option discrimination is required; today's allocation and VM transitions are
representation choices. First attribute them separately. This plan may investigate
prepared enum handling, but does not authorize a heap-layout/Option representation
migration or a private map-only result format.

## Mature architecture defaults

Prefer an established mechanism when its ownership and semantic assumptions fit.
The default is to adapt the models below through existing owners, not invent another
cache, proof wrapper or fast-path subsystem for each workload. A custom alternative
must explain which concrete Kagari requirement the mature model cannot satisfy,
what minimal adaptation is needed, and how correctness and performance will be tested.
This chooses reference models, not unmeasured implementation details or speedups.

### Reference versions and evidence

Sources were consulted on 2026-10-17. Lua sources identified themselves as **5.4.9**;
OpenJDK implementation references are pinned to **jdk-25-ga**, x86 template
interpreter code. These are not observations of JIT-compiled Java execution. The
**Java SE 25 JVM specification** establishes abstract contracts, not physical frame
layout or a requirement to generate assembly handlers. Rust implementation/API
references are pinned to **1.90.0**; the MIR development guide is a rolling explanation
read on that date, not a benchmark or versioned implementation guarantee.

| Source | Mechanism actually used as evidence |
| --- | --- |
| Lua 5.4.9 [lopcodes.h](https://www.lua.org/source/5.4/lopcodes.h.html), [lvm.c](https://www.lua.org/source/5.4/lvm.c.html), [ldo.c](https://www.lua.org/source/5.4/ldo.c.html) | Register operands, immediate/constants, comparison-controlled jumps, arguments already on the shared stack and frame switching within one interpreter invocation. |
| JVMS SE 25 [verification](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-4.html#jvms-4.10), [frames](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-2.html#jvms-2.6), [typed instructions](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-2.html#jvms-2.11.1) | Link-time instruction/initialization constraints, predeclared frame requirements and operations whose types are already known. |
| OpenJDK jdk-25-ga [verifier.cpp](https://github.com/openjdk/jdk/blob/jdk-25-ga/src/hotspot/share/classfile/verifier.cpp), [stackMapFrame.cpp](https://github.com/openjdk/jdk/blob/jdk-25-ga/src/hotspot/share/classfile/stackMapFrame.cpp), [templateTable_x86.cpp](https://github.com/openjdk/jdk/blob/jdk-25-ga/src/hotspot/cpu/x86/templateTable_x86.cpp) | verify_iload/get_local establish local contracts; fast_iload and iop2 generate direct typed handlers. prepare_invoke consumes resolved method-entry facts while retaining dynamic duties. |
| OpenJDK jdk-25-ga [templateInterpreterGenerator_x86.cpp](https://github.com/openjdk/jdk/blob/jdk-25-ga/src/hotspot/cpu/x86/templateInterpreterGenerator_x86.cpp), [interp_masm_x86.cpp](https://github.com/openjdk/jdk/blob/jdk-25-ga/src/hotspot/cpu/x86/interp_masm_x86.cpp), [frame.cpp](https://github.com/openjdk/jdk/blob/jdk-25-ga/src/hotspot/share/runtime/frame.cpp) | generate_normal_entry positions parameters and initializes additional locals; dispatch/safepoint handlers and VM calls publish execution state; oops_interpreted_do scans mapped live references. |
| Rust 1.90.0 [MIR VarDebugInfo and VarDebugInfoContents](https://github.com/rust-lang/rust/blob/1.90.0/compiler/rustc_middle/src/mir/mod.rs#L1231-L1289), [MIR guide](https://rustc-dev-guide.rust-lang.org/mir/index.html) | Named-variable debug identity maps to a place or constant independently of physical storage; source variables and temporaries need not have separate storage categories. |
| Rust 1.90.0 [HashMap Entry](https://doc.rust-lang.org/1.90.0/std/collections/hash_map/enum.Entry.html), Lua 5.4.9 [lvm.h](https://www.lua.org/source/5.4/lvm.h.html), [ltable.c](https://www.lua.org/source/5.4/ltable.c.html) | An occupied/vacant entry or found slot carries a membership decision into its update instead of asking independent layers to rediscover it. |

### Preferred models and priority

This table maps decisions into the existing phases; it adds no new work stream.
Priority follows proof dependencies and measured impact, not a claim that every
listed mechanism must be implemented.

| Priority / phase | Preferred mature model | Kagari change to investigate | Disposition |
| --- | --- | --- | --- |
| Prerequisite, IP08 | JVM verifier-to-interpreter proof boundary | Inventory exactly which facts the executable seal, physical preparation and admitted window establish. | Adopt the proof discipline; do not assume missing proofs. |
| First implementation focus, IP09 | Lua internal frame switching, OpenJDK resolved invocation facts | One internal call/return transaction consumes prepared facts instead of reacquiring the same evidence through general APIs. | Adapt through current windows/roots; a new stack ABI remains conditional. |
| Next representation decision, IP10 | Rust debug-location separation and Lua direct register operands | Map source locals to their current execution locations, avoiding a permanently separate mirror slot solely for inspection. | Prefer this model if local transport is changed; preserve existing debug availability. |
| Same IP10 decision, not a second phase | JVM type-directed handlers and Lua compact operand forms | Compare a uniform prepared operation model with the current indirect-kernel ABI and operand/state traffic. | Adapt only with attribution; retain common numeric semantics. |
| Bounded library work, IP11 | Rust Entry / Lua found-slot update | One checked map insertion owner; one sealed Vec admission/commit transaction where reentry is excluded. | Prefer existing standard mechanisms before custom handles/storage. |
| Cross-cutting, IP09-IP11 | Managed-runtime state publication at effect boundaries | Explicit PC, root and executable-dependency publication before callbacks, allocation/GC, parking and observation. | Retain/adapt the current foundation; do not weaken polling. |

#### Proof boundary: establish once, consume only while valid

JVMS verification proves instruction types, valid local access and assignment before
use. OpenJDK's local-load handler therefore need not rediscover those facts. This
is a useful target for Kagari, but current verification is not yet equivalent:

- [MIR flow verification](../crates/kagari-mir/src/verify/flow.rs) checks definite
  initialization along reachable control flow.
- [Bytecode flow verification](../crates/kagari-bytecode/src/verifier/flow.rs)
  currently returns early for ordinary functions that neither suspend nor contain
  MakeFuture. Its full initialization analysis is therefore not a general seal for
  ordinary source-free bytecode. A declared slot representation is not that proof.
- [Physical layout](../crates/kagari-runtime/src/module/execution/layout.rs) assigns
  bank/slot locations. Slot coloring must preserve any initialization/type evidence;
  a ScalarSlot alone does not prove initialized contents or live-window authority.
- [Frame APIs](../crates/kagari-runtime/src/frame.rs), including jump_to and writes,
  and resume/native-result publication must be included in the proof inventory.
  An in-range arbitrary PC is not automatically a verified CFG successor.

Retain the existing dynamic checks pending this inventory. Their completeness is
not established here: scalar initialization flags describe physical slots rather
than every logical definition after reuse, managed slots have different admission,
and an in-range PC does not prove a legal CFG transition. This review demonstrates
neither a safety defect nor grounds for deleting checks. Before optimizing any check,
name its producer, consumer, validity interval and invalidation boundary. If the required general bytecode proof is outside the bounded
IP10 change, explicitly defer the dependent optimization. A larger verifier migration
requires a scope decision here before implementation, not a second verification engine.
Rust slice bounds that remain for safe memory access are also distinct from semantic
slot-range validation; a proof model does not automatically remove their machine cost.

#### Calls: reuse the execution model, not Lua's raw pointers

Both Lua and OpenJDK make callee frame requirements available before entry, and
parameters can already occupy the callee's expected area. Kagari already prepares
CalleeLayout and source/destination locations; retain them. The first change should
consolidate admission/transfer/publication through those owners, not add another
callsite table or skip dynamic selection.

A Lua-like outgoing/incoming argument-window convention is a conditional second
choice only if transfer/initialization attribution warrants a new ABI. It must
support disjoint live caller data, movable backing storage, stale-window rejection,
managed roots, owned suspension and synchronous host reentry. Lua's closure/stack
retention and OpenJDK's method-holder rooting are references, not substitutes for
Kagari's pinned Program/environment edges. OpenJDK still initializes extra locals;
verification is not evidence that mature interpreters perform no frame clearing.

#### Locals: debug information describes storage, not duplicate execution

For `sum = sum + value`, prefer an operation reading/writing the selected execution
locations with debug metadata explaining where source variable sum is available.
Do not require `load local -> calculate temporary -> store local` solely to keep a
second permanent copy for inspection. Rust's VarDebugInfo is the reference separation,
not a proposal to import Rust's ownership model or an SSA rewrite.

Kagari's [debug emission](../crates/kagari-compiler/src/bytecode/debug.rs) currently
records live ranges of fixed local slots, and
[frame preparation](../crates/kagari-runtime/src/module/execution/layout.rs) allocates
those slots separately from temporary colors. If transport is removed, MIR/debug
facts must map into valid execution locations at every existing observation point,
including branches, captures, calls and resume. Rust may report optimized-out values;
that is not authorization to weaken Kagari's current local-availability contract.
Location metadata is bounded immutable function data, not a second mutable local store.

#### Typed operations: specialize known facts, preserve checked semantics

Kagari already selects numeric types and kernels before execution. Borrow the mature
idea of type-directed execution; do not claim this selection is missing today.
The remaining question is whether a coherent handler/operand representation avoids
material kernel-call, Result-ABI and transport costs. A direct handler should reuse
the existing semantic owner through compiler-visible calls or bounded shared generation,
not duplicate checked i32 operations in another loop.

Do not import Java/Lua wrapping addition, Java's reduced narrow-integer instruction
set or its operand-stack architecture. Kagari's numeric domains, overflow traps,
evaluation order and source locations remain authoritative. Compact register encoding
is the closer interpreter baseline; JVM typing/proof separation is the useful lesson.
HotSpot's assembly-generated template interpreter is not a requirement to add a native
handler generator to this safe-Rust runtime.

#### Collections: carry the lookup decision into the update

Prefer occupied/vacant transaction ownership over outer contains_key, inner
contains_key and final insert. Use the existing Rust map and standard Entry-style
mechanism where compatible; no new hashing implementation or dependency is presumed.
The standard API alone does not guarantee Kagari's fallible reserve-before-commit,
exact growth accounting or custom-bucket atomicity. Capacity preparation, mutation
guards and auxiliary bucket updates must belong to the same authoritative transaction.
No entry reference may survive table growth or a callback. Do not promise exactly
one hash/probe merely from using Entry; measure the actual resulting searches.

For sealed Vec set, transfer the same principle to a checked element update whose
borrow excludes conversion/reentry. Keep SDK revalidation when conversion can run
foreign code. A lookup/update transaction is not a persistent cache and must not
outlive its exclusive storage access.

#### Publication: borrow the boundary discipline, not weaker cancellation

OpenJDK spills cached interpreter state and establishes runtime-visible frame/PC
state at VM/safepoint transitions. Its runtime interpreter oop maps are not the
class-file verification StackMapTable. Kagari already roots managed banks and frame
metadata; reuse that precise-root model instead of adding another GC-map subsystem
because Java has one.

Any reduced PC/state traffic must retain complete effects, live operands/results,
program/environment roots and released transient borrows before runtime observation.
Java safepoint coordination is not Kagari's per-execution cancellation or sliced
resumption. Copying a polling interval or unifying unrelated signals without proving
ordering/wakeup/visibility semantics is not approved. Optimize the cost of honoring
existing points, not the meaning or frequency of those points.

### Adoption rule

Retain already-sound foundations, use a mature model as the default, and adapt only
the boundaries that differ. A proposal must identify concrete remaining work,
mechanism/source version, semantic owner, evidence lifetime, mismatched assumptions,
expected work reduction and validation. If the mature mechanism fits, prefer it over
a bespoke alternative. If it does not, explain the smallest required difference.
Neither a source citation nor architectural familiarity establishes a Kagari speedup.

## Ownership and design constraints

### Calls and frames

- Retain prepared target records, direct argument transfer, scalar return packets
  and free-list window reuse. Do not replay superseded argument-vector or repeated
  preparation optimizations.
- Review candidate-value admission, heap-value admission, physical admission and
  transfer as one transaction. Scalar arguments already avoid heap graph walks;
  measure their remaining source-window/state work rather than claiming otherwise.
- Reuse evidence only while no callback, collection, suspension, mutation of the
  relevant state or authority change can invalidate it. Publication must establish
  all managed and executable roots before any safepoint.
- Keep host entry, untrusted/source-free products, dynamic selection and resumed
  execution as explicit checked boundaries. Prepared descriptors are not perpetual
  permission to execute a retired or unavailable candidate.
- Ordinary nested calls retire tail windows. Do not use independent-session
  compaction as their explanation or introduce segmentation without separate evidence.

### Instructions, locals and observation

- MIR owns checked program semantics and source/debug facts; bytecode/preparation
  owns their physical interpreter representation. Backends must not infer types again.
- Review a uniform operand/instruction/local model, not five isolated i32 shortcuts.
  Keep the shared checked numeric implementation; do not duplicate arithmetic semantics.
- Define logical-PC mapping, local availability, trap order, intermediate roots and
  slice/resume points before eliminating transport or combining operations.
- Keep cancellation and observation guarantees. Investigate generated call/ABI and
  state traffic without silently polling less often or disabling checks for a benchmark.
- A compact encoding trades decoding against footprint; direct handlers trade code
  size against indirect calls. Select through evidence, not a presumed winner.

### Collections and result publication

- One insertion owner should decide new versus replacement and own capacity,
  accounting and commit. Preserve allocation-failure atomicity and structural revisions.
- A sealed Vec transaction may share checks only if no conversion/reentry gap exists.
  General SDK operations must retain revalidation across real callbacks and conversions.
- Respect alias/mutation guards, checked handles, selected Hash/Eq and exact installed
  declarations. Do not recognize collection method names or create duplicate algorithms.
- Allocation-capable primitives require explicit roots, PC and effect transitions;
  never classify them as a nonallocating closed operation to keep them in a cursor.
- General enum representation changes remain outside scope. If needed, record the
  required decision and stop that branch rather than hiding it in a Map optimization.

## Phase order and bounded deliverables

### IP08 — Freeze evidence and attribute remaining execution costs

Before production edits, freeze the ordinary baseline with the existing benchmark
workflow on the implementation machine/toolchain. The Windows investigation is
starting evidence, not a substitute for that before/after baseline.

Select existing representative workloads: arithmetic/branches; direct/helper/calls/
fibonacci; concrete/interface/shared-generic calls; arrays/maps. Collect ordinary
CPU-stack or instruction-offset attribution where tooling permits, plus independent
logical/protocol/allocation counts and focused assembly. Do not start with a new
benchmark matrix or a permanent generic profiling framework.

Deliver a non-overlapping cost map where possible. Inclusive families must not be
added. Report missing symbols/inlined attribution and unavailable hardware counters.
For each proposed change, state concrete evidence, owner/model, preserved invariants,
selected contract tests, target benefit and regression/memory/setup gates **before**
implementation. Include the verifier/preparation/entry/PC proof inventory from
[the mature-model review](#proof-boundary-establish-once-consume-only-while-valid),
not just hotspot percentages. If a broad migration has no adequate attribution,
leave it deferred.

### IP09 — Internal call admission and return

Start from the mature-model defaults: retain prepared callee requirements and
resolve the remaining internal transition through one owner, adapting Lua/OpenJDK
call discipline to checked Kagari windows. Design that transition to reduce repeated
argument/state processing while retaining transactional publication. Treat concrete, interface, shared, closure and
managed-return paths as consumers of the same lifecycle, not separate semantic engines.

First validate the model on existing direct/helper, recursion and managed-string
controls. Extend only to the dynamic consumers actually affected. A new stack ABI or
retained backing-storage model is conditional on measured initialization/transfer
cost, with roots, active extents and stale-window behavior defined in advance.

Acceptance must show actual reduction in targeted work and a paired execution benefit,
without sacrificing cleanup, reload pinning, host reentry or memory bounds. A warm
preparation counter remaining zero is a guard, not the benefit itself.

### IP10 — Instruction and operand model

Using IP08 attribution and the retained IP09 model, select at most one coherent
representation change for the first checkpoint. Inspect the out-of-line poll call,
kernel Result ABI, surviving operand checks, local transport and logical-PC state.
Do not assume all source-level checks survive or that a jump-table change is needed.

Use Rust-style debug-location separation as the default if local transport is
changed, and provide debugger/local-location and control-flow mapping. Review which
verifier facts actually reach physical operands; do not treat current ordinary
bytecode as definitely initialized merely because MIR was verified. If numeric dispatch/encoding changes, cover all affected numeric domains
through the common semantic owner. If fusion lacks an interior-PC/root/resumption
model, leave it deferred. Acceptance includes code/metadata footprint and preparation
cost as well as execution; fewer logical operations alone do not establish speed.

### IP11 — Collection transactions and enum-boundary decision

Start with standard Entry/found-slot transaction ownership for the source-confirmed
map membership duplication and sealed Vec preflight/commit overlap through their
existing storage owners. Preserve the general custom-key
and reentrant SDK paths; update callers rather than retaining duplicate implementations.

Measure map native transition, result construction, enum handling and GC separately.
Only if attribution justifies it, design prepared enum test/projection using the
existing ordinary enum representation and one checked implementation. Otherwise
record the decision and leave that branch deferred. Do not require enum unboxing or
activate the separate collection migration to complete this phase.

### IP12 — Integrated evaluation and handoff

Evaluate the retained changes together against the frozen baseline, not just each
local success in isolation. Use the unchanged matched workloads and selected lifecycle/
retained-memory controls from the affected scope. Keep compilation/preparation,
execution, allocation and retained memory distinct.

If targets are missed, revisit the affected model and document the outcome; do not
add benchmark exceptions or endlessly extend the phase list. State separately:
implementation scope complete, local checks passed, pending CI and Lua parity met or
unmet. Move lasting contracts to architecture/spec owners and measured conclusions
to the existing performance document; remove this plan after completion and handoff.

## Verification and performance acceptance

Follow the [benchmark procedure](../benchmarks/lua-comparison/README.md#beforeafter-workflow)
without changing algorithms, sizes, result checks or semantic policies. Freeze old/new
ordinary images with the same harness, lockfile, features, profile and toolchain.
Use the fixed seven alternating process pairs, three warmups and eleven samples;
report paired ratios/ranges and investigate uncertainty without favorable reruns.
No builds, tests, allocation instrumentation or sampling overlap ordinary timing.

Use the existing 1.05 execution regression limit unless a stricter bound is declared
before the phase. Predeclare targeted benefit and setup/retained-memory limits from
IP08 evidence; no numerical gain is invented by this document. A <=1.05 result alone
is not acceptance of an optimization with no demonstrated targeted benefit. If a
required gate fails, disclose it and diagnose the model before further local tuning.

Local correctness selects a small existing contract set for the affected boundary:

- Calls: argument/result admission, depth, frame isolation, overflow/trap cleanup,
  managed roots, synchronous reentry and generation-pinned generic/interface reload.
- Instructions: checked numeric domains, once-only evaluation and trap order,
  debugger logical positions, cancellation and sliced/owned resumption.
- Collections: bounds, alias guards, structural revisions, custom Hash/Eq reentry,
  failure atomicity, enum provenance and source-free executable validation.

Record exact selected commands and rationale in the ledger before each implementation
checkpoint. Reuse passing evidence for unaffected code and avoid redundant smoke tests.
Run repository structure, format, workspace all-target Clippy and diff checks for
implementation acceptance. Full-workspace tests and the complete feature/backend/
platform matrix remain CI-only; focused local checks cannot establish that acceptance.
Documentation-only preparation needs link/content/diff checks, not workspace builds.

## Progress and decision ledger

- [x] IP08: baseline, attribution and predeclared scoped gates.
- [x] IP09: shared internal call admission/return model and evaluation.
- [x] IP10: instruction/operand decision and justified local implementation.
- [ ] IP11: collection transactions and explicit enum-boundary decision.
- [ ] IP12: integrated evaluation, acceptance report and documentation handoff.

### IP08 — 2026-10-17

Activated by the user's goal-mode/phase-commit authorization. The investigation
remains recorded at [its measurement owner](performance-baseline.md#interpreter-attribution-windows-2026-10-17).
Before production edits, froze the ordinary release image with the original,
source-form and owned-execution-control suites:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --freeze-baseline target/interpreter-ip08/before
uv run python scripts/benchmark_lua.py --source-forms --interpreter-only --freeze-baseline target/interpreter-ip08/before
uv run python scripts/benchmark_lua.py --execution-controls --interpreter-only --freeze-baseline target/interpreter-ip08/before
```

Image SHA-256: `2a6605f8368310cae43e4981c66a55d7df376dc088b4f66a432574c9bcf5286c`.
Lockfile SHA-256: `1a18e39bd7312c9ca332af0ebeb81aabb3aca293256f75c85b028a3873b7ba06`.
Revision, machine, release profile, features and default build parallelism match the
Windows investigation. These are one-process freezes, not optimization acceptance.
All checksums passed. Source-form timing was visibly variable, including Lua;
P/E-core placement and desktop load are uncontrolled. Keep these reports and use
fresh seven-pair comparisons; do not replace inconvenient baseline samples.

The source/assembly/protocol evidence above supports bounded work, not a complete
CPU cost partition. WPR/xperf are installed, but no process-isolated stack recording
was established; no machine-wide tracing was started. Current CPU shares and hardware
counter attribution remain unavailable. This limits the justified migration size.
The proof inventory above also blocks removing initialization checks or assuming
arbitrary in-range PCs satisfy verified control flow.

#### Predeclared implementation decisions and gates

- **IP09:** consolidate existing admission into a private owner. Inspect candidate
  membership and heap ownership in one managed-value traversal, retaining the original
  candidate-first error order and reporting heap failure only after cancellation,
  arity/environment and execution-borrow admission. No safepoint/callback/mutation may
  intervene. Reuse one storage borrow for nested return retirement/publication,
  preserving late destination validation and the allocating adaptation boundary.
  Target `calls <= 0.98` candidate/baseline; recursion and source-form controls must
  remain `<= 1.05`. No new stack ABI, retained cache or frame field.
- **IP10:** first test ordinary cross-crate `#[inline]` on the existing cancellation
  check (Rust's inline attribute is a code-generation hint, not a semantic change).
  The release assembly currently calls this tiny Acquire-load predicate at each
  original boundary. Target `arithmetic <= 0.98`; branches and call controls `<= 1.05`;
  confirm the outlined call disappears. Poll positions/order remain identical.
  This is a local code-generation correction, **not** a completed instruction-model
  redesign. Debug-location/local transport, complete bytecode proof, numeric-kernel
  ABI replacement and fusion are explicitly deferred for inadequate attribution.
- **IP11:** one scoped map membership/update transaction and one sealed Vec
  admission/commit owner, through existing storage. Target `maps <= 0.98` and
  `arrays <= 0.98` respectively, with the other controls `<= 1.05`. Evaluate these
  independently if either misses. Standard Entry is a model, not permission for
  infallible growth before the existing fallible reserve/accounting gate. Preserve
  custom-key behavior, SDK conversion revalidation and every existing poll. Enum
  in-region handling is deferred: no separate CPU attribution justifies extending
  the effect/root proof in this checkpoint.
- Each retained candidate must reduce its intended source/assembly work, pass its
  target and preserve all selected semantic contracts. A miss triggers diagnosis
  and rejection or an explicit bounded scope decision, not stacked local tuning.
- For all phases: unchanged workloads/toolchain/features; seven alternating pairs;
  other measured execution rows `<= 1.05`. Diagnostic setup median (total setup
  per process) `<= 1.10`, ordinary executable size `<= 1.02` per phase. These local
  changes add no preparation product or retained descriptor; explain any unexpected
  setup/size failure rather than claiming it passed. No new steady-state per-call
  allocation; unchanged window/layout representation and bounded reuse. Final owned
  controls must release all roots/depth and diagnostic teardown must have zero net
  requested bytes; requested bytes are not RSS or an OOM-failure proof.

#### Selected correctness commands

IP09 runs the runtime storage/frame owners plus representative VM frame, trap,
reload and reentry contracts; it does not sweep the VM package:

```text
cargo test -p kagari-runtime --lib frame::values::tests::
cargo test -p kagari-runtime --lib frame::execution_frames::
cargo test -p kagari-vm --lib deterministic_frames_
cargo test -p kagari-vm --lib traps_release_frame_roots_and_call_depth
cargo test -p kagari-vm --lib generic_interface_and_retained_closure_keep_their_environment_after_reload
cargo test -p kagari-vm --lib host_reentry_cannot_swallow_root_termination_and_releases_borrows
cargo test -p kagari-vm --lib async_owned_drive_contract
```

IP10 reuses passing unaffected call evidence and selects cancellation boundaries:

```text
cargo test -p kagari-common --lib cancellation
cargo test -p kagari-vm --lib cancellation_after_a_host_effect_releases_frames_and_preserves_the_effect
cargo test -p kagari-vm --lib cancellation_at_observed_boundaries_cleans_callback_scopes
```

IP11 selects map accounting/revision, custom-key reentry, Vec alias/bounds and SDK
callback mutation contracts:

```text
cargo test -p kagari-vm --lib hash_map_replaces_duplicates_and_accounts_units
cargo test -p kagari-vm --lib collection_revisions_distinguish_replacements_from_structural_writes
cargo test -p kagari-vm --lib custom_hash_handles_use_selected_calls_with_gc_reentry_and_trap_cleanup
cargo test -p kagari-vm --lib custom_key_factories_pin_selected_operations_through_reentry_and_reload
cargo test -p kagari-vm --lib vector_primitives_preserve_element_access_aliases_and_bounds
cargo test -p kagari-vm --lib callback_alias_writes_and_nested_edits_are_rejected_without_changing_slots
```

Run the required structure/format/workspace Clippy/diff checks at implementation
checkpoints. Full workspace tests and the exhaustive backend/platform matrix remain
CI-only. IP12 compares all unchanged original/source-form workloads and the owned
lifecycle controls to the frozen IP08 image. No new build errors are known at IP08;
existing compiler/CI follow-ups stay with the roadmap.

### IP09 — 2026-10-17

Implemented one private admission owner, replacing the split stack/constructor
orchestration. Candidate membership and heap identity share a managed traversal;
heap failure remains deferred, and later candidate/window errors keep their original
precedence. Physical/domain admission and argument copy remain separate. Returns
retain one bank borrow across retirement/publication, with allocation-capable adapters
outside it and destination validation after retirement. No persistent fields, ABI,
cache, allocation site or window representation were added.

All selected commands above passed: 7 storage tests, 14 frame tests (including two
new distinct precedence/atomicity contracts), and 7 VM tests. The manual release
window-timing test remains deliberately ignored during correctness testing; it is
not a skipped contract test. Structure (1,287 files, no exceptions), format,
workspace all-target Clippy with denied warnings and diff checks passed. Manual
review covered production imports, ownership, private visibility and cleanup order.

Seven-pair ordinary comparisons against IP08 passed all checksums and checker gates:

| Workload | Median candidate/baseline | Pair range |
| --- | ---: | ---: |
| calls | 0.97725 | 0.94334-1.05190 |
| fibonacci | 0.99459 | 0.93119-1.02001 |
| forms_helper | 0.96473 | 0.93203-1.04881 |
| forms_string_calls | 0.98224 | 0.93124-1.01391 |

The other ten source-form controls span 0.99543-1.03757, all within 1.05. Total
setup paired medians were 0.99096 (calls), 0.99829 (recursion), 1.00036 (forms).
Executable size stayed 26,843,136 bytes; image SHA-256 is
`f94306166c22b46cd0d1be9ae43984c6ef6b5428aede1adb4f52eea9974c6752`.
Reports were produced at 11:28:48, 11:30:45 and 11:32:21 UTC. Reproduction uses the
existing driver with `--workload calls`, `--workload fibonacci` or `--source-forms`,
`--baseline-executable target/interpreter-ip08/before`, then the checker (calls adds
`--target calls=0.98`). The measured target passes narrowly; the wide ranges do not
prove a statistically stable 2% gain. Independent helper movement and removed work
support retaining the bounded owner correction, not claiming recursion improved.
Final integrated allocation/lifecycle diagnostics remain IP12 work; no new allocation
or retained-state mechanism was introduced. CI and Lua parity remain unverified/unmet.

### IP10 — 2026-10-17

Selected the predeclared local code-generation correction, not a representation
migration: ordinary `#[inline]` on `CancellationToken::check`. Its Acquire load,
result, identity/subscriptions and all consumers' poll locations are unchanged.
Rust 1.90's [inline attribute contract](https://doc.rust-lang.org/1.90.0/reference/attributes/codegen.html#the-inline-attribute)
is a hint; actual release-object inspection establishes this compiler's result.
The unbounded scalar slice previously had two cancellation-check call relocations;
now it has none and directly reads/tests the cancellation flag. Other operand
bounds/initialization checks and the common numeric kernels remain unchanged.
Broad local/debug transport, complete bytecode initialization proof, numeric ABI
and fusion remain deferred for the reasons in IP08, not silently counted as delivered.

Froze the accepted IP09 ordinary executable and original/forms reports before the
edit using the IP08 freeze commands with `target/interpreter-ip09/before`.
Seven-pair comparisons against that image passed all checksums and checker gates:

| Workload | Median candidate/baseline | Pair range | Total setup ratio |
| --- | ---: | ---: | ---: |
| arithmetic | 0.83496 | 0.81809-0.85699 | 0.99491 |
| branches | 0.85936 | 0.83999-0.87411 | 1.00385 |
| calls | 0.94873 | 0.90982-0.98592 | 0.99891 |

Reports started at 11:38:33, 11:40:51 and 11:42:27 UTC. Reproduce with the existing
paired driver, `--workload arithmetic`, `branches` or `calls`, IP09's baseline image,
and the checker (`--target arithmetic=0.98`). These are local measured improvements
in complete workloads, not isolated poll costs or a general speed guarantee.
Image size fell from 26,843,136 to 26,782,208 bytes (0.99773); SHA-256:
`a350af02b023ad7795db07ed573cb96ad6361f1d2f7ce8d6c47207e4e8663ca9`.
No storage, allocation or preparation model changed.

The selected common subscription test and both VM cancellation/effect/cleanup tests
passed. Required structure, format, workspace all-target Clippy and diff checks
passed. IP09 evidence still covers unaffected admission, roots/reentry and reload;
complete CI and integrated IP12 acceptance remain outstanding.
