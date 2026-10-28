# Standard library API inventory

Status: standalone inventory requested on 2026-10-26. This is a signature-first
catalog of implemented APIs and proposed library coverage, not an execution plan,
implementation authorization or release promise. IndexMap/IndexSet and
BTreeMap/BTreeSet are requested expansion targets, not installed APIs.

The [roadmap](implementation-roadmap.md) owns activation and sequencing. The
[GC execution plan](paged-incremental-gc-design.md#execution-plan) and
[native collections plan](native-collections-plan.md#sequencing-and-phases) retain
their scope and ledgers. Their external BTreeMap<i32, T> fixture is not a production
generic BTreeMap. This inventory neither enlarges PG/NC acceptance nor reopens
cancellation-policy design.

## How to read the signatures

- **Implemented**: present in current declarations, or explicitly identified below
  as a language/host facility. It does not mean a new test matrix was run.
- **Draft — gap**: proposed completion of an existing family; not callable today.
- **Draft — requested**: the requested Index/BTree products; signatures and semantics
  still need approval.
- **Draft — candidate**: optional product to evaluate, not mandatory foundation.
- **Host-provided**: requires an explicitly installed service, not ambient authority.
- **Deferred**: scope recorded without inventing a settled API.

Signature blocks use **normalized interface notation**, not compilable Kagari source.
`Type<T> { ... }` groups methods by owner; `trait` lists interface requirements;
`self` is the receiver, and its absence denotes a static operation. Parameter names
are descriptive, not necessarily the registration's generated names. `where` states
bounds; associated-type equalities describe checked capabilities. `fn(A) -> B` is
callback notation. No Rust references, borrowing, moves or trait-object restrictions
are implied. Heading groups are topics, not newly approved module paths.

Lengths, indices and capacities use `u64`, not host `usize`. Returned objects may
share identity; copying a container does not deep-copy its elements. A displayed
method on a trait is available through that capability; concrete return-type
exceptions are shown separately. Proposed cursor/error types are explicitly named
sketches, not existing declarations.

### Contents

1. [Protocols and conversions](#1-protocols-and-conversions)
2. [Option, Result and ControlFlow](#2-option-result-and-controlflow)
3. [Numbers and booleans](#3-numbers-and-booleans)
4. [Strings, characters and bytes](#4-strings-characters-and-bytes)
5. [Collection interfaces](#5-collection-interfaces)
6. [Array, Vec, HashMap and HashSet](#6-array-vec-hashmap-and-hashset)
7. [IndexMap and IndexSet](#7-indexmap-and-indexset)
8. [BTreeMap and BTreeSet](#8-btreemap-and-btreeset)
9. [Other collection candidates](#9-other-collection-candidates)
10. [Iteration, algorithms and grouping](#10-iteration-algorithms-and-grouping)
11. [Ranges](#11-ranges)
12. [Async, time and randomness](#12-async-time-and-randomness)
13. [Formats and optional packages](#13-formats-and-optional-packages)
14. [Host services and tooling](#14-host-services-and-tooling)
15. [Implementation boundaries](#15-implementation-boundaries)

### Authority and installation

Current signatures were checked against the
[module contributors](../crates/kagari-stdlib/src/foundation.rs),
[composition/prelude](../crates/kagari-stdlib/src/assembly/composition.rs),
[declaration catalog](../crates/kagari-stdlib/src/catalog/mod.rs) and the owners
linked below. [Standard declarations](spec/standard-declarations.md),
[builtins](spec/builtins.md), [collection access](spec/collection-access.md) and
[value semantics](spec/value-semantics.md) own their detailed contracts. An internal
Rust container or an old prospective name is not evidence of an installed API.

Keep existing canonical core/alloc owners and identity-preserving std facades.
The SDK installs the foundation; low-level Runtime consumers install native
modules explicitly. Additional pure modules may be optional. Clocks, entropy,
logging, IO and application services require host providers. New products do not
automatically enter the prelude. These names do not imply Rust no_std semantics.

## 1. Protocols and conversions

**Implemented** — [protocol declarations](../crates/kagari-stdlib/src/foundation/protocols.rs).

```text
trait PartialEq {
    eq(self, other: Self) -> bool
}
trait Eq: PartialEq {}
trait PartialOrd: PartialEq {
    partial_cmp(self, other: Self) -> Option<Ordering>
}
trait Ord: Eq + PartialOrd {
    cmp(self, other: Self) -> Ordering
}
Ordering = Less | Equal | Greater

trait Hash { hash(self) -> i64 }
trait Debug { debug(self) -> String }
trait Display { display(self) -> String }
trait Default { default() -> Self }

trait From<S> { from(value: S) -> Self }
trait Into<T> { into(self) -> T }
trait TryFrom<S> {
    type Error
    try_from(value: S) -> Result<Self, Self::Error>
}
trait TryInto<T> {
    type Error
    try_into(self) -> Result<T, Self::Error>
}
trait FromStr {
    type Err
    from_str(text: String) -> Result<Self, Self::Err>
}
```

`Into` is derived from `From`, and `TryInto` from `TryFrom`; direct implementations
of the reverse traits are rejected. `Infallible` represents impossible conversion
failure. Implementations are type-specific: the existence of Default or FromStr
does not give every type that capability.

**Implemented** — operator/call/index protocols. Each declares `type Output`;
this compact listing shows its method, not a single combined trait.

```text
Add<Rhs>      add(self, rhs: Rhs) -> Self::Output
Sub<Rhs>      sub(self, rhs: Rhs) -> Self::Output
Mul<Rhs>      mul(self, rhs: Rhs) -> Self::Output
Div<Rhs>      div(self, rhs: Rhs) -> Self::Output
Rem<Rhs>      rem(self, rhs: Rhs) -> Self::Output
BitAnd<Rhs>   bitand(self, rhs: Rhs) -> Self::Output
BitOr<Rhs>    bitor(self, rhs: Rhs) -> Self::Output
BitXor<Rhs>   bitxor(self, rhs: Rhs) -> Self::Output
Shl<Rhs>      shl(self, rhs: Rhs) -> Self::Output
Shr<Rhs>      shr(self, rhs: Rhs) -> Self::Output
Neg           neg(self) -> Self::Output
Not           not(self) -> Self::Output
Index<I>      index(self, index: I) -> Self::Output
Fn<Args>      call(self, args: Args) -> Self::Output
```

`Args` is a tuple. Identity operators remain distinct from value equality. User
structs/enums do not receive an implicit identity-based Eq/Hash implementation.

**Draft — gap** — comparison conveniences:

```text
Ordering {
    is_eq(self) -> bool
    is_lt(self) -> bool
    is_gt(self) -> bool
    reverse(self) -> Ordering
    then(self, other: Ordering) -> Ordering
    then_with(self, next: fn() -> Ordering) -> Ordering
}
min<T: Ord>(left: T, right: T) -> T
max<T: Ord>(left: T, right: T) -> T
clamp<T: Ord>(value: T, min: T, max: T) -> T
```

`then_with` would invoke its callback only for Equal. Invalid clamp bounds require
an explicit failure rule. **Deferred:** derive authoring, universal clone/deep-copy,
and native associated constants/const evaluation. Do not add protocols merely to
match Rust's trait count or simulate constants through compiler-recognized names.

## 2. Option, Result and ControlFlow

**Implemented** — [enums](../crates/kagari-stdlib/src/foundation/enumerations.rs)
and [propagation](../crates/kagari-stdlib/src/foundation/propagation.rs):

```text
Option<T> = Some(T) | None
Result<T, E> = Ok(T) | Err(E)
ControlFlow<B, C> = Break(B) | Continue(C)

Option<T> { default() -> Option<T> }

trait FromResidual<R> {
    from_residual(residual: R) -> Self
}
trait Try: FromResidual<Self::Residual> {
    type Output
    type Residual
    from_output(output: Self::Output) -> Self
    branch(self) -> ControlFlow<Self::Residual, Self::Output>
}
```

Option defaults to None without `T: Default`. Result has no general default.
Matching and checked propagation exist; the following combinators do **not** follow
from that fact. Preserve retained Err diagnostic origin.

**Draft — gap** — Option conveniences:

```text
Option<T> {
    is_some(self) -> bool
    is_none(self) -> bool
    is_some_and(self, predicate: fn(T) -> bool) -> bool
    map<U>(self, transform: fn(T) -> U) -> Option<U>
    and_then<U>(self, transform: fn(T) -> Option<U>) -> Option<U>
    filter(self, predicate: fn(T) -> bool) -> Option<T>
    or_else(self, fallback: fn() -> Option<T>) -> Option<T>
    unwrap_or(self, fallback: T) -> T
    unwrap_or_else(self, fallback: fn() -> T) -> T
    unwrap_or_default(self) -> T where T: Default
    ok_or<E>(self, error: E) -> Result<T, E>
    ok_or_else<E>(self, error: fn() -> E) -> Result<T, E>
}
Option<Option<T>> { flatten(self) -> Option<T> }
Option<Result<T, E>> { transpose(self) -> Result<Option<T>, E> }
```

**Draft — gap** — Result and ControlFlow conveniences:

```text
Result<T, E> {
    is_ok(self) -> bool
    is_err(self) -> bool
    is_ok_and(self, predicate: fn(T) -> bool) -> bool
    is_err_and(self, predicate: fn(E) -> bool) -> bool
    map<U>(self, transform: fn(T) -> U) -> Result<U, E>
    map_err<F>(self, transform: fn(E) -> F) -> Result<T, F>
    and_then<U>(self, transform: fn(T) -> Result<U, E>) -> Result<U, E>
    or_else<F>(self, recover: fn(E) -> Result<T, F>) -> Result<T, F>
    unwrap_or(self, fallback: T) -> T
    unwrap_or_else(self, fallback: fn(E) -> T) -> T
    unwrap_or_default(self) -> T where T: Default
    ok(self) -> Option<T>
    err(self) -> Option<E>
}
Result<Result<T, E>, E> { flatten(self) -> Result<T, E> }
Result<Option<T>, E> { transpose(self) -> Option<Result<T, E>> }

ControlFlow<B, C> {
    is_break(self) -> bool
    is_continue(self) -> bool
    break_value(self) -> Option<B>
    continue_value(self) -> Option<C>
    map_break<D>(self, transform: fn(B) -> D) -> ControlFlow<D, C>
    map_continue<D>(self, transform: fn(C) -> D) -> ControlFlow<B, D>
}
```

Fallback values are eager arguments; fallback callbacks are lazy. Callbacks run
only for the selected branch, once. **Candidate:** `unwrap(self) -> T` and
`expect(self, message: String) -> T` on Option/Result, plus Result error-side
extraction. These would trap on the wrong variant, not turn faults into Result.
A standard Error display/source-chain protocol remains a separate design choice.

## 3. Numbers and booleans

Current owner: [numeric library](../crates/kagari-stdlib/src/foundation/numeric.rs).
`I` below means each concrete integer type: i8/i16/i32/i64/u8/u16/u32/u64.
`F` means f32 or f64. These are catalog abbreviations, **not numeric traits**.

**Implemented** — in addition to checked language arithmetic:

```text
I {
    wrapping_add(self, rhs: I) -> I
    wrapping_sub(self, rhs: I) -> I
    wrapping_mul(self, rhs: I) -> I
    checked_add(self, rhs: I) -> Option<I>
    checked_sub(self, rhs: I) -> Option<I>
    checked_mul(self, rhs: I) -> Option<I>
    checked_div(self, rhs: I) -> Option<I>
    checked_rem(self, rhs: I) -> Option<I>
    overflowing_add(self, rhs: I) -> (I, bool)
    overflowing_sub(self, rhs: I) -> (I, bool)
    overflowing_mul(self, rhs: I) -> (I, bool)
    saturating_add(self, rhs: I) -> I
    saturating_sub(self, rhs: I) -> I
    saturating_mul(self, rhs: I) -> I
    rotate_left(self, count: u32) -> I
    rotate_right(self, count: u32) -> I
}
U { wrapping_add_signed(self, rhs: S) -> U }
```

Here U/S are corresponding unsigned/signed widths, for example u32/i32.

```text
F {
    abs(self) -> F
    sqrt(self) -> F
    floor(self) -> F
    ceil(self) -> F
    round(self) -> F
    trunc(self) -> F
    fract(self) -> F
    min(self, other: F) -> F
    max(self, other: F) -> F
    clamp(self, min: F, max: F) -> F
    is_nan(self) -> bool
    is_infinite(self) -> bool
    is_finite(self) -> bool
    sin(self) -> F
    cos(self) -> F
    tan(self) -> F
    asin(self) -> F
    acos(self) -> F
    atan(self) -> F
    atan2(self, other: F) -> F
    exp(self) -> F
    ln(self) -> F
    powf(self, exponent: F) -> F
    powi(self, exponent: i32) -> F
}
f32 { to_bits(self) -> u32; from_bits(bits: u32) -> f32 }
f64 { to_bits(self) -> u64; from_bits(bits: u64) -> f64 }
```

Numeric types and bool implement FromStr with ParseError; parsing does not
implicitly trim. Potentially failing integer conversions use TryFromIntError;
lossless TryFrom conversions use Infallible. Floats retain IEEE-style PartialEq
and do not automatically implement Eq/Hash.

**Draft — gap** — integer utility surface, expanded per applicable concrete type:

```text
I {
    min(self, other: I) -> I
    max(self, other: I) -> I
    clamp(self, min: I, max: I) -> I
    checked_pow(self, exponent: u32) -> Option<I>
    checked_div_euclid(self, rhs: I) -> Option<I>
    checked_rem_euclid(self, rhs: I) -> Option<I>
    checked_ilog(self, base: I) -> Option<u32>
    count_ones(self) -> u32
    count_zeros(self) -> u32
    leading_zeros(self) -> u32
    trailing_zeros(self) -> u32
    swap_bytes(self) -> I
    to_le_bytes(self) -> [u8]
    to_be_bytes(self) -> [u8]
    from_le_bytes(bytes: [u8]) -> Result<I, ByteLengthError>
    from_be_bytes(bytes: [u8]) -> Result<I, ByteLengthError>
    from_str_radix(text: String, radix: u32) -> Result<I, ParseError>
    to_str_radix(self, radix: u32) -> String
}
S {
    checked_abs(self) -> Option<S>
    signum(self) -> S
    is_positive(self) -> bool
    is_negative(self) -> bool
}
```

S denotes signed integers. ByteLengthError is a proposed error; `[u8]` does not
encode the width in its type. Radix ranges and invalid formatting radix behavior
must be specified. ParseError::InvalidRadix is not evidence of a current parser.

**Draft — gap/candidate** — floating and boolean utilities:

```text
F {
    total_cmp(self, other: F) -> Ordering
    signum(self) -> F
    is_sign_positive(self) -> bool
    is_sign_negative(self) -> bool
    is_normal(self) -> bool
    is_subnormal(self) -> bool
    hypot(self, other: F) -> F
    exp2(self) -> F
    log2(self) -> F
    log10(self) -> F
    to_radians(self) -> F
    to_degrees(self) -> F
    sinh(self) -> F
    cosh(self) -> F
    tanh(self) -> F
    mul_add(self, multiplier: F, addend: F) -> F
}
bool {
    then<T>(self, make: fn() -> T) -> Option<T>
    then_some<T>(self, value: T) -> Option<T>
}
```

Total comparison would not change float equality/hash contracts. Precision,
NaN/signed-zero and overflow policies remain explicit. Decimal, big integer and
rational types are optional candidates, not automatic arithmetic promotion.

## 4. Strings, characters and bytes

**Implemented** — [String](../crates/kagari-stdlib/src/foundation/string.rs):

```text
String {
    default() -> String
    len(self) -> u64
    is_empty(self) -> bool
    contains(self, pattern: String) -> bool
    starts_with(self, prefix: String) -> bool
    ends_with(self, suffix: String) -> bool
    find(self, pattern: String) -> Option<u64>
    slice(self, start: u64, end: u64) -> String
    trim(self) -> String
    trim_start(self) -> String
    trim_end(self) -> String
    replace(self, from: String, to: String) -> String
    split(self, separator: String) -> List<String>
    chars(self) -> CollectionCursor<char>
    char_indices(self) -> CollectionCursor<(u64, char)>
    bytes(self) -> CollectionCursor<u8>
}
```

String is immutable UTF-8. Length/search/slice indices are byte offsets; slice
checks boundaries and copies today. `split` is eager. Explicit traversal methods
are lazy; String itself has neither implicit Iterable nor indexing.

**Implemented** — [char](../crates/kagari-stdlib/src/foundation/character.rs):
Unicode scalar comparison/hash/default; `From<char>` for u32 and String;
`TryFrom<u32>` for char with CharTryFromError. Display/Debug and identifier
interpolation already provide basic formatting. String + String is not currently
implemented; adjacent identifier interpolation such as `"{left}{right}"` is available.

**Draft — gap** — string concatenation through the ordinary Add protocol:

```text
impl Add<String> for String {
    type Output = String
    add(self, rhs: String) -> String
}

"AAA" + "BBB" -> "AAABBB"
```

Concatenation returns a new immutable String without modifying either operand.
Both operands must be String; this does not introduce implicit formatting of other
values. Preserve left-to-right, once-only operand evaluation. The implementation
should construct the result in O(left.len() + right.len()) time and output space,
with checked combined length and ordinary allocation-failure handling. Repeated
concatenation in a loop can still copy a growing prefix on each iteration; this
is not a substitute for the candidate builder below. No special compiler operator
or GC container mechanism is required beyond the selected Add implementation.

**Draft — gap** — other text utilities:

```text
String {
    parse<T: FromStr>(self) -> Result<T, T::Err>
    rfind(self, pattern: String) -> Option<u64>
    split_once(self, separator: String) -> Option<(String, String)>
    rsplit_once(self, separator: String) -> Option<(String, String)>
    splitn(self, limit: u64, separator: String) -> List<String>
    lines(self) -> List<String>
    split_whitespace(self) -> List<String>
    strip_prefix(self, prefix: String) -> Option<String>
    strip_suffix(self, suffix: String) -> Option<String>
    to_ascii_lowercase(self) -> String
    to_ascii_uppercase(self) -> String
    to_lowercase(self) -> String
    to_uppercase(self) -> String
    repeat(self, count: u64) -> String
    from_utf8<I: Iterable<Item = u8>>(bytes: I) -> Result<String, Utf8Error>
    from_utf8_lossy<I: Iterable<Item = u8>>(bytes: I) -> String
}
char {
    is_ascii(self) -> bool
    is_ascii_digit(self) -> bool
    is_alphabetic(self) -> bool
    is_alphanumeric(self) -> bool
    is_numeric(self) -> bool
    is_whitespace(self) -> bool
    is_control(self) -> bool
    is_lowercase(self) -> bool
    is_uppercase(self) -> bool
    to_digit(self, radix: u32) -> Option<u32>
    from_digit(digit: u32, radix: u32) -> Option<char>
    len_utf8(self) -> u64
    len_utf16(self) -> u64
    to_lowercase(self) -> String
    to_uppercase(self) -> String
}
```

Utf8Error is proposed. Unicode case mapping can expand into several scalars,
hence String results for char mappings. New eager splitting matches the current
split contract; a streaming alternative needs a distinct return type/contract.

**Draft — candidate** — construction without repeated concatenation:

```text
StringBuilder {
    new() -> StringBuilder
    with_capacity(bytes: u64) -> StringBuilder
    push_str(self, text: String) -> ()
    push_char(self, value: char) -> ()
    clear(self) -> ()
    to_string(self) -> String
}
```

The candidate builder's result would be an immutable snapshot, not an escaping
borrow; aliases keep referring to the builder. Rich format specifications and a
writer abstraction need separate selection. `[u8]` and compact Vec<u8> already
provide byte storage; no separate Bytes type is implied. Checked byte readers,
endian access and codecs may build on these. **Deferred:** zero-copy shared views,
normalization, graphemes, locale/collation and regex. Byte, scalar and grapheme
units must never be interchanged implicitly.

## 5. Collection interfaces

**Implemented** — [collection protocols](../crates/kagari-stdlib/src/foundation/protocols/collections.rs).
These are shared capabilities, not storage layouts. Readonly interfaces are live,
shallow views and do not prohibit mutation through another alias.

```text
trait List<T>: Index<u64, Output = T> + Iterable<Item = T> {
    len(self) -> u64
    is_empty(self) -> bool
    get(self, index: u64) -> Option<T>
}
trait MutableList<T>: List<T> {
    push(self, value: T) -> ()
    pop(self) -> Option<T>
    insert(self, index: u64, value: T) -> ()
    remove(self, index: u64) -> Option<T>
    clear(self) -> ()
    set(self, index: u64, value: T) -> ()
}
trait Map<K, V>: Iterable<Item = (K, V)> {
    len(self) -> u64
    is_empty(self) -> bool
    contains_key(self, key: K) -> bool
    get(self, key: K) -> Option<V>
}
trait MutableMap<K, V>: Map<K, V> {
    insert(self, key: K, value: V) -> ()
    remove(self, key: K) -> Option<V>
    clear(self) -> ()
}
trait Set<T>: Iterable<Item = T> {
    len(self) -> u64
    is_empty(self) -> bool
    contains(self, value: T) -> bool
}
trait MutableSet<T>: Set<T> {
    insert(self, value: T) -> ()
    remove(self, value: T) -> bool
    clear(self) -> ()
}
```

Map/Set interfaces do **not** require Hash or Ord; concrete storage selects its
membership requirements. Concrete fluent mutators below differ from interface
unit-returning methods. Do not silently substitute Rust's insert return types.

**Draft — gap** — shared conveniences; final default-method versus extension
ownership is not selected:

```text
List<T> {
    first(self) -> Option<T>
    last(self) -> Option<T>
    binary_search(self, value: T) -> Result<u64, u64> where T: Ord
    binary_search_by(self, compare: fn(T) -> Ordering) -> Result<u64, u64>
    binary_search_by_key<K: Ord>(self, key: K, project: fn(T) -> K)
        -> Result<u64, u64>
    partition_point(self, predicate: fn(T) -> bool) -> u64
    chunks(self, size: u64) -> Iterator<List<T>>
    windows(self, size: u64) -> Iterator<List<T>>
}
Map<K, V> {
    keys(self) -> Iterator<K>
    values(self) -> Iterator<V>
    get_key_value(self, key: K) -> Option<(K, V)>
}
MutableMap<K, V> {
    get_or_insert_with(self, key: K, make: fn() -> V) -> V
    retain(self, keep: fn(K, V) -> bool) -> ()
    extend<I: Iterable<Item = (K, V)>>(self, source: I) -> ()
}
Set<T> {
    is_subset(self, other: Set<T>) -> bool
    is_superset(self, other: Set<T>) -> bool
    is_disjoint(self, other: Set<T>) -> bool
    union(self, other: Set<T>) -> Iterator<T>
    intersection(self, other: Set<T>) -> Iterator<T>
    difference(self, other: Set<T>) -> Iterator<T>
    symmetric_difference(self, other: Set<T>) -> Iterator<T>
}
```

In **draft blocks only**, `Iterator<T>` abbreviates a result implementing
`Iterator<Item = T>`; it is not a new generic form of the installed Iterator trait.
The eventual declaration may name a concrete adapter or a checked interface.

Binary search's Ok is a found position, Err an insertion position, not an engine
failure. Sorted/partitioned input is required. Draft chunks/windows copy each
emitted container structure, sharing elements; zero size would be rejected. This
is not a zero-copy view or a promise of O(1) work per copied window.

Draft set algebra is lazy, using the participating sets' membership semantics;
union emits the left traversal then right-only items, intersection/difference use
left order, symmetric difference emits left-only then right-only. Unspecified
source order stays unspecified. Agreement of membership semantics, mutation
invalidation and resulting complexity need approval. Do not add an unnecessary
Hash bound to generic Set.

## 6. Array, Vec, HashMap and HashSet

### Array: fixed length

**Implemented** — [Array](../crates/kagari-stdlib/src/foundation/arrays.rs):

```text
[T] {
    len(self) -> u64
    is_empty(self) -> bool
    index(self, index: u64) -> T
    iter(self) -> CollectionCursor<T>
    eq(self, other: [T]) -> bool where T: PartialEq
}
```

Construction uses literal/repeat syntax; indexed assignment is language syntax.
Array is Iterable, not List/MutableList. It has no installed new/from/default/
from_iter/get/set method surface and no Eq/Hash implementation.

### Vec: growable sequence and stack

**Implemented** — [Vec](../crates/kagari-stdlib/src/foundation/vector.rs):
implements MutableList, Iterable and the algorithms in section 10.

```text
Vec<T> {
    new() -> Vec<T>
    from(array: [T]) -> Vec<T>
    default() -> Vec<T>
    from_iter<I: Iterable<Item = T>>(source: I) -> Vec<T>
    iter(self) -> CollectionCursor<T>

    push(self, value: T) -> Vec<T>
    insert(self, index: u64, value: T) -> Vec<T>
    set(self, index: u64, value: T) -> Vec<T>
    clear(self) -> Vec<T>
}
```

`from(array)` is inherent, not an implementation of From<[T]>. Default needs no
`T: Default`. The four concrete mutators return the same receiver; the corresponding
interface methods return unit. Read/removal methods have the signatures in section 5.

**Draft — gap**:

```text
Vec<T> {
    with_capacity(capacity: u64) -> Vec<T>
    capacity(self) -> u64
    reserve(self, additional: u64) -> ()
    try_reserve(self, additional: u64) -> Result<(), ReserveError>
    shrink_to_fit(self) -> ()
    shrink_to(self, minimum_capacity: u64) -> ()
    from_fn(count: u64, make: fn(u64) -> T) -> Vec<T>
    truncate(self, len: u64) -> ()
    resize(self, len: u64, value: T) -> ()
    resize_with(self, len: u64, make: fn() -> T) -> ()
    extend<I: Iterable<Item = T>>(self, source: I) -> ()
    append(self, source: Vec<T>) -> ()
    split_off(self, at: u64) -> Vec<T>
    swap_remove(self, index: u64) -> Option<T>
}
```

ReserveError is proposed. New mutators above provisionally return unit; whether
to match older fluent methods is an explicit API review item. Draft append moves
source elements and empties the source; self-append and overlapping extend require
an explicit alias rule, not accidental infinite iteration. Resize with one object
shares that object; resize_with invokes a generator for each new element.

### HashMap and HashSet: unordered membership

**Implemented** — [hash collections](../crates/kagari-stdlib/src/foundation/hash.rs).
Other methods come from MutableMap/MutableSet and Iterable:

```text
HashMap<K: Eq + Hash, V> {
    new() -> HashMap<K, V>
    from_iter<I: Iterable<Item = (K, V)>>(source: I) -> HashMap<K, V>
    insert(self, key: K, value: V) -> HashMap<K, V>
    clear(self) -> HashMap<K, V>
    iter(self) -> CollectionCursor<(K, V)>
}
HashSet<T: Eq + Hash> {
    new() -> HashSet<T>
    from(array: [T]) -> HashSet<T>
    from_iter<I: Iterable<Item = T>>(source: I) -> HashSet<T>
    insert(self, value: T) -> HashSet<T>
    clear(self) -> HashSet<T>
    iter(self) -> CollectionCursor<T>
}
```

Insert/clear return the same receiver, **not** Rust's Option/bool insert result.
Map remove returns Option<V>; set remove returns bool. Traversal order is unspecified;
equal map keys keep the last inserted value. HashMap has no registered array from;
neither hash collection currently has Default. HashSet.from is inherent.

**Draft — gap**:

```text
HashMap<K: Eq + Hash, V> {
    default() -> HashMap<K, V>
    from(entries: [(K, V)]) -> HashMap<K, V>
    with_capacity(capacity: u64) -> HashMap<K, V>
    capacity(self) -> u64
    reserve(self, additional: u64) -> ()
    try_reserve(self, additional: u64) -> Result<(), ReserveError>
    shrink_to_fit(self) -> ()
}
HashSet<T: Eq + Hash> {
    default() -> HashSet<T>
    with_capacity(capacity: u64) -> HashSet<T>
    capacity(self) -> u64
    reserve(self, additional: u64) -> ()
    try_reserve(self, additional: u64) -> Result<(), ReserveError>
    shrink_to_fit(self) -> ()
    retain(self, keep: fn(T) -> bool) -> ()
    extend<I: Iterable<Item = T>>(self, source: I) -> ()
}
```

Capacity is a concrete-storage capability, not a universal Map/Set requirement.
Existing concrete Vec/HashMap/HashSet content PartialEq/Eq/Hash implementations are
conditional; membership bounds do not alone grant every content protocol.

## 7. IndexMap and IndexSet

**Draft — requested**, not implemented. Implement MutableMap/MutableSet and
Iterable, reusing the shared signatures rather than adding Runtime container cases.
To match existing concrete hash collections, these drafts use fluent insert/clear.

```text
IndexMap<K: Eq + Hash, V> {
    new() -> IndexMap<K, V>
    default() -> IndexMap<K, V>
    with_capacity(capacity: u64) -> IndexMap<K, V>
    from_iter<I: Iterable<Item = (K, V)>>(source: I) -> IndexMap<K, V>
    insert(self, key: K, value: V) -> IndexMap<K, V>
    clear(self) -> IndexMap<K, V>
    iter(self) -> Iterator<(K, V)>
    get_index(self, index: u64) -> Option<(K, V)>
    index_of_key(self, key: K) -> Option<u64>
    first(self) -> Option<(K, V)>
    last(self) -> Option<(K, V)>
    remove(self, key: K) -> Option<V>
    remove_index(self, index: u64) -> Option<(K, V)>
    swap_remove(self, key: K) -> Option<V>
    swap_remove_index(self, index: u64) -> Option<(K, V)>
}
IndexSet<T: Eq + Hash> {
    new() -> IndexSet<T>
    default() -> IndexSet<T>
    with_capacity(capacity: u64) -> IndexSet<T>
    from_iter<I: Iterable<Item = T>>(source: I) -> IndexSet<T>
    insert(self, value: T) -> IndexSet<T>
    clear(self) -> IndexSet<T>
    iter(self) -> Iterator<T>
    get_index(self, index: u64) -> Option<T>
    index_of(self, value: T) -> Option<u64>
    first(self) -> Option<T>
    last(self) -> Option<T>
    remove(self, value: T) -> bool
    remove_index(self, index: u64) -> Option<T>
    swap_remove(self, value: T) -> bool
    swap_remove_index(self, index: u64) -> Option<T>
}
```

Proposed behavior:

- New keys append; overwriting a value/duplicate insertion keeps the position and
  originally stored equal key/member. Remove then reinsert appends.
- Ordinary remove preserves order and may shift O(n) entries. Swap removal explicitly
  sacrifices order. Positions are not stable handles; get_index avoids ambiguity
  when K itself is an integer.
- Example: insert keys 30,10,20; replace 10; remove/reinsert 10. Orders are 30,10,20;
  30,10,20; then 30,20,10.
- Capacity/keys/values/retain/set algebra should follow the suitable shared drafts;
  sorting/move_index/swap_indices are later optional APIs, not initial requirements.
- Prefer order-independent content equality/hash; positional equality needs a
  distinct name. Cursor invalidation on replacement versus structural edits must
  be specified. These are decisions to approve, not current runtime guarantees.

No LinkedHash aliases are proposed. The indexmap crate is a possible storage
foundation, not proof that ordinary Rust Hash/Eq callbacks support script reentry.

## 8. BTreeMap and BTreeSet

**Draft — requested**, not implemented. Implement MutableMap/MutableSet and
Iterable with ascending traversal. Ord defines membership equivalence via Equal
and must agree with Eq; membership does not require Hash.

```text
BTreeMap<K: Ord, V> {
    new() -> BTreeMap<K, V>
    default() -> BTreeMap<K, V>
    from_iter<I: Iterable<Item = (K, V)>>(source: I) -> BTreeMap<K, V>
    insert(self, key: K, value: V) -> BTreeMap<K, V>
    clear(self) -> BTreeMap<K, V>
    iter(self) -> Iterator<(K, V)>
    first(self) -> Option<(K, V)>
    last(self) -> Option<(K, V)>
    pop_first(self) -> Option<(K, V)>
    pop_last(self) -> Option<(K, V)>
    range<R: RangeBounds<K>>(self, bounds: R) -> Iterator<(K, V)>
    lower_bound(self, bound: Bound<K>) -> Option<(K, V)>
    upper_bound(self, bound: Bound<K>) -> Option<(K, V)>
}
BTreeSet<T: Ord> {
    new() -> BTreeSet<T>
    default() -> BTreeSet<T>
    from_iter<I: Iterable<Item = T>>(source: I) -> BTreeSet<T>
    insert(self, value: T) -> BTreeSet<T>
    clear(self) -> BTreeSet<T>
    iter(self) -> Iterator<T>
    first(self) -> Option<T>
    last(self) -> Option<T>
    pop_first(self) -> Option<T>
    pop_last(self) -> Option<T>
    range<R: RangeBounds<T>>(self, bounds: R) -> Iterator<T>
    lower_bound(self, bound: Bound<T>) -> Option<T>
    upper_bound(self, bound: Bound<T>) -> Option<T>
}
```

Here the proposed lower_bound is the first item satisfying the lower constraint,
and upper_bound the last satisfying the upper constraint. Included/Excluded select
inclusive/exclusive comparisons; Unbounded selects first/last respectively. These
are value lookups, not Rust borrowed cursor APIs. Exact names and invalid-range
behavior remain review items. Range results would be owned live revision-checked
cursors, not escaping tree loans or whole-tree snapshots.

Target complexity: O(log n) lookup/update and O(log n + k) traversal of k range
entries, excluding user callback costs. Do not restart at the root on every next.
For the Index example, BTree traversal remains 10,20,30 after replacement/reinsertion.
Keys' ordering-relevant state must remain stable through aliases and reload; no
automatic rebuild is promised when an Ord body changes.

Generic script Ord may fail, allocate and reenter. The fixed-i32 NC fixture can
use Rust Ord, but does not prove this general case. Use a callback-compatible tree
algorithm or audited adaptation, never a hidden global context or panic as the
ordinary comparison error channel. Content equality/hash are separate conditional
contracts. **Deferred:** split_off/append/merge, editable entries and rank/select.

## 9. Other collection candidates

**Draft — candidate** — no comparison requirement for a deque:

```text
VecDeque<T> {
    new() -> VecDeque<T>
    with_capacity(capacity: u64) -> VecDeque<T>
    from_iter<I: Iterable<Item = T>>(source: I) -> VecDeque<T>
    len(self) -> u64
    is_empty(self) -> bool
    front(self) -> Option<T>
    back(self) -> Option<T>
    get(self, index: u64) -> Option<T>
    push_front(self, value: T) -> ()
    push_back(self, value: T) -> ()
    pop_front(self) -> Option<T>
    pop_back(self) -> Option<T>
    clear(self) -> ()
    iter(self) -> Iterator<T>
}
BinaryHeap<T: Ord> {
    new() -> BinaryHeap<T>
    from_iter<I: Iterable<Item = T>>(source: I) -> BinaryHeap<T>
    len(self) -> u64
    is_empty(self) -> bool
    peek(self) -> Option<T>
    push(self, value: T) -> ()
    pop(self) -> Option<T>
    clear(self) -> ()
    iter(self) -> Iterator<T>
}
```

The heap draft is max-first; ordinary iter is not sorted. Arbitrary MutableList
writes are inappropriate, and ordering-relevant element mutation needs a policy.
**Deferred:** separate Stack/Queue wrappers, LinkedList, concurrent/persistent/weak
collections, escaping entries, drain/splice/extract iterators and mutable views.
Such APIs need concrete demand and traced lifetime/partial-consumption contracts.

## 10. Iteration, algorithms and grouping

### Iteration and construction

**Implemented** — [iteration](../crates/kagari-stdlib/src/foundation/iteration.rs):

```text
trait Iterator {
    type Item
    next(self) -> Option<Self::Item>
}
trait Iterable {
    type Item
    type Iter: Iterator<Item = Self::Item>
    iter(self) -> Self::Iter
}
trait FromIterator<T> {
    from_iter<I: Iterable<Item = T>>(source: I) -> Self
}
trait Sum<T> {
    sum<I: Iterable<Item = T>>(source: I) -> Self
}
trait Product<T> {
    product<I: Iterable<Item = T>>(source: I) -> Self
}
```

Iterator receives identity Iterable. Generic iterables do not have to return
CollectionCursor. In the blocks below, `T` abbreviates `Self::Item`.

**Implemented** — shared Iterable adapters and consumers:

```text
Iterable<Item = T> {
    map<U>(self, transform: fn(T) -> U) -> MapIterator<T, U>
    filter(self, predicate: fn(T) -> bool) -> FilterIterator<T>
    filter_map<U>(self, transform: fn(T) -> Option<U>) -> FilterMapIterator<T, U>
    take(self, count: u64) -> TakeIterator<T>
    skip(self, count: u64) -> SkipIterator<T>
    enumerate(self) -> EnumerateIterator<T>
    zip<I: Iterable>(self, other: I) -> ZipIterator<T, I::Item>
    chain<I: Iterable<Item = T>>(self, other: I) -> ChainIterator<T>
    collect<C: FromIterator<T>>(self) -> C
    fold<A>(self, initial: A, step: fn(A, T) -> A) -> A
    sum<S: Sum<T>>(self) -> S
    product<P: Product<T>>(self) -> P
    count(self) -> u64
    find(self, predicate: fn(T) -> bool) -> Option<T>
    any(self, predicate: fn(T) -> bool) -> bool
    all(self, predicate: fn(T) -> bool) -> bool
    for_each(self, action: fn(T) -> ()) -> ()
}
```

Adapters are lazy; consumers drive traversal. Copies of existing cursors share
progress; arbitrary iterators are not universally fused. Zip pulls left before
right and does not undo a left read when the right side ends. Structural mutations
commit before invalidating subsequent affected reads; short-circuit consumers do
not make an extra read merely to detect invalidation.

### Sorting, filtering and content comparison

**Implemented** — [list algorithms](../crates/kagari-stdlib/src/foundation/lists.rs):

```text
List<T> {
    sorted(self) -> List<T> where T: Ord
    sorted_by(self, compare: fn(T, T) -> Ordering) -> List<T>
    sorted_by_key<K: Ord>(self, key: fn(T) -> K) -> List<T>
    reversed(self) -> List<T>
    distinct(self) -> List<T> where T: Eq
}
MutableList<T> {
    sort(self) -> () where T: Ord
    sort_by(self, compare: fn(T, T) -> Ordering) -> ()
    sort_by_key<K: Ord>(self, key: fn(T) -> K) -> ()
    reverse(self) -> ()
    retain(self, predicate: fn(T) -> bool) -> ()
    dedup(self) -> () where T: Eq
}
list_content_equal<T: PartialEq>(left: List<T>, right: List<T>) -> bool
set_content_equal<T: Eq>(left: Set<T>, right: Set<T>) -> bool
map_content_equal<K: Eq, V: PartialEq>(left: Map<K, V>, right: Map<K, V>) -> bool
```

- Sort mutates and returns unit, including on Vec. Sorted returns List, not a
  concrete Vec in its declaration. Sorting is stable.
- Key selectors run during comparisons, not through a hidden cached-key prepass.
- Distinct keeps first occurrences globally under Eq; Eq-only distinct is not
  promised linear-time hashing. Dedup removes only consecutive equal elements.
- A struct needs Ord for sort(), but sort_by can supply a comparator directly.
  Hash containers require Eq + Hash, trees Ord; merely storing a struct in Vec
  or applying for_each/retain does not require those protocols.

### Additional adapters and consumers

**Draft — gap** — result iterator notation follows section 5:

```text
Iterable<Item = T> {
    flat_map<U, I: Iterable<Item = U>>(self, transform: fn(T) -> I) -> Iterator<U>
    take_while(self, predicate: fn(T) -> bool) -> Iterator<T>
    skip_while(self, predicate: fn(T) -> bool) -> Iterator<T>
    step_by(self, step: u64) -> Iterator<T>
    reduce(self, combine: fn(T, T) -> T) -> Option<T>
    min(self) -> Option<T> where T: Ord
    max(self) -> Option<T> where T: Ord
    min_by(self, compare: fn(T, T) -> Ordering) -> Option<T>
    max_by(self, compare: fn(T, T) -> Ordering) -> Option<T>
    min_by_key<K: Ord>(self, key: fn(T) -> K) -> Option<T>
    max_by_key<K: Ord>(self, key: fn(T) -> K) -> Option<T>
    position(self, predicate: fn(T) -> bool) -> Option<u64>
    partition(self, predicate: fn(T) -> bool) -> (Vec<T>, Vec<T>)
    try_fold<A, E>(self, initial: A, step: fn(A, T) -> Result<A, E>)
        -> Result<A, E>
    try_for_each<E>(self, action: fn(T) -> Result<(), E>) -> Result<(), E>
}
Iterable<Item = I> where I: Iterable {
    flatten(self) -> Iterator<I::Item>
}
Iterable<Item = (A, B)> {
    unzip(self) -> (Vec<A>, Vec<B>)
}
Iterable<Item = Result<T, E>> {
    try_collect<C: FromIterator<T>>(self) -> Result<C, E>
}
Iterable<Item = Option<T>> {
    collect_option<C: FromIterator<T>>(self) -> Option<C>
}
```

The fallible drafts intentionally show Result-specific signatures, not an already
selected generic Try design. A later design may instead extend collect through
carrier FromIterator implementations. Do not install overlapping alternatives
without choosing one owner. Early Err/None is normal short-circuiting, not trap or
cancellation recovery. Zero step, ties and key-selector evaluation counts need
explicit contracts.

**Draft — gap** — global grouping and aggregation:

```text
Iterable<Item = T> {
    group_by<K: Eq + Hash>(self, key: fn(T) -> K) -> IndexMap<K, Vec<T>>
    count_by<K: Eq + Hash>(self, key: fn(T) -> K) -> IndexMap<K, u64>
    associate_by<K: Eq + Hash>(self, key: fn(T) -> K) -> IndexMap<K, T>
    chunk_by(self, same_group: fn(T, T) -> bool) -> Iterator<Vec<T>>
}
Iterable<Item = String> {
    join(self, separator: String) -> String
}
```

Proposed group_by calls key once per element, preserves first-seen group order and
source order inside each group. For `[a1, b1, a2]` with keys `[A, B, A]`, the result
is `{A: [a1, a2], B: [b1]}`. Chunk_by groups adjacent runs instead. Associate_by
provisionally keeps the last value for an equal key at the key's first position.
These are materialized IndexMap results, not universal destination-polymorphic
contracts; destination flexibility remains a design choice.

**Draft — candidate** — cursor tools:

```text
Iterable<Item = T> {
    inspect(self, action: fn(T) -> ()) -> Iterator<T>
    fuse(self) -> Iterator<T>
    peekable(self) -> Peekable<T>
}
Peekable<T>: Iterator<Item = T> {
    peek(self) -> Option<T>
}
```

Peekable is a proposed traced adapter, not a borrowed reference. **Deferred:**
double-ended/exact-size capability protocols, arbitrary reverse traversal,
specialized streaming adapters and cached-key sorting. Do not silently buffer an
entire source to simulate a capability it does not have.

## 11. Ranges

**Implemented** — [ranges](../crates/kagari-stdlib/src/foundation/ranges.rs):

```text
Bound<T> = Included(T) | Excluded(T) | Unbounded
trait RangeBounds<T> {
    start_bound(self) -> Bound<T>
    end_bound(self) -> Bound<T>
}

a..b    : Range<T>
a..=b   : RangeInclusive<T>
a..     : RangeFrom<T>
..b     : RangeTo<T>
..=b    : RangeToInclusive<T>
..      : RangeFull
```

Construction is syntax, not registered new/start/end field access. Supported
integer ranges with a starting bound are iterable; RangeFull is not generic.

**Draft — gap**:

```text
RangeBounds<T> {
    contains(self, value: T) -> bool where T: PartialOrd
}
Range<T> { is_empty(self) -> bool where T: PartialOrd }
RangeInclusive<T> { is_empty(self) -> bool where T: PartialOrd }
```

Partial-order incomparability must be specified. Collection range consumers have
their own units and ownership contracts. **Deferred:** general user-type stepping,
floating ranges and bound-transformation utilities without a concrete consumer.

## 12. Async, time and randomness

### Existing task foundation

**Implemented** — [tasks](../crates/kagari-stdlib/src/foundation/tasks.rs) and
[future composition](../crates/kagari-stdlib/src/foundation/future.rs):

```text
TaskScope {
    spawn<T>(self, factory: fn() -> Future<T>) -> Result<Task<T>, SpawnError>
}
Task<T> { cancel(self) -> () }
SpawnError = ScopeClosed | CapacityExceeded | DispatchUnavailable

join<A, B>(left: Future<A>, right: Future<B>) -> Future<(A, B)>
try_join<A, B, E>(left: Future<Result<A, E>>, right: Future<Result<B, E>>)
    -> Future<Result<(A, B), E>>
```

TaskScope is host-created; spawn takes a factory, not a Future value. Future is
cold/single-drive and has no method surface; `.await` is syntax, not await(). Task
supports repeat awaits. Runtime remains single-driver and Send + !Sync.

**Draft — candidate**:

```text
join_all<T, I: Iterable<Item = Future<T>>>(inputs: I) -> Future<Vec<T>>
try_join_all<T, E, I: Iterable<Item = Future<Result<T, E>>>>(inputs: I)
    -> Future<Result<Vec<T>, E>>
race<T>(left: Future<T>, right: Future<T>) -> Future<T>
```

Input/result order, sibling cleanup, partial startup and bounded concurrency need
a lifecycle design before approval. Select/timeout are not implied by pair join.
Existing cancellation behavior is unchanged; redesign remains deferred.

### Pure time values and explicit providers

**Draft — candidate**, with proposed Duration/TimeError types:

```text
Duration {
    from_millis(value: u64) -> Result<Duration, TimeError>
    from_secs(value: u64) -> Result<Duration, TimeError>
    as_secs(self) -> u64
    subsec_nanos(self) -> u32
    checked_add(self, other: Duration) -> Option<Duration>
    checked_sub(self, other: Duration) -> Option<Duration>
}
```

**Host-provided draft** — Clock, Timer, Instant and Timestamp are proposed types,
not installed services:

```text
Clock {
    monotonic_now(self) -> Instant
    wall_now(self) -> Result<Timestamp, TimeError>
}
Timer {
    sleep(self, duration: Duration) -> Future<Result<(), TimeError>>
}
Instant {
    checked_duration_since(self, earlier: Instant) -> Option<Duration>
}
```

Monotonic instants and wall timestamps are not interchangeable. Provider identity,
precision, test clocks and wait cleanup need contracts. Timezones/calendars remain
optional separately scoped packages.

### Randomness

**Draft — candidate** — explicit state; Rng is a provisional type name:

```text
Rng {
    from_seed(seed: u64) -> Rng
    next_u64(self) -> u64
    random_f64(self) -> f64
    range_u64(self, start: u64, end: u64) -> Result<u64, RandomError>
    shuffle<T>(self, values: MutableList<T>) -> ()
    choose<T>(self, values: List<T>) -> Option<T>
}
```

RandomError is proposed. Specify unbiased ranges, float interval and a concrete
algorithm/version before promising reproducible sequences. Host entropy may seed
an RNG through a separate installed provider; no implicit global RNG or
cryptographic guarantee. Cryptographic RNG is a separate security scope.

## 13. Formats and optional packages

**Draft — candidate** — illustrative module signatures only. JsonValue, JsonError
and DecodeError are proposed data/error models, not aliases for runtime Value or
host-side Serde. All listed modules are absent from the installed script library.

```text
json {
    parse(text: String) -> Result<JsonValue, JsonError>
    stringify(value: JsonValue) -> Result<String, JsonError>
}
hex {
    encode<I: Iterable<Item = u8>>(bytes: I) -> String
    decode(text: String) -> Result<Vec<u8>, DecodeError>
}
base64 {
    encode<I: Iterable<Item = u8>>(bytes: I) -> String
    decode(text: String) -> Result<Vec<u8>, DecodeError>
}
```

JSON needs a number model, duplicate-key/order policy, depth limits and cycle
rejection before its types can be approved. Typed conversion requires an explicit
serialization protocol, not unrestricted reflection over Rust host state. Codec
alphabet/padding, strict versus lenient decoding and error offsets also need rules.

Remaining families are intentionally a scope list rather than fabricated APIs:

| Family | Status / decision still needed |
| --- | --- |
| TOML, CSV and configuration formats | Candidate; application demand and data model. |
| UUID/identifiers | Candidate; parsing/formatting versus entropy-backed generation. |
| Checksums/content digests | Candidate; stable algorithm separate from lookup Hash. |
| Compression, cryptography, regex, URL processing | Deferred independent dependency, resource and security review. |
| Normalization, grapheme/locale services | Deferred Unicode version and text-unit contracts. |
| Vector/matrix/geometry/statistics | Optional domain libraries unless recurring workloads justify standardization. |
| Decimal/big integer/rational | Candidate precision/range products; no scalar semantic change. |

Persisted hashes and serialized ordering must not depend accidentally on runtime
lookup hashes or unspecified hash-container traversal. Code identity and arbitrary
host/runtime handles are not portable data by default.

## 14. Host services and tooling

**Implemented boundary, not ordinary generic library declarations:** typed Native
registration, scoped Ref/RefMut host access, retained opaque objects and host waits.
The composition installs core::host/std::host and prelude Ref/RefMut. This does not
install a filesystem, network stack or unrestricted Rust references.

Existing compiler/host bridges, shown separately to avoid inventing generic methods:

```text
type_of(value) -> String
print(message: String) -> ()                 requires installed host::log
get_field(base, "literal_field_name")         compiler-checked field result
set_field(base, "literal_field_name", value)  compiler-checked; returns base
set_index(base, index, value)                 compiler-checked; returns base
```

These sketches are not arbitrary reflective access signatures. Field names must
be literals and accesses remain checked; type_of currently returns String, not a
new TypeInfo product. CLI/browser output adapters are provider examples, not
universal ambient output availability.

**Host-provided / deferred signature design:**

| Family | Required boundary before fixing signatures |
| --- | --- |
| Reader/writer/streams | Byte storage, partial IO, async waits, errors and explicit close/flush. |
| Filesystem and paths | Host authority, platform paths, traversal rules and resource ownership. |
| Network/HTTP | Provider installation, limits, streaming and cancellation cleanup. |
| Environment/process access | Explicit host exposure, never implicit engine authority. |
| Persistence/database/assets/game services | Application-owned transactions and typed interfaces. |
| Logging | Sink installation, levels/structured values and output-failure policy. |
| Assertions/approximate comparison | Candidate library helpers; diagnostics and failure semantics. |
| Deterministic clocks/RNG fixtures | Candidate test providers, not production global state. |
| Source test discovery/runner | Separate script-testing design, not implied by assertion functions. |
| Debugger/reflection/profiling extensions | Separate tooling/runtime owners. |

GC timing is not a close, flush or transaction boundary. Standardizing an interface
does not require bundling every provider or enabling it in every embedding.

## 15. Implementation boundaries

This is one catalog, not a second implementation queue. Before implementing a draft:

1. Select the canonical owner, default/optional/provider layer, dependencies and
   prelude policy; verify source-free installation.
2. Approve exact bounds, returns, units, eager/lazy behavior, callback order/count,
   normal errors versus traps and completed effects on failure. Do not weaken
   assertions or translate engine faults into arbitrary Result values.
3. Specify shared identity, readonly aliases, self-operations, cursor invalidation,
   key stability and reload behavior. Ordinary calls select current implementations;
   compatible reload does not promise automatic rehash/rebuild.
4. Use public general Native capabilities: the engine owns identities, roots,
   checked access, publication, scanning and accounting; libraries own payload
   storage/algorithms. All containers must be implementable outside Runtime through
   ordinary Opaque payloads, without new Value tags, compiler families or GC branches.
5. Retain extracted objects and trace internal edges/temporary work state; use short
   loans and complete publication barriers. No Rust borrow may survive script
   callbacks, allocation or an escaping iterator.
6. State compare/move/allocation costs and auxiliary space separately from arbitrary
   callback costs and actual GC work. No hidden whole-container retention snapshot,
   scalar boxing or repeated complete rescanning to simplify an algorithm.
7. Check u64-to-host sizing and platform limits; define backing charges, allocation
   failure and explicit external-resource cleanup on all exits.
8. Reuse semantic tests and generated declarations/docs; update implementation status
   only when the product is exposed. Detailed validation and progress belong to the
   activated implementation owner, not this catalog.

Index/BTree are useful generality checks for reordered buffers, node scans,
fallible comparisons and retained range cursors. They are **not** prerequisites
for completing the existing Vec/HashMap/HashSet GC migration and its bounded
external proof. No deadlines, new PG/NC gates or expansion phases are added here.

### Comparative references and limits

- [indexmap 2.14.2](https://docs.rs/indexmap/2.14.2/indexmap/): insertion/current-order
  collections and positions; a candidate foundation, not a selected Cargo dependency
  or proof that its callbacks fit Kagari.
- [Rust BTreeMap](https://doc.rust-lang.org/std/collections/struct.BTreeMap.html):
  ordered membership, ranges and key stability. Rust Ord/borrowed iterators do not
  express fallible script calls and GC loans.
- [Rust collections](https://doc.rust-lang.org/std/collections/index.html): distinct
  sequence/hash/ordered/priority roles. Kagari retains its shared-object model.

This catalog changes documentation only. No dependency, API registration, performance
prototype or behavioral test was added; references are not measured speed claims.
