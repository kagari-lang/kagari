# Module-Owned Standard Library Registration Plan

Status: active; implementation and one commit per completed SR phase are authorized.
SR01-SR04 are locally complete; SR05 is next. This plan owns an overall replacement of
standard-library declaration/binding assembly, not a String-only cleanup. Unimplemented directory
sketches remain proposals. Current specifications remain authoritative until
implementation updates them.

## Goal and authority

Each standard-library module owns its explicit Kagari declarations, documentation,
implementation associations, storage bindings and exports. The top-level stdlib
entrypoint composes modules; it does not dispatch individual methods by internal
symbol spelling. The engine continues to install the complete library through one
ordinary atomic batch operation.

Follow [project goals](project_goal.md), [architecture](architecture.md#unified-library-registration),
[native declarations](spec/standard-declarations.md),
[identity ownership](architecture.md#definition-identity-ownership) and the
[roadmap](implementation-roadmap.md#module-owned-standard-library-registration).
This document owns SR01-SR06 scope, design decisions, acceptance and progress.
The confirmed product shape is module-local ownership, one complete fluent method
definition per submission, one stdlib installation entrypoint and internal staged
finalization. Exact Rust type names and unresolved SR01 mechanics remain proposals;
implementation must not silently revert to mandatory three-step method authoring.

## Evidence and root cause

The inspected implementation has sound checked module installation underneath a
fragmented standard-library authoring layer:

| Current owner | Responsibility and problem |
| --- | --- |
| [catalog/mod.rs](../crates/kagari-stdlib/src/catalog/mod.rs) | Builds a transient `std::__foundation` assembly, then partitions it into canonical modules |
| [catalog/strings.rs](../crates/kagari-stdlib/src/catalog/strings.rs) | Defines signatures/docs and synthesizes `$foundation_string_*` implementation names |
| [catalog/partition.rs](../crates/kagari-stdlib/src/catalog/partition.rs) | Infers publication owners partly through internal-name prefixes and special cases |
| [bindings.rs](../crates/kagari-stdlib/src/bindings.rs) | Enumerates native declarations, routes names to handlers, matches remaining names and selects storage by module identity; also contains library algorithms |
| [bindings/strings.rs](../crates/kagari-stdlib/src/bindings/strings.rs) | Repeats internal names to select concrete callbacks or primitives |
| [native builder](../crates/kagari-runtime/src/native/builder.rs) | Provides checked declaration adoption, local binding maps and module construction; retain this boundary |
| [inherent builder](../crates/kagari-runtime/src/native/builder/inherent.rs) | Already connects `define_method` and `bind` through `FunctionRef`; reuse this model |

For `text.contains("x")`, authoring currently creates an internal name, publication
uses its prefix to select the module, and binding matches the name again to find
the Rust function. Execution uses prepared identities/bindings, not this string
dispatch on every call. The defect is duplicated authoring knowledge and implicit
ownership, not evidence of per-call name-lookup overhead.

The target records the signature and callback association together under the
String module's explicit owner. Checked finalization produces the same script
operation, with the same UTF-8 and scoped heap-access behavior. A Rust callback
rename changes its Rust reference, not a remote matching table. Public names such
as `contains` remain explicit API data.

## Scope and exclusions

In scope:

- All bundled core/alloc/std declarations, inherent methods, trait implementations,
  default bodies, callable requirements, documentation, aliases/prelude and roles.
- All bundled callback, primitive and storage registrations, including String,
  scalar operations/construction/comparison, arrays/ranges, enums/propagation,
  Vec/HashMap/HashSet, collection interfaces, iterators, Future and tasks.
- The declaration-only and executable products and all callers that consume them.
- Minimal reusable changes to existing native authoring/building APIs needed for
  explicit module ownership, forward references and direct checked associations.
- Removal of obsolete name-routing, transient ownership inference and duplicate
  assembly paths; related tests, examples and contract documentation.
- A bounded execution-path non-regression check for migrated binding categories:
  authoring improvements must not introduce new per-invocation preparation.

Out of scope:

- New language/library APIs, changes to public core/alloc/std paths or semantics,
  optional-library/no_std modes, dynamic plugin discovery or live registration.
- VM/JIT rewrites, ABI/version bumps without a separate compatibility requirement,
  generated declaration binaries or a source parser as a registration dependency.
- [NC collection storage migration](native-collections-plan.md), the
  [host facade redesign](host-api-refactor.md), new conversion/borrow features,
  persistent caches and unrelated performance work. Existing repeated invocation
  checks/conversions belong to the queued
  [call-boundary follow-up](review.md#call-boundary-cost-follow-up), not an implicit
  SR requirement to optimize the entire native call path. Regressions introduced
  by SR itself must be repaired within SR.
- A procedural-macro framework or global enum containing every standard method.
  Explicit ordinary Rust registration with complete fluent definitions is the
  baseline; macro/code-generation sugar is not a prerequisite or a second
  signature authority.

## Target architecture

### 1. One semantic owner per module

Organize source by library responsibility rather than separate global declaration
and implementation trees. A possible layout is:

```text
src/
  lib.rs                  public entrypoints only
  assembly.rs             module composition and preparation
  string/
    mod.rs                String declarations and implementation associations
    search.rs             search callbacks when a separate file is useful
    transform.rs          transformations when a separate file is useful
  vec/
  collections/
  iter/
  option/
  result/
  numeric/
  task/
```

Small modules may remain single files. Rust source modules need not map one-to-one
to script modules, but every authored item must have an explicit canonical script
owner. Shared algorithms have one meaningful owner and explicit consumers; do not
move mixed policies into `common` merely to eliminate dependency edges.

A module owns its signatures, docs, role-bearing declarations, default bodies,
concrete result facts, implementation associations and storage. Cross-cutting
recipes may share helpers, but must receive the owner/references explicitly rather
than infer ownership from
receiver names, string prefixes or a global post-processing switch.

### 2. Direct checked implementation associations

The user-confirmed default is one complete method definition submitted once:
explicit signature, documentation, optional semantic facts and direct implementation
association are authored together. Internal scoped handles and checked binding
primitives still connect the products. Separate declare/configure/bind operations
are advanced mechanisms, not the ordinary authoring workflow.

Conceptual authoring, not a currently callable API:

```text
String module:
  define contains(self: String, pattern: String) -> bool
    with substring documentation and the explicit search::contains adapter

  define len(self: String) -> u64
    with byte-length documentation and the StringByteLength primitive
```

Callbacks, native primitives and existing default-body representations remain
distinct implementation forms. Do not turn a primitive into a slower callback or
invent a separate semantics implementation just to fit a uniform function pointer.
Shared generic bodies may bind several declarations explicitly; preserve applied
trait identity, type arguments, associated outputs and binder ownership.

IDs remain scoped, validated references, not Rust pointer values, raw numeric
constants, registration-order guesses or a global per-method enum. Names remain
useful for public declarations, diagnostics and portable identity. Eliminate the
use of private symbol spelling to discover callbacks or module owners; deleting
all textual identities is neither required nor desirable.

### 3. One authoring definition, separable products

Authoring co-location must not couple compiler/tooling consumers to an installed
runtime. The same module definition supplies two separable products:

```text
module-owned registration recipes
              |
    explicit staged assembly
         |                         |
checked declarations    pending implementation associations
         |                         |
analysis/documentation     checked NativeModules
                                   |
                          atomic batch installation
```

Declaration records contain no Rust callback, factory, heap object or live host
state. Binding recipes can defer preparation until the provider catalog is
complete; retaining a Rust function reference is not executing it. Declaration-only
consumption must not allocate a runtime, register storage, invoke native callbacks
or load/parse generated Kagari source. Do not maintain a second hand-authored
signature list for this mode.

Runtime binding preparation consumes the exact checked declaration set from the
same recipe/model. Avoid reconstructing the complete catalog independently for
each module or callback. Reuse immutable snapshots where their identity and
lifetime are explicit; do not add global mutable runtime state or speculative
persistent caching.

### 4. Internal finalization, not two public authoring inventories

Use one module authoring function. Collect drafts and direct associations first;
perform contract-dependent work only after all definitions are available. Do not
require authors to repeat nominal names/arity in a `reserve` function and a
`define` function. The earlier two-pass API sketch is superseded: a reserved trait
identity does not supply the members/defaults needed to expand its implementations.

The internal lifecycle is:

1. Establish final module owners in one assembly-owned `DefinitionContext`, and
   adopt external providers once. Module drafts are owner-scoped views, not
   separately cloned/imported provider catalogs.
2. Run each module recipe once. Collect declaration drafts, explicit forward
   references, implementation/default intents and pending Rust bindings/storage.
3. Complete type/trait/member declarations. Expand implementations and default
   templates against complete contracts, preserving applied arguments, hidden
   associated arguments, requirement owners and concrete-result evidence.
4. Validate and seal the complete semantic module set, including exports and
   dependency edges. Publish immutable scoped records and their derived catalog.
5. Consume pending adapters/storage against the sealed contracts to produce
   ordinary NativeModules. Existing installation owns atomic registry publication.

A forward reference carries a canonical identity, expected category/application
and assembly scope, not a fabricated completed `TypeRef` or `TraitRef`. Existing
checked references retain complete declaration records and must not acquire an
"incomplete but mutable later" meaning. A definition fulfills its identity once;
foreign definitions cannot be redefined, kind/arity disagreements reject, and
unresolved references cannot escape sealing. All generated helper identities must
exist before the published snapshot is frozen. Execution preparation cannot add
or modify declarations, defaults, dependencies or proof requirements afterward.

`String.find -> Option<u64>` is a simple forward reference. The decisive case is a
trait whose signatures refer to another module's type while that module implements
the trait using a default. Identity interning alone cannot expand that default.
Use collection followed by existing semantic lowering, not a dependency scheduler,
a fake provider record or repeated execution of authoring callbacks until they work.
Legal reference cycles do not imply acceptance of invalid recursive proof contracts.

Generic collection/finalization belongs with registration; stdlib policy stays in
stdlib. Share the semantic lowering/checking implementation with ordinary module
builders rather than build a second validator. SR01 must demonstrate this split on
real generic defaults before fixing the exact public Rust signatures.

### 5. One installation entrypoint, multiple real modules

Retain `kagari_stdlib::modules()` and the engine's existing ordinary
`install_all` path unless a concrete implementation constraint requires a reviewed
change. There is no need for a wrapper type that only forwards these calls.

The top-level assembler lists module contributors and coordinates stages; it does
not know individual String methods or contain Vec/Future/task storage switches.
All modules keep canonical core/alloc/std identities, checked aliases and explicit
prelude membership. Installing the library once does not mean merging it into one
monolithic `NativeModule` or recreating `std::__foundation` as a published owner.

## Proposed authoring API

SR01 now implements the runtime assembly and complete-method APIs described below.
The stdlib recipes, contributor layout and production entrypoint wiring remain a
**proposal**, not a completed migration. Examples are migration sketches marked as
`text`, with imports omitted; they use current API signatures but are not claimed
as compiled stdlib recipes. Current implementations are in
[runtime assembly](../crates/kagari-runtime/src/native/assembly.rs),
[method declarations](../crates/kagari-runtime/src/native/declarations.rs) and
[complete authoring](../crates/kagari-runtime/src/native/assembly/authoring.rs).

### API surface and ownership

| Surface | Current contract | Migration boundary |
| --- | --- | --- |
| `ModuleDefinition::new(identity, define)` | Retain one canonical owner and one `FnOnce(&mut ModuleDraft<'_>) -> NativeResult<()> + 'static` recipe | Implemented; stdlib contributor recipes remain proposed |
| `ModuleSetBuilder::new(&catalog)?` | Start an isolated context with explicit external providers; returns `NativeResult<Self>` | Implemented, not an implicit stdlib bootstrap |
| `set.add(definition)?` | Retain a recipe, rejecting duplicate/external owners before execution | Implemented composition operation |
| `set.finish()?` | Run recipes, lower semantic intents/defaults, validate and seal into `ModuleAssembly` | Does not run factories or prepare adapters |
| `assembly.declarations()` | Clone an independently retained `DeclarationSnapshot`; expose `modules()` and `catalog()` | No pending Rust implementation ownership in the snapshot |
| `assembly.into_modules()?` | Consume pending associations against sealed contracts and return `Vec<NativeModule>` | One-shot preparation through existing binding validators |
| `assembly.try_into_send()?` | Consume into `SendModuleAssembly`, rejecting any explicitly local pending binding | No preparation; transferred assembly also exposes `declarations()` and `into_modules()` |
| `module.define_constructor(name, constructor)?` | Author builtin-constructor metadata under its real owner | Distinct from nominal native storage and enums |
| `module.reference_type(path, kind, arity)?` | Take `DefinitionPath`, `TypeDefKind`, `usize`; return `DraftReference` | Explicit forward reference, not a completed type handle |
| `module.reference_trait(path, arity)?` | Take `DefinitionPath`, `usize`; return `DraftReference` | `reference.apply(arguments)?` returns `Type` for either reference kind |
| `module.inherent(receiver, configure)?` | Collect an inherent group for an explicit `Type` receiver | Complete ownership checks use the collected contracts |
| `MethodDecl::instance(name)` / `MethodDecl::static_method(name)` | Select whether to insert the receiver; `.parameter(name, ty)` adds explicit arguments | Implemented method signature constructors |
| `methods.define(definition)?` | Submit one complete `MethodDefinition` containing signature, docs, facts and pending implementation | No mandatory second configure/bind pass |
| `.produces(ty)` / `.configure(callback)` | Return `MethodAuthoring` for concrete-result facts or method-local configuration | Configuration runs once during submission, before sealing/preparation |
| `.implementation(adapter)` / `.implementation_with(factory)` | Complete a definition with an explicitly local pending adapter/factory | Valid for local assemblies, rejected by `try_into_send` even if captures happen to be Send |
| `.implementation_send(adapter)` / `.implementation_with_send(factory)` | Complete a definition with an explicitly transferable pending adapter/factory | Use for shared stdlib recipes; only the retained adapter or factory must be Send |
| `SealedPreparation::declaration()` / `.catalog()` | Read the exact sealed native target and complete catalog | Both factory variants receive `&SealedPreparation<'_>`, not `&DeclarationCatalog` |
| `module.bind_storage(&type_handle, storage)?` | Associate a local nominal storage declaration with a matching descriptor | No Map/Set storage-model change |
| `kagari_stdlib::modules()` / `builder.install_all(modules)` | Existing construction and atomic installation entrypoints | Retained; production stdlib still uses the old contributors |

These generic capabilities belong to `kagari-runtime::native`; stdlib owns the
recipes and canonical module list. `ModuleDraft` is an unsealed, owner-scoped view,
not a completed ModuleBuilder with weakened invariants. Recipes and configuration
callbacks may retain local non-Send state; transfer retains neither after sealing.

The local factory bound is
`FnOnce(&SealedPreparation<'_>) -> NativeResult<A> + 'static`, with
`A: NativeAdapter<M, R>`. The shared variant adds `Send` to the factory, not to `A`:
the returned adapter is created and immediately prepared on the consuming thread.
Direct `_send` attachment instead requires the retained adapter itself to be Send.
All four terminal methods return `MethodDefinition`; portable declarations never
contain callbacks or factories.

A canonical recipe can call multiple Rust contributor functions on the same draft.
For example, the one `core::ops` recipe can call operator, propagation and range
contributors. They do not each register a duplicate `core::ops` module. Leaf
contributors receive their owner from that recipe; no filesystem layout or receiver
spelling is used to infer a different publication identity.

### String module: signature and implementation together

**Proposal:** move the existing callbacks from
[`bindings/strings.rs`](../crates/kagari-stdlib/src/bindings/strings.rs) into the
illustrated `search`/`transform` helpers. These modules and their `definition`
entrypoint are not current stdlib APIs. The proposed `string/mod.rs` recipe uses
the implemented runtime API:

```text
mod search;
mod transform;

pub(crate) fn definition() -> ModuleDefinition {
    ModuleDefinition::new(namespaces::module("alloc", "string"), define)
}

fn define(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    module.documentation("Immutable UTF-8 strings.");
    module.define_constructor("String", NativeTypeConstructor::String)?;
    let string = Type::scalar(BuiltinType::String);
    let boolean = Type::scalar(BuiltinType::Bool);
    let u64_type = Type::scalar(BuiltinType::U64);

    module.inherent(string.clone(), |methods| {
        methods.define(
            MethodDecl::instance("contains")
                .parameter("pattern", string.clone())
                .returns(boolean.clone())
                .documentation("Test for a literal substring; the empty pattern matches.")
                .implementation_send(NativeBinding::new(
                    vec![Codec::Exact(string.clone()), Codec::Exact(string.clone())],
                    Codec::Exact(boolean.clone()),
                    search::contains,
                )),
        )?;

        methods.define(
            MethodDecl::instance("len")
                .returns(u64_type)
                .documentation("Return the UTF-8 byte length.")
                .implementation_send(NativeBinding::primitive(
                    NativePrimitive::StringByteLength,
                )),
        )?;
        Ok(())
    })
}
```

String remains a builtin value representation, but its existing constructor
metadata is necessary for public declaration identity and receiver ownership.
`define_constructor` preserves that record; it does not create a nominal heap
object or install new storage. The previous methods-only String example omitted
this requirement and could not bootstrap through the existing ownership checks.

The inherent group supplies the receiver type; `MethodDecl::instance` inserts its
argument, while `.parameter` lists only the remaining arguments. Associated
functions use `MethodDecl::static_method`, which inserts no receiver.
`search::contains` retains its scoped `CallContext`/heap-view behavior. The explicit
raw binding preserves both String input codecs and the Boolean result codec.
Do not pass this context-only function through the current flat typed adapter:
that adapter describes zero converted arguments, not arbitrary arguments obtained
from the context. Do not weaken exact codecs to `Codec::Value` or silently change
all context-only callbacks to make this example work. A small local raw-binding
helper may remove repetition without changing the checking contract.

`methods.define` adds the complete authored unit to the draft, rejecting local
duplicates/invalid ownership without leaving a half-added method on immediate
failure. Full codec/receiver/signature checks still run against finalized contracts
before a NativeModule escapes. `implementation_with` stores a factory rather than
running it during authoring; it cannot add declarations or semantic facts later.
The signature and codec contract remain distinct: one defines the Kagari API, the
other states what the Rust entry accepts. A single submission does not remove
their agreement check or imply that all checks can run immediately. Exact/raw
codec checks are not equivalent to proving a contextual owned adapter's concrete
Rust mapping: those adapters currently perform additional invocation-time checks.
Preserve that distinction rather than claim that finalizing any binding makes
all argument/result conversions prevalidated.

The chain denotes an authoring object. Attaching an implementation turns
`MethodDecl` or `MethodAuthoring` into `MethodDefinition`, keeping Rust ownership
outside portable metadata. Configuration callbacks run in insertion order after
fluent signature setters and `produces`; failed submission publishes no partial
method. Finalization separates semantic records from pending implementation
ownership and uses the shared declaration/binding machinery.

This is required ordinary authoring ergonomics, not deferred optional syntax sugar.
Use a returned handle only when another declaration or advanced configuration needs
it. Ordinary bounds, requirements and concrete-result facts should be authorable
on the complete definition without a mandatory second configuration pass.

### Cross-module types and storage

**Proposal:** owner recipes use the existing `identity::type_id` and
`identity::trait_id` helpers to name canonical declarations, not runtime IDs or a
new enum of all standard operations. The following migration sketch references
Option with its required path, kind and arity.

Option's recipe defines its enum, generic parameter and variants once through the
shared enum-authoring operations. String can record a reference before that recipe
runs; completing and checking the whole set is the assembler's responsibility.
For a generic result, adapter construction may require the finalized catalog:

```text
let option = module.reference_type(
    identity::type_id("Option", DefinitionKind::Enum),
    TypeDefKind::Enum,
    1,
)?;
let result = option.apply([Type::scalar(BuiltinType::U64)])?;

module.inherent(Type::scalar(BuiltinType::String), |methods| {
    methods.define(
        MethodDecl::instance("find")
            .parameter("pattern", Type::scalar(BuiltinType::String))
            .returns(result)
            .implementation_with_send(|preparation: &SealedPreparation<'_>| {
                let string = Type::scalar(BuiltinType::String);
                Ok(NativeBinding::new(
                    vec![Codec::Exact(string.clone()), Codec::Exact(string)],
                    Codec::Exact(Option::<u64>::kagari_type(preparation.catalog())?),
                    search::find,
                ))
            }),
    )?;
    Ok(())
})?;
```

An explicit canonical reference is declaration resolution, not implementation
discovery. Reference requests record expected category and applied arguments;
sealing validates actual existence, category, visibility, arity and bounds, and
computes dependency edges. Option need not precede String because drafts do not
consult a fabricated complete provider. Forward member/default references use the
same scoped identity discipline.

Vec's owner similarly completes its nominal type and calls
`module.bind_storage(&vec_handle, NativeStorage::sequence(0))?`. Future and task
owners supply their current descriptors locally. The top-level assembler contains
no corresponding module-identity switch. In declaration-only use these storage
associations remain pending; no runtime storage installation occurs.

HashMap/HashSet currently use constructor-backed Map/Set representations, not Vec's
nominal `bind_storage` model. Move their constructor metadata and callbacks without
turning this registration refactor into NC. Managed enums are a third category.

SR01 separates trait/default/override semantic intents from pending adapters and
uses shared default lowering during sealing; stdlib contributors have not yet
migrated to this path. Preserve hidden associated arguments, applied trait handles,
method-local generics, callable-slot order and concrete-result evidence. For a
default factory, `preparation.declaration()` is the private lowered template,
whereas `NativeAdapter::prepare` receives the original authored method signature.
Template requirement tokens must be paired with
`NativeBinding::declared(preparation.declaration(), callback)` to retain their
owner; the sealed view is not permission to reuse another target's tokens.

### Complete definition with an interface result

The existing String.split shape demonstrates why authoring facts remain distinct
without requiring three separate registration calls:
[`catalog/strings.rs`](../crates/kagari-stdlib/src/catalog/strings.rs) declares
`List<String>` and records concrete `Vec<String>` evidence;
[`bindings/strings.rs`](../crates/kagari-stdlib/src/bindings/strings.rs) selects
`method::<Vec<String>>` with two exact String inputs and an exact Vec result.
**Proposal:** preserve that shape inside the migrated String recipe as follows:

```text
let string = Type::scalar(BuiltinType::String);
let list = module.reference_trait(identity::trait_id("List"), 1)?;
let vector = module.reference_type(
    identity::type_id("Vec", DefinitionKind::AssociatedType),
    TypeDefKind::NativeStorage(NativeStorageLayout::Sequence { element: 0 }),
    1,
)?;
let public_result = list.apply([string.clone()])?;
let concrete_result = vector.apply([string.clone()])?;

module.inherent(string.clone(), |methods| {
    methods.define(
        MethodDecl::instance("split")
            .parameter("separator", string.clone())
            .returns(public_result)
            .produces(concrete_result)
            .documentation(
                "Split by a literal separator, preserving endpoint empties. \
                 An empty separator splits at Unicode scalar boundaries with endpoint empties.",
            )
            .implementation_with_send(|preparation: &SealedPreparation<'_>| {
                let string = Type::scalar(BuiltinType::String);
                Ok(NativeBinding::new(
                    vec![Codec::Exact(string.clone()), Codec::Exact(string)],
                    Codec::Exact(Vec::<String>::kagari_type(preparation.catalog())?),
                    transform::split,
                ))
            }),
    )?;
    Ok(())
})?;
```

This is one `methods.define` submission, not one undifferentiated execution phase:

1. **Author and submit:** the definition contains the instance signature, public
   result, concrete-result fact, documentation and one pending Send factory.
   Submission does not call that factory. `produces` is optional for simple results
   such as Bool, but records the Vec evidence needed behind this List result.
2. **Seal:** `set.finish()` resolves List/Vec references and validates interface
   conformance and concrete-result evidence against the complete module set.
   `assembly.declarations()` exposes these facts without running any factory.
3. **Construct and prepare:** `into_modules()` invokes the factory once with the
   exact sealed target and catalog. Here the factory uses `preparation.catalog()`
   to construct the exact Vec codec; it does not create a Vec value. The returned
   `NativeBinding` immediately passes through `NativeAdapter::prepare` against the
   sealed contract, checking the two input codecs and concrete result agreement.
   Neither the factory nor prepare may add or repair semantic facts. There is no
   recursive call to `StandardDeclarations` and no engine installation here.
4. **Invoke later:** each script split call still takes scoped views of both String
   arguments, polls cancellation while collecting owned Rust strings, handles
   `try_reserve` failure, drops the views, and calls `cx.encode_result(parts)`.
   Runtime conversion/heap allocation and the linked Vec-to-List adaptation remain
   execution work, not factory work. The factory does not rerun per invocation.

Thus co-locating declaration and implementation does not erase their agreement
checks, runtime borrowing, cancellation or allocation costs. It also does not prove
all contextual owned-adapter Rust mappings at registration: those adapters retain
invocation-time checks. `transform::split` is the proposed location of the existing
callback, not a new implemented helper or a flat zero-argument adapter.

### Standard-library composition and consumer entrypoints

**Proposal:** the aggregate recipe lists modules, not individual functions. The
`assemble` helper and contributor `definition` functions below are proposed stdlib
wiring, not existing entrypoints. Only three contributors are shown; this is not a
closed executable foundation. SR04 must include the full SR01 inventory:

```text
fn assemble(external_providers: &DeclarationCatalog) -> NativeResult<ModuleAssembly> {
    let mut set = ModuleSetBuilder::new(external_providers)?;
    set.add(string::definition())?;
    set.add(option::definition())?;
    set.add(vec::definition())?;
    set.finish()
}
```

For a complete self-contained contributor set, the bundle recipe would call
`assemble(&DeclarationCatalog::default())`; external providers are explicit, never
an implicit standard-library catalog. Bootstrapping must not call
`StandardDeclarations` from inside its own assembly.

The SR01 [shared bundle](../crates/kagari-stdlib/src/assembly.rs) already implements
`Bundle::new(name, recipe)`, `semantic()` and `modules()`, but
[`lib.rs`](../crates/kagari-stdlib/src/lib.rs) currently enables it only under
`cfg(test)`. Production `kagari_stdlib::modules()` still delegates to
`bindings::modules()`, and the existing declaration facade has not switched to the
assembly projection. SR02-SR04 own contributor/entrypoint migration; SR05 owns
removal of the old independent caches.

The proposed shared wiring uses one bundle, not a fresh assembly per `modules()`
call. Collection consumes its recipe once, seals, then calls `try_into_send()`
without preparing adapters. `semantic()` retains only checked semantic products;
`modules()` consumes pending preparations once and caches either complete modules
or failure as `NativeResult<Arc<[NativeModule]>>`. Shared recipes therefore use
`implementation_send` / `implementation_with_send`; ordinary local attachment is
not implicitly upgraded. A global bundle requires its retained recipe to be Send,
not Sync. Recipes may still create thread-local authoring state before transfer.

The migration target is to back the declaration-facing facade with the snapshot
projection rather than another inventory. Source tooling must not retain pending
Rust adapters. Consuming `into_modules` rejects missing/invalid bindings before
returning modules; it does not allocate a script heap or install into an engine.

Engine construction retains its existing single batch operation:

```text
// Inside default engine construction; not an additional user registration step.
builder.install_all(kagari_stdlib::modules()?)?;
```

Low-level runtimes continue to use the existing `NativeModule::install_all` path.
There is no required new `engine.register_string` or forwarding-only
`StandardLibrary` wrapper. Ordinary third-party modules retain the existing
`ModuleBuilder` path; multi-module libraries may use the same staging capability.

### Observable API acceptance

These remain acceptance obligations, not a claim that the proposal snippets compile
or that stdlib migration is complete. Current runtime assembly tests and stdlib
bundle tests exercise the implemented mechanisms; their actual validation status
belongs to the phase ledger. The authoring contract must demonstrate:

- Submit complete method definitions with direct callback/primitive adapters, and
  a split-style interface result with produces plus deferred preparation, without
  requiring separate declare/configure/bind calls.
- Bootstrap String's constructor ownership from empty external providers.
- Complete reciprocal trait/type/implementation references independently of module
  order, including a generic default and an override with concrete-result facts.
- Prove a List hidden-iterator default and a distinct paired Iterable argument
  mapping; compare declaration projections with and without adapter preparation.
- Read checked declarations without preparing adapters or installing storage.
- Reject unresolved references, duplicate/foreign handles, mismatched bindings
  and incomplete executable products at the designated phase.
- Compose a library through one module list, without a global method registry or
  a second implementation-name table.

## Deeper boundary review and decisions

This review supersedes the initial assumption that directory regrouping plus a
reserve/define wrapper around existing builders would be sufficient. Findings are
from static source inspection, not new runtime reproductions. They refine the
existing scope; they do not activate NC, compiler redesign or the host facade.

### Ownership is not the same as filesystem layout

One canonical module recipe composes multiple ordinary Rust contributors. Preserve
these distinct owners instead of moving public declarations to match directories:

| Concern | Example / required owner |
| --- | --- |
| Trait declaration | Default belongs to core::default |
| Receiver implementation | Default for String belongs to alloc::string |
| Default template | Owned by its declaring trait/module; applications retain explicit substitutions |
| Concrete override | Owned by the implementation module, pointing at its checked callable target |
| Shared Rust algorithm | A meaningful library helper, reusable without sharing semantic identity |
| Facade/prelude export | std facade declares an alias of the canonical target, not another declaration |

The module recipes for core::ops, core::cmp, core::iter and std::collections each
compose several semantic contributors. Duplicate canonical owners remain errors;
calling a contributor on an existing draft is not another module registration.
Constructor-backed records (String/Map/Set/ranges/Ref/RefMut/cursors), nominal
registered storage (Vec/Future/tasks/adapters), and managed enums must be inventoried
separately. Their different validation/representation rules are not accidental
registration boilerplate.

### Four identities must not be collapsed

[Existing native declaration assembly](../crates/kagari-stdlib/src/bindings.rs)
deduplicates by implementation ID, while public declaration IDs can differ. The
[new authoring shorthand](#proposed-authoring-api) must preserve this distinction:

1. A declared trait/impl member identifies the script-visible callable contract.
2. A generic default template has its own identity and explicit applied arguments.
3. A native binding target identifies a checked executable registration and its
   callable requirements; it may serve compatible explicit declarations.
4. A Rust function pointer is an implementation detail, not a semantic identity.

Ordinary new methods may use a per-declaration binding target. Reusing a Rust
algorithm does not automatically merge targets, nor require cloning its semantic
implementation. Existing shared scalar bindings must be deliberately preserved
through a scoped shared-target handle or replaced with separate checked bindings
without losing applied identity/requirements. SR01 records that finite choice
before migration; do not add a global target registry or deduplicate by pointer.

Default application identity/substitution is not optional optimization. Preserve
it through the shared semantic lowering. In particular,
[List default lowering](../crates/kagari-stdlib/src/catalog/list_methods.rs) and
[Iterable default lowering](../crates/kagari-stdlib/src/catalog/iterable_defaults.rs)
carry hidden associated iterator/item arguments and concrete-result contracts that
[generic default lowering](../crates/kagari-runtime/src/native/builder/trait_builder/defaults.rs)
does not currently express through the simple builder sketch. SR01 must unify the
required capability rather than leave a second stdlib-only lowering algorithm.

### Define what is actually checked at each boundary

[DefinitionMetadata](../crates/kagari-common/src/identity/metadata.rs) checks scoped
references; it is not proof that every referenced declaration exists or that all
semantic contracts are valid. A catalog index is not a substitute for the complete
module records: docs, exports and prelude facts must remain available.

| Boundary | Guarantees required before returning |
| --- | --- |
| Draft authoring | Valid local mutation handle, no duplicate definition/association, explicit expected reference category |
| Semantic sealing | All declarations fulfilled; complete type/trait/default/impl and conversion metadata; valid bounds/requirements/concrete results; existing accessible export targets; canonical owners and complete dependency edges |
| Native module preparation | Existing registration-time codec/receiver/access checks appropriate to each adapter category; complete native targets/storage; no semantic mutation after sealing, no blanket proof of future dynamic conversions |
| Installation | Existing duplicate/conflict checks across the actual installed module set, required-provider closure, host-map preparation and atomic publication |
| Runtime load/call | Artifact/ABI/ownership/generation/access checks remain in their existing owners |

Factor shared semantic finalization from current catalog/module checks, including
export-target and dependency validation now performed in
[NativeModule::checked](../crates/kagari-runtime/src/native/module.rs). Do not
publish a weaker "checked declarations" product and hope execution preparation
will catch semantic defects later. Preserve external-provider contracts as part
of the sealed snapshot; providers are not locally redefined, even identically.

Do not add a throwaway NativeInstallation pass inside `into_modules`. The existing
[installation preparation](../crates/kagari-runtime/src/native/installation.rs)
already builds reusable registry and host maps. Rechecking at a genuinely different
trust boundary remains mandatory; repeated preparation is not validation evidence.
Diagnostics must identify canonical module/member, failing phase and conflicting
provider or expected/actual contract. Do not report a bare pending-list index.

### Execution cost is a separate boundary

A concise authoring API must lower to existing execution mechanisms, not become
another runtime interpreter for method definitions. `catalog` is a preparation
input and Codec is a binding contract; neither implies an unconditional runtime
catalog lookup or full codec comparison for every call. Nor does prepared
registration prove that all invocation checks have disappeared.

Current code-derived distinctions, not timing measurements:

- [Call-site linking](../crates/kagari-runtime/src/module/linked_execution/native_calls.rs)
  checks native arity and prepares operand locations, selecting primitive or
  callback bodies. [Linked invocation](../crates/kagari-runtime/src/native/binding.rs)
  still checks arity and performs cancellation/access/result handling for callbacks.
- String contains uses exact raw codecs and scoped string access, not the owned
  input conversion pipeline. Its per-call tag/heap checks are distinct from the
  registration-time codec-to-signature comparison.
- [Owned invocation](../crates/kagari-runtime/src/native/typed.rs) and
  [argument conversion](../crates/kagari-runtime/src/native/conversion/arguments.rs)
  repeat mapping/arity/value work; split also uses composite output conversion.
  These existing costs are tracked by the call-boundary follow-up, not declared
  fixed by relocating registrations.

SR's execution contract is:

1. Complete-definition wrappers and adapter factories exist only in authoring or
   preparation. Factory execution, declaration discovery, catalog reconstruction
   and registration locks must not be added to the per-call path.
2. Preserve primitive authority and linked selection, raw scoped views, flat typed
   adapters and owned contextual conversion as distinct data paths behind the same
   registration API. Do not route contains through owned String copies or turn len
   into a callback merely to fit a uniform wrapper.
3. Do not add generic per-call signature/codec walks, argument containers, repeated
   conversion layers or dynamic implementation-name dispatch through new helpers.
   Preserve existing allocation contracts; a similar bytecode instruction count
   does not establish this requirement.
4. Do not globally remove checks, mark arbitrary callbacks as trusted/converted or
   cache user conversion outcomes to make SR pass performance checks. Any relocated
   proof needs an owner, concrete type/application and provider/generation lifetime,
   and a validated invocation path allowed to consume it. That optimization needs
   explicit scope and tests; dynamic GC/ownership/borrow/result checks still apply.

Lowering a fluent definition to the existing registration representation should
leave per-call behavior unchanged. Prove this for the finite representative set
below; do not promise faster script execution as an outcome of SR.

### Binding ownership and cache lifecycle

[NativeAdapter::prepare](../crates/kagari-runtime/src/native/adapter.rs) consumes its
adapter and does not promise Clone/Send/Sync. Therefore a generic draft assembly is
one-shot, with independently owned immutable declarations and separately owned
pending adapters. It is not automatically Clone, globally cached or replayable.
Do not globally strengthen existing immediate adapter bounds to accommodate a
cache; deferred storage needs explicit lifetime bounds on that operation alone.

The stdlib-specific sharing target is one bundle owner with:

- An immutable semantic snapshot usable immediately by declaration-only consumers.
- A one-time pending-binding preparation cell, producing cached immutable modules
  on first executable use; it consumes adapters once, not once per runtime.
- Acyclic construction: module recipes never call StandardDeclarations to obtain
  the foundation they are currently building. Callback preparation uses the sealed
  catalog supplied to it, not a hidden global foundation lookup.

Stdlib's own entries can meet the sharing constraints without imposing them on all
third-party assemblies. SR01 must prove the concrete synchronization/ownership
implementation and reentrancy/error behavior; do not hold a preparation lock while
recursively requesting the same bundle. A failed preparation publishes no partial
module list and cannot fall back to the old binder. Declaration consumers retain
only their immutable projection, not pending callbacks. Each installed runtime
still has independent heap, registry/borrow state and roots.

The existing immutable enum-handle paths in
[bindings/enums.rs](../crates/kagari-stdlib/src/bindings/enums.rs) and
[declarations.rs](../crates/kagari-stdlib/src/declarations.rs) need an explicit owner
in this cache graph. Reuse prepared symbolic handles from the same snapshot or
existing checked call metadata, not a second independently constructed catalog.
Actual enum allocation must still use runtime-local generation-pinned layouts;
never cache a runtime-local object/layout globally to simplify registration.

### Classify string-based policy instead of banning strings

SR01 records this finite classification and migrates the relevant owner once:

| Mechanism | Decision |
| --- | --- |
| Private implementation-name matches and prefix-based partitioning | Remove; direct handles and explicit module owners replace them |
| Global scans selecting overrides, callable requirements or concrete results by method name | Replace with configuration at the owning method/implementation definition |
| Documentation switches selecting text by module/trait spelling | Move authored text beside definitions; retain generic rendering and inheritance utilities |
| Public canonical identity descriptors and explicit member/variant selection | Retain checked lookup; do not replace with unchecked numeric constants |
| Exact reserved language-role identity recognition in types/HIR | Preserve current mechanism and origin/shape checks; no new role-assignment model |
| Constructor/representation recognition required by generic execution | Preserve existing validated semantics; do not broaden into NC |
| Runtime enum/declaration caches | Consolidate their ownership/lifetime as above; retain generation-local allocation checks |

The current renderer recognizes language roles from canonical protocol identity;
ModuleDecl does not own a new role-assignment table. "Module-owned roles" means
owning the relevant declarations, not silently redesigning compiler authority.
Conversely, Into/TryInto/TryFrom conversion adapters are ordinary semantic metadata
and must have an explicit supported authoring path, not be forgotten because they
are neither callbacks nor reserved language roles.

### Completion evidence, not only successful reconstruction

Before deleting the old producer, perform one bounded normalized old/new semantic
comparison of the complete standard-library declaration inventory. Compare owners,
exports/prelude, constructor/storage categories, signatures/bounds, conversion
adapters, default applications, callable requirements, concrete results and docs.
Normalize scoped IDs and deliberately replaced internal symbols. Preserve
association topology unless SR01 explicitly chooses to split a shared binding
target; normalize that specific transformation while still comparing all applied
arguments, requirements and declaration contracts. A repeated build of the new
producer alone cannot detect a consistently omitted declaration.

Capture the normalized baseline before replacing the first contributor, or pin a
reproducible pre-migration producer revision with its exact extraction procedure.
SR05 cannot depend on old contributors that earlier phases have already deleted.
This is temporary migration evidence, not a permanent snapshot product or a
full-workspace test sweep. Keep temporary captures under ignored target storage;
record the reproducible method, producer revision and actual conclusions here. Delete
the comparison bridge at SR05 and keep behavioral/rejection tests at their owners.
Every intentional public semantic difference requires separate authorization;
internal identity changes require an explicit normalized mapping.

## Reference mechanism and applicability

SR01 uses the following bounded mechanism comparison, not one borrowed framework.
The version pins identify inspected references, not dependencies to add to Kagari.

| SR01 decision | Primary reference | Adopt | Do not assume |
| --- | --- | --- | --- |
| Module-local implementation ownership | Lua 5.4.8; Rune 0.14.1 | Module owns explicit implementation associations; top-level composition lists modules | Dynamic registration establishes static generic contracts |
| Complete declarations before dependent checking | Rust 1.90.0 hir_analysis | Collect item contracts before implementation/default checking | Identity reservation alone supplies a complete trait; rustc queries are required |
| Explicit native association | Java SE 21 JNI RegisterNatives | Associate a declared member/signature with a direct native entry at registration | An untyped native function pointer has been ABI-verified |
| Retained preparation evidence and lifetime | Wasmtime 38.0.0 InstancePre / TypedFunc | Resolve/check before use; retain the result under explicit ownership constraints | All runtime checks disappear; evidence is valid in any runtime or generation |

Consulted primary source: Lua **5.4.8**, [lstrlib.c](https://github.com/lua/lua/blob/v5.4.8/lstrlib.c)
and [linit.c](https://github.com/lua/lua/blob/v5.4.8/linit.c).
The string library owns its implementation functions and `luaL_Reg` table;
`luaopen_string` publishes that library, and `luaL_openlibs` composes library
entrypoints rather than individual string functions.

Adopt local registration ownership and top-level composition. Do not adopt Lua's
mutable dynamic table as Kagari's type/ABI authority. Lua's state-bound publication
does not supply Kagari's static generic contracts, source-free artifact checks,
scoped IDs or generation-pinned dependencies. Kagari therefore needs explicit
staged declarations plus checked local bindings, extending its existing native
builders rather than copying Lua's execution model. This is a maintainability and
correctness design; no execution speedup is inferred from the comparison.

For staged semantic finalization, also consulted Rust **1.90.0**
[rustc_hir_analysis](https://github.com/rust-lang/rust/blob/1.90.0/compiler/rustc_hir_analysis/src/lib.rs):
its documented separation of item collection, coherence and body checking supports
making declaration contracts available before dependent checks. It does not justify
copying rustc's query engine, borrow model or compilation pipeline. Kagari's drafts
are native registration inputs, with explicit runtime identities and installation
boundaries; reuse existing declaration validation rather than add source analysis.

### Rune: local metadata and handler ownership

Rune **0.14.1** primary sources:
[Module](https://docs.rs/crate/rune/0.14.1/source/src/module/module.rs),
[function metadata](https://docs.rs/rune/0.14.1/src/rune/function_meta.rs.html),
[Context](https://docs.rs/crate/rune/0.14.1/source/src/compile/context.rs) and
[signature metadata](https://docs.rs/crate/rune/0.14.1/source/src/compile/meta.rs).
`Module::function_meta` collects documentation/function metadata and a direct
shared handler in module records; `Context::install` consumes those records and
`Context::runtime` projects runtime data. This supports complete local definitions
and separating authoring records from an execution product, without rediscovering
implementations through private symbol spelling.

The limits are material: Rune's argument/result descriptions are Rust-derived and
some are doc-feature metadata, not Kagari's explicit static signature authority.
Installation visits types before associated items, but provider module order still
matters, and installation is not Kagari's atomic batch transaction. Build-time
Context retains callbacks. Trait installation wires handlers/default fallbacks;
it does not prove Kagari's hidden associated-argument/default-template lowering or
reciprocal static contract completion. Adopt local ownership, not its hash identity,
macro authority, order dependence or mutable installation lifecycle.

### JNI: explicit declaration-to-entry association

Java SE **21** [RegisterNatives specification](https://docs.oracle.com/en/java/javase/21/docs/specs/jni/functions.html#registernatives)
registers a class-local array of name, signature and function pointer records.
It rejects absent/non-native methods. This is a precedent for explicit native
associations against existing declarations, not name-based discovery at each call.
Java declarations remain separate from the native implementation records.

JNI's void function pointer does not prove C ABI/signature correctness; native
code must meet that contract. JNI local/global references and thread-bound JNIEnv
also have different lifetimes from Kagari's scoped host borrows and pinned reload.
Use the separation of declared contract and native entry, not JNI's unsafe-pointer
assumptions, mutable rebinding or manual reference API. No JNI performance result
is evidence for Kagari's callback cost.

### Wasmtime: prepared evidence is owned, not universal

Wasmtime **38.0.0** primary documentation:
[InstancePre](https://docs.rs/wasmtime/38.0.0/wasmtime/struct.InstancePre.html) and
[TypedFunc](https://docs.rs/wasmtime/38.0.0/wasmtime/struct.TypedFunc.html).
`Linker::instantiate_pre` resolves imports and performs the relevant type checking
before producing InstancePre; later instantiation reuses those resolved imports.
It can be store-independent only when its captured imports are store-independent;
wrong Store/Engine use remains invalid. `Func::typed` establishes a checked
parameter/result relation represented by TypedFunc rather than treating a raw
function handle as evidence of any requested signature.

Apply this principle to SR01's sealed declarations and existing prepared native
contracts: identify the exact provider/application scope and retain valid facts,
without repeating authoring at invocation. Do not literally introduce InstancePre
or TypedFunc clones into Kagari: NativeInstallation and linked call/signature
records already own relevant boundaries. Wasmtime's Wasm ABI/Store model does not
supply Kagari's nominal trait defaults, conversion semantics, GC borrowing or
hot-reload guarantees. Never use unchecked typed construction as a shortcut.
This reference informs the preparation boundary; optimizing existing invocation
checks remains the separately queued follow-up.

### Why Kagari still needs a local synthesis

None of these references establishes the complete combination of explicit Kagari
signatures, associated-output/default-template contracts, declaration-only tooling,
source-free validation and generation-pinned native dependencies. Reuse module
ownership, semantic collection and scoped preparation evidence through Kagari's
existing models. Do not invent a plugin framework, global runtime identity or a
second semantic implementation to imitate an upstream API.

SR01 records how each unresolved reference/identity/default/cache choice uses one
of these mechanisms or why it differs. The representative proofs in SR01 remain
required: citations do not establish order independence, safe lifetime reuse or
correct implementation in this repository. This research adds no migration phase,
benchmark target or requirement to integrate another runtime.

## Preserved contracts

- Explicit Kagari signatures remain authoritative. Rust adapter codecs, receiver
  policies, native storage shapes and callable requirements remain checked.
- Required language roles remain unique, complete and origin/shape validated;
  same-named application traits cannot acquire reserved authority.
- Public paths, visibility, aliases, prelude, generated docs/navigation, generic
  bounds, associated outputs and concrete-result evidence retain their meaning.
- Preserve checked numeric behavior, evaluation/trap order, mutation commit points,
  cancellation polling and cleanup on errors, cancellation and host reentry.
- Preserve GC tracing/rooting, scoped string/host borrows, runtime ownership and
  generation-pinned calls/dependencies. Shared module definitions contain no
  per-runtime heap, mutable registry or captured session roots.
- Preserve ordinary third-party module registration and source-free execution.
  New generic authoring machinery must not import stdlib into runtime/HIR/compiler.
- Internal implementation identities may be replaced coherently, without aliases
  for unpublished artifacts. Regenerate affected development fixtures at coherent
  checkpoints; preserve meaningful source-free and reload tests. Public canonical
  identity and aliases are not disposable implementation details.

## Execution phases

Phases are ordered SR01 through SR06. String is an early representative migration,
not the completion boundary. Every phase updates the ledger below.

### SR01 — Establish explicit module-owned assembly

- [x] Re-audit all catalog/binding/partition/storage contributors and consumers;
  record a finite owner-to-owner migration inventory in this plan.
- [x] Audit declaration-only consumers, constructor/conversion metadata, default
  template capabilities, routing classification and the enum/cache owner graph.
- [x] Capture the pre-migration normalized declaration baseline and representative
  execution-path evidence, or pin reproducible producers/extraction procedures,
  before replacing contributors.
- [x] Resolve the user-confirmed single-submission API into compiling callback,
  primitive and split-style concrete-result examples; document exact signatures,
  identity lifecycle, deferred preparation, phase errors and sharing rules.
- [x] Implement the minimum shared capability through existing native builders.
- [x] Prove empty-provider constructor ownership, reciprocal trait/impl references,
  the chosen scalar binding-target model, List hidden-iterator defaults and distinct
  paired Iterable mappings, including identical sealed declarations without adapter
  preparation.
- [x] Prove unresolved/foreign/duplicate handle rejection, missing export rejection,
  signature mismatch and atomic failure; record shared-target identity choices.

Exit: the API/lifecycle proofs above pass before SR02 begins. Record finite choices
for draft-reference representation, default-template lowering, shared-target
identity and one-shot/cache ownership here. An unresolved required capability is
an SR01 blocker, not permission to add stdlib bypasses during bulk migration.
Representative proofs are not permission to migrate every module in SR01. Any
temporary integration bridge has a named SR05 removal point.

### SR02 — Migrate String and simple foundational modules

- [x] Move String constructor metadata/signatures/docs/callbacks/primitive
  associations into its owner.
- [x] Migrate scalar, array/range and enum/propagation contributors where applicable,
  including their construction, comparison and Default implementations.
- [x] Bind implementations directly through checked handles and explicit owners;
  remove the migrated entries from old name-routing paths immediately.
- [x] Preserve String's Option/List/Vec references and compare len/contains/split
  across bytecode, linked body and runtime adapter categories; retain the primitive
  length path and scoped string inputs.

Exit: adding a String method changes only the String owner/implementation and
relevant behavioral tests, not a global dispatch or partition table. Existing
numeric and propagation semantics remain intact.

### SR03 — Migrate interfaces, defaults and collections

- [x] Migrate remaining standard traits, generic construction/comparison/hash
  helpers, collection protocols and default bodies.
- [x] Migrate Vec's nominal storage and methods, and HashMap/HashSet constructor
  metadata and methods, to their owners without changing representation categories.
- [x] Migrate Iterable, lazy adapters, cursors and optimized overrides with explicit
  dependencies, requirements and concrete-result facts.
- [x] Remove superseded central dispatch/storage cases without changing collection
  algorithms, storage representation or cursor revision/borrow contracts.

Exit: generic/default/override associations are explicit and share existing checked
semantics; no duplicate implementation is introduced for the new registration path.
NC remains independently queued.

### SR04 — Complete module ownership and declaration projections

- [x] Migrate Future/task/scope storage and callbacks and any remaining contributors
  identified in SR01; every inventoried entry has a final owner.
- [x] Consolidate module docs, conversion metadata, aliases and prelude under
  explicit definitions; preserve canonical language-role recognition and retain
  generic rendering/documentation inheritance utilities.
- [x] Feed declaration-only consumers from the same authored definitions without
  preparing runtime bindings or rendering/parsing source during registration.
- [x] Verify representative generated source navigation and default engine
  installation against the assembled canonical module set.

Exit: the full standard-library inventory has migrated, not merely String and Vec.
Top-level composition contains no per-method or per-storage implementation policy.

### SR05 — Cut over consumers and delete obsolete assembly

- [ ] Switch all production stdlib consumers to the new assembly products and
  remove temporary bridges, duplicate caches and legacy recipe entrypoints.
- [ ] Complete the normalized old/new semantic inventory comparison and the bounded
  execution-path comparison, recording allowed internal identity mappings, public
  contract equivalence and any changes in per-call work.
- [ ] Delete the transient foundation assembly/name-inferred partition mechanism,
  callback discovery matches, temporary comparison bridge and obsolete private-name
  dependencies in tests.
- [ ] Retain semantic ownership/binding assertions when replacing tests that assert
  incidental `$foundation_*` spellings; do not weaken rejection/behavior coverage.
- [ ] Review helpers, visibility, imports/re-exports, scope remapping and effective
  LOC; remove forwarding-only modules introduced by file moves.
- [ ] Batch regeneration of affected disposable artifacts and execute selected
  source-free, host-composition and generation-pinned boundary cases.

Exit: one production authoring/assembly path remains. No migration fallback or
compatibility alias is retained. Internal symbols that remain are identity data,
not an implementation-discovery convention.

### SR06 — Final local acceptance and contract handoff

- [ ] Complete selected behavioral/boundary and execution non-regression evidence;
  resolve all carried implementation errors and SR-introduced regressions. Record
  unchanged known call costs separately; do not repeat unaffected passing checks.
- [ ] Run repository structural checks, formatting, workspace all-target Clippy
  and diff checks; record actual results and remaining CI scope separately.
- [ ] Update architecture and native-registration documentation with final owners,
  lifecycle and a practical module-authoring example using real APIs.
- [ ] Move unresolved work/CI acceptance to the roadmap or review and retire this
  completed plan and its index links. Git history retains the phase ledger.

Exit: all required local gates pass and lasting documentation describes the real
implementation. Full CI acceptance is not inferred from local focused results.

## Migration and build policy

Prefer buildable coherent checkpoints, but a tightly bounded replacement may carry
an intermediate error only if its exact command, representative diagnostic, cause
and repairing phase are recorded here. Do not disable validation or add production
stubs to make an intermediate build appear successful. Attempt affected checks at
architecture boundaries, and run selected tests when their units build. Do not
repeat unchanged failures before their owning repair phase.

A temporary old/new coexistence is allowed only for unmigrated owners during
SR01-SR04. Each declaration has one active producer and implementation owner;
there is no fallback from a failed new binding to old string matching. SR05 deletes
all bridges before acceptance. Do not preserve old internal APIs/readers for
hypothetical compatibility or routinely bump ABI identifiers.

Refresh status and relevant diffs at activation; preserve concurrent user work.
Coordinate changes to shared builder/identity/storage APIs with NC or host tasks
without silently activating those plans. Commits require user authorization;
authorized implementation checkpoints use `Plan-Phase: SRxx` trailers and describe
breaking authoring API impacts or carried build failures when applicable.

## Validation strategy

Reuse existing contract owners, including stdlib
[registration tests](../crates/kagari-stdlib/src/tests/registration.rs),
[documentation tests](../crates/kagari-stdlib/src/tests/documentation.rs) and
[runtime-boundary module](../crates/kagari-stdlib/src/tests/runtime_boundary/mod.rs),
plus relevant runtime builder, HIR, SDK and artifact tests. Resolve exact current
targets/test names during SR01; do not invent command filters or assume renamed
tests ran. Record the selected commands and rationale before each checkpoint.

| Changed boundary | Focused local evidence |
| --- | --- |
| Authoring/assembly | Single scoped draft owner, constructor bootstrapping, direct binding, missing export/reference and duplicate/foreign rejection, wrong codec/storage rejection, reciprocal trait/impl providers |
| Public declaration product | Canonical paths/re-exports/prelude, representative generic/default/role contracts, docs and navigation; compare normalized semantic records rather than raw scoped IDs |
| String | Callback contains/find, primitive len, UTF-8 slice rejection, split's declared/concrete result and representative cancellation behavior; preserve linked primitive/raw callback categories |
| Generic modules | Chosen scalar binding-target model with its preserved contracts, hidden-iterator List default, distinct paired Iterable mapping, concrete-result override, lazy cursor and mutation/trap-order regression |
| Storage/lifecycle | Representative Vec and Future/task registrations, GC cleanup and per-runtime isolation using existing contract tests |
| Consumers | Default engine plus application composition; equivalent semantic sealing without adapters; actual cold source-free execution; no cache recursion or shared runtime state |
| Reload | An existing pinned native/library dependency case demonstrating old-generation calls remain valid |

Add tests only for distinct uncovered contracts, especially staging and identity
failures; do not duplicate every method's existing behavioral suite. A successful
module build is not source-free execution evidence. A source-enabled test does
not prove compiler-free dependency isolation. Preserve a real no-source consumer
and inspect its feature/dependency boundary using existing project tooling.

### Bounded execution-path comparison

Use len, contains and split as three source-level probes: primitive dispatch,
scoped raw callback, and allocating concrete-to-interface result. Add one existing
flat scalar and one existing owned/contextual adapter case to cover categories
not represented by those strings; reuse tests/workloads rather than invent a new
benchmark framework. SR01 records exact functions, inputs, commands and revisions.

For each case compare these three layers before/after:

| Layer | Inspect | Does not establish |
| --- | --- | --- |
| Bytecode and native import contracts | Callee identity, arguments, copies/boxing/conversions, concrete result and requirements | The Rust work hidden behind one native call |
| Linked execution record | Primitive versus callback, pinned target, prepared argument locations and result adapter | Absence of checks/allocations within the callback or converter |
| Runtime implementation | Per-call signature work, decoding, roots/allocations, result adaptation and factory invocations | Timing cost without measurement |

Inspect semantically normalized results, not byte-for-byte equality of scoped
ordinals. Include a genuinely source-free load for the relevant contract evidence;
source compilation alone cannot prove link-time validation. Existing CLI exposes
artifact emission but no dedicated bytecode-disassembly command was found during
planning. A targeted test or temporary probe is enough; do not add a public dump
command or disassembler project to SR.

Reuse existing [native allocation contracts](../crates/kagari-vm/tests/allocation_contracts/native_allocations.rs)
for directly affected scalar/scoped-view paths. Do not claim general owned adapters
or split are allocation-free. For unchanged execution code and equivalent linked
representations, structural evidence plus affected tests is sufficient; no new
blanket benchmark gate is required at every phase.

If SR changes adapter category, code executed per call, conversion/root placement
or interface-result preparation, perform bounded before/after release measurements
for those cases in addition to correctness checks. Keep compilation, installation,
first-use and warm-call timing separate; consume results, avoid constant-folded
probes, and use identical inputs/semantics. Record allocations where affected and
revision, toolchain, machine, profile/features, parallelism, cache state and run
variability. Diagnose reproducible regressions before acceptance; do not invent a
universal percentage threshold or infer speed from instruction count alone.

Temporary outputs belong under ignored target storage; record conclusions and
reproduction commands in this ledger. Existing repeated-call costs remain with
the linked review owner. This finite comparison is an SR non-regression obligation,
not authorization to implement the broader call-boundary optimization queue.

Routine iteration runs a small affected test selection. Final local acceptance
adds only a small directly affected regression set, not a per-crate sweep. Full
workspace tests and exhaustive feature/backend/platform validation are CI-only.
At implementation checkpoints run:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Use workspace profiles, default target/parallelism and bounded command timeouts.
A timeout or early build failure is not a passed selection. Re-run failed/affected
cases after batched repairs; retain valid evidence for unchanged code. If setup
performance becomes a concrete concern, measure preparation separately with the
required environment/workload metadata; this plan promises no unmeasured speedup.
Documentation-only planning requires content/link/diff checks, not Rust builds.

## Progress ledger

| Phase | Scope status | Validation / carried errors |
| --- | --- | --- |
| SR01 | Locally complete: shared assembly, semantic lowering, identity and lifecycle proofs | Focused assembly/default/cache tests and local gates pass; baseline captured; independent exit review closed; CI matrix unrun |
| SR02 | Locally complete: String/numeric/array/range/enum/propagation owners and production Bundle | Selected registration/docs/behavior/artifact tests and execution comparison pass; exact target assertions preserved; CI matrix unrun |
| SR03 | Locally complete: protocols, defaults, Vec/hash/collection/iteration owners | 30 distinct focused tests passed; shared default fix verified; local gates and independent review pass; CI unrun |
| SR04 | Locally complete: Future/tasks, owner-authored prose, declaration/navigation projections | Selected task/lifecycle/docs/registration tests and local gates passed; review doc-routing issue fixed; CI unrun |
| SR05 | Not started | Depends on SR04; owns bridge removal |
| SR06 | Not started | Depends on SR05; full matrix remains CI-owned |

Planning baseline: inspected declaration/binding/partition code, generic builders,
identity snapshots, default templates, adapter contracts, module/installation
validation, registration tests, architecture and primary Lua/Rust references.
The deeper review replaced public reserve/define passes with one draft recipe and
internal semantic finalization, corrected constructor/raw-adapter assumptions,
and moved complex default/identity/cache proofs into SR01. The user confirmed
single-submission fluent definitions. Subsequent call-path inspection added a
bounded bytecode/link/runtime non-regression contract and explicitly left existing
invocation-cost optimizations with their review owner. The initial document review did not establish build health. Implementation evidence
and the finite migration inventory follow; neither expands this plan into unrelated
API redesign.

### SR01 activation and finite migration inventory

Authorized producer baseline: `86fb2e8be132e669e7d2bc4cf7f5fa6ee7d7bb6c`.
At activation, only this plan and its README/roadmap/review links were modified.
No stdlib contributor has been replaced yet.

| Existing contributors | Final semantic owners / migration phase |
| --- | --- |
| catalog contracts/roles and propagation trait declarations | core ops/iter/cmp/hash/fmt/convert; SR02-SR03 |
| catalog construction | Default: core default; Into/TryFrom/TryInto: convert; FromStr: str; FromIterator/Sum/Product: iter; SR03 |
| catalog defaults | Constructor/impl facts: alloc string/vec, core host/array/ops/iter, std collections; SR02-SR03 |
| catalog enums | Option: option; Result: result; Ordering: cmp; Infallible: convert; numeric errors: num; Bound/ControlFlow: ops; SR02 |
| catalog strings and bindings strings | alloc string; SR02 |
| construction_defaults and construction/default_construction bindings | Scalars: core num; Vec: alloc vec; Option: core option; hash: std collections; String: alloc string; SR02-SR03 |
| collections/list_methods/interface_protocols and list bindings | Traits/defaults/interface Debug: std collections; overrides: alloc vec; SR03 |
| comparison and comparison/array bindings | Shared content helpers/map/set: std collections; Vec: alloc vec; array: core array; SR02-SR03 |
| vector/hash bindings and bindings.rs helpers | Receiver owners; shared cursors: core iter; SR03 |
| iterable/iterable_defaults and iterable bindings | core iter, including eight adapters; SR03 |
| propagation implementations | Carrier owners: option/result/ops; SR02 |
| future/tasks and task bindings | core future/task; SR04 |
| documentation | Authored prose beside owners; generic rendering/inheritance stays shared; SR02-SR04 |
| partition/mod/identities/key, bindings routing, namespaces/identity/declarations | Explicit composition and facades; preserve canonical lookup, delete discovery/partition bridges at SR05 |

Inventory: 18 canonical declaration owners, 17 std facades and prelude (36 modules).
Preserve the empty `std::array` facade. `TraitDef::conversion_adapter` for
Into/TryInto/TryFrom remains semantic metadata, not callback-discovery policy.
String retains its builtin constructor, HashMap/HashSet retain Map/Set constructors,
and Vec/Future/tasks/adapters retain their distinct registered storage categories.
Manual host path registration remains on the existing checked ModuleBuilder API;
stdlib does not require a second path-authoring surface in ModuleDraft.

### Reproducible pre-migration declaration baseline

At the producer revision above, build `kagari-stdlib --lib` with the locked
workspace. Enumerate `StandardDeclarations::default().declarations()`, sort modules
by canonical ModuleIdentity, and encode each using
`ModuleDecl::compiler_input_bytes(definitions, default cancellation)`.
The aggregate concatenates little-endian u64 UTF-8 owner-name length, owner bytes
(`package::path`), little-endian u64 encoding length and encoding bytes.
Two independent processes produced identical outputs (38 capture files).

- 36 modules, 33 types, 41 traits, 226 implementations, 31 free functions,
  28 private functions, 93 explicit exports, 92 callable-requirement owners,
  11 concrete-result entries.
- Aggregate: 371618 bytes; SHA-256
  `eba747634e8114095d2e7c1484919cc70c52c3451979f3b573e6c882f39bc79f`.
- Human-readable inventory: 10636 bytes; SHA-256
  `da0eccce216da2c4a426b42bd92e67ba7fef41f0114c437d14c0b8dec465e9be`.

This encoding normalizes process-local identities, not private names, occurrence
order or intentionally split targets. SR05 must map those changes explicitly and
compare semantic facts, not interpret a changed digest as a regression by itself.
Temporary captures are under ignored `target/sr01/baseline`; the revision and
extraction procedure above are the durable reproduction authority.

### SR01 execution-path baseline

An unmodified archive of the producer revision was built with the default target
and workspace profiles. A source-enabled producer emitted an artifact; a separate
consumer built without source/native features loaded it. Its normal/build dependency
graph excluded source/HIR/syntax/package/compiler/MIR/codegen crates. Both processes
executed these parameterized wrappers and ended with zero active roots:

| Probe/input | Result | Bytecode / linked category |
| --- | --- | --- |
| String len: `aé🙂z` | 8u64 | LoadLocal, Native Call, Return; StringByteLength, managed String to scalar U64 |
| contains: `aé🙂z`, `é🙂` | true | Two LoadLocal, Native Call, Return; raw Callback, exact String/String to Bool |
| split: `\|é\|\|🙂\|`, separator `\|` | Five fields: empty, é, empty, 🙂, empty | Raw Callback, concrete Vec<String>, declared List<String>, linked result adapter; no explicit MakeInterface instruction |
| Existing allocation fixture `measure::native::add(20,22)` | 42 | Flat scalar Callback, exact I32 codecs and scalar slots |
| Vec<i32> `[3,1,2]`.clear(), then receiver len | 0u64 | Owned/contextual clear_fluent Callback with Value codecs, then length Callback |

Six imports are closed applications with no selected requirements. Callback records
retain prepared scoped signatures. Split alone has a result adapter; its observed
`applied: None` still takes runtime type-argument resolution. Canonical import
identity must accompany normalized linked records; table/slot numbers are not
stable semantic identities.

The Rust paths retain their existing checks: primitive len uses prepared locations
and checked string access; contains borrows both strings without owned input copies;
split polls, allocates owned fields/sequence payload, roots intermediate values and
adapts Vec to List. Flat scalar decoding uses fixed scalar codecs. Owned clear still
maps arguments, collects/clones Value arguments, retains roots/ScriptVec ownership,
and validates storage/mutation/result. No speed improvement or general allocation-free
claim follows from this evidence. Execution plus linked classification and source
inspection is not Rust function-level tracing.

Reproduction: at the pinned revision, build the SDK library with
`--no-default-features --features source`, compile a standalone producer against that
exact Cargo JSON artifact, emit the five wrappers and execute the inputs above.
Then build the SDK with `--no-default-features`, compile a separate consumer against
that artifact, load the emitted bytecode and repeat the assertions. Inspect wrapper
instructions/native imports and bounded `Runtime::modules()` Debug projections of
LinkedNativeCall; normalize only scoped IDs, retaining canonical imports. Check the
consumer dependency graph using Cargo tree normal/build edges. Temporary probe code
and outputs are in ignored `target/sr01/execution`; do not depend on them as permanent
documentation or a new product.

Capture SHA-256: artifact
`cd18830a23b53967a96ecd9d2c65c99be9386ce63b4e215fd0a73133222c246d`;
bytecode projection
`4dee596a521af5933134e70263710b1aa2a1532f69db37d03f9270c1ef773834`;
source-free linked projection
`6e09b6663368f6b70ef4ef0aeec475e57ed07ad17a4ab5a573d0431ee9bb0fe4`.

The existing VM allocation test
`native_allocations::warmed_scalar_and_contiguous_i32_boundaries_add_no_allocations`
passed at the baseline (100000 warmed scalar calls, contiguous i32 lengths 0/16/16384,
1000 calls each; zero allocation/reallocation/deallocation/requested-byte counts).
This excludes compilation, installation, frame setup and SDK conversion; it does
not measure split/owned allocations. Environment: Windows 11 x86_64 MSVC,
rustc 1.99.0 b940084d7, workspace dev/test opt-level 1, default build parallelism,
existing dependency cache. No release timing comparison was performed.

### SR01 implementation decisions and focused validation

- One DefinitionContext per batch; DraftReference/DraftMember are explicitly incomplete
  scoped references, not fabricated completed TypeRef/TraitRef handles. Sealing
  checks even unused forward members, complete contracts and dependencies.
- A shared pure default lowerer transforms signatures, bounds, hidden associated
  arguments, requirements and concrete results. Ordinary TraitBuilder and deferred
  assembly reuse it; stdlib's old transform remains a named SR03/SR05 removal target.
- Generic inherent substitution also has one shared lowerer. Override facts are
  applied under actual impl/method binders before final semantic checks.
- ModuleDefinition/ModuleAssembly are consumed once; no Clone/Send requirement is
  imposed on authoring state. DeclarationSnapshot contains no factories and is
  independently cloneable. Final callback Send/Sync requirements are unchanged.
- Factories receive a read-only SealedPreparation containing their own scoped native
  declaration and catalog. NativeAdapter still receives the authored signature.
  Default token provenance may be authored method or sealed template; validated
  scoped identity is preserved and foreign owners are rejected.
- Provider ownership is explicit catalog inventory, including empty/re-export-only
  modules; references do not establish ownership. Module/declaration import and
  executable assembly share shape/export/application validation.
- NativeModule/install_all retain binding/storage/host/atomic publication checks.
  Factory errors return no partial batch; external factory side effects cannot be
  rolled back. No invocation code or ABI identifier was changed.

Selected tests exercise new authoring/sealing and directly affected old builders,
not a workspace sweep. Actual passing selections include:

```text
cargo test -p kagari-runtime --lib native::assembly:: --no-fail-fast
cargo test -p kagari-runtime --lib native::builder::default_lowering::tests::
cargo test -p kagari-runtime --lib generic_default_is_derived_from_the_method_contract
cargo test -p kagari-runtime --lib native_default_rejects_unproven_operations_and_incompatible_codecs
```

The latest assembly selection passed 35 tests. This includes actual default-to-selected
calls for authored/template/imported-template tokens and same-batch forward-member
calls in either recipe order, rather than only preparing panic callbacks. Shared
lowerer tests passed nine cases. Structure, formatting, workspace all-target Clippy
with warnings denied and diff checks passed after integration. Intermediate fixture
scope/type and generic-environment test-driver errors were repaired; none is carried.
Full workspace/backend/platform matrices remain CI-owned. Independent final review
closed the original token-owner, factory-contract, forward-member, override-fact
and empty-provider defects. No known SR01 implementation errors remain.

Final scalar-target choice: each declaration owns an independent checked target;
sharing a Rust algorithm never deduplicates targets by callback pointer. Executed
i32/i64 implementations using the same callback retained distinct imported/linked
identities and exact signatures; wrong codecs on the second target were rejected.
SR05 may normalize old shared scalar targets into these independent targets while
still comparing each application/signature/requirement contract.

Explicit `implementation_send` / `implementation_with_send` terminals retain Send
pending captures without changing ordinary Local adapters or imposing Sync on
factories. `try_into_send` moves only sealed data and transferable pending entries;
it does not execute factories, reauthor declarations or repeat sealing. Default
wrapping retains the category and both products use one preparation implementation.
The transfer tests passed four cases, with six default/token and two Local/projection
regressions also passing. Batch-failure tests prove completed and unexecuted resources
are released once, later factories do not run, and retained semantic snapshots remain
valid; completed external side effects are not described as rolled back.

The stdlib Bundle now owns one recipe, an immutable Semantic projection (including
symbolic enum handles from the same snapshot), pending Send assembly and cached
complete NativeModules or terminal error. Short mutex ownership transfer finishes
before factory execution. OnceLock serializes concurrent callers; a thread-local
phase guard rejects same-thread reentry before waiting, including swallowed recursive
errors. Unwind panics become cached phase errors; abort/double panic cannot recover.
Factories must not spawn and wait for work reentering the same bundle: cross-thread
wait-cycle detection is intentionally not supplied. Symbolic handles contain no
runtime-local layout or root, and declaration consumers do not retain pending state.
Rust 1.90 Mutex/OnceLock ownership, reentry and panic semantics inform this boundary;
Wasmtime's preparation reference above informs only instance-independent capture rules.

`cargo test -p kagari-stdlib --lib assembly::` passed lifecycle coverage for declaration
access without preparation, cross-thread/concurrent one-time preparation, terminal
failure/reentry and independently retained semantics. The same bundle implementation
is temporarily test-gated in lib.rs: SR02 activates it with real contributors; SR05
removes the old CONTRACTS/MODULE/OPTION_MEMBERS/ENUM_TYPES owners, not this bundle.
This is a finite integration bridge, not a second test-only cache or fallback path.
The final structure/fmt/workspace Clippy/diff gates passed after these changes.
No claim is made that stdlib production consumers have already migrated.

### SR02 activation: public projections and intermediate assembly

SR01 checkpoint: `1022b771`. Inspection showed that legacy partitioning also owns
facade/prelude exports and validates the incomplete old contributor set. Removing
String/enums/ranges from that set while leaving projection there would lose public
exports. Therefore SR02 pulls forward only pure facade/prelude composition from
SR04/SR05: compose the complete explicit public inventory from new and remaining
legacy contributors, then seal everything once. Collection/Iterable defaults and
Future/task implementation migration stay in their assigned phases.

Remaining legacy declarations are raw owner-scoped inputs, not externally checked
providers. `ModuleDraft::import_declaration` adopts an input into the same batch and
still requires exact binding coverage and final sealing. It rejects foreign or
nonempty owners; it cannot replace an already authored module. This finite bridge
is removed with obsolete producers at SR05. Structural generic receivers such as
arrays reuse the existing inherent lowerer through `inherent_with`; declaration
prose can be attached through a local scoped `document` operation. Twelve affected
draft tests and all local lightweight/Clippy gates passed for these additions.
Those preliminary checks did not claim production acceptance; the completed
checkpoint evidence follows.

### SR02 completion

Production now uses the SR01 Bundle (its test gate is removed). New module-local
contributors are `foundation/{string,numeric,arrays,ranges,enumerations,propagation}`.
They own direct implementations and documentation; corresponding legacy declaration
producers and name-router cases are removed. `assembly/composition` builds complete
facades/prelude and combines only remaining raw seeds with these recipes. There is
no full-foundation-then-filter producer and no fallback on new binding failure.

String retains StringByteLength, scoped exact raw callbacks, and split's declared
List/concrete Vec result. Numeric conversion/aggregation, arrays/ranges and carrier
propagation reuse their existing algorithms. Remaining shared execution helpers,
collection/default/iteration producers and task producers belong to SR03/SR04;
SR05 removes the remaining seed/partition/cache bridge. `document` and override
`documentation` attach prose at actual local IDs without changing execution.

Inventory compared to SR01: 36 modules, 33 types, 41 traits, 226 implementations,
31 functions, 28 private functions, 93 exports, 87 nonempty requirement owners and
11 concrete results are unchanged. Requirement map entries increase 92 to 106 only
because fluent inherent authoring adds 14 empty entries, not selected obligations.
The full normalized semantic comparison remains SR05 work.

The pinned SR01 source/source-free probe was rerun against the migrated producer:
all five input/result/root assertions passed, and all five wrapper instruction
sequences plus six linked body/codec/result-adapter records match after scoped-ID
normalization. String len remains primitive, contains raw scoped, and split uses
the same concrete-to-interface adapter. No new per-call preparation or speed gain
is claimed. Temporary outputs remain under target/sr01/execution.

Focused validation passed runtime draft tests (12), stdlib registration and generated
doc examples, wide-index/UTF-8 boundaries, bundle/enum identity tests, selected SDK
String/numeric/default/aggregation/Try/FromResidual/array/range behavior, and affected
compiler/HIR/VM/artifact cases. Review found target assertions had become too weak
when replacing private names; these now resolve expected canonical owner, receiver,
member and applied trait, compare full scoped identity, and retain Vec fluent result
versus Unit. The repair selection passed:

```text
cargo test -p kagari-compiler --lib -- tests::bytecode::lowering::foundation_calls_lower_to_provider_qualified_native_imports tests::bytecode::lowering::lowers_aggregate_and_access_instructions tests::lower::foundation_calls_lower_to_checked_native_imports tests::lower::lowers_tuple_array_struct_and_access_expressions --exact
cargo test -p kagari --lib runtime::tests::language::operator_traits::arithmetic_stays_direct_and_indexing_uses_its_checked_native_binding -- --exact
```

Structure/fmt/workspace Clippy/diff gates passed. A stale build-product configuration
issue was resolved by invalidating affected runtime/stdlib products, not by changing
validation. No errors are carried; full workspace/backend/platform CI remains unrun.

### SR03 completion

Foundation contributors now own protocols (all 41 traits and conversion metadata),
List/MutableList defaults, Vec, hash containers, content comparison and iteration.
List and Iterable's old stdlib-specific default lowerers and migrated router cases
are deleted. Shared algorithms remain single implementations, selected directly by
owner-authored associations, not by private symbol matching. Only Future/task raw
producers and spawn/cancel discovery remain for SR04.

Runtime adds complete free-function submissions through `assembly/function`, using
FunctionBuilder for generic parameters/bounds/requirements/concrete results. The
new draft `define_function` takes one complete definition (a raw declaration/pending
tuple is also a complete unit), replacing its newly introduced two-argument form.
Immediate adapters retain their bounds; explicit transferable factories require Send
captures, not Send outputs or Sync authoring state.

A real generic-override test exposed duplicate implicit `Self: Iterable` and explicit
`Self: Iterable<Item=..., Iter=...>` obligations. Default assembly now removes only an
implicit constraint subsumed by an explicit one with the same receiver, trait,
arguments and all original equalities. Public method bounds and ordered requirements
remain; shared lowering and proof checks still run. This repaired the MIR shared-call
failure without weakening verification.

Verified slot order: List comparison (when needed), iter, next, optional set/remove;
Vec override comparison only; Vec FromIterator iter/next; hash FromIterator
hash/eq/insert/iter/next; zip/chain left iter/next then right iter/next; collect/sum/product
destination call with source type argument. List keeps concrete Vec results. Vec
primitive/scoped/owned categories, Map/Set builtin constructors, opaque lazy adapters
and cursor revision/borrow semantics are unchanged.

Actual selected validation (30 distinct tests) covered configured free functions
(3), default/token/transfer/lifecycle (8), production registration and doc examples
(2), custom/generic/interface List and Iterable, paired laziness/resumption, cursor
GC/failure, key type bounds, collection content/custom hash, construction/defaults,
reverse conversions, destination delegation, navigation and static rejection.
Representative exact commands:

```text
cargo test -p kagari-runtime --lib native::assembly::function::tests
cargo test -p kagari-runtime --lib -- native::assembly::default_tests native::assembly::tests::lifecycle native::assembly::transfer_tests::local_pending_rejection_reports_canonical_target_without_preparation
cargo test -p kagari-stdlib --lib tests::registration::canonical_foundation_contracts_survive_scoped_publication_and_installation -- --exact
cargo test -p kagari-stdlib --lib tests::documentation::standard_documentation_covers_the_registered_api_and_examples_analyze -- --exact
cargo test -p kagari --lib runtime::tests::language::iteration_traits::iterable_defaults_chain_on_containers_generic_custom_and_interface_receivers -- --exact
cargo test -p kagari --lib -- --exact runtime::tests::language::list_algorithms::generic_custom_receiver_uses_its_associated_iterator_in_default_calls runtime::tests::language::iteration_traits::paired_adapters_select_both_sources_and_preserve_laziness_and_resumption runtime::tests::language::collection_interfaces::hash_collection_interfaces_use_custom_key_protocols
```

The first behavioral selection passed 10/11; the implicit-bound repair above fixed
the only failure, which passed on targeted rerun. Further affected navigation/lazy
and destination/construction selections passed. Structure: 1347 Rust files, zero
violations/exceptions; fmt, workspace all-target Clippy and diff checks passed.
Independent review found no further SR03 blocker or weakened assertions. Counts
remain 36 modules, 33 types, 41 traits, 226 implementations, 93 exports, 87 nonempty
requirement owners and 11 concrete results. Full semantic normalization, post-migration
execution comparison and complete CI matrix are not inferred from these counts.

### SR04 completion

Checkpoint input: `426b38d7`. `foundation/future` and `foundation/tasks` now own
Future/Task/TaskScope storage, SpawnError, method signatures/docs and direct Rust
implementations. Spawn retains method-local T, ordinary callable coercion, three
admission errors and rooting; cancel retains generic receiver and cooperative
semantics. Remaining task private-name dispatch and old producer/partition helpers
are deleted. No executable contributor remains on the old producer path.

Module/type/trait/member prose is supplied with its owner declaration. Review caught
a moved-but-still-central member-doc switch; it was removed, and protocol signatures
now submit prose together. Generic same-batch inheritance copies actual trait-member
documents by scoped identity after impl expansion, preserves explicit overrides and
never prepares bindings. External catalogs do not carry prose, so this inheritance
is explicitly limited to same-batch declarations. A canonical documentation capture
before/after this repair was identical (238274 bytes). The remaining documentation
utility only renders Markdown; it contains no standard API inventory or text switch.

Production semantic tests prove real task declarations do not consume pending
preparation. Doc tests use declaration-only inputs and cover types, variants and
implementation members. Navigation resolves Future/Task/TaskScope/SpawnError and
spawn/cancel by full canonical identities and real source ranges. Module metadata,
aliases/facades/prelude are explicit; their raw import transport remains a named SR05
bridge, not a second executable declaration producer.

Passing local selections included production registration/storage, generated doc
examples, concurrent bundle preparation/terminal failure, SDK task handler, owned
future factory, lifecycle reload and failed dependency boundaries, plus navigation.
The final documentation repair ran:

```text
cargo test -p kagari-runtime --lib -- --exact native::assembly::seal::documentation_tests::canonical_member_docs_inherit_in_both_orders_without_overwriting_explicit_prose
cargo test -p kagari-stdlib --lib -- --exact tests::documentation::standard_documentation_covers_the_registered_api_and_examples_analyze tests::registration::canonical_foundation_contracts_survive_scoped_publication_and_installation
cargo test -p kagari --lib -- --exact engine::tests::registration_sources::generated_files_docs_and_navigation_share_the_checked_snapshot engine::tests::registration_sources::task_navigation_uses_canonical_authored_members_and_docs
```

All five passed. Prior unchanged async evidence was retained, not rerun for a new
transcript. Structure (1346 files, zero violations/exceptions), fmt, workspace Clippy
and diff checks passed. Initial fixture type/visibility errors were fixed; none are
carried. SR05 owns remaining catalog/enum caches, raw module-metadata import, and
moving shared implementation algorithms from the bindings tree into their actual
module owners. No complete CI matrix or final semantic comparison is claimed.
