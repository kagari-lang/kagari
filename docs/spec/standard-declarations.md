# Native declarations and library modules

A Script definition has a Kagari body. A Native declaration describes an external
implementation; its signature does not grant permission to call Rust. A host
must install the matching binding. Neither route infers a Kagari API from a Rust
signature or requires a separate binary declaration product.

This page describes current implemented behavior. Structured registrations and
explicitly supplied external declaration sources feed common HIR and semantic
checking under a common static environment. Runtime implementation registration is
separate from those binding-free semantic facts; generated views are optional origins.
[Architecture](../architecture.md#unified-library-registration) defines the owners.
The [native registration guide](../native-registration.md) explains the authoring
lifecycle, Local/Send preparation and a complete multi-owner fluent example.

## Ownership and installation

Normal source analysis includes the [eleven packaged modules](../../library/core/src/lib.rs)
without installing anything. They own the 26 language traits, nine ordinary traits,
`Option`/`Result` and their propagation, ranges and the `String` type. Their Script
bodies compile through the same pipeline as the application's. The optional stdlib
owns the six collection interfaces, Future/task APIs, `std` facades and extension
methods. For example, `std::string` supplies `trim` on the existing `alloc::string::String`;
the canonical type declaration alone supplies no such method. Array
literals construct builtin fixed-length arrays independently of installed declarations.
Concrete Vec/HashMap/HashSet providers are not currently supplied by the bundle;
their restoration belongs to the separate NC collection work. References below
to collection algorithms/storage describe retained interfaces and execution
constraints, not an available default concrete collection provider.
HIR checks the canonical source declarations against the compiler's language-role requirements. Installation remains independent of syntax/HIR.
Each declaration has one canonical core/alloc/std owner; checked `std` re-exports
retain its definition identity. Native modules carry public alias targets.
Sealing checks those targets against the supplied catalog: an authored owner
is verified declaration evidence, not an installed module.
Authored export aliases use `(NameNamespace, spelling)` keys, allowing Type and
Value aliases with the same spelling. Validation checks category agreement with
the canonical target and rejects duplicates within one category. The supported
portable aliases remain traits, registered types and enum variants. Canonical
definition identities and native symbol uniqueness are independent of spelling.
Generated source views follow the same two-space lookup rules as authored source.
The [namespace and prelude inventory](builtins.md#namespaces-and-prelude) defines
public paths and default name visibility.

Map/Set interfaces do not prescribe storage or traversal order. They do not imply
an installed HashMap/HashSet implementation. NC concrete collection restoration
is deferred; LinkedHashMap/LinkedHashSet and other additional containers are not
part of the current bundle either.

`Runtime::new` constructs an empty native registry. Low-level consumers explicitly
install the Native providers their programs require. A source-enabled engine builder
exposes verified core contracts in `declarations()` before installation. To request
stdlib, compose `StandardLibrary::new(builder.declarations())?`, then install
`library.modules()?`. A frontend-free host can instead compose that library with
`StandardLibrary::from_verified_program` from verified bytecode containing the
required core owners. Neither construction path installs callbacks. A core-only
Script program runs without stdlib, `.kgr` files or a frontend at execution. The builder's `install(module)` adds
a module, and `with_native_modules` installs exactly the supplied modules, without
an implicit foundation. The SDK has no production stdlib dependency.
The fallible mutable builder uses `install`/`install_all` as atomic transactions;
`build` consumes it. `builder.declarations()` supplies the installed authoring
providers to application ModuleBuilders. Duplicate module identities, conflicting
bindings, missing dependencies and declaration mismatches are rejected. A finished module contains
both portable declarations and runtime-local Rust entries/storage descriptors.
Compiler-only consumers can read the same declarations without a runtime or native
adapter preparation. Stdlib's declaration view and prepared Native modules come from the same sealed
bundle; its core references retain their separate verified source owners.

`ModuleBuilder::from_declaration` adopts separately authored `ModuleDecl` records
into its own definition scope. `declared_function(name)` selects a free function;
`declared_method(implementation, name)` selects a member using its position in that
declaration's implementation list. Both return the same binding token for `bind`.
Missing implementation/member selections reject immediately, and `finish` rejects
missing callbacks. The declaration remains the signature and identity authority.

Package names selected by the import environment resolve to their unique physical
root module when present. Packages supplied only through child modules use an implicit
namespace prefix; supplied children populate a unique parent's namespace only where
no explicit declaration already owns that name. Explicit package aliases determine
targets before dependency-derived spellings. All routes retain ordinary member
visibility, ambiguity and selected-exposure checks; a dependency is not private-path
access. The [common import environment](../../crates/kagari-hir/src/imports/environment.rs)
retains explicit module dependencies and selected preludes independently of HIR nodes.

[`core::future::Future<T>`](../../library/core/future.kgr) is the canonical
single-drive native storage type, with one invariant output parameter. Core
source owns the declaration and Runtime owns its storage binding. Basic `async`
and `.await` require no optional standard library. The optional `std::future`
facade and its prelude re-export the same declaration; without them, import
`core::future::Future` if spelling the type explicitly. There is no public
constructor or trait-based await protocol. Typed
`bind` with `Async<A>` selects this checked storage role;
ordinary invocation captures values and first await submits the operation. Source
async callables construct the same Future role with private script resume state;
both producers support encoded wait/resume.
`ModuleBuilder::bind_storage` binds a separately authored local type declaration
to its Rust storage descriptor, validating exact declaration/layout agreement and
rejecting duplicate or foreign bindings. Compiler declarations contain no Rust
factory, payload or completion endpoint.

The authored
`core::iter::Iterator` trait owns all seventeen defaults, including lazy
transformations, paired traversal and terminal consumption.

These interface declarations do not restore a concrete collection constructor.
For an installed bundle, a current scalar-only example is:

```kagari
fn main() -> i32 {
    val value = 40i32;
    value.wrapping_add(2)
}
```

The generic iterator defaults do not depend on the deferred List edit algorithms.
`Iterator.map<U>` accepts `fn(Self::Item) -> U` and returns
`core::iter::MapIterator<Self::Item, U>`. Filter, take, filter_map, skip,
enumerate, zip and chain return element-only adapters in the same module; the full signatures and consumption rules are in
[builtins](builtins.md#iteration-protocols). An `Iterable` supplies only `iter`: a collection or generic iterable calls
`iter().map(f)`, while a cursor uses `map(f)` directly. Identity iteration
preserves the current progress of a cursor.
Collect, sum and product delegate to FromIterator, Sum and Product on the
requested destination. Fold consumes one cursor in source order and returns its
initial accumulator on empty input. Concrete collection cursors remain
responsible for rejecting stale reads after structural writes.

String and the numeric scalars also supply finite inherent methods; see the
[String surface](builtins.md#primitive-operations-and-formatting) and
[numeric inventory](builtins.md#numeric-scalar-methods). Predecessor APIs
outside the accepted surface, including parse conveniences, enum combinators,
snapshots, windows, grouping and set algebra, remain withdrawn. They are not
compatibility requirements. New libraries may implement such APIs through ordinary
registrations.

### Numeric declaration boundary

Canonical `core::num` owns the installed integer-policy and width-matched float
utility/math declarations. Each concrete receiver has its own checked method target,
including exact argument and result types; static `f32::from_bits(u32)` and
`f64::from_bits(u64)` are ordinary associated functions, not receiver methods or
numeric casts. The [numeric inventory](builtins.md#numeric-scalar-methods) owns
signatures and [value semantics](value-semantics.md#floating-point-utilities-and-math)
owns exceptional values, power guards and precision.

The same authored signatures, documentation and bindings feed generated source,
checked native imports and source-free runtime installation. These are ordinary
registered calls, not names recognized specially by compiler or VM loops. Integer
checked results use the pinned standard `Option<T>`; overflowing results use
`(T, bool)` with the normal managed-result/root handoff. Float scalar codecs retain
the declared width and exact bit transport; this does not guarantee arithmetic NaN
payloads. Invalid float clamp bounds produce `ScriptTrap` after normal argument
admission, not a host panic; malformed native signatures/carriers still fail the
usual validation boundary.

No parallel free-function math namespace, compatibility aliases, associated-constant
authoring path or native-call const evaluator is introduced. Existing constants,
scalar const evaluation and backend admission remain unchanged; calling a registered
numeric method is not evidence of JIT/native execution.

## Explicit registration

Two supported authoring paths produce the same checked native modules.
`ModuleBuilder` uses already available explicit providers; module-owned assembly
collects same-batch contracts before preparing implementations. Neither path
infers a Kagari declaration from a Rust callback.

### Module-owned assembly

`ModuleDefinition` contains one canonical owner and one consumed recipe.
`ModuleSetBuilder` adopts external providers once into a shared definition context;
its owner-scoped `ModuleDraft` records complete declarations, docs, exports,
storage and deferred bindings. `finish()` runs recipes and seals semantic facts
without invoking adapter factories. Duplicate/external owners, unfulfilled forward
references, invalid scopes/categories/binders, unsupported exports and missing
binding/storage coverage reject rather than producing weaker declaration-only data.

`MethodDecl` and `FunctionDecl` use fluent `configure`, `produces`, documentation
and implementation terminals. `configure` uses the ordinary `FunctionBuilder`;
`produces` requires a checked concrete implementation of the declared interface
result. Generic inherent groups, trait defaults and implementation overrides reuse
runtime's shared semantic lowerers. Override facts retain requirement slot order
and are substituted under actual implementation/member binders.

`DeclarationSnapshot` independently retains immutable declarations and the sealed
catalog, with no pending factories. `ModuleAssembly::into_modules()` consumes
preparation once, returning all modules or an error without installation. Factories
receive `SealedPreparation` for their exact target and catalog. Preparation failure
drops completed candidates and unexecuted factories, not retained snapshots, and
does not undo external Rust side effects. Ordinary assembly does not catch panics.

Local implementation terminals impose no additional `Send` bound. Explicit
`implementation_send` / `implementation_with_send` terminals allow
`try_into_send()` to move sealed pending work without invoking it. Configuration
remains local; final callbacks retain their existing thread-safety requirements.
The standard library privately caches collection/preparation results and terminal
errors; this is not an extra requirement on ordinary `ModuleBuilder` users. See
[the guide](../native-registration.md#local-versus-transferable-pending-work) for
transfer, reentry and failure boundaries, and its
[runtime boundaries](../native-registration.md#the-standard-bundle-and-runtime-boundaries)
for cooperative cancellation and generation-pinned handles.

### Immediate module builders

`ModuleBuilder` owns a module's declarations. `FunctionBuilder` introduces fresh
Kagari generic parameters, parameter/result types, bounds and selected callable
requirements. `TypeBuilder` declares a nominal native type and its storage.
`implement` introduces scoped implementation parameters; `inherent_impl` and
`trait_impl` declare the appropriate method groups. Native implementations
reference complete checked trait contracts instead of redeclaring them.
Generic and associated bounds contain ordinary applied trait records, including
their exact identities, arguments and associated outputs; there is no separate
builtin-constraint variant. Bound normalization preserves complete applications
and canonical identity order, and declaration/loading checks still enforce trait
kind, arity, member ownership and binder scope. Source operators use the same
[operation-specific bounds](traits.md#operation-specific-bounds).

A bound is not a callable dependency. For example, a hash key requires Eq + Hash,
while a selected equality callback names PartialEq::eq. Native declarations keep
both the admission requirement and the exact executable dependency; shared callers
forward checked operations without runtime trait lookup.

Collection interfaces use ordinary methods and parent traits. There is no
storage-access marker on trait declarations. Installed implementations can supply
protocols for nominal interface values; type-owned protocols belong to that
interface's defining module. The standard library explicitly implements bounded
Debug for its six collection interfaces, without PartialEq, Eq or Hash.
`Interface::debug_preview` supplies bounded formatting. Content helpers compare
through selected element/key protocols; boxing never forwards a hidden receiver's
equality. These operations grant no storage layout or writable methods. Concrete host
Map/Set access descriptors remain checked at their independent storage boundary.

`ModuleBuilder::define_enum` authors an ordinary managed nominal enum rather than
an opaque Rust storage object. Its `EnumBuilder` declares type parameters and unit
or tuple-payload variants, exposes `self_type()` for recursive payloads, and
validates the completed declaration before publishing it into the module's
authoring providers. `finish()` returns the same `TypeRef` handle used by generic
implementation builders; `apply` produces `Ty::Enum` for an enum declaration.
`TypeRef::variant` returns an owner-qualified `VariantRef`, not a raw discriminant.
Enum and variant documentation use `documentation` and `variant_documentation`.
Module completion checks the referenced enum providers and generic applications;
installation rejects absent or conflicting dependencies atomically.
These declarations use ordinary nominal enum HIR, constructed directly from
registration or lowered from explicitly supplied complete source.
Native functions obtain scoped applied types through
`NativeCall::result_type_argument` or `argument_type_argument`.
`allocate_enum(&applied, &variant, fields)` checks the installed nominal owner,
variant membership, substituted payload types and live heap handles before
allocation. `enum_argument_is` and `enum_argument_field` inspect rooted arguments
through those same pinned layouts; wrong variants and payload indices fail.
Enum codecs use managed Values while the declared native signature retains the
exact nominal type. Root newly allocated Values before further allocation or
synchronous reentry. Generic native enum templates travel in checked executable
layouts, so artifact-only hosts use these APIs without source analysis.

For example, a scalar application entry can be declared and bound as follows:

```rust
use kagari_runtime::native::{
    binding::NativeResult,
    builder::ModuleBuilder,
    context::NativeCall,
    declarations::FunctionDecl,
    module::NativeModule,
    types::Type,
};

fn module(providers: &kagari_runtime::native::catalog::DeclarationCatalog) -> NativeResult<NativeModule> {
    let mut module = ModuleBuilder::new("demo::numbers", providers)?;
    let maximum = module.define_function(FunctionDecl::new("maximum"))?;
    module.function(&maximum, |function| {
        function.parameter("left", Type::i32())?;
        function.parameter("right", Type::i32())?;
        function.returns(Type::i32())?;
        Ok(())
    })?;
    module.bind(maximum, |_cx: &mut NativeCall<'_>, left: i32, right: i32| -> NativeResult<i32> {
        Ok(left.max(right))
    })?;
    module.finish()
}
```

`bind` accepts an explicit raw/primitive binding, a flat call-context entry or
an owned typed callback. Adapter selection retains the authored Kagari signature;
all forms use its binding validator and contextual entries check each concrete
application before conversion effects. Rust definitions never infer Kagari
traits, impls or generic constraints.

Registration checks ownership, generic scope, complete trait method coverage,
substituted signatures and storage compatibility before publishing a module.
Unbound functions, foreign declaration handles and incompatible Rust conversions
are rejected. The application fixture at
[native provider](../../crates/kagari/tests/support/native_provider.rs) demonstrates a generic
factory and a non-sequence object with an inherent method and retained callback.

Documentation registration preserves complete Markdown, blank paragraphs and
fenced examples. `ModuleBuilder::documentation`, `TraitBuilder::documentation`
and `TypeBuilder::documentation` set their overviews. `FunctionDecl::documentation`
and `MethodDecl::documentation` attach item/member documentation.
`TraitBuilder::associated_type_documentation` checks the declared member name.
Trait implementations inherit the provider's method docs across module boundaries;
`MethodsBuilder::documentation(name, text)` overrides a particular implementation.
Finished `ModuleDecl` records retain module and member documentation independently
of executable signatures and contract matching.

## Declaration analysis inputs

All routes use the same HIR function/type/member/impl records and semantic checks:

- The frontend supplies packaged core once per analysis input set, not once per
  user file. Its modules have stable `kagari-core:///` or `kagari-alloc:///` URIs,
  package aliases and explicit dependencies, but no prelude. A competing owner
  is an error. Snapshots retain the inputs for navigation and replay.
- `AnalysisDatabase::set_native_modules` lowers owned, scoped `ModuleDecl` records
  directly. Construction establishes declaration-to-HIR correspondence; no rendered
  text or parser round trip is required. Exact identity references still undergo
  visibility, dependency, category, arity, binder, bound and associated-output checks.
- `set_native_sources` pairs registrations with explicitly supplied complete
  `DeclarationSource` documents. The ordinary parser lowers them; structural and
  non-trivia correspondence checks prevent attaching registration metadata to a
  changed signature. Only static contracts attach to HIR; implementation binding
  remains a separate Runtime responsibility.
- `set_external_modules` accepts offline `ExternalModule` inputs: real source plus
  scoped static sidecars. These are checked alongside project source without an
  installed provider. The same checked complete-program boundary admits them for
  compilation; supported-operation checks still apply. They install no Rust entries.

Structured declarations retain structured origins and documentation. Their logical
file keys are not authored source: syntax, source surfaces and physical declaration
locations may be absent. Parsed inputs retain their actual source ranges. Do not
invent spans or treat a rendered view as a complete authored-source manifest.

### Explicit Native syntax and fields

The [ordinary grammar](syntax.md#native-declarations) and common static rules admit
these declarations in structured, external and ordinary project inputs. Native
spelling declares external implementation, never an installed Rust callback. This
example supplies declarations, not executable bindings:

```kagari
pub native type Counter<T> {
    pub var value: T,
    pub val child: Child,
}

pub native type Child;

pub native fn echo<T>(value: T) -> T;

trait Read {
    fn read(self) -> i32;
    fn answer(self) -> i32 {
        42
    }
}

impl Counter<i32> {
    pub native fn new(value: i32) -> Counter<i32>;
}

impl Read for Counter<i32> {
    native fn read(self) -> i32;
}
```

`native fn` ends in `;`, never a Script block. This applies to free functions,
instance methods (`self`) and associated functions (no `self`); generics, bounds
and ordinary impl/trait rules still apply. `native type Name;` is fieldless;
`native type Name { ... }` uses ordinary fields, attributes, visibility, `var`/`val`,
generic types and bounds. A type is not an alias merely because it ends in `;`.

Function implementation kinds are **Required**, **Script** and **Native**.
`Read.read` is Required; `Read.answer` is a complete Script default; the impl's
`read` is Native. Script definitions, including embedded library source, retain
all fields, bodies and defaults: do not replace them with signatures, empty bodies
or synthetic implementations. Structured `ModuleDecl.script_bodies` can retain
ordinary [ScriptExpr](../../crates/kagari-types/src/declaration/script.rs) expression
bodies, including helper calls with explicit type/value arguments; complete supplied
source retains full blocks. Missing bodies reject. Default authoring lowers library
algorithms to ordinary Script methods calling private Native helpers, not executable
default recipes. Helper references are module-qualified so same-named parameters do
not capture them. Genuine Native default declarations also remain supported.

A structured trait impl can supply an ordinary Script member with
`with_script_method`. At seal time its builder describes the expression; it is
not a Rust callback at execution. For `<D as From<S>>::from(self)`, `trait_call`
names the qualified static member, type receiver `D` and applied trait `From<S>`;
`parameter(0)` supplies the value `self`. Impl binders `S` and `D` belong to the
impl, not the method. Direct structured input and parsed declaration views use
the same qualified-call checks.

Native and Script fields share ownership, type, visibility and writeability
checking. For `counter: Counter<i32>`, `counter.value = 1` type-checks, while
`counter.value = false` does not. `val child` forbids rebinding that slot; reading
it returns an ordinary Child object handle.
It does not freeze the referent through other aliases. Scalar reads copy the
scalar value. This frontend support creates no field adapter or storage authority;
unsupported executable Native fields still reject at the compiler/runtime boundary.

The standard library does not provide script reference wrapper constructors.
User declarations named `Ref` or `RefMut` follow ordinary type application rules
and receive no special borrowing, projection or conversion behavior.

### Offline static metadata

[`ExternalModule`](../../crates/kagari-hir/src/analysis/external.rs) carries facts
that source syntax alone cannot express. This is an in-memory analysis input,
not a new sidecar file format, exporter, watcher or provider product:

| Input | Static meaning |
| --- | --- |
| Source, package alias, prelude and dependencies | Complete declarations/Script bodies, ownership, exposure and explicit library dependencies. |
| `types` | Checked canonical constructor selection or Host category. Ordinary nominal Native types need no entry; physical storage layout is not an HIR sidecar. |
| `hosts` | Scoped `ExternalHost` root, complete schema closure (including the exact root) and checked paths. A Host category without its schema/path metadata rejects, rather than becoming an ordinary Native object. Parsed fields must match the schema. |
| `conversions` | Forward conversion contract on the actual trait: static method, associated error member and result enum identities. Exposure and signature are checked; there is no reverse relationship record or implicit implementation proof. |
| `enum_failures`, `import_namespaces` | Failure-origin flags on actual enum variants and namespace categories on exact named import leaves. |
| `methods` | Exceptional receiver-access/override restrictions, targeted at the actual function or trait/impl member. Impl selectors count blocks in source order. |
| `contracts` | `NativeContract` ordered selected operations and optional concrete interface result, with explicitly imported definition scopes. |

Canonical String/Range constructor identity, arity and
field shape are checked at common declaration preparation even if `types` is empty.
[Reserved language roles](../../crates/kagari-hir/src/language/items.rs) require their
canonical owners and independent visibility/binder/parent/member-shape checks,
uniqueness and completeness, not an origin-granted foundation privilege.

[Import scopes](../../crates/kagari-hir/src/imports/environment.rs) retain package
aliases, explicit dependencies and selected preludes. Producer/package defaults are
normalized before graph construction; an explicit module scope replaces those
defaults. Accessible prelude bindings enter the ordinary implicit tier, and multiple
selected preludes conflict. [AnalysisInputs](../../crates/kagari-hir/src/analysis/inputs.rs)
retains explicit scopes for replay; semantic setters invalidate dependent caches and
fingerprints. Coherence follows actual declaration/receiver ownership, not input origin.

Native operation slots retain order and duplicates, receiver, applied trait/member
and method type arguments. They are not interchangeable with admission bounds.
The common checker validates category, exact identity/arity, binder scope, bounds,
associated-member ownership and concrete-result conformance, including obligations
involving Self. Sidecars neither supply Rust callbacks nor bypass these checks.
Missing/ambiguous targets, constructor-owner conflicts and invalid Host paths reject.
An offline Native declaration remains unbound even after successful analysis;
text, URI, matching names and static metadata never confer installed binding authority.

## Engine source presentation

Source-enabled SDK engines default to the direct structured route above.
`engine.native_declaration_sources()` still returns rendered immutable presentation
views and sites, but those views are not the default analysis inputs. Registered
Markdown is available without a parsed owning source; source locations are optional.
For navigable files, a tooling host explicitly selects materialized views before
sealing, retaining the parsed/validated route:

```rust
use kagari::engine::KagariEngine;
use kagari_stdlib::StandardLibrary;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = KagariEngine::builder()?;
    let library = StandardLibrary::new(builder.declarations())?;
    builder.install_all(library.modules()?)?;
    // Construct application modules against builder.declarations(), then install them.
    builder.declaration_cache("target/kagari-declarations");
    let engine = builder.build().map_err(|error| format!("{error:?}"))?;
    assert!(!engine.native_declaration_sources().is_empty());
    Ok(())
}
```

Omitting the directory keeps presentation views in memory without filesystem IO
and analysis uses structured declarations. Setting `declaration_cache` pairs the
rendered documents with registrations through `set_native_sources`, validates them
and publishes the files. CLI source commands configure this existing route
automatically. The cache uses renderer-version/content paths;
unchanged content reuses paths and doc-only changes preserve old snapshot targets.
SDK errors retain the failing filesystem path and IO cause. Published normalized
absolute paths and declaration ranges match the analyzed content; LSP integrations
encode paths as file URIs at their protocol boundary. HIR declaration snapshots
provide `documentation` and `module_documentation`; full analysis supplies
`documentation_at` and `module_documentation_at` for item and module references.
They preserve complete registered Markdown; a physical owning location exists only
when real source was supplied. The materialized route retains precise parsed ranges.
Source-independent declarations are checked during engine preparation; declarations
requiring user-source traits are checked with those providers in the source snapshot.

Artifact-only and native-only SDK consumers install the same checked registration
records without a source frontend, generated KGR or declaration binary. User-program
KBC/MIR artifacts and linked verification remain independent execution features.

## Checked executable contracts

Structured, external and ordinary source declarations share binding-free HIR and
static checking. Required/Script/Native remain distinct; Native has one checked
semantic kind without an installed-entry selector. HIR retains caller-visible
Host/access policy, ordered operations and result constraints, not callbacks, Rust
storage adapters or internal loans. The [compiler handoff](../../crates/kagari-hir/src/compile_input/program.rs)
requires complete checked facts and supported executable operations, not a provider
registry or an input-route privilege. Missing demanded implementations are a linking
failure rather than a different type-checking result.

Executable lowering carries provider-qualified native imports with concrete
types, signatures, bounds and selected callable dependencies. Portable validation
proves those facts against the executable dependency closure. Artifact loading checks the installed entry
against the complete carried declaration; matching names alone are insufficient.

An ordinary generic bound states applicability. A selected callable requirement
additionally states that the Rust body invokes a particular trait member. Linking
resolves that member once for the concrete native application. `cx.selected(slot)`
borrows the prepared callable; invocation does not repeat source lookup or trait
resolution. Implicit language operations and explicit script implementations
retain distinct checked targets.

Ordinary default materialization substitutes trait, receiver, method and
associated-output arguments through normal instance planning. Interface tables retain
`method_origins` linking an impl-owned placement to its canonical authored member.
[Contract verification](../../crates/kagari-contract/src/types/verify.rs) checks those
origins and final override policy as internal materialization provenance.
[Materialization verification](../../crates/kagari-contract/src/types/materialization.rs)
checks placed Script identities against portable source/body records already checked
against instructions by [bytecode verification](../../crates/kagari-bytecode/src/program.rs).
These records do not require physical source text or a complete authored-source manifest,
but internal consistency does not authenticate the installed library's semantics:
a product can change its body and carried evidence together.

[Runtime Script authentication](../../crates/kagari-runtime/src/native/registry/scripts.rs)
independently retains the installed canonical ScriptExpr and function contract, and
compares them with the actual bytecode call/value trace: selected callees, call order,
argument flow, applied types/signatures and returned value. Trait/receiver/method
binders and associated projections/families are applied through the checked impl's
outputs before comparison. For final Script methods supplied by a Native registration, absence of the
installed canonical expression fails closed. Portable origins, a matching Script signature or an
artifact-supplied digest cannot authorize a replacement; parsed source alone is not
that independent canonical authority. This check covers the current expression grammar,
rejects unsupported trace effects and requires neither HIR nor source parsing.
Native materializations instead preserve the applied declaration, ordered operations,
result and access contract for independent matching to the installed implementation.
These are ordinary callable/body contracts, not a serialized default recipe.

In contrast, the canonical `Into` and `TryInto` blanket impls are compiled
from [core source](../../library/core/convert.kgr), not authenticated against an
installed Script template. The ordinary executable proof must establish the
forward bound; `TryInto` takes its `Error` from the selected `TryFrom` impl.
A Script forward call needs no provider. A native one still requires its checked binding.

Runtime bindings and Rust storage factories are local installation data, not
serialized function pointers. Missing, duplicate/conflicting, foreign-context or
mismatched implementations reject at composition/linking; static declarations do not
bypass those checks. Artifacts retain portable identities and contracts.
Executable loading works without HIR, source parsing or generated declaration text.
Named callable bindings select current implementations across reload without an
obsolete executable lease. Created closures/Futures and admitted computations retain
exact bodies; ordinary calls inside them still use current implementations.

## Calls, roots and storage

Ordinary native functions and their script callbacks execute synchronously.
`NativeCall` exposes checked arguments, selected calls, allocation and execution
checks. `CallableHandle` borrows a callback during its invocation. A payload that
retains a callback uses `StoredCallable` and traces it. Returned script heap values
must be rooted before further allocation or reentry; callback argument packs do
not create a script tuple merely to carry Rust arguments.

Scoped sequence views borrow rooted existing storage. Primitive layouts use
contiguous typed buffers, including empty sequences whose layout is determined by
the declared element type. Heap-reference elements use traced Values. Bulk slice
access does not copy the entire sequence or allocate a per-call scratch buffer.
Mutable access respects native borrow/callback guards and storage revisions. These
sequence codecs and edit leases accept registered nominal sequence storage, never
builtin arrays. Fixed arrays share internal buffer kernels only after separate
family and element-contract validation.

`SequenceEdit` leases the actual sequence buffer for synchronous mutation. The
lease excludes receiver-slot access through other aliases and restores storage on
success, failure or unwind. Primitive edits do not clone the buffer. Traced values
keep explicit roots while sorting may move elements into Rust scratch space.
Sort, reverse, retain and dedup preserve completed effects; no rollback guarantee
or atomic bulk replacement is required. Receiver identity, generation, callback
protection and storage revisions remain checked.

`NativePayload` supplies tracing, logical units and ordinary Rust destruction.
Every retained script Value and stored callable must be traced. `NativeStorage`
registers the payload layout and either a checked factory or an explicitly
constructed payload. New opaque types use the generic Opaque handle;
they do not require new compiler, ABI, verifier or VM type variants.

## Sorting and lazy iteration

Primitive infallible ordering sorts the compact Rust slice directly. Script Ord,
supplied comparators and key selectors run synchronously while editing the actual
buffer. Sorting is stable; key selectors run during comparisons without a caching
prepass. A failed comparison prevents further user comparisons. Rust may finish
internal bookkeeping; all original elements survive, but their order can change.
Length-changing algorithms preserve completed removals. Custom set/remove failures
can leave earlier writes or removals. Comparison counts are unspecified.

Core adapters store cursors, callbacks and progress in ordinary Script objects
and closures. Zip and chain retain both cursors. Construction performs no
`next` or callback calls. Aliases share progress; map consumes its input before
invoking the mapper, even if the mapper traps or advances the same cursor.
There is no adapter-wide reentry guard. Normal cancellation, call depth and
concrete source checks apply. Base collection cursors still validate revisions
on reads; early termination needs no iteration lease. The generic execution loop does
not identify an adapter by its library name.

Numeric `TryFrom` pairs are ordinary registered implementations with declared
`Error` outputs. Scalar numeric code computes conversion outcomes; it does not
choose a library enum or error declaration.

Option, Result, Ordering, Bound, ParseError, TryFromIntError and ControlFlow are
ordinary enum records owned by their core-library modules. The never type `!`
used in residuals and lossless conversions is built in, not a registered enum. Standard Rust bodies
construct and inspect them through checked declaration handles and pinned nominal
layouts. No semantic type, fixed runtime tag or dedicated instruction inventories
these families. Iterator and comparison contracts retain only the checked symbolic
member bindings they consume. Library authoring handles are cached; layouts and
type arguments remain scoped to the executing generation.

A variant's `reports_failure` fact is explicit checked declaration/layout metadata.
Packaged core supplies it for canonical `Result::Err`, which has one payload field.
Construction captures optional diagnostic origin. Ordinary user source enums and
native enum builders leave the fact false. Equality, hashing and payload access ignore it.

Try and FromResidual are authored core traits; their standard implementations live
in the library alongside the carrier declarations. A native carrier uses
`ModuleBuilder::implement`, `trait_impl`, `associated_type` and `bind`, just
like other trait implementations. `NativeCall::type_parameter` retains the
selected result's executable scope when constructing its nested residual.
Result's Script FromResidual impl calls the checked `F: From<E>` method. Static
bound methods can be selected or forwarded through the same validated witness
path as receiver methods; no execution-time trait search is added.

Result branch copies origin into its residual, and FromResidual copies it into
the returned failure after conversion. The [authored bodies](../../library/core/result.kgr)
request that transfer explicitly; constructing an unrelated new `Err` does not.
The compiler's generic `?` path selects calls and ControlFlow variant operations
for the operand's checked carrier, not a hardcoded Option/Result branch.

## Tooling

Generated `.kgr` is a read-only projection for navigation, completion, signatures
and documentation. Generic binders use `T` for one parameter and `T1, T2, ...`
for multiple parameters. Method-local binders use `M` / `M1, M2, ...` when an
enclosing generic binder already uses `T`, avoiding shadowing. Declarations,
bounds and bodies share this spelling; semantic binder positions remain zero-based.
The generated view is not an independently installable library or a second
signature authority. Default compilation lowers structured registration records;
explicit supplied views instead undergo ordinary parsing/lowering and non-trivia
correspondence checks before static contracts attach. Both routes check complete
ordinary Script default bodies and calls to declared Native helpers, including unused
defaults; no dedicated forwarding-call record or installed selector is attached to HIR.
Offline inputs must preserve complete Script definitions/defaults and required static sidecars.
Authored core declarations and members own their source ranges and documentation,
including when reached through an installed `std` re-export. They need no generated
view. Native registrations own their own prose; without materialization a physical
range may be absent. Standard and application Native views use the same renderer;
materialized navigation resolves parsed ranges in the exact supplied snapshot.
Changing a view's URI changes its presentation location, while its registered
module and callable identities remain unchanged. Native registration does not infer
a Rust implementation source location or use a URI to grant native authority.
Other generated views include library-owned types,
inherent methods and Native trait implementations. `MapIterator.next` instead
navigates to its ordinary Script implementation in `core::iter`.

For [authored core API docs](../../library/core/), use the trait or type as the
place to explain when to choose it, what its caller observes, and which choices
belong to the implementer. Keep a member's hover useful on its own: for example,
`Iterator::zip` says which cursor advances when only one side has an item. It
need not repeat the trait tutorial. Show the implementation and a caller with an
observable result when the signature alone cannot explain the operation. Core
examples should include their own imports and run without stdlib unless they
explicitly build on a preceding example or name an optional provider.

Current direct/offline equivalence and static-authority coverage lives in
[HIR external tests](../../crates/kagari-hir/src/analysis/external/tests.rs) and
[SDK registration-source tests](../../crates/kagari/src/engine/tests/registration_sources.rs).
Collection-dependent coverage remains subject to the separate NC restoration gap;
source-free executable checks are independent of declaration presentation.
[Performance measurements](../performance-baseline.md) record measured costs;
the [roadmap](../implementation-roadmap.md) owns pending work.
