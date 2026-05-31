# Kagari Typed Path Mutation Specification

This document defines the model for mutating host-owned structured data through ordinary Kagari field and index syntax.

Runtime behavior is defined in [runtime.md](/Users/mikai/CLionProjects/kagari/docs/spec/runtime.md).
Host interop is defined in [host-interop.md](/Users/mikai/CLionProjects/kagari/docs/spec/host-interop.md).
Execution behavior is defined in [execution.md](/Users/mikai/CLionProjects/kagari/docs/spec/execution.md).
Bytecode behavior is defined in [bytecode.md](/Users/mikai/CLionProjects/kagari/docs/spec/bytecode.md).

## Purpose

Typed path mutation is the mechanism that lets Kagari scripts write natural game-logic code over Rust-owned data without exposing Rust references, lifetimes, or borrow checking to script authors.

Example:

```kagari
player.info.level = 12
player.inventory.items[item_id].count -= 1
target.combat.hp -= damage
```

These expressions look like ordinary nested field mutation.
For host-owned values, their semantic meaning is a checked mutation of a typed path rooted at a host object.

## Design Goals

- allow ordinary field and index syntax for host-owned state
- keep field access statically typed where host metadata is available
- avoid exposing raw Rust references to scripts
- avoid requiring script-visible Rust borrow rules
- allow scripts to keep multiple nested views into the same host object graph
- support dirty tracking, validation, persistence updates, and event hooks
- support hot reload through schema and path metadata
- keep the execution model compatible with both the interpreter and future JIT backends
- avoid unnecessary allocation for nested host-backed field access

## Non-Goals

Typed path mutation does not provide:

- raw `&T` or `&mut T` values to script code
- long-lived Rust borrows stored in Kagari locals, globals, closures, or GC objects
- reflection-based field lookup for ordinary game logic
- stringly typed mutation paths in compiled code
- automatic mutation access to every field of every host type
- a replacement for host-side authorization, validation, or persistence policy

## Core Model

For host-owned structured data, a field or index chain is compiled into a typed path.

The source expression:

```kagari
player.inventory.items[item_id].count -= 1
```

is not modeled as:

```text
borrow player mutably
borrow player.inventory mutably
borrow player.inventory.items[item_id] mutably
borrow player.inventory.items[item_id].count mutably
```

It is modeled as:

```text
ModifyPath(
  root = player,
  path = inventory.items[item_id].count,
  op = SubAssign,
  value = 1
)
```

The host or runtime applies the mutation to the Rust-owned data structure through a registered, checked access path.

## Terminology

### Host Root

A host root is a script-visible handle to a host-owned object.

Examples:

- `PlayerRef`
- `EntityRef`
- `BattleRef`
- `WorldRef`
- `HostObjectId`

A host root is not owned by the Kagari GC.
It identifies data whose lifetime and storage are controlled by the Rust host.

### Typed Path

A typed path is a statically resolved sequence of field and index steps from a host root to a target value.

Conceptually:

```text
TypedPath {
  root_type: TypeId,
  result_type: TypeId,
  segments: [PathSegment],
  access: ReadOnly | ReadWrite,
  schema_epoch: SchemaEpoch,
  path_id: PathId
}
```

Path segments may include:

- named struct fields
- tuple or positional fields, if the language supports them
- map/list/index steps
- host-defined virtual fields
- host-defined computed views, if registered as path-accessible

Field names are resolved during compilation or module loading.
Compiled code refers to compact path identifiers or typed descriptors, not to string arrays.

For source-level assignment, the target field must be writable according to Kagari field policy.
Fields declared with `val` are read-only after initialization and fields declared with `var` are writable.
Host-backed fields must satisfy both the source-level writable-field rule and the host registration policy.

### Host Path View

A host path view is a script-visible value that represents a root plus a base typed path.

Example:

```kagari
val combat = player.combat
combat.hp -= damage
```

Conceptually:

```text
combat = HostPathView(root = player, base_path = combat)

ModifyPath(
  root = player,
  path = combat.hp,
  op = SubAssign,
  value = damage
)
```

