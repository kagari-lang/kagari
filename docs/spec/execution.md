# Kagari Execution Model

This document specifies the execution strategy for Kagari.

The execution pipeline supports:

- strongly typed scripting
- GC-backed runtime
- host interop
- reflection
- hot reload
- a bytecode-first implementation strategy

Backend abstraction rules are defined in [codegen-backend.md](codegen-backend.md).
Bytecode rules are defined in [bytecode.md](bytecode.md).
Module execution rules are defined in [modules.md](modules.md).

## Design Goals

- make bytecode interpretation the primary semantic execution model
- support precompiled bytecode artifacts for faster loading and distribution
- support optional JIT compilation without making it the semantic authority
- avoid coupling the execution strategy directly to AST structures
- keep runtime services shared across interpreter and optional JIT backends

## Execution Strategy

The execution strategy is:

1. parse source and prepare the installed standard declaration package
2. resolve and check ordinary HIR declarations and callable applications
3. lower checked facts to concrete, verified MIR
4. lower verified MIR to bytecode and portable executable contracts
5. validate and link the complete program, then execute bytecode in a VM

This makes the bytecode VM the main semantic backend.

Bytecode is a verified, distributable execution target, with observable behavior
defined by the common executable contract below.

## Native standard calls

Engine and host native calls retain distinct provider contracts. Each engine import
carries its selected declaration, concrete signature, substitutions, bounds and
protocol witnesses; validation and execution need no source-analysis dependency.
Required methods select a checked script/native/host implementation before execution.
No public standard algorithm is expanded into a second compiler implementation.

Direct native calls share existing Rust helpers. Resumable calls retain explicit
roots and continuations on the active execution frames, release dynamic borrows
before callbacks, and preserve each original logical operation's charge. Native,
script and nested native callbacks share cancellation, call-depth and allocation
limits. Iterator captures retain the checked import and defining dependency versions.
Core iterator/range operations use the same typed primitives as native entrypoints.

Storage mutations preserve their commit point: preparation failures publish no
partial update; an already committed update remains visible if a later charge,
callback or return-value publication fails. Unit-returning required mutations charge
storage work and Unit publication separately. Cleanup releases only the current
session/frame suffix and its guards, leaving completed side effects intact. JIT
unsupported decisions fall back before entry and never restart partially executed code.

## Why Bytecode-First Fits Kagari

Kagari is not a minimal native-only systems language.
The language model emphasizes:

- embeddability
- host interop
- reflection
- cancellation and execution phase
- hot reload

These features all benefit from a stable runtime and VM layer.

Native AOT and JIT are backend layers; they do not define language semantics.

## Implementation Status

The repository contains bytecode-first implementation components:

- `crates/kagari-bytecode/src/lib.rs`
- `crates/kagari-mir/src/lib.rs`
- `crates/kagari-vm/src/lib.rs`
- `crates/kagari-runtime/src/backend.rs`
- `crates/kagari-codegen-cranelift/src/lib.rs`

These components are part of the bytecode-first execution model.
The Cranelift backend is optional and must preserve interpreter-visible behavior through the `CodegenBackend` boundary.

## Execution Tiers

Kagari has multiple execution tiers:

### Tier 0: Interpreter

The interpreter is the primary execution engine.

Responsibilities:

- define the concrete semantics of bytecode
- provide the first correct implementation of call frames
- integrate host interop and borrow guards
- integrate declared interface checks
- integrate reflection and type metadata
- integrate hot reload and module epochs

This is the most important tier for correctness.

### Tier 1: Baseline JIT

The first JIT tier is a baseline function compiler.

Responsibilities:

- compile eligible functions from verified MIR and explicit ABI/link descriptions
- preserve interpreter semantics
- reduce interpreter dispatch overhead
- continue using shared runtime helpers for complex operations

This tier avoids speculative optimization.

### Tier 2: Optimizing JIT

An optimizing JIT is an optional backend tier for workloads that justify it.

Responsibilities:

- inlining
- specialization
- improved register allocation
- reduced helper calls
- guarded fast paths

This tier is optional.

## Common executable contract

Verified MIR and carried ABI facts define the shared executable contract. Bytecode
is the interpreter target; portable MIR is independently reverified for native
preparation and must reproduce the same complete bytecode program canonically.

This means:

- interpreter behavior follows the verified bytecode operations and metadata
- JIT compilation preserves the same effects, charges, traps and value contracts
- runtime metadata attaches to modules, functions, and program points
- source analysis is absent from artifact loading and native preparation

Bytecode remains a distributable, verified execution product.

## Bytecode Artifact Format

Kagari's `.kbc` format is a precompiled bytecode artifact, not native code.

Use cases:

- faster startup than source recompilation
- module caching
- host distribution of script packages
- signing or integrity validation
- hot-reload comparisons

This matches the naming already documented in [README.md](../../README.md).

## AOT in the Near Term

The first form of AOT for Kagari is:

