# Rust Value and Opaque Interoperability Plan

Status: queued, documentation only. This plan defines typed Rust value conversion
and retained opaque objects without exposing Rust borrows to scripts. It records
the agreed behavior and proposed binding API; the examples are target API sketches,
not interfaces that exist today. Implementation and commits are not authorized by
this documentation task.

The later [runtime ownership and host object design](runtime-ownership-and-host-api-design.md)
owns central runtime storage, automatic script-object roots, the Send runtime
contract, common typed native adapters and managed-payload editing. RI reuses that
foundation and retains DTO derives, Serde and independently retained external
Opaque objects. Its phases extend accepted GO capabilities rather than implement
a second conversion or root system. Editing a runtime-owned payload does not grant
exclusive access to an externally shared Opaque object.

Build on the current [native registration](spec/standard-declarations.md) and
[installation-based access model](spec/execution.md). Activation and scheduling
belong to the [roadmap](implementation-roadmap.md). Synchronous interop does not
depend on async. Re-audit affected APIs at activation; existing specifications
remain in force until an implementation updates them.

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
- Opaque objects may expose modifying methods. In this version Rust objects own
  their safe interior mutation; the wrapper does not manufacture exclusive access.
- Registration and exposure authorize use under the current execution model.
  Properties have declared writeability, not additional runtime permission flags.

Excluded here are script-visible `&T`/`&mut T`, automatic binding of exclusive
`&mut self` receivers, borrowed strings/slices, borrowed subobject projection,
revocable entity handles, and open Rust generic factories. Existing scoped borrow
and typed-path mechanisms are not deleted or relaxed by this proposal.

## Current implementation boundary

The [host interop specification](spec/host-interop.md) already separates portable
declarations from runtime bindings. The following are useful foundations, not
proof that this proposed SDK exists:

- [Host value declarations](../crates/kagari-types/src/host_interface/value_type.rs)
  include primitives, containers and opaque declaration identities, but not a
  complete portable ordinary struct/enum binding model for this proposal.
- [Type and member declarations](../crates/kagari-types/src/host_interface/type_declaration.rs)
  describe fields and methods; method contracts include a typed receiver.
- [Host handles and callbacks](../crates/kagari-runtime/src/host.rs) provide nominal
  type and registry checks. Current callbacks receive low-level script values.
- [Root registration](../crates/kagari-runtime/src/host/registry.rs) records handle
  information. It does not itself store and retain an arbitrary Rust `T`.
- The [host interface example](../crates/kagari-embed/examples/host_interfaces.rs)
  separates offline compilation and runtime implementation installation.
  [Integration tests](../crates/kagari-embed/tests/host_interfaces.rs) cover existing
  rooted interface behavior including GC, reentry and reload.

The automatic derives, typed function adapters, Serde bridge and owned opaque
storage below require new work. Do not infer their availability from the existing
`HostTypeOwnership` enum or reinterpret old non-owning handles as owning objects.

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
| `Opaque<T>` | Registered opaque nominal type | Retain or resolve a typed object handle |
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
from the same definition:

```rust
#[derive(KagariValue)]
struct DamageRequest {
    target_id: i64,
    damage: i32,
}

let game = registry.module("game");
game.value::<DamageRequest>()?;
```

Typed function registration then derives its signature and boundary adapters:

```rust
fn echo(request: DamageRequest) -> DamageRequest {
    request
}

game.function("echo", echo)?;
```

Kagari sees `game::DamageRequest` as an ordinary nominal script value type. Data
converted from Rust is a snapshot: mutating that script object does not update the
original Rust object. Once created, it obeys ordinary Kagari shared-object rules;
copying a script variable is not another deep serialization.

The derive must make field exposure and script writeability explicit and documented.
The proposed default is declared fields with read-only bindings; an explicit derive
option may expose mutable script fields. Such mutation still affects only the
converted snapshot. Unsupported field/variant forms produce diagnostics. A manual
adapter is available for external types or deliberately different representations.

Mixed value and opaque data use the same recursion:

```rust
#[derive(KagariValue)]
struct BattleInput {
    damage: i32,
    config: Opaque<GameConfig>,
}
```

Only the outer structure and ordinary value fields are converted. `config` retains
the same Rust object. Returning such a structure follows exactly the parameter rule.
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

The registration module owns the path:

```rust
#[derive(KagariValue)]
#[kagari(name = "DamageInput")]
struct DamageRequest {
    target_id: i64,
    damage: i32,
}

let game = registry.module("game");
game.value::<DamageRequest>()?;
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

The main construction API takes ownership and hides storage choices:

```rust
let config = Opaque::new(GameConfig::load());
let another = config.clone();
```

`GameConfig::load` is an application placeholder here. `Opaque<T>: Clone` must not
add a `T: Clone` bound or invoke the payload's clone implementation. Both handles
denote the same object. No `DerefMut`, implicit deep copy or automatic
exclusive access is provided. Shared Rust access is available for implementing
methods; references obtained there never become script values in this design.

Objects must not contain borrowed data with a lifetime shorter than their retention;
the proposed first binding requires `T: 'static`. This does not mean the object
lives forever. The GO thread-transfer contract requires Send for runtime-owned
payloads. An independently shared Opaque owner must additionally satisfy the
requirements of its storage and access model; for example, sharing Arc<T> across
threads requires T: Send + Sync. Thread-affine objects remain in external host
services. This replaces the earlier assumption that every Opaque payload can stay
inside a fixed-thread isolate without transfer constraints.

Observable lifetime requirements are:

- Live Rust wrappers and reachable script handles retain the payload.
- Script assignment and repeated conversions of a cloned wrapper preserve object
  identity within the runtime; they do not allocate independent payloads.
- Temporary conversions, Native calls, reentry and traps retain valid objects until
  cleanup can safely release them. Failed publication must not leak permanent roots.
- Dropping a Rust wrapper does not invalidate a script handle that still retains
  the object. After all owners release it, the payload is destroyed once.
- Script release timing depends on collection. No business-critical resource
  cleanup may rely on prompt GC or script finalizers.
- Runtime teardown releases its own handles, not independently retained Rust owners.
  Runtime registry/generation/type checks reject foreign, stale or forged values.

The GC traces script-side handles and releases their retention records; it does not
scan arbitrary Rust object graphs. An object table must not retain every object
forever merely because it was once registered. Release must occur on an allowed
thread and must not run reentrant script code from a payload destructor. Finalize
the destruction/panic policy before implementation.

The first release excludes opaque payloads or captured host callbacks that create
unmanaged ownership cycles back into the script heap. Explicit script roots remain
necessary for any independently authorized host-retained script values; they do not
automatically solve cross-heap cycles. Opaque handles are not persistence tokens.

Rust payload memory is outside script heap accounting. Track/bound handle storage,
but do not claim that script heap limits bound arbitrary Rust allocations. Hosts
remain responsible for payload/resource quotas and bounded Native work.

## Methods and properties

Register all members at the type's module boundary:

```rust
game
    .opaque::<SkillConfig>("SkillConfig")?
    .property("power", |c: Opaque<SkillConfig>| c.power)?
    .property("name", |c: Opaque<SkillConfig>| c.name.clone())?
    .method("damage", |c: Opaque<SkillConfig>, level: i32| {
        c.power * level
    })?;
```

The arithmetic above only illustrates binding; production Native code must define
its overflow/error behavior and not inherit accidental build-profile differences.
Registration does not expose all Rust fields automatically. A property is a declared
getter-backed member, initially read-only. The compiler resolves the member using
its declaration; runtime execution uses linked Native entries, not reflection over
Rust field names. Kagari code sees ordinary syntax:

```kagari
fn calculate(config: SkillConfig, level: i32) -> i32 {
    val power = config.power
    return config.damage(level)
}
```

Methods are ordinary registered Native calls with an opaque receiver. Property
getters use the same conversion and invocation machinery with appropriate member
metadata. Preserve declared effects, evaluation order and once-only receiver
evaluation. No per-type or per-member cases belong in generic compiler/VM dispatch.

A getter or method returning `Opaque<Child>` produces a `Child` handle; returning a
value-bound child produces converted data. `Option<Opaque<Child>>` recursively maps
to `Option<Child>`. There is no configuration-specific branch. Returning an internal
`&Child` does not automatically become a retained child; the host must supply an
owned/retained wrapper. Borrowed subobject projection remains deferred.

## Mutation through methods

Opaque does not mean immutable. Host objects may own safe interior mutability and
expose operations that change the shared object:

```rust
use std::cell::Cell;

struct Player {
    score: Cell<i32>,
}

game
    .opaque::<Player>("Player")?
    .property("score", |p: Opaque<Player>| p.score.get())?
    .method("set_score", |p: Opaque<Player>, value: i32| {
        p.score.set(value);
    })?;
```