A host path view is not a Rust reference.
It does not keep a Rust field borrowed.
It preserves enough root and path identity for later reads and writes to be checked and executed by the host.

## Script Semantics

Field and index syntax has one of two semantic meanings depending on the base value:

1. For script-owned Kagari values, it accesses ordinary script fields or elements according to the script object model.
2. For host-backed roots or host path views, it extends a typed host path.

Assignment to a host-backed place lowers to a path mutation operation.

Examples:

```kagari
player.info.level = 12
player.combat.hp += 100
player.inventory.items[item_id].count -= 1
```

Representative lowering:

```text
SetPath(root = player, path = info.level, value = 12)
ModifyPath(root = player, path = combat.hp, op = AddAssign, value = 100)
ModifyPath(root = player, path = inventory.items[item_id].count, op = SubAssign, value = 1)
```

Reading from a host-backed path lowers to a typed read operation.

```kagari
val level = player.info.level
```

Representative lowering:

```text
ReadPath(root = player, path = info.level) -> i32
```

## Local Nested Views

Kagari allows nested host-backed values to be assigned to locals as path views.

Allowed:

```kagari
val info = player.info
val combat = player.combat

info.level = 12
combat.hp -= damage
```

This must not create detached Rust references.

Conceptually:

```text
info = HostPathView(root = player, base_path = info)
combat = HostPathView(root = player, base_path = combat)
```

Host path views may be copied as ordinary small values inside one VM isolate.
Copying a view copies root and path identity; it does not duplicate or borrow the underlying Rust data.

Storage of host path views in GC-managed objects, globals, or closures is controlled by the host and language profile.
Initial rule:

- host roots and host path views may be stored in locals
- storage in long-lived script objects, globals, or closures is disabled by default
- embeddings may opt into longer-lived handles only when they can validate host object lifetime and reload compatibility

## Static Validation

Typed path mutation is validated before execution whenever host metadata is available.

The compiler or module loader rejects:

- missing fields
- missing index accessors
- writes to read-only paths
- value types that do not match the path result type
- mutation through a root type that does not expose the requested path
- public hot reload entry points that depend on unstable host path ABI

The validation result is represented in typed IR or bytecode as a resolved path descriptor.

## Runtime Validation

Runtime checks are still required because host state may change independently of compiled script code.

Runtime path operations validate:

- root handle validity
- root object epoch or generation, if tracked
- schema epoch or path ABI compatibility
- capability and host policy requirements
- dynamic index validity
- host-side invariants and validation hooks

Failures are reported as script/runtime errors unless they indicate a Kagari implementation invariant violation.

## Performance Model

Typed path mutation must avoid runtime reflection in ordinary field access.

Compiled path operations use compact descriptors such as:

- integer `PathId`
- host-generated enum variants
- interned path descriptors
- static field metadata plus dynamic operands

Compiled code does not repeatedly construct string paths such as:

```text
["inventory", "items", item_id, "count"]
```

Instead, dynamic components are passed separately from the static path identity.

Conceptually:

```text
ModifyPath(
  root = player,
  path_id = PlayerPath::InventoryItemsCount,
  dynamic_args = [item_id],
  op = SubAssign,
  value = 1
)
```

### Descriptor Execution

The baseline implementation may execute typed paths through a compact path descriptor.
This keeps code size controlled for large Rust structures and deep paths.

Conceptually:

```text
PathDescriptor {
  root_type: Player,
  result_type: i32,
  segments: [
    Field(inventory),
    Field(items),
    Index(dynamic_arg_0),
    Field(count)
  ],
  access: ReadWrite
}
```

This descriptor is typed and validated.
It is not string reflection.

### Specialization

The runtime and future JIT may specialize frequently executed paths.

Specialization tiers:

1. execute cold paths through generic typed descriptors
2. cache path descriptors and host adapter lookups
3. use direct adapter function pointers for hot paths
4. let a future JIT emit specialized fast paths when profitable

Typed path mutation must not require eager code generation for every possible deep field path.
Large host structures remain practical by defaulting to compact descriptors and specializing only observed or declared hot paths.

## Code Size Guidance

