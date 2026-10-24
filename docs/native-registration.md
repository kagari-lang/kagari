# Authoring native library modules

Native registrations are explicit Kagari contracts with checked Rust implementations.
They do not infer the script API from Rust signatures. This guide covers the runtime
[authoring API](../crates/kagari-runtime/src/native/assembly.rs); the
[declaration specification](spec/standard-declarations.md) defines the preserved
contracts and [architecture](architecture.md#unified-library-registration) defines
ownership. Generated `.kgr` files are analysis/navigation views, not another producer.

## Choose the authoring boundary

- **`ModuleBuilder`** builds a module against explicit, already available providers.
  Keep using it for ordinary application bindings, typed host types/paths and
  independently authored declarations. `ModuleBuilder::from_declaration` remains
  the checked adoption API; it is not deprecated. See the
  [scalar registration example](spec/standard-declarations.md#explicit-registration).
- **`ModuleDefinition` + `ModuleSetBuilder`** author an interdependent batch. Each
  definition supplies one canonical `ModuleIdentity` and an owned `FnOnce` recipe.
  A `ModuleDraft` collects declarations and implementation associations under that
  owner. Use this path for same-batch references and for obtaining checked
  declarations without preparing native adapters.

Both routes reuse native signature/implementation lowering and catalog/module
validation, and produce ordinary `NativeModule` values for `install_all`. Neither
creates or installs a runtime. A prepared module does not prove that its external
providers are installed in the destination engine: installation checks the actual
closed batch again. Supply the engine builder's `declarations()` as the provider
catalog when composing application libraries with that engine.

## Collection, sealing and preparation

1. `ModuleSetBuilder::new(&providers)` imports the external provider catalog once
   into one shared definition context. `add` records recipes, rejecting duplicate
   and external owners, including empty/export-only owners, before running them.
2. Consuming `finish()` runs recipes sequentially in insertion order, then lowers
   implementation/default intents against the complete contracts and seals the
   batch. Missing forward declarations/members, foreign scopes, wrong categories
   or arities, invalid bounds/requirements/concrete results, incomplete native
   associations and invalid exports reject here. Factories are not invoked.
3. `assembly.declarations()` clones an independently retained `DeclarationSnapshot`.
   `modules()` exposes this batch's records; `catalog()` includes adopted providers.
   The snapshot contains no recipes, pending factories or runtime roots. Dropping
   an unused assembly drops its pending work without invalidating the snapshot.
4. Consuming `into_modules()` invokes each pending factory/adapter at most once,
   validates the resulting binding against its sealed target, and returns the
   complete module vector. It neither changes the declarations nor installs them.
5. The host passes that vector to the ordinary atomic batch installer. Source
   rendering, executable loading and invocation have their own checks; none can
   substitute for successful semantic sealing or adapter preparation.

Factory order is not an inter-factory communication API. If a factory fails after
an earlier module has prepared, completed candidates and remaining pending work
are dropped, later factories do not run, and no partial vector is returned.
Previously retained declarations remain usable. Rust side effects performed by
recipes/factories are **not rolled back**. Plain runtime assembly propagates errors
and does not catch unwinding panics or cache/retry a consumed assembly.

### Local versus transferable pending work

| Terminal | Retained work | Transfer rule |
| --- | --- | --- |
| `implementation(adapter)` | Local adapter | No added `Send` requirement |
| `implementation_with(factory)` | Local one-shot factory | May capture thread-local authoring state |
| `implementation_send(adapter)` | Transferable adapter | Stored adapter must be `Send` |
| `implementation_with_send(factory)` | Transferable one-shot factory | Factory captures must be `Send`; the adapter it creates need not be |

Recipes and `.configure(...)` callbacks execute locally and need not be `Send`.
The final `NativeBinding` retains its existing callback `Send`/`Sync` requirements;
Local pending work does not permit a non-thread-safe runtime callback.

`try_into_send()` consumes a sealed `ModuleAssembly` and moves only sealed facts,
storage descriptors and explicitly transferable pending work into a
`SendModuleAssembly`. It does not prepare adapters or seal again. Even a Local
entry whose captures happen to be `Send` rejects transfer: choose the `_send`
terminal explicitly. Failure consumes the assembly and identifies the first
Local module/member; retain `declarations()` beforehand if needed. A transferred
assembly uses the same one-shot `into_modules()` pipeline on its consuming thread.
This is not permission for concurrent access to a script heap.

## A complete two-owner example

The example below owns `demo::packets::Packet` and `demo::contracts::TextView`.
The Packet recipe runs first and references a trait member not yet declared.
It submits complete fluent inherent methods, a checked trait override, a raw enum
constructor, a fixed primitive, an interface result with a concrete-result proof,
and a method-local generic identity function. Reversing the two recipes also works.

The constructor factory reads its **own sealed declaration** to select the result
codec and the **same sealed catalog** to capture a symbolic variant handle. It
never captures an engine, bundle, heap root or runtime-specific enum layout.

```rust
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
};
use kagari_runtime::native::{
    assembly::{
        ModuleAssembly, ModuleDefinition, ModuleSetBuilder,
        authoring::{PendingBinding, SealedPreparation},
        draft::{ModuleDraft, TraitImplementation, traits::TraitOverride},
    },
    binding::{Codec, NativeBinding, NativeResult},
    catalog::DeclarationCatalog,
    declarations::MethodDecl,
    primitive::NativePrimitive,
    types::Type,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{declaration::{TypeDefKind, VariantDef}, scalar::BuiltinType};

fn owner(module: &str) -> ModuleIdentity {
    ModuleIdentity { package: PackageId("demo".into()), path: vec![module.into()] }
}

fn path(module: &str, kind: DefinitionKind, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: owner(module),
        path: vec![DefinitionPathSegment { kind, name: name.into(), occurrence: 0 }],
    }
}

fn string() -> Type {
    Type::scalar(BuiltinType::String)
}

fn contracts(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    module.documentation("Read-only text projection implemented by packet owners.");
    let id = module.definition(DefinitionKind::Trait, "TextView")?;
    let text = module.trait_method(id, MethodDecl::instance("text").returns(string()))?;
    module.define_trait("TextView", 0, |record, _| {
        record.methods.push(text);
        Ok(())
    })?;
    let member = module.member(id, DefinitionKind::Method, "text")?;
    module.document(member, "Return the retained text without changing the receiver.")?;
    Ok(())
}

fn packets(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let packet = module.define_type("Packet", TypeDefKind::Enum, 0, |record, _| {
        record.variants.push(VariantDef {
            name: "Text".into(), payload: vec![string().abi().clone()], reports_failure: false,
        });
        Ok(())
    })?;
    module.document(packet.id(), "An immutable packet retaining one String.")?;
    let view = module.reference_trait(path("contracts", DefinitionKind::Trait, "TextView"), 0)?;
    let text = module.reference_method(&view, "text", 0)?;
    let concrete = packet.apply([])?;
    module.inherent(concrete.clone(), |methods| {
        methods.define(
            MethodDecl::static_method("new")
                .parameter("text", string())
                .returns(concrete.clone())
                .documentation("Retain text in a new packet.")
                .implementation_with_send(|sealed: &SealedPreparation<'_>| {
                    let declaration = sealed.declaration();
                    let result = Type::from_semantic(
                        declaration.records().function.return_type.clone(),
                        declaration.definitions(),
                    )?;
                    let variant = sealed.catalog()
                        .type_reference(&path("packets", DefinitionKind::Enum, "Packet"))?
                        .variant("Text")?;
                    Ok(NativeBinding::new(
                        vec![Codec::Exact(string())], Codec::Exact(result),
                        move |cx| cx.allocate_enum(
                            &cx.result_type_argument()?, &variant, vec![cx.argument(0)?],
                        ),
                    ))
                }),
        )?;
        methods.define(
            MethodDecl::instance("as_view")
                .produces(concrete.clone())
                .returns(view.apply([])?)
                .documentation("Return the same packet through TextView.")
                .implementation_send(NativeBinding::new(
                    vec![Codec::Exact(concrete.clone())], Codec::Exact(concrete.clone()),
                    |cx| cx.argument(0),
                )),
        )?;
        methods.define(
            MethodDecl::static_method("byte_len")
                .parameter("text", string()).returns(Type::u64())
                .implementation_send(NativeBinding::primitive(NativePrimitive::StringByteLength)),
        )?;
        methods.define(
            MethodDecl::static_method("identity")
                .configure(|function| {
                    let item = function.type_parameter("T")?.ty();
                    function.parameter("value", item.clone())?;
                    function.returns(item)?;
                    Ok(())
                })
                .documentation("Return the argument unchanged, including object identity.")
                .implementation_send(NativeBinding::new(
                    vec![Codec::Value], Codec::Value, |cx| cx.argument(0),
                )),
        )?;
        Ok(())
    })?;
    let binding = PendingBinding::implementation_with_send(|sealed| {
        let variant = sealed.catalog()
            .type_reference(&path("packets", DefinitionKind::Enum, "Packet"))?
            .variant("Text")?;
        Ok(NativeBinding::new(vec![Codec::Value], Codec::Exact(string()),
            move |cx| cx.enum_argument_field(0, &variant, 0)))
    });
    module.trait_implementation_with(0, |_| Ok(TraitImplementation {
        receiver: concrete, interface: view.apply([])?, bounds: vec![], bindings: vec![],
    }.with_overrides([TraitOverride::new(text, binding)])))
}

fn collect(providers: &DeclarationCatalog) -> NativeResult<ModuleAssembly> {
    let mut set = ModuleSetBuilder::new(providers)?;
    set.add(ModuleDefinition::new(owner("packets"), packets))?;
    set.add(ModuleDefinition::new(owner("contracts"), contracts))?;
    set.finish()
}

fn main() -> NativeResult<()> {
    let providers = StandardDeclarations::default().catalog()?;
    let assembly = collect(&providers)?;
    let snapshot = assembly.declarations(); // No factory has run.
    assert_eq!(snapshot.modules().len(), 2);
    let modules = assembly.try_into_send()?.into_modules()?;
    assert_eq!(modules.len(), 2); // Prepared, not installed.
    Ok(())
}
```

For an application, call `collect(builder.declarations())`, then
`builder.install_all(assembly.into_modules()?)` and consume the engine builder with
`build()`. The standard providers are dependencies, not reauthored local modules.
Once installed, the example supports the following Kagari operation:

```text
use demo::packets::Packet;
pub fn check() -> bool {
    val packet = Packet::new("hello");
    packet.as_view().text() == "hello"
        && Packet::byte_len("hello") == 5u64
        && Packet::identity(42) == 42
}
```

`as_view` does not invent interface evidence: sealing requires Packet's TextView
implementation. Its callback returns the concrete Packet; the ordinary linked
result adapter exposes the declared interface. A primitive selects a fixed runtime
kernel with validated codecs, not an arbitrary callback annotated as pure. Raw
`Codec::Value` bindings still obey the authored signature, handle and result checks.
The enum constructor returns immediately after allocation; another allocation or
synchronous script call would require rooting that new value first.

## Generic contracts, defaults and selected calls

Use `inherent_generic` for a local nominal generic receiver or `inherent_with` for
a structural receiver. These reuse the ordinary implementation lowerer and actual
impl/method binders. `FunctionDecl` has the same `configure`, `produces` and terminal
implementation family for complete free-function submissions. Configurations run
once, in insertion order, after fluent signature setters and `produces`, regardless
of where `.configure(...)` appeared in the chain.

Forward references are **incomplete promises**, not fabricated `TypeRef`/`TraitRef`
contracts. `reference_type`, `reference_trait`, `reference_method` and
`reference_associated_type` record expected scope/category/arity; sealing checks
fulfillment even for unused references. `DraftMember::projection`/`bind` express
associated outputs. `DraftMember::requirement` creates a `DraftCallableRequirement`;
`FunctionBuilder::requires_draft` submits that selected member call;
`bound_draft` expresses an admission bound. A bound alone is not a selected call.
Preserve requirement insertion order: distinct slots remain distinct even when
they describe the same obligation. A
[`NativeCallableRequirement`](../crates/kagari-types/src/declaration/requirement.rs)
is symbolic member-call evidence, not a function pointer or runtime argument list:
its `arguments` are method type arguments; trait arguments belong to `interface`.

For example, `MutableList<T>::dedup` admits `T: Eq`, but calls `T`'s
`PartialEq::eq`. The [generic default](../crates/kagari-stdlib/src/foundation/lists.rs)
also requires receiver iteration, iterator advancement and removal. The
[Vec override](../crates/kagari-stdlib/src/foundation/vector/operations.rs) edits
its storage directly and needs only the element comparison dependency, retrieved
through its declaration-scoped `SelectedCall`. Inferring admission bounds solely
from callbacks would lose the stronger Eq contract; copying default requirements
into every override would retain operations that its body does not use.

`define_trait_default` accepts a complete `MethodDefinition` or a
`DefaultDefinition` with explicit template arguments and proof facts. Runtime's shared default lowerer supplies template
binders and associated-output substitution; do not author a second template
lowerer. `TraitOverride::requires`, `produces` and `documentation` attach
implementation-specific facts/prose without redefining the trait signature.
The real impl/member binders are substituted before existing proofs run. See the
[List owner](../crates/kagari-stdlib/src/foundation/lists.rs) and
[iteration owner](../crates/kagari-stdlib/src/foundation/iteration.rs) for concrete
hidden-iterator and paired-requirement applications.

A factory's `SealedPreparation::declaration()` names its exact binding target,
including lowered ordered requirements and concrete result. For trait defaults
that target is the sealed private template, while adapter preparation still uses
the authored method signature. Pair template-owned
`SelectedCall::from_declaration` tokens with
`NativeBinding::declared(sealed.declaration(), callback)` so their provenance is
retained. Do not borrow tokens from a different member or discover templates by
name/impl occurrence. Reusing a Rust algorithm does not merge binding identities:
each declaration retains its own checked target.

## Exports, documentation and storage

`export_path` supports forward canonical exports; sealing checks target existence,
namespace/category and duplicates through the ordinary export validator. A path
being interned does not establish a provider. `dependency` records an actual
module-level requirement, including empty providers; it rejects self-dependencies
and sealing rejects absent owners. Do not add dependencies merely because an
empty facade has a similarly named canonical module: `std::array` has no exports
or dependencies. Other facade dependencies follow their exports; the prelude's
explicit inventory and dependencies belong to
[composition](../crates/kagari-stdlib/src/assembly/composition.rs).

Attach prose at the declaration owner: fluent `documentation`, module
`documentation`, or `document(local_id, markdown)` for local types/members.
Trait implementation docs inherit actual member docs unless explicitly overridden.
Aliases preserve canonical identity/docs. Rendering may inherit/format prose, but
must not select an independent description through a global method-name switch.

A builtin constructor (`define_constructor`) records semantic ownership; it does
not imply native storage. Map/Set builtins are not nominal storage bindings. For
`TypeDefKind::NativeStorage`, `bind_storage` must match the local layout and generic
parameters exactly. Payload tracing, scoped host borrowing and runtime ownership
remain mandatory; assembly is not a route to unrestricted Rust references.

Advanced payload implementations supply
`NativePayload::charge(&self) -> NativeResult<HeapCharge>`, not `units()`.
Use `HeapCharge` from `kagari_runtime::resource`: logical units exclude the heap
identity, and retained bytes cover exclusively owned indirect backing, such as a
byte vector's capacity. Do not add the payload's shallow Rust size: preparation
adds the erased Box size exactly once. Do not charge referenced GC objects, shared
metadata or external host resources. Label capacity-based estimates and excluded
unobservable allocations; these numbers are GC pressure, not exact allocator/RSS
measurements or quotas.

Preparation samples the side-effect-free, non-reentrant callback once. The runtime
records the result, updates it through checked storage mutations and releases it
without calling the provider during sweep or take. A provider must not change
installed units or resize attributed backing through interior mutability. Prefer
the fixed-data or managed storage capabilities when they fit; a custom payload
charge does not grant arbitrary editing. See the
[host accounting contract](spec/host-interop.md#native-payload-accounting) and
[runtime coverage](spec/runtime.md#gc-heap).

## The standard bundle and runtime boundaries

The standard library's private [Bundle](../crates/kagari-stdlib/src/assembly.rs)
retains one collected semantic projection and one terminal preparation result.
Public `catalog::declarations`, `catalog::shared` and `StandardDeclarations` read
that same sealed product without preparing callbacks. The actual immutable
projection lives in [catalog](../crates/kagari-stdlib/src/catalog/semantic.rs);
`StandardDeclarations::catalog()` does not reconstruct a second provider cache.
`kagari_stdlib::modules()` prepares once and shares completed native modules.

Collection runs on the first caller's thread. The bundle requires transferable
pending work before publishing semantics; a shared recipe needs `Send`, not `Sync`.
The first module caller consumes preparation; concurrent callers share its result.
Ordinary errors and unwinding panics become terminal cached failures with no retry
or partial reuse. Same-thread recursive collection/preparation rejects even when
the initializer swallows the nested error. Preparation may read already published
semantics, but factories should use their supplied preparation view. There is no
general cross-thread wait-cycle detector: never spawn and join work that waits for
the same initializing bundle. Abort/double-panic termination is not recoverable.
Preparation failure does not invalidate previously published semantics.

Native callbacks capture immutable symbolic handles from the supplied sealed
catalog, not global lookup services or runtime-local roots/layouts. Per-runtime
loading/invocation still validates handles, generations, codecs, declared access,
selected-call proofs and concrete results. Shared registration is not a shared
script heap. Ordinary script/native callback calls select current implementations
through checked installation routes; created closures/Futures and admitted
computations retain exact bodies. Registration does not bypass protected Hash/Eq
or static dependency checks: unknown native reentry remains conservative, not a
claim of sealed behavior. See [publication contracts](spec/module-activation.md).

Assembly methods currently take no caller cancellation token. They are synchronous
collection/preparation operations, not cancellable script sessions; factory Rust
work is not automatically preempted. At execution time cancellation remains
cooperative: long callback loops must use `CallContext::poll()` and keep existing
native helpers' polling, rooting and cleanup behavior. Cancellation/traps retain
completed script and host effects; neither registration nor cancellation promises
rollback of those effects. See [execution](spec/execution.md) for execution policy.
