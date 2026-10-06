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

## Value Shape

Enum objects contain an `EnumTag` and immutable payload slots. Declared variants
hold an `EnumVariantRef` from a verified module; standard Option/Result have typed
tags. Names are display metadata. Allocation checks runtime ownership, payload
arity, storage/representation and nested payload type/schema consistency before
publishing the allocation. Declared payload structs/enums must match the receiving
version's nominal layout. Standard-enum built-ins reject same-named declared types.
Enum equality compares tags/layouts and then members using script equality, so
mutable members compare by identity. Enum objects retain their executable version
and trace their payloads through the same mark-sweep root mechanism. Publication
does not invalidate old values; rooted old enum objects keep their original layout.

Script struct objects contain a `StructLayoutRef` and positional `Value` slots.
The layout handle comes from a verified `LoadedModule` and retains that immutable
executable generation. Objects do not duplicate field names. Allocation requires
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

The current `value.rs` file already separates script-owned handles from host-backed handles.

The value shape is:

```text
Value {
  Unit,
  Bool(bool),
  I32(i32),
  I64(i64),
  F32(f32),
  F64(f64),
  Str(StringHandle),
  GcHandle(GcObjectId),
  InterfaceHandle(InterfaceObjectId),
  HostOwned(HostObjectId),
  HostPathView(HostPathViewId),
  Ephemeral(EphemeralValueId)
}
```

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

The implemented collector is nonmoving, stop-the-world mark-sweep. It traverses
tuples, enum payloads, array/map values and struct fields with an explicit work stack,
handles cycles, and reclaims unreachable slots. Reuse increments the slot generation;
a saturated generation retires its slot. Reads, writes, root registration and script
equality reject foreign, stale or incorrectly tagged references. Failed handle
validation cannot consume heap growth units or change the target object.

The marker walks identities through storage-provided edges. Object representations
own reference traversal; closure signatures and runtime type compatibility have
separate execution owners. Heap objects and executable programs share one mark
worklist. Published and staged programs, active-call/code leases, host roots and
heap root records seed traversal. Runtime-owned execution windows independently
seed their values and program/environment metadata, including suspended callers.
Prepared-method and selected-native-call metadata is stored alongside persistent
roots; public leases never own that storage.
A reached program traces its dependency members'
module slots; a reached closure or interface traces its executable owner and
environment. Metadata traversal uses an explicit worklist, visits shared descriptors
once and excludes pure layout provenance from executable state retention. Program
edges validate the installed generation and runtime identity. An obsolete module
slot containing its own closure, or mutually referring obsolete programs, does not
by itself keep either side alive.

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

Collection completes marking and accounting before
removing any slots, then detaches unreachable objects, operation groups, method
applications, interface snapshots and module instances before running their
Rust destructors outside storage-table borrows. Tracing and destruction cannot
reenter execution or mutate roots. A tracing panic quarantines the runtime without
sweeping a partial graph. Destructor panics quarantine it while disposal continues
for other detached objects; each payload is dropped once. Process aborts, including
a second panic during Rust's own unwinding, remain outside recovery guarantees.
Runtime::collect_garbage reports reclaimed heap objects, reclaimed_operation_groups,
reclaimed_method_applications, reclaimed_interface_snapshots and reclaimed_modules;
there is no independent module-only sweep that could discard code reached from the heap.

Runtime::collect_garbage includes registered host/debug roots, execution windows, module slots,
initializer results and pending host-path mutation records. Path-view dynamic arguments
are traced; Rust host objects and borrowed resources remain host-owned. Path-operation
arguments and old/new values are temporarily rooted across host
read/preparation callbacks, including preparation that explicitly collects. Commit
actions only apply prepared host state; they cannot collect or execute scripts.
Register/local slots occupy reusable contiguous windows and stay conservatively
rooted until overwritten or their frame is dropped. Persistent host leases are
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

GcHeapConfig.collection_threshold schedules automatic collection at instruction
safepoints, including the current scalar JIT helper. Collection never runs in the
middle of a standard mutation. Scheduling counts live heap units plus operation
group records, so metadata-only allocation can also trigger collection. The default
minimum threshold is 1024; after collection it grows to twice the combined live
count. None disables automatic collection,
while explicit runtime collection remains available. Heap units are accounting units,
not byte measurements. Stats report live/peak units, live objects, collection count,
reclaimed object count and the last pause. Incremental and generational GC remain deferred.

Host retention uses gc::roots::RootedValue, returned by Runtime::root_value. The
heap owns a generational root table containing the values; handles contain checked
heap/root identities and Arc leases. Clones share one entry. Dropping the last lease
removes retention eligibility; expired entries are pruned on registration and
collection. A concurrent last drop may retain an object for one extra collection.
RootedValue::value and set require the owning heap and validate identity/generation;
replacement also validates incoming references. Value::clone alone does not retain
heap objects. Frame/native/debug slots use the same table through RootSet.
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
Runtime::read_module_slot and write_module_slot are the execution access boundary.
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

Runtime::retain_module checks the supplied LoadedModule's actual installed program, including
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
A prepared method carries copied receiver/type data and checked metadata identities,
so it does not keep an interface record alive after runtime teardown.

ModuleStore owns each ModuleRecord by value: module instance slots, installed native
links and mutable layout caches retire together during coordinated collection.
LoadedModule shares only an immutable ProgramDescriptor through Arc. The descriptor
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
callback descriptors trace identities; host-owned PinnedFunction handles retain the
closure and its executable scope automatically. Neither form keeps runtime storage
alive after teardown. RootedCallable has been removed.
An explicit diagnostic snapshot copy is not an execution or retention capability.
Holding a scoped heap view blocks mutation with a checked error and no partial
write or accounting change; collection rejects the borrow before any detachment.

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
Nested module entries must belong to its pinned dependency program. Its central
record retains a program lease; ending the last scope needs no module-store borrow.
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
ModuleStore owns version reservation along with publication, so publishing a new
version does not invalidate a borrowed old-version session. Registration and owner
replacement still require mutable access; a live execution guard prevents moving
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

The model is:

```text
VerifiedProgram { root: ModuleRef, code: Arc<[Arc<BytecodeModule>]> }
Runtime { modules: ModuleStore, heap, host_bindings, execution_cache, limits }
ModuleStore { records: Map<ModuleKey, ModuleRecord>, publication, retention }
ModuleRecord { module: LoadedModule, instance: ModuleInstance, native_links, layout_cache }
LoadedModule { program: Arc<ProgramDescriptor>, slot: ModuleRef }
```

The execution format is allowed to diverge from raw IR, but the runtime keeps the concept of a loaded module with versioned identity.

## Hot Reload

Hot reload is coordinated through explicit module epochs.

The implementation in [reload.rs](../../crates/kagari-runtime/src/reload.rs) is compatible with this.

The runtime uses epochs for:

- module version tracking
- stale handle detection
- metadata comparison across reloads
- state migration tooling

## Suspension and Ephemerality

If Kagari adds suspension points such as `yield` or `await`, the runtime distinguishes:

- suspendable values
- non-suspendable ephemeral values

Borrowed host handles are explicitly non-suspendable.

This means:

- a frame with live borrowed host handles must not be suspended
- runtime stack snapshots must reject non-suspendable values

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
