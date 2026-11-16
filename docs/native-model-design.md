# Native model: frontend boundary remediation and deferred direction

**Status: frontend architecture acceptance is reopened.** NM01–NM04 implemented direct lowering,
explicit Native syntax, fields and sampled analysis/execution paths, but did not finish the
reviewed ownership replacement. HIR still carries registration/binding state, NativeDefault recipes,
a mixed Native type classification and parallel registration contracts. Passing tests for those
paths do not establish the corrected architecture.

The user's follow-up establishes the binding-free HIR boundary below. The
[remediation inventory](#9-frontend-and-linking-boundary-remediation) is the single current
corrective-work list; recording it does not implement its unchecked items. The
[roadmap](implementation-roadmap.md#native-model-replacement) owns activation/status. Earlier NM
phase commits and validation evidence remain in Git history; their completion claim is not current
architecture acceptance.

[HIR architecture](architecture/hir.md), [Native declarations](spec/standard-declarations.md),
[syntax](spec/syntax.md#native-declarations) and [embedding](spec/embedding-api.md) describe existing
implementation surfaces. Where their registration-dependent contracts conflict with this reviewed
target, they require the updates listed below rather than redefining the target to match code.
Sketches in sections 4–6 remain deferred product/build/reference discussion, not prerequisites for
removing binding state from HIR or claims about currently emitted library products.

## 1. Inputs and ownership

```text
Rust ModuleDecl records ------------------+-> ordinary HIR -> shared semantic analysis
external .kgr plus static metadata -> parse+                         |            |
user source -----------------------> parse+                    compiler      analyzer
```

Syntax owns one recoverable grammar. HIR owns declaration/type/member identities, resolution,
checking and origin attribution. Analyzer owns position/navigation/documentation queries over
those facts. Rust callbacks, storage adapters, live state and installation authority remain at
the existing registration/Runtime boundary. There is no Runtime-to-HIR dependency, new facade
crate, arena, universal node identity or authoring DSL.

**Reviewed target:** structured and parsed declarations converge on the same binding-free HIR
and common checking. LSP analysis and preparation for execution differ in input representation,
not static permissions, binding requirements or resulting semantic signature kinds. Origins and
physical source availability may differ; declaration/type/member facts and diagnostic rules must
agree under the same semantic environment.

Required trait members, Script blocks and Native declarations remain distinct. A Native function
means that its implementation is external, not that an implementation is already bound. HIR neither
holds Native entry selectors nor checks binding completeness. Linking consumes checked declaration
identities/contracts plus external implementations and validates missing, duplicate or mismatched
bindings. Default methods use ordinary Script definitions/calls or genuine Native declarations;
the current NativeDefault recipe/forwarding machinery is a replacement gap, not the target.

## 2. Entry into HIR

### Reservation, references and checking

Existing registration records supply construction-time declaration/HIR correspondence. Native
contracts reuse scoped type/operation vocabulary, with recursive semantic checks before publication;
interning a DefinitionId is not proof of a declaration, application, trait bound or implementation.
Exact type references skip spelling selection, never category, arity, bounds, dependency or access
checks. They retain a selected exposure route: a public re-export can expose a public declaration
inside a private module without exposing its defining path. Ambiguous public routes need explicit
selection; a retained route disappearing invalidates reuse rather than choosing another identity.

A new public session/reservation/body-builder framework remains a possible future authoring design,
not part of this implementation. Any such framework must reserve actual owners before filling
references, reject foreign/unfinished slots, retain binder ownership and publish no authority on
failure or cancellation. Current checked-library import uses explicit scope remapping.

### Source declarations and ordinary defaults

Explicit `native fn` / `native type` parse in the same grammar as ordinary definitions. Native
spelling cannot create executable bindings. Any declaration-authoring admission policy must remain
separate from implementation availability and cannot create an LSP-versus-execution type system.
The current registered/external semantic split is listed for removal below. Library inputs retain
complete Script definitions/default bodies, including Rust-embedded source; authoring location does
not change implementation kind. Missing dependencies are errors, not a reason to fabricate stripped
traits or fake defaults.

ExternalModule accepts source plus scoped static sidecars for language constructors, Host schemas/
paths, receiver/override restrictions, ordered operation/result requirements and foundation/
conversion trait contracts. These describe analysis semantics only. They carry no Rust callback,
provider key or executable binding. Actual foundation and Host inputs use the common checker;
constructor ownership conflicts reject across the complete mixed environment. The current API
loads these inputs explicitly. A filesystem exporter, persisted sidecar encoding and directory
publication/watching remain future integration work.

A possible future Script-body authoring API may retain actual block source and its exact range,
parsed under the owning signature and binder environment. It must preserve cancellation/limits,
source-view identity and body ownership; generated wrapper functions and string substitutions are
not a replacement for a real fragment entrypoint. A generalized body-authoring API remains
deferred, but NativeDefault recipe removal is now an explicit remediation item. Preserve complete
ordinary default bodies through existing source inputs rather than retaining recipes until a new
builder API exists.

### Origins, not mandatory generated text

Source nodes retain exact authored ranges. Structured nodes and canonical declarations instead
retain scoped declaration attribution; exact source locations, syntax and written signatures are
optional. The current graph uses an empty logical registration key, explicitly not authored source
or an editable declaration document. Complete-source packaging rejects absent authored manifests;
normal executable compilation, whole-document updates and source-free loading remain valid.
Debug capture omits unavailable positions rather than inventing implementation spans.

Default SDK preparation consumes structured registrations. Explicit declaration_cache materialization
retains the existing parsed/validated source-view route for navigation. This does not introduce a
new exporter or make rendered text installation authority. Future build data must encode portable
library/member attribution and remap identities, not persist process-local handles.

### Library APIs versus compiler intrinsics

Output/reflection extraction is deferred. Preserve existing print/log, type_of, get_field,
set_field and set_index semantics, declared access and evaluation order. A frontend model cleanup
is not permission to redesign these APIs or silently admit unsupported executable operations.

## 3. Declaration records

Functions, Native types, fields and impls use ordinary owned HIR tables and checked IDs. Native
fields share Field records with Script structs; FieldOwner names the actual struct or Native type.
The target has no parallel registration-owned semantic signature catalog. Currently NativeContract
and registered_declarations duplicate operation/result facts, while TypeConstructor still derives
its classification from NativeTypeKind. Both gaps are explicit remediation items. Builtin
interpretation must belong to the language/type owner: String, ranges and references retain their
existing semantic types and identity/arity rules, irrespective of Native methods on those types.

### Native argument semantics and Rust adaptation

An ordinary object parameter carries the existing object identity, not an implicit clone, Rust
reference or ownership transfer. Protected Object/Opaque handles preserve identity. Supported Rust
DTO/Serde extraction is binding conversion: no automatic writeback, alias-graph preservation or
hidden ownership transfer is promised. A mandatory observable snapshot needs its own explicit API.
HostCallPolicy, initializer/effect evidence, codecs and scoped borrow validation remain at their
existing boundaries; HIR has no parallel passing-mode array or serialization flag.

### Native operation requirements

Ordered NativeCallableRequirement slots retain receiver, applied trait, actual trait-member identity
and method-owned arguments; repeated slots remain distinct. Signatures come from the member owner,
not a copied parameter/result schema. The shared checker verifies recursive type categories/arity,
scoped binders, owning-trait and associated-output bounds, method obligations and concrete interface
results. Owner-qualified Self obligations must be proved, not deferred like an unapplied declaration.
Rust bodies remain opaque; declared operation evidence is not Runtime name lookup.

### Native fields

Declared fields specify types, visibility and var/val, not Rust offsets or unrestricted references.
Reading an ordinary managed object field has an ordinary object-handle type; it does not insert
Ref wrappers. Val prevents slot replacement, not mutation through another permitted alias.
Frontend field support does not implement new Runtime access adapters; unsupported executable
Native-field routes reject explicitly. Existing Host paths keep their own checked access semantics.

### Ref / RefMut: recognition now, behavior later

Ref/RefMut use ordinary type-application parsing and identity-based constructor resolution. Their
semantic records retain the exact target and Shared/Mutable access, with existing target/position
eligibility and execution checks. No new borrowing, conversion, projection, forwarding, escape or
alias-analysis capability is implemented. Ordinary Native self is not a Rust receiver loan.

Future reference behavior may expose scoped location handles whose actual overlapping loans are
checked at callback entry, not continuously held Rust references. A future child projection must
preserve parent capability, path overlap, scope/generation and cleanup; val constrains replacement
rather than blanket object immutability. Whether and how reference methods forward remains open.
These directions cannot silently become callable trait requirements or field-execution acceptance.

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

**User decision:** keep private binary prebuild products for compilation/Runtime, but deliver
build-generated .kgr declarations to LSP. These are different outputs from the same selected,
checked library model; LSP does not require binary contracts, MIR, bytecode or Rust bindings.

```text
Host/library build -> common declaration checking
                   +-> private contracts / shared bytecode / MIR -> compiler and Runtime
                   +-> exported .kgr declarations + docs         -> analyzer

Rust declarations --------------------+-> common input -> HIR -> checking (compilation)
exported .kgr -> declaration parser ---+-> common input -> HIR -> checking (LSP)
user source joins the corresponding common analysis in both routes
```

LSP reads the declaration directory; proposed default: <project-root>/target/kagari/native-api/.
A project override is shared by build integration and analyzer. No individual binary artifact
paths need configuring. Only necessary library/module/dependency/configuration attribution
accompanies the declaration files; it is not a second binary semantic input requirement.

- Build integration renders the selected Host project's library/dependency set from the same
  checked declarations used for private products. Pure compilation/Runtime never needs to parse
  this text; generating it is a separate build output for tooling, not a required semantic stage.
  Do not inspect Runtime memory or execute target Rust registration callbacks for offline LSP.
- Analyzer loads .kgr on startup and reloads on publication, parses external declarations, and
  analyzes them with user source through common HIR. Definition locations point into the exported
  read-only documents; analyzer owns navigation indexes. Parsed IDs are scoped to its own context,
  not imported raw process-local IDs from the build. No second type checker or Rust binding
  validation runs in LSP; it does not establish executable/provider authority.
- Preserve module identity, visibility, fields, generics/bounds, impl relationships and every
  caller-relevant static contract. Export complete Script definitions owned by the library,
  including Rust-embedded traits, functions, structs and real default bodies, alongside Native
  declarations. Already available source dependencies remain referenced without duplicate owners.
  No body stripping, default-stub grammar or Rust-compiled-string-to-Native conversion is allowed;
  report genuinely missing dependencies rather than fabricate definitions.
- Refresh when declarations, dependencies, configuration or exported documentation change;
  implementation-only edits need not rewrite identical declarations. Isolate configurations and
  select explicitly, never by newest timestamp. Removing a declaration removes its analyzer API.
- Stage output and atomically publish a complete snapshot after selected build/key checks succeed.
  build.rs alone cannot certify enclosing build success. Failed builds retain explicitly
  last-successful declarations. Missing/invalid exports produce an actionable build/configuration
  diagnostic, not an empty Native API. Loading exports cannot enable Native user-source authoring.
- Tooling files are disposable and unnecessary for execution deployment. Binary prebuild products
  remain separately embedded/consumed and independently verified; .kgr is not their replacement.
  Directory refresh does not authorize a new LSP scheduler or incremental compiler.

Direct structured lowering and explicit offline declaration analysis are implemented; see the
[HIR input contracts](architecture/hir.md#native-declaration-inputs-and-origins). Build export,
directory publication/configuration and watch/refresh integration remain deferred. Their export
encoding must preserve the current static sidecars, not only pretty declaration signatures.

### Production versus installation

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

The frontend replacement is not architecturally complete. Acceptance requires the binding-free
boundary and inventory below, not merely successful paths through the retained registration model.
Native fields and equivalent structured/parsed analysis do not imply new Runtime execution,
provider products, binary formats, GC adapters or startup improvements. Ordinary defaults,
reflection/output, source-free validation, roots, cancellation and reload keep their guarantees.

Representative current contracts are independently asserted at the HIR/analyzer/compiler/SDK
owners. Exact source packaging, installed binding agreement and malformed input rejection remain
separate from successful frontend analysis. The roadmap records actual local checks and CI limits;
no full-workspace, exhaustive backend/platform or performance acceptance is implied.

## 8. Review decisions and limits

Current decisions are explicit unified Native syntax, ordinary HIR ownership, checked exact
exposures, real/optional source origins and identical static analysis without binding state.
Existing model rustdoc describes current records, not proof that registration-dependent records
satisfy this target. No renaming, wrapper conversion or moved check can replace the ownership change.

Remaining directions require separate authorization: registration/provider products, reusable
preparation and generic/link caches, binary versus generated-code prebuild representation,
producer/bootstrap and HOST/TARGET configuration, standalone MIR/shared libraries, export-sidecar
encoding, directory publication/refresh, and new Ref behavior. Existing preparation/workflow is
active, not an obsolete compatibility reader. Preserve these discussions while keeping follow-up
scope finite at the existing roadmap owner.

## 9. Frontend and linking boundary remediation

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

For `pub native fn answer() -> i32;`, both input formats must accept the same well-typed call
`val value: i32 = answer();` without looking up a Rust entry. The checked signature remains Native
in both cases. A later execution request fails linking if the implementation is missing, duplicated
or incompatible. Supplying a matching implementation permits linking without changing the HIR or
rechecking types under a registration privilege. A source navigation location may be unavailable
for structured input; that is not unavailable semantic evidence.

This is not authorization to weaken executable verification or admit unsupported Native field
operations. Backend capability checks and linking checks still apply at their proper boundaries.
No new provider/prebuild product, Runtime-to-HIR edge or public registration DSL is required.

### A. Remove registration-dependent HIR state and rules

**Owners:** lower.rs, native/api.rs, typeck/check.rs, typeck/constraints.rs,
semantic/signatures.rs, aggregates/implementations.rs, compile_input/, imports/ and affected lint/
cache consumers under [kagari-hir](../crates/kagari-hir/src/lower.rs).

- [ ] Remove registered_native_api, external_api and their registered/external/library status
  accessors from LoweredModule. Preserve actual module/dependency metadata and optional origins,
  not source-format-specific semantic authority.
- [ ] Remove native_functions and HIR NativeBinding. Native declarations carry no installed entry
  or NativeDefault binding selector; implementations are supplied separately to linking.
- [ ] Replace checked-signature Native(binding) versus NativeDeclaration with one Native semantic
  kind. Retain Required/Script/Native distinctions, resolved signatures and genuine static contracts.
- [ ] Remove type checking's missing-binding/implementation-binding agreement branches. Restore those
  requirements at linking using checked declaration identities and the actual implementation set.
- [ ] Remove the blanket offline-declaration compilation rejection. Compilation consumes the same
  checked facts regardless of input representation; absence of bindings is a linking failure.
- [ ] Remove registered_predicate's source-origin-dependent bound-target relaxation. Specify/check
  the same supported bound forms through both inputs, preserving all actual bounds and obligations.
- [ ] Remove engine_owned and the separate external_type_owned/external_conversion_owned privilege
  paths. Derive impl coherence, builtin defining ownership and conversion eligibility from common
  declaration identities/rules, not how the declarations entered HIR.
- [ ] Replace registration-based cache/lint/import branches with actual semantic inputs, explicit
  analysis scope and module metadata. Preserve invalidation, visibility, ambiguity and cancellation.

### B. Remove duplicate registration contracts from the compiler handoff

**Owners:** [compile_input/declarations.rs](../crates/kagari-hir/src/compile_input/declarations.rs),
[native/api.rs](../crates/kagari-hir/src/native/api.rs), and compiler source/lower/ ABI, callable
requirement and result-adaptation consumers.

- [ ] Remove LoweredModule.registered_declarations, RegisteredDeclarations, CheckedModule.registration
  and registration-specific accessors. Split their contents rather than discarding validations.
- [ ] Give operation requirements, result constraints and generic templates one checked semantic
  owner. Compiler planning/portable emission consumes that owner, not the parallel registration copy.
  Current NativeContract/registered_declarations agreement by construction is not sufficient.
- [ ] Move entry selection, actual adapters and execution representation to implementation/linking
  inputs. Preserve independent signature, Host policy, effect and executable agreement checks.
- [ ] Move implementation-side expected source-trait matching obligations to composition/linking.
  Ordinary trait definitions/references, dependencies and selected exposures stay in common analysis;
  copied expected traits do not replace actual owners or grant private-path access.
- [ ] Remove native_definitions as an attachment-only identity scope. Retain DefinitionTable and
  explicit remapping at the common input/analysis owner; structured origins must still resolve IDs.

### C. Replace NativeDefault recipes with ordinary definitions and calls

**Owners:** [callable.rs](../crates/kagari-types/src/callable.rs), HIR structured lowering,
typeck/check/native_defaults.rs, typeck/table.rs, aggregate defaults, compile_input/ and compiler
source/lower/instances/defaults.rs, plus affected registration/wire/Runtime consumers.

- [ ] Remove NativeDefaultApplication, CallableImplementation::NativeDefault and NativeBinding::Default.
- [ ] Remove recipe-based synthetic forwarding-body construction, NativeDefaultCheck and the
  dedicated TypeTable native-default call records.
- [ ] Remove NativeDefaultCall, CheckedModule.native_defaults and dedicated compiler default
  selection/expansion/ABI emission based on those records.
- [ ] Replace associated dedicated serialization/mapping/verification records after their consumers
  use ordinary bodies/call facts. Unpublished products are disposable; no compatibility alias,
  legacy reader or routine format/ABI bump is required.
- [ ] Retain real Script default definitions and ordinary calls to declared Native helpers. Genuine
  Native-provided default methods remain supported without carrying an installed binding in HIR.
  Required methods must not become defaults or acquire fake bodies during migration.

### D. Remove NativeTypeKind as the mixed HIR type model

**Owners:** [native.rs](../crates/kagari-hir/src/native.rs), language/types.rs, native input producers,
imports/type interpretation, aggregate signatures and compiler ABI/representation consumers.

- [ ] Remove NativeTypeKind and TypeConstructor::registered(NativeTypeKind) from HIR semantics;
  moving its match to language/ while retaining the registration classification is not completion.
- [ ] Interpret String, Range and Reference through the language/type owner with exact constructor
  identity, arity and eligibility checks. Native methods do not create nominal Native String/Range.
- [ ] Keep ordinary Native declaration identity, generic parameters, fields and methods on their
  actual declaration/checking owners, independent of storage-management choices.
- [ ] Retain caller-visible Host path/access/capability and target restrictions as explicit static
  contracts shared by both inputs. Do not silently turn Host capabilities into unrestricted managed
  objects or delete safety distinctions merely to remove the old enum.
- [ ] Move physical storage layout and Rust implementation details to compiler representation or
  linking/Runtime ownership as applicable. Preserve unsupported-operation rejection and source-free
  verification; this remediation does not implement new field adapters or reference operations.

### E. Remove LoweredModule side maps by assigning actual semantic owners

| Current attachment | Required disposition |
| --- | --- |
| native_dependencies | Common module/dependency input; retain reachability and dependency checks. |
| native_package_alias | Common package/module metadata, not registration authority. |
| native_prelude | Explicit common prelude participation metadata. |
| native_import_namespaces | Namespace category on the actual import/export record; remove ordinal side mapping. |
| registered_enum_failures | reports_failure on variant declarations/checked variants; preserve error-origin behavior. |
| native_trait_adapters | Static conversion relationships on the trait semantic owner; actual Rust adapters remain outside HIR. |
| method_policies | Separate trait override semantics, caller-visible capability restrictions and adapter-internal Rust loans; assign each to its actual owner. |
| registered_traits / external_traits | Remove input-route-specific full trait catalogs; retain common actual definitions and necessary independent language-role/shape validation. |
| native_trait_references | Common semantic dependencies/exposures versus implementation-side expected matching, split as in B. |
| language_foundation | Remove origin-granted authority; retain common role identity, shape, uniqueness/completeness checks. |
| native_types | Replace the mixed type/schema/representation map as in D, not a renamed wrapper. |
| native_attributes | Delete the production-unwritten span allowlist and its attribute-check bypass; retain actual attribute validation. |

- [ ] Migrate each live reader/writer to its stated owner; do not delete facts merely because the
  old attachment disappears. Both source and structured inputs must express the same static facts.
- [ ] Remove the unused registered_enum_failure accessor, dangling readonly Map/Set comment and
  registration-authority documentation. Do not preserve forwarding accessors or new hidden side maps.
- [ ] Normalize structured and parsed frontend entrypoints into the common semantic environment.
  Rendering remains optional presentation; registration/source correspondence checks cannot remain
  a backdoor for binding authority or a second semantic catalog.

### Sequencing, acceptance and remaining limits

Implement the corrected common semantic owners and update their producers/consumers cohesively;
then remove binding-dependent checking/handoffs, NativeDefault recipes and mixed type interpretation.
Check intermediate build boundaries explicitly rather than adding compatibility wrappers, unchecked
success, disabled admission or fake bodies. Review docs/tests alongside the owning code. This list
is not a new general Runtime/provider redesign or activation of the deferred sketches above.

Required behavioral acceptance:

- Equivalent structured/parsed inputs produce the same declaration identities after explicit scope
  remapping, types, bounds, member/default selection, field writeability and diagnostic kinds.
  Optional source locations and input versions are allowed to differ, never static privileges.
- HIR/type checking/checked callable signatures need no Native implementation registry. Linking a
  missing, duplicate or mismatched implementation fails without changing those semantic facts;
  matching implementations succeed under the existing execution safety contract.
- Malformed applications, wrong roles, foreign/stale handles, private exposure bypasses, coherence
  violations and unmet obligations reject identically through both routes. Independent role/shape
  oracles remain; removing duplicate registration catalogs must not make checking circular.
- Ordinary defaults, generic/associated operations and results, actual Native calls, Host access,
  DTO/handle identity, roots/reentry/trap cleanup, cancellation, reload and source-free validation
  retain their owner-level assertions. Unsupported new capabilities remain explicitly unsupported.
- No NativeDefault recipe or registration-derived NativeTypeKind remains in the replaced model.
  Semantic constraints have one checked owner; compiler emission does not read a parallel copy.

Inspect and strengthen existing tests rather than mechanically duplicating them. Run focused affected
contracts plus the required structure/fmt/workspace Clippy/diff gates at implementation checkpoints.
Full-workspace and exhaustive feature/backend/platform tests remain CI-only. Historical passing NM
checks are evidence for unchanged behavior, not passing evidence for this unimplemented remediation.
The known NC missing-Vec fixture remains separately owned; do not weaken it or substitute a provider.
This documentation update runs content/link/diff checks only and makes no new build/test claim.

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
