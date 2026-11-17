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

Audit generic parameters, local bindings and associated members only for concrete cases where
an ID addresses a source-map slot instead of its authoritative record. The parameter repair
does not authorize a uniform ID representation, physical per-owner storage or universal graph.
Record a demonstrated mismatch and its consumers before activating another slice.

`LoweringId`, `ResolvedTypeTarget` and internal `PreparedSignatures` name current roles;
`VisibleBinding`/`CompletionCandidate` name analyzer products. `FileAnalysis`/`AnalyzedModule`,
`BodySignatures` and `update_source` stay unchanged. Broader naming, incremental parsing,
function-granularity reuse/reclamation and LSP scheduling require separate scope.

## LoweredModule boundary remediation

[Native input implementation](architecture/hir.md#native-declaration-inputs-and-origins) supports
direct lowering and parsed inputs with common binding-free HIR and checking regardless of
LSP/execution intent. A–F were implemented in `4e6cc2d3a`: `LoweredModule` retains only `source`,
`module` and `source_map`; `Module.nodes` holds `HirNodes`. Common import scopes and actual
declarations own static facts, not registration attachment maps. Native implementations are matched
at linking, outside HIR. The [boundary contracts](native-model-design.md#9-frontend-and-linking-boundary-remediation)
describe these owners and preserved validations, not pending migration. Broader provider/prebuild
products stay deferred.

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
