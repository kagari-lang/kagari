# Language Foundation and Native Libraries

The installed `core`/`alloc`/`std` foundation defines complete language types and
protocols. Its 26 language traits and 15 other foundation traits are explicit
standard-library registrations. Their generated source is parsed and checked
against the installed declarations and validated language roles. Runtime supplies their basic checked native implementations. Optional
library modules and application modules use the same explicit registration API;
see [native declarations](standard-declarations.md). Generated `.kgr` serves tooling
and is not an executable standard library.

The foundation is always installed. List/MutableList own common algorithm methods,
String owns its inherent methods, `core::num` owns the finite scalar numeric
methods below, and `core::iter::Iterable` owns the finite lazy transformation and
terminal API. The [collection contract](collection-access.md) defines the collection
surface. Predecessor APIs outside these inventories, including enum combinators,
extended iterator algorithms, collection snapshots and additional container classes,
remain withdrawn; no compatibility aliases are installed.

## Namespaces and prelude

Declarations have one canonical owner. Public `std` re-exports preserve that
identity, including trait implementations, generic bounds, associated members,
constructors and tooling navigation.

| Canonical module | Declarations |
| --- | --- |
| `core::ops` | Operator traits, Index, Fn, Try, FromResidual, ControlFlow, RangeBounds, range types and Bound |
| `core::cmp` | PartialEq, Eq, PartialOrd, Ord and Ordering |
| `core::hash` | Hash |
| `core::fmt` | Debug and Display |
| `core::convert` | From, Into, TryFrom, TryInto and Infallible |
| `core::iter` | Iterator, Iterable, FromIterator, Sum, Product, CollectionCursor and eight lazy adapter types |
| `core::str` | FromStr |
| `core::default` | Default |
| `core::num` | ParseError, TryFromIntError, scalar numeric inherent methods and numeric native implementations |
| `core::char` | CharTryFromError and character default/conversion implementations |
| `core::option`, `core::result` | Option and Result |
| `core::future` | Cold single-drive Future |
| `core::task` | Task, TaskScope, SpawnError and cold two-input join/try_join |
| `alloc::string`, `alloc::vec` | String and Vec |
| `std::collections` | List/MutableList, Map/MutableMap, Set/MutableSet, HashMap and HashSet |

Corresponding `std::ops`, `std::cmp`, `std::hash`, `std::fmt`, `std::convert`,
`std::iter`, `std::str`, `std::default`, `std::num`, `std::char`, `std::option`, `std::result`, `std::future`, `std::string`
and `std::vec` expose the canonical declarations through checked re-exports.
`std::task` exposes the `core::task` declarations, including `join` and `try_join`.
The package/module spelling does not change Kagari's GC or shared-object semantics.
There is no installed `core::language` module or ArrayList declaration.

