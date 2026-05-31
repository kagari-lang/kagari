# Kagari Runtime Metadata and Reflection

This document defines Kagari's runtime metadata and optional reflection model.

The main goal is to provide the metadata needed by the compiler, VM, GC, host integration, tooling, and hot reload without turning ordinary Kagari code into a dynamic reflection-driven language.

Security rules are defined separately in [security.md](security.md).
Host interop rules are defined separately in [host-interop.md](host-interop.md).
Runtime model rules are defined separately in [runtime.md](runtime.md).
Typed path mutation rules are defined separately in [typed-path-mutation.md](typed-path-mutation.md).

## Core Position

Kagari needs runtime metadata.
Kagari does not require general script-visible runtime reflection as part of the core game-logic programming model.

Ordinary Kagari code uses:

- static types
- ordinary field and method access
- traits or interfaces
- generated registration tables
- typed path mutation for host-owned state
- explicit host APIs

Ordinary game logic does not depend on:

- string-based field lookup
- reflection-based field writes
- dynamic method invocation through reflection
- reflection as the trait dispatch mechanism

Reflection may exist as an optional, profile-gated capability for debugging, editor tools, diagnostics, inspection, migration tools, and privileged host-controlled workflows.

## Design Goals

- provide stable runtime type identity
- support GC tracing and value layout metadata
- support host type and function registration
- support typed path validation and hot reload compatibility checks
- support debugger, editor, and GM tooling inspection
- share type identity with trait/interface dispatch and downcast
- keep ordinary script execution statically typed and predictable
- keep host reflection opt-in and capability-gated

## Non-Goals

The core reflection model does not provide:

- unrestricted script-visible runtime reflection
- reflection-based mutation as the ordinary field write model
- automatic reflection for every host type
- unrestricted dynamic method invocation
- full generic-instantiation reflection
- compile-time metaprogramming through runtime reflection
- access to private or non-registered host state

## Metadata Layers

Kagari distinguishes four related but separate layers.

### Internal Runtime Metadata

Internal runtime metadata is required.
It is used by Kagari implementation components rather than ordinary scripts.

Examples:

- `TypeId`
- object layout metadata
- field and variant metadata
- method and trait/interface metadata
- GC trace descriptors
- host type registration metadata
- typed path descriptors
- ABI fingerprints
- module epoch metadata

This layer is part of the runtime contract even when script-visible reflection is disabled.

### Compile-Time Metadata

Compile-time metadata comes from declarations, attributes, and host binding descriptions.

Examples:

```kagari
@handler(LoginRequest)
pub fn on_login(ctx: GameCtx, req: LoginRequest) -> Result<()> {
    ...
}

@persist("player")
pub struct PlayerState {
    id: PlayerId
    level: i32
}
```

This metadata may generate:

- handler registration tables
- persistence schemas
- validation tables
- typed path tables
- editor schemas
- hot reload ABI fingerprints

Compile-time metadata is not the same as script-visible runtime reflection.

### Host and Tooling Introspection

Host and tooling introspection may inspect registered metadata.

Examples:

- debugger object inspectors
- editor property panels
- schema generation tools
- hot reload compatibility reports
- migration tools
- GM tools with host-granted permissions

This layer may expose richer metadata than ordinary scripts can access.
It is controlled by host policy and capabilities.

### Script-Visible Reflection

Script-visible reflection is optional.
It is disabled or restricted in the default game-logic profile unless an embedding explicitly enables it.

When enabled, it starts with read-oriented metadata inspection.
Mutation and dynamic invocation are separate privileged capabilities, not implied by basic reflection.

## Core Metadata Model

The central metadata object is `TypeInfo`.

Conceptually:

```text
TypeInfo {
  id: TypeId,
  name: String,
  kind: TypeKind,
  fields: [FieldInfo],
  variants: [VariantInfo],
  methods: [MethodInfo],
  traits: [TraitInfo],
  abi_fingerprint: AbiFingerprint
}
```

The layout is implementation-defined.
The important capabilities are:

- stable type identity within a module epoch
- structural metadata for registered fields and variants
- method and trait/interface metadata
- layout information for GC and VM use
- comparison data for hot reload validation
- host registration and path validation support

## Type Identity

