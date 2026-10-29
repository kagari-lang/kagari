# Kagari Runtime Model

This document specifies the runtime model for Kagari.

The goal is to unify the runtime-facing concepts that appear across the syntax, trait, reflection, security, and host-interop documents.

Execution-model rules are defined in [execution.md](execution.md).
Backend abstraction rules are defined in [codegen-backend.md](codegen-backend.md).
Typed path mutation rules are defined in [typed-path-mutation.md](typed-path-mutation.md).

## Design Goals

- define a coherent runtime object model for script-owned and host-owned values
- support GC-managed script values and frame-scoped host borrows in one runtime
- share runtime type identity across reflection, interface values, and downcast
- support installed host interfaces, cooperative cancellation and lifecycle accounting
- support hot reload without binding the runtime directly to AST details

## Implementation Status

The current runtime crate defines the main system boundaries:

- `crates/kagari-runtime/src/lib.rs`
- `crates/kagari-runtime/src/value.rs`
- `crates/kagari-runtime/src/gc.rs`
- `crates/kagari-runtime/src/host.rs`
- `crates/kagari-runtime/src/reload.rs`
- `crates/kagari-runtime/src/backend.rs`

This specification uses those boundaries.

## Top-Level Runtime Structure

The runtime structure is:

```text
Runtime {
  gc: GcHeap,
  types: TypeRegistry,
  host: HostRegistry,
  limits: RuntimeLimits,
  epochs: ModuleEpochAllocator,
  modules: ModuleStore
}
```

The exact field layout is implementation-defined, but these responsibilities remain distinct.

## Core Runtime Subsystems

The runtime is organized around these subsystems:

- GC heap for script-owned objects
- type registry for runtime type identity and metadata
- host registry for exposed functions and types
- runtime call-depth limits and root cancellation
- reload coordinator for module epochs
- module store for loaded code units and runtime module state

## Value Model

Kagari values are split into two broad categories:

1. storable script values
2. frame-scoped ephemeral values

This distinction is important for host interop and safety.

### Storable Script Values

Storable values are valid in:

- locals
- GC object fields
- globals
- closure environments
- return values

These include:

- primitive scalars
- GC handles
- script-owned aggregate values
- host roots or host path views, if the embedding permits them as handle values
- interface values, if represented as storable heap values

Executable closures are GC objects containing a verified function slot and
ordered captures. Immutable bindings capture their ordinary values; mutable
bindings captured by a closure use GC cells so outer and inner assignments share
the same storage. The object retains the execution version that created it.
Borrowed host handles and path views are not valid closure captures.

This category describes runtime values in general, not `const` item eligibility.
In the current module model, `const` items are compile-time by-value scalars only and do not materialize frozen GC-backed objects.

### Ephemeral Values

Ephemeral values are runtime values that must not escape a frame boundary.

Examples:

- borrowed host references
- certain temporary VM handles
- future non-suspendable runtime resources

The key property is:

- ephemeral values are not legal heap payloads

## Machine and resource boundaries

Script integers have fixed widths independent of the host address width. Standard
lengths, capacities, indices and logical counts use `u64`. A value such as
`4294967296u64` is valid on a 32-bit host: storing it in a one-element array needs
one 64-bit element, not that many elements. Iterator take/skip counts remain wide
logical values rather than machine storage positions.

Rust `usize` still describes physical storage lengths, positions and allocation.
Crossing from a script integer into storage requires a checked conversion or a
bounds proof in the wide domain before conversion. Invalid indices must not
truncate or wrap into valid positions. Error policy belongs to the operation:
indexing traps, optional lookups return absence where specified, and string slices
also validate UTF-8 boundaries. Preserve once-only, left-to-right evaluation and
completed side effects on failure.

