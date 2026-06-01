# Kagari Builtins and Standard Library Specification

This document defines the builtin and standard library surface required for a production-ready Kagari runtime.
It describes language-level standard capabilities and standard modules, not host application APIs.

## Design Goals

- keep the standard surface predictable, deterministic, and fully typed
- support game and business scripting without broad system APIs
- keep dangerous capabilities behind host APIs and security profiles
- make builtin types visible to type checking, bytecode, reflection metadata, and JIT lowering
- avoid hidden dependence on Rust standard-library concepts that do not exist in Kagari
- avoid implementing core containers twice; runtime-native containers own storage, GC behavior, resource accounting, and intrinsic dispatch

## Core Builtin Types

The core type set includes:

- `()`
- `bool`
- signed integers: `i8`, `i16`, `i32`, `i64`, `isize`
- unsigned integers: `u8`, `u16`, `u32`, `u64`, `usize`
- floating-point numbers: `f32`, `f64`
- `String`
- arrays or vectors as `[T]`
- ordered maps as `Map<K, V>`
- ordered sets as `Set<T>`
- tuples
- user-defined structs and enums
- trait/interface value types
- `Option<T>`
- `Result<T, E>`

The numeric type names follow Rust spelling.
The semantics do not import Rust ownership or borrowing.

## Collection Types

The collection surface includes:

- `[T]` dynamically sized array/vector values
- `Map<K, V>` insertion-ordered key/value values
- `Set<T>` insertion-ordered unique-value values
- tuple values
- string values

Collection storage is runtime-native.
It is not implemented by Kagari source-level data structures.
The compiler, IR, bytecode verifier, runtime, GC, reload validation, reflection metadata, debugger, and JIT boundary must all understand these collection categories structurally.

`Map` and `Set` are deterministic insertion-ordered collections.
The Rust runtime implementation should use `indexmap` for their backing storage unless a future implementation proves an equivalent deterministic order, hash behavior, and performance profile.

Map and set keys are restricted to standard hash-key types in the initial production surface:

- `bool`
- signed and unsigned integer types
- `String`

Floating-point keys are not part of the production baseline.
Struct, enum, tuple, array, map, set, host value, and interface value keys require a later explicit hash/equality specification before they can be accepted.

The standard library may use builtin type constraints in signatures:

- `HashKey`: values accepted as `Map` keys and `Set` members
- `Iterable`: values accepted by the iterable protocol
- `Item<I>`: the element type yielded by an iterable value
- `OrderedNumber`: numeric values accepted by ordering helpers
- `SignedNumber`: signed integer and floating-point numeric values
- `Comparable`: values with standard equality semantics

Array operations include:

- length
- empty check
- indexing
- append/push
- pop
- insert
- remove
- clear
- iteration

Array mutation follows the ordinary Kagari value model and security/resource rules.

Map operations include:

- construction of an empty map
- length
- empty check
- key containment
- lookup returning `Option<V>`
- insertion and update
- removal returning `Option<V>`
- clear
- iteration over keys, values, and entries in insertion order

Set operations include:

- construction of an empty set
- length
- empty check
- value containment
- insertion
- removal
- clear
- iteration in insertion order
- union, intersection, and difference helpers

Set algebra helpers may be source-level facades when they only compose native set intrinsics and preserve deterministic ordering.

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

String operations include:

- byte length
- scalar length where the operation is defined over Unicode scalar values
- empty check
- equality
- concatenation
- containment
- prefix and suffix checks
- basic slicing by validated UTF-8 boundary
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

The standard module set is deterministic and typed.
Standard modules are compiler-known exports backed by intrinsic identifiers.
They may also expose optional `.kg` facade functions for pure helper logic, but native runtime builtins remain the source of truth for core containers, strings, numeric operations, and security-sensitive helpers.

Core standard modules:

```text
std::debug
std::math
std::array
std::map
std::set
std::string
std::option
std::result
std::iter
```

Host-sensitive modules such as file system, networking, timers, database, logging sinks, and service registries are not core standard modules.
They are host APIs and require explicit exposure through the host registry.

### Standard Module Shape

`std::array` provides typed operations for `[T]`, including:

- `len<T>(value: [T]) -> usize`
- `is_empty<T>(value: [T]) -> bool`
- `get<T>(value: [T], index: usize) -> Option<T>`
- `push<T>(value: [T], item: T) -> [T]`
- `pop<T>(value: [T]) -> Option<T>`
- `insert<T>(value: [T], index: usize, item: T) -> [T]`
- `remove<T>(value: [T], index: usize) -> Option<T>`
- `clear<T>(value: [T]) -> [T]`

`std::map` provides typed operations for `Map<K, V>`, including:

- `new<K, V>() -> Map<K, V>`
- `len<K, V>(value: Map<K, V>) -> usize`
- `is_empty<K, V>(value: Map<K, V>) -> bool`
- `contains_key<K, V>(value: Map<K, V>, key: K) -> bool`
- `get<K, V>(value: Map<K, V>, key: K) -> Option<V>`
- `insert<K, V>(value: Map<K, V>, key: K, item: V) -> Map<K, V>`
- `remove<K, V>(value: Map<K, V>, key: K) -> Option<V>`
- `clear<K, V>(value: Map<K, V>) -> Map<K, V>`
- `keys<K, V>(value: Map<K, V>) -> [K]`
- `values<K, V>(value: Map<K, V>) -> [V]`
- `entries<K, V>(value: Map<K, V>) -> [(K, V)]`