Host binding generators avoid generating a full accessor or mutator for every possible deep path in a complex Rust object graph.

Strategies:

- generate metadata per registered type and field
- generate small per-field or per-type adapters
- compose deep paths through typed descriptors
- generate full-path adapters only for explicitly registered or observed hot paths

This keeps the host integration scalable while preserving a clear optimization path.

## Dirty Tracking and Host Hooks

Path mutation provides enough structured information for the host to react to changes.

Host-side effects may include:

- marking dirty fields
- generating persistence updates
- emitting game-domain events
- syncing client state
- recording audit or debug logs
- running validation hooks

Example mutation record:

```text
MutationRecord {
  root: player,
  path: inventory.items[item_id].count,
  op: SubAssign,
  old_value: optional,
  new_value: optional
}
```

Old-value capture is controlled by host policy because it may affect performance.

## Relationship to Host Borrowing

Typed path mutation is distinct from frame-scoped host borrowing.

Frame-scoped host borrows are runtime or host ABI values used when entering functions that require temporary `&T` or `&mut T` access.
They are non-storable and must not escape their call frame.

Typed path mutation is the normal mechanism for script-visible nested field access over host-owned state.
It does not expose or store Rust borrows.

This split preserves Rust-side safety without making Rust's borrow checker part of the script language.

## Relationship to GC

The Kagari GC does not trace into host-owned object graphs.

Host roots and host path views are non-owning references from the GC's point of view.
If they are allowed in storable script values, the runtime must treat them as handles and validate host-side liveness before use.

Intermediate host-backed field views do not allocate long-lived proxy objects by default.
Simple view values are representable as compact root-plus-path handles.

## Relationship to Hot Reload

Typed paths are part of the host/script ABI.

Hot reload validation compares enough metadata to determine whether previously compiled path operations remain valid.

Relevant metadata includes:

- root type identity
- field or accessor identity
- result type
- read/write access policy
- dynamic index shape
- schema epoch or ABI fingerprint

If a reload changes a registered host path incompatibly, the new module fails validation or requires an explicit migration/compatibility rule.

Existing active calls continue using the module epoch they started with.
New calls use the latest successfully published module and its validated path metadata.

## Relationship to Security

Host path access is a host-controlled capability.

The host defines:

- which root types are visible
- which fields are readable
- which fields are writable
- which paths require capabilities
- which paths are unavailable in restricted profiles
- which validation hooks run before or after mutation

Path mutation must not bypass host API exposure, language profile checks, capability checks, or resource policy.

## IR and Bytecode Contract

The frontend lowers host-backed field and index chains into typed path operations before bytecode execution.

Useful IR or bytecode-level operations include:

- `ReadPath`
- `SetPath`
- `ModifyPath`
- `MakePathView`

The exact opcode names may differ.
The important requirement is that the execution layer can distinguish:

- ordinary script aggregate access
- host-backed typed path read
- host-backed typed path mutation
- construction of a lightweight path view

Future JIT backends call the same runtime helper or host adapter surface used by the interpreter unless a specialized fast path has been proven equivalent.

## V1 Feature Set

The first usable typed path mutation version includes:

- host root handles
- host type metadata for selected fields
- typed path descriptors
- read path
- set path
- compound assignment path mutation
- local host path views
- read-only path rejection
- dynamic index arguments for registered indexable fields
- runtime capability and liveness checks
- dirty path reporting hooks

## V1 Exclusions

Typed path mutation excludes:

- runtime string-based field mutation as the ordinary access model
- arbitrary reflection-based mutation
- storing long-lived mutable Rust references in script values
- eager full-path code generation for every possible deep path
- mutation of unregistered host fields
- cross-thread access to one mutable host-backed script view without host coordination

## Implementation Order

Incremental implementation order:

1. register host root types and selected fields
2. resolve field and index chains into typed path descriptors
3. lower simple reads and writes into `ReadPath` and `SetPath`
4. add compound assignment through `ModifyPath`
5. add local `HostPathView` values
6. add dirty tracking and validation hooks
7. add descriptor caching and direct host adapter dispatch
8. add hot path specialization only after baseline semantics are stable
