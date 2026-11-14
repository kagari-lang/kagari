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
- `crates/kagari-runtime/src/execution/memory.rs`
- `crates/kagari-runtime/src/execution/roots.rs`
- `crates/kagari-runtime/src/host.rs`
- `crates/kagari-runtime/src/reload.rs`
- `crates/kagari-runtime/src/backend.rs`

This specification uses those boundaries.

## Top-Level Runtime Structure

The runtime structure is:

```text
Runtime {
  memory: ManagedMemory, // owns RootDriver<ExecutionRoots>
  resources: ResourceState, // control, sessions and execution-space accounts
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
- VM-byte and Native-stack limits, diagnostic logical depth and root cancellation
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
managed-heap quotas; the execution-space ceiling is a separate VM-stack guard.
[Host scalar conversions](host-interop.md#fixed-width-numeric-host-boundary) are distinct from zero-copy element storage.

## Value Shape

EnumObject contains an EnumVariantRef from a verified module, immutable traced
payload slots and an optional detached diagnostic origin. Standard Option/Result
constructors use their checked nominal variant facts. Names are display metadata. Allocation checks runtime ownership, payload
arity, storage/representation and nested payload type/schema consistency before
publishing the allocation. Declared payload structs/enums must match the receiving
version's nominal layout. Standard-enum built-ins reject same-named declared types.
Enum equality compares tags/layouts and then members using script equality, so
mutable members compare by identity. Enum objects retain immutable version metadata
and trace their payloads through the same mark-sweep root mechanism. Publication
does not invalidate old values; rooted old enum objects keep their original layout.

Script struct objects share a closed layout derived from `StructLayoutRef` and
store packed scalar bytes and separate traced managed cells in their actual
StructObject provider. Scalars use exact semantic widths and preserve float bits;
Unit needs no bytes. Managed fields hold tagged StoredValue edges. Layout-directed
tracing visits those cells, while checked scalar codecs serve prepared execution
and ValueView-based Host/reflection access.
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
representations, while managed access requires same-heap scoped authority or an
authenticated source/root. Execution and Host-root directories retain their own
checked generations; an opaque managed pointer is never public input authority.
Reachable interfaces and closures retain executable module instances through graph
edges. Ordinary data layouts retain immutable type metadata without retaining the
generation's mutable module state. Selected generic operations, parent scopes and
cached method applications contribute executable edges through their environments.

Explicit reflection resolves a field name through the retained layout metadata
and uses the same slot write checks. Reflection cannot bypass read-only fields
or scalar representation checks. Named `StructValueField` records exist only in
diagnostic snapshots, not heap storage or allocation APIs.

Runtime separates scoped access, traced storage and escaping Host retention.
`ScopedValue<'gc>` contains same-scope managed access or inline scalar bits;
`StoredValue` contains private Edge storage or inline bits; `RootedValue` owns
actual Host retention where managed data must survive across collecting calls.
`ValueView<'view>` exposes protected pointer-free observations through a current
Runtime/NativeCall. Stored edges are not Copy, public constructors cannot forge a
managed tag, and copying a view/coordinate does not create a root.

The physical categories include scalars, String/tuple/range/fixed array/enum/struct,
Native identity, closure/cell/interface, Host root/path and ephemeral descriptors.
There are no privileged Vec/Map/Set value families. Scalar banks use eight-byte
payloads plus initialization flags; traced cells use StoredValue. Layout sizes
are implementation observations, not portable ABI guarantees. Physical GC kind
and script nominal type are separate admission requirements.

Strings and tuple membership are immutable. Updating a tuple creates replacement
membership, sharing referenced children; mutation of an aliased mutable child
remains visible through all aliases. String equality/hashing uses contents,
independently of storage identity. Internal carrier equality describes transport
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

Runtime's ManagedMemory owns one `kagari_gc::RootDriver<ExecutionRoots>`. ResourceState,
semantic lookup, installed Host authority and execution policy remain Runtime
responsibilities. The collector knows only its own allocations, ordinary edges,
Host root leases and a complete driver-supplied source. Rust Host borrows stay
outside the managed heap.

### Managed graph and access

The collector is precise, nonmoving, nongenerational incremental mark-sweep.
Object headers/descriptors establish physical identity; branded Gc access is
available only inside a paired memory/source scope. Private Edge carriers express
relationships in actual providers and execution storage. Scope reads/mutations
retain terminal-state, physical representation and publication checks. Language
nominal typing, declared access and Host capability validation remain Runtime-owned.
No arbitrary Rust locals are scanned.

