# HIR Responsibility and Identity Cleanup

**Status:** scope narrowed by user decision. Keep the current owned, ID-linked HIR and
category tables. The external-arena/direct-reference HA00/HA01 migration is withdrawn,
not completed. Its outstanding lifetime/session proofs are no longer acceptance requirements.
HR00 is recorded; HR01 parameters, HR02 attribute attachment and HR03 plain-Vec alias cleanup
are implemented with focused validation, but required workspace checks remain blocked below. [Roadmap](implementation-roadmap.md#hir-storage-and-attribute-refactor)
owns activation.

## Model and responsibilities

```text
LoweredModule
├─ source                         exact source/provenance
├─ module
│  ├─ items                       top-level declaration links in source order
│  ├─ attributes                  payloads with typed declaration/member targets
│  ├─ uses                        authored use identities and flattened leaf ranges
│  ├─ functions / consts / ...     owning per-kind declaration Vecs
│  ├─ Function.params             owning parameter records in call order
│  └─ Body                        owning per-kind node Vecs, with owner tags
├─ source_map                     node/declaration IDs → source sites

└─ existing Native metadata       unchanged registration semantics

resolution / declarations / type checking
└─ separate semantic identities, bindings, types and checked facts
```

| Responsibility | Owner |
| --- | --- |
| Declaration data and category traversal | Module's existing per-kind collections |
| A function's ordered parameter records | Function.params |
| Expressions, blocks, statements, patterns, places and type syntax | Body's existing per-kind collections |
| Structural identity and ownership | HIR storage/lowering; IDs name records, not source-map allocation slots |
| Diagnostic/navigation locations | SourceMap, interpreted against the matching source |
| Resolved names, semantic types and checked calls | Existing semantic analysis records |
| Shared result retention | Existing analysis/SDK owners; not part of node layout cleanup |

One authoritative record per entity. Existing inline lists and typed accessors remain useful;
no requirement to give every node the same physical layout or index representation.
HirOwner already tags nodes: do not introduce OwnerInfo/Vec<Node>, physical per-owner storage,
or a universal HirId merely to make the model resemble rustc.

HirArenaId currently identifies one lowering; it is not a bump allocator. Keep generation/
foreign-owner checks. No SnapshotId, bumpalo, node-level Arc or borrowed-HIR lifetime migration
is included. Existing module/result-level Arc ownership is not being removed.

## First implementation slice: parameters

Before HR01, ownership was split:

```text
SourceMap.push_param(span) → module-wide ParamId.index
Function.params           → actual Param records in per-function call order
```

Implemented: the existing qualified ParamId addresses the zero-based position in its
owning Function.params. No new ID representation or storage model is introduced.

```text
ParamId { arena: lowering_identity, owner: Body(Function(f)), index: parameter_position }

Module.parameter(p)
  → validate lowering identity and function owner
  → functions[f.index()].params[p.index()]
  → validate the stored Param identity

SourceMap.param_span(p)
  → validate lowering identity and registered parameter identity
  → recorded source span
```

```rust
// Abbreviated current layout; no new node ownership model.
struct Function {
    id: FunctionId,
    params: Vec<Param>,
    // Other existing fields.
}

struct Param {
    id: ParamId,
    name: String,
    ty: TypeRefId,
}

// Parameter part of SourceMap; other categories retain their current layout.
// param_spans: HashMap<ParamId, Span>
```

- Lowering chooses the function/list slot, creates one Param, and records its span under
  that ID. insert_param replaces push_param; no independent parameter-ID counter or
  compatibility allocator remains in SourceMap.
- Both free-function and method lowering follow this rule, including self and bodyless
  requirements. The position is in the retained HIR list, not an AST token/recovery slot.
- Param carries identity/name/type syntax only, not a configurable writeability field.
  Parameters cannot be rebound: resolver binding creation and checked source signatures
  explicitly use Val. Generic binding, field and Host access policies are unchanged.
- Closure parameters still use LocalId. Do not migrate local bindings/generic binders or
  other SourceMap allocators as part of this parameter slice.
- Other IDs such as ExprId still address module-wide category vectors. Consistency means
  each ID addresses its authoritative record, not that all indices become function-local.
- Preserve type-syntax ownership, once-only lowering and source ranges. Foreign/stale IDs
  must remain rejected even when two functions both have parameter index zero.

Example, with illustrative identities:

```text
fn left(x: i32) {}
fn right(y: i32) {}

left.params[0].id  = { lowering: A, owner: Function(left),  index: 0 }
right.params[0].id = { lowering: A, owner: Function(right), index: 0 }

Different owners distinguish the two parameters; SourceMap does not assign their positions.
```

## Attribute attachment

Module owns the source-ordered attribute list. Types live in hir::attribute, not lower;
there is no compatibility reexport or retained target_span field.

```rust
struct AttributeFact {
    name: String,
    form: AttributeForm,
    span: Span,
    target: AttributeTarget,
}

enum AttributeTarget {
    Item(Item), // Methods/local functions use the canonical FunctionId.
    Use(UseId),
    Field(FieldId),
    Variant(VariantId),
    AssociatedConst { owner: AssociatedOwner, index: usize },
    AssociatedType { owner: AssociatedOwner, index: usize },
}

enum AssociatedOwner {
    Trait(TraitId),
    Impl(ImplId),
}
```

Targets address records in the matching module: associated indices select the owner's
member-category list, including requirements without definitions. One UseDecl owns the
contiguous range of its flattened import leaves; grouped/recovered uses keep one identity,
even with zero leaves. Initializer ConstItem rows do not duplicate associated attachments.

Lowering attaches while constructing each known target; no independent AST attribute walk
or span-to-declaration search remains. SourceMap.attribute_target_span reads existing item
ranges or additional full member/use ranges. Attribute spans order facts and locate diagnostics;
lexical policy containment still uses ranges, while child inheritance uses ModuleIdentity.
Language roles require a typed trait target; aliases and lint also match typed targets.
Payload recovery/unknown attributes/Native admission stay unchanged. Fields/variants remain
unsupported lint policy targets, and no new parser syntax or attribute targets are enabled.

## Other bounded follow-ups
- **Inline identities:** audit generic parameters, local bindings and associated members
  for the same record-versus-source-slot confusion. Record concrete cases here before
  expanding implementation; they are not automatically included in the parameter slice.
- **Naming:** the user authorized uniform removal of the 21 plain Vec Buffer aliases in
  kagari-hir. Fields now use Vec<T> directly; no compatibility aliases/new wrappers remain.
  SmallVec aliases retain explicit inline-capacity policy. MIR/bytecode containers and
  non-Vec diagnostic ownership aliases are outside this cleanup; storage is unchanged.
- **Compiler/tool boundary:** the [consumer-backed review](hir-model-review.md#compiler-and-tooling-boundary)
  classifies shared semantic facts, editor-derived indexes, lint projections and naming.
  The HR04–HR08 implementation plan below is authorized; commit each buildable phase.
- **Caches:** owned syntax/semantic records, weak graph/query indexes and split Native
  preparation are implemented. Leave their behavior unchanged in this cleanup. The old
  proposed session cutover is cancelled. Further retention-policy work needs a concrete
  warm-query/retained-result requirement, not an arena prerequisite. With no live graph,
  current caches force fresh lowering/checking; no performance improvement is claimed.

Native registration/Provider redesign, new tool crates/features, incremental parsing,
function-granularity reuse/reclamation and LSP scheduling remain deferred in the
[review notes](hir-model-review.md). This cleanup makes no new sharing/reclamation promise.

## Compiler/tool implementation plan

**HR04–HR08 authorized for phased implementation and commits.** Continue in this plan rather
than opening a parallel queue. The goal is no editor-only index preparation on compilation
routes, not merely moving query methods. Existing owned HIR, staged semantic results,
checking/admission and SDK compiler APIs remain the foundation.

```text
HIR: semantic result + diagnostic/debug origins + ID-based semantic reads
 ├─ compiler: checked dependency closure → MIR/bytecode
 ├─ lint: usage and attribute policy projections
 └─ analyzer library: cursor queries and demand-built IDE indexes
                       └─ existing analyzer binary: protocol/document lifecycle
```

Proposed editor API shape (names illustrative, not implemented):

```rust
struct FileQueries<'a> {
    analysis: &'a FileAnalysis,
}

struct SnapshotQueries<'a> {
    analysis: &'a AnalysisSnapshot,
    navigation: Option<NavigationIndex>,
}
```

Views borrow a fixed semantic owner; node IDs/indexes never outlive or switch that owner.
Use equivalent narrow declaration/signature/function views for existing staged queries;
do not make every IDE request run full snapshot analysis. Retained requests can own the
ordinary analysis result and lend it to these views. This is not a borrowed-HIR/arena API
or a new persistent editor cache.

| Phase | Concrete changes | Buildable acceptance |
| --- | --- | --- |
| HR04: lint projections | Move queries/usage and queries/attributes to kagari-lint; update rules/policy and tests. Add only semantic read access actually needed by these consumers. | CLI lint/check reuse the same facts. Unused/capture/static eligibility and typed attribute inheritance/recovery remain; no HIR→lint/analyzer dependency. Delete the old HIR projection APIs, not forward them. |
| HR05: IDE API cutover | Add a real library target in existing kagari-analyzer. Move offset queries across declaration/file/signature/function/snapshot results, namespaces, calls, docs and completion presentation, plus type_hits construction. Factor shared member eligibility into a narrow HIR semantic API consumed by both checking and completion. Update SDK/stdlib examples/tests and mixed compiler tests in the same checkpoint. | Real library query tests/consumers work; binary protocol capabilities stay unchanged. HIR publishes results without type_hits; no HIR/SDK/compiler production dependency on analyzer. Keep scope/site/prefix metadata only as the explicitly temporary input for HR06/HR07 below. No empty facade, old inherent-query forwarders or duplicate index computation. |
| HR06: bindings and scopes | Give compiler/declaration/lint consumers an owner-aware canonical binding directory and enumeration, including unused binders. Separate resolver work frames from published results. Move persistent cursor visibility/shadow history and declaration-site selection into analyzer ScopeIndex/DeclarationSiteIndex, derived on demand from matching syntax/HIR/resolution and origins. | Captures, named-local-function environments, rebinding policy and declaration identities are unchanged. Compiler never needs the editor scope index. Scope entries reference canonical bindings, not a second authoritative name/writeability record. Remove temporary editor-history readers/exports introduced in HR05. |
| HR07: path provenance/origins | Move resolver/import/type prefix hits, expression lookup_hits and joined-prefix expansion into analyzer navigation built after semantic settlement. Remove remaining editor-only SourceMap fields; retain actual diagnostic/debug/reuse/reload origins and ResolvedTypeRef.target. Preserve import solver observations required by semantic lookup. | Import binding/diagnostic/convergence/cancellation contracts and dual-namespace/glob/copied-source navigation remain. Compilation no longer constructs editor histories, prefix-hit collections or joined-prefix indexes. Remove HR05's remaining temporary prefix/site views; no solver weakening or second resolver in analyzer. |
| HR08: naming and final integration | Rename HirArenaId→LoweringId, TypeTarget→ResolvedTypeTarget and internal PreparedAnalysis→PreparedSignatures, preserving checks. Editor names become VisibleBinding/CompletionCandidate as they move in HR05. Keep FileAnalysis/AnalyzedModule, BodySignatures and update_source renames out unless a concrete remaining ambiguity is separately agreed. Update architecture/API examples. | Dependency audit and focused compiler/tool/source-free/reload tests pass in their designated scope; no old API aliases, editor-only preparation on compile routes or carried integration errors. Full matrices remain CI-only. |

### Dependency and intermediate-state rules

- HR04 is independent and intentionally first. HR05 is a coherent dependency cut: docs,
  namespace navigation and completion already call file/snapshot position helpers. Move
  that closure together instead of leaving HIR methods calling analyzer or duplicating them.
- HR05's library exports protocol-neutral query types; LSP conversion stays in the binary.
  SDK/stdlib test/example-only IDE uses may use dev dependencies or move to analyzer tests.
  Compiler production code keeps ID-based facts; its tests retain semantic snapshot-target
  assertions without requiring editor cursor lookup. Share receiver/substitution/bound
  eligibility primitives; do not impose the editor's incomplete-candidate policy on checking.
- HR05 must expose matching-owner lowering/origin reads for signature-stage and selected-body
  views: their current position adapters reach private preparation fields. Do not expose
  PreparedAnalysis or force full analysis as a substitute. Preserve signature-before-body and
  selected-body-only routes while relocating cursor assertions.
- HR05 may still read existing mixed scope/site/prefix data through focused borrowed views.
  This is temporary data access, not API compatibility: HR06 removes scope-history access,
  HR07 removes prefix/site access. The existing metadata has one owner until removed.
  Do not mark the separation complete at HR05 or add a second authoritative model.
- In HR06, derive visibility indexes from selected binding identities/structure, not another
  resolver. If origin data is insufficient, resolve that producer/consumer seam within the
  checkpoint; do not publish stubs or silently change shadowing/local-function semantics.
  HR04's lint usage pass must switch from scope enumeration to the canonical binding directory,
  including unused declarations, without a lint→analyzer dependency. Any remaining selected-body
  contains/routing helper must use semantic owner/structure rather than persistent editor history.
- HR07 explicitly removes the producer/read path of ResolvedNames.lookup_hits as well as prefix
  hit vectors, unless an actual semantic consumer justifies a retained fact. Derive navigation
  provenance through shared semantic lookup and verify agreement with selected resolution.
- No phase changes weak/strong graph retention, introduces an IDE host/scheduler, enables
  new LSP features, removes generation checks, redesigns Native registration, or adds arenas.
  Cache retention and request scheduling require their own concrete requirements later.

### Validation selection

Each phase must build its affected targets and commit only its cohesive change. Move existing
editor/lint assertions to the new owner, keeping semantic halves in HIR and independent
oracles. Preserve incomplete source, Type/Value ambiguity, equal-glob origins, shadowing,
copied/native locations, UTF-8/CRLF and inline routing, old results, cancellation, capture/
completion evidence, Native admission and checked compiler/reload/source-free boundaries.

Locally run affected target/name-filtered tests, structure, fmt, workspace all-target Clippy
and diff checks; record actual failures and unrun CI scope here. Check dependency direction
from Cargo metadata/normal dependency trees, including non-tool SDK/CLI compilation routes.
Use selected existing compilation workloads or structural call-path checks to confirm no
IDE-index preparation remains; do not claim a speedup without before/after measurements.
Local phase evidence is recorded below; full workspace tests remain CI-only.

## Checkpoints and validation

HR00–HR03 replace the withdrawn arena phases; earlier implementation commits remain in Git.
HR04–HR08 above are active follow-up work; phase progress is recorded below.

| Checkpoint | Scope and acceptance |
| --- | --- |
| HR00: responsibilities | Recorded current owners, the parameter defect and finite scope; docs/comments only. |
| HR01: parameters | Implemented function-local ParamId positions, explicit source registration, checked structural and typed lookup, and signature remapping. Focused checks pass; required workspace acceptance remains blocked below. |
| HR02: attributes | Implemented Module-owned facts and typed attachment during lowering; migrate role/alias/lint consumers and preserve recovery, grouped uses and inheritance. Inline-identity review remains separate, not permission for a graph rewrite. |
| HR03: container naming | Removed plain Vec Buffer aliases and their imports/use sites throughout HIR; retain actual storage policy and owner/ID invariants. No runtime/layout change or compatibility names. |

HR01 should strengthen existing lowering, arena/owner-isolation, signature/body reuse and
checked-input scenarios. Assert actual parameter records, types, owner/slot rejection and
source ranges; preserve method/self, Native declaration and independent fresh-analysis routes.
Cover edit remapping and inline physical UTF-8/CRLF provenance without exhaustive matrices.
Sample compiler handoff and SDK analysis/replay. No bump allocation/lifetime tests are needed.

Run focused affected tests, structure, fmt, workspace all-target Clippy and diff checks at
implementation checkpoints. Full workspace tests and exhaustive backend/platform matrices
remain CI-only. HR00 needed only content/link/diff checks.

HR01 local evidence: 16 focused HIR tests, two compiler handoff/generic-method tests and two
SDK analysis/replay tests pass. Existing scenarios were strengthened for same-position
parameters with distinct owners/types, self/bodyless methods, actual record lookup, stale IDs,
checked typed bindings, edit reuse and physical inline offsets. No new test cases were added.
Structure, changed-file formatting, HIR all-target Clippy and diff checks pass. Five ordinary
new fixtures pass kg fmt --check; the declaration-only fixture requires declaration parsing
and the malformed-recovery witness preserves authored bytes. Direct traversal keeps using
already-borrowed records; it does not redundantly resolve their IDs through Module.parameter.
The Param.writeability follow-up passes 11 existing focused tests for rebinding rejection,
mutable fields/indexes, lowering/method owners and body reuse. Its constant-field shape
assertion was removed; semantic rejection and mutable-object behavior remain tested.

HR02 local evidence: 49 selected tests pass (lowering/roles/Native/alias/unknown-attribute
ranges/cancellation, lint and compiler/SDK integration). Existing scenarios assert exact target
sequences and payloads, field ranges, required associated members, single default attachment,
grouped/empty uses, local functions and inline ownership. Variant syntax remains unsupported;
no new variant-parser matrix is claimed. Four ordinary fixtures pass kg fmt --check; malformed
bytes remain unchanged. Structure, changed-file formatting, HIR/lint all-target Clippy and diff
checks pass. No speedup is measured; ordinary declaration ranges are not duplicated.

HR03 local evidence: HIR all-target check and ten existing focused tests (lowering and
signature/edit reuse), structure, changed-file formatting, HIR all-target Clippy and diff
checks pass. All 21 removed aliases have no remaining HIR references; no new tests or fixtures
are justified for this mechanical type-spelling cleanup.

HR04: lint owns binding usage and typed-attribute policy projections; HIR no longer
exports them. Twenty-seven selected lint/HIR/CLI tests pass, including unused bindings,
captures, static eligibility and attribute inheritance. HIR/lint all-target check/Clippy,
structure, changed-file/fixture formatting and diff checks pass. Scope enumeration remains
a temporary semantic input until HR06 establishes canonical bindings.

HR05: protocol-neutral analyzer library now owns position queries, calls, documentation,
namespace navigation and completion presentation. Borrowing views retain declaration,
signature-only and selected-body routes; type navigation is built on demand. Shared member
eligibility remains in HIR. All 142 analyzer library tests and four stdio lifecycle tests
pass; focused HIR (22), compiler (2), SDK example and affected all-target checks pass.
The normal dependency audit has no HIR/compiler/SDK → analyzer edge. Structure, HIR/analyzer
Clippy and diff checks pass. Existing scope/site/prefix metadata is temporary until HR06/HR07;
this is not yet the completed removal of editor-only preparation.

On rustc 1.100.0, workspace fmt is blocked only by existing differences in untouched
kagari-syntax/src/ast/tokens.rs; touched hir/ids.rs is formatted. Workspace Clippy still reports
needless_range_loop at kagari-compiler/src/bytecode/placement.rs:110. SDK builds emit an unused
optional blake3 manifest warning. Do not silently fix unrelated findings or claim workspace
acceptance; no performance improvement or exhaustive matrix pass is claimed.

## Reference boundary

[rustc HIR at 5f4effd5795](https://github.com/rust-lang/rust/blob/5f4effd5795/compiler/rustc_hir/src/hir.rs)
and [HirId](https://github.com/rust-lang/rust/blob/5f4effd5795/compiler/rustc_hir_id/src/lib.rs)
separate owner/local identity and attribute metadata. Borrow those responsibility boundaries,
not rustc's arena/reference allocation or complete query model. Kagari retains typed category
storage and current owned result APIs; this is a local correctness/readability cleanup.
