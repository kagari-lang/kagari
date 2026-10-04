# Collection Interfaces and Storage

This defines the collection access and construction contract. Value, identity
and failure rules are defined in [value semantics](value-semantics.md).
The current compiler-owned catalog defines the foundational collection traits.
The runtime foundation supplies the three default implementations through
[ordinary native registration](standard-declarations.md).
Generated .kgr files are tooling views, not the source of these declarations.

## Types

| Read-only interface | Writable interface | Canonical default implementation |
| --- | --- | --- |
| `List<T>`, abbreviated `[T]` | `MutableList<T>: List<T>` | `Vec<T>` |
| `Map<K, V>` | `MutableMap<K, V>: Map<K, V>` | `HashMap<K, V>` |
| `Set<T>` | `MutableSet<T>: Set<T>` | `HashSet<T>` |

These are complete compiler-owned trait contracts, usable as generic bounds or
dynamic interface types. Native and script types implement them through ordinary
trait checking. The contracts, parent relationships, associated outputs and
read-only/writable conversions exist without optional native libraries.

`[T]` means read-only `List<T>`, not fixed-size storage, a slice or a borrow.
Existing `[1, 2]` and `[value; count]` literals create the canonical `Vec<T>`.
Vec, HashMap and HashSet have compiler-owned nominal declarations and
minimal Rust construction, access, mutation and iteration implementations in the
runtime foundation. These use ordinary checked native bindings/storage registration
and remain available when optional modules are off. Optional providers do not
redeclare or replace the default types. Selecting defaults adds no map/set literal
syntax and does not make Map/Set interface constructors choose concrete storage.
Vec::from_fn and other callback conveniences remain library algorithms.

Concrete additional classes register native storage independently of their trait
impls. An interface does not select an allocator or implementation. Additional
list/hash/tree/queue types use the same checked registration path without extending
core value/type enums for each collection.

Map and Set interfaces do not impose Eq/Hash or traversal order. Default HashMap
requires `K: Eq + Hash`; HashSet requires `T: Eq + Hash`. These concrete types
promise neither insertion order nor sorted traversal. Equal map keys keep the
last inserted value; sets discard equal duplicates. Stored key equality and hashes
must remain stable, including changes through aliases. Optional LinkedHashMap/
LinkedHashSet provide insertion order; TreeMap/TreeSet may require Ord. Their
implementations and extension algorithms belong to library modules.

Default HashMap uses Rust `std::collections::HashMap`; HashSet uses
`std::collections::HashSet`. `indexmap` is reserved for optional standard-library
LinkedHashMap/LinkedHashSet. Storage bindings preserve checked Kagari Eq/Hash key
semantics rather than infer them from Rust types.

## Read-only access is shallow and live

Binding mutability, container mutation and element mutation are independent:

- `val` prevents rebinding; `var` permits it.
- `List`, `Map` and `Set` hide container mutators.
- An element's own type still controls its fields and nested containers.

