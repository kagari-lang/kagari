# Kagari Host Interop Specification

This document defines the host interop model for Kagari, with a focus on Rust embedding.

The main goal is to let Rust applications expose types and functions to Kagari while preserving Rust-side safety constraints and avoiding unnecessary data copies.

Runtime behavior is defined in [runtime.md](/Users/mikai/CLionProjects/kagari/docs/spec/runtime.md).
Execution behavior is defined in [execution.md](/Users/mikai/CLionProjects/kagari/docs/spec/execution.md).
Backend abstraction is defined in [codegen-backend.md](/Users/mikai/CLionProjects/kagari/docs/spec/codegen-backend.md).
Typed path mutation is defined in [typed-path-mutation.md](/Users/mikai/CLionProjects/kagari/docs/spec/typed-path-mutation.md).

## Design Goals

- allow Rust functions and types to be registered into Kagari explicitly
- support efficient access to Rust-owned data without copying large objects into script memory
- preserve Rust aliasing rules at the host boundary
- keep borrowed host references scoped to a single call frame
- keep script-visible nested host field access separate from raw Rust borrowing
- support concrete registration of generic Rust APIs

## Non-Goals

The interop scope excludes:

- transparent automatic binding for arbitrary Rust APIs
- unconstrained registration of open-ended Rust generics
- escaping Rust borrows into GC-managed Kagari objects
- a full mirror of Rust's type system inside Kagari

## Core Split

Host interop is split into two concerns:

1. registration of types and functions
2. borrow-boundary management

These concerns are related, but they are not collapsed into one mechanism.

Registration decides what the script can name and call.
Borrow-boundary management decides how Rust-owned data may be accessed safely during execution.

## Type Registration

Rust types are registered explicitly.

Conceptually:

```rust
registry.register_type::<Player>("game.Player");
registry.register_type::<Vec2>("math.Vec2");
```

Registration attaches at least:

- script-visible type name
- runtime `TypeId`
- reflection metadata if enabled
- trait metadata if enabled
- host access policy

## Function Registration

Rust functions are also registered explicitly.

Conceptually:

```rust
registry.register_fn("game.heal", |player: &mut Player, hp: i32| {
    player.hp += hp;
});
```

Function registration records:

- exposed symbol name
- parameter list
- passing style for each parameter
- return type
- capability requirements if any

This aligns with the existing host runtime surface in [host.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-runtime/src/host.rs).

## Passing Styles

Host parameters distinguish between:

- owned values
- shared borrows
- unique borrows

This is the minimal set needed to model Rust interop safely.

Conceptually:

```text
Owned
SharedBorrow
UniqueBorrow
```

These remain host-boundary concepts even though Kagari itself does not implement Rust-style borrowing internally.

## Ordinary Owned Values

Owned values are appropriate for:

- small copyable Rust scalars
- values intentionally marshaled into Kagari
- script-owned wrappers

Examples:

- `i32`
- `bool`
- `String` when copying is acceptable
- small POD-like structs

Owned passing is simpler but may be too expensive for large host-owned structures.

## Borrowed Host Values

Borrowed host values are the important case for low-copy interop.

Model:

- `&T` becomes a frame-scoped shared borrow handle
- `&mut T` becomes a frame-scoped unique borrow handle

These handles are script-visible only through host-boundary objects.
They are not ordinary Kagari values that may live freely in the GC heap.

This is the intended use case for:

- large game state objects
- ECS views
- UI trees
- simulation state
- other long-lived Rust-owned data

## Frame-Scoped Borrow Rule

All Rust borrows passed into Kagari are scoped to the dynamic extent of a single host-to-script call.

This is the core safety rule.

Conceptually:

```text
HostCallGuard<'host> {
  frame_id: FrameId,
  borrow_table: BorrowTable
}
```

Borrowed handles carry enough metadata to validate:

- which frame created them
- whether the borrow is shared or unique
- which concrete host object they refer to

## No-Escape Rule

Borrowed Rust references must not escape the call frame that created them.

That means they cannot be:

- returned from script back as ordinary long-lived values
- stored inside GC-managed script objects
- stored in globals
- captured by closures
- carried across `yield`, `await`, coroutine suspension, or hot-reload boundaries

This rule is enforced by the host interop layer and the semantic/runtime validation passes.

## Enforcement Model

The no-escape rule does not rely on a single mechanism.

The enforcement model has three layers:

1. representation restrictions
2. static validation
3. runtime guard checks

