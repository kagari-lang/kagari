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

## LoweredModule boundary remediation

[Native input implementation](architecture/hir.md#native-declaration-inputs-and-origins) supports
direct lowering and parsed inputs, but registration/binding authority and misplaced static side maps
still remain in LoweredModule and its consumers. The reviewed target is identical binding-free HIR
and checking regardless of LSP/execution intent; Native implementations are matched at linking.
The [single remediation inventory](native-model-design.md#9-frontend-and-linking-boundary-remediation)
owns removal, semantic-owner migration and preserved validations. A live consumer is migration work,
not justification for keeping the old ownership model. Broader provider/prebuild products stay deferred.

## Native model

The NM01–NM04 commits exist, but architecture acceptance is reopened. Their implementation and
prior focused validation do not resolve the registration-dependent frontend split. The
[roadmap](implementation-roadmap.md#native-model-replacement) tracks outstanding remediation;
[the design inventory](native-model-design.md#9-frontend-and-linking-boundary-remediation) is its
only corrective-work list, alongside separately deferred whole-stack discussion.

- Ordinary HIR declares Required/Script/Native functions, fields, methods/associated functions
  and trait impls. Rust implementation code stays outside HIR; no general trait receiver loans.
- Compilation consumes structured Rust declarations directly; analyzer parses exported .kgr into
  the same unchecked input and uses common analysis with user source and exact navigation origins.
- Native field names/types, var/val and ordinary child-object handles are frontend requirements.
  New Rust field adapters/execution are deferred after scope narrowing, not implied by type checks.
- Ref/RefMut recognition and target/access type records are included, but no new borrowing,
  projection, adaptation, forwarding or execution features. Reuse builtin reference semantics.
- NativeDefault recipes must become ordinary definitions/calls; NativeTypeKind must cease mixing
  builtin interpretation, declarations and execution representation. Preserve existing defaults,
  Host/reference safety and executable validation, but remove binding checks and privilege from HIR.
  This does not require a new prebuild producer or generalized registration framework.
- The design retains future library/provider/build/cache and directory-refresh discussion as
  explicitly deferred direction, not current acceptance.

[Replacement boundaries](native-model-design.md#7-replacement-boundaries-and-acceptance) and
[review limits](native-model-design.md#8-review-decisions-and-limits) preserve existing safety
while preventing a HIR cleanup from silently becoming a whole Runtime migration.
