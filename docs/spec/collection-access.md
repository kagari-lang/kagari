# Collection Interfaces and Storage

This is the authoritative collection access and construction contract. Value,
identity and failure rules are defined in [value semantics](value-semantics.md).
Public declarations, implementations and executable API examples live in
`stdlib/array.kgr`, `stdlib/map.kgr` and `stdlib/set.kgr`.

## Types

| Read-only interface | Writable interface | Initial concrete implementation |
| --- | --- | --- |
| `List<T>`, abbreviated `[T]` | `MutableList<T>: List<T>` | `ArrayList<T>` |
| `Map<K, V>` | `MutableMap<K, V>: Map<K, V>` | `LinkedHashMap<K, V>` |
| `Set<T>` | `MutableSet<T>: Set<T>` | `LinkedHashSet<T>` |

These are ordinary source-declared traits, usable as generic bounds or dynamic
interface types. Script structs can implement them. The concrete classes supply
native storage; the interface does not select an allocator or implementation.
`[1, 2]`, `[value; count]` and `ArrayList::from_fn` create `ArrayList` objects.
`[T]` means read-only `List<T>`, not fixed-size storage, a slice or a borrow.

Map and Set interfaces do not impose Eq/Hash or traversal order. The initial
LinkedHash implementations require `Eq + Hash` on keys/elements and preserve
insertion order. Equal map keys keep the last inserted value; sets discard equal
duplicates. Stored key equality and hashes must remain stable, including changes
through aliases. Tree-based and other concrete implementations are future work.

## Read-only access is shallow and live

Binding mutability, container mutation and element mutation are independent:

- `val` prevents rebinding; `var` permits it.
- `List`, `Map` and `Set` hide container mutators.
- An element's own type still controls its fields and nested containers.

```kgr
struct Cell { var value: i32 }
fn main() {
    val storage = [Cell { value: 1 }];
    val writable: MutableList<Cell> = storage;
    val readable: [Cell] = writable;
    readable[0].value = 42;
    writable.push(Cell { value: 7 });
    std::debug::assert_eq(readable.len(), 2usize, "live view");
}
```

Replacing `readable[0]` or calling `readable.push(...)` is rejected. Updating its
referenced Cell is permitted and requires no container writeback. Read-only views
provide neither deep freezing nor snapshots. Native structural iteration guards
cover the underlying object across all views and aliases.

## Conversion and dispatch

Concrete implementations convert implicitly to their implemented interface.
Mutable interfaces upcast to the read-only parent; reverse conversion and
implicit downcasts are rejected. Type arguments are invariant: converting an
outer container does not recursively convert elements or grant nested writes.
Conversions apply to arguments, assignments, returns, fields and contextual
expressions. Branches of the same collection family can join at a read-only view.

The underlying object identity survives interface wrapping and upcasting.
Collection-interface `==`, `===` and hashing use that identity, including repeated
views of a custom implementation. Native storage equality remains identity-based;
this change does not introduce structural collection equality. Other unrelated
interfaces retain their existing equality restrictions.

Concrete native operations retain intrinsic fast paths. Generic calls select a
concrete implementation during compilation; dynamic calls use the existing
interface tables. Creating a view can allocate an interface wrapper, which retains
its data object and execution version. Views do not copy collection storage.

## Shared interface surface

| Interface | Members, in addition to its parents |
| --- | --- |
| `List<T>` | `len`, `is_empty`, `get`; inherits `Index<usize, Output=T>` and `Iterable<Item=T, Iter=Iter<T>>` |
| `MutableList<T>` | `push`, `pop`, `insert`, `remove`, `clear`, `set` |
| `Map<K,V>` | `len`, `is_empty`, `contains_key`, `get`; inherits `Iterable<Item=(K,V), Iter=Iter<(K,V)>>` |
| `MutableMap<K,V>` | `insert`, `remove`, `clear` |
| `Set<T>` | `len`, `is_empty`, `contains`; inherits `Iterable<Item=T, Iter=Iter<T>>` |
| `MutableSet<T>` | `insert`, `remove`, `clear` |

List interface indices use `usize`; unsuffixed literals receive that context.
Native ArrayList indexing additionally accepts the existing integer index types.
Out-of-bounds `get`/`remove` return None; invalid indexing, `set` and `insert` trap.
List insert permits an index equal to the length. `pop` returns None when empty.
Map remove returns the previous value or None; Set remove returns a boolean.
Interface push/insert/set/clear return unit. Existing native fluent push/insert
methods continue returning their concrete receiver.

