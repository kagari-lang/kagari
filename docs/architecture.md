# Kagari Architecture

This document defines the production architecture for Kagari.
It describes the intended system shape that implementation work must converge on.
When existing code conflicts with the specifications, the specifications are authoritative.

## Architectural Principles

- Kagari is a statically typed, GC-backed scripting language for Rust-hosted applications.
- The source language is Rust-inspired in syntax and Kotlin-like in value ergonomics.
- Script authors do not work with Rust lifetimes, Rust borrowing, or script-level `dyn Trait`.
- Hot reload is a core runtime property, not a later patch over module loading.
- Host-owned state and Kagari-owned state remain explicit and separately controlled.
- Bytecode and typed IR are semantic boundaries shared by the interpreter and future machine-code backends.
- Cranelift JIT is an optional backend layer and never the definition of language semantics.

## Specification Authority

The implementation must be driven by the documents under `docs/spec/`.
The current Rust code is an implementation snapshot and may contain legacy behavior that no longer matches the specifications.

Examples of implementation behavior that must not be preserved for compatibility when it conflicts with spec:

- `let` / `let mut` as source binding syntax
- script-visible `static` or `static mut` module storage
- script-visible Rust-style `dyn Trait`
- long-lived host mutable references represented as script values
- ordinary field mutation lowered through reflection helpers
- runtime string field lookup in hot field-access paths

## Workspace Shape

The repository is a Rust workspace with structural separation between language phases:

```text
crates/
  kagari-common   shared source, span, diagnostic, and identifier infrastructure
  kagari-syntax   lexer, parser, concrete syntax tree, and AST views
  kagari-hir      lowering, name resolution, type checking, traits, and semantic tables
  kagari-ir       typed IR, bytecode, metadata tables, and backend-neutral lowering
  kagari-runtime  values, GC, module store, host registry, security context, and reload state
  kagari-vm       bytecode interpreter
  kagari-cli      command-line entry point and pipeline driver
```

Additional backend crates may be added when they make ownership and dependency boundaries clearer.
A Cranelift backend should live outside the frontend, HIR, and core runtime crates.

## Compilation Pipeline

The production pipeline is:

```text
package root / host source / bytecode artifact
  -> module loader
  -> source or verified .kbc artifact
  -> source
  -> tokens
  -> syntax tree / AST views
  -> HIR
  -> name resolution
  -> type checking and trait/interface validation
  -> typed IR
  -> verified bytecode
  -> interpreter execution
  -> optional baseline JIT execution
```

The parser owns source spelling and recovery.
HIR owns language meaning.
Typed IR owns normalized control flow and typed operations.
Bytecode owns the interpreter contract.
JIT backends consume typed IR or a verified bytecode-like lowered form without changing observable behavior.

Module loading is defined in `docs/spec/module-loading.md`.
Bytecode artifact boundaries are defined in `docs/spec/artifacts.md`.

## Source Language Layer

The syntax layer implements the grammar in `docs/spec/syntax.md` and `docs/kagari.ebnf`.

Core source-language facts:

- local bindings use `val` and `var`
- fields use `val field: T` and `var field: T`
- function parameters are ordinary non-rebindable bindings
- method receivers use `self`
- unit is written as `()`
- `const` is a compile-time value item
- script-visible `static` module storage is not part of the language surface
- trait names are interface value types directly; there is no script-level `dyn Trait`

The syntax layer must not encode semantic shortcuts that only exist because of the current implementation.

## Builtins and Standard Modules

Kagari has a small standard surface defined in `docs/spec/builtins.md`.

The builtin layer owns:

- primitive numeric, boolean, string, unit, tuple, array, `Option`, and `Result` types
- standard modules such as `std::debug`, `std::math`, `std::array`, `std::string`, `std::option`, and `std::result`
- iterable protocol support used by `for`
- builtin metadata for type checking, bytecode, reflection profiles, reload validation, and JIT lowering

Host-sensitive APIs such as file system, networking, timers, persistence, service registries, and logging sinks are host APIs.
They are not exposed as unrestricted core standard modules.

## HIR, Resolution, and Type System

HIR is the first semantic representation.
It should erase parser trivia and expose stable semantic nodes for later passes.

HIR and semantic analysis own:

- module item collection and visibility
- local, parameter, field, function, const, trait, impl, and module namespaces
- `val` / `var` writeability rules
- field writeability and assignment validation
- function signatures and `()` return behavior
- generic parameter and trait-bound checking
- interface value compatibility
- concrete type identity for `is<T>` and `downcast<T>`
- compile-time metadata and generated registration data

