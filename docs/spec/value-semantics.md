# Values and Expression Evaluation

This is the authoritative v1 contract for observable value behavior. Physical
allocation, Rust `Clone`/`PartialEq`, and backend choices do not define semantics.

## Values and identity

Scalars, strings, tuples, and enums (including Option and Result) have value
semantics. Tuple and enum copies preserve the semantics of each member; copying
a member that refers to a mutable object does not copy that object. Enum payloads
cannot be reassigned in place.

Structs, arrays, Vec objects, maps, and sets are mutable identity-bearing objects. Assignment,
argument passing, and returns share their identity. `val` prevents rebinding a
slot, not mutation of the referenced object. Container copy operations are
shallow. There is no generic deep-copy or deep-freeze operation in v1.
[Module statics](modules.md#persistent-module-statics) use the same value model:
`static val` prevents rebinding its cell, not mutation through writable aliases.

[Pattern-based declarations](syntax.md#blocks-and-statements) extract members under
this same value model: scalar leaves copy values and object leaves retain shared
identity. Destructuring neither moves members out nor implicitly clones/freezes an
object; rebinding an extracted `var` leaf does not write back to its source field.

[Collection access](collection-access.md) distinguishes read-only `List<T>`, `Map<K, V>` and `Set<T>` from writable `MutableList<T>`,
`MutableMap<K, V>` and `MutableSet<T>`. Concrete storage is `Vec`,
`HashMap` and `HashSet`. Array literals infer `[T]`. Access
conversion preserves the underlying object. Copy/factory algorithms are not
part of the current foundational collection interface.
Read-only views do not freeze referenced objects or other writable aliases.

An interface may retain a checked durable host root as its concrete payload;
this does not make host borrow tokens or path views valid heap payloads. See the
[host interface contract](traits.md#host-associated-outputs-and-interfaces).

[Equality and hashing](#equality-and-hashing) defines explicit protocol selection
and conditional builtin/library implementations. User Structs and enums require
explicit implementations; object identity alone supplies no equality or hashing.
Tuple and core enum protocols compose the selected member implementations.
Interface boxing supplies no comparison capabilities.

Core enum comparison uses declaration identity, applied type arguments and
variant identity; adding or reordering private variants in a later execution
version does not change comparison of an existing variant with the same members.
Floating-point equality follows IEEE comparisons (`NaN != NaN`, `-0 == +0`).
Map and Set keys require `Eq + Hash` and trace retained keys as well as values.
Authors must preserve equality-relevant state while keys are stored.
See [standard protocols](builtins.md#collection-types).

Structural edits through an alias commit and invalidate existing native collection
cursors. A stale cursor traps on its next source read; retaining a cursor never
blocks writes. Replacing an element or mutating its referenced object remains
allowed. See [iteration protocols](builtins.md#iteration-protocols) for the exact
invalidation, short-circuit and custom iterator rules.

String len, find and slice use UTF-8 byte counts/offsets; slices validate UTF-8
boundaries. Explicit lazy chars yields `char`, char_indices yields `(u64, char)`
with byte offsets, and bytes yields UTF-8 `u8` values. For `"aé🙂"`, len is seven,
chars yields three scalars and char_indices reports offsets 0, 1 and 3. Scalars
are not grapheme clusters; combining marks remain separate items.
`text.chars().count()` counts scalars. There is no implicit String iteration or
integer String indexing.

String cursor copies share progress, not independent traversal state; a new
chars, char_indices or bytes call creates a fresh cursor. The cursor retains and
traces its immutable source through GC and async suspension. A native next
commits progress only after successful item/result publication, so a publication
failure leaves that cursor unadvanced. A later adapter callback failure does not
undo an already completed source pull. See the
[String traversal contract](builtins.md#explicit-string-traversal).

Integer arithmetic is checked in every backend and build mode. Explicit wrapping
operations are the only exception. Floating-point optimization must preserve
specified results.

Numeric literals follow the [literal typing rules](syntax.md#literals): suffixes
select a primitive type, while unsuffixed literals use context and otherwise
default to i32 or f64. Integers must fit the selected type; a negative literal
can represent its signed minimum, but negating that minimum again traps.
Float literal conversion must produce a finite value at the selected precision;
runtime floating-point operations retain their IEEE behavior. Invalid literal
ranges are diagnosed during analysis, including in const initializers and match
patterns. A literal pattern must have the scrutinee's type.

## Accepted comparison design

Comparison eligibility is independent of assignment, sharing, GC ownership and
interface receiver admission. Values may still become dynamically compatible trait
interfaces through checked implementations. No Object type, marker or bound is
introduced.

### Equality requires PartialEq

`==` requires a statically proven applicable PartialEq implementation; `!=`
negates its result. Operators, `.eq()`, generic code and container operations
use the same selected implementation. Builtin and conditional library
implementations count as evidence; users need not handwrite every comparison.
Missing or unsatisfied evidence is a compile-time error, never an automatic
identity comparison or a deferred missing-comparison trap. An author may
explicitly implement PartialEq using `self === other` for an object type.

| Type | Equality contract |
| --- | --- |
| Unit, bool, integers, String | Builtin value comparison. |
| char | Builtin [Unicode scalar comparison](builtins.md#character-scalar), not integer coercion. |
| Float | Builtin IEEE comparison, including NaN unequal to itself; no Eq/Hash. |
| Tuple | Conditional member comparison when every member supports PartialEq. |
| User Struct or enum | Explicit implementation; neither object identity nor enum member comparison is automatically supplied. |
| Core Option / Result / Ordering / Bound / ControlFlow | Standard-library conditional variant/member comparison; all payload types must satisfy the required bounds, not only the active variant. |
| Array / `Vec<T>` | Conditional content comparison when `T: PartialEq`; compare length and then corresponding elements in order. |
| `HashSet<T>` | Conditional content comparison under `T: Eq + Hash`. |
| `HashMap<K,V>` | Conditional content comparison under `K: Eq + Hash` and `V: PartialEq`. |
| Native script object | Applicable registered implementation; no identity fallback. Existing host-handle/path restrictions remain separate. |
| Trait interface value | Requires an implementation for the interface value type itself; hidden receiver implementations are not automatically exposed. |
| Generic T | Requires PartialEq evidence in the checked generic context. |
| Function / closure | No automatic comparison. |

User Structs and enums opt into comparison, following Rust's explicit-implementation
model. Memberwise derivation may be designed separately; this decision does not
add derive syntax. Standard-library Option/Result retain their conditional
implementations even though they use enum representation. There is no automatic
identity PartialEq, Eq or Hash; having an identity does not make an object a hash
key. Explicit implementations and conditional content protocols retain Eq/Hash
requirements and callback consistency.

Boxing or adapting an interface never adds comparison capabilities. For example,
if Color implements PartialEq and Read, Color values can compare, but values
erased to Read cannot compare unless Read's interface value type itself has an
applicable PartialEq implementation. Adding PartialEq as a parent is not an
erased comparison contract: Kagari's `other: Self` remains subject to dynamic
compatibility checking.

The six collection interfaces have no receiver-comparison forwarding
implementations. They retain their Debug implementations and the explicit
[content helpers](collection-access.md#content-protocols-and-helpers), including
their element/key bounds and cross-implementation behavior. Consequently,
`Vec<i32>` equality remains available, while `List<i32>` equality is rejected under
the library declarations. Interface-level content PartialEq and
source-authored interface impl targets are separate future API decisions.

The reference model is Rust's [PartialEq](https://doc.rust-lang.org/std/cmp/trait.PartialEq.html)
and [conditional Box comparison](https://doc.rust-lang.org/std/boxed/struct.Box.html):
wrapping a value does not supply a missing comparison implementation. Keep
Kagari's current same-type comparison scope; Rust's heterogeneous `PartialEq<Rhs>`,
borrowing and ownership model are not added by this decision. Rust can use
PartialEq with a fixed Rhs in a trait object; that does not make Kagari's
unresolved `other: Self` dynamically compatible.

### Identity requires statically known object operands

`===` and `!==` require both operands to have statically known identity-bearing
object types: Struct, native script objects, Array, Vec, HashMap or HashSet.
Different concrete object types may be compared without a common interface,
boxing or PartialEq selection. The result tests the same underlying object;
distinct objects compare unequal. `!==` negates that result. Neither operator
is overloadable or invokes user comparison code.

Reject either operator at compile time if either operand is a trait interface,
a value type, a function/closure or a bare generic parameter whose object category
is unknown. A PartialEq/Eq bound does not prove object identity. An object shape
such as `Vec<T>` is eligible even when T is generic or an interface: eligibility
concerns the operand itself, not the categories of its members.

Illustrative operand types and checking results:

```text
Item === Item          // Allowed; neither needs PartialEq.
Vec<T> === Vec<T>      // Allowed; the container has identity.
T === T                // Rejected; T's object category is unknown.
Item === Read          // Rejected; one operand is an interface.
Read === Read          // Rejected, even if both hide the same Item.
Color === Color        // Rejected; enum values have no object identity.
```

Interface construction/upcasting preserves sharing and underlying object
identity, but does not permit identity operators through an interface-typed
expression. There is no interface-wrapper identity or hidden-value identity
trap in the operator contract. A checked explicit downcast to an object
type can make identity comparison legal without copying the object; a downcast
to a value type cannot. This is the required semantic interaction, not a claim
that source downcast syntax/APIs are implemented. Downcast failure must be
handled through its own checked contract. Generic interface infrastructure can
use subscription handles, node IDs or explicit keys when the concrete type is
unknown; no universal identity API is introduced.

Both operands still evaluate left to right exactly once. Identity remains stable
through mutation and GC relocation; invalid, foreign or stale handles remain
errors rather than unequal objects. Preserve roots, owner/generation checks,
selected comparison generations, cancellation and cleanup. Source checking and
independent executable validation must agree on both operators' eligibility;
backends consume checked facts rather than rediscovering comparison policy.

## Equality and hashing

Runnable coverage is provided by [standard-traits.kgr](../../examples/syntax/standard-traits.kgr)
and the standard-protocol integration tests.

### One implementation selection rule

For a given concrete type, select its applicable explicit implementation or its
eligible builtin/library implementation. Missing evidence is a static error.
User Structs and enums have no automatic comparison implementation. Operators,
method calls, generic code and containers use the same selected implementation;
imports cannot silently change it. Assignment and copying retain their existing
semantics regardless of custom equality.

### Operators and defaults

`==` calls the type's PartialEq comparison; `!=` negates that result. Eq extends
PartialEq without adding a method: it promises an equivalence relation, including
reflexivity, symmetry and transitivity. Eq does not mean field equality or
immutability. `.eq()` and generic comparisons use the same implementation as `==`.

`===` and `!==` use stable runtime-owned object identity and cannot be overridden.
Their [static operand rule](#identity-requires-statically-known-object-operands)
rejects interface, value and unknown generic operands. Identity is independent of
PartialEq and remains stable through mutation and GC relocation.

| Type | Supplied comparison | Supplied hash | Implementation owner |
| --- | --- | --- | --- |
| User Struct / enum | None | None | Explicit implementation in defining module |
| Tuple | Conditional corresponding members | Conditional member hashes | Builtin |
| Core Option / Result / Ordering / Bound / ControlFlow | Conditional variant/member comparison | Conditional variant/member hashes | Core library |
| Array | Conditional ordered contents, PartialEq only | None | Standard library |
| Vec / HashMap / HashSet | Conditional contents | Conditional contents | Standard library |
| Unit / bool / integers / String | Value | Value | Builtin |
| Float | IEEE comparison, PartialEq only | None | Builtin |

The six collection interfaces provide bounded Debug, without PartialEq/Eq/Hash.
Concrete collection bounds and cross-implementation helpers are specified in
[collection content protocols](collection-access.md#content-protocols-and-helpers).
Other interfaces, host handles/paths and function values gain no automatic equality
or hashing; this does not open host equality implementations.

### Explicit Struct and enum implementations

Struct and user enum declarations permit the same explicit implementations.
Implementations of these canonical protocols must be in the type's defining
module. This prevents a downstream import from changing equality for values
already stored by a dependency. Generic implementations are selected for each
concrete instantiation under their declared bounds:


| Explicit implementations | `==` / `.eq()` | Eq and Hash eligibility |
| --- | --- | --- |
| None | Not comparable | Not a key |
| PartialEq only | Custom comparison | No implicit Eq or Hash; not a key |
| PartialEq and Eq | Custom comparison | Explicit Eq, no implicit Hash; not a key |
| PartialEq, Eq and Hash | Custom comparison | Explicit Eq and custom Hash; key eligible |

PartialEq, Eq and Hash are separate opt-ins. Eq requires PartialEq; Hash requires
the complete PartialEq/Eq/Hash set for key use. Eq-only and Hash-only declarations
without their required protocols are rejected. Comparison alone does not require Hash.

An explicit enum comparison defines the entire comparison, including whether
different variants are equal. Its custom hash must agree; the runtime does not
prepend a variant tag or perform a variant inequality check before calling it.
An explicit implementation is checked under its declared bounds, without an
automatic requirement that every payload implement the same protocol.

Custom comparison does not change GC ownership, shallow copies or identity
operators. Struct `===` remains identity comparison even when `==` is customized;
enum values do not gain `===` by implementing comparison.

### Member composition

Default Tuple comparison applies `==` to corresponding members, left to right,
stopping at the first unequal member. Conditional core enum comparison first checks the
variant, then compares that variant's payload members in declaration order in
the same way. A payload-free variant equals the same variant of the same
concrete enum type. Different concrete types are not made comparable by this
rule. The members' selected implementations may be builtin or user-defined;
there is no separate equality rule for composites containing custom members.

Default hashing combines member hashes in the same member order, and enum
hashing also includes enum and variant identity. A Tuple or eligible core enum
implements PartialEq, Eq or Hash only when all member types support the
corresponding protocol. This checks every enum variant, not only the currently
constructed variant. Core Option, Result, Ordering, Bound and ControlFlow follow these rules.
The all-members rule applies to default implementations, not explicit enum
overrides. Floats retain PartialEq without Eq or Hash when nested in a composite.

For example, Option composes explicit member comparison:

```kagari
struct Point { val x: i32, val y: i32 }
impl PartialEq for Point {
    fn eq(self, other: Self) -> bool {
        self.x == other.x && self.y == other.y
    }
}
impl Eq for Point {}

fn compare() -> bool {
    val a = Point { x: 1, y: 2 };
    val b = Point { x: 1, y: 2 };
    // Both composite comparisons use Point's custom comparison.
    (a, 7) == (b, 7) && Some(a) == Some(b)
}
```

`compare()` returns true; `a === b` would be false. Point has no Hash here, so
neither `(Point, i32)` nor `Option<Point>` is eligible as a hash key. Adding a matching
Point Hash implementation makes both eligible through default composition.

An explicit enum override can instead ignore its variants:

```kagari
enum Identifier { Local(i32), Remote(i32) }
fn number(value: Identifier) -> i32 {
    match value {
        Identifier::Local(id) => id,
        Identifier::Remote(id) => id,
    }
}
impl PartialEq for Identifier {
    fn eq(self, other: Self) -> bool { number(self) == number(other) }
}
impl Eq for Identifier {}
impl Hash for Identifier {
    fn hash(self) -> i64 { number(self).hash() }
}
```

Local(42) and Remote(42) now compare equal and hash equally, including inside a
Tuple, Option or a Set. Without these implementations Identifier cannot compare
or hash. The custom hash does not include the variant.

### Execution paths

Identity operators and builtin scalar operations retain direct execution paths; String
uses builtin content operations. Composites use their members' selected paths,
calling user implementations only where required. Compilation/linking can select
these paths for concrete instances, including monomorphized generic code. These
are execution choices, not different observable equality contracts. A fast path
must not bypass a selected user implementation or change comparison order and
failure behavior. `===` never invokes a user callback.

### Hash containers and user obligations

Map keys and Set elements continue to require `Eq + Hash`, including in generic
code. A Map's values do not need those bounds. Lookup hashes the key, then uses
the key equality selected at construction to distinguish candidate keys. Those
[stored protocols](collection-access.md#stored-key-protocols-across-reload) remain
pinned across reload; a later caller's `==` selection need not use the same body.

Users implementing these protocols must ensure:

- Equal values have equal hashes: `a == b` implies `a.hash() == b.hash()`.
  The converse is not required; hash collisions are valid.
- Eq comparison is an equivalence relation. Comparison and hashing are
  consistent while the state they depend on is unchanged.
- While a key belongs to a container, the state determining its equality and
  hash remains stable, including state reached through aliases or external
  dependencies. Unrelated fields may still change.
- Comparison/hash callbacks do not mutate the participating key state or the
  container currently performing the operation.

The language does not prove these semantic properties, freeze keys, deep-copy
keys, observe all mutations or automatically reindex containers. Violations are
user logic errors: lookups/removals may miss entries and logical duplicates may
appear. They must not compromise memory safety, GC or runtime integrity. No
guarantee is made that every violation is detected or produces a diagnostic.
Dangerous mutation reentry into the active container must be rejected with a
trap; this is an engine integrity check, not general key-stability enforcement.

To change equality-relevant state, remove the key from every affected container
before changing it, then reinsert it. This also applies to composite keys that
refer to that object. Hash codes are runtime lookup aids, not unique IDs,
cryptographic hashes or persistent fingerprints.

For example, an author may explicitly give `User { var id: i32 }` identity
equality and a consistent hash. Changing `id` then does not affect membership,
and a different User with the same id
is a different key. With custom equality and hashing based on `id`, separately
allocated Users with the same id are equal keys; changing id while stored
violates the container contract. In either case `===` distinguishes the objects.

### Callback failure boundary

Custom equality/hash calls execute before the container commits its modification.
A failed callback does not commit the pending insertion, update or removal.
Earlier callback effects elsewhere are not rolled back. Execution must preserve
GC roots and release guards on failure; no mutable storage borrow may span
arbitrary script callbacks. These are implementation requirements, separate from
the user's responsibility for valid equality and stable keys.

Container operations hash the query once, then compare it with stored candidates
in the matching hash bucket, in insertion order. The query is the comparison
receiver. Comparisons stop at the first match; updates retain the original key.
There is no identity shortcut around an explicit comparison, even for aliases.
Reentrant reads are allowed; mutation of the active container traps, including
updates which would not change its length. Traps and cancellation release
the lookup guard and temporary roots.

Builtin-only keys retain native hashing and lookup. Composite values containing
custom members use reusable compiled comparison/hash helpers. These helpers and
user callbacks run through ordinary linked calls, cooperative cancellation and GC
safepoints; JIT-ineligible paths use the interpreter.


## Evaluation and assignment

Operands, receivers, arguments, and dynamic indexes evaluate left to right,
exactly once. Short-circuit operators evaluate only the selected operand.

For `root()[index()].field += rhs()`, projected-location evaluation is:

1. Evaluate the root and indexes and retain a location description.
2. Evaluate the right-hand side.
3. Validate the location and read its current value.
4. Compute and validate the replacement, including overflow and resource checks.
5. Commit one modification.

Plain assignment follows the same order without reading the old value. A path
denotes the current location, not a detached Rust reference or a pinned element.
If RHS execution removes the location, the final write fails. RHS effects already
performed remain visible. No mutable host borrow spans RHS execution.

Arithmetic compound forms are `+=`, `-=`, `*=`, `/=` and `%=` with matching
numeric operands. Bitwise forms are `&=`, `|=` and `^=` with matching integer
operands; `<<=` and `>>=` accept an integer count of any width. The root of a mutable object location retains the identity evaluated
before the RHS; rebinding the root variable does not retarget it. Captured indexes
are applied to the current contents after the RHS, so replacing an intermediate
element changes which current field is modified. A local scalar or tuple slot is
read after the RHS. Updating a tuple member prepares a new tuple value and thus
requires a writable enclosing slot; modifying a mutable object referenced by a
tuple does not replace the tuple.

### Static roots

Reading a static is effectful: it may invoke its fixed initializer, allocate,
reenter or fail. An unused read still executes, and a read in an untaken branch
must not be hoisted into execution. See [lazy activation](module-activation.md#lazy-static-activation).

For both `S = rhs()` and `S += rhs()`, ensure and read S before evaluating the RHS.
Plain assignment does not skip initialization merely because it replaces the
result. Direct static compound assignment computes from the value captured by
that read, unlike a local scalar/tuple slot read after the RHS. For example, if S
is initially 1 and `rhs()` sets S to 10 and returns 2, `S += rhs()` commits 3.
Failure before commit retains any completed RHS effects, including its write of 10.

For `S.values[index()] += rhs()`, capture S's object root first, evaluate the index
once, then evaluate the RHS once. Only afterward resolve intermediate projections,
check bounds, read the current destination value and commit. Rebinding S during
index/RHS evaluation does not retarget the captured root. Replacing that root's
`values` field does change the array used at commit: intermediate objects are not
prematurely captured. A bounds failure therefore occurs after RHS effects, which
remain visible. Initializer execution and access retry never repeat index/RHS
computation or hold a storage borrow across callbacks.

## Struct update construction

For `Player { health: record(200), ..make_player() }`, evaluate record first, then
make_player once and in full, then read only omitted members from its result and
allocate the new Player. The base expression still executes when no member is
inherited. Reads are not hoisted before explicit/base effects: another alias may
change a member before it is copied. Explicit values remain rooted across base
calls, collection and synchronous host reentry. Traps, cancellation and early
returns retain completed effects and release existing temporary/frame roots.

The result has independent outer identity and scalar slots. Referenced fields,
such as an items collection, retain shared identity: changing the new player's
health does not alter the base's health, while mutating their shared items is
visible through either alias. The base remains usable. There is no implicit Clone,
deep copy, move or in-place update. All-field access and exact nominal/generic
eligibility are checked by [struct update typing](syntax.md#struct-update-expressions).
This follows Rust 1.90's terminal-base, visibility and operand-order contracts but
not its move/partial-invalidation ownership model.

## Acceptance examples

- Mutating `b` after `val b = a` changes the object observed through `a`.
- Separately allocated objects have unequal identity; `==` requires PartialEq.
- Core enum values compare by variant and selected member implementations; user enums require explicit comparison.
- A shallow copy has new container identity but shares referenced elements.
- Structural mutation through an alias commits; the old cursor traps on its next read.
- A projected compound assignment evaluates root/index/RHS once and reads the
  destination after RHS; direct static compound assignment captures its value
  before RHS. Neither writes after a failed RHS, invalidated location, or overflow.

The shared language and collection suites check identity through arguments and
returns, independent storage from explicit constructors, and shared referenced
elements. Source, artifact and JIT-enabled paths retain these contracts; native
preparation may use interpreter fallback.

Iterator terminals and host collection traversals retain source values without
blocking structural writes. Native cursors check structural revisions before
reading again, including a read that would report exhaustion. Optimized Vec and
host traversal paths perform equivalent checks around callback-driven traversal.
See [the iteration contract](builtins.md#iteration-protocols) for invalidation,
short circuit, replacements and custom iterator policy.

Callbacks run on ordinary script frames with their arguments and accumulator
registered as execution roots. Completed callback side effects survive later
failure. Short-circuit operations consume the decisive item and leave subsequent
items available through the same iterator and its aliases.

Heap pop/remove/clear APIs return `Result` for operational failure. `Ok(None)`
from Vec pop/remove or Map remove, and `Ok(false)` from Set remove, indicate
normal absence only. Invalid keys/handles, callback protection and execution
rejection are errors. Clear returns `Result<(), RuntimeError>`. Standard helpers
preserve this distinction and check callback protection before allocating their
script-level Option result; rejected operations do not change contents or live occupancy.

Array element and Struct field-slot replacement return `Result<(), RuntimeError>`.
Invalid payloads, slots, layouts and write permissions fail before assignment.
They preserve execution-rejection categories instead of collapsing failure into
`None`. Reflective writes translate ordinary script errors to reflective-write
errors, but retain engine-fault categories. An internal struct storage/layout
inconsistency quarantines the runtime; it is not an ordinary script error.

Array replacement bounds failures have runtime category `IndexOutOfBounds`
(`KG_RUNTIME_INDEX_OUT_OF_BOUNDS`). The VM maps that category directly to its
index trap; it does not infer a cause by rereading array length after a failed
write. Earlier payload/handle/execution rejections keep their own categories.
Reflective writes continue to expose ordinary bounds failures as reflective-write
errors; embedding classifies them as script failures rather than engine faults.

## Fixed-width bitwise operations

Integer `&`, `|`, `^` and `!` operate on the operand's declared width. Binary
bitwise operands have the same type. `<<` and `>>` accept any integer count,
return the left operand's type, and trap for negative counts or counts at least
that type's width. Left shift discards high bits; signed right shift extends the
sign and unsigned right shift fills with zero. These rules also apply to consts.

Precedence, from tight to loose, is arithmetic, shifts, `&`, `^`, `|`, comparison,
equality, `&&`, `||`. The shift characters must be adjacent; nested generic
closing brackets continue to parse independently. `%=` and bitwise compound
assignments use the existing evaluate-location, evaluate-RHS, read, check, commit
sequence. Invalid shifts do not commit a target change.

Operator traits `BitAnd`, `BitOr`, `BitXor`, `Shl`, `Shr` and `Not` use ordinary
static dispatch. User implementations may choose an associated Output type.
Compound assignment remains restricted to primitive operations.

## Explicit integer arithmetic

Ordinary arithmetic and compound assignment trap on overflow, including signed
MIN divided or remaindered by -1. Installed scalar methods explicitly select a
different policy without changing those operators. The exact signatures and finite
inventory are in [builtins](builtins.md#numeric-scalar-methods).

- `wrapping_add`, `wrapping_sub` and `wrapping_mul` compute modulo 2^width and
  interpret the result at the receiver's signedness.
- `checked_add`, `checked_sub`, `checked_mul`, `checked_div` and `checked_rem`
  return `Some(result)` when representable and `None` on arithmetic failure.
  Division truncates toward zero; a nonzero remainder has the receiver's sign.
  Zero divisors and signed MIN / -1 or MIN % -1 return `None`, even though the
  mathematical remainder in the last case is zero.
- `overflowing_add`, `overflowing_sub` and `overflowing_mul` return the wrapped
  result and an arithmetic overflow flag. For signed receivers this flag is not
  an unsigned carry or borrow flag.
- `saturating_add`, `saturating_sub` and `saturating_mul` clamp the mathematical
  result to the receiver's range; unsigned subtraction bottoms at zero.
- Unsigned `wrapping_add_signed` adds a signed offset of the corresponding width
  and returns an unsigned result modulo 2^width. It is not installed on signed
  receivers.
- `rotate_left` and `rotate_right` circularly rotate the fixed-width bit pattern.
  Their `u32` count is reduced modulo the receiver's bit width; zero and multiples
  of the width leave it unchanged. Signed results reinterpret the rotated bits
  at the original signedness. Ordinary shifts still trap for out-of-width counts.

Receiver and arguments must match the declared types exactly and are evaluated
once, left to right. These policies replace only the selected arithmetic failure
outcome; runtime and allocation errors still propagate. Wrong arity, width or
signedness is rejected by source checking or executable/native contract validation,
not silently converted. Explicit casts retain the rules below.

## Floating-point utilities and math

Installed `f32` and `f64` methods use exact, width-matched native scalar bindings:
`f32` operations do not widen to `f64` and narrow again. Arguments must have the
declared types and are evaluated once, left to right. The
[finite API inventory](builtins.md#numeric-scalar-methods) includes utilities,
classification, bit reinterpretation and bounded extended math, not every Rust
float method. Numeric domain errors, overflow and underflow return ordinary float
results rather than arithmetic traps or host panics, except for invalid `clamp`
bounds as specified below. Other runtime errors still propagate.

### Utilities, bounds and exact bits

- `abs` makes negative zero positive and either infinity positive; NaN stays NaN.
- `sqrt` returns the **correctly rounded** square root at the receiver's width.
  Negative nonzero inputs, including negative infinity, return NaN; positive
  infinity remains positive infinity and negative zero remains negative zero.
- `floor` rounds toward negative infinity, `ceil` toward positive infinity,
  `round` to the nearest integral float with halfway cases away from zero, and
  `trunc` toward zero. Signed zeros and infinities are retained; NaN stays NaN.
  These return floats, not integer conversions.
- `fract` follows the matching Rust primitive's `self - self.trunc()`: negative
  nonintegral inputs have negative fractions, infinities return NaN, and zero
  signs follow the subtraction rather than a separate sign-preservation rule.
- `min` and `max` ignore one NaN and return the other operand; two NaNs return
  NaN. Equal positive and negative zero operands may return either sign.
  Infinities compare normally. These are not total-order operations and do not
  give floats `Ord`.
- `clamp(min, max)` validates both bounds **even when the receiver is NaN**.
  Either NaN bound or `min > max` produces Kagari `ScriptTrap`, never a Rust panic.
  Infinite bounds and equal opposite-signed zero bounds are legal. With valid
  bounds a NaN receiver returns NaN, an out-of-range value returns its bound, and
  an in-range receiver is retained, including its zero sign. Replacing this with
  a `max`/`min` composition changes the contract.
- `is_nan` recognizes quiet and signaling NaN patterns; `is_infinite` recognizes
  either infinity; `is_finite` includes subnormals and both signed zeros.
- `to_bits` and static `from_bits` reinterpret IEEE 754 bits, never perform `as`
  conversion. `f32::from_bits(bits).to_bits()` with `u32` bits and the analogous
  `f64`/`u64` round trip preserve every bit, including zero signs, subnormals,
  infinities and quiet/signaling NaN signs and payloads. Every input bit pattern
  is accepted. There is no signaling-NaN exception mode.

Bit-exact transport is not a promise about subsequent arithmetic, casts,
formatting or math. In particular, arithmetic/math NaN signs and payloads are not
specified, even when an input was constructed with `from_bits`.

### Extended math domains and powers

Angles are radians. `sin` and `tan` preserve signed zero, `cos` of either zero
is 1, and all three return NaN for infinite or NaN inputs. Near an approximate
tangent pole, no exact pole or particular infinity is promised. `asin` and `acos`
accept [-1, 1] and return NaN outside it; `asin` preserves signed zero, while
`acos(1)` is positive zero. `atan` accepts all finite inputs, preserves signed
zero and maps infinities to the corresponding signed pi/2. Nonzero angles remain
subject to the precision boundary below.

`y.atan2(x)` is the four-quadrant angle in [-pi, pi], not `atan(y / x)`.
The receiver is y and the argument is x. Zero y with positive x returns y's zero
sign; with negative x it yields pi with y's sign. This includes signed-zero x:
`atan2(+0, -0)` yields +pi and `atan2(-0, -0)` yields -pi, whereas positive-zero x
returns y's signed zero. Nonzero y with either zero x, or infinite y with finite x,
yields pi/2 with y's sign. Finite y with positive infinite x returns y-signed zero;
with negative infinite x it yields y-signed pi. Both infinite coordinates yield
y-signed pi/4 for positive x or y-signed 3*pi/4 for negative x. Either NaN returns NaN.

`exp` of either zero is 1; positive infinity returns positive infinity and negative
infinity returns positive zero. Finite results may overflow to infinity or underflow
to a positive subnormal or zero. `ln(1)` is positive zero, either signed zero yields
negative infinity, negative inputs yield NaN, and positive infinity remains positive
infinity. NaN inputs to either method return NaN.

`powf(exponent: F)` uses a same-width floating exponent. Explicit library guards
return 1 for either signed zero exponent with **every** base, and for base 1 with
**every** exponent, including quiet and signaling NaNs. These guards run before
NaN handling; otherwise a NaN operand returns a quiet NaN of unspecified sign and
payload. Remaining inputs use the matching Rust `powf` operation:

- A finite negative nonzero base with a finite noninteger exponent returns NaN.
  A finite integer exponent produces a negative result exactly when it is odd.
- For nonzero exponents, zero bases return zero for positive exponents and infinity
  for negative exponents. Negative zero has a negative result only for finite odd
  integer exponents. Infinite bases have the same sign rule, with infinite magnitude
  for positive exponents and zero magnitude for negative exponents; a noninteger
  exponent does not make negative infinity a domain NaN.
- With either infinite exponent, absolute base 1 returns 1, including base -1.
  Positive infinite exponent gives positive zero for absolute base below 1 and
  positive infinity above 1; negative infinite exponent reverses those results.

`powi(exponent: i32)` accepts the entire exact `i32` range without converting it
to float. Exponent zero returns 1 even for quiet/signaling NaNs, zero and infinities;
a NaN base with nonzero exponent returns a quiet NaN. Base 1 always returns exactly
1; base -1 returns exactly -1 for odd exponents and 1 for even exponents. For other
non-NaN bases, the library computes the matching Rust `powi` magnitude on the
absolute base and restores a negative base's sign exactly when the original `i32`
exponent is odd. This preserves parity at `i32` extrema even where a direct host
signed-base operation would lose it; it is not a promise to match direct Rust
`powi` for every input.

Negative finite bases are valid for every integer exponent. For nonzero exponents,
zero bases yield zero for positive exponents and infinity for negative exponents;
infinite bases yield infinity for positive exponents and zero for negative
exponents. A negative base gives a negative result exactly for odd exponents,
including signed zero/infinity results. Finite power results may overflow to
infinity or underflow to subnormals or zero; negative exponents compute reciprocal
powers without trapping. `powi` and `powf` need not round identically even for
integer-valued exponents. Neither is replaced with `exp(exponent * ln(base))`.

### Precision and implementation references

The correctly rounded `sqrt` guarantee is stronger than the contract for extended
trigonometric, exponential, logarithmic and power methods. Extended-math precision
is unspecified and may vary with platform, Rust version or even between invocations
in one execution. There is no universal ULP bound, cross-platform bit identity or
bit-identical repeated approximate result guarantee. Tests at finite reference
points require justified, width-specific tolerances; matching the same host's Rust
result alone is not an independent accuracy proof. No fast-math, reassociation or
implicit fused multiply-add is introduced.

Primary semantic references consulted on 2026-10-21 are pinned to Rust **1.90.0**;
that version is a reference, not a toolchain requirement:

- Rust core [signed integers](https://raw.githubusercontent.com/rust-lang/rust/1.90.0/library/core/src/num/int_macros.rs)
  and [unsigned integers](https://raw.githubusercontent.com/rust-lang/rust/1.90.0/library/core/src/num/uint_macros.rs):
  explicit overflow policies and rotations. Kagari reuses its width-aware kernels,
  not Rust's Option/tuple storage or build-mode overflow policy.
- Rust core [f32](https://raw.githubusercontent.com/rust-lang/rust/1.90.0/library/core/src/num/f32.rs)
  and [f64](https://raw.githubusercontent.com/rust-lang/rust/1.90.0/library/core/src/num/f64.rs):
  classification, bit reinterpretation and NaN-ignoring bounds operations. Kagari
  validates invalid clamp bounds into `ScriptTrap` instead of a Rust panic.
- Rust std [f32](https://raw.githubusercontent.com/rust-lang/rust/1.90.0/library/std/src/num/f32.rs)
  and [f64](https://raw.githubusercontent.com/rust-lang/rust/1.90.0/library/std/src/num/f64.rs):
  width-specific intrinsic/platform math and the sqrt/extended-precision distinction.
  Kagari's power guards and exact integer-parity restoration adapt host exceptional
  behavior; ordinary magnitude precision remains Rust-dependent.

These native method calls are not added to compile-time evaluation. Existing scalar
const operations and casts are unchanged. Native associated constants, a public RNG,
full ray tracing and broader JIT/native execution are not supplied by this inventory.

## Explicit numeric conversions

There are no implicit integer-width, signedness, or integer/float conversions.
`as` is an explicit numeric cast, evaluated once. It binds tighter than
multiplication and looser than unary operators. Parenthesize the cast before `<`
or `<<` when the tokens would otherwise begin generic type arguments.

- Integer to integer: widening sign-extends signed sources and zero-extends
  unsigned sources; narrowing preserves the low destination bits; equal-width
  signedness changes reinterpret the same bits.
- Integer to float: round to the nearest representable value, ties to even.
- Float to integer: truncate toward zero, saturate to the destination range,
  and convert NaN to zero. Infinite values saturate too.
- Float to float: f32 widens exactly to f64; f64 narrows to f32 using the host's
  Rust cast behavior. This does not promise cross-platform NaN payload identity.
- Bool to integer: false becomes zero and true becomes one. Other boolean casts,
  object casts and user-defined `as` hooks are unsupported.

The language's `From<S>` contract supports error conversion by `Result ?`,
including identity and lossless primitive cases. Into and TryInto derive from
From and TryFrom; fallible conversions preserve the destination's associated error.
FromStr and primitive parsing are also foundation contracts; see the
[foundation inventory](builtins.md#foundation-trait-inventory). They do not imply restoring
the whole parsing/helper catalog. Const numeric casts share runtime conversion
semantics; trait calls remain outside scalar const-safe evaluation.

See [numeric conversions](../../examples/syntax/numeric-conversions.kgr).

## Repeat arrays and bulk replacement

`[value; count]` constructs a fresh builtin `[T]` array whose length remains fixed.
Length is not a type argument; rebinding a `var` to another array changes only that
slot. Existing aliases retain the original object and length.
Evaluate `value`, then the `u64` count, exactly once. The value is evaluated even
for zero length. Repetition requires a type proven to contain no shared mutable
object identity: scalars, String and integer ranges qualify; Tuple and enum payloads
are checked recursively across every variant. Structs, containers (including
read-only views), interfaces, closures and host handles do not qualify. This rule
also applies at lengths zero and one and to an empty variant such as `None` in
`Option<Struct>`. An unconstrained generic element type cannot prove the requirement.
The count may be a runtime expression. Checked allocation arithmetic and cooperative
cancellation still apply. A valid u64 count need not be an allocatable size; see
[machine and resource boundaries](runtime.md#machine-and-resource-boundaries).

Objects requiring separate identity must be initialized separately, for example
`[Cell { value: 1 }, Cell { value: 1 }]`. A repeated reference can be expressed
explicitly as `[cell, cell]`. Optional factory/fill/copy algorithms are deferred.

## Ranges and interval copying

Range expressions are immutable bounds values rather than arrays. See
[range syntax and iteration](syntax.md). Iterators own their cursor; assigning a
range copies its bounds, and iterating it twice creates independent cursors.
RangeBounds belongs to the language. Optional interval-copy algorithms remain
deferred and do not add methods to the foundational collection interfaces.
