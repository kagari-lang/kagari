# kagari-gc implementation design

This document owns the standalone library's data structures, safety invariants
and operation protocols. The [redesign plan](gc-redesign-plan.md) owns remaining
scope, integration phases and acceptance; the [crate guide](../crates/kagari-gc/README.md)
provides executable usage examples. Runtime is in the RR03 coordinated consumer
cutover; the active execution plan owns its current build errors.

Implemented: typed Rust and fixed dynamic objects, host roots, pointer-sized
scoped identities, checked field access, provider mutation/cursor repair,
incremental collection, reclaimable metadata and opt-in driver safepoints.
RootDriver supplies direct execution-root scanning, tested with a Rust frame
adapter. Runtime packed-field integration, arbitrary erased providers,
integrated pressure policy and complete performance
instrumentation remain target contracts, explicitly identified below.

GC traces only its own allocations through explicit roots and ordinary edges;
there is no Domain or foreign-store binding. Managed storage is nonmoving, with
1 KiB pages, a 1024-unit normal allowance and 32-owner retirement batches.
Changes to these parameters require measured justification. Historical Runtime
source links pin revision 0490f8a4; they do not describe a completed cutover.

## Reading and implementation order

Read [ownership](#engine-ownership) first, then [storage](#physical-storage),
[roots](#roots-and-access), [marking](#marking-state), and
[object/root ownership](#object-graph-and-root-ownership). The
[operation algorithms](#operation-algorithms) define commit/failure order. The
[Runtime map](#runtime-integration-map) identifies what must leave GcHeap.
Borrowing restrictions are exercised by compile-fail examples beside the actual
[Heap](../crates/kagari-gc/src/heap.rs), [Scope](../crates/kagari-gc/src/access/scope.rs)
and [field APIs](../crates/kagari-gc/src/access/fields.rs), alongside behavioral
and Miri coverage of the implementation.

Code in `text` fences is a Rust-shaped schema: fields, ownership and state are
normative design decisions, while visibility and helper names may be refined.
These sketches explain ownership and algorithms; they are not production stubs.
Executable provider examples live in [tests/support](../crates/kagari-gc/tests/support.rs)
and the [linked-list example](../crates/kagari-gc/examples/linked_list/main.rs).
Focused behavior tests remain required before changing Runtime's collector.

## Engine ownership

```text
Heap {
    heap_id: HeapId,
    config: HeapConfig,
    physical: PhysicalHeap,
    roots: RootTable,
    cycle: Option<CollectionCycle>,
    active_scan: Option<ActiveScan>,
    workspace: ScanWorkspace,
    accounting: MemoryLedger,
    observations: CollectorObservations,
}

ObjectRef = private NonNull<ObjectHeader>
HeapId = nonzero, process-local u64
```

Heap owns no Runtime, Value, session, ModuleStore, source span, script type or
Native registry. It knows heap objects, its host root table and bounded root
enumeration supplied by the collection driver. Runtime
represents reachability-bearing state as ordinary payloads and keeps semantic
lookup/version policy outside the collector. Host Rust state remains host-owned;
host-retained managed targets use explicit Root/RootSet leases; execution roots
remain in the driver's authoritative frame storage.

The standalone facade uses `&mut Heap` for exclusive operations. The
RootDriver keeps that heap private and lends only restricted execution services;
Runtime may borrow the driver through one checked interior borrow at a short
access or service boundary. The collector owns phase and work; its exclusive loan
and split services make a graph-work mutex unnecessary. Root release has its own
small synchronized port. Scanner/charge/destructor failures fault the driver
without allowing sweep over an incomplete graph. Do not add storage locks or
RefCell admission per scalar instruction merely to cross a crate boundary.

Proposed module responsibilities:

| Module | State and algorithms |
| --- | --- |
| heap | Operation entry, field splitting, heap/phase admission, teardown |
| allocation | Page/large ownership, reservation, geometry and initialized storage |
| descriptor | Layout/type keys, scan/drop dispatch and descriptor leases |
| roots | Root packs, generation checks, release credits and root cursors |
| access | Read loans, initialization, checked field selectors and provider mutation |
| marking | Phase/epoch, marks, credits, jobs, active owner and closure |
| sweep | Physical cursor, preflight, batches and out-of-borrow disposal |
| accounting / observation | Committed bytes/work and opt-in fixed-buffer observations |

Library roots explicitly expose necessary types under repository re-export rules.
Semantic policies must not move into a generic helper or common crate to satisfy
dependencies. The library's own tests use ordinary Rust fixtures and no Kagari
crate; Runtime integration tests live with the affected Runtime contracts.

## Physical storage

### Object header and identity

```text
#[repr(C)]
ObjectHeader {
    descriptor_id: u32,
    allocation_id: u32,
} // exactly 8 bytes

PayloadLocation {
    allocation: u32,
    slot: u16,         // zero for a large allocation
}

PhysicalHeap {
    allocations: Table<Page>,
    available: [Option<u32>; 96],
    first_active: Option<u32>,
    sweep: PhysicalCursor,
    descriptors: DescriptorPool,
    backing_bytes: usize,
    not_sync: PhantomData<Cell<()>>,
}

Page = Small(SmallPage) | Large(LargePage)
```

ObjectRef names the header, not the payload. Admission first establishes a live
source/root/loan, then reads the header, resolves the original allocation and
descriptor, validates occupancy/type/bounds, and computes the payload offset.
An admitted scoped handle retains those facts until its scope ends; ordinary
scoped scalar access does not repeat physical resolution in release builds.
Never read an arbitrary expired pointer to test whether it is expired. Public
roots validate their heap/slot/generation before yielding any address.

There is no per-object generation directory. Allocation table indices identify
physical backing and can be reused only after cursor repair, mark unregistration
and final backing/descriptor disposal. Work must not refer to a released page.
Descriptor IDs are pinned by occupied/retiring slots and prepared descriptor
leases; they cannot be recycled while those leases remain. Weak caches validate
their original registration and do not revive an old numeric ID.

### Reclaimable metadata directories

Physical pages, root slots and descriptor entries use the same internal Table.
Small tables are direct growable arrays. Leaves contain at most 256 Option entries;
64-way directory branches are added only as IDs outgrow a leaf. Eight leaf bits
and up to four six-bit branches cover u32 IDs. Full-subtree bitmaps choose reusable
vacancies without a historical per-ID free list. Preparation reserves storage;
insertion commits without allocation. Lookup validates membership before returning
an entry. No managed objects or live IDs move when directory storage changes.

Removing the last entry in a block releases that block and empty ancestors, with
at most five bounded nodes affected. A lone high-ID survivor keeps only its block
and directory path, not all preceding empty slots. Partial blocks still fragment;
this is not compaction of live entries. A failed preparation can leave one empty
path for reuse/maintenance. Idle maintenance prunes it and collapses a sole child
zero into the root, preserving numeric IDs and restoring shallow access when possible.

This table owns storage only: roots own generations/leases, descriptors own use
credits/layout pins, and physical retirement owns page/backing lifetime. Metadata
reclamation does not weaken those admission or retirement boundaries. Numeric ID
exhaustion is checked. See [measurement and rationale](gc-redesign-plan.md#collection-and-retained-capacity).

### Small-page geometry and bitmaps

Reuse [current geometry](https://github.com/kagari-lang/kagari/blob/0490f8a4071f11ffcb98679f0eb3dd67eb8c9854/crates/kagari-runtime/src/gc/directory/pages/allocation/geometry.rs):

```text
SLAB_BYTES = 1024
STRIDES = [8,16,24,32,40,48,56,64,80,96,112,128,
           160,192,224,256,320,384,448,512,640,768,896,1024]
ALIGNMENTS = [16, 64]
EDGE_CLASSES = [edge_free, traced]
GROUPS = 24 * 2 * 2 = 96

ClassGeometry {
    stride: usize,
    slot_count: usize,       // floor(1024 / stride), at most 128
    bitmap_words: usize,    // two occupancy planes
    bits_offset: usize,
    marks_offset: usize,
    trailer_layout: Layout,
}

SmallPage { backing: Arc<Backing> } // sole typed trailer-access owner
SmallTrailer {
    geometry: &'static ClassGeometry,
    group: usize,
    free_count: usize,
    available_links: Links,
    active_links: Links,
    occupied: geometry-sized bitmap,
    unavailable: geometry-sized bitmap,
    marks: PageMarks,
}

Backing {
    base: NonNull<u8>,
    allocator_layout: Layout,
    payload_bytes: usize,
}
```

Trailer fields are physically coallocated after the 1 KiB payload region using
checked Layout composition, not an independent Vec/Box per page. Geometry is
immutable. Backing clones keep bytes alive but expose no safe payload/trailer
references; only the non-Clone page owner accesses controls.

| Logical slot state | occupied | unavailable | Authority |
| --- | ---: | ---: | --- |
| Free | 0 | 0 | Allocator may reserve |
| Reserved | 0 | 1 | InitTicket owns rollback; GC cannot scan |
| Initialized | 1 | 0 | Allocation/descriptor and marks are published |
| Retiring | 0 | 1 | Retired token owns destruction; allocator cannot reuse |
| Invalid | 1 | 1 | Fault before dereference or detach |

Reserved and Retiring share a bitmap encoding; their linear ticket distinguishes
them. Do not add a per-slot state enum solely to duplicate ticket ownership.
Free count equals slot_count minus the popcount of both planes. Tail bits above
slot_count must be zero. Slot addresses derive from checked base + slot * stride.

Select the first stride fitting the combined header/payload layout and divisible
by its alignment. Up to 16-byte alignment uses the 16-byte tier; 32/64-byte
alignment uses the 64-byte tier. Larger size/alignment uses an exact-layout large
allocation. Availability cannot cross alignment tiers by accidental address
alignment. A zero-sized payload still gets a unique initialized header.

### Large allocation and sweep cursor

```text
LargePage { backing: Arc<Backing> }
LargeTrailer {
    marks: PageMarks,                // only slot zero is used
    descriptor: Option<DescriptorId>,
    active_links: Links,
}

PhysicalCursor { page: Option<u32>, slot: u16 }
```

The large payload, alignment padding and trailer are one checked allocation.
There is no small-page slot array for large objects. Retain page-local marks
with the original backing.

The physical store owns its sweep cursor. When unlinking a page, repair a cursor
that names it before making its allocation index reusable, as in the
[existing cursor](https://github.com/kagari-lang/kagari/blob/0490f8a4071f11ffcb98679f0eb3dd67eb8c9854/crates/kagari-runtime/src/gc/directory/pages/sweep.rs).
Between service calls retain only page/slot progress. Reload bitmap evidence on
each borrow; two scratch words suffice for the largest small class. Do not keep
an occupancy mask, raw page slice or payload reference across a yield.

### Representation descriptors

```text
Descriptor {
    id: DescriptorId,
    type_key: RepresentationKey,        // physical Rust/erased representation
    allocation_layout: Layout,
    payload_offset: usize,
    scan: ScanDispatch,
    state_layout: Layout,
    drop_payload: Option<unsafe fn(payload_pointer)>,
    charge: unsafe fn(payload_pointer) -> ObjectCharge,
}

DescriptorPool {
    slots: Table<DescriptorEntry>,
    active: Vec<DescriptorId>,
    types: HashMap<TypeId, DescriptorId>,
}

DescriptorEntry { descriptor: Arc<Descriptor>, uses: usize }
```

Physical type/layout/scan/drop facts belong here. Kagari nominal type, module
provenance, field visibility and executable leases remain in Runtime. Typed Rust
registration verifies TypeId/Layout once. Fixed dynamic records use the safe
schema registration described below; arbitrary erased provider registration
remains an unsafe target contract requiring alignment, offsets, state and dispatch
proofs before admission.
Runtime interns its semantic layout separately and holds a compact descriptor
lease for prepared field access. It must not retain LoadedModule through a
generic physical descriptor.

The implemented descriptor table keeps stable ID slots plus a dense vector of
occupied IDs. Sweep snapshots the vector length and visits it backwards; removing
an unused descriptor swaps only an already-visited or newly appended ID into that
position. Object use credits and external Arc leases still prevent reuse of live
identities. Empty radix blocks release on retirement; no flat historical free
list remains. Dense traversal buffers shrink at idle metadata maintenance.
Each descriptor also stores the allocator group derived from its immutable
layout/alignment/edge kind. TypedDescriptor::alloc_leaf/alloc_with reuse this
registration for scoped allocations; Scope's convenience allocation still registers
on demand. Both enter the same checked allocation transaction, including heap
identity, object/backing/indirect limits, rollback and current-phase marking.

Descriptor state workspace is reserved/grown at registration before the descriptor
can be used. One reusable aligned workspace serves the one active object scan.
No allocation is needed to begin or resume an admitted scan. Cursor state is
owned, Send, pointer-free and contains no roots or hidden graph edges; use the
existing Copy-state rule initially. Drop dispatch is omitted only when Rust
needs_drop is false, or the erased representation contract proves no resources
are owned. A no-reference payload may still require destruction.

### Runtime-defined fixed records

Fixed dynamic records are implemented in [dynamic](../crates/kagari-gc/src/dynamic.rs).
It deliberately supports immutable field lists containing fixed-width integers,
bool, char, floating-point scalars and nullable managed references. FieldType is a
closed enum; the sealed Scalar trait prevents custom codecs or invalid scalar
representations. Runtime retains field names, nominal types, visibility and any
required/non-null initialization rules. The collector receives physical facts only.

```text
DynamicShape {
    identity: Arc<()>,             // builder identity, distinct from descriptor ID
    layout: Layout,                 // inline data size and alignment
    fields: Box<[Field { kind, offset }]>,
    references: Box<[usize]>,       // validated byte offsets, immutable
}
Representation = Rust { type_id, begin, scan, charge, destroy }
               | Dynamic(DynamicShape)
DynamicLayout { heap: HeapId, descriptor: Arc<Descriptor> }
DynamicLayoutBuilder { identity: Arc<()>, fields: Vec<FieldType> }
ScalarKey<V> / ReferenceKey { identity: Arc<()>, logical_index, type_marker }
dynamic::ScalarField<V> { layout: DynamicLayout, offset, type_marker }
dynamic::ReferenceField { layout: DynamicLayout, offset }
```

Access::register_dynamic places fields by descending alignment, breaking ties by
their original index, through checked Layout::extend and final padding. Typed
keys preserve logical field identity while physical offsets change. The closed field types
have sizes divisible by power-of-two alignments, so this order eliminates padding
between fields; final record padding remains. A temporary index vector and sort
are used only during registration, with no additional per-access work or per-object
metadata. It computes reference offsets from the resulting logical field table,
then installs a fresh descriptor in the existing pool. Each registration has an
independent identity, even at equal shapes. Semantic interning belongs to Runtime;
there is no second generic shape cache. Layout and field leases pin descriptor
identity against recycling; allocated/retiring objects use the existing usage
credits. Layout metadata contains no module or executable ownership.

DynamicLayoutBuilder::scalar<V> and reference append fields and return opaque
ScalarKey<V>/ReferenceKey values directly. Builder growth uses ordinary Rust Vec
allocation behavior; it does not promise recoverable allocation errors.
Registration and accessor binding remain fallible. Registration consumes the non-Clone builder;
keys survive moving it or growing its field vector. DynamicLayout::scalar and
reference consume the matching key and produce a pinned accessor. Binding compares
shared schema identity with Arc::ptr_eq before reading the logical field slot;
keys from other builders return Representation even at equal types/positions.
The key's Rust type prevents scalar/reference and scalar-type confusion. There
is no public raw-index binding or field-key constructor.

Each builder owns one fresh Arc identity allocation. Keys and the registered
DynamicShape share it, preventing identity reuse while any key survives. Keys
retain neither a descriptor nor managed objects; only bound accessors pin layout
metadata. This adds construction/binding metadata, not per-object storage or
per-access name/key lookup. Field operations retain their existing exact-layout,
heap-state, scalar-loan and reference-publication checks. Named Rust layout structs
and Runtime semantic maps own names; neither belongs in the physical collector.

This first subset uses aligned scalar/reference fields. Current Runtime packed
layouts use byte codecs and can have smaller offsets; adopting packed codecs or
mapping existing Value tags requires its separate integration proof/measurements.
The supported primitives and reference default values are not arbitrary Rust T.
Initialization zeroes the data and installs typed None reference slots. Every
field is valid before publication; required script fields are completed under the
VM's constructor semantics before script-visible exposure. Dynamic records own no
indirect capacity or custom destructor, so charge is zero and destruction is a
no-op; referenced Rust objects retain their ordinary charge/Drop behavior.

Scope::alloc_dynamic shares the physical reservation, initialization completion,
charge admission, current-phase allocation marking and rollback/commit transaction
with typed allocation. The transaction marks storage initialized before running
fallible charge/scan validation, so failure consumes owned payloads exactly once.
Dynamic initialization contains no outgoing edges and needs no initial graph walk.
No Box<Value>, Box<[u8]> payload, per-object field table or extra root is created.
The eight-byte header still identifies the descriptor and backing allocation.

Collection dispatch matches Representation once per erased operation. Dynamic
begin initializes a usize reference-table index in the normal scan workspace.
Scan charges every inspected slot, including None; Pending leaves an unvisited
target at the current index. The collector resumes the same object without keeping
a byte slice or field pointer. No-reference shapes use EdgeFree. Fixed-position
field updates never alter these scan coordinates or an existing restart decision.

Gc<AnyObject>, Root<AnyObject> and Edge<AnyObject> use the existing identity/root/
stored-carrier implementations. AnyObject is an uninhabited marker, not GcObject;
safe Rust cannot request a payload loan of it. Gc::erase preserves scope/liveness;
Scope::downcast requires an actual matching Rust TypeId. A dynamic layout with the
same bytes as u64 still fails that check. Root lease type metadata describes the
handle's static or erased view; physical representation is checked separately.
Mixed Rust/dynamic edges are visited by the same EdgeVisitor and page marker.

Scalar/reference field access checks heap and exact pinned descriptor before using
the prepared offset. Scoped admission supplies live-source retention. The object
header's descriptor ID is compared with the field token's ID after both heap
checks. Live-object use counts and the token's Arc prevent either ID from being
recycled; equal-shaped registrations still differ. Debug builds also resolve and
assert descriptor pointer identity. Scalar get/set is a closed operation with no
callbacks, changing neither charge nor tracing. Reference get returns a scoped
erased identity. Reference set publishes the protected incoming target through
the existing marker before a typed pointer store; failure preserves the old slot.
Pointers never round-trip through integer bits, and no raw byte or unrestricted
mutable payload view is exposed. These fixed field commits need no Rust ObjectMut
guard: the exclusive Scope excludes competing loans and there is no user code,
structural change or fallible work after the store. Custom Rust containers retain
the existing mutation guard/cursor-repair protocol.

Typed ScalarField::borrow_mut returns &mut V directly; the dynamic counterpart
returns Result<&mut V,GcError> because it admits an erased object's exact layout.
Both tie the reference lifetime to the exclusive Scope borrow, not to the field
selector. Admission happens once per loan. The field lease may be dropped afterward:
the live object pins its immutable descriptor, and Scope excludes collection.
The returned reference allows repeated scalar edits without descriptor checks,
charge sampling, scan invalidation or finalization on each edit. It cannot overlap
another object access or escape Scope. This is a scoped field reference, never a
general mutable T or reference slot. Typed fields rely on the existing unsafe
declaration contract; dynamic scalar validity follows from the sealed scalar set.
Unwind and forgetting a reference need no repair because every allowed field
value preserves tracing, scan coordinates and charge. Existing pending Restart
state is untouched. Heap-fault checks remain on each new loan. Scope brands prove heap membership;
unbranded roots, edges and layout tokens retain their admission checks.

Scoped reads and typed ScalarField::borrow_mut reuse Gc<T>'s storage and
representation proof. ObjectHeader::layout<T> owns the header-plus-T layout for
both registration and access, including alignment and zero-sized representations.
Scope::typed_pointer checks heap state, then computes the typed pointer from
this fixed layout. Matching invariant Scope/Gc brands prove heap membership
without a runtime ID comparison. It performs no release-mode page/slot/descriptor
lookup. Debug builds resolve storage and assert the exact type and resulting
pointer. The raw pointer remains internal; shared payload reads require &Scope
and mutable payload references require an exclusive &mut Scope loan. Copying a
Gc never grants overlapping read/write or mutable/mutable access.
Allocation, typed-root/root-pack admission, edge following and erased downcast
establish that proof; unsafe source_get must supply it under its provider contract.
No current operation changes the Rust representation during a scope. Root/erased
admission and dynamic layout checks remain fallible. Gc and stored Edge each
contain one pointer; there is no persistent pointer cache or new public unsafe API.

Access::with_reference also asserts the established Rust type only in debug
builds. Its callers are with_object (an admitted immutable Root<T>) and scoped
follow (an admitted Gc<T>); scoped read uses typed_pointer. Root construction fixes or validates the actual
representation; this private helper does not accept untyped public input. Root
identity checks and physical resolution remain, as do target type checks when
following a stored edge or converting erased/root-pack targets into typed handles.

## Roots and access

### Root table and release transport

```text
RootId { heap: HeapId, slot: u32, generation: u64 }

Root<T> { lease: Arc<RootLease>, type_tag: PhantomData<fn() -> T> }
RootSet { lease: Arc<RootLease> }
RootLease { id: RootId, released: Arc<Mutex<ReleasePort>>, armed: AtomicBool }

RootTable {
    heap: HeapId,
    slots: Table<RootSlot>,
    active: Vec<u32>,               // dense occupied IDs, independent of storage blocks
    serial: u64,                    // checked heap-local registration sequence
    releases: Arc<Mutex<ReleasePort>>,
}
RootSlot { generation: u64, entry: RootEntry }
RootEntry {
    active_index: usize,
    lease: Weak<RootLease>,
    targets: Box<[Option<ObjectRef>]>,
    mode: Immutable | Replaceable,
    typed_handle: Option<TypeId>, // Rust T or AnyObject; None for a replaceable pack
}
ReleasePort { connected: bool, credits: usize, pending: VecDeque<RootId> }
RootCursor { end: usize, next: usize, active: Option<RootId>, target: usize }
```

RootCursor indexes a captured prefix of active, not all historical slots. Root
release is consumed only in Sweep, after root scanning; swap-removal updates the
moved entry's active_index. New registrations append, and publication protects
targets beyond the captured prefix. Capacity is reserved before root commit.
Lease identity compares the existing Weak and Arc addresses without constructing
a temporary Weak. Root replacement admits the destination once, prepares and
publishes the source under an exclusive table loan, then commits without another
admission; preparation failure preserves the old slot.

Root and RootSet use one table and one lease protocol. A typed Root names an
immutable single-object target. A replaceable RootSet has a fixed target count;
replace a slot through publication, and allocate a new pack when cardinality
changes. create_roots reserves and registers an initially empty pack; setting
a slot imports a protected object, while clearing a slot stores None. This avoids
shifting a partially scanned pack. These packs serve independent host retention;
Runtime execution frames use the direct source below without a mirror. The core
stores no Value. Coupled host wrapper tag/root replacement remains one prepared,
non-reentrant commit.

Before installing a lease, reserve its radix slot/active-vector capacity and one pending
release credit. A prepared lease starts unarmed; only root-table commit arms its
release notification. Failed private preparation therefore creates no spurious
release for an unregistered slot. Clones share the Arc lease, not extra table registrations. Last
drop only appends its RootId under ReleasePort's mutex; it cannot allocate, call
providers, traverse the graph or destroy payloads. Keep a credit until that
notification is drained. Poison recovery of this small cleanup mutex must not
discard notifications. Heap teardown disconnects it, so later Root drop is inert.

At entry to the root-retirement stage, capture the pending count and consume that
many FIFO notices only. Later arrivals remain for the next cycle; they cannot keep
this cycle in retirement forever or overtake the captured prefix. VecDeque capacity
is reserved by the same release-credit admission, so last-root Drop still never
allocates. Each service shares the 32-total detached-owner cap with payload disposal.

Resolve a root in this order: heap ID, slot bounds, generation, occupied entry,
matching lease identity, mode/representation, then target. Each prepared registration
consumes a checked heap-local serial for its generation, including preparations
that subsequently fail. Removing/recreating a block never resets this serial.
Exhaustion rejects new registrations without invalidating existing roots. A root cursor revalidates its active
RootId on resume; a replaced slot is skipped, and new-root publication protects
the new pack. Only entries with a live external lease are roots from this table. An
expired entry may contain inert addresses awaiting release drain; never inspect
those referents. Discard its carriers without dereferencing them.

### Driver-owned root sources

The standalone [root-source API](../crates/kagari-gc/src/root_source.rs) implements
this protocol. Runtime's concrete adapter remains an integration target. Root sources
enumerate references to this heap's objects; their own Rust records are neither
marked nor swept. There is no Domain, external node ID, foreign object visitor or
dependency on a Runtime type. One aggregate source includes all execution roots,
including suspended/parked and outer reentry frames and their managed dependencies.
The collector combines it with the ordinary host root table.

Ownership sketch (actual owners, not compilable API declarations):

```text
RootDriver<R: RootSource> {
    heap: Heap,                       // private; never separately lent or extracted
    source: SourceScan<R>,            // private authoritative storage and progress
}
SourceScan<R> {
    roots: R,
    cursor: Option<R::Cursor>,        // initialized on every engine cycle start
    complete: bool,                   // valid only for that cycle
}

RootSource unsafe implementation contract:
    empty -> source without managed references
    begin_scan -> bounded cursor initialization
    scan_slice(cursor, budget, visitor) -> Pending | Complete | error

Runtime source storage:
    frame identities/generations -> current managed/scalar bank ranges
    managed slots and live execution-dependency references
    frame lifecycle/frontier metadata

Runtime cursor:
    finite captured frontier, stable frame identity/generation, field/slot ordinal
```

RootDriver owns one Heap and one source for their entire usable lifetime. It
offers execution scopes and step/collect through the same engine; it is not a
second collector. Construct it before installing execution references. Do not
offer safe raw Heap extraction, replacement of a populated source, or collection
with an optional/omitted source. Its scope interface must not expose a collection
capability. RootDriver::access lends ordinary Access for layout registration and
rooted/provider operations, never the raw Heap. Plain Heap remains usable for
standalone host-root-only consumers.
All explicit collect, pressure assistance and future automatic service on a
RootDriver route through its source. Drop permanently closes heap access and
disposes storage without trying to trace a partially dropped source. Host roots
outliving shutdown become inert under the existing teardown contract.

R owns frame semantics; GC only lends a restricted marking visitor and budget.
Its unsafe implementation must supply complete, same-heap, live references and
enumerate within charged work. It cannot allocate managed objects, call user
code, reenter, collect, or mutate frame storage during a scan slice. The visitor
cannot escape. Errors/unwinds quarantine the collection before any sweep; an
incomplete source can never count as Complete. The shared engine resets source
progress at each cycle start and requires both source and host-table scans plus all marking
work to finish before Sweep. Empty sources still complete through this protocol.

Source access is encapsulated by reviewed methods, not unrestricted mutable
managed-slot slices. RootDriver::scope lends Scope and RootAccess together.
RootAccess::source_mut is unsafe: an adapter must not replace/extract the source
or expose carriers; it checks the supplied Scope's heap identity before lending
storage. Scope::source_get imports a retained same-source slot under the adapter's
liveness/type proof; Scope::source_edge publishes a scoped target before storage.
These are provider primitives, not safe arbitrary address admission. The
[Frames adapter](../crates/kagari-gc/examples/root_access/frames.rs) encloses them
behind safe enter/leave/with_frame and get/set/copy methods. Its admitted frame
window reuses owner/generation evidence until the loan ends. An operation borrows disjoint heap/source state
and produces scoped handles from retained slots under its admitted source proof.
Gc<'gc,T> cannot be stored across scopes: source storage uses private inert
carriers/Runtime encodings whose interpretation is justified by this protocol.
Exiting a scope leaves every value needed by later execution in source storage
or an owning host root. A raw copied address in arbitrary Rust scratch is not a
root. The root_source contract tests exercise this facade and unwind behavior;
Runtime must still prove its concrete Value/dependency representation.

For a reference assignment `b = a`:

1. Admit heap/source identity and the frame window for a closed execution region;
   preserve language type, bounds, initialization and trap-order checks.
2. Read a from retained storage. An external value still needs ordinary checked
   admission; a source proof cannot admit foreign or stale handles.
3. Publish its target using the existing phase/mark owner, then store b's value/tag
   in a no-fail suffix. Neither collection nor reentry can separate these actions.
   Publication failure leaves b unchanged; conservative marking may remain.
4. Release storage/access loans before any safepoint or callback. Reacquire fresh
   admission after collection, reentry, resizing or frame lifecycle changes.

Scalar stores need no reference publication. Clearing a reference needs no new
target mark, but a value returned or transferred elsewhere must first be retained
in its destination. GC Idle may use a short publication path only when the
capability already proves ownership/liveness; public checked entrypoints keep
their validation. Marking keeps incoming publication for every installation,
including a new frame beyond the captured scan frontier. During Sweep, only
survivors/new allocations can be installed; this is not a resurrection interface.

The source cursor never retains a slice, Vec address or iterator between service
calls. Reacquire buffers and validate frame generations on each resume. Merely
using a numeric position is insufficient if compaction moves an unvisited slot
behind it: preserve stable logical slot coordinates, repair the cursor under the
same closed mutation, or publish every relocated target that could be skipped.
Frame metadata/dependency slots obey the same rule. New/reused frames publish all
initial references before becoming observable; retired frames cannot later be
interpreted through stale cursor identities. Return/trap/cancellation remove a
frame only after surviving results have been transferred.

For example, after scanning slot 0 of `[None, B]`, moving B to slot 0 and clearing
slot 1 publishes B before the move commits. The later slot-1 scan can safely see
None. This uses the existing incoming barrier and requires no final atomic stack
rescan, per-frame dirty registry or Go-style hybrid deletion barrier.

During host reentry, suspend outer storage loans but keep outer frames in the
same source. Nested execution borrows the same driver after the outer operation
ends. A source describing only the currently executing frame is unsound. Neither
the driver nor the generic collector determines Kagari call/parking policy.

The [decision and acceptance owner](gc-redesign-plan.md#remaining-decisions-and-work)
records remaining integration and acceptance obligations.

### Short access capabilities

```text
Access<'h> {
    heap: &'h mut Heap, // methods lend disjoint physical/root/marker/state services
    non_transferable: PhantomData<Rc<()>>,
}

View<'read, T> { object: &'read T, retention: short source-loan capability }
Edge<T> { reference: ObjectRef, type_tag: PhantomData<fn() -> T> }
Scope<'gc> { access: Access<'gc>, invariant_brand: PhantomData<fn(&'gc ()) -> &'gc ()> }
Gc<'gc,T> { reference: ObjectRef, invariant_brand, type_tag }
```

Edge has one pointer of storage; PhantomData adds no bytes. On a 64-bit target
its intended size/alignment is that of the object pointer. Root and Edge refer
to the same allocated payload; constructing an Edge does not copy that payload,
allocate a per-edge record or increment a per-target reference count. Roots have
their separate table/lease cost. An Edge has no independent retention authority.

Gc is a transient, Copy identity rather than a stored edge or a data reference.
Its pointer occupies 8 bytes on the current 64-bit target; lifetime and type
markers add no storage, and there is no per-handle allocation/root lease. Heap::scope creates an invariant
generative lifetime that cannot escape or be shortened into another scope's
brand. Keep all actual object/Edge representations 'static and unbranded. Handles
may coexist, including aliases; Scope lends payloads only through short shared
callbacks or exclusive loans. Clearing an edge/root slot does not invalidate a
handle in the same noncollecting scope. Scope::retain or set_root is required
before taking the target across collection/callback boundaries.

The only private constructor establishes a live, initialized, same-heap target
with the exact Rust representation (or AnyObject erasure). Allocation, Root and
RootSet imports, checked edge following and downcasts establish this at admission;
unsafe source_get requires the paired retained source slot to establish it.
Erase preserves the existing brand. Init::edge_scoped and Mutation::edge_scoped
accept a handle without sharing its scope brand, so they keep physical membership
validation; RootAccess likewise keeps its heap-ID check. A state failure can occur
inside a live Scope and be caught by the caller, so branding does not replace the
Faulted gate. Closed is still handled by the shared state checker. Neither this
proof nor its pointer-sized handle admits arbitrary dynamic layout tokens.

Scope::read/get/get_root/follow/downcast and typed/dynamic scalar/reference getters
use shared Scope borrows. Nested reads, including aliases of the same object,
are allowed; the higher-ranked callback still prevents borrowed payloads/edges
from escaping. Scope::follow selects an edge through this shared read kernel and
validates its original-heap membership and target representation before returning
a handle; it does not request Retention's mutable root/marker services. Writes,
allocation, publication, root registration and mutable loans retain &mut Scope.
The compiler excludes mutation while any shared read is active, and excludes
reads while a mutable field/object loan remains in use. No RefCell, lock or runtime
borrow counter is introduced. The separate rooted Access::with_object callback
still requires &mut Access because View::retain can register a child Root.

Heap::access checks phase before splitting heap-owned fields. It holds no graph
mutex guard and accepts no Runtime binding. Access's exclusive heap loan excludes
step/collect/shutdown; the Runtime facade also rejects reentry through an aliased
checked borrow. Drop Access before callbacks or collection.

Scanning a payload borrows initialized bytes and the descriptor. Publication can
resolve another object through a shared reborrow of the physical store and shade
its disjoint page marks; it does not request &mut PhysicalHeap while the payload
loan is alive. Page marks use Cell under the exclusive driver. Marker work and
root registration use disjoint mutable services, with no graph mutex across
provider code, destructors or a service return. The only synchronized port is
last-root release, which grants no physical access.

Public reads use a higher-ranked callback whose return type is independent of
the view lifetime. Returning an owned scalar or Root is allowed; returning &T,
a borrowed string slice, View or borrowed edge reference is not. NativePayload-style unsafe
implementers must ensure shared T access cannot mutate edges/charge secretly via
interior mutability. No unrestricted safe mutable payload reference is exposed.

For read-only traversal, View::follow takes a source-borrowed &Edge and returns a
same-heap, exact-type &U bound to the enclosing read callback. Its physical loan
excludes collection/mutation and every followed source remains borrowed for that
interval. Walk the graph within one with_object callback without registering
temporary roots, marking targets or cloning leases. Reject foreign edges by
checking the candidate live header's original page/membership before exposing a
payload reference; do not probe an expired owned carrier. The higher-ranked
callback prevents followed references from escaping. No new branded object model
or independent borrowed-handle registration is introduced.

For extraction beyond the callback or across source mutation, View can retain a
child while its source loan remains valid.
Root registration borrows the disjoint root table and page-mark services;
it does not end the source loan before protecting the result. Copying Edge does
not create retention and supplies no safe dereference/retain API by itself.

### Concrete entrypoint shapes

All inputs below are borrowed unless an owned token is explicitly consumed.
There is no domain type parameter, external registration or binding argument.
The public managed-type trait is [GcObject](../crates/kagari-gc/src/object.rs),
including scanning and owned-capacity accounting. Its optional future derive
uses `#[derive(GcObject)]`; physical payload names describe storage data.

The typed and fixed dynamic subsets are implemented. register_erased and take
below describe remaining target interfaces, not public methods of the first crate.

```text
Heap::new(config) -> Heap
Heap::access(&mut self) -> Result<Access<'_>, GcError>
Heap::scope<R>(for<'gc> FnOnce(&mut Scope<'gc>) -> Result<R,GcError>) -> Result<R,GcError>
Heap::step(&mut self, WorkBudget) -> Result<StepReport, GcError>
Heap::safepoint(&mut self) -> Result<Option<AutomaticReport>, GcError>
Heap::collect(&mut self) -> Result<CollectionReport, GcError>
Heap::shutdown(&mut self) -> Result<ShutdownReport, GcError>

Scope::get<T>(&self, &Root<T>) -> Result<Gc<'gc,T>,GcError>
Scope::get_root<T>(&self, &RootSet, index) -> Result<Gc<'gc,T>,GcError>
Scope::set_root<T>(&RootSet, index, Option<Gc<'gc,T>>) -> Result<(),GcError>
Scope::retain<T>(Gc<'gc,T>) -> Result<Root<T>,GcError>
Scope::alloc_leaf<T>(T) -> Result<Gc<'gc,T>,GcError> // requires EdgeFree
TypedDescriptor<T>::alloc_leaf(&self, &mut Scope<'gc>, T) -> Result<Gc<'gc,T>,GcError>
TypedDescriptor<T>::alloc_with(&self, &mut Scope<'gc>, initializer) -> Result<Gc<'gc,T>,GcError> // unsafe
unsafe Scope::alloc_with<T>(initializer) -> Result<Gc<'gc,T>,GcError>
Scope::read<T,R>(&self, Gc<'gc,T>, for<'read> FnOnce(&'read T) -> R) -> R
Scope::follow<T,U>(&self, Gc<'gc,T>, source-borrowed edge selector) -> Option<Gc<'gc,U>>
unsafe Init::edge_scoped<T>(Gc<'gc,T>) -> Result<Edge<T>,GcError>

Access::register<T: GcObject>() -> Result<TypedDescriptor<T>, GcError>
DynamicLayoutBuilder::new() -> DynamicLayoutBuilder
DynamicLayoutBuilder::scalar<V: Scalar>(&mut self) -> ScalarKey<V>
DynamicLayoutBuilder::reference(&mut self) -> ReferenceKey
Access::register_dynamic(DynamicLayoutBuilder) -> Result<DynamicLayout,GcError>
Scope::alloc_dynamic(&DynamicLayout) -> Result<Gc<'gc,AnyObject>,GcError>
DynamicLayout::scalar<V: Scalar>(ScalarKey<V>) -> Result<dynamic::ScalarField<V>,GcError>
DynamicLayout::reference(ReferenceKey) -> Result<dynamic::ReferenceField,GcError>
Gc<T>::erase() -> Gc<AnyObject> // preserves the same scope
Scope::downcast<T: GcObject>(&self, Gc<AnyObject>) -> Result<Gc<T>,GcError>
Access::register_erased(unsafe representation contract) -> Result<DescriptorLease, GcError>
unsafe Access::alloc_with<T,F>(descriptor, initializer: F) -> Result<Root<T>, GcError>
    F: for<'init> FnOnce(Init<'init>) -> Result<T, GcError>
unsafe Init::edge<T>(&Root<T>) -> Result<Edge<T>, GcError>
Access::alloc_leaf<T: GcObject>(value: T) -> Result<Root<T>, GcError> // requires EdgeFree
Access::with_object<T,R,F>(root: &Root<T>, reader: F) -> Result<R, GcError>
    F: for<'read> FnOnce(View<'read,T>) -> R
View::retain<T>(&mut self, &Edge<T>) -> Result<Root<T>, GcError>
View::follow<U>(&self, &'read Edge<U>) -> &'read U
ScalarField<T,V>::get(&Scope, Gc) -> V
ScalarField<T,V>::set(&mut Scope, Gc, V) -> ()
ScalarField<T,V>::borrow_mut(&mut Scope, Gc) -> &mut V
EdgeField<T,U>::get(&Scope, Gc) -> Option<Gc<U>>
EdgeField<T,U>::set(&mut Scope, Gc, Option<Gc>) -> ()
Access::create_roots(slot_count: usize) -> Result<RootSet, GcError>
Access::replace_root(&RootSet, index, protected_object_or_empty) -> Result<(), GcError>
```

Root admission validates heap/slot/generation/lease identity before any header
access. Existing traced edges require their source loan instead of arbitrary
address probing. Init::edge imports a live same-heap root into the unpublished
payload. The initialization exclusion keeps imported targets physically valid
until completion even if a source root is released meanwhile. It exposes no
reentry or collection operation. The final allocation scan validates/publishes
outgoing edges before committing the object; Init::edge need not duplicate that
marking pass. It creates no new Root per imported edge.

Initialization exclusion proves target validity only during that operation.
A higher-ranked callback does not prevent an unbranded Edge from escaping through
a captured outer variable. Therefore alloc_with and edge imports are unsafe
provider operations, Edge is opaque and non-Copy/non-Clone, and safe preassembled
allocation requires EdgeFree. Safe APIs cannot insert an arbitrary saved carrier.
Reviewed providers encapsulate construction and expose safe ordinary operations;
a general builder/derive requires its own complete safety proof. The
[remaining authoring work](gc-redesign-plan.md#remaining-decisions-and-work)
does not change the current unsafe contract.

The implemented ScalarField<T,V> and EdgeField<T,U> retain typed read/write
function-pointer projections declared once under an unsafe provider contract.
ScalarField requires Copy values whose arbitrary safe mutation preserves tracing,
scan coordinates, charge and representation invariants. EdgeField supports an
optional edge at a fixed scan position; setting it publishes before replacement.
Both projections must identify the same field and be pure/infallible. Ordinary
get/set/borrow_mut operations are safe and return directly. Invalid typed access
or stored-edge representation is a contract violation and panics before the
rejected address is dereferenced. Heap-state checks remain active. The matching
invariant Scope/Gc brands exclude foreign scoped handles at compile time; Root,
RootSet, layout-token and unbranded edge admission still validate external inputs
in release builds.
Scope::read returns the callback's own result without adding a GC Result layer.
An application callback panic does not roll back completed scalar edits.

Closed fixed-edge writes admit lifecycle state once: typed_pointer for typed
receivers, exact-layout pointer admission for dynamic receivers, and an immediate
state check in source_edge for execution-root publication. Scope::publish_admitted
reuses that admission and the invariant target brand. No callback, reentry or
state-changing operation may intervene. Writing None still admits the receiver.
The crate-internal Marking::publish_admitted contains the single marking algorithm;
its checked publish wrapper remains for other provider/root/scanner callers.
Debug assertions enforce the admitted precondition. Physical membership, sweep
liveness, queue/work accounting and publication-before-store remain checked.
There is no public unchecked API or cached state proof across operations.

Fixed-edge publication completes before the store. If the reserved marking
operation fails, Scope::publish_admitted faults the heap before returning the error to
its caller; typed EdgeField::set panics on that failure. Catching it cannot resume
the collector. External admission failures leave a healthy heap usable. Dynamic
field APIs retain explicit layout/admission errors;
their shared publication kernel also faults on a marking failure. Allocation,
root retention/import, dynamic downcast and collection/provider boundaries keep
Result. Collection failures can be reported to the host without unwinding its
whole call stack, but Faulted permits shutdown only, not collector repair.

Runtime compact packed layouts still need
prepared identity, offset/encoding and bounds evidence; this typed mechanism does not
claim to implement the Runtime's compact-offset fast path. Runtime must preserve
language access/type checks and compatible-layout fallback when integrating it.
The [host/execution distinction](gc-redesign-plan.md#remaining-decisions-and-work)
keeps these kernels shared: interpreter field operations consume admitted frame
and region evidence instead of creating host Root leases for every object load.
Scalar writes need no reference barrier; managed destinations, including frame
root slots, still perform required publication. Do not infer zero instruction
cost from the absence of per-field Root creation or a generic view callback.
The [region-boundary contract](gc-redesign-plan.md#remaining-decisions-and-work)
defines exits by proof validity and cooperative requests, rather than a fixed
instruction count. Preserve current logical-PC polling and distinguish it from
the collector's work budget; release loans before servicing a request/transition.

### Provider mutation entrypoint

The private Access::borrow_object_mut kernel requires lifecycle admission from
its caller. All entries (rooted Access::with_object_mut and scoped with_object_mut/
borrow_object_mut) check state before invoking it, without an intervening callback
or state transition. The kernel asserts that precondition in debug builds;
physical/type/active-scanner validation and charge/finalization remain shared.
A faulted existing Access or Scope is rejected before lending T or running the
provider callback. Admission does not authorize continuing after a charge failure.

The fixed-field API is insufficient to express an independently implemented
container operation. Complete the proposed provider surface as follows; these
are implemented by the typed Rust provider kernel; the snippets remain schematic:

```text
unsafe Access::with_object_mut<T,R,F>(
    &mut self, owner: &Root<T>, operation: F
) -> Result<R, GcError>
    T: GcObject
    F: for<'op> FnOnce(&'op mut T, &mut Mutation<'op,T>) -> Result<R,GcError>

Mutation<'op,T: GcObject> // bound to the admitted receiver and its ScanState type
unsafe Mutation<T>::edge<U>(&Root<U>) -> Result<Edge<U>, GcError>
unsafe Mutation<T>::edge_scoped<U>(Gc<'gc,U>) -> Result<Edge<U>, GcError>
unsafe Mutation<T>::retain_existing<U>(&Edge<U>) -> Result<Root<U>, GcError>
unsafe Mutation<T>::publish_existing<U>(&Edge<U>) -> Result<(), GcError>
unsafe Mutation<T>::with_scan_state<R,F>(&mut self, operation: F) -> R
    F: for<'scan> FnOnce(Option<&'scan mut T::ScanState>) -> R
unsafe Mutation<T>::continue_scan(&mut self)
Mutation<T>::prepare_charge(ObjectCharge) -> Result<(), GcError>
```

The unsafe entrypoint is for reviewed provider implementations, not a safe edit
closure offered to ordinary clients. Its caller guarantees complete tracing,
no hidden interior edge/charge mutation, source protection for existing-edge
operations, valid storage throughout errors/unwinds, and an honest continuation
proof. Provider methods can encapsulate it behind a safe operation such as
ObjectVec::remove. It does not perform Kagari access/type policy.
The Vec/Map listings below show that provider kernel; they do not define a
complete safe custom-object/container authoring layer. Its lifetime/admission,
barrier and accounting proofs remain required before exposing safe wrappers.

with_object_mut validates the root/descriptor and establishes the exclusive
operation loan. It reads the registered current charge and creates a linear
mutation guard. The guard defaults to Restart, pins the
receiver, excludes collection and competing payload loans, and owns accounting
finalization. It reborrows only disjoint root/header/page-mark services;
those services cannot create another payload reference aliasing the active &mut T.
No graph guard spans the provider closure. A higher-ranked closure result cannot
contain the temporary &mut T or Mutation loan.

The operation guard also admits a disjoint typed scan-state loan. Before invoking
the provider, it checks whether ActiveScan names this exact receiver, has initialized
state and has no pending Restart. If so, the retained descriptor must match T and
its registered ScanState layout/dispatch before lending the workspace bytes as
T::ScanState. A descriptor/type mismatch faults admission before provider edits;
never interpret another owner's state or expose raw workspace casts to providers.
The typed API uses the same representation registration as scanning, not an
independent unchecked state-type parameter. Erased providers require equivalent
registration-bound evidence rather than guessing a Rust type from an offset.

with_scan_state passes Some(state) only for that reusable active state; it passes
None if there is none, another owner is active, or this owner's restart is already
pending. This is not a claim that no collector cycle exists. The short state loan
cannot escape, start/resume scanning, allocate or call scripts. It may accompany
the receiver's exclusive object loan because the workspace is disjoint and scan
state contains no object pointers or borrows. All admission is complete before
this infallible helper invokes the provider's closed commit. continue_scan records
that this operation preserves continuation; it never clears an earlier pending
Restart. On error/unwind before that assertion, the guard defaults to Restart
for this owner. Restoring accounting and valid object storage remains mandatory.

There is no caller-supplied resource plan before the provider can inspect its
storage. The provider reads its actual len/capacity under this loan, decides
whether growth is needed and prepares fallible capacity/accounting work through
the operation's services before its destructive commit. The core does not infer
container geometry. Ordinary users call push/remove, not an API requiring them
to predict hidden buffer capacity. Growth-admission details remain a GCR00 proof
obligation; do not replace the removed plan with another guessed pre-access bound.

edge imports a live same-heap root and publishes it before returning a stored
carrier. The two existing-edge methods are unsafe because an arbitrary copied
Edge has no liveness authority: the provider must source it from this admitted
payload/traced scratch and retain that source until publication/retention finishes.
retain_existing creates the result root before a removal. publish_existing shades
a traced relocation without adding a permanent root per element. continue_scan
is unsafe and records Continue only after the caller has proved the saved scan
is still valid or all skipped/moved targets are covered. Any later edits must
preserve that proof. There is no freely asserted safe continuation boolean.

On ordinary exits the mutation guard samples the infallible registered charge,
commits accounting and applies Continue/Restart before releasing exclusion.
`ObjectMut::commit_accounting` also commits while retaining the exclusive loan,
without clearing the preinstalled scanner fallback. A provider changing charge
must checkpoint before exposing a safe proxy that may be forgotten. Later
`parts`/callback edits invalidate that checkpoint; finalization samples only if
needed, so an already committed unchanged loan does not call the hook twice.
The existing admitted snapshot becomes the next operation's old charge.
Scalar-field loans remain charge-preserving and require no checkpoint.

HeapStats reports committed `provider_units` as a u128 sum of usize declarations,
separately from object count. Allocation, mutation and retirement update bytes
and units together after complete checked preparation; shutdown clears both.
Runtime owns peak/session policy, and these units do not affect collector pacing.
A detached `heap::accounting::AccountingReport` records the latest access or service
interval's before/after occupancy and independent committed unit/indirect-byte
peaks. Logical occupancy includes object count plus provider units as a u128 sum.
The report resets when a new access/service interval starts, including rejected
admission, and reading it has no effect. Actual allocation/mutation commits update
peaks; failed preflight requests do not. Collection/shutdown expose their reduced
current occupancy. Runtime consumes reports after physical loans end, including
unwinding, without installing a callback or shared ResourceState in the collector.
No per-object charge directory, session identity or language graph is added.

An error before a destructive commit leaves logical contents intact; capacity changes
already committed remain honestly charged. An interrupted operation must have
restored or retained scan-valid payload state before this finalization. Drop
does not attempt a general rollback of arbitrary Rust edits. Broken storage,
accounting or scan contracts fault the engine. Removing the up-front plan does
not remove existing resource/limit checks: growth preparation stays fallible and
precedes growth/commit as required by its owner. Post-edit charge sampling does
not substitute for quota admission. Provider growth policy must account for
allocator capacity behavior and any retained/staged backing; its actual loan,
failure and unwind composition must be proved before implementation. The removals
below inspect their storage in the closure and need no growth reservation.

### New API example: a resumable Vec provider

The following example uses the proposed standalone types. Item is assumed
registered as an edge-free leaf; all references target ordinary heap objects.
Error spelling and omitted charge code are mechanical details; the ownership,
order and unsafe obligations are required.

```text
struct ObjectVec { items: Vec<Edge<Item>> }
struct VecScan { next: usize, end: usize }

unsafe impl GcObject for ObjectVec {
    type ScanState = VecScan;
    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> VecScan {
        VecScan { next: 0, end: self.items.len() }
    }

    fn scan(&self, state, budget, visitor) -> ScanResult {
        while state.next < state.end {
            if !budget.take(1) { return Ok(Pending); }
            if let Some(edge) = self.items.get(state.next) {
                // The registered scanner's live owner protects the stored edge.
                if unsafe { visitor.try_visit_stored(edge, budget)? } == Pending {
                    return Ok(Pending);
                }
            }
            state.next += 1;
        }
        Ok(Complete)
    }

    // charge reports len units and Vec-owned indirect capacity; no child bytes.
}

fn ObjectVec::remove(
    access: &mut Access<'_>, owner: &Root<ObjectVec>, index: usize
) -> Result<Root<Item>, GcError> {
    unsafe {
        access.with_object_mut(owner, |vec, mutation| {
            let source = vec.items.get(index).ok_or(IndexError)?;
            let result = mutation.retain_existing(source)?;
            mutation.with_scan_state(|scan| {
                // No allocation, callbacks, collection or fallible work in this commit.
                vec.items.remove(index);
                if let Some(state) = scan {
                    if index < state.next {
                        state.next -= 1;
                    }
                    if index < state.end {
                        state.end -= 1;
                    }
                }
            });
            mutation.continue_scan();
            Ok(result)
        })
    }
}
```

Only the provider indexes Vec or invokes remove. The mutation capability knows
the receiver identity and admits its typed scan state; it does not interpret
indices or derive which elements move. All fallible result rooting and admission
precede Vec's no-allocation commit. Stored Edge destruction invokes no user code
and does not destroy Item; the result Root already retains the removed target.
The provider changes the container and cursor under the same excluded loan.
If scan visitation needs marker budget it returns Pending without advancing
state.next. The inspected
slot and lazy mark initialization have separate charge ownership; previously
prepaid provider units use the existing credit path rather than charging twice.

For a successful ordered removal, let k = len - index - 1. The example shifts k
stored edges and repairs at most two cursor fields: container work is Theta(k + 1),
with constant auxiliary container space and constant additional scanner maintenance.
Result Root registration and possible root-table growth are separate costs; there
is no suffix publication loop. This matches the growth order of Rust's
[Vec::remove](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.remove),
and requires no relocation-publication pass over the shifted suffix. Repeated
front removal is quadratic. Reduced work is not measured throughput acceptance,
and the collector step budget does not bound this mutator operation's duration.
Long-operation cancellation remains an integration obligation.

The selected ordered-removal protocol requires 0 <= next <= end <= old_len, with
end exclusive. All ObjectVec structural operations must preserve this invariant
or request Restart before the state can be reused. After successful removal at i,
decrement next if i < next and end if i < end, using their pre-edit values. If
i == next, leave next unchanged so the shifted successor is visited next. Removing
an appended element at or beyond end changes neither field. The repaired cursor
names the remaining original unvisited elements in their new positions, without
revisiting the whole prefix or missing a boundary element. Appended targets outside
the captured extent still need their separate publication.

When there is no reusable active state for this owner, the closed removal needs
no cursor update: an unstarted scan begins fresh, a completed scan already covered
old references, or a pending Restart initializes fresh state on resume. Result
retention remains necessary in every case. The typed implementation and external
provider tests exercise this protocol; erased-layout/Miri validation remains open.
Do not apply this numeric repair to
arbitrary reorderings or opaque HashMap iterators; retain covered publication or
Restart for operations without their own continuation proof.

### New API example: an opaque standard HashMap provider

A separate application provider may use the ordinary Rust HashMap directly:

```text
struct ObjectMap { items: HashMap<u64, Edge<Item>> }

unsafe impl GcObject for ObjectMap {
    type ScanState = ();
    const SCAN_KIND: ScanKind = ScanKind::Indivisible;

    fn begin_scan(&self) {}

    fn scan(&self, _state, budget, visitor) -> ScanResult {
        // Standard HashMap traversal also inspects empty bucket capacity.
        budget.charge(self.items.capacity());
        for edge in self.items.values() {
            // Target marking charges its separate work, including any excess.
            unsafe { visitor.visit_stored_indivisible(edge, budget)? };
        }
        Ok(Complete)
    }

    // charge counts capacity units and estimated HashMap backing; no child bytes.
}

fn ObjectMap::remove(
    access: &mut Access<'_>, owner: &Root<ObjectMap>, key: u64
) -> Result<Option<Root<Item>>, GcError> {
    unsafe {
        access.with_object_mut(owner, |map, mutation| {
            let Some(source) = map.items.get(&key) else { return Ok(None); };
            let result = mutation.retain_existing(source)?;
            map.items.remove(&key);
            // No partial HashMap iterator/cursor survives an indivisible scan.
            mutation.continue_scan();
            Ok(Some(result))
        })
    }
}
```

try_visit_stored and visit_stored_indivisible are generic EdgeVisitor operations,
not container hooks. The former reserves remaining mark work and can return
Pending before publishing a target; the latter charges the required excess and
never returns Pending. Both require the scanner's admitted source-liveness proof,
validate the original target/mark and report faults rather than allow partial
mark success. They never invoke scripts or recursively scan the target in place.

This map is deliberately Indivisible: its standard iterator is created and
consumed under one scan loan, and no mutator runs in its middle. A pending map
starts with its current contents; a completed map already protected its old
references; new imports use mutation.edge. Thus private bucket relocation needs
no saved-iterator repair. Managed keys would require scanning/importing both key
and value. Rust u64 hashing/equality here invokes no script callback.

The cost is an unbounded-in-size map scan. This example demonstrates provider
extensibility, not a replacement for the resumable indexed stdlib map or a claim
that a large HashMap meets normal-pause gates. To require bounded map scanning,
use the indexed-reference layout described below, or another explicitly proved
representation. Core dispatch still uses only the descriptor's scan kind/function.

### Calling these providers through the new facade

```text
let mut access = heap.access()?;
let descriptor = access.register::<ObjectVec>()?;
let vector = access.alloc_with(&descriptor, |mut init| Ok(ObjectVec {
    items: vec![init.edge(&item_a)?, init.edge(&item_b)?],
}))?;
let removed = ObjectVec::remove(&mut access, &vector, 0)?;
drop(access);
heap.step(WorkBudget::new(1024))?;
// removed is an independent Root<Item>, not an unrooted returned Edge.
```

GcObject::scan returns ScanResult, including checked visitor failure, for these
new-library examples. These listings remain schematic, while concrete typed
versions compile in the independent crate's external provider tests. Opaque-map,
custom erased-layout and Runtime prepared-field portions remain target contracts;
the fixed dynamic record API described above is implemented. The separate
Rust borrow proof below covers only its stated declaration/loan properties;
actual API compile-fail doctests now also reject escaping View/scan-state loans
and safe calls to raw initialization.

## Marking state

```text
Phase = Idle | Roots | Mark | Sweep | Faulted | Closed
Marking {
    phase: Phase,
    epoch: u64,
    pending: Vec<Job>,
    cycles: u64,
    publication_work: usize,
}
Job = DirectBlock(allocation_index)

PageMarks {
    epoch: Cell<u64>,
    discovered: [Cell<u64>; 2],
    scanned: [Cell<u64>; 2],
    queued: Cell<bool>,
}
MarkRef<'m> = Slot(&'m PageMarks, slot, allocation_index)

ActiveScan {
    owner: ObjectRef,
    descriptor_lease: DescriptorLease,
    restart: bool,
}
CollectionCycle {
    root_cursor: RootCursor,
    retirement: Option<Retirement>,
    pressure_at_start: CommittedPressure,
    work: usize,
    service_time: Duration,
}
```

The single active cursor lives in Heap. ActiveScan is the sole active identity;
its presence proves that the registered state was initialized successfully.
Restart requests discard the old Copy state on resume. It is empty at completed
Idle. The marker queue contains page jobs, not a second copy of the active owner.

Phase is authoritative in Marking, never mirrored in Access. There is no graph
mutex in the exclusive standalone driver. Collection scanner/charge failure or
panic faults the heap and prevents sweep over a partial mark. Destructor panic
faults before the next destructor and the detached batch still finishes cleanup.
An ordinary provider operation unwind instead invalidates its active continuation
and finalizes charge if storage remains valid. Closed permanently rejects access.
Do not clear a fault to resume ordinary work.

White means epoch mismatch or neither bit; Gray is discovered without scanned;
Black has scanned. Lazy epoch initialization clears exactly four bitmap words
and is charged. Checked epoch exhaustion faults instead of wrapping and making
old bits current. All mark and queue access occurs under the owning heap
loan and split marker services. A MarkRef never survives that loan.

Before physical admission, reserve queue capacity for the current page count
plus the one possible new page, and deduplicate page jobs with queued. Never size
this reservation from the highest historical page ID. Each page has at most one queued
job; occupancy and the physical object count own object admission. The exclusive
driver needs no duplicate identity-credit directory or registered flag per page.
A large allocation uses the same rule with slot zero. No per-edge registry is needed.

AllocationGuard owns an unavailable slot, descriptor and rollback without exposing
initialized occupancy. Publish outgoing edges using the same GcObject scanner
and prepare an unarmed result lease before committing. Commit sets current-phase
color, initialized header/occupancy, descriptor use count and the armed result root.
On failure the guard destroys only initialized private storage and releases its
slot/page. Spare capacity can remain; no initialized unrooted result escapes.

An active owner remains Gray until its scanner finishes. Other gray slots in
the same block remain queued. At most one descriptor state is initialized in
ScanWorkspace. A provider Pending leaves its numeric cursor and active identity
in Heap; all payload/page/graph loans are released. Completion reacquires the
same identity/mark before changing it to Black.

Admitted Rust payloads and owned scan state must be Send + 'static, as in the
current provider contract. Erased layout/scan/drop dispatch must preserve that
condition. Opaque pointer carriers may have narrow justified Send/Sync transport
implementations only because they expose no safe dereference; they do not make
the physical owner Sync. Active access and payload loans remain non-transferable.

## Object graph and root ownership

GcObject scanners visit only objects allocated in this Heap. An opaque Runtime
ID is not a traced reference. There is no callback for resolving, marking or
sweeping records in another store. Cross-heap references are rejected at root
admission; stored carriers require the registered scanner's source-liveness proof.

Root/RootSet leases express retention from outside the managed graph. A host
structure may own roots while using the standalone collector. Internal cycles
must use traced Edge carriers, not Root fields that unconditionally retain their
targets. RootSet is a root-table pack, not a way to register foreign graph nodes.

Runtime maps a live published installation, active frame, parked task or host
handle to genuine roots. It maps function/environment/static relationships whose
lifetime depends on reachability to ordinary managed objects. Scanning and
retirement of these objects use the same descriptor/mark/sweep engine as any
application payload. Runtime keeps semantic identity/generation validation,
publication policy, indexes and error mapping; GC stores no module semantics.

The concrete mapping and cleanup of lookup indexes remain GCR02 obligations.
Do not preserve external records by turning every row into a permanent root,
hide managed edges inside an unscanned Arc, scan arbitrary host Rust graphs,
or weaken pinned-reload behavior. If the object model fails a concrete required
scenario, bring that evidence to the existing plan before extending the API.

### Closed physical edge inspection

Scope::inspect_edges reuses registered begin/scan dispatch for read-only admission
walks over erased/dynamic providers. EdgeVisitor selects marking or inspection;
the marking path retains its existing publication kernel. Inspection validates
same-heap live targets, brands them for the current closed scope and calls a
read-only consumer. It neither sets mark bits nor adds roots. Only the inspector's
private, pointer-free Copy state advances; the collector's suspended cursor and
workspace remain untouched. Duplicate fields remain separate observations.

EdgeInspector owns reusable aligned scanner scratch, excluding any object handle,
root or result proof. Its capacity is external traversal memory and is observable
separately. Each scan gets the normal 1,024-unit allowance, enforces provider bounds/
progress and runs a caller checkpoint between turns, even without outgoing edges.
An indivisible provider can exceed the allowance; an admission consumer can still
poll during its edge visits. ControlFlow stops either consumer without a sentinel
provider error. Storage/provider failures remain GcError. Shared callbacks cannot
mutate or collect the scoped heap; callback panic unwinds as for Scope::read,
leaving no collector state change. A later inspection initializes fresh scratch.

This supports Runtime data validation without a second native tracing contract.
For example, an opaque payload may contain a mutable link to a Host descriptor:
Runtime enqueues each physical child and checks it after that provider scan. The
collector never interprets the Host descriptor or executable relationship. The
unsafe provider still owes complete edges and nonallocating/nonreentrant scanner
code; the inspection consumer may reserve fallible Rust traversal scratch, with no
managed allocation or mutation available through shared Scope.

Reference mechanism: [OpenJDK 21+35 iterator.hpp](https://github.com/openjdk/jdk/blob/jdk-21%2B35/src/hotspot/share/memory/iterator.hpp)
separates object-field iteration through OopClosure from GC-specific iteration
services. Kagari similarly changes the edge consumer rather than asking providers
for another tracer. Kagari uses invariant Rust scopes and pointer-free resumable
state under exclusive heap ownership, not JVM safepoints, concurrent visitation or
mutable oop addresses. No collector algorithm or Runtime type policy moves here,
and this comparison does not establish correctness or performance.

## Mutation and scanner coordination

### Stepwise iteration and mutable loans

The linked-list consumer uses Heap::scope and keeps its current Gc identity in
ordinary business iterator state. The core does not own path traversal.
values implements Iterator<Item = i64>, reading one checked scalar and following
one checked edge per step. values_mut captures the next Gc identity before
borrowing the current ScalarField, then returns Option<&'step mut i64>.
Neither path registers/clones a Root per step or calls a GC continuation method.

The host List owns one fixed-cardinality RootSet with a single head slot, allocated
by List::new(&mut Access). An empty list keeps this one heap-bound registration.
push_front constructs a node whose edge names the old head, then replaces slot zero
through Scope::set_root; insertion creates no independent Root or release notice.
pop_front retains its escaping result before changing the head slot, publishes
the successor into that slot, and clears the removed node's next edge. Root-limit
failure leaves the list unchanged, and a returned Root keeps only the popped node
alive after dropping the list. The private head reader treats Index as empty only
because construction fixes this pack's length to one; foreign-heap, state and
representation errors propagate. No collector API or tracing model changes.

The scalar loan holds only a Rust reference to the declared field; the old
ScalarMut wrapper and finish API are removed. Its exclusive Scope loan must end
before the next step. Shared/cyclic paths may
revisit objects without overlapping mutable references or a visited-node registry.
The iterator is lending, not Iterator<Item = &mut T>. References, container
structure and capacity are never exposed by a scalar projection. EdgeField owns
publication and fixed-position continuation; custom structural providers retain
cursor-repair/Restart and capacity-admission obligations.

ObjectMut and with_object_mut share one admission/accounting/finalization kernel.
A callback temporarily reborrows Mutation services for its own short lifetime;
admitted capacity/continuation results propagate back even if the callback panics.
No unsafe lifetime extension is used to work around guard Drop's exclusive loan.
finish reports finalization errors; ordinary Drop performs the same cleanup and
faults on failure. Scalar loans bypass this general mutation kernel and their
completed edits survive early exit and unwind without finalization.

Before exposing an owned mutable guard, mark this owner's active scan Restart and
remember whether its previous state was reusable. with_scan_state can lend that
saved state despite this guard's conservative fallback; it cannot lend a state
that already had a pending Restart. Successful proven continuation clears only
this guard's fallback. Errors/unwinds/explicit invalidation keep Restart. Thus
safe mem::forget cannot leave a stale scanner reusable. Safe wrappers cannot rely
on Drop to establish valid storage or account capacity changes. Capacity-changing
operations must finish before
exposing a safe result. This is not a safe general-purpose mutable object iterator.

The entire Scope lifetime excludes collection and same-heap reentry. To pause
traversal across such a boundary, retain the required target/root-pack state
first; no scoped Gc may escape and later resume after collection. Allocation
inside Scope uses the same reservation/initialization/publication kernel as rooted
allocation but skips root preparation/commit. Its result is protected by the
interval until retained, stored in a traced object/root slot, or left as garbage.
Both paths preserve rollback, charge and current-phase allocation admission.

### Reference publication and scan continuation

Every imported/newly exposed reference uses a protected source, publication and
a non-reentrant commit. Internal relocation additionally needs the continuation
coverage proof below or scanner invalidation. On Roots/Mark, publish shades white
targets Gray (or Black for proven
edge-free leaves) and reserves no new queue storage for admitted targets. In
Sweep, existing targets must already be Black; new initialized allocations are
admitted Black. Idle still checks heap/type/liveness admission before skipping
mark work.

The core receives a generic continuation decision, not a container operation:

```text
ScanChange = Continue | Restart
MutationCommit { owner: ObjectRef, change: ScanChange, charge_after: ObjectCharge }

after successful or unwind-finalized commit:
    install committed charge
    if change == Restart and heap.active_scan.owner == owner:
        heap.active_scan.restart = true
```

Continue means that the saved state still denotes a safe finite scan: no remaining
live reference can escape future visitation without already being protected by
publication. There are three sufficient proofs:

1. Unscanned original positions remain interpretable. Incoming targets are
   published, and removed/missing positions are safely charged/skipped.
2. The provider repairs its admitted typed state during the same closed structural
   commit, preserving the meaning of the remaining unvisited extent. Ordered Vec
   removal uses this protocol; generic GC does not interpret the cursor fields.
3. Before reordering, the provider republishes every moved target that could fall
   behind the cursor or outside its captured extent. On resume it reacquires the
   current payload/buffer, keeps no stale iterator/pointer, and safely skips old
   positions beyond the current extent. New imports are published separately.

Thus coordinate changes alone do not require Restart. Continue must be derived
from checked fixed-field operations, provider-owned cursor repair, covered-move
primitives or the unsafe provider's reviewed continuation contract, not a freely
asserted safe boolean.
Arbitrary edits without such a proof use Restart. On resume reset that owner's
scanner to a fresh finite extent and clear restart; previously shaded targets
remain conservative. This is a fallback for an invalid continuation, not a new
default rescan of operations with a valid cursor-repair or covered-move proof.
Continue never clears a pre-existing pending Restart. A failed/unwound repair
leaves Restart as the fallback, not a partially repaired reusable cursor.

Only the single active owner has a cursor to reset. Pending owners start fresh;
Black owners need no rescan of already-known edges, but all incoming targets
still pass publication. White owners are scanned fresh when discovered. This
uses the active identity, not a new per-object revision directory or a container
type test. Continuous invalidation may postpone completion; pressure assistance drains
the same finite scan without mutator interleaving. No fairness or pause bound is
inferred for adversarial providers. Scan/charge hooks cannot mutate or reenter.

Provider editing remains unsafe unless expressed through checked field/container
primitives. A mutation guard prepares resources before structural change and
finalizes charge/scan validity on normal/error/unwind exits. Publication failure
precedes a destination overwrite. Never use a post-edit rescan to discover a
reference that was unprotected during an intervening callback.

### Vec: left shift, append and reallocation

The existing [VecStorage scanner](https://github.com/kagari-lang/kagari/blob/0490f8a4071f11ffcb98679f0eb3dd67eb8c9854/crates/kagari-stdlib/src/collections/vector/storage.rs)
stores main/scratch positions and captured ends; it reacquires current slices for
each loan and retains no Vec iterator or buffer pointer. For a reference buffer:

```text
before: [A, B, C, D, E], next = 2, captured_end = 5
remove(0)
after:  [B, C, D, E],    next = 2
```

C moves from unvisited index 2 to already-passed index 1. Merely continuing misses
it. The selected new ObjectVec::remove repairs next to 1 and captured_end to 4
in the same commit as the removal. Resuming visits C/D/E, with no relocation
publication. A returned A still needs independent result retention before removal.
Deleting at next leaves next unchanged and shrinks captured_end; deleting before
next shrinks both; deleting inside the remaining extent shrinks only captured_end;
deleting at or beyond captured_end shrinks neither. This also covers tail removal,
an empty remaining extent, and repeated removals interleaved with scan steps.

The current production Vec implementation instead republishes the traced suffix
before removing; it is a valid prior strategy, not the selected new removal API.
The new design changes no production code here. Insert still uses covered suffix
publication, including an old final element that may move beyond captured_end;
its cursor remains bounds-valid as length grows. Arbitrary reorderings require
their own repair/coverage proof or Restart. Do not treat removal's two decrements
as a general insertion, permutation or multi-buffer repair algorithm.

Append publishes its new target before commit. It need not extend the old cursor
end to rediscover that target. Buffer reallocation alone preserves logical
positions, so reacquiring the current slice makes continuation safe without
restarting. Mid-array removal shifts edges and uses constant cursor maintenance;
insertion still includes covered-publication work. No measured speed claim follows
from either protocol. Scalar buffers have no managed edges.

### Map: reference slots separate from bucket indexes

Current [HashTable](https://github.com/kagari-lang/kagari/blob/0490f8a4071f11ffcb98679f0eb3dd67eb8c9854/crates/kagari-stdlib/src/collections/hash.rs) already uses:

```text
slots:   Vec<Entry { key: StoredValue, value: StoredValue, ...scalar links... }>
buckets: HashMap<i64, usize>      // script hash -> scalar slot position
```

The scanner walks slots' key/value fields, not the Rust HashMap's buckets. An
index rehash changes only scalar positions inside buckets and leaves the scanned
reference coordinates intact. New entries append to slots after key/value
publication. Replacing a value publishes the new target before slot replacement.

Dense removal uses swap_remove. With slots [a:A, b:B, c:C, d:D] and a cursor past
b:B, deleting a:A moves d:D to slot zero. The existing remove republishes both
the moved key d and value D before relocation, then repairs scalar bucket/link
positions. Continue is safe even though the cursor will not revisit slot zero.
Retain any returned value before removing its old protection.

An arbitrary std HashMap containing references needs its own sound scanner/move
protocol. The public iterator cannot be retained across mutation, and restarting
iter().skip(n) does not supply a stable physical-bucket cursor. Do not smuggle
private bucket knowledge into the generic GC, or replace the existing indexed
storage to solve that unrelated problem. The core only calls the registered
scanner and consumes Continue/Restart; it has no Vec/HashMap branches.

## Operation algorithms

### Script operations and collector actions

This is the object-only target model, not a claim that Runtime already uses
kagari-gc. Runtime evaluates expressions and enforces declared access, concrete
types, checked arithmetic and trap order. Providers implement container behavior.
The core consumes protected object references, publication, root-slot updates,
scan coordination and accounting; it does not interpret script field names or
implement Vec/HashMap algorithms.

| Script operation, schematic | Runtime/provider action | Collector action |
| --- | --- | --- |
| player.hp = 42 | Validate the receiver/field and store scalar bits | No reference publication; retain valid physical access evidence |
| a = b, for object values | Publish and replace the destination frame's value/tag in one prepared commit; the frame is directly scanned | Mark the incoming target when required; no mirrored root slot or per-assignment host Root lease |
| player.target = enemy | Prepare a checked reference-field write with protected operands | Publish enemy before overwriting the stored edge; fixed-position continuation remains valid |
| player.target = None | Clear the optional reference using its checked encoding | Remove the edge; no new target to publish and no immediate recursive freeing |
| vec.push(enemy) | Prepare capacity and append under the container contract | Publish the incoming edge, account for length/capacity, preserve/prove scan continuation |
| vec.remove(i) | Protect the result, remove and repair the admitted Vec cursor in one closed commit | Retain the result in its destination, preserve next/end or a pending Restart, update charge; no suffix publication |
| map[key] = value | Perform user hash/equality work outside conflicting loans, then revalidate the destination and commit | Publish managed key/value targets and coordinate the provider's scan/storage changes |
| construct an object | Reserve backing and result protection, initialize complete storage | Publish outgoing edges, admit the current-phase mark and commit a rooted result |
| return, clear a slot, leave a frame, trap or cancel | Transfer live results first, then clean the appropriate frame/root slots | Remove obsolete roots; remaining graph reachability determines later reclamation |

Scalar-only containers still have capacity/accounting work even though their
elements need no reference barrier. Removing a reference does not prove its target
dead: other fields, execution frames or host roots may retain it. A target already
marked during an incremental cycle can conservatively survive that cycle after
its final edge is removed. No prompt destructor or collection-on-every-write
semantics are introduced.

A script result uses an admitted directly scanned frame destination, while an owning
host result uses a Root lease. Both use the same retention/publication owner and
must protect the result before its original source is removed or a callback can
run. The algorithms below name both destinations; concrete integration signatures
and frame publication/commit proofs remain GCR00/GCR02 obligations, not a second
root registry or a second allocation/removal semantic implementation.

### Allocate and publish

```text
1. Validate heap/phase and descriptor; retain descriptor registration.
2. Prepare result protection: reserve a Root slot/release credit for an owning
   result, or validate/reserve a directly scanned frame destination for script.
   Reserve page/large backing, work capacity, initialization scratch and accounting
   bounds. Create a linear InitTicket.
3. Initialize only the reserved slot. Import every edge from a live root/loan.
   No collection/reentry occurs. Custom erased providers must track partially
   initialized fields. The implemented fixed dynamic records initialize every
   scalar to zero and every reference to None before publication.
4. Complete the payload/header, inspect all outgoing edges through its scanner,
   and publish their targets. Failure: destroy initialized state, roll back the
   ticket, and expose no object/root. Conservative shading may remain.
5. In a closed no-fail suffix, admit the object's mark/credits, commit occupancy,
   install the prepared result protection and committed charge, then return the
   owning Root or the protected script result through its original caller.
```

Result-protection/capacity failure must happen before step 5. A queued new node cannot
be observed before occupancy commits because the exclusive access does not yield.
Temporary reservation ownership is separate from initialized GC-visible storage.
Explicit try_reserve/Layout failures return errors; ordinary Arc or allocator
abort behavior still follows Rust/platform allocation semantics. Do not promise
recoverable OOM for every allocation without an actual fallible allocator API.

### Assign a field or replace a root

```text
1. Runtime checks declared access and concrete type, preserving trap order.
2. Access resolves live receiver, descriptor and checked field selector.
3. Resolve and publish the protected incoming target; reserve commit needs.
4. Commit the physical field/root slot once, update charge if affected, report
   ScanChange. No callback/collection can separate publication and commit.
```

Scalar fields use their prepared encoding/offset and no reference barrier.
Language typing and readonly rules are not implemented again in the core. Runtime
must preserve its existing value-admission/receiver-error precedence when it
prepares this operation; the physical API does not choose script trap ordering.

### Remove and return an element

```text
1. Locate the element under its traced source's loan.
2. Prepare result protection while the element is still traced: install an owning
   Root for host escape, or validate/stage the directly scanned frame destination
   for script transport without changing that destination. Publish the target.
3. Prepare commit/accounting resources and all fallible checks before destructive
   changes. Admit receiver-bound scan-state access for ordered Vec removal; other
   representations may require covered relocation publication or Restart. The
   source loan still protects the result.
4. In the closed no-fail suffix, install any staged script result protection
   before removing its source, commit removal and any provider-owned cursor repair,
   finalize charge, and report Continue or Restart without clearing an earlier
   pending Restart. Release the payload loan after completing the paired commit.
5. Return the independent Root or rooted script value; callbacks can now run.
```

If preparation fails, the source and original script destination remain intact;
an unused temporary result Root is released normally. If an algorithm removes
several entries before a callback,
all removed live values must reside in rooted or traced scratch first. A Rust Vec
of unrooted copied Edge values is insufficient scratch storage.

## Cycle state machine and sweep

| Phase | Work | Legal mutator interval | Completion/fault rule |
| --- | --- | --- | --- |
| Idle | Decide whether to start; capture root cursors and checked pressure | Ordinary checked allocation/mutation | Setup failure exposes no half-cycle |
| Roots | Scan host root packs and the driver's execution root source in finite slices | Publish every new/replaced root and edge | Both root cursors Complete for this epoch before Mark closure |
| Mark | Resolve gray blocks, resume the single active owner | Publication and structural coordination stay active | No active owner/pending gray job before closing |
| Sweep | Retire heap objects and released root packs | Access survivors/new objects only; no resurrection | Finish retirement and disposal before Idle |
| Faulted | Preserve diagnostics/cleanup, reject new publication and ordinary service | No ordinary script continuation | Never sweep a partial mark result |
| Closed | Physical objects disposed and root release disconnected | No further access | External handles remain inert |

```text
driver.step(budget):
    admit legal heap access and current fault state
    if Idle: reserve setup work, increment checked epoch, capture finite roots
    Roots: advance host-table and execution-source cursors under the shared budget
    Mark: process queued gray owners until Pending or marking closure
    before Sweep: reserve transition work and validate heap retirement state
    Sweep: advance one retirement stage under remaining budget
    release every loan/guard; return invocation-local report and progress
```

Object allocation and reference publication may occur between slices. Newly
admitted marks use current-phase colors; existing white nodes cannot be rescued
after Sweep begins. Physical cursor unlink repair and root-slot generation
checks handle backing/root-table changes; the execution source preserves logical
cursor identity under its own generation/relocation contract. Original page marks
remain authoritative. A work queue is traversal state, not an extra external root.

Full collect finishes an active cycle and then runs a fresh cycle over current
roots using the same engine. Assistance drains the active cycle without normal
service yields. Neither path changes provider limits, cleanup or retirement caps.
No new cycle begins while an old detached batch remains undisposed.
Public step times its complete service/disposal interval. Full collect calls the
same untimed service core and records one invocation-wide duration, avoiding a
clock read for every 32-owner batch. No scheduling or work cap changes.


### Retirement structures and ordering

The implemented batch in [sweep](../crates/kagari-gc/src/sweep.rs) uses individual
linear owners and shared pins; there is no separate range/page destruction path.

```text
SweepStage = Physical | Roots { remaining } | Descriptors { remaining }
RetirementBatch {
    owners: [Option<Owner>; 32],     // declared before pins: destroyed first
    pins: [Option<Pin>; 32],
    len: usize,
    pin_count: usize,
}
Owner = Object(Retired) | Roots(RootEntry)
Pin { descriptor: Arc<Descriptor>, backing: Arc<Backing>, page: u32 }
Retired {
    reservation: Reservation,
    descriptor: NonNull<Descriptor>, // stable Arc pointee, pinned by this batch
    pointer: NonNull<u8>,
    initialized: bool,
}
```

Sweep passes its known page/slot to object_at, which checks occupancy before
reading initialized header/descriptor/payload information. It does not reconstruct
a slot by dividing an object address. Ordinary source admission retains the full
live-address membership checks. Consecutive retired objects sharing page and
exact descriptor reuse the most recent pin pair; mixed runs get separate pins.
This costs constant work per owner and allocates no batch metadata. The cap is
32 total objects and released root packs, irrespective of sharing.

Before each detach, validate charge against the ledger and prepare any needed
pins. Detach consumes storage ownership and decrements the object count; charge
is then removed from the ledger. No caller can extract a Retired token from the
batch. Dispose outside storage loans, consume initialized before each destructor,
catch its panic, fault before the next destructor, and finish slot/use-credit
release. Pins outlive both normal disposal and remaining owners dropped during
an early error/unwind. A preparation failure after partial detach faults the heap;
those already detached owners are disposed, not rolled back into initialized
storage. Shutdown handles remaining occupied objects, then releases metadata.

An EdgeFree representation can own resources or indirect capacity: charge and
destroy dispatch remain. No trivial-destructor specialization is inferred from
tracing kind. Supported Rust unwind is contained; process abort or double panic
inside a destructor is not recoverable. Destructors cannot access managed peers
or reenter the heap. No next batch starts before the current batch finishes.

## Work, pressure and validation ownership

Keep the [existing budget definitions](https://github.com/kagari-lang/kagari/blob/0490f8a4071f11ffcb98679f0eb3dd67eb8c9854/crates/kagari-runtime/src/native/storage/scan.rs)
as the first implementation: normal allowance 1024; a provider unit reserving one
edge plus four bitmap-reset words costs at most five; bounded providers advertise
at most 204 units. Step accepts allowances of at least eight units. If an active
bounded scanner needs more than the requested allowance, StepReport.required_budget
reports max(8, max_work * 5). The caller must retry with that allowance to advance
this scanner; repeated smaller steps can report zero work without faulting the
heap. A call with enough total allowance but too little remaining work yields and
can progress on the next call with the same allowance. Normal collect uses 1024,
which accommodates every admitted bounded scanner. Indivisible providers retain
their explicit excess-work contract.
Charge inspected holes, live slots and root/retirement work too.
Prepaid visitor work and marker work must have one charge owner, not both charge
the same reservation. All charge/reservation operations are checked before the
cursor advances. Actual indivisible excess is reported without subtracting it
from pauses. Reuse current stage precharges; extraction is not permission to
increase allowances to pass timing gates.

MemoryLedger records committed provider indirect capacity, managed peak, and
cycle-local committed allocation growth; PhysicalHeap owns backing bytes.
Descriptor/control capacity remains outside the reported managed-byte ledger. Runtime combines core
figures with non-GC Runtime resource pressure and enforces its own quotas. Reuse
grow-before-commit and unwind-finalized charges. Count physical backing once,
including retiring extent, and release it after final disposal. Reports do
not equate requested bytes with allocator usable bytes or process RSS.

First-version accounting decision: generic T has no separate committed-charge
record in its eight-byte header. Sample its registered infallible charge hook
under the live source loan before detach, validate it against the global ledger,
then subtract exactly once. A faulty hook faults before that object's detach;
shutdown skips charge hooks and disposes valid owned storage. Mutation snapshots
old charge and finalizes the new charge on success/error/unwind. Growth calls
prepare_charge with staged actual capacity before installation. This avoids a
per-object accounting directory or an extra payload wrapper. The safety contract
requires stable, pure charge outside checked edits; full requested-byte accounting
for Arc/table/control overhead remains diagnostic work, not HeapStats' managed peak.

| Validation | Owner and release behavior |
| --- | --- |
| Root heap/generation/lease identity | Public operation boundary; retained in release |
| Object liveness before direct dereference | Source/root/loan construction; no expired-address probing |
| Descriptor/type/offset/alignment | Registration plus exact access proof; unsafe memory validity remains required |
| Incremental active owner or root-slot replacement | Resume boundary; dynamic behavior, not a debug assertion |
| Kagari declared access/concrete type/host scope | Runtime semantic/host boundary, shared across callers |
| Proven loop-local offset/class facts | Prepared capability; avoid repeated admission within its lifetime |
| Derived counters guaranteed by construction | Internal assertions; debug-only only when release safety does not depend on them |

The implementation must manually review any unchecked optimization against the
capability that proves it. Do not remove checks simply because their callers are
internal, or add full public validation to every prepared scalar operation.

### Idle metadata maintenance and observations

Heap/RootDriver::metadata_stats walks allocated directory blocks and reports their
entry count, slot capacity and requested bytes separately from object backing.
It also reports active-vector, release-queue, mark-queue and scan-workspace capacity;
TypeId HashMap capacity is in entries, not implementation-specific byte estimates.
This diagnostic locks the release port and is not a constant-time hot-path counter.
Individual Arc allocations, root-pack targets, descriptor-owned layouts, external
frame storage, allocator overhead and RSS are excluded. HeapStats managed-byte
accounting and admission quotas retain their existing meanings.

Empty table blocks release during ordinary retirement. Large auxiliary buffer
rebuilds run through Heap/RootDriver::trim_metadata only in Idle, with no active
scan. It returns None during an active cycle and errors on Faulted/Closed. It
neither traces roots nor invokes managed providers. collect calls it after draining;
a safepoint that finishes a cycle also calls it. CollectionReport and AutomaticReport
include maintenance_elapsed within elapsed, outside collector work units. Explicit
step does not run this copying suffix; step-only drivers schedule trim_metadata
at their chosen idle boundary. Safepoint maintenance, even after AutomaticKind::Step,
has no work/time bound and may briefly block concurrent final-root notifications.

Vectors, queues and the type map shrink only when capacity exceeds twice
max(2 * needed, 16), retaining
max(2 * needed, 16) slots; zero need releases the buffer. Active vectors and the
TypeId map use current populations. ReleasePort uses all outstanding credits under
its mutex, including live leases and queued notices; final Root Drop stays allocation
free. Marking reserves from current pages and may shrink only with an empty idle
queue. Workspace is sized/aligned for all still-registered descriptors, including
externally pinned layouts without objects. Workspace replacement uses the size hysteresis above or reduced alignment,
and allocates the exact current size/alignment requirement. Shrinking discards
inactive Copy state; a replacement allocation failure preserves the valid larger
workspace. With no
descriptors it releases completely. Shutdown also releases the pending mark buffer.

### Automatic service at driver safepoints

[heap/pacing](../crates/kagari-gc/src/heap/pacing.rs) implements an opt-in scheduling
policy over the same service engine. Heap::safepoint and RootDriver::safepoint
require exclusive driver access outside Access/Scope; the latter always supplies
the complete execution root source. They return None when sleeping, otherwise an
AutomaticReport identifying Step or Assist, actual work/retirements/cycles and
elapsed service/disposal time. The embedding chooses when to call; there is no
background thread, hidden collection inside allocation, or initializer retry.

```text
HeapConfig { ..., pacing: PacingConfig }
PacingConfig {
    min_growth_bytes: 16384,
    growth_percent: 50,
    assist_multiplier: 1,
}
MemoryLedger { indirect, peak, allocation_growth }
Pacing { config, threshold }
```

The growth counter adds successfully committed shallow object layout bytes and
positive indirect-capacity changes. Reusing a slab slot still counts; failed
admission/initialization adds nothing. Capacity changes committed before a provider
returns an error still count through normal finalization. The counter saturates
on overflow because it is an advisory pacing observation, not quota accounting.
It resets on cycle start, including explicit step/collect. After each completed
cycle, threshold becomes max(min_growth_bytes, managed_bytes * growth_percent / 100),
where managed_bytes is backing plus indirect capacity. Config values must be
nonzero. No second authoritative memory accounting table is introduced.

An idle heap starts when growth reaches threshold, at least 32 root-release
notifications are pending, or pressure justifies service. An active cycle always
advances, including after allocation stops. Ordinary wakeup performs one service
call with allowance 1024 and cap 32 detached owners; declared indivisible scans
can still exceed the work allowance. During an active cycle, growth reaching
threshold * assist_multiplier requests a current-cycle drain. Idle bursts use at
least two threshold multiples, so ordinary wakeup remains incremental. Reaching
75% of any finite configured backing/indirect/object/root limit also requests an
assist when there is new growth, an active cycle or pending releases. Stable idle
live data alone does not cause repeated pressure collections.

An assist starts or finishes exactly one cycle via the same bounded chunks. It
does not run the extra fresh cycle required by explicit collect's contract.
Collected-after-start allocations may remain as floating garbage until a later
cycle. Assists have no wall-clock bound and cannot be interrupted by the caller
inside the safepoint; hosts needing that control use explicit step. Admission
limits stay hard checks, and a scope can exhaust them before its next safepoint.
No guarantee is made that a subsequent allocation will fit. Thresholds are
provisional controls, not a bound on memory for every graph or safepoint cadence.

The cycle-relative threshold and allocation-driven scheduling follow the same
broad ownership separation as gc-arena's allocation debt and Lua's incremental
steps; Kagari's hard normal-work allowance, detached-owner cap and generic
resumable providers remain distinct. Source/version, rejected initial thresholds,
long-ring tests and measured throughput/tails are recorded in the active plan.

Mark publication first validates live-source page membership/occupancy; only a
newly discovered object requires descriptor scan-kind lookup. Payload access
continues to resolve its descriptor. Starting an object scan shares its checked
location with the first scanner invocation, while resumed scans still reacquire
and validate their owner. Sweep uses existing occupancy/color words to skip live
or empty intervals (at most two words per page), charging every logical slot
before moving the cursor. Dense dead runs keep direct per-object retirement;
there is no extra cached bitmap or weakened scan/retirement budget.

## Runtime integration map

```text
Runtime {
    memory_and_roots: checked owner of RootDriver<ExecutionRoots>,
    execution: execution/session/depth/termination state,
    // ExecutionRoots owns compact ExecutionValues and execution-dependency roots
    modules: semantic lookup/version policy + genuine publication roots,
    host: host registry and borrow state,
    ...existing scheduler/native entry owners...
}

Managed module/environment/static representations: ordinary GcObject implementations
Internal relationships: ordinary Edge carriers
Published installations and independently retained host handles: Root/RootSet
Active/parked computations: direct execution root source
```

RR03's execution source issues the canonical execution identity used by Runtime's
control directory. Its typed internal-node dependencies are scanned Edge carriers,
separate from tagged language values; internal code/environment/application nodes
do not acquire a language Value tag or a Host Root merely for execution transport.
Native suffix retirement clears its local values and typed dependencies together.
The active plan distinguishes this implemented source protocol from the remaining
non-building Runtime frame/provider consumers.

Runtime's object operations borrow disjoint execution/semantic/memory fields;
Heap/Access stores no persistent Runtime borrow. An interpreter region admits
frame banks and memory access, publishes incoming references and updates the authoritative
frame slots in one prepared commit, then releases access before step or a user
callback. The driver lends a complete execution root source during each scan
slice, alongside the GC's host root table. Real frame commits, return rooting,
reentry and unwind behavior remain required Runtime tests.

Managed state carries all outgoing managed references in traced fields. A module
lookup ID remains a Runtime semantic identity and must resolve to a valid managed
handle before access; placing an unchecked ID in a payload is not sufficient to
retain its target. Lookup indexes must not root all old versions permanently.
GCR02 must choose their validity/cleanup contract using existing semantics and
prove a function/module/static cycle dies after genuine roots are released.
Immutable edge-free code/resources may remain Rust-owned backing held by managed
state; their destruction cannot traverse removed peers or reenter collection.

Runtime explicitly releases publication/frame/host roots in its shutdown order.
Heap::Drop must also be safe when a standalone caller omits shutdown: disconnect
roots, permanently close admission, dispose owned allocations without tracing or
dereferencing peers, and release backing. Safe callers need no external adapter
to make heap teardown memory-safe; Root drops after teardown are inert.

| Current source | Implementation destination |
| --- | --- |
| gc/directory/pages, object_header, backing | allocation/descriptor modules; retain geometry, bitmap and cursor contracts |
| gc/graph/marks and work/phase portions | marking for physical objects only; remove metadata identity branches |
| gc/roots table/release portions | one roots protocol with optional ObjectRef targets |
| gc/collection lifecycle/physical retirement | heap/sweep orchestration; no caller-store stages |
| frame/values and session stores | execution-owned authoritative banks; a generic root adapter enumerates managed targets without mirrored RootSet packs |
| gc/fields admission, values/array/future/task semantics | Runtime object/execution owners; retain common storage kernels |
| module and execution_metadata scanning/sweep | replace reachability-bearing records with ordinary GcObject implementations; retain Runtime lookup/version checks |
| native/storage scan/charge and StoredValue | general GcObject/provider integration; declaration/call evidence remains Native-owned |

Rehome methods by responsibility, not by copying whole old files. Remove the old
production collector/forwarding models at cutover. Do not change static typing,
once-only effects, trap ordering, rooted return transfer, cancellation or reload
selection to make the new API fit. Existing type/code retention debt and historical
performance gates remain at their original review/plan owners.
