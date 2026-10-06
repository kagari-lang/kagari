# Kagari Architecture

This document describes current ownership and execution boundaries. Language
behavior is defined in [the specifications](README.md#language-and-execution-specifications).
[The roadmap](implementation-roadmap.md) owns pending work; proposals below are
explicitly marked and do not describe implemented behavior.

## Architectural Principles

- Kagari is a statically typed, GC-backed language for Rust-hosted applications.
- Recoverable syntax/HIR supports incomplete source and tooling. Only checked
  facts enter executable lowering; backends do not resolve syntax or traits.
- Verified MIR is the common backend handoff; bytecode is its interpreter target.
- Executable loading and verification work without source analysis.
- Rust host state and the script heap have separate ownership and access rules.
- Calls retain exact dependency generations across hot reload.
- Left-to-right once-only evaluation, checked arithmetic, trap order, completed
  effects and cleanup are preserved across interpreter and native execution.
- Installed interfaces determine available APIs. Cancellation and call-depth
  protection remain; there is no generic permission matrix or execution charging.

## Workspace Shape

| Crate | Current responsibility |
| --- | --- |
| kagari-common | Portable definition identities/tables/mappings, debug spans, cancellation and bounded decoding |
| kagari-types | Semantic types, declarations, symbolic defaults, generic checks, numeric/collection/range semantics and offline host schemas |
| kagari-source | Source documents/revisions, diagnostics, literal grammar, line indices and navigation provenance |
| kagari-syntax | Lexer, parser, CST and AST views |
| kagari-hir | Recoverable analysis, resolution, typing and tooling queries |
| kagari-abi | Physical value representations, helper ABI, native code descriptors and memory lifetime |
| kagari-contract | Executable envelopes, checked call/layout/operation contracts, physical lowering and linked verification |
| kagari-mir | Typed CFGs, verification, bounded analyses/passes and portable encoding |
| kagari-compiler | Checked source lowering, specialization, bytecode emission and native links |
| kagari-bytecode | Interpreter model, independent verification, codec and artifact envelope |
| kagari-codegen | Compilation-only verified MIR interface |
| kagari-codegen-cranelift | Scalar native emission and executable code ownership |
| kagari-runtime | Values, GC, host state, sessions, native registration/calls and reload |
| kagari-vm | Interpreter/frame driver, debugger and prepared tier selection |
| kagari-embed | Host SDK, preparation and execution orchestration |
| kagari-cli | Arguments, filesystem IO and presentation |

Runtime, bytecode and VM have no production dependency on MIR, source/syntax/HIR or
codegen. Types depends only on common among workspace crates. MIR depends on
contract/ABI/types/common. Contract depends on ABI/types/common; ABI
depends only on Serde and has no semantic model, foundation catalog or renderer.
Compiler core works without `source`.
Backends depend on codegen/MIR/ABI and backend libraries, not runtime, compiler,
bytecode or SDK. ABI has no frontend/build-time source generator dependency.
Source-based dev-dependencies do not change those production constraints.
LLVM remains deferred; no placeholder backend crate is required.

Native artifacts split physical `kagari-abi::native::NativeArtifact` from
contract-owned executable function identity, logical stack maps and debug metadata.
`NativeCompilationProduct` retains the executable page owner alongside that checked
envelope. Register/Local stack-map entries are logical slots, not a physical native
GC protocol. Semantic-to-slot lowering belongs to contract. Native `.kgr` rendering
belongs to HIR tooling and the SDK's `source` feature; runtime installations expose
checked declaration records without generating text.

Shared declaration records belong to types, including `FnDecl`, `TraitDef`,
`TypeDef`, `ModuleDecl` and native callable requirement templates. Implementation
tags retain symbolic declaration/default links; they contain no Rust entry,
physical signature or selected callback. Contract owns concrete native imports,
interface execution tables, selected instances and linked proof catalogs. Types
checks declarations, binders, substitutions, trait ancestry and applications
against explicitly supplied records. Module declaration validation also receives
an explicit receiver-ownership lookup; the existing bundled provider supplies
canonical ownership during the remaining registration migration.

HIR depends only on source, syntax, types and common among workspace crates.
`AnalysisDatabase::default()` has no installed library. Callers supply the complete
registration closure with `set_native_modules`; `analyze_source` takes the same
explicit provider vector, and `native::render::declaration_source` takes its
provider slice. Installed package aliases, an explicitly marked prelude and array
interfaces come from these records. Language-role validation compares checked
signatures with the supplied registration declarations. The SDK and independent
producers currently install the bundled foundation explicitly; LR moves its
concrete authoring owner into stdlib. Older snapshots retain their declarations
when the current database replaces its registration inputs.

## Compilation Pipeline

```text
source database / snapshots
  -> syntax and recoverable HIR analysis
  -> checked program
  -> compiler source lowering and bounded reachable monomorphization
  -> verified concrete MIR with sealed analyses
       -> compiler core bytecode emission -> verified bytecode
       -> codegen + explicit helper links -> owned native product

.kbc envelope (bytecode + optional portable MIR)
  -> SDK PreparedProgram (validation and, with native, canonical MIR correspondence)
  -> runtime-specific linking and immutable module versions
  -> interpreter, or explicit native preparation/install/execute
```

HIR owns source meaning and incomplete-source queries. MIR owns concrete typed
operations, explicit CFG/effects/origins and checked execution facts. Compiler owns
lowering, not script execution. Backends consume the verified handoff without
recovering semantics from bytecode or re-resolving syntax. Runtime owns shared
execution services; the VM supplies the interpreter frame driver.

HIR scalar facts represent every integer, including i32, as an i128 payload with
an integer-only `kagari-types::scalar::IntegerType` tag. Checked construction
enforces the selected type's range; constant evaluation uses the shared integer
operations. Compiler lowering selects physical constants from that checked tag.

Bytecode-owned `VerifiedBytecodeProgram` retains resource-bounded graph verification
in an immutable, non-serializable value. Consuming artifact validation checks the
entire open envelope and retains that seal. Native preparation decodes and verifies
MIR, lowers a bounded comparison candidate and requires exact canonical equality
with the sealed bytecode before publishing native input. Equality includes float
bits, dependency identities, layouts, imports, effects, roots and debug metadata;
the comparison candidate is never returned as executable code. Ordinary compiler
emission still independently verifies every generated bytecode graph.

Runtime `VerifiedProgram` adopts this bytecode-owned seal and shares immutable code
across runtime instances. Host bindings, heap state, authority and installed native
handles remain local to each runtime. Mutable extraction discards the seal;
decoded or changed inputs are bounded, validated and sealed again. See
[artifacts](spec/artifacts.md) and [module loading](spec/module-loading.md).

## Language contracts and native implementations

The installed `core`/`alloc`/`std` foundation defines all 40 foundation
traits, primitive/value declarations, standard enums, range forms, String and the
canonical Vec/HashMap/HashSet types. See [the current trait inventory](spec/builtins.md#foundation-trait-inventory).
These declarations remain available independently of optional libraries.
`[T]` means List<T>; list literals create Vec. There is no separate
fixed-length array type or Rust slice promise. Readonly views are shallow.
HashMap/HashSet require checked Eq + Hash keys and use Rust standard hash storage
without a traversal-order guarantee. Collection and String behavior lives in
[collection access](spec/collection-access.md) and [builtins](spec/builtins.md).

Application and library modules use explicit ModuleBuilder declarations and
scoped implementation blocks. `bind` checks Rust codecs against Kagari signatures;
`bind_with` supplies explicit codecs. Rust signatures do not infer Kagari traits.
`finish` checks the declaration/binding/storage closure; installation validates
dependencies and publishes atomically. Runtime construction starts empty. Engine
construction explicitly installs standard modules through the same builder path
as application modules, retaining independent heaps, host state and generations.

All 40 standard traits are explicit registrations owned by `kagari-stdlib`.
Complete generated modules pass through the declaration parser and ordinary HIR
lowering. The renderer records identity/range mappings while producing source;
parsed attribute nodes identify language roles on their owning declarations.
There is no handwritten-core extraction, source splice or declaration binary.
Header collection binds LangRole to declaration IDs;
signature completion validates installed origin, uniqueness, required roles,
binders, parents and member types before body selection. The 26 core signatures
belong to explicit standard registrations, checked against the installed records
and the compiler requirements.
Rust library records under `kagari-stdlib::catalog` generate declaration views
that use the ordinary declaration parser and HIR lowering. Non-trivia syntax is
checked against authoritative registrations before native storage, bindings and
default metadata attach. Core traits retain role/shape validation. Artifacts carry
declarations, native imports, signatures and selected witnesses; loading checks
them against installed implementations without parsing source. The ownership model below preserves source-free execution.
See [native declarations](spec/standard-declarations.md) for the registration API.

## Crate responsibility target

CR01-CR02 separate shared semantic models from executable contracts; LR01-LR03
complete unified library registration. This ownership supersedes AC01-AC05's
combined semantic/executable contract model. HIR depends on explicit semantic
providers; stdlib owns concrete declarations and implementations. The
[execution roadmap](implementation-roadmap.md) records checkpoints and validation.

| Crate | Target responsibility |
| --- | --- |
| `kagari-common` | Spans, cooperative cancellation and portable/scoped definition identity, tables and mappings; no host or language API inventory |
| `kagari-types` | Source-independent types, signatures, traits, generic/member constraints, module declarations/docs, semantic role metadata, offline host schemas, numeric semantics and declaration-level checks |
| `kagari-source` | Source documents/revisions, line indices, diagnostics, literal grammar and navigation provenance |
| `kagari-syntax` | Lexer, parser, CST and AST |
| `kagari-hir` | Recoverable resolution, inference, trait selection, semantic checking and source queries; rendering explicit registered semantic declarations |
| `kagari-compiler` | Checked HIR to MIR lowering, bounded specialization, semantic-to-execution conversion, bytecode emission and backend link preparation |
| `kagari-mir` | Lowered typed CFGs/instructions, analyses, passes, verification and portable MIR encoding |
| `kagari-contract` | Executable call/native dependencies, logical slots/layouts, operations/effects, semantic-to-physical mapping, executable envelopes and shared linked verification |
| `kagari-abi` | Physical values, calls, helper ABI, backend descriptors and executable ownership interfaces |
| `kagari-bytecode` | Interpreter instructions/programs, bounded artifact encoding and independent verification |
| `kagari-codegen` | Compilation-only backend interface over verified MIR and owned native products |
| `kagari-codegen-cranelift` | Cranelift machine-code emission |
| `kagari-runtime` | Values, GC, roots, host state/borrows, native builders/bindings, linking, versions and execution services |
| `kagari-vm` | Interpreter/frame driver, call dispatch and debugger control |
| `kagari-stdlib` | Concrete core/alloc/std declarations, docs, exports, Rust bindings and algorithms |
| `kagari-embed` | Engine/SDK composition of registrations, compilation, loading, preparation and execution |
| `kagari-cli` | Arguments, filesystem IO and presentation |

The names identify responsibilities; reducing dependency count is not an
acceptance criterion. MIR legitimately connects semantic records and execution
representations. HIR does not need execution models to understand a declaration.

```text
types     -> common
source    -> common
syntax    -> source, common
HIR       -> source, syntax, types, common
contract  -> types, ABI, common
MIR       -> types, contract, ABI, common
bytecode  -> types, contract, ABI, common
runtime   -> types, contract, ABI, bytecode, common
stdlib    -> types, runtime registration/checked execution API
```

Types has no production dependency on ABI, contract, frontend or runtime. Common
has no reverse semantic dependency. ABI has no semantic, source or runtime
dependency. HIR has neither direct nor transitive execution-layer dependencies.
Compiler's source feature consumes HIR; compiler core, backends and runtime keep
their source-free boundaries. Codegen/backends consume MIR/contracts/ABI, not
runtime or stdlib. Embed composes the required subsystems and stdlib; standalone
analysis receives declarations without installing a runtime. Generic runtime,
HIR, compiler, MIR, bytecode and backends do not depend on concrete stdlib.

Pure `Ty`, nominal/generic/trait/function/member records, ModuleDecl/docs and
general type operations belong to types. Collection access, range forms, numeric
evaluation and offline host declarations belong to focused types modules. Numeric
evaluation stays shared between constant evaluation and runtime so casts, widths, overflow and trap behavior remain consistent. Span and
definition identity remain usable by runtime/debug metadata without pulling in
source documents or analysis.

Split mixed records rather than moving entire files mechanically. Declaration
models can describe symbolic defaults and storage/role capabilities without
owning selected executable targets, machine representations or Rust pointers.
Concrete native imports, selected callback/result adapters, interface execution
tables and linked dependency verification remain in contract. Pure type
substitution and declaration-level constraint checks belong to types; checks
that consume executable dependency records remain in contract. Runtime builders
produce semantic declarations plus separate execution bindings. Host schemas
describe declared access and passing; actual Rust borrow state remains runtime-local.
Bounded codecs follow their records rather than creating a shared artifact catalog.

`Ty` has no execution representation method. Contract exposes explicit
semantic-to-slot lowering used by compiler and executable verification. HIR's
inference/unknown/error types remain local; checked portable types describe a
different stage and need not absorb incomplete-source analysis state. HIR converts
checked declarations to shared semantic records without depending on execution.

```text
user KGR + complete generated declaration KGR
  -> syntax -> HIR checked program
  -> compiler lowering/specialization -> verified MIR
       -> compiler bytecode emission -> verified bytecode
       -> codegen/backend -> owned machine code

registered semantic declarations -> generated KGR -> HIR
registered Rust entries/storage  -> runtime installation
```

Populate the two new owners directly; do not introduce forwarding crates,
compatibility aliases, extra host/numeric/trait crates or new MIR stages. First
complete CR01's semantic ownership, then CR02's consumer/dependency boundaries,
then LR01-LR03's concrete stdlib registration. Required intermediate build failures
are bounded and recorded in the roadmap; CR02 closes its carried errors before LR01.

## Unified library registration

This registration path supersedes AC02/NS01's source-authority and embedded
trait-product choices. The preceding crate target defines the semantic and
execution split; the [roadmap](implementation-roadmap.md) records implementation
and acceptance.

Standard and application native libraries supply the same kind of module:
explicit Kagari declarations, documentation, dependencies and checked Rust
bindings/storage. Engine construction assembles these modules through one
registration path. Default construction registers the bundled standard library;
the generic runtime and HIR do not discover or inject another foundation catalog.
Builder-time registration is the initial integration point. Live mutation of an
already-used engine's registrations is a separate lifecycle decision.

```text
bundled standard modules       application native modules
             \                 /
              Engine registration
              /                 \
checked declaration snapshot    runtime-local native installation
              |                 |
generated .kgr documents        Rust entries and storage bindings
              |
ordinary parser/HIR analysis + cached files for editor navigation
```

Registration records are the signature authority. The source-enabled engine
projects its installed declarations into `.kgr` documents before source
analysis. HIR resolves library and user calls against those documents and checks
source/registration correspondence before attaching native metadata. Generated
documents include traits, types, functions, impls, public re-exports, documentation
and navigation sites. They are analysis views, not executable library bodies.
Native-only and artifact-only consumers install the same records and entries
without rendering or parsing source. Successful registration already provides
their declarations; there is no additional metadata acquisition step.

For editor/LSP use, the tooling host materializes these documents as real `.kgr`
files in a configurable generated-source cache, for example
`target/kagari-declarations/kgr-native-v1/<content-hash>.kgr`. The SDK returns
normalized absolute file paths and precise ranges in the same rendered content;
LSP adapters encode those paths as file URIs at their protocol boundary. Analysis can retain an
in-memory snapshot; materialization does not require rereading files to recover
registered declarations. Filesystem IO belongs to the tooling/host integration,
not types, contract, HIR's renderer or generic runtime.

Cache identity includes the declaration/documentation content and renderer
version. Unchanged content reuses a stable path; changed content receives a new
snapshot path so existing navigation ranges retain their original text. Publish
complete files before exposing navigation targets and retain files referenced by
active snapshots. Generated files are disposable views marked as generated;
editing them does not modify native signatures or Rust bindings. Both standard
and application modules use this materialization path. In-memory embedding does
not require filesystem access, while editor integration supplies navigable files.

### Documentation in registered modules

Documentation is part of the native registration authoring API for standard and
application modules. Module, trait, type, free-function and method builders expose
a consistent `documentation(...)` operation accepting full Markdown text. Existing
registered member kinds, including associated types/constants, fields and enum
variants, must also carry documentation where exposed. Callers attach docs through
the owning builder or typed member reference, without editing an internal identity
map. Multiline literals and `include_str!` of library-owned Markdown are supported
authoring choices; there is no separate generated documentation authority.

Render module documentation as inner `//!` comments and item/member documentation
as attached `///` comments. Preserve paragraphs, headings, lists, links, blank
lines and fenced Kagari examples. Extend source documentation queries to recognize
module docs as well as item docs. The cached file, hover and declaration-document
queries use the same complete text; a summary may be derived for completion but
must not replace or truncate the stored documentation. Generated positions account
for doc lines so navigation still targets the correct declaration/member.

Standard-library docs explain behavior, parameters/results, relevant failure and
mutation semantics, and useful examples rather than repeating signatures. Preserve
and extend the existing core docs when converting them to registrations. Trait
member docs remain available through implementation navigation; explicit impl docs
take precedence over inherited member docs. Re-export navigation preserves the
canonical declaration and its documentation. Module overview docs explain how its
types and functions are used together.

Documentation-only changes invalidate the rendered-source/documentation cache,
without changing executable type identity, native binding compatibility or ABI
versions. Keep documentation attached to authoring records; executable validation
must not depend on Markdown contents. This requirement covers registration APIs,
generated files and analysis queries; it does not add a documentation website or
an automatic doctest runner.

### Registration API

The Engine builder uses the same explicit-signature/binding model as standalone
native libraries. Registration and configuration operations mutate the builder;
`build` consumes it and seals the module set.

| API | Behavior |
| --- | --- |
| `KagariEngine::builder() -> NativeResult<KagariEngineBuilder>` | Construct a registration builder and install `kagari_stdlib::modules()` through the ordinary batch installation path; report any failure |
| `builder.declarations() -> &DeclarationCatalog` | Read the checked providers already registered with this builder; no global foundation fallback |
| `ModuleBuilder::new(identity, providers)` | Construct one native module using an explicit provider catalog; existing `with_modules` supports additional explicit providers |
| `builder.install(module) -> NativeResult<()>` | Validate and atomically add one completed module to the builder's registrations |
| `builder.install_all(modules) -> NativeResult<()>` | Stage a module set and validate its complete dependency closure before publishing, including mutually dependent standard modules |
| `builder.declaration_cache(path)` (source feature) | Configure SDK tooling materialization of generated KGR files; pure rendering remains in HIR |
| `builder.build() -> Result<KagariEngine, EmbeddingError>` | Check required language bindings, seal the registration set and prepare source views when enabled |
| `engine.native_declaration_sources()` (source feature) | Return the complete registered source views and their published locations, including docs and navigation sites |

Use mutable builder operations for registration/configuration. `build` consumes
the builder. `ModuleBuilder::finish` checks declarations, storage and bindings
before producing a `NativeModule`. Its provider snapshot must be explicit and
consistent with the engine's installed providers. Reusable libraries can return
completed modules for `install`/`install_all`; they need not depend on the SDK.
Stdlib exposes `modules() -> NativeResult<Vec<NativeModule>>` using runtime's
registration types and internally staged declarations, with no SDK dependency.

`KagariEngine::new(config)` remains the default-library convenience, implemented
through this builder path rather than through a separate foundation constructor.
The builder is the explicit fallible entrypoint. An empty staging builder used
internally is not a new no_std engine mode. Generic Runtime construction starts
with execution services only; Engine installs its sealed modules into each runtime.

Module/trait/type builders expose `documentation(&mut self, text)`; FunctionDecl
and MethodDecl expose chainable `documentation(self, text) -> Self`. These accept
full Markdown. Type registration still requires checked NativeStorage and payload
tracing, and method implementations retain explicit signature/binding checks.
Methods and associated members keep typed references for configuration; docs must
survive finalization instead of being discarded when a method lowers its signature.
Module overview text has an explicit module-level field on ModuleDecl; item docs
remain attached to their declaration identities in authoring metadata. No Markdown
fields are added to physical ABI records or executable function signatures.

During `build`, source-enabled SDK tooling renders from the sealed declarations.
When a cache directory is configured, it publishes complete files and associates
their URIs with the same analyzed snapshots before exposing navigation. Without a
directory it retains in-memory documents. Cache IO/errors belong to this SDK
tooling integration, not to module registration or runtime execution; SDK errors
retain the failing path and underlying cause. CLI source commands supply `target/kagari-declarations`; LSP hosts configure a
directory through the builder. Embedded hosts can choose either mode.
Standalone HIR accepts explicit declaration records and source origins from its
caller without depending on Engine or stdlib.

### Responsibility boundaries

| Owner | Target responsibility |
| --- | --- |
| `kagari-common` | Spans, cancellation and definition identity infrastructure |
| `kagari-types` (new, populated during CR01) | Shared semantic types/declarations/docs, generic constraints, role metadata, offline host schemas and declaration-level validation |
| `kagari-contract` | Executable call/layout/operation contracts, linked validation, representation conversion and bounded executable encoding |
| `kagari-abi` | Execution representations, physical native calls, helper signatures and executable ownership interfaces |
| `kagari-stdlib` (new, populated during LR01) | Bundled core/alloc/std declarations, docs, prelude/re-export inventory, native bindings and library algorithms |
| `kagari-runtime` | Generic native module builders, registration checks, linking, heap/host services and execution state |
| `kagari-embed` | Assemble and validate the engine's module set, supply declarations to analysis and install matching implementations in runtimes |
| `kagari-hir` | Render registered declarations, analyze generated/user source, collect checked language roles and provide tooling queries |

`TraitDef` is a shared types model; a particular Add or List declaration is an
instance owned by its library. Neither types nor contract constructs the standard
library, owns its public API inventory or requires a default global catalog.
Split semantic and executable fields/algorithms at their actual responsibility
boundaries; this migration does not create one crate per type or verifier.

The concrete stdlib crate owns the existing declarations and implementations,
rather than forwarding another crate's catalog. It depends on types and the
runtime registration API. Embed depends on stdlib. HIR, compiler, bytecode and
generic runtime have no production dependency on stdlib. Source-only tooling
receives declaration records from its caller; it does not install an execution
runtime to discover APIs. Runtime context operations needed by stdlib must remain
generic checked operations; moving code cannot expose unchecked heap internals.

LR01 moved the declaration recipes, library-owned namespace inventory, native
bindings and bundled collection algorithms to `kagari-stdlib`. Runtime construction
is empty until modules are installed explicitly; Engine construction selects and
installs standard modules through the mutable registration builder. Namespace
ownership follows consumer responsibility: public
library paths belong to stdlib, while executable ownership validation consumes
checked declaration/representation bindings. Semantic language-role metadata
belongs to types; primitive execution facts and executable role bindings belong
to contract when required by source-free verification. Neither is a library API
inventory. HIR consumes semantic records through explicit provider inputs.

### Core traits and registration authority

The 26 core trait signatures are ordinary explicit standard-library registrations,
alongside the other 14 traits. Their KGR analysis views use the same renderer as
application modules, retaining complete documentation and navigation. There are
no independent handwritten core trait declarations, embedded declaration binary,
checked-in generated snapshot or build-time frontend.

Complete rendered modules use the ordinary declaration parser and HIR lowering.
Language attributes are recognized through parsed attribute nodes and their
owning declarations, followed by origin and shape checks. Selection and navigation
never search for attribute spellings, blank-line delimiters or copied source text.

The renderer records declaration identities and output ranges while producing
text; parsed syntax nodes supply semantic declaration/member spans. Navigation
uses those ranges in the exact generated snapshot, including its cached file,
without searching for attribute spellings or declaration text. Documentation
paragraphs, fenced examples and blank lines cannot delimit or truncate a trait.
Compiler-recognized roles still need explicit checks for origin, identity,
uniqueness, completeness, binders and the members required by language semantics.
Registration supplies the declarations and checked role/representation bindings;
HIR collects them from trusted generated views. A copied `#[lang]` attribute or a
same-named application trait cannot acquire reserved authority. Shape validation
uses the installed declaration and the compiler's actual semantic requirements,
not a previous binary product. Ordinary native modules use the same declaration
and binding checks; only the narrow language-role requirements are additional.

Module builders now receive an explicit `DeclarationCatalog`. The opt-in
`kagari-stdlib::declarations::StandardDeclarations` helper exposes standard
references and a checked catalog; the generic runtime does not construct it. Engine publication validates
the complete dependency closure and rejects duplicates or mismatches atomically.
Analysis snapshots and runtime installation derive from the same registered module
set, including bundled adapters such as MapIterator. Existing snapshots and loaded
programs retain their checked identities and generation ownership.

### Binary products and scope

The former declaration binary, decoder, regeneration example and source/product
comparison workflow have been removed. Native registration needs no binary
intermediary. Existing user-program KBC/MIR
artifacts and their validation remain separate execution features.

Crate metadata analogous to `.rmeta` is deferred to a concrete design for Kagari
crates, separate compilation and build scheduling/parallelism. It must not be
introduced to carry declarations between an engine and its own registered modules.
The migration retains the NS01 core/alloc/std paths, canonical re-export identity,
explicit prelude, Vec, Iterable and existing language/GC semantics. It does not add
no_std, broader JIT support, new traits or a package manager.

## Synchronous calls and registered storage

Native entries return synchronously. NativeContext receives owned Rust arguments
or retained handles; recursive adapters check declared types and protect arguments
and results automatically. Object, NativeObject, collection and interface handles
preserve shared identity. PinnedFunction and prepared Method/Field descriptors
retain exact applied scopes and program generations; weak binding caches retain
no obsolete version by themselves. Selected trait operations consume installed
compiler evidence and reenter the same execution stack. The SDK with_context scope
applies cancellation, observation and execution policy to host object operations.

NativeStorage attaches a Rust payload to an ordinary nominal native type.
NativeData supports short fixed-data edits; ManagedStorage describes private traced
fields and checked writes. Complete builders root initializers until publication.
Advanced NativePayload supplies tracing, logical size and destruction. Its scoped
borrows validate the registered Rust type and cannot span collection or reentry.
Advanced CallContext and borrowed views remain for storage operations; StoredCallable
is a traced heap edge, while host-retained callbacks use PinnedFunction.
New payloads require no concrete Value/HeapObject variants. Scalar sequences use
typed compact buffers; GC-bearing sequences use traced Values. Hash callbacks run
outside table borrows with checked hashes and stable key tokens.

Vec stable sorting edits its actual buffer through a scoped lease. Roots
protect reference elements during callback reentry. Cleanup restores valid storage
on every exit; failure may change ordering but preserves the original elements.
Completed payload mutations remain. No universal collection rollback is promised.
Lazy native adapters trace their source/callback edges; iteration scopes retain
guards through completion or failure. Generic execution does not name MapIterator.

### Prepared native type facts

Runtime-local linked native bindings lazily prepare concrete scoped signatures.
TypeArgument shares immutable validated type facts, memoized parameters and enum
layout applications. Caches follow the binding/type descriptor's lifetime and do
not hold executable leases; nominal provenance retains immutable layout generations.
Exact prepared enum layout/payload-scope identity permits reuse of its checked
contract; different applications or generations still compare complete layouts.
Every value access continues to validate heap ownership, slot generation and access.

Typed callbacks borrow their enclosing synchronous call's program retention.
Public standalone conversion scopes and escaping handles retain independent leases.
Argument values snapshot before user converters, and custom Rust mapping checks
continue to execute before effects. This preparation does not change enum allocation,
mutation commit ordering or the collector algorithm.

## Interface dispatch

Interface GC objects reference runtime-owned receiver descriptors by checked IDs.
Rooted calls select a checked ordinal or shared operation without copying the whole
table. Receiver signatures/operations are prepared on demand and reused; method-local arguments
and caller witnesses remain per-call. Immutable type facts preserve origins;
executable environments and parent/operation/application caches use checked IDs.
Coordinated tracing reclaims cycles across metadata, heap objects and old programs.
Inherited calls retain the original root; escaping upcasts publish normal checked
GC interface values. Metadata sharing does not replace GC roots or code ownership.

Generic methods through interface values use checked shared script/native entries
with explicit type arguments and operation witnesses. Static calls retain
specialization. Native concrete-result adapters carry selected implementation
proofs, validate the concrete return and box through the pinned interface table.
Runtime dispatch does not infer types, search implementations or specialize code.
See [traits](spec/traits.md) and [native declarations](spec/standard-declarations.md).

## Definition identity ownership

`DefinitionPath` is the exact owned package/module/kind/name/occurrence locator
used by authoring inputs, fingerprints, debugging and explicit cross-context
queries. Published native declarations, HIR caches, checked MIR/bytecode and
runtime metadata use `identity::table::DefinitionId`: an eight-byte Copy handle
with private process-local table and node indices. Bare handles have no Serde
codec. Owners retain immutable checked `DefinitionTable` snapshots; builders
append without changing existing indices. Independent tables reject each other's
handles, even when the paths are equal. Import resolves exact paths once and maps
all referenced ancestors into the destination context.

Semantic records have one identity-parameterized definition rather than parallel
path and handle models. Ordinary module-owned mapping implementations enumerate
the identity fields, reject collapsed keys and preserve cancellation and bounds.
Source checking, proof validation and editor display may project transient authoring
records. These projections confer no executable seal and are not retained beside
published compact records. Mutable extraction discards checked ownership evidence.
HIR arenas/body handles and runtime generations/epochs remain separate safeguards.

Portable KBC/MIR data uses canonical exact tables and four-byte local references.
Only referenced nodes and ancestors are emitted. Process table numbers, insertion
order and unrelated interned nodes do not affect canonical bytes or fingerprints;
ordered arguments, fields and instructions retain their semantic order. Decoding
rejects invalid/foreign references, duplicate identities, forward/cyclic parents,
paths above 64 segments and tables above one million records before semantic
verification and executable adoption. Existing envelope and type limits still apply.

Runtime preparation normalizes independent verified inputs into its own context
while retaining original immutable version identity and dependency versions.
Nonempty module metadata is copied during contextual normalization; code-image
sharing across independent runtimes must not be inferred from original version
sharing. Definition table snapshots share their prefix storage, but appending
while a snapshot is retained currently copies the table's index containers.

## Runtime Model

The active [runtime ownership and host object API](runtime-ownership-and-host-api-design.md)
replaces distributed Rc ownership with central checked stores and automatic host
leases, and targets exclusive execution in a Send runtime. It also owns typed
registration, managed object mutation and checked host function/trait calls. The
following paragraphs describe the implemented GO baseline and IP01–IP03 interpreter storage.

The runtime owns values, the script GC heap, explicit roots, host registry,
module versions, installed native code owners and execution sessions. The VM
drives verified bytecode against these services. GC does not scan or own Rust
host state. Host calls use scoped borrow validation; deep host mutation uses
checked typed paths rather than retained Rust references or reflective field lookup.
See [runtime](spec/runtime.md), [host interop](spec/host-interop.md) and
[typed path mutation](spec/typed-path-mutation.md).

Persistent root values live in a heap-owned generational table. Host/debug handles
carry Arc leases and checked root identities; value access requires the owning heap.
Execution values instead occupy reusable contiguous runtime-owned frame windows
in separate scalar and managed banks. Scalar slots hold complete 64-bit payloads
and explicit initialization flags; managed slots retain ordinary Values and full
handle identities. GC traces the managed bank and program/environment edges, including suspended
callers, independently of host leases. Session frames use indexed storage; transient
interpreter cursors borrow the checked frame, session and operand window once,
without a session/root-table lookup for each operand. Cursors are released before
GC, observation, calls and reentry; bounds, publication and sticky termination
checks remain enforced. Window generation checks reject expired native views.
VerifiedProgram derives compact physical operations once from sealed bytecode.
Bounded control-flow liveness and conservative intervals assign reusable physical
temporary slots without renaming canonical registers used by contract verification.
Prepared operands directly name physical slots. Cold instructions, native views and
frame inspection translate logical indices; named locals retain fixed debug slots.
If the preparation work/state budget is exhausted, that function keeps distinct
register slots. The budget is shared across the complete verified program.
Immutable FrameLayout locations map canonical registers/locals to bank offsets,
physical representations and direct scalar admission domains. Reused slots stay
within one bank. A managed capture cell's semantic type describes its content;
the slot retains the managed physical representation and cell read/write validation.
Cold host/native/debug access materializes scalar Values on demand and validates
incoming representations/domains. Unavailable scalar inspection produces Unit;
executing an uninitialized operand quarantines. Frame entry reserves both banks
before copying arguments; release clears managed roots and initialization state.

Scalar operations prepare concrete function pointers to shared `kagari-types`
payload kernels after sealing. All integer widths, arithmetic/bit/shift families,
numeric comparisons, f32/f64 operations and supported casts use raw payloads;
mixed-width shift counts keep both checked domains. Source-width overflow and
IEEE bits remain intact. These function addresses are runtime preparation data,
never serialized artifact operands. Native public argument/result adapters remain
Value interfaces; typed call transfers are the following integration phase.
Scalar operands and numeric contracts are inline; identity-bearing types, strings,
call arguments and other variable-length metadata remain in the canonical immutable
instruction records. Their logical PC is the index, so normalization and hot reload
do not require a second semantic description or change debug locations. The VM
borrows these records at slow boundaries instead of cloning wide instructions.
The runtime cursor supplies a closed scalar execution region: it fetches only
sealed operations and reuses entry authority without admitting caller callbacks
or external Values. Managed operands/replacements and identity comparisons leave
the region before normal ownership/drop and heap semantics run. The VM owns the
frame driver, cold dispatch, safepoints and observation. The cursor checks
cancellation and collection/observer eligibility at each original program point;
the driver has already checked the first PC before acquiring it. Full safepoints
and error observation run after releasing the cursor. Values retain full scalar
precision and complete handle identities; large immutable host descriptors are
shared out of line rather than inflating every scalar execution slot.

The SDK's default artifact path applies the bounded MIR pass pipeline before both
bytecode and portable native input emission. Copy forwarding requires equal complete
semantic contracts and immutable temporary definitions. It retains local stores,
heap/module/cell reads, checked traps and side effects. Scalar constants are shared
and moved to the dominating entry block; entry operations retain their order.
Modified MIR is reverified to rebuild control flow, roots and source/debug facts.
Explicit diagnostic lowering can disable these passes. Ordinary script calls copy
checked register arguments directly between arena windows after capacity growth,
without an intermediate value vector; callers remain rooted during the copy.

Leases can move across threads and outlive runtime teardown without owning heap
storage. Runtime owns session, frame, program and executable metadata stores by
value. Borrowed execution/host scopes prevent owner replacement during execution
and release roots, call depth and borrow leases on every exit. LoadedModule shares
immutable verified descriptors; installed native callbacks and layout caches live
in module records. GC traces executable IDs and exact program dependencies together,
validates storage before sweeping and disposes of records outside table borrows.
Object policy publishes references through checked storage paths, including module
slots, frame/root slots and lazy metadata caches. Runtime and the VM are Send but
not Sync: ownership can move after borrowed synchronous scopes end. Shared callback
registrations require Send + Sync, whereas exclusively owned native payloads and
observers require only Send. Runtime owns the boxed observer/debug session; nested
drivers borrow it only while reporting events. A Tokio dev-only acceptance test
owns a runtime across message-receive awaits; no runtime/VM production dependency
on Tokio or asynchronous script execution is introduced.

Installation determines exposed native/host APIs; declared visibility, writeability,
storage access, ownership and generations remain checked. Root cancellation is
sticky across calls, callbacks and reentry. Calls, loops and long native work poll
at safe boundaries without splitting indivisible commits. Call depth is bounded.
Hosts own admission and deadlines; runtime provides no hard preemption or generic
CPU/memory quota. See [execution](spec/execution.md) and [security](spec/security.md).

## Embedding API

Current entrypoints are KagariEngine, KagariRuntime, PreparedProgram and
ExecutionContext. SDK features are independent: no features gives artifact-only
interpretation; `source` adds the frontend; `native` adds frontend-free MIR/codegen
preparation. Default features enable both; the host supplies a concrete backend.
Source-only builds can emit portable MIR for native-only consumers.

Prepare reusable checked products, then link them in each runtime. `execute`
interprets; `prepare_native` compiles/caches and installs exact-version handles;
`execute_prepared` consumes that decision. Reload validates a prepared candidate
before publication and retains old dependency versions needed by values/calls.
Public facade changes are [queued](host-api-refactor.md), not current API names.
See [embedding](spec/embedding-api.md), [loading](spec/module-loading.md) and
[activation](spec/module-activation.md).

## Debugger and Tooling

The interpreter-first debugger uses source maps, safe points, rooted frame/value
inspection and module epochs. IDE/DAP transport belongs outside runtime.
DebugProtocolAdapter is the implemented boundary; see [debugger](spec/debugger.md#adapter-boundary).
Attached observers force pre-entry fallback when native code lacks callbacks.
Ordinary reflection stays within declared metadata and member adapters.

## Baseline Cranelift JIT

The compilation-only CodegenBackend consumes verified MIR and explicit helper
links. Products own executable pages independently of backend lifetime; installed
handles retain exact program/dependency generations. Supported functions are
zero-argument, straight-line Unit/Bool/i32 constants, moves, checked arithmetic,
supported comparisons and return. Unsupported locals/control flow/calls/GC values
fall back before entry. Failures after entry never restart interpretation.

Wider native support needs a concrete call/result/status ABI, real native-root
publication, checked cross-module targets and conservative call effects. Logical
stack-map schemas alone do not establish GC integration. The outstanding
[architecture review](architecture-review-2026-10-03.md#native-expansion-gates)
records those gates. See [JIT](spec/jit.md) and [backend contract](spec/codegen-backend.md).

## Contract and common responsibility cleanup

AC01-AC05 established an earlier partition. The
[crate responsibility target](#crate-responsibility-target) refines it through
CR01-CR02 and LR01-LR03: ABI owns physical binary interfaces, types owns shared
semantic declarations and contract owns executable records and linked checks.
Stdlib owns concrete core/alloc/std declarations and Rust implementations;
source/tooling utilities belong to source and HIR.

### Agreed implementation order

The implemented checkpoints separate ABI/contract first (AC01), analyze source
roles next (AC02), integrate native-generated views and declaration-driven library
policy (AC03), then migrate loading/tooling/common ownership (AC04). The roadmap
records final acceptance and phase commits.

Shared types, generic binders/bounds and trait/impl/function declarations belong to
types. Logical layouts, executable interface/call records and linked validation
belong to contract; physical runtime/codegen interfaces belong to ABI. A generic
TraitDef belongs to types, while its particular Add or List definition belongs
to stdlib registration. ABI never resolves script types or owns those declarations.

### Ownership boundaries

| Responsibility | Implemented owner |
| --- | --- |
| Physical values, calling conventions, helper symbols/signatures, native entries and physical root locations | Narrow ABI; no semantic catalog, frontend or runtime implementation dependency |
| Semantic types, declarations and generic/member checks | Types; source-independent and separate from physical/execution facts |
| Logical layouts, executable imports/interface records and linked verification | Contract; uses types and ABI without a frontend |
| Syntax-required trait declarations and role selection | Stdlib registration, types role identities and compiler semantics using ordinary parser/HIR |
| Collection/standard-library traits, types, methods and Rust bodies | Explicit native library ownership; Rust declarations generate `.kgr` for compiler/LSP analysis |
| Substitution and implementation proofs | Types performs semantic substitution/checks; contract validates linked executable proofs with bounds |
| Source rendering, navigation and diagnostics | Source owns documents and provenance; HIR tooling owns rendering and queries; no executable dependency on generated text |

### Naming policy for the split

Use short, meaning-based names with module ownership, following rustc's distinction
between types, declarations/definitions, signatures and physical calling data.
AC01 removes Abi prefixes/suffixes from semantic records. Do not replace
them with a blanket Contract prefix/suffix. Keep `kagari-abi` as the physical crate
name; a crate name need not be repeated in every type it owns.

The naming references are rustc's [type model](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/index.html)
(Ty, TraitDef, FieldDef, VariantDef and FnSig) and
[function declaration](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_hir/hir/struct.FnDecl.html).
Its [FnAbi](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/callconv/struct.FnAbi.html)
and [ArgAbi](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/callconv/struct.ArgAbi.html)
describe native argument passing. Borrow the naming distinctions, not rustc's
interning, lifetimes, type system or record layouts.

| Retired name | Implemented name | Meaning |
| --- | --- | --- |
| AbiType | Ty | Semantic type expression |
| NominalAbiType | NominalTy | Applied nominal type, including its arguments and associated bindings |
| FunctionAbi | FnDecl | Named function declaration, including generics, parameters and implementation policy |
| ParameterAbi | Param | Named value parameter |
| TypeAbi / TypeAbiKind | TypeDef / TypeDefKind | Type definition and its struct/enum/native category |
| TraitAbi | TraitDef | Trait definition, parents and associated members |
| ConstAbi | ConstDef | Named constant definition |
| FieldAbi / VariantAbi | FieldDef / VariantDef | Field and enum-variant definitions |
| GenericParameterAbi | GenericParam | Scoped generic parameter identity |
| GenericBoundAbi / ConstraintAbi | GenericBound / Constraint | A type's bounds and individual constraints |
| AssociatedConstAbi / AssociatedTypeAbi | AssociatedConstDef / AssociatedTypeDef | Associated-member declarations |
| AssociatedTypeFamilyAbi | AssociatedTypeFamily | Implementation-side associated type family |
| InterfaceTableAbi | InterfaceTable | Checked interface implementation/member table |
| PublicAbiItem | PublicItem | Public module contract record |
| ModuleAbi | ModuleContract | Executable module's carried declaration/verification metadata |

ModuleContract names a specific responsibility, not a suffix applied to every
semantic record. ModuleDecl owns native authoring registrations, documentation and templates;
ModuleContract carries the checked executable subset. Their responsibilities differ.
ImplDecl and TraitContract retain their existing meaningful names;
PublicAbiItemBuffer is removed in favor of Vec<PublicItem>.

NominalTy is not renamed TraitRef because it also represents struct/enum instances
and associated bindings. FnDecl is not FnSig because it carries names, binders,
bounds and implementation policy in addition to parameter/result types. Use FnSig
only for an actual isolated signature when a consumer needs that structure.
TypeDefKind describes declarations; TyKind would describe type-expression variants.
This naming task does not introduce either new wrapper solely to imitate rustc.

ABI keeps descriptive names such as NativeType, NativeHelperSignature, JitValue
and ExecutableEntryPoint. Apply representation qualifiers where they distinguish
actual contracts. Migrate direct consumers without forwarding aliases or broad
re-exports. The table records the completed migration; no compatibility aliases are retained.

### Language items and ordinary trait records

A syntax-required declaration may use `#[lang = "add"]`. Parsing recognizes the
attribute; HIR collects a role-to-declaration ID and uses normal checked trait
selection. Executable consumers use selected callable identities/signatures/witnesses.
Ordinary library traits require no global protocol enum.

#### Core trait inventory

The installed foundation retains 26 of its 40 traits as language items:
Try/FromResidual supply propagation; From supplies checked implicit identity and
lossless conversion adapters. Other roles select syntax or implicit value implementations. Their declarations are ordinary foundation source, analyzed
through parser/HIR. Language roles select those declarations; builtin or native
implementations remain separate. Core source and role collection are implemented in AC02. AC03 implements native
view analysis and library capability/adapter records.

| Core traits | Count | Compiler consumer |
| --- | --- | --- |
| Add, Sub, Mul, Div, Rem | 5 | Binary arithmetic operators |
| BitAnd, BitOr, BitXor, Shl, Shr | 5 | Bitwise and shift operators |
| Neg, Not | 2 | Unary negation and logical/bitwise negation |
| PartialEq, PartialOrd | 2 | Equality and relational operators |
| Index | 1 | Indexed reads; writable indexing keeps its separate checked bridge |
| Fn | 1 | Existing callable/closure trait semantics |
| Iterator, Iterable | 2 | Iteration and `for` lowering |
| Debug, Display | 2 | Debug and ordinary interpolated-string formatting |
| Try, FromResidual | 2 | Checked branch/residual calls for `?` |
| From | 1 | Implicit identity and lossless numeric conversion adapters |
| Eq, Hash, Ord | 3 | Existing implicit eligibility, identity/composite equality and hashing, and builtin total ordering |

Eq/Hash/Ord are retained because the current compiler supplies implicit value
implementations, not because a hash container or sorting method alone needs a
language role. PartialEq/Hash/Debug defaults for user values must remain eligible
under the current rules; floats do not acquire Eq/Hash/Ord. Explicit implementations
take precedence where specified. Moving all such defaults to a library would be
a separate semantic-implementation change, not a prerequisite for this cleanup.

The other 14 traits use ordinary native-library declaration/implementation records:

| Library traits | Count | Ownership reason |
| --- | --- | --- |
| List, MutableList, Map, MutableMap, Set, MutableSet | 6 | Container interfaces, inheritance and algorithms are library policy |
| RangeBounds | 1 | Range syntax constructs a value; its bounds interface is a registered implementation |
| Into, TryFrom, TryInto | 3 | Ordinary conversion APIs; Result FromResidual selects From |
| FromStr | 1 | Parsing API |
| FromIterator, Sum, Product | 3 | Construction and aggregation APIs |

All 40 traits remain mandatory and available without optional modules. Compiler
recognition and library availability are separate decisions. Preserve current
Into/From and TryInto/TryFrom derivation and checked numeric conversions through
checked implementation/adaptation records. Relocating their declarations alone
does not itself replace selection. Installed conversion adapter records carry the
forward trait/method and associated error identities; source and portable proofs
read those records without recognizing Into/TryFrom/TryInto in the language enum.
No new source blanket-implementation or coherence feature is introduced.

The audit evidence is the operator/format/propagation selection in HIR and the
implicit implementation rules in `kagari-hir/src/language/semantics.rs`. In that
implementation RangeBounds uses registered engine implementations, while Eq,
Hash and Ord participate in implicit eligibility. Use this behavior, rather than
module placement, to classify ownership.

Type/member bindings are a separate list: String literal representation,
ControlFlow variants for `?`, Option iterator results, Ordering results, range construction, `[T]`'s
List declaration, default Vec construction and writable indexed access.
These bindings select checked declarations/members or intrinsic representations;
they do not justify adding all library traits to a language-role enum. In particular,
the bracket bridge may refer to native-authored List without duplicating its
definition. String methods remain ordinary library implementations. IndexMut is not introduced.

The completed [nominal enum and propagation design](enum-propagation-plan.md)
has replaced closed enum representation with ordinary library declarations and
pinned layouts. `?` checks Try::Output/Residual and the enclosing FromResidual
obligation in HIR; lowering consumes its checked calls and ControlFlow members.
Standard Option/Result/ControlFlow policy belongs to registered library bodies.
Custom source/native carriers use the same selected execution path. Native Result
conversion carries a checked From callback; implicit adapters include the applied
source interface in their identity and forwarding preserves supplying generations.

Validate unknown/duplicate/missing roles, installed origin, declaration kind,
binder arity and required member shapes. Application attributes or copied names
cannot acquire reserved roles. Preserve exact scopes and generation checks.

### Narrow ABI data inventory

ABI describes representations and the runtime/codegen calling boundary. It does
not describe a script declaration's meaning. The following inventory uses existing
type names to identify the data to retain or split, rather than proposing a second
parallel model:

| ABI data | Existing structures | Required content |
| --- | --- | --- |
| Lowered value representations | ValueType | Scalar slot representations, heap/host references and tagged shared-generic values; no nominal type arguments or collection kinds |
| Machine helper signatures and links | NativeType, NativeHelperSignature, NativeHelperSymbol, NativeHelperDeclaration, NativeLinkDescription | Machine parameter/result kinds, helper symbols and resolved process addresses |
| Native entry/result convention | JitCompiledFunction, JitValue, JIT status/value tags | Physical entry signature, `repr(C)` result fields, tags and status codes |
| Target and executable entry descriptors | BackendId, BackendTarget, ExecutableEntryPoint; physical portions of ExecutableFunctionArtifact | Target triple, pointer width, features, ABI identifiers, entry symbol/address and emitted code offsets |
| Executable memory lifetime | NativeCodeOwner, physical product portion of NativeCompilationProduct | An opaque owner retaining executable pages while installed code is reachable; no backend implementation or runtime state |
| Physical safe points and roots | Physical portions of ExecutableSafepoint/ExecutableStackMap when supported | Native code offsets and real machine root locations understood by the runtime/backend |

Move ValueType's BuiltinType/HostValueType conversion policy to contract-side
lowering; the ABI enum must not import semantic type definitions. A tagged generic
representation does not erase generic typing: binders, arguments and proofs remain
in the contract and are checked before execution.

Split the current native artifact envelope. Function identities, logical
register/local maps and source/debug metadata belong to executable contract/MIR
metadata, which may wrap the physical ABI product. Keep only physical facts and
opaque shared boundary identifiers in ABI; never make ABI depend on contract to
recover a semantic function or type. Actual memory ownership remains explicit.
Current ExecutableStackMapLocation::Register/Local entries are logical slot
identifiers, not a proven native GC root protocol. Preserve their coverage at the
logical owner; implement physical root publication only with the native feature
that consumes it.

The current native helper kinds are Pointer/I32/I64, and the current compiled entry
is zero-argument with Unit/Bool/i32 results. This separation does not claim wider
calls or GC support. Introduce byte sizes, alignment, field offsets or additional
machine passing modes only when an implemented backend/helper needs them.

The data moved out of ABI is equally explicit:

| Data | Proposed owner |
| --- | --- |
| AbiType/NominalAbiType, scalar semantic types, generic parameters/bounds, projections and substitutions | Contract type model and focused type operations |
| Module/Function/Type/Trait/Impl declarations, associated members and semantic native import signatures | Contract declaration model |
| InterfaceTableAbi, selected call records, generic bodies and implementation witnesses/proofs | Contract callable/interface model and verification |
| StructLayout/EnumLayout and SemanticSlots | Contract logical layouts and semantic slot metadata; these are not byte-offset layouts |
| EffectSet and logical RuntimePrimitive/operation contracts | Checked execution/MIR contracts; their machine helper signatures remain ABI |
| Protocol roles and language trait source | Language foundation and compiler role selection |
| Native library trait/type definitions, defaults and collection-specific policy | Their native library owner, expressed through ordinary contract records |
| DeclarationSource/rendering and spans/navigation | Source/tooling owner |

Bounded decoding, shared numeric behavior, identities and host schemas follow
their concrete consumers in AC01's module audit. This table does not relocate
them wholesale into ABI or common. Contract retains source-independent signature,
layout, access and generic/interface verification; narrowing ABI removes none of
those loading checks. Keep runtime/helper ABI identifiers distinct from artifact
format identifiers, without routine unpublished-version bumps.

### Library identities and representation boundaries

Generic HIR, contract and execution consumers read installed storage-access facts,
nominal implementation headers, declared parents and checked members. The language
Protocol inventory contains only 26 core roles. A private RegistrationTrait enum
belongs solely to the Rust library catalog. Storage joins/inference, readonly
matching, identity/equality and interface dispatch do not recognize collection
names. NativeTypeKind/NativeTypeConstructor retain representation descriptors;
ordinary new containers use existing nominal NativeStorage registration without
a new generic type or execution-dispatch variant.

The bounded `builtin::array_bridge` owns existing syntax bindings: `[T]` selects List<T>; `[a, b]`
constructs the mandatory Vec<T>; `[value; count]` repeats construction without
putting length in the type. Indexed assignment needs a checked writable member
contract. Preserve left-to-right once-only evaluation, trap/allocation order and
completed effects when replacing MakeArray/RepeatArray lowering. These bridges
must not become a second complete collection catalog.

String literal typing/representation may remain intrinsic while its methods are
ordinary native-authored implementations. Adding trim or split needs no compiler
method enum. Audit storage layouts, CollectionAccess, standard enums and
RuntimePrimitive separately: they may carry required tracing/access/operation facts.
Retain those at their consumer layer; do not erase host restrictions or replace
every engine instruction merely to move declaration ownership.

Existing range syntax remains supported, including `1..3`, `1..=3` and open-bound
forms. `1..=3` constructs RangeInclusive<i32> under ordinary integer inference;
its lazy iteration includes both endpoints through Iterable/Iterator. The compiler
needs range construction/type bindings, while RangeBounds describes the resulting
value's bounds through a registered implementation. Moving that trait to native
library ownership neither removes range syntax nor requires a RangeBounds lang item.

### Declaration preparation and dependency direction

```text
registered semantic declarations -> complete generated declaration source
  -> ordinary parser/HIR, validated roles and declarations
  -> checked semantic contracts and selected calls
  -> verified MIR -> bytecode/native products

checked executable products + installed Rust implementations
  -> source-independent validation/linking -> execution
```

Rust native definitions remain authoritative for ordinary libraries. Generated
source is not independently editable authority. Check analyzed declaration IDs,
binders, signatures, bounds, parents and associated members against native records;
source spans remain analysis data. Remove duplicate injected library catalogs.
Collect headers/roles before dependent bodies from the explicit provider set.
Runtime installation never parses generated source.

The dependency direction is contract -> ABI, never ABI -> contract. Neither has
syntax/HIR or frontend build dependencies. Semantic type-to-representation lowering
belongs to contract and uses physical ABI facts. Split logical from physical layouts.
Choose role/preparation owners from concrete consumers; add no empty/forwarding crates.

Audit common by responsibility: source/diagnostic/literal tooling; portable identities
versus source revisions/spans; shared numeric semantics; portable host schemas;
cancellation and decode mechanisms. Preserve one checked numeric implementation and
source-free executable consumers. A shared consumer count is not an ownership rule.

AC04 implements the common audit with `kagari-source` owning source documents,
FileId/Revision/FileSpan, diagnostics, line indices and literal grammar. Frontend
consumers import that owner directly; compiler/SDK dependencies are gated by
`source`, and backend/VM test dependencies do not enter their production graphs.
CR01 leaves portable definition identities, debug spans, cancellation and bounded
decoding in common. Shared numeric semantics, host schemas, CollectionAccess and
RangeKind now belong to types; contract and runtime consume these semantic facts
without source analysis or a concrete library API catalog.

Core and application documentation is authored in registered module/item metadata.
Complete generated views retain module Markdown, member docs and renderer-recorded
sites. Analysis snapshots own parsed text and locations; optional SDK cache files
use those same physical paths and ranges. There is no separate authored-core
provenance path. Loading and reload compare every reserved core trait and installed storage/conversion capability against the exact
runtime registration, including private records and products with no native calls.
The installed core modules collectively expose all 26 required declarations;
each owning module must expose its assigned roles exactly once.

The eight RuntimePrimitive entries remain checked execution helpers: value
comparison/equality/hash/formatting, StringPartsJoin and Assert. Their signatures,
effects and operand facts belong to contract/MIR verification; Rust bodies belong
to runtime and physical helper calls to ABI/backend. No primitive owns or defines
a source trait. Existing storage layouts, roots, scoped host borrows, cancellation
and generation-pinned execution retain their current owners.

### Scope and completion boundary

The [roadmap](implementation-roadmap.md#contract-and-common-responsibility-cleanup)
owns AC01-AC05, checks and activation. Preserve the current finite trait/API surface,
mandatory foundation, shared generics, storage safety and reload pinning. No new
containers/traits, general downcast, async/permission redesign, blanket standard-enum
replacement, stable external ABI or compatibility reader is authorized here.
Performance effects are unmeasured. A future kagari-ffi C adapter belongs over
kagari-embed; internal helper ABI stays separate, and no placeholder crate is needed.
