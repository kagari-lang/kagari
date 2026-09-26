# Kagari Builtins and Standard Library Specification

This document defines the builtin runtime semantics and standard-library design.
The implemented public signatures, method views, API documentation and examples
are owned by the [bundled declaration sources](../../stdlib/README.md), which the
compiler reads at build time. The [declaration architecture](standard-declarations.md)
describes their binding and tool-query boundaries.

Unqualified helper names such as `print` and `type_of` are consulted only after
lexical and declared names. A same-named user function is an ordinary script call;
a non-callable local produces a call-target diagnostic. Reflection permissions
apply to resolved reflection helper calls. Standard methods and qualified standard
functions share one semantic intrinsic target, with method receivers evaluated
before explicit arguments according to [value-semantics.md](value-semantics.md).
When permitted by the active reflection profile, `set_field` with a statically
known field name and `set_index` with a known element type supply that target
type as RHS context. They use ordinary assignment's known-member conflict rules:
erroneous members do not suppress mismatches in independently known members.
Statically nonexistent fields and invalid receiver/index types are HIR errors,
including non-integer indices and out-of-range constant Tuple indices. Array
bounds still depend on runtime length. A failed field read has an error type,
not a Unit value; unknown operands retain their primary diagnostic.
`set_field` requires a `var` field even when reflection writes are enabled.
A `val` receiver binding may reference an object with writable fields, but
reflection does not make a declared `val` field writable. Rejected writes still
check the RHS using the field's known type.
`get_field` and `set_field` require a compile-time String field name. A known
String without a compile-time value produces
`KG_TYPE_REFLECTION_FIELD_NAME_NOT_CONSTANT`; a different known type produces
an argument-type diagnostic. These failures retain the resolved helper target,
and `set_field` still checks its RHS for independent errors.
Both reflection writes compare the RHS against the target type only when the
RHS can complete normally. A return during RHS evaluation preserves earlier
effects and skips the final write. Target writeability/index checks and inner
expression diagnostics remain active.
A resolved field-name expression retains the field declaration identity for
navigation, including read-only or type-invalid writes. Its expression type
remains String; the member target is separate semantic information.
Standard function and method argument checks retain known-member conflicts in
partially erroneous types. Whole unknown/error operands do not generate duplicate
argument-type diagnostics; missing operands are reported by the arity check.
Qualified standard functions check receiver shape only if their first operand
can produce a value. A terminating first operand supplies no container/string/
Option/Result/iterable receiver or subsequent argument context. Arity and inner
expression diagnostics remain active; later operands do not execute.
Reflection get_field/set_field/set_index follow the same produced-receiver rule
for target resolution. A terminating receiver supplies no field or element type;
field-name validation and independent later operand diagnostics still apply.
Receiver termination skips all remaining operand effects and the final access.

Container calls provide checked element/key/value types as context for subsequent
arguments. This applies equally to methods and qualified functions, with the
receiver (or first explicit container argument) checked first. Set algebra uses
the receiver Set type, and Option/Result `unwrap_or` uses the payload type for its
fallback. Context does not coerce incompatible arguments or infer backwards.
Unary `-` accepts a generic operand constrained by `SignedNumber`, including
where-clause and forwarded bounds. `OrderedNumber` alone does not suffice because
it also permits unsigned numbers. Concrete instantiations retain ordinary checked
negation semantics.
`min`, `max`, and `clamp` check the OrderedNumber requirement on each operand.
`assert_eq` checks PartialEq on both operands. Known mismatches remain errors
when another operand is erroneous; the first operand also supplies context to
subsequent matching operands. During error recovery, known numeric operands can
restore the result type of a min/max/clamp call without permitting codegen.
String length uses `len_bytes()` or `len_chars()`; the obsolete standalone
`String.len()` path is removed without an alias.
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

- `Array<T>` (`[T]`) and `MutableArray<T>` resizable arrays
- `Map<K, V>` and `MutableMap<K, V>` insertion-ordered maps
- `Set<T>` and `MutableSet<T>` insertion-ordered sets
- tuple values
- string values

Collection storage is runtime-native.
It is not implemented by Kagari source-level data structures.
The compiler, IR, bytecode verifier, runtime, GC, reload validation, reflection metadata, debugger, and JIT boundary must all understand these collection categories structurally.

`Map` and `Set` are deterministic insertion-ordered collections.
The Rust runtime implementation should use `indexmap` for their backing storage unless a future implementation proves an equivalent deterministic order, hash behavior, and performance profile.