A `u64` length does not promise that every representable size can be allocated.
Capacity depends on address width, element size, allocator/runtime limits and
available memory. Allocation checks length and byte-size arithmetic without
wraparound. Ordinary checked failures retain their specified error and mutation
contracts; underlying out-of-memory failure may be fatal and is not promised
recoverable. Equal source and inputs have the same specified numeric behavior
within platform resources, not identical memory limits. There are no per-script
memory quotas. [Host scalar conversions](host-interop.md#fixed-width-numeric-host-boundary)
are distinct from zero-copy element storage.

## Value Shape

Enum objects contain an `EnumTag` and immutable payload slots. Declared variants
hold an `EnumVariantRef` from a verified module; standard Option/Result have typed
tags. Names are display metadata. Allocation checks runtime ownership, payload
arity, storage/representation and nested payload type/schema consistency before
publishing the allocation. Declared payload structs/enums must match the receiving
version's nominal layout. Standard-enum built-ins reject same-named declared types.
Enum equality compares tags/layouts and then members using script equality, so
mutable members compare by identity. Enum objects retain immutable version metadata
and trace their payloads through the same mark-sweep root mechanism. Publication
does not invalidate old values; rooted old enum objects keep their original layout.

Script struct objects share a closed layout derived from `StructLayoutRef` and
store positional field bytes inline in nonmoving slab slots or large allocations.
Scalars use their exact semantic widths, preserving float bits; Unit needs no bytes. Managed fields retain
a tag and full owner/slot/generation identity. Layout-directed tracing visits only
reference fields. Safe byte codecs also serve Value-based host/reflection access.
The layout comes from a verified `ExecutableModule` and retains immutable version
metadata. Objects do not duplicate field names. Allocation requires
a layout belonging to the receiving runtime and validates field count, payload
storage boundaries and value representations before publishing the allocation.
GcHeapConfig configures collection scheduling; successful mutations update live
occupancy used by the collector. Checked capacity and allocation failures preserve
the target. There are no heap or cumulative-allocation quotas. See the
[failure contract](failure-semantics.md#modification-guarantees) for commit order.
Field reads require a matching nominal layout; writes additionally require a
writable slot and matching value representation before changing the target.

Layouts from the same executable generation compare by shared handle and index.
Across generations, access requires the same runtime owner and equal nominal
declaration, field identities, slot order, representations and permissions.
Publication does not invalidate an object's retained layout. Root calls now pin
their executable dependency program. Heap-valued fields still have coarse nominal
representations, while heap references carry a unique heap owner, slot and generation.
Reachable interfaces and closures retain executable module instances through graph
edges. Ordinary data layouts retain immutable type metadata without retaining the
generation's mutable module state. Selected generic operations, parent scopes and
cached method applications contribute executable edges through their environments.

Explicit reflection resolves a field name through the retained layout metadata
and uses the same slot write checks. Reflection cannot bypass read-only fields
or scalar representation checks. Named `StructValueField` records exist only in
diagnostic snapshots, not heap storage or allocation APIs.

Internal values are compact tagged copies; owned payloads live outside the value
slot. The implementation uses 16-byte `Value` in release and debug builds. Builtin
objects carry nonowning direct references to stable allocation headers; Native/Array
aliases temporarily retain 12-byte checked identities containing owner, slot and
allocation generation until the extraction protocol supports traced tombstones.
Copying a value does not retain its referent or allocate/copy payload bytes.
The implemented categories are:

```text
Value {
  Unit,
  Bool(bool),
  Char(char),
  I32(i32),
  I64(i64),
  U64(u64),
  F32(f32),
  F64(f64),
  Str/Tuple/Range/Enum/Struct/Closure/Cell(GcRef),
  Array/GcHandle(HeapObjectId), // Native-backed collections use these tags.
  Interface(InterfaceObjectId), // GcRef to a wrapper tracing checked metadata.
  HostRoot/HostPathView/Ephemeral(GcRef),
  RuntimeEphemeral(EphemeralValueId)
}
```

Direct references require a live frame, root or traced owner through every runtime
query, including validation. They cannot be queried after reclamation. Host code
retains `RootedValue` or typed handles; cloning retention does not copy script objects.
Packed managed fields preserve pointer provenance using typed unaligned access.
Their current 13-byte tagged extent accommodates both direct references and Native
checked identities; the Native migration owns removal of that intermediate layout.

Strings and tuple membership are immutable. Updating a tuple creates replacement
membership, sharing referenced children; mutation of an aliased mutable child
remains visible through all aliases. String equality/hashing uses contents,
independently of storage identity. Raw Rust Value equality describes transport
identity and must not replace language equality. Host root/path/borrow descriptors
are heap-owned records, while Rust host objects and borrow authority stay with the
host registry and active scope. The descriptor's continued existence never extends
a host borrow past its validated scope.

Module constants use portable checked indices. Materialized strings belong to a
runtime-local module-version cache, participate in GC tracing, and are shared by
repeated loads. Sharing verified code between runtimes does not share heap values.
An escaped string needs an ordinary root and does not retain obsolete executable
state. Internal string access borrows a scoped view, released before script-heap
allocation, collection or callback boundaries; owned host String conversion copies
the bytes.

The representation must preserve:

- clear separation between GC-managed and non-GC-managed values
- a representation for interface values
- a representation for host roots and host path views that are handles, not Rust references
- a representation for frame-scoped host borrows

Kagari does not require a runtime notion of "read-only heap object" just to support `const`.
Future shared frozen objects must be modeled explicitly rather than folded into ordinary `const` items.

## GC Heap

The GC heap is responsible for script-owned memory.

Its responsibilities include:

- object allocation
- object tracing and reclamation
- heap accounting
- integration with runtime resource limits

The GC must not own Rust host borrows.

The implemented collector uses stop-the-world mark-sweep over stable checked
handles. A compact generation directory locates nonmoving slab slots and separately
tracked large allocations. Small backing uses independent 1 KiB slabs aligned to
16 bytes, shared by size class and edge capability rather than nominal type.
Combined layouts above 1024 bytes or aligned above 16 use exact-layout large
allocations. These are not subdivisions of a retained 4 KiB block. Native headers
and actual Rust payloads are colocated; struct field bytes are inline, not stored
in a compacting global arena. No permanent per-native-payload Box remains, although
admission/factory transport may stage one before placement. Scoped storage borrows
still exclude collection and reentry. It traverses
tuples, enum payloads, array/map values and struct fields with an explicit work stack,
handles cycles, and reclaims unreachable slots. Reuse increments the slot generation;
a saturated generation retires its slot. Reads, writes, root registration and script
equality reject foreign, stale or incorrectly tagged references. Failed handle
validation cannot consume heap growth units or change the target object.

Native mutation revisions belong to runtime-owned native records, separately from
the common identity generation/locator. New identities begin with a fresh revision;
ordinary collection leaves its address unchanged and payload replacement keeps
the receiver's state.
Structural edits, checked native writes and same-payload library recovery retain
their operation-specific invalidation/commit order. Exhaustion rejects the operation
without wrapping. An operation guard excludes conflicting slot access, not the
scanner; payloads remain traced while callbacks may collect.

An interface heap identity directly locates a runtime-owned snapshot. Heap aliases
have independent checked identities even when they share one metadata snapshot.
Metadata roots may keep the snapshot and receiver alive after heap aliases expire;
later publication creates a fresh heap identity. Complete metadata owner/generation
checks remain at token admission and metadata access. The compact directory locator
requires a representable snapshot slot; publication fails before heap growth or
identity publication when that bound is exceeded. Collection validates the complete
graph and accounts dead wrapper units before invalidating heap IDs and detaching
metadata. Cross-store borrow conflicts are preflighted before the first irreversible
detach. Later physical preparation failures obey the batch-atomic contract below,
not whole-cycle rollback.

The phase/epoch marker walks checked identities through storage-provided edges.
Object representations own reference traversal; closure signatures and runtime type
compatibility have separate execution owners. Heap objects and executable programs
share one graph driver. Its pending jobs name 128-slot blocks in original identity
stores, with discovered/scanned bitmaps, or singleton static identities. They are
not payload slab addresses. Each block has at most one queued job, while one owned
cursor scans an individual identity; generations remain checked by the original
store. Work-queue credits and admitted-identity credits have separate bounds. Published and staged programs, active-call/code leases, host roots and
heap root records seed traversal. Runtime-owned execution windows independently
seed their values and program/environment metadata, including suspended callers.
Prepared-method and selected-native-call metadata is stored alongside persistent
roots; public leases never own that storage.
A reached program traces its dependency members' bound static cells and
materialized constants; cells trace their Ready values or pending initializer
recipes. A reached closure or interface traces its executable owner and environment.
Metadata traversal uses an explicit worklist, visits shared descriptors once and
excludes pure layout provenance from executable state retention. Program edges
validate the installed generation and runtime identity. An obsolete
program/cell/closure cycle, or mutually referring obsolete programs, does not by
itself keep either side alive.

Publication validates the original owner's full identity and value representation
before consulting authoritative graph phase, even while Idle. Graph-only marks are
constructed only when needed; this is not an exemption from ordinary managed-write
validation. The generic managed tag accepts occupied managed kinds; the specialized
Array tag additionally checks the native Array subtype. Reads do not create retention.
Explicit Faulted state permits diagnostic graph reads but rejects publication/new
admission. Poisoned state instead rejects graph access, publication and work acquisition:
a new unwind across graph-guard work publishes this terminal state before unlock.
A guard acquired and successfully released wholly during an existing unwind can perform
mandatory recovery without creating poison; it never clears preexisting Poisoned state.
The guard records entry panic state rather than treating all cleanup during unwind as
interrupted work. Pure identity inspection is not graph access; production collection
faults also quarantine execution.

Operation groups live in a runtime-owned generational store. They contain receiver
tables and individual concrete selections. Environments carry an OperationId for a
selected witness: a group identity plus a checked member ordinal. Forwarding reuses
that ID; it does not clone ownership of the descriptor. Method metadata roots use
the same identity. Reads validate runtime ownership, slot generation and member
bounds. Allocation does not make a group a root. Tracing a selected operation also
reaches its supplying group without exposing sibling operations as caller-selected
witnesses. Associated type facts remain separately available for layout resolution.
Executable metadata is validated on publication and frame entry. A detached snapshot
cannot republish an environment after its operation records have been collected,
even when the supplying code is still installed; an explicit root preserves those
records for use across collection.

Operation groups own their BoundOperation records by value. A rooted operation
selection holds its checked ID, not shared ownership of the record. Internal method
views borrow the central record temporarily and are released before execution or
allocating additional groups. RootedInterfaceMethod's implementation, target,
parameter_types and return_type inspection requires the owning Runtime and checks
the root identity. Public signature inspection returns copied data; call preparation
and dispatch read the record through scoped views.

Dead heap-object discovery visits occupied page/large allocations through their
directory back-references, not every historical directory slot. Interface wrappers
have no concrete payload row, so a narrow index records their occupied directory
slots without owning snapshots or replacing the direct snapshot locator. Wrapper
retirement removes those index entries before directory reuse; no wrapper is placed
in a slab. Generation tombstones and the separate inert identities left by native
take remain. Vacant directory slots form the free-identity chain without a separate
free Vec. Representation tokens retain descriptor admission for live and retiring
payloads; a slot or large allocation cannot be reused until disposal finishes.
The last disposed slot releases empty slab backing immediately, independently of
directory identity history. Descriptor/drop provenance survives disposal, including
panic-contained cleanup, without becoming an executable graph root.

Initial roots and graph owners expose slice cursors, and managed frame writes shade
before publication even after the root cursor has passed their slot. Native publication
and existing-edge republication join the same marker. Production nevertheless drains
these slices synchronously; no incremental safepoint scheduling is enabled yet.
Physical page/slot sweep uses a persistent cursor and reusable bounded page/range
disposal, not a complete heap dead-object list; inspected holes and survivors also
consume work. Remaining metadata/module inventories and interface batch filtering
remain separate costs, not an O(live) total-collector bound. PG04 is locally complete
at `fe56b675`; the
[phase ledger](../paged-incremental-gc-design.md#pg04-marker-implementation-and-open-acceptance)
separates fresh and reused evidence. PG05 owns remaining sweep adapters, pressure-driven
execution scheduling and production interleaving. Its integrated worktree implementation
has repaired preflight/unwind regressions and source-approved budget/cache corrections
with selected tests passing. Ordinary-build collector observation windows are implemented;
automatic-harness check mode and disabled-observation correction checks pass. The
completed frozen run passes all 22 observed normal-step gates (34 no-activity rows are
not passes). Fresh memory passes 55/55; ordinary timing failures still prevent PG05
acceptance, as recorded in the
[PG05 ledger](../paged-incremental-gc-design.md#pg05-integrated-driver-and-open-acceptance);
the synchronous description here records the accepted PG04 checkpoint. The selected 1024-unit normal budget
is not a measured wall-clock deadline; PG06 and full CI acceptance remain outstanding.

The integrated PG05 driver uses one collection engine with caller-selected work allowance:
normal service supplies 1024 structural units across all phases; explicit collection and
pressure assists supply `usize::MAX` to drain rather than repeatedly imposing the normal
slice limit. Retirement obeys the supplied allowance. This does not change the 32-owner
disposal cap, bounded-provider admission or resumable-provider chunk limits. Explicit
collection finishes any active cycle, then one fresh cycle; pressure assist finishes the
active cycle. Neither is a normal-step pause sample or a wall-clock bound.

Collection closes marking and preflights all stores before destructive admission, and
repeats that preflight when Sweep resumes after a possible mutator interval, not between
batches within the same exclusion. It then prepares accounting and detaches unreachable
objects and metadata in batches of
at most 32 owners total. A full-page token counts all its payloads; the current batch
finishes disposal before another batch/domain detaches. Physical preparation is
batch-atomic: identity/location/charge checks and aggregate credit subtraction are
prepared before that batch detaches. A later preparation failure faults/quarantines
the runtime with the failing batch still stored; earlier batches remain retired and
accounted. There is no whole-cycle rollback. Rust destructors run outside storage-table
borrows, consuming initialization before dispatch. Tracing and destruction cannot
reenter execution or mutate roots. A tracing panic quarantines the runtime without
sweeping a partial graph. Destructor panics quarantine it before the next destructor
while disposal continues for other detached objects; each payload is dropped once.
Process aborts, including a second panic during Rust's own unwinding, remain outside
recovery guarantees. Metadata output/cache-release/disposal reservations use the captured
remaining-slot upper bound, capped at 32, before detachment. This bound supplies no identity
proof: an exhausted cursor still validates its owner, but needs no output backing.
Runtime::collect_garbage reports reclaimed heap objects, reclaimed_operation_groups,
reclaimed_method_applications, reclaimed_interface_snapshots and reclaimed_modules;
there is no independent module-only sweep that could discard code reached from the heap.

Runtime::collect_garbage includes registered host/debug roots, execution windows,
reachable static cells, initializer results and pending host-path mutation records.
Path-view dynamic arguments are traced; Rust host objects and borrowed resources
remain host-owned. Path-operation
arguments and old/new values are temporarily rooted across host
read/preparation callbacks, including preparation that explicitly collects. Commit
actions only apply prepared host state; they cannot collect or execute scripts.
Execution slots occupy reusable contiguous windows. Canonical temporary registers
map to physical slots using prepared control-flow liveness; values stay rooted until
the physical slot is overwritten or its frame is dropped. Simultaneously live
registers cannot alias, including loop backedges and call operands/results. Named
locals retain distinct fixed slots and their debugger availability. Cold/native
access translates canonical indices, while prepared instructions use bounded
physical operands directly. Persistent host leases are
separate. Checked cursors borrow the current frame/session/window across consecutive
non-reentrant instructions and release all borrows before collection, observation,
calls and reentry. Cancellation and collection/observer eligibility are checked at
each original logical PC; the compact execution product retains a one-to-one mapping
to verified bytecode and source/debug records. Window
identities check owner and generation before native access. Active-root diagnostics
count both persistent root groups and execution windows. Trap and
call-depth failure drop frame roots through the same frame cleanup path.
Commit invariant failures use this cleanup path too, and quarantine the runtime.
Execution, allocation, collection and mutation entry points then reject further
work with EngineFault. There is no reset API; inspecting existing counters and
discarding the runtime remain possible.

`HeapCharge` separates `logical_units: usize` from `retained_bytes: usize`.
Logical occupancy includes the heap identity and each representation's payload
units; units are not byte measurements. Attributed bytes describe committed backing
pressure, not RSS, allocator usable size, all runtime memory or a heap quota.
The current attribution is deliberately bounded:

- Strings contribute their actual capacity, not just UTF-8 length.
- Physical slab/large backing includes inline native headers/payloads and struct
  bytes once, including retained class/page slack. Native provider charges include
  exclusively owned indirect backing, not a second shallow Box allocation. Sequence
  buffers use actual capacity times their physical element width (zero for Unit, `Value` width for
  traced slots). Managed field buffers, iterator key buffers and cold native Future
  capture/parameter buffers likewise use their known capacities. Task failure
  payloads include their owned error-message capacity.
- Library Map/Set payloads charge actual dense-entry capacity and a capacity-based
  estimate for their prehashed scalar index. Collision links are fields of each
  entry, not per-bucket Vec allocations. Rust's HashMap does not expose exact
  bucket/control allocation sizes; this part remains a pressure estimate.
- Referenced GC objects are charged separately. Shared type/code/layout metadata,
  external host resources and runtime directory/side-table/frame backing
  are excluded from this pressure attribution. Other non-native object backing is not
  covered by this byte ledger. Providers must identify unobservable allocations
  as excluded, not claim that a zero report proves no allocation.

Unsafe `NativePayload::charge() -> HeapCharge` is infallible and constant-time.
Admission and every completed general edit sample it, including error/unwind
finalization. Physical backing accounts for shallow payload bytes separately;
there is no additional Box-size charge. The runtime-owned `NativeObject` records
the provider charge; allocation uses it, nonmoving storage preserves it,
and checked storage mutations update it alongside committed storage. Sweep and take
release recorded charges without provider calls. Scanner and loan obligations are
specified by the [provider protocol](host-interop.md#general-native-payload-protocol).
A prepared replacement has its own charge; unchanged logical length does not imply
unchanged capacity. Library Vec main/scratch/progress remain in the installed
payload and charged through callback recovery; removals reduce logical units but
do not invent a capacity release. Claiming a cold Future transfers its capture backing out of the payload's
charge; its physical allocation remains charged until released. Native take releases payload units and bytes but
keeps one logical identity unit until its inert aliases are no longer traced.
See [provider obligations](host-interop.md#native-payload-accounting) and
[reservation failure](failure-semantics.md#modification-guarantees).

With `execution-diagnostics`,
`GcHeap::storage_usage() -> Result<StorageUsage, RuntimeError>` inspects concrete
backing lengths/capacities without creating roots, descriptors or backing buffers.
It rejects an active native payload loan or conflicting heap/interface storage
borrow; exclusively loaned inline payloads cannot be inspected. Callers must handle
that error, not treat it as an empty snapshot. Diagnostics separately report slab
and large backing, slot metadata, occupancy/retirement bitmaps, page-directory
capacity, small/large controls, backing Arc controls, representation descriptors
and descriptor-directory capacity. `initialized_payload_bytes` and
`native_payload_bytes` are subsets of backing, not additional allocations;
`unused_backing_bytes` includes vacant slots, class rounding and page tails.
Descriptor registry occupancy/capacity is reported without claiming exact hash
bucket/control bytes. Roots, executable metadata, canonical layout children,
registration metadata, other payload children and allocator overhead remain outside
these partial counts. They are neither process totals nor a peak-memory measurement.
Runtime diagnostics use generic payload charges rather than concrete collection
downcasts; stdlib collection/work counters belong to its separate diagnostics module.
The opt-in scoped allocator's requested-live-byte high-water measurement additionally
observes temporary retirement queues, but is not RSS or allocator usable size.

`GcHeapConfig.collection_threshold` enables automatic collection at execution
safepoints, including the current scalar JIT helper. Collection never runs inside
allocation, scoped storage borrowing or a standard mutation. Either of two
independent pressure thresholds can make collection due:

- Logical heap units plus operation-group, method-application, interface-snapshot
  and environment record counts reach the logical threshold (default floor 1024).
- Attributed retained bytes reach `retained_byte_threshold` (default floor 64 KiB).

After collection, each threshold becomes the greater of its configured floor and
twice its corresponding live count/bytes; floors are at least one and doubling
saturates. The byte floor is a policy default, not a measured optimum.
`collection_threshold: None` disables both automatic triggers; explicit runtime
collection remains available under the normal execution/borrow checks. For example,
a newly allocated empty String with 128 KiB of capacity has one logical unit but
can make byte-pressure collection due; allocation itself does not collect it.
`ResourceCounters` and `GcHeapStats` expose current/peak logical units and `u128`
current/peak retained bytes. `GcCollection` reports reclaimed units and retained
bytes as well as objects; GC stats also report collection count and the last pause.
These are attributed occupancy counters, not cumulative allocation or quotas.
The integrated incremental driver remains under PG05 acceptance; generational GC
remains deferred. Ordinary-build service observations are specified below.

Host retention uses gc::roots::RootedValue, returned by execution, typed conversion
and scoped retention. Raw backend registration is owned by GcHeap through unsafe
root_value_transport/root_execution_values_transport. Inputs require continuous
protection through publication; root registration cannot revive expired references.
Owning values reject nonstorable host capabilities, while execution packs preserve
their original scope requirements. The heap owns a generational root table containing the values; handles contain checked
heap/root identities and Arc leases. Clones share one entry. Registration reserves
one final-release notification credit and free-slot capacity. The last Arc lease
Drop enqueues its complete owner/slot/generation ID in a shared mutex-protected Vec
without allocation or borrowing heap storage. Credits cover live leases, pending
IDs and an ID being drained; poison recovery does not discard notifications.
The table drains IDs in place on registration, before snapshots and during
collection, checking owner, bounds, generation and absence of a strong lease before
clearing an entry. Duplicate, stale, foreign or still-live notifications cannot
retire a root. Reuse increments generation; exhaustion retires the slot. Identity
and credit retire before detached contents are destroyed outside the transport
lock. Registration no longer prunes the historical slot array.

Value and metadata snapshots still upgrade each weak lease and hold it while
copying that entry. An undelivered release is not evidence of liveness. A successful
upgrade racing the final host drop can conservatively retain an object for that
collection, never omit a live root. Messages carry no values, metadata or runtime
owner; table destruction disconnects the transport and frees its pending buffer,
making later lease drops harmless.
RootedValue::value and set are crate-private and validate the owning heap,
identity/generation and exact lease pointer. Safe replace consumes another retained
root; raw value_transport/set_transport require explicit backend protection.
Replacement also validates incoming references. Copying a raw Value does not retain
heap objects. Persistent native/debug retention uses this table; active interpreter
frames are traced from runtime-owned scalar/managed windows.
Executable metadata roots are published through Runtime so program ownership and
metadata generations are validated before replacing the old references. Failed
replacement preserves the previous roots; released or foreign metadata cannot
resurrect storage. Collector corruption tests inject invalid roots only through a
test-only storage hook.

Lazy parent-interface, method-application and receiver-operation edges use the
same controlled publication rule. Object policy prepares records and reads caches;
only the metadata link boundary can fill the private cells. Publication checks
the destination identity/slot and incoming executable graph before changing any
cell. Receiver preparation validates the shared and method cells together, so a
failed write cannot publish half the pair. Preparation performs no callback or
safepoint between validation and commit. Cached edges participate in coordinated
tracing and do not independently retain otherwise unreachable records.

Root leases are Send + Sync and can be cloned/dropped on other threads without
accessing values. They do not own the heap or payload storage; runtime teardown
releases that storage even if leases survive, and use against another runtime is
rejected. Runtime is Send and not Sync. Hosts can transfer its ownership between
threads outside synchronous execution/borrow scopes, or keep it in a Send future
across message-receive awaits. RefCell/Cell storage remains exclusively owned;
this does not enable concurrent access or asynchronous script execution.

`Opaque<T>` retains a checked root into runtime-owned native storage; cloning the
handle preserves identity without cloning T. Read/edit acquire short checked
loans. Explicit take validates extraction capability and active operations before
atomically removing the payload and invalidating every old alias. Failed take
leaves storage unchanged. Old roots can retain an inert invalidation identity,
but cannot access the payload or revive it after reinsertion. Payload destruction
belongs to the runtime until successful take, then to the receiving Rust owner.
See [opaque access](embedding-api.md#retained-objects-and-prepared-calls).

External `Ref<T>`/`RefMut<T>` instead reserve host-owned storage under an invariant
scope brand. GC retains only checked view identities; addresses remain in the
host borrow owner. Scope exit revokes every root/child view before the Rust loan
ends, including errors and supported unwind. Holding a copied script Value cannot
extend that lifetime. These views cannot cross suspension or runtime transfer.
See [scoped borrowing](host-interop.md#borrowed-host-values).

Shared native/host registrations and storage factories require Send + Sync and
use Arc. Runtime-owned native payloads and observers require only Send. Their
storage and destruction move with the runtime. Host-owned mutable services captured
by shared callbacks must synchronize their own state. Prepared host-path commit
actions remain synchronous call-local values and need no Send bound. Standard
registrations are immutable process-wide OnceLock data; no runtime storage is shared.

Immutable type arguments, lexical type bindings, storage/layout contracts and
stored callable/cursor descriptors use Arc and satisfy Send + Sync. These contain
only checked IDs, immutable type/code facts and the synchronized definition context;
they do not share mutable heap/session/program storage. Transferring type facts
does not revive a collected executable environment. Native code products and
installed descriptors also share transferable finalized owners; each installation
is still checked against its runtime and exact dependency versions.

Runtime owns GcHeap directly. Iteration, mutation and key-lookup exclusion records
also live in the heap. Native operations borrow short leases; guards stored in
frames hold owning lease tokens, without owning those records or the heap. Cursor
leases expire with their execution session, including cancellation and trap exits.
Reopening a retained cursor checks the source revision before acquiring a fresh
lease. Expired records cannot block writes and are pruned during acquisition/GC.
Runtime teardown releases payload storage even when public roots or collection
leases remain alive; dropping those leases afterward does not access the destroyed
heap. Execution/session and host-borrow guards borrow runtime-owned storage and
cannot outlive or move their runtime.

Runtime::retain_module registers version retention through
module::retention::ProgramLease. Runtime owns execution-state and candidate-access
policy; ModuleStore owns records and no longer retains ResourceState. Instance
reads and writes enter through Runtime's checked APIs. ModuleStore's instance and
retention mutation methods are crate-private storage operations.
Runtime::read_module_slot_transport and write_module_slot_transport are the unsafe
backend execution access boundary; callers protect raw inputs and publish copied
results before collection or callbacks.
Writes check the installed descriptor, slot bounds and mutability, its bytecode
representation, storable/live heap references and candidate ownership before
replacing a reference. Candidate checks also apply between initialization and
publication; publication still rechecks the transitive graph after intervening
object mutations. The old value is disposed outside the module-store borrow, with
no safepoint or callback between validation and the write. Reads detect damaged
storage and quarantine the runtime. module_instance_snapshot returns a detached
diagnostic copy; production APIs expose no mutable slot vector.
The runtime owns ModuleStore's mutable records directly. Staged handles contain a
lifecycle token instead of a shared store. Publication checks the token and installed
code identity against the destination runtime. Dropping a candidate immediately
invalidates access to its members, even if a separate retention lease still exists;
the coordinated collector later disposes of its records. Abandoned candidates request
collection at the next automatic safepoint or validated load/stage operation.
Disabling automatic GC leaves that disposal to explicit collection or runtime teardown.
ModuleStore::loaded_count reports available members, excluding abandoned candidates;
ResourceCounters does not maintain a second module count.

Runtime::retain_module checks the supplied ExecutableModule's actual installed program, including
runtime ownership; numeric module keys alone cannot authorize retention. Clones
share one registration, and last-drop releases eligibility without borrowing the
module store. Active sessions and installed native code use these leases; native
code pins every member of its dependency program. Closure/interface heap objects
use traced edges instead of lease roots. Lease tokens can outlive the runtime and
do not own module instances. Selected-operation descriptors hold executable edges,
not independent program leases: an obsolete instance/closure/environment cycle is
reclaimed in one collection once external roots disappear. Detached metadata
snapshots cannot republish a closure after its executable dependencies have been
released. Retention counts describe explicit leases, not all reachable programs.
Operation groups can form cycles through cached method applications and environments;
their checked edges let one collection detach the records and release that cycle.
Method applications live by value in a runtime-owned generational table. Caches,
prepared methods and metadata roots store ApplicationId edges; IDs alone do not
retain records. Scoped method views check runtime ownership and generation before
borrowing an application. Closed method applications reuse their receiver environment,
while method-local generic arguments remain call-specific. Both operation groups
and applications contribute to automatic collection pressure even without heap
allocations. A borrowed metadata view prevents sweeping before any store detaches.
Interface snapshots also live by value in a runtime-owned generational table.
Heap interface wrappers, prepared selections and cached parent views use checked
InterfaceSnapshotId edges. A metadata root reaches its receiver even when no heap
wrapper remains; parent caches retain their exact executable dependencies. Repeated
upcasts reuse the parent record while retaining separate wrapper identities.
Snapshot reads are scoped borrows; snapshot occupancy contributes to automatic GC.
Heap wrappers use the common nonmoving allocator and bounded physical retirement.
Their pages may be released while an independent metadata root retains the snapshot;
that snapshot never implicitly retains a wrapper identity.
A prepared method carries copied receiver/type data and checked metadata identities,
so it does not keep an interface record alive after runtime teardown.

ModuleStore owns each ModuleRecord by value: module instance slots, installed native
links and mutable layout caches retire together during coordinated collection.
ExecutableModule shares an immutable ProgramDescriptor through Arc. The descriptor
contains verified code and inert identity/binding facts; it is Send + Sync and cannot
retain installed callbacks, caches or mutable module state. Native dispatch resolves
its binding through ModuleStore and releases the store borrow before invoking Rust.
Publication, execution entry, retention and metadata traversal check the exact
installed descriptor as well as the module key and runtime owner. Copying keys,
epochs or binding facts does not authorize an uninstalled program.

Installed generic layout applications use the module record's cache. Pure descriptor
inspection can also derive a layout without that cache, including after the old
executable record has been collected or while instance storage is borrowed. This
preserves old type provenance without granting execution or instance access. Reading
detached type facts never repopulates the released runtime record.

Executable environments live by value in EnvironmentStore. TypeEnvironment is a
checked EnvironmentId plus immutable TypeBindings; cloning it neither roots the
record nor shares ownership of executable storage. Parent environments and selected
operations are ID edges, marked with the rest of the metadata graph. Publication
checks their runtime, slot and generation before inserting a record. Adding call-local
selections publishes a new environment rather than mutating an existing application.
Environment occupancy contributes to automatic collection pressure. Scoped record
views prevent allocation and sweeping with a checked error before any detachment.

TypeBindings contains substitutions, associated-type facts and type-only parent
scopes. Layouts, type-argument origins and compatibility checks retain these immutable
snapshots without retaining executable environments. Type information remains usable
after environment collection; executing a captured environment still requires its
live checked ID and explicit reachability. Bare handles cannot revive released
records, alias recycled slots or keep runtime storage alive after teardown.

ClosureValueSnapshot records live by value in the existing heap object slots;
there is no separate owning Rc or closure metadata table. PreparedClosure stores
only the closure Value identity, and its snapshot inspection requires the owning
Runtime. Runtime::resolve_closure returns a checked scoped view. ExecutionStack
accepts a closure Value, resolves it in that runtime and roots captures in the new
frame before execution. Native call preparation releases metadata borrows before
script invocation; callbacks can allocate, collect and reenter normally. Stored
callback descriptors trace identities; host-owned `Function` handles retain the
required callable state automatically. Named values use live routes without obsolete
code leases; created closures retain exact body/capture owners. Neither form keeps
runtime storage alive after teardown. RootedCallable has been removed.
An explicit diagnostic snapshot copy is not an execution or retention capability.
Holding a scoped heap view blocks mutation with a checked error and no partial
write or accounting change; collection rejects the borrow before any detachment.

### Collector observation windows

[`gc::metrics`](../../crates/kagari-runtime/src/gc/metrics.rs) provides opt-in,
fixed-space observations in ordinary builds, without `execution-diagnostics` or a
counting allocator. The heap owns the observation buffer; copied results own no heap,
program, descriptor or root references and cannot affect liveness or execution policy.

| `GcHeap` method | Contract |
| --- | --- |
| `start_collection_observation() -> Result<(), RuntimeError>` | Starts or resets a window, clearing its counts/maxima and recording whether a cycle is already active. First start allocates one fixed-size buffer; reset reuses it. Normal execution/borrow exclusion applies, including quarantine rejection. Start/reset is rejected during admitted collector service, including destructor callbacks; the existing window is unchanged. |
| `collection_observation() -> Option<CollectionObservation>` | Copies the current window without resetting it; None means observation is disabled. Results remain available after a trace fault. |
| `finish_collection_observation() -> Option<CollectionObservation>` | Outside collector service, returns the window and releases its fixed backing; subsequent reads return None until another start. During admitted service (including destructor callbacks), returns None without changing the window. |

No history, callback, root registration or per-step observation allocation is added.
Optional observation clocks, sample construction and accumulation are gated before
operations when observation is disabled; structural work charging and public `last_pause`
remain active independently. This does not promise zero total collector instrumentation
cost. Start/reset/finish are host-selected boundaries, not collector steps. They may
change a window between service invocations, including while an incremental cycle is
still active, but cannot change observation enablement inside an admitted invocation.
A destructor callback may copy the current snapshot; it cannot start/reset or finish the
window during service. Snapshot reads need not include the still-running operation or
invocation. No active observation borrow is held across a yield or user callback. Counters and
work/time sums saturate instead of wrapping. Work means charged structural units,
not CPU instructions; a failed bounded scan can retain its prepaid reservation on
unwind rather than refunding unused work.

`CollectionObservation` separates three disjoint invocation series: `normal`,
`pressure_assist` and `explicit`. Each `WorkMeasurements` reports count/failures and
total/last/maximum work and duration. Disabled/no-op safepoints and operations rejected
before collector admission are not samples. Structural `slices` report work only;
slices inside an explicit call or assist are **not normal automatic pause samples**.

Invocation timing covers the complete admitted service, including entry guards,
completed-cycle/threshold bookkeeping, owned-cursor destruction, discarded automatic
reports and error normalization. It ends immediately before the final telemetry update;
that update and return-to-caller instructions are excluded. It excludes interleaved
mutator time. Raw pauses **include indivisible operations** and are never adjusted by
subtracting their attribution. Explicit active-plus-fresh completion is one explicit
invocation, not two inferred normal steps.

`cycles` accumulates full collector work/service time for cycles begun in the window,
including completion bookkeeping. `active_cycle_at_start` identifies a cycle already
in flight; its eventual full result goes to `carried_cycles`, including accumulated
pre-window work/time. For example, resetting observation after a normal slice clears
normal maxima but does not restart the collector: later completion of that old cycle
is carried, while a subsequent fresh cycle belongs to `cycles`. Do not attribute the
carried total wholly to the new workload/window. Entry-guard timing is available only
while observation is enabled; cycle service totals exclude gaps spent running mutator
code and are not wall-clock elapsed time from first to last slice.

Indivisible attribution is an array indexed by the eleven `IndivisibleKind` values:
`ProviderScan`, `PayloadDrop`, `BackingRelease`, `MetadataDrop`, `RootDrop`,
`DescriptorDrop`, `WeakTokenDrop`, `DirectoryCompaction`, `EmptyMapRelease`,
`QueueCompaction` and `LifecycleDrop`. Each kind accumulates count, total items/work,
known bytes, unknown-size count and duration, retaining only its slowest sample.
Physical payload destruction counts **each actual payload destructor**, including
those inside page/range batches, rather than retaining only a batch maximum. Range
retirement tokens carry no accumulator: an enabled disposal caller owns the call-scoped
measurement, merges it after disposal, and retains no observation reference across a
yield. Disabled disposal and fallback cleanup use the same destruction loop without an
accumulator; destructor panic containment, charges and public pause timing are unchanged.

A sample contains a static owner/representation name, optional native-provider
`TypeId`, local item/work extent, optional known bytes and duration. Provider scans
can identify their Rust type; physical payload drops identify representation kind,
not necessarily that Rust type. Known bytes describe local backing or shallow size,
not transitive destructor cost, RSS or necessarily bytes freed; None is explicitly
counted as unknown, never silently treated as a zero-size object. Samples retain no
script identities. Nested attribution is evidence about concrete operations, not an
exemption for an entire collection, nor a value to subtract from an unrelated maximum.

These APIs permit ordinary-build observation but do not establish a pause guarantee
or pass the performance gates. The active [PG05 ledger](../paged-incremental-gc-design.md#pg05-integrated-driver-and-open-acceptance)
owns remaining harness/measurement acceptance, not a second metrics progress queue.

## Type Registry

The runtime contains a unified type registry.

This registry backs:

- reflection
- runtime type checks
- interface values
- downcast
- host type registration

The model is:

```text
TypeRegistry {
  by_id: Map<TypeId, TypeInfo>,
  by_name: Map<String, TypeId>
}
```

Each `TypeInfo` carries:

- `TypeId`
- name
- kind
- field metadata
- variant metadata
- method metadata
- implemented trait metadata

Reflection rules are defined in [reflection.md](reflection.md).

## Interface Values

Trait/interface values are modeled as runtime interface objects.
Kagari does not expose Rust-style `dyn` trait-object syntax to scripts.

The model is:

```text
InterfaceObject {
  data: ValueHandle,
  concrete_type_id: TypeId,
  trait_id: TraitId,
  vtable_id: TraitVTableId
}
```

This model supports:

- dynamic dispatch
- reflection over both concrete and interface identity
- `is<T>`
- `downcast<T>`

Trait-system rules are defined in [traits.md](traits.md).

## Host Registry

The host registry manages:

- exposed host functions
- exposed host types
- parameter passing metadata
- declared effects for host entry points

This extends the current shape in `host.rs`.

The model is:

```text
HostRegistry {
  functions: Map<Symbol, HostFunction>,
  types: Map<TypeId, HostTypeInfo>
}
```

## Host Call Frames

Host-to-script and script-to-host calls that involve borrowed host data create explicit call frames.

The model is:

```text
HostCallGuard {
  frame_id: FrameId,
  borrow_table: BorrowTable
}
```

This frame owns the validity of all borrowed host handles created during the call.

## Borrow Table

The borrow table is responsible for preserving Rust aliasing rules at the interop boundary.

It tracks:

- which host object ids are currently borrowed
- whether the borrow is shared or unique
- which frame owns the borrow

The model is:

```text
BorrowTable {
  entries: Map<HostObjectId, BorrowState>
}

BorrowState {
  frame_id: FrameId,
  kind: Shared | Unique,
  shared_count: u32
}
```

This table rejects:

- multiple simultaneous unique borrows of the same object
- a unique borrow while any shared borrow is active
- a shared borrow while a unique borrow is active

## Frame-Scoped Host Borrow Tokens

Borrowed host values used during host calls are represented explicitly.

The model is:

```text
FrameHostBorrowToken {
  frame_id: FrameId,
  object_id: HostObjectId,
  type_id: TypeId,
  borrow_kind: Shared | Unique,
  epoch: BorrowEpoch
}
```

These tokens are:

- valid only during their owning frame
- non-storable in GC-managed objects
- rejected at suspension boundaries

Host interop rules are defined in [host-interop.md](host-interop.md).

## Execution Control and Lifecycle Accounting

RuntimeLimits contains a runtime-wide optional max_call_depth (default Some(256)).
Runtime owns ResourceState by value within its heap; components use scoped borrows,
not Rc/Weak ownership of the resource/session graph. ExecutionSession<'runtime>
identifies a central record containing the root program, immutable
options and cancellation state. SessionId checks the runtime owner, slot and
generation; removing a session invalidates its ID before the slot can be reused.
Initializers, entry execution, callbacks and backend fallback share these inputs.
There are no instruction, wall-time, allocation, host-call or reflection quotas.
Live heap occupancy, module count and call depth serve GC and ownership diagnostics.
Installation determines API availability; member access obeys language/interface
contracts. See [security](security.md) for the trust boundary.

Termination stays recorded until the final session scope drops. Nested execution
cannot replace root inputs or swallow cancellation. A fresh root starts clean.
Nested entries validate installation authority and retain exact admitted executable
owners; ordinary calls select current implementations rather than a session-wide old
dependency set. Its central record retains the admitted root's program lease; ending
the last scope needs no module-store borrow.
Synchronous host callbacks reenter the existing
explicit driver with the same runtime and root options. Each host context owns its
borrow guard; outer scopes remain active during nested execution. SessionStore owns
the records and one ExecutionFrame stack per session, including interpreter, nested
callbacks and VM native entry scopes. Records and frame stacks have separate borrow
domains so frame access can check session cancellation and account resources.
An immutable view of another session's frames does not delay ending an empty
suspended session: its roots, lease eligibility and record are released immediately.
Only the empty frame-table bucket waits for the next mutable table access.
ExecutionStack guards remember their stack base and unwind only their
own suffix, releasing roots and depth counters even after termination/quarantine.
Manual public call-depth entry/exit APIs are removed; only frame scopes update it.
HostBorrowTable owns its borrow records by value. HostCallGuard borrows that table
and, for runtime calls, the resource gate; it cannot outlive either. HostResourceScope
holds root leases into the central root table and registers its frame identity in
the session. Host calls and path callbacks use this scope, and cleanup removes its
registration, releases borrow/root leases, then drops its session handle.
Host scopes can outlive an outer session handle
without clearing its cancellation or termination state. ExecutionSession::host_scope_count
reports registered host scopes for diagnostics and cleanup assertions.
VM/SDK execution and reload publication use checked shared runtime references.
Runtime is not Sync: these references do not authorize concurrent execution. The
driver, store borrows and session identities enforce scoped synchronous reentry.
ModuleStore owns version reservation along with publication. Runtime-wide synchronous
activation guards reject publication inside script, observers, callbacks, reentry or
driver cleanup; parked old-version sessions may remain without blocking publication.
Registration and owner replacement still require mutable access; a live execution guard prevents moving
or destroying the runtime. Transfer is possible once these scopes have ended.
Frames own their immutable loaded version; no borrowed bytecode lifetime crosses
runtime entry. Frames and stacks do not retain a heap owner; slot access receives
the current Runtime and checks its identity before touching the root table. A foreign
context is rejected before any frame or slot change. Invalid frame access,
suspended-scope mutation and out-of-order scope destruction quarantine the runtime instead of resuming a damaged stack.

Runtime owns one boxed ExecutionObserver; set/clear operations require no active
session and reject outstanding observer borrows. Typed Ref/RefMut views inspect its
state without sharing storage ownership. attach_execution_observer activates it
once for a root before frames run; nested drivers inherit that activation. The
observer's begin hook initializes the pinned program, and observe receives the
complete stack at instruction/trap boundaries with exclusive observer access.
Initialization and observation hold short immutable stack borrows and must not
invoke script execution. The VM stores DebugSession in this same runtime-owned
slot; it keeps no separate debugger owner across host reentry.

Cancellation is checked cooperatively at interpreter/JIT safepoints and native
polling boundaries. It cannot preempt a blocking host callback. Primitive bulk
operations may finish before the next poll. A prepared commit is uninterrupted:
cancellation requested inside it is observed after its target and dirty record
are committed. ExecutionCounters reports lifecycle peaks and elapsed time;
optional tracing provides observations without execution charging.
`ExecutionOptions::inputs` supplies
a fixed logical Unix time in milliseconds and a random seed for the root session.
Host callbacks access those values through `HostCallContext`; random draws use a
per-root SplitMix64 stream, including synchronous reentry. A new root starts at
the seed again. When host-call recording is enabled, the root session retains an
ordered trace with the complete verified code fingerprint, root identity, inputs,
bounded argument/result snapshots and outcome categories. It reserves call slots
before entering callbacks, preserving order through synchronous reentry. The
trace caps at 10,000 calls and reports further dropped calls; captured strings,
tuples and argument lists also report truncation. Opaque handles are diagnostic
runtime identities. The trace is not an external-result replay format.

## Module Store

The runtime distinguishes loaded module code from the compilation pipeline.

The ownership model (schematic, not a Rust field listing) is:

```text
Prepared program -> immutable verified code and descriptors
Runtime -> module records, installation static cells, heap, host bindings
Module record -> loaded generation, static bindings, constants, linked metadata
Program / Module -> live installation / logical member
ExecutableModule -> exact program descriptor and member slot
CallRoute -> checked installation declaration/application, no executable lease
Static binding -> checked installation cell identity
```

Loaded code has versioned identity; mutable static storage belongs to the
installation, not to a raw slot array copied for each module epoch.

Exact prepared-version identity survives cloning and cross-context normalization,
but independently prepared equal bytecode or matching fingerprints are not the same
version. Identity alone owns no original executable graph or definition snapshot
and cannot authorize execution or resurrect a retired generation. Normalization
imports from the current immutable scope; destination-local IDs and accepted-prefix
validation remain mandatory. Runtime ownership, epochs and actual callable/native
execution leases are separate checks and owners. See
[definition identity ownership](../architecture.md#definition-identity-ownership)
for the private token and installation-scoped writer mechanisms.

Type/layout provenance remains readable after executable retirement without granting
executable admission. Current type-only holders can nevertheless retain immutable
program payloads through their supplying descriptor; retirement of a registry entry
is not proof of payload destruction. The measured remaining split is tracked by
[R13](../review.md#r13--type-and-layout-provenance-retains-retired-code).

## Persistent static cells

Each installation owns checked cells; each generation has bindings from its local
static operands to those cells. Imports bind the supplier's declaration rather
than allocating another cell. Independent loaded roots and runtimes never share
cells merely because they share prepared code. Access validates ownership,
generation, declared type and access rights through the binding; it does not scan
all historical versions. [Module activation](module-activation.md) owns lazy
initialization, fixed recipes, compatibility and transactional publication.

The initializer commit and ordinary stores validate the complete semantic value,
not just its physical representation. Validation includes applied aggregate,
interface, native, callable and durable-storage contracts. Scoped borrows and
other ephemeral capabilities cannot escape into cells. A normal store requires a
Ready mutable cell; only initializer completion may establish the first value,
including for `static val`. Public runtime reads/writes are Ready-only and never
execute a recipe. Diagnostic snapshots report the state without forcing it;
these APIs still enforce candidate restrictions and quarantine. A copied raw
Value is not an external GC root: retaining it across collection requires the
ordinary root API.

The joint GC graph is:

```text
active installation / retained executable / active initializer
  -> generation bindings -> static cells
       Ready         -> stored value graph
       Uninitialized -> introducing recipe's pinned executable
       Initializing  -> active recipe and execution roots
       Failed        -> detached bounded diagnostic only
```

A recipe stays fixed even if first used after several publications; its ordinary
helper calls select current implementations rather than a pinned old dependency set.
Publication does not reset Ready values or retry Failed cells. Once Ready,
the value may independently retain old closure/Future bodies and lexical type or
interface evidence. Named callable values do not retain obsolete code just to route
later calls; ordinary calls use current implementations. Exact computation ownership
is not a permanent historical root.
Failed cells do not retain executable recipe edges. Pure type/layout descriptors
are not executable or mutable-state roots. Cycles through programs, cells and
heap values, including cross-installation cycles, can be reclaimed once external
roots and active installation/execution ownership disappear.

Initializer results remain rooted through return, semantic admission and Ready
publication. No cell-storage borrow spans script, native, observer or destructor
callbacks. Stack drop and cancellation retire active attempts under
[initialization failure cleanup](failure-semantics.md#static-initialization-failures).
Native payload disposal uses the existing detach-before-destroy and panic
containment rules above. This adds neither an independent collector nor a reset
operation. Live cell state is not an artifact or a disk snapshot.

## Hot Reload

Hot reload is coordinated through explicit module epochs.

[Publication and static compatibility](module-activation.md#static-publication-compatibility)
define the implemented contract; [reload.rs](../../crates/kagari-runtime/src/reload.rs)
implements staging/commit and [publication.rs](../../crates/kagari-runtime/src/publication.rs)
owns host-top-level synchronous admission. Frozen declaration and local static-initializer
checks use installation inventory independently of live heap values; ordinary
Hash/Eq and helper bodies may change. Source snapshots retain
immutable diagnostics separately from executable leases and SDK compilation capsules.

The runtime uses epochs for:

- module version tracking
- stale handle detection
- metadata comparison across reloads
- exact computation ownership, current ordinary-call selection and shared static bindings

Epoch changes do not migrate values, reset static cells or implement state
snapshot/restore. [Compatible publication](module-activation.md#compatible-declaration-changes)
preserves existing values and frozen declaration contracts.

## Suspension and Ephemerality

At `.await` suspension points, the runtime distinguishes:

- suspendable values
- non-suspendable ephemeral values

Borrowed host handles are explicitly non-suspendable.

This means:

- a frame with live borrowed host handles must not be suspended
- runtime stack snapshots must reject non-suspendable values

The [parking transition contract](execution.md#owned-interpreter-driving) validates
actual reachable values under the collection guard, not just their static types.
Safety is operation-local and must be re-established after mutation or host polling.

## Runtime Errors vs Engine Bugs

The runtime classifies failures clearly.

Script/runtime errors include:

- rejected interface or execution-phase access
- invalid reflective writes
- use of an expired host borrow
- resource limit violations

Engine bugs include:

- internal invariant violations
- invalid unchecked access to stale handles
- corrupted runtime bookkeeping

This distinction determines whether the runtime reports a script trap or panics internally.

## v1 Runtime Slice

The first runtime version includes:

- primitive values
- GC handle values
- host function registry
- explicit host passing styles
- frame-scoped host borrow handles
- type registry with stable `TypeId`
- root cancellation and execution phase
- module epochs

This supports the current language model without locking in a highly complex VM object model.

## Implementation Order

The incremental implementation order is:

1. strengthen the `Value` model around script values versus host values
2. add a runtime `TypeRegistry`
3. extend `HostRegistry` with host type metadata
4. add `HostCallGuard` and `BorrowTable`
5. validate installed contracts at host entry points
6. connect module epochs and stale-handle checks
7. grow reflection and interface values on top of the shared type registry

This order lets the runtime stay coherent while each subsystem is added with a clear responsibility boundary.
