# HIR Model Review Notes

**Status:** HR04–HR08 compiler/tool separation is implemented on owned, ID-linked HIR.
Required acceptance remains open in the
[roadmap](implementation-roadmap.md#hir-storage-and-attribute-refactor).
The arena/reference migration is withdrawn, not completed. Native model implementation,
inline-identity review and cache-retention policy changes remain separate follow-ups.
[Current HIR](architecture/hir.md) and [Runtime](architecture/runtime.md) own implemented contracts.

## Selected storage and attribute scope

`Module`/`Body` retain ordinary owned category tables and ID links. `Function.params` owns
parameter positions; `Module` owns typed attribute attachments. `LoweringId` identifies one
lowering, not an allocator. There is no borrowed graph, universal owner table or session cutover.
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

## Deferred LoweredModule boundary

The [Native model proposal](native-model-design.md#2-entry-into-hir) owns the target entry:
source and typed declaration inputs lower into ordinary HIR with explicit origins. Generated
source is optional; no name/span/ordinal attachment join or registration-only contract bundle
belongs in the target LoweredModule. Current code still uses registration attachments.

## Native model

The overall design now lives in [Native model: declarations, HIR and implementations](native-model-design.md).
It supersedes both the earlier sketch here and the attachment-only draft; implementation remains
unstarted. Keep one proposal owner, not two competing data models.

- HIR contains ordinary declarations with Required/Script/Native implementations and type/access
  contracts; Rust callbacks remain outside it.
- Common LibraryContracts separates checked declarations from script bodies and NativeProvider
  bindings. The checked function's definition identity is its Rust binding key.
- Default methods use ordinary script calls to Native helpers; no NativeDefault recipe.
- Prebuilt fixed libraries and demanded linking address startup scale, not only attachment cost.
- [Output/reflection APIs](native-model-design.md#library-apis-versus-compiler-intrinsics) belong
  to libraries; Runtime keeps safe access primitives. Static field typing needs an explicit
  replacement contract, while scalar arithmetic/constant rules remain compiler semantics.

[Replacement and acceptance](native-model-design.md#7-replacement-boundaries-and-acceptance)
preserve checked/source-free, GC/reentry, cancellation and reload contracts. Source syntax,
structured origins, prebuilt artifacts and dynamic generic admission remain explicit
[design decisions](native-model-design.md#8-remaining-design-decisions), not implemented guarantees.
