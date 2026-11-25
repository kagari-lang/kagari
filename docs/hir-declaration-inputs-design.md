# HIR declaration inputs: compile from records, navigate from source

**Status: proposed replacement, not current behavior.** This is the input design for the [Native model](native-model-design.md), not a new provider format or an LSP implementation plan. The [current input contract](spec/standard-declarations.md#declaration-analysis-inputs) still applies until migration. The design removes `ExternalModule`; it does not rename it or distribute its attachment maps among new wrappers.

**First checkpoint: HIR input and resolution only.** The [execution plan](hir-declaration-inputs-plan.md) owns phases, evidence and progress. Trace a request as `SourceSnapshot + AnalysisInputs → LoweredModule per logical module → one import graph → declarations → signatures → bodies`. The two input routes must resolve the same definitions; they need not have the same source ranges. Host execution, async retention and editor refresh have separate checkpoints below. Keep their existing safety checks until their consumers migrate; do not make them new HIR inputs.

## Final input types

Two values answer different questions: `SourceSnapshot` is **the files in this request**; `AnalysisInputs` is **the environment in which to analyze them**. The Rust definitions below are the target, not a new public API to copy into each crate. Imports, derives and visibility are omitted. Unless marked as an excerpt, all fields are shown.

```rust
struct SourceSnapshot { // existing, unchanged
    revision: Revision,                            // revision of this capture
    files: Arc<BTreeMap<FileId, Arc<SourceFile>>>, // exact documents, including user overlays
    packages: Arc<PackageEnvironment>,             // project's selected packages and dependencies
}

struct AnalysisInputs { // changed: no ExternalModule, native_sources or LibrarySources
    core: Arc<[Arc<SourceFile>]>,                  // bundled .kgr documents; shared once, outside `files`
    native_modules: Vec<Arc<DefinitionMetadata<ModuleDecl>>>, // compiler registrations; empty for LSP
    import_environment: ImportEnvironment,         // explicit module aliases, edges and preludes by importer
    source_root: String,                           // stable lexical base for source paths
    parse_limits: ParseLimits,                     // limits on caller source; bundled core uses its own defaults
    const_limits: ConstLimits,                     // constant-evaluation budget
    max_semantic_diagnostics: usize,               // diagnostic budget
}
```

There is **no** `CompilerInput`/`LspInput` enum and no `ModuleDecl` inside a source file. Keep the existing query shape: `AnalysisDatabase::declarations(&mut self, source: SourceSnapshot, cancel: &CancellationToken)` returns `Result<DeclarationSnapshot, AnalysisError>`. The database carries the settings shown in `AnalysisInputs`; `inputs()` captures them for replay, not the mutable definition allocator or previous HIR results. A new editor buffer revision changes `source`, not the Host registrations.

## One declaration owner per module per request

| Consumer | User modules | Language core | Host modules |
| --- | --- | --- | --- |
| Compiler | Project `.kgr` | Bundled authored `.kgr`, once | Scoped Rust `ModuleDecl` records, lowered directly |
| Editor (LSP via analyzer) | Project `.kgr`, including unsaved overlays | The same bundled sources, once | Host-generated `.kgr`, parsed as ordinary source |

In Rust, the setup at the call site differs only in **which documents and records are supplied** (independent sessions; `imports` denotes the corresponding import selections):

```rust
// Compiler: `project` has the user files; `registrations` has scoped ModuleDecl records.
let mut compiler = AnalysisDatabase::new(root)?;
compiler.set_import_environment(imports.clone());
compiler.set_native_modules(registrations);
let compile_decls = compiler.declarations(project, &cancel)?;

// Editor: `workspace` already contains user files AND generated Host .kgr documents.
let mut editor = AnalysisDatabase::new(root)?;
editor.set_import_environment(imports);
let editor_decls = editor.declarations(workspace, &cancel)?;
```

Each database includes bundled core by default. A declaration tells the checker **what a script can name and do**. A Rust callback tells Runtime **how to execute an operation**. Neither frontend consumes callbacks. The editor does not run the Host, receive `ModuleDecl`, import process-local definition IDs, or reconstruct a registration from text plus sidecars. The compiler does not read editor exports, even when asked to write them. No third, independent offline declaration mode is required.

### Follow one call

Suppose a Host registers `number() -> i32` in a module exposed as `game::api`. Its generated document contains `pub native fn number() -> i32;`. The user writes `fn score() -> i32 { game::api::number() }`.

```text
Compile: scoped ModuleDecl(game::api) -> direct lowering --+
Editor:  game/api.kgr                -> parse and lower ---+  choose ONE row
                                                          v
User files + authored core -> parse and lower ----> set of LoweredModule
                                                          |
                                     imports and declaration identities
                                                          |
                                      signatures, then body checking
```

Both resolve `number` in the same logical module and reject `number(1)` for the wrong arity. The editor's definition site is the `number` token in the captured generated file; the compiler can have a structured origin with no token at all. Compare canonical module/member identities across runs, **not** the integer IDs allocated by separate analyses. Only compilation proceeds to checked executable contracts. A missing Rust entry is a link/load error, not an editor diagnostic.

## Supporting input types

Use the existing [source capture](../crates/kagari-source/src/source_database.rs) for editor exports. The Host/exporter must give the tooling adapter a mapping from document to **logical module**, not just a directory of content-hashed filenames. Use the existing `CapturedSource` vocabulary; no new on-disk semantic manifest is proposed:

```rust
struct CapturedSource { // existing: what the source database receives, not an extra HIR input
    key: String,          // stable document key for replacement and overlays
    display_name: String, // physical or virtual path for navigation and diagnostics
    module: ModuleIdentity, // canonical package + module segments, independent of the filename
    text: String,         // exact .kgr text, including docs and complete Script bodies/defaults
}

struct ImportEnvironment {
    modules: BTreeMap<ModuleIdentity, ModuleScope>, // importing module -> its explicit selections
}

struct ModuleScope {
    packages: PackageScope,                 // package names available to this importer
    dependencies: BTreeSet<ModuleIdentity>, // explicit module edges, even without a written use
    preludes: BTreeSet<ModuleIdentity>,     // implicit imports; conflicting selections are diagnosed
}

struct PackageScope {
    dependencies: BTreeMap<String, PackageId>, // alias visible here -> canonical package
    selection: String,                        // producer's selection fingerprint for replay
}
```

Reuse these [import scope types](../crates/kagari-hir/src/imports/environment.rs); none grants private access. Normalize producer and project defaults once before building the graph; an explicit importer scope replaces its defaults. Inline modules inherit the appropriate producer context, not arbitrary global aliases. Delivery may include identity and import configuration, **not** field policies, method selectors or an ID table for editor checking.

**Rendering is relative to the importer.** For a generated `pub use lib::Token`, choose `lib` from the generated module's effective scope, not the target package's preferred spelling. Resolve the emitted path back to the registered target's canonical identity and namespace. If no legal spelling exists, report the export error rather than silently selecting another `Token`. Projection parity is checked with two packages exporting `Token`, comparing definition targets, not just diagnostics.

**The editor owns refresh.** Keep package base text, user overlays keyed to their logical document, and generated read-only documents separate. Replacing the generated set produces a new effective `SourceSnapshot`; it must not pass that set to `SourceDatabase::replace_captured` on the live editor database, which replaces the whole batch and resets overlays. Check edit → refresh exports → close: the edit survives refresh, closing restores disk text, and an older snapshot still navigates to its old export. HIR consumes the resulting snapshot; it does not merge files or watch directories.

### What remains on the compiler side?

Keep [`ModuleDecl`](../crates/kagari-types/src/declaration/module.rs). It describes a Rust-registered module, not every possible Kagari source file. `DefinitionMetadata<ModuleDecl>` pairs this record with the definition table owning its IDs. Adopt that scope before lowering; do not copy raw IDs between sessions.

```rust
struct ModuleDecl {
    identity: ModuleIdentity,                   // module that owns the following definitions
    package_alias: Option<String>,              // exposed package spelling, if any
    prelude: bool,                              // offered as an implicit import
    dependencies: BTreeSet<ModuleIdentity>,     // other module owners this registration needs
    types: Vec<TypeDef>,                        // declared types and their members
    traits: Vec<TraitDef>,                      // traits defined here

    implementations: Vec<ImplDecl>,             // impl blocks declared here
    functions: Vec<FnDecl>,                     // free function signatures and implementation kinds
    script_bodies: BTreeMap<DefinitionId, ScriptExpr>, // Script definition -> supported expression body
    private_functions: BTreeSet<DefinitionId>,  // callable by ID, not publicly exported
    exports: BTreeMap<ExportName, DefinitionId>, // re-export name/category -> original definition
    variant_exports: BTreeSet<String>,          // enums whose variants are exported at module scope
    module_documentation: String,               // module overview rendered for tooling
    documentation: BTreeMap<DefinitionId, String>, // declaration -> documentation
    paths: Vec<HostPathDeclaration>,            // executable Host access contracts, not HIR field permissions
    callable_requirements: BTreeMap<DefinitionId, Vec<NativeCallableRequirement>>, // ordered operations per callable
    concrete_results: BTreeMap<DefinitionId, Ty>, // callable -> concrete result behind an interface
}
```

No field holds a Rust callback. A Native function has a signature but no Rust body here; a structured Script default may have a `script_bodies` entry. [Direct lowering](../crates/kagari-hir/src/native/structured.rs) constructs HIR without generating or parsing text.

### An impl refers to a trait; it does not define it again

```rust
struct ImplDecl { // existing structured impl record in ModuleDecl.implementations
    generic_params: Vec<GenericParam>, // type parameters declared by this impl
    bounds: Vec<GenericBound>,         // obligations under which the impl applies
    trait_type: Option<NominalTy>,     // Some(applied trait identity); None for an inherent impl
    for_type: Ty,                      // type receiving the implementation
    methods: Vec<FnDecl>,              // methods supplied by this impl, with their own signatures
}
```

Suppose `game::api` defines `pub trait Health { fn hp(self) -> i32; }`. The Host registers `impl Health for Player`: `trait_type` identifies `game::api::Health`, `for_type` identifies `Player`, and `methods` declares the Host's `hp(self) -> i32`. Only `game::api` defines `Health`. Registration can precede the arrival of the `.kgr` file: recording an identity is not proof that the trait exists or is accessible. The Host does not supply a second `TraitDef` to fill that gap. Its registration builder must obtain **the impl's own `FnDecl`** from an explicit declaration or a typed adapter signature and check the binding against it. The [existing builder](../crates/kagari-runtime/src/native/builder/implementation.rs) uses the trait contract to construct methods; copying `Health.hp` into the impl would turn the later signature comparison into a comparison with itself.

Once both modules are in the graph, HIR resolves the trait in the impl's import scope. A structured identity and a written path must reach the same visible trait under the same dependency rules. The ordinary impl checker compares against that owner's checked signature, including generic substitution, bounds and associated members. Missing required methods fail; omitted methods with defaults need not be repeated. If `Health.hp` changes to return `String`, the unchanged Host method returning `i32` fails just as a source impl would. This follows [Rust's trait implementation rule](https://doc.rust-lang.org/reference/items/implementations.html#trait-implementations); Kagari resolves the owner in the joint analysis, not necessarily when the Host builds its registration.

**Loading is a separate check.** When emitting an executable that requires a foreign trait, derive its required interface from the checked owner's definition and the resolved uses (including bounds and Native callable requirements, not only impl headers). Keep the portable requirement in the artifact, **not** in `ModuleDecl` or `LoweredModule`. Source-free linking compares it with the reachable owner's contract and validates the actual installed bindings against the required operations. Comparing two copies taken from the importing artifact would not establish compatibility. Runtime needs neither `.kgr` nor HIR. Preserve the [existing load checks](../crates/kagari/src/runtime/tests/embedding/offline_nominal/traits.rs): derive mandatory trait identities independently from the installed provider's requirements, so omission cannot authorize a load. For this migration keep exact contract matching, including rejection of a changed trait with an added default method against an unchanged installed expectation. Keep that expectation at the installation/compiled-contract boundary, not as a Host-authored `TraitDef` in HIR. Relaxing compatibility is a separate decision.

### The common result

Both paths produce the same [`LoweredModule`](../crates/kagari-hir/src/lower.rs), before type checking:

```rust
struct LoweredModule {
    source: Arc<SourceFile>,           // real document or registration's logical graph key
    module: Module,                    // per-module HIR declarations and body nodes
    input_kind: ModuleInputKind,       // provenance only; never selects a type checker
}

enum ModuleInputKind {
    Source,     // user, generated Host or bundled core .kgr: real text and ranges
    Structured, // registration: logical module/file identity, no invented source span
}
```

[Declaration preparation](../crates/kagari-hir/src/analysis/declarations.rs) collects lowered modules, rejects competing library/project owners, builds one import graph and resolves declarations before signatures and bodies. Keep existing project ambiguity diagnostics. Keep definition adoption, cancellation, per-source diagnostics, immutable old snapshots and checked compiler admission. For implementation precedent, [rust-analyzer at Rust 5f4effd57950](https://github.com/rust-lang/rust/blob/5f4effd57950/src/tools/rust-analyzer/docs/book/src/contributing/architecture.md) puts source/project acquisition outside semantic analysis. Kagari additionally has a structured compiler input and a source-free Runtime; we reuse the ownership boundary, not its incremental database.

### Host declarations after lowering

For a `Player` with a field `hp`, the declaration, the selected method and the checked field uses have these target shapes. They are **not** three new containers: simplify the existing [native type and field](../crates/kagari-hir/src/hir/item/adt.rs), [inherent method signature](../crates/kagari-hir/src/aggregates.rs) and [type table](../crates/kagari-hir/src/typeck/table.rs).

```rust
struct NativeTypeDeclaration { // remove host: Option<HostContract>
    origin: NodeOrigin,              // source site or structured registration origin
    name_span: Option<Span>,         // exact name for navigation, if there is text
    id: NativeTypeId,                // slot in this module's native type declarations
    visibility: Visibility,          // who can name this type
    name: String,                    // e.g. Player
    generic_params: Vec<GenericParamId>, // declared type parameters, in order
    bounds: Vec<TraitBound>,         // where-clause requirements
    trait_bounds: Vec<TraitRef>,     // traits required on the type surface
    fields: Vec<Field>,              // ordinary field declarations, in order
}

struct Field { // unchanged; shared with script structs
    origin: NodeOrigin,              // where the field was declared
    name_span: Option<Span>,         // written name, when present
    id: FieldId,                     // owning declaration and field slot in this HIR
    visibility: Visibility,          // which callers may access it
    writeability: Writeability,      // val or var, independent of access path
    name: String,                    // e.g. hp
    ty: TypeRefId,                   // HIR type reference; signature checking resolves it
}

struct InherentMethodSignature { // no receiver_access: Rust loans are not lookup criteria
    id: SourceDeclRef,               // qualified source declaration reference
    declaration: DefinitionId,       // canonical method identity in this analysis
    site: Declaration,               // navigation record
    owner: TypeId,                   // receiver type pattern
    visibility: Visibility,          // who can call it
    function: TypedFunction,         // checked Kagari signature, including self
}

struct TypeTable { // excerpt: field selection and expression/place types only
    field_types: HashMap<FieldId, TypeId>,      // declaration -> checked field type
    expr_fields: HashMap<ExprId, DefinitionId>, // field read -> selected field definition
    place_fields: HashMap<PlaceId, DefinitionId>, // assignment target -> selected field definition
    exprs: HashMap<ExprId, TypeId>,             // expression -> inferred value type
    places: HashMap<PlaceId, TypeId>,           // assignment target -> expected value type
}
```

For `player.hp = 10`, HIR needs neither a callback ID nor a registered path: `place_fields` selects `Player.hp`, `places` gives `i32`, and the field declaration supplies `var`. Lowering to executable operations consumes these checked IDs and the compiler's registered contracts. Remove `ResolvedHostPath` / `ResolvedHostPlacePath` as field-checker outputs; keep handle, schema and scoped loan checks outside these structures. Async capture adds no permission field.

**Removed inputs:** `ExternalModule`, `LibrarySources`, the parallel `native_sources` list and both `set_external_modules` / `set_native_sources` routes. Rendered `.kgr` remains a tooling output, not an attachment to a registered HIR node. The following table maps the deleted fields; it does not define another input struct.

## Where each `ExternalModule` field goes

This is a deletion map, **not** a recipe for serializing the old fields into generated files. The test for moving a fact is: can changing only this fact change whether a user's script is valid, its selected member or its inferred type? If so, a published editor projection must express it or derive it unambiguously. Other facts may stay compiler/linker-only.

| Current field | Replacement or removal |
| --- | --- |
| `source` | Real source capture: ordinary request files or the bundled core slice. |
| `package_alias` | Importer's package alias → package ID binding. Do not use the generated filename as an alias. |
| `prelude` | Producer selection becomes `ModuleScope.preludes` of importing modules. Core itself supplies no implicit prelude. |
| `dependencies` | Explicit edges in `ModuleScope.dependencies`, in addition to package selections and authored imports. |
| `enum_failures` | For core, derive failure reporting from the exact canonical `Result::Err` identity, not `Err` spelling or an external flag. Preserve trace creation and forwarding; see the [existing narrowing design](native-model-design.md#enum-failure-origin-metadata-deferred-narrowing). |
| `import_namespaces` | Source re-exports must resolve to the same identity **and name category** as the structured export. If the spelling is ambiguous between categories, fix the projection or specify a source disambiguation before supporting that export; never choose the first match. |
| `types` | Recognize language constructors by canonical owner and checked shape on both paths. An ordinary `native type` has no constructor privilege. Host-specific checks belong with `paths` and the registered type, not a second string-keyed category map. |
| `hosts` | The registered type owns its fields; the editor reads their types, visibility and `val`/`var` from generated source. Remove path-specific field permissions from script checking. Keep schema and adapter validation at the Host boundary. |
| `conversions` | Emit actual traits/impls and resolve them normally where sufficient. Static conversion adapter selection currently affects member lookup; for any such API not derivable from canonical language roles and source declarations, projection parity is an open requirement, not a removable binding detail. |
| `methods` | Emit member declarations on their actual owner. Preserve script-visible override rules. Method lookup checks the declared `self` type, visibility, signature and bounds; it does not filter candidates by the Rust adapter's `&self` or `&mut self` requirement. Validate adapter compatibility at registration/linking and enforce actual loans during execution. No selectors keyed by the ordinal of an `impl`. |
| `contracts` | Preserve verified ordered operations, Host call policies and concrete interface result evidence on the structured path. Editor needs the public bounds, effective access restrictions and result type visible to the caller, not operation-slot or implementation tables. Audit policies that affect caller diagnostics before removal. |

### Host field access belongs to the declaring type

If `Player.current` and `Player.baseline` both have type `Stats`, then `player.current.hp` and `player.baseline.hp` both select **the same field declaration**, `Stats.hp`. With `pub var hp: i32`, assignment to either is legal; with `pub val hp: i32`, assignment to neither is legal. Declaring `Player.baseline` itself `val` prevents replacing `player.baseline`; it does not freeze the `Stats` object. The exporter writes `Stats.hp` once. Neither the compiler nor the editor consults a permission table keyed by the route to it.

The current [Host field checker](../crates/kagari-hir/src/typeck/body/host_access.rs) consults path contracts for reads as well as writes. In the target design, reading `player.current.hp` gets its `i32` type directly from `Stats.hp`. HIR records the selected field IDs and types. The compiler consumes those facts to lower the read or write without resolving field names again; the editor needs no path contract. At registration/linking, an adapter must support the operations the field promises. Reject an inconsistent binding there; do not grant unchecked writes, silently narrow `var` to `val`, or introduce path annotations for the editor. Runtime still enforces schema, handle and scoped borrow safety when executing the operation.

The execution checkpoint must distinguish **reading a prefix** (`player.baseline`) from **writing the final field** (`Stats.hp`); do not require a writable adapter for replacing `baseline` just to traverse it. This does not upgrade a borrowed read-only runtime view. For a virtual member or Host index, require an actual source-visible member or index declaration; otherwise expose the operation as an explicit method rather than inferring a field from its adapter. Preserve once-only evaluation and mutation order of the [existing mixed index/virtual path operation](../crates/kagari/src/runtime/tests/embedding/offline_nominal/paths.rs), including its source-free load. Keep its current executable lowering until a checked replacement passes that scenario.

### Async checks the value at execution, not the Native type name

HIR still checks normal parameter, capture and `await` typing. It must not reject an async parameter or capture merely because its type is Native, Host or contains such a value. A Kagari-managed Native object is captured and traced like any other managed object. No borrow marker or async eligibility flag is exported to the editor.

For example, an async function may take `player: Player`, wait for the next tick, then read `player.hp`. A Future may retain a checked Host handle, **not a Rust `&Player` or `&mut Player`**. On access, Runtime validates the handle's owner, generation and any scope, then the adapter obtains a fresh short loan. If the player was removed during the wait, report a runtime error before accessing Rust state. If an actual borrow or mutation guard would have to escape its call or remain active at suspension, reject that operation before it escapes or parks; checking a dangling Rust reference after resumption is not a safety mechanism.

Runtime owns object invalidation, not HIR or the GC. Start with non-reused Host object IDs within a registry: retiring an object invalidates its handle permanently. If IDs must be recycled, require a checked per-object generation before allowing reuse; a schema version is not an object generation. A Future retaining player A must fail on access after A is retired, never read a new player B under the same ID. Do not make restricted path views or live loans storable just to retain durable handles. The [current registry](../crates/kagari-runtime/src/host/registry.rs) and [value storage rules](../crates/kagari-runtime/src/value/properties.rs) need independent lifecycle evidence before enabling this behavior.

Migration includes the current HIR `contains_host_value` checks and the [execution contract](spec/execution.md#owned-async-execution): distinguish retained handles from active resources at cold Future creation and suspension, including MIR/bytecode verification and source-free loading. Keep independent verifier checks for malformed artifacts, root lifetimes and cleanup on cancellation or trap. Acceptance: both frontend routes accept the same `Player` example; actual suspension and resume preserve a managed object or valid handle; invalidating the Host handle during the wait fails on access; a live Rust loan is never parked.

### Other caller-visible facts

Check ambiguous re-exports, trait conversion adapters and final defaults against their source projections. If a required API cannot express a caller-visible distinction, fix the projection or source rule before removing its current route; do not publish a misleading declaration. Preserve complete registered Script bodies on both routes. If a current paired input supplies a real Script body missing from registration, move that definition to authored source or make the structured body complete before removing the paired API. A generated cache must never become the compiler's hidden source of function bodies.

## Migration constraints

1. **Separate assembly from analysis.** Preserve the existing renderer as Host tooling output; the [SDK currently renders before selecting the HIR route](../crates/kagari/src/engine/declarations.rs). Make `declaration_cache` control publication, not how compilation lowers a registration. Requesting generated text for tooling remains possible. Materialized editor files do not turn a structured compilation into a complete authored-source manifest; callers requiring one must receive the existing missing-manifest outcome when appropriate.
2. **Prove declaration equivalence.** Compare one source-only analyzer request with one structured-Host compiler request. Use the `Health` impl and same-named `Token` cases above: both routes must select the same canonical owner, reject the same wrong signature or inaccessible trait, and navigate to exact name ranges when real text exists. Preserve core ownership, dependency scopes, ordinary project ambiguity and snapshot replay. A prepared snapshot suffices; this checkpoint does not claim working editor refresh, Host writes or async suspension.
3. **Cut over consumers, then remove obsolete inputs.** Replace the [registration API that takes an expected `TraitDef`](../crates/kagari-runtime/src/native/builder/references.rs) with scoped identity references at their use sites. Remove `ModuleDecl.trait_references`, `LoweredModule.trait_references` and the `registered_trait_access` lookup exception together. Migrate [compiler contract emission](../crates/kagari-compiler/src/source/lower/abi.rs) to checked owners before removing the old copy path. On both routes exercise a source-owned `Health` with a Host impl, including a changed return type and an inaccessible trait; independently reject an incompatible source-free link. Retire `ExternalModule`, its attachment/selectors and the `set_external_modules` and paired `set_native_sources` paths, including replay/fingerprints that reference them. Move core to authored sources with reserved owner/shape checks. Keep independent structured contract validation, not old offline attachment tests. Update SDK callers, examples, specs and the HIR guide together. Recompute environment identities from actual inputs (core text/owners, declarations, scope selections and limits); preserve overlay revisions and old snapshot navigation. Do not reuse stale products or bump the unpublished ABI for this change.
4. **Separate follow-up gates.** Compiler/linker: verify the `val` prefix/`var` leaf, mixed-path operation order, one matching source-free load and one incompatible binding rejection before retiring their old contracts. Runtime: permit async retention only after the valid, retired and live-loan cases above pass; do not broaden general value storage at the HIR checkpoint. Editor: verify overlay/base separation on export refresh at the workspace owner. Until each gate passes, retain its existing checks and do not claim that input equivalence establishes execution or tooling acceptance.

This changes frontend inputs, tooling publication and their consumers. It does not introduce binary prebuilds, prescribe export directory discovery, redesign Host borrowing, or change reload call selection. The [execution plan](hir-declaration-inputs-plan.md) sequences these requirements; the [current spec](spec/standard-declarations.md) remains the behavior reference until implementation lands.
