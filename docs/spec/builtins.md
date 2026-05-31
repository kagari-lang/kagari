# Kagari Builtins and Standard Library Specification

This document defines the minimum builtin surface required for a production-ready Kagari runtime.
It describes language-level builtins and standard modules, not host application APIs.

## Design Goals

- keep the core standard surface small and predictable
- support game and business scripting without broad system APIs
- keep dangerous capabilities behind host APIs and security profiles
- make builtin types visible to type checking, bytecode, reflection metadata, and JIT lowering
- avoid hidden dependence on Rust standard-library concepts that do not exist in Kagari

## Core Builtin Types

The core type set includes:

- `()`
- `bool`
- signed integers: `i8`, `i16`, `i32`, `i64`, `isize`
- unsigned integers: `u8`, `u16`, `u32`, `u64`, `usize`
- floating-point numbers: `f32`, `f64`
- `String`
- arrays or vectors as `[T]`
- tuples
- user-defined structs and enums
- trait/interface value types
- `Option<T>`
- `Result<T, E>`

The numeric type names follow Rust spelling.
The semantics do not import Rust ownership or borrowing.

## Collection Types

The baseline collection surface includes:

- `[T]` dynamically sized array/vector values
- tuple values
- string values

Map/dictionary types are not part of the minimum builtin surface unless a host or standard module explicitly provides them.

Array operations include:

- length
- indexing
- append/push
- pop
- iteration

Array mutation follows the ordinary Kagari value model and security/resource rules.

## Option and Result

`Option<T>` and `Result<T, E>` are standard enum types.

They support:

- construction through variants
- pattern matching
- type checking as ordinary generic enums
- reflection metadata when the active profile exposes metadata

They are not magic control-flow constructs.

## String

`String` is a GC-managed script value.

Baseline string operations include:

- length
- equality
- concatenation
- basic slicing only if the runtime defines UTF-8 boundary behavior
- formatting through standard helper functions or host-provided formatting APIs

Locale-aware formatting and advanced text processing are not part of the core builtin requirement.

## Numeric Operations

Baseline numeric support includes:

- arithmetic
- comparison
- unary negation for signed numeric types
- explicit casts where the language defines them

Overflow behavior must be specified by the implementation mode:

- checked traps
- wrapping operations through explicit functions
- host-selected debug/release policy only if it is not observable as undefined behavior

The production baseline should prefer checked traps for ordinary arithmetic unless a later numeric spec defines a different rule.

## Boolean and Control Helpers

Boolean values support:

- `&&`
- `||`
- `!`
- equality

Short-circuit behavior is part of language semantics and must be preserved by bytecode and JIT backends.

## Builtin Modules

The standard module set is intentionally small.

Baseline modules:

```text
std::debug
std::math
std::array
std::string
std::option
std::result
```

Host-sensitive modules such as file system, networking, timers, database, logging sinks, and service registries are not core standard modules.
They are host APIs and require explicit exposure through the host registry.

## Debug and Logging

`std::debug` may expose development helpers such as:

- debug print
- assertion helpers
- value formatting for diagnostics

Production embeddings may disable or redirect these helpers through security and host policy.
Game logic should not depend on unrestricted stdout or file-system logging.

## Iteration

The builtin iterable protocol covers arrays, strings when enabled, and host-exposed iterable values.

`for` loops operate over values accepted by the iterable protocol.
The protocol must be represented in type checking and lowering, not implemented as ad hoc runtime reflection.

## Host-Provided Builtins

Hosts may register additional builtin-like modules.

Host-provided modules must:

- use stable module identities
- declare capability requirements
- expose typed signatures
- participate in reflection only according to policy
- participate in hot reload validation when used by compiled modules

Host-provided builtins are not part of the language core.

## Reflection and Metadata

Builtin types and modules participate in internal metadata.

Metadata supports:

- type checking
- bytecode validation
- interface dispatch
- reflection profiles
- JIT lowering
- diagnostics

Script-visible reflection over builtins remains profile-gated.

## Acceptance Criteria

The builtin surface is complete when:

- all core builtin types are represented in the type checker and runtime
- bytecode and VM operations cover baseline numeric, boolean, string, array, tuple, `Option`, and `Result` behavior
- `for` loops lower through a defined iterable protocol
- host-sensitive APIs are not exposed as core standard modules
- builtin metadata supports diagnostics, reflection profiles, reload validation, and JIT lowering
