# Native declarations and library modules

Kagari declarations describe Kagari types, functions, traits and implementations.
Rust entries implement those declarations. Registration is explicit: a Rust
function's signature does not define a Kagari API, and no declaration attribute
macro is involved. `kagari-stdlib` owns all 40 standard trait registrations,
including the 26 reserved language traits. Source-free runtime installation uses
these same declarations and local Rust bindings without a binary declaration product.

This page describes current implemented behavior. CR01-CR02 established shared
semantic ownership and explicit analysis providers. LR01 moved concrete standard
registrations and algorithms to their own crate. LR02 connects complete generated
modules, cached navigation and syntax-based role recognition to Engine registration.
LR03 removes the unused trait product and independent handwritten declarations.
The [single execution plan](../implementation-roadmap.md#crate-responsibility-migration-cr01-cr02-complete)
owns sequencing and acceptance.

## Ownership and installation

The installed `core`/`alloc`/`std` foundation contains complete language contracts:
operators, comparisons, hashing, Index, Fn, iteration, collection interfaces,
formatting and error conversion. It also declares Option/Result/Ordering/Bound,
range forms and the default Vec/HashMap/HashSet types. The SDK installs their basic native operations by default. Array literals
construct Vec. Explicit registrations declare the core traits; validated language
roles select their semantic duties. Installation remains independent of syntax/HIR.
Each declaration has one canonical core/alloc/std owner; checked `std` re-exports
retain its definition identity. Native modules carry public alias targets, and
installation verifies that the canonical declarations exist in their providers.
The [namespace and prelude inventory](builtins.md#namespaces-and-prelude) defines
public paths and default name visibility.

Map/Set interfaces do not prescribe storage or traversal order. Default HashMap
and HashSet use Rust `std::collections` hash tables and require Eq + Hash keys.
They promise neither insertion nor sorted order. LinkedHashMap/LinkedHashSet and
other additional containers belong to optional modules; none are installed by
the current bounded library proof.

`Runtime::new` constructs an empty native registry. Low-level consumers explicitly
install `kagari_stdlib::modules()` with `NativeModule::install_all`, including for
source-free execution. All engine constructors select and install the standard
modules. The builder's `install(module)` adds an application module,
and `with_native_modules` accepts application modules in addition to the foundation.
The fallible mutable builder uses `install`/`install_all` as atomic transactions;
`build` consumes it. `builder.declarations()` supplies the installed authoring
providers to application ModuleBuilders. Duplicate module identities, conflicting
bindings, missing dependencies and declaration mismatches are rejected. A finished module contains
both portable declarations and runtime-local Rust entries/storage descriptors.
Compiler-only consumers can read the same declarations without a runtime.

List/MutableList declare common algorithms directly. The bundled
`std::collections` module supplies lazy map as an ordinary function:

```kagari
use std::collections;
fn main() -> i32 {
    val values = [22, 20];
    values.sort();
    val mapped = collections::map(values, |value| value + 1);
    match mapped.next() { Some(value) => value + values[1], None => 0 }
}
```

List provides sorted, sorted_by, sorted_by_key, reversed and distinct. MutableList
provides sort, sort_by, sort_by_key, reverse, retain and dedup. Natural sorting
requires T: Ord, while key methods accept a method-local K: Ord. Eq-only distinct
preserves first occurrences without adding a Hash bound. These are declared trait
methods with native defaults and optimized Vec overrides. Custom containers
reuse the defaults through their selected iterator and set/remove operations.
`map<T, U>` accepts a Vec and `fn(T) -> U`, returning the library-owned
`MapIterator<T, U>`. It remains an ordinary free function.

String also supplies the finite inherent methods documented in
[builtins](builtins.md). Predecessor APIs outside the accepted surface, including
parse conveniences, enum combinators, iterator terminals, snapshots, windows,
grouping and set algebra, remain withdrawn. They are not compatibility
requirements. New libraries may implement such APIs through ordinary registrations.

## Explicit registration

`ModuleBuilder` owns a module's declarations. `FunctionBuilder` introduces fresh
Kagari generic parameters, parameter/result types, bounds and selected callable
requirements. `TypeBuilder` declares a nominal native type and its storage.
`implement` introduces scoped implementation parameters; `inherent_impl` and
`trait_impl` declare the appropriate method groups. Native implementations
reference complete checked trait contracts instead of redeclaring them.
`TraitBuilder::storage_view` declares an installed readonly/mutable storage
capability. It does not grant a new heap representation; nominal implementations
retain their storage and ownership checks. Script traits cannot author this fact.

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
These declarations use ordinary generated enum syntax and parsed nominal HIR.
Native functions obtain scoped applied types through
`CallContext::result_type_argument` or `argument_type_argument`.
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
    context::CallContext,
    declarations::FunctionDecl,
    module::NativeModule,
    types::Type,
};

use kagari_stdlib::declarations::StandardDeclarations;

fn module() -> NativeResult<NativeModule> {
    let standard = StandardDeclarations::default();
    let mut module = ModuleBuilder::new("demo::numbers", &standard.catalog()?);
    let maximum = module.define_function(FunctionDecl::new("maximum"))?;
    module.function(&maximum, |function| {
        function.parameter("left", Type::i32());
        function.parameter("right", Type::i32());
        function.returns(Type::i32());
        Ok(())
    })?;
    module.bind(maximum, |_cx: &mut CallContext<'_>, left: i32, right: i32| -> NativeResult<i32> {
        Ok(left.max(right))
    })?;
    module.finish()
}
```

`bind` checks supported argument/result codecs against the declared Kagari
signature. `bind_with` accepts explicit codecs for cases requiring additional
control. Both produce the same prepared native binding; neither infers Kagari
traits, impls or generic constraints from Rust definitions.

Registration checks ownership, generic scope, complete trait method coverage,
substituted signatures and storage compatibility before publishing a module.
Unbound functions, foreign declaration handles and incompatible Rust conversions
are rejected. The application fixture at
`crates/kagari-embed/tests/support/native_provider.rs` demonstrates a generic
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

## Engine source presentation

Source-enabled engines render every installed module from the sealed registrations
and check complete generated KGR with the ordinary frontend before publishing it.
`engine.native_declaration_sources()` returns those immutable views and their sites.
For example, a tooling host configures file materialization before sealing:

```rust
let mut builder = KagariEngine::builder()?;
// Construct application modules against builder.declarations(), then install them.
builder.declaration_cache("target/kagari-declarations");
let engine = builder.build()?;
```

Omitting the directory keeps views in memory without filesystem IO. CLI source
commands configure it automatically. The cache uses renderer-version/content paths;
unchanged content reuses paths and doc-only changes preserve old snapshot targets.
SDK errors retain the failing filesystem path and IO cause. Published normalized
absolute paths and declaration ranges match the analyzed content; LSP integrations
encode paths as file URIs at their protocol boundary. HIR declaration snapshots
provide `documentation` and `module_documentation`; full analysis supplies
`documentation_at` and `module_documentation_at` for item and module references.
All return the complete registered Markdown from parsed owning source locations.

Artifact-only and native-only SDK consumers install the same checked registration
records without a source frontend, generated KGR or declaration binary. User-program
KBC/MIR artifacts and linked verification remain independent execution features.

## Checked executable contracts

Source analysis parses native-generated declaration views through ordinary
declaration analysis, then attaches checked registration metadata. Lowering carries
provider-qualified native imports with concrete types, signatures, bounds and
selected callable dependencies. Portable validation proves those facts against
the executable dependency closure. Artifact loading checks the installed entry
against the complete carried declaration; matching names alone are insufficient.

An ordinary generic bound states applicability. A selected callable requirement
additionally states that the Rust body invokes a particular trait member. Linking
resolves that member once for the concrete native application. `cx.selected(slot)`
borrows the prepared callable; invocation does not repeat source lookup or trait
resolution. Implicit language operations and explicit script implementations
retain distinct checked targets.

Runtime bindings and Rust storage factories are local installation data, not
serialized function pointers. Artifacts retain portable identities and contracts.
Executable loading works without HIR, source parsing or generated declaration text.
Retained callables pin their defining code generation across reload.

## Calls, roots and storage

Ordinary native functions and their script callbacks execute synchronously.
`CallContext` exposes checked arguments, selected calls, allocation and execution
checks. `CallableHandle` borrows a callback during its invocation. A payload that
retains a callback uses `StoredCallable` and traces it. Returned script heap values
must be rooted before further allocation or reentry; callback argument packs do
not create a script tuple merely to carry Rust arguments.

Scoped sequence views borrow rooted existing storage. Primitive layouts use
contiguous typed buffers, including empty arrays whose layout is determined by
the declared element type. Heap-reference elements use traced Values. Bulk slice
access does not copy the entire array or allocate a per-call scratch buffer.
Mutable access respects alias/iteration guards and storage revisions.

`SequenceEdit` leases the actual sequence buffer for synchronous mutation. The
lease excludes receiver-slot access through other aliases and restores storage on
success, failure or unwind. Primitive edits do not clone the buffer. Traced values
keep explicit roots while sorting may move elements into Rust scratch space.
Sort, reverse, retain and dedup preserve completed effects; no rollback guarantee
or atomic bulk replacement is required. Receiver identity, generation, iteration
protection and storage revisions remain checked.

`NativePayload` supplies tracing, logical units and ordinary Rust destruction.
Every retained script Value and stored callable must be traced. `NativeStorage`
registers the payload layout and either a checked factory or an explicitly
constructed payload. New opaque types use the generic NativeObject representation;
they do not require new compiler, ABI, verifier or VM type variants.

## Sorting and lazy iteration

Primitive infallible ordering sorts the compact Rust slice directly. Script Ord,
supplied comparators and key selectors run synchronously while editing the actual
buffer. Sorting is stable; key selectors run during comparisons without a caching
prepass. A failed comparison prevents further user comparisons. Rust may finish
internal bookkeeping; all original elements survive, but their order can change.
Length-changing algorithms preserve completed removals. Custom set/remove failures
can leave earlier writes or removals. Comparison counts are unspecified.

Only genuinely retained computations need state. MapIterator retains a
`NativeCursor`, a stored mapper and a reentry guard. Construction performs no map
calls. Aliases share progress. Its next consumes one source item before invoking
the mapper; a mapper failure does not cause that item to be processed again.
The public Option is allocated once for a successful next result.

A payload's `iteration_sources` exposes managed cursor dependencies. Those are also
GC edges. A `for` scope walks this graph, protects source collection structure and
releases its guards on break, return, failure or cancellation. Dropping a scope
permits later mutation while a retained cursor may resume if its source remains
valid. Reentrant next on the same adapter is rejected. The generic execution loop
does not identify an adapter by its library name.

Installed conversion adapter records identify the forward trait/method, result
constructor and associated error members used by checked selection and reverse
conversion. Numeric TryFrom pairs are ordinary registered implementations with
declared Error outputs. Scalar numeric code computes conversion outcomes; it does
not choose a library enum or error declaration. Portable proofs validate the
installed contracts without source analysis.

Option, Result, Ordering, Bound, ParseError, TryFromIntError, Infallible and
ControlFlow are
ordinary enum records owned by their core-library modules. Standard Rust bodies
construct and inspect them through checked declaration handles and pinned nominal
layouts. No semantic type, fixed runtime tag or dedicated instruction inventories
these families. Iterator and comparison contracts retain only the checked symbolic
member bindings they consume. Library authoring handles are cached; layouts and
type arguments remain scoped to the executing generation.

A variant's `reports_failure` fact is explicit checked declaration/layout metadata.
The library sets it on Result::Err, which has exactly one payload field. Construction
captures optional diagnostic origin; checked native library methods copy that
sidecar after validated enum construction. Source enums and ordinary native enum
builders leave the fact false. Equality, hashing and payload access ignore it.

Try and FromResidual are ordinary registered traits; their standard bodies live
in the library alongside the carrier declarations. A native carrier uses
`ModuleBuilder::implement`, `trait_impl`, `associated_type` and `bind_with`, just
like other trait implementations. `CallContext::type_parameter` retains the
selected result's executable scope when constructing its nested residual.
Result FromResidual declares a checked F: From<E> callable requirement. Static
bound methods can be selected or forwarded through the same validated witness
path as receiver methods; no execution-time trait search is added.

Result branch copies origin into its residual, and FromResidual copies it into
the returned failure after conversion. `CallContext::forward_enum_origin` uses
checked live enum values and a validated installed owner; both values must stay
rooted across allocation. The compiler's generic `?` path emits ordinary selected
calls and ControlFlow variant operations, without a Result-specific origin opcode.

## Tooling

Generated `.kgr` is a read-only projection for navigation, completion, signatures
and documentation. Generic binders use `T` for one parameter and `T1, T2, ...`
for multiple parameters. Method-local binders use `M` / `M1, M2, ...` when an
enclosing generic binder already uses `T`, avoiding shadowing. Declarations,
bounds and bodies share this spelling; semantic binder positions remain zero-based.
The generated view is not an independently installable library or a second
signature authority. The compiler parses/lowers this view through ordinary
declaration machinery and checks non-trivia correspondence with registration
before attaching native storage, bindings and default provenance. Registered
trait defaults include real forwarding bodies to typed native helpers; ordinary
body checking must prove those calls before direct native lowering. The SDK
checks unused defaults too, before publishing materialized source files. Declaration
locations and docs come from the same records used by compilation. Standard
registrations own all trait and module documentation. Core and application views
use the uniform renderer and optional materialized declaration cache; navigation
resolves their parsed owning ranges in the exact generated snapshot.
Other generated views include library-owned types,
inherent methods and trait impl methods, including MapIterator.next.

Behavioral and tooling coverage lives in native_provider_reset,
VM library_collections/library_mapping and the independent artifact_features
consumer. [Performance measurements](../performance-baseline.md) record measured
costs; the [roadmap](../implementation-roadmap.md) owns pending work.
