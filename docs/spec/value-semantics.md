# Values and Expression Evaluation

This is the authoritative v1 contract for observable value behavior. Physical
allocation, Rust `Clone`/`PartialEq`, and backend choices do not define semantics.

## Values and identity

Scalars, strings, tuples, and enums (including Option and Result) have value
semantics. Tuple and enum copies preserve the semantics of each member; copying
a member that refers to a mutable object does not copy that object. Enum payloads
cannot be reassigned in place.

Structs, arrays, maps, and sets are mutable identity-bearing objects. Assignment,
argument passing, and returns share their identity. `val` prevents rebinding a
slot, not mutation of the referenced object. Container copy operations are
shallow. There is no generic deep-copy or deep-freeze operation in v1.

An interface may retain a checked durable host root as its concrete payload;
this does not make host borrow tokens or path views valid heap payloads. See the
[host interface contract](traits.md#host-associated-outputs-and-interfaces).

[Equality and hashing](#equality-and-hashing) defines explicit protocol selection
and type-specific defaults. Without overrides, scalars and strings compare by
value, tuples by corresponding members, enums by nominal type, variant and
corresponding members, and mutable objects by identity. Composite defaults use
the selected member implementations. Interface values and host handles/path
views do not support general equality.

Default enum comparison uses declaration identity, applied type arguments and
variant identity; adding or reordering private variants in a later execution
version does not change comparison of an existing variant with the same members.
Floating-point equality follows IEEE comparisons (`NaN != NaN`, `-0 == +0`).
Map and Set keys require `Eq + Hash` and trace retained keys as well as values.
Default object identity is unaffected by content mutation; custom equality/hash
requires the user to preserve equality-relevant state while keys are stored.
See [standard protocols](builtins.md#collection-types).

Iteration prevents structural mutation of the iterated collection through any
alias: insert, remove, clear, reorder, and length-changing operations fail before
changing it. Replacing an existing element without changing structure is allowed.
Mutating an object referenced by an element is allowed. Iteration protection ends
on exhaustion, break, return, or failure for native for loops, including nested
iteration. Direct native cursors and custom iterator wrappers follow the
[iteration protocol lifecycle](builtins.md#iteration-protocols).

String lengths and slices use byte offsets; slices validate UTF-8 boundaries.
Unicode scalar counting is a separately named operation. Integer arithmetic is
checked in every backend and build mode. Explicit wrapping operations are the
only exception. Floating-point optimization must preserve specified results.

Current unsuffixed numeric literals are i32 and f32. Integer magnitudes must fit
i32, except that the magnitude in `-2147483648` forms the i32 minimum value.
Negating that value again traps. Float literal conversion must produce a finite
f32; runtime floating-point operations retain their IEEE behavior. Invalid
literal ranges are diagnosed during analysis, including in const initializers
and match patterns. A literal pattern must have the scrutinee's type.

## Equality and hashing

The operators, defaults and overrides below are implemented. Runnable coverage
is provided by [standard-traits.kgr](../../examples/syntax/standard-traits.kgr)
and the standard-protocol integration tests.

### One implementation selection rule

For a given concrete type, use its explicit protocol implementation when one
exists; otherwise use the type's eligible builtin default. This rule applies
equally to user Structs and enums. The selected implementation is the same in
operators, method calls, generic code and containers; imports or call sites must
not silently select a different comparison. Assignment and copying retain their
existing semantics regardless of custom equality.

### Operators and defaults

`==` calls the type's PartialEq comparison; `!=` negates that result. Eq extends
PartialEq without adding a method: it promises an equivalence relation, including
reflexivity, symmetry and transitivity. Eq does not mean field equality or
immutability. `.eq()` and generic comparisons use the same implementation as `==`.

`===` and `!==` compare object identity and cannot be overridden. They apply to
Struct, Array, Map and Set, using stable runtime-owned identity rather than a
physical address. Scalar, String, Tuple and enum values do not acquire identity
operators merely because their implementation allocates storage. Interface and
host identity comparisons are outside this extension.

The defaults differ by type, but follow the same selection rule:

| Type | Default comparison | Default hash | User override |
| --- | --- | --- | --- |
| Struct | Object identity | Stable object identity | Allowed |
| User enum | Same concrete enum type and variant, then corresponding members | Combine enum/variant identity and member hashes | Allowed |
| Tuple | Corresponding members in position order | Combine member hashes in position order | No independent override |
| Option / Result | Standard enum variant and member comparison | Standard enum variant and member hashes | No independent override |
| Array / Map / Set | Object identity | Stable object identity | No override |
| Unit / bool / integers / String | Value | Value | No override |
| Float | IEEE comparison | None | No override |

Struct defaults supply PartialEq, Eq and Hash regardless of field types. Default
Tuple and enum protocols are conditional, as described below. Interfaces, host
handles/paths and function values remain outside general equality and hashing;
this extension does not open host equality implementations.

### Explicit Struct and enum implementations

Struct and user enum declarations permit the same explicit implementations.
Implementations of these canonical protocols must be in the type's defining
module. This prevents a downstream import from changing equality for values
already stored by a dependency. Generic implementations are selected for each
concrete instantiation under their declared bounds:


| Explicit implementations | `==` / `.eq()` | Eq and Hash eligibility |
| --- | --- | --- |
| None | Eligible type default | Eligible type defaults |
| PartialEq only | Custom comparison | No implicit Eq or Hash; not a key |
| PartialEq and Eq | Custom comparison | Explicit Eq, no implicit Hash; not a key |
| PartialEq, Eq and Hash | Custom comparison | Explicit Eq and custom Hash; key eligible |

An explicit PartialEq replaces the default comparison and removes the implicit
Eq and Hash implementations. Eq must then be declared explicitly and Hash must
be implemented explicitly when needed. Hash-only overrides retaining default
comparison are rejected, as are Eq-only declarations; custom key protocols use the complete
PartialEq/Eq/Hash set. Comparison alone does not require a Hash implementation.

An enum's explicit comparison replaces its entire default comparison, including
the variant check. It may consider different variants equal. Its custom hash
must then agree with that decision; the runtime must not prepend a variant tag
or perform a variant inequality check before the user implementation. An explicit
enum implementation is checked under its declared bounds, not an automatic
requirement that every payload implement the same protocol.

Custom comparison does not change GC ownership, shallow copies or identity
operators. Struct `===` remains identity comparison even when `==` is customized;
enum values do not gain `===` by implementing comparison.

### Member composition

Default Tuple comparison applies `==` to corresponding members, left to right,
stopping at the first unequal member. Default enum comparison first checks the
variant, then compares that variant's payload members in declaration order in
the same way. A payload-free variant equals the same variant of the same
concrete enum type. Different concrete types are not made comparable by this
rule. The members' selected implementations may be builtin or user-defined;
there is no separate equality rule for composites containing custom members.

Default hashing combines member hashes in the same member order, and enum
hashing also includes enum and variant identity. A Tuple or default enum
implements PartialEq, Eq or Hash only when all member types support the
corresponding protocol. This checks every enum variant, not only the currently
constructed variant. Option and Result follow these same default enum rules.
The all-members rule applies to default implementations, not explicit enum
overrides. Floats retain PartialEq without Eq or Hash when nested in a composite.

For example, under the target contract:

```kagari
struct Point { val x: i32, val y: i32 }
impl PartialEq for Point {
    fn eq(self, other: Self) -> bool {
        self.x == other.x && self.y == other.y
    }
}
impl Eq for Point {}
enum Message { Empty, Position(Point) }

fn compare() -> bool {
    val a = Point { x: 1, y: 2 };
    val b = Point { x: 1, y: 2 };
    // Both composite comparisons use Point's custom comparison.
    (a, 7) == (b, 7) && Message::Position(a) == Message::Position(b)
}
```

`compare()` returns true; `a === b` would be false. Point has no Hash here, so
neither `(Point, i32)` nor Message is eligible as a hash key. Adding a matching
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
Tuple, another enum or a Set. Without these overrides, the default variant
comparison makes them unequal. The custom hash does not include the variant.

### Execution paths

Builtin identity and scalar operations retain direct execution paths; String
uses builtin content operations. Composites use their members' selected paths,
calling user implementations only where required. Compilation/linking can select
these paths for concrete instances, including monomorphized generic code. These
are execution choices, not different observable equality contracts. A fast path
must not bypass a selected user implementation or change comparison order and
failure behavior. `===` never invokes a user callback.

### Hash containers and user obligations

Map keys and Set elements continue to require `Eq + Hash`, including in generic
code. A Map's values do not need those bounds. Lookup hashes the key, then uses
the same equality implementation as `==` to distinguish candidate keys.

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

For example, a default `User { var id: i32 }` compares and hashes by identity:
changing `id` does not affect membership and a different User with the same id
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
updates which would not change its length. Trap and budget termination release
the lookup guard and temporary roots.

Builtin-only keys retain native hashing and lookup. Composite values containing
custom members use reusable compiled comparison/hash helpers. These helpers and
user callbacks run through ordinary linked calls, logical budgets and GC
safepoints; JIT-ineligible paths use the interpreter.


## Evaluation and assignment

Operands, receivers, arguments, and dynamic indexes evaluate left to right,
exactly once. Short-circuit operators evaluate only the selected operand.

For `root()[index()].field += rhs()`, evaluation is:

1. Evaluate the root and indexes and retain a location description.
2. Evaluate the right-hand side.
3. Validate the location and read its current value.
4. Compute and validate the replacement, including overflow and resource checks.
5. Commit one modification.

Plain assignment follows the same order without reading the old value. A path
denotes the current location, not a detached Rust reference or a pinned element.
If RHS execution removes the location, the final write fails. RHS effects already
performed remain visible. No mutable host borrow spans RHS execution.

The compound forms are `+=`, `-=`, `*=` and `/=` and require matching numeric
operands. The root of a mutable object location retains the identity evaluated
before the RHS; rebinding the root variable does not retarget it. Captured indexes
are applied to the current contents after the RHS, so replacing an intermediate
element changes which current field is modified. A local scalar or tuple slot is
read after the RHS. Updating a tuple member prepares a new tuple value and thus
requires a writable enclosing slot; modifying a mutable object referenced by a
tuple does not replace the tuple.

## Acceptance examples

- Mutating `b` after `val b = a` changes the object observed through `a`.
- Separately allocated mutable objects with identical fields compare unequal.
- Separately constructed equal enum values compare equal.
- A shallow copy has new container identity but shares referenced elements.
- Structural mutation through an alias during iteration fails without mutation.
- A compound assignment evaluates root/index/RHS once, reads after RHS, and never
  writes after a failed RHS, invalidated location, or overflow.

The shared `language_contract` suite checks Map and Set identity through arguments
and return values, including distinct collections with equal contents. It also
checks `Map.values()` sharing mutable element objects while creating independent
array structure, and `Set.to_array()` allowing independent element replacement
and growth. Enum/tuple member comparisons retain Map identity after mutation.
These fixtures run through source, artifact loading and the existing JIT/fallback.

The runtime callback entry for `std::iter::for_each` holds an iteration guard for
Array, Map and Set until callbacks finish or fail. Hosts use
`GcHeap::begin_collection_iteration` when they retain the same iteration scope;
the guard roots the collection and nested guards release independently. Structural
builtin failures occur before result allocation or collection mutation. Updating
an existing Map key and inserting an already-present Set member do not change
structure. Array element replacement remains allowed.

This callback entry visits a shallow entry snapshot in insertion/index order.
Pending snapshot values are explicit roots, so callback-driven replacement and
collection do not invalidate later callback arguments. This runtime API is a
foundation for source iteration; full source callback/for-loop lowering and its
exit-path acceptance remain tracked separately in the foundation roadmap.

Heap pop/remove/clear APIs return `Result` for operational failure. `Ok(None)`
from Array pop/remove or Map remove, and `Ok(false)` from Set remove, indicate
normal absence only. Invalid keys/handles, iteration protection and execution
rejection are errors. Clear returns `Result<(), RuntimeError>`. Standard helpers
preserve this distinction and check iteration protection before allocating their
script-level Option result; rejected operations do not change contents or quota.

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
