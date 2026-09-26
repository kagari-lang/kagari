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

The current implementation uses the equality rules below. The accepted extension
in [Equality and hashing](#equality-and-hashing) defines custom comparison and
identity operators; those features are not implemented yet.

Equality compares scalars and strings by value, tuples by corresponding members,
and enums by nominal type, variant, and corresponding members. Mutable objects
compare by identity. Interface values and host handles/path views do not support
general equality. A tuple/enum supports equality only if its members do.
Enum comparison uses declaration identity, applied type arguments and variant
identity; adding or reordering private variants in a later execution version does
not change equality of an existing variant with the same members.
Floating-point equality follows IEEE comparisons (`NaN != NaN`, `-0 == +0`).
Map and Set keys require the standard `Eq + Hash` protocols. Unit, bool, integers,
String, qualifying tuples/enums and mutable identity objects qualify. Float,
interface and host values do not. Keys preserve the equality rules above; mutable
object contents never affect key equality or hashing. Containers trace retained
keys as well as values. See [standard protocols](builtins.md#collection-types).

Iteration prevents structural mutation of the iterated collection through any
alias: insert, remove, clear, reorder, and length-changing operations fail before
changing it. Replacing an existing element without changing structure is allowed.
Mutating an object referenced by an element is allowed. Iteration protection ends
on exhaustion, break, return, or failure, including nested iteration.

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

Status: accepted target contract, pending implementation. The current compiler
still seals PartialEq/Eq/Hash and does not support `===` or `!==`. Examples in
this section describe the target behavior, not runnable coverage. Other current
implementation descriptions above remain accurate until this extension lands.

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

An ordinary Struct with no explicit equality/hash implementations receives
identity-based PartialEq, Eq and Hash. Both `==` and `===` are therefore available
and agree by default. Custom implementations do not change assignment, aliasing,
GC ownership or shallow-copy behavior.

| Explicit Struct implementations | `==` / `.eq()` | `===` | Hash and key eligibility |
| --- | --- | --- | --- |
| None | Identity | Identity | Identity Hash; satisfies Eq + Hash |
| PartialEq only | Custom comparison | Identity | No implicit Eq or Hash; not a key |
| PartialEq and Eq | Custom comparison | Identity | No implicit Hash; not a key |
| PartialEq, Eq and Hash | Custom comparison | Identity | Custom Hash; satisfies Eq + Hash |

An explicit PartialEq replaces the default comparison and removes the implicit
Eq and Hash implementations. Eq must then be declared explicitly and Hash must
be implemented explicitly when needed. Hash-only overrides retaining default
identity comparison are rejected; custom key protocols use the complete
PartialEq/Eq/Hash set. Comparison alone does not require a Hash implementation.

This override policy concerns user Structs. Builtin Array, Map and Set retain
identity semantics; scalar and String comparisons retain value semantics.
Tuple, enum, Option and Result compose their members' applicable protocols,
including custom Struct comparisons, and qualify for Eq/Hash only when all
members qualify. Float retains IEEE PartialEq without Eq or Hash. This extension
does not open custom enum or host equality implementations.

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
