# Authoring native library modules

Native registrations are explicit Kagari contracts with checked Rust implementations.
They do not infer the script API from Rust signatures. This guide covers the runtime
[authoring API](../crates/kagari-runtime/src/native/assembly.rs); the
[declaration specification](spec/standard-declarations.md) defines the preserved
contracts and [architecture](architecture.md#unified-library-registration) defines
ownership. SDK analysis lowers registrations directly, even when a host publishes
generated `.kgr` for tooling. Those files are not compiler inputs or another
signature authority.

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
                        move |cx| {
                            let text = cx.argument(0)?;
                            cx.allocate_enum(&cx.result_type_argument()?, &variant, &[text])
                        },
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
    let assembly = collect(&DeclarationCatalog::default())?;
    let snapshot = assembly.declarations(); // No factory has run.
    assert_eq!(snapshot.modules().len(), 2);
    let modules = assembly.try_into_send()?.into_modules()?;
    assert_eq!(modules.len(), 2); // Prepared, not installed.
    Ok(())
}
```

For an application, call `collect(builder.declarations())`, then
`builder.install_all(assembly.into_modules()?)` and consume the engine builder with
`build()`. This example defines its own modules and uses the String scalar;
it does not need stdlib's String extension methods.
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

Admission bounds and selected calls have different owners. For example, a
library algorithm may require `T: Eq` while calling `PartialEq::eq`. An override
must declare the calls its own body uses rather than copying a default body's
requirements; inferred callback requirements cannot replace stronger admission
bounds. The [numeric reductions](../crates/kagari-stdlib/src/foundation/numeric.rs)
require the source’s `Iterable::iter` and its cursor’s `Iterator::next` without
assuming a particular storage representation.

`define_trait_default` accepts a complete `MethodDefinition` or a
`DefaultDefinition` with explicit helper type arguments and proof facts. Runtime's
[shared default lowerer](../crates/kagari-runtime/src/native/builder/default_lowering.rs)
supplies the private Native helper's binders and associated-output substitution;
the trait method is Script and its complete ordinary call expression is retained in
`ModuleDecl.script_bodies`. There is no executable default-application recipe or
special HIR forwarding-call table. Do not author a second helper lowerer.
`TraitOverride::requires`, `produces` and `documentation` attach
implementation-specific facts/prose without redefining the trait signature.
The real impl/member binders are substituted before existing proofs run. Omitting a
method inherits its default. An explicit implementation with the applied signature
wins; a mismatched signature rejects, whether the declaration came from text or Rust.

[Trait impl ownership](../crates/kagari-types/src/declaration/ownership.rs) is package-based.
A package can implement its own trait. For a foreign trait, scan the receiver then
the trait arguments: there must be a local input type, with no uncovered impl
parameter before the first local type. Overlap, bounds, visibility and signatures
are checked separately. This applies to `From` and `TryFrom`, not inherent impls.
A Host source or destination is not disqualifying; a conversion returning a Host
value must still return a genuine handle.

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

A String or range constructor record (`define_constructor`) must match its canonical
owner, name and declared shape. It cannot confer builtin identity or native storage
on another declaration. For
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

## Registering Host fields and path operations

Declare the fields as Kagari should see them, including their visibility, type
and `val`/`var`. Then use `ModuleBuilder::define_path` for each supported chain
and `bind_path` for its storage adapter. Registration checks the paths you declare:
a read-write path needs both a read and a prepared-write hook. It does not
require a path for every possible use of a `var`. HIR records the checked field
IDs; lowering a field use needs a unique path for that root and ordered chain,
with matching types and sufficient access. A missing path or a read-only path
for a write fails lowering without changing the script declaration.

For example, a writable path to `player.baseline.hp` may traverse a readable
`val baseline: Stats` to a `var hp`. It permits replacing neither `baseline`
nor a `val hp`, and it does not upgrade a readonly runtime view.

For an index or virtual segment, declare the script-visible operation explicitly.
Use `bind_path_operation(method, &path, kind)` instead of `bind` for a Native
method backed by that path; `bind_path` still supplies its storage hooks. A free
Native function can use the same binding. Declare a non-generic, non-suspending
call with owned arguments: the Host root first (`self` on a method), then one
index per index segment in traversal order, then the RHS for `Set` or `Add`.
Writes require a read-write path and a host-mutation effect. `Read` returns the
leaf; `Set` takes a leaf value and returns unit; `Add` takes a numeric leaf value
and returns the updated value. No Rust receiver-loan binding is needed: the
path service checks access when it performs the operation.

The portable declaration binds the callable identity to the kind and exact path.
A decoded artifact rejects a missing adapter or a same-signature method bound
to a different path before execution. For the distinction between argument
order and descriptor slots, and the commit and dirty-record guarantees, see
[Host path semantics](spec/host-interop.md#kagari-side-semantics).

## Analysis and declaration presentation

Runtime registration supplies actual Rust bindings independently of how a tool
presents declarations. HIR consumes only binding-free static facts under the same
rules for each input representation. Packaged core is already source: its docs and
member ranges belong to the authored `.kgr` declaration, even through a `std`
re-export. Do not register it again or generate a replacement view. For additional
inputs the choices are:

- `AnalysisDatabase::set_native_modules`: owned `ModuleDecl` records lower directly
  into common HIR, then undergo ordinary resolution and checking. Declaration-to-HIR
  identity correspondence is constructed with the nodes, not recovered from text.
- For an editor, bind the generated `.kgr` documents to their published module
  identities alongside user files in `SourceSnapshot`. Supply the effective import
  environment. HIR checks the declared API as ordinary source, with no registration
  IDs, Rust callbacks or Host path descriptors. Core remains a separate set of
  packaged source documents in both requests.

An external Host handle renders as `pub native host type Player;` or, for example,
`pub native host type Player { pub var hp: i32 }`. Both forms preserve the handle
category, not its storage or a borrow policy. Ordinary opaque Native storage renders
as `pub native type Player;` even if its Rust adapter takes a loan. `host` remains
an identifier elsewhere. A source-only editor can check `player.hp`; compiling with
the generated Host declaration as the owner fails with `MissingHostSchema`, even
for the fieldless form. Supply the owning registration instead of its text; then
the field operation still needs the matching path described above.

Under the [declaration-input contract](spec/standard-declarations.md#declaration-analysis-inputs),
ordinary Script definitions and defaults must remain complete, including embedded source: signatures or placeholder
bodies are not substitutes. Structured expression bodies and parsed blocks use
ordinary resolution, body checking and generic instance planning; module-qualified
helper references cannot be captured by parameters. Genuine Native defaults remain
external implementation declarations. Portable origins and instruction-checked body
records establish internal materialization provenance, not semantic authentication.
[Runtime Script authentication](../crates/kagari-runtime/src/native/registry/scripts.rs)
compares the actual bytecode call/value trace against the independently installed
canonical ScriptExpr and function contract, applying binders and normalizing associated
outputs through the checked impl when the product claims an inherited installed
body. A same-signature substitute with new instruction evidence still rejects.
An ordinary authored trait without an installed owner needs no registration. See [checked contracts](spec/standard-declarations.md#checked-executable-contracts).
Native field support creates no Runtime adapter. HIR selects methods by their
Kagari signatures; any Opaque receiver loan belongs to the registered binding,
not to method lookup or the Host path adapter.

[ImportEnvironment](../crates/kagari-hir/src/imports/environment.rs) separately supplies
package aliases, dependencies and selected preludes, with producer/package defaults
normalized before graph construction. Explicit aliases determine package selection;
dependencies do not grant private access, and coherence uses actual defining owners.
Immutable analysis inputs retain explicit scopes; replay also needs the physical source snapshot. New requests use the
current scopes, and scope changes reset the environment fingerprint. Runtime
`NativeBinding` values and Rust storage/access adapters remain valid
implementation-side objects, never HIR authority.

Source-enabled engines check registrations structurally. Registered docs and
identities do not require a physical owning source. For navigation, call
`engine.native_declaration_sources(project, cancel)` with the authored project
snapshot: the SDK returns generated documents and their effective import scopes.
Without a cache this request renders in memory. Set `builder.declaration_cache(path)`
before `build()` to publish immutable files instead. Build publishes the
source-independent registrations; the tooling request can include registrations
that require trait declarations from the project. Neither publication nor the CLI
changes which inputs the compiler checks. See the
[materialization example](spec/standard-declarations.md#engine-source-presentation).

An editor can replace its generated documents and scope selections together.
Publication does not add a directory watcher or provider discovery protocol. A
generated URI cannot select a Rust implementation or grant layout access.

### Private core Native bindings

`core::ops` and `core::result` own the private declarations for range endpoint
projection and `Result` error-origin transfer. There is no second registered
owner or duplicate HIR signature source. For example, HIR checks
`range_bound(self, false)` by the ordinary private declaration, and the compiler
records its applied signature and dependency. The same verified contract is used
when loading a compiled artifact without source.

The loader selects the Runtime-owned binding by canonical declaration identity,
not by a function spelling in a script call or a product-supplied intrinsic flag.
It compares the declaration to the fixed implementation contract. Portable
verification checks exact call slots, declaring-module authorization and the
reporting `Result` shape for origin transfer. Normal Native registration cannot
replace these entries. The Rust implementations use protected value/scoped type
views, and the Native boundary owns result publication.

## The standard bundle and runtime boundaries

Engine and Runtime constructors start with no optional Native providers. A source-enabled
builder exposes the verified core declarations before installation. To add stdlib:

```rust
let library = kagari_stdlib::StandardLibrary::new(builder.declarations())?;
builder.install_all(library.modules()?)?;
```

A frontend-free host can use `StandardLibrary::from_verified_program` with verified
bytecode selected by the host that contains the packaged core owners; application modules in that image
are not stdlib providers. Composing the bundle validates declarations but installs
nothing. Only install it if needed. For example, `String` is a core type, but its
`trim` method requires `std::string`. Script-only core calls need neither `.kgr`
files nor stdlib at execution. Missing or mismatched native bindings fail admission.

The standard library's private [Bundle](../crates/kagari-stdlib/src/assembly.rs)
retains one collected semantic projection and one terminal preparation result.
`library.declarations()` borrows that sealed product without preparing callbacks.
Its [catalog](../crates/kagari-stdlib/src/catalog/semantic.rs) includes the verified
core contracts. `library.modules()` prepares once per bundle and shares completed
native modules.

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
computations retain exact bodies. Registration preserves frozen declarations, local
static-initializer bindings, executable verification and retained demands; unknown
native reentry does not impose a global body freeze. Callback roots, scoped borrows,
reentry and cancellation checks still apply. See [publication contracts](spec/module-activation.md).

Assembly methods currently take no caller cancellation token. They are synchronous
collection/preparation operations, not cancellable script sessions; factory Rust
work is not automatically preempted. At execution time cancellation remains
cooperative: long callback loops must use `NativeCall::poll()` and keep existing
native helpers' polling, rooting and cleanup behavior. Cancellation/traps retain
completed script and host effects; neither registration nor cancellation promises
rollback of those effects. See [execution](spec/execution.md) for execution policy.
