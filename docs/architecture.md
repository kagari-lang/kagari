# Kagari Architecture

This document describes current ownership and execution boundaries. Language
behavior is defined in [the specifications](README.md#language-and-execution-specifications).
[The roadmap](implementation-roadmap.md) owns pending work; proposals below are
explicitly marked and do not describe implemented behavior.

For source-level data structures and ID lookup, start with the
[HIR reading guide](architecture/hir.md).

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

## Source namespaces and import ownership

HIR source analysis publishes one immutable `NamespaceCatalog` owned by
`ModuleGraph`. Each module's `ModuleImportFacts.scope` shares the catalog's exact
`Arc<NameTable>`; declaration and body resolution use that table. The catalog
owns unfiltered namespace candidates and never points back to the graph, import
facts, declarations or lowerings. Lookup supplies the importer, host declarations
and cancellation token, then checks visibility at each path component.

`SourceUnit` identifies the logical module, file, revision and HIR arena.
`SourceDeclRef` pairs that unit with a declaration item; imports and module headers
are not declaration items. `NamespaceId` distinguishes source modules, associated
member containers, host modules and installed package prefixes. Aliases and public
re-exports share canonical targets. Lowering to a local HIR ID requires the complete
current source unit to match; retained targets cannot index a newer arena. Duplicate
logical modules remain ambiguous while each source unit retains its own facts.

A real import leaf produces one `ImportDirective` with its named/glob kind,
optional explicit alias, leaf/root source ranges, resolution state and direct
dependency edges. Scope entries always have a local name and retain candidates
separately by strength: declarations and explicit imports, then globs, then
package/prelude bindings. Strong collisions stay errors; equal glob targets keep
all origins. An unresolved strong name blocks weaker candidates. Import conflicts
belong to graph diagnostics; pure declaration conflicts remain declaration-stage
diagnostics. Entering
`m::nested::value` traverses namespaces without creating child scope bindings or
synthetic import records. Installed APIs supply ordinary namespace/package inputs
and registered dependency metadata without fabricated imports.

Lookup returns the canonical target and selected binding origins independently.
Import, expression and type path prefixes retain physical source sites for tooling;
executable lowering consumes qualified checked declarations and graph dependencies,
not syntax provenance. Source-free artifact loading and native backends do not
consume the HIR catalog. Signature projections follow declarations reachable from
the local scope and use independently checked callee signatures. Cache reuse checks
reachable namespace surfaces and the relevant body/signature inputs; local arena
reuse still goes through the existing explicit remappers. Older snapshots retain
their immutable catalogs and continue answering old queries.

The fixed-point rescan schedule and iteration bound remain unchanged. SA1 inline
AST reuse, SA2 scheduling and SA3 exhaustion handling remain follow-ups in
[the repository review](review.md). No import-resolution performance claim is made.

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
`[T]` is a builtin fixed-length array, independent of installed declarations.
Literal and repeat construction establish its runtime length. Vec is separate
nominal sequence storage; explicit Vec::from/HashSet::from constructors copy
array elements into independent collection storage. List views remain shallow.
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

Runtime-local linked native bindings own one scoped-signature preparation cell:
closed bindings populate it after staging establishes exact program provenance and before
candidate publication; applied generic bindings populate it during application preparation.
Signature consumers only read prepared facts. Preparation failure abandons the candidate
through its existing lease and collection protocol. Argument views, result validation
and native object construction
reuse those same immutable type facts; there is no separate applied-signature store.
Generic native applications are published under their linked member, import and
generational type-environment identity. Their signatures, result adapters and selected
calls are immutable program edges, retained through the common bounded descriptor
index. An active native frame window independently traces its application before
callbacks or collection, so reentry may evict an index entry without invalidating the
outer call. Selected-call edge tracing is shared with stored native selections;
escaping typed host handles still explicitly acquire their own roots. Generic entry
does not recreate native applications or host-style selected-call roots on a hit.
Builtin comparison result contracts use the same TypeArgument preparation and enum
construction path. Each exact runtime-linked program member lazily owns at most two
closed result arguments: Ordering and Option<Ordering>. Operand validation precedes
first preparation, and the optional result is prepared only after rooting any inner
Ordering value. Reuse still validates the installed owner and declared enum member;
payload checks and ordinary result allocation remain. These facts retain immutable
provenance, without Values or executable leases, and disappear with the member's
runtime record. Reload cannot inherit another version's result arguments.
TypeArgument shares immutable validated type facts, memoized parameters, exact
type/provenance identities and enum layout applications. Nominal type application
prepares a successful admission proof once, retaining only the immutable program,
complete canonical layout identity and aggregate kind. Scope preparation can retry a
previously unavailable proof; failures/absent identities are not cached. The proof
does not retain layout argument bindings, Values or executable environments, so it
cannot create a cycle through a supplied argument's type scope. Value matching reads
that proof through the existing bounded layout-admission owner; genuine cross-version
comparisons and missing identities keep the full scoped fallback. Enum type admission
accepts any valid member, while pattern admission independently checks its tag.
Native enum construction retains its member-selection facts, but no longer provides
a separate member-by-member type matcher. Each prepared member pairs its applied
layout with the complete portable declaration identity resolved at preparation.
Named construction reads the prepared final segment; portable VariantRef admission
compares the full identity, including module, segment kinds and occurrences, before
using that layout. Repeated construction no longer walks the definition table to
recover names or hash a portable path back into an ID. Independently authored equal
handles remain valid; same-spelled foreign members fail. Payload scope and generation
still come from the applied layout, with existing owner and heap checks unchanged.
These facts belong to TypeArgument's existing lazy member list, adding no separate
global/call-site admission cache, frame field or executable root.
Foundation iterator next calls construct through the native function's prepared
result TypeArgument. The declared-result adapter validates the iterator contract,
then uses the existing advance/finish/commit kernel: payload roots and mutation
guards remain live, and checked enum allocation must succeed before cursor progress
is committed. It does not derive a fresh Option type for every item. General raw
iterator operations still derive independent result types when no declared native
result contract is being used; they share the same iterator checks and commit kernel.
No result type is cached on a cursor or an unscoped primitive type argument, where
the correct supplying program could differ between callers. Foundation Option
authoring member handles are prepared from the existing registration catalog before
bindings are published, removing their duplicate first-execution catalog build.
Struct application checks are shared by
nominal preparation and host object binding; enum preparation belongs to the common
type-application layer. Its borrowed type view carries
the checked closed result alongside the original expression and supplying scope.
Container/tuple projections preserve both trees; arbitrary lexical children inherit
closed evidence only when they belong to the checked closed tree. Storage contracts
establish heap-type validity before exposing this evidence. Compatibility reuses the
closed result while still checking nominal provenance and genuine layout differences.
Debug builds assert that supplied closed facts contain no unresolved types.
One value matcher serves raw and prepared views, preserving facts through array, map,
set and iterator admission. Tuple descent follows the bounded checked type tree;
matching no longer creates a worklist for every scalar or container. Runtime admission
adds live-value/host-root checks, including nested tuples; heap storage keeps its
host-free contract. Collection commit compares complete source/target contracts.
These views add a transient borrowed pointer, without new retained descriptors, frame
fields, type caches or executable roots. Interpreter aggregate
construction, native enum preparation and host object binding share one layout-scope
preparer. Its lazily created program-root store retains at most 128 declaration/argument
scopes and 128 complete applied identities per aggregate kind; each member separately
retains at most 128 struct and 128 enum applications, including their immutable scopes.
These use the same bounded index as executable descriptors, with distinct ownership rules:
layout facts own immutable provenance and no executable leases or GC edges. Cache
eviction, store borrowing or retirement can require preparation again; existing
descriptors remain readable. Supplying versions remain part of the exact identity.
At linking, equivalent struct/enum layouts across members of one pinned program
receive a canonical identity. An applied template resolves that identity during
preparation when a matching linked layout exists; the application cache retains
the result. Linked identity tables retain locations in immutable bytecode, not duplicated
layouts, native links, Values or executable leases. Complete equality establishes
identity; a hash only selects candidates. Runtime-only applications normalize complete
shape and scoped argument identities under the same program. A scoped application may
reuse a linked identity only when every argument has the same supplying provenance.
Application preparation seals scope and identity together; consumers cannot modify the
scope afterward. Dynamic IDs start after linked IDs and are never recycled on eviction;
scope IDs are also monotonic within their exact program. Exhaustion, detached facts or
a borrowed store retain full compatibility checks when no prepared proof is available.
Same-program accesses compare complete canonical identities instead of whole layouts,
including equivalent scoped/unscoped producers and consumers across members. A distinct
prepared layout pair goes through the shared struct/enum compatibility admission owner.
The consumer's immutable program description lazily retains at most 128 successful
relations across both aggregate kinds, keyed by complete producer/consumer identities
and the producer version. A weak producer descriptor must also match exactly on lookup.
This pure type evidence remains usable after executable retirement and cannot keep the
producer's code or runtime resources alive. A lazy mutex protects optional evidence when
type facts cross threads; graph comparison runs outside the lock. Missing identities,
eviction or unavailable cache access retain full checks. A new version cannot inherit an
old proof, and a proof never authorizes execution, a heap handle or mutable access.
Raw type-expression checks retain full resolution and graph-comparison fallbacks.
A borrowed closed spelling does not replace a complete layout proof.
Every value access continues to validate heap ownership, slot generation and access.
Enum variant comparison goes directly through this shared admission policy after
runtime/variant checks; the older same-member/Arc shortcut is removed. Interpreter
patterns, native enum argument access and raw enum type checks borrow immutable
tag/payload storage. A field read copies one Value without cloning the payload list.
The enclosing frame/native argument retains the root, and the borrow ends before
collection, heap allocation or callbacks. Intrinsic enum equality, ordering,
formatting, key extraction, range-bound reads and error previews use the same
immutable views. Their recursion only reads the script heap; Rust output/key
allocation does not collect or call user code. Key extraction copies Values into
its work stack and retains the key's required nominal data, without copying a
second payload vector. Reflection copies the type name and releases the view
before allocating the returned script string. Host nominal validation and SDK
Option/Result conversion project only the layout and bounded payload state before
type preparation or user conversion; the existing host/conversion roots retain
inputs. Native child converters may collect or mutate aliases after the borrow
ends. Public `enum_snapshot` remains an owning inspection API, not an internal
execution adapter; its copies do not root payload handles.
Standard-library enum consumers use one checked borrowed projection for nominal
declaration, member and payload inspection. Iterator/construction readers copy at
most one Value, comparison readers return a Rust Ordering, and propagation copies
a bounded branch/payload state. The borrow ends before type preparation, allocation
or reentry; existing argument roots and explicit payload roots retain live values.
New enum constructors allocate their own required payload containers. Propagation
preserves result-type failure precedence and forwards failure origin metadata.
The former owned member-name/payload inspection helper is removed. Standard enum
authoring handles come from the declaration inventory's existing lazy store through
`StandardDeclarations::enumeration(name)`; warm lookup no longer constructs a
temporary StandardDeclarations instance. This declaration cache owns no runtime
layouts, values or executable versions.

Linked functions own one layout operand table for struct/enum construction, enum
patterns and applied field access. Equal operands within a function share one entry;
the logical PC selects it without reconstructing type arguments. Closed operands are
prepared before candidate publication. Scoped operands prepare at their first original
instruction, using the frame's exact immutable type environment, and publish only
successful layout facts. The same prepared table serves later reads and constructions.
Each linked member lazily retains at most 128 function/environment applications
through the common bounded index, keyed by FunctionRef and complete EnvironmentId
including owner and generation. Function entry selects that exact execution descriptor;
the frame uses its existing execution reference without a separate application field.
Descriptors share immutable call/primitive tables, constants and layout operands, and
own their applied layout cells. Optional eviction cannot invalidate an active frame's
descriptor. A debug-only environment field checks application identity. These cells
store immutable layout provenance, not Values or executable environment leases;
runtime ownership and environment generation admission stay with the frame. No store
borrow crosses collection or callbacks. Closed execution allocates no application
index. This replaces per-operation type resolution and the separate scoped-field
argument copies; ordinary enum heap allocation is unchanged.

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
following paragraphs describe the implemented ownership, compact values and interpreter storage.

The runtime owns values, the script GC heap, explicit roots, host registry,
module versions, installed native code owners and execution sessions. The VM
drives verified bytecode against these services. GC does not scan or own Rust
host state. Host calls use scoped borrow validation; deep host mutation uses
checked typed paths rather than retained Rust references or reflective field lookup.
See [runtime](spec/runtime.md), [host interop](spec/host-interop.md) and
[typed path mutation](spec/typed-path-mutation.md).

Internal `Value` is a 16-byte `Copy` tagged value. Scalars retain complete payloads;
strings, immutable tuples/ranges, mutable objects and host descriptors carry checked
12-byte heap identities (owner, slot, generation). Copying a Value neither copies
text/aggregate contents nor acquires ownership or a root. Tuple updates allocate a
replacement tuple while sharing referenced children; mutable children retain alias
semantics. Host descriptors are heap records, but referenced Rust state remains
outside the script heap. Runtime-only ephemeral IDs remain a distinct value tag.
Every heap access still checks owner, generation and object kind. Saturated slot
generations retire storage; owner/slot exhaustion cannot truncate or wrap identity.

Canonical `LoadConst` refers to a deduplicated module constant table by checked
`ConstantId`. Equality of portable float constants uses their exact bits. Shared
verified programs contain no runtime heap identities. Each runtime module record
lazily materializes its own string constants and traces them as edges of that
version. A module-owned ConstantPool uses single-assignment cells shared by its
runtime-local function links. Frame entry admits the links together with closed
interface calls; warmed prepared loads address a physical destination and checked
constant ordinal without resolving the module store again. They retain heap identity
and destination representation checks. An empty cell exits the cursor, releases all
transient frame/bank/session borrows, and invokes the same materializer as the checked
public/native entry. Allocation cannot collect or invoke user code; it publishes the
module edge before the next safepoint. The active frame roots that exact version
through materialization and destination publication. The cold transition consumes no
extra logical instruction and recomputes GC state at the successor. Neither the pool
nor its function links supply independent roots or live in shared verified code.
Repeated loads copy only the Value. Reclaiming an obsolete module releases
its pool; independently rooted escaped strings keep their bytes without retaining
the old module state. Standard string input operations borrow scoped UTF-8 views;
owned Rust String conversion remains an explicit copy at the host boundary.

Persistent root values live in a heap-owned generational table. Host/debug handles
carry Arc leases and checked root identities; value access requires the owning heap.
Execution values instead occupy reusable contiguous runtime-owned frame windows
in separate scalar and managed banks. Scalar slots hold complete 64-bit payloads
and explicit initialization flags; managed slots retain ordinary Values and full
handle identities. GC traces the managed bank and program/environment edges, including suspended
callers, independently of host leases. Session frames use indexed storage.
`ExecutionStack::execute_region` admits the active session/frame scope and window
owner/generation, then borrows bounded scalar/managed banks for the closed operation.
Its cursor is private and cannot escape to callers or accept callbacks/external Values.
Region admission also selects one immutable ExecutionFunction and borrows its exact
LoadedModule and optional LinkedFunction separately from the mutable PC/executing
position. Scalar, managed, field and index handlers share those views. The scalar
kernel operates directly on this cursor; it no longer reconstructs a second cursor
or reselects the function after every managed operation. Field/index ordinal checks
and lazy scoped-layout transitions remain, and missing runtime links still fail at
the operation that needs them. No persistent frame field, code clone or extra module
lease is needed: all views end with the original frame/window borrow.
Internal operand access reuses these borrows without repeated session or window lookup;
the region releases them before returning to GC, observation, calls or reentry.
Checked external frame access and region entry still enforce sticky termination and
quarantine. Internal writes retain heap-value validation, bounds and slot representation
checks through the same bank access implementation as external frame access.
Window generation checks continue to reject expired native views.

The VM driver classifies authoritative runtime state on activation/resume and after
call, return or await transitions. Ordinary progress retains its script action instead
of probing pending waits and native entry/return state. This action is local control
flow, not a second persistent session state; every resumed activation classifies again.
Region exits release operand borrows before observation, collection, native callbacks,
frame changes or parking. Synchronous native reentry must unwind back to its caller;
the next region still admits the session/frame/window. Cancellation and debugger
checks retain their original logical execution points. Actual waits are polled before
the slice check. Native completion publishes its result into the traced frame before
the driver can park; resume consumes it through the common return protocol without
repeating the native callback. The removed per-frame native-state probe API and VM
region-exit forwarding enum have no compatibility replacements.

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

Statically selected script, shared script/native, interface and closure calls transfer
arguments directly between disjoint frame banks. Sealed call sites own physical source locations and the
return destination; the selected callee's layout owns parameter placement and semantic
admission. There is no duplicate per-call-site callee layout or source/target-pair table.
Borrowed host values, captures and window sources feed the same argument iterator,
transactional admission and frame publication; shared/interface/closure calls do not pack a temporary
Value vector. Window identity, initialization, bounds and scalar domains are checked
before growing the banks. Scalar-only sources skip heap-reference walks. Concrete scalar
returns use an opaque packet until the caller slot or public host boundary;
shared environments and interface adapters retain their full return validation.
Function/ModuleFunction, Shared, InterfaceMethod and ClosureRegister call sites prepare
physical arguments and return destinations once. The executing canonical PC selects the record; module slots bind
through the caller's pinned program descriptor. Shared calls select their HP01-owned
environment and retain caller-scoped semantic argument checks before common admission.
Interface calls retain a `MethodInvocation` containing checked selection/application
identities. Preparation borrows immutable snapshot/operation facts instead of cloning
receiver type descriptions. The selected receiver is a borrowed parameter prefix;
remaining parameters come directly from the caller window. Selection/application edges
are published with the callee's program, environment and argument roots before any
safepoint. They remain live even after optional application-cache eviction or caller
register replacement. Retirement releases these edges with the window.
`RootedInterfaceMethod` is the host-retained wrapper around the same descriptor. Host
entry validates its lease and keeps it alive until window publication; internal dispatch
does not construct that wrapper or refresh a host root container. Host/internal arguments
and results use the same method-view signature checks and result adapters.
Linked members own interface-call contracts containing the expected interface type
with lexical provenance, method type arguments and operation witnesses, independently
of receiver values. Sealed call records classify actual environment dependence and
assign dense function-local ordinals. Closed contracts, including independent calls
inside generic functions, are prepared before candidate publication. Linking validates
their graph and proves that every executable dependency belongs to the same pinned
program. Function entry admits one linked execution record containing the immutable
call table, its module's constant pool and layout operands when needed;
functions with no runtime links need no such record.
Calls borrow facts directly by ordinal while the active window's program root protects
the record and its traced edges.
These code-bounded links are not optional application caches and cannot be evicted.
Environment-dependent contracts use the common bounded descriptor index keyed by
verified function/PC and exact EnvironmentId, with checked publication/GC tracing.
Changed scopes receive different entries; an expired scope is rejected even if a
cached descriptor remains. The previous closed-scope dynamic index path is removed.
Shared-call application entries can retain reusable environments and operation groups
while their supplying program remains live, independently of escaped closure values.
Detached Rust snapshots confer no retention: once the program graph is retired, they
cannot republish a stale environment into a newly published program.
Receiver selection,
argument/result checks and cross-version interface compatibility still run per call.
Application arguments share their prepared exact type identities. Each immutable
interface method binding derives its structural selection identity once, without
capturing the receiver or an applied result. The existing application index consumes
these shared identities; host entry constructs the same argument bundle. No separate
application solver or result cache is introduced. Canonical applied-layout admission
and TypeArgument-owned proofs reuse nominal facts; genuine invocation-scoped argument,
result and dynamic compatibility checks still run at the call boundary.
Executable environments are published by the runtime after validating their complete
parent/operation graph. The central environment record retains a flat list of exact
program dependencies alongside its immutable edges. Frame entry checks the environment's
owner/slot/generation and current program availability without rebuilding graph traversal
sets. Abandoned candidate leases can invalidate a dependency even before collection, so
availability checks remain mandatory. Environment extensions publish new records; GC
still traces the original edges and atomically validates the graph before detachment.
The dependency list creates no host lease or independent root and does not prevent
reclamation of unrooted environments, operation groups or old programs.
Closure selection and signature admission belong to the runtime call transition. It
borrows the closure once, compares physical parameters without temporary representation
vectors, then supplies captures as the same borrowed prefix used for method receivers.
Host closure entry converges on this policy with a checked value slice. Capture ownership,
semantic environments and parameter banks are admitted before frame publication; the
heap borrow ends before script/native execution. Scoped semantic substitution remains
part of closure signature admission where required.
Shared verified records contain no runtime-local identities. Admission reuses one
session/scope check until frame creation, while retaining dynamic argument, depth
and cancellation checks. All frame returns share retirement, depth release and caller
publication. Required environment/interface adapters run before retirement while
callee roots are live, with no exclusive frame/bank borrow across adaptation.
Unadapted scalar packets retain raw payload transfers; general Values keep heap
ownership and destination checks. Root/factory conversion releases the stack borrow.
Arena allocation order
allows last-allocated windows to truncate both banks; independent out-of-order
retirement still compacts ranges and preserves surviving window identities.
Native callable frames have prepared signature layouts. Native arithmetic and
casts admit tagged inputs once and invoke the same payload kernels as scripts.
Managed results retain their normal allocation/root protocol.

Scalar operations prepare concrete function pointers to shared `kagari-types`
payload kernels after sealing. All integer widths, arithmetic/bit/shift families,
numeric comparisons, f32/f64 operations and supported casts use raw payloads;
mixed-width shift counts keep both checked domains. Source-width overflow and
IEEE bits remain intact. These function addresses are runtime preparation data,
never serialized artifact operands. Native public argument/result adapters remain
Value interfaces; concrete script transfers remain raw through the caller bank.
Scalar operands and numeric contracts are inline; identity-bearing types, strings,
call arguments and other variable-length metadata remain in the canonical immutable
instruction records. Their logical PC is the index, so normalization and hot reload
do not require a second semantic description or change debug locations. The VM
borrows these records at slow boundaries instead of cloning wide instructions.
The runtime cursor supplies a closed nonallocating execution region: it fetches only
sealed operations and reuses entry authority without admitting caller callbacks
or external Values. Field instructions reference a dense function-local table of physical receiver/
value locations, access direction, slots and portable layout IDs. These records
keep the common instruction stream at 24 bytes. Concrete layouts bind directly to
the admitted frame's exact version; scoped applications exit the cursor before
layout preparation, with no operand-bank borrow held. Runtime-linked function
records retain scoped field type arguments from normalized code; the shared verified
execution table holds no runtime-local DefinitionIds. Both paths use the same field
actions and checked heap storage. Write-value admission still precedes layout
preparation and receiver access; reads prepare their layout before reading the
receiver. Type mismatches preserve VM error categories and trap observation without
falling back to canonical interpretation. Copying a field value does not allocate, create a root lease or run a
destructor; the frame and object remain traced at the surrounding safepoints.
The scalar loop hands prepared managed operations back to the cursor's object
handlers without releasing its frame/window access. Keeping those handlers outside
the scalar loop prevents their storage checks from changing its inlining budget;
the handoff retains original logical PC and instruction-slice accounting.
`PreparedManagedOperation` is an opaque sealed record shared by instructions and
this handoff. Its internal variants cover physical value copies, linked constant loads,
managed returns, field/index ordinals and runtime-linked native operation ordinals.
It cannot represent a scalar opcode
or require an unreachable generic-operation arm. Managed local/register copies use
prepared physical locations and the ordinary bank read/write representation rules,
retaining heap owner/generation/kind validation without repeating public frame
admission. Both slots remain traced in the admitted window. Managed returns carry
the rooted value to common frame retirement; there is no separate VM return decoder.
The previous VM LoadConst/LoadLocal/StoreLocal/Move/Return and aggregate field/index handlers
are removed, along with the cursor's logical-register mapping helpers. SDK/reflection
field access still uses the same checked GC storage kernels.
Scalar exits explicitly distinguish boundary, slice, safepoint, return and managed
operation. The region reconstructs payload-free exits directly; only returns and
managed operations carry data across that internal boundary. A cold constant miss
additionally carries its destination and ordinal out of the complete cursor, where
allocation runs after all transient borrows end. PreparedTransition describes the
constant-materialization, scoped-field and tuple-update requests; a separate
non-inlined completion handler owns their allocating work. Ordinary region admission
does not contain those preparation bodies, and the completion
path cannot accept an already-finished region as a request. Operation failures travel
through RegionError, separate from successful RegionExit values; the VM maps runtime
faults, type mismatches and invalid indices to their original error categories before
trap observation. Detailed RuntimeError data is boxed only on failure, keeping its
message/trace payload out of ordinary successful region and object-handler results.
Index descriptors retain physical base/index/access locations without runtime
identities. Array reads/writes use the checked heap kernels shared with SDK/native
adapters, retaining current bounds, access, element type and generation checks.
Tuple writes exit through a compact prepared-operation ordinal and validated index;
the active frame keeps both operands rooted. Completion rereads those physical
operands, copies immutable membership and publishes the new tuple before the next
safepoint. No callback, collection or observation intervenes between cursor exit and
operand acquisition. There is no extra logical PC or instruction-slice charge.
NativeBinding distinguishes callback bodies from runtime-owned NativePrimitive
implementations. Selecting a primitive installs its fixed kernel and exact codecs;
a callback cannot assert a purity flag. StringByteLength reads immutable UTF-8
storage; VecIndex and VecSet/VecSetFluent read or replace checked array elements.
Both scoped native invocation and the prepared cursor use these fixed kernels.
Registration checks the exact parameter/result relationships, including the Vec
element type and mutable access for replacement. Linking requires the actual
installed body, an already prepared exact signature and absence of selected
operations/result adapters. Shared verified instructions contain only native-call
ordinals; runtime links hold operation-specific facts. String length needs only
its physical source; Vec operations also retain argument locations and the
supplying record's immutable native signature. Its closed type provenance belongs
to the already pinned program; no new heap value or type environment is retained.
Arbitrary callbacks and adapters keep the native boundary. Both native cancellation
polls, current heap identity checks and physical result admission are preserved.
No method-name recognition, source analysis or per-call descriptor allocation is
involved. Fixed kernels return a compact Value-or-boxed-error result; detailed
diagnostics occupy storage only on failure, and the cursor transfers that error
box into RegionError. The ordinary native callback API keeps its existing error
type. Private kernel instantiations retain each operation's result facts through
publication while sharing cancellation, error propagation and destination checks;
String's scalar result is not merged with Vec's general result before writing it.
Native execution reports only completion or a native boundary; the region owns
general exit construction. A fixed kernel cannot request unrelated preparation
transitions or script returns through its continuation type.
The stdlib's separate string length/index bodies and owning Vec setter
adapters are removed. SDK and native adapters share checked array reads and setter
preflight/commit kernels. Detached storage remains unavailable, not an empty array;
callbacks, readonly access, bounds and current element contracts remain checked.
The fixed setter copies an already rooted Value without a Rust conversion or heap
growth. SDK conversions still retain their own roots, limits and safepoints because
they may allocate or reenter; committing afterward rechecks the actual storage.
Each scalar segment reuses the region's immutable function and disjoint mutable
logical-PC fields and bounded scalar/initialization slices. Fetch borrows one
immutable instruction; dispatch reads only its selected payload rather than copying
the complete nested enum before classifying it. Instructions still check operand
bounds and initialization without recovering the function or bank range. Closed
managed handoffs keep the same cursor; region exit releases every borrowed view
before callbacks, collection or arena growth. The separate non-inlined scalar
kernel keeps object handlers out of its instruction loop and inlining budget.
The scalar kernel selects bounded or unbounded stepping once at segment entry. Bounded
execution borrows the remaining count directly; unbounded execution carries no
countdown state. Both use the same generic instruction loop, so the optional mode
is not decoded at each logical instruction. Bounded execution retains saturating
step consumption, the already-admitted first instruction, and the original ordering
of exhaustion, cancellation, observation and GC checks. Managed handoffs consume no
second step; successors resume normal boundary checks. The loop owns instruction
decoding and dispatch directly; it returns only when leaving the scalar segment,
without a per-instruction progress result or a second continuation dispatch.
Canonical instructions remain one-to-one with prepared instructions.
Scoped fields use the same physical execution kernel as concrete fields once their
function/environment's layout operand is ready. Only first-use preparation exits the
cursor; subsequent accesses keep its admitted frame/window scope. Other unmigrated operations still
use the ordinary canonical boundary; field migration does not imply their completion.
The VM owns the
frame driver, cold dispatch, safepoints and observation. The cursor checks
cancellation, observer requests and candidate-reclamation notifications at each
original program point. The last unpublished CandidateLease release sets a runtime-
local atomic request without borrowing or retaining module storage; publication
disarms that lease. Weak identity/strong-count checks still own program availability.
Collection consumes the request before discovering roots, restores it if graph
processing fails, and leaves concurrent releases pending for a later safepoint.
The signal is conservative and never grants execution authority. External safepoints
retain module-store borrow validation; the closed region polls the signal without
reacquiring that borrow or scanning the staged program table. Collector threshold
eligibility is invariant within the closed
region and is recomputed on entry after every allocating or reentrant boundary;
the driver has already checked the first PC before acquiring it. Full safepoints
and error observation run after releasing the cursor. Values retain full scalar
precision and complete handle identities; large immutable host descriptors are
shared out of line rather than inflating every scalar execution slot.

Standard Vec length/read/push operations borrow the native frame's existing roots
and prepared type arguments, retaining declared access, bounds and dynamic lease
checks without constructing temporary owning SDK handles. Callback algorithms and
Rust-retained handles keep the owning conversion protocol. Builtin map reads borrow
the stored key contract and entries together; custom Hash/Eq callbacks still run
outside table borrows. Collection Option results reuse immutable declaration handles
but validate runtime-local enum layouts and allocate ordinary traced enum objects.

The SDK's default artifact path applies the bounded MIR pass pipeline before both
bytecode and portable native input emission. Copy forwarding requires equal complete
semantic contracts and immutable temporary definitions. It retains local stores,
heap/module/cell reads, checked traps and side effects. Local snapshot facts may
cross CFG edges only when every processed predecessor agrees on the same immutable
definition; unprocessed backedges and calls kill forwarding. Named local stores
remain for debugging; fresh verification rebuilds roots, liveness and provenance.
Scalar constants are shared
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

Builtin Array typing and MakeArray/RepeatArray lowering own fixed-length syntax
and element context. Array indexing and indexed assignment carry intrinsic checked
facts; assignment does not require MutableList. The bounded `builtin::array_bridge`
retains only independent Array context and installed List indexing roles. Nominal
Vec and List indexing use checked Index/MutableList contracts. Place lowering
preserves once-only root/index/RHS evaluation and completed effects.

Runtime-owned Array objects and registered nominal sequences may share compact
SequenceStorage kernels after family admission. Growth, capacity and detached
edit leases require nominal sequence storage; builtin array replacement retains
bounds, owner/generation and callback guards. No public sequence codec or Rust Vec
conversion accepts an Array handle. String interpolation uses fixed [String]
temporary storage owned by its compiler/runtime primitive contract.

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

Registered trait defaults render as real single-tail-call bodies with explicit
generic arguments, forwarding parameters in order to typed native helpers. HIR
checks each body in its declaring context and records its call in
`TypeTable::native_default_call` only after type/bound checking and registration
agreement. Source lowering consumes that call and keeps direct native dispatch,
without an extra script frame. Interface metadata retains the same portable
recipe; source-free linking independently validates it against registration.
The SDK checks all provider bodies before publishing generated cache files and
retains the successful analysis under their final source identities. Signature
queries remain body-free; invalid or cancelled analysis publishes no checked body.

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

## Owned interpreter activation (AX01)

Runtime session storage owns both synchronous and independently parked frame stacks.
An owned execution token requests cancellation/retirement without keeping a Runtime
borrow. `session::owned` validates entry/activation identity and drains abandoned
records; `frame::owned` admits only safe slice exits, retaining owned iteration
leases while excluding mutation/host/native borrows. Call depth is parked/restored
per root. Operand windows can be released out of order and compact their backing
banks without changing live generational identities.

VM `vm::owned` supplies start/drive orchestration over the existing executor. The
cursor yields after the requested instruction interval; the executor defers the
exit through non-suspendable synchronous operations. Native bodies cannot be forcibly
preempted. Completion roots its output and releases frames; a wake requests host
attention without entering script. `native::future` and `frame::waiting` connect
typed cold producers to durable completion reservations. A session owns its pending consumer/destination; the VM
returns Waiting and resumes the same PC after checked driver-side conversion.
Heap Future state is claimed once, so aliases cannot repeat submission. Ordinary
entry and native installation reject resume bodies. MIR Await lowers into the same
instruction; both MIR and bytecode independently check initialized live values,
iteration-stack joins and host capabilities at suspension points. Sealed bytecode
retains computed await liveness, which physical allocation maps to retained managed
locations. The runtime discards dead slots before its resource check, preserving
live aliases and debugger-visible locals. These facts are rebuilt after decoding,
never trusted from an artifact flag. The foundation owns `core::future::Future<T>`;
the SDK's `runtime::owned` maps drive results to embedding errors while reusing the
runtime owner, readiness and cleanup path. Per-execution cancellation is separate
from the host's shared context signal. Native preparation rejects resume bodies
before invoking a backend. Source async functions and explicit async closures lower
to ordinary factories plus private resume functions. Portable `MakeFuture` captures
checked arguments without running the resume body; its sealed payload retains a
fully captured closure with the existing executable/type environment edges.
Independent verifiers check the Future storage role, resume target, exact capture
signature and output. Runtime graph checks cover reachable cells/interfaces and
native traced edges at capture, first drive and safe parking.

The SDK/VM `start_future` entry explicitly queues one Future layer. Ordinary calls
return cold values, including Future-valued outputs without flattening. Nested
script awaits push owned resume frames; native waits also work directly as a root
without an artificial script caller. Real deferred for-body waits retain the same
cursor and lease, observe later element replacement and reject structural writes.
Cancellation and owner retirement release the lease. Source, Task scope and
lifecycle integration have passed focused local gates; the roadmap distinguishes
these results from full GitHub CI acceptance.

`task` owns bounded scope admission, generation-checked identities, readiness,
dependencies and terminal reports. `spawn` queues a checked callable without
invoking it; the first owned activation calls that factory and drives its Future.
Task payloads trace a cached result independently of execution/report records.
Waiters only observe that cache and subscribe to readiness; they never drive the
target. A waiter cancellation removes its dependency, while target cancellation
propagates the original failure identity. Scope close requests cancellation, and
the serialized driver drains cleanup even when IO or its dispatcher never replies.

Waiting frames and cold factories retain their original executable/type environment
through compatible reload. New roots resolve the new publication. Detached bounded
Spawn/Await origins preserve portable task, scope and code identities without
retaining GC or version leases. Debug snapshots identify their root execution;
independent parked roots do not become synchronous reentry frames or consume each
other's single-step requests. Observers can be replaced between bounded drives.

The [async_tasks host](../crates/kagari-embed/examples/async_tasks/main.rs) and its
source-free artifact consumer exercise two independently registered providers and
a non-Actor dispatcher. No Tokio runtime, JIT suspension or script threads are
required. Native preparation declines resume bodies before backend entry.
