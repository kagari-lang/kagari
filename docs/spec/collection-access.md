# Collection access and construction proposal

Status: planned, not implemented. This document records the proposed collection
API and implementation acceptance criteria. Current `[T]`, `Map<K, V>` and
`Set<T>` are writable shared objects, and Map/Set constructors remain module
functions. The [implementation roadmap](../implementation-roadmap.md#collection-access-and-construction)
tracks the transition; the current [value contract](value-semantics.md) remains
authoritative until the implementation checkpoints land.

C01a has added access metadata to HIR, ABI and host interface types, with bounded
encoding and fingerprint coverage. This is a representation foundation, not an
implemented read-only API or execution guarantee. Instruction-level access
validation and the public declaration migration remain pending.

## Type surface

| Collection | Read-only access | Writable access |
| --- | --- | --- |
| Resizable array | `Array<T>`, abbreviated `[T]` | `MutableArray<T>` |
| Hash map | `Map<K, V>` | `MutableMap<K, V>` |
| Hash set | `Set<T>` | `MutableSet<T>` |

The array spelling is agreed for this design, but is not implemented yet.
`[T]` describes a resizable array viewed through read-only access; it is
not a slice, fixed-size array, or Rust borrow. An array literal produces a new
`MutableArray<T>`, which can be passed or assigned to a read-only array type.
An explicit `[T]` annotation therefore removes write access through that binding.

These are native access types over shared storage, not user-implementable
collection interfaces. Existing Iterator/IntoIterator protocols remain the
extension point for custom iteration. General collection-interface hierarchies
and generic variance are separate features.

## Binding, access and element mutation

Three independent rules apply:

1. `val` prevents rebinding; `var` permits rebinding.
2. The collection type controls modification of its entries or elements.
3. The element's own type and field declarations control modification of objects
   obtained from the collection.

The following examples describe proposed behavior, not runnable current examples:

```kgr
val writable: MutableMap<String, i32> = MutableMap::new();
writable.insert("score", 10);

val readable: Map<String, i32> = writable;
std::debug::assert(readable.get("score") == Some(10), "shared entry");
writable.insert("score", 20);
std::debug::assert(readable.get("score") == Some(20), "live view");

// Rejected: a read-only reference cannot modify entries.
readable.insert("score", 30);
```

```kgr
val writable = [10, 20];
writable.push(30);
val readable: [i32] = writable;

// Rejected: both operations require writable array access.
readable.push(40);
readable["".len_bytes()] = 40;
```

Read-only access prohibits replacement as well as structural modification.
`array[i].field = value` can still modify a `var` field on a referenced Struct
obtained from a read-only array. It does not replace `array[i]`. A nested writable
container likewise retains its declared write access when read from an outer
read-only container. No recursive freezing or copying is implied.

A read-only view is live: other writable aliases can change the same object.
Consequently, it guarantees neither a stable snapshot nor stable hash keys.
Equality/hash-relevant key state must still remain stable while stored.

Iteration guards continue to apply to the underlying object across all aliases.
Reading through a read-only view does not allow a writable alias to bypass an
active structural-iteration guard. Existing failure atomicity, GC ownership and
rooting rules remain in force.

## Conversions and inference

Writable-to-read-only conversion is an implicit, allocation-free weakening of
access for the same collection kind and exactly the same type arguments:

- `MutableArray<T>` to `Array<T>`;
- `MutableMap<K, V>` to `Map<K, V>`;
- `MutableSet<T>` to `Set<T>`.

Conversion preserves object identity and hash. Both access types retain the
existing identity-based default equality/hash behavior. Mixed-access identity
and equality comparisons use the common read-only view when type arguments match.
This proposal does not adopt Kotlin's structural collection equality.

Assignment, argument passing, returns and field initializers support the same
conversion. Conditional and match expressions with otherwise identical mutable
and read-only collection branches can use the common read-only result type.
An unconstrained inferred variable retains the actual writable type of a literal
or mutable constructor. A read-only annotation must not be widened by inference.

Read-only-to-writable conversion is forbidden, even when storage was originally
created by a mutable constructor. Rebinding a `var` does not upgrade its declared
read-only type. Reflection, generic calls and interface conversion must not
recover write access from that view.

Type arguments are invariant in this first implementation. In particular,
`MutableArray<MutableArray<T>>` does not implicitly become
`MutableArray<Array<T>>`: that would permit inserting a read-only element into
storage whose other aliases expect writable elements. Nor does the initial
design introduce recursive conversions inside Option, Result, tuples or user
generic types. Construct such values with the required member types explicitly.

Access conversion is directional assignability, not symmetric type equality or
generic unification. Generic constraints and concrete instances preserve the
qualified type. Both access types support their eligible read-only protocols;
writing always requires a writable receiver.

## Constructors and standard signatures

Empty constructors become type-associated functions:

```kgr
val array: MutableArray<i32> = MutableArray::new();
val map: MutableMap<String, i32> = MutableMap::new();
val set: MutableSet<String> = MutableSet::new();

val empty_array: Array<i32> = Array::new();
val empty_map: Map<String, i32> = Map::new();
val empty_set: Set<String> = Set::new();
```

Each constructor allocates a fresh object and returns its declared access type.
Read-only empty constructors grant no script write access to the new object;
they do not establish a separate frozen representation. Populated read-only
values can be built with writable access and returned through a read-only type.
Array literals remain available. Paired `from` factories below are included in
this batch. Capacity, general copy/clone protocols and freeze APIs remain separate.

### Populated collection factories

Provide both read-only and writable construction from initial contents. The
proposed spelling uses associated `from` functions alongside `new`, rather than
adding global camelCase factory names:

```kgr
val names = Array::from(["Ada", "Lin"]);
val editable_names = MutableArray::from(["Ada", "Lin"]);

val scores = Map::from([("Ada", 10), ("Lin", 20)]);
val editable_scores = MutableMap::from([("Ada", 10), ("Lin", 20)]);

val roles = Set::from(["reader", "writer"]);
val editable_roles = MutableSet::from(["reader", "writer"]);

editable_names.push("Sam");
editable_scores.insert("Sam", 30);
editable_roles.insert("admin");
```

The corresponding receiver-free signatures are:

| Associated function | Input | Output | Bounds |
| --- | --- | --- | --- |
| `Array::from` | `items: [T]` | `Array<T>` | None |
| `MutableArray::from` | `items: [T]` | `MutableArray<T>` | None |
| `Map::from` | `entries: [(K, V)]` | `Map<K, V>` | `K: Eq + Hash` |
| `MutableMap::from` | `entries: [(K, V)]` | `MutableMap<K, V>` | `K: Eq + Hash` |
| `Set::from` | `items: [T]` | `Set<T>` | `T: Eq + Hash` |
| `MutableSet::from` | `items: [T]` | `MutableSet<T>` | `T: Eq + Hash` |

These are associated factories, not new builtin `From` trait implementations.
The first batch accepts arrays using existing array and Tuple syntax. It does not
introduce variadic parameters, spreading, an infix `to` operator, or unrestricted
iterator inputs. A mutable input array is accepted through ordinary read-only
access weakening. Nonempty inputs infer their element/key/value types; empty
inputs need sufficient context, such as an explicit result annotation:

```kgr
val empty: Map<String, i32> = Map::from([]);
```

Each call allocates a fresh collection, including empty inputs. Factories never
return the input container itself, and do not retain its entry slots. In
particular, `Array::from(writable)` is a shallow snapshot of the element slots;
ordinary `val view: Array<T> = writable` remains a live view. Replacing an input
element or changing the input length later does not change the constructed
collection. Referenced element objects retain identity and remain mutable where
their own types permit it. `MutableArray::from(readable)` thus obtains a writable
copy without upgrading access to the original collection.

Inputs are evaluated once, left to right, before the factory call; tuple keys are
evaluated before their values. Factories traverse the resulting input in index
order and use the same selected Eq/Hash implementations as normal insertion. For
duplicate Map keys, the last supplied value wins. Sets discard equal duplicates.
Iteration order and representative key/element identity retain the existing
insertion contract; these factories add no new sorting guarantee. See Kotlin's
[mapOf](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/map-of.html)
and [mutableMapOf](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/mutable-map-of.html)
for the paired factory and last-value convention being adapted.

The input stays structurally guarded while native construction reads it, including
during user Eq/Hash calls. The private destination becomes observable only after
successful construction. A trap, cancellation or budget failure returns no partial
result and releases construction roots and guards through ordinary session cleanup.
Earlier argument effects and effects completed by user Eq/Hash calls are not
rolled back. The factory itself does not write to the input array or element
objects. Custom key equality/hash stability remains the caller's responsibility.

Both the input and in-progress destination must remain rooted across callbacks,
GC and host reentry. Implement the two access variants with shared allocation and
insertion machinery, while retaining distinct checked return types and declaration
identities. Public signatures, documentation and examples belong in `stdlib/*.kgr`.

Replace `std::map::new()` and `std::set::new()` without compatibility aliases.
Read-only and writable constructor bindings must be source-owned declarations
with distinct semantic member identities, even if they share an allocator.
Navigation must resolve each associated function to its declaration.

For the existing standard surface:

| Operations | Receiver/input access | Result access |
| --- | --- | --- |
| Array len/is_empty/get/join | Read-only | Scalar, String or existing element type |
| Array push/pop/insert/remove/clear | Writable | Existing payload result, or writable receiver |
| Map len/is_empty/contains_key/get | Read-only | Scalar or existing value type |
| Map insert/remove/clear | Writable | Existing payload result, or writable receiver |
| Map keys/values/entries | Read-only | Fresh writable shallow array |
| Set len/is_empty/contains | Read-only | Scalar |
| Set insert/remove/clear | Writable | Existing scalar result, or writable receiver |
| Set to_array | Read-only | Fresh writable shallow array |
| Set union/intersection/difference | Read-only for both inputs | Fresh writable set |

Existing mutation return conventions are retained in this proposal: for example,
Map insert returns the receiver, not Rust's previous-value Option. Constructor
syntax alignment does not silently change unrelated API behavior.

Read methods accept either access type through the permitted conversion. Free
functions and method views use the same signatures: `std::map::insert(readable,
key, value)` is rejected just like `readable.insert(key, value)`. Mutable results
that alias the receiver cannot be obtained through a read-only receiver.
Iteration and formatting consume either access type without changing access to
the object. Iterator state itself remains mutable; yielded elements retain their
declared types.

## Implementation boundaries

- **Declaration sources:** declare all six types and associated constructors in
  `stdlib/*.kgr`. Extend declaration parsing and generated binding metadata as
  needed; do not add parallel handwritten public signature tables. Validate
  owners, duplicate members, receiver access and intrinsic contracts at build time.
- **HIR:** retain collection access in concrete types, substitutions, generic
  instance keys, assignability, branch joins and source queries. Preserve normal
  error recovery. Completion must omit unavailable write methods, and diagnostics
  should name the writable type required by an attempted mutation.
- **Writes:** check method calls, free functions, direct and compound index
  assignment, reflection helpers and typed paths. Resolve the actual object being
  modified: replacing an element and modifying its referenced object differ.
- **IR and artifacts:** carry enough access information through verified call and
  storage contracts to reject forged writes and access upgrades before execution.
  Merely erasing both types to an unqualified reference before verification is
  insufficient. Include access in canonical encoding and fingerprints; increment
  relevant format/ABI versions and reject old products without migration.
- **Host bindings:** encode access in offline signatures and check runtime binding
  consistency. Trusted host owners may keep writable aliases, just as script
  callers can; a read-only script view is not a security or deep-freeze boundary.
  Host arguments and results must obey their declared access contracts.
- **Runtime and JIT:** reuse Array/Map/Set allocation, tracing, identity and
  mutation machinery. Do not put a global read-only flag on the shared object:
  writable and read-only aliases coexist. Validated access weakening requires
  neither a wrapper allocation nor interface dispatch. Both backends consume the
  verified contracts and preserve existing evaluation and commit order.

## Acceptance cases

1. Mutable constructors and literals infer writable types; explicit read-only
   annotations remove write access. `val` writable mutation succeeds; rebinding
   a `val` still fails. `var` read-only mutation still fails.
2. Mutable-to-read-only assignment, arguments, returns, fields and branch joins
   work. Reverse conversions, generic upgrades and nested invariant violations
   produce diagnostics. Erroneous neighboring code remains queryable.
3. Methods, qualified functions, index replacement, compound assignment,
   reflection and typed paths consistently reject writes through read-only access.
   Mutation of a referenced object's writable field remains valid.
4. Views observe changes through writable aliases, preserve identity/hash, survive
   GC and retain foreign/stale-handle rejection. Iteration guards cover all aliases.
5. Native read-only results cannot expose a writable alias to their receiver.
   Fresh result containers are writable and shallow; referenced objects keep identity.
6. Standard declaration documentation, signatures, constructor navigation and
   incomplete-member completion match the actual checked API.
7. Source and artifact execution, interpreter and existing JIT paths agree. Forged
   access contracts and old formats are rejected before execution. Host declaration
   fingerprints distinguish access changes.
8. All six populated factories infer types, accept context-typed empty arrays and
   return their declared access type. Repeated Map keys keep the last value; Set
   duplicates use selected Eq/Hash. Constructor results have fresh identity even
   for empty inputs. Array copies preserve slot independence and element identity.
9. Factory argument evaluation is left-to-right and single-evaluation. Custom
   Eq/Hash failure, GC, host reentry and attempted structural mutation of input do
   not expose partial results or leak roots/iteration guards. Completed callback
   side effects retain the ordinary failure contract.

Examples above remain design snippets until the corresponding implementation
tests and runnable examples land. Each implementation checkpoint runs relevant
tests and `git diff --check`; final acceptance includes formatting, workspace
clippy and workspace tests.
