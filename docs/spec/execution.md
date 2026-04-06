# Kagari Execution Model Draft

This document describes a proposed execution strategy for Kagari.
It is a design draft, not a finalized implementation contract.

The main goal is to define a practical execution pipeline that fits Kagari's current direction:

- strongly typed scripting
- GC-backed runtime
- host interop
- reflection
- hot reload
- a bytecode-first implementation strategy

Backend abstraction direction is drafted separately in [codegen-backend.md](/Users/mikai/CLionProjects/kagari/docs/spec/codegen-backend.md).

## Design Goals

- make bytecode interpretation the primary semantic execution model
- support precompiled bytecode artifacts for faster loading and distribution
- leave a clean architectural path for later JIT compilation
- avoid coupling the execution strategy directly to AST structures
- keep runtime services shared across interpreter and future JIT backends

## Recommended Execution Strategy

The recommended strategy is:

1. parse source
2. perform semantic analysis and typing
3. lower to a typed IR
4. lower to bytecode
5. execute bytecode in a VM

This makes the bytecode VM the main semantic backend.

The important point is that bytecode is not just a cache format.
It is the first real execution target and the place where language behavior should be made concrete.

## Why Bytecode-First Fits Kagari

Kagari is not currently being shaped as a minimal native-only systems language.
Its design already emphasizes:

- embeddability
- host interop
- reflection
- security capabilities
- hot reload

These features all benefit from a stable runtime and VM layer.

If native AOT or JIT is made primary too early, the project is forced to solve too many backend-specific problems before the core language model is stable.

## Current Project Direction

The current repository already points toward a bytecode-first path:

- [bytecode.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-ir/src/bytecode.rs)
- [module.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-ir/src/module.rs)
- [lib.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-vm/src/lib.rs)

This draft recommends continuing in that direction.

## Execution Tiers

Kagari should be designed with multiple execution tiers in mind:

### Tier 0: Interpreter

The interpreter is the primary execution engine in the early versions.

Responsibilities:

- define the concrete semantics of bytecode
- provide the first correct implementation of call frames
- integrate host interop and borrow guards
- integrate capability checks
- integrate reflection and type metadata
- integrate hot reload and module epochs

This is the most important tier for correctness.

### Tier 1: Baseline JIT

If JIT is added later, the first JIT tier should be a baseline function compiler.

Responsibilities:

- compile hot functions from typed IR or bytecode into machine code
- preserve interpreter semantics
- reduce interpreter dispatch overhead
- continue using shared runtime helpers for complex operations

This tier should avoid speculative optimization at first.

### Tier 2: Optimizing JIT

An optimizing JIT may be added later if real workloads justify it.

Potential responsibilities:

- inlining
- specialization
- improved register allocation
- reduced helper calls
- guarded fast paths

This tier should be considered optional and future-facing.

## Bytecode as the Main Semantic Contract

Bytecode should be treated as the primary execution contract between the frontend and runtime.

This means:

- interpreter behavior should be defined against bytecode semantics
- future JIT compilation should preserve bytecode-visible behavior
- runtime metadata should attach naturally to modules, functions, and instructions

This is preferable to treating bytecode as a disposable intermediate artifact.

## Bytecode Artifact Format

Kagari's `.kbc` format should be treated as a precompiled bytecode artifact, not as native code.

Recommended use cases:

- faster startup than source recompilation
- module caching
- host distribution of script packages
- signing or integrity validation
- hot-reload comparisons

This matches the naming already documented in [README.md](/Users/mikai/CLionProjects/kagari/README.md).

## AOT in the Near Term

The first practical form of AOT for Kagari should be:

- ahead-of-time compilation from source to bytecode artifact

That means:

- source AOT to `.kbc`
- not native-code AOT as the primary path

This gives the project many of the practical benefits of AOT without prematurely committing to a native backend architecture.

## Native AOT

Native-code AOT may still make sense later for selected deployment targets.

However, it should not currently be the primary execution strategy.

Reasons:

- hot reload is harder
- host interop and borrow boundaries are harder to evolve
- reflection and dynamic metadata become more backend-sensitive
- development iteration slows down

Native AOT should be treated as a later backend experiment, not as the initial execution foundation.

## Why JIT Should Not Be Front-Loaded

JIT is not primarily blocked by code generation.
It is blocked by semantic stabilization.

Before JIT becomes worthwhile, Kagari needs:

- a stable calling convention
- a stable value model
- a stable runtime helper ABI
- a stable GC and safepoint model
- a stable host interop boundary
- a stable module epoch and invalidation story

Until those pieces exist, JIT adds complexity faster than it adds value.

## Design Hooks for Future JIT

