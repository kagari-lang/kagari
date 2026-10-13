# Host interface integration into NativeModule

Status: scope agreed; implementation is unstarted. Planning baseline: `11be3667`.
The [roadmap](implementation-roadmap.md#review-follow-up-order) owns sequencing;
[SA21](review.md#sa21-unify-rust-registration-through-nativemodule-with-scoped-host-access)
owns the finding and deferred borrowing follow-up. This plan owns HR01-HR03,
registration API proposals, migration acceptance and the progress ledger. Start
with the [complete Player example](#complete-example-expose-a-rust-player) for
the proposed usage.
API sketches are proposals until implementation updates their specification owner.

## Outcome and scope

Merge the existing HostInterface declaration and registration path into
`NativeModule`. Standard-library, application native and existing host APIs use
one declaration/binding/install model. Preserve existing observable host behavior.

Included:

- Existing host types, functions, methods, associated types, trait implementations,
  declared fields and typed paths represented under shared module declarations.
- Checked Rust bindings, transactional module installation, source/tooling imports,
  executable contracts and source-free linking through that shared authority.
- Migration of consumers and removal of obsolete HostInterface registration APIs,
  duplicate records and the separate portable interface format.

Deferred: typed Rust `&T`/`&mut T` input adapters, new script `Ref<T>`/`RefMut<T>`
types, new scoped-entry conveniences and new field projection/replacement semantics.
Their design must address simultaneous mutable access to disjoint fields of one
host object before activation; whole-root exclusion is not an accepted final
solution for that new API. This task preserves existing scoped-borrow and typed-path
checks and does not weaken them to approximate the deferred feature.

Persistent borrowed returns, retained Opaque storage implementation, native
collection migration, SDK/package renaming, live module installation and R1 remain
separate work. No new purity system, registration permission matrix or empty
extension framework is required.

Reuse [unified library registration](architecture.md#unified-library-registration),
[host interop](spec/host-interop.md), [typed paths](spec/typed-path-mutation.md) and
[runtime ownership](runtime-ownership-and-host-api-design.md). Current specifications
own execution semantics throughout this migration.

## Current architecture and target ownership

Ordinary native modules already combine ModuleDecl records with checked Rust
bindings and Engine installation. Host APIs separately publish HostInterface and
register host types/functions/paths into runtime services. The duplicated authority
is the defect; external objects, scoped leases and path commits still need their
own focused runtime owners.

| Current owner | Migration responsibility |
| --- | --- |
| [ModuleBuilder](../crates/kagari-runtime/src/native/builder.rs), [NativeModule](../crates/kagari-runtime/src/native/module.rs) | Carry and validate the existing host declaration/binding capabilities under the shared module model. |
| [HostInterface](../crates/kagari-types/src/host_interface/mod.rs) | Move required semantic facts to shared declarations and retire the parallel aggregate/format. |
| [Engine builder](../crates/kagari-embed/src/engine/builder.rs) | Install one module catalog transactionally; expose dependencies through its declaration catalog. |
| [Host registry](../crates/kagari-runtime/src/host/registry.rs) | Keep runtime-local root identities and schema/liveness checks; remove duplicated signature and registration authority. |
| [Host borrows](../crates/kagari-runtime/src/host/borrows.rs), [resource scopes](../crates/kagari-runtime/src/host_scope.rs) | Preserve existing alias checking, roots, scope ownership and cleanup. |
| [Typed callbacks](../crates/kagari-runtime/src/native/typed.rs), [argument conversion](../crates/kagari-runtime/src/native/conversion/arguments.rs) | Keep current owned conversions and integrate existing host callbacks through the common checked invocation boundary. New reference decoding is deferred. |
| [NativeStorage](../crates/kagari-runtime/src/native/storage.rs) | Keep GC-owned payloads separate from externally owned Rust state. |

Portable semantic declarations belong in `kagari-types`; selected executable
contracts belong in `kagari-contract`. Rust bindings, runtime-local identities,
leases and storage guards belong in `kagari-runtime`. HIR imports the shared
metadata; executable lowering and backends consume checked facts. The SDK owns
installation orchestration. Generated source remains a tooling view, not a second
signature authority or a dependency of source-free execution.

Unification does not require a single giant registry. Keep internal host services
where they own distinct lifecycle or access contracts. Existing host-context APIs
may survive where necessary; retain no facade that merely forwards obsolete
registration calls.

## Registration API proposal

### Module authoring

Extend `ModuleBuilder` and `NativeModule` rather than introducing a HostModule.
Use ordinary function/member declaration identities. Host representation and typed
path facts extend their appropriate shared owners instead of embedding an intact
HostInterface as a second authority.

The following is API-shape pseudocode. Exact signatures and local binding factory
requirements are an HR01 gate:

```text
ModuleBuilder::new(module_identity, providers)
ModuleBuilder::define_type(shared_host_type_declaration) -> TypeRef
ModuleBuilder::define_function(shared_function_declaration) -> FunctionRef
ModuleBuilder::define_path(shared_path_declaration) -> PathRef

ModuleBuilder::bind_host_type(type_ref, rust_host_binding)
ModuleBuilder::bind(function_ref, native_binding)
ModuleBuilder::bind_host_path(path_ref, existing_path_adapter)
ModuleBuilder::finish() -> NativeModule
```

All function bindings use `bind`, including ordinary typed functions and existing
host callbacks. Proposed `NativeBinding::typed(callback)` and
`NativeBinding::host(callback)` adapters select owned argument conversion or the
existing checked host-context/value callback contract. They converge on the same
declaration validation and installation path. Adapter constructors carry no second
signature authority. Consolidate the current function-binding conveniences around
this entrypoint during migration; do not add a separate host-function method.
Type/storage and path binding remain distinct operations with their own contracts.

The `define_type` entry above denotes extending the current type builder, not a
commitment to replacing its existing signature. Host binding helpers attach
implementations to shared declarations; they do not carry independent signatures.
Reuse the existing callback/context and path read/validate/prepare-write protocols.
Methods and trait implementations keep ordinary checked selection. Required host
passing/access facts migrate intact even though new ergonomic Rust reference
adapters are deferred.

`engine_builder.declarations()` provides metadata for already installed modules,
including dependencies referenced by the new module. Reading it does not attach
objects or execute callbacks. A standalone declaration catalog remains usable
without an engine. Registration must support existing mutually referencing host
types without requiring a brittle single-type installation order.

### Complete example: expose a Rust Player

This example follows one host-owned struct from registration through execution.
All domain state, lookup helpers and adapter callbacks are defined below. The
blocks form one example in order; imports and final public module paths are omitted
because the registration API is not implemented. All blocks are Rust-like or Kagari
pseudocode, not a currently compilable sample. The existing runnable
[atomic host path example](../crates/kagari-runtime/examples/atomic_host_path.rs)
provides the current read/prepare/commit API on which this proposal builds.

The intended script behavior is: receive a checked `Player` handle, read hp=100,
set hp=75, and return 75. The host observes the same object updated to 75. Neither
Rust reference decoding nor future `Opaque<T>` storage is used.

#### 1. Define the application state and its object lookup

```text
use std::{
    collections::HashMap,
    sync::{Arc, atomic::{AtomicI32, Ordering}},
};

struct Player {
    hp: AtomicI32,
}

struct Players {
    objects: HashMap<HostObjectId, Arc<Player>>,
}

impl Players {
    fn get(&self, id: HostObjectId) -> Result<Arc<Player>, HostError> {
        self.objects.get(&id).cloned()
            .ok_or_else(|| HostError::new("unknown player"))
    }
}

let player_id = HostObjectId(7);
let player = Arc::new(Player { hp: AtomicI32::new(100) });
let players = Arc::new(Players {
    objects: HashMap::from([(player_id, Arc::clone(&player))]),
});
```

`Players` is application code, not a new Kagari registry. It owns Rust objects;
Kagari's runtime registry separately validates script root ownership and schemas.
The example uses an immutable object table and keeps the player alive throughout
execution. Mutable/removable tables must supply their own safe liveness contract.
Atomic storage makes the captured state safely shareable; the example performs no
concurrent writes and does not claim atomicity across an entire script operation.

#### 2. Define the hp adapter and register the type and field

```text
let read_players = Arc::clone(&players);
let write_players = Arc::clone(&players);

let hp_adapter = HostPathAdapter::new()
    .with_read(move |_call, path| {
        let player = read_players.get(path.root.object_id())?;
        Ok(Value::I32(player.hp.load(Ordering::SeqCst)))
    })
    .with_prepare_write(move |_call, path, record| {
        let Value::I32(hp) = record.new_value else {
            return Err(HostError::new("hp expects i32"));
        };
        if hp < 0 {
            return Err(HostError::new("hp cannot be negative"));
        }
        let player = write_players.get(path.root.object_id())?;
        Ok(PreparedHostPathWrite::new(move || {
            player.hp.store(hp, Ordering::SeqCst);
        }))
    });

let mut engine_builder = KagariEngine::builder();
let mut module =
    ModuleBuilder::new("game::actors", engine_builder.declarations())?;

let mut player_builder = module.host_type::<Player>("Player")?;
player_builder.field("hp", Type::i32(), PathAccess::ReadWrite, hp_adapter)?;
let player_type = player_builder.finish()?;
```

`HostPathAdapter`, `with_read`, `with_prepare_write` and `PreparedHostPathWrite`
reuse existing runtime concepts. The adapter first validates the new scalar and
resolves its host object. Preparation does not write the field. Its commit closure
only stores the checked value; it cannot fail or reenter script. Runtime path
execution keeps its existing borrow, root, dirty-ledger and commit checks.

`host_type` and `field` are proposed conveniences over shared declaration/binding
construction. They record the nominal Player identity, local Rust type mapping,
a writable i32 member, its one-segment typed path and the attached adapter. This
example uses schema epoch 0. They do not instantiate a Player, derive a Rust
converter or expose unregistered fields. `finish` returns the declared type handle.

For this ordinary field, the caller does not manually assemble a path declaration.
The generated path facts are exactly:

| Fact | Value |
| --- | --- |
| Root | The registered `game::actors::Player` declaration |
| Segment | The registered `hp` member identity |
| Result | `i32` |
| Access | Read/write |
| Schema epoch | 0 |
| Implementation | The `hp_adapter` defined above |

Explicit path authoring remains available for existing advanced nested/indexed
paths. Field convenience construction must use the same declaration validation and
adapter machinery, not a parallel field implementation.

#### 3. Bind a function accepting Player, plus an ordinary native function

```text
let health = module.define_function(
    FunctionDecl::new("health")
        .host_parameter("player", &player_type, HostPassingStyle::SharedBorrow)
        .returns(Type::i32()),
)?;

let health_players = Arc::clone(&players);
module.bind(health, NativeBinding::host(move |call, arguments| {
    let [Value::HostRoot(slot)] = arguments else {
        return Err(HostError::new("health expects a Player handle"));
    };
    let root = call.runtime().gc().host_root(*slot)
        .ok_or_else(|| HostError::new("invalid Player handle"))?;
    let player = health_players.get(root.object_id())?;
    Ok(Value::I32(player.hp.load(Ordering::SeqCst)))
}))?;

let maximum = module.define_function(
    FunctionDecl::new("maximum")
        .parameter("a", Type::i32())
        .parameter("b", Type::i32())
        .returns(Type::i32()),
)?;
module.bind(maximum, NativeBinding::typed(
    |_context, (a, b): (i32, i32)| Ok(a.max(b))
))?;

engine_builder.install(module.finish()?)?;
let engine = engine_builder.build()?;
```

Both functions use `bind`. `NativeBinding::host` receives the existing checked
host context and Value arguments; `NativeBinding::typed` performs owned argument
conversion. Their constructors and the shared `bind` signature are proposed.

`host_parameter` is a proposed builder spelling for existing host nominal-type and
passing metadata. SharedBorrow tells invocation to hold the current shared host
lease while calling the callback. It does not decode a Rust `&Player` argument or
introduce a new script reference type. The host-call boundary validates the root's
runtime owner, nominal Player type, schema and access before entering the callback.
The callback then looks up the application object by the validated root's ID.

Script therefore passes a Player struct handle to `health`, not a numeric object ID.
The handle's internal runtime representation is deliberately visible in this
low-level adapter example because ergonomic Rust reference inputs are deferred.

#### 4. Compile the script and attach the particular player

```text
let source = r#"
use game::actors::{Player, health, maximum};

pub fn tick(player: Player) -> i32 {
    val before = health(player);
    player.hp = maximum(before - 25, 0);
    player.hp
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

let runtime_player_type = runtime.resolve_host_type(&player_type)?;
let root = runtime.runtime_mut().register_host_root(
    player_id, runtime_player_type, HostSchemaEpoch::new(0),
)?;
let argument = runtime.runtime().gc().alloc_host_root(root)?;
let result = runtime.execute(&loaded, "tick", &[argument], &context)?;

assert_eq!(result.return_value.value(runtime.runtime().gc()), Some(Value::I32(75)));
assert_eq!(player.hp.load(Ordering::SeqCst), 75);
```

`resolve_host_type` is a proposed checked lookup from the module's declaration
handle to this runtime's installed type slot. It rejects missing/incompatible
bindings; it does not register another copy of the type. The remaining execution
sequence follows existing SDK operations; exact error conversions and signatures
will be finalized in HR01. Handle allocation is immediately followed by execution,
whose existing argument-rooting boundary retains it during the call. Longer-lived
host storage of script values must use the existing explicit root mechanism.

The registered root identifies Player 7 in this runtime. It does not transfer the
Rust object into the script heap: the captured `Players` service owns that object.
This example intentionally shares the same Players service if the module creates
multiple runtimes; applications requiring separate state need runtime-local bindings.

#### 5. Follow one operation through the layers

1. `health(player)` resolves to the installed function declaration. Invocation
   checks the Player root and acquires the declared shared lease. The callback
   reads the Rust object's hp=100; the lease ends when the call completes.
2. `maximum(100 - 25, 0)` uses the ordinary typed native adapter and returns 75.
3. `player.hp = 75` resolves the statically declared hp path. The runtime acquires
   the existing write access, roots values and invokes the adapter's preparation.
   The adapter validates hp and returns the pending write. Runtime commit publishes
   the update and dirty record according to the current mutation contract.
4. The final `player.hp` read uses the same registered field adapter and returns 75.
   The Rust assertion sees 75 because both accesses address the same Player object.

Assigning `player.hp = -1` instead makes preparation return an error before the
store: that assignment does not change hp. Earlier completed operations remain
visible. No whole-script rollback or automatic field borrowing is implied.

### Struct parameters

An owned Rust struct argument uses the ordinary typed conversion boundary. For
example, assume an explicitly registered nominal `DamageRequest` with `amount`
and `armor` fields and matching `KagariType`/`FromKagari` implementations. The
following shows binding after that declaration and conversion setup; it does not
add automatic DTO derives to this migration:

```text
struct DamageRequest { amount: i32, armor: i32 }

fn calculate_damage(request: DamageRequest) -> i32 {
    request.amount.saturating_sub(request.armor).max(0)
}

function = module.define_function(
    FunctionDecl::new("calculate_damage")
        .parameter("request", damage_request_type)
        .returns(Type::i32())
)
module.bind(function, NativeBinding::typed(
    |_context, (request,): (DamageRequest,)| Ok(calculate_damage(request))
))
```

This callback receives owned converted data; changes to that Rust value do not
mutate the original script object. Automatic struct derives remain owned by the
[interoperability plan](rust-interop-design.md#value-binding).

An existing externally owned host struct instead uses its declared host type and
the host callback adapter. Script passes a checked host handle; the callback uses
the existing runtime/context and application object lookup, not an implicit Rust
struct copy or `&T` decoder. Both representations use the same function `bind`
entrypoint while preserving their distinct conversion and ownership contracts.

### Installation and runtime objects

`KagariEngineBuilder::install` / `install_all` remains the installation entrypoint.
Validate identity, signature, host representation, declared access, path schemas,
dependencies and binding completeness before publishing changes. A failed module
or batch must leave the builder unchanged.

Runtime preparation installs the module's local bindings through the common path.
Application objects still attach to each runtime through existing root lifecycle
operations. Type/function installation and object attachment remain separate;
attaching an object must not require reinstalling a second type declaration.

Shared module bindings cannot capture short-lived application borrows. If existing
host callbacks require per-runtime state, define a checked local factory at their
binding owner and migrate concrete consumers in HR02. Preserve current sharing,
thread-transfer and active-scope constraints without introducing new reference APIs.

### Invocation and effects

Native callbacks are potentially effectful, including callbacks using shared Rust
access or owning opaque handles. Interior mutation, captured state and IO cannot
be inferred from parameter access modes. Shared registration supplies conservative
internal call-effect metadata automatically. Preserve argument/receiver evaluation
order, runtime checks, cancellation and completed side effects.

Existing checked path writes keep validation, reservation, infallible commit and
dirty-ledger behavior. Ordinary host callbacks keep their current failure and
notification contracts. Registration unification adds no blanket dirty reporting,
transactional callback guarantee or new projection policy.

### Retained Opaque integration

The [Rust interoperability plan](rust-interop-design.md#opaque-ownership-and-access)
owns future retained `Opaque<T>` storage and conversions. Those bindings must use
the same module declarations and installation, while retaining their own ownership
contract. Do not implement speculative opaque storage or reference adapters here.

`Opaque<T>` is a Rust wrapper; script sees the registered nominal type. Cloned
handles retain the same object. Its methods use shared access and host-owned safe
interior mutation; runtime-local borrow checks cannot establish exclusivity against
external Rust aliases. Returning an independently retained handle is distinct from
returning a Rust reference borrowed from an input. Neither this migration nor a
future borrowed-input adapter may silently convert the former into `&mut T`.

## Migration invariants

- Preserve static types, member/trait selection, declared access and runtime/schema
  ownership. Matching names or Rust type names alone do not prove identity.
- Preserve root lifetimes, existing scope escape checks, alias conflicts and cleanup
  on traps, cancellation, depth exhaustion and supported Rust unwinding.
- Keep typed path operations' once-only evaluation, read-after-RHS semantics,
  preparation/commit and dirty records. No new replacement/relocation behavior.
- Retain checked source-free codecs and linking. Serialize portable declarations
  and contracts, never Rust addresses, TypeId values, callbacks or live scopes.
- Preserve pinned generations and synchronous reentry. Do not rebind an active call
  to the newest module because its exported name matches.
- Retain meaningful source-free, ownership, malformed-artifact and backend coverage.
  Replace obsolete models directly; refresh disposable fixtures at a coherent
  checkpoint without routine unpublished ABI/version bumps.

## Implementation stages

### HR01: Map existing contracts and freeze registration APIs

Inventory current declaration consumers, SDK entrypoints, host callback state,
macros, codecs, backend imports and runtime preparation. Map each existing fact
and binding to its shared owner, including type batches and methods/traits.
Freeze host binding helpers, the field convenience builder, declaration-to-runtime
type lookup and any necessary runtime-local factory contracts. Reconcile every
proposed operation in the complete example with these existing owners.
Record the consumer/removal map here before changing implementation.

Gate: every current host capability has a migration destination; API examples can
be implemented without new Rust reference decoding, script wrapper types or
changed host execution semantics.

### HR02: Migrate registration and consumers end to end

Extend shared declarations and module validation; migrate source/tooling import,
SDK installation, runtime preparation, executable codecs/linking and affected
backends together for each coherent slice. Ordinary and existing host functions
must share declaration authority and checked invocation preparation.

Keep intermediate builds working. Migrate producers and consumers together where
an API change cannot safely be split. Remove superseded entrypoints as their
consumers migrate. Any temporary bridge needs a specific HR03 removal entry here;
do not create new dual signature authorities or bypass validation to compile.

Gate: a module containing ordinary natives and existing host type/method/path
bindings works through source compilation and source-free loading. Rejected
installation is atomic; malformed/mismatched contracts fail before execution.
Selected tests demonstrate existing path, borrow, reentry and reload behavior.

### HR03: Complete legacy removal and focused integration

Remove remaining HostInterface aggregation/serialization, obsolete registration
APIs, duplicate records and migration bridges. Migrate examples and macro consumers;
batch affected artifact refreshes. Retain focused host identity/path/borrow services.

Update architecture, host/embedding/path specifications and API docs with the final
registration contract. Resolve carried errors. Mark only registration unification
complete in SA21; borrowing/projection follow-up remains deferred there. Remove this
plan after implementation completion and move outstanding work to its proper owner.

Gate: no consumer needs the old registration path; focused local acceptance passes,
and remaining exhaustive CI scope is reported separately.

## Validation and acceptance

Select affected tests within existing owners for each slice; do not run every
listed target at every checkpoint or duplicate the workspace suite locally.

| Scope | Existing test owners |
| --- | --- |
| Declaration/binding validation and atomic installation | Runtime `native_registration`, `host_registration`; SDK `registration_sources`. |
| Host methods, associated types and interface dispatch | SDK `host_interfaces`. |
| Source-free loading, path/schema/access rejection | SDK `offline_nominal` path cases; runtime `offline_host`. |
| Existing host identity, borrow and cleanup guarantees | Selected runtime `host_borrows`, `host_scopes`, `host_objects` and VM `native_boundary` host cases. |
| Integration | Small affected pinned-reload regression selection; representative actual-native host case if backend contracts change. |

Use package/target/test-name filters and `--no-fail-fast` for multiple selected
targets. Include `cargo check -p kagari-embed --no-default-features --lib` when
validating the migrated source-free dependency boundary. Interpreter fallback is
not actual-native evidence. New tests are justified by uncovered contracts, not
changed private structures or migration-specific API spellings.

Implementation checkpoints include import/ownership/visibility/effective-LOC
review, `uv run --locked scripts/check_structure.py`, formatting,
`git diff --check`, affected tests and scoped Clippy. Full workspace tests/Clippy
and exhaustive feature/backend matrices belong to CI. Record selected commands,
rationale, failures and unrun CI scope below. This plan makes no performance claim.
Documentation-only edits require content/link/diff checks, not Cargo validation.

## Progress ledger

- [ ] HR01: existing contract mapping and registration API frozen.
- [ ] HR02: shared registration and existing consumers migrated.
- [ ] HR03: legacy removal and focused integration complete.

Scope checkpoint: registration unification is the next task. New `&T`/`&mut T`
adapters and field-view extensions are deferred; existing safety checks remain
part of migration acceptance. No implementation or Cargo validation is claimed.
No intermediate build errors have been introduced by this documentation task.
Implementation commits use Conventional Commits and a phase trailer such as
`Plan-Phase: HR02`.
