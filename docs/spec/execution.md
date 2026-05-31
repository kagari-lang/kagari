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

1. parse source
2. perform semantic analysis and typing
3. lower to a typed IR
4. lower to bytecode
5. execute bytecode in a VM

This makes the bytecode VM the main semantic backend.

The important point is that bytecode is not just a cache format.
It is the first real execution target and the place where language behavior is made concrete.

## Why Bytecode-First Fits Kagari

Kagari is not a minimal native-only systems language.
The language model emphasizes:

- embeddability
- host interop
- reflection
- security capabilities
- hot reload

These features all benefit from a stable runtime and VM layer.

Native AOT and JIT are backend layers; they do not define language semantics.

## Implementation Status

The repository contains bytecode-first implementation components:

- `crates/kagari-ir/src/bytecode/mod.rs`
- `crates/kagari-ir/src/module/mod.rs`
- `crates/kagari-vm/src/lib.rs`
- `crates/kagari-runtime/src/backend.rs`
- `crates/kagari-jit-cranelift/src/lib.rs`

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
- integrate capability checks
- integrate reflection and type metadata
- integrate hot reload and module epochs

This is the most important tier for correctness.

### Tier 1: Baseline JIT

The first JIT tier is a baseline function compiler.

Responsibilities:

- compile hot functions from typed IR or bytecode into machine code
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

## Bytecode as the Main Semantic Contract

Bytecode is the primary execution contract between the frontend and runtime.

This means:

- interpreter behavior is defined against bytecode semantics
- JIT compilation preserves bytecode-visible behavior
- runtime metadata attaches to modules, functions, and instructions

Bytecode is not a disposable intermediate artifact.

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
- capability checks
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
- may trigger capability checks
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

Workflow:

1. interpret bytecode normally
2. count function executions or hotness
3. identify hot functions
4. compile hot functions to machine code
5. redirect future calls through a function entry table

This keeps the design understandable and avoids tracing complexity.

## JIT Backend Style

A baseline JIT has these properties:

- direct lowering from typed IR or bytecode IR
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
- capability checks
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
- function-level code cache
- baseline JIT backend

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
