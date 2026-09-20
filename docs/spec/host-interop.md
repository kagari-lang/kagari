# Kagari Host Interop Specification

This document defines the host interop model for Kagari, with a focus on Rust embedding.

The main goal is to let Rust applications expose types and functions to Kagari while preserving Rust-side safety constraints and avoiding unnecessary data copies.

Runtime behavior is defined in [runtime.md](runtime.md).
Execution behavior is defined in [execution.md](execution.md).

Typed-path adapters provide `with_read`, `with_validate`, and `with_prepare_write`.
The last callback returns a `PreparedHostPathWrite` containing a single commit
action. The [failure contract](failure-semantics.md#modification-guarantees)
defines preparation, reservation cleanup, ledger publication and fault isolation.
Hosts inspect `Runtime::host_dirty_paths` after execution and clear consumed
records explicitly. There is no synchronous dirty callback. A runnable example is
`cargo run -p kagari-runtime --example atomic_host_path`; it demonstrates a full
ledger rejecting the next update while preserving the preceding field change.
Backend abstraction is defined in [codegen-backend.md](codegen-backend.md).
Typed path mutation is defined in [typed-path-mutation.md](typed-path-mutation.md).

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

1. offline declarations of types and functions
2. runtime bindings checked against those declarations
3. borrow-boundary management

These concerns are related, but they are not collapsed into one mechanism.

Declarations decide what the script can name and call; runtime bindings supply implementations.
Borrow-boundary management decides how Rust-owned data may be accessed safely during execution.

## Type Registration

Rust types are registered from a portable `HostTypeDeclaration`. It owns nominal
type/member identities, export labels, structural signatures, read/write and path
permissions, receiver/parameter passing styles, effects, capabilities and docs.
Its default constructor derives an identity in the application host namespace;
providers can supply their own package/module identity. The runtime binding adds
only a Rust type name; callers no longer supply member slots or ABI fingerprints.

```rust
let mut declaration = HostTypeDeclaration::new("game.Player");
declaration.id = host_type_identity("game.PlayerState");
declaration.fields.push(HostFieldDeclaration::new(
    &declaration.id, "score", HostValueType::I32,
));
let type_id = runtime.register_host_type(
    HostTypeRegistration::new(declaration, "Player"),
)?;
```

Registration attaches at least:

- script-visible type name
- runtime `TypeId`
- reflection metadata if enabled
- checked field and method metadata
- host access policy

Registration rejects duplicate declaration identities even under different export
labels. Identity validation runs before publishing general type metadata, so
rejected registrations do not leave a partially registered type. Link validation
requires every opaque signature type, including nested references, to have a
binding. A HostInterface includes the complete closure of referenced type
declarations. Linking compares the complete required member contract against the
registered declaration; documentation changes do not alter this contract.
Calls check that root/borrow runtime slots correspond to the required
declaration; matching display names or value categories are insufficient.

Root handles are created only by `register_host_root` and carry registry ownership.
Host calls, temporary scopes and path-view chaining reject foreign roots even when
object IDs, type slots, schemas and fingerprints coincide.

`register_host_types` accepts a batch, including mutually referencing types. It
validates identities and members, resolves structural/nominal types and generates
reflection slots in a temporary metadata table. Only a complete successful batch
is published. Single-type registration uses the same path. Offline declarations
can be serialized and queried without a runtime or service initialization; HIR
catalog type IDs belong to one declaration revision and reject stale queries.
Source-level host type resolution, method execution and trait implementation
binding retain separate R06/R07 acceptance; member metadata is not an executable
method callback.

## Function Registration

The current function API is `HostFunction::new(declaration, callback)`. Its
`HostFunctionDeclaration` comes from `kagari_common::host_interface`, which has no
runtime dependency. It carries a `DefinitionId`, export label, typed parameters
and result, passing styles, capabilities, effects, resource cost and documentation.
`HostValueType` covers scalar and composite representations plus opaque nominal
types. Opaque references use declaration identities, not runtime type slots.
The old callback-owned metadata model, arbitrary ABI fingerprint field, static
string type names and `with_metadata` constructor have been removed.

Callbacks receive `(&HostCallContext, &[Value])`. The context exposes the checked
runtime and a call-scoped borrow guard; it cannot be constructed by hosts. Runtime
entry registers a HostResourceScope in the active session and keeps argument roots
and borrow leases there until the call ends. Callbacks can add temporary values with
retain_temporaries; permanent retention still requires RootedValue. Borrowed
results are rejected before their scope ends. The public registry/function invoke
bypasses are removed; host invocation goes through Runtime permission, resource,
signature and heap validation.

`kagari_vm::reenter(context, loaded, function, args)` drives the existing explicit
frame executor synchronously. The loaded handle and FunctionRef select a linked
version, which must belong to the current root program and already be initialized.
Reentry does not initialize modules or switch to the latest epoch. Argument and
result representations are checked against the linked signature, and arguments
must contain valid references owned by this runtime. The return is a RootedValue;
keeping it alive retains the object across subsequent GC and host operations.
Outer frames and borrow guards remain live while nested calls run. Borrow conflicts
are checked across these scopes, and each scope releases only its own resources.
An ordinary nested trap may be handled by the host. Cancellation or budget
termination remains recorded for the root even if the host ignores the error.
Nested calls share the session's frame stack and debugger observer. Breakpoint and
trap snapshots include suspended callers at their actual call instruction, plus
the nested frames. The synchronous callback path currently uses the interpreter.

SharedBorrow/UniqueBorrow parameters acquire corresponding object leases when
passed a HostRoot. Forwarded borrow tokens must belong to this runtime, still be
live and satisfy the required mode; they cannot satisfy Owned passing. Conflicting
arguments reject before invoking the callback, releasing leases already prepared
for earlier arguments. Borrow tokens carry a table owner in addition to frame and
epoch, so coincident frame numbers in other runtimes confer no authority.

Runtime::host_scope creates explicit temporary scopes and validate_host_borrow
checks tokens; the old enter_host_call and mutable borrow-table access are removed.
Scopes participating in a root retain its options/version until the last scope
ends. Root and lease cleanup is unconditional after traps, cancellation, budgets
and quarantine. Direct host operations outside script execution use the same scope
implementation with runtime defaults and do not implicitly create a script root.

One declaration can be cloned into a `HostInterface` for offline tooling and into
the runtime binding. `HostRegistry::link_interface` checks required declarations
against installed bindings without calling them. It rejects missing bindings,
identity/signature/borrow/effect/capability/cost mismatches. Documentation changes
do not change the call contract. Registration rejects duplicate identities and
labels, and invalid declarations leave the registry unchanged.

Interface encoding uses the `KHI\0` magic and version 4, fixed-width little-endian
fields and a 4 MiB limit. Types and functions are sorted by declaration identity. Decoding
rejects other versions, malformed input, duplicates and trailing data. Function
fingerprints use domain-separated FNV-1a-64 over the versioned canonical contract;
documentation is excluded. Binding checks compare the complete contract rather
than treating a matching fingerprint as sufficient evidence. Each value type uses
a flat preorder node sequence, limited to 4096 nodes and depth 64. Invalid child
counts, trailing nodes, excessive depth and invalid Map/Set key types are rejected.
The function fingerprint domain is `kagari-host-function-v2`; type, field and
method fingerprints have separate v1 domains. Member declaration order is retained
because it determines runtime slots. Versions 1 through 3 are not decoded.

The source `print` entry and CLI log binding use the same `standard_log`
declaration. Run `cargo run -p kagari-runtime --example offline_host` for an
offline export/decode/bind/check example.

The compiler consumes an immutable HostDeclarations input. KagariEngine's
set_host_interface accepts decoded declarations and performs no runtime
registration. Source may call demo::echo directly, import demo::echo with an
optional alias, use a nested import group, or import demo as a module alias.
Function declarations use dotted export labels; source paths use double colons.
Labels select the exposed source path; DefinitionId remains the binding identity.
The std namespace is reserved. Ambiguous function/module paths and unspellable
declaration paths are rejected. Unknown imports and duplicate import aliases
produce source diagnostics instead of disappearing during lowering.

Host declaration revisions participate in analysis caching and body reuse.
Snapshots retain their declaration catalog, signatures, documentation and scoped
host IDs; a host ID from another catalog cannot resolve by coincident index.
AnalysisSnapshot exposes both source revision and host revision. FileAnalysis's
host_function_at query returns the declaration at a callee or function import
without executing code, including through source facades.
Changing the catalog invalidates semantic reuse while unchanged source can reuse
its parsed CST. Correct neighboring functions remain queryable after import or
call errors. Host calls require the host-call language profile and are excluded
from scalar constant evaluation.

The source call boundary supports scalar and nested Tuple, Array, Map, Set, Option
and Result signatures. Map keys and Set elements must be bool, i32, i64 or String.
Composite arguments use `Owned` passing: shared mutable objects retain their
identity, and tuples retain value semantics. This does not provide a Rust borrow
lease or make a naked Value an owning GC root. Owned arguments reject frame-scoped
host borrows even inside nested tuples; script containers cannot store them.
Arguments stay rooted during the
callback; longer host retention requires an explicit rooted handle. Both argument
and result checks inspect nested members and standard enum tags, reject foreign
or stale heap references, and observe execution termination while traversing.
Argument mismatch prevents callback execution. A result mismatch rejects the
result without rolling back effects already performed by the callback.
Opaque host types resolve in source annotations and call signatures by declaration
identity. Direct imports, module aliases and source facade re-exports share the
same semantic targets. Host types are distinct from script structs, even when
their names coincide; they do not support script construction or generic equality.
`host_type_at` exposes the offline declaration and documentation without inventing
a source location. Erroneous applications retain the base target, and immutable
snapshots retain their original host catalog after interface input changes.
Nominal host references survive public ABI encoding and generic function
instantiation. Lowering collects required type declarations and their transitive
member references in identity order, including dependencies used only in source
signatures. Unused catalog types are omitted. Linking checks these complete
contracts before publication, even if there are no host calls in the program.
`HostHandle` is a separate execution representation; script heap objects cannot
satisfy an opaque host parameter's representation. Runtime nominal identity,
ownership, borrow and escape checks still apply. This does not authorize host
handles or borrows as default script heap payloads. Host field/path binding, host trait implementations and generated typed paths
remain R06/R07 work.
Host methods are callable through their receiver: `player.read_score()`.
`HostTypeDeclaration::method_contract` derives the executable contract from the
member identity, signature, receiver passing style, effects, capabilities and cost.
The first argument is an explicit `self` of the declaring nominal host type;
member parameters cannot also use that reserved name. There is no second editable
method signature. `HostFunction::method` binds a callback to that definition.
Registration requires its declaring host type to be installed and checks the
derived contract before publishing the callback slot. Required method imports
must match their type's member declaration; a missing method callback rejects
loading even if the type metadata is registered.
Analysis catalogs derive method call targets without starting a runtime. HIR owns
the receiver expression and member call identity, including when arguments have
errors; `host_function_at` returns the generated contract and original member docs.
Lowering evaluates the receiver first and each explicit argument once, then emits
the existing linked host call operand. Methods cannot be imported as free functions
through the type's label. Calls use the generated `type-symbol.method-name` label
for host exposure policy and preserve the declared capability/effect checks.
Receiver borrow leases use the ordinary host call scope, including cleanup after
failure and rejection of conflicting synchronous reentry. Method source calls,
artifacts and existing JIT fallback share this execution path. Opaque receivers
remain roots or valid borrow tokens; this does not make path views opaque objects.
Public host function, type and module re-exports retain their offline declaration
identities through source facades. The import graph resolves the final binding once; name resolution,
signature catalogs and navigation consume it. The original source dependency is
retained for dependency-first initialization even when all calls target hosts.
For example, `pub use demo::echo as call; pub use demo as service;` permits clients
to import `call` or `service` from the facade, or call `facade::call(...)` and
`facade::service::echo(...)` through a source module alias. These calls retain
ordinary lexical shadowing and require the original host symbol's permission and
matching runtime binding; re-exporting grants no additional authority.
Run cargo run -p kagari-embed --example offline_compile to compile an imported
host call without any runtime or callback registration.

Required bytecode declarations now link mandatorily before module
publication, including artifact reloads. Compiled host calls use registry-owned
slots; labels remain declaration and policy metadata. A wrong runtime cannot
reuse a coincident slot. Loading executes no callbacks. See [bytecode.md](bytecode.md)
and [artifacts.md](artifacts.md) for the executable and encoding contracts.

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

This aligns with the existing host runtime surface in [host.rs](../../crates/kagari-runtime/src/host.rs).

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
HostCallGuard {
  frame_id: FrameId,
  borrow_table: BorrowTable
}
```

Borrowed handles carry enough metadata to validate:

- which frame created them
- which runtime borrow table owns them
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

[Failure semantics](failure-semantics.md) is authoritative for cleanup, resource
termination, partial business effects, and runtime reuse.

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

The runtime representation uses the host value categories present in [value.rs](../../crates/kagari-runtime/src/value.rs):

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
    fn from_arg(arg: &Value, frame: &'frame HostCallGuard) -> Result<Self>;
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

Reflection is defined in [reflection.md](reflection.md).

## Interaction with Traits

Host types may implement script-visible traits.

Behavior:

- trait metadata may be attached during type registration
- host values may be viewed through trait/interface value types
- `is<T>` and `downcast<T>` rely on concrete type identity

Trait-system behavior is defined in [traits.md](traits.md).

## Interaction with Security

Host interop is part of the security boundary.

Behavior:

- host APIs are opt-in exposures
- host reflection is separately gated
- dynamic loading and powerful host services are controlled through capabilities and profile checks

Security behavior is defined in [security.md](security.md).

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
