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
| By ref | `Ref<T>` is a scoped path handle; a native invocation temporarily receives `&T`. | RF01-RF06; unstarted. |
| By ref mut | `RefMut<T>` is a scoped writable path handle; a native invocation temporarily receives `&mut T`. | RF01-RF06, including simultaneous disjoint field access; unstarted. |

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
gate. The current NativePayload bound on external lending is an intermediate
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


- [x] HR01: migration map, by-value contract and RI dependencies frozen.
- [ ] HR02: shared registration and selected by-value/legacy consumers migrated.
- [ ] HR03: dependent legacy retirement complete after RF05.
- [x] RF01: compiling scope/adapter proof and finite capability contract frozen.
- [ ] RF02: checked reference/receiver/escape and portable contracts implemented.
- [ ] RF03: safe external scopes and whole-object loans implemented.
- [ ] RF04: generated inline projections and atomic path loans implemented.
- [ ] RF05: common bind, SDK and source-free reference paths integrated.
- [ ] RF06: focused safety/integration complete; CI/Miri status recorded separately.

RI progress remains in the interop plan. The combined order above is the sole
cross-plan schedule; these lists are not duplicate RI checklists. Current state:
API/lifetime prototype and RI01-RI02 schema/conversion implemented, no carried production build failure. The
proof and focused results above do not establish production scope/projector or
complete combined acceptance. Implementation checkpoints use Conventional Commits with the owning
phase trailer, for example `Plan-Phase: RF04`.
