# HIR Model Review Notes

**Status:** HR04–HR08 compiler/tool separation is implemented on owned, ID-linked HIR.
Required acceptance remains open in the
[roadmap](implementation-roadmap.md#hir-storage-and-attribute-refactor).
The arena/reference migration is withdrawn, not completed. Native frontend phases 1–3 are
implemented; their acceptance limits, inline-identity review and cache-retention policy changes
retain their existing roadmap owners.
[Current HIR](architecture/hir.md) and [Runtime](architecture/runtime.md) own implemented contracts.

## Selected storage and attribute scope

`Module`/`HirNodes` retain ordinary owned category tables and ID links. `Module.nodes` is shared
module-wide storage; semantic body names such as `BodyOwner`, `BodyRoot` and `CheckedBodyInput`
are unchanged. `Function.params` owns parameter positions; `Module` owns typed attribute
attachments. `LoweringId` identifies one lowering, not an allocator. There is no borrowed graph,
universal owner table or session cutover.
See [storage and IDs](architecture/hir.md#storage-and-ids) for current data structures.

## Compiler and tooling boundary

The proposed retain/move/split inventory is implemented. The
[architecture owner](architecture/hir.md#editor-query-views-and-indexes) documents `.queries()`
views, canonical bindings, demand-built scope/site/navigation indexes and shared semantic
member eligibility. HIR retains checked admission, `ResolvedTypeRef.target`, import solver
observations and diagnostic/debug/reload origins. Lint projections are private to lint;
compiler, SDK and HIR production targets do not depend on analyzer. No new LSP capability,
references/rename index or scheduling model is implied.

### Cache policy is separate

Current caches retain owned declaration/signature/body records but only weak graph/query
handles; full-file bundles are weakly indexed. Without a live matching graph, queries lower/check
afresh. Native preparation retains validated registration/source/parse, not graph-local HIR.
Body facts remain separate from query context: sharing a body cannot retain stale dependency
validity or eligibility. See [cache ownership](architecture/hir.md#query-work-reuse-and-publication).

A bounded strong-cache follow-up first needs a concrete warm-query/retained-result workload,
retained unit, invalidation/eviction and clone contract, then measurements. It must preserve
identity/remapping and fact/context separation. No policy change or speedup is part of HR04–HR08.

### Finite inline-identity follow-up

The audit confirms that `LocalId`, `GenericParamId` and `TraitMethodId` are allocated from
source-map span slots while their records live elsewhere. Their finite replacement is part of
the [queued origin ownership plan](#node-owned-origins-and-record-based-ids), not a separate queue.
The parameter repair does not authorize a uniform ID representation, physical per-owner storage
or universal graph.

`LoweringId`, `ResolvedTypeTarget` and internal `PreparedSignatures` name current roles;
`VisibleBinding`/`CompletionCandidate` name analyzer products. `FileAnalysis`/`AnalyzedModule`,
`BodySignatures` and `update_source` stay unchanged. Broader naming, incremental parsing,
function-granularity reuse/reclamation and LSP scheduling require separate scope.

## Analysis flow and naming review

**Status: discussion recorded; revisit after the source walkthrough.** No final names, module
moves or implementation phases are approved. This review is independent of SO00–SO03 below.

The main flow is hard to discover: declaration/signature/body orchestration is split between
`lib.rs`, `analysis.rs` and `analysis/{declarations,signatures,body}.rs`. Names also mix stage
results, retained facts and query views without consistently identifying those roles.

The conceptual full-analysis path is:

```text
Prepare structure     source AST / structured declarations -> LoweredModule per module
Build declarations    module namespaces + imports + declaration identities/conflicts
Analyze signatures    semantic types, generics, bounds and aggregate contracts
Analyze bodies        constant prerequisites, lexical resolution, types and selected operations
Admit compilation     checked dependency closure + matching facts/origins -> CheckedCompilation
```

HIR already contains declaration contents. "Build declarations" means establishing lookup and
semantic identity around those records, not constructing another Function/Struct definition.
Calls and field accesses are checked in body analysis; registering an impl does not validate its
trait obligations. This is a dependency outline, not a mandatory whole-input pass sequence:
declaration-only, signature-only and selected-body queries retain their existing depth boundaries.

### Direction to evaluate

- Put a short input/output/non-responsibility contract at each stage entry and link one reading
  path from the crate root. Distinguish per-module work from whole-input graph/catalog preparation.
- Prefer the existing stage owners under `analysis/`; move core orchestration out of `lib.rs`
  where responsibilities fit. Keep import solving in `imports`, name-resolution algorithms in
  `resolver`, checking algorithms in `typeck` and compilation admission in `program`/`compile_input`.
- Separate semantic stage work from database scheduling, cache reuse and snapshot retention.
  Full and selected-body routes should share applicable mechanisms without eagerly checking more.
- Review stage results, retained facts and file/query wrappers together before renaming; merge
  overlapping ownership where demonstrated, rather than add forwarding wrappers or aliases.

| Current structures | Review question |
| --- | --- |
| `DeclaredAnalysis`, `PreparedSignatures`, `AnalyzedModule` | Can names consistently identify the completed stage? `DeclarationAnalysis`, `SignatureAnalysis`, `ModuleAnalysis` are candidates only. |
| `DeclarationRecord`, `SignatureRecord` | Can names express retained semantic facts and distinguish them from graph-bearing stage views? |
| `FileDeclarations`, `FileSignatures`, `FileAnalysis` | Which wrappers own distinct source/query context, and which overlap internal stage results? Do not merge away real lifetime/retention boundaries. |
| `TypeTable`, `ModuleFacts`, `ModuleSignatures`, `BodySignatures` | Make types, selected operations, aggregate results and signature normalization stages discoverable without multiple ambiguous "facts" owners. |

Before implementation, agree on a stage input/output diagram, an existing-type retain/merge/rename
map and a file-ownership map. Preserve recoverable diagnostics, positive completion evidence,
source-free input, scoped identities, checked admission and existing cache/retention contracts.
This is not authorization to redesign storage, introduce a pipeline framework or change semantics.

## LoweredModule boundary remediation

[Native input implementation](architecture/hir.md#native-declaration-inputs-and-origins) supports
direct lowering and parsed inputs with common binding-free HIR and checking regardless of
LSP/execution intent. A–F were implemented in `4e6cc2d3a`: `LoweredModule` retains only `source`,
`module` and `source_map`; `Module.nodes` holds `HirNodes`. Common import scopes and actual
declarations own static facts, not registration attachment maps. Native implementations are matched
at linking, outside HIR. The [boundary contracts](native-model-design.md#9-frontend-and-linking-boundary-remediation)
describe these owners and preserved validations, not pending migration. Broader provider/prebuild
products stay deferred.

## Node-owned origins and record-based IDs

**Status: queued; design selected, implementation not started.** Keep owned category vectors
and ID links. Move ranges/origins onto the records they describe; allocate IDs from actual record
slots. This is an ownership cleanup, not a demonstrated correctness bug or performance win.
The [roadmap](implementation-roadmap.md#hir-storage-and-attribute-refactor) owns scheduling.

### Target structures

Illustrative fields below omit unrelated existing fields. All six node payload types
(`ExprData`, `StmtData`, `BlockData`, `PlaceData`, `PatternData`, `TypeData`) gain `origin`.
Declarations, parameters and inline members likewise own their origins.

```text
// hir/origin.rs; relocate the existing attribution contract.
NodeOrigin = Source(Span) | Structured { declaration: DefinitionId }
NodeOrigin::source_span() -> Option<Span>

LoweredModule {
    source: Arc<SourceFile>,
    module: Module,
    input_kind: ModuleInputKind,  // Source | Structured; replaces the map's input flag
}

Module {
    functions: Vec<Function>, consts: Vec<ConstItem>, ...,
    generic_params: Vec<GenericParam>,
    nodes: HirNodes,
}

HirNodes {
    arena: LoweringId,
    exprs: Vec<(HirOwner, ExprData)>, ...,
    locals: Vec<(HirOwner, Local)>,
}

ExprData { origin: NodeOrigin, kind: ExprKind }
ConstItem {
    id: ConstId, origin: NodeOrigin, name: String, name_span: Option<Span>,
    owner: Option<ConstOwner>, visibility: Visibility,
    ty: Option<TypeRefId>, initializer: ExprId,
}
StaticItem { ..., origin: NodeOrigin, name_span: Option<Span>,
             initializer_span: Option<Span> }
Param { id: ParamId, origin: NodeOrigin, name: String, ty: TypeRefId }

Local { name: String, origin: NodeOrigin }
PatternKind::Name { local: LocalId }
ClosureParam { local: LocalId, ty: Option<TypeRefId> }

GenericParam { name: String, bounds: Vec<TraitRef>, origin: NodeOrigin }
// Declaration/member generic lists become Vec<GenericParamId>.
TraitMethod { id: TraitMethodId, origin: NodeOrigin, ..., function: FunctionId }
```

For `pub const LIMIT: i32 = 10;`, lookup becomes:

```text
item = lowered.module.constant(id)
item.origin.source_span()  -> full declaration range
item.name_span             -> LIMIT token range
```

There is no second map lookup. `ConstId` still identifies the same constant; semantic tables
may continue using it as a key. `source` retains the matching text/identity/revision, not spans.
`source_surface()` still rejects structured inputs; keep their logical registration key unchanged.

### Allocation and identity

| Category | Authoritative storage and allocation |
| --- | --- |
| Ordinary declarations | Matching `Module` vector length; choose the slot before children when an owner ID is needed. |
| Functions, including nested functions | Existing `Lowerer.functions: Vec<Option<Function>>`; reserve from its length, fill once, reject unfinished slots. Never allocate from the unfinalized `Module.functions`. |
| Blocks/statements/expressions/places/patterns/types | Matching `HirNodes` vector length; append owner and origin-bearing payload together. |
| Parameters | Existing function-local slot and qualified ID; origin belongs to `Param`. |
| Fields/variants | Existing enclosing-declaration/member-slot IDs; origin belongs to the inline record. |
| Locals | New real `HirNodes.locals` records; keep the existing lowering/owner-qualified ID shape. |
| Generic binders | New real `Module.generic_params` records; retain plain module-local `GenericParamId`. |
| Trait methods | `TraitMethodId { lowering, owner: TraitId, slot }`; checked lookup through `Module::trait_method`. |

The two new vectors store actual names/bounds/origins, not placeholders for counting. Names move
out of pattern leaves/closure parameters into `Local`; generic records are not copied into
multiple declaration lists. Semantic resolver bindings remain derived facts, not HIR allocators.
No universal node table, redundant per-category counter or stable-across-edit ID is introduced.
Keep `LoweringId`, existing owner/bounds checks and matching-module requirements for plain IDs.
All slot allocations must check integer capacity before narrowing.

`Lowerer` owns the active `HirOwner` and structured declaration attribution; nested lowering
saves/restores both. `HirNodes` owns the lowering identity. Construct origins during allocation,
not by a later span-based join. Parameters and child/member lists retain their present ordering.

**Local rules:** preserve every current binder occurrence, including recovery, shorthand and
provisional pattern names. A name resolved as a variant does not become a lexical binding.
Shadowing creates distinct IDs; captures reuse the original ID; only the first `Or` alternative
supplies canonical bindings. Closure parameters stay locals. A local table avoids reserving a
closure expression solely to identify parameters lowered before that expression.

**Generic rules:** allocate each authored binder once. Inherited method lists reuse IDs:

```text
trait Source<T> { fn map<U>(self, value: T, other: U) -> T; }
Source.generic_params = [gT]
map.generic_params    = [gT, gU]
Module.generic_params = [T_record, U_record]
```

`gT` retains semantic owner `Source`, position 0; `gU` has owner `map`, own position 0,
not effective-list position 1. `Declarations` retains original-owner deduplication and semantic
binder ownership. Preserve reverse name lookup/shadowing. Method `where` constraints must not
mutate an inherited shared binder's bounds. Associated-type binders follow the same rule.

### Range and origin ownership

- Full declaration/member ranges belong to their records, including associated constants/types
  and authored `UseDecl`. Grouped imports retain one authored use target despite flattened leaves.
- Exact name ranges belong to named declaration records. Optional ranges represent unavailable
  source, not invented token locations; preserve malformed/recovery ranges as currently authored.
- `StaticItem.initializer_span` retains the full AST initializer, including erased parentheses.
  Do not reconstruct it from the lowered expression or widen it to the declaration.
- Typed attribute attachment stays on `Module`. Attribute syntax keeps its own ranges; target
  lookup follows the typed target to the actual record, not a duplicate target-span map.
- `AssociatedConst.name_ref` / `AssociatedType.name_ref` stay until their association/diagnostic
  consumers are migrated on evidence; moving spans alone does not make them redundant.
- Structured origins retain scoped `DefinitionId` attribution and yield no source span. Preserve
  existing synthetic/recovery anchor behavior; an empty anchor is not authored source evidence.
- Move `lower/identity.rs` structured-definition validation to traversal of record-owned origins.
  Relocation must not bypass foreign-definition validation or checked compilation admission.

### Consumer changes and reuse

| Owner | Required adaptation |
| --- | --- |
| `lower/context.rs`, `lower/item.rs`, structured lowering | Record-based allocation, inline origins, owner restoration and reserved-function finalization. |
| `hir/nodes.rs`, `hir/item/*`, `hir/ids.rs` | Local/generic record lookup, trait-member lookup, complete-ID iteration and validity docs. |
| `declarations`, resolver, type checking | Dereference local/generic records; preserve binding publication, bounds and semantic owners. |
| `typeck/aliases.rs`, `typeck/signature_reuse.rs`, `TypeTable::restore_function` | Iterate real records/IDs instead of reconstructing IDs from source-map slots. |
| Analyzer and lint | Read actual declaration/member/node origins; keep editor indexes and lint projections at their existing owners. |
| `compile_input`, compiler source lowering, `source_inspection`, `update_source` | Preserve debug/diagnostic ranges, checked source provenance and exact static-initializer evidence. |

Retain current reuse eligibility: matching body/signature and dependency/environment evidence is
required before remapping. Build temporary old-to-new ID correspondences from actual records,
matched owners and ordered relative ranges. Preserve repeated/empty ranges as separate occurrences;
never use a single `Span -> ID` map. Unmatched correspondence falls back to checking, not partial
fact publication. Do not broaden source-less reuse merely because inline origins are available.

Audit ID-bearing values as well as keys: `ResolvedTypeTarget::Generic`, captures, selected-call
receivers and Host path roots/dynamic arguments must refer to the destination lowering. Derive
generic correspondence from matched binder lists and require consistency when inherited IDs recur.
Definition-table remapping is not a substitute for lowering-local ID remapping.

### Execution checkpoints

Each checkpoint must build affected consumers and preserve working behavior. Move a category's
origin reads with its storage change; do not introduce duplicate authoritative origins or lasting
forwarding APIs. During the migration `SourceMap` may retain only categories not yet moved.

| Phase | Change and completion boundary |
| --- | --- |
| SO00 — Record-backed identities | Add real local/generic records, migrate name/binder consumers and inherited generic lists; make trait methods owner-relative. SourceMap temporarily retains their range storage, keyed by these record-issued IDs, and no longer allocates these IDs. Verify nested function reservation and all three identity routes. |
| SO01 — Node-owned origins | Relocate `NodeOrigin` to `hir/origin.rs`; add origins to node/local records; allocate ordinary node IDs from their payload vectors. Migrate diagnostics, iteration, body/signature remapping and compiler/analyzer reads for these categories; remove their source-map vectors/owner copies. |
| SO02 — Declaration-owned origins | Move remaining declaration/member/name/initializer ranges and generic origins; migrate both input routes, attribute target lookup, structured identity validation and source provenance consumers. Move construction context into Lowerer, preserve input-kind/source-surface behavior, then delete SourceMap, its LoweredModule field and obsolete OriginTarget dispatch. |
| SO03 — Integration and documentation | Complete selected downstream regressions and required quality gates; update architecture/rustdoc examples. Remove this execution detail after lasting contracts reach architecture owners; keep unresolved work in the roadmap. |

No compatibility exports, old/new access paths, version bumps or artifact regeneration solely
for unpublished API changes. If a checkpoint cannot build through the intended boundary, repair
its affected consumers before marking it complete; do not disable validation or carry fake stubs.

### Acceptance and validation

Strengthen existing owner scenarios before adding fixtures. New complete Kagari programs belong
in the owning crate's fixtures, not a duplicate migration test suite.

| Contract | Focused evidence |
| --- | --- |
| Const/node lookup | Assert exact full/name/node ranges through actual records, including recovery; allocation no longer depends on span-table length. |
| Local/member identities | Existing lowering-identity tests reject foreign/wrong-owner lookup; two traits' method slot zero resolves to different records/functions. |
| Generic identity | Existing inherited-generic navigation tests assert original semantic owner/own slot, method shadowing and bounds; inherited lists dereference one shared record. |
| Reuse | Existing body neighbor-edit and signature reuse scenarios compare refreshed local/generic targets, types and ranges with fresh analysis; mismatches recheck safely. |
| Native source-free route | Structured and parsed inputs retain equivalent checked meaning but distinct source availability; invalid structured definition references still fail. |
| Source provenance | Existing static update/source inspection tests preserve full parenthesized initializer evidence, binding identity and checked admission; analyzer navigation and lint targeting retain exact sites. |
| Execution boundary | Sample existing compiler/source-free and actual Native tests for call selection, fields and captures; no changed runtime semantics are expected. |

At each implementation checkpoint run the small changed contract set, structure check, formatting,
workspace all-target Clippy and `git diff --check` required by repository rules. Do not run full
workspace tests locally; exhaustive feature/backend/platform tests remain CI-only. Record exact
selected commands/results, new blockers and unrun scope here while implementation is active.
For this queued documentation change only, validate links/content/diff; no build/test success is claimed.

Primary reference: rustc's [HIR nodes and owner indexes](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_hir/src/hir.rs)
and [lowering ID allocation](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_ast_lowering/src/lib.rs),
reviewed at commit `5f4effd579500175af844241eb179e756190f1e7`. Borrow separation of node identity
from source location, not rustc's heterogeneous owner-local numbering or arena lifetimes.
Kagari's owned typed vectors supply their own slots; arena/Arc migration, cache policy, incremental
parsing, per-owner physical allocation, Native registration and Runtime installation are out of scope.

## Native model

Phases 1–3 remove the registration-dependent frontend split, dedicated default recipes and mixed
HIR type/storage classification. Implementation is not all-green acceptance: the
[roadmap](implementation-roadmap.md#native-model-replacement) retains known baseline failures and
local/CI limits. The [boundary contracts](native-model-design.md#9-frontend-and-linking-boundary-remediation)
record implemented ownership alongside explicitly deferred whole-stack discussion, not a second queue.

- Ordinary HIR declares Required/Script/Native functions, fields, methods/associated functions
  and trait impls. Rust implementation code stays outside HIR; no general trait receiver loans.
- Compilation consumes structured Rust declarations directly; analyzer parses exported .kgr into
  the same unchecked input and uses common analysis with user source and exact navigation origins.
- Native field names/types, var/val and ordinary child-object handles are frontend requirements.
  New Rust field adapters/execution are deferred after scope narrowing, not implied by type checks.
- Ref/RefMut scope is recognition of target T and shared/mutable category only. Concrete access
  and runtime design remains undecided: ordinary wrappers, generated methods, storable handles and
  dynamic expiry are proposals, not current acceptance. Existing reference semantics and safety
  checks remain; no new borrowing, projection, adaptation, forwarding or execution is implied.
- Ordinary Script definitions/calls replace dedicated NativeDefault recipes; language constructor
  interpretation is separate from Native declarations and execution storage. Binding checks belong
  at linking, not HIR. Existing defaults, Host/reference safety and executable validation remain.
  No new prebuild producer or generalized registration framework was required.
- The design retains future library/provider/build/cache and directory-refresh discussion as
  explicitly deferred direction, not current acceptance.

[Replacement boundaries](native-model-design.md#7-replacement-boundaries-and-acceptance) and
[review limits](native-model-design.md#8-review-decisions-and-limits) preserve existing safety
while preventing a HIR cleanup from silently becoming a whole Runtime migration.
