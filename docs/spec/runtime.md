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
  reloads: HotReloadCoordinator,
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

This aligns with `gc.rs`.

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
Kagari does not expose Rust-style `dyn Trait` syntax to scripts.

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
