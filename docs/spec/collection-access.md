# Collection Interfaces and Storage

This defines the collection access and construction contract. Value, identity
and failure rules are defined in [value semantics](value-semantics.md).
The 2026-10-02 [native collections reset](../native-provider-refactor.md) makes
foundational collection traits compiler-owned. This is the target contract;
the predecessor implementation is removed/replaced in the plan's phase order.
Generated .kgr files are tooling views, not the source of these declarations.

## Types

| Read-only interface | Writable interface | Initial concrete implementation |
| --- | --- | --- |
| `List<T>`, abbreviated `[T]` | `MutableList<T>: List<T>` | `ArrayList<T>` |
| `Map<K, V>` | `MutableMap<K, V>: Map<K, V>` | `LinkedHashMap<K, V>` |
| `Set<T>` | `MutableSet<T>: Set<T>` | `LinkedHashSet<T>` |

These are complete compiler-owned trait contracts, usable as generic bounds or
dynamic interface types. Native and script types implement them through ordinary
trait checking. The contracts, parent relationships, associated outputs and
read-only/writable conversions exist without optional native libraries.

`[T]` means read-only `List<T>`, not fixed-size storage, a slice or a borrow.
Existing `[1, 2]` and `[value; count]` literals create the canonical `ArrayList<T>`.
Its minimal construction, access, mutation and iteration implementation is part
of the runtime foundation and remains available when algorithm modules are off.
There is no optional provider that redeclares or replaces the same core array type.
ArrayList::from_fn and other callback conveniences remain library algorithms.

Concrete additional classes register native storage independently of their trait
impls. An interface does not select an allocator or implementation. Additional
hash/tree/queue types use the same checked registration path without extending
core value/type enums for each collection.

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
Native ArrayList indexing additionally accepts the existing integer index types.
Out-of-bounds `get`/`remove` return None; invalid indexing, `set` and `insert` trap.
List insert permits an index equal to the length. `pop` returns None when empty.
Map remove returns the previous value or None; Set remove returns a boolean.
Interface push/insert/set/clear return unit. Existing native fluent push/insert
methods continue returning their concrete receiver.

## Optional library algorithms

The algorithms and extended constructors below describe native modules when
installed. They are not additional required members of compiler-owned collection
traits or a promise to restore all predecessor APIs in the reset proof.
Sorting, search, grouping, joins and callback conveniences use ordinary registered
functions or library-owned extension traits. Compiler-owned contracts do not
contain their implementations. Other concrete collection types belong to their
own native modules.

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

The compiler-owned language catalog owns foundational interface signatures,
documentation and parent/associated contracts. Tooling projects those declarations
and maps them to generated navigation spans. Native modules own their concrete
implementation records, storage bindings and optional algorithm declarations.
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

The following describes predecessor coverage, not acceptance of the unimplemented
reset. The active plan owns focused checks and final integration evidence.

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

MutableList requires swap, reverse, truncate and extend in addition to its basic
mutators. Native operations prepare before committing; custom implementations
must uphold the same single-operation failure contract. Extend accepts a readonly
List and appends its shallow snapshot; extending from the same backing object
appends the original elements once. Iterator sources can be collected explicitly.
Swap validates both indices first. Truncate never grows. ArrayList::swap_remove
returns Option, consistent with remove, and replaces the removed slot with the
last element; its order is intentionally unstable. Reordering rejects active
native iteration and invalidates earlier closed cursors through the revision.

ArrayList, LinkedHashMap and LinkedHashSet expose with_capacity, capacity and
reserve. Reserve takes an additional count relative to current length and may
overallocate; no exact growth strategy is promised. Reservation preserves length,
order and identity and does not invalidate positional iteration. Failed capacity
preparation preserves logical contents. Capacity preparation is charged to the
allocation budget; live heap units continue counting stored values, not allocator
capacity or physical bytes. These APIs are concrete-storage operations.

## Set relations and algebra

Set supplies sealed is_subset, is_superset and is_disjoint defaults without Eq/Hash
bounds. Each query uses the other set's membership policy and short-circuits.
Implementations used together should agree on equivalence. Union, intersection,
difference and symmetric_difference accept readonly Set operands and construct
fresh LinkedHashSet storage, requiring Eq + Hash. Results retain shallow elements
in left traversal order, followed by newly accepted right elements where needed.
Both sources remain under iteration protection during traversal.

## Callback mutations and bulk commits

MutableMap requires get_or_insert_with and update. The former calls its factory
only for an absent key; the latter calls its transform exactly once with Option<V>.
Both return shallow V values, not entry handles. Native implementations prevent
all target container writes through aliases during callbacks, including replacing
an existing value. Reads and changes inside separate referenced payload objects
remain allowed. Validation, key lookup and allocation complete before committing.
Custom MutableMap implementations must uphold the same public contract.

ArrayList supplies retain, stable sort/sort_by/sort_by_key and adjacent dedup.
LinkedHashMap and LinkedHashSet supply retain. These are concrete-storage
operations: atomic bulk replacement cannot be built from arbitrary user-defined
individual setters. Preparation runs callbacks on ordinary VM frames, then one
runtime commit replaces slots/order and updates structural revision. Failure
leaves the target slots/order unchanged; completed payload and external effects
remain. Frame cleanup releases both callback mutation guards and temporary roots.
Native structural iteration rejects the final replacement.

Retain invokes its predicate exactly once per original element in traversal order,
unless an earlier call fails. Retained map/set keys keep their stored hash tokens;
user Hash/Eq are not rerun by retention. Sort uses stable bottom-up merging with
O(n log n) comparisons and O(n) working storage. Key extraction runs once per
element in original order before comparisons. Comparator consistency is the
caller's obligation. Dedup compares a candidate with the last retained element,
keeping the first element of each equal consecutive run; it is not global dedup.

## Lazy list snapshots and range removal

List.windows(size) yields overlapping full windows; List.chunks(size) yields
disjoint chunks with a possibly shorter tail. Both return Iter<List<T>>, reject
zero size immediately, and copy slots when each item is yielded. Snapshot slots
are independent and readonly; payload objects remain shared. Constructor calls
len/iter but does not read element values. Native source structural guards remain
active until exhaustion, explicit iterator closure or session cleanup. Ordinary
slot replacement is visible to later snapshots without changing previous ones.
Custom List implementations must keep indexed reads and iteration consistent.
Resuming after early closure validates native source revisions and restores guards;
a structurally changed source is rejected before reading the next snapshot.

ArrayList.remove_range resolves RangeBounds once, validates the interval, prepares
both remaining slots and the readonly removed List, then commits immediately.
No removal is deferred to iteration or destruction. Invalid bounds, failed result
allocation and active structural iteration leave slots unchanged. Completed
argument/bound-evaluation effects remain. Empty ranges return an empty list.