`std::prelude` explicitly exports Iterator, Iterable, FromIterator, PartialEq,
Eq, PartialOrd, Ord, From, Into, TryFrom, TryInto, Default, Fn, Option, Result, Some, None,
Ok, Err, String, Vec, Future, Task, TaskScope and SpawnError. `join` and `try_join`
require an explicit import or qualified call. Iterable is a retained Kagari extension. Local declarations
and explicit imports take precedence over these implicit imports.
Operators use their checked language roles independently of name imports; writing
an explicit operator trait bound or implementation requires that name in scope.
Hash, Debug, Display, operator traits other than Fn, collection interfaces,
HashMap/HashSet, Ordering, ranges and error types require explicit imports.
New public library declarations do not automatically enter the prelude.
Generic operation requirements use [ordinary trait bounds](traits.md#operation-specific-bounds),
not compiler-recognized numeric constraint names.

## Structured Future combinators

The standard library declares ordinary generic functions in `core::task`:

```text
join<A, B>(left: Future<A>, right: Future<B>) -> Future<(A, B)>
try_join<A, B, E>(left: Future<Result<A, E>>, right: Future<Result<B, E>>)
    -> Future<Result<(A, B), E>>
```

Construction evaluates the two argument expressions in ordinary left-to-right order
but does not start either Future. For example, `core::task::join(async { read_a().await },
async { read_b().await }).await` permits both IO waits to be pending without creating
public Tasks. Awaiting starts both branches cooperatively, returns values
in input order, and completes local cleanup before exposing the result. `join`
does not inspect Result values. `try_join` returns the first observed Err without
replacing its optional diagnostic origin, or Ok of both success values. A selected
Err closes the unfinished sibling before publication; an execution failure closes
siblings and terminates the parent for either combinator. Execution
failures and cancellation are not converted to E. Duplicate/already-awaited Future
operands trap; nested Future outputs are not flattened. See the
[execution contract](execution.md#structured-two-input-composition) for admission,
cycle checks, bounds, shared Task waits and cancellation ownership.

## Foundation trait inventory

The installed foundation contains these 41 traits. All remain
available without optional libraries. This is the implemented inventory, not a
decision that every trait needs a compiler language role.

| Family | Traits |
| --- | --- |
| Collection interfaces | List, MutableList, Map, MutableMap, Set, MutableSet |
| Iteration | Iterator, Iterable |
| Equality and hashing | PartialEq, Eq, Hash |
| Ordering | PartialOrd, Ord |
| Arithmetic | Add, Sub, Mul, Div, Rem |
| Bitwise and shifts | BitAnd, BitOr, BitXor, Shl, Shr |
| Unary operators | Neg, Not |
| Indexing and calls | Index, Fn |
| Ranges | RangeBounds |
| Formatting | Debug, Display |
| Conversion | From, Into, TryFrom, TryInto |
| Propagation | Try, FromResidual |
| Parsing | FromStr |
| Construction and aggregation | Default, FromIterator, Sum, Product |

The [ownership partition](../architecture.md#core-trait-inventory) has 26
language items and 15 ordinary native-library traits, retaining mandatory
availability of all 41. Parser/HIR analyzes their generated declarations; checked
role identities select syntax semantics, and library policy follows installed
declaration/implementation records. Algorithms and additional containers remain
separate from trait declarations. Try, FromResidual and ControlFlow require
explicit imports; they are not prelude additions.

## Default construction

`core::default::Default` is an ordinary core-library trait, also exported through
`std::default` and the prelude. It declares required static `fn default() -> Self`;
it has no language role, implicit implementation or special runtime constructor.
User types implement it explicitly and use ordinary static calls:

```text
struct Count { val value: i32 }
impl Default for Count { fn default() -> Self { Count { value: 42 } } }
val direct = Count::default();
val contextual: Count = Default::default();
fn make<T: Default>() -> T { T::default() }
```

The trait is static-only, not an interface value type. Standard implementations
return Unit, false, correctly typed integer/positive floating zero, empty String,
a fresh empty Vec<T>, or None for Option<T>. The [character implementation](#character-scalar)
adds the scalar default described below. Vec and Option do not require
T: Default. Other types need an explicit implementation; no tuple/array/map/set,
Result, host/view or implicit user-struct implementations are supplied.
Default does not zero-initialize arbitrary types or enable derive or enum default
selection. [Struct update](syntax.md#struct-update-expressions) is a separate ordinary
construction form; its base may use Default or any eligible struct expression.

## Core Builtin Types

The core type set includes:

- `()`
- `bool`
- `char`
- signed integers: `i8`, `i16`, `i32`, `i64`
- unsigned integers: `u8`, `u16`, `u32`, `u64`
- floating-point numbers: `f32`, `f64`
- `String`
- builtin fixed-length `[T]` arrays
- registered `Vec<T>` storage and read-only `List<T>` interfaces
- map interfaces as `Map<K, V>`
- set interfaces as `Set<T>`
- tuples
- user-defined structs and enums
- trait/interface value types
- `Option<T>`
- `Result<T, E>`
- `Future<T>` (cold script/native producer values and explicit await through the owned driver)

The numeric types have fixed widths on every target. There are no machine-width
integer types or aliases. Standard lengths, capacities, indices and logical counts
use `u64`; signed 64-bit numeric contracts use `i64`. This does not change
user-defined Index parameter types or introduce implicit numeric conversions.
The semantics do not import Rust ownership or borrowing. Physical storage remains
subject to [machine and resource boundaries](runtime.md#machine-and-resource-boundaries).

## Character scalar

`char` is exactly one Unicode scalar: U+0000 through U+10FFFF, excluding the
surrogate range U+D800 through U+DFFF. It is not a byte, UTF-16 code unit,
grapheme cluster or alias of `u32`. It follows ordinary scalar value-copy/clone
semantics; assigning or passing a character does not allocate a shared object.
Its exact type is preserved in fields, generic containers and persistent statics.
Changing a static from `char` to `u32` is not a compatible reload type change.

The minimal protocols are PartialEq/Eq, PartialOrd/Ord, Hash, Display, Debug and
Default. Equality compares scalar values, ordering is by scalar value and equal
characters hash equally. Display emits the scalar's UTF-8 text. Debug emits a
single-quoted escaped character using Rust's character Debug escape policy (for
example, NUL is `'\0'`, newline is `'\n'` and an apostrophe is `'\''`). Default
returns NUL, U+0000. This adds no new Copy/Clone trait declarations.

The exact character conversion declaration is fixed as follows (signatures only;
trait names refer to their canonical standard declarations):

```text
// In core::char; std::char re-exports this same enum, not a second type.
pub enum CharTryFromError {
    InvalidScalar,
}

impl Default for char {
    fn default() -> char;
}

impl From<char> for u32 {
    fn from(value: char) -> u32;
}

impl From<char> for String {
    fn from(value: char) -> String;
}

impl TryFrom<u32> for char {
    type Error = core::char::CharTryFromError;
    fn try_from(value: u32) -> Result<char, core::char::CharTryFromError>;
}
```

`CharTryFromError` has no type parameters and its only variant, `InvalidScalar`,
has no payload. It is not a prelude name. `u32::from(character)` returns the scalar
value; `String::from(character)` encodes that scalar as UTF-8. `char::try_from(value)`
returns `Err(core::char::CharTryFromError::InvalidScalar)` for surrogates or values
above U+10FFFF, otherwise `Ok(character)`. This is domain validation, not numeric
truncation. Existing Into/TryInto adapters use these forward implementations.

There is no implicit character/integer conversion or `as` cast in either direction,
character arithmetic, bitwise operation, increment, FromStr parsing, Sum or Product.
Character literal spelling and matching belong to [syntax](syntax.md#character-literals);
constant eligibility follows [the scalar const rules](traits.md#associated-constants).
This surface does not add character-range iteration or new String operations.

## Bytes and byte arrays

A [byte-character literal](syntax.md#byte-literals), such as `b'A'` or `b'\xFF'`,
is an ordinary `u8`, not a new scalar type or a Unicode `char`. Its type is fixed
rather than inferred as another integer width. Existing `u8` operations and
explicit numeric conversions apply.

Cooked `b"..."` and raw `br"..."` / `br#"..."#` byte strings construct ordinary
mutable fixed-length `[u8]` arrays. Every evaluation allocates fresh array storage,
even for empty literals or repeated calls evaluating the same source expression.
Assignment, arguments and returns share an already constructed array's identity:
mutating an alias updates that array, but not another evaluation's result.
Content equality (`==`) compares bytes; identity (`===`) distinguishes storage,
including distinct empty arrays.

These arrays have the normal array `len`, `is_empty`, indexed replacement and
Iterable surface, with `u64` lengths and `u8` elements. They do not grow or become
Vec/List values implicitly. `Vec::from(bytes)` explicitly copies the elements into
independent growable storage. Byte arrays are not String values: NUL and FF are
valid payload bytes, with no UTF-8 validation or implicit text conversion. Literal
source content must be ASCII; cooked escapes can express any byte. Neither form
interpolates.
See [byte literal syntax](syntax.md#byte-literals) for escapes, delimiters and
multiline rules, and [collection contracts](collection-access.md) for array access.
This adds no String indexing, traversal or general UTF-8 conversion API.

## Collection Types

The complete [collection contracts](collection-access.md) define List/MutableList,
Map/MutableMap and Set/MutableSet. They are generic bounds and interface types.
Vec/HashMap/HashSet are the canonical defaults and remain available with
optional modules disabled. Array literals create fixed-length `[T]`;
`Vec::from(array)` and `HashSet::from(array)` construct independent collection
storage explicitly. Default hash containers
use Rust std::collections and promise no insertion or sorted traversal order.
Additional container types register storage and ordinary trait impls without
adding concrete-type dispatch to the compiler or VM.

The [equality and hashing contract](value-semantics.md#equality-and-hashing)
defines conditional implementations, explicit Struct/enum implementations and user obligations for
mutable keys. Default HashMap keys and HashSet elements require the
checked `Eq + Hash` contracts; Map/Set interfaces do not:

- unit, bool, char, integers and String use value equality and hashing;
- Tuple, Option and Result compose the protocols of all members;
- user Structs and enums require explicit implementations;
- Array has conditional content PartialEq only, without Eq/Hash;
- Vec, HashMap and HashSet use their applicable stdlib content implementations;
  unsatisfied bounds reject comparison without an identity fallback. See
  [collection protocols and helpers](collection-access.md#content-protocols-and-helpers).

Float, collection-interface, host handle/path and function values are not keys.
Conditional core enum eligibility checks every variant. A complete explicit user
enum protocol may ignore payloads which do not themselves implement Eq/Hash.
Objects referenced by keys are traced while the owning container is reachable.
Builtin-only keys use a bounded immutable canonical key and native lookup.
Custom keys hash and compare in ordinary script frames before committing a
modification; no storage borrow spans a callback. Mutation of the active
container traps, and failure releases lookup guards and temporary GC roots.
Authors may explicitly choose identity equality with a consistent hash; key
stability remains the author's responsibility.

## Option and Result

`Option<T>` and `Result<T, E>` are standard enum types.

Their constructors `Some(value)`, `None`, `Ok(value)` and `Err(error)` are
available through the prelude, `Option::Some` / `Option::None`,
`Result::Ok` / `Result::Err`, and the `core::option` / `core::result` namespaces
(or their `std` re-exports).
Standard-module imports support aliases and wildcard imports for these variants.
User bindings take precedence over implicit prelude names. `None` has no payload
and is written without parentheses. Patterns use the same resolved identities,
including nested, alternative and binding-condition patterns.

Constructor type arguments come from payloads, body constraints, an explicit call
such as `Err::<i32, String>("missing")`, or an explicit owner such as
`Result<i32, String>::Err("missing")`. Missing unconstrained arguments
are diagnosed: give `val outcome: Result<i32, String> = Ok(42)` an annotation when
there is no surrounding expected type. Constructors are not first-class functions.

The postfix `?` evaluates its operand once. `Some(v)` / `Ok(v)` produce `v`;
`None` / `Err(e)` return failure from the nearest function or closure.
Option propagates only into Option; Result propagates only into Result.
An operand `Result<T, E>` requires an enclosing `Result<U, F>` with `F: From<E>`.
Identical errors use the built-in identity conversion. Different errors call the
selected standard `From` implementation once on Err; Ok never calls it.
Their success types may differ. The return context of
a closure is independent of its enclosing function. Postfix chaining such as
`read()?.field` and `nested??` is supported.

The conversion uses one direct `From` implementation or generic bound. It does not
search conversion chains or fall back to `TryFrom`, and does not add conversions
to ordinary assignments, arguments or returns. Source errors are inferred before
conversion selection; an unconstrained error uses the enclosing error as a
fallback. An unconstrained closure error can likewise use the source error.
An independently constrained error is never changed to make a conversion fit.
Conversion runs in an ordinary script frame, sharing root cancellation and call-depth limits.
Its effects are not rolled back if it traps; traps are not converted into Err.
The outer failure retains the original Err metadata after its payload changes.
Propagation uses the ordinary protocols described below. There is no
`throw`/`try`/`catch` or built-in Error value. New Err captures
its source and stack; propagation preserves it. See
[error reporting](error-reporting.md).
See [failure semantics](failure-semantics.md) for traps and termination, which
`?` cannot intercept, and the [executable example](../../examples/syntax/result-option.kgr).

### Propagation protocols

`core::ops` owns the following ordinary registered declarations, also re-exported
by `std::ops`:

```kagari
pub enum ControlFlow<B, C> { Break(B), Continue(C) }
pub trait FromResidual<R> {
    fn from_residual(residual: R) -> Self;
}
pub trait Try: FromResidual<Self::Residual> {
    type Output;
    type Residual;
    fn from_output(output: Self::Output) -> Self;
    fn branch(self) -> ControlFlow<Self::Residual, Self::Output>;
}
```

For an operand of type A and an enclosing return type R, `?` requires `A: Try`
and `R: FromResidual<A::Residual>`. Its expression type is `A::Output`.
The operand and selected `branch` execute once. Continue yields its payload;
Break calls the selected `from_residual` once and returns from the nearest function
or closure. Traps, cancellation and completed effects follow ordinary call rules.
The compiler records checked method/signature/variant identities; executable
loading validates these selections without source parsing or trait inference.

| Carrier | Output | Residual | Accepted return carrier |
| --- | --- | --- | --- |
| Option<T> | T | Option<Infallible> | Option<U> |
| Result<T, E> | T | Result<Infallible, E> | Result<U, F> where F: From<E> |
| ControlFlow<B, C> | C | ControlFlow<B, Infallible> | ControlFlow<B, D> |

Local source structs/enums and registered native nominal types can implement the
same traits under the existing ownership, coherence and associated-output rules.
Distinct residual types prevent automatic cross-carrier conversion. Explicit custom
FromResidual implementations may accept another residual. Generic functions use
bounds such as `A: Try` and `R: FromResidual<A::Residual>`; an Output constraint
or return annotation is required when inference otherwise has no unique type.
Dynamic interface propagation, generic defaults, try blocks and conversion-chain
search are outside the implemented contract. Missing bounds and incompatible
carriers produce bound diagnostics before execution.

See [try-protocols.kgr](../../examples/syntax/try-protocols.kgr) for a custom
carrier, generic propagation and ControlFlow.

They support:

- construction through variants
- pattern matching
- type checking as ordinary generic enums
- reflection metadata through declared reflection metadata

They are not magic control-flow constructs.

## Primitive operations and formatting

Checked numeric operations, comparisons, bitwise operators and explicit numeric
casts follow [value semantics](value-semantics.md). No implicit numeric conversion
is introduced. Repetition evaluates its element and count once, from left to right,
and requires a repeatable value type even for an empty result.

String is an immutable value. Ordinary cooked strings support
[identifier-only interpolation](syntax.md#interpolated-strings): `{name}` selects
Display and `{name:?}` selects Debug through checked formatting contracts. Each
occurrence reads then formats its value before the next name is read, including
repeated names and statics with lazy initialization. Callbacks preserve normal
effects and failure behavior. Raw strings never interpolate; both cooked and raw
strings support the [multiline literal rules](syntax.md#string-literals).
The optional predecessor query and parse helpers are not implicitly restored.

The inherent String surface is len/is_empty (UTF-8 bytes), literal
contains/starts_with/ends_with/find, checked slice(start, end), Unicode whitespace
trim/trim_start/trim_end, literal replace, eager split returning List<String>, and
explicit lazy chars/char_indices/bytes traversal.
len returns u64, find returns Option<u64>, and slice takes u64 byte offsets.
Slice uses a half-open byte range and rejects reversed,
out-of-range and non-boundary offsets. Empty patterns match at Unicode scalar
boundaries, including both ends: find returns zero, replace inserts there, and
split retains endpoint empty fields. A nonempty separator on empty input produces
one empty string. Inputs remain immutable. Ordinary native bodies implement these
language-owned signatures; no String trait, regex or locale API is added. The slice
trap is IndexOutOfBounds for all invalid byte ranges, including UTF-8 boundaries.

Unqualified compiler helpers such as print and reflection names are consulted only
after lexical and declared names. A same-named function is an ordinary script call;
a non-callable local is a call-target error. Logging requires an explicitly declared
and installed host.log binding. Reflection remains subject to declared member access
and metadata; it is not how native libraries dispatch their methods.

### Explicit String traversal

String supplies three lazy methods using the ordinary Iterator/Iterable protocols:

| Method | Item | Meaning |
| --- | --- | --- |
| `text.chars()` | `char` | Unicode scalars in source order |
| `text.char_indices()` | `(u64, char)` | UTF-8 byte offset and scalar |
| `text.bytes()` | `u8` | UTF-8 encoded bytes in order |

For `"aé🙂"`, chars yields `'a'`, `'é'`, `'🙂'`; char_indices yields
`(0, 'a')`, `(1, 'é')`, `(3, '🙂')`; bytes yields seven bytes. A Unicode scalar
is not a grapheme cluster: a base letter and a combining mark remain separate
items. `text.chars().count()` counts scalars, while len, find and slice retain
the byte-based contracts above. String does not implement implicit iteration:
`for item in text` and integer String indexing are rejected. Select a traversal
method explicitly; its cursor supports the existing adapters and terminals.

Construction retains the immutable source without pulling an item or building an
intermediate List. Copies of one cursor share progress; each new chars,
char_indices or bytes call creates independent progress. Empty and exhausted
cursors return None. A retained cursor traces its source across GC and async
suspension, without retaining a Rust string borrow. Each native next previews an
item and publishes its result before committing progress: failed item/result
publication does not advance that cursor. This is not rollback of consumption
already completed before an adapter callback fails; the ordinary
[adapter rules](#iteration-protocols) still apply.

## Numeric scalar methods

The foundation installs ordinary inherent methods owned by canonical `core::num`;
calling them on primitive values requires no import. There is no parallel free
`math` namespace, compatibility alias catalog or generic `Float` trait. Operands
match their declared width and signedness exactly, without implicit conversion.
Receiver and arguments are evaluated once, left to right. The
[value-semantics contract](value-semantics.md#explicit-integer-arithmetic) owns
arithmetic outcomes, float edge cases, traps and precision.

### Integer inventory

For `T` in `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, the following
are instance methods. This is **17 policy variants across eight types, with 132
concrete bindings**: sixteen methods on each type plus `wrapping_add_signed` on
only the four unsigned types.

| Methods | Explicit arguments | Result |
| --- | --- | --- |
| `wrapping_add`, `wrapping_sub`, `wrapping_mul` | `rhs: T` | `T`, modulo 2^width |
| `checked_add`, `checked_sub`, `checked_mul`, `checked_div`, `checked_rem` | `rhs: T` | `Option<T>` |
| `overflowing_add`, `overflowing_sub`, `overflowing_mul` | `rhs: T` | `(T, bool)`, wrapped result and arithmetic overflow flag |
| `saturating_add`, `saturating_sub`, `saturating_mul` | `rhs: T` | `T`, saturated to its range |
| `rotate_left`, `rotate_right` | `count: u32` | `T`, count modulo bit width |
| `wrapping_add_signed` (unsigned only) | `rhs: i8/i16/i32/i64` for receiver `u8/u16/u32/u64`, respectively | Unsigned receiver type |

Ordinary operators remain checked and trapping, not wrapping or saturating.
Checked division/remainder return `None` for zero divisors and signed MIN / -1 or
MIN % -1. There are no additional wrapping division/remainder, shift or power
families implied by these names.

### Float inventory

For `F` in `f32`, `f64`, **15 utility names at each of two widths** give 30
bindings; **11 extended-math names at each width** give another 22 bindings.
All are instance methods except the explicitly static `from_bits` constructors.

| Methods | Explicit arguments | Result |
| --- | --- | --- |
| `abs`, `sqrt`, `floor`, `ceil`, `round`, `trunc`, `fract` | None | `F` |
| `min`, `max` | `other: F` | `F` |
| `clamp` | `min: F`, `max: F` | `F` |
| `is_nan`, `is_infinite`, `is_finite` | None | `bool` |
| `to_bits` | None | `u32` on `f32`; `u64` on `f64` |
| Static `f32::from_bits`, `f64::from_bits` | `bits: u32`, `bits: u64`, respectively | `f32`, `f64`, respectively |
| `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `exp`, `ln` | None | `F` |
| `atan2` | `other: F` (x; receiver is y) | `F` |
| `powf` | `exponent: F` | `F` |
| `powi` | `exponent: i32` at both widths | `F` |

See [floating-point semantics](value-semantics.md#floating-point-utilities-and-math)
for NaN-ignoring min/max, permitted equal-zero sign variation, clamp-bound
`ScriptTrap`, exact bit transport, power guards and the correctly rounded sqrt
versus unspecified extended-math precision boundary. Native associated constants
and compile-time evaluation of these native calls are not implemented. Existing
scalar const operations remain unchanged; public RNG APIs, full ray tracing and
JIT/native-backend expansion are outside this inventory.

## Ordering protocols

`Ordering` (also `core::cmp::Ordering`) has unit variants `Less`, `Equal`, `Greater`.
Type aliases (`use core::cmp::Ordering as Order`) and variant imports
(`use core::cmp::Ordering::*`) work in constructors and patterns.
`PartialOrd: PartialEq` declares `partial_cmp(self, other: Self) -> Option<Ordering>`.
`Ord: Eq + PartialOrd` declares `cmp(self, other: Self) -> Ordering`.
Comparison operators select PartialOrd; None makes each of `<`, `<=`, `>` and `>=`
false. Implementations must agree with equality and with each other. Primitive
integers, bool, char, unit, String and Ordering supply both; floats only PartialOrd.
There is no automatic Struct, Tuple or user-enum ordering. Custom Struct/enum
implementations use static calls and the ordinary failure/effect boundary.
See [ordering.kgr](../../examples/syntax/ordering.kgr).


## Arithmetic operator protocols

`core::ops::{Add, Sub, Mul, Div, Rem}` are explicitly imported traits with one explicit RHS
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

`core::ops::{Neg, Not}` declare `type Output` and `fn neg(self) -> Self::Output`
(respectively `not`). They control unary `-` and `!`; signed builtin numeric
negation and bool negation keep direct instructions. Integer Not complements
all bits of the declared width. Custom outputs may differ
from the receiver. `&&`/`||` remain bool-only short-circuit operators.


## Read-only indexing protocol

`core::ops::Index<I>` declares `type Output` and `fn index(self, rhs: I) ->
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

The compiler owns `core::convert::From<S>` with
`fn from(value: S) -> Self`. It supports explicit user-defined infallible conversions
and Result error propagation. Identity conversion preserves the same value or object
identity. Distinct error conversion runs once on Err, never on Ok, and cannot search
a chain of intermediate conversions. Conversion traps retain completed effects.

Into<T> is derived from T: From<Self>; TryInto<T> is derived from T: TryFrom<Self>,
including the same associated Error. Direct implementations of the reverse
contracts are rejected. TryFrom<S> declares `type Error` and
`fn try_from(value: S) -> Result<Self, Self::Error>`. FromStr declares `type Err`
and `fn from_str(text: String) -> Result<Self, Self::Err>`. Static and qualified
calls use ordinary checked trait selection, including generic receivers.

The runtime foundation implements FromStr for numeric scalars and bool, without
trimming input; errors use ParseError. Scalar TryFrom uses the existing checked
numeric conversion rules with Infallible for lossless cases and TryFromIntError
for narrowing integer cases. Optional helper methods are not required. Numeric
`as` conversions retain their language rules.

## Iteration protocols

FromIterator<T>, Sum<T> and Product<T> are core contracts. Their static methods
`from_iter`, `sum` and `product` each take `I: Iterable<Item = T>` and return Self.
The foundation supplies Vec, HashSet and HashMap construction and same-scalar
numeric aggregation. HashSet requires Item: Eq + Hash and deduplicates values.
HashMap requires pair items `(K, V)` with K: Eq + Hash; later values replace
earlier values for equal keys.
Empty sums return zero and empty products return one; integer overflow traps at
the declared scalar width. Custom destinations can implement these same contracts.

`core::iter::{Iterator, Iterable}` are prelude traits:

```kagari
trait Iterator {
    type Item;
    fn next(self) -> Option<Self::Item>;
}
trait Iterable {
    type Item;
    type Iter: Iterator<Item = Self::Item>;
    fn iter(self) -> Self::Iter;
}
```

Iterable supplies these default methods, with `T = Self::Item`. Iterator keeps
only next and obtains this same API through identity Iterable. Containers,
constrained generic receivers, custom Iterable implementations and interface
values support the same method chains; explicit overrides retain precedence.

| Method | Result |
| --- | --- |
| `map<U>(fn(T) -> U)` | `MapIterator<T, U>` |
| `filter(fn(T) -> bool)` | `FilterIterator<T>` |
| `filter_map<U>(fn(T) -> Option<U>)` | `FilterMapIterator<T, U>` |
| `take(u64)` / `skip(u64)` | `TakeIterator<T>` / `SkipIterator<T>` |
| `enumerate()` | `EnumerateIterator<T>`, Item = `(u64, T)` |
| `zip<I: Iterable>(I)` | `ZipIterator<T, I::Item>`, Item = `(T, I::Item)` |
| `chain<I: Iterable<Item = T>>(I)` | `ChainIterator<T>` |
| `collect<C: FromIterator<T>>()` | C |
| `fold<A>(A, fn(A, T) -> A)` | A |
| `sum<S: Sum<T>>()` / `product<P: Product<T>>()` | S / P |
| `count()` | u64 |
| `find(fn(T) -> bool)` | Option<T> |
| `any(fn(T) -> bool)` / `all(fn(T) -> bool)` | bool |
| `for_each(fn(T) -> ())` | () |

The adapter types belong to core::iter and have no public constructor. Calling a
transformation obtains each selected source cursor once, after left-to-right
argument evaluation; it does not advance a cursor or invoke a callback. Copies
share source progress and adapter state. An item stays consumed if its callback
fails. Filter and filter_map skip rejected items in source order. They do not
fuse a resumable source after None. Skip discards only its initial allowance;
take(0) never advances its source and a consumed take allowance remains exhausted.
Enumerate starts at zero and uses checked u64 increment. Take/skip allowances
remain u64 logical counts, not machine allocation sizes: a large allowance is
valid even when it exceeds the host's addressable storage.

Zip obtains and advances left before right. A left None prevents a right pull;
a right None leaves the already-pulled left item consumed. Zip does not rewind or
latch exhaustion. Chain switches permanently to its second cursor after the
first left None; it does not fuse the second cursor.

Terminal methods stop at the first None. Find, any and all short-circuit without
an extra pull. Empty find returns None, any returns false, all returns true,
count returns zero, for_each returns unit and fold returns its initial value.
Count uses checked u64 addition. Collect, sum and product call the requested
destination's existing constructor/reducer with the original source; they do not
obtain an unused extra cursor. Destination context or explicit method type
arguments must determine the result type, for example:

```kagari
val mapped: Vec<i32> = [1, 2, 3].map(|item| item + 1).collect();
val total: i32 = mapped.filter(|item| item > 2).sum();
```

`for pattern in expression` evaluates the expression once, calls `iter`
once, and repeatedly calls `next`. Some supplies the next item; None ends the
loop. Continue proceeds to the next call; break and return perform normal resource
cleanup. The next call is an ordinary script call for custom iterators, with the
same cancellation, GC roots, trap behavior and current implementation selection as
other ordinary methods.
An Iterator automatically implements identity Iterable, including under generic
bounds. It cannot also declare a conflicting Iterable implementation.
Custom iterables return an iterator whose Item agrees with their own Item.

The default containers implement Iterable with checked associated iterator
types, available without optional library adapters. The foundation's shared
`CollectionCursor<Item>` implements Iterator and identity Iterable.
The concrete `CollectionCursor<T>` type is distinct from the `Iterable::Iter` associated type:
an Iterable implementation may use `type Iter = CollectionCursor<T>` or select a custom iterator.
Array/Set items are elements, Map items are `(key, value)` tuples in the
concrete implementation's traversal order. String's
[explicit traversal methods](#explicit-string-traversal) return cursors, rather
than making String itself Iterable.
Iter construction retains the source and reads each slot on demand, without
copying all items. Contained objects retain their identity. Copying a cursor shares
its position; converting a collection again creates independent progress.
A custom iterator owns its state and consistency rules; no automatic clone,
reset, exact-length or fused-iterator promise is imposed on its implementation.

Native collection cursors record their source's structural revision. Structural
edits through any alias commit normally and invalidate existing cursors; the next
source read traps before yielding an item or reporting exhaustion. This includes
resuming after a loop break, callback, root call or async suspension. Revision
checks are deterministic within the isolate, not a cross-thread synchronization
mechanism. Length/key-set changes and sequence reordering invalidate cursors;
element replacement, replacing an existing Map value and inserting an already
present Set member do not. No-op removals do not invalidate a cursor.

Creating or retaining a cursor never blocks collection writes. Cursor lifetime
needs no close call or GC-triggered release. Adapters check sources only when
pulling them: take at its limit returns None without reading its source, and zip
with an exhausted left side does not read the right side. Short-circuit terminals
and break perform no extra reads. Already completed callback effects and writes
survive a later invalidation error; yielded values remain ordinary values.

```text
val values = Vec::from([1, 2]);
val cursor = values.iter();
values[0] = 10;       // Replacement is visible to cursor.next().
values.push(3);      // Commits and invalidates the existing cursor.
cursor.next();       // Traps: invalid or structurally modified iterator.
```

Native payloads trace retained source cursors and callbacks through NativePayload::trace.
Roots, checked call routes and exact retained computation bodies preserve GC and
reload safety independently of
iteration validity. Native storage borrows, sorting/mutation commits and hash
callback guards remain enforced. Custom iterators own their consistency policy;
the Iterator/Iterable traits do not impose a universal collection lock.

Host retention uses the normal rooted-value API. Cursor handles are runtime-owned,
generation checked, and trace their source, callbacks and adapter state. Ordinary
callback calls follow current routes; created closures retain exact bodies. Cursors
are not transferable across runtimes. They provide no
Eq/Hash, serialization of execution state, or script constructor.
See [iterators.kgr](../../examples/syntax/iterators.kgr).

## Host boundaries and metadata

Filesystem, network, timers, logging sinks and application services are host APIs.
The native library mechanism does not implicitly grant them authority. Host-owned
Rust state remains outside the script heap and uses declared typed access paths.

Executable contracts carry checked identities, signatures, associated outputs,
selected callables and concrete layouts. Metadata supports verification, interface
dispatch, reload, tooling and reflection within declared policy. Source-free artifact
loading validates these contracts against installed native modules. Backends
consume checked executable facts and may fall back before entering unsupported
functions; interpreter fallback is not a native-code performance claim.