This keeps the common case fast and makes failures easier to classify.

### Representation Restrictions

Borrowed host values are not modeled as ordinary freely storable script values.

Instead, they are represented as a special frame-scoped kind of runtime handle.

Conceptually:

- ordinary script values may be stored in locals, fields, globals, closures, and GC objects
- borrowed host handles are frame-scoped and non-storable by default

The following are illegal at the representation level:

- storing a borrowed host handle into a GC object field
- storing a borrowed host handle into a global slot
- placing a borrowed host handle into a closure environment

The representation excludes invalid states before runtime recovery would be required.

### Static Validation

The front end rejects obvious escape paths before execution.

Examples that produce diagnostics:

- returning a borrowed host value
- assigning a borrowed host value into a location that may outlive the current frame
- capturing a borrowed host value in a closure
- holding a borrowed host value across `yield`, `await`, or other suspension points

This does not require a full Rust-style borrow checker.
It only requires tracking which values are frame-scoped host borrows and which operations would cause them to escape.

### Runtime Guard Checks

Runtime checks are still necessary as a final line of defense.

Each frame-scoped host borrow token carries enough metadata to validate:

- which call frame created it
- whether the handle is still valid
- whether the current operation is compatible with the borrow kind

Metadata:

```text
FrameHostBorrowToken {
  frame_id: FrameId,
  object_id: HostObjectId,
  borrow_kind: Shared | Unique,
  type_id: TypeId,
  epoch: BorrowEpoch
}
```

Whenever a host borrow is dereferenced, the runtime verifies:

- the current frame matches `frame_id`
- the handle has not expired
- unique-versus-shared rules are respected

These checks prevent front-end omissions from turning into Rust undefined behavior.

## Failure Classification

Violations of host-borrow rules are not normally handled with `panic!`.

Failure model:

- compile-time detectable escape: diagnostic error
- runtime misuse caused by script execution state: script trap or runtime error
- violated engine invariant: panic or debug assertion

Examples:

- `return borrowed_player` is a compile-time error
- using a host borrow after its frame expired is a runtime error
- dereferencing an already-invalid internal borrow handle without checking is an engine bug

This distinction matters because script mistakes are user-facing errors, while panics are reserved for Kagari implementation bugs.

## Suspension Boundaries

If Kagari later supports `yield`, `await`, coroutines, or other suspension points, borrowed host values are explicitly non-suspendable.

Rule:

- a suspension point is illegal if any live local is a borrowed host handle

This can be enforced through a targeted liveness check rather than a full general borrow analysis.

## GC Interaction

Borrowed host values are not representable as ordinary GC-managed field values.

Rule:

- GC object layouts accept only storable script values
- frame-scoped borrowed host handles are excluded from that set

This provides a strong structural guarantee that host borrows cannot silently persist in heap state.

## Alias Checking at the Host Boundary

Even if Kagari itself does not implement Rust-style borrow checking internally, the host boundary still must preserve Rust's aliasing rules.

In particular:

- the same Rust object must not be exposed as two simultaneous unique borrows
- the same Rust object must not be exposed as both a shared borrow and a unique borrow at the same time

These conflicts are checked when arguments are marshaled into a host-call frame.

This rule applies to actual frame-scoped host borrows.
It does not forbid multiple script-visible host path views that share the same root, because path views are root-plus-path handles rather than live Rust borrows.
Conflicts for path operations are handled when each checked path read or mutation is executed.

## Runtime Representation

The runtime representation uses the host value categories present in [value.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-runtime/src/value.rs):

- `HostRef`
- `HostMut`

They are frame-scoped handles rather than unconstrained runtime values.

Conceptually:

```text
FrameHostBorrowToken {
  frame_id: FrameId,
  object_id: HostObjectId,
  borrow_kind: Shared | Unique,
  type_id: TypeId
}
```

## Kagari-Side Semantics

Kagari script code does not need to spell Rust borrow syntax directly.
Normal member syntax may be backed either by ordinary script-owned values or by host-backed typed paths.

Rules:

- host metadata determines the host passing style
- script code uses normal calls and member access
- host-backed field and index chains lower to typed path operations where appropriate
- frame-scoped host borrow handles are not the script-visible model for nested field access

Example:

```kagari
player.heal(10)
player.hp = 100
```

For host-owned state, assignment such as `player.hp = 100` lowers to a checked path mutation operation rooted at `player`.
It does not require the script runtime to store a Rust `&mut Player` or `&mut player.hp`.