Runtime type identity is unified.

The same concrete type identity is used by:

- runtime metadata lookup
- trait/interface dispatch metadata
- `is<T>`
- `downcast<T>`
- host type registration
- typed path root and result validation
- hot reload compatibility checks

Type identity rules:

- every runtime object or host-registered value has a concrete `TypeId`
- `TypeId` values are stable within a module epoch
- cross-epoch compatibility is determined through fingerprints or explicit compatibility rules
- downcast succeeds by comparing concrete runtime type identity

This prevents the runtime from growing multiple incompatible type identity systems.

## Type Kinds

The metadata system distinguishes at least:

- primitive
- tuple
- array or list
- map
- struct
- enum
- function
- trait or interface
- dynamic interface object, if represented separately
- host object
- host path view

This does not require exposing an exhaustive user-facing enum.
The runtime and tooling layers need a clear internal classification.

## Field Metadata

Field metadata is useful for validation, tooling, hot reload, and typed path mutation.

Conceptually:

```text
FieldInfo {
  id: FieldId,
  name: String,
  ty: TypeId,
  readable: bool,
  writable: bool,
  visibility: Visibility,
  path_access: None | ReadOnly | ReadWrite,
  abi_fingerprint: AbiFingerprint
}
```

Field metadata supports:

- static field existence checks
- type checking
- field visibility diagnostics
- GC layout and tracing
- serialization and persistence schemas
- typed path descriptor construction
- hot reload layout comparison

For host-owned state, field metadata does not imply that scripts receive Rust field references.
Host-backed field access uses typed path mutation when it is exposed to ordinary Kagari code.

## Variant Metadata

Enum metadata exposes enough information for tooling, pattern validation, serialization, and hot reload.

Conceptually:

```text
VariantInfo {
  id: VariantId,
  name: String,
  fields: [FieldInfo],
  tag: VariantTag,
  abi_fingerprint: AbiFingerprint
}
```

The runtime may use variant metadata for:

- active variant inspection in tooling
- serialization helpers
- migration checks
- debugger displays
- pattern-match validation support

## Method Metadata

Method metadata is descriptive.

Conceptually:

```text
MethodInfo {
  id: MethodId,
  name: String,
  params: [ParameterInfo],
  return_type: TypeId,
  origin: MethodOrigin,
  capability_requirements: CapabilitySet,
  abi_fingerprint: AbiFingerprint
}
```

Method metadata is useful for:

- editor completion
- documentation tooling
- host registration
- ABI validation
- trait/interface dispatch table construction

Metadata about a method does not imply unrestricted dynamic invocation of that method.

## Relationship to Typed Path Mutation

Typed path mutation is not runtime reflection.

For ordinary game logic, this source:

```kagari
player.info.level = 12
```

lowers to a checked typed path operation, not to:

```text
type_of(player).field("info").field("level").set(player, 12)
```

Reflection metadata may help build or validate path descriptors, but execution uses resolved `PathId` values and host adapters rather than string field lookup.

This distinction is important for:

- static type checking
- performance
- dirty tracking
- host validation
- hot reload compatibility
- clear security boundaries

## Optional Script Reflection

If an embedding enables script-visible reflection, the smallest useful surface is read-oriented.

Restricted API:

```kagari
val ty = type_of(value)
val name = ty.name()
val kind = ty.kind()
val fields = ty.fields()
```

The baseline script-visible API includes:

- `type_of(value) -> TypeInfo`
- `TypeInfo.id()`
- `TypeInfo.name()`
- `TypeInfo.kind()`
- metadata iteration over registered fields, variants, methods, and traits

This API returns metadata objects.
It does not imply ordinary dynamic field access or mutation.

## Reflective Reads

Reflective reads are optional and profile-gated.

When provided, reflective reads:

- only access members registered for reflection
- check runtime capabilities
- perform runtime type checks
- respect host visibility and exposure policy
- return result-like errors rather than panicking on normal misuse

Reflective reads are useful for:

- debuggers
- inspectors
- editor previews
- diagnostic logging
- privileged tools

Reflective reads do not replace ordinary typed field access in game logic.

## Reflective Writes

Reflective writes are not part of the default script reflection surface.

