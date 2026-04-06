# Kagari Runtime Model Draft

This document describes a proposed runtime model for Kagari.
It is a design draft, not a finalized implementation contract.

The goal is to unify the runtime-facing concepts that appear across the syntax, trait, reflection, security, and host-interop documents.

Execution model direction is drafted separately in [execution.md](/Users/mikai/CLionProjects/kagari/docs/spec/execution.md).
Backend abstraction direction is drafted separately in [codegen-backend.md](/Users/mikai/CLionProjects/kagari/docs/spec/codegen-backend.md).

## Design Goals

- define a coherent runtime object model for script-owned and host-owned values
- support GC-managed script values and frame-scoped host borrows in one runtime
- share runtime type identity across reflection, `dyn Trait`, and downcast
- support host capability enforcement and resource accounting
- support hot reload without binding the runtime directly to AST details

## Existing Direction

The current runtime crate already sketches the main system boundaries:

- [lib.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-runtime/src/lib.rs)
- [value.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-runtime/src/value.rs)
- [gc.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-runtime/src/gc.rs)
- [host.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-runtime/src/host.rs)
- [reload.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-runtime/src/reload.rs)

This draft builds on those boundaries rather than replacing them.

## Top-Level Runtime Structure

A useful conceptual runtime structure is:

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

The exact field layout may differ, but these responsibilities should stay distinct.

## Core Runtime Subsystems

The runtime should be organized around these subsystems:

- GC heap for script-owned objects
- type registry for runtime type identity and metadata
- host registry for exported functions and types
- security context for capabilities and resource policy
- reload coordinator for module epochs
- module store for loaded code units and runtime module state

## Value Model

Kagari values should be split into two broad categories:

1. storable script values
2. frame-scoped ephemeral values

This distinction is important for host interop and safety.

### Storable Script Values

Storable values may be placed in:

- locals
- GC object fields
- globals
- closure environments
- return values

These include:

- primitive scalars
- GC handles
- script-owned aggregate values
- dynamic trait objects, if represented as storable heap values

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

## Recommended Value Shape

The current [value.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-runtime/src/value.rs) file already separates script-owned handles from host-backed handles.