This keeps Kagari syntax clean while still allowing efficient Rust-backed mutation.

Typed path mutation is the normal model for ergonomic field-style access to Rust-owned game state.
Frame-scoped host borrows remain useful for host function calls that require temporary `&T` or `&mut T` access, but they are not used to represent long-lived script field views.

## Registration of Generic Rust Functions

Kagari does not register open Rust generics directly.

Generic registration rule:

- register concrete instantiations separately

Example:

```rust
registry.register_fn("util.sort_i32", |xs: &mut [i32]| { ... });
registry.register_fn("util.sort_string", |xs: &mut [String]| { ... });
```

This is simpler because:

- Rust generic instantiation and Kagari generic instantiation are not the same system
- overload and resolution behavior remains explicit
- diagnostics are easier to keep understandable

## Registration of Generic Rust Types

Use the same strategy for generic host types in v1:

- register concrete instantiations explicitly

Examples:

```rust
registry.register_type::<Vec2>("math.Vec2");
registry.register_type::<InventorySlot<PlayerId>>("game.InventorySlot_PlayerId");
```

The host may expose script-facing aliases that hide the Rust naming detail.

## Deferred Generic Factory Model

A later version may support a generic registration factory.

Conceptually:

```rust
registry.register_generic_type("game.Buffer", resolver);
registry.register_generic_fn("util.sort", resolver);
```

The resolver would receive a concrete Kagari instantiation request and decide whether it can map it to a supported Rust specialization.

This is a later extension, not a v1 requirement.

## Binder Traits

A practical host binding layer can be expressed through conversion traits.

Conceptually:

```rust
trait FromKagariArg<'frame>: Sized {
    fn from_arg(arg: &Value, frame: &'frame HostCallGuard<'frame>) -> Result<Self>;
}

trait IntoKagariValue {
    fn into_value(self) -> Value;
}
```

These conversions can then be implemented for:

- primitive owned types
- script-owned handles
- `&'frame T`
- `&'frame mut T`
- specialized host wrappers

## Guarded Call Boundary

Call sequence:

1. host creates a call frame guard
2. arguments are marshaled into frame-scoped values
3. script executes
4. frame guard is dropped
5. all host borrows associated with the frame become invalid

This structure provides a clean lifetime boundary without requiring Rust's borrow checker to be reproduced inside Kagari.

## Interaction with Reflection

Host interop and reflection remain distinct but composable.

Behavior:

- host types may opt into reflection
- host objects may expose metadata for tooling, diagnostics, or reload validation
- reflective reads over host objects are optional and capability-gated
- reflective writes over host objects are privileged tooling operations, not the ordinary mutation model
- ordinary script mutation of host-owned structured state uses typed path mutation

Reflection is defined in [reflection.md](/Users/mikai/CLionProjects/kagari/docs/spec/reflection.md).

## Interaction with Traits

Host types may implement script-visible traits.

Behavior:

- trait metadata may be attached during type registration
- host values may be viewed through trait/interface value types
- `is<T>` and `downcast<T>` rely on concrete type identity

Trait-system behavior is defined in [traits.md](/Users/mikai/CLionProjects/kagari/docs/spec/traits.md).

## Interaction with Security

Host interop is part of the security boundary.

Behavior:

- host APIs are opt-in exposures
- host reflection is separately gated
- dynamic loading and powerful host services are controlled through capabilities and profile checks

Security behavior is defined in [security.md](/Users/mikai/CLionProjects/kagari/docs/spec/security.md).

## V1 Feature Set

The first usable host interop version includes:

- explicit type registration
- explicit function registration
- owned, shared-borrow, and unique-borrow passing styles
- frame-scoped host borrow handles
- borrow conflict checks at the call boundary
- no-escape enforcement for borrowed values
- concrete registration of Rust generic instantiations

## V1 Exclusions

The first usable host interop version excludes:

- arbitrary open generic registration
- escaping borrowed host values into script-owned storage
- cross-yield borrowed host handles
- automatic reflection for all host types
- unrestricted host mutation without registration and policy checks

## Implementation Order

Incremental implementation order:

1. explicit host type and function registration
2. owned-value marshaling
3. frame-scoped shared-borrow support
4. frame-scoped unique-borrow support
5. borrow conflict checks
6. no-escape validation
7. concrete generic-instantiation registration

This gives Kagari a practical embedded scripting model early, especially for the "Rust owns the data, Kagari patches the behavior" use case.