The [equality and hashing contract](value-semantics.md#equality-and-hashing)
defines defaults, custom Struct/enum implementations and user obligations for
mutable keys. Map and Set keys require the canonical standard `Eq + Hash`:

- unit, bool, integers and String use value equality and hashing;
- Tuple, Option and Result compose the protocols of all members;
- user enums use explicit implementations, or eligible variant/member defaults;
- Struct uses explicit implementations, or stable object identity;
- Array, Map and Set use stable object identity, independently of their contents.

Float, interface, host handle/path and function values are not keys themselves.
Default enum eligibility checks every variant. A complete custom enum protocol
may ignore payloads which do not themselves implement Eq/Hash.
Objects referenced by keys are traced while the owning container is reachable.
Builtin-only keys use a bounded immutable canonical key and native lookup.
Custom keys hash and compare in ordinary script frames before committing a
modification; no storage borrow spans a callback. Mutation of the active
container traps, and failure releases lookup guards and temporary GC roots.
Mutation of an identity key cannot invalidate its hash; custom key stability is
the user's responsibility.

Raw `GcHeap` key helpers only execute builtin protocols. Hosts must call script
entrypoints for custom-key lookup and mutation; raw collection snapshots, length
queries and clear operations do not invoke equality/hash callbacks.

The standard declarations include `std::cmp::{PartialEq, Eq, PartialOrd, Ord}`,
`std::hash::Hash`, `std::fmt::{Debug, Display}`, and
`std::ops::{Add, Sub, Mul, Div, Rem, Neg, Not, Index}`,
`std::convert::{From, Into, TryFrom, TryInto}` and `std::iter::{Iterator, IntoIterator}`.
Their short names are available in the prelude;
normal declarations and imports shadow them. Aliases and wildcard imports retain
the declaration identity. `Eq` extends `PartialEq`. These are ordinary trait
bounds, including on associated outputs and GAT parameters. A user trait with
the same spelling receives no intrinsic implementation.

```kagari
trait PartialEq { fn eq(self, other: Self) -> bool; }
trait Eq: PartialEq {}
trait Hash { fn hash(self) -> i64; }
trait Debug { fn debug(self) -> String; }
trait Display { fn display(self) -> String; }
```

These signatures illustrate the canonical contracts; redeclaring them creates
new traits. Script Structs and enums can implement PartialEq, Eq and Hash in
their defining module, following the [value contract](value-semantics.md).
Floats implement PartialEq, but not Eq or
Hash. Hash values are runtime hash codes, not persistent fingerprints, and have
no cross-version or cross-runtime stability promise. User implementations must ensure that equal keys in one runtime
hash equally. `==`, `.eq()` and Map/Set lookup share the same rules.

Scalars implement Debug and Display. Debug additionally formats tuples and enums
structurally and mutable objects as bounded identity previews (for example,
`Array@4:0`). Strings are escaped and quoted in Debug and returned as text in
Display. Nominal Struct, enum and host types can explicitly implement Debug or
Display using normal trait methods; explicit implementations take precedence over
intrinsic defaults. Automatic aggregate Debug uses intrinsic member formatting,
not nested script callbacks. Host values and opaque enum payload categories use
category placeholders, such as `<host>` and `<function>`, without reading host
state. It is not a serialization format or a derived impl.
The initial protocols support static bounds and calls; using them or their
subtraits as erased interface value types is rejected with a source diagnostic.

Key preparation is limited to 65,536 canonical components; automatic formatting
to depth 64 and 1 MiB of output. Exceeding these limits traps before any mutation.
`HashKey` and `Comparable` are removed source-level pseudo-bounds. Use `Eq + Hash`
and `PartialEq`. `Iterable`, `Item<I>`, `OrderedNumber`, and `SignedNumber` remain
intrinsic capabilities for builtin iteration and numeric signatures; they are not
aliases for user-implementable operator traits.

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

Their constructors `Some(value)`, `None`, `Ok(value)` and `Err(error)` are
available through the prelude, `Option::Some` / `Option::None`,
`Result::Ok` / `Result::Err`, and the `std::option` / `std::result` namespaces.
Standard-module imports support aliases and wildcard imports for these variants.
User bindings take precedence over implicit prelude names. `None` has no payload
and is written without parentheses. Patterns use the same resolved identities,
including nested, alternative and binding-condition patterns.

Constructor type arguments come from payloads, an expected type, or an explicit
owner such as `Result<i32, String>::Err("missing")`. Missing unconstrained arguments
are diagnosed: give `val outcome: Result<i32, String> = Ok(42)` an annotation when
there is no surrounding expected type. Constructors are not first-class functions.

The postfix `?` evaluates its operand once. `Some(v)` / `Ok(v)` produce `v`;
`None` / `Err(e)` return the original failure value from the nearest function or
closure. Option propagates only into Option; Result propagates only into Result
with the same error type. Their success types may differ. The return context of
a closure is independent of its enclosing function. Postfix chaining such as
`read()?.field` and `nested??` is supported.

Use `ok_or(error)` or `ok_or_else(|| error)` to convert Option into Result,
and `map_err(|error| converted)` to change error types before propagation.
`ok_or` evaluates its error argument eagerly; `ok_or_else` calls its closure
only for None. `map`, `map_err` and `and_then` check callback signatures and call
script closures only on the selected variant, using ordinary execution frames.
There is no implicit error conversion, general `Try`/`FromResidual` protocol,
`throw`/`try`/`catch` or built-in Error value in this version. New Err captures
its source and stack; propagation and map_err preserve it. See
[error reporting](error-reporting.md).
See [failure semantics](failure-semantics.md) for traps and termination, which
`?` cannot intercept, and the [executable example](../../examples/syntax/result-option.kgr).

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

Ordinary integer arithmetic traps on overflow in every build mode and backend.
Explicit wrapping operations are the only exception. Equality, copying, iteration,
and evaluation order follow [value semantics](value-semantics.md). Standard
mutation failures follow [failure semantics](failure-semantics.md).

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
Declarations, signatures, documentation and examples are defined in `stdlib/*.kgr`. Native runtime helpers implement their storage and execution behavior.
Standard library calls must not be resolved through script-visible reflection, host string dispatch, or source-level reimplementations of core storage.

The production execution path is:

```text
typed source call
  -> standard function or method metadata
  -> stable IR and bytecode intrinsic identifier
  -> bytecode verifier signature and key-eligibility checks
  -> VM/runtime standard helper
```

Runtime helpers own GC tracing, mutation semantics, resource accounting, deterministic traps, and profile or capability checks where a helper is policy-sensitive.
The interpreter and optional JIT fallback paths must observe the same intrinsic semantics.

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

### Standard Value Semantics

Arrays, maps, sets, strings, `Option`, and `Result` are structural runtime values.
Arrays, maps, and sets share heap storage. Only their `Mutable*` access types permit entry modification; read-only views remain live. See [collection access](collection-access.md).
String values are script-visible text values with validated UTF-8 boundary behavior for slicing.
`Option<T>` and `Result<T, E>` are ordinary standard enum values and are not hidden control-flow constructs.

`Map<K, V>` and `Set<T>` preserve insertion order.
The Rust runtime implementation uses `indexmap` to provide deterministic script-visible ordering.
The same ordering is used by `keys`, `values`, `entries`, `to_array`, set algebra helpers, iterable helpers, display/debug classification, and reflection metadata.

### Standard Module Shape

The collection modules provide paired native types and associated factories:

| Module | Read-only type | Writable type | Constructors |
| --- | --- | --- | --- |
| `std::array` | `Array<T>` / `[T]` | `MutableArray<T>` | `Type::new()`, `Type::from(items)` |
| `std::map` | `Map<K,V>` | `MutableMap<K,V>` | `Type::new()`, `Type::from(entries)` |
| `std::set` | `Set<T>` | `MutableSet<T>` | `Type::new()`, `Type::from(items)` |

Factories accept arrays; map entries are `(K, V)` tuples. Empty construction
needs sufficient type context. Map keys and set elements require `Eq + Hash`.
Read methods accept either access type; mutators require `Mutable*` receivers.
Array literals infer `MutableArray<T>`. Returned key/value/entry arrays and set
algebra results are fresh writable containers. `from` always allocates fresh
shallow storage, while assignment to a read-only type creates a live alias.

Full signatures, failure behavior and examples live in the source declarations:
[array](../../stdlib/array.kgr), [map](../../stdlib/map.kgr),
[set](../../stdlib/set.kgr). The [collection contract](collection-access.md)
defines assignability, mutation and factory guarantees. The old module-level
Map/Set `new` functions are removed.

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
- `ok_or<T, E>(value: Option<T>, error: E) -> Result<T, E>`
- `ok_or_else<T, E>(value: Option<T>, error: fn() -> E) -> Result<T, E>`

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
- `to_array<I>(value: I) -> MutableArray<Item<I>> where I: Iterable`
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
- `assert_eq<T>(lhs: T, rhs: T, message: String) -> () where T: PartialEq`
- `panic(message: String) -> ()`

Debug helpers may trap, emit debugger events, or call host-provided debug sinks according to the active runtime profile.
They must not grant unrestricted file, terminal, network, or process access.

### Standard Library Example

```kagari
fn main() -> (usize, bool, usize, bool, i32) {
    val values = [1, 2];
    values.push(3);

    val scores: MutableMap<String, i32> = MutableMap::new();
    scores.insert("alice", 10);
    scores.insert("bob", 12);

    val names: MutableSet<String> = MutableSet::new();
    names.insert("alice");
    names.insert("bob");

    std::debug::assert(scores.contains_key("alice"), "missing score");

    (
        values.len(),
        std::string::starts_with("kagari", "ka"),
        scores.keys().len(),
        names.contains("bob"),
        std::math::max(10, 12)
    )
}
```

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


## Ordering protocols

`Ordering` (also `std::cmp::Ordering`) has unit variants `Less`, `Equal`, `Greater`.
Type aliases (`use std::cmp::Ordering as Order`) and variant imports
(`use std::cmp::Ordering::*`) work in constructors and patterns.
`PartialOrd: PartialEq` declares `partial_cmp(self, other: Self) -> Option<Ordering>`.
`Ord: Eq + PartialOrd` declares `cmp(self, other: Self) -> Ordering`.
Comparison operators select PartialOrd; None makes each of `<`, `<=`, `>` and `>=`
false. Implementations must agree with equality and with each other. Primitive
integers, bool, unit, String and Ordering supply both; floats only PartialOrd.
There is no automatic Struct, Tuple or user-enum ordering. Custom Struct/enum
implementations use static calls and the ordinary failure/effect boundary.
See [ordering.kgr](../../examples/syntax/ordering.kgr).


## Arithmetic operator protocols

`std::ops::{Add, Sub, Mul, Div, Rem}` are prelude traits with one explicit RHS
parameter and an associated `Output`. Each declares `fn add(self, rhs: Rhs) ->
Self::Output` (respectively sub/mul/div/rem). Operator expressions and method calls
select the same applied implementation. Multiple applications of the same protocol
are selected by the RHS type; unrelated same-named traits remain ambiguous.
Different operand/result types are allowed;
no implicit numeric conversion or RHS default type parameter is introduced.
Matching builtin numeric types use their existing checked instructions. User
Struct/enum implementations run ordinary methods; their effects are not rolled
back on failure. These traits do not enable compound-assignment overloads.
See [operators.kgr](../../examples/syntax/operators.kgr).

`std::ops::{Neg, Not}` declare `type Output` and `fn neg(self) -> Self::Output`
(respectively `not`). They control unary `-` and `!`; signed builtin numeric
negation and bool negation keep direct instructions. Custom outputs may differ
from the receiver. `&&`/`||` remain bool-only short-circuit operators.


## Read-only indexing protocol

`std::ops::Index<I>` declares `type Output` and `fn index(self, rhs: I) ->
Self::Output`. `container[i]` and `container.index(i)` use the selected method.
Arrays supply builtin integer indexing; existing Tuple and host-path syntax
retain their specialized rules. No Map/String indexing is added by this checkpoint.
Returned values obey ordinary value semantics. A returned Struct shares its
object identity: `container[i].field = value` requires a writable field but no
container setter or writeback. A custom getter runs once during target preparation,
and its returned object remains the target even if RHS code changes container
contents. Existing native location revalidation semantics are unchanged.
Index does not permit replacing `container[i]` or mutating a returned Tuple value
in place. Methods may trap, and their completed effects are not rolled back.
Use an explicit Option-returning get method for recoverable lookup failure.
See [index.kgr](../../examples/syntax/index.kgr).

These operator/ordering traits use the standard protocol identities described in
[traits](traits.md#standard-protocol-identities). Implementations must belong to
the script type's defining module. Standard traits and their descendants remain
static-only; this does not change ordinary user-defined dynamic interfaces.

## Conversion protocols

`std::convert::{From, Into, TryFrom, TryInto}` are static-only prelude traits:

```kagari
trait From<S> { fn from(value: S) -> Self; }
trait Into<D> { fn into(self) -> D; }
trait TryFrom<S> { type Error; fn try_from(value: S) -> Result<Self, Self::Error>; }
trait TryInto<D> { type Error; fn try_into(self) -> Result<D, Self::Error>; }
```

Implement From/TryFrom on the destination. `D::from(source)` and
`D::try_from(source)` are associated calls with no receiver. Qualified paths
select the applied trait. `source.into()` and `source.try_into()` derive from the
same destination implementation; explicit Into/TryInto impls are rejected.
The target comes from the result annotation/return context or a unique generic
bound. Ambiguous targets need an annotation. Generic bounds and Error projections
use the same derivation as calls. Operands evaluate once; no implicit conversion,
error conversion during `?` or exception handling is introduced. Error origin
metadata follows [error reporting](error-reporting.md).

Identity `From<T> for T` preserves ordinary value/reference semantics and cannot
be overridden. Custom conversions must belong to the defining module of a script
nominal source or destination. Host conversions and overlapping identity impls
are rejected. No blanket From-to-TryFrom conversion or numeric conversion matrix
is introduced: fallible conversions explicitly return Result and may use any
error type. A From method may still trap like any script function; its contract
is that expected conversion failures do not use a business error result.
See [conversions.kgr](../../examples/syntax/conversions.kgr).


## Iteration protocols

`std::iter::{Iterator, IntoIterator}` are static-only prelude traits:

```kagari
trait Iterator {
    type Item;
    fn next(self) -> Option<Self::Item>;
}
trait IntoIterator {
    type Item;
    type IntoIter: Iterator<Item = Self::Item>;
    fn into_iter(self) -> Self::IntoIter;
}
```

`for pattern in expression` evaluates the expression once, calls `into_iter`
once, and repeatedly calls `next`. Some supplies the next item; None ends the
loop. Continue proceeds to the next call; break and return perform normal resource
cleanup. The next call is an ordinary script call for custom iterators, with the
same budget, GC roots, trap behavior and pinned code versions as other methods.
An Iterator automatically implements identity IntoIterator, including under generic
bounds. It cannot also declare a conflicting IntoIterator implementation.
Custom iterables return an iterator whose Item agrees with their own Item.

Arrays, Map, Set and String implement IntoIterator using the opaque shared
`Cursor<Item>` type. Cursor implements Iterator and identity IntoIterator.
Array/Set items are elements, Map items are `(key, value)` tuples in insertion
order, and String items are single Unicode scalars represented as String.
Cursor construction takes a shallow snapshot, preserving existing native for-loop
semantics: contained objects retain their identity. Copying a Cursor shares its
position; converting a collection again creates a new position and snapshot.
A custom iterator owns its state and consistency rules; no automatic clone,
reset, exact-length or fused-iterator promise is imposed on its implementation.

Native cursors reject structural modification of their source while actively
iterating. For-loop guards end on exhaustion, break, return or failed execution;
nested loops retain independent guards. Direct into_iter/next use keeps its guard
until None or the end of the root execution session. Root-session cleanup also
runs after trap, cancellation and budget exhaustion, independently of GC timing.
A rooted Cursor can survive between calls and resume. Resuming after its source
was structurally changed traps; nonstructural updates preserve the snapshot.
For loops suspend a native cursor on exit, so the same cursor may resume later
if the source structure is unchanged. Custom iterators have no implicit native
source guard: wrappers that acquire native cursors follow the direct-call rules.

Host retention uses the normal rooted-value API. Cursor handles are runtime-owned,
generation checked, and trace both their source and snapshot. They retain their
execution version; they are not transferable across runtimes. They provide no
Eq/Hash, serialization of execution state, or script constructor.
See [iterators.kgr](../../examples/syntax/iterators.kgr).

## String construction

`std::array::join(value: [String], separator: String) -> String`, also available as
`value.join(separator)`, joins already formatted strings. Empty input returns an
empty string; separators appear only between adjacent elements. It does not mutate
its source or call user code. Size arithmetic is checked and result allocation is
fallible. String concatenation remains `lhs.concat(rhs)`.

Interpolated `f"..."` expressions use standard Display/Debug dispatch and the same
native join operation. See [interpolated strings](syntax.md#interpolated-strings)
for evaluation order, escaping and propagation rules. String `+`, builders and
extended format options are separate features.
