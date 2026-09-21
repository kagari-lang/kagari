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
- support host capability enforcement and resource accounting
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
  security: SecurityContext,
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
- security context for capabilities and resource policy
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
charging resources. Declared payload structs/enums must match the receiving
version's nominal layout. Standard-enum built-ins reject same-named declared types.
Enum equality compares tags/layouts and then members using script equality, so
mutable members compare by identity. Enum objects retain their executable version
and trace their payloads through the same mark-sweep root mechanism. Publication
does not invalidate old values; rooted old enum objects keep their original layout.

Script struct objects contain a `StructLayoutRef` and positional `Value` slots.
The layout handle comes from a verified `LoadedModule` and retains that immutable
executable generation. Objects do not duplicate field names. Allocation requires
a layout belonging to the receiving runtime and validates field count, payload
storage boundaries and value representations before charging allocation units.
`ResourcePolicy` is the sole source of heap and allocation limits. `GcHeapConfig`
only configures collection scheduling. Runtime and standard-library allocations
and growth update the same counters on successful commit; heap statistics read
those counters directly. There is no explicit accounting synchronization API.
Allocating heap operations return structured runtime errors, including distinct
heap-limit and cumulative-allocation failures. See the
[failure contract](failure-semantics.md#modification-guarantees) for commit order.
Field reads require a matching nominal layout; writes additionally require a
writable slot and matching value representation before changing the target.

Layouts from the same executable generation compare by shared handle and index.
Across generations, access requires the same runtime owner and equal nominal
declaration, field identities, slot order, representations and permissions.
Publication does not invalidate an object's retained layout. Root calls now pin
their executable dependency program. Heap-valued fields still have coarse nominal
representations, while heap references carry a unique heap owner, slot and generation.
Full reachability of interface/capture versions and module instance state remains open.

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

Runtime::collect_garbage includes registered host/frame/debug roots, module slots,
initializer results and pending host-path mutation records. Path-view dynamic arguments
are traced; Rust host objects and borrowed resources remain host-owned. Path-operation
arguments and old/new values are temporarily rooted across host
read/preparation callbacks, including preparation that explicitly collects. Commit
actions only apply prepared host state; they cannot collect or execute scripts.
Register/local
slots stay conservatively rooted until overwritten or their frame is dropped. Trap and
budget failure drop frame roots through the same frame cleanup path.
Commit invariant failures use this cleanup path too, and quarantine the runtime.
Execution, allocation, collection and mutation entry points then reject further
work with EngineFault. There is no reset API; inspecting existing counters and
discarding the runtime remain possible.

GcHeapConfig.collection_threshold schedules automatic collection at instruction
safepoints, including the current scalar JIT helper. Collection never runs in the
middle of a standard mutation. The default minimum threshold is 1024 heap units;
after collection it grows to twice the live units. None disables automatic collection,
while explicit runtime collection remains available. Heap units are accounting units,
not byte measurements. Stats report live/peak units, live objects, collection count,
reclaimed object count and the last pause. Incremental and generational GC remain deferred.

Host retention uses RootedValue, returned by Runtime::root_value. Its clones share a
registered root and the last drop releases it. Value::clone only copies a script value;
it does not keep heap objects alive. RootedValue::set checks the destination heap and
replacement references. The former GcRootId/update/release APIs are removed. Frame
storage uses RootSet with short validated accesses. Root handles and runtime callbacks
are local to one thread; callbacks may capture rooted values without Send/Sync bounds.

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
- capability requirements for host entry points

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

## Security Context

The runtime carries security-relevant execution state.

The model is:

```text
SecurityContext {
  profile: LanguageProfile,
  capabilities: CapabilitySet,
  resources: ResourcePolicy
}
```

This context is the runtime-side anchor for:

- capability checks
- resource limits
- feature-gated runtime behavior

Security rules are defined in [security.md](security.md).

## Resource Accounting

The runtime maintains counters or budgets for:

- instruction steps
- wall-clock or host-supplied time budget
- current and peak heap size
- module count
- call depth

These counters are updated in runtime execution paths, not inferred after the fact.

ExecutionSession owns a root program, immutable execution options, cancellation
and budget baselines. Initializers, entry execution and backend fallback share
these inputs. Instruction, allocation, host-call and reflection limits count usage
since root entry; a subsequent independent call receives its own budget. Runtime
counters remain cumulative. Live heap, dirty-ledger size and loaded-module limits
apply to current occupancy, and collection does not refund root allocation usage.
Host operations outside a session use RuntimeConfig resource defaults directly.

Resource exhaustion remains recorded until the final session scope drops; nested
entries cannot replace inputs or reset the remaining budget. Effective permissions
and host policy come from the active session, even if runtime defaults change.
Nested module entries must belong to its pinned dependency program. ModuleStore
shares its interior state so owned scopes can retain/release versions without a
mutable borrow spanning execution. Synchronous host callbacks reenter the existing
explicit driver with the same runtime and root options. Each host context owns its
borrow guard; outer scopes remain active during nested execution. The session owns
one ExecutionFrame stack, including interpreter, nested callbacks and VM native
entry scopes. ExecutionStack guards remember their stack base and unwind only their
own suffix, releasing roots and depth counters even after termination/quarantine.
Manual public call-depth entry/exit APIs are removed; only frame scopes update it.
HostResourceScope registers host leases and temporary roots in the same session.
Host calls and path callbacks use this scope, and cleanup removes its registration
before dropping the session handle. Host scopes can outlive an outer session handle
without resetting its budget or permissions. ExecutionSession::host_scope_count
reports registered host scopes for diagnostics and cleanup assertions.
Frames own their immutable loaded version; no borrowed bytecode lifetime crosses
runtime entry. Invalid frame access, suspended-scope mutation and out-of-order
scope destruction quarantine the runtime instead of resuming a damaged stack.

The root observer receives the complete stack at instruction and trap boundaries.
It cannot be replaced by nested execution or first installed while frames are
running. Observations hold short immutable stack borrows and must not invoke script
execution. The VM uses this boundary for shared debugger events during host reentry.

Cancellation and an optional monotonic wall-time budget are checked cooperatively
at instruction safepoints and before resource-consuming operations. They cannot
preempt a blocking host callback. A prepared commit is uninterrupted: cancellation
requested inside it is observed after its target and dirty record are committed.
ExecutionCounters reports root activity, root peaks and elapsed wall time; the
unused wall-time field in cumulative ResourceCounters is removed. These operational
deadlines do not expose a script clock or complete the deterministic-context work.

## Module Store

The runtime distinguishes loaded module code from the compilation pipeline.

The model is:

```text
ModuleStore {
  loaded: Map<ModuleName, LoadedModule>
}

LoadedModule {
  name: ModuleName,
  epoch: ModuleEpoch,
  ir: IrModule,
  state: ModuleRuntimeState
}
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

- denied capability checks
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
- capability context
- module epochs

This supports the current language model without locking in a highly complex VM object model.

## Implementation Order

The incremental implementation order is:

1. strengthen the `Value` model around script values versus host values
2. add a runtime `TypeRegistry`
3. extend `HostRegistry` with host type metadata
4. add `HostCallGuard` and `BorrowTable`
5. integrate capability checks into host entry points
6. connect module epochs and stale-handle checks
7. grow reflection and interface values on top of the shared type registry

This order lets the runtime stay coherent while each subsystem is added with a clear responsibility boundary.
