# Collection Interfaces and Storage

This defines the collection access and construction contract. Value, identity
and failure rules are defined in [value semantics](value-semantics.md).
The standard library owns the collection traits and the three default
implementations through [ordinary native registration](standard-declarations.md).
Generated .kgr files are tooling views, not the source of these declarations.

## Types

| Read-only interface | Writable interface | Canonical default implementation |
| --- | --- | --- |
| `List<T>` | `MutableList<T>: List<T>` | `Vec<T>` |
| `Map<K, V>` | `MutableMap<K, V>: Map<K, V>` | `HashMap<K, V>` |
| `Set<T>` | `MutableSet<T>: Set<T>` | `HashSet<T>` |

These are ordinary installed trait contracts, usable as generic bounds or
dynamic interface types. Native and script types implement them through ordinary
trait checking. Their methods and declared parents determine available operations;
there is no separate collection marker or Object upper bound. The bundled provider
installs these declarations independently of optional native libraries.

`[T]` is a builtin fixed-length array. `[1, 2]` and `[value; count]` construct
arrays; their length is established at construction and is absent from the type.
Arrays permit bounded element replacement and share identity through assignment,
arguments and returns. They do not implement List/MutableList or expose growth,
removal, capacity or detached sequence edits. Their core registrations supply
len/is_empty, Index and Iterable independently of Vec's declarations.

`Vec<T>` is distinct nominal native sequence storage. It implements List and
MutableList through ordinary checked registrations. Use `Vec::from(array)` or
`HashSet::from(array)` for explicit construction; there is no implicit conversion
between Array and either Vec or List. A new collection has independent storage
and shares referenced element objects. Vec preserves order and duplicates; HashSet applies
its checked Eq/Hash implementations and has unspecified traversal order.