Other handles observe the updated score. The property is read-only in script syntax
even though the object has a modifying method. Read-only field access is not proof
of deep immutability or absence of aliases. Mutation effects belong to the Native
contract and must not be inferred as pure from a shared Rust receiver.

The host selects safe interior mutation/synchronization appropriate to its object.
Bindings neither add a mandatory mutex nor hand out `&mut T` behind a shared handle.
Do not hold a dynamic borrow or lock across script reentry or suspension; conflicts
need defined host errors rather than undefined behavior, accidental panics or VM
deadlocks. The engine cannot make arbitrary trusted Rust code cooperative.

Automatic `&mut self` adapters and writable property setters are deferred to the
borrow/mutation design. Native methods are not automatically transactional: already
completed host effects remain completed if later result conversion fails. Preserve
existing prepare/commit guarantees where the operation explicitly promises them.

## Configuration reload and future async use

The [update model](update-model-design.md) distinguishes compatible code publication
from fresh-environment state replacement. Value conversion is a reusable foundation,
not a whole-heap snapshot API. State replacement exports explicit data and business
IDs; Opaque objects and old runtime handles are not serialized into a new heap.
Resources are reattached only through checked host policy after writer quiescence.

For immutable configuration, a handle represents a fixed snapshot. Publishing a new
configuration does not retarget existing handles. New lookups can return the new
snapshot; old data remains retained while used. A service that always queries the
latest configuration is a distinct API, not an implicit property of opaque handles.

Script module reload and configuration data replacement are separate operations.
Preserve generation-pinned callable contracts and check type/schema compatibility;
keeping a payload alive does not authorize an old handle under an incompatible type.

The [async design](async-execution-design.md) may retain owned values and valid opaque
handles in suspended executions, subject to its root and lifecycle rules. This plan
adds no suspension support. Thread-affine external objects stay with their host
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

Document schema versions/fingerprints and bump affected artifact/runtime formats.
Update consumers directly; do not keep duplicate old/new semantic implementations
or silently ignore obsolete metadata. This does not permit removing validation,
existing scoped borrow protections or typed-path behavior outside this scope.

## Implementation phases

All phases are unstarted. Reconcile the accepted GO foundation before activation;
already implemented nominal declarations, roots and typed adapters are reused.
Review the remaining RI-specific API gates below.

- [ ] RI00: Re-audit the completed predecessor and existing tests. Decide trait
  signatures, supported numeric/collection/enum forms, field exposure/writeability,
  Serde attribute matrix, fallible-call channel and destruction/thread policy.
  Fix ordinary-value alias/cycle behavior, catalog identity rules and phase scope.
- [ ] RI01: Introduce portable nominal value schemas and registration graph
  validation. Integrate ordinary HIR types, checked contracts, linking and artifact
  validation. Test offline registration, missing dependencies and atomic rejection.
- [ ] RI02: Extend GO directional conversion contexts and typed Native adapters
  to the RI value-schema subset. Cover nested DTO conversion, limits and failures
  without adding another root or invocation implementation.
- [ ] RI03: Implement retained opaque storage and `Opaque<T>` without `T: Clone`.
  Add methods/properties through common Native bindings, identity/lifetime checks
  and safe host-managed mutation. Keep borrowed adapters out of this phase.
- [ ] RI04: Implement `KagariValue`, schema generation and the explicit optional
  Serde adapter against the accepted subset. Add compile-fail tests and an external
  embedding consumer proving that new structs and opaque types need no core edits.
- [ ] RI05: Update SDK examples and interop/type/memory/reload specifications,
  remove superseded binding models within scope, and run final acceptance.

At each checkpoint record commands, actual results, any carried failures and their
owning follow-up phase here. Do not conceal failures with stubs or weakened tests.
The default is a buildable checkpoint; any intermediate-breakage policy needs
explicit approval when activating this plan. Implementation commits, when separately
authorized, follow repository Conventional Commit and breaking-change rules.

## Acceptance and verification

Required coverage includes:

- Primitive, struct, enum and nested composite round trips; directional-only types;
  range/type errors; ordinary copy isolation and documented graph rejection.
- Mixed value/opaque fields across arguments, returns, properties and containers.
- Non-`Clone` payloads with cloned wrappers; identity preservation; shared mutation;
  no automatic `&mut` access or public Rust reference representation.
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

Run the relevant predecessor feature/backend matrix and all repository final checks:

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
