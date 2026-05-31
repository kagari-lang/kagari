# Kagari Codegen Backend

This document specifies the backend abstraction model for Kagari.

The goal is to let Kagari adopt machine-code backends such as Cranelift without coupling the language, runtime, or typed IR directly to one backend implementation.

## Design Goals

- keep frontend and semantic analysis independent from backend choice
- keep Kagari IR independent from backend-specific IR structures
- isolate machine-code generation in replaceable backend implementations
- share one runtime ABI across interpreter and codegen backends
- make it feasible to add or swap backends later without rewriting the language stack

## Core Principle

Kagari treats code generation as a backend behind a stable internal interface.

The stack is:

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

This boundary is mandatory.

## Architecture Layers

The architecture is split into four layers:

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

This layer must not know about Cranelift, LLVM, or any other backend library.

## Layer 2: Kagari IR

Kagari IR captures:

- typed operations
- function structure
- module structure
- effect information
- type metadata references
- helper-call boundaries

It must not contain:

- backend-specific SSA nodes
- backend-specific register abstractions
- backend-specific block builders
- backend-specific calling-convention objects

Backend-specific concepts are confined to backend implementations.

## Layer 3: Runtime ABI

The runtime ABI is the contract between generated code and the Kagari runtime.

It defines:

- how parameters are passed
- how return values are represented
- how locals and temporaries are lowered
- how runtime helpers are called
- how host calls are entered
- how safepoints are represented
- how errors or traps are surfaced

This ABI is backend-independent.

That means:

- Cranelift code calls the same helpers the interpreter logically depends on
- a future LLVM backend would target the same helper surface

## Layer 4: Concrete Backend

This is the only layer allowed to know about a specific codegen framework.

Examples:

- `CraneliftBackend`
- `LlvmBackend`
- experimental backend

Each backend is responsible for lowering Kagari IR plus runtime ABI calls into its own internal representation.

## Stable Kagari Concepts

The following are Kagari-owned concepts:

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

## Backend Interface

The backend interface has this semantic shape:

```text
CodegenBackend {
  compile_function(ir_fn, abi, target) -> CompiledFunction
  compile_module(ir_mod, abi, target) -> CompiledModule
}
```

Supporting concepts:

```text
BackendTarget
CompiledFunction
CompiledModule
CodeBlob
RelocationInfo
TrapTable
SafepointTable
```

Names are implementation details.
The interface is framed in Kagari terms rather than in a backend library's native API.

The current Rust interface lives in `crates/kagari-runtime/src/backend.rs`.
It exposes `BackendId`, `BackendTarget`, `BackendFunctionInput`, `ExecutableFunctionArtifact`, safepoint and debug metadata, backend diagnostics, and `CodegenBackend`.
The implemented trait compiles one function at a time and optionally invokes a compiled artifact:

```text
CodegenBackend {
  backend_id() -> BackendId
  target() -> BackendTarget
  compile_function(input) -> ExecutableFunctionArtifact
  invoke_function(artifact, runtime) -> Value
}
```

Module-level compilation remains a valid future extension of the same boundary, not a requirement of the current trait.
`kagari-jit-cranelift` implements this boundary for the optional baseline Cranelift backend.

## Runtime ABI Surface

The runtime ABI exposes helpers for operations that are:

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

This keeps codegen backends focused on lowering, not on reimplementing runtime semantics.

## Value Representation Boundary

Value representation is defined once by Kagari, then lowered by each backend.

Examples of representation questions:

- how small integers are represented
- whether aggregates are boxed or unboxed
- how interface values are represented
- how host borrow handles are passed

The backend consumes these decisions; it does not own them.

Otherwise backend choice starts to dictate language semantics.

## Safepoint and Stack Map Boundary

Safepoint strategy is a Kagari-level concern.

The backend receives:

- where safepoints must exist
- what values are live across them
- what stack-map information needs to be emitted

This is especially important for:

- GC
- host interop
- hot reload invalidation
- future suspension support

## Effect Metadata

Kagari IR classifies operations by effect.

Effect categories include:

- may allocate
- may trap
- may call host
- may trigger capability checks
- may suspend
- may require safepoint metadata

Backends consume these flags during lowering.

This keeps backend implementations simpler and keeps semantic effect knowledge in the Kagari-owned layer.

## Cranelift Boundary

When Cranelift is used as a backend, this separation means:

- only the Cranelift backend crate or module knows about Cranelift IR builders and contexts
- Kagari IR does not become Cranelift-shaped
- runtime helper conventions stay reusable
- a later backend does not require frontend or runtime redesign

Cranelift is an implementation detail of one backend, not the definition of Kagari execution.

## What Not to Do

The following are invalid architecture choices:

- storing Cranelift value or block ids in Kagari IR nodes
- exposing Cranelift type objects in runtime ABI definitions
- passing Cranelift contexts through frontend or middle-end APIs
- designing Kagari IR solely around one backend's conveniences
- baking backend register or SSA assumptions into language semantics

These decisions violate the backend boundary.

## Backend-Specific Responsibilities

The following remain inside a specific backend implementation:

- backend IR construction
- target ISA selection
- register allocation specifics
- machine code emission
- relocation handling details
- JIT memory setup details
- object emission details

These are expected to differ across backends.

## Cranelift Backend

Cranelift satisfies the requirements for a first machine-code backend:

- it is practical for baseline JIT work
- it avoids hand-writing multi-platform machine-code emitters
- it is reasonably aligned with Rust-based implementation work

It is treated as:

- a concrete backend
- not the backend abstraction itself

## Adding Another Backend Later

Adding another backend requires:

- writing a new lowering from Kagari IR to the new backend IR
- implementing the same runtime ABI surface
- producing the same metadata outputs needed by the runtime

It must not require:

- rewriting parsing
- rewriting semantic analysis
- redesigning host interop
- redesigning reflection
- redesigning the type registry

## Expected Cost of Switching Backends

Even with good abstraction, switching or adding a backend is not free.

The cost is concentrated in:

- codegen lowering
- backend-specific metadata emission
- backend-specific target support

If the cost spreads into:

- frontend data structures
- runtime value semantics
- security model
- host interop semantics

then the abstraction boundary has been drawn too low.

## Implementation Order

The incremental implementation order is:

1. stabilize Kagari-owned IR and function metadata
2. define runtime helper ABI
3. define backend-neutral compiled-code interfaces
4. implement the interpreter against the same semantic model
5. add a first machine-code backend
6. only later consider additional backends

This order keeps backend experimentation from destabilizing the rest of the language implementation.

## Relationship to Other Specifications

This document complements:

- [execution.md](execution.md)
- [runtime.md](runtime.md)
- [host-interop.md](host-interop.md)

Those documents define the execution model, runtime model, and host boundary.
This document defines how code generation backends plug into that larger architecture.
