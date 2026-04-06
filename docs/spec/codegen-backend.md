# Kagari Codegen Backend Draft

This document describes a proposed backend abstraction model for Kagari.
It is a design draft, not a finalized implementation contract.

The goal is to let Kagari adopt a first machine-code backend such as Cranelift without coupling the language, runtime, or typed IR directly to one backend implementation.

## Design Goals

- keep frontend and semantic analysis independent from backend choice
- keep Kagari IR independent from backend-specific IR structures
- isolate machine-code generation in replaceable backend implementations
- share one runtime ABI across interpreter and codegen backends
- make it feasible to add or swap backends later without rewriting the language stack

## Core Principle

Kagari should treat code generation as a backend behind a stable internal interface.

The stack should look roughly like:

```text
source
-> AST
-> typed semantics
-> Kagari IR
-> backend abstraction
-> concrete backend
-> machine code or object code
```

The important rule is:

- Kagari IR belongs to Kagari
- backend IR belongs to the backend

This boundary should stay sharp.

## Recommended Layers

The architecture should be split into four layers:

1. language frontend
2. Kagari IR and metadata
3. runtime ABI
4. concrete codegen backend

### Layer 1: Language Frontend

This includes:

- parsing
- name resolution
- type checking
- trait analysis
- reflection and security validation

This layer should know nothing about Cranelift, LLVM, or any other backend library.

## Layer 2: Kagari IR

Kagari IR should capture:

- typed operations
- function structure
- module structure
- effect information
- type metadata references
- helper-call boundaries

It should not contain:

- backend-specific SSA nodes
- backend-specific register abstractions
- backend-specific block builders
- backend-specific calling-convention objects

If those leak in, changing backends later becomes expensive.

## Layer 3: Runtime ABI

The runtime ABI is the contract between generated code and the Kagari runtime.

It should define:

- how parameters are passed
- how return values are represented
- how locals and temporaries are lowered
- how runtime helpers are called
- how host calls are entered
- how safepoints are represented
- how errors or traps are surfaced

This ABI should be backend-independent.

That means:

- Cranelift code calls the same helpers the interpreter logically depends on
- a future LLVM backend would target the same helper surface

## Layer 4: Concrete Backend

This is the only place that should know about a specific codegen framework.

Examples:

- `CraneliftBackend`
- `LlvmBackend`
- future experimental backend

Each backend is responsible for lowering Kagari IR plus runtime ABI calls into its own internal representation.

## What Should Be Stable Before Choosing a Backend

The following should be treated as stable Kagari-owned concepts:

- `KagariIrModule`
- `KagariIrFunction`
- `FunctionId`
- `ModuleId`
- `ModuleEpoch`
- `ValueRepr`
- `RuntimeAbi`
- `EffectFlags`
- `SafepointKind`

These are Kagari concepts, not Cranelift concepts.

## Suggested Backend Interface

A useful starting abstraction is something like:

```text
CodegenBackend {
  compile_function(ir_fn, abi, target) -> CompiledFunction
  compile_module(ir_mod, abi, target) -> CompiledModule
}
```

Possible supporting concepts:

```text
BackendTarget
CompiledFunction
CompiledModule
CodeBlob
RelocationInfo
TrapTable
SafepointTable
```

The exact names do not matter.
What matters is that the interface is framed in Kagari terms rather than in a backend library's native API.

## Suggested Runtime ABI Surface

The runtime ABI should expose helpers for operations that are:

- effectful
- security-sensitive
- hard to inline safely
- shared between interpreter and JIT

Typical helpers include:

- allocation
- write barriers
- host calls
- capability checks
- reflection access
- downcast checks
- dynamic dispatch support
- error and trap construction

This allows the codegen backend to stay focused on lowering, not on reimplementing runtime semantics.

## Value Representation Boundary

Value representation should be defined once by Kagari, then lowered by each backend.

Examples of representation questions:

- how small integers are represented
- whether aggregates are boxed or unboxed
- how dynamic trait objects are represented
- how host borrow handles are passed

The backend should consume these decisions, not own them.

Otherwise backend choice starts to dictate language semantics.

## Safepoint and Stack Map Boundary

Safepoint strategy should also be a Kagari-level concern.

The backend should receive:

- where safepoints must exist
- what values are live across them
- what stack-map information needs to be emitted

This is especially important for:

- GC
- host interop
- hot reload invalidation
- future suspension support

## Effect Metadata

Kagari IR should eventually classify operations by effect.

Useful categories include:

- may allocate
- may trap
- may call host
- may trigger capability checks
- may suspend
- may require safepoint metadata

Backends should consume these flags during lowering.

This keeps backend implementations simpler and keeps semantic effect knowledge in the Kagari-owned layer.

## Why This Helps with Cranelift

If Cranelift is the first backend, this separation means:

- only the Cranelift backend crate or module knows about Cranelift IR builders and contexts
- Kagari IR does not become Cranelift-shaped
- runtime helper conventions stay reusable
- a later backend does not require frontend or runtime redesign

This is the right way to use Cranelift: as an implementation detail of one backend, not as the definition of Kagari execution.

## What Not to Do

Avoid these mistakes:

- storing Cranelift value or block ids in Kagari IR nodes
- exposing Cranelift type objects in runtime ABI definitions
- passing Cranelift contexts through frontend or middle-end APIs
- designing Kagari IR solely around one backend's conveniences
- baking backend register or SSA assumptions into language semantics

These decisions make backend replacement far more expensive later.

## What Can Safely Be Backend-Specific

The following can remain inside a specific backend implementation:

- backend IR construction
- target ISA selection
- register allocation specifics
- machine code emission
- relocation handling details
- JIT memory setup details
- object emission details

These are expected to differ across backends.

## Cranelift as a First Backend

Cranelift is a good candidate for a first machine-code backend because:

- it is practical for baseline JIT work
- it avoids hand-writing multi-platform machine-code emitters
- it is reasonably aligned with Rust-based implementation work

But it should still be treated as:

- the first backend
- not the backend abstraction itself

## Adding Another Backend Later

If the architecture is clean, adding another backend should mostly require:

- writing a new lowering from Kagari IR to the new backend IR
- implementing the same runtime ABI surface
- producing the same metadata outputs needed by the runtime

It should not require:

- rewriting parsing
- rewriting semantic analysis
- redesigning host interop
- redesigning reflection
- redesigning the type registry

## Expected Cost of Switching Backends

Even with good abstraction, switching or adding a backend is not free.

The cost should be concentrated in:

- codegen lowering
- backend-specific metadata emission
- backend-specific target support

If the cost instead spreads into:

- frontend data structures
- runtime value semantics
- security model
- host interop semantics

then the abstraction boundary was probably drawn too low.

## Recommended Implementation Order

If implemented incrementally, the recommended order is:

1. stabilize Kagari-owned IR and function metadata
2. define runtime helper ABI
3. define backend-neutral compiled-code interfaces
4. implement the interpreter against the same semantic model
5. add a first machine-code backend
6. only later consider additional backends

This order keeps backend experimentation from destabilizing the rest of the language implementation.

## Relationship to Other Drafts

This document complements:

- [execution.md](/Users/mikai/CLionProjects/kagari/docs/spec/execution.md)
- [runtime.md](/Users/mikai/CLionProjects/kagari/docs/spec/runtime.md)
- [host-interop.md](/Users/mikai/CLionProjects/kagari/docs/spec/host-interop.md)

Those documents define the execution model, runtime model, and host boundary.
This document focuses specifically on how code generation backends should plug into that larger architecture.
