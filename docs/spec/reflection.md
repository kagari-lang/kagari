# Kagari Reflection Draft

This document describes a proposed reflection model for Kagari.
It is intended as a design draft, not a finalized implementation contract.

The main goal is to support runtime inspection, tooling, host integration, and hot-reload-friendly metadata without forcing the language into a fully dynamic object model.

Security direction is drafted separately in [security.md](/Users/mikai/CLionProjects/kagari/docs/spec/security.md).
Host interop direction is drafted separately in [host-interop.md](/Users/mikai/CLionProjects/kagari/docs/spec/host-interop.md).
Runtime model direction is drafted separately in [runtime.md](/Users/mikai/CLionProjects/kagari/docs/spec/runtime.md).

## Design Goals

- support runtime type inspection
- support controlled field and variant introspection
- share type identity with `dyn Trait`, `is<T>`, and `downcast<T>`
- keep reflection compatible with a GC-backed runtime
- make reflection opt-in where appropriate

## Non-Goals

The first reflection version should not attempt to reproduce the full reflection model of languages such as C# or Java.

In particular, v1 should avoid:

- unrestricted dynamic method invocation
- automatic reflection for every host type
- full generic instantiation reflection
- unrestricted private-state exposure
- compile-time metaprogramming through reflection

## Core Model

Reflection should be built around runtime metadata objects.

The central concept is `TypeInfo`.

Conceptually:

```text
TypeInfo {
  id: TypeId,
  name: String,
  kind: TypeKind,
  fields: [FieldInfo],
  methods: [MethodInfo],
  variants: [VariantInfo],
  traits: [TraitInfo]
}
```

The exact layout does not matter as much as preserving these capabilities:

- stable runtime type identity
- inspectable structural metadata
- integration with dynamic dispatch and downcast

## Type Identity

Reflection should share the same runtime type identity used by:

- `dyn Trait`
- `is<T>`
- `downcast<T>`

This is the recommended unifying rule:

- every runtime object has a concrete `TypeId`
- reflection looks up metadata through that `TypeId`
- dynamic trait objects preserve the concrete `TypeId`
- downcast succeeds by comparing concrete `TypeId`

This prevents the runtime from growing multiple incompatible type identity systems.

## Reflection Scope

Reflection should be explicitly scoped.

The recommended default is:

- script-defined types may expose baseline metadata
- structural field access should be opt-in
- host-defined types should be reflectable only when explicitly registered

This prevents accidental overexposure and keeps host integration under control.

## Opt-In Model

A reasonable design is to make reflective capabilities configurable per type.

Possible script-facing direction:

```kagari
@reflect
struct Player {
    hp: int,
    name: String,
}
```

Possible host-facing direction:

```rust
register_type::<Player>()
    .reflect_name()
    .reflect_fields()
    .reflect_methods();
```

The final surface syntax does not need to be decided yet.
The important point is that reflection should be a registered capability, not an automatic global default.

## Baseline Runtime API

The smallest useful reflection API should include a very small entry surface.

Recommended user-facing direction:

- `type_of(value) -> TypeInfo`
- `TypeInfo.id()`
- `TypeInfo.name()`
- `TypeInfo.kind()`
- `TypeInfo.fields()`
- `TypeInfo.methods()`
- `TypeInfo.variants()`

Example:

```kagari
let ty = type_of(player);
print(ty.name());
```

The intent is to keep built-in reflection entrypoints minimal.
Once a script has a `TypeInfo`, most further reflection should look like ordinary method calls on ordinary runtime objects, not a large family of special forms or special keywords.

## User-Facing API Shape

The recommended surface shape is:

- keep the number of reflection builtins very small
- prefer returning reflection objects such as `TypeInfo` and `FieldInfo`
- expose operations through methods on those objects

Recommended direction:

```kagari
let ty = type_of(player);
let field = ty.field("hp");

print(ty.name());
print(field.name());
print(field.get(player));
field.set(player, 100);
```

This keeps reflection aligned with the rest of the language:

- one small entry builtin such as `type_of`
- ordinary method calls after that
- no need for many special reflection keywords

The internal runtime may still use helper operations such as `ReflectGetField` or `ReflectSetField`, but those should be treated as implementation detail, not the preferred user-facing surface.

## Type Kinds

The runtime should at least distinguish:

- primitive
- struct
- enum
- trait object
- function
- host object

This does not need to imply a user-visible exhaustive enum in v1, but the metadata system needs an internal notion of type kind.

## Field Reflection

Field reflection is one of the highest-value features and should be supported in a controlled way.

Recommended user-facing API shape:

```kagari
let ty = type_of(value);
let field = ty.field("hp");

field.get(value)
field.set(value, 100)
```

Recommended behavior:

- field lookup is by declared field name
- `get_field` returns an optional or result-like value on failure
- `set_field` performs runtime type checking
- field writes are rejected when reflection write access is not enabled

