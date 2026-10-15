# Unified NativeModule registration and value/reference API proposal

Status: active combined execution plan; HR01/RF01 proof and RI01-RI02 schema/conversion complete; HR02 unified bind and inferred registration retirement implemented; live-host declaration migration remains. The user
included Ref/RefMut in the same delivery as registration and Opaque. The
[roadmap](implementation-roadmap.md#host-registration-and-interop-delivery) owns
placement; [SA21](review.md#sa21-unify-rust-registration-through-nativemodule-with-scoped-host-access)
owns the finding. This document owns HR01-HR03, RF01-RF06 and the combined order;
[RI00-RI05](rust-interop-design.md#implementation-phases) retain their own schema,
conversion, Opaque and derive responsibilities and progress ledger. Start with the
[by-value example](#complete-example-player-by-value) or the
[reference example](#complete-example-disjoint-player-fields).
All new API spellings below are proposals, not currently callable APIs.
The interop plan owns the selected
[Opaque/Ref/RefMut relationship and host method API](rust-interop-design.md#relationship-to-ref-and-refmut).
Opaque transfers payload ownership to runtime; Ref/RefMut lend host-owned data.
Rust traits never automatically become Kagari trait implementations.

## Three passing modes

The intended Rust boundary has three modes. Registration unification starts with
by value. Reference behavior will be implemented in RF01-RF06 within the same delivery,
with distinct gates from HR01-HR03 and RI03. The initial API/lifetime proof is recorded in the progress ledger.

| Mode | Ordinary struct representation and behavior | Status |
| --- | --- | --- |
| By value | Convert Rust data to an ordinary Kagari object; conversion back produces an independent Rust value. | Current API design focus; reuse existing owned conversion foundations. |
| By ref | `Ref<T>` is a scoped path handle; a native invocation temporarily receives `&T`. | RF01-RF04 implemented; RF05 integration and RF06 acceptance remain. |
| By ref mut | `RefMut<T>` is a scoped writable path handle; a native invocation temporarily receives `&mut T`. | RF01-RF04 implemented, including disjoint field loans; RF05/RF06 remain. |

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

Ordinary DTO conversion is separate from runtime-owned opaque storage. The target
boundary below distinguishes representation from a short Rust borrow. These rules
also apply to explicitly registered methods; ordinary Rust method/trait definitions
are not exported by data schema registration.

| Script input | Rust `T` | Rust `&T` | Rust `&mut T` |
| --- | --- | --- | --- |
| Ordinary GC-owned `T` | Schema-directed independent value conversion. | Reject. | Reject. |
| Runtime-owned `Opaque<T>` | Explicit host take only; never implicit decoding. | Checked shared access through the declared native receiver adapter. | Checked exclusive access with a proven storage capability. |
| `Ref<T>` | Reject implicit conversion; explicit snapshot is a later extension. | Checked short shared loan. | Reject. |
| `RefMut<T>` | Reject implicit conversion; explicit snapshot is a later extension. | Checked short shared loan. | Checked short exclusive loan. |

One registered nominal identity does not make these representations interchangeable.
A GC object has no implied Rust `T` layout. Never silently copy it into a temporary
Rust object to satisfy a borrowed parameter, or copy modified data back afterward.
Reference capability/type mismatches reject during checking; live loan conflicts
reject at invocation before callback entry. Ordinary DTO and native representations
are explicitly distinct bindings, not interchangeable instances of one nominal
type. Inherent host methods are registered once for their target descriptor and
can admit Opaque/Ref/RefMut access according to their declared receiver requirement.

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

The [reference design](#ref-and-refmut-execution-contract) covers borrowed inputs,
generated struct projections and scoped entry under RF01-RF06.
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
Receiver tuple position does not define passing semantics. The reference
[receiver contract](#method-receivers-and-trait-selection) records the additional
declaration, lookup and borrowing requirements before reference implementation.

Use the declaration-aware path currently implemented by
[ModuleBuilder::bind](../crates/kagari-runtime/src/native/builder.rs) and
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
| Module declarations and installation | [ModuleBuilder](../crates/kagari-runtime/src/native/builder.rs), [NativeModule](../crates/kagari-runtime/src/native/module.rs), [Engine builder](../crates/kagari/src/engine/builder.rs) already exist. Merge the separate HostInterface authority here. |
| Owned conversions | [KagariType / IntoKagari / FromKagari](../crates/kagari-runtime/src/native/conversion/mod.rs) and rooted argument conversion already exist; reuse them. Existence of these traits alone does not establish general automatic struct conversion. |
| Ordinary struct metadata | [Shared declarations](../crates/kagari-types/src/declaration/mod.rs) already model Struct and fields. A native-storage builder is not proof of a complete DTO authoring API. |
| Type authoring convenience | `module.register_type::<T>()` registers schema/mapping metadata without creating objects or selecting a passing mode; it is proposed integration with RI schemas. |
| Function binding | The single `bind` entrypoint automatically adapts typed closures using their declaration context and accepts explicit low-level bindings; consolidate existing helpers through the checked invocation machinery. |
| DTO derive | `KagariValue` is proposed RI work. Track its actual implementation separately from registration acceptance. |
| SDK entry | [execute_typed](../crates/kagari/src/runtime/typed.rs) exists and uses directional owned conversion. |

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
migration destination. Removal that depends on reference implementation must name
its RF checkpoint; defer that retirement to HR03 after RF05 instead of keeping an
indefinite bridge or deleting live-host behavior. Do not promise complete legacy removal by deleting meaningful
behavior/tests, keeping forwarding APIs indefinitely or reintroducing reference
adapters into the by-value example.

Existing root ownership, borrow/escape checks, path preparation/commit, schemas,
cancellation and generation-pinned reentry/reload remain obligations wherever
those mechanisms are still used. No old reference-access contract becomes an
owned snapshot merely because its registration entrypoint changes.

### Retained Opaque integration

The [interop ownership contract](rust-interop-design.md#opaque-ownership-and-access)
owns Opaque<T>: a rooted handle to a runtime-owned Rust payload. The host moves T
into the runtime, accesses it through checked read/edit, or explicitly takes it
back and invalidates all aliases. Reuse NativeObject storage and roots; do not
build another ownership table or leave two public handles for the same contract.
This is the selected revision of the Opaque proposal, not implemented behavior.

Ref<T>/RefMut<T> instead borrow host-owned data for a scope. They can reuse the
same native type descriptor and explicitly registered shared/mutable methods;
changing ownership does not require duplicate method registration. The
[complete API example](rust-interop-design.md#methods-and-properties) shows both
runtime-owned and scoped use. Opaque does not convert its payload into a GC DTO,
provide unrestricted Deref/DerefMut, or silently take T during argument decoding.

The ownership and method adapter changes belong to RI03; external reference
scopes/projections belong to RF01-RF06 in the same delivery. They share checked
invocation machinery while preserving the HR phase boundaries. Ordinary DTO schemas remain a
separate representation without automatic Rust method or trait exposure.

## Ref and RefMut execution contract

This section records the reference contract assigned to RF01-RF06. All reference
syntax, type constructors and SDK helpers below remain target API sketches until
their owning phase passes; recording a phase does not prove the API implemented. The current owned adapter cannot decode arbitrary Rust
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
RI continues to own schema/derive machinery; RF04 owns the
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

### Method receivers and trait selection

The [complete method and scope example](rust-interop-design.md#methods-and-properties)
is the API owner for Opaque/Ref/RefMut. It registers native Player once, declares
inspect with shared receiver access and heal with mutable receiver access, and
binds the Rust implementations once. Host object storage is not ordinary GC DTO
storage. Registration of a data schema alone does not expose Rust inherent methods.

| Input representation | Registered `&self` method | Registered `&mut self` method |
| --- | --- | --- |
| Runtime-owned Opaque<T> (native T in script) | Checked shared payload access. | Checked exclusive access with a proven storage capability. |
| Host-owned Ref<T> | Checked shared access. | Reject: missing mutable capability. |
| Host-owned RefMut<T> | Checked shared access. | Checked exclusive access. |
| Ordinary converted DTO | No automatic host method exposure. | No automatic host method exposure. |

`ReceiverAccess::Shared/Mutable` in the proposed MethodDecl records required host
access; it is not a different nominal T declaration for every handle mode. Member
lookup through Ref/RefMut resolves explicitly registered host members on the target
nominal descriptor and checks the view capability. This does not attach borrowed
Rust methods to ordinary GC instances, infer methods from Rust impl blocks or add
same-name overloads distinguished only by ownership mode. Current MethodDecl::lower
has no such metadata; its implementation needs a coherent declaration/source-free
contract change, not just a receiver-first tuple.

Method calls evaluate the receiver and arguments left to right, once. Validate and
prepare the full pack before obtaining any Rust references. Include the receiver
in batch loan checking: a whole-root mutable receiver conflicts with any borrowed
child argument, even through another alias. RefMut may call a shared method through
a short shared loan, not a permanent downgrade. Borrowed results cannot escape;
release references before result conversion can allocate or execute script while
retaining the required roots. Methods use the same conservative effects and trap
cleanup rules as free functions.

The first runtime-owned receiver adapter reuses non-reentrant payload loans; the
external path adapter may admit disjoint reentry only once its existing path/loan
proofs below are established. A common binding API does not erase those storage
capabilities or promise reentry for every borrowed callback. Scoped adapters need
a compiling higher-ranked lifetime proof; FromKagari does not decode arbitrary &T.

Rust traits and Kagari traits are independent. A Rust impl neither creates a Kagari
trait nor satisfies a script bound. To expose a Kagari impl, explicitly register
or resolve its trait declaration, name its script receiver contract, and bind its
members, possibly calling Rust trait methods in the callbacks. Source-free loading
validates the same facts. See the
[trait registration boundary](rust-interop-design.md#rust-traits-and-kagari-traits).

Inherent method reuse does not require users to implement three trait variants.
Do not add automatic Rust-trait import, implicit view impl generation or a mandatory
three-impl API. General view-to-interface/generic adaptation remains a separate
concrete-use decision; no such rule is needed for inspect/heal calls. Consuming
Rust self methods are also separate: initial ownership recovery is explicit host
take on Opaque, never consumption of a borrowed host Ref/RefMut.

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

### Reference validation

RF01 freezes safe scope signatures and the adapter compile proof before production
references can be exposed. RF02 owns checked frontend and portable contracts;
RF03-RF05 implement scopes, projections and invocation. RF06 closes integration.
The acceptance below belongs to those phases, not extra requirements to finish
inside HR01-HR03. Exact API spelling is finalized by proof, not by the examples.

Validation must cover disjoint mutable fields in one call and nested reentry;
shared overlaps; rejection of duplicate, ancestor and whole-root conflicting
loans; complete-pack validation before callback effects; readonly capability;
foreign/stale handles and pinned reload; deterministic cleanup; replacement with
idle versus active views; and rejection of escaping references/handles. Include
compile-fail checks for host root reaccess/movement and escaping borrowed outputs,
plus focused Miri coverage of generated projection/loan lifetimes when implemented.
Use existing host boundary test owners; keep source-free evidence distinct.
Cover the three passing modes across free functions, inherent methods and explicit
trait impls, including mixed arguments and static rejection as well as dynamic
conflicts. Reuse equivalent cases; do not create a redundant Cartesian test suite.
Compile-fail evidence must cover callback reference escape as well as scope escape;
source-free cases must reject incompatible receiver/impl contracts independently
of frontend checking.

Borrowing removes DTO reconstruction for this example, but still costs checked
path resolution and loan bookkeeping. No speedup is claimed. Measure frequent
small native calls separately from conversion/storage work before introducing
caches or routing existing stdlib primitives through this adapter.

## Call-boundary cost follow-up

Static review at `47297a0d` confirms these paths are unchanged from report baseline
`f1494d1c`. These are code-derived costs, not benchmark results or speedup claims:

- [RootTable::insert](../crates/kagari-runtime/src/gc/roots.rs) calls `prune` over
  every slot before each insertion, including when reusable slots already exist.
  [Vec conversion](../crates/kagari-runtime/src/native/conversion/composites.rs)
  calls `encode_value` per element; the
  [conversion context](../crates/kagari-runtime/src/native/conversion/context.rs)
  retains temporary roots for heap-backed results until the enclosing scope ends.
  For n such elements and initial slot high-water mark R, scanning can cost
  O(nR + n^2) when live temporary roots force table growth. Existing free capacity
  can avoid the quadratic growth term, but not the repeated high-water scan.
- [Typed native invocation](../crates/kagari-runtime/src/native/typed.rs) builds a
  `Vec<Value>`; [argument decoding](../crates/kagari-runtime/src/native/conversion/arguments.rs)
  copies it into rooted storage and creates a root lease even for scalar packs.
  Mapping checks recur at pack and element conversion boundaries. Value/handle
  copying here is not necessarily a deep object copy. Individual scalar conversion
  avoids an object root, which does not eliminate the pack-level allocation.

The GC root owner should evaluate scope/batch storage or bounded amortized reclaim;
the typed invocation owner should evaluate borrowed argument windows and reuse of
checked mapping facts. Preserve snapshot/retention across user conversion and
reentry, collection during conversion, owner/generation/value checks, cancellation
and cleanup on every exit. Separate stable declaration proofs from invocation-time
facts before relocating validation. Root reclamation must still handle last-lease
drop and runtime transfer safely. Do not remove roots merely because the simple
scalar case does not need GC protection.

Keep owned conversion, scoped borrowing and explicit raw/primitive implementations
behind one validated binding entrypoint without forcing one data-movement path.
The [VM allocation tests](../crates/kagari-vm/tests/native_allocations.rs) establish
warm zero-allocation scalar/SequenceHandle paths; they do not establish that the
general typed adapter is allocation-free. Preserve those existing contracts during
registration migration.

This is a bounded follow-up owned here, not an additional HR01-HR03 optimization
gate. Before optimization, measure equivalent raw/typed/owned cases; add ref/refmut
cases only when those adapters exist. Vary argument/payload size, root-table
high-water mark, path depth and active loans where applicable. Separate compile,
installation, conversion and warm-call costs; report allocations, retained memory
and timing with revision, machine, toolchain, profile/features, parallelism and
cache state. Include representative bulk calls and small frequent calls without
assuming one is faster. Reuse focused correctness tests for GC/reentry/trap cleanup;
no retained performance conclusion can be inferred from registration unification.

## Combined delivery order

Registration, Opaque and Ref/RefMut are one selected delivery. Keep responsibilities
and progress at their existing owners; do not create a parallel umbrella checklist.
This order supersedes standalone RI sequencing only at the explicit shared gates:

| Order | Work | Required result before proceeding |
| --- | --- | --- |
| 1 | HR01 + RI00 + RF01 | One migration map and compiling API/lifetime proofs, including receiver metadata, scope ownership and the accepted payload/schema subset. |
| 2 | RI01-RI02, then HR02 | Shared schema/conversion facts and one checked declaration/bind/install path; live-host consumers keep their current behavior. |
| 3 | RI03 + RF02 | After the shared owned-registration gate, complete reference-dependent HR02 live-host declaration migration with RF02-RF05. Runtime-owned Opaque read/edit/take and method adapters; reference types, escape rules and portable validation. RF02 consumes the receiver facts fixed in the first gate. |
| 4 | RF03, then RI04 and RF04 | Safe external whole-object scopes, then derive-generated inline field projections and batch path loans. RF04 depends on RI04's schema/projection support. |
| 5 | RF05 | One callable registration works for supported Opaque, Ref and RefMut inputs through SDK, VM and source-free loading. |
| 6 | HR03, RF06 and RI05 | Remove migrated legacy registration, finish reference/interop acceptance, and move current contracts to their specification owners. |

RI03 owns the common lifetime-aware native method adapter. RF01 establishes its
compile proof early; RF05 extends it with scoped external/path argument sources,
not a second callback system. RI04 owns schema/derive output; RF04 owns projector
safety and path execution. Optional Serde retains its RI scope and is not a new
reference prerequisite. The independent root/typed-call optimization follow-up,
NC migration, Host API overhaul, general view trait forwarding, borrowed returns,
async reference retention and wider native backend coverage are outside this delivery.

Each checkpoint remains buildable. Early reference contracts must explicitly
reject execution until the required runtime capabilities are installed; do not
publish an apparently usable but incomplete feature or use production stubs.
Run selected boundary tests as each owner becomes buildable; record unexpected
integration failures and their exact follow-up phase without repeatedly rerunning
unchanged failures. HR03 waits for dependent RF migration rather than prematurely
removing host access; final local acceptance cannot retain known integration errors.
Full CI acceptance stays separately reported and is never inferred from local tests.
Pending supported-runner Miri/CI evidence permits subsequent local integration;
a discovered safety failure must be repaired, and pending evidence cannot be
reported as final safety acceptance.

## Implementation stages

### HR01: Map contracts and establish the bounded migration

Inventory ordinary declarations, existing host-root/path consumers, converters,
source/tooling imports, codecs, runtime preparation and backend dependencies.
Record their owners and removal destinations here. Freeze the one-`bind` API and
by-value registration/conversion contract. Prove closure/prebuilt-binding trait
coherence and type inference, receiver-first tuples, and declaration-aware generic
validation with a compiling prototype. Identify the exact RI schema/derive
capabilities required to make the complete example runnable and their owners.
Align RI examples with explicit declaration plus `bind`; do not retain parallel
signature-inference APIs. Record receiver metadata/lookup requirements and reserved
reference-stage boundaries before freezing the owned method declaration surface.
Compile the owned/raw/primitive prototype in HR01 and coordinate RF01's borrowed
adapter proof; arbitrary view trait forwarding is not introduced by this delivery.

Gate: registration responsibilities are separated from RF implementation;
existing live-host behavior has an explicit migration disposition. State which
by-value proof can use existing/manual converters and which user-facing conveniences
are delivered by RI01/RI02/RI04. Do not mark the complete example implemented by assumption.

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
script aliases and unchanged original host data. This checkpoint validates the
by-value path; RF05 adds reference acceptance in the same delivery.

### HR03: Close integration and retire obsolete registration

Complete the HR01 retirement map, including legacy interface aggregation/format
and migrated registration APIs. Preserve existing host execution services where
required by their consumers. Update specifications and examples with implemented
contracts after RF05 supplies the dependent reference paths. Keep registration
retirement distinct from RF06 and RI05 final contract acceptance.

Gate: all work in the bounded registration scope and its selected checks pass;
legacy-removal dependencies are accounted for rather than hidden. Mark only the
completed registration scope resolved. Keep this plan until RF06 and the combined
integration gates finish; then move durable contracts to their owners and remaining
explicitly excluded work to the roadmap/review before retiring the plan.

### RF01: Freeze scope and borrowed adapter contracts

With HR01/RI00, inventory the actual host scopes, path commits, native payload
access, typed bind adapters and executable receiver facts. Produce a compiling
prototype for mixed owned/shared/mutable arguments and methods using the single
bind entrypoint, including Opaque's native loans and external scoped inputs.
Prove that the callback is valid for every invocation lifetime and cannot return
or retain a borrowed output. Safe scope APIs must prevent conflicting host root
access/movement and escaping view tokens. No general FromKagari for &T is allowed.

Gate: passing compile examples plus compile-fail cases for callback/scope escape,
root reaccess/movement and incompatible shared-to-mutable adaptation. Freeze the
supported storage/projection capabilities and error ordering. Reuse this adapter
proof for RI03; failed Rust inference/lifetime proofs require API correction here,
not unsafe lifetime widening or a second binding entrypoint.

### RF02: Check reference types and portable execution facts

Add explicit Ref/RefMut target/capability facts to the existing type, HIR, MIR and
executable contract owners. Check method receiver admission, left-to-right place
preparation and initial non-escape rules: no reference storage in heap containers,
closure capture, returned views, suspension or implicit snapshots. Register the
required prelude/types through the ordinary declaration path. Rust traits do not
supply Kagari impls; retain explicit script trait registration.

Source-free verification must independently reject malformed target/member facts,
capability upgrades and forbidden escape/suspension paths. Portable records carry
field/type identities, never pointers or Rust offsets. Backend code consumes checked
operations; unsupported native bodies select the existing interpreter route before
entry instead of compiling fake reference support.

Gate: focused accepted/rejected source cases and corrupted-artifact cases establish
the same contracts. Execution explicitly checks installed capabilities until RF03-
RF05 supply them. This phase does not infer Rust layout or claim runnable scope APIs.

### RF03: Implement external scopes and whole-object loans

Build safe scope entry/exit and checked external root registration on the existing
host services. Scope/epoch, runtime owner, target schema, root alias identity and
shared/mutable capability are validated before access. Host lending reserves storage
for the scope; it is not an active native loan conflicting with all later accesses.
Provide whole-object shared/exclusive invocation, keeping loan release and scope
revocation deterministic on return, errors, cancellation, depth exhaustion and
supported Rust unwind. An active external scope prevents incompatible runtime
transfer. A Ref/RefMut can never take ownership of its host payload.

Gate: the shared inspect and mutable heal examples run through focused runtime
fixtures with shared-to-mutable rejection, overlapping reentry rejection, stale/
foreign handle rejection and cleanup. Compile-fail scope guarantees from RF01 remain
covered. No field disjointness is claimed until RF04 replaces whole-root overlap
checking with the validated path model.

### RF04: Generate inline projections and batch path loans

Consume RI04's schema/derive descriptors to generate local raw field projectors for
the finite ordinary inline-struct subset. Keep portable field identity separate
from installed Rust layout and pin projector/schema generations across reload.
Never materialize a mutable ancestor reference before projecting disjoint fields.
Reject packed/union/variant-dependent/indexed or aliased-pointer projections until
their storage proofs exist; no handwritten per-type dispatcher in generic runtime.

Unify field operations and native argument access under one path overlap checker.
Prepare the complete pack before atomic loan acquisition; equal and prefix paths
conflict if either request is mutable, while proven diverging inline fields may
coexist. Validate existing outer loans and release all partial acquisitions on
failure. Reentry may access a disjoint path only under the proved external loan
contract; do not loosen Opaque's non-reentrant payload borrowing by analogy.
Idle views resolve current storage after permitted replacement; active loans block
incompatible replacement/removal. Preserve prepared assignment commits and dirty
path reporting for completed native writes.

Gate: run the Stats/Inventory two-field example, shared overlap and duplicate/
ancestor/whole-root rejection cases, partial-failure cleanup, replacement and
pinned-projector checks. Generated projection code needs focused Miri evidence on
a supported runner; code review and ordinary tests do not substitute for that gate.

### RF05: Integrate one binding and SDK execution path

Extend RI03's common adapter with external whole-object and field-path argument
sources. SDK scoped execution, VM dispatch and source-free installation reuse the
same declaration, receiver and complete-pack checks. One registered inspect/heal
body serves Opaque and supported Ref/RefMut views; shared views cannot invoke mutable
members. Mixed DTO/scalar/borrowed arguments retain roots across conversion and do
not expose Rust references until conversion finishes. Result conversion runs after
loans end, with required roots retained and completed effects preserved on errors.

Migrate the live-host consumers assigned by HR01 without turning them into DTO
snapshots. Include explicit Kagari trait binding and rejected implicit Rust-trait
exposure; this is not authorization for general view-interface forwarding. Keep
raw/primitive bindings and their existing warm allocation contract available.

Gate: end-to-end SDK method/free-function cases, the full scope and projection
examples, and source-free fixtures pass. An interpreter-only consumer remains
frontend-free; backend selection is explicit and no unsupported native reference
execution is counted as implemented. HR01's dependent legacy paths are ready for HR03.

### RF06: Close reference safety and combined integration

After HR03 retirement, consolidate the existing host scope/borrow/registration and
SDK fixtures around the final API. Reuse cases establishing the same contract;
retain distinct source-free, alias/generation, GC/root, cancellation, pinned reload,
reentry and cleanup evidence. Cover unavailable Opaque receivers after take together
with scoped view invalidation, without pretending their ownership rules are equal.
Update the runnable examples and owning specifications with actual APIs; remove
obsolete prototypes/bridges and carry no known local integration failure.

Gate: selected Rust/compile-fail tests and structural/format/diff checks pass, and
projector Miri plus exhaustive feature/backend/platform checks have an explicit
CI status. If Miri cannot run locally, its runner configuration and pending result
remain open acceptance; never label them passed. Report implementation completion
separately from pending CI, with any required safety failure still blocking final
acceptance. Coordinate RI05 documentation closure; only retire this plan after its
HR/RF contracts and cross-plan integration obligations are fulfilled.

## Validation and acceptance

### RF02 implementation decision: scoped semantic types

Native and shared call applications now carry the receiver access copied from the
original callable declaration. Their logical signature and entry identity remain
nominal: `Player.inspect` still takes Player and selects the same binding. A caller
may supply Ref<Player> or RefMut<Player> only when the installed declaration
requires a permitted loan. MIR and source-free bytecode independently check the
actual view target/capability before admitting its HostHandle transport; unknown
host facts, other targets and capability upgrades do not satisfy this adaptation.
Application linking checks the copied access against the original native
declaration, including generic shared bodies. No Rust offsets or pointer facts are
added to portable calls. Runtime scope/storage admission remains a separate gate.

Outer RefMut-to-Ref weakening now uses the existing checked view coercion and
access-flow owners. It preserves the exact target and never changes nested type
arguments or supplies mutable capability to a shared view. Methods without explicit
receiver access remain unavailable through host views; Rust trait implementations
still require an explicit Kagari impl. Field-place operations remain RF02/RF04 work.

Reference target/access facts use Ty::Reference and its HIR counterpart, with the
existing bounded identity, substitution, wire and application validators. Ref and
RefMut are ordinary NativeTypeConstructor declarations in core::host, exported by
the prelude; their record does not add a second nominal target or Rust layout.
Concrete targets are ordinary structs or opaque native payload storage, with target
binders rechecked after application. Other native layouts have no reference contract.
Native runtime scope/projection capabilities are separately required; accepting a
DTO target schema never interprets its ordinary GC object as a Rust reference.

Direct parameters/locals are non-owning view positions. Owning aggregates, captures,
returned views and suspension reject. Callable signatures may mention view inputs
without containing an actual view; capture and invocation flows remain independently
checked. Source-free access flow rejects missing provenance as well as capability
upgrades and hidden reference storage. Method receiver access/target lookup, checked
place operations and installed scope admission remain RF02–RF05-owned; this first
slice does not mark RF02 complete or claim usable host lending.

The selected Cranelift fallback regression found a pre-existing host-target defect:
host_isa always enables PIC, while cranelift-jit 0.136.2's JITModule::new only permits
PIC on x86_64. On the aarch64-apple-darwin development host its final successful
scalar compilation panics after the unsupported cases. The immediate RF02 integration
follow-up limits host PIC to the supported architecture and reruns that failure;
it adds no native reference execution or backend operation family.

Reuse existing test owners and add only uncovered contract cases:

| Contract | Focused evidence |
| --- | --- |
| Binding/type agreement and atomic install | Runtime `native_registration`, `host_registration`; SDK `registration_sources`. |
| Unified bind adaptation | Focused API compile coverage for direct scalar/owned-struct closures, method receiver tuples and explicit raw/primitive bindings; retain generic per-application mismatch checks before converters or callback effects at existing owners. |
| Preserved low-level allocation contract | Existing VM warm scalar/SequenceHandle allocation assertions for affected binding paths; do not claim general typed zero allocation. New cost measurements belong to the bounded follow-up above. |
| Owned conversion | Runtime `native_conversion` and selected SDK typed-call cases: nominal mismatch, nested conversion failure/roots, independent Rust values, script alias behavior and explicit return conversion. |
| Shared type/member/trait selection | SDK `host_interfaces` and directly affected declaration tests. |
| Source-free contracts | Selected runtime `offline_host` and SDK `offline_nominal` cases; keep host-root cases distinct from by-value DTO cases. |
| Preserved live-host consumers | Selected runtime `host_borrows`, `host_scopes`, `host_objects` and VM host-boundary cases for the actual migrated paths. |
| Pinned execution | Small affected reload/reentry regression set; actual-native case only when the backend contract changes. |
| RF01 lifetime proof | Focused Rust compile-pass/fail cases for the final scope and adapter API; escaping outputs, host root reaccess/movement and overlapping adapter implementations must reject. |
| RF02 reference contracts | Affected HIR/bytecode verification owners: receiver capabilities, forbidden escape/suspension, and malformed source-free target/member facts. |
| RF03-RF04 scope/path safety | Existing runtime `host_scopes`, `host_borrows`, `host_objects` owners plus generated-projector tests; same root, prefix/divergent paths, unwind cleanup, replacement and projector generation. Supported-runner Miri covers the unsafe projection boundary. |
| RF05-RF06 combined delivery | SDK `host_interfaces` and `offline_nominal`, runtime native registration/conversion and selected VM boundary cases: one method registration across storage modes, mixed argument packs, Opaque take and view invalidation, explicit Kagari trait binding. |

At implementation checkpoints select package/target/test-name filters, affected
Clippy, structure/format/diff checks and, where dependency boundaries change,
`cargo check -p kagari --no-default-features --lib`. Use `--no-fail-fast` for
multiple selected test targets. Full workspace and exhaustive backend/feature
matrices belong to CI. Do not equate a manual conversion test with derive coverage.
Record actual commands, rationale, failures and unrun CI scope in the ledger.
Documentation-only changes need content/link/diff checks, not Cargo validation.

## Progress ledger

### RF03 checkpoint: branded external roots and whole-object methods

`Runtime::with_host_scope` now owns a fresh invariant brand, exclusive runtime
reservation and the existing HostResourceScope guard. Its borrow/borrow_mut APIs
use installed NativeType<T> descriptors and retain the original Rust loan until
scope exit. HostBorrowTable's active frames hold the scoped Rust address, exact
Rust type, applied target, retained program and access. Ephemeral GC headers carry
only inert owner/frame/epoch/object/access identities. The existing object counts
and frame cleanup own active loans; installed and external identity domains avoid
collisions with user-assigned legacy root numbers. No additional callable registry
or execution engine is introduced.

The lifetime-aware receiver adapter now decodes an Opaque root or scoped capability
under one checked argument pack, completes owned conversions before lending, and
uses the same callback/result boundary. External storage does not use the Opaque
data-edit flag; exact installed Rust mapping and exclusive host reservation still
apply. Whole-object read/edit helpers enforce nested conflicts and release on
error/unwind. The VM scoped entry validates closed concrete parameters, uses its
ordinary interpreter/session and rejects ephemeral results. Field place facts,
ordinary Struct lending, multi-loan packs and SDK context/backends remain RF02/RI04/
RF04/RF05 integration work; the RF03 checklist remains open for that integration
gate at this checkpoint. The then-current NativePayload bound on external lending was an intermediate
dependency on NativeType's checked descriptor, not a requirement that host-owned
DTOs become GC payloads. RI04/RF04 must supply the checked ordinary Struct/Rust
mapping descriptor and lift that external bound at the common loan owner; RF05
is its removal checkpoint. No duplicate nominal declaration is permitted to work
around this gap.

Selected local evidence: VM external methods after KBC round trip with GC threshold
one; RefMut-to-shared method use; shared-to-mutable, stale and foreign rejection;
nested shared/exclusive conflicts; retained completed writes and loan/scope unwind
cleanup. The existing generic native impl fixture now executes view forwarding
through both script and native shared bodies. It initially failed because bounded
compiler instantiation did not descend into Reference; adding that existing type
owner's child traversal and checked shared receiver admission resolved it. Initial
string inspection needed dereferencing its GC Ref guard. All five other selected
payload-method cases passed; the repaired generic case passed separately. Final
scope cleanup case passed after its additional unwind coverage. Production scope
escape/root reaccess/runtime transfer/thread transfer/loan escape doctests passed.
Its final selection also passes real synchronous shared reentry, rejects mutable
reentry into an active root loan, preserves completed writes across recursive depth
exhaustion, releases execution state after cancellation and permits a healthy call
afterward. The depth assertion initially used the wrong error variant spelling;
corrected to ResourceLimitExceeded. Scoped runtime/VM Clippy and structure/format/
diff checks passed. Borrow record/key
logic moved to host/borrows.rs when the host facade exceeded its LOC limit; no
exception was added. Scoped example Clippy passed; supported-runner Miri is tracked below.

`cargo run -p kagari --example scoped_host` passed, printing
`owned hp=70, borrowed hp=70`. The example registers heal/inspect once and executes
them through Opaque, RefMut and shared script access to that mutable view. It uses
the current interpreter API directly; SDK convenience and explicit context routing
remain RF05-owned.

Focused Miri command started on aarch64-apple-darwin with
nightly-2026-07-27 (rustc 1.99.0-nightly dc3f85158), installed Miri, default workspace
profile/parallelism and default interpreter flags:
`cargo +nightly-2026-07-27 miri test -p kagari-vm --test native_boundary external_scopes_use_registered_payload_methods -- --nocapture`.
The selected test is still running; no passing Miri result is claimed. This covers
the real whole-object pointer/loan boundary. Generated field projector Miri and
exhaustive workspace/feature/backend/platform acceptance remain RF04/RF06/CI-owned.

### RF02 checkpoint: view member admission and call facts

HIR member selection uses the target of a view only for an explicitly loan-capable
native member. Applied calls keep their original nominal signature and common
binding. NativeImport/SharedCall wire, remapping, constructor and linking owners
carry/check the declaration's receiver access. Physical validators admit HostHandle
transport only for that fact; MIR semantic registers and bytecode flow facts must
independently establish its exact target/capability. Native backend support is not
inferred from these records. Runtime access still rejects unavailable scope/storage.

Selected checks passed: HIR forwarding with RefMut-to-Ref, narrowed local and generic
inference; source-free reference target/storage/suspension corruptions; Opaque method
execution with additional accepted view bodies after KBC round trip; generic native
impl/local binders with a view call in a shared generic body; existing source-free
closed shared application retention/release. A selected member test rejected
shared-to-mutable calls, view access to an owned callback and inferred Rust trait
exposure. Its artifact mutations rejected copied access corruption and a consistent
shared parameter/ABI rewrite attempting the registered mutable call. Initial Clippy
reported one collapsible conditional; it was repaired. Scoped contract/HIR/compiler/
VM lib/native-boundary Clippy, structure, formatting and diff checks passed.

RF02 remains open for field-place facts and execution admission; RF03-RF05 own
external lending, projections and common callback transport. Changed development
wire records invalidate old caches; affected tracked fixtures will be regenerated
once at the combined adapter/field-contract checkpoint. No format identifier bump,
compatibility reader or unsafe pointer is introduced. Projector Miri and exhaustive
workspace/feature/backend/platform acceptance retain their RF/CI owners.

### RI03/RF02 checkpoint: common borrowed receiver binding

RI03's runtime-owned method gate is implemented. Receiver access is a portable
fact on the existing callable declaration and a checked local fact on its binding.
The lifetime-aware adapter selects through ordinary bind and reuses contextual
conversion/invocation plus Opaque's existing guarded storage access. Concrete
native trait impls can declare their own access before binding, independently of
the Kagari trait's ordinary Self requirement. Installed agreement remains mandatory.

Focused method, capability, take/pinned receiver and production compile-fail checks
passed; the detailed selection and initial fixture repairs are recorded in
[RI03's checkpoint](rust-interop-design.md#ri03-checkpoint-registered-lifetime-aware-methods).
Scoped runtime/VM native-boundary Clippy, structure, format and diff checks passed.
The local/native access contract is now ready for RF02's view receiver admission
and RF03-RF05's external scope/path transport. HR02/RF02 remain open; no external
scope implementation, projector Miri result or exhaustive CI result is claimed.

### RF02 checkpoint: reference targets and non-escape facts

Shared portable/HIR types, ordinary core::host/prelude declarations and registration
Type::ref_/ref_mut now describe nominal host targets and access capabilities. Bounded
wire/identity/substitution, generic inference/recovery and dependency traversal share
the existing owners. Declaration/slot/layout validation admits direct views while
rejecting owning storage and returned views. Source and source-free flows separately
reject DTO-to-view substitution, shared-to-mutable upgrade, container/cell/interface
escape, closure capture and async retention. Function input signatures do not count
as captured locations. Rust pointer/projector installation remains a later gate.

Two selected HIR reference cases passed: same-capability forwarding/generic target
inference, and rejected target/escape/container/capture/async cases. The forwarding
case also passed after adding Ref<_> local inference. The existing nested-readonly
native import regression passed. The VM native-boundary reference artifact case
passed after KBC encode/decode/adoption/verification, including four corruptions:
capability upgrade, scalar target, hidden tuple storage and resume-body marking.
Runtime/HIR/VM lib checks, scoped HIR/runtime/VM lib/native-boundary Clippy and
source-free SDK `cargo check -p kagari --no-default-features --lib` passed. Structure
(1190 files, zero findings), formatting and diff checks passed. Initial integration
failures were missing constructor handling and the new core::host module documentation
inventory; both are resolved. One Clippy fixture allocation was corrected.

The Cranelift unsupported-input regression currently fails at its final unrelated
scalar success check with the aarch64 PIC assertion described above. The immediately
following RF02 checkpoint owns its configuration repair; the check is not counted
as passed. Full workspace/feature/backend/platform acceptance remains CI-owned.
No unsafe projection exists in this slice; projector Miri remains RF04/RF06-owned.
RF02 and RI03 remain open for reference receivers, place/execution contracts and
the shared borrowed adapters; HR02's live-host migration still depends on them.

### RF02 integration follow-up: host ISA configuration

Host ISA construction now enables PIC only on x86_64, as required by the installed
Cranelift JIT. The existing Windows x86_64 AOT subset keeps its PIC setting. The
previously failing unsupported-input test (including reference input rejection)
and the selected cross-thread finalized-code/runtime execution and release test
both passed on aarch64-apple-darwin. Scoped Cranelift lib Clippy, structure,
formatting and diff checks passed. This resolves the carried local configuration
failure; exhaustive platform/backend coverage remains CI-owned.

### HR01/RF01 inventory and API proof (2026-10-15)

Baseline `c54001c5` had a clean workspace. Inventory confirms that storage,
roots, conversion, installation and execution services already exist; this
migration replaces registration ownership, not those services.

| Existing owner/consumers | Destination and removal checkpoint |
| --- | --- |
| `native/builder.rs`, `builder/inherent.rs`, `builder/implementation.rs`; stdlib builders and application native modules | One declaration-aware `bind` adapter, retaining raw/primitive bindings and their allocation behavior. HR02 removed `bind_with`, `bind_typed`, receiver-separated typed registration and alternate default-body binding entrypoints; HR02 also removed inferred FunctionSpec/add_function/add_async_function and standalone exact typed constructors; preauthored stdlib contextual bindings retain the same conversion service. RF05 adds borrowed adapters to the same entrypoint. |
| `native/conversion/`, `native/typed.rs`, `native/objects/` | RI01 schema registration and RI02 nominal conversion use existing TypeArgument, roots, depth/node/cycle limits and checked object layouts. RI04 derives project those same contracts. No second root or invocation implementation. |
| `native/payload/`, `native/storage.rs`, `gc/native.rs` | RI03 renames the public NativeObject handle to Opaque directly, adds extraction capability and atomic heap removal, and reuses non-reentrant read/edit loans. Internal GC storage records remain private. |
| `types/host_interface/`, HIR `host.rs`/`host/origin.rs`, SDK `engine/source.rs::set_host_interface` | General type/function/member/impl facts move to ordinary ModuleDecl/NativeDeclaration and generated registration views in HR02. Portable roots/path requirements remain checked until RF05 migrates them; standalone HostInterface bytes/aggregation retire in HR03. |
| Runtime `host.rs`, `host/registry.rs`, `host_scope.rs`, `host/borrows.rs`; VM host dispatch and path operations | Preserve owner/epoch, rooted temporaries, path preparation/commit, dirty paths and failure order. RF03 supplies branded external lending; RF04 supplies path-aware batch loans and generated projectors; RF05 routes live-host calls through the common native adapter. HR03 removes superseded registration, never live-host semantics. |
| SDK `host_interfaces`, `collection_access`, `source_modules`, `offline_functions`/`offline_nominal`; runtime `host_registration`, `host_nominal`, `offline_host`/`offline_types`, `host_objects`/`host_scopes`/`host_borrows`; VM host boundary fixtures | Owned function cases migrate in HR02. Live-root/path/reentry/reload cases migrate in RF05, with source-free ownership cases retained separately. Existing manually adapted collections keep their checked storage/path behavior; collection-library migration stays excluded. |
| Checked HIR/MIR/bytecode and contract host records; source-free loader | RF02 adds target/capability, receiver and non-escape facts. RF05 installs corresponding local storage/projector capabilities. Remove obsolete parallel host callable records only after their consumers migrate in HR03. Unsupported native reference bodies retain explicit interpreter selection. |

The compile-only prototype is in
[`tests/interop_api`](../crates/kagari-runtime/tests/interop_api/prototype.rs).
It proves marker-disjoint owned/shared/mutable/raw/primitive adaptation through
one `bind`, receiver-first tuples and mixed shared/mutable/owned arguments.
Rust closure context parameters must be explicitly typed (`&mut NativeContext`
or `&mut LoanContext`); untyped `_cx` did not satisfy trait inference. This is an
API inference requirement, not a passing-mode enum or another binding entrypoint.
Production adaptation will consume the declaration's arity/signature and retain
contextual checks on every concrete generic application; the prototype does not
claim production registration or runtime references are implemented.

Scope spelling is corrected to `runtime.with_host_scope(|scope| { ...
scope.execute_scoped(...) })`: the scope owns runtime access while lending host
storage. The prototype uses an invariant fresh scope brand with an environment
outliving the brand. `borrow`/`borrow_mut` reserve the host borrow for that brand,
not just until a view's last use. Scope output is independent of the brand.
Compiler rejection covers scope-token escape, root reaccess after consuming a
view, root movement, callback borrowed output/retention and shared capability
upgrade. The passing case admits successive shared/mutable scopes, two mutable
inputs and mixed inputs. No unsafe dereference, widened lifetime or borrowed
FromKagari implementation is used in this proof.

Receiver facts belong in portable native declarations, not inferred Rust traits
or codecs alone. Shared/Mutable requirements admit only a matching native target
or scoped view with sufficient capability; GC DTO instances never become Rust
references. Complete-pack type/value conversion precedes loans; validate storage,
owner/scope/generation and acquire the batch before callbacks; release all loans
before result conversion. LoanContext initially supplies cancellation without
heap allocation or script execution; RF04 may expose checked disjoint external
reentry, without changing Opaque's non-reentrant storage rule. Inline struct
projectors use raw field addresses and acquire no ancestor references.

The finite payload/value/Serde subset and identity decisions are recorded in
[RI00](rust-interop-design.md#implementation-phases). The prototype is temporary
proof, with removal owned by RF05/RF06 when these compile cases exercise production
APIs. It is not a second public adapter implementation.

Validation: `cargo test -p kagari-runtime --test interop_api` passed the executable
compile-pass case and six diagnostic-checked compile-fail cases. The initial run
expected a lifetime diagnostic for borrowed callback output; Rust correctly
rejected it at adapter selection (`E0277`), so the expectation was corrected.
`native_registration::explicit_binding_uses_the_same_signature_check` and
`native_conversion::argument_signature_errors_precede_any_user_conversion_effects`
passed as small regressions of the retained registration/conversion boundary.
Structure passed (1177 files, zero findings). Initial format check found only
new-driver layout; corrected before checkpoint. No Miri safety result is claimed:
this prototype constructs no pointers. Actual projector Miri belongs to RF04/RF06;
full workspace/feature/backend/platform checks remain CI-owned.
The selected VM regression
`typed_generic_native_selected_member_keeps_applied_object_results_and_rejects_other_shapes`
also passed: installed contextual generic mappings preserve object identity and
reject an incompatible concrete application before callback effects. Final
structure/format/diff checks passed. HR01/RF01's proof gate is complete; production
schema, scope and adapter delivery remains in the subsequent combined phases.


### HR02 checkpoint: common bind adapter

`native/adapter.rs` selects existing flat call-context conversion, owned contextual
conversion, owning receiver/tuple conversion, or an explicit `NativeBinding` from
the authored `FnDecl`. All builder forms expose `bind`: module functions, inherent
methods, explicit trait members and defaults. Contextual callbacks retain
per-application conversion checks rather than creating exact codecs up front.
Scalar/primitive entries retain their existing converter and execution path.
Caller context annotations select disjoint inference markers; no new roots or
invocation engine is introduced. The old alternate binding methods are removed
and their repository callers migrate directly. Current guides use `bind`.

This is a buildable slice, not the entire HR02 gate: inferred FunctionSpec and
standalone exact typed constructors, by-value legacy host consumers and shared
HostInterface declaration ownership still require migration. Live root/path
semantics stay at their existing owner until RF03-RF05, with final aggregation
retirement in HR03. No new compatibility bridge is introduced.

Selected validation: runtime/stdlib library check, explicit raw signature
mismatch/retry test and the source-free mixed DTO callback case passed.
The selected generic member regression also passed. Runtime lib/registration-test
scoped Clippy, structure (1184 files, zero findings), formatting and diff checks
passed. No production build failure remains. Full workspace/feature/backend/platform
coverage remains CI-owned.


### HR02 checkpoint: explicit signatures and cold producer integration

Removed `native/registration.rs` and public `FunctionSpec`, `add_function`,
`add_async_function`, `NativeBinding::typed` and `typed_method`. Existing synchronous
callers now author `FunctionDecl` parameters/results explicitly, then bind their
callback. The one concrete interface-return case explicitly calls `produces`;
compiler-selected boxing and pinned declaration identity remain intact. Default
library registrations still prepare preauthored metadata through the existing
contextual adapter; this is the same checked installation/conversion path.

The existing cold producer is an `Async<A>` adapter to `bind`, with explicit
`Future<R>` return declaration and typed owned tuple/Completion inputs. It retains
its existing exact argument/result codec check, capture validation, first-await
conversion/submission and completion/cancellation services. This migrates an
existing caller dependency; it adds no asynchronous borrowed-input feature.
Owning/reflected handles are never substituted for scoped Rust references.

The runtime-independence SDK case now installs an ordinary application module in
only one runtime and loads the same compiled artifact into both: the installed
runtime returns 42, the other rejects its missing binding, and heap roots remain
owner-specific. It preserves the original independence boundary while removing
that selected by-value legacy HostFunction consumer. Live-root/path/interface
consumers remain at their existing owner until the reference-capability facts and
scopes of RF02-RF05 can replace them coherently. HR02 remains unchecked while this
shared live-host declaration migration is outstanding; no legacy retirement or
combined acceptance is claimed from the owned delivery gate.

Validation passed: selected source-free VM owned/business-result conversion,
explicit interface output adaptation, output-error completed effects/trap cleanup;
three VM cold-producer lifecycle cases; SDK materialized documentation/navigation,
SDK owned native wait, cancellation/output publication, and two SDK installation
independence cases (across focused runs). `cargo run -p kagari --example typed_native`
ran and printed `["hello", "雪", ""]`. Runtime library and VM boundary-test scoped
Clippy passed, as did SDK Clippy limited to these three test targets and the two
migrated examples. Structure passed (1183 files, zero findings), format/diff passed.

Initial migration checks found duplicate/unused imports, lost returned declaration
references, a closure borrow overlapping its builder, and a missing cold-callback
tuple annotation; repaired before successful reruns. The new SDK runtime test first
used private `main` as a public named entry; corrected to the specified
`ProgramEntry::Main` selector and reran only that failure. Obsolete FunctionSpec
shape-inference assertions were removed with that API; authored raw signature
mismatch/atomic retry remains at `native_registration`, pre-effect pack validation
at `native_conversion`, and this VM case retains completed-output effects and trap
cleanup assertions. No carried production/test failure remains. Full workspace,
feature/backend/platform coverage and projector Miri remain in their designated
CI/RF04/RF06 scope.


RI04's next owned-conversion checkpoint supplies direct optional Serde and static
KagariSchema derive; its implementation and focused results are recorded in the
[interop ledger](rust-interop-design.md#ri04-checkpoint-direct-optional-serde-and-static-serialization-schemas).
It uses the same native bind/install boundary and checked object/conversion owners.
Serialization metadata supplies no Rust projection proof; ordinary Struct lending
and generated projectors remain the next RI04/RF04 integration slice. The earlier
whole-object Miri selection had no result at that checkpoint. The next checkpoint
provides the focused RF03 pointer gate; RF04-RF06 remain open.

The RI04/RF03 common-target checkpoint lifts that intermediate NativePayload bound
without weakening storage factories or Rust lifetimes. NativeType and ordinary Struct
lending share RustType descriptors and one receiver adapter. GC structs cannot supply
Rust loans. Focused VM tests, production lifetime proofs and actual-pointer Miri now
establish RF03's whole-object gate; details and the stopped earlier VM selection are
in the [interop ledger](rust-interop-design.md#ri04rf03-checkpoint-common-rust-targets-and-ordinary-struct-lending).
Field places, inline descriptors/projectors, atomic packs and SDK/live-host integration
remain RF02/RI04/RF04/RF05. This closes RF03 only, not combined acceptance.

The next RI04/RF04 checkpoint completes RI04's finite descriptor coordination:
KagariValue emits raw direct-field projectors matched transactionally to the same
schema, carried by local catalog mappings rather than portable Rust layout data.
The [interop ledger](rust-interop-design.md#ri04rf04-checkpoint-finite-generated-field-layout-descriptors)
records the focused generated-pointer Miri result. Runtime path/batch integration
and checked field operations remain RF04/RF02; no reference execution gate closes
from this descriptor result alone.

RF04's next checkpoint implements branded Rust field locations using those
descriptors and extends the existing borrow owner from whole-root counts to
generated inline path counts. The [interop ledger](rust-interop-design.md#rf04-checkpoint-scoped-inline-locations-and-one-path-loan-owner)
records actual-pointer Miri coverage of disjoint reentry, ancestor conflicts,
replacement and unwind. Atomic native packs and source/portable field operations
remain open; this checkpoint does not close RF04 or combined acceptance.

The RF04/RF05 common-pack checkpoint now supplies atomic external path acquisition
and mixed Opaque/external method input adaptation through the existing bind owner.
The [interop ledger](rust-interop-design.md#rf04rf05-checkpoint-common-atomic-borrowed-argument-packs)
records source-free tests, actual-pack Miri and repaired native authoring/target
derivation gaps. RF04/RF02 field execution, pinned projectors and RF05 SDK/live-host/
capability integration remain open; no combined completion is claimed.

The RF02/RF04 script-field checkpoint supplies checked portable scoped aggregate
facts, structured child locations, owned leaf reads and prepared assignments through
the same generated path owner as borrowed packs. Source-free field/pack execution,
forged capability rejection, root preparation/replacement and actual field-codec
Miri pass. The [interop ledger](rust-interop-design.md#rf02rf04-checkpoint-checked-script-inline-field-execution)
records local evidence, the repaired concrete-field transition and pending CI/KBC
batch. This closes RF02/RF04's field/path gates; SDK/context/capability forwarding
and live-host dirty-path migration remain RF05, followed by HR03/RF06/RI05 retirement.

The RF05 SDK checkpoint adds context-aware scoped entry to the existing native scope
owner. The VM requires explicit options, the SDK requires ExecutionContext, and JIT
policies reject before callback entry. Focused SDK, compile-fail, source-free library,
VM scope regression and example checks pass; the
[interop ledger](rust-interop-design.md#rf05-checkpoint-sdk-scoped-execution-context)
records evidence. Raw capability forwarding and live-host migration still own RF05's
remaining gate; HR02/HR03, RF06 and RI05 remain open.

RF05's formal shared capability checkpoint fixes the reproduced raw forwarding
defect at LinkedNativeFunction entry, with one rooted restricted ArgumentView seen
by every raw getter and call-scoped view. The caller's RefMut alias keeps its
capability. Source-free forwarding/GC/alias, two existing mixed-pack/field cases
and the actual path-loan kernel passed; the kernel also passed Miri (37.87 seconds,
nightly-2026-07-27 on aarch64-apple-darwin, default configuration). Runtime/VM
focused Clippy and structure (1229 files, zero violations) passed. The interop ledger
records reproduction, ownership and remaining legacy/reflective work. This does
not close RF05 or the combined delivery.

### HR02/RF05 integration boundary: existing live-root declarations

The remaining registry carries address-free external object identities and manual
field/index/virtual path services, not runtime-owned Rust payloads. Preserve that
execution storage and its whole-root/path borrow, preparation/commit, dirty ledger
and generation checks. Replacing it with an Opaque marker payload or an owned DTO
would change the contract and is outside this registration migration.

Move its nominal fields and methods into TypeDef/FnDecl/ImplDecl under ModuleDecl.
Retain only specialized external-root policy (ownership eligibility, reflection,
declared path/read access), passing styles and effects beside those shared facts;
do not retain a second parameter/result/name authority. Paths refer to these
shared field/type identities. Existing HostValueType conversion, standalone
HostInterface aggregation/format and HostFunction registration retire after their
consumers use the shared records. Ty::Host remains the semantic external-ID target;
it does not imply a Rust layout or admit generated Ref<T> loans. Generated Rust
Ref/Opaque mappings retain their already-proved target boundary.

Keep each slice buildable: first introduce validated portable policy facts and
round-trip/remapping proof, with production admission closed until its storage and
adapter are installed; then migrate declaration ingestion and native callback
adaptation together with selected live-root consumers. The final adapter uses the
same declaration-aware bind, NativeImport and installed callback dispatch. Remove
the old host callable linking/invocation registration at HR03, while retaining
the required root/path execution services. No generic permission matrix or host
storage/lifecycle overhaul is introduced. Source-free, owner/schema, path commit,
reentry/cancellation and pinned-reload cases remain separate behavioral evidence.

The existing warmed scalar/contiguous-i32 boundary check also passed after the
shared-capability change: 100000 scalar calls and 1000 calls for lengths 0/16/16384
each produced zero allocations/reallocations/deallocations in their warmed windows.
This preserves that contract; it is not a speed comparison or a claim about DTO/
reference calls. The fixture excludes compilation and setup from those windows.
The allocation-contract run used rustc 1.98.1 (48a229cea), Apple M1 Max/arm64,
the workspace test profile (inherited dev opt-level=1), default features/parallelism
and an already-built target; each measured window followed the fixture's warmup.

### HR02 checkpoint: portable external access policy in shared declarations

TypeDefKind::Host now carries only external identity eligibility/reflection/path
policy. FieldDef keeps the one name/type/visibility/write contract and adds optional
external read/path policy. FnDecl owns the one parameter/result signature and adds
optional passing/effect policy. Shared validators enforce capability/count rules,
parameter-label uniqueness and immutable configuration shape restrictions. Ordinary
DTO fields reject external policy. Ty::Host resolves its defining module through
the common receiver owner without becoming a Rust Ref target. Identity remapping,
interface instantiation and the canonical records preserve policy facts.

Two focused types contract cases passed: canonical compilation input across
adoption, type/function record round trips, invalid paths/borrow styles, duplicate
parameter labels, immutable scalar/Option acceptance and owning-container rejection,
and oversized policy decoding rejected before truncated elements. The existing
public ABI round-trip case and source-free shared-forwarding regression passed.
Runtime/HIR library checks and focused library Clippy passed; types library/test
Clippy, structure (1231 Rust files, zero violations), formatting and diff passed.
Intermediate missing kind handling/constructors and test fixture/API diagnostics
were repaired; there is no carried local build/test failure.

This is the portable model slice, not executable legacy migration. NativeModule
explicitly rejects these new host types and policy-bearing entries until RF05's
external-ID adapter is installed. That finite admission gate and the old enum
owners under host_interface retire during the HR02/RF05 migration/HR03 cleanup;
there is no compatibility reader or alias. Existing live-root registration remains
unchanged until consumers migrate. Fn/field wire layouts changed without an
unpublished format/ABI identifier bump; disposable KBC fixtures join the already
pending RF06 regeneration batch. Full workspace/feature/backend/platform validation
remains CI-owned and unrun locally. HR02 and the combined goal remain open.

### HR02/RF05 checkpoint: external policy at ordinary native entry

FunctionDecl now authors the passing/effect policy beside its shared signature.
The inferred HostCallContext callback adapter binds that declaration through
ModuleBuilder::bind, exact policy proof, NativeModule installation and ordinary
NativeImport dispatch. Raw/owned callbacks cannot bind a policy-bearing signature;
a HostCallContext callback cannot bind an unclassified signature. Function ABI
lowering preserves that shared policy through source-free execution.

The adapter reuses HostResourceScope, temporary roots, the existing borrow table,
candidate authority and execution session. It checks candidate arguments/effects,
starts recording, validates rooted argument types, acquires access, enters the
callback, validates result/type, polls cancellation, rejects borrow escape and
checks heap validity. Callback errors keep their provenance; cleanup on errors and
unwind uses the existing scope. Recorded symbols now identify the canonical module
and declaration path (including nonzero occurrence); a separately registered
legacy string label does not become a new execution lookup system. Recording is
optional and does not change unclassified scalar/flat callbacks.

Focused evidence: runtime/compiler library checks and
`cargo test -p kagari-vm --test native_boundary host_policy_uses_native_entry_with_rooted_inputs_and_execution_session -- --nocapture`
passed. The source-free, GC-threshold-one case checks declaration/binding policy
mismatch, retained String inputs, deterministic time, success/error call traces,
immutable configuration versus denied candidate services, cancellation precedence,
and unwind cleanup followed by healthy execution. Initial test compilation
reported an unsupported zero-argument unit callback spelling and undereferenced
GC String guard; these were corrected. Its
first error assertion omitted the existing `host call failed:` prefix and was
corrected without changing production error semantics. No carried local failure.

`cargo run -p kagari --example host_reentry` passed after migrating its callback
from register_host_function to one explicit native declaration/bind/install. The
example reenters the current pinned script root, keeps a scratch Array rooted,
retains the callback result through collection, observes two script frames and
releases it after host retention ends. The shared-capability forwarding regression
and warmed scalar/contiguous-i32 allocation selection passed. The latter measured
zero allocations/reallocations/deallocations/bytes for 100,000 scalar calls and
1,000 calls at each bulk length 0/16/16,384. Environment: rustc 1.98.1, Apple M1 Max,
aarch64-apple-darwin, workspace test profile inheriting dev opt-level 1, default
features/parallelism/target and already built warmed execution windows; compilation
and setup excluded. No speed claim or zero-allocation claim for host scopes/DTOs.
Runtime/compiler library, VM native-boundary and SDK host_reentry example Clippy
passed; structure (1233 Rust
files, zero violations), formatting and diff passed.

The policy-bearing function admission gate is removed because its production
adapter exists. Host TypeDef installation is still gated: live root/type/field/
path metadata and consumers migrate next; the legacy function/type registration
entrypoints remain until that finite migration and HR03 retirement. General heap
admission has not been widened to accept ephemeral external borrow tokens.
No new unsafe projection or lifetime model was introduced; prior focused Miri
remains evidence for unaffected loan/projector owners. Exhaustive workspace,
feature/backend/platform acceptance and pending fixture regeneration remain CI/RF06
owned. HR02/RF05 and the combined goal remain open.

### HR02 checkpoint: shared external type declaration ingestion

Registered native declaration views now resolve Host TypeDefs to Ty::Host at their
shared Struct-kind identity. They retain the checked TypeDef in the HIR native
representation, including remapped member types, and render the address-free type
as an opaque source declaration. This is not ordinary GC field syntax or a Rust
layout. Compiler ABI projection copies its shared fields/access policy directly;
it does not reconstruct them from nonexistent GC fields in that source view.

Focused HIR declaration ingestion passed, including exact external target typing,
Ref/RefMut target rejection and rejection of unregistered methods. The selected
compiler ABI case passed: public writable scalar field, private readonly recursive
external field, preserved policy/nominal scope and public ABI validation. Initial
check/test compilation reported a missing Host representation match, missing
schema initializer and wrong AnalysisResult field access; these are repaired.

A temporary full-MIR probe at this architecture boundary deliberately used the
same declaration fixture and ordinary `snapshot.check_program` followed by
`lower_program_to_mir`. It failed with
`Lowering { module: example::roots, error: MissingBinding("nominal host type declaration") }`.
Reproduction: register a Host TypeDef with a `parent: Ty::Host(self)` field through
native module inputs, analyze `fn main() {}`, check its provider closure, then lower
the checked program. The collector at source/lower/host.rs still resolves only the
old HostDeclarations table. HR02/RF05's next shared host dependency/runtime metadata
migration owns this integration error; it is not full MIR/bytecode acceptance.
The temporary probe was removed; the ABI contract test retains its distinct
passing coverage. The existing scoped-reference forwarding/field regression passed;
HIR/compiler library/test Clippy, structure (1235 Rust files, zero violations),
formatting and diff passed. NativeModule Host-type execution remains closed until that
collector, source-free checks, runtime metadata and live-root installation migrate.

The next runtime boundary must stage reflected TypeDef/FnDecl/ImplDecl metadata and
native registration together before publishing either. NativeInstallation prepares
an immutable metadata seed for independent runtimes; each runtime owns its TypeId
slots and external-root registry. Runtime-local installation stages against current
metadata so existing roots retain their owner/schema/ABI facts. Manual field/path
storage services remain the same owner and must consume the common field/type IDs;
no converted second declaration or implicit Rust layout is needed. Legacy input,
callable linking and enum owners retire at HR03 after consumers migrate.

No unsafe pointer/lifetime changes; previously passing loan/projector Miri remains
applicable. Full workspace/feature/backend/platform checks stay in CI. HR02/RF05
and the combined goal remain open; the recorded new external-type MIR integration
failure must be resolved before their acceptance.

### HR02 checkpoint: shared external policy ownership

HostPassingStyle, HostFunctionEffects, HostTypeOwnership, HostReflectionPolicy and
PathAccess now belong to declaration::host alongside their shared policy records.
Consumers import this owner directly. The former public paths are removed without
forwarding exports or duplicate enums; variant order, serialized shape and access
checks are preserved. Legacy two-case Visibility remains with its declaration
until that declaration is retired: the common four-case visibility cannot be
substituted mechanically without reviewing access behavior.

Selected validation passed: runtime/compiler library check, the two shared policy
contract tests, the existing Host metadata record roundtrip/import test, and types
library/selected-target Clippy. Structure (1235 Rust files, zero violations), fmt
and diff checks passed. The mechanical import pass first missed a grouped path
and private imports in the former defining modules; the resulting compilation
errors are repaired. No unsafe/lifetime change; prior Miri evidence still applies.
No workspace sweep or feature/backend/platform matrix was run; CI owns those.
The previously recorded new external-type MissingBinding MIR error remains carried
by HR02/RF05 dependency and metadata integration. This checkpoint does not satisfy
that gate or complete the combined goal.

### HR02 checkpoint: shared external dependencies in executable products

The previously recorded new external-type `MissingBinding("nominal host type
declaration")` failure is resolved. HIR exposes the original checked local/imported
external TypeDef by its nominal ID. MIR dependency collection retains that schema
with its ID in TypeDeclaration, the same identity-plus-shared-record pattern as
NativeDeclaration. It does not translate fields or methods into the old Host DSL,
create a Rust layout, or infer new semantics. Recursive external fields close and
deduplicate this dependency set; ordinary DTO nominals are not added as live roots.

MIR and bytecode carry these dependencies through bounded codecs and explicit
scope remapping. The existing common type/public declaration validator checks the
schema, names, private fields and policy. External identities in public records,
native templates/targets/call signatures and executable semantic slots require a
carried dependency. Old and shared records cannot claim the same ID. Program
linking requires the carried shared schema to equal the actual TypeDef in its
reachable defining module, so a self-consistent dependency copy cannot authorize
different fields/access or substitute an unrelated provider. These checks add no
native callback route and do not relax roots or borrow validation.

Selected validation passed: the expanded existing external ABI contract case now
analyzes a relay signature plus a private recursive external field, lowers the
complete MIR provider closure, roundtrips/revalidates MIR and KBC, checks the KBC
loader, and rejects independently valid readonly dependency copies that disagree
with their defining module in both products. The existing legacy host import
interning/tamper case and imported script trait implementation case passed. Initial
mechanical code and test compilation mistakes (mapper method, declaration/name
owner and an omitted artifact compatibility import) are repaired; there is no
carried Rust build error. Library Clippy covered changed types/HIR/contract/MIR/
bytecode/compiler owners, and compiler library/test Clippy passed. Structure
(1236 Rust files, zero violations), formatting and diff passed. No unsafe/lifetime
change; existing Miri loan/projector evidence remains applicable. Full workspace,
feature/backend/platform validation remains CI-owned and unrun locally.

NativeModule external-type execution is still gated: transactional runtime metadata
and existing live-root/path consumers must consume the shared schema next. The
legacy declarations remain only for their unmigrated consumers; HR03 must remove
those records/registration paths after the existing manual path and trait coverage
has migrated. The portable layout changed without identifier bumps under the
unpublished-artifact policy; affected disposable fixtures remain batched at RF06.
HR02/RF05 and the combined goal remain open.

### HR02/RF05 checkpoint: installed authority for shared external schemas

Runtime source-free loading now requires every carried external TypeDeclaration
to equal its exact installed NativeModule catalog TypeDef. Installed external
schemas, like ordinary value schemas, also require their defining module's public
record to remain exact even if no callback imports it. Portable shape/provider
validation alone cannot publish an uninstalled external identity. The existing
transactional loader rejects before module publication; roots and callbacks are
not fabricated to satisfy the check.

The focused runtime loader boundary passed with a source-free external schema:
loading without its installed contract rejects with the expected module-validation
message and leaves the module store empty. Runtime library Clippy, structure
(1236 Rust files, zero violations), fmt and diff passed. A preliminary attempt to
put this loader assertion in the compiler ABI test failed because that crate has
no direct runtime dependency. The assertion was moved to its runtime contract
owner; the compiler test was restored and no new dependency was introduced.
No unsafe/lifetime changes or new Miri requirement. Existing portable dependency
and old host/trait regression results remain evidence; exhaustive CI scope is
unrun. Runtime shared metadata/root installation and HR02/RF05 acceptance remain
outstanding, with no carried Rust build failure.

### HR02 checkpoint: external catalog closure and metadata visibility

Native registration dependency collection no longer skips Ty::Host. It requires an
actual common Host TypeDef, follows its fields recursively through the existing
schema dependency resolver, and retains exact records for per-binding/install
validation. A Struct/DTO ID cannot supply an external type just because both IDs
use DefinitionKind::Struct. Recursive external identities deduplicate normally;
unrelated DTO schemas do not become external objects or implicit method exports.

FieldInfo now uses common declared Visibility directly. The remaining old Host
metadata producer explicitly maps its Private/Public cases to the same common
cases. Manual path publication still requires exactly Public, preserving hidden,
disabled and readonly field rejection. The other common visibility cases remain
representable without conflating them with old access facts; this changes the
FieldInfo Rust API directly without a forwarding type or ABI/version bump.

The new native catalog closure boundary passed (private recursive parent plus
another external schema, no unrelated DTO retention, forged DTO-as-external
rejection). The existing nominal field/path contract and metadata registry/path
records cases passed. A mistyped initial path test-name filter selected zero tests;
this was corrected and the actual existing contract ran. An obsolete test import
warning was removed. Runtime library/test Clippy, structure (1236 Rust files, zero
violations), fmt and diff passed. No unsafe/lifetime change; previous Miri remains
applicable. Complete workspace/feature/backend/platform tests are CI-owned and
unrun. Shared runtime metadata preparation/installation and legacy retirement
remain the next HR02/RF05 work; the combined goal is still active.

### HR02 checkpoint: transactional shared external metadata snapshots

The native installation owns an immutable reflection seed prepared directly from
its checked TypeDefs. External nominal rows are reserved before fields, preserving
self/mutual recursion. Owned member types use the same Ty/schema identities and
actual generic substitutions; DTO rows do not acquire methods, traits or external
storage. Root and field fingerprints include the declaring identity as well as the
schema, so identical scalar fields in different modules remain distinct contracts.
Preparation rejects unknown/forged external targets, mismatched nominal storage,
unresolved/reference member types and metadata name conflicts before publication.

Runtime construction copies the prepared value-only rows into independent mutable
registries. Runtime-local installation stages from its current metadata, checks
all owners/bindings and prepares the complete candidate before publishing entries
and metadata together. Existing runtime-local rows survive successful installation;
failed preparation cannot publish a partial root or field graph. The Ty-to-row
index belongs to this registry/definition context and persists only with its
checked immutable installed schemas; it is neither a Rust pointer cache nor a
second object store. Legacy metadata remains solely for unmigrated consumers and
is still scheduled for HR03 retirement.

Selected validation passed: two metadata boundaries cover recursive external and
ordinary DTO fields, declared visibility/access, distinct-owner fingerprints,
failure after candidate reservation and independent runtime copies. The existing
native installation rollback/isolation case now also checks preservation of a
runtime-local metadata row. Runtime library/test Clippy passed after replacing a
bool-then filter with an explicit filter/map; initial missing import and nominal
display formatting compilation errors are repaired. Structure (1238 Rust files,
zero violations), fmt and diff passed. No unsafe or lifetime changes; previous
Miri evidence remains applicable. Full workspace/feature/backend/platform coverage
remains CI-owned and unrun locally.

This seed currently installs fields and owned member types. Explicit common method/
trait metadata, live-root publication and manual paths remain the next coherent
integration slice. The external NativeModule execution gate remains closed until
those consumers use the shared records; HR02/RF05 and the combined goal remain open.

### HR02 checkpoint: explicit common external members

The checked native catalog now retains inherent ImplDecls as well as trait
ImplDecls. These are the original common records, not a reflection-only member
DSL. Trait proof construction selects only explicit trait impls; inherent records
grant no trait witness. Source-free dependency matching recognizes the existing
InherentTable contract separately from the existing InterfaceTable contract.

External metadata reads its concrete receiver/impl/member records from that
catalog. Inherent members and explicit Kagari trait members retain their distinct
origins. It includes the checked NativeDeclaration (identity, policy, signature,
bounds and requirements) in method fingerprints and the applied Kagari trait's
schema/identity in trait fingerprints. Reflection slots belong to the runtime
metadata snapshot and remain stable when later explicit impls sort before prior
members. A DTO's schema does not infer Rust methods/traits or become an external
object. This adds no callback dispatch route or Rust reference.

Kagari traits keep their independent Self signature. A concrete native impl may
declare external passing/effects when the trait has no such storage policy; the
common function validator and ordinary binding-policy equality still validate
that explicit local fact. Signature, bounds, associated output and declared
override checks remain exact. The builder's concrete external-member authoring,
runtime root/path publication and source access migration remain the next slice;
external NativeModule execution stays gated.

Selected validation passed: all three metadata boundaries, including explicit
inherent/trait members and stable slots after another module's impl is added;
existing generic native trait receiver/binder registration; reused foundation
installation requirements; and the VM native inherent method/prepared-call case.
The new member test initially reused a foreign catalog scope for the second
snapshot and correctly hit the duplicate-name guard; it now explicitly adopts
the expanded checked catalog into the installation's retained context. An initial
private metadata ID accessor error is repaired without widening public APIs.
Runtime library/test and types library Clippy passed. Structure (1239 Rust files,
zero violations), fmt and diff passed. No unsafe/lifetime changes or new Miri
requirement. Exhaustive workspace/feature/backend/platform validation is CI-owned
and unrun locally; no carried Rust build/test failure. HR02/RF05 remain open.

### HR02 checkpoint: common external type/member authoring

The existing TypeBuilder now selects either traced native storage or an external
identity policy. External declarations use their real Struct ID, have no generic
parameters/Rust storage binding, and adopt common scoped FieldDefs without a
second field schema. TypeRef applies the identity as Ty::Host and uses the ordinary
value adapter; it never describes a Rust reference or a GC native payload.
TypeBuilder finishes through the shared type validator before retaining the row.

Inherent MethodDecls and explicit trait MethodsBuilder members can declare external
passing/effects beside the existing signature. Trait impl policy must be selected
before bind, cannot change after callback selection, and the ordinary binding
policy equality rejects a raw binding without that authority. The concrete impl
owns the final policy; the independent trait Self signature remains unchanged.
Common declaration validation now requires a native signature explicitly containing
an external ID (including nested arguments/results) to carry its access policy.
Script relay signatures and trait Self templates acquire no automatic host policy.
The recursive catalog closure fixture now declares its Owned-ID passing explicitly.

Selected validation passed: common external authoring/binding policy boundary,
both portable host-policy roundtrip/rejection cases, recursive external catalog
closure, existing generic receiver/binder registration, full external ABI/MIR/KBC
boundary, and the existing imported script trait implementation case. A negative
test initially used Value as a flat typed Rust argument, which that adapter does
not support; it now uses the actual existing explicit raw binding API and observes
the intended policy rejection. Runtime library/test and types library Clippy,
structure (1239 Rust files, zero violations), fmt and diff passed. No unsafe or
lifetime changes; prior Miri remains applicable. Exhaustive matrices stay in CI.

The authoring proof still observes the explicit external execution gate at module
finalization. Runtime root slot publication, common manual paths/source member
access and source trait references remain the owning HR02/RF05 integration work;
this checkpoint does not claim those APIs runnable or combined acceptance complete.

### HR02 checkpoint: shared whole-root installation and execution

Prepared external metadata now binds its original declaring identity to the
existing host registry's physical TypeId slots. Runtime construction publishes
that checked value-only mapping into an independent registry; local installation
stages it alongside native entries and reflection before publishing any of them.
Root registration keeps the existing object IDs, owner, schema epoch, ownership
and access checks. No Rust object enters the heap and no legacy nominal schema is
manufactured from a shared TypeDef. The temporary legacy slot producer belongs
to the existing HR03 retirement scope.

Source-free module linking requires the exact shared external schema and its
installed slot. NativeModule now admits external types without manual field
paths; the execution gate remains for declared field paths until their shared
contract validation and execution are integrated. One common inherent method and
one explicit Kagari trait method execute through the ordinary VM/native path.

Selected validation passed: the new source-free root case exercises generic
trait dispatch, inherent mutation, collection inside both callbacks, subsequent
exclusive borrowing and foreign-runtime rejection before callback entry. The
existing installation failure/isolation case passed. Structure checked 1240 Rust
files with zero violations; fmt, diff and runtime library/test Clippy passed.
No unsafe or lifetime changes;
previous Miri evidence remains applicable. Exhaustive matrices remain CI-owned.
Manual paths, source trait references and dependent legacy retirement remain
HR02/RF05/HR03 work; the combined goal is not complete.

### HR02 checkpoint: one shared manual path contract owner

Manual path records now consume Ty and resolve fields against common TypeDefs.
The common schema/value/field fingerprint owner is `types/declaration/fingerprint`;
runtime external reflection uses that same owner. The superseded HostInterface
resolver has been replaced by a producer conversion into those common records,
not a second path validator. Its temporary `host_interface/paths.rs` adapter,
including the hidden `path_types` producer hook, must be removed in HR03 when the
remaining legacy fixtures and producers migrate.

Runtime path preparation binds those common declarations to the existing physical
root/field slots. It verifies the declared root, result, every segment input and
access, and retains descriptor/root owner, epoch, dynamic argument, borrow, commit
and dirty-path checks. The old independently computed path fingerprint and the
now-redundant second portable resolution are removed. Shared field fingerprints
include their original schema/owner; legacy metadata remains only for its unmigrated
producers. This changes disposable development path encodings/fingerprints without
bumping publication identifiers; affected retained fixtures remain RF06-owned.

Selected validation passed: the three existing portable path boundaries cover
readonly index authority, mixed field/index/virtual wire roundtrip and nested field
owners; physical fingerprint stability and unbound-type rejection passed in the
runtime. The multi-index/virtual/trailing-field SDK source-free case passed; all
three external metadata boundaries passed after moving their fingerprint owner.
The atomic_host_path example ran and preserved readonly hp=20 before its writable
update to hp=30. Types/runtime library/test Clippy, structure (1242 Rust files,
zero violations), fmt and diff passed. Initial wire, bytecode, MIR, compiler and
HIR consumer errors are repaired. The mixed path negative case exposed a missing
external-schema closure check for index parameters; that check is restored and
its original assertion passes. No carried build/test failure, unsafe or lifetime
change; prior Miri remains applicable. Exhaustive matrices remain CI-owned.

New NativeModule field-path execution remains gated. Its explicit portable path
requirements must now be retained beside common module declarations, checked and
installed transactionally, then consumed directly by source analysis and executable
linking. Path views must use the same native entry with their declared capability
and existing path borrow/epoch checks. Source trait references and dependent legacy
retirement remain HR02/RF05/HR03 work; combined acceptance remains open.

### HR02/RF05 checkpoint: explicit module paths through source-free execution

`ModuleDecl::paths` now owns explicit manual contracts at the root type's declaring
module. Catalog adoption, canonical compiler input and dependency closure retain
their identities and index/virtual result requirements. The actual path model and
mapping move directly to `types/declaration/path`; its bounded file codec belongs
to `declaration/wire/path`. All callers import those owners. There is no forwarding
module or compatibility alias at the retired `host_interface::path` location.

`ModuleBuilder::define_path` and `bind_path` retain declaration-first authoring and
check duplicate/missing bindings plus declared read/prepared-write capability.
The existing HostPathAdapter remains a storage-operation hook, not a method-call
route. NativeModule and NativeInstallation prepare physical field/index/virtual
types, descriptors and adapters before publication through the original root/path
service. Runtime-local installation preserves roots, loans and dirty records;
reusable installations create independent runtime registries. A metadata conflict
publishes neither new callables nor path descriptors. `resolve_host_path` looks up
an installed scoped declaration without registering another path.

HIR retains shared external schemas and explicit paths on installed native types,
including unnamed path targets without importing additional source names. One
source selector serves remaining legacy producers and new module declarations;
the common path contract resolver still owns semantics. MIR retains those facts;
bytecode now emits `path_declarations` outside HostInterface. Verification resolves
all path requirements, checks physical records and result/write contracts, and
source-free linking requires exact installed schemas and bound adapters, including
unused declarations. The legacy type-to-schema producer conversion and envelope
decode hook remain temporary HR03 removal work; they implement no second resolver.

The existing SDK multi-index/virtual/trailing-field case migrates to common native
declarations and bindings. Its trace remains `make, first, second, rhs, read,
prepare, commit, read`, result 13 and dirty record 10→13. The atomic_host_path
example also migrates and demonstrates readonly rejection plus prepared commits.
Current contracts are updated in the host interop specification.

Focused local validation (full workspace/features/backends/platforms remain CI):

- `cargo test -p kagari --test offline_nominal source_multi_index_virtual_and_trailing_field_use_one_host_path`: passed one contract, exercising direct interpreter, KBC-roundtripped source-free interpreter and current-platform Cranelift execution.
- `cargo test -p kagari --test offline_nominal source_field_chains_use_offline_contracts_and_evaluate_the_root_once`: passed one regression; once-only root evaluation and missing path admission remain checked across that same selected execution set.
- `cargo test -p kagari-runtime --lib external_authoring_uses_common_type_and_member_binding_policies`: passed one authoring/installation contract, now covering capability mismatch, independent runtime roots and transactional metadata-conflict rollback.
- `cargo run -p kagari-runtime --example atomic_host_path`: passed; output `readonly view preserved hp=20; writable path updated hp=30`.
- `cargo test -p kagari-bytecode --test identity artifact_preserves_portable_virtual_path_declarations`: passed after migrating its fixture to shared schema/path records and preserving the corrupt-ID assertion. The initial run parsed an obsolete wire prefix; the first migrated fixture also lacked its defining public schema and correctly failed provider validation. The repaired fixture retains that exact provider, identity adoption, cancellation, canonical bytes and invalid-ID rejection coverage.
- `cargo clippy -p kagari-runtime -p kagari-compiler -p kagari-hir -p kagari-bytecode --lib --tests -- -D warnings`: passed the affected default library/test compilation scope. Structure checked 1246 Rust files with zero violations; formatting and diff checks passed.

Initial compilation exposed redundant old error wrappers at the moved wire owner,
four test import scopes still naming policies under retired owners, and an example
using a private catalog query. These are repaired directly; the example uses public
scoped metadata rather than widening catalog visibility. No unsafe/lifetime change
is introduced, so previous scoped Miri/compile-fail evidence remains applicable;
this metadata/path checkpoint does not claim a new Miri run or full acceptance.

The native field-path installation gate is open. Manual path-view arguments and
their formal shared/unique capability checks, explicit source trait references,
legacy producer/callable retirement and final RF06/RI05 integration remain required.
HR02/RF05/HR03 and the combined goal are still open.

### HR02/RF05 checkpoint: manual views at the ordinary native boundary

Runtime type matching now validates manual roots, complete path-view chains and
live external-ID tokens through one checked host identity helper. A view must
match its installed result slot, registry/root identity, descriptor continuity,
schema epoch, retained dynamic arguments and inherited capability. Native host
adaptation and temporary resource scopes consume that helper; the old boundary's
token-type-only special case is removed.

Formal shared host arguments publish rooted readonly view/token aliases before
callback getters run, preserving the caller's writable capability. Token weakening
retains the same owner/frame/epoch and validates against the original unique loan;
it cannot satisfy unique access or outlive the original frame. Chained views cap
access at their parent's capability, and writes check the supplied view as well
as the target descriptor. Manual method adaptation takes the actual whole-root
lease, preserving existing path-service reentry conflicts without manufacturing
Rust pointers or implicit DTO conversions. Token methods move beside the loan
table to keep the host facade below its effective-LOC limit.

Selected validation:

- `cargo test -p kagari-vm --test native_boundary shared_root_registration_runs_native_methods_and_explicit_traits_source_free`: passed the extended existing contract. The same inherent method and explicit trait callback execute for roots and writable path views, readonly views invoke shared methods and reject mutable methods before mutation, readonly aliases cannot create writable children or write through an otherwise writable descriptor, original views retain writable access, GC runs inside callbacks, whole-root unique reentry conflicts and foreign-root rejection remain checked.
- `cargo test -p kagari-runtime --lib shared_external_alias_preserves_owner_epoch_and_original_unique_token`: passed the token owner/epoch/weakening/cleanup boundary. The initial test compile used a nonexistent RootSet append method; a preallocated root slot replaces it. Moving token methods exposed one missing explicit owner import, repaired directly before rerunning affected integration.
- `cargo +nightly-2026-07-27 miri test -p kagari-runtime --lib shared_external_alias_preserves_owner_epoch_and_original_unique_token -- --nocapture`: passed one case in 4.76 seconds, aarch64-apple-darwin, workspace test profile/default parallelism/default Miri target, MIRIFLAGS unset. This tests the actual token/GC boundary; prior projector Miri evidence remains applicable to unchanged pointer kernels.
- Runtime library/test compilation and VM library/selected native_boundary target Clippy passed. Structure checked 1247 Rust files with zero violations after the responsibility split; formatting and diff checks passed.

No unsafe implementation or Rust lifetime relaxation is introduced. Explicit
source-trait references, legacy producer/callable retirement and RF06/RI05 final
integration remain required. Exhaustive workspace/features/backends/platforms are
pending CI; HR02/RF05/HR03 and the combined goal remain open.

### HR02/RF05 checkpoint: explicit source-trait requirements

`ModuleBuilder::reference_trait` adopts a scoped expected `TraitDef` plus portable
identity and returns the existing `TraitRef`. `ModuleDecl::trait_references` owns
that expectation; it defines/exports no synthetic source trait. Identity traversal,
canonical compiler inputs, declaration catalogs and binding dependency closure
retain its exact contract, including unused references. A shared bounded validator
rejects reserved/native adapter authority, invalid identities and wrong ownership;
authoring additionally rejects duplicate references. Required method bindings and
associated outputs use the existing trait impl/bind owner. Script defaults may be
omitted from the native impl and retain their actual source bodies.

Installation publishes checked expectations with the ordinary native catalog,
without a second callable route. Existing source-free dependency and installed
trait validation require the defining program's exact public ABI. Foreign source
module dependencies resolve only for their checked native importer; nominal
surface preparation includes those exact modules and cache comparison retains
their identities. No global package alias is added, and normal visibility checks
still apply. SDK construction checks the closed source-independent native subset;
the complete generated set is checked with actual user source. Source-independent
engine baselines retain their warmed analysis rather than warming a discarded
clone. The split is necessary because reference expectations cannot manufacture
the source trait being referenced.

The existing callback-publication/portable trait fixture migrates to this API,
renamed `source_trait_reference_uses_explicit_binding_and_portable_abi`. It obtains
the expectation from checked source ABI, retains a live external Counter root,
requires `get` binding, supplies Output=i32, executes the source `again` default
through KBC roundtrip and returns 42. It rejects native authority, duplicate
references, missing actual source, unbound required members and a coherent changed
source ABI at runtime loading; ordinary foreign scripts still cannot use the native
importer's source dependency alias. GC runs inside the bound callback.

Initial fixture compilation exposed nonexistent TypeRef/PreparedProgram helpers;
the test uses actual type application and the artifact's retained scope. Required
member identities were initially absent after record adoption; implicit member
identity collection now precedes shape validation in both authoring and declaration
construction. Native foreign-source name resolution needed both dependency package
lookup and nominal surface preparation; the scoped module dependency owner handles
both. A policy-only manual artifact corruption was rejected by the existing
interface verifier; the final negative case instead recompiles a coherent changed
source contract, preserving verifier strength. No carried production failure or
lifetime/unsafe change; prior pointer Miri evidence remains applicable. Additional
legacy consumers and combined HR03/RF06/RI05 acceptance remain open.

Focused local checks passed:

- `cargo test -p kagari --test offline_nominal source_trait_reference_uses_explicit_binding_and_portable_abi`: one migrated source/source-free contract passed after the final SDK validation split (17.66 seconds).
- `cargo test -p kagari --test default_methods defaults_can_call_inherited_methods_and_use_associated_outputs`: one directly affected script default/associated output regression passed.
- `cargo test -p kagari-hir --lib body_edits_rebase_impl_trait_references_in_signature_cache`: one signature cache/trait identity regression passed.
- `cargo clippy -p kagari --lib --test offline_nominal -- -D warnings` and `cargo clippy -p kagari-hir -p kagari-runtime -p kagari-types --lib -- -D warnings`: affected SDK target and declaration/import/binding libraries passed. Structure checked 1248 Rust files with zero violations; formatting and diff checks passed.

No new Miri run is claimed for this address-free metadata/source checkpoint.
The previous token and projector checks cover unchanged lifetime/pointer kernels;
full workspace, feature, backend and platform matrices remain pending CI.

### HR02/RF05 checkpoint: SDK function and tooling consumers

Six SDK fixture files now declare, bind and install application functions/types
through ModuleBuilder and NativeModule. Offline function facades retain alias
chains, exact common import/declaration identity, absent and incompatible provider
rejection before callback entry, direct/KBC/interpreter/host-Cranelift traces and
cold compilation without constructing a Runtime. The scalar constant case keeps
compile-time native calls forbidden and proves compilation invokes no callback.
The failing-shift case retains the GC-owned Vec, completed index/RHS effects and
unchanged target after the trap. No value snapshot substitutes for live state.

Canonical external artifact coverage now adopts A/B declarations in opposite
identity allocation histories through the shared catalog. The final provider has
one identical semantic declaration order in both histories; artifact bytes remain
identical. Package input coverage compares complete native provider configurations,
keeps already checked package identities and pinned old calls, and verifies that
fresh sessions retain the configured NativeModule providers. Mutating an engine's
legacy interface is no longer used by these consumers.

Source-module trace fixtures bind their callback once before compilation. Missing
providers fail loading the entire dependency closure even when the root function
contains no native call, publish zero modules, and invoke no callback. The correctly
configured Runtime loads at epoch one; unused callback bodies remain unexecuted.
Execution reports preserve the ordered [4, 2, 1, 3] host results, deterministic
inputs and identical source-free traces.

Initial compile failures were missing an independently used DefinitionContext
import, an incorrect FunctionDecl parameter spelling, and an incorrect scalar
callback context/result spelling. These fixture-only errors were repaired. Native
package members require explicit source imports; migrated source uses ordinary
`use` declarations rather than the legacy interface's implicit global namespace.
Two offline cases initially failed with unknown names and passed after that repair.
No carried implementation failure and no unsafe/lifetime change.

Focused local validation passed (eight existing affected contract cases):

- `cargo test -p kagari --test offline_functions --no-fail-fast`: the facade/backend
  trace case passed in the first run; the other two initially failed as above.
  Subsequent filtered runs of `offline_declarations_compile_without_a_runtime_then_link_and_execute`
  and `declared_host_calls_compile_but_cannot_run_in_scalar_constants` passed.
- `cargo test -p kagari --test numeric_operations failed_shift_keeps_target_and_completed_rhs_effects`: passed.
- `cargo test -p kagari --test source_snapshots host_artifacts_are_canonical_across_declaration_adoption_histories`: passed.
- `cargo test -p kagari --test package_inputs captured_packages_share_the_sdk_pipeline_and_retain_generations`: passed (20.58 seconds).
- Filtered `source_modules` runs of `execution::execution_report_records_code_inputs_and_ordered_host_results`
  and `execution::dependency_bindings_are_checked_before_execution`: both passed.
- `cargo clippy -p kagari --test offline_functions --test numeric_operations --test source_snapshots --test package_inputs --test source_modules -- -D warnings`: passed.
  Structure checked 1248 Rust files with zero violations; formatting and diff checks passed.

These are selected SDK consumer boundaries, not a workspace sweep. Unchanged
pointer/token Miri evidence remains applicable; no new Miri run is claimed for
fixture-only authoring migration. Standard log/print, remaining live-root/path
consumers and HR03 producer/callable retirement remain required. Exhaustive
workspace, feature, backend and platform validation remains CI-owned.

### HR02/RF05 checkpoint: composite native consumers and pinned reentry

The offline composite fixture now uses shared Type/FnDecl authoring, one bind per
callback and NativeModule installation. Its Array/Map/Set/Option/Result tuple keeps
ordinary checked types, native argument/result validation and existing temporary
roots. The make callback resolves `payload` in the current execution root's pinned
bytecode, then uses the existing synchronous reentry service. It can therefore be
bound before source compilation without capturing a later compiler-owned function
ID. Its declared effects include allocation, host services and traps. Echo still
collects inside the callback and returns its rooted input; retained nested values
are checked after collection on direct, KBC/interpreter and host-Cranelift routes.

The other existing cases retain nested source type rejection, contextual empty
Array inference, completed/terminating operand behavior, exact callback counts and
invalid source diagnostics. Bindings and captured counters are installed once;
each execution route resets only its fixture observations. No cold callback or
metadata-only inspection executes application services. The obsolete KHI producer,
interface installer and late HostFunction registrations leave this entire file.

The initial compile attempt used the private catalog `definitions` helper.
The fixture now supplies the actual public retained ModuleDecl identity scope to
Type::from_semantic instead of widening catalog visibility. No production build
failure, unchecked shape conversion, pointer or Rust lifetime change remains.

Focused local validation passed:

- `cargo test -p kagari --test offline_composite --no-fail-fast`: all three existing
  affected cases passed (32.93 seconds).
- `cargo clippy -p kagari --test offline_composite -- -D warnings`: passed.
- Structure, formatting and diff checks passed, with 1248 Rust files and zero
  structure violations. Previous unchanged pointer/token Miri evidence applies;
  no new Miri result is claimed for this consumer-only migration.

Remaining legacy producers/consumers, HR03 removal and RF06/RI05 acceptance remain
required. Complete workspace/features/backends/platforms are still pending CI.

### HR02/RF05 checkpoint: print and the common log declaration

Print's fixed service contract moves to a cold ordinary ModuleDecl/FnDecl in
`kagari-types::declaration::logging`; application callbacks bind/install it through
the same ModuleBuilder/NativeModule owner as every other native function. The
builder can look up an already declared local function for binding, which makes
preauthored declaration consumption concrete without re-declaring signatures.

Compilation must still work without a runtime or logger callback. A print demand
will therefore carry its checked declaration-only native provider through the
ordinary MIR/bytecode module graph, with normal native targets, dependency closure,
link proofs and exact installed entry validation. This projection does not export
a script namespace or provide a callback. Missing installation must continue to
reject at load; no default logger, alternate call service or relaxed signature
validation is introduced. The former `standard_log` factory and its temporary
projection have now been removed after migrating every callback consumer. The
common module owns the canonical root host/log identity; unpublished old log
artifacts are disposable and join the RF06 fixture checkpoint. Other old host
registration producers and callable records still require HR03 retirement.
All logger consumers now bind this declaration through the ordinary native entry.
The CLI installs it before building its engine for source, KBC and AOT loading.
VM callbacks retain synchronous reentry, root termination, debug caller frames,
GC and exception cleanup. Source compilation without a callback still succeeds;
the existing Never fixture checks that unconfigured runtime loading rejects before
side effects. Declaration adoption preserves canonical IDs and the complete common
native contract. Compiler fixtures reject omitted, duplicate, mismatched or
conflicting provider requirements through the existing executable verifier.

Initial integration errors were repaired: an existing builder method required the
lookup name `declared_function`; direct log sources now explicitly import their
native module; debug fixtures identify the common native owner rather than the
retired host-import flag. A misplaced test import interrupted formatting until its
inner module docs were restored. The adoption fixture compares full adopted native
contracts instead of requiring an unrelated ModuleDecl equality implementation.
No lifetime, root, borrow or pointer validation was weakened.

Actual focused local validation:

- `cargo test -p kagari-types --test host_identity log_declaration_retains_scoped_contract_and_canonical_binding_after_adoption`: passed.
- `cargo test -p kagari-bytecode --test identity required_host_fingerprint_is_derived_and_independent_of_docs_and_order`: passed. This remaining legacy fingerprint fixture uses an unrelated output declaration, not a second log factory.
- `cargo test -p kagari --test never never_preserves_effects_and_releases_resources_on_traps_and_cancellation`: passed (9.68 seconds).
- `cargo test -p kagari --lib completed_effect_survives_trap`: passed (9.34 seconds).
- `cargo test -p kagari --test embedding_api installed_host_and_reflection_helpers_are_available`: passed (12.90 seconds).
- Selected VM unit cases `executes_source_lowered_declared_host_log`,
  `host_reentry_cannot_swallow_root_termination_and_releases_borrows`,
  `source_artifact_and_jit_fallback_resolve_imports_to_registered_slots` and
  `nested_breakpoints_and_traps_include_the_suspended_host_caller`: passed.
- `cargo test -p kagari-vm --test native_boundary native_boundary_control::host_reentry_returns_to_the_synchronous_native_caller`: passed (7.81 seconds).
- Selected compiler unit cases `host_imports_are_interned_and_checked_before_execution`
  and `conflicting_native_contracts_cannot_be_hidden_by_import_interning`: passed.
- `cargo test -p kagari-hir --lib checks_print_builtin_signature`: passed.
- `cargo run -p kagari-cli -- run --no-jit target/interop-tools/print-common.kgr`:
  passed, printing `common log`. `target/debug/kg emit -o target/interop-tools/print-common.kbc target/interop-tools/print-common.kgr`
  and `target/debug/kg run-artifact --no-jit target/interop-tools/print-common.kbc`
  also passed with the same output. Reproduce with `fn main() { print("common log"); }`;
  the ignored probe is not required to retain this evidence.
- Scoped Clippy passed for changed types/HIR/compiler/runtime libraries, VM library
  and `native_boundary`, SDK library/`never`/`embedding_api`, and CLI `kg`.
  Formatting, diff and structure checks passed (1250 Rust files, zero violations).

The selection covers the changed declaration, executable contract, shared entry,
reentry/cleanup and CLI source-free boundary. Pointer/token kernels are unchanged;
prior Miri evidence applies and no new Miri result is claimed. Complete workspace,
feature/backend/platform checks, including the AOT feature, remain CI-owned and
unrun locally. Remaining legacy consumers and HR03/RF06/RI05 integration are open.

### HR02/RF05 checkpoint: common trait declaration validation consumer

The remaining offline nominal trait schema fixture now authors the same private
source reference, external Counter and explicit get binding through ModuleBuilder.
Its cold invalid declarations use the ordinary scoped ModuleDecl catalog validator,
covering wrong result/parameter types, missing/extra members and wrong applied
arity. Generated source checking retains the distinct Eq and Hash bound rejection
for f32, and rejects a changed actual source result with
`KG_TYPE_TRAIT_METHOD_MISMATCH` before callback execution. The complete fixture
file no longer authors KHI, installs HostInterface or registers HostFunction.

Initial fixture compilation used nonexistent convenience/query methods; actual
scalar authoring and retained file IDs replaced them. Empty application arguments
may remain for inference, so the invalid arity assertion belongs to completed impl
validation. Expected diagnostics now follow the shared generic-bound and trait
method checker rather than the removed legacy table checker's wording/kind.
No production contract, validation or assertion was weakened.

Focused `cargo test -p kagari --test offline_nominal host_trait_table_is_checked_against_script_trait_signatures`
passed (7.20 seconds). Scoped Clippy for the same target, formatting and diff checks
passed. Earlier structure evidence covers these test-only import/fixture changes;
no new unsafe/root code or Miri run is involved. Other nominal/path consumers,
print/log and old production entrypoint retirement remain required; the complete
workspace and feature/backend/platform matrix remains pending CI.

### HR02/RF05 checkpoint: private generic source-trait integration

The existing host trait-bound execution fixture now declares Counter and explicitly
references the private source-owned Readable<T>, binding its i32 and bool impls
once through ModuleBuilder. The source trait remains private. Direct, KBC,
interpreter and host-Cranelift execution preserve result 42 and the trace
`make, flag, make, number`. Missing impl bounds and overlapping source impls remain
rejected. Ordinary scripts cannot import that private trait: only the exact
registered native importer receives access to its canonical checked reference.
No source package alias or ordinary source visibility is widened.

Migration exposed real ownership gaps. Concrete generic bodies can mention a host
type absent from their source imports; lowering now obtains its schema from its
exact checked defining module. Private foreign traits previously failed the
executable verifier's public-only lookup. MIR and bytecode now carry common
TraitContract references separately from definitions; both validate reference
shape and exact reachable defining ABI. The native installation retains requirements
per module owner, and loading compares the entire carried list with those installed
requirements, including unused entries. References cannot define a trait, supply
script defaults, introduce native adapter authority, or omit their requirements
from source-free products. The existing public source-trait case verifies omitted
requirements and changed source ABI rejection against the old installation.

Wire mapping and bounded decoding include the same scoped records. Memory/wire
resource checks count reference tables, nested contracts, associated constants and
parent arguments before fingerprinting. Unpublished identifiers are unchanged;
affected disposable artifacts require the already planned RF06 coherent fixture
checkpoint. Root-module external type names now consistently produce
`demo::Counter`, without the previous doubled separator.

Initial failures were resolved: generated private trait visibility, missing generic
host schema, public-only interface verification and installed public-only dependency
lookup. The public ABI-negative fixture now distinguishes rejection during emission
from rejection of a coherently rebuilt provider at old-runtime load. A resource
fixture initially used a nonexistent DefinitionKind and an obsolete host-path
decoder diagnostic; both were corrected while retaining pre-verification rejection.
The selected installation regression exposed an overly strict duplicate-owner
check; adding members in the same module remains allowed when its reference
requirements are identical, while conflicting requirements reject transactionally.

Actual focused local validation:

- `cargo test -p kagari --test offline_nominal source_trait_reference_uses_explicit_binding_and_portable_abi`: passed (25.03 seconds), including omitted requirements and changed installed ABI.
- `cargo test -p kagari --test offline_nominal host_trait_bound_calls_use_bound_methods_across_execution_routes`: passed (17.32 seconds), including private import visibility and all three existing execution routes.
- `cargo test -p kagari-bytecode --lib nested_layout_and_host_path_counts_are_bounded_on_all_artifact_routes`: passed after the documented fixture repairs (0.01 seconds), including nested members and the top-level reference table.
- `cargo test -p kagari-hir --lib body_edits_rebase_impl_trait_references_in_signature_cache`: passed (1.61 seconds).
- `cargo test -p kagari-runtime --lib shared_registrations_keep_installation_and_failure_state_runtime_local`: initially failed; passed after allowing identical requirements during module growth (0.01 seconds).
- `cargo clippy -p kagari-bytecode -p kagari-contract -p kagari-hir -p kagari-mir -p kagari-compiler -p kagari-runtime --lib -- -D warnings`: passed.
- `cargo clippy -p kagari --test offline_nominal -- -D warnings`: passed.
- Structure checked 1248 Rust files with zero violations; formatting and diff checks passed.

The selection covers changed portable contract/resource boundaries, trait cache
rebasing, runtime installation and the migrated SDK semantic owner. It is not a
workspace sweep. No unsafe pointer, Rust loan or root lifetime implementation
changed; prior kernel Miri evidence remains applicable and no new Miri run is
claimed. Standard print/log, remaining old consumers, HR03 retirement and RF06/RI05
integration remain required. Complete feature/backend/platform validation stays
CI-owned and unrun locally.

### HR02/RF05 checkpoint: common nominal type and method consumers

The offline nominal handle fixture now declares left::Item, its right::Item field
dependency and an unrelated unused::Other through ordinary ModuleBuilder schemas.
It binds make/take and the explicit add member once before engine construction;
each route creates only runtime-local roots and resets observed application state.
The full carried schemas are compared after scoped adoption, including the field
dependency and original ownership/reflection/access policies. Annotation-only
dependencies reject when omitted from the executable product or absent from the
runtime installation. The unrelated installed type remains outside that closure.

The member fixture retains exact access-policy mismatch rejection, missing-member
binding rejection, receiver-before-argument once-only evaluation and GC cleanup.
Its recursive conflict now uses a checked source entry and synchronous reentry
under the existing pinned execution root, instead of invoke_host's retired parallel
registration route. The exclusive lease rejects the nested add before its callback;
two outer calls still produce 42 and 44 on the existing three routes.

The first migrated navigation case failed because host_type_at only queries the
legacy HostDeclarations table. Common providers already own ordinary generated
declaration locations and checked external schemas. The case now uses the existing
definition_at plus Declarations::external_type APIs, preserving terminal-position,
UTF-8/CRLF and signature/full-analysis agreement. No forwarding query or metadata
projection was added. This is the intended unified tooling model; the old query's
remaining legacy callers still require HR03 migration and removal.

Actual `cargo test -p kagari --test offline_nominal handles::` passed all four
changed cases (14.43 seconds). After strengthening full schema/return-identity
comparisons, the two affected dependency cases passed again (14.52 seconds).
The initial wrong-query failure is resolved. Selected Clippy, structure, formatting
and diff checks pass; structure checks 1251 Rust files with zero violations.
No unsafe pointer, loan or root kernel changed, and no new Miri run is claimed.
The remaining nominal path consumers and other legacy producer/callable retirement
remain required. Complete workspace/features/backends/platforms stay pending CI.

### HR02/RF05 checkpoint: ordinary collection host consumers

The observable language-contract fixture now carries observe::vector as a cold
common ModuleDecl with the exact stdlib Vec<i32> identity. Analysis imports that
declaration; runtime binding adopts the same records and returns the explicitly
rooted existing vector. It neither snapshots the vector nor changes mutation,
trap, cancellation or root cleanup expectations. The fixture no longer owns any
HostInterface or HostFunction registration.

The collection access fixture also uses ordinary common function declarations and
one preinstalled callback per member. Its source-free result-family negative loads
an old Array-returning artifact against a separately installed Vec-returning
contract, retaining rejection before execution. Map/Set read-only result views
continue to share storage, preserve content/Eq/Hash protocols and observe mutation
through the original alias. Sources explicitly import demo, so ordinary namespace
resolution cannot turn the access-negative cases into unrelated missing-name errors.
No collection library representation or implementation was migrated.

Focused local validation passed:

- `cargo test -p kagari --lib heap_compound_reads_rhs_write`: passed (9.73 seconds).
- `cargo test -p kagari --lib heap_cancellation_preserves_earlier_write`: passed (9.75 seconds).
- `cargo test -p kagari --test collection_access host_`: both changed cases passed
  (11.25 seconds), with existing source-free result-family and read-only protocol
  assertions. An obsolete metadata import warning was removed afterward.
- `cargo clippy -p kagari --lib --test collection_access -- -D warnings`: passed.
  Formatting and diff checks passed; structure checked 1251 Rust files with zero
  violations.

These selected consumers establish the changed common registration, shared object
and cleanup boundaries; the full language-contract suite and complete workspace,
feature/backend/platform matrix remain CI-owned. No unsafe/kernel lifetime code
changed and no new Miri run is claimed. Other legacy consumers and HR03 retirement
remain required; combined acceptance remains open.

### HR02/RF05 checkpoint: common manual path consumers and imported fields

All remaining offline nominal path/index fixtures now declare types, functions and
manual paths through ModuleBuilder, bind callbacks/adapters once and install the
ordinary modules before creating execution runtimes. The whole offline_nominal
target no longer authors HostInterface/KHI or registers legacy host functions/types
and paths. It retains the same explicit roots and existing checked path service;
neither field DTO copies nor Rust object references replace live host state.

The field-chain case compares complete carried schemas and the exact path after
adoption, rejects a separately installed schema/function module that omits the
path, and evaluates the root once. Both index cases retain index-before-RHS order
and one prepared read/write, including skipping an intermediate array field read.
The existing write case retains all ten assignment/rebinding scenarios across its
three execution routes: post-RHS reads, overflow/division traps, deleted targets,
original-root capture, prepare-before-commit, dirty values and completed RHS effects.

The first compile attempt accessed a private root field in a fixture; the checked
root is now compared in full against the runtime's existing root record. Execution
then exposed a real catalog omission: importing TypeDef records collected implicit
variants but not field identities. collect_implicit_definitions now interns each
declared field under its exact type owner, matching ModuleDecl's existing identity
collector. This grants no access or ABI authority and adds no second resolver.
An intermediate fixture attempted finish before binding a declared path and was
correctly rejected. The negative installation now omits the path declaration;
finished production modules still require every declared path binding.

Actual focused local validation:

- `cargo test -p kagari --test offline_nominal source_field_chains_use_offline_contracts_and_evaluate_the_root_once`: passed (20.81 seconds) after the recorded repairs.
- `cargo test -p kagari --test offline_nominal paths::source_host_`: all three
  selected index/write cases passed (111.63 seconds), including all existing
  assignment/rebinding scenarios. The unchanged multi-index case was not rerun.

- `cargo test -p kagari-runtime --test native_registration scoped_type_inputs_preserve_snapshots_and_compare_complete_identity`: passed.
- Scoped Clippy passed for the runtime library and SDK offline_nominal target.

Structure, formatting and diff checks pass (1251 Rust files, zero structure
violations). No pointer, Rust loan or root kernel changes; no new Miri run is
claimed. The complete workspace/features/backends/platform matrix remains CI-owned.
Other legacy consumers and HR03/RF06/RI05 integration remain open.

### HR02/RF05 checkpoint: anonymous source traits and runnable interface example

The host_interfaces example now explicitly references the private Reader owned by
its standalone source module, registers Counter's impl/associated Item and binds
read exactly once through the common ModuleBuilder. make uses the same installation
and resolves its runtime-local root. Static generic calls, associated projection
and boxed interface dispatch execute after ordinary KBC encoding/decoding and
return I32(42). No HostInterface bytes, HostFunction registration, duplicate inherent
member or runtime source analysis remains in the example. Cold source contract
checking runs before any runtime or root exists; it executes no callback.

The initial example run rejected generated syntax: standalone canonical package
families contain `source-file:` and cannot appear literally in source paths. The
native path renderer now gives these families a valid generated qualifier and
avoids a trailing separator for root modules. Namespace lookup resolves this
spelling only against that native importer's explicit canonical dependencies,
without adding a global source/package alias. All matching dependencies participate
in lookup, so a spelling collision remains ambiguous rather than selecting one.
Canonical IDs, private-trait admission and exact ABI requirements are unchanged.

The new focused anonymous-source fixture checks successful explicit native impl
checking and rejects ordinary import of a public sibling through that generated
qualifier. This distinguishes absence of a global alias from a private-member
visibility rejection. Existing named-source package alias isolation is reused as
the small directly affected regression boundary.

Actual focused local validation:

- `cargo run -p kagari --example host_interfaces`: passed after fixing the recorded
  syntax failure; printed `static + dynamic host interface result: I32(42)`.
- `cargo test -p kagari --test offline_nominal anonymous_source_trait_qualifiers_remain_scoped_to_native_importers`: passed (6.12 seconds).
- `cargo test -p kagari-hir --lib declared_source_aliases_cannot_shadow_installed_native_packages`: passed (0.14 seconds).
- HIR library and scoped SDK example/target Clippy passed. Structure, formatting
  and diff checks passed (1251
  Rust files, zero structure violations).

The embedding guide now describes the common registration entry, and the owning
interop spec records scoped anonymous qualifiers and the runnable example. No
unsafe/root/loan kernel changed; no new Miri run is claimed. Remaining legacy
interface and low-level consumer migration, HR03 retirement and RF06/RI05 final
integration remain required. Complete workspace/features/backends/platforms stay
CI-owned and unrun locally.

### HR02/RF05 checkpoint: common dynamic interface consumers and receiver bounds

The SDK host_interfaces target now uses one cold ModuleBuilder fixture for Counter,
its explicit private/public source Reader impl, associated output and make binding.
All eight existing interface cases have migrated; no HostInterface producer,
HostFunction registration or late host-type registration remains in this target.
The ordinary directory test entry separates shared registration from behavior.
Callbacks bind once before engine construction; roots remain runtime-local and
cold compilation asserts that no callback ran. The direct/KBC/interpreter/host
Cranelift cases retain exact call traces and now also check released result roots.

Child-to-parent upcasting, imported generic inputs, associated output mismatches,
nested foreign-root rejection, shared receiver conflicts, trap cleanup, collection,
foreign owner/scoped-ref rejection and reload pinning retain their original
boundaries. Interface tables now belong to the native provider. The initial
root-module table-count assertion failed (zero versus one); it now inspects the
actual provider. Interface construction has a successful owner-matched control
before the foreign/scoped negative cases. Synchronous interface reentry initially
looked for answer in the provider entry module; program_root selects the original
source root within that same pinned graph. Old/new interface calls still return
42/43 after reload. No latest-generation lookup or root/borrow relaxation is used.

The six portable tamper cases use common impl/dispatch records: missing associated
outputs, an unsupported Eq/Hash output, a method/output disagreement, an unauthorized
legacy bridge flag, a missing method slot and a redirected native target. Obsolete
host-script shim body mutations are replaced by direct dispatch-slot validation;
the common native path has no such script shim. Both graph verification and
artifact construction must reject every mutation.

The migrated cold output-bound case exposed a shared HIR defect: validating
Reader::Item: Describe<Self> in an impl retained the trait's abstract Self instead
of substituting the concrete Counter receiver. ApplicationContext now carries the
root impl receiver into the existing generic/associated constraint substitution.
Nested type applications deliberately start with no receiver binding. The first
repair changed projection ownership without substituting its Self, so the Vec
foundation rejected Iter<T>; binding Self before resolving sibling projections
restores the existing Iterable contract. A mechanical traversal edit also initially
failed compilation with an iterator-reference error; explicit borrowed iterators
resolved it. No checks are skipped. The pure source receiver test checks both
Related<Reader> acceptance and Related<Other> rejection. The portable native case
still rejects removal of Other's Describe<Counter> evidence without calling Rust.
The trait specification records the substitution contract beside associated outputs.

Actual focused local validation:

- SDK host_interfaces selected direct/KBC/Cranelift dispatch passed (16.05 seconds),
  child upcasting passed (11.51 seconds), six tamper cases passed (10.58 seconds),
  GC/reentry/reload/trap cleanup passed (15.18 seconds), and the cold output-bound
  case passed after repair (10.37 seconds). Imported generic dispatch, registered
  output mismatch and nested-root validation passed together in the four-case run
  that first exposed the output-bound failure (32.63 seconds); only the failing
  case was rerun after that repair.
- The HIR `associated_` filter selected 14 related tests: 12 passed, including the
  new concrete receiver proof, erased-Self rejection, generic families, projection
  ownership and cached query cases. Two pre-existing fixture failures were
  reproduced independently with the prior application checker (the first baseline
  attempt failed to initialize the new context field and ran no tests; the corrected
  baseline ran both). Imported Stream used an unbound anonymous consumer for a named
  package; binding the consumer to pkg preserves the original imported-supertrait
  assertions. The completion fixture explicitly registered interface impls while
  expecting absence of methods. It now tests both absence of an impl and explicit
  native interface impls, retaining the original negative assertions and matching
  associated-output requirements. Their focused reruns passed (0.64/7.87 seconds).
- Scoped HIR library and SDK host_interfaces Clippy passed. SDK Clippy initially
  rejected a post-Default fixture field assignment; direct configuration initialization
  resolved it. Structure passed for 1252 Rust files with zero violations. Formatting
  and diff checks passed; no workspace or feature/backend/platform sweep ran.

No unsafe pointer or loan/root kernel changed; a new Miri run is not warranted for
this metadata/checker/consumer checkpoint. Remaining low-level SDK/standalone example
consumers, dependent legacy producer/callable retirement and HR03/RF06/RI05 final
integration remain open. Complete workspace/feature/backend/platform acceptance is
CI-owned and not claimed from these focused results.

### HR02/RF05 checkpoint: cold shared declarations and field tooling

The offline_compile example now authors ordinary ModuleDecl schemas, declarations
and manual path requirements, with no HostInterface producer/bytes, HostFunction,
NativeModule, runtime or callback. AnalysisDatabase consumes these cold records
beside the standard declaration catalog. The existing facade, array echo, field
read/write/index/virtual paths and receiver method scenarios retain their source
coverage. The emitted KBC includes portable MIR, full function-section counts,
exact header/root identity, two external schemas and four used path requirements;
bounded decoding and loader validation run without installed Rust implementations.
All native imports use common declarations. Used path requirements belong to the
calling source module, not the provider; the initial provider-count assertion
failed and now checks their actual executable owner.

Qualified shared external type references initially selected their qualifier and
separator as well as their terminal name. The existing signature/full query
position selector now recognizes checked external TypeId::Host facts, preserving
canonical identity and alias semantics. The SDK signature navigation case now
covers both unqualified and aliased qualified names with CRLF and multibyte text.
Its first compile failed after moving the generated String before computing test
positions; computing offsets before transfer resolved the error.

External fields have canonical checked identities and schemas but need not have
source declaration locations. The example's initial definition_at field query
therefore returned None. FileAnalysis::field_at now exposes the existing checked
read/place field identity using the same captured terminal spans, without creating
synthetic source fields or resolving types again. The legacy typed host_field_at
query delegates position selection to it and remains on HR03's explicit retirement
list. The example resolves field IDs in its retained table and navigates methods
through their genuine generated declaration origins. Initial missing/wrong-module
DefinitionId imports were corrected to the actual table owner before checks passed.

Actual focused local validation:

- cargo run -p kagari --example offline_compile passed, reporting fingerprint
  8da78a0db89642a1, 196 common native applications, nine executable function records
  and a 1,084,351-byte artifact. It checks cold navigation plus codec/loader contracts.
- SDK offline_nominal selected signature-navigation case passed (2.09 seconds).
- HIR library field_ filter passed all 15 selected source/external field cases
  (3.82 seconds), including rejected writes, body reuse, mixed chains and remapping.
- HIR library and SDK example/offline_nominal Clippy passed after removing a
  redundant String conversion. Structure passed for 1252 Rust files with zero
  violations after importing the standard catalog from its module owner. Formatting
  and diff checks passed.

No unsafe, root or loan kernel changed, so this checkpoint needs no new Miri run.
The remaining SDK low-level path fixture, runtime/other compiler consumers and
legacy producer/callable retirement still block HR02/HR03/RF05 final integration.
The complete workspace, features/backends/platforms remain CI-owned and unrun locally.

### HR02/RF05 checkpoint: SDK low-level path registration

The embedding_api fixture now declares Player, its hp schema, player callback and
read/write path adapters through one ModuleBuilder, installed before runtime root
creation. No HostFunction, HostTypeRegistration, standalone HostInterface or late
type/function registration remains in SDK tests/examples. The low-level hand-built
bytecode boundary remains distinct from source compiler tests: it carries common
native declarations, explicit provider dependencies, exact external schemas and
semantic HostHandle register facts. Every PathRecord has a shared declaration and
is fingerprint-checked before artifact construction. Runtime physical slots are no
longer hand-registered primitive metadata.

Before editing, the selected reload fixture failed with InvalidPathLayout during
artifact construction: it had an unbacked PathRecord and fabricated a fingerprint
by XOR, so it could never reach its reload assertion. The replacement installs
both legitimate shared/read-write contracts for hp once. The unused-path candidate
selects the other valid declared access contract, retaining unchanged public type
ABI. Both artifacts verify; reload specifically reports
KG_RELOAD_PATH_FINGERPRINT_MISMATCH, with unchanged module count and active epoch.
No verifier or reload check is weakened. Read-only adapters have no write handler.

Actual focused local validation: the two installed_host_path cases passed together
(1.36 seconds), and the repaired reload contract case passed (1.18 seconds). Scoped
embedding_api Clippy, structure (1252 files, zero findings), formatting and diff
checks passed. Only fixture authoring changed; no unsafe kernel or new Miri scope.
Remaining runtime/compiler consumers and HR03 legacy format/callable retirement
still block combined completion; the full workspace/backend/feature/platform
matrix stays CI-owned and unrun locally.

### HR02/RF05 checkpoint: runtime sessions and common offline execution

installation_access and execution_sessions now have no legacy registration or
HostInterface producer. Their affected callbacks declare and bind through ordinary
ModuleBuilder before loading. A narrow integration fixture builds source-free
free-function contracts from shared declarations and invokes the existing native
frame driver, retaining roots, session ownership and cleanup. It has no resolver,
conversion, borrow semantics or callback implementation of its own. It deliberately
rejects layouts, generic proofs, path and trait-impl inputs outside this fixture's
finite scope. Ordinary modules share it through tests/support, without handwritten
include or cross-target production exports.

The candidate result test previously registered callbacks after loading its empty
baseline. It now installs them first, loads their exact declarations, then populates
a retained rooted old array. Fresh allocations use the active frame's pinned
execution root. Calls enter the actual candidate member; nested old arrays still
fail before publication and fresh nested arrays survive collection. The candidate
effect test retains pure/configuration acceptance, service/mutation/suspension
rejection before callbacks, path-effect precedence, exact callback counts and
resource cleanup. Installed-call rejection before installation and sticky callback
cancellation are tested through the same linked native frames.

The runtime offline_host example now authors cold ModuleDecl and KBC contracts
without a callback/runtime, then adopts that declaration into ModuleBuilder and
binds once. It decodes/validates the KBC, invokes rooted native frames, checks trace
inputs/outcome, stages a reload, initializes its candidate and publishes. Its first
compile exposed incorrect assumptions about load_artifact and CandidateSession
accessors; it now uses existing artifact loader validation/load_program and the
actual staged candidate module. No new public execution entrypoint was introduced.
The run also found a genuine native host-trace defect: root ModuleIdentity display
already ended in ::, causing demo::::limit. begin_trace now formats package and
module path segments directly before the member identity. Nested module components
and occurrence suffixes retain their canonical spellings. No identity check changed.

Actual focused local validation:

- Selected installation_access installed-call and sticky-cancellation cases passed
  separately (both under 0.01 seconds after compilation). The first compile used a
  nonexistent HostCallPolicy::default; explicit policy fields fixed it before tests.
- Selected execution_sessions candidate effects and nested old/fresh results passed
  separately (both under 0.01 seconds); unchanged session cases were not rerun.
- cargo run -p kagari-runtime --example offline_host passed after the trace fix,
  reporting a 1,255-byte common contract and immutable configuration result 42.
  It also asserts zero retained roots after candidate publication.
- Runtime library, this example and both affected integration targets passed scoped
  Clippy. Structure passed for 1254 files with zero violations; formatting and diff
  checks passed. The initial target waited on an unrelated build-directory lock;
  no other process or profile/build settings were changed.

No unsafe or pointer/loan kernel changed, so no new Miri run is required here.
Runtime offline_host/host_registration/host_nominal and remaining source/tooling
legacy consumers, the trait-table example and HR03 retirement remain outstanding.
Complete workspace, backend/feature/platform and exhaustive safety acceptance remain
CI-owned and are not claimed from these focused checks.

### HR02/RF05 checkpoint: runtime offline contract consumers

All eight offline_host cases now author ordinary ModuleDecl/FnDecl records and bind
through ModuleBuilder. Their portable owner is KBC, with native declarations and
common imports; no standalone HostInterface bytes, host function slot, compatibility
alias or legacy registration remains in this target. The existing finite source-free
fixture supplies ordinary frame invocation and table-scoped metadata.

The migrated boundaries retain missing-binding rejection before module publication,
unchanged reload epoch/resources, shared loaded code, cross-runtime owner rejection
with a matching local positive control, argument/result validation before/after
callbacks, exact callback counts and cleanup. Cold codec adoption compares native
contracts/imports after importing both tables into one context; foreign and stale
prefixes still reject. Identity, input/output type, effects and a valid String shared
versus owned passing mismatch reject linking without Rust calls. Documentation changes
leave executable bytes unchanged. Failed invalid/duplicate installation leaves the
original callback callable with its original result.

KBC now owns the portable canonicality test: equal contracts encoded from differently
populated process tables are byte-identical, malformed magic/version/trailing/truncated
input rejects, and duplicate native declarations reject artifact construction. Old
KHI-only 4 MiB/version/fingerprint-domain assertions retire with that format rather
than imposing a second envelope on common declarations. The existing KBC artifact
owner retains 64 MiB envelope, nested count/depth and encoded-size coverage in
artifact/canonical_tests.rs (including portable_mir_bounds_apply_before_decoding_and_to_combined_envelope_size,
nested_layout_and_host_path_counts_are_bounded_on_all_artifact_routes and
header_metadata_is_covered_and_old_format_cannot_be_opted_into). These unchanged
codec tests remain in their CI scope; this fixture does not duplicate them.
Immutable configuration retains recursive rejection for arrays, tuple-wrapped mutable
sets, Option<Host>, and shared inputs, while accepting Option<(String, i32)>.

Actual focused validation: all eight runtime offline_host cases passed (0.01 seconds),
then the encoding case passed separately after adding explicit unsupported-version
input. The first compile omitted NominalTy's associated-output map, then used the
wrong empty container type; initializing its actual map resolved both mechanical
errors before tests ran. Scoped offline_host Clippy and structure (1254 files, zero
findings) passed. Formatting check found one new import layout and passed after rustfmt;
diff checks passed. No runtime/unsafe/loan kernel changed and no new Miri run is needed.
Remaining nominal/type/path and trait-table consumers plus legacy production owners
still block HR02/HR03/RF05 completion. Full workspace/backend/features/platforms remain
CI-owned and unrun locally.

### HR02/RF05 checkpoint: runtime roots, path values and explicit trait example

The shared_host_callbacks root-lease test now binds its captured RootedValue through
ModuleBuilder, carries the exact source-free native contract, and invokes the
ordinary frame driver. It retains the original array across collection and verifies
that the callback's one owning root remains after frame cleanup. The substrate path
fixture now declares Player/hp and its path through the same module builder and
bind_path, installs once, then creates a runtime-local root. It uses resolved physical
types instead of the old assumed result slot. Existing category, non-storable,
GC-payload rejection and trace-root assertions remain at their original owner.

host_trait_table is now a runnable common-registration example. It registers
Opaque<Counter>, explicitly declares Kagari Readable and its impl, marks get's shared
receiver access, and binds a LoanContext callback calling RustReadable::read. It
compiles source against cold declarations, encodes/decodes and validates KBC, installs
the ordinary native modules, checks the exact Kagari trait table and invokes its
native application with a runtime-owned Opaque root. It returns 42 and releases all
object roots. Rust trait implementation alone adds no Kagari declaration or member.
No runtime example now uses legacy HostInterface/HostFunction/type registration.

The example initially applied ? to compiler diagnostic enums lacking Error; explicit
checked-stage expectations fixed compilation without changing those APIs. A borrowed
iterator over temporary LoadedModule handles also failed compilation; table counting
now completes within each retained member's scope. The first run correctly rejected
an unregistered Rust representation when only native storage was declared. Using the
existing register_type::<Opaque<Counter>> schema entry supplies the local Rust mapping
before the trait callback is bound, while explicit trait/impl/member declarations
remain mandatory. No new registration shortcut or weakened proof was introduced.

Actual focused local validation: selected GC captured-root case passed (0.01 seconds),
substrate categories and host_objects_are_not_gc_payloads_or_trace_targets passed
separately (under 0.01 seconds each), and cargo run -p kagari-runtime --example
host_trait_table passed with its explicit impl/Rust callback result 42. Scoped Clippy
for the example and these two integration targets passed. Structure passed for 1254
files with zero violations after importing the standard module constructor directly;
formatting and diff checks passed. No unsafe/loan/projector kernel changed, so no new
Miri run is warranted. Remaining runtime registration/nominal/type/path fixtures,
compiler/tooling legacy consumers and HR03 producer/format/callable retirement remain
open; exhaustive workspace/backend/features/platform checks stay CI-owned.

### HR02/RF05 checkpoint: runtime registration contract consumers

All six host_registration cases now use common ModuleBuilder declarations/bindings
and installation. The source-free fixture was extended only for their concrete
external schemas: each retained TypeDeclaration and public type shares the original
canonical owner; generic proofs, GC layouts, paths and trait impls remain excluded.
No legacy type/function registration or assumed physical primitive slot remains.

Borrowed callback results still reject with HostBorrowEscape and release the unique
lease; a following scope can reacquire it with no leaked roots. Function metadata
retains exact unique/owned passing, effects, canonical identity and common imports,
then invokes the original receiver/argument result. Duplicate functions/types reject
before replacing a published callback or metadata slot. The metadata case compares
schema/field/method fingerprints across independent runtime installations, validates
all scalar/self parameter slots, then creates a root of that installed type. The
obsolete arbitrary rust_type_name label is replaced by the actual canonical shared
schema name. Reflection methods now expose their explicit self parameter followed
by hp, matching their ordinary FnDecl rather than a second member signature.

Actual focused validation: the six-case host_registration run passed five tests
and failed only a fixture assertion expecting define_type to reject immediately.
A TypeBuilder is provisional; completing a duplicate external declaration is the
publication boundary. That focused corrected case passed (under 0.01 seconds).
Scoped host_registration Clippy, structure (1254 files, zero findings), formatting
and diff checks passed. No production/unsafe/root/loan implementation changed and
no new Miri run was needed. Remaining runtime legacy consumers are host_scopes,
offline_types, host_nominal and typed_path_views; compiler/tooling producers and
HR03 format/callable retirement remain open. Full workspace/backend/features/platform
acceptance remains CI-owned and unrun locally.

### HR02/RF05 checkpoint: common frames preserve scoped callbacks

The four callback-bearing host_scopes cases now declare/bind/install once through
ModuleBuilder and use ordinary rooted native frames. They retain nested shared/unique
conflicts, callback failure cleanup, temporary values across nested collection,
deterministic execution inputs, and outer/inner/failing trace invocation order. Their
physical external type comes from installed metadata; allocation and reentry use the
actual executing module. The separate owner/epoch, cancellation, unwind, quarantine
and actual-pointer loan fixtures remain at their existing owners.

Migration exposed a production diagnostic defect: shared argument narrowing first
used boolean type matching, which swallowed ExpiredHostBorrow and returned generic
ModuleValidation before the host adapter could validate the token. The existing
capability owner now validates/narrows first, then the exact declaration check runs
before publishing the inert alias into rooted arguments. Expired/foreign capabilities
retain their specific diagnostic; no identity, lifetime, access or root check was
removed, and no callback executes on rejection.

Actual focused validation: the borrow/reentry case initially failed only its expired
token assertion (all prior conflict and release assertions passed). It passed after
the narrowing fix. The temporary-collection, replayable-input and trace-reentry cases
each passed separately (all four under 0.01 seconds). Scoped runtime library and
host_scopes Clippy passed; structure checked 1254 files with zero findings. Formatting
required one assertion wrap and passed after rustfmt; diff checks passed. No unsafe
pointer/loan or root-lifetime implementation changed, so no new Miri run is warranted.
Remaining runtime consumers are offline_types, host_nominal and typed_path_views;
compiler/tooling producers and HR03 retirement remain open. Full workspace, backend,
feature/platform and exhaustive safety acceptance remain CI-owned and unrun locally.

### HR02/RF05 checkpoint: offline nominal and trait contracts

All four runtime offline_types cases now use common declarations, explicit binding
and decoded/loader-validated KBC. Cold Player/Team schemas retain mutual nominal
references, array element provenance, writable score metadata, an explicit self
parameter and an inherent Result-returning method. Runtime metadata uses resolved
canonical types and shared field fingerprints. No HostInterface/KHI or legacy type,
method or trait registration remains in this target.

Cold method binding exposed a concrete API gap: from_declaration could select only
free functions. ModuleBuilder::declared_method now selects a local impl by its
ModuleDecl position and member name, producing the existing FunctionRef token for
the same bind preparation, policy/signature checks and installation. Unknown impls
or members reject; no second binding/invocation mechanism was added. The standard
declaration specification documents these selection semantics.

Explicit trait contracts reject an unbound module before installation, reject a
source-free trait caller when only type/trait declarations are installed, and reject
a differently bound receiver passing policy. A matching installed impl loads the
same decoded program. Invalid missing/duplicate schema members and repeated module
installation retain transactional rejection. Four independently valid alternate
contracts (field type, field access, receiver passing, effects) reject before module
publication against the original installation. Type/field/method/module documentation
changes produce byte-identical executable KBC and load successfully.

Actual focused validation: all four offline_types cases passed in 39.06 seconds;
after replacing the invalid-field case with a duplicate-field declaration, that
selected case passed separately (0.62 seconds). Scoped runtime library/offline_types
Clippy and structure (1254 files, zero findings) passed. Formatting required import
and function wrapping and passed after rustfmt; diff checks passed. No unsafe,
pointer/loan or root kernel changed, so no new Miri run was required. Runtime
host_nominal and typed_path_views, compiler/tooling consumers and HR03 retirement
remain open. Complete workspace/backend/feature/platform acceptance remains CI-owned.

- [x] HR01: migration map, by-value contract and RI dependencies frozen.
- [ ] HR02: shared registration and selected by-value/legacy consumers migrated.
- [ ] HR03: dependent legacy retirement complete after RF05.
- [x] RF01: compiling scope/adapter proof and finite capability contract frozen.
- [x] RF02: checked reference/receiver/escape and portable contracts implemented.
- [x] RF03: safe external scopes and whole-object loans implemented.
- [x] RF04: generated inline projections and atomic path loans implemented.
- [ ] RF05: common bind, SDK and source-free reference paths integrated.
- [ ] RF06: focused safety/integration complete; CI/Miri status recorded separately.

RI progress remains in the interop plan. The combined order above is the sole
cross-plan schedule; these lists are not duplicate RI checklists. Current state:
API/lifetime prototype and RI01-RI02 schema/conversion implemented, no carried production build failure. The
proof and focused results above establish the owned conversion, Opaque, scoped
loan/projector and checked portable external-dependency slices. Whole-root common
registration/installation and explicit native field/index/virtual paths execute;
path-view argument capabilities and explicit source trait references are integrated.
Remaining legacy consumers, producer/callable retirement and complete combined
acceptance remain outstanding. Implementation checkpoints use Conventional Commits with the owning
phase trailer, for example `Plan-Phase: RF04`.