- ahead-of-time compilation from source to bytecode artifact

That means:

- source AOT to `.kbc`
- not native-code AOT as the primary path

This provides AOT loading and distribution benefits without making native code the semantic foundation.

## Native AOT

Native-code AOT is a backend option for selected deployment targets.

It is not the primary execution strategy.

Reasons:

- hot reload is harder
- host interop and borrow boundaries are harder to evolve
- reflection and dynamic metadata become more backend-sensitive
- development iteration slows down

Native AOT is a backend, not the initial execution foundation.

## JIT Preconditions

JIT is not primarily blocked by code generation.
It is blocked by semantic stabilization.

JIT depends on:

- a stable calling convention
- a stable value model
- a stable runtime helper ABI
- a stable GC and safepoint model
- a stable host interop boundary
- a stable module epoch and invalidation story

Until those pieces exist, JIT is implementation work without semantic authority.

## JIT Integration Hooks

JIT backends reuse the same runtime model through these hooks:

- typed IR that is independent from AST shape
- bytecode or IR with stable function and module identifiers
- explicit runtime helper calls for complex operations
- explicit function metadata
- explicit safepoint-aware call boundaries
- epoch-aware module and function invalidation

These hooks keep JIT backends from changing language semantics.

## Runtime Helper ABI

Operations that are difficult, effectful, or security-sensitive go through runtime helpers rather than being special-cased in only one backend.

Examples:

- allocation
- GC write barriers
- host calls
- typed host path access and mutation
- declared interface checks
- reflection access
- downcast checks
- interface dispatch helpers when needed

The interpreter and JIT backends share the same semantic authority.

## Function Metadata for JIT

The runtime and IR layers record function metadata such as:

- function id
- module id
- module epoch
- local layout
- parameter layout
- return convention
- effect flags
- safepoint metadata

This metadata also supports interpreter diagnostics, verification, and runtime bookkeeping.

## Instruction Effect Classification

Instructions and IR operations are classifiable by effect.

Effect flags:

- may allocate
- may trap
- may call host
- may trigger declared interface checks
- may suspend
- may become a safepoint

This classification is valuable for:

- interpreter bookkeeping
- verifier logic
- JIT lowering
- later optimization passes

## Baseline JIT Strategy

The first JIT step is:

- function-level baseline JIT

The implemented workflow explicitly prepares eligible functions through the SDK,
installs native products into a runtime and executes a prepared entry. The interpreter
handles unsupported cases before native entry. Execution counts/hotness, automatic
first-call preparation and entry-table tier switching are future scheduling work;
the current API does not implement those policies automatically.

## JIT Backend Style

A baseline JIT has these properties:

- direct lowering from verified MIR and explicit ABI/link descriptions
- minimal speculation
- no mandatory deoptimization support in the baseline tier
- heavy reuse of runtime helpers

This keeps the baseline JIT tier aligned with interpreter semantics.

## Cranelift-Like Backends

A Rust-friendly function compiler such as Cranelift fits Kagari's baseline JIT backend requirements.

This backend style provides:

- faster implementation than hand-written machine code emission
- cross-platform realism
- good fit for function-level code generation
- enough control to integrate runtime helper calls

Cranelift-style code generation is a backend choice, not a language-semantic commitment.

## GC and Safepoints

JIT design reserves space for GC integration even when GC is simple.

That means the backend model includes:

- safepoints
- root maps or stack maps
- call boundary metadata

The architecture includes this metadata even when the interpreter does not fully exploit it.

## Host Interop and JIT

Machine code must not bypass the host interop safety model.

In particular, JIT code must still respect:

- frame-scoped host borrows
- host call guards
- borrow kind checks
- declared interface checks
- no-escape invariants

JIT code calls shared runtime helpers at these boundaries unless a specialization is proven to preserve the same safety checks.

Host interop rules are defined in [host-interop.md](host-interop.md).

## Hot Reload and JIT

Hot reload means compiled code cannot be treated as permanently valid.

The model is:

- code cache entries are keyed by module id, epoch, and function id
- function entry points are indirected through a table
- reloading a module invalidates or replaces affected entries

This avoids direct patching of every call site.

## Deoptimization

The first JIT tier avoids requiring deoptimization.

This means:

- no heavy speculative assumptions
- no aggressive type specialization that requires rollback
- no dependence on tracing-JIT behavior

Deoptimization belongs to an optimizing JIT tier.

## v1 Execution Stack

The first execution stack is:

- source frontend
- typed IR
- bytecode lowering
- interpreter
- `.kbc` bytecode artifacts for caching and distribution

This validates the language and runtime design without making JIT part of the semantic foundation.

## Future Work

Future execution extensions include:

- richer bytecode metadata
- interpreter profiling counters
- broader native coverage and scheduling over the existing function cache
- physical GC maps and observer callbacks for additional native functions

Later backend experiments include:

- speculative specialization
- inlining-heavy optimizing JIT
- native AOT experiments