## Enum Reflection

Enums should expose enough reflection to support tools and data-driven code.

Suggested capabilities:

- list enum variants from `TypeInfo`
- inspect the active variant of a value
- read payload fields for the active variant

Possible API direction:

```kagari
let ty = type_of(value);
let variant = ty.active_variant(value);

print(variant.name());
print(variant.fields());
```

This is especially useful for debuggers, inspectors, and serialization helpers.

## Method Reflection

Method reflection should initially expose metadata only.

Suggested baseline:

- method name
- parameter metadata
- return type metadata
- trait or inherent origin

This is enough for:

- inspectors
- editor integration
- documentation tooling
- host integration layers

It is not necessary to support unrestricted runtime invocation in v1.

## Dynamic Invocation

Dynamic invocation is possible, but it should not be part of the first reflection milestone unless there is a strong concrete need.

Examples of complexity it introduces:

- overload or candidate selection
- generic method instantiation
- runtime argument conversion
- `ref` argument handling
- error reporting quality

If added later, it should likely be a separate capability from basic reflection.

## Relationship to Traits

Reflection and traits should be related but not conflated.

Recommended split:

- traits define callable capability sets
- reflection exposes metadata about those capabilities
- downcast is driven by concrete runtime type identity

This means reflection may answer questions such as:

- what traits does this type implement?
- what methods are exposed through a given trait?

But reflection should not become the mechanism that resolves trait semantics.

## Relationship to `dyn Trait`

`dyn Trait` values should remain reflectable.

For a dynamic trait object:

- `type_of(value)` should report the concrete underlying type, or expose both concrete and interface type information
- trait metadata should remain accessible
- `is<T>` and `downcast<T>` should work through the same underlying type identity

The most practical model is to expose both views when useful:

- concrete type view
- interface view

## Relationship to Hot Reload

Reflection metadata is especially valuable for hot reload.

It can support:

- structural compatibility checks
- field migration tooling
- editor/debugger updates across reload epochs
- runtime validation of changed object layouts

This means type metadata should be designed with versioning in mind.

At minimum, the runtime should be prepared for:

- type ids that remain stable within a reload epoch
- metadata comparisons across epochs
- field-name-based matching when migrating state

## Host Interoperability

Host objects should not automatically expose full reflection.

Recommended model:

- a host type may expose only its name
- or name plus selected fields
- or selected methods
- or a richer fully registered metadata view

This keeps the host application in control of what scripts can observe and modify.

## Access Control

Reflection should not silently bypass visibility rules unless the type explicitly allows it.

Recommended v1 rule:

- reflection only exposes members that are registered as reflectable

If the language later grows stronger visibility boundaries, reflection should continue to respect the registered exposure policy.

## Recommended v1 Feature Set

The first usable reflection version should include:

- `type_of`
- `TypeInfo` with `id()`, `name()`, and `kind()`
- `FieldInfo`
- enum variant metadata objects
- optional trait implementation metadata
- controlled field read and write through reflection objects
- shared type identity with `dyn Trait`, `is<T>`, and `downcast<T>`

## Recommended v1 Exclusions

The first usable reflection version should exclude:

- unrestricted `invoke`
- generic reflection over every possible instantiation
- automatic reflection for all host types
- unrestricted access to non-reflectable fields
- compile-time reflection features

## Suggested Metadata Types

One reasonable internal model is:

```text
TypeInfo {
  id: TypeId,
  name: String,
  kind: TypeKind,
  fields: [FieldInfo],
  methods: [MethodInfo],
  variants: [VariantInfo],
  implemented_traits: [TraitInfo]
}

FieldInfo {
  name: String,
  ty: TypeId,
  readable: bool,
  writable: bool
}

MethodInfo {
  name: String,
  params: [ParameterInfo],
  return_type: TypeId,
  origin: MethodOrigin
}

VariantInfo {
  name: String,
  fields: [FieldInfo]
}
```

This is only a conceptual model, but it is a useful target for runtime and tooling design.

The recommended script-facing mapping is:

- `type_of(value) -> TypeInfo`
- `TypeInfo.field(name) -> FieldInfo?`
- `FieldInfo.get(value) -> Value`
- `FieldInfo.set(value, next) -> Value`
- `TypeInfo.active_variant(value) -> VariantInfo?`

This keeps the reflection surface object-oriented and avoids expanding the core language with many reflection-specific forms.

## Recommended Implementation Order

If implemented incrementally, the recommended order is:

1. runtime `TypeId`
2. `TypeInfo` registry
3. `type_of`
4. field metadata and `get_field`
5. controlled `set_field`
6. enum reflection
7. trait metadata exposure
8. optional dynamic invocation

This keeps the early system useful for debugging and tooling without forcing the hardest pieces first.
