# Unified NativeModule registration and value/reference API proposal

Status: revised proposal for review; implementation is unstarted. Source baseline:
`d3d25ceb`. The [roadmap](implementation-roadmap.md#review-follow-up-order) owns
sequencing; [SA21](review.md#sa21-unify-rust-registration-through-nativemodule-with-scoped-host-access)
owns the finding and queued reference work. This document owns HR01-HR03,
registration migration acceptance and the queued reference design. Start with the
[by-value example](#complete-example-player-by-value) or the
[reference example](#complete-example-disjoint-player-fields) for the intended usage.
All new API spellings below are proposals, not currently callable APIs.

## Three passing modes

The intended Rust boundary has three modes. Registration unification starts with
by value. Reference behavior is designed below; its implementation remains queued
outside HR01-HR03.

| Mode | Ordinary struct representation and behavior | Status |
| --- | --- | --- |
| By value | Convert Rust data to an ordinary Kagari object; conversion back produces an independent Rust value. | Current API design focus; reuse existing owned conversion foundations. |
| By ref | `Ref<T>` is a scoped path handle; a native invocation temporarily receives `&T`. | Proposed below; implementation deferred. |
| By ref mut | `RefMut<T>` is a scoped writable path handle; a native invocation temporarily receives `&mut T`. | Proposed below, including simultaneous disjoint field access; implementation deferred. |

For an ordinary by-value struct, the boundary performs schema-directed conversion.
It can allocate the Kagari object directly; no byte serialization or JSON round
trip is needed. Once constructed, the object uses ordinary Kagari fields and GC
ownership. Script field reads/writes do not call Rust field adapters, resolve host
object IDs, acquire Rust borrow leases or commit changes to the original Rust object.
Exported fields of ordinary by-value structs default to writable Kagari fields.
Their schema records this as normal field metadata; local `val`/`var` continues
to control binding reassignment independently of object-field mutation.

The boundary conversion is distinct from script assignment. In script,
`val alias = player` aliases the same Kagari object under existing value semantics.
It does not run conversion again. Passing that object to an owned Rust `Player`
parameter performs conversion and creates independent Rust data. Returning that
Rust value constructs a Kagari object; it does not patch the original script object.

Rust move semantics still apply: passing an owned Player moves that Rust value.
If the host wants to retain an original for comparison, it explicitly clones it
before the call. Registration does not silently clone arbitrary Rust payloads.

## Scope and ownership

Unify standard-library, application native and existing host declarations/bindings
under `NativeModule`, with one function registration and engine installation path.
The public by-value API comprises type schemas, directional value converters and
ordinary native function bindings. It does not require a passing-mode argument.

The [interoperability plan](rust-interop-design.md#value-binding) owns general DTO
schema/conversion support, derives, optional Serde and retained Opaque storage.
The complete example below includes its proposed `KagariValue` derive to show the
end-user experience. Implementing that derive is not silently added to HR01-HR03.
HR01 must identify the concrete registration/conversion capabilities needed by
this example and record the boundary to RI01/RI02/RI04. A manual adapter may prove
the registration contract using existing conversion traits; it must not be
presented as proof that the future derive is implemented.

The [reference design](#queued-ref-and-refmut-design) covers borrowed inputs,
generated struct projections and scoped entry without activating implementation.
Borrowed results, collection element projections and general relocation semantics
remain separate extensions. SDK naming, collection migration, live module
installation and R1 remain separate work.

Current contracts remain owned by [architecture](architecture.md#unified-library-registration),
[value semantics](spec/value-semantics.md), [host interop](spec/host-interop.md) and
[runtime ownership](runtime-ownership-and-host-api-design.md).

## Complete example: Player by value

The four blocks below form one example in order. They are Rust-like and Kagari
pseudocode, not a currently compilable program. Module import paths for proposed
APIs and enclosing Rust error-return boilerplate are omitted; all application types,
functions, registration and execution steps are shown. No externally supplied
field/path adapter or object lookup helper is assumed.

### 1. Define the Rust data and functions

```text
#[derive(Clone, KagariValue)]
struct Player {
    hp: i32,
}

fn heal(mut player: Player, amount: i32) -> Player {
    player.hp = player.hp.saturating_add(amount.max(0));
    player
}

fn maximum(a: i32, b: i32) -> i32 {
    a.max(b)
}
```

`KagariValue` is the proposed DTO derive owned by the interoperability plan. It
provides one named struct schema and checked `KagariType`, `IntoKagari` and
`FromKagari` implementations. Exported fields become ordinary writable Kagari
fields, so `hp` is generated as `pub var hp: i32`. The Rust `Clone` derive is used
only to preserve the original host value in step 4; value binding does not
generally require Clone.

Conceptually the registered script type is:

```text
pub struct Player {
    pub var hp: i32,
}
```

This is generated from the schema; users do not write and register another copy.
The exact rendering follows Kagari's type/member declaration syntax.

### 2. Register the type and functions in one module

```text
let mut engine_builder = KagariEngine::builder();
let mut module =
    ModuleBuilder::new("game::actors", engine_builder.declarations())?;

let player_type = module.register_type::<Player>()?;

let heal_function = module.define_function(
    FunctionDecl::new("heal")
        .parameter("player", player_type.clone())
        .parameter("amount", Type::i32())
        .returns(player_type.clone()),
)?;
module.bind(
    heal_function,
    |_context, (player, amount): (Player, i32)| Ok(heal(player, amount)),
)?;

let maximum_function = module.define_function(
    FunctionDecl::new("maximum")
        .parameter("a", Type::i32())
        .parameter("b", Type::i32())
        .returns(Type::i32()),
)?;
module.bind(
    maximum_function,
    |_context, (a, b): (i32, i32)| Ok(maximum(a, b)),
)?;

engine_builder.install(module.finish()?)?;
let engine = engine_builder.build()?;
```

`module.register_type::<Player>()` is a proposed schema-registration convenience returning
an ordinary `Type` describing `game::actors::Player`. It validates the schema,
registers its nominal identity and connects the Rust mapping to that declaration.
It neither instantiates or copies a Player nor registers a persistent Rust object.
Register the type once: the same nominal identity supports owned parameters and,
when reference support is implemented, `Ref<Player>` and `RefMut<Player>` inputs.
Passing behavior belongs to the function signature and the supplied argument,
not to a separate type-registration mode. Schema support alone does not prove
that a Rust representation supports safe field projection.

`define_function` declares script signatures; `bind` attaches implementations.
The signatures remain authoritative and are checked against adapter mappings.
For ordinary closures, `bind` supplies the declaration context and builds the
typed adapter automatically, using the existing owned conversion model for both
scalar and struct arguments/results. A public function's normal Rust `Player`
parameter means by value; there is no extra enum selecting that behavior.

`engine_builder.declarations()` supplies the metadata catalog of modules already
installed, so dependencies can be resolved and validated. It does not execute
callbacks. A standalone catalog remains usable without constructing an engine.
The stdlib and application modules use the same validation and installation path.

### 3. Script code uses ordinary objects

```text
use game::actors::{Player, heal, maximum};

pub fn tick(player: Player) -> Player {
    val alias = player;
    player.hp = maximum(player.hp - 25, 0);
    // player.hp and alias.hp are both 75 for the example input.

    val healed = heal(player, 10);
    // healed.hp is 85. player.hp and alias.hp remain 75.
    healed
}
```

The first assignment changes the Kagari object, so both script aliases observe
it. `heal` then decodes that object into an owned Rust Player with hp=75, changes
that independent Rust value to hp=85 and returns it. Return conversion constructs
the separate Kagari object named `healed`. Ordinary value conversion does not
retain a hidden link to either the original Rust input or the earlier script object.
The subtraction is valid for the concrete input 100; other inputs retain Kagari's
normal checked numeric behavior.

### 4. Compile, execute and convert the result back to Rust

```text
let source = r#"
use game::actors::{Player, heal, maximum};

pub fn tick(player: Player) -> Player {
    val alias = player;
    player.hp = maximum(player.hp - 25, 0);
    val healed = heal(player, 10);
    healed
}
"#;

let file = engine.set_source("mem://player-example", source.into(), SourceLayer::Base)?;
let checked = engine.compile_snapshot(engine.source_snapshot(), file, &Default::default())?;
let artifact = engine.emit_bytecode(&checked, Default::default())?;
let program = PreparedProgram::from_artifact(
    artifact, &Default::default(), &Default::default(),
)?;

let context = ExecutionContext::default();
let mut runtime = engine.runtime(context.clone());
let loaded = runtime.load_program(&program, Default::default())?;

let original = Player { hp: 100 };
let result: Player = runtime.execute_typed(
    &loaded, "tick", (original.clone(),), &context,
)?;

assert_eq!(original.hp, 100);
assert_eq!(result.hp, 85);
```

The outer Rust tuple is the argument list: `(original.clone(),)` supplies one Player
argument. `execute_typed` and the source compile/load sequence reuse current SDK
operations. Their ability to accept this specific Player depends on the proposed
schema/conversion implementation, not on new host-root APIs.

| Boundary or operation | Data produced or modified |
| --- | --- |
| Host calls tick | IntoKagari creates a script Player with hp=100 from the supplied owned Rust value. |
| Script assigns player.hp | Ordinary script storage changes to hp=75; its script alias sees the update. |
| Script calls native heal | FromKagari creates an independent Rust Player with hp=75. |
| Rust heal returns | IntoKagari creates another script Player with hp=85. |
| Script returns to host | FromKagari creates the Rust result with hp=85. |
| Original host value | Remains hp=100 because the host explicitly supplied a clone. |

There are no per-field Rust callbacks in this flow. Generated converters access
fields only while encoding/decoding at a language boundary. GC roots, nominal type
checks, bounds and conversion failures remain enforced by the shared conversion
and invocation services.

## One function-binding entrypoint

`module.bind(function_ref, implementation)` accepts either an ordinary typed
closure or an explicitly constructed low-level `NativeBinding`. Both paths attach
to the same declaration and use the same checked registration/install machinery.

| Supplied implementation | Adaptation |
| --- | --- |
| Typed closure `Fn(&mut NativeContext, Args) -> NativeResult<Result>` | Build the owned conversion adapter using the referenced declaration's argument list and generic context. |
| `NativeBinding::new(codecs, result_codec, callback)` | Use explicit low-level codecs and the CallContext/Value callback, retaining signature and boundary validation. |
| `NativeBinding::primitive(selected_primitive)` | Install the explicitly selected fixed runtime implementation and validate its supported signature. |

The ordinary closure contract uses an outer argument tuple. Instance methods place
the receiver first in that tuple, for example `(player, amount): (Player, i32)`.
The method declaration still owns receiver identity, type and dispatch; treating
its arguments uniformly does not turn it into a free function. The adapter must
root and validate the complete argument pack before a converter can reenter.

Use the declaration-aware path currently implemented by
[ModuleBuilder::bind_typed](../crates/kagari-runtime/src/native/builder.rs) and
the contextual adapter in [typed bindings](../crates/kagari-runtime/src/native/typed.rs).
Do not implement automatic wrapping by unconditionally calling the standalone
`NativeBinding::typed(catalog, callback)` constructor: that constructor resolves
exact codecs from Rust types up front, while contextual mappings must be checked
against each concrete generic application. Preserve declaration identity and
callable requirements when consolidating preauthored-declaration bindings.

Resolve declaration membership, signature/codec compatibility and duplicate
bindings at registration. Preserve per-application checks before argument
conversion or callback effects, together with result conversion, ownership and
root validation. An explicit low-level binding cannot bypass these boundaries.

Primitive selection is explicit. An arbitrary callback cannot acquire primitive
semantics by its name, apparent body or Rust argument types. Primitive and raw
Value implementations remain available to the stdlib and advanced native authors;
ordinary application examples use direct closures.

The Rust adapter trait must distinguish the closure and prebuilt-binding cases
without overlapping implementations or unreliable type inference. HR01 requires
a compiling API proof for scalar/struct closures, receiver tuples, generic
declarations and explicit raw/primitive bindings before freezing its signature.
The exact trait spelling is an implementation choice, not a new user-selected
passing mode. This is a consolidation of existing adapter responsibilities, not
a claim that today's `bind` already accepts every proposed form.

## Implemented foundations and proposed additions

| Capability | Current evidence and required work |
| --- | --- |
| Module declarations and installation | [ModuleBuilder](../crates/kagari-runtime/src/native/builder.rs), [NativeModule](../crates/kagari-runtime/src/native/module.rs), [Engine builder](../crates/kagari-embed/src/engine/builder.rs) already exist. Merge the separate HostInterface authority here. |
| Owned conversions | [KagariType / IntoKagari / FromKagari](../crates/kagari-runtime/src/native/conversion/mod.rs) and rooted argument conversion already exist; reuse them. Existence of these traits alone does not establish general automatic struct conversion. |
| Ordinary struct metadata | [Shared declarations](../crates/kagari-types/src/declaration/mod.rs) already model Struct and fields. A native-storage builder is not proof of a complete DTO authoring API. |
| Type authoring convenience | `module.register_type::<T>()` registers schema/mapping metadata without creating objects or selecting a passing mode; it is proposed integration with RI schemas. |
| Function binding | The single `bind` entrypoint automatically adapts typed closures using their declaration context and accepts explicit low-level bindings; consolidate existing helpers through the checked invocation machinery. |
| DTO derive | `KagariValue` is proposed RI work. Track its actual implementation separately from registration acceptance. |
| SDK entry | [execute_typed](../crates/kagari-embed/src/runtime/typed.rs) exists and uses directional owned conversion. |

Portable type/member facts belong in `kagari-types`, executable contracts in
`kagari-contract`, converters/callbacks in `kagari-runtime`, and installation
orchestration in the SDK. Source-free execution must not require rendering or
parsing generated declarations. Methods, trait implementations and associated
types continue to use checked shared identities and selection.

Callbacks are potentially effectful regardless of parameter types. Registration
supplies conservative internal execution metadata. A by-value parameter does not
prove purity: the callback may modify captured state or perform IO. Preserve
argument evaluation order, cancellation, roots and already completed effects on
later conversion failures.

## Existing HostInterface migration boundary

Existing HostInterface mixes general declaration/registration responsibilities
with contracts for live host roots, borrowed access and typed paths. Classify them
before replacing APIs:

- General nominal type/function/member facts and signature validation move to the
  shared module and binding owners.
- Ordinary by-value data uses Kagari-managed objects and directional converters.
  Its fields are ordinary fields; do not attach host-path machinery to them.
- Existing consumers that access live Rust objects cannot be silently converted
  to snapshots. Inventory those consumers separately, including source-free
  contracts and host safety tests. Keep their existing checked execution services
  while migrating declaration/install ownership; do not present that lower-level
  machinery as another public passing mode or as the completed future ref API.

HR01 must give each existing live-host consumer a concrete shared-contract
migration destination. If removal depends on queued reference implementation,
record that dependency and bound the current retirement scope explicitly before
implementation. Do not promise complete legacy removal by deleting meaningful
behavior/tests, keeping forwarding APIs indefinitely or reintroducing reference
adapters into the by-value example.

Existing root ownership, borrow/escape checks, path preparation/commit, schemas,
cancellation and generation-pinned reentry/reload remain obligations wherever
those mechanisms are still used. No old reference-access contract becomes an
owned snapshot merely because its registration entrypoint changes.

### Retained Opaque integration

The [interoperability plan](rust-interop-design.md#opaque-ownership-and-access)
owns retained `Opaque<T>`. It describes an explicit representation choice: an owned
wrapper retains a Rust object rather than structurally copying its payload. That
wrapper travels through the owning conversion boundary; it does not add a fourth
passing mode and does not grant `&mut T` access to externally shared data.

This is distinct from ordinary Player-by-value conversion above. There is no
fallback from failed struct conversion to an opaque handle. Opaque implementation,
borrowed subobject projections and reference adapters remain separate queued work.

## Queued Ref and RefMut design

This section records the target reference API; it does not expand HR01-HR03 into
reference implementation. All reference syntax, type constructors and SDK helpers
below are proposals. The current owned adapter cannot decode arbitrary Rust
references through `FromKagari`; a lifetime-aware invocation adapter is required.

### Handles, schemas and generated projections

`Ref<T>` and `RefMut<T>` are script-visible, scope-bound location handles. They
identify a runtime owner, host scope/epoch, root, ordered field identities, target
type and shared or mutable access capability. They contain no script-dereferenceable
Rust pointer. Field identities are resolved from installed declarations, not
looked up by a user-written string switch on every access.

Copying a handle copies the location capability, not the Rust payload or an active
Rust reference. Multiple aliases of a `RefMut<T>` may therefore exist in script;
each operation still acquires a checked loan before touching host storage.
`RefMut<T>` may supply a shared input; `Ref<T>` cannot supply a mutable input.
A shared view does not freeze aliases or prevent Rust interior mutability.

For ordinary inline structs, extend the proposed `KagariValue` derive to generate
local field projections alongside its existing schema/conversion responsibilities.
`register_type::<T>()` installs those projections when supported, against the same
nominal type/field identities. Users do not implement path lookup, getters,
`stats_mut` helpers or per-field access flags for these ordinary structures.
RI continues to own schema/derive machinery; this reference follow-up owns the
projection safety contract and its runtime integration.

Member access on a structured host view yields another view:
`player.stats` is `RefMut<Stats>` under `RefMut<Player>`, or `Ref<Stats>` under
`Ref<Player>`. Reading a scalar such as `player.stats.hp` ordinarily yields an
owned `i32` through a short shared loan. In a reference-typed argument position,
a checked host field place may instead produce the corresponding path argument.
A previously copied scalar local does not become a host path. Frontend lowering
must retain that distinction explicitly; backends consume the checked operation.
Passing a view to an owned DTO parameter requires explicit snapshot conversion,
outside the initial reference example; it must not silently change passing modes.

### Complete example: disjoint Player fields

These three blocks form one independent example. They are API-shape pseudocode;
imports and enclosing Rust error-return boilerplate are omitted. Both shared and
mutable functions use `module.bind`, with no handwritten projection callbacks.

Define the Rust types and functions:

```text
#[derive(KagariValue)]
struct Stats {
    hp: i32,
}

#[derive(KagariValue)]
struct Inventory {
    gold: i32,
}

#[derive(KagariValue)]
struct Player {
    stats: Stats,
    inventory: Inventory,
}

fn read_hp(stats: &Stats) -> i32 {
    stats.hp
}

fn heal(stats: &mut Stats, inventory: &mut Inventory) -> i32 {
    let spent = inventory.gold.clamp(0, 10);
    inventory.gold -= spent;
    stats.hp = stats.hp.saturating_add(spent);
    stats.hp
}
```

Register each type once and bind both functions:

```text
let mut engine_builder = KagariEngine::builder();
let mut module =
    ModuleBuilder::new("game::actors", engine_builder.declarations())?;

let stats_type = module.register_type::<Stats>()?;
let inventory_type = module.register_type::<Inventory>()?;
let player_type = module.register_type::<Player>()?;

let read_hp_function = module.define_function(
    FunctionDecl::new("read_hp")
        .parameter("stats", Type::ref_(stats_type.clone()))
        .returns(Type::i32()),
)?;
module.bind(
    read_hp_function,
    |_context, (stats,): (&Stats,)| Ok(read_hp(stats)),
)?;

let heal_function = module.define_function(
    FunctionDecl::new("heal")
        .parameter("stats", Type::ref_mut(stats_type.clone()))
        .parameter("inventory", Type::ref_mut(inventory_type.clone()))
        .returns(Type::i32()),
)?;
module.bind(
    heal_function,
    |_context, (stats, inventory): (&mut Stats, &mut Inventory)| {
        Ok(heal(stats, inventory))
    },
)?;

engine_builder.install(module.finish()?)?;
let engine = engine_builder.build()?;
```

The proposed `Type::ref_` and `Type::ref_mut` constructors describe signatures;
they do not borrow an object during registration. Registering Player references
the already registered Stats/Inventory declarations. A second passing mode never
creates another nominal Player definition.

Compile the script and lend a host object for synchronous execution:

```text
let source = r#"
use game::actors::{Player, heal, read_hp};

pub fn tick(player: RefMut<Player>) -> i32 {
    val stats = player.stats;
    val inventory = player.inventory;
    heal(stats, inventory);
    read_hp(stats)
}

pub fn inspect(player: Ref<Player>) -> i32 {
    read_hp(player.stats)
}
"#;

let file = engine.set_source("mem://player-refs", source.into(), SourceLayer::Base)?;
let checked = engine.compile_snapshot(engine.source_snapshot(), file, &Default::default())?;
let artifact = engine.emit_bytecode(&checked, Default::default())?;
let program = PreparedProgram::from_artifact(
    artifact, &Default::default(), &Default::default(),
)?;

let context = ExecutionContext::default();
let mut runtime = engine.runtime(context.clone());
let loaded = runtime.load_program(&program, Default::default())?;
let mut player = Player {
    stats: Stats { hp: 60 },
    inventory: Inventory { gold: 30 },
};

let hp: i32 = runtime.with_host_scope(|scope| {
    let argument = scope.borrow_mut(&player_type, &mut player)?;
    runtime.execute_scoped(&loaded, "tick", (argument,), &context)
})?;

assert_eq!(hp, 70);
assert_eq!(player.stats.hp, 70);
assert_eq!(player.inventory.gold, 20);

let observed: i32 = runtime.with_host_scope(|scope| {
    let argument = scope.borrow(&player_type, &player)?;
    runtime.execute_scoped(&loaded, "inspect", (argument,), &context)
})?;
assert_eq!(observed, 70);
```

`with_host_scope`, `borrow`, `borrow_mut` and `execute_scoped` are proposed safe
interfaces, not wrappers that already exist. Their Rust signatures must keep the
host borrow valid until the registered capability is revoked, prevent root access
or movement that violates the borrow, and reject escaping scope tokens/results.
The sketch requires a compiling lifetime proof before the exact signatures freeze.
Script parameters use the proposed prelude types `Ref` and `RefMut`.

| Step in tick | Runtime behavior |
| --- | --- |
| Enter the host scope | Lend the original Player; preserve its address and lifetime. No DTO copy. |
| Evaluate the two field expressions | Produce `(root, [stats])` and `(root, [inventory])` views. No live Rust field references yet. |
| Invoke heal | Acquire both disjoint mutable loans, project the fields, and pass short-lived `&mut Stats` / `&mut Inventory`. |
| Return from heal | Release both loans; the host fields now contain hp=70 and gold=20. The script views remain usable in their scope. |
| Invoke read_hp | Temporarily acquire a shared loan on `[stats]`; return the owned integer 70. |
| Leave the host scope | Invalidate its handles and unregister storage before the Rust host resumes access to Player. |

### Path conflicts and reference construction

For the initial supported representation, paths contain only generated direct or
nested inline struct fields. Paths on the same root overlap when equal or when
one is a prefix of the other; diverging structural fields are disjoint. The empty
path denotes the whole root. Distinct root IDs prove independence only because
root registration itself must preserve Rust borrowing and alias identity; creating
another ID is never a way to evade conflicts.

| Simultaneous requests on one Player | Result |
| --- | --- |
| `&mut player.stats`, `&mut player.inventory` | Allowed: disjoint fields. |
| `&player.stats`, `&mut player.inventory` | Allowed: disjoint fields. |
| `&player.stats`, `&player.stats` | Allowed: shared overlap. |
| `&mut player.stats`, `&mut player.stats` | Reject before native callback entry. |
| `&player.stats`, `&mut player.stats.hp` | Reject: ancestor/descendant overlap. |
| `&mut player`, `&player.inventory` | Reject: whole root overlaps every field. |

Evaluate arguments left to right, once. Prepare/root the entire argument pack
and finish any conversions that can reenter before constructing Rust references.
Validate scope, runtime owner, schema/generation, target type and access, then
acquire all required loans as one batch, checking both sibling arguments and outer
active calls. On failure, release partial acquisitions without entering the
callback. Completed argument-evaluation/conversion effects are not rolled back.
Projection failure also releases the batch before callback entry.

The current [borrow table](../crates/kagari-runtime/src/host/borrows.rs) counts loans
by whole object. Supporting this proposal requires path-aware loan records and a
shared overlap checker for native calls and host field operations. Merely relaxing
the root count would permit aliasing. The host's scope-long lending of Player is a
storage/lifetime reservation; it must not count as an active whole-Player native
loan that conflicts with every field request.

Generated projection code must compute field pointers directly from the valid
root pointer, without first creating `&mut Player` or another borrowed ancestor.
Only after validation and batch acquisition may it materialize references to the
selected fields. Creating two whole-Player mutable references and then projecting
their fields is unsound even if the final paths are disjoint. Keep the unsafe
projection/reference construction in a small audited runtime/derive boundary;
user code stays safe. Raw projections still require valid provenance, in-bounds
storage, initialized aligned targets and a live owner.

Rust's [borrow-splitting model](https://doc.rust-lang.org/nomicon/borrow-splitting.html)
supports disjoint struct fields; its [raw field-address documentation](https://doc.rust-lang.org/stable/std/ptr/macro.addr_of_mut.html)
explains projection without an intermediate reference and the remaining validity
requirements. These support the implementation technique, not proof of Kagari's
scope, alias or reentry implementation.

Portable declarations/artifacts carry field identities and contracts, never Rust
offsets, addresses or function pointers. Runtime installation links them to local
generated projectors for the exact schema. Execution pins those bindings across
reload; a new declaration cannot reinterpret an active root's layout.

### Lifetime, reentry and mutation rules

Actual Rust references live only inside one synchronous native invocation. An
invocation adapter must be universally callable over its borrow lifetime, with
outputs independent of that lifetime. Do not implement general `FromKagari for &T`
or extend references to `'static`. Typed closure inference, mixed owned/borrowed
arguments and the higher-ranked adapter need a compile prototype.

Path handles may be forwarded through synchronous script calls within their
origin scope. Initially they cannot be returned, stored in GC objects/containers,
captured by closures, retained by a native callback or held across suspension.
Frontend checks and runtime validation, including source-free execution, enforce
this boundary. Scope invalidation and loan release use deterministic guards on
normal return, errors, traps, cancellation, depth exhaustion and Rust unwinding
where supported; GC collection is not the release mechanism.

During synchronous reentry, outer native loans stay active. Nested operations may
borrow a disjoint sibling but must reject an overlapping incompatible access.
No mutable borrow of the bookkeeping table or whole-root Rust reference may span
the callback. Shared-root scopes never upgrade to mutable access. Shared inputs
may still change interior state, so callbacks retain conservative execution
metadata regardless of their signature.

An active field loan blocks incompatible writes or replacement of that field and
its ancestors. An idle view retains a location, not a cached field address: after
an allowed replacement, its next operation resolves the current field at that
path. Root movement/removal is prohibited throughout its lending scope. Native
`&mut` callbacks directly change host state; callback failure does not roll back
completed writes. Existing validated/prepared script path assignments keep their
commit semantics. If dirty-path reporting applies, native mutable calls must
conservatively report each supplied mutable path as potentially modified once
callback execution starts, including failure; exact internal writes cannot be
inferred from `&mut T`.

The first projection implementation covers ordinary inline structs only. Packed
fields, unions, variant-dependent fields, custom dereference/index operations,
aliased pointer targets and collection elements require separate storage/alias
proofs. Different field names or map keys alone cannot establish disjointness.
Unsupported reference operations must diagnose the missing capability; an owned
converter or a custom serialized schema is not evidence of a valid projector.
Retained Opaque objects and GC-owned collection wrappers keep their separate
storage contracts; registration unification does not grant access to their payloads
as arbitrary Rust references.

### Follow-up activation and validation

Keep this follow-up under SA21 and the existing roadmap, after bounded registration
unification. Before implementation, freeze the safe scope signatures and adapter
compile proof, projection capability/derive boundary, checked frontend operations
and source-free contract changes. Add its finite implementation phases here when
activated; do not silently add them to HR01-HR03 or claim the example works today.

Validation must cover disjoint mutable fields in one call and nested reentry;
shared overlaps; rejection of duplicate, ancestor and whole-root conflicting
loans; complete-pack validation before callback effects; readonly capability;
foreign/stale handles and pinned reload; deterministic cleanup; replacement with
idle versus active views; and rejection of escaping references/handles. Include
compile-fail checks for host root reaccess/movement and escaping borrowed outputs,
plus focused Miri coverage of generated projection/loan lifetimes when implemented.
Use existing host boundary test owners; keep source-free evidence distinct.

Borrowing removes DTO reconstruction for this example, but still costs checked
path resolution and loan bookkeeping. No speedup is claimed. Measure frequent
small native calls separately from conversion/storage work before introducing
caches or routing existing stdlib primitives through this adapter.

## Implementation stages

### HR01: Map contracts and establish the bounded migration

Inventory ordinary declarations, existing host-root/path consumers, converters,
source/tooling imports, codecs, runtime preparation and backend dependencies.
Record their owners and removal destinations here. Freeze the one-`bind` API and
by-value registration/conversion contract. Prove closure/prebuilt-binding trait
coherence and type inference, receiver-first tuples, and declaration-aware generic
validation with a compiling prototype. Identify the exact RI schema/derive
capabilities required to make the complete example runnable and their owners.

Gate: general registration work is separated from queued reference implementation;
existing live-host behavior has an explicit migration disposition. State which
by-value proof can use existing/manual converters and which user-facing conveniences
remain RI dependencies. Do not mark the complete example implemented by assumption.

### HR02: Unify declarations, bindings and consumers

Migrate shared declarations, module validation, source/tooling import, runtime
installation and source-free linking in coherent, buildable slices. Both stdlib
and application bindings use the same function entrypoint and checked preparation.
Use one concrete owned-struct conversion case to establish the by-value contract;
reuse RI-owned schema/conversion capabilities when available and existing checked
manual adapters otherwise. Do not build a competing DTO or serialization system.

Preserve separately inventoried live-host execution contracts during their
registration migration. Remove obsolete entrypoints as their consumers migrate.
Any temporary bridge needs a bounded removal owner/checkpoint; never introduce
unvalidated conversions or a second signature authority to make a slice compile.

Gate: shared registration and atomic rejection work through source and source-free
consumers. The selected by-value case proves independent boundary values, ordinary
script aliases and unchanged original host data. Deferred reference APIs are not
part of the public acceptance example.

### HR03: Close integration and retire obsolete registration

Complete the HR01 retirement map, including legacy interface aggregation/format
and migrated registration APIs. Preserve existing host execution services where
required by their consumers. Update specifications and examples with implemented
contracts. Report remaining RI capabilities and deferred reference work explicitly.

Gate: all work in the bounded registration scope and its selected checks pass;
legacy-removal dependencies are accounted for rather than hidden. Mark only the
completed SA21 scope resolved. Move remaining work to its existing roadmap/review
owner and remove this execution plan when its implementation is complete.

## Validation and acceptance

Reuse existing test owners and add only uncovered contract cases:

| Contract | Focused evidence |
| --- | --- |
| Binding/type agreement and atomic install | Runtime `native_registration`, `host_registration`; SDK `registration_sources`. |
| Unified bind adaptation | Focused API compile coverage for direct scalar/owned-struct closures, method receiver tuples and explicit raw/primitive bindings; retain generic per-application mismatch checks before converters or callback effects at existing owners. |
| Owned conversion | Runtime `native_conversion` and selected SDK typed-call cases: nominal mismatch, nested conversion failure/roots, independent Rust values, script alias behavior and explicit return conversion. |
| Shared type/member/trait selection | SDK `host_interfaces` and directly affected declaration tests. |
| Source-free contracts | Selected runtime `offline_host` and SDK `offline_nominal` cases; keep host-root cases distinct from by-value DTO cases. |
| Preserved live-host consumers | Selected runtime `host_borrows`, `host_scopes`, `host_objects` and VM host-boundary cases for the actual migrated paths. |
| Pinned execution | Small affected reload/reentry regression set; actual-native case only when the backend contract changes. |

At implementation checkpoints select package/target/test-name filters, affected
Clippy, structure/format/diff checks and, where dependency boundaries change,
`cargo check -p kagari-embed --no-default-features --lib`. Use `--no-fail-fast` for
multiple selected test targets. Full workspace and exhaustive backend/feature
matrices belong to CI. Do not equate a manual conversion test with derive coverage.
Record actual commands, rationale, failures and unrun CI scope in the ledger.
Documentation-only changes need content/link/diff checks, not Cargo validation.

## Progress ledger

- [ ] HR01: migration map, by-value contract and RI dependencies frozen.
- [ ] HR02: shared registration and selected by-value/legacy consumers migrated.
- [ ] HR03: bounded retirement and focused integration complete.

Design checkpoint: `register_type::<T>()` registers shared type/schema metadata;
ordinary struct by-value conversion uses Kagari object storage. The queued
Ref/RefMut proposal uses scoped path handles, generated structural projections and
batch-checked disjoint loans. Both complete examples are end-user targets with
explicit implementation dependencies. Reference implementation remains outside
HR01-HR03; no code changes, Cargo validation or runtime behavior changes are claimed.
Implementation commits use Conventional Commits and phase trailers such as
`Plan-Phase: HR02`.