All active and parked operands, conversion locals, queued factories, pending
results/waits and executable dependencies are in ExecutionRoots. Scalar cells
need no managed edge; managed cells initialize to Unit. FrameStorage owns one
linear suffix mark, and FrameSlots names its checked bank ranges. Full execution
owner/index/generation checks precede cross-activation access. Backing growth
preserves coordinates; retirement clears edges before inaccessible backing reuse.
There is no mirrored RootSet per frame or independent Window identity directory.

Ordinary GcObject providers represent script data, Native identities/payloads,
closures/Futures/Tasks, interfaces/applications/environments, revisions and statics.
Private payload fields use the same physical scanner as ordinary GC. Current and
staged publications plus real Host leases seed the graph; active sources retain
exact admitted revisions. Lookup indexes and immutable Arc code pins do not replace
managed edges. A rootless object/closure/environment/static cycle is collectible,
including cross-installation cycles. R13's separate type/layout provenance can
retain immutable code; no immediate per-body unloading guarantee is implied.

Public invalid/foreign/stale inputs fail admission without mutating the target.
Corruption in an admitted graph or provider failure quarantines Runtime. Collection
and arbitrary callbacks/reentry require ending source, frame and payload loans;
short operation scopes reacquire them afterward. A managed result must be published
to its destination or genuine Host root before old protection is retired. Closed
return/field transitions perform no callback or collection in that handoff.

### Collection, pressure and failure

`Runtime::gc_safepoint` polls execution and services collection only at an admitted
boundary. `GcHeapConfig.collection_threshold` defaults to 1,024 logical units and
`retained_byte_threshold` to 64 KiB. Either committed-pressure threshold can request
service. Active incremental cycles continue through later boundaries; after
completion the next thresholds use the configured floor or twice live occupancy,
with checked/saturating arithmetic. The byte floor is policy, not a measured optimum.
`collection_threshold: None` disables automatic service, not explicit collection.
Runtime allocation/mutation access does not itself run arbitrary collection.

`Runtime::collect_garbage` completes an active cycle if necessary and always runs
a fresh cycle, then performs admitted retirement/cache maintenance. Its report
contains completed cycles, work, detached owners, elapsed time and included idle
maintenance. It does not report the removed per-metadata-store sweep categories.
A normal 1,024-unit allowance and 32-detached-owner cap bound declared work/owners,
not arbitrary provider, allocator or Rust destructor wall time. Total operation
latency includes its synchronous disposal suffix.

Traced state is detached before Rust destruction, and every owner is disposed once.
Provider trace faults cannot sweep a partially established graph. Destructor faults
quarantine while remaining detached cleanup continues. Faulted collector state
permits shutdown only; Runtime has no reset/recovery API. Completed script/Host
effects remain committed. Process abort or a second panic during unwinding remains
outside supported recovery.

### Accounting and observations

Collector HeapStats reports actual object count, provider units, managed/indirect
backing, workspace, registrations, completed cycles and pacing observations.
ResourceCounters combines object count, provider logical units and active conversion
preparation into u128 heap-unit occupancy/peaks. Provider indirect backing is
separate from shallow pages, directories, code, external Rust state and RSS.

ObjectCharge excludes shallow bytes and referenced managed objects. Concrete
payload GcObject implementations own complete tracing and committed indirect
capacity; NativePayload selects registration, not a second trace/accounting model.
Strings use capacity; fixed arrays use physical element width and actual capacity;
managed field/capture/argument buffers account their owned backing. Failed preflight
changes no committed charge; replacements publish/account their new backing before
old ownership is released. Runtime attributes each closed access/service interval,
including growth-then-shrink peaks, before another session or observer executes.
Read-only counters neither scan/reclaim storage nor reset peaks.