If an embedding provides reflective writes for privileged tooling, they must be separately gated from metadata reads.

Reflective writes require:

- an enabled language profile feature
- a runtime capability such as `reflection_write`
- host type opt-in
- field-level write exposure
- runtime type checking
- host validation hooks
- audit or diagnostics support when appropriate

Reflective writes must not bypass typed path mutation policy for ordinary game state.
For host-owned state, ordinary script mutation continues to use typed path operations.

## Dynamic Invocation

Dynamic method invocation is excluded from the initial reflection scope.

It introduces complexity around:

- overload or candidate selection
- generic method instantiation
- runtime argument conversion
- path-view arguments
- capability checks
- error reporting
- hot reload ABI compatibility

Dynamic invocation requires a separate privileged capability, not a consequence of basic metadata reflection.

## Relationship to Traits and Interfaces

Reflection and traits/interfaces are related but not conflated.

Layer split:

- traits or interfaces define callable capability sets
- dispatch uses static resolution or runtime vtables
- reflection exposes metadata about those capability sets
- downcast uses concrete runtime type identity

Reflection is not the mechanism that resolves normal method calls or trait/interface dispatch.

For dynamic interface values, metadata may expose:

- the interface type
- the concrete underlying type, when policy permits
- the methods available through the interface

## Relationship to Hot Reload

Runtime metadata is especially important for hot reload.

It supports:

- public function ABI comparison
- type layout comparison
- field and variant compatibility checks
- typed path ABI validation
- state migration tooling
- debugger and editor updates across epochs

Metadata supports:

- type ids stable within an epoch
- ABI fingerprints across epochs
- field-name and field-id matching for migration tools
- path descriptor compatibility checks

Reflection metadata from old epochs may remain reachable while old module code or values are still reachable.
Normal GC and module epoch lifetime rules determine when old metadata can be reclaimed.

## Host Interoperability

Host objects do not automatically expose reflection.

Host behavior:

- host types are opaque by default
- host type names may be exposed independently from fields
- selected fields may be exposed for metadata inspection
- selected fields may be exposed for typed path access
- reflective read access is a separate opt-in
- reflective write access is a separate privileged opt-in

This keeps the host application in control of what scripts and tools can observe or modify.

## Access Control

Reflection must not silently bypass visibility or host exposure rules.

Access controls:

- language profile gates script-visible reflection syntax or APIs
- runtime capabilities gate metadata read, reflective read, reflective write, and dynamic invocation separately
- host registrations decide which types and members are exposed
- field and method metadata carry read/write/invoke policy

If a member is not registered for reflection, reflective APIs behave as if it is unavailable.

## Runtime Helpers

The runtime may still contain helper operations such as:

- `ReflectTypeOf`
- `ReflectGetField`
- `ReflectSetField`
- `ReflectInvoke`

These helpers are implementation details for optional reflection profiles and tooling integration.
They are not the preferred lowering target for ordinary typed field access, host-backed path mutation, or trait/interface dispatch.

## Initial Feature Set

The initial metadata and reflection scope includes:

- runtime `TypeId`
- `TypeInfo` registry
- field, variant, method, and trait/interface metadata
- host type metadata registration
- metadata needed for GC tracing
- metadata needed for typed path descriptor validation
- metadata needed for hot reload ABI fingerprints
- optional `type_of` in profiles that enable script-visible reflection
- optional read-only metadata objects for tooling and diagnostics

## Initial Scope Exclusions

The initial metadata and reflection scope excludes:

- default script-visible runtime reflection
- reflection-based field mutation in ordinary game logic
- unrestricted reflective writes
- unrestricted dynamic invocation
- automatic reflection for all host types
- generic reflection over every instantiation
- reflection as the mechanism for ordinary trait/interface dispatch
- reflection as the mechanism for ordinary host-backed field mutation

## Implementation Phases

The implementation can be staged in this order:

1. runtime `TypeId`
2. internal `TypeInfo` registry
3. GC layout and trace metadata
4. host type metadata registration
5. field and variant metadata
6. typed path descriptor metadata
7. ABI fingerprints for hot reload validation
8. optional read-only `type_of` and metadata APIs
9. optional reflective reads for tooling profiles
10. privileged reflective writes only if a concrete embedding needs them