Type checking must reject invalid programs before IR lowering whenever the violation is statically knowable.
Runtime checks remain required for host state, capabilities, dynamic indexes, and hot reload epochs.

## Typed IR and Bytecode

Typed IR is the backend-neutral executable semantics.
Bytecode is the compact register/local format used by the interpreter.

The IR and bytecode layer owns:

- explicit control flow
- register/local operand flow
- aggregate construction and access
- direct script calls and host/runtime helper calls
- module initialization lowering
- typed path descriptors
- effect metadata
- safepoint and root metadata
- hot reload ABI fingerprints
- debug/source span metadata

Ordinary script aggregate access and host-backed typed path access are different operations.
Host-backed field/index mutation must lower to typed path operations or typed runtime helpers, not reflection-based string mutation.

## Runtime Model

The runtime owns execution state and services shared by the interpreter and JIT.

Core runtime subsystems:

- value representation
- GC heap for Kagari-owned values
- explicit roots for host-retained Kagari values
- module store with epochs
- function and bytecode artifact registry
- type and interface metadata registry
- host registry
- security context and capability state
- resource accounting
- hot reload coordinator

The GC manages Kagari script data.
It does not own Rust host objects, Rust references, or the Rust object graph.

## Host Interop and Typed Path Mutation

Host interop exposes Rust functionality through explicit registration.

The host boundary has two separate mechanisms:

- frame-scoped host borrow tokens for temporary host calls using `&T` or `&mut T`
- typed path mutation for ergonomic field/index access to host-owned domain state

Typed path mutation represents a checked path rooted at a host object.
It carries typed metadata, dynamic index operands, access policy, dirty tracking hooks, and reload validation data.
It must not store Rust `&mut` references in script values.

## Embedding API

The Rust embedding surface is defined in `docs/spec/embedding-api.md`.

The embedding API owns:

- compile, load, execute, and reload entry points
- host registry setup
- module loader configuration
- execution context construction
- runtime capability and resource policy
- structured diagnostics and runtime errors
- interpreter/JIT execution policy

Embedding APIs expose stable Kagari concepts rather than parser or backend internals.
Convenience CLI behavior must remain a thin layer over the same embedding pipeline.

## Interpreter

The interpreter is the semantic execution foundation.
It executes verified bytecode against the runtime model.

The interpreter must:

- preserve bytecode-visible control flow and value behavior
- enforce traps and runtime errors consistently
- call runtime helpers at allocation, host, reflection, security, and path boundaries
- maintain correct stack/root metadata for GC
- respect module initialization and hot reload epochs
- reject unsupported or unverified bytecode instead of guessing behavior

## Baseline Cranelift JIT

The baseline JIT is optional and function-level.
It compiles typed IR or verified bytecode-like IR to machine code through Cranelift.

The JIT must:

- preserve interpreter semantics
- share runtime helper ABI boundaries
- emit safepoint and stack-map metadata
- respect host interop and typed path mutation checks
- invalidate compiled artifacts on incompatible module epoch or ABI changes
- avoid mandatory deoptimization, tracing behavior, and optimizing-tier complexity

`docs/spec/jit.md` defines the JIT contract.

## Hot Reload

Hot reload is built around module epochs and validated publication.

Reload must:

- compile and validate a new module before publishing it
- preserve the current active module when validation fails
- compare public ABI fingerprints, type metadata, interface tables, and typed path descriptors
- keep old code and metadata reachable while old values or calls need them
- ensure new calls use the latest successfully published epoch
- avoid implicit migration of script-visible module storage

Script-visible durable module storage is deferred until the reload model defines explicit versioning and migration rules.

## Security and Reflection

Security is layered:

- language profile
- runtime capabilities
- host API exposure
- resource policy

Reflection is metadata-driven and profile-gated.
Ordinary game-state mutation must not go through reflection.
Privileged reflective writes, when provided by an embedding, are separate from typed path mutation and must be explicitly gated.

## Production Readiness Definition

Kagari is production-ready when:

- syntax, semantics, runtime, and bytecode match the specifications
- module loading, artifact validation, and embedding APIs are stable
- the builtin surface is implemented and tested
- incompatible legacy language forms have been removed
- conformance tests cover accepted and rejected source programs
- interpreter behavior is deterministic and verified through integration tests
- host interop enforces no-escape and aliasing rules
- typed path mutation is validated, efficient, and reload-aware
- hot reload cannot corrupt the active runtime on failure
- security profiles and reflection gates are enforced at runtime boundaries
- baseline Cranelift JIT can be enabled without changing language behavior
- documentation matches implementation and gives Codex agents an executable roadmap