Native-only operations such as `fill`, `copy_within`, `copy_from`
and set algebra remain on the concrete implementation in this batch. The source
API shows this boundary explicitly. Iterators retain their existing lazy methods.

`LinkedHashMap.keys()`, `values()` and `entries()` return read-only `List<K>`,
`List<V>` and `List<(K,V)>` snapshots, respectively. They preserve insertion order
and allocate independent slots; later map changes do not change the snapshot.
Referenced key/value objects remain shared. Use `ArrayList::from(snapshot)` for
an explicitly writable copy, or `map.iter()` to traverse without a snapshot.

String lists provide `join(separator)` through their read-only contract, so
`map.keys().join(", ")` works for String keys. String-yielding iterators also provide
join; other elements require explicit formatting. See [string construction](builtins.md#string-construction).

Custom implementations must uphold their interface contracts, including the
single-mutation failure guarantee and consistency of get/index/iteration. The
language cannot automatically roll back arbitrary code in a user method.

## Construction and copying

Constructors and `FromIterator` belong to concrete types:

```kgr
val storage = ArrayList::from([10, 20]);
val list: List<i32> = storage;
val map: Map<String, i32> = LinkedHashMap::from([("answer", 42)]);
val set: Set<i32> = LinkedHashSet::from([10, 20, 10]);
val collected: ArrayList<i32> = list.iter().map(|x| x + 1).collect();
val copied = ArrayList::from(list);
```

Use `ArrayList::new`, `LinkedHashMap::new` and `LinkedHashSet::new` for empty
storage. Context or explicit type arguments supply otherwise unknown element
types. Interfaces do not have `new`, `from` or a blanket `FromIterator` that
silently chooses storage. Result/Option collection likewise names a concrete
inner destination, for example `Result<ArrayList<T>, E>`.

The three `from` factories accept read-only List inputs and allocate fresh shallow
storage. Map inputs contain `(K,V)` pairs. Referenced elements retain identity,
but input slots are not retained. `ArrayList::from(readable)` obtains a writable
copy without upgrading the original view. `copy_from` accepts a List and
snapshots its iteration before committing the destination replacement. Source
callbacks can have side effects; failures do not commit the destination copy.
Factories and copying release temporary roots and iteration guards on failure.
Previously completed callback effects are not rolled back.

`[value; count]` retains its value-only repetition rule; use `from_fn` for distinct
objects. See [array repetition](value-semantics.md#repeat-arrays-and-bulk-replacement).

## Compiler, artifacts and host boundaries

The standard declaration catalog owns interface signatures, docs, source
locations and native impl witnesses. HIR owns coercions, parent interface facts
and selected methods. Completion exposes only the members of the visible type.
Native bridge functions are ordinary verified code with concrete signatures;
interface contracts and parent tables are verified and linked before execution.
Bytecode rejects forged receiver upgrades and raw storage writes through an
interface. KBC and runtime ABI v86 encode native bridges, normalized snapshot
operations and the copy intrinsic; older products are rejected without compatibility
decoding.

The host ABI continues to describe native storage access independently of the
script interface hierarchy. Native read-only host arrays/maps/sets retain their
restricted capabilities; they cannot be converted to mutable collection views.
Trusted hosts remain responsible for their declared effects and roots. GC,
resource budgets, pinned versions and cleanup apply equally to native and script
interface implementations. This batch does not add a new JIT backend.

## Validation

Coverage includes native and script implementations, generic and dynamic calls,
mutable-to-read-only upcasts, live aliasing, indexed compound assignment, custom
Eq/Hash keys, interface identity, malformed artifacts and declaration navigation.
Tests run through source, encoded artifacts and existing JIT fallback, including
GC on every allocation. Standard API documentation blocks compile and execute.

Readonly Map exposes keys/values/entries as standard traversal defaults. They
produce independent shallow List snapshots in the implementation's iteration
order, without imposing Eq/Hash on the interface. Native concrete maps retain
their direct snapshot path. These traversal defaults cannot be overridden.
The writable array copy operation is named copy_from; the former slice-oriented
spelling is removed, without an alias.

List provides first/last, membership, prefix/suffix matching and binary_search as
standard defaults, including for user implementations. Membership and positional
matching require PartialEq; binary_search requires Ord and ascending sorted input.
It returns Ok(a matching index) or Err(the insertion index); duplicate matches may
select any equal position. Empty prefixes/suffixes match. Multi-element queries
retain an iteration guard while comparison callbacks run, rejecting structural
changes through aliases of native backing storage. Custom List implementations
must keep len/get/index and iteration consistent. The default methods are sealed.
