# HIR Model Review Notes

**Status:** HR04–HR08 compiler/tool separation is implemented on owned, ID-linked HIR.
Required acceptance remains open in the
[roadmap](implementation-roadmap.md#hir-storage-and-attribute-refactor).
The arena/reference migration is withdrawn, not completed. Native frontend phases 1–3 are
implemented. SO00–SO03 integrates origins and record-based IDs into the owned HIR model. The
roadmap retains local acceptance limits and separate follow-ups.
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

`LoweringId`, `ResolvedTypeTarget` and internal `PreparedSignatures` name current roles;
`VisibleBinding`/`CompletionCandidate` name analyzer products. `FileAnalysis`/`AnalyzedModule`,
`BodySignatures` and `update_source` stay unchanged. Broader naming, incremental parsing,
function-granularity reuse/reclamation and LSP scheduling require separate scope.

## Analysis flow and naming review

**Status: discussion recorded; revisit after the source walkthrough.** Final names, module moves and implementation phases remain open.

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

### Explicit source extents

**Status: deferred.** [SourceFile](../crates/kagari-source/src/source.rs) stores `origin: FileId`
next to `inline_range: Option<Span>`. For a whole source, the origin is the file's own ID;
only an inline module needs a separate origin and extent. Consider keeping `SourceFile` as
one struct and replacing those two fields with a source-owned extent:

```rust
enum SourceExtent {
    Whole,
    Inline { origin: FileId, range: Span },
}
```

Keep the logical ID, revision, module identity and shared text/line index on `SourceFile`.
`origin_id()` derives the physical origin from the extent. In
[source preparation](../crates/kagari-hir/src/analysis/source_preparation.rs), `Whole` selects
the root AST and `Inline` selects an exact module-body range. An independently supplied
inline view still needs that range validated and must report `InvalidSourceRange` on failure.
Children discovered while walking the parent AST already have their syntax node and should
not look it up again. This is a representation change, not a change to `SourceSnapshot`,
synthetic `FileId` allocation or parse reuse.

## LoweredModule boundary remediation

[Native input implementation](architecture/hir.md#native-declaration-inputs-and-origins) supports
direct lowering and parsed inputs with common binding-free HIR and checking regardless of
LSP/execution intent. A–F were implemented in `4e6cc2d3a`. `LoweredModule` owns the source and module, and
`Module.nodes` holds `HirNodes`. SO02 moves the remaining origins onto the records. Common import scopes and actual
declarations own static facts, not registration attachment maps. Native implementations are matched
at linking, outside HIR. The [boundary contracts](native-model-design.md#9-frontend-and-linking-boundary-remediation)
describe these owners and preserved validations, not pending migration. Broader provider/prebuild
products stay deferred.

## Deferred reverse lookup and remapping scans

HIR IDs select records and their origins directly. A general span-to-node query would return
candidates, since distinct records can cover the same range. Existing editor indexes are
derived by the analyzer from the matching source and semantic facts.

Body reuse knows the target function. Each function that reaches remapping still scans five
module-wide node collections in each lowering (expressions, locals, places, patterns and type
references). It filters on the matching owner and function extent, then pairs entries in slot
order using relative ranges. Signature reuse scans the old and new type-reference collections
once per attempt and pairs ordered IDs by owner and projected range. Both preserve repeated
occurrences. See [body remapping](../crates/kagari-hir/src/typeck/table.rs) and
[signature remapping](../crates/kagari-hir/src/typeck/signature_reuse.rs).

General reverse lookup and scan reduction remain deferred after SO00–SO03. A later body-reuse
review can evaluate enumeration from the known function to its ordered node IDs. Evaluate the
cost on a representative edit workload and preserve owner, relative-range and repeated-occurrence
checks.

## HIR visibility follow-up

Origin validation follows the IDs carried by each record, not the owning module's input
kind. This is scoped identity validation; it does not define a new mixed-origin policy.
`typeck::reuse` and `BodyReuse` are crate-visible; `environment_matches` and `restore`
are restricted to `typeck`. The remaining visibility candidates are separate from the origin fix:

- **Internal helpers:** `CheckedAnalysis::to_unverified` has only HIR admission test callers; keep `FileAnalysis::to_unverified` separate because analyzer/compiler
  tests use it. `ModuleSignatures::type_bounds` and `statics` have only HIR callers.
- **ID construction:** make plain local HIR ID constructors crate-visible after moving
  the analyzer test from `StructId::new(0)` to the ID of the actual lowered declaration.
  Keep public ID types and accessors. Plain IDs still require the matching module.
- **Coupled storage:** keep `LoweredModule` source, input kind and module together. Give
  consumers shared access to `Module` declaration slices and node lookup, not writable
  vectors or independent replacement of `HirNodes`. Keep record origins/name ranges,
  IDs, binder lists and body/initializer roots writable only on the construction side.
  Check `DeclarationHeaders` and its binder access when migrating these readers.
- **Published analysis:** `AnalyzedModule.signatures` already has external read routes
  through file-stage APIs. Other facts have actual external readers; preserve those
  reads through the owning views before restricting fields. Keep checked admission
  and owner-scoped body access intact. Public read-only enums and ID accessors do not
  require a replacement model.

Confirm the changes against compiler, analyzer, lint, SDK examples and benchmarks;
workspace-only caller evidence does not establish downstream compatibility. Do not add
mutable accessors or another HIR model just to preserve field access.

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
- 9G removes script reference wrappers. Neither same-named user types nor ordinary
  Native declarations gain borrowing, projection or forwarding. Independent Host
  path and Opaque loan contracts remain.
- Ordinary Script definitions/calls replace dedicated NativeDefault recipes; language constructor
  interpretation is separate from Native declarations and execution storage. Binding checks belong
  at linking, not HIR. Existing defaults, Host safety and executable validation remain.
  No new prebuild producer or generalized registration framework was required.
- The design retains future library/provider/build/cache and directory-refresh discussion as
  explicitly deferred direction, not current acceptance.

[Replacement boundaries](native-model-design.md#7-replacement-boundaries-and-acceptance) and
[review limits](native-model-design.md#8-review-decisions-and-limits) preserve existing safety
while preventing a HIR cleanup from silently becoming a whole Runtime migration.