`execution_usage` additionally observes actual initialized/reserved source, frame,
uncommon and collector/control capacity. VM max_stack_bytes charges committed
per-execution storage, not total process memory. Opt-in global allocator diagnostics
measure owned requests and event peaks; those are distinct from provider accounting,
RSS and internal realloc copy coexistence. See [measured boundaries](../performance-baseline.md#rr05-allocator-event-peaks-and-complete-diagnostic-lifetimes).

### Host roots, payloads and borrowed state

RootedValue owns a genuine collector Root for managed values, while scalars remain
inline. Clones share the same root registration. Final lease Drop queues a
pre-reserved release notification without borrowing Runtime or recursively destroying
objects. Service authenticates ownership/generation and drains notices; reported
registrations may include dropped leases awaiting service. Late drops after heap
shutdown are inert. A view, type snapshot or unchecked numeric ID grants no root.

`Opaque<T>` retains a checked managed Native identity; cloning preserves identity
without cloning T. Read/edit acquires short checked loans. Fixed Rust data and
traced managed fields have separate providers. Collection destroys unreachable
payloads exactly once; shutdown releases Runtime-owned storage even if public
leases survive. There is no payload extraction/take capability.

External Ref/RefMut reserves Host-owned Rust state under invariant scope authority.
Managed descriptors contain checked views, never unrestricted Rust references.
Scope exit revokes all children before the Rust loan ends on success, trap,
cancellation or supported unwind. Such views cannot escape to durable managed
fields, closure captures or suspension. Native/Host registrations and factories
are Send + Sync; Runtime-owned payloads and observers need Send. Runtime itself is
Send, not Sync, and transfers only between inactive execution/borrow scopes.

### Executable state and publication

RevisionState, static cells, environments, applications, interface/callable state
and constants participate in the same managed graph. Semantic keys remain checked
Runtime identities; they must resolve through an actual publication/source/root
before physical access. An immutable descriptor or detached diagnostic copy cannot
resurrect released state. Type-only metadata stays readable without granting execution.

Runtime static reads/writes require exclusive access, actual publication/source
protection, Ready state, declared mutability, bounds and full nominal/storable
validation. Candidate restrictions remain; failed writes preserve old state.
Snapshots own roots for copied values and cannot edit authoritative cells. Live old
Host descriptors follow current static contracts; admitted frames use their exact
revision's bindings. No cell/payload loan crosses an initializer, callback or GC.

Publication prepares and validates new graph/call facts before visibility, then
commits without user callbacks. Abandoned staging loses authority immediately and
requests later fresh-cycle retirement. Disabled automatic GC leaves disposal to
explicit collection or teardown. Real retained computations preserve selected
bodies/providers/environments, while ordinary inner calls select current code.
Weak lookup caches do not root all historical revisions. The [reload walkthrough](../architecture/runtime/reload.md)
and [activation contract](module-activation.md) own exact selection/commit rules.

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
HostCallGuard<'runtime> {
  table: &'runtime HostBorrowTable,
  resources: Option<&'runtime ResourceState>,
  frame: Option<HostFrameAccess<'runtime>>
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

RuntimeLimits configures nonzero max_stack_bytes (default 8 MiB per execution)
and max_native_depth (default 32 nested Native/compiled entries). Committed VM
capacity and Native/Rust-stack admission have independent guards; see
[execution control](security.md#execution-control) for accounting and platform
boundaries. StackOverflow and AllocationFailure terminate the current execution.
Runtime owns ResourceState beside ManagedMemory; components use scoped borrows,
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
dependency set. Its source prefix retains the admitted root's RevisionState edge,
with control-side retention bookkeeping. Ending the last scope needs no module-store borrow.
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
owns checked source-local, Host-frame and activation marks and registers its frame
identity in the session. Host calls and path callbacks use this scope; cleanup
revokes borrows, retires source locals and ends its admitted activation/session
marks. Only escaping values require genuine Host roots.
Host scopes can outlive an outer session handle
without clearing its cancellation or termination state. ExecutionSession::host_scope_count
reports registered host scopes for diagnostics and cleanup assertions.
VM/SDK execution, collection and publication use exclusive mutable Runtime access.
Read-only observations may use shared access. Checked source/session identities
and scoped loans preserve synchronous reentry without concurrent heap access.
ModuleStore owns version reservation along with publication. Runtime-wide synchronous
activation guards reject publication inside script, observers, callbacks, reentry or
driver cleanup; parked old-version sessions may remain without blocking publication.
Registration and owner replacement still require mutable access; a live execution guard prevents moving
or destroying the runtime. Transfer is possible once these scopes have ended.
Frames own their immutable loaded version; no borrowed bytecode lifetime crosses
runtime entry. Frames and stacks do not retain a heap owner; slot access receives
the current Runtime and checks its identity before touching the execution source. A foreign
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
are committed. ExecutionCounters reports current and peak call depth and heap units;
a fresh root's heap peak starts at its current Runtime occupancy and does not
inherit a previous root's peak. Nested synchronous entries share the active root's
peak. Reading counters does not sample the platform clock. Optional tracing provides
observations without execution charging.
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
these APIs still enforce candidate restrictions and quarantine. A protected
view is not an external GC root; retaining across collecting calls requires a
genuine RootedValue.

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