A future conceptual shape could be:

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
  DynHandle(DynObjectId),
  HostOwned(HostObjectId),
  Ephemeral(EphemeralValueId)
}
```

The exact enum does not matter as much as preserving:

- clear separation between GC-managed and non-GC-managed values
- a representation for dynamic interface objects
- a representation for frame-scoped host borrows

One important consequence is that Kagari does not currently require a runtime notion of "read-only heap object" just to support `const`.
If a future design needs shared frozen objects, that should be modeled explicitly rather than folded into ordinary `const` items.

## GC Heap

The GC heap is responsible for script-owned memory.

Its responsibilities include:

- object allocation
- object tracing and reclamation
- heap accounting
- integration with runtime resource limits

The GC should not own Rust host borrows.

This aligns with the existing design direction in [gc.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-runtime/src/gc.rs).

## Type Registry

The runtime should contain a unified type registry.

This registry should back:

- reflection
- runtime type checks
- `dyn Trait`
- downcast
- host type registration

Conceptually:

```text
TypeRegistry {
  by_id: Map<TypeId, TypeInfo>,
  by_name: Map<String, TypeId>
}
```

Each `TypeInfo` should carry at least:

- `TypeId`
- name
- kind
- field metadata
- variant metadata
- method metadata
- implemented trait metadata

Reflection direction is drafted in [reflection.md](/Users/mikai/CLionProjects/kagari/docs/spec/reflection.md).

## Dynamic Trait Objects

`dyn Trait` values should be modeled as runtime interface objects.

Conceptually:

```text
DynObject {
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

Trait-system direction is drafted in [traits.md](/Users/mikai/CLionProjects/kagari/docs/spec/traits.md).

## Host Registry

The host registry should manage:

- exported host functions
- exported host types
- parameter passing metadata
- capability requirements for host entry points

This extends the current shape in [host.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-runtime/src/host.rs).

Conceptually:

```text
HostRegistry {
  functions: Map<Symbol, HostFunction>,
  types: Map<TypeId, HostTypeInfo>
}
```

## Host Call Frames

Host-to-script and script-to-host calls that involve borrowed host data should create explicit call frames.

Conceptually:

```text
HostCallGuard {
  frame_id: FrameId,
  borrow_table: BorrowTable
}
```

This frame owns the validity of all borrowed host handles created during the call.

## Borrow Table

The borrow table is responsible for preserving Rust aliasing rules at the interop boundary.

It should track:

- which host object ids are currently borrowed
- whether the borrow is shared or unique
- which frame owns the borrow

Conceptually:

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

This table should reject:

- multiple simultaneous unique borrows of the same object
- a unique borrow while any shared borrow is active
- a shared borrow while a unique borrow is active

## Host Borrow Handles

Borrowed host values should be represented explicitly.

Conceptually:

```text
HostBorrowHandle {
  frame_id: FrameId,
  object_id: HostObjectId,
  type_id: TypeId,
  borrow_kind: Shared | Unique,
  epoch: BorrowEpoch
}
```

These handles are:

- valid only during their owning frame
- non-storable in GC-managed objects
- rejected at suspension boundaries

Host interop direction is drafted in [host-interop.md](/Users/mikai/CLionProjects/kagari/docs/spec/host-interop.md).

## Security Context

The runtime should carry security-relevant execution state.

Conceptually:

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

Security direction is drafted in [security.md](/Users/mikai/CLionProjects/kagari/docs/spec/security.md).

## Resource Accounting

The runtime should maintain counters or budgets for:

- instruction steps
- wall-clock or host-supplied time budget
- current and peak heap size
- module count
- call depth

These counters should be updated in runtime execution paths, not inferred after the fact.

## Module Store

The runtime should distinguish loaded module code from the compilation pipeline.

Conceptually:

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

The exact execution format may later diverge from raw IR, but the runtime should keep the concept of a loaded module with versioned identity.

## Hot Reload

Hot reload should be coordinated through explicit module epochs.

The existing direction in [reload.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-runtime/src/reload.rs) is already compatible with this.

The runtime should be prepared to use epochs for:

- module version tracking
- stale handle detection
- metadata comparison across reloads
- state migration tooling

## Suspension and Ephemerality

If Kagari later adds suspension points such as `yield` or `await`, the runtime should distinguish:

- suspendable values
- non-suspendable ephemeral values

Borrowed host handles should be explicitly non-suspendable.

This means:

- a frame with live borrowed host handles may not be suspended
- runtime stack snapshots must reject non-suspendable values

## Runtime Errors vs Engine Bugs

The runtime should classify failures clearly.

Script/runtime errors include:

- denied capability checks
- invalid reflective writes
- use of an expired host borrow
- resource limit violations

Engine bugs include:

- internal invariant violations
- invalid unchecked access to stale handles
- corrupted runtime bookkeeping

This distinction should guide whether the runtime reports a script trap or panics internally.

## Recommended v1 Runtime Slice

The first practical runtime version should include:

- primitive values
- GC handle values
- host function registry
- explicit host passing styles
- frame-scoped host borrow handles
- type registry with stable `TypeId`
- capability context
- module epochs

This is enough to support the current language-design direction without prematurely locking in a highly complex VM object model.

## Recommended Implementation Order

If implemented incrementally, the recommended order is:

1. strengthen the `Value` model around script values versus host values
2. add a runtime `TypeRegistry`
3. extend `HostRegistry` with host type metadata
4. add `HostCallGuard` and `BorrowTable`
5. integrate capability checks into host entry points
6. connect module epochs and stale-handle checks
7. grow reflection and `dyn Trait` on top of the shared type registry

This order lets the runtime stay coherent while each subsystem is added with a clear responsibility boundary.
