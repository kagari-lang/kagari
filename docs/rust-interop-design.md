# Rust Value and Opaque Interoperability Plan

Status: active combined host/interop delivery; RI01-RI02 and HR02 owned registration implemented; RI03/RF02 proceed with reference-dependent HR02 migration still open.
This plan defines typed Rust value conversion and runtime-owned opaque objects. The user selected
runtime ownership with checked host read/edit and explicit take; Rust and Kagari
traits remain independent and require explicit script registration. Examples are
target API sketches, not interfaces that exist today. The
[combined delivery order](host-registration-plan.md#combined-delivery-order)
coordinates HR01-HR03, RI00-RI05 and RF01-RF06 in the same selected scope; each
plan retains its own responsibilities and progress.

The later [runtime ownership and host object design](runtime-ownership-and-host-api-design.md)
owns central runtime storage, automatic script-object roots, the Send runtime
contract, common typed native adapters and managed-payload editing. RI reuses that
foundation and owns DTO derives, Serde and the Opaque facade/extraction additions.
Its phases extend existing NativeObject storage, roots and access rather than
implement a second object table or root system. External shared resources require
an explicit payload wrapper; exclusive access to that wrapper does not manufacture
exclusive access to an externally shared resource inside it.

The [SA21 registration proposal](host-registration-plan.md#retained-opaque-integration)
unifies declaration/installation ownership and presents ordinary structs through
by-value conversion to Kagari objects. Its complete example uses this plan's
proposed value schema/derive API; it does not claim those capabilities already
exist or move RI01/RI02/RI04 into a second implementation queue. The
[Ref/RefMut execution contract](host-registration-plan.md#ref-and-refmut-execution-contract)
adds scoped path handles and generated projections for disjoint inline fields;
RF01-RF06 implement those capabilities in the same delivery. This plan owns DTO
schema/conversion/derives and the runtime-owned Opaque contract. Registered host
methods declare shared or mutable receiver access and acquire short checked loans
from their actual storage owner. An Opaque handle is not a freely dereferenceable
Rust reference. Shared references may change interior state; registration retains
conservative call-effect metadata.

Build on the current [native registration](spec/standard-declarations.md) and
[installation-based access model](spec/execution.md). Activation and scheduling
belong to the [roadmap](implementation-roadmap.md). Synchronous interop does not
depend on async. Re-audit affected APIs at activation; existing specifications
remain in force until an implementation updates them.

The [single binding entrypoint](host-registration-plan.md#one-function-binding-entrypoint)
owns registration syntax: register type metadata, declare functions/members, then
attach implementations with `bind`. Declarations are the signature authority;
Rust converters/adapters are checked against them. Value and opaque registration
share this model without separate inferred-signature or chained binding APIs.
The examples below use this one model;
exact new member-builder spellings still require the owning plan's compile proof.
Reference receivers follow its
[conversion and receiver rules](host-registration-plan.md#method-receivers-and-trait-selection).
Ordinary GC DTOs do not automatically receive host methods. Native object access
uses its registered capability; no Rust reference is obtained by layout casting.

## Design decisions

- Ordinary value bindings convert data into script-owned values. Opaque bindings
  pass typed handles to Rust objects without traversing their contents.
- Conversion is selected by the declared type, never by trying serialization and
  falling back to an opaque representation at runtime.
- Parameters, results, properties, collection elements and nested fields use the
  same recursive conversion rules. Configuration objects have no special path.
- `Opaque<T>` is a Rust binding wrapper. Kagari sees the registered nominal type,
  such as `game::GameConfig`, not `Opaque<GameConfig>`.
- `Opaque<T>` is cloneable without requiring `T: Clone`. Cloning retains the same
  object; it does not copy the payload. Users need not choose `Arc` or another
  internal retention mechanism.
- Module registration determines the namespace. A derive supplies a short type
  name and schema, not a repeated module prefix.
- Runtime owns opaque payloads; both host access and registered methods use checked
  loans. Explicit take transfers ownership out and invalidates all old aliases.
- Ref/RefMut lend host-owned data for a scope, reusing explicitly registered host
  methods. RF01-RF06 own their implementation within the combined delivery.
- Rust traits never automatically supply Kagari traits or methods. Script exposure
  requires explicit declarations and bindings, including explicit Kagari impls.
- Registration and exposure authorize use under the current execution model.
  Properties have declared writeability, not additional runtime permission flags.

Excluded here are raw script-visible Rust references, borrowed strings/slices,
retained borrowed subobjects, general consuming-self adapters and open Rust generic
factories. Scoped external Ref/RefMut construction and path projections remain in
the reference plan. Runtime-owned shared/mutable method adapters belong to RI03,
with lifetime machinery proved in RF01 and reused in RF05. Existing scoped borrow and
typed-path protections remain in force.

## Current implementation boundary

The combined implementation now includes ordinary transactional schema graphs,
rooted nominal DTO conversion, runtime-owned Opaque with atomic take, checked
lifetime-aware methods, branded external whole-object scopes and the interpreter's
scoped entry. Dedicated `kagari-derive::KagariValue` generates finite named structs
and unit/tuple enums through that same schema/conversion owner. Automatic
`register_type::<Opaque<T>>()` registers an explicit NativePayload storage policy,
checks its Rust identity and maps only the Opaque wrapper to the module's nominal
declaration. NativeData's existing proof grants its data/take flags; the default
payload factory grants read-only traced storage. No Rust method or trait is inferred.
The optional direct Serde adapter and `KagariSchema` derive now use those same
checked conversion boundaries. Ordinary Struct lending now uses the common RustType target descriptor without
NativePayload. Generated projections, mixed loans and final SDK/legacy integration
remain in RI04/RF04/RF05. The historical inventory below is the
activation baseline, not a claim that these newer checkpoints remain unimplemented.

Current owners are the [schema graph](../crates/kagari-runtime/src/native/schema.rs),
[rooted conversions](../crates/kagari-runtime/src/native/conversion/context.rs),
[Opaque storage](../crates/kagari-runtime/src/native/payload/mod.rs),
[branded scope](../crates/kagari-runtime/src/host_scope/scoped.rs) and the
[common lifetime-aware adapter](../crates/kagari-runtime/src/native/typed/loans.rs).
The existing live-host interface/path consumers retain their checked behavior until
RF05 migrates them and HR03 removes their obsolete registration aggregation.
Neither HostTypeOwnership nor an ordinary GC DTO grants a Rust reference.

## One type mapping and conversion boundary

Three conceptual capabilities define a binding:

| Capability | Responsibility |
| --- | --- |
| `KagariType` | Describe a script type and its dependencies without a runtime or sample value |
| `IntoKagari` | Convert Rust data to a checked script representation |
| `FromKagari` | Convert a checked script value into owned Rust data or a retained opaque wrapper |

The precise trait signatures remain an implementation gate. Type description must
resolve names through a registration context, rather than returning a process-global
script ID. Conversion needs a runtime context for allocation, temporary roots,
nominal type checks, object retention and errors. These are fallible operations;
an infallible `into_value(self) -> Value` API is insufficient.

Keep input and output capabilities separate. A return-only type need not implement
input conversion. Typed function registration validates the capabilities needed by
its signature, rather than demanding every type support both directions.

| Rust binding type | Script representation | Boundary behavior |
| --- | --- | --- |
| Supported scalars and `String` | Corresponding primitive | Checked conversion; strings may allocate |
| Value-bound struct or enum | Ordinary nominal struct or enum | Convert declared fields or variant payloads |
| `Opaque<T>` | Registered native nominal type | Retain or resolve a checked handle to this runtime's payload; never implicitly take T |
| `Option<T>` and tuples | Corresponding composite | Recursively apply the element mapping |
| Supported `Vec<T>`, maps and sets | Declared script collection | Convert elements; obey key and collection contracts |
| `Serde<T>` | The nominal value type described by its schema | Use the explicit Serde adapter, not a dynamic dictionary |

Define numeric range checks and supported widths explicitly. Unsupported Rust
types must fail registration/derive or require an explicit adapter, not silently
truncate or stringify. Collection conversion must preserve the declared script
collection semantics, without promising the source Rust collection's iteration
order when that order is not stable. Register concrete generic instantiations.

`Result<T, E>` used as application data maps to a script `Result`. Binding/runtime
failure needs a distinct fallible-call channel so a business `Err` is not silently
turned into a VM trap. Final spelling of that channel is a design gate.

The companion [host API plan](host-api-refactor.md#confirmed-argument-and-return-rules)
owns the confirmed host-call argument adapter: the outermost Rust tuple is always
the parameter list, and each element uses `IntoKagari`. `IntoKagari` for a tuple
still produces one script tuple value. Return conversion uses `FromKagari` for one
complete value, never implicit multiple-return expansion. This does not change RI's
conversion scope or make RI depend on implementation of the later facade.

## Value binding

The default path uses a dedicated derive to generate schema and direct conversions
from the same definition. This registration sketch is a target API, not compiling
Rust against the current SDK:

```text
#[derive(KagariValue)]
struct DamageRequest {
    target_id: i64,
    damage: i32,
}

let mut engine_builder = KagariEngine::builder();
let mut game = ModuleBuilder::new("game", engine_builder.declarations())?;
let request_type = game.register_type::<DamageRequest>()?;
```

Declare the script signature, then bind a checked typed adapter in the same module:

```text
fn echo(request: DamageRequest) -> DamageRequest {
    request
}

let echo_function = game.define_function(
    FunctionDecl::new("echo")
        .parameter("request", request_type.clone())
        .returns(request_type.clone()),
)?;
game.bind(echo_function, |_cx, (request,): (DamageRequest,)| Ok(echo(request)))?;
engine_builder.install(game.finish()?)?;
let engine = engine_builder.build()?;
```

Kagari sees `game::DamageRequest` as an ordinary nominal script value type. Data
converted from Rust is a snapshot: mutating that script object does not update the
original Rust object. Once created, it obeys ordinary Kagari shared-object rules;
copying a script variable is not another deep serialization.

The derive exports ordinary by-value struct fields as writable Kagari fields
(`var` in the generated declaration). Script mutation affects only the converted
object. Local binding reassignment follows the normal `val`/`var` rules separately
from field mutation. Document which fields are exported; unsupported field/variant
forms produce diagnostics. A manual adapter is available for external types or
deliberately different representations.

The proposed `module.register_type::<T>()` registers DTO schema/mapping metadata;
`register_type::<Opaque<T>>()` registers a native payload representation. Neither
creates an object. Reference support reuses the selected target declaration;
registering a DTO is not registration of an opaque representation or host methods.
RI04 and RF04 coordinate derive output with checked
local projections for supported inline fields, under the reference plan's safety
contract. An owned converter, custom schema or Serde mapping alone does not prove
that a field can be borrowed from the original Rust representation.

Mixed value and opaque data use the same recursion:

```text
#[derive(KagariValue)]
struct BattleInput {
    damage: i32,
    config: Opaque<GameConfig>,
}
```

Only the outer structure and ordinary value fields are converted. `config` retains
the same runtime-owned Rust object, with runtime-owner checks on every conversion;
it cannot be used in another runtime. Returning a structure follows the same rule.
Input conversion of ordinary mutable script data yields owned Rust data; it does
not secretly borrow script storage or mutate the source graph.

By-value conversion does not promise to preserve arbitrary graph aliasing or cycles.
Reject cycles that cannot be represented by the target owned data model with a
bounded diagnostic. Define this policy explicitly rather than recursing indefinitely.

## Optional Serde adapter

Serde's `Serializer` and `Deserializer` provide a format-independent conversion
model. A Kagari adapter can construct/read script values directly, without an
intermediate JSON string or byte buffer. See the [Serde data model](https://serde.rs/data-model.html).

Static script typing additionally needs a schema available before any value exists.
Serializing a sample cannot discover all enum variants or the element type of an
empty collection. This plan therefore proposes an explicit adapter:

```rust
#[derive(Serialize, Deserialize, KagariSchema)]
struct DamageRequest {
    target_id: i64,
    damage: i32,
}

fn process(request: Serde<DamageRequest>) -> Serde<DamageResult> {
    // Business implementation omitted in this API sketch.
}
```

Register `Serde<DamageRequest>` through the value-binding API; the wrapper is absent
from the script signature. Outbound conversion requires schema plus `Serialize`;
inbound conversion requires schema plus `DeserializeOwned`. The latter avoids
retaining borrows into script storage, consistent with
[Serde's lifetime guidance](https://serde.rs/lifetimes.html).

This is an opt-in adapter, not a blanket conversion implementation for every
`T: Serialize`. That would conflict with explicit representations and allow a Rust
serialization implementation to choose the script type accidentally.

The initial automatic subset should cover supported primitives, named structs and
explicitly supported composites/enum forms. The exact acceptance matrix must be
fixed before implementation. [Serde attributes](https://serde.rs/attributes.html)
can change representation; schema generation must honor supported attributes and
reject unsupported ones. In particular, do not silently accept `flatten`, conditional
field omission, asymmetric names or custom serializers using the Rust field schema.
Custom mappings need an explicit matching schema and conversion validation.

Opaque handles have no generic Serde wire encoding. Do not encode a runtime object
ID as an integer and recreate an allegedly valid object on deserialize. Use the
dedicated value derive for structures containing opaque fields. Arbitrary existing
Serde types are not guaranteed to be directly bindable.

## Module registration and identity

The registration module owns the path. This independent variant uses the same
registration flow as above:

```text
#[derive(KagariValue)]
#[kagari(name = "DamageInput")]
struct DamageRequest {
    target_id: i64,
    damage: i32,
}

let mut engine_builder = KagariEngine::builder();
let mut game = ModuleBuilder::new("game", engine_builder.declarations())?;
let request_type = game.register_type::<DamageRequest>()?;
```

The resulting name is `game::DamageInput`. The attribute is optional and accepts
only a short name; do not repeat `game.` or `game::` in a derive. Opaque registration
uses the same module ownership and short-name rule.

Portable identity comes from package identity, module path and declaration identity.
Rust `TypeId` may locate a local binding but must not be serialized as ABI identity.
Members have stable declaration identities and contract fingerprints as required
by the provider/link model; display names alone do not prove compatibility.

Within one registration catalog, reject a second nominal registration of the same
Rust type binding under another module/name. Ordinary export aliases may refer to
the original declaration without creating another type. Intentionally exposing one
Rust payload through two representations requires explicit distinct binding types,
for example a DTO newtype and an opaque object binding, not an implicit override.
Independent catalogs/runtimes need not share a process-global naming table.

Finalize registration as a validated dependency graph, allowing mutually referring
schemas without requiring a fragile registration order. Missing bindings, duplicate
identities and conflicting definitions must reject publication without partially
installing a usable API. Repeated dependency references are not duplicate definitions.

## Opaque ownership and access

`Opaque<T>` is the typed, rooted handle to an exclusively runtime-owned
Rust payload. Insertion moves T into the runtime. Cloning the handle retains the
same object without requiring `T: Clone`; neither Rust nor script aliases acquire
a Rust reference merely by holding a handle. The wrapper has no Deref/DerefMut.
Kagari sees the registered native nominal type, such as `game::Player`, not the
Rust wrapper spelling `Opaque<Player>`.

This uses the existing
[Opaque/NativeType storage and access](../crates/kagari-runtime/src/native/payload/mod.rs).
The public NativeObject handle and create_native constructor have been replaced
directly, without compatibility aliases. Construction, checked read, fixed-data
edit and explicitly edge-free take are implemented. Shared/mutable method adapters now reuse the common checked bind and invocation
owner across Opaque and scoped external storage. Field/path integration and HR
registration retirement retain their RF/HR acceptance gates.

### Relationship to Ref and RefMut

| Handle | Payload owner | Lifetime and access |
| --- | --- | --- |
| `Opaque<T>` | Runtime owns T. | Rooted host handles and reachable script references retain the object within that runtime; checked shared/mutable access, and explicit host ownership extraction. |
| `Ref<T>` | Host continues to own T. | Scope-bound shared access; no transfer, retention beyond scope or ownership extraction. |
| `RefMut<T>` | Host continues to own T. | Scope-bound shared or exclusive access with dynamic conflict checks; no transfer or ownership extraction. |

The latter two are [host location capabilities](host-registration-plan.md#ref-and-refmut-execution-contract),
not long-lived Rust references. All three can use explicitly registered host
methods on the same native nominal type. The method's required access and the
actual storage/handle capability determine whether invocation is admitted. No
second Player registration or duplicate method bodies are required for borrowing.

Ordinary DTO conversion is a separate representation. Registering its schema does
not expose Rust methods or traits. A native Player has no structural DTO conversion
by default; if both forms are needed, expose an explicit PlayerSnapshot/value
binding with its own nominal identity and conversion operation. Existing structured
DTO-backed Ref examples retain their explicitly registered projection capabilities;
that capability does not attach host methods to ordinary GC instances.

Borrowing an Opaque payload for one native call does not create a script Ref value.
Retained Ref views into runtime-owned objects, implicit conversion between these
handle categories and arbitrary cross-runtime sharing are outside this initial
scope. External shared resources can be represented by explicitly registered
payload wrappers; their synchronization/ownership rules are not inferred from T.

### Host construction, access and extraction

The target facade below is pseudocode, with a checked loaded program and the
registered Player type supplied by the method example. It must resolve the exact
runtime/type generation through existing prepared NativeType machinery; this is
not permission to bypass installation or execute using unchecked type IDs.

```text
let player: Opaque<Player> = runtime.insert_opaque(
    &loaded, &player_type, Player { hp: 100 },
)?;
let alias = player.clone();
let hp = runtime.read(&player, |p: &Player| p.hp)?;
runtime.edit(&player, |p: &mut Player| {
    p.hp = p.hp.saturating_add(20);
})?;

// Passing a clone into script retains the same object, not a copy of Player.
runtime.execute_typed(&loaded, "on_reward", (player.clone(),), &context)?;

let recovered: Player = runtime.take(&player)?;
// player, alias and every script alias can no longer access the removed object.
```

`read`/`edit` lend references only inside the closure. They must preserve the
existing non-escaping, non-reentrant native storage access contract: no script
execution, suspension, collection or runtime transfer while the storage loan is
active. Borrow guards, not GC timing, end access on success, error and unwind.
Completed edits remain visible on failure. Handle-based complex operations may
alternate access and script calls after releasing each loan.

The first safe scope reuses proven registered storage capabilities. Current direct
editing supports fixed-size NativeData without script edges; it does not admit
arbitrary String/Vec fields or hidden script handles. General owned Rust payloads
need explicit tracing/accounting/mutation capabilities before unrestricted edit or
extraction is accepted. Existing managed storage uses controlled setters for its
script edges; do not turn those into unchecked `&mut T` access. `T: Send + 'static`
is required for runtime-owned payloads, with no blanket Sync requirement because
the runtime is exclusively driven. Nested externally shared storage must still
satisfy its own Rust access requirements.

`take` has the following proposed contract:

- Validate runtime owner, exact registered Rust type, generation, storage extraction
  capability and absence of active/in-flight payload loans before mutation. The
  initial implementation rejects extraction during native storage borrows/reentry.
- Other inactive handles, including script aliases and parked execution references,
  do not by themselves prevent extraction. Commit removal once and invalidate all
  aliases before returning T; subsequent access or a second take returns a
  structured unavailable-object error. A failed precondition leaves the object
  present and unchanged.
- Preserve identity/generation protection so slot reuse or reinsertion of the
  recovered T cannot resurrect an old handle. Old pinned methods still reject an
  extracted receiver; pinning code is not permission to access missing storage.
- Release payload accounting and traced edges through the storage owner, and
  transfer destruction responsibility to the caller exactly once. The first
  extraction proof covers payloads without script edges; extraction of managed
  script references requires a separate rooting/ownership conversion proof.
- Taking T is never implicit FromKagari decoding or a side effect of cloning or
  dropping a handle. It does not run a script callback, and it does not roll back
  previously completed script/host effects.

Without take, unreachable payloads are collected once; live host roots prevent
collection while the runtime exists. Runtime teardown destroys its remaining
payloads and invalidates retained handles. A handle does not keep an independently
accessible Rust allocation alive after its runtime is gone. Explicit take is the
way to move a supported payload out before teardown. Destructors cannot reenter
script; runtime destruction and extraction must not double-drop a payload.

### Methods and properties

The runtime-owned borrowed adapter is implemented through the existing `bind`:
use `MethodDecl::receiver_access(ReferenceAccess::Shared/Mutable)`, with an
explicit `&mut LoanContext<'_>` callback context and a flat outer tuple beginning
with `&T`/`&mut T`. The access enum is owned by `kagari_types::reference` and the
context by `kagari_runtime::native::typed::loans`. Current mutable Opaque admission
requires installed `NativeStorage::data`; shared access also supports registered
traced payloads. Automatic registration/derive and external views in the complete
example below remain subsequent combined gates.

Register a host type once, explicitly declare its callable members, then bind
implementations through the common binding API. This complete registration sketch
uses the existing implementation-group shape with proposed receiver access metadata
and lifetime-aware bind adaptation. All new spellings require a compiling proof;
it is not currently callable Rust API.

```text
struct Player { hp: i32 }

impl Player {
    fn inspect(&self) -> i32 { self.hp }

    fn heal(&mut self, amount: i32) -> i32 {
        self.hp = self.hp.saturating_add(amount.max(0));
        self.hp
    }
}

let mut engine_builder = KagariEngine::builder();
let mut module = ModuleBuilder::new("game", engine_builder.declarations())?;
// The descriptor also needs a proven storage/access capability for this payload.
let player_type = module.register_type::<Opaque<Player>>()?;
module.implement(player_type.clone(), |implementation| {
    implementation.inherent_impl(|methods| {
        let inspect = methods.define_method(
            MethodDecl::instance("inspect")
                .receiver_access(ReferenceAccess::Shared)
                .returns(Type::i32()),
        )?;
        methods.bind(inspect, |_cx, (p,): (&Player,)| Ok(p.inspect()))?;

        let heal = methods.define_method(
            MethodDecl::instance("heal")
                .receiver_access(ReferenceAccess::Mutable)
                .parameter("amount", Type::i32())
                .returns(Type::i32()),
        )?;
        methods.bind(heal, |_cx, (p, amount): (&mut Player, i32)| {
            Ok(p.heal(amount))
        })?;
        Ok(())
    })
})?;
engine_builder.install(module.finish()?)?;
let engine = engine_builder.build()?;
```

An automatic storage descriptor for this Player is a proposed convenience, not an
unverified safe trait assertion. A compiling prototype may use existing supported
native_data storage to establish the fixed-data case. Reuse declaration and bind
validation; `ReferenceAccess` describes required host access, not a second binding
entrypoint. The borrowed adapter receives a restricted context that cannot execute
scripts while these Rust references are alive. Complete-pack validation/conversion
precedes loan acquisition; returned data must not borrow from p. End the loan before
allocating/publishing the converted result, retaining the receiver/result roots as
required. Shared callbacks can have interior effects, so shared does not mean pure.

The same registered methods are available through each admitted representation:

```text
use game::Player;

fn on_reward(player: Player) -> i32 { player.heal(20) }
fn inspect_guest(player: Ref<Player>) -> i32 { player.inspect() }
fn reward_guest(player: RefMut<Player>) -> i32 { player.heal(20) }
```

Here bare Player means the registered native object, never a DTO with the same
name. `Ref<Player>.heal` rejects for insufficient capability; actual borrow conflicts
and foreign/stale/extracted objects are rejected at runtime. The host creates the
external views using the same descriptor:

```text
let mut external = Player { hp: 100 };
runtime.with_host_scope(|scope| {
    let view = scope.borrow_mut(&player_type, &mut external)?;
    runtime.execute_scoped(&loaded, "reward_guest", (view,), &context)
})?;
assert_eq!(external.hp, 120);

runtime.with_host_scope(|scope| {
    let view = scope.borrow(&player_type, &external)?;
    runtime.execute_scoped(&loaded, "inspect_guest", (view,), &context)
})?;
```

These scoped helpers remain reference-stage work. Rust cannot access or move the
lent object incompatibly until the scope ends. Scope exit revokes all views without
dropping or taking the host's payload. Registration is reused; ownership differs.

Properties require explicit getter/setter declarations and use the same shared or
mutable receiver checks. Opaque fields are not automatically exposed as ordinary
GC fields or projected paths. A returned Opaque<Child> must name a separately
registered, retained runtime object; an internal `&Child` is not such a handle.
Consuming Rust `self` methods are a separate adapter extension: the first API uses
explicit host `take`, never silently moves T out to satisfy a method parameter.

### Rust traits and Kagari traits

Rust and Kagari traits are independent systems. Implementing a Rust trait, matching
its name or deriving a Rust trait supplies no Kagari declaration, impl or interface
conversion witness. Exposing a Rust trait method as an ordinary registered method
also does not implicitly implement a Kagari trait.

To expose a Kagari trait implementation, explicitly register/resolve that Kagari
trait declaration, register an impl for the actual script receiver contract, and
bind each required member. Rust trait methods may implement those callbacks. For
example, a Rust `impl RustHealable for Player` can be used in a callback bound to an
explicitly registered Kagari `Healable` member; omitting that registration leaves
Kagari `T: Healable` unsatisfied. Source-free linking checks the same declaration,
receiver access, signature and impl requirements.

Optional registration sketch, added before finishing the same module. Assume the
Rust trait RustHealable is implemented for Player with `heal(&mut self, i32) -> i32`.
The explicit Kagari declaration below is separately named Healable; resolving a
trait already declared by another module would reuse its checked TraitRef instead.
The Kagari trait keeps its ordinary Self signature. Required Rust payload access
is metadata on this native impl member, not an imported Rust trait receiver rule.
The implemented impl builder sets this fact before binding with
`methods.receiver_access`. Installed declaration/binding validation checks it
without changing the trait's ordinary Self signature.

```text
let healable = {
    let mut declaration = module.define_trait("Healable")?;
    declaration.define_method(
        MethodDecl::instance("heal")
            .parameter("amount", Type::i32())
            .returns(Type::i32()),
    )?;
    declaration.finish()?
};
module.implement(player_type.clone(), |implementation| {
    implementation.trait_impl(healable.apply([])?, |methods| {
        methods.receiver_access("heal", ReferenceAccess::Mutable)?;
        methods.bind("heal", |_cx, (p, n): (&mut Player, i32)| {
            Ok(RustHealable::heal(p, n))
        })
    })
})?;
```

This registers a Kagari impl for the native Player receiver. Without it, Rust's
impl and the inherent heal binding alone do not satisfy a Kagari Healable bound.
The snippet does not assert an additional impl for Ref<Player> or RefMut<Player>.

Do not require three copies of inherent method registration or manufacture view
trait impls just because Opaque/Ref/RefMut share a target schema. This proposal does
not add a general view-to-trait forwarding or borrowed interface erasure rule. If
a concrete script generic/interface use needs it, its accepted receiver/capability
mapping must be explicitly registered and checked before use. Ordinary inherent
calls above do not depend on that future trait adaptation design.

## Configuration reload and future async use

The [update model](update-model-design.md) distinguishes compatible code publication
from fresh-environment state replacement. Value conversion is a reusable foundation,
not a whole-heap snapshot API. State replacement exports explicit data and business
IDs; Opaque objects and old runtime handles are not serialized into a new heap.
Resources are reattached only through checked host policy after writer quiescence.

For immutable configuration exposed without editing/extraction to its consumers,
a handle represents a fixed snapshot. Publishing a new configuration does not
retarget old handles. New lookups return the new snapshot; old payloads remain
retained while reachable unless the host explicitly takes them or tears down the
runtime. A service querying the latest configuration is a distinct API.

Script module reload and configuration data replacement are separate operations.
Preserve generation-pinned callable contracts and check type/schema compatibility;
keeping a payload alive does not authorize an old handle under an incompatible type.

The [async design](async-execution-design.md) may retain owned values and valid opaque
handles in suspended executions, subject to its root and lifecycle rules. This plan
adds no suspension support. Parked handles do not guarantee an object cannot be
explicitly taken; resumption validates liveness before payload access. Ref/RefMut
and active Rust loans cannot cross suspension. Thread-affine objects stay with their host
service; external completion threads do not directly access the script heap.

## Architecture and failure boundaries

Keep representation categories generic and metadata-driven. Derives and SDK builders
produce one binding descriptor used for offline declarations and runtime linking;
do not hand-maintain a second script signature catalog. Optional generated declaration
text is a projection, not another authoritative source.

Portable schema/ABI contracts contain no Rust pointers, Rust `TypeId`, trait objects
or callbacks. HIR checks ordinary nominal declarations; lowering consumes checked
representation facts. Runtime owns conversion safety and retained objects; the SDK
and derive implementation provide ergonomic registration. A proc-macro crate is
justified when implemented, not as an empty placeholder now. Executable contracts
must remain independent of frontend and derive dependencies.

Conversions validate types, limits and ownership, root intermediate script objects
across allocations, and release temporary state on failure. Large traversals check
coarse work/cancellation and bounded depth; they are not an unbudgeted Native loop.
Input conversion completes before invoking the business callback. Output conversion
failure cannot roll back effects that the callback already performed. Error messages
should identify the callable and nested field/index without leaking opaque contents.

Preserve schema/ABI validation and record contract changes; follow the unpublished
development policy rather than routinely bumping format/runtime identifiers.
Update consumers directly; do not keep duplicate old/new semantic implementations
or silently ignore obsolete metadata. This does not permit removing validation,
existing scoped borrow protections or typed-path behavior outside this scope.

## Implementation phases

RI00 inventory is recorded below. Reconcile the existing GO foundation in RI00 and reuse
implemented nominal declarations, roots and typed adapters. Follow the combined
order linked above rather than running this list as an isolated migration. RF01
supplies the borrowed adapter proof before RI03; RI04 supplies the schema/derive
capabilities required by RF04; RI05 closes integration after HR03 and RF05/RF06.

Design checkpoint: runtime-owned Opaque with read/edit/take replaces independent
external ownership in this proposal. RI03 reuses existing storage and owns the
new extraction/receiver adapter work; RF01-RF06 own external Ref/RefMut in the
same delivery. No implementation is marked complete by this planning checkpoint.
The selected API shape is explicit type/member registration plus bind, with Rust
and Kagari traits kept independent. Exact facade/adapter signatures and broader
payload capabilities require compile proofs; this checkpoint changes no runtime
behavior and reports no test or performance result.

- [x] RI00: Re-audit the completed predecessor and existing tests. Decide trait
  signatures, supported numeric/collection/enum forms, field exposure,
  Serde attribute matrix, fallible-call channel and destruction/thread policy.
  Fix ordinary-value alias/cycle behavior, catalog identity rules and phase scope.
  Inventory NativeObject reuse, the single public handle migration, payload
  capabilities and atomic take/invalidation. Coordinate the shared descriptor and
  lifetime proof with HR01/RF01; do not reimplement their binding prototype.
  Separate explicit Kagari trait registration from Rust trait implementations.
- [x] RI01: Introduce portable nominal value schemas and registration graph
  validation. Integrate ordinary HIR types, checked contracts, linking and artifact
  validation. Test offline registration, missing dependencies and atomic rejection.
- [x] RI02: Extend GO directional conversion contexts and typed Native adapters
  to the RI value-schema subset. Cover nested DTO conversion, limits and failures
  without adding another root or invocation implementation.
- [x] RI03: Build Opaque on existing runtime-owned storage/roots without T: Clone
  or parallel handle implementations. Add checked host read/edit/take, atomic alias
  invalidation and shared/mutable method adapters for proven payload capabilities.
  Reuse common declaration/bind validation and RF01's compiling lifetime proof.
  External Ref/RefMut scopes/projections are delivered by RF03-RF05, not RI03;
  keep this phase's adapter extensible only for those concrete argument sources.
- [x] RI04: Implement `KagariValue`, schema generation and the explicit optional
  Serde adapter against the accepted subset. Add compile-fail tests and an external
  embedding consumer proving that new structs and opaque types need no core edits.
  Coordinate finite inline field descriptors with RF04, which owns generated
  projector safety and execution; no second schema or handwritten projection API.
- [ ] RI05: Update SDK examples and interop/type/memory/reload specifications,
  remove superseded binding models within scope, and close integration with HR03
  and RF06. Record local and CI/Miri acceptance separately; do not retire this plan
  with required Opaque/reference integration outstanding.

At each checkpoint record commands, actual results, any carried failures and their
owning follow-up phase here. Do not conceal failures with stubs or weakened tests.
The default is a buildable checkpoint; any intermediate-breakage policy needs
explicit approval when activating this plan. Implementation commits, when separately
authorized, follow repository Conventional Commit and breaking-change rules.

### RI00 activation inventory and decisions (2026-10-15)

Reuse `native/conversion` directional traits and contexts. Existing primitive
converters support unit/bool, i8/i16/i32/i64/isize, u8/u16/u32/u64/usize, f32/f64
and String; integer decoding already uses checked widths. Initial automatic schemas
cover named non-generic structs and unit/tuple enum variants, with tuples,
Option/Result, Vec and explicit registered collections composing the same mapping.
Unsupported char/i128/u128, borrowed strings/slices, unions, packed structs,
variant-dependent projections and open Rust generic factories diagnose rather
than coerce. Named enum payloads require a separately defined portable shape and
are excluded from the initial automatic subset. Generic manual declarations and
converters remain supported by existing checked applications.

Fields exported by a DTO schema are public writable script fields. Registration
supplies no methods or Kagari impls. Derives supply a short name (optional
`#[kagari(name = "...")]`) and dependency graph, never a module-qualified process
identity. Rust TypeId locates only local mappings; duplicate mappings in the same
catalog reject, and independent catalogs may choose independent module names.
A DTO and Opaque representation require distinct explicit binding types. Register
complete headers before resolving recursive dependency edges, and validate the
staged graph before publication; a failed registration publishes no usable subset.

Owned conversion duplicates repeated noncyclic children into independent Rust
values and rejects a cycle into an active owned conversion. Existing
`ConversionContext::active` owns that rule, with bounded depth/nodes and scratch
roots; Opaque/other retained handles preserve identity instead of decoding their
contents. Into/From capabilities stay directional. `NativeResult<R>` carries call
failure; business `Result<T,E>` remains ordinary script Result data. Output
conversion failure preserves completed callback effects.

Opaque replaces only the public NativeObject handle, using the same RootedValue,
NativeType descriptor, program retention and GC slot/generation. Read retains
NativePayload tracing/accounting. Direct edit initially retains NativeData's
recursive fixed-size edge-free contract; managed script edges keep controlled
setters. Extraction requires an explicit proven edge-free capability independent
of Clone/Copy, so non-Clone payloads can be retained and supported payloads taken
without copying. An empty runtime trace is not evidence for this capability.
Managed script-edge extraction is excluded until ownership/root transfer is
proved. RI03 adds this capability at the existing storage owner and atomically
removes a validated slot after rejecting active loans/native operations; all old
roots/aliases then fail generation/liveness checks. Failed take changes nothing.
Reinsertion uses a new generational identity. Payload accounting is released by
the heap owner, and destruction transfers exactly once to the caller.

Payloads remain Send + 'static, without a blanket Sync requirement. Runtime
teardown drops remaining payloads; retained handles cannot independently access
storage after teardown. Drop cannot reenter script. External scope lending is
separate from heap ownership and cannot take the host object. Existing
HostResourceScope is a runtime root/borrow guard, not yet a Rust host-reference
lifetime proof; RF01 supplies that proof and RF03 links actual lending.

Optional Serde is explicit `Serde<T>` plus KagariSchema and directional
Serialize/DeserializeOwned. Initial accepted shape: supported primitives,
named structs, unit/newtype/tuple enum variants, tuples, Option/Result and supported
collections. Symmetric short `rename`/`rename_all` and unconditional skip/default
must agree between schema and conversion. Reject flatten, conditional omission,
untagged/internal/adjacent tagging, asymmetric names, borrow, custom serializers,
remote and opaque handles unless a separately authored matching schema/converter
proves the shape. No sample serialization or opaque-ID encoding supplies schema.
RI04 implements diagnostics against this finite matrix rather than silently
using raw Rust fields when Serde changes representation.

[HR01/RF01's ledger](host-registration-plan.md#progress-ledger) owns the shared
migration/receiver/scope proof. RI00 does not build a competing adapter. Initial
code inspection found no production failure; focused primitive/argument agreement
checks are recorded there. Automatic schemas/derive, native extraction, scope/path
execution and their tests are still required. Local acceptance selects those
changed contracts plus directly affected regressions; exhaustive workspace,
feature/backend/platform CI and actual projector Miri remain separately owned.

### RI01 schema checkpoint (2026-10-15)

Implemented catalog-local Rust representation mappings and transactional
`ModuleBuilder::register_type`. The KagariType registration hook visits a bounded
graph, reserves nominal headers before recursive children, and rejects unfinished
headers even when a custom schema catches a child error. Failed staging may intern
disposable identity records but publishes no declarations, mappings or bindings.
Repeated dependencies reuse their header; explicit duplicate registrations and
cross-module rebinding of one Rust representation reject atomically. Mappings
carry no serialized TypeId; existing portable TypeDef/FieldDef/VariantDef own shape.

Existing ModuleDecl validation now admits ordinary structs and validates their
fields, identities and nominal dependencies. HIR renders writable struct fields
and processes them as ordinary structs, with generated field navigation sites.
The dependency collector now retains Struct contracts rather than overlooking
them. Source-free installation checks exact registered ordinary schemas even
when a module has no native calls. An independently valid artifact with the same
nominal identity and changed field capability is rejected against the installed
schema; internal layout corruption also remains rejected by bytecode validation.
Vec/Option/Result/tuple registration recursively visits mapped child dependencies.
DTO registration creates no Rust method or Kagari trait exposure. RI04 still owns
automatic derive output; these tests use explicit manual schema hooks.

Five selected `native_schema` cases passed across focused runs: recursive offline
schema/rendering, missing dependency rollback, caught-child rollback, catalog-local
identity/batch install and source-free schema agreement. Source-free cases encode,
decode and verify KBC artifacts before loading. Initial test-only failures used a
private Type comparison and assumed qualified names within the same module;
corrected to portable owner comparison and the renderer's existing local spelling.
The corrupted schema case was strengthened from inconsistent layout mutation to
an independently compiled/verified provider, proving installed agreement directly.
No production failure remains from these runs. `cargo check -p kagari-runtime --lib`
and `cargo check -p kagari-hir --lib` passed. Runtime lib/schema-test scoped Clippy,
structure, format and diff checks passed before checkpoint (final test edits were
rechecked). The existing nested owned-value/GC-root conversion regression also
passed after the catalog/loading changes. No unsafe projector is introduced here; Miri remains RF04/RF06-owned.
Full workspace/feature/backend/platform acceptance is pending CI.

### RI02 checkpoint: nominal directional conversion

Ordinary struct conversion shares the existing prepared `StructLayoutRef`,
`ObjectType`/field mapping and rooted `ObjectBuilder` construction. It adds no
second heap object constructor. Decode roots a complete field snapshot before
child converters, so alias mutation and collection during one child do not change
later snapshot values. Enum conversion shares complete-pack argument validation,
rooting and runtime enum allocation. Field/variant error context preserves the
original error kind and execution trace. Directional adapters remain independent;
no Rust borrow is manufactured from a DTO. Derive generation remains RI04-owned.

The selected source-free VM case passes `Packet { hp: 7, shared: Vec::from([1]) }`
through a typed callback. The returned hp becomes 8 while the input remains 7;
its explicitly retained vector handle shares identity and both aliases see the
appended element. Ordinary nested Tree/Choice conversion runs under collection
threshold 1; limits, incomplete/duplicate initializers, child panic cleanup and
snapshot stability are covered at the runtime owner. The existing enum cycle case
now exercises these helpers: shared noncyclic children copy successfully, an owned
cycle rejects, and explicitly retained children preserve their identity.

Validation: the seven initially selected `native_schema` cases passed, followed by
the added `dto_snapshot_survives_alias_mutation_collection_and_child_unwind` case.
`native_conversion::owned_conversion_rejects_a_cycle_but_accepts_shared_noncyclic_children`
and VM `nominal_dtos_use_typed_callbacks_while_explicit_handles_keep_shared_identity`
passed. Runtime lib plus these two runtime test targets passed scoped Clippy.
The first library check found the existing object preparation method private to
its module; it is now crate-visible because DTO conversion is a genuine consumer
of the same retained owner, without a forwarding facade or new public API.
No carried build failure remains. VM test-target Clippy, structure (1183 files, zero findings), formatting
and diff checks also passed. Full feature/backend/platform/workspace coverage
is CI-owned; this phase introduces no unsafe projection requiring Miri.

### RI03 implementation decision: moved aliases and collection

The existing GC requires every traced ID to resolve. Simply freeing a taken native
slot would leave its old roots, script fields or pinned receiver captures pointing
to a missing object, which quarantines collection. Extraction therefore needs an
explicit, inert moved-identity record at the GC owner: it proves that this exact
heap/slot/generation was invalidated by successful take, carries no payload or
executable state, and is terminal only for collector traversal. Ordinary access,
conversion and publication checks continue to reject it. Foreign/fabricated or
otherwise stale IDs remain invalid; collector tolerance is limited to the proven
moved identities.

Take validates capability, payload identity, generation, active accesses and
fallible bookkeeping before detaching storage. It returns the original Rust value
without cloning, increments/retires the old slot generation and makes the slot
reusable while old aliases still exist. Payload accounting is released; one logical
heap unit remains charged for each moved identity until collection proves no root,
heap or metadata edge retains it, then prunes it. This bounds identity bookkeeping
under repeated create/take without claiming zero-cost invalidation. There is no
second payload/object system or revived alias on slot reuse. Managed script-edge
extraction remains outside the proven RI00 subset; an empty trace is not proof.

The shared owned-registration gate is implemented. HR02's live-host declaration
migration still requires the reference target/capability facts and usable external
scopes; it remains open while RI03/RF02-RF05 supply these concrete dependencies.
This does not relax the final combined acceptance or retire old live-host services
before their checked replacement is integrated.

### RI03 checkpoint: Opaque ownership extraction

The public rooted NativeObject handle is replaced directly by Opaque; constructors,
managed wrappers, conversion and method consumers use the same storage and roots.
There is no public compatibility alias. NativeTake explicitly proves edge-free
extraction independently of Clone/Copy/Sync; installed storage must separately
grant take capability. NativeData grants this proof recursively. Traced and managed
storage does not infer it from an empty trace. GC take preflights loans, active
operations, runtime/generation, Rust payload identity and fallible bookkeeping,
then detaches the original value and invalidates every old alias atomically.

Focused VM native-boundary tests passed: non-Clone take with duplicate script/host
aliases, parked ScriptValue, active-read rejection, same-slot/new-generation reuse
and exactly-once destruction; denied extraction with unchanged payload/accounting;
stale edit and pinned method calls after reinsertion; existing checked-data edit
cleanup on errors/unwind. The existing runtime malformed executable-edge collection
test also passed, preserving rejection before detachment. An initial slot-reuse
fixture retained an unrelated temporary script array until the post-take collection;
collecting that temporary before take made physical slot reuse deterministic without
weakening the assertion. No production failure remains. Runtime lib check, scoped
runtime-lib/VM-native-boundary Clippy, structure (1186 files, zero findings), formatting
and diff checks passed. Missing captured output from three completed checks was
recovered by focused reruns, rather than assuming success.

RI03 remains open for borrowed invocation adapters shared with the reference phases.
This slice adds no raw projection; actual projector Miri remains RF04/RF06-owned.
Exhaustive workspace/feature/backend/platform validation remains pending CI.

### RI03 checkpoint: registered lifetime-aware methods

Shared/mutable callback tuples now use one `NativeAdapter` selection and the
existing contextual invocation/conversion boundary. `LoanContext` permits polling
without exposing heap or script execution services. The HRTB callback and static
owned result prevent borrowed output/retention. Declaration, builder and installed
binding checks agree on receiver access; raw/owned adapters cannot claim it.
The common boundary checks Rust mappings and installed mutation capability before
converters, then decodes a rooted complete pack before lending. Pack roots remain
through result conversion, after the payload guard has released access.

The internal erased editing path recovers its fixed-data proof from the immutable
storage flag and matching Rust TypeId. Only `NativeStorage::data<S: NativeData>`
grants the flag; public Opaque::edit still requires NativeData. This permits the
same eventual callback shape for external non-Copy targets without granting
traced/managed/take-only Opaque storage unrestricted mutation. Existing GC storage
guards, revision publication, execution exclusion and cleanup remain the owners.

Access may also be set on a concrete native trait impl before bind. The required
Kagari Self signature stays independent: trait implementation comparison permits
this local access fact while retaining all other policy/signature checks, and the
ordinary native declaration/installed binding validators verify its actual target
and adapter. A Rust trait method supplies one explicitly registered callback; a
separate script impl of the same Kagari trait still uses ordinary script Self.

Selected VM checks passed: prepared/pinned methods, result String allocation under
collection threshold 1, foreign/runtime and pinned reload checks, explicit native
trait callback plus independent script impl, rejected callback/access mismatch with
successful retry, denied direct editing on same-Rust-type non-data storage, and
take alias/pinned-method rejection after reinsertion. Two production LoanContext
compile-fail doctests passed for returned/captured references. Initial fixture
failures were an unavailable String constructor spelling and an unconstrained
panic-only callback output; both were corrected. A trait-builder initializer field
was placed on the adjacent inherent builder in the first edit and corrected after
the focused check. No carried failure remains. Scoped runtime/VM native-boundary
Clippy, structure, formatting and diff checks passed. These checks establish the
runtime-owned RI03 gate; shared Ref/RefMut transport, root/path loans, mixed borrowed
packs and final source-free integration remain RF02-RF05. No new unsafe projection
is introduced; projector Miri and exhaustive matrices retain their RF/CI owners.

### RI04 checkpoint: dedicated DTO derive and automatic Opaque dependencies

The new proc-macro crate generates finite named structs and unit/tuple enums through
TypeRegistration and existing rooted struct/variant converters. It depends on Rust
syntax/token libraries only; the executable/runtime production graph does not
depend on a derive or frontend. Rust definitions in the embedding consumer supply
short names (optionally `#[kagari(name = "ShortName")]`), schemas and conversion.
The module still owns namespace and catalog-local identity. No methods/traits or
Rust references to ordinary GC objects are inferred.

NativePayload supplies a checked local `registration_storage` policy for automatic
Opaque dependencies. Default traced payloads grant read access; NativeData's existing
unsafe proof selects data/take storage. Manual NativeTake payloads may select their
explicit takeable factory. Registration rejects a factory with a different Rust
payload identity atomically. Opaque's contextual checks remain available for explicit
generic native applications; automatic DTO fields reuse the wrapper's catalog
mapping. This replaces the earlier always-error schema hook without introducing
another registry, public handle alias or GC object model.

The existing recursive Tree and Choice fixtures now use the derive; Leaf retains
its custom converter because it owns alias-mutation and child-unwind coverage.
Focused recursive DTO/enum conversion, schema rendering and snapshot/unwind tests
passed. Mixed DTO conversion with an Opaque child preserves the same payload, data
editing and all-alias invalidation after take; wrong factory identity rejects without
publishing a mapping. Eight production compile-fail doctests reject generic/borrowed,
tuple/packed/union/record-enum and unsupported scalar forms. Initial test placement
used the wrong integration-root module path; fixed with the ordinary nested owner.
A first test-name filter selected zero tests; the actual nested DTO test was run and
passed. Structure initially found qualified import paths; they were corrected.
Scoped derive/runtime lib/schema Clippy and consumer-example Clippy passed.

`cargo run -p kagari --example value_interop` passed, printing
`Applied(Response { amount: 40 })`. This embedding consumer defines its own DTOs,
enum and Opaque payload, compiles/loads an artifact and uses the ordinary typed
call boundary. The example and mixed test require no new core type switches.
Dedicated derive output does not yet supply inline projector facts. At this
checkpoint optional Serde, ordinary Struct lending and projector integration remained
RI04/RF04 work; the next checkpoint supplies Serde. RI04 stays open.
The earlier whole-object Miri selection is still running under RF03, without a
passing result. No generated unsafe projection exists in this derive slice. Full
workspace/feature/backend/platform validation remains CI-owned.

### RI04 checkpoint: direct optional Serde and static serialization schemas

Runtime's opt-in `serde` feature supplies `Serde<T>` plus `KagariSchema`; SDK forwards
that feature. Dedicated derive produces module-owned finite static declarations,
with schema identity keyed by the wrapper, rather than discovering sample shapes.
Primitive/composite schemas reuse the existing type constructors. Production
Serializer/Deserializer invoke Serde directly: checked ObjectField preparation,
ObjectBuilder initializers and complete struct/sequence/enum snapshots remain at
their existing owners. Child adapters use ConversionContext roots, cycle/depth/node/
string limits and cancellation. No JSON/buffer/tree intermediate, pointer
reinterpretation, alternate call dispatcher or Serde Opaque-ID mapping is introduced.

Supported automatic shapes are owned supported scalars/String, Vec, tuples,
Option/Result, named structs and unit/newtype/tuple enums. Symmetric short names and
rename_all follow Serde's field/variant casing; unconditional skip/default are
honored, including skipped tuple/newtype payloads. Names must be script identifiers.
Changed tagging, flattening, conditional omission, asymmetric/custom/borrow/remote
attributes reject during derive. Unsupported scalar and Opaque fields have no schema.
Serialize and DeserializeOwned are independent capabilities. GC structs always have
all declared fields; default does not turn incomplete objects into valid inputs.
Serde schemas describe serialization, never Rust field layout or projector safety.

Six selected Serde cases passed across the schema test owner: source-free nominal
round trips, all variant shapes, empty Vec/Option, Result, renames/skips/default,
directional-only types, actual custom Serialize behavior, limits/error paths,
visitor mutation+GC+unwind snapshots, cycles, bounded collect_str and duplicate-field
preflight before child effects. The preexisting nested DTO/enum round-trip regression
passed without the Serde feature after sharing sequence snapshot preparation.
Ten compile-fail doctests reject the unsupported attribute/shape matrix and prove
DeserializeOwned even with an explicitly authored borrowed schema. Initial consumer
callbacks omitted the required owned NativeContext parameter; corrected the example,
without adding a callback API. Initial structure/import warnings were corrected.
No carried build or test failure remains.

Local selection and actual results:

- `cargo test -p kagari-runtime --features serde --test native_schema native_schema_serde`: five cases passed before the final two local repairs; the additional
  `serde_formatted_output` case validates those repairs. Unchanged tests remain evidence.
- `cargo test -p kagari-runtime --features serde --doc native::serde::KagariSchema`:
  ten compile-fail cases passed, including the corrected explicit borrowed schema.
- `cargo test -p kagari-runtime --test native_schema nested_dtos_and_enum_payloads`:
  one directly affected shared-conversion regression passed.
- Scoped derive/runtime/schema and SDK example Clippy, core runtime Clippy with
  Serde disabled, structure, formatting and diff checks passed after the selected
  repairs. The structure checker reviewed 1214 Rust files with zero findings.
- `cargo run -p kagari --example serde_interop --no-default-features --features source,serde`:
  passed; the consumer's compiled/loaded typed call prints `Applied(7, 20)`.

RI04 still owns ordinary Struct descriptors and coordination with RF04's generated
projectors; no projector or unsafe change is added by Serde. The earlier selected
whole-object RF03 Miri invocation is still running, without a passing result. Full
workspace/feature/backend/platform coverage and final projector Miri remain CI/
RF04/RF06-owned; the combined plan remains open.

### RI04/RF03 checkpoint: common Rust targets and ordinary Struct lending

`native::rust_type::RustType<T>` is one metadata-only applied target descriptor.
NativeType owns its storage policy plus that same descriptor; ordinary registered
structs bind it directly through Runtime. External scope/loan APIs now require
Send + 'static rather than NativePayload. Every scope retains runtime exclusivity,
invariant brand, owner/generation, exact Rust identity, capability and deterministic
revocation. No new object storage, public NativeObject alias, copying or GC-to-Rust
reinterpretation is introduced.

Private native read/downcast adapters recover T from exact installed storage TypeId;
all creation/tracing factories still require NativePayload, direct Opaque editing
still requires the sealed fixed-data flag, and take still requires its independent
proof. The same NativeAdapter receiver path accepts external ordinary structs or
native payloads. Owned GC structs cannot supply Rust loans: source member lookup,
MIR/bytecode call flow and runtime preflight enforce that distinction. Ordinary DTO
round trips and explicit methods remain separate. Native constructors/descriptors
and existing scoped callers migrate directly to the common target API.

The ordinary Struct source-free fixture passes Ref/RefMut to one explicitly bound
method group, mutates the original Rust object, rejects writer reentry during a
shared loan, preserves independent DTO copies, and keeps a returned heap String
rooted through a later scoped call/collection. Scope results now retain temporary
roots until exit. Zero-sized root reservations no longer conflate independent Rust
places sharing an address; copying one view still preserves its alias identity.

Actual focused evidence: two `scoped_struct` VM cases passed; six existing native
payload/method cases passed (generic evidence, explicit traits, take/pinned calls,
external reentry/cancellation/depth/unwind); five production scope compile-fail
proofs passed. Core/compiler library checks and scoped contract/HIR/MIR/bytecode/
runtime/VM Clippy passed. The directly migrated scoped example runs and prints
`owned hp=70, borrowed hp=70`.

The original full VM Miri selection was stopped after 65 minutes without a result;
it is not counted as passing or a diagnosed safety failure. Its source analysis and
standard-library construction ran inside the interpreter. Replaced local safety
selection with the actual pointer/heap/frame boundary under
`tests/scoped_loans/mod.rs`, without a substitute implementation or weakened checks:

- `cargo +nightly-2026-07-27 miri test -p kagari-runtime --test host_scopes external_root_actual_pointer -- --nocapture`: one case passed, including nested compatible
  reads, writer conflicts, completed edits on errors, loan/scope unwind, GC, stale
  revocation and healthy reuse.
- `cargo +nightly-2026-07-27 miri test -p kagari-runtime --test host_scopes distinct_zero_sized -- --nocapture`: one case passed; independent zero-sized roots admit
  simultaneous unique loans and aliases of one root still reject.

Both corresponding ordinary tests passed. Runner: aarch64-apple-darwin,
nightly-2026-07-27 (1.99.0-nightly), workspace test profile/default parallelism,
MIRIFLAGS unset; no alias checks disabled. Miri ignores that profile's optimization
level as documented by its warning. These results establish the RF03 whole-object
pointer gate; generated projector Miri and exhaustive backend/platform CI are still
pending. Initial module placement and a test's nonexistent Type::string helper were
corrected; no carried production/test failure remains. RI04 still owns finite inline
field descriptors in coordination with RF04. The combined goal remains active.

### RI04/RF04 checkpoint: finite generated field layout descriptors

`KagariValue` now emits a local `GeneratedField` list for named ordinary inline
structs. The existing staged `TypeRegistration::structure` validates each field's
name, Rust root identity and semantic child type against the same schema before
publication. Catalog Rust mappings carry the immutable list through ordinary
imports, dependency closure and native module installation. Portable declarations
retain their existing field identities/types; no Rust address, offset or projector
function is serialized. Manual converters and `KagariSchema` supply no layout proof.

The generated function uses `addr_of_mut!` directly on the raw root. It creates no
ancestor reference. The hidden unsafe constructor documents the direct initialized,
aligned inline-place contract; deriving still rejects packed structs, unions and
variant fields. A retained Opaque field describes its Rust handle wrapper, never
the payload behind that handle. Function addresses are not semantic identities;
mapping compatibility compares exact Rust root/field identities and schema types.

Focused runtime projection tests passed (two cases): simultaneous disjoint
Stats/Inventory references from generated pointers, and transactional rejection of
a mismatched field name. Recursive-header/schema and source-free schema-loading
regressions passed. Scoped runtime/derive library Clippy, formatting, the structural
checker (1219 files, zero violations) and diff checks passed. The raw
projection test also passed under `nightly-2026-07-27` Miri on aarch64-apple-darwin
with MIRIFLAGS unset, workspace test profile/default parallelism and Miri's default
target directory. This proves the generated address boundary only. Runtime path
handles, atomic loan packs, replacement/reentry and their Miri gate remain RF04;
checked field operations remain RF02, and SDK/live-host integration remains RF05.
RI04's schema/derive/Serde and finite descriptor responsibilities are complete;
the combined delivery remains open. Full workspace/feature/backend/platform checks
remain CI-owned.

### RF04 checkpoint: scoped inline locations and one path loan owner

Rust scopes now prepare `RustField<T, F>` from generated layout proofs pinned by
the common RustType descriptor's immutable catalog and Program lease. A scoped
field view retains a root/place identity, target and generated field-identity
chain; it retains no field address. Every read/edit resolves the reserved raw root
through that chain after acquiring its path loan. Equal/prefix paths overlap,
diverging generated structural paths are disjoint, and the empty path preserves
whole-root access. Existing installed host tokens keep empty paths in their separate
object identity domain; one borrow table/guard/count owner checks and releases both
representations. Opaque's non-reentrant GC payload adapter is unchanged.

Shared views inherit shared access. Ordinary schema or Serde metadata supplies no
field proof. Target/Rust identity, owner, generation and capability checks precede
projection. An active field prevents incompatible ancestor/replacement access;
an idle nested view reads newly replaced contents on its next access. Root addresses
remain reserved for the invariant scope lifetime. No mutable ancestor reference is
created while disjoint field references coexist.

Focused local results: the runtime scoped field fixture passed normally and under
Miri (`nightly-2026-07-27`, aarch64-apple-darwin, MIRIFLAGS unset, default Miri target,
workspace test profile/default parallelism). It covers simultaneous disjoint unique
loans, shared overlap, duplicate/ancestor/whole-root conflicts, nested scalar access,
replacement and unwind cleanup. All three small actual-pointer scope fixtures,
the five legacy borrow-owner tests and five production lifetime compile-fail
doctests passed. Scoped runtime library/host_scopes Clippy, structural review
(1220 files, zero violations), formatting and diff checks passed.

This closes only the scoped Rust field boundary. Full argument-pack preparation/
atomic loan acquisition, scalar script reads/prepared assignments, checked portable
field operations and pinned-projector execution integration remain RF04/RF02/RF05.
The combined checklist remains open and exhaustive matrices remain CI-owned.

### RF04/RF05 checkpoint: common atomic borrowed argument packs

The existing bind adapter now accepts a shared/mutable free-function input and
all four two-input shared/mutable combinations, followed by owned arguments (total
arity at most twelve). The first input may instead be the explicitly declared
Opaque method receiver; the second reference input is an external scoped location.
One method body serves Opaque and external receivers. There is no new invocation
entrypoint, general FromKagari for Rust references, or simultaneous Opaque payload
borrowing model. Runtime-owned payload access retains its original GC loan and
storage edit proof; external references use the existing scoped path owner.

The contextual invocation still roots the complete pack and decodes owned inputs
before acquiring loans. The scoped owner validates every target/capability, acquires
all path records under one cleanup guard, and only then computes raw field pointers
and constructs HRTB references. A rejected later acquisition releases earlier records
without callback entry. Callback error/unwind releases the complete pack and preserves
completed writes. Result conversion follows release. Single scoped loans use the same
pack path. No bookkeeping borrow or mutable ancestor spans callback execution.

Integration initially failed because native module authoring still rejected Ref
parameters, then because TypeArgument lacked reference-target derivation. The first
failure occurred at ModuleBuilder::finish (unsupported initial native API type); the
second occurred during tick's native call (derived type parameter). Both are repaired:
native authoring admits already-checked direct parameter views, without admitting
view implementation receivers/bound targets, and reference derivation reuses the
existing origin-preserving TypeArgument child mechanism. Storage, return, capture,
suspension and target validation remain in their original contract owners.

Focused results passed: source-free native boundary free-function packs with
disjoint generated fields, shared weakening, duplicate unique rejection before
callback and once-only completed owned-conversion effects; the same mutable pair
method on Opaque and external receivers; six existing receiver/generic/trait/take/
scope regressions; three production borrowed callback compile-fail doctests.
The actual atomic-path kernel passed normally and under Miri (nightly-2026-07-27,
aarch64-apple-darwin, MIRIFLAGS unset, workspace test profile/default parallelism,
default Miri target). It exercises all four combinations, partial acquisition
cleanup, ancestor conflict, error/unwind and retained writes. Scoped runtime/types/
VM Clippy, structure (1223 files, zero violations), formatting and diff checks passed.
No carried build/test failure remains; exhaustive matrices remain CI-owned.

RF04 remains open for script scalar reads/prepared assignments and pinned-projector
execution; RF02 owns checked portable field operations. RF05 still owns SDK context,
live-host migration and final source-free/capability integration. During that integration,
audit weak Ref parameters and raw/reflective callback forwarding: formal shared access
must not become a mutable capability merely because the original caller supplied
RefMut. Any necessary temporary capability restriction must preserve the original
mutable alias and use the common checked location owner. This is an existing
no-upgrade requirement, not a new permission matrix or a claim of an observed exploit.

## Acceptance and verification

Required coverage includes:

- Primitive, struct, enum and nested composite round trips; directional-only types;
  range/type errors; ordinary copy isolation and documented graph rejection.
- Mixed value/opaque fields across arguments, returns, properties and containers.
- Non-Clone payload retention without copy; shared/exclusive access and capability
  rejection; reference escape compile-fail coverage and no unguarded dereference.
- Take with inactive aliases, active-loan rejection, failed-take atomicity, repeated
  take/access rejection, slot reuse/reinsertion, parked handles, pinned methods,
  accounting/traced-edge cleanup and exactly-once destruction after extraction.
- Explicit shared/mutable method registration, receiver/argument batch validation,
  and rejection of inferred Rust-trait exposure. Ref variants are tested in their
  owning phase; nominal data registration alone does not expose host methods.
- Script reachability, host retention, GC during conversion/callbacks, reentry,
  cancellation/traps, last-owner destruction and runtime teardown without leaks.
- Foreign runtime/type, stale generations, fabricated object IDs and incompatible
  reload contracts rejected through every entry path, including artifacts.
- Module-derived names, short-name overrides, duplicate registration rejection,
  dependency cycles and offline declarations without invoking host code.
- Serde's approved shape/attribute matrix, empty collections and enum variants;
  unsupported/custom shape diagnostics; no opaque-ID serialization escape hatch.
- Input failure before business effects and output failure after completed effects;
  heap/work/depth limits and cleanup of partially constructed values.
- Synchronous configuration snapshot retention across data publication, separately
  from script generation pinning. Async suspension tests belong to async activation.

Local validation selects affected payload/registration/conversion and a small
dependent regression set, plus lightweight structure/format/diff checks. Full
workspace and feature/backend/platform acceptance belongs to CI:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Compare repeated large-configuration opaque calls against by-value DTO conversion.
Verify payloads are not deep-copied and object tables are reclaimed, rather than
claiming zero allocations or zero-cost property access. Record toolchain, machine,
profile, features, default Cargo parallelism, cache state and workload; separate
compilation from execution. Store temporary output under ignored `target/` and keep
durable conclusions here. No benchmark result is claimed by this proposal.