Even though JIT should not be front-loaded, the current architecture should leave room for it.

The most important hooks are:

- typed IR that is independent from AST shape
- bytecode or IR with stable function and module identifiers
- explicit runtime helper calls for complex operations
- explicit function metadata
- explicit safepoint-aware call boundaries
- epoch-aware module and function invalidation

These hooks let a later JIT reuse the same runtime model rather than forcing a redesign.

## Runtime Helper ABI

Operations that are difficult, effectful, or security-sensitive should go through runtime helpers rather than being special-cased in only one backend.

Examples:

- allocation
- GC write barriers
- host calls
- capability checks
- reflection access
- downcast checks
- dynamic trait dispatch helpers when needed

This is important because both the interpreter and a future JIT should share the same semantic authority.

## Function Metadata Needed for JIT

The runtime and IR layers should be prepared to record function metadata such as:

- function id
- module id
- module epoch
- local layout
- parameter layout
- return convention
- effect flags
- safepoint metadata

This metadata is useful even before JIT exists.

## Instruction Effect Classification

Instructions or IR operations should eventually be classifiable by effect.

Examples of useful flags:

- may allocate
- may trap
- may call host
- may trigger capability checks
- may suspend
- may become a safepoint

This classification is valuable for:

- interpreter bookkeeping
- verifier logic
- future JIT lowering
- later optimization passes

## Baseline JIT Strategy

If JIT is later added, the recommended first step is:

- function-level baseline JIT

Recommended workflow:

1. interpret bytecode normally
2. count function executions or hotness
3. identify hot functions
4. compile hot functions to machine code
5. redirect future calls through a function entry table

This keeps the design understandable and avoids tracing complexity.

## Recommended JIT Backend Style

For Kagari's likely needs, a baseline JIT should look like:

- direct lowering from typed IR or bytecode IR
- minimal speculation
- no mandatory deoptimization support in the first version
- heavy reuse of runtime helpers

This is not the most aggressive design, but it is the most practical one.

## Cranelift-Like Backend Direction

If Kagari later adopts a Rust-friendly JIT backend library, a Cranelift-like approach is a practical fit.

Why this style is suitable:

- faster implementation than hand-written machine code emission
- cross-platform realism
- good fit for function-level code generation
- enough control to integrate runtime helper calls

This is a strategic direction, not a tool commitment.

## GC and Safepoints

JIT design must reserve space for GC integration even if GC is simple at first.

That means planning for:

- safepoints
- root maps or stack maps
- call boundary metadata

Otherwise, a later JIT will become tightly coupled to a too-simple early GC design.

Even if the first interpreter does not fully exploit this metadata, the architecture should allow it to exist.

## Host Interop and JIT

Future machine code must not bypass the host interop safety model.

In particular, JIT code must still respect:

- frame-scoped host borrows
- host call guards
- borrow kind checks
- capability checks
- no-escape invariants

This means JIT code should usually call shared runtime helpers at these boundaries unless a future proof allows safe specialization.

Host interop direction is drafted in [host-interop.md](/Users/mikai/CLionProjects/kagari/docs/spec/host-interop.md).

## Hot Reload and JIT

Hot reload means compiled code cannot be treated as permanently valid.

A practical model is:

- code cache entries are keyed by module id, epoch, and function id
- function entry points are indirected through a table
- reloading a module invalidates or replaces affected entries

This is much easier than trying to patch every call site directly in the first version.

## Deoptimization

The first JIT tier should avoid requiring deoptimization.

This means:

- no heavy speculative assumptions
- no aggressive type specialization that requires rollback
- no dependence on tracing-JIT behavior

Deoptimization can be introduced later if and when an optimizing JIT exists.

## Recommended v1 Execution Stack

The recommended first practical execution stack is:

- source frontend
- typed IR
- bytecode lowering
- interpreter
- `.kbc` bytecode artifacts for caching and distribution

This is enough to validate the language and runtime design without prematurely paying JIT complexity costs.

## Recommended v2 Execution Extensions

When the runtime and IR have stabilized, likely next steps are:

- richer bytecode metadata
- interpreter profiling counters
- function-level code cache
- baseline JIT backend

Only after that should the project consider:

- speculative specialization
- inlining-heavy optimizing JIT
- native AOT experiments

## Recommended Implementation Order

If implemented incrementally, the recommended order is:

1. strengthen typed IR and bytecode structure
2. define VM call frames and helper ABI clearly
3. define `.kbc` artifact boundaries
4. add module and function identifiers plus epochs
5. add instruction effect metadata
6. add profiling counters
7. add baseline JIT as an optional backend

This order keeps the interpreter as the semantic foundation while leaving a credible path toward JIT later.
