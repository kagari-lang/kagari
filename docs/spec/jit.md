# Kagari Baseline JIT Specification

This document defines the baseline JIT contract for Kagari.
The JIT is an optional execution backend.
It is not the semantic authority for the language.

## Design Goals

- provide a path to high-performance execution for hot functions
- preserve interpreter-visible behavior exactly
- reuse the same runtime helper ABI as the interpreter
- integrate with GC root maps, safepoints, host interop, security checks, and hot reload
- keep Cranelift isolated behind a backend boundary
- avoid optimizing-tier complexity in the production baseline

## Non-Goals

The baseline JIT does not provide:

- an optimizing tier
- tracing JIT behavior
- mandatory deoptimization
- speculative inlining
- backend-specific language semantics
- direct access to Rust host data that bypasses runtime policy
- a replacement for the bytecode verifier

## Semantic Authority

The interpreter and verified bytecode define observable execution behavior.
JIT code must produce the same results, traps, resource checks, host calls, path mutations, reflection gates, and hot reload behavior as the interpreter.

When the JIT cannot compile a function while preserving those rules, execution falls back to the interpreter.
Fallback is an implementation detail and must not be observable except through performance.

## Input Form

The baseline JIT consumes one of these forms:

- typed IR after semantic validation
- verified bytecode
- a bytecode-like lowered form produced from verified bytecode

The input must contain:

- function signature and local/register layout
- typed operand information
- direct call targets where statically known
- runtime helper call boundaries
- effect metadata
- safepoint metadata
- stack map information or enough data to derive it
- module epoch and ABI fingerprint references
- typed path descriptors for host-backed path operations

Cranelift value, block, and type objects must not leak into frontend, HIR, typed IR, or runtime public types.

## Backend Boundary

A Cranelift backend owns:

- Cranelift context and builder setup
- function lowering to Cranelift IR
- machine-code emission
- relocation and symbol resolution
- executable memory management
- compiled artifact registration
- stack map extraction
- backend-specific diagnostics

The core language crates own:

- source semantics
- typed IR and bytecode contracts
- runtime helper ABI
- value representation
- GC, host interop, security, and hot reload policy

## Runtime Helper ABI

JIT code calls shared runtime helpers for operations that may allocate, trap, inspect metadata, call host code, mutate host state, interact with reflection, touch security state, or require safepoint behavior.

Required helper categories:

- allocation
- array and aggregate helpers when not inlined safely
- type checks and downcasts
- interface dispatch
- direct and indirect script calls
- host function calls
- typed path read, set, modify, and view operations
- reflection metadata access
- resource accounting
- trap construction
- GC safepoints
- hot reload epoch checks

Helper signatures must be stable enough for generated code to call them through a backend-neutral ABI layer.
Rust implementation details may change behind that layer.

## Value Representation

The JIT uses the runtime value representation.
It may use backend-local unboxed temporaries only when the observable value model is preserved.

Generated code must be able to:

- materialize runtime `Value` instances at helper boundaries
- report live GC references at safepoints
- preserve interface value identity and concrete type identity
- preserve host handle identity without treating host objects as GC-owned
- reject or fall back for value forms the baseline backend cannot represent

## GC and Safepoints

Generated code must expose GC roots precisely enough for the runtime collector.

The baseline JIT must provide:

- safepoints at helper calls that may allocate or trigger collection
- stack maps for live GC values
- metadata for live interface values and path views when they contain GC-managed components
- no hidden host borrow state across safepoints that can suspend or escape a frame

If precise metadata is unavailable for a function, that function must run in the interpreter.

## Host Interop and Typed Path Mutation

JIT code must not bypass host policy.

Host calls and typed path operations must preserve:

- capability checks
- host API exposure checks
- frame-scoped borrow-token rules
- no-escape validation
- aliasing validation at the host boundary
- typed path descriptor validation
- dynamic index validation
- dirty tracking and host hooks
- reload epoch checks

Specialized typed path fast paths are allowed only when they are proven equivalent to the helper path and retain all required checks or guards.

## Hot Reload

Compiled JIT artifacts are tied to:

- module id
- module epoch
- function id
- public ABI fingerprint
- type layout fingerprint
- interface table fingerprint
- typed path descriptor fingerprint
- runtime helper ABI version

When a reload invalidates any of these dependencies, affected compiled artifacts must stop being used.
New calls use artifacts from the latest successfully published epoch.
Old calls may continue using old artifacts only while the owning epoch remains valid and reachable.

Failed reload validation must not replace active compiled artifacts.

## Security

Generated code executes under the same security context as interpreted code.

The JIT must preserve:

- language profile restrictions already encoded by frontend validation
- runtime capability checks
- host API exposure policy
- reflection gates
- resource limits
- trap behavior

A restricted profile may disable JIT execution entirely.

## Compilation Policy

Baseline JIT compilation is function-level.

The runtime may compile:

- all eligible functions when a module is loaded
- selected functions on first call
- selected functions after an execution-count threshold

The compilation policy is not language-visible.
It must not change program results or reload behavior.

## Fallback Rules

The JIT must fall back to the interpreter when:

- bytecode verification fails
- required type or effect metadata is missing
- stack maps cannot be emitted safely
- a runtime helper ABI is unavailable
- a function uses unsupported instructions
- host or security policy disables JIT
- the module epoch or ABI dependency is invalid

Fallback should be recorded in diagnostics or tracing when enabled.

## Baseline Feature Set

The production baseline includes:

- Cranelift backend isolated behind a backend trait or crate boundary
- function-level compilation
- integer, boolean, unit, tuple, array, and struct value support through runtime representation
- direct calls and runtime helper calls
- control flow, branches, loops, and returns
- GC safepoints and root metadata at helper boundaries
- typed path operations through helpers
- artifact invalidation by module epoch and ABI fingerprints
- interpreter fallback for unsupported functions

## Scope Exclusions

The baseline excludes:

- optimizing tier
- speculative type feedback
- deoptimization
- cross-function inlining
- tracing JIT
- machine-code persistence across process restarts
- backend-specific syntax or user controls

## Acceptance Criteria

The JIT feature is acceptable when:

- every compiled function has an interpreter equivalence test
- unsupported functions fall back cleanly
- compiled artifacts are invalidated by reload dependency changes
- GC root metadata is validated in tests
- typed path mutation through JIT calls the same policy surface as the interpreter
- host borrow tokens cannot escape through compiled frames
- disabling JIT through policy forces interpreter execution
- `cargo test --workspace` passes with JIT feature tests enabled
