# Native model: frontend boundary remediation and deferred direction

**Implemented:** compilation lowers scoped registrations directly into HIR. Editors
parse published `.kgr` documents alongside user source. Both check the declared
API using the same language rules. Only registration, executable admission and
Runtime carry the evidence needed to bind and execute a Host operation.

The [implemented boundary contracts](#9-frontend-and-linking-boundary-remediation) record ownership,
compiler/linking obligations and independent source-free validation, not a completed checklist
ledger. The [roadmap](implementation-roadmap.md#native-model-replacement) owns validation outcomes,
remaining failures and follow-up sequencing; earlier NM implementation history remains in Git.

[HIR architecture](architecture/hir.md), [Native declarations](spec/standard-declarations.md),
[syntax](spec/syntax.md#native-declarations), [embedding](spec/embedding-api.md) and
[native authoring](native-registration.md) own the lasting implemented contracts. The
[core-model inventory](#core-model-inputs-and-derived-analysis) preserves the Rust ownership
sketches and derivation rules below, without introducing another catalog or authoring API.
Sections 4–6 distinguish proposed provider and build products from current SDK publication.
The [Ref/RefMut removal](#g-remove-script-reference-views) removes the built-in declarations and
scoped-view feature without placeholders. Independent Host paths and Opaque loans remain.
Any replacement type declarations and Runtime invocation remain separate design work; the former
phases 4–5 are not activated.

## 1. Inputs and ownership

```text
compiler: scoped ModuleDecl -> direct HIR ----+
editor: generated documents -> parse/lower ---+-> common checking
both: packaged core and user source -> lower -+
```

Syntax owns one recoverable grammar. HIR owns declaration/type/member identities, resolution,
checking and origin attribution. Analyzer owns position/navigation/documentation queries over
those facts. Rust callbacks, storage adapters, live state and installation authority remain at
the existing registration/Runtime boundary. There is no Runtime-to-HIR dependency, new facade
crate, arena, universal node identity or authoring DSL.

**Implemented boundary:** structured and parsed declarations converge on the same binding-free HIR
and common checking. With the same imports, client code sees the same signatures,
field access, defaults and diagnostics. Origins differ, and only structured inputs
carry the Host and Native registration evidence needed for executable admission.

Required trait members, Script blocks and Native declarations remain distinct. A Native function
means that its implementation is external, not that an implementation is already bound. HIR neither
holds Native entry selectors nor checks binding completeness. Linking consumes checked declaration
identities/contracts plus external implementations and validates missing, duplicate or mismatched
bindings. Default methods use ordinary Script definitions/calls or genuine Native declarations;
there is no dedicated default recipe or forwarding-call table. Runtime's real NativeBinding
objects remain implementation-side installation data, not HIR declaration state.

## 2. Entry into HIR

### Reservation, references and checking

Existing registration records supply construction-time declaration/HIR correspondence. Native
contracts reuse scoped type/operation vocabulary, with recursive semantic checks before publication;
interning a DefinitionId is not proof of a declaration, application, trait bound or implementation.
Exact type references skip spelling selection, never category, arity, bounds, dependency or access
checks. They retain a selected exposure route: a public re-export can expose a public declaration
inside a private module without exposing its defining path. Ambiguous public routes need explicit
selection; a previously selected route is not authority if it disappears from the current scope.

A new public session/reservation/body-builder framework remains a possible future authoring design,
not part of this implementation. Any such framework must reserve actual owners before filling
references, reject foreign/unfinished slots, retain binder ownership and publish no authority on
failure or cancellation. Current checked-library import uses explicit scope remapping.

### Source declarations and ordinary defaults

`native fn`, `native type` and `native host type` parse in the same grammar as ordinary
definitions. Only the last form declares a Host handle; it can declare fields but not
type parameters. `host` remains a name elsewhere. No spelling creates an executable binding. All input representations use common static rules;
implementation availability is not a declaration-authoring permission or an LSP-versus-execution
type system. Library inputs retain complete Script definitions/default bodies, including
Rust-embedded source; authoring location does not change implementation kind. Missing dependencies
are errors, not a reason to fabricate stripped traits or fake defaults.

The editor needs only source documents, their module identities and effective import
scopes. It checks fields, receiver and result types without Rust passing policies.
The compiler also validates registration requirements and concrete results, then
projects Host execution contracts at admission. Neither input route can fabricate
a runtime binding. Common checks derive reserved language roles, String/Range
constructors and the failure origin of `Result::Err` from owner and shape.

A possible future Script-body authoring API may retain actual block source and its exact range,
parsed under the owning signature and binder environment. It must preserve cancellation/limits,
source-view identity and body ownership; generated wrapper functions and string substitutions are
not a replacement for a real fragment entrypoint. That generalized API remains deferred.
Current structured declarations retain complete expression bodies in `ModuleDecl.script_bodies`
using [ScriptExpr](../crates/kagari-types/src/declaration/script.rs): names, declaration references
and calls with explicit type/value arguments. Complete parsed source retains ordinary blocks.
Default authoring supplies ordinary Script methods calling private Native helpers. Module-qualified
helper references prevent same-named parameters from capturing those calls; missing bodies reject.

### Origins, not mandatory generated text

Source nodes retain exact authored ranges. Structured nodes and canonical declarations instead
retain scoped declaration attribution; exact source locations, syntax and written signatures are
optional. The current graph uses an empty logical registration key, explicitly not authored source
or an editable declaration document. Complete-source packaging rejects absent authored manifests;
normal executable compilation, whole-document updates and source-free loading remain valid.
Debug capture omits unavailable positions rather than inventing implementation spans.

SDK preparation consumes structured registrations with or without `declaration_cache`.
The cache opts into publishing generated documents for navigation; without it, only an
explicit tooling request renders them. Neither route makes text installation authority.
Future build data must encode portable library/member attribution and remap identities,
not persist process-local handles.

### Library APIs versus compiler intrinsics

Output is an explicitly installed library. Private core Native helpers link
against [checked authored declarations](native-registration.md#private-core-native-bindings)
rather than frontend name exceptions. The former script reflection helpers are
removed; [script reflection](spec/reflection.md#optional-script-reflection) is a
separate design, not an extension of name lookup.

## 3. Declaration records

Functions, Native types, fields and impls use ordinary owned HIR tables and checked IDs. Native
fields share Field records with Script structs; FieldOwner names the actual struct or Native type.
There is no parallel registration-owned semantic signature catalog. NativeContract owns declared
ordered operation requirements, an optional concrete result and registered Host policy;
parsed Native signatures have no registration evidence. The [language/type owner](../crates/kagari-hir/src/language/types.rs) interprets canonical
String and range constructors independently of storage representation or
Native methods on those types. Common identity/arity/shape checks apply across input routes;
physical storage and Rust adapters remain outside HIR.

### Core-model inputs and derived analysis

The implemented core is the complete binding-free declaration and body model together with its
explicit semantic environment and origins. All retained semantic results derive from these inputs
under common language rules. "Core" names ownership categories below, not a new `CoreHirModel`
container, parallel catalog, crate or authoring framework. Section 9 records the implemented
producer/consumer boundaries; concrete reference access/runtime design remains undecided.

#### Authoritative inputs to retain

The Rust declarations below are ownership sketches of the existing records, not standalone
compilable examples or new public APIs. Imports and visibility are omitted. Declaration collections
retain their existing records; selected-field projections of derived structures below are
explicitly marked and do not authorize dropping unshown semantic facts. These sketches describe
the implemented model; the Ref/RefMut removal boundary is described separately below.

**Module and node storage.** [Module](../crates/kagari-hir/src/hir/item.rs) owns `Module.nodes`,
the shared [HirNodes](../crates/kagari-hir/src/hir/nodes.rs) store.
[LoweredModule](../crates/kagari-hir/src/lower.rs) has exactly the three fields below; no
registration attachments remain on its envelope.

```rust,ignore
struct LoweredModule {
    source: Arc<SourceFile>, // Identity/revision; authored text is optional.
    module: Module,
    input_kind: ModuleInputKind, // Authored text or structured registration.
}

struct Module {
    attributes: Vec<AttributeFact>,
    uses: Vec<UseDecl>,
    items: Vec<Item>, // Top-level declaration order.
    functions: Vec<Function>,
    consts: Vec<ConstItem>,
    statics: Vec<StaticItem>,
    modules: Vec<ModuleDecl>,
    imports: Vec<Import>,
    structs: Vec<Struct>,
    native_types: Vec<NativeTypeDeclaration>,
    type_aliases: Vec<TypeAlias>,
    enums: Vec<Enum>,
    traits: Vec<TraitDef>,
    impls: Vec<Impl>,
    nodes: HirNodes,
    generic_params: Vec<GenericParam>, // Binder records indexed by GenericParamId.
}

struct HirNodes {
    arena: LoweringId,
    blocks: Vec<(HirOwner, BlockData)>,
    stmts: Vec<(HirOwner, StmtData)>,
    exprs: Vec<(HirOwner, ExprData)>,
    places: Vec<(HirOwner, PlaceData)>,
    patterns: Vec<(HirOwner, PatternData)>,
    types: Vec<(HirOwner, TypeData)>, // Declared type references, not inferred types.
    locals: Vec<(HirOwner, Local)>,
}
```

HirNodes is module-wide storage for declaration type references and all function/constant/static
body nodes, not one callable's implementation body. Owner-qualified vectors and lowering identity
are preserved; this introduces neither per-function allocation nor an arena allocator.
A Script function still selects its root with BlockId. BodyOwner, BodyRoot and CheckedBodyInput
continue to name actual semantic body owners, roots and checked views; do not rename them merely
to match the storage container.

The nodes retain child/evaluation order, real Script/default bodies and exact scoped references
with selected exposure inputs. They contain no dedicated default recipes, forwarding substitutes,
executable instructions or live runtime values.

**Callable declarations and static requirements.** Reuse the
[function records](../crates/kagari-hir/src/hir/item/function.rs). Native denotes external
implementation, never installed implementation.

```rust,ignore
struct Function {
    id: FunctionId,
    kind: FunctionKind,
    is_async: bool,
    visibility: Visibility,
    name: String,
    generic_params: Vec<GenericParamId>,
    bounds: Vec<TraitBound>,
    params: Vec<Param>,
    return_type: Option<TypeRefId>,
    implementation: FunctionImplementation,
}

enum FunctionImplementation {
    Required,
    Script(BlockId),
    Native(NativeContract),
}

struct NativeContract {
    host_policy: Option<HostCallPolicy>, // Registration evidence, not a method lookup key.
    // Ordered slots; repeated requirements stay distinct.
    callable_requirements: Vec<NativeCallableRequirement>,
    concrete_result: Option<Ty>,
}
```

There is no Native entry selector, provider identity, callback, codec or ABI adapter in these
records. The receiver’s Rust loan mode is not a HIR method-selection input.
Checked Native operation/result contracts have one semantic owner, as in
[9B](#b-remove-duplicate-registration-contracts-from-the-compiler-handoff).

**Type and member declarations.** Ordinary and Native fields share
[Field](../crates/kagari-hir/src/hir/item/adt.rs); no physical representation is attached.

```rust,ignore
struct NativeTypeDeclaration {
    id: NativeTypeId,
    is_host: bool, // Host handle on either input route; not a storage adapter.
    visibility: Visibility,
    name: String,
    generic_params: Vec<GenericParamId>,
    bounds: Vec<TraitBound>,
    trait_bounds: Vec<TraitRef>,
    fields: Vec<Field>,
}

struct Field {
    id: FieldId, // Includes the actual struct/Native declaration owner.
    visibility: Visibility,
    writeability: Writeability,
    name: String,
    ty: TypeRefId,
}

// Failure origin is derived later, after resolving the owner and payload types.
struct Variant {
    id: VariantId,
    name: String,
    payload: Vec<TypeRefId>,
}
```

Struct/enum/trait/impl records retain binder IDs, inline members and ordinary default links.
Authored binder records live in `Module.generic_params`. HIR checks field type,
visibility and `val`/`var`. The Host marker preserves handle identity in source;
it grants neither a Script constructor nor a loan. [Registered field validation](../crates/kagari-hir/src/analysis/registered_fields.rs)
compares structured fields to their adopted owner; executable path descriptors and
receiver loans belong to [admission](../crates/kagari-hir/src/compile_input/program/registration.rs).

**Identity and origins.** Retain the existing DefinitionContext/DefinitionTable, declaration-to-
definition correspondence, module ownership, lowering-local IDs and explicit scope import/remapping.
A table entry alone proves no declaration or language obligation. There is no registration-only
identity scope, global name lookup authority or universal node ID. Each HIR record carries a
[NodeOrigin](../crates/kagari-hir/src/hir/origin.rs) with its authored position or scoped
structured attribution. Source snapshots retain optional authored text for those positions.

Declared static contracts are essential inputs, not facts inferred from opaque Rust bodies. For
example, an ordered operation requirement or a Host access restriction cannot be reconstructed
from a function's parameter/result types alone. Their checked forms are derived; their declarations
must have an actual input owner. A semantic environment must include the necessary dependencies;
missing declarations remain errors rather than being obtained from a live Runtime.

LoweredModule retains the structural module, source identity and origins, not a registration
attachment bag. This does not require changing its source-less logical-key representation now.
`Module.native_types` contains both ordinary Native and Host handle declarations.
Constructor interpretation and physical representation have separate owners;
[9D](#d-remove-nativetypekind-as-the-mixed-hir-type-model) records their separate owners.

#### Reuse the existing package, module and namespace models

No additional "common module metadata" catalog is required. Reuse the existing
[package environment](../crates/kagari-source/src/package.rs):

```rust,ignore
struct PackageEnvironment {
    scopes: BTreeMap<PackageId, PackageScope>,
}

struct PackageScope {
    dependencies: BTreeMap<String, PackageId>, // Dependency alias -> package identity.
    selection: String, // Selected producer revision/configuration fingerprint.
}
```

[ImportEnvironment and ModuleScope](../crates/kagari-hir/src/imports/environment.rs) retain explicit
module scopes using the same PackageScope vocabulary:

```rust,ignore
struct ImportEnvironment {
    modules: BTreeMap<ModuleIdentity, ModuleScope>,
}

struct ModuleScope {
    packages: PackageScope,
    dependencies: BTreeSet<ModuleIdentity>,
    preludes: BTreeSet<ModuleIdentity>,
}
```

[Input normalization](../crates/kagari-hir/src/analysis/environment.rs) uses explicit scopes where
supplied and otherwise derives producer/package defaults before graph construction. Explicit
package aliases take precedence over dependency-derived spellings. Missing targets and conflicting
prelude selections reject; dependencies grant reachability, never private member access.
[AnalysisInputs](../crates/kagari-hir/src/analysis/inputs.rs) captures explicit scopes and declaration
inputs. Replay also requires the physical source snapshot. Scope changes reset the environment fingerprint, and new requests use the
current inputs. These include actual dependency declarations and complete owned Script bodies,
not copied expected traits or a live implementation registry. ModuleGraph, NamespaceCatalog and
[ModuleImportFacts](../crates/kagari-hir/src/imports.rs) derive the resolved graph, bindings and
reachability from those inputs and the declarations within each request. The derived graph is not
an independent source of dependency declarations.

Package aliases resolve to existing package/module identities. Prelude contributions use the
existing `BindingOrigin::Prelude` and `NameEntry.implicit`; package bindings use
`BindingOrigin::Package`. Preserve strong/glob/implicit precedence, ambiguity and access checks.
Implicit bindings are not fabricated authored imports, duplicate member catalogs or input-origin
permissions. Explicit input dependencies and dependencies observed during resolution both reach
the existing graph; inspecting `use` leaves alone cannot recover the complete input closure.
[9E](#e-remove-loweredmodule-side-maps-by-assigning-actual-semantic-owners) records the common
package/module/import owners.

#### Derived structures and their limits

These are derived records or selected-field projections of existing records, not additional core
input owners. Unshown member, protocol, pattern, constraint and provenance facts remain required.

**Resolution output.** Existing declaration indexes, ModuleGraph, NamespaceCatalog, NameTable and
resolved local/import/exposure facts derive scope lookup and reachability. In particular:

```rust,ignore
struct ModuleImportFacts {
    directives: Vec<ImportDirective>,
    scope: Arc<NameTable>,
    diagnostics: Vec<Diagnostic>,
    dependencies: Vec<ModuleHandle>, // Resolved closure, not new dependency declarations.
}

struct NameEntry {
    strong: Vec<BindingCandidate>,
    globs: Vec<BindingCandidate>,
    implicit: Vec<BindingCandidate>,
}
```

These indexes add no privileged declarations. Each request derives them from its current inputs.

**Checked declarations.** BodySignatures/TypedFunction and AggregateCatalog resolve declared types,
binders, bounds, fields, trait/impl relationships, language roles and static Native contracts.
The following excerpt is in the semantic-signature owner, separate from structural Function above:

```rust,ignore
// Selected-field projection; parameter/binder identities and static contracts remain required.
struct TypedFunction {
    id: FunctionId,
    name: String,
    implementation: FunctionImplementation,
    generic_params: Vec<GenericParameterType>,
    bounds: HashMap<TypeId, Vec<NominalType>>,
    params: TypedParameterBuffer,
    return_type: TypeId,
}

// One checked Native kind, without an installed implementation selector.
enum FunctionImplementation {
    Required,
    Script,
    Native(NativeContract),
}
```

Raw declarations and checked output are different stages, not competing checked catalogs. The
checked Native requirements retain their static contract as in 9B; this projection does not
introduce a second catalog or attachment API. Logical field/variant/member slots are not byte offsets.

**Checked bodies.** [TypeTable](../crates/kagari-hir/src/typeck/table.rs) and
[ModuleFacts](../crates/kagari-hir/src/semantic/body.rs) retain semantic results:

```rust,ignore
// Selected-field projection; existing coercion/protocol/pattern/constraint facts also remain.
struct TypeTable {
    type_refs: HashMap<TypeRefId, ResolvedTypeRef>,
    field_types: HashMap<FieldId, TypeId>,
    exprs: HashMap<ExprId, TypeId>,
    locals: HashMap<LocalId, TypeId>,
    places: HashMap<PlaceId, TypeId>,
    calls: HashMap<ExprId, ResolvedCall>,
    expr_fields: HashMap<ExprId, DefinitionId>,
    place_fields: HashMap<PlaceId, DefinitionId>,
    // Ordinary call facts; no dedicated default-call or implementation binding map.
}

struct ResolvedCall {
    target: CallTarget, // Declaration/member identity or semantic operation category.
    receiver: Option<ExprId>, // Evaluated once before explicit arguments.
    type_arguments: Vec<TypeId>,
    signature: Option<AppliedCallSignature>,
}

// Selected-field projection of checked body facts.
struct ModuleFacts {
    signatures: Arc<BodySignatures>,
    consts: HashMap<ConstId, TypeId>,
    const_values: HashMap<ConstId, ScalarValue>,
    type_table: TypeTable,
    valid_bodies: HashSet<FunctionId>,
    valid_static_initializers: HashSet<StaticId>,
}
```

Open generic binders/projections may be valid results; inference holes are temporary. No callback
address, linked entry, physical layout or ABI recipe belongs in these facts.

**Completion and compiler views.** Diagnostics and positive completion derive from checking;
CheckedCompilation/CheckedModule/CheckedBodyInput admit matching facts and dependency closures.

```rust,ignore
// Selected-field projection of the existing checked view, not another owned catalog.
struct CheckedModule<'a> {
    handle: ModuleHandle,
    lowered: &'a LoweredModule,
    environment: CheckedEnvironment<'a>,
    bodies: HashMap<BodyOwner, CheckedBodyInput<'a>>,
    registration: ExecutionRegistration, // Execution-only projection for this owner.
}
```

Recovered/selected-body facts alone do not authorize compilation. Views check input ownership and
completion, not implementation availability or installed adapters. Retained snapshots and
analyzer/lint projections preserve or read these results in their original semantic environment,
identity scope, source revision, analysis scope and settings. Provider-only changes grant no static
authority. Editor indexes remain analyzer-owned.

The word "derived" does not require rerunning analysis for every consumer, storing every fact twice,
or discarding provenance. Share immutable results and their matching owners. Resolved call/member
facts remain compiler inputs so the backend does not repeat source name resolution or type checking.
Logical field/variant/member slots remain semantic positions, not byte offsets. Recognized compiler
intrinsics can retain their semantic operation category without carrying a runtime helper address.

Not every remaining Rust structure is a retained checking result. Inference variables, worklists,
recursion guards and local solver assumptions are temporary checking workspace; cancellation,
limits and snapshot ownership support analysis. Source revisions and optional origins are
retained input/provenance, not inferred from TypeTable. None of these creates another semantic
source of truth. Physical layouts, ABI/result adapters, actual Rust implementations, final links,
live services and Runtime generations belong outside this model and are supplied/validated later.

#### Derivation invariant and operation example

For the `answer()` example in [section 9](#boundary-and-operation-example), the core contains the
Native declaration, its declared signature, the call structure, identity context and complete
semantic environment. Resolution/checking derives the selected declaration and `() -> i32` call
contract without an implementation set. Supplying or removing a Rust implementation changes
linking's outcome, not those facts. Optional source sites can differ across input representations;
static privileges and diagnostic rules cannot.

Every retained semantic field must therefore identify its authoritative core input and the rule
that derives it. A view must identify the result and validity context it reads. Facts needed
to preserve a guarantee have an actual core owner, rather than an unreviewed side map or removal
under the label "derived". This is the ownership criterion for the implemented A–E boundaries,
not authorization for another HIR storage, query-engine, provider or build-product redesign.

### Native argument semantics and Rust adaptation

An ordinary object parameter carries the existing object identity, not an implicit clone, Rust
reference or ownership transfer. Protected Object/Opaque handles preserve identity. Supported Rust
DTO/Serde extraction is binding conversion: no automatic writeback, alias-graph preservation or
hidden ownership transfer is promised. A mandatory observable snapshot needs its own explicit API.
A structured NativeContract can carry HostCallPolicy; a client call does not need a passing
policy to type-check. Initializer/effect evidence, Rust codecs and scoped borrow validation
stay at their execution owners. HIR has no parallel
passing-mode array, serialization flag or internal Rust-loan model.

### Native operation requirements

Ordered NativeCallableRequirement slots retain receiver, applied trait, actual trait-member identity
and method-owned arguments; repeated slots remain distinct. Signatures come from the member owner,
not a copied parameter/result schema. The shared checker verifies recursive type categories/arity,
scoped binders, owning-trait and associated-output bounds, method obligations and concrete interface
results. Owner-qualified Self obligations must be proved, not deferred like an unapplied declaration.
Rust bodies remain opaque; declared operation evidence is not Runtime name lookup.

### Native fields

Declared fields specify types, visibility and var/val, not Rust offsets or unrestricted references.
Reading an ordinary managed object field has an ordinary object-handle type. Val prevents slot
replacement, not mutation through another permitted alias.
Frontend field support does not implement new Runtime access adapters; unsupported executable
Native-field routes reject explicitly. Existing Host paths keep their own checked access semantics.

### Enum failure-origin metadata

HIR derives the `reports_failure` bit on the checked signature, not the authored variant.
Both declaration routes must resolve the public canonical `Result<T, E>`, its ordered
`Ok(T)` and `Err(E)` members and their binder identities. Only that `Err` is marked.
Registered records and portable layouts retain the bit; verification rejects inconsistent
bits even for private or imported layouts without a local public declaration.

Runtime construction reads the verified bit. A fresh `Err` captures its bounded stack;
forwarding preserves the original trace. See [origin ownership and propagation](spec/error-reporting.md#origin-ownership-and-propagation).

### Ref / RefMut: removal boundary

The standard library no longer declares or exports `Ref<T>` or `RefMut<T>`, and the script
scoped-view behavior is gone. Neither spelling is reserved: a user-declared type with either name
follows ordinary generic rules and does not acquire borrow semantics. `Player.stats: Stats` and
other declarations of the former target types remain unchanged. See
[9G](#g-remove-script-reference-views) for the removed and preserved contracts.

## 4. Checked libraries and Rust providers

**Deferred whole-stack sketch:** not implemented or required by the current HIR scope. A future
registration/preparation redesign should directly produce a usable library; binary encoding is
an optional persistence/build integration choice, not a mandatory render/parse/register cycle.

```text
CheckedLibrary { contracts: Arc<LibraryContracts>, bodies: CheckedBodies }

LibraryContracts {
    definitions: DefinitionTable,
    modules: CheckedModuleMetadata,
    declarations: CheckedDeclarations,
    host_paths: HostPathContracts,
    body_contracts: BodyContracts,
}

NativeProvider {
    contracts: Arc<LibraryContracts>,
    functions: Map<DefinitionId, RustFunctionBinding>,
    types: Map<DefinitionId, RustTypeBinding>,
    paths: Map<HostPathId, RustPathBinding>,
}

RustTypeBinding {
    rust_type: RustTypeId,
    provider: GcProviderBinding,
    access: TypeAccessAdapter,
}
```

LibraryContracts owns checked declarations/relationships and body identities/entry requirements,
not HIR, code, callbacks or whole Programs. CheckedBodies owns frontend body facts separately.
A function's own DefinitionId is its binding identity; generated keys identify its ordinary
encoded definition-table entry, not another Native entry namespace. NativeImportId may still
index an executable import table. Runtime imports/remaps identities before accepting binding keys.

RustTypeBinding supplies a declared NativeManaged implementation, not automatic exposure of a
Rust type. GcProviderBinding uses the existing layout/scan/accounting/drop protocol, not another
heap or duplicate hook registry. Runtime drives roots, loans and mutation publication. Compiled
binding setup creates actual Rust TypeIds/codecs/GC adapters in the process and injects live
services; none are serialized. Only suitably immutable bindings share across recipients.

Admission rejects undeclared/duplicate/wrong-context bindings, missing demanded implementations
and incompatible required declaration/type/trait-reference/path metadata, including metadata unused
by a Native call. An origin marker, fingerprint or serialized checked flag grants no authority.

## 5. Calls, generics and reload

**Deferred execution design:** preserve current call/generic/reload behavior in this HIR change;
no new fixed-link preparation, application cache or provider lifecycle is required here.

| Case | Compilation | Recipient preparation |
| --- | --- | --- |
| Closed call | Check application, choose operations/results and demand closure; specialize where appropriate | Prepare identities, codecs, access and final links before publication |
| Shared entry, known closed environment | Emit shared code plus checked type/operation/construction arguments | Prepare the closed mapping before publication |
| Environment supplied later | Prebuild shared code and environment requirements | Authenticate/map supplied evidence at application admission, before entry; no HIR, trait solver or Rust codegen |
| Repeated admitted environment | No new compilation | Reuse valid prepared facts; retain actual-value/protection checks |

Reuse existing shared MIR semantics: ValueType::Generic describes tagged transport, while
SemanticSlots, binder/bound contracts, operation and construction evidence determine its type
and legality. A physical Generic tag alone is neither a template nor proof. Source-free Runtime
always receives executable shared entries, not a promise to lower a compiler template later.

```text
LinkedNativeCall { binding, signature, mapping, operations, result_adapter }
```

Final links belong to recipient publication/application records. Extend fixed-link preparation
for known closed shared/Native applications rather than only warming an evictable cache.
Keep dynamically supplied applications in existing bounded Runtime caches: exact linked member,
import, full generational environment, selected provider/operation dependencies and protection.
Current descriptor indexes use a 128-entry FIFO bound; do not introduce a global name/type cache.
Eviction/retirement can require preparation again; cached descriptors do not keep active managed
applications alive or grant authority. Retain actual environment/body/provider edges and roots.

NativeCall is a temporary exclusive Runtime activation over protected operands and pending results.
End physical loans before GC/reentry; all exits retire the admitted suffix. Actual values,
generations, loans, roots, cancellation and result publication remain checked. Ordinary calls use
current implementations; closures/Futures/admitted frames retain selected bodies/providers, and
their ordinary inner calls still use current links. Refresh target-dependent mappings on reload.

## 6. Preparation and standard-library scale

**Deferred build/tooling integration:** retained discussion, not this migration's deliverable.
Binary products versus generated Rust tables, producer ownership and startup-cost work need a
separate approved design. Parsing exported declarations is in scope; generating/publishing them
after Cargo builds, directory watching and new binary products are not.

### Private product and compiler/bootstrap boundary

```text
LibraryBuildProduct {
    semantic_key,       // model + explicit config + authored inputs + dependency semantic keys
    target_requirements, // portable applicability constraints, not HOST-selected layouts
    contracts: LibraryContracts,
    executable_code: SharedBytecodeLibrary,
    compiler_templates: MirLibrary,
}

MirLibrary {
    definition_scope, contracts_identity,
    modules: existing_typed_MIR_records,
    typed_imports, body_owners_and_default_applications,
    source_availability,
}
```

These are sections owned by contract/bytecode/MIR, not one large record forced into contract.
Compiler assembles them. Reuse existing MIR instructions, SemanticSlots and scoped types with
library import metadata; do not invent a second body IR. Runtime reads contracts/executable code;
application tooling reads templates. No HIR handles, callback pointers, Rust TypeIds, physical GC
hooks or live Runtime environments are encoded.

The library producer checks every owned Script/default body, even unused ones, and lowers it
under canonical Self/trait/method binders. External dependencies remain typed imports and bound
operations remain evidence slots. Include owned closure/resume bodies, but do not enumerate
impl-inherited defaults or recursively specialize imported bodies just to export one library.
Body/default-application metadata records ownership and relationships, not an all-impl closure.
Executable imports resolve to existing shared executable entries with checked applications. Only
application compilation may create specializations from MIR templates. A source-free link with
missing executable coverage fails; it never invokes the producer or compiler. The existing
program demand scheduler closes actual application executable requirements.

Verify local bodies against explicit import contracts, then verify the complete selected executable
graph after relocation. Factor shared verification, never weaken complete-program checks or emit
placeholder bodies. Symbolic default export and import-aware verification are required compiler
work, not claims that today's program compiler can already emit this product unchanged.

### Build integration shared by stdlib and business libraries

```text
library build.rs -> compiler[source] -> HIR / MIR / bytecode / contract
                 -> dependency canonical build-data sections

library target code -> runtime / contract / bytecode + target dependencies
```

The producer has no production dependency on stdlib or the crate it is building. Owned .kgr/
structured inputs are readable by build.rs without calling that target crate's modules() or
binding constructors. Producer outputs private data and declaration-key glue to OUT_DIR; target
code embeds matching data and compiles adapter setup. No new producer crate or recursive Cargo.

Do not consume a build-dependency's HOST-selected embedded product as if it described TARGET.
Start with one explicit canonical semantic configuration; libraries needing variants expose a
finite explicit configuration set independently of HOST feature selection. The proposed accessor is:

```text
dependency.canonical_data(configuration)
    -> embedded bytes + frontend-free identity/requirement descriptors
```

It returns already-produced canonical contracts/MIR/bytecode, independent of the accessor's
HOST-selected features; it performs no checking, MIR production or binding construction.
The actual TARGET-selected semantic key is exported separately. HOST build-dependencies may
include the frontend; the deployed TARGET normal dependency graph must not.
Consumer build.rs
selects dependency configurations and validates requirements against TARGET/CARGO_CFG_TARGET_*;
relevant features map to explicit semantic inputs, not hidden host cfg! decisions.

Generated target integration checks actual target dependency semantic keys against the selected
keys. Mismatch fails the build; do not guess dependency features, search OUT_DIR paths, or silently
substitute HOST semantics. Physical layouts, Rust code/hooks and native/AOT objects stay target-
local. Target-specific machine code is not part of these canonical data sections.

Cargo tracks author files/configuration and producer dependencies. Compare HOST/TARGET selections
using semantic keys, not entire product envelopes. Semantic target differences require an explicit
semantic configuration. Validate portable target requirements separately; target-specific prepared
code/artifacts carry their actual target/configuration identity. Write outputs only when contents
change. Reuse unchanged builds, invalidate/rebuild stale private data. No public binary compatibility,
legacy reader or migration layer; users neither manage the encoding nor deploy an intermediate
file. Embedded data survives in its matching executable. Existing KBC/ABI contracts remain separate.

### Native API discovery for tooling

The SDK already publishes the editor input; it does not produce the proposed private
library binaries. `engine.native_declaration_sources(project, cancel)` renders the
selected registrations against the project's declarations. The result supplies
logical owners, stable document keys, display locations, text and effective import
selections. [`render_native_document`](../crates/kagari-hir/src/native/render/publication.rs)
checks re-export identity and both namespaces in the solved importer scope.

Without `declaration_cache` the documents stay in memory. Opting into a cache publishes
immutable files; it does not change compiler input. The [editor workspace](../crates/kagari-analyzer/src/workspace.rs)
can replace the generated set and import environment. It analyzes those documents with
user source and packaged core, retaining real ranges for navigation and no registration
IDs. Complete Script defaults remain bodies; only a `TypeDefKind::Host` owner renders
as `native host type`. Consequently publishing `Player` does not change its `===`
or protocol rules, or publish an adapter. See the [SDK publication guide](native-registration.md#analysis-and-declaration-presentation).

Automating discovery after a Cargo build, selecting a declaration directory and
watching it for changes are separate proposals, not implicit SDK or LSP behavior.

### Production versus installation

In the proposed private-product design (not the current publication API):

| Stage | Work |
| --- | --- |
| Host/library build | Check fixed inputs, export typed templates/shared code, compile Rust adapters |
| Business script compile | Finalize actual source dependencies, check calls/applications, select operations and demanded products |
| Detached preparation | Validate/share immutable products without recipient authority |
| Install/publication | Inject live services, import identities, match Provider, prepare necessary mappings and atomically link |
| Invocation | Execute prepared adapters with actual-value and execution checks |

Runtime never reparses/rechecks fixed library declarations. Installing a large library is not
promised O(1): decoding/verification, recipient identities and final links remain necessary.
Demand closure includes layouts, interface slots, callbacks, result adapters and retained
computations; arbitrary Rust callbacks do not imply no dependencies. Share immutable data where
valid, not Runtime authority. Some dynamic environments still need first-admission preparation.

## 7. Replacement boundaries and acceptance

Phases 1–3 implement the binding-free frontend/compiler handoff, ordinary default bodies and
separate Runtime implementation/authentication boundary described below. Native fields and
equivalent structured/parsed analysis do not imply new Runtime field adapters, provider products,
binary formats, GC adapters or startup improvements. Reflection/output behavior, roots,
cancellation and current-versus-retained reload selection retain their existing owners.

Exact source packaging, installed implementation agreement, semantic authentication
and malformed-input rejection are separate obligations. The [roadmap](implementation-roadmap.md#native-model-replacement)
owns follow-up sequencing; this document does not record test counts or claim
performance or backend/platform acceptance.

## 8. Review decisions and limits

Implemented decisions are unified Native syntax, ordinary HIR ownership, checked exact
exposures, real/optional source origins and common static analysis without binding state.
Review repairs preserve module-qualified helper selection under parameter shadowing, explicit
package-alias precedence, canonical constructor identity and shape checks, and
conversion rejection for extra obligations unsupported by intrinsic derivation. These are common
semantic rules, not input-origin privileges. Source-free materialization provenance and independent
Runtime authentication are distinct contracts, detailed in 9F; neither is inferred from a passing
signature check or an artifact-supplied digest.

Remaining directions require separate authorization: registration/provider products, reusable
preparation and generic/link caches, binary versus generated-code prebuild representation,
producer/bootstrap and HOST/TARGET configuration, standalone MIR/shared libraries, automatic
directory discovery/watching, and any replacement for the script reference views.
Reference representation, access API and lifetime policy remain undecided. Existing
preparation/workflow is active, not an obsolete compatibility reader. The roadmap owns the removal
checkpoint; it does not schedule a replacement reference feature.

## 9. Frontend and linking boundary remediation

A–F record the implemented phases 1–3 ownership and review-repaired contracts. Their headings
remain stable link anchors, not pending work or a completed checklist ledger. G records the implemented
reference feature removal; sections 4–6 discuss deferred provider/build designs.

### Boundary and operation example

```text
structured declarations ------------------+
exported .kgr -> parse --------------------+-> common HIR -> common semantic checking
user source -> parse ---------------------+                       |
                                                       checked declaration/call facts
                                                                 |
external Native implementations -------------------------------> linking
                                                                 |
                                                        executable publication
```

For `pub native fn answer() -> i32;`, both input formats accept the same well-typed call
`val value: i32 = answer();` without looking up a Rust entry. The checked signature remains Native
in both cases. A later execution request fails linking if the implementation is missing, duplicated
or incompatible. Supplying a matching implementation permits linking without changing the HIR or
rechecking types under a registration privilege. A source navigation location may be unavailable
for structured input; that is not unavailable semantic evidence.

This is not authorization to weaken executable verification or admit unsupported Native field
operations. Backend capability checks and linking checks still apply at their proper boundaries.
No new provider/prebuild product, Runtime-to-HIR edge or public registration DSL is required.

### A. Remove registration-dependent HIR state and rules

**Implemented ownership.** [LoweredModule](../crates/kagari-hir/src/lower.rs) retains source,
module and input kind. [HirNodes](../crates/kagari-hir/src/hir/nodes.rs) stores module-wide nodes
with their origins and preserves semantic body identities and owner/lowering checks. HIR has no
installed Native entry selector or registered/external permission bit.

[Checked signatures](../crates/kagari-hir/src/semantic/signatures.rs) distinguish Required, Script
and Native(NativeContract), without a bound/unbound semantic split. Common declaration, body,
application and bound checking needs no implementation registry. Coherence and conversion
eligibility use the [package orphan rule](../crates/kagari-types/src/declaration/ownership.rs).
A local trait suffices. For a foreign trait, scan the receiver then trait arguments:
require a local input type and no uncovered impl parameter before it. `From`/`TryFrom`
follow this rule, including for Host types. A Host result must still be a genuine handle.
Overlap, access, bounds and signature agreement are separate; inherent impls have their own rule.
[Compiler admission](../crates/kagari-hir/src/compile_input/program.rs) requires a checked complete
dependency closure, positive body evidence and supported executable operations. A
source-declared Host owner fails with `MissingHostSchema`, even when fieldless and unused.
The compiler needs the structured owner, not its rendered text or a live Rust callback. [Lint](../crates/kagari-lint/src/lib.rs) uses the root package's analysis
scope rather than registration status.

### B. Remove duplicate registration contracts from the compiler handoff

**Implemented ownership.** [NativeContract](../crates/kagari-hir/src/hir/item/function.rs) and its
checked signature retain registered operation slots, optional concrete result and Host policy.
[CheckedModule](../crates/kagari-hir/src/compile_input/program.rs) exposes checked environments
and body views. Its [execution projection](../crates/kagari-hir/src/compile_input/program/registration.rs)
remaps Host schemas, paths, bound path operations and receiver loans into the checked
identity scope. None determines whether a script receiver matches a method.

[Compiler ABI projection](../crates/kagari-compiler/src/source/lower/abi.rs) derives executable
Native declarations from these checked facts. Applied signatures, bounds, Host policy, ordered
operations, effects and result adapters retain independent executable validation. Implementation
selectors and Rust adapters remain composition/linking inputs. Expected source-trait agreement
does not require copying expected traits into HIR. The [compiler](../crates/kagari-compiler/src/source/program/trait_requirements.rs)
derives portable trait requirements from checked owners. The [SDK](../crates/kagari/src/engine/pins.rs)
supplies mandatory public trait identities for participating importers and compares the
emitted contracts with its installation pins. [Runtime matching](../crates/kagari-runtime/src/native/registry/matching.rs)
independently validates installed implementations.

### C. Replace NativeDefault recipes with ordinary definitions and calls

**Implemented ownership.** [Default authoring](../crates/kagari-runtime/src/native/builder/default_lowering.rs)
supplies an ordinary Script method expression and a separately declared private Native helper.
[ModuleDecl.script_bodies](../crates/kagari-types/src/declaration/module.rs) retains the complete
ScriptExpr, including explicit helper type/value arguments; parsed inputs retain complete blocks.
[Structured lowering](../crates/kagari-hir/src/native/structured/items.rs) and
[rendering](../crates/kagari-hir/src/native/render.rs) qualify same-module helper references so
parameters cannot capture them. Missing bodies reject rather than becoming success stubs.

Resolution, [ordinary call facts](../crates/kagari-hir/src/typeck/table.rs), checked body views and
[generic instance planning](../crates/kagari-compiler/src/source/lower/instances/calls.rs) carry
defaults through compilation. Trait, receiver, method and associated-output substitutions remain
checked; Required members stay Required and genuine Native defaults stay Native declarations.
Dedicated default applications, forwarding-call tables and portable default recipes are removed.
Unpublished affected products are disposable: no compatibility reader or routine format/ABI bump
is introduced. Source-free materialization and semantic authentication have the distinct owners in 9F.

### D. Remove NativeTypeKind as the mixed HIR type model

**Implemented ownership.** [Language constructor identities](../crates/kagari-types/src/declaration/native.rs)
and [HIR type interpretation](../crates/kagari-hir/src/language/types.rs) select String and Range semantics independently of Native storage classification.
[Common declaration checking](../crates/kagari-hir/src/language/types.rs) checks visibility,
arity, fields, bounds and Host conflicts at the canonical definition. Actual declarations establish constructor ownership; a table entry
or same-named user type alone is insufficient. Impl ownership derives from these checked owners
and ordinary nominal identities, never registered/external source privileges.

[NativeTypeDeclaration](../crates/kagari-hir/src/hir/item/adt.rs) owns identity, binders, bounds,
fields and the `is_host` category on both routes. Physical layouts and Rust
storage/access adapters belong to executable representation and installation. Unsupported Native
field execution still rejects. [9G](#g-remove-script-reference-views) removes the script wrapper
route; it does not select replacement access semantics.

### E. Remove LoweredModule side maps by assigning actual semantic owners

**Implemented ownership.** Static facts now belong to the common inputs and actual declarations:

| Fact | Owner and contract |
| --- | --- |
| Dependency edges, aliases and preludes | [ImportEnvironment/ModuleScope](../crates/kagari-hir/src/imports/environment.rs) with existing PackageScope; [normalization](../crates/kagari-hir/src/analysis/environment.rs) precedes graph construction. Explicit aliases take precedence; accessible prelude bindings use ordinary implicit tiers. |
| Resolved edges and exposure | ModuleGraph/ModuleImportFacts and the [namespace catalog](../crates/kagari-hir/src/imports/catalog.rs); explicit and observed dependencies retain reachability without granting private access. |
| Re-exports and failure origin | Publication checks the solved targets in both namespaces before rendering `pub use`. The checked canonical `Result::Err` signature supplies `reports_failure`. |
| Conversions and defaults | The checked forward conversion signature identifies the relationship. Omission inherits a default; a matching explicit impl wins, and a mismatched signature rejects. |
| Host and Native calls | The declared receiver, parameters and result govern client calls; registered operation slots, concrete results and Host policy are not editor inputs. |
| Language roles and constructors | Canonical declaration ownership plus [independent role/shape checks](../crates/kagari-hir/src/language/items/shapes.rs); no foundation flag, attribute allowlist or registration privilege. |

Conversion relationships must name actual accessible targets; extra supertraits/bounds unsupported
by intrinsic derivation reject. [Immutable analysis inputs](../crates/kagari-hir/src/analysis/inputs.rs)
retain explicit scopes; the physical documents are supplied separately. New requests use the current scopes and non-syntactic
contracts, which are also reflected in the
[environment fingerprint](../crates/kagari-hir/src/analysis/input_fingerprint.rs). Optional source
presentation grants no additional static authority.

### F. Adapt producers and consumers across the pipeline

**Implemented pipeline.** [Native assembly](../crates/kagari-runtime/src/native/assembly/draft.rs),
stdlib and external adapters supply ordinary declarations/static contracts and complete Script
bodies. [SDK declaration preparation](../crates/kagari/src/engine/declarations.rs) supplies these
inputs separately from execution implementations. It renders documents only for
publication or an explicit tooling request. Analyzer and lint read checked client
facts and their origins; they do not consume installed entries.

The compiler consumes [checked environments and bodies](../crates/kagari-hir/src/compile_input/program.rs),
projects [executable ABI contracts](../crates/kagari-compiler/src/source/lower/abi.rs) and plans
ordinary generic/default calls without rereading a registration catalog or re-resolving source.
Generic substitution, associated outputs, ordered effects, demand closure, unsupported-capability
rejection and Native imports remain explicit. The static `concrete_result` constraint preserves
the `C: I` proof and applied interface selection through
[compiler result planning](../crates/kagari-compiler/src/source/lower/instances/results.rs),
[portable adapter validation](../crates/kagari-contract/src/native_import/result.rs) and
[Runtime result checks](../crates/kagari-runtime/src/native/result.rs); invocation does not run a
trait solver to recover missing evidence.

**Source-free provenance is not semantic authentication.** Portable interface `method_origins`
associate an impl-owned placement with its canonical authored trait member.
[Contract verification](../crates/kagari-contract/src/types/verify.rs) checks those relationships,
applied signatures/bounds, placement and implementation identity.
[Materialization verification](../crates/kagari-contract/src/types/materialization.rs) ties placed
Script identities to portable source/body records already checked against instructions by
[bytecode program verification](../crates/kagari-bytecode/src/program.rs). These checks establish
internal consistency and materialization provenance; they do not authenticate installed library
semantics. An attacker can change a product's body, origin and digest together. A Script-kind or
signature match, coherent origins or a self-consistent instruction certificate is not an
independent authority for an inherited installed body.

[Runtime Script authentication](../crates/kagari-runtime/src/native/registry/scripts.rs) retains
the installed module's canonical ScriptExpr and function contract independently of the product.
It reads an actual bytecode call/value trace and compares callee identities, argument flow,
ordered calls, type applications, signatures and returned value with that installed expression.
Trait/receiver/method binders are applied before comparison; associated projections and families
are normalized through the checked applied impl's outputs, with unrelated projections left
symbolic and invalid/cyclic transforms rejected. Authentication does not parse source, invoke HIR,
trust compiler assertions about expected instructions, or rerun trait selection. Unsupported
instructions/effects reject rather than being skipped. A product claiming an inherited installed Script body must match its canonical
expression. An explicit impl selects its own method instead. Ordinary authored traits
without an installed owner do not require registration. This authenticator covers the current ScriptExpr grammar,
not arbitrary future block-source semantics. Portable source/body evidence requires no physical
source file or complete authored-source manifest.

[Native linking](../crates/kagari-runtime/src/native/registry.rs) independently matches installed
entries and [ordinary Native materializations](../crates/kagari-runtime/src/native/registry/matching.rs).
Runtime's actual NativeBinding objects, storage/access adapters, value checks, roots, loans,
result publication, reentry/trap cleanup and generation ownership remain valid execution-side
mechanisms. Missing demanded implementations, duplicates, wrong-context bindings and signature,
policy, ordered-operation or result mismatches remain errors. Cancellation and ordinary-current
versus retained-body reload selection are unchanged. Runtime depends on neither HIR nor a compiler
to manufacture absent executable products.

These adaptations introduce no compatibility reader, routine ABI/format bump, provider/prebuild
product, directory watcher or new reference operation. Wider Native backend/platform
support remains deferred; executable validation and the roadmap's known failures are not waived.

### G. Remove script reference views

**Implemented boundary.** Script can no longer receive an external Rust `Player` through the
former `RefMut<Player>` wrapper and call its projected `heal` method. The
[SDK example](../crates/kagari/examples/scoped_host.rs) instead uses a nominal `Player` backed by
runtime-owned Opaque storage. There is no replacement type, placeholder, alias, or compatibility route.

The removal spans these layers as one checkpoint; no runnable carrier remains without its source
or executable safety rules.

1. **Standard library and HIR.** The foundation, prelude and declaration exports no longer supply
   the built-in declarations or re-exports. Both structured and source-backed inputs drop their
   special constructor checks, field and method projection, implicit narrowing and carrier-specific
   escape/suspension rules. Ordinary generics and other language constructors remain;
   Host handle lifetimes and loans have separate execution checks.
2. **Compiler, verification, Runtime and SDK.** The dedicated reference type, transport, lowering,
   verifier rules and Rust-to-script lending entrypoints are removed. Independent executable
   validation must not admit the removed carrier or reinterpret it as another type in an artifact.
   `HostScope` and `execute_scoped` remain for routes such as
   [manual Host adapters](../crates/kagari-stdlib/src/tests/execution/native_boundary/native_boundary_conversion/host_adapter.rs).
   `Opaque<T>`, Host paths and checked short Rust loans retain their ownership, aliasing, reload and
   cleanup guarantees. `receiver_access` still describes binding/access checks, not wrapper lookup.
3. **Consumers and acceptance.** Examples and specs use the retained APIs. Discard affected
   unpublished artifacts/caches; no compatibility reader or routine ABI bump.
   Check both declaration routes no longer provide the standard names and same-named user types act
   like ordinary types. Retain focused source-free and actual Host/Opaque call regressions,
   preserving distinct loan and cleanup assertions. Checks using the retired wire encoding were
   temporary migration probes, not maintained historical fixtures. Run the repository's local
   structure, fmt, workspace all-target Clippy and diff gates; full workspace tests and exhaustive
   matrices remain CI-only.

**Deferred, not part of removal:** whether types such as `Player` should later supply their own view
and methods, how declarations are generated or named, and how Runtime binds those calls and checks
handle lifetime/access. No public generic wrapper, per-type view, implicit conversion, or change
to `Player`'s fields is required now.

#### Rust receiver loans and script method selection

`player.heal(10)` is selected by the declared receiver and Kagari signature, not by
whether Rust uses a direct payload loan or asks a context to edit the receiver.
Registration and linking must agree on the installed storage and binding contract.
Runtime checks the actual handle, loan conflicts and lifetime when granting access;
a method signature grants neither an unrestricted reference nor a loan across `await`.

### Sequencing, acceptance and remaining limits

Phases 1–3 close the binding-dependent frontend/default/classification replacement described by
A–F. 9G removes the script reference views in a separate checkpoint. Sections 4–6 retain deferred
provider/build/prebuild designs. The removal does not activate a replacement reference feature.

The lasting acceptance invariants are:

- The [core-model inventory](#core-model-inputs-and-derived-analysis) owns retained static inputs
  and derived facts' validity contexts. No parallel checked catalog or cache grants authority.
- Equivalent input representations use common identities after scope remapping, declaration/body
  checking, bounds, default selection, field policy and diagnostics; optional origins may differ.
- Malformed applications, wrong roles, stale/foreign handles, private exposure bypasses, coherence
  violations and unmet obligations reject through common rules and independent semantic oracles.
- Compilation consumes supported complete checked facts and required registration evidence
  without a live implementation registry.
  Linking validates installed implementations and claimed inherited Script bodies independently of the
  product's internally coherent provenance, including applied associated projections.
- Ordinary defaults and generic/associated operations retain their Host/GC, roots/loans, actual
  returned-value, cancellation and reload guarantees. Runtime still enforces actual
  Host handle ownership, access and lifetime; unsupported capabilities reject.

The [roadmap](implementation-roadmap.md#native-model-replacement) owns follow-up work.
A design invariant is not validation evidence. Full-workspace tests and exhaustive
backend/platform matrices remain CI-only.

## References

Mechanisms inspected in local Rust commit **5f4effd579500175af844241eb179e756190f1e7**:

- [ForeignItemKind / TraitFn](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_hir/src/hir.rs): explicit bodyless foreign signatures and ordinary required/provided trait bodies; no machine callbacks in HIR.
- [Definition construction](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_ast_lowering/src/lib.rs): record node/definition correspondence during lowering, not rendered-text joins.
- [MIR export](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_metadata/src/rmeta/encoder.rs): should_encode_mir/encode_mir export reusable generic compiler data separately from code generation.
- [Mono collection](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_monomorphize/src/collector.rs): instance collection is a distinct concern; do not eagerly enumerate generic defaults.

Borrow those mechanisms, not Rust's arena lifetimes, pointer ABI, object-safety limits or source
span conventions. Kagari needs shared generic interfaces, managed roots, exact source projections
and reload selection. Build semantic configurations are an explicit Kagari Cargo-integration rule,
not a claim that rustc's metadata format solves HOST/TARGET selection for us.
Execution safety remains with [Native calls](architecture/runtime/calls.md#native-call-prepared-slots-and-mutable-services),
[reload](architecture/runtime/reload.md), [Rust storage](../crates/kagari-runtime/src/native/storage.rs)
and the [GC provider protocol](../crates/kagari-gc/src/object.rs). No performance benefit is proven.