Vec, HashMap and HashSet remain available when optional modules are off. Optional
providers do not redeclare or replace their canonical declarations. Interfaces do
not select storage or provide implicit constructors. There is no map/set literal
syntax. Vec::from_fn and callback conveniences remain library algorithms.

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
fn main() -> u64 {
    val storage = Vec::from([Cell { value: 1 }]);
    val writable: MutableList<Cell> = storage;
    val readable: List<Cell> = writable;
    readable[0].value = 42;
    writable.push(Cell { value: 7 });
    readable.len() // 2: both views refer to the same object.
}
```

Replacing `readable[0]` or calling `readable.push(...)` is rejected. Updating its
referenced Cell is permitted and requires no container writeback. Read-only views
provide neither deep freezing nor snapshots. Structural edits through any writable alias
invalidate existing native cursors, which check revisions on subsequent reads.

## Conversion and dispatch

Concrete implementations convert implicitly to their implemented interface.
Mutable interfaces upcast to the read-only parent; reverse conversion and
implicit downcasts are rejected. Type arguments are invariant: converting an
outer container does not recursively convert elements or grant nested writes.
Conversions use [supplied target contexts](traits.md#accepted-interface-conversion-context):
an annotated variable, assignment destination, parameter, return or field type.
Without that context, `if`/`match` neither search common traits/parents nor coerce
one branch to another branch's interface type. Branches already sharing the same
interface type still merge. Non-interface readonly adaptation remains separate.

Underlying object identity survives interface wrapping and upcasting, but
`===`/`!==` require statically known concrete object operands. Interface operands
are rejected, as described in [value semantics](value-semantics.md#operators-and-defaults).
Boxing grants no comparison capability. The six collection interfaces provide
bounded Debug without PartialEq/Eq/Hash or hidden receiver comparison forwarding.
Use the explicit content helpers below for cross-implementation comparison.

Concrete native operations retain intrinsic fast paths. Generic calls select a
concrete implementation during compilation; dynamic calls use the existing
interface tables. Creating a view can allocate an interface wrapper, which retains
its data object and execution version. Views do not copy collection storage.

## Shared interface surface

| Interface | Members, in addition to its parents |
| --- | --- |
| `List<T>` | `len`, `is_empty`, `get`; inherits `Index<u64, Output=T>` and `Iterable<Item=T>` |
| `MutableList<T>` | `push`, `pop`, `insert`, `remove`, `clear`, `set` |
| `Map<K,V>` | `len`, `is_empty`, `contains_key`, `get`; inherits `Iterable<Item=(K,V)>` |
| `MutableMap<K,V>` | `insert`, `remove`, `clear` |
| `Set<T>` | `len`, `is_empty`, `contains`; inherits `Iterable<Item=T>` |
| `MutableSet<T>` | `insert`, `remove`, `clear` |

`Iterable` declares its associated `Iter` with an `Iterator<Item = Item>` bound.
Concrete implementations supply their own iterator type; the contract does not
require a particular optional `Iter<T>` class. Checked interface metadata retains
actual associated outputs and targets for static and dynamic dispatch.

Standard collection lengths and capacities use `u64`. List interface indices use
`u64`; unsuffixed literals receive that context. Nominal Vec indexing uses its
checked Index<u64> implementation. Builtin array indexing accepts the existing
integer index types; user-defined Index implementations retain their declared
parameter types. Storage access checks bounds before conversion to a machine
position; large indices must not wrap to a smaller valid position. Allocation
sizes are subject to [machine and resource boundaries](runtime.md#machine-and-resource-boundaries).
Out-of-bounds `get`/`remove` return None; invalid indexing, `set` and `insert` trap.
List insert permits an index equal to the length. `pop` returns None when empty.
Map remove returns the previous value or None; Set remove returns a boolean.
Interface push/insert/set/clear return unit. Existing native fluent push/insert
methods continue returning their concrete receiver.

## Content protocols and helpers

Stdlib declares these concrete conditional implementations through the same
checked protocol and native-call contracts used by other libraries:

| Concrete type | PartialEq | Eq | Hash |
| --- | --- | --- | --- |
| Array `[T]` | T: PartialEq | — | — |
| Vec<T> | T: PartialEq | T: Eq | T: Eq + Hash |
| HashSet<T> | T: Eq + Hash | T: Eq + Hash | T: Eq + Hash |
| HashMap<K,V> | K: Eq + Hash, V: PartialEq | K: Eq + Hash, V: Eq | K: Eq + Hash, V: Eq + Hash |

Unsatisfied content bounds are a static error; there is no identity fallback.
Vec<f32> has content PartialEq without Eq/Hash. Arrays supply PartialEq only,
comparing length and then corresponding elements in order through selected
callbacks; sharing an object does not bypass those callbacks.

`std::collections::list_content_equal`, `set_content_equal` and
`map_content_equal` accept two List<T>, Set<T> or Map<K,V> interfaces. Their bounds
are respectively T: PartialEq; T: Eq; and K: Eq, V: PartialEq. They work across
native and script implementations without selecting either receiver's object
comparison policy. Lists compare in iteration order and stop at the first mismatch.
Sets and maps compare independent of iteration order, matching each member/entry
once using the selected element/key equality. Matching supports multiple candidates;
a custom collection's lookup relation cannot replace the helper's selected equality.

Callbacks and iterator reads preserve left-to-right execution and completed effects.
Builtin structural edits invalidate existing cursors; the next affected read traps.
A helper that already short-circuited does not perform another read to force a trap.
Custom iterators retain their declared behavior. Working values are rooted and
long operations poll cancellation. No implicit snapshot, freeze, rollback or
coinductive cycle comparison is introduced; recursion retains call-depth limits.

Content hashing returns i64 bits and uses wrapping u64 arithmetic. With
`mix(h, x) = (h XOR x) * 1099511628211` and seed `14695981039346656037`, Vec folds
mix over element hashes in order and finally mixes length. HashSet sums
mix(seed, element_hash) and mixes cardinality. HashMap sums
mix(mix(seed, key_hash), value_hash) and mixes cardinality. Set/map hashes are
independent of traversal order. These algorithms belong to stdlib, separately
from identity operators and internal hash-table storage.

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

Iterable owns the finite transformation and terminal API described in
[builtins](builtins.md#iteration-protocols). MapIterator retains its source cursor and mapper. Constructing it does not call
the mapper. Aliases share progress, and next consumes its input before invoking
the mapper. Source cursors remain ordinary GC edges, including through erased Iterator
interfaces; structural changes are detected on their next read. See [native declarations](standard-declarations.md).

APIs outside the plan's accepted rows, including windows/chunks, capacity APIs,
set algebra, joining, grouping and additional constructors, remain deferred.
They are not required methods of List, Map, Set or their mutable counterparts.
There are no temporary implementations or aliases for predecessor names.

## Construction

Use Vec::new, HashMap::new and HashSet::new for empty storage. Context or
explicit type arguments supply otherwise unknown element types. Interfaces do not
have constructors that silently choose a concrete implementation.

```kgr
val storage = Vec::from([10, 20]);
val readable: List<i32> = storage;
val map: HashMap<String, i32> = HashMap::new();
val set: HashSet<i32> = HashSet::new();
```

Array literals and `[value; count]` construct builtin fixed-length arrays. Repetition
retains its value-only rule, including empty results. Distinct reference objects
require separate construction; the application-provider factory in the reset
proof demonstrates how an optional native function supplies that algorithm.

`collect<C>()` delegates to FromIterator. Vec collects items in source order;
HashSet deduplicates Eq + Hash items; HashMap collects `(K, V)` pairs with Eq +
Hash keys and preserves the last value for an equal key. Custom destinations
retain their own consumption rules, including short circuit. Construction keeps
source items' shared identity and checks source revisions before subsequent reads.

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
custom Eq/Hash keys, concrete identity and rejection of interface identity, malformed artifacts and generated
navigation. The accepted sorting/lazy/object proof covers source and encoded
artifacts, GC during callbacks, cleanup, reload and independent source-free loading.
The [performance report](../performance-baseline.md) records scoped measurements;
coverage for retired algorithms does not imply that those APIs remain installed.


## Host storage boundary

The remaining `CollectionAccess` descriptor belongs to the existing concrete
Map/Set representation and checked Rust host admission. It is absent from ordinary
trait declarations and is not a second authority for interface operations.

| Retained mechanism | Responsibility and validation |
| --- | --- |
| Ty/TypeId Map/Set descriptors and wire encoding | Preserve exact host/storage type identity, including nested invariant arguments. Native declaration adoption records readonly occurrences only after matching the installed signature. |
| Outer readonly conversion and branch joins | Permit only mutable-to-readonly adaptation of the same concrete Map/Set family. Compiler moves retain the new static view without relabeling another alias. Generic implementation matching requires exact descriptors. |
| Native Map/Set handles and host paths | Validate declared access, ownership and scoped borrows before reads/writes; an ordinary interface cannot recover writable storage. |
| Bytecode access-flow and callable verification | Reject forged readonly-to-writable flows and incompatible receiver witnesses independently of source checking. |
| Stdlib concrete registrations | Declare exact readonly read/iteration/content implementations separately from writable implementations; both use the same library algorithms. |

The [native collections migration](../native-collections-plan.md) owns replacing
these concrete families with nominal storage. Host registration/borrow unification
is separately queued. Neither migration should restore a trait access marker or
change the interface, identity and content contracts above. Runtime module-store
`check_collection_access` refers to garbage-collection safepoint borrowing, not to
collection-interface authorization.
