# Library-owned native collections execution plan

Status: agreed direction recorded; implementation has not started. The
[roadmap](implementation-roadmap.md#library-owned-native-collections) owns queue
placement. This plan owns NC01-NC06, the bounded API design, migration acceptance
and progress ledger. API sketches are proposals, not currently callable APIs.

Storage inventory refresh, 2026-10-18: the prioritized
[runtime storage redesign](implementation-roadmap.md#runtime-storage-redesign) and
[object overhead work](implementation-roadmap.md#object-storage-overhead) are locally complete.
NC01 remains queued and unstarted. The inventory reflects runtime `65bbf477`:
compact directory, typed native payload store, packed script-struct arena, weak
shared interface descriptions and direct snapshot locations. Mutation revisions
belong to runtime-owned NativeObject records, not the common directory or a
library-controlled payload field. NC must reuse that owner and define checked
public edit/commit access instead of creating another revision counter.
Full owner/slot/generation handles, native tracing, extraction, revision checks and
panic-contained disposal remain the contracts NC must reuse. NC still owns
library collection storage/algorithms; the predecessor does not perform NC's
collection migration or add new collection-family policy to generic storage.

## Outcome and scope

Make Vec, HashMap and HashSet ordinary nominal native objects registered through
NativeModule. Their Rust library implementations own storage and algorithms;
declarations describe generic parameters, bounds, methods and trait implementations.
Adding an application collection must not require a new semantic type, runtime
value variant, compiler-recognized container layout or backend operation.

```text
Rust collection storage and operations
    -> checked NativeModule declarations and local storage/entry bindings
    -> ordinary kgr imports, method calls and trait dispatch
```

Keep existing script APIs and observable collection semantics:

```kgr
val values = Vec::from([1, 2, 3]);
values.push(4);
val readable: List<i32> = values;

val scores: HashMap<String, i32> = HashMap::new();
scores.insert("Alice", 100);
val view: Map<String, i32> = scores;

val unique = HashSet::from([1, 2, 2, 3]);
```

Array literals still construct builtin fixed-length arrays, distinct from Vec and
List. Assignment/upcasts preserve shared identity. Read-only interfaces hide
mutators without freezing objects or introducing borrow syntax. Map/Set interfaces
impose neither hashing nor order; HashMap/HashSet require Eq + Hash on keys/elements.
Ordered implementations can require Ord instead.

The [collection content contracts](spec/collection-access.md#content-protocols-and-helpers)
own conditional Array PartialEq, stdlib PartialEq/Eq/Hash for Vec/HashSet/HashMap,
and the three content comparison functions. The implemented
[comparison rules](spec/value-semantics.md#accepted-comparison-design) have no
identity equality defaults or collection-interface receiver forwarding. NC must
reuse checked content implementations, helpers, interface Debug and ordinary
selected calls without reintroducing comparison certificates or duplicate
algorithms. Internal hash-table storage remains NC-owned.

Trait access markers and their builder/wire APIs are removed. The remaining
[host/storage descriptor inventory](spec/collection-access.md#host-storage-boundary)
belongs to the current concrete Map/Set families, typed host paths/handles and
source-free access-flow checks. NC replaces those families coherently and preserves
exact readonly admission, checked setters, roots and supplying generations. SA21
consumes the same ordinary declarations while owning Rust borrowing and registration.
Neither owner may turn an interface into a second storage/access authority.

The queued [paged incremental GC design](paged-incremental-gc-design.md) owns the
collector/page architecture. Accepted sequencing, 2026-10-26: opaque registration,
the general payload protocol and affected collection integration are one coordinated
replacement migration. Implement reference publication, accounting, scanning and
opaque API changes together, not as a separate post-GC API revision. NC owns
concrete collection layouts, algorithms and its detailed phases; the GC replacement
cannot reach final acceptance with container-specific collector/storage paths
deferred to NC. Planning this shared scope does not activate implementation.
Current-call selection follows the implemented
[publication contract](spec/module-activation.md#live-installations-and-call-selection),
not historical wording that would pin every stored method body.

Scope includes collection declarations, public native storage capabilities,
retained checked calls, collection cursors, host collection adapters, executable
contracts and source-free validation. Migrate affected collection paths end to end.
Generic iteration mechanisms follow the implemented iteration contracts.

Exclude a kgr rewrite of collection algorithms, dynamic-library loading, changes
to the implemented host registration/interface contracts, further identity
redesign, new language syntax, a borrow checker, async collections, new collection
methods and production
BTreeMap/IndexMap libraries. Small external collection fixtures prove extensibility;
they do not create a new standard-library product obligation. String, Range,
Future, Task and TaskScope representations are not migration targets.

## Baseline evidence and root cause

Planning baseline: `9cd90557`, after the five Iterable commits following
`35de1818`. Iterable APIs are implemented and locally accepted; the roadmap records
the remaining CI gap, while current contracts live in the specifications.
NC01-NC06 have not started.
Recheck implementation at NC01 without reopening the completed Iterable API work.

| Current owner | Evidence and disposition |
| --- | --- |
| [Vec declarations](../crates/kagari-stdlib/src/foundation/vector.rs), [hash declarations](../crates/kagari-stdlib/src/foundation/hash.rs) | Vec is nominal but declares Sequence storage; HashMap/HashSet still use special native constructors. Move all three to ordinary native declarations. |
| [Shared native declarations](../crates/kagari-types/src/declaration/native.rs), [HIR application](../crates/kagari-hir/src/native.rs) | Collection constructor/layout variants leak implementation families into semantic modeling. Remove these dependencies coherently rather than rename them. |
| [Heap directory](../crates/kagari-runtime/src/gc/directory.rs), [shared interfaces](../crates/kagari-runtime/src/gc/interfaces.rs) | The directory owns generation/locator; native records own mutation revisions. Interfaces locate metadata snapshots directly, preserving distinct checked heap/metadata identities and roots; shared descriptions do not own receivers. Packed script fields do not reinterpret Rust payload memory. No public raw arena/ownerless-token API was added. |
| [Native storage](../crates/kagari-runtime/src/native/storage.rs), [type builder](../crates/kagari-runtime/src/native/builder/type_builder.rs) | Factories, payload bindings, tracing and scoped type arguments already exist. Native revision initialization/reset/replacement/exhaustion has one runtime owner; checked public mutation access remains NC01 work. Extend these owners only for proven gaps. |
| [Managed fields](../crates/kagari-runtime/src/native/payload/managed/mod.rs), [fixed native data](../crates/kagari-runtime/src/native/payload/data.rs) | Safe edits separate fixed Rust data from checked script edges. Dynamic storage must preserve that boundary; arbitrary mutable payload access is not a solution. |
| [Sequences](../crates/kagari-runtime/src/native/sequence.rs), [hash payloads](../crates/kagari-runtime/src/native/hashed.rs), [tables](../crates/kagari-runtime/src/native/hash_storage.rs) | Runtime owns concrete storage and optimizations. Relocate collection policy while preserving useful storage foundations and checks. |
| [Hash operations](../crates/kagari-stdlib/src/foundation/hash/operations.rs), [codecs](../crates/kagari-runtime/src/native/binding.rs) | Methods depend on special value tags, heap operations and layout admission. Replace these with checked nominal access. |
| [Iterable declarations/defaults](../crates/kagari-stdlib/src/foundation/iteration.rs), [shared default lowerer](../crates/kagari-runtime/src/native/builder/default_lowering.rs), [operations](../crates/kagari-stdlib/src/foundation/iteration/operations.rs), [lazy cursors](../crates/kagari-stdlib/src/foundation/iteration/cursor.rs) | Uniform defaults, Iterator identity adaptation and nominal lazy adapters are implemented. Reuse these algorithms and checked iter/next calls; the old Vec-only free map and NativeCursor SDK were removed. |
| [Call context](../crates/kagari-runtime/src/native/context.rs), [stored selections](../crates/kagari-runtime/src/native/stored_selection.rs) | Payload allocation retains selected calls as metadata edges and later invokes them through the rooted receiver. Reuse this mechanism for collection key calls; public constructor/factory ergonomics still need NC01 review. |
| [Vec construction/requirements](../crates/kagari-stdlib/src/foundation/vector.rs), [hash construction/requirements](../crates/kagari-stdlib/src/foundation/hash.rs), [Vec operations](../crates/kagari-stdlib/src/foundation/vector/operations.rs), [shared construction traversal](../crates/kagari-stdlib/src/foundation/iteration/traversal.rs) | Vec/HashMap/HashSet FromIterator and collect delegation already work. Hash construction selects insert, but still declares Ty::Map/Set and branches on Value::Map; migrate representation without replacing consumption semantics. |
| [Base cursors](../crates/kagari-runtime/src/gc/iter.rs) | Cursors check structural revisions on reads; wrappers retain ordinary traced edges, without iteration leases or consumer-scope traversal. Revision ownership for general library storage remains a native storage boundary. |

Today map insertion passes through Value::Map and GcHeap map operations into a
runtime-owned table. That path also enforces key/value typing, allocation
admission, custom Hash/Eq calls, lookup guards and mutation commits. Vec similarly
uses runtime sequence operations despite its nominal identity. These are real
responsibilities, not removable overhead.

The defect is coupling those responsibilities to concrete container families.
Runtime should own general checking/lifetime mechanisms, and libraries should
own storage and operation policy. Historical motivation is not inferred from
current code. This migration makes no unmeasured performance claim.

### Reuse the completed Iterable mechanisms

For `val result: Vec<i32> = values.map(f).take(2).collect();`, the existing default calls
the selected `iter` once. Its adapter payload stores the source and callback,
and allocation retains the selected `next` slot. Consumption invokes that stored
target with the original supplying generation. `collect` delegates to the
destination's FromIterator; traversal opens the existing consumer scope.

After NC, the same flow must work when values is the migrated Vec or an external
collection. Only the concrete source/destination storage, their methods/cursors
and general storage protection change. Do not recreate map/filter/take, the other
adapters, terminal algorithms, Iterator identity adaptation or destination dispatch.

The existing public retention path is:

```rust
// During a checked native constructor, with declared callable requirements:
cx.allocate_result_payload(payload, &[hash_slot, eq_slot])?;
// During a later method, whose argument 0 roots that native receiver:
cx.call_stored_values(0, 0, &[key])?;
```

Allocation copies the chosen selected calls into object metadata slots, in the
supplied order. StoredSelection stays runtime-private; libraries do not construct
owning metadata roots inside payloads. StoredCallable remains the mechanism for
traced script closure values. These are existing capabilities, not new NC APIs.
They do not by themselves provide checked resizing, public revision/commit access or a
public factory-level key-call registration contract.

## Ownership and representation

| Layer | Target responsibility |
| --- | --- |
| types / HIR | Native definition identity and arguments; ordinary declarations, trait bounds and checked calls. No concrete Map/Set semantic type family. |
| contract / compiler / backends | Verify/lower nominal native calls, interface evidence and storage admission. No collection-name-based type inference or method rewriting. |
| runtime native infrastructure | Identity, installed payload bindings, scoped access, type/owner/generation checks, tracing, roots, allocation accounting and retained-call lifetime. |
| stdlib collections | Vec/HashMap/HashSet declarations, buffers/tables, methods, iteration policy and concrete optimizations. |
| application module | Its own storage and methods through the same public capabilities used by stdlib. |

Portable declarations contain no Rust TypeId, function pointer or physical payload
layout. Installation binds the checked nominal definition to local Rust storage
and entries. Source-free loading rejects missing/mismatched bindings without HIR.

Remove collection-only Map/Set TypeId/Ty and Value variants and constructor
branches. Retire Sequence/Map/Set layout categories as collection identities or
blanket storage authorization. A generic buffer/slot capability may remain only
with an explicit owner and validated access contract; it cannot assert that its
owner is Vec, List or Iterable. Preserve builtin Array support and justified
shared low-level buffers without keeping a second Vec implementation in runtime.

Vec/HashMap/HashSet base iterators become ordinary registered native objects;
the lazy adapters already are. NC01 inventories CollectionCursor/Iter consumers,
including builtin Array, String and Range, before removing any shared cursor tag
or layout. Migrate the concrete collection cursor paths and leave the other
consumers functional through their existing representation unless a bounded
shared-mechanism change is necessary. Residual builtin cursor support is allowed;
using it to retain runtime-owned Vec/HashMap/HashSet policy is not. Unrelated
iterator or builtin storage redesign is outside this task.

## Public API shape

### Ordinary Native callers versus storage providers

Native functions using GC objects should call checked handle methods directly:
field `get/set` and collection `get/set/push/insert/remove`. Accessing several
objects must not require nested payload-edit closures or long-lived borrow guards.
The [GC design's Native-facing operations](paged-incremental-gc-design.md#native-facing-gc-object-operations)
own the examples, root/edge distinction and incremental publication requirements.
Existing Object/Field and ScriptVec access provide the baseline shape; this does
not claim that the proposed general storage protocol is implemented.

The registration and payload sketches below are for collection implementers, not
ceremony required of ordinary Native callers. NC01 must preserve flat handle
operations while moving concrete storage into libraries. Runtime owns reference
publication, root protection, accounting and native revision updates; callers do
not manage barriers or mutation counters. Provider access remains audited, and
this distinction does not grant arbitrary mutable access to traced Rust payloads.

### Opaque registration and public categories

Accepted API direction, 2026-10-26; not implemented. Keep the existing public
vocabulary: `Ref<T>` is a scoped shared borrow, `RefMut<T>` is a scoped mutable
borrow, and `Opaque<T>` is a retained handle to an opaque Native object. Opaque
describes access to a Rust representation that is not exposed as script fields;
it does not mean that the collector cannot trace the representation. Current
runtime-owned payloads use GC for their lifetime, but that implementation choice
must not introduce a separate public "GC type" category or `define_gc_type` API.
Host-owned instances remain host-owned when borrowed; registering a type does not
by itself transfer an instance into runtime storage. Borrowing still follows the
existing access and lifetime contracts.

Ordinary opaque registration must associate the script declaration with its Rust
payload type once. Derive the storage descriptor from that association and the
payload's trace/accounting/access contract; do not require a second routine
`native_storage(NativeStorage::payload::<T>())` declaration. `NativeStorage` remains
a lower-level runtime descriptor, not another public object category. Reconcile
explicit declaration building with the existing `Opaque<T>` registration and
`NativePayload::registration_storage` paths, rather than adding a parallel facade.
Special factory/managed-field requirements remain explicit where needed; they
must not make ordinary opaque declarations repeat the type association.

The sketches below use `define_opaque::<Payload>(name)` to illustrate the selected
opaque vocabulary and single registration step. Exact builder spelling and
signatures remain NC01 work; these are not currently callable APIs. Instances of
the registered type are held through the existing `Opaque<Payload>` handle, not
a second GC handle wrapper. Script code continues to use its nominal name, such
as `Vec<T>`; script generic arguments retain checked runtime type evidence.

This naming/registration change does not grant unrestricted mutable payload access.
Complete dynamic reference-bearing edits, accounting, revisions and publication
under the existing Native owner. Ordinary Vec/HashMap/HashSet implementations must
use that public opaque path, with no collection-specific storage admission or
dispatch privilege. Efficient calls use prepared Native entries and checked payload
access; retain compact library-owned buffers and validate costs through the focused
measurements below, without promising unmeasured parity with special entrypoints.
Implement this adjustment within the GC replacement and its general payload
protocol. NC01/NC02 remain the detailed API work breakdown, not a later independent
delivery. Do not first implement a new collector around special collection storage
and defer the ordinary opaque boundary until afterward.

### End-to-end API proposal for review

Selected direction, 2026-10-26: paged precise tracing with incremental collection.
Opaque payloads may contain Value and ordinary Rust containers through an unsafe
provider contract. Native entry does not disable GC. This supersedes the provisional
counted-Value/cycle-collector proposal and its untraced detached-buffer sorting path.
Keep Ref<T>, RefMut<T> and Opaque<T> as the public ownership/borrowing categories.
This records design scope only; implementation is not activated. Examples below are
pseudocode, not final callable signatures. The
[GC design](paged-incremental-gc-design.md#incremental-tracing-and-mutation) owns
roots, barriers, scan progress and reclamation.

| Audience | Proposed surface | Responsibility |
| --- | --- | --- |
| Type author | define_opaque::<T>(name), existing generic/method builders | Associate declaration and Rust payload once through an unsafe provider contract; no automatic safe admission of arbitrary T |
| Host borrowing | Existing Ref<T> / RefMut<T> and host scopes | Host owns Rust state; scoped access does not move it into the heap |
| Ordinary Native code | Retained dynamic/typed values, Opaque<T>, Object and library handles | Safe arguments/results remain rooted across allocation and script calls; no manual rooting for each ordinary field/collection operation |
| Storage provider | Unsafe NativePayload integration and unrestricted read/edit; scoped mutation services | Trace internal edges, publish writes, account backing and protect extracted values; no collection or script execution during conflicting payload loans |
| Callback-bearing algorithm | Generic operation protection, temporary roots or traced work state | Keep every live temporary visible at collecting boundaries; end loans before callbacks and restore valid storage on all exits |

Do not add a GcObject public category, special GC Vec, NoGc whitelist, split opaque
categories or mandatory lifetime branding to prohibit Value fields. Rust Vec remains
a storage option; script generic applications are checked runtime evidence, not
arbitrary Rust generic instantiations. Preserve compact scalar buffers.

Interim unsafe provider obligations:

- Make NativePayload an unsafe integration contract. Trace every strong internal
  edge, including references in nested/shared storage and captured executable state.
  Empty trace is valid only when there are no relevant edges. The visitor discovers
  reachable objects; it does not count owners or recursively walk the graph itself.
- Preserve runtime/type/generation checks, scoped borrows, incremental publication,
  partial-scan progress, mutation revisions, accounting and panic-safe destruction.
  Interior mutability cannot bypass these requirements. Unsafe admission does not
  disable runtime validation or permit invalid Rust aliasing.
- Value in a payload is an internal non-rooting value. Moving/copying it into a Rust
  local, Option or Vec does not automatically retain it. Establish an existing root
  or another traced edge before a collecting boundary; root publication also obeys
  the active incremental barrier. Do not embed permanent external roots per element.
- Short non-collecting mutation intervals may transfer references or restore valid
  representation. They must end before script callbacks, including callbacks made
  through captured Runtime aliases. Native calls as a whole remain collectable.
- Safe library handles/results encapsulate retention; raw provider access is the
  audited boundary. Preserve scoped host/frame token restrictions, selected call
  generations, cancellation and cleanup on every exit.

#### Consolidated Native API surface

Keep existing runtime-owned root mechanisms and prepared conversion owners. Do not
carry forward the counted Value/control-block layout or force a ScriptValue-to-Value
ownership migration. Exact public naming remains open; ordinary safe Native bindings
must not leak unrooted values across collection. Internal compact Value transport
can remain non-rooting under its protected execution contract.

| Surface | Contract |
| --- | --- |
| Internal Value | Compact scalar/object representation; an object ID alone is not a root or an owning reference count |
| Retained dynamic/typed Native value | Runtime-owned root protection, with existing owner/type/conversion checks; reusable across collecting calls |
| Opaque<T> | Retained typed handle to an opaque Rust payload; hidden representation does not imply edge-free data |
| Ref<T> / RefMut<T> | Scoped access to host-owned Rust state, not a general heap-loan bypass |
| NativeContext | Checked calls, conversion and GC at valid boundaries |
| NativeType<T> | Existing prepared concrete type/application and checked creation evidence |

Opaque declaration and creation retain one owner:

```text
module.define_opaque::<PlayerData>("Player")?;
// Attach fields/methods with the existing builders; install and bind the type.
let player: Opaque<PlayerData> = player_type.create(cx, PlayerData { ... })?;
```

Use NativeType::create, with an unsafe NativePayload implementation carrying the
provider proof. Do not add native_storage registration, cx.create_opaque or an
untyped Opaque::new without runtime/type evidence. Conversions reuse existing
traits and validate applied type, runtime identity and pinned execution evidence.

Provider protocol sketch:

```text
unsafe trait NativePayload: Any + Debug + Send {
    fn trace(&self, visit: &mut dyn FnMut(&Value));
    fn charge(&self) -> NativeResult<HeapCharge>;
    // Descriptor also declares edge-free, bounded or resumable scanning capability.
    // Exact resumable cursor/mutation interface remains a design prerequisite.
}

struct VecStorage {
    element: CheckedElementType,
    values: Vec<Value>,
}

unsafe impl NativePayload for VecStorage {
    fn trace(&self, visit) {
        for value in &self.values { visit(value); }
    }
    // charge reports exclusively owned backing capacity through HeapCharge.
}
```

This whole-object trace demonstrates reachability only; it is an indivisible O(n)
scan. Large standard-library payloads require the GC design's resumable scan and
mutation protocol before bounded-work claims. Edge-free Rust data may have empty
trace but still needs accounting/destruction. Trace cannot execute script, mutate
storage or retain borrowed payload references. Do not interpret an unsafe provider
as permission to silently skip references or hide unbounded work.

Reuse read/edit for unrestricted payload access rather than adding with_storage:

```text
unsafe fn Opaque<T>::read<R>(
    &self, cx: &NativeContext,
    operation: scoped FnOnce(&T) -> NativeResult<R>,
) -> NativeResult<R>;

unsafe fn Opaque<T>::edit<R>(
    &self, cx: &mut NativeContext,
    operation: scoped FnOnce(&mut T, &mut StorageAccess) -> NativeResult<R>,
) -> NativeResult<R>;
```

Both unrestricted paths remain unsafe initially: shared access may still mutate
through interior mutability or extract an unrooted Value. Views cannot escape, and
collection/reentry through any alias is excluded only for the active loan. Audited
safe library methods wrap these calls; proven fixed-data conveniences can remain
safe. A returned raw Value remains the provider's responsibility until rooted or
published into another protected location.

StorageAccess represents existing runtime-owned validation, retention/publication,
accounting and scan-progress services. Reuse a retain operation for temporary roots;
do not introduce a public prepare_removal/commit_removal pair or a second root
registry. Exact helper signatures remain open. Fallible root/barrier/accounting
capacity must be prepared before destructive commit; no fallible post-edit full
scan can retroactively establish safety.

For get/pop/remove, establish result retention while the value is still protected
by the source/short loan. If retention fails, do not commit removal. For push/set,
keep the input rooted until the destination edge is published with its barrier.
Release temporary roots after handoff, including error exits. A bare Rust pop
inside an unsafe provider is permitted, but its result cannot cross a collecting
boundary without protection. Safe library pop returns a retained result:

```text
let item = values.pop(cx)?;        // retained result through the library facade
cx.collect_garbage()?;
if let Some(item) = item { destination.push(cx, item)?; }
```

Ordinary Native callers do not manually register a root here. Providers implement
the retention protocol once; rooting APIs are not required at every user-level call.

### Concrete Vec sorting API proposal

Selected tracing direction, 2026-10-26. Keep the safe library facade and existing
[sorting semantics](spec/collection-access.md#foundation-algorithms):

```text
ScriptVec<T>::sort(cx) -> NativeResult<()>                  // selected T: Ord
ScriptVec<T>::sort_by(cx, Function<(T, T), Ordering>) -> NativeResult<()>
ScriptVec<T>::sort_by_key<K>(cx, Function<T, K>) -> NativeResult<()> // K: Ord
```

Resolve and pin comparator/key-selector evidence once. Sort uses Ord, not Hash;
key selectors run during comparisons without a cached-key prepass. Preserve stable
worst-case O(n log n) comparisons/moves, with callback and GC work accounted for
separately. No preparatory O(n) retention snapshot or index construction is allowed.

For reference-bearing collecting callbacks, own/adapt the stable sorting algorithm
so its temporary slots and merge buffers are GC-visible. Reusable temporary roots
or library-private traced work state are candidate representations of this selected
boundary; the exact algorithm and work representation remain open. This does not
introduce public SortWork/SortSession APIs or a special collector container case.

The source and scratch together cover every live element. Before every callback:

1. Complete transfers under short non-collecting access; publish roots/edges and
   repair incremental scan progress before losing prior protection.
2. Expose only initialized scanner-visible ranges and end all conflicting heap loans.
3. Invoke the retained comparator/key selector with protected arguments/results;
   GC can progress during nested allocating script execution.

Keep the receiver rooted and protected against alias slot access/recursive sorting
through existing operation guards. That guard does not disable GC. A detached Vec
buffer must acquire a traced owner; mem::take alone is insufficient. Ordinary Rust
sort's private scratch cannot be exposed merely by rooting the receiver and the two
comparison arguments. Root/scan registration must not secretly walk all inputs at
algorithm entry; the transfer/publication protocol must prove this.

Restore valid source/work state before releasing protection on success, error,
cancellation or unwind. Stop user callbacks after the first failure; preserve the
original element multiset, permit partial reordering and retain completed callback
effects. No user callback or fallible allocation may be needed solely to restore
ownership. Reuse existing error semantics; do not retain an adapter that depends on
unobservable Rust scratch. Cancellation remains active within long library work.

Rust kernels without collecting callouts remain reuse candidates, including compact
scalar sorting, but long indivisible work still requires latency/cancellation review.
Do not infer speed parity from O(n log n). Measure comparisons/moves, root/barrier
work, allocations, peak scratch memory and GC-step delay across scalar, cheap-script
and allocating-script comparators. Test collection at temporary transfers, partial
scans, nested callbacks, errors/unwind and restoration. The exact kernel, scan cursor
protocol and measured acceptance remain pre-implementation decisions.

### Other standard-library operation boundaries

The [operation-family inventory](paged-incremental-gc-design.md#future-standard-library-extraction-families)
includes future collection/iterator designs. Do not rewrite the whole standard
library or assume only Hash/Eq call script. Reuse Rust storage and algorithms where
all values remain protected at collecting boundaries.

- for_each/map/filter/fold/zip/collect retain only the required current items,
  accumulator and iterator state; end loans before user callbacks/custom next.
  Allow GC during callbacks, rather than delaying collection for the entire loop.
- Hash/Eq use rooted stable candidates and lookup/callback/commit protection, without
  silently retrying observable callbacks. Existing primitive fast paths remain.
- retain/dedup and other compaction algorithms need controlled temporaries when
  callbacks observe a representation with holes. Use shared root/work-state machinery
  and preserve their completed-removal/error semantics; no full-input root prepass.
- Future heap/tree algorithms need the same audit around comparison callouts. Rust
  Hash/Eq/Ord do not directly express context-bearing fallible script calls.
- Escaping iterators/entry-like state use traced owners and revision checks; borrowed
  Rust guards or no-GC regions cannot span arbitrary script lifetimes/suspension.

Validate these families at their existing contract owners, including collection
inside callbacks and long allocating loops. No percentage of future APIs, latency
bound or measured speedup is claimed. GC itself still needs incremental scan,
barrier and sweep protocols; collecting Native callbacks alone do not bound pauses.

### Collection registration and storage

Reuse ModuleBuilder, NativePayload and the underlying NativeStorage descriptor,
applying the opaque registration adjustment above. These sketches show
responsibilities, not compiling Rust. Settle exact names, signatures and binding
ergonomics at NC01; omitted method signatures and required trait members must be
supplied in implementation.

```text
let mut declaration = module.define_opaque::<VecStorage>("Vec")?;
let t = declaration.type_parameter("T")?;
let vec_type = declaration.finish()?;

module.implement(vec_type, |group| {
    let t = group.parameter("T")?.ty();
    group.associated_function("new", vec_new)?;
    group.associated_function("from", vec_from_array)?;
    group.trait_impl(core.mutable_list(t), |methods| {
        methods.bind("push", vec_push)?;
        methods.bind("set", vec_set)
        // Bind remaining members and implement parent traits separately.
    })
})?;
```

VecStorage retains its checked element contract and native buffer. Keep compact
scalar storage where valid; object elements require traced values. A kgr generic
parameter cannot dynamically instantiate arbitrary Rust generic code. Select
storage from checked type arguments, never the first element or a textual name.
The registered constructors build VecStorage with that checked contract and use
the existing Native allocation owner; registration need not duplicate a factory
for every constructor. The constructor/factory binding details remain NC01 work.

```text
let mut declaration = module.define_opaque::<HashMapStorage>("HashMap")?;
let k = declaration.type_parameter("K")?;
let v = declaration.type_parameter("V")?;
declaration.bound(k.ty(), core.eq())?;
declaration.bound(k.ty(), core.hash())?;
let map_type = declaration.finish()?;
// Constructors declare checked Hash/PartialEq::eq requirements, create storage
// with checked key/value contracts, then use allocate_result_payload with those
// selected slots. Register Map/MutableMap/Iterable implementations normally.
```

HashMapStorage retains key/value contracts and its table; the containing native
object's metadata retains selected key operations. Eq remains the semantic bound;
the callable equality member is PartialEq::eq, as in current hash construction.
HashSet shares the key-operation machinery. Prefer checked constructor requirements
and existing retained slots rather than inventing a separate type-level resolver
or a cx.retain_call API. NC01 settles public builder support, slot ownership and
how new/from/from_iter and host conversion supply equivalent evidence. If a
factory path is required, record its precise gap before extending StorageContext.

Register Iterable Item/Iter and Iterator::next normally. Do not infer traversal
from physical storage. Preserve default/override dispatch through interfaces.
Constructors explicitly declare Array/Iterable inputs and use ordinary inference.

## Runtime capabilities and safety

NC01 records exact APIs and the safe/trusted boundary for each row; NC02 implements
only gaps required by these collections and the external proof.

| Capability | Required contract |
| --- | --- |
| Native object handle | Check runtime ownership, generation, nominal application and installed payload mapping. Interfaces never grant arbitrary payload downcasts. |
| Dynamic construction/mutation | Validate stored types and heap ownership before publication; prepare allocation/accounting before mutation, update accounting on growth/shrink and preserve required write barriers. |
| Scoped access | References cannot escape; no storage borrow spans GC or callback/reentry. Safe APIs cannot overwrite traced slots with unchecked values. |
| GC edges/roots | Trace stored values and call dependencies; root reads/removals/candidates across allocations or callbacks; deterministically release failed-construction and temporary roots. |
| Retained calls | Reuse allocate_result_payload selected slots and call_stored_values. Preserve signatures/generations and metadata edges versus external roots. Complete only constructor/host-path gaps; no borrowed CallContext or repeated name lookup. |
| Operation guards and iterator validity | Preserve short native borrow, hashing and mutation-commit guards. Libraries own structural-edit/cursor policy through checked access to the runtime-owned native revision; writes commit and stale reads trap. Define public edit/commit capabilities without a second counter or lifetime iteration leases. Source/callback retention uses ordinary GC tracing; ordinary calls select current bodies while retained computations keep exact owners. |

Do not expose arbitrary safe &mut P for reference-bearing or variable-allocation
payloads. Reuse/extend managed storage or define a bounded audited storage-provider
contract with explicit obligations. Making private heap methods public or accepting
an empty trace does not establish safety. External implementers must satisfy the
contract without runtime-private imports, whitelists or disabled validation.

For map.insert(key, value), preserve this logical sequence:

1. Evaluate receiver/key/value once, left to right; validate the native boundary.
2. Acquire key-operation protection and call selected Hash.
3. Retain a candidate under scoped access before ending the loan.
4. Invoke selected Eq outside that borrow; reject conflicting reentry and preserve
   completed effects. Continue collision search with cancellation checks.
5. Prepare result/allocation requirements and commit while protection keeps the
   candidate valid. Release temporary roots/protection on every exit.

Preserve trap/commit ordering, including allocation before destructive removal
where required. No retries that silently repeat user callbacks. Rust Value Hash/Eq
cannot substitute for Kagari operations. Primitive fast paths require evidence of
the selected primitive implementation, not just a scalar key type. Stored key
hash/equality stability remains the user's existing obligation.

Vec retains bounds/slot-update ordering, aliases, compact buffers and sort/edit
lease guarantees. Failed in-place algorithms preserve their specified completed
effects and element-multiset guarantees; do not add rollback semantics.

Iteration preserves declared order, shared progress, structural mutation rules and
live shallow views. Structural edits commit and invalidate subsequent source reads;
short-circuit consumers do not force an extra read. Release temporary roots on
completion and failure, without introducing cursor lifetime leases.

## Consumers and optimizations

Inventory type substitution/formatting, interface coercions, executable schemas,
codecs, bytecode verification, lowering, host ScriptVec/ScriptMap/ScriptSet
adapters, source revision checks, reflection/tooling and artifact producers. Separate
concrete native handles from generic List/Map/Set handles: interface adapters
invoke selected methods rather than infer storage from the trait name.

Review prior optimizations. Retain compact buffers and validated primitive hashing;
remove repeated preparation/checks only when their guarantees have a new owner.
Exact standard-Vec fast paths may remain as library paths with checked identity
and call evidence. Opaque, GcHandle or a buffer capability alone never
authorizes bypassing custom iteration or overrides. Do not add caches or blanket
boxed storage merely to simplify migration.

NC01 captures a small reproducible baseline for scalar Vec traversal/editing,
reference-bearing Vec and primitive/script-key HashMap operations. NC06 repeats it
with toolchain, machine, profile/features, parallelism, cache state and workload
recorded. Separate preparation from execution. Investigate material regressions
through ownership/data-flow review and record measured tradeoffs. This is not a
new speed target or the Lua parity workstream; do not repeatedly run full benchmarks.

## Sequencing and phases

The [Iterable APIs](spec/builtins.md#iteration-protocols) and
[retained native cursors](spec/standard-declarations.md#sorting-and-lazy-iteration)
are implemented at this baseline; NC01-NC06 remain queued and unstarted.
Use the current [scoped identity APIs](architecture.md#definition-identity-ownership).
Refresh NC01 against those APIs instead of rebuilding path/ID variants or treating
the historical baseline signatures as a compatibility requirement.
Within the joint GC/opaque/collection migration, collection work begins with NC01,
using the existing retained slots, adapters, terminal consumers and FromIterator
implementations. Do not add another Iterable phase or make the trait surface
depend on this storage migration. Reuse implemented host registration/scoped
borrowing; the roadmap owns subsequent activation changes.

| Phase | Scope and exit condition |
| --- | --- |
| NC01: inventory and API contract | Trace dynamic edits, source revisions, factories, key calls, host conversions and artifacts against a fresh activation baseline, including the accepted OM owners. Specify opaque registration without redundant storage declarations, reusing Ref/RefMut/Opaque categories and existing registration owners. Record exact reuse of retained slots/revision checks, constructor evidence, builtin cursor disposition, optimization review and baseline commands. Establish the external fixture and only the missing public capabilities. |
| NC02: general storage capabilities | Implement the unified opaque registration contract and replace superseded registration paths. Complete checked dynamic edits, allocation/accounting and public native revision/commit access under the existing owner. Extend retention only for proven constructor/host gaps. Demonstrate public-API-only reference-bearing storage participating in existing nested/paired adapters with invalidation and callback failure cleanup. No second guard or selected-call subsystem. |
| NC03: Vec migration | Move concrete storage/operation policy and its base cursor to the library; replace Sequence admission and consumers while retaining builtin Array support. Preserve defaults, indexing, sorting, existing FromIterator/pipelines, host adapters and compact buffers. Remove the obsolete parallel Vec path. |
| NC04: HashMap/HashSet migration | Register both as nominal objects; move table/key policy and base cursors to the library, replace special type/value/layout branches and migrate host/contract/backend consumers. Rebind existing FromIterator insert/iter/next requirements to nominal types. Preserve collisions, last-value-wins/dedup rules, custom Hash/Eq, cleanup and current callback selection under the publication contract. |
| NC05: external extensibility and cursors | Close remaining collection cursor consumers and prove a public-API-only BTreeMap<i32, T> fixture implements Map/MutableMap/Iterable with reference-bearing values and source revision checks. Exercise existing adapters and collection terminals without core enum changes or new default algorithms. Separately prove retained script comparison calls, not only Hash/Eq. No production ordered collection required. |
| NC06: final integration | Remove obsolete paths/bridges, update implemented docs/examples/tooling, refresh affected artifacts once, complete host/source-free acceptance, repeat focused measurements and run final local checks. Report CI separately. |

The external BTreeMap fixture can use Rust integer ordering because its keys are
fixed to i32. This does not prove arbitrary script Ord can be passed directly to
std::collections::BTreeMap: generic ordered storage needs a callback-compatible
library algorithm. The separate comparison proof validates the runtime capability
without adding that algorithm's production implementation to this task.

Default to buildable checkpoints; group declaration/execution/caller changes when
inseparable. Attempt necessary intermediate checks and record failures with command,
representative diagnostic, cause and owning phase here and in commit bodies. No
weakened assertions or production stubs. Temporary bridges need explicit removal
by NC06. Replace unpublished APIs directly; retain no compatibility aliases,
forwarding implementations or old-format readers. Keep ABI/format identifiers
unless publication or explicit compatibility requirements call for a bump.
Implementation commits use `Plan: native-collections-plan NC0N` trailers.

## Verification and acceptance

Reuse existing semantic owners, extending only distinct missing contracts:

| Coverage owner | Required evidence |
| --- | --- |
| [Interfaces](../crates/kagari/src/runtime/tests/language/collection_interfaces.rs), [access](../crates/kagari/src/runtime/tests/language/collection_access.rs), [arrays](../crates/kagari/src/runtime/tests/language/array_operations.rs) | Nominal distinction, generic/interface dispatch, invariance, shallow views, aliases, bounds, Array separation and once-only mutation targets. |
| [Iteration](../crates/kagari/src/runtime/tests/language/iteration_traits.rs), [conversions](../crates/kagari/src/runtime/tests/language/conversion_traits.rs), [library collections](../crates/kagari-vm/src/tests/library_collections.rs) | Preserve implemented adapter/terminal behavior while changing source/destination storage: generic/interface traversal, custom destination dispatch, HashMap last-value-wins, HashSet deduplication, early exit and shared progress. |
| [Lazy cursor boundaries](../crates/kagari-vm/src/tests/library_collections/library_mapping/mod.rs), [source-free pipeline fixture](../crates/kagari/tests/fixtures/feature_artifact.kgr) | Reuse retained_zip_uses_both_original_custom_sources_and_callbacks_after_reload and existing recursive-next/cycle/invalidation coverage. Prove migrated sources propagate through nested/paired adapters and source-free short-circuit consumption, without rebuilding a duplicate iterator suite. |
| [Native builder](../crates/kagari-stdlib/src/tests/runtime_boundary/native_builder.rs), [native boundary](../crates/kagari-vm/src/tests/native_boundary.rs) | Public extensions, wrong nominal/payload/type/owner rejection, dynamic edges, roots, reentry/traps/cancellation, cyclic collection cleanup and host retention. |
| [Reload](../crates/kagari/src/runtime/tests/embedding/generic_reload.rs), [artifacts](../crates/kagari/tests/artifact_features.rs), [native artifacts](../crates/kagari/src/program/tests/native_artifacts.rs) | Retained type/cursor and exact computation owners, current ordinary callback bodies, source-free linking, forged storage/call evidence rejection and backend agreement. |

Cover script key collisions and side effects, failures before commit, GC objects
returned by reads/removals, alias mutation during protected operations, cancellation
and post-error reuse. Consolidate existing cases rather than duplicate smoke suites.
Preserve source-free, host ownership and backend cases with distinct contracts.

Acceptance requires all three standard collections to use the public extension
path. Ordinary opaque declarations associate the Rust payload once, without
duplicate storage declarations or a new GC-specific public category. Reuse the
existing wrong-payload/type/owner and borrowed/retained lifetime contract coverage
when consolidating registration. The external fixture cannot import runtime-private
storage, add a compiler name check or receive special installation privileges. Classify remaining
collection branches by owner: library algorithms and tests are valid; concrete
storage semantics in HIR/type enums or compiler/backend dispatch are not. Document
justified generic buffer/Array helpers rather than relabeling special cases.

During implementation run affected focused tests, formatting, structural review,
`uv run --locked scripts/check_structure.py` and `git diff --check`. Review imports,
re-exports, visibility, effective LOC and responsibilities. NC06 also uses targeted
tests plus a small regression set for directly affected contracts, and full-workspace
Clippy locally (`cargo clippy --workspace --all-targets -- -D warnings`);
record the selection rationale and results. Batch repairs and rerun failed/affected
cases only, completing skipped coverage within that scope. Do not run workspace
suites locally or reconstruct them through package commands.

The following full sequence belongs to CI; structure, formatting, workspace Clippy
and diff checks also run locally. Workspace tests and exhaustive feature/backend
validation remain CI-only:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
cargo test -p kagari --no-default-features --test artifact_features
git diff --check
```

GitHub CI owns the full feature/backend matrix; local success does not establish
CI acceptance. Planning-only edits need content/link/diff checks, not builds.

## Progress and error ledger

- [x] Record direction, concrete API examples, scope and acceptance.
- [x] Rebase planning on 9cd90557 and distinguish completed IA capabilities from
  remaining collection storage/constructor/source-protection gaps.
- [x] Refresh native revision and interface storage owners after accepted OM,
  2026-10-18; NC implementation and activation remain separate.
- [x] Clarify ordinary Native GC handle operations versus collection-provider
  storage access and link the shared GC API contract, 2026-10-26.
- [x] Record the accepted opaque registration adjustment: preserve Ref/RefMut/Opaque
  categories, infer the underlying storage descriptor from one payload association,
  with NC01/NC02 detailing the API work within the GC replacement, 2026-10-26.
- [x] Couple opaque API and general payload changes with GC implementation;
  require removal of special collection collector/storage paths before joint
  acceptance, 2026-10-26.
- [x] Present an end-to-end API draft connecting opaque registration, retained
  extraction, an explicit audited storage boundary and collecting sort work state;
  exact provider signatures remain under review, 2026-10-26.
- [x] Select interim unsafe opaque integration and unrestricted payload access;
  defer new NoGc/derive, split-type and lifetime-brand admission mechanisms. Retain
  safe rooted handles and audited library methods, with complete tracing, barrier,
  extraction and cleanup obligations, 2026-10-26.
- [x] Select tracing/incremental GC on 2026-10-26, superseding counted Value and
  cycle collection. Permit Value in unsafe Opaque providers, preserve safe retained
  Native handles and allow GC during Native callbacks.
- [x] Consolidate the Native API draft around existing roots, NativeType creation,
  unsafe scoped read/edit and retain/publication services; no with_storage or public
  prepare_removal/commit_removal pair. Exact resumable-scan/helper signatures remain open.
- [x] Select library-controlled temporaries for callback-bearing algorithms; remove
  the counted detached-buffer/Rust-sort conclusion. Preserve stable worst-case
  O(n log n), no retention prepass, receiver guards and callback/error semantics.
- [x] Record future operation families including Ord, predicates and custom iterators;
  per-item rooted iteration permits GC without whole-call exclusion. Incremental
  GC pause/progress validation remains separate from Native liveness correctness.
- [ ] NC01: Inventory and API contract.
- [ ] NC02: General storage capabilities.
- [ ] NC03: Vec migration.
- [ ] NC04: HashMap/HashSet migration.
- [ ] NC05: External extensibility and cursors.
- [ ] NC06: Final integration.

No implementation or performance claims are made at this checkpoint. No intermediate
failure is carried. Exact API names and the NC01 decisions remain implementation
design work, not acceptance waivers. Update this ledger rather than create a
parallel progress/review queue.

Planning refresh: inspected Iterable declarations/default requirements, adapter
bindings, retained-call APIs, destination construction and source revision validation
at 9cd90557. The old NativeCursor/mapping path is not a migration prerequisite.
Historical Iterable validation remains in Git; it does not establish NC acceptance.
Earlier test-cost measurements at 35de1818 remain historical and are
not a performance baseline for the rebased implementation.

Documentation-only validation uses content/link/diff checks; implementation
measurements begin at NC01.

Design reference: Rust models ordinary library structs through
[TyKind::Adt](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_type_ir/enum.TyKind.html);
[HashMap](https://doc.rust-lang.org/src/std/collections/hash/map.rs.html) and
[BTreeMap](https://doc.rust-lang.org/src/alloc/collections/btree/map.rs.html) are
library implementations. Kagari adopts that ownership boundary with precompiled
native entries, traced identity and checked callbacks; it does not import Rust's
script-visible borrowing model or promise identical storage/call costs.
