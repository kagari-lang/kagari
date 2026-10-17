# Language Foundation and Native Libraries

The installed `core`/`alloc`/`std` foundation defines complete language types and
protocols. Its 26 language traits and 15 other foundation traits are explicit
standard-library registrations. Their generated source is parsed and checked
against the installed declarations and validated language roles. Runtime supplies their basic checked native implementations. Optional
library modules and application modules use the same explicit registration API;
see [native declarations](standard-declarations.md). Generated `.kgr` serves tooling
and is not an executable standard library.

The foundation is always installed. List/MutableList own common algorithm methods,
String owns its inherent methods, and `core::iter::Iterable` owns the finite
lazy transformation and terminal API. The
[collection contract](collection-access.md) defines the finite API surface.
Predecessor APIs outside that inventory, including
numeric convenience methods, enum combinators, extended iterator algorithms,
collection snapshots and additional container classes, remain withdrawn.

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
| `core::num` | ParseError, TryFromIntError and numeric native implementations |
| `core::option`, `core::result` | Option and Result |
| `core::future` | Cold single-drive Future |
| `alloc::string`, `alloc::vec` | String and Vec |
| `std::collections` | List/MutableList, Map/MutableMap, Set/MutableSet, HashMap and HashSet |

Corresponding `std::ops`, `std::cmp`, `std::hash`, `std::fmt`, `std::convert`,
`std::iter`, `std::str`, `std::default`, `std::num`, `std::option`, `std::result`, `std::future`, `std::string`
and `std::vec` expose the canonical declarations through checked re-exports.
The package/module spelling does not change Kagari's GC or shared-object semantics.
There is no installed `core::language` module or ArrayList declaration.

`std::prelude` explicitly exports Iterator, Iterable, FromIterator, PartialEq,
Eq, PartialOrd, Ord, From, Into, TryFrom, TryInto, Default, Fn, Option, Result, Some, None,
Ok, Err, String, Vec and Future. Iterable is a retained Kagari extension. Local declarations
and explicit imports take precedence over these implicit imports.
Operators use their checked language roles independently of name imports; writing
an explicit operator trait bound or implementation requires that name in scope.
Hash, Debug, Display, operator traits other than Fn, collection interfaces,
HashMap/HashSet, Ordering, ranges and error types require explicit imports.
New public library declarations do not automatically enter the prelude.
Generic operation requirements use [ordinary trait bounds](traits.md#operation-specific-bounds),
not compiler-recognized numeric constraint names.

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
a fresh empty Vec<T>, or None for Option<T>. Vec and Option do not require
T: Default. Other types need an explicit implementation; no tuple/array/map/set,
Result, host/view or implicit user-struct implementations are supplied.
Default does not zero-initialize arbitrary types or enable derive or enum default
selection. [Struct update](syntax.md#struct-update-expressions) is a separate ordinary
construction form; its base may use Default or any eligible struct expression.

## Core Builtin Types

The core type set includes:

- `()`
- `bool`
- signed integers: `i8`, `i16`, `i32`, `i64`, `isize`
- unsigned integers: `u8`, `u16`, `u32`, `u64`, `usize`
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

The numeric type names follow Rust spelling.
The semantics do not import Rust ownership or borrowing.

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

- unit, bool, integers and String use value equality and hashing;
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

String is an immutable value. String interpolation selects Display or Debug through
ordinary checked formatting contracts; formatting callbacks preserve normal
left-to-right evaluation, effects and failure behavior. The optional predecessor
query, parse and string-iterator helpers are not implicitly restored.

The inherent String surface is len/is_empty (UTF-8 bytes), literal
contains/starts_with/ends_with/find, checked slice(start, end), Unicode whitespace
trim/trim_start/trim_end, literal replace and eager split returning List<String>.
find returns Option<usize>; slice uses a half-open byte range and rejects reversed,
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

## Ordering protocols

`Ordering` (also `core::cmp::Ordering`) has unit variants `Less`, `Equal`, `Greater`.
Type aliases (`use core::cmp::Ordering as Order`) and variant imports
(`use core::cmp::Ordering::*`) work in constructors and patterns.
`PartialOrd: PartialEq` declares `partial_cmp(self, other: Self) -> Option<Ordering>`.
`Ord: Eq + PartialOrd` declares `cmp(self, other: Self) -> Ordering`.
Comparison operators select PartialOrd; None makes each of `<`, `<=`, `>` and `>=`
false. Implementations must agree with equality and with each other. Primitive
integers, bool, unit, String and Ordering supply both; floats only PartialOrd.
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
| `take(usize)` / `skip(usize)` | `TakeIterator<T>` / `SkipIterator<T>` |
| `enumerate()` | `EnumerateIterator<T>`, Item = `(usize, T)` |
| `zip<I: Iterable>(I)` | `ZipIterator<T, I::Item>`, Item = `(T, I::Item)` |
| `chain<I: Iterable<Item = T>>(I)` | `ChainIterator<T>` |
| `collect<C: FromIterator<T>>()` | C |
| `fold<A>(A, fn(A, T) -> A)` | A |
| `sum<S: Sum<T>>()` / `product<P: Product<T>>()` | S / P |
| `count()` | usize |
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
Enumerate starts at zero and uses checked usize increment.

Zip obtains and advances left before right. A left None prevents a right pull;
a right None leaves the already-pulled left item consumed. Zip does not rewind or
latch exhaustion. Chain switches permanently to its second cursor after the
first left None; it does not fuse the second cursor.

Terminal methods stop at the first None. Find, any and all short-circuit without
an extra pull. Empty find returns None, any returns false, all returns true,
count returns zero, for_each returns unit and fold returns its initial value.
Count uses checked usize addition. Collect, sum and product call the requested
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
same cancellation, GC roots, trap behavior and pinned code versions as other methods.
An Iterator automatically implements identity Iterable, including under generic
bounds. It cannot also declare a conflicting Iterable implementation.
Custom iterables return an iterator whose Item agrees with their own Item.

The default containers implement Iterable with checked associated iterator
types, available without optional library adapters. The foundation's shared
`CollectionCursor<Item>` implements Iterator and identity Iterable.
The concrete `CollectionCursor<T>` type is distinct from the `Iterable::Iter` associated type:
an Iterable implementation may use `type Iter = CollectionCursor<T>` or select a custom iterator.
Array/Set items are elements, Map items are `(key, value)` tuples in the
concrete implementation's traversal order. String traversal is not provided by
the current foundation.
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
Roots and pinned selected calls preserve GC and reload safety independently of
iteration validity. Native storage borrows, sorting/mutation commits and hash
callback guards remain enforced. Custom iterators own their consistency policy;
the Iterator/Iterable traits do not impose a universal collection lock.

Host retention uses the normal rooted-value API. Cursor handles are runtime-owned,
generation checked, and trace their source, callbacks and adapter state. They retain their
execution version; they are not transferable across runtimes. They provide no
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