## Implementation Order

The incremental implementation order is:

1. strengthen typed IR and bytecode structure
2. define VM call frames and helper ABI clearly
3. define `.kbc` artifact boundaries
4. add module and function identifiers plus epochs
5. add instruction effect metadata
6. add profiling counters
7. add baseline JIT as an optional backend

This order keeps the interpreter as the semantic foundation while preserving JIT as an optional backend path.

## Owned interpreter driving (AX01)

The VM exposes `start` and `drive` for verified script entries, including native-wait
resume bodies. Start validates and retains arguments without executing script. An `OwnedExecution` token contains
a checked session identity and retirement/cancellation notification state, not a
Runtime borrow. `drive` borrows the runtime for one exclusive activation and returns
`Runnable`, `Waiting` or `Complete(Result<RootedValue, VmError>)`. Its nonzero
instruction slice is a scheduling interval; native calls remain cooperative and a slice may overrun
until all transient resources have left. Synchronous execute/reentry uses the same
frame store and interpreter without host-visible slice exits.

Independent roots retain their own frames, cancellation, code pins and call-depth
state. Runtime current call depth is restored for the selected root and is zero
between activations; changing slices cannot bypass its limit. Runtime-owned operand
windows reuse retired identities with generation checks. Out-of-order frame cleanup
compacts their banks while preserving live window identities and roots.

Owned collection iteration leases stay in parked frames, preserving cursor progress
and structural-write exclusion. Mutation guards, live scoped host values, native
borrows and synchronous reentry prevent a slice exit. Drop requests retirement;
`drain_retired_executions`, the next owned start/drive or runtime destruction releases
the frames and leases. Host control wakers can be registered on the owner token.
Independent-root entry/drive during an activation is rejected; ordinary checked
synchronous reentry remains available and shares the active root.

Native and script Future waits use this same driver. Source `async`/`.await` is
implemented through checked cold factories and resume bodies. Host Task scopes
and shared waits use the same driver; generic script spawn remains AX04 work.

## Owned async execution draft (AX01-AX04)

Source async callables, Future/Task waits and host scope driving are implemented
below; generic script spawn remains AX04 work. The
[AX00 contracts](../async-execution-design.md#concrete-implementation-contracts-ax00)
and [execution plan](../async-execution-plan.md) define the handoff.

Runtime-owned execution records survive short driver activations. The driver
returns Runnable, Waiting or Complete, and completion endpoints only publish
readiness. Direct Future await drives once in the current execution;
Task await waits for a scope-owned execution. Owned iteration leases remain in parked frames,
so ordinary for bodies can await with existing structural-mutation exclusion.
Rust borrows, mutation guards and synchronous reentry cannot cross suspension.

### Owned readiness and cancellation notifications (AX02)

Owned execution readiness is a durable coalescing bit. Host wakers only request
scheduling; registering a waker rechecks existing readiness. Activation claims the
bit, a runnable slice republishes it, and terminal retirement/runtime destruction
invalidates it. The common cancellation token supports scoped weak wake
subscriptions. Owned execution registers such a subscription, so cancelling the
original host-supplied token wakes the control path as well as cancellation through
the owner handle. Registrations do not keep completed execution owners alive.

A bytecode Await stores the continuation destination in the session and advances
the PC once. A pending poll parks without republishing runnable readiness; an
already racing completion/cancellation wake remains durable. Subsequent driver
activations poll the existing operation, never claim the Future or submit again.
Successful conversion writes the checked destination and resumes at the saved PC.
Cancellation is checked before accepting or publishing the result.

Portable Await requires a Future or Task storage role, matching semantic input/output
slots and `EffectSet::may_suspend`. This flag marks a resume body, not a suspension
safety proof: the runtime also checks the complete active resource chain before
starting any await, including immediately completing operations. Ordinary calls
and ordinary closure construction cannot select a resume body; synchronous frame
entry and native backend installation reject it before entering its body. Owned
entry can drive it. MIR Await is lowered and encoded with the same type contract.
Both verifiers check definite initialization and live slots, including debug-visible
locals, and require matching iteration-stack depth at normal joins/backedges.
Return/trap still unwinds owned leases. Live host capabilities cannot cross an
await; owned iteration resources can. The bytecode verifier derives bounded,
non-serializable await liveness. Runtime preparation maps it through physical slot
reuse, preserving a location if any logical alias is live. Dead managed slots are
discarded before the runtime resource check so unused values cannot falsely prevent
suspension or retain heap objects. The foundation publishes `core::future::Future<T>`
and `core::task::Task<T>`/`TaskScope`,
and the SDK exposes this driver with or without source/native features. Each owned
execution has its own cancellation token. Cancelling the owner terminates only that
execution; the host-supplied context token forwards cancellation to all executions
started with it, without receiving local cancellation requests back.
These transitions do not add polling loops, thread preemption or cancellation of
another execution merely because it is being observed.