```kgr
struct Cell { var value: i32 }
fn main() -> usize {
    val storage = [Cell { value: 1 }];
    val writable: MutableList<Cell> = storage;
    val readable: [Cell] = writable;
    readable[0].value = 42;
    writable.push(Cell { value: 7 });
    readable.len() // 2: both views refer to the same object.
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
| `List<T>` | `len`, `is_empty`, `get`; inherits `Index<usize, Output=T>` and `Iterable<Item=T>` |
| `MutableList<T>` | `push`, `pop`, `insert`, `remove`, `clear`, `set` |
| `Map<K,V>` | `len`, `is_empty`, `contains_key`, `get`; inherits `Iterable<Item=(K,V)>` |
| `MutableMap<K,V>` | `insert`, `remove`, `clear` |
| `Set<T>` | `len`, `is_empty`, `contains`; inherits `Iterable<Item=T>` |
| `MutableSet<T>` | `insert`, `remove`, `clear` |

`Iterable` declares its associated `Iter` with an `Iterator<Item = Item>` bound.
Concrete implementations supply their own iterator type; the contract does not
require a particular optional `Iter<T>` class. Checked interface metadata retains
actual associated outputs and targets for static and dynamic dispatch.

List interface indices use `usize`; unsuffixed literals receive that context.
Native Vec indexing additionally accepts the existing integer index types.
Out-of-bounds `get`/`remove` return None; invalid indexing, `set` and `insert` trap.
List insert permits an index equal to the length. `pop` returns None when empty.
Map remove returns the previous value or None; Set remove returns a boolean.
Interface push/insert/set/clear return unit. Existing native fluent push/insert
methods continue returning their concrete receiver.

## Foundation algorithms

List declares sorted, sorted_by, sorted_by_key, reversed and distinct; sort, sort_by,
sort_by_key, reverse, retain and dedup belong to MutableList. Method-specific Ord
and Eq bounds do not restrict construction of the interface itself. Key methods
accept a method-local K: Ord and are callable through interfaces. The methods
provide native defaults which custom implementations may reuse or override.

Sorting is stable. Key selectors run during comparisons without an implicit
cached-key prepass. In-place algorithms preserve container identity and do not
promise rollback on callback failure, receiver failure or cancellation. Built-in
Vec sorting preserves the original element multiset; its order may change
on failure. Length-changing operations and writes through custom containers may
leave partial progress. Completed callback effects remain visible. New-result
methods copy the collection structure without cloning referenced elements.
Comparator consistency is the caller's obligation; comparison counts are unspecified.

Native direct sequence edits hold an exclusive lease on receiver slots. Callback
access to those slots is rejected until the lease ends; unrelated values and
referenced element objects remain usable. Storage is restored on every exit path,
including errors and unwinds, with completed edits preserved.

The former sort/sort_by free functions are replaced by these trait methods.
Vec operates on its actual compact buffer. Custom defaults traverse the
selected iterator into typed working storage; sorting and reversal write through
set, while retain and dedup remove rejected elements as they proceed. No atomic
bulk commit or rollback-only storage buffer is required.

MapIterator retains its source cursor and mapper. Constructing it does not call
the mapper. Aliases share progress, and next consumes its input before invoking
the mapper. Managed cursor dependencies participate in scoped iteration guards,
including through erased Iterator interfaces. See [native declarations](standard-declarations.md).

APIs outside the plan's accepted rows, including windows/chunks, capacity APIs,
set algebra, joining, grouping and additional constructors, remain deferred.
They are not required methods of List, Map, Set or their mutable counterparts.
There are no temporary implementations or aliases for predecessor names.

## Construction

Use Vec::new, HashMap::new and HashSet::new for empty storage. Context or
explicit type arguments supply otherwise unknown element types. Interfaces do not
have constructors that silently choose a concrete implementation.

```kgr
val storage = [10, 20];
val readable: List<i32> = storage;
val map: HashMap<String, i32> = HashMap::new();
val set: HashSet<i32> = HashSet::new();
```

Array literals and `[value; count]` construct the canonical Vec. Repetition
retains its value-only rule, including empty results. Distinct reference objects
require separate construction; the application-provider factory in the reset
proof demonstrates how an optional native function supplies that algorithm.

## Compiler, artifacts and host boundaries

The compiler-owned language catalog owns foundational interface signatures,
documentation and parent/associated contracts. Tooling projects those declarations
and maps them to generated navigation spans. It also declares the three default
container types. Runtime owns their basic implementations and storage bindings;
optional native modules own additional types, impls and algorithm declarations.
HIR owns coercions and checked implementation selection. Completion exposes only
the members of the visible type. Native entries are checked against concrete
signatures; interface contracts and parent tables are verified/linked before
execution. Rust bodies remain trusted implementations, not bytecode whose
behavior is proven by declaration validation.
Bytecode rejects forged receiver upgrades and raw storage writes through an
interface. The current KBC/runtime ABI encodes native bridges and normalized collection
operations; see [artifact versions](artifacts.md). Older products are rejected
without compatibility decoding.

The host ABI continues to describe native storage access independently of the
script interface hierarchy. Native read-only host arrays/maps/sets retain their
restricted capabilities; they cannot be converted to mutable collection views.
Trusted hosts remain responsible for their declared effects and roots. GC,
pinned versions and cleanup apply equally to native and script implementations.
Storage factories, trace/drop hooks and scoped access form the generic native
object boundary; they do not introduce a new JIT backend.

## Validation

Retained coverage exercises native and script implementations, generic and dynamic
calls, mutable-to-read-only upcasts, live aliasing, indexed compound assignment,
custom Eq/Hash keys, interface identity, malformed artifacts and generated
navigation. The accepted sorting/lazy/object proof covers source and encoded
artifacts, GC during callbacks, cleanup, reload and independent source-free loading.
The [performance report](../performance-baseline.md) records scoped measurements;
coverage for retired algorithms does not imply that those APIs remain installed.