`std::set` provides typed operations for `Set<T>`, including:

- `new<T>() -> Set<T>`
- `len<T>(value: Set<T>) -> usize`
- `is_empty<T>(value: Set<T>) -> bool`
- `contains<T>(value: Set<T>, item: T) -> bool`
- `insert<T>(value: Set<T>, item: T) -> Set<T>`
- `remove<T>(value: Set<T>, item: T) -> bool`
- `clear<T>(value: Set<T>) -> Set<T>`
- `to_array<T>(value: Set<T>) -> [T]`
- `union<T>(lhs: Set<T>, rhs: Set<T>) -> Set<T>`
- `intersection<T>(lhs: Set<T>, rhs: Set<T>) -> Set<T>`
- `difference<T>(lhs: Set<T>, rhs: Set<T>) -> Set<T>`

`std::string` provides typed operations for `String`, including:

- `len_bytes(value: String) -> usize`
- `len_chars(value: String) -> usize`
- `is_empty(value: String) -> bool`
- `concat(lhs: String, rhs: String) -> String`
- `contains(value: String, needle: String) -> bool`
- `starts_with(value: String, prefix: String) -> bool`
- `ends_with(value: String, suffix: String) -> bool`
- `slice(value: String, start: usize, end: usize) -> Option<String>`

`std::option` provides typed helpers for `Option<T>`, including:

- `is_some<T>(value: Option<T>) -> bool`
- `is_none<T>(value: Option<T>) -> bool`
- `unwrap_or<T>(value: Option<T>, fallback: T) -> T`
- `map<T, U>(value: Option<T>, mapper: fn(T) -> U) -> Option<U>`
- `and_then<T, U>(value: Option<T>, mapper: fn(T) -> Option<U>) -> Option<U>`

`std::result` provides typed helpers for `Result<T, E>`, including:

- `is_ok<T, E>(value: Result<T, E>) -> bool`
- `is_err<T, E>(value: Result<T, E>) -> bool`
- `unwrap_or<T, E>(value: Result<T, E>, fallback: T) -> T`
- `map<T, U, E>(value: Result<T, E>, mapper: fn(T) -> U) -> Result<U, E>`
- `map_err<T, E, F>(value: Result<T, E>, mapper: fn(E) -> F) -> Result<T, F>`
- `and_then<T, U, E>(value: Result<T, E>, mapper: fn(T) -> Result<U, E>) -> Result<U, E>`

`Option` and `Result` helper functions operate over ordinary standard enum values.
They are not magic control-flow constructs.

`std::iter` exposes shared iterable protocol helpers for arrays, maps, sets, strings, and host-exposed iterable values, including:

- `len<I>(value: I) -> usize where I: Iterable`
- `is_empty<I>(value: I) -> bool where I: Iterable`
- `get<I>(value: I, index: usize) -> Option<Item<I>> where I: Iterable`
- `to_array<I>(value: I) -> [Item<I>] where I: Iterable`
- `for_each<I>(value: I, callback: fn(Item<I>) -> ()) where I: Iterable`

The iterable protocol is represented in type checking and lowering, not implemented through runtime reflection.

`std::math` provides deterministic numeric helpers over supported numeric types, including:

- `min<T>(lhs: T, rhs: T) -> T where T: OrderedNumber`
- `max<T>(lhs: T, rhs: T) -> T where T: OrderedNumber`
- `clamp<T>(value: T, min: T, max: T) -> T where T: OrderedNumber`
- `abs<T>(value: T) -> T where T: SignedNumber`
- `floor(value: f64) -> f64`
- `ceil(value: f64) -> f64`
- `round(value: f64) -> f64`
- `sqrt(value: f64) -> f64`
- `sin(value: f64) -> f64`
- `cos(value: f64) -> f64`
- `tan(value: f64) -> f64`

Float helpers must define deterministic trap or result behavior for invalid inputs before they are exposed in restricted production profiles.

`std::debug` provides profile-controlled development helpers, including:

- `print(message: String) -> ()`
- `assert(condition: bool, message: String) -> ()`
- `assert_eq<T>(lhs: T, rhs: T, message: String) -> () where T: Comparable`
- `panic(message: String) -> ()`

Debug helpers may trap, emit debugger events, or call host-provided debug sinks according to the active runtime profile.
They must not grant unrestricted file, terminal, network, or process access.

## Debug and Logging

`std::debug` may expose development helpers such as:

- debug print
- assertion helpers
- value formatting for diagnostics

Production embeddings may disable or redirect these helpers through security and host policy.
Game logic should not depend on unrestricted stdout or file-system logging.

## Iteration

The builtin iterable protocol covers arrays, maps, sets, strings when enabled, and host-exposed iterable values.

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
- bytecode and VM operations cover numeric, boolean, string, array, map, set, tuple, `Option`, and `Result` behavior
- `Map` and `Set` use deterministic insertion order and are implemented with `indexmap` or an explicitly equivalent ordered backing
- map and set key eligibility is enforced by type checking and bytecode verification
- standard modules resolve to typed intrinsic metadata rather than reflection or host-string dispatch
- `for` loops lower through a defined iterable protocol
- host-sensitive APIs are not exposed as core standard modules
- builtin metadata supports diagnostics, reflection profiles, reload validation, and JIT lowering
