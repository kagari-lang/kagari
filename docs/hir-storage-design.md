# HIR Responsibility and Identity Cleanup

**Status:** scope narrowed by user decision. Keep the current owned, ID-linked HIR and
category tables. The external-arena/direct-reference HA00/HA01 migration is withdrawn,
not completed. Its outstanding lifetime/session proofs are no longer acceptance requirements.
[Roadmap](implementation-roadmap.md#hir-storage-and-attribute-refactor) owns activation.

## Model and responsibilities

```text
LoweredModule
├─ source                         exact source/provenance
├─ module
│  ├─ items                       top-level declaration links in source order
│  ├─ functions / consts / ...     owning per-kind declaration Vecs
│  ├─ Function.params             owning parameter records in call order
│  └─ Body                        owning per-kind node Vecs, with owner tags
├─ source_map                     node/declaration IDs → source sites
├─ attributes                     exclusive payload forms; attachment review below
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

Current ownership is split:

```text
SourceMap.push_param(span) → module-wide ParamId.index
Function.params           → actual Param records in per-function call order
```

Target: the ID addresses the record's real storage. Keep the existing qualified ParamId;
its index becomes the zero-based position in its owning Function.params.

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
// Target sketch; no new node ownership model.
struct Function {
    id: FunctionId,
    params: Vec<Param>,
    // Other existing fields.
}

struct Param {
    id: ParamId,
    name: String,
    ty: TypeRefId,
    // Existing writeability.
}

// Parameter part of SourceMap; other categories retain their current layout.
// param_spans: HashMap<ParamId, Span>
```

- Lowering chooses the function/list slot, creates one Param, and records its span under
  that ID. Replace push_param with explicit registration; no independent parameter-ID
  counter or compatibility allocator remains in SourceMap.
- Both free-function and method lowering follow this rule, including self and bodyless
  requirements. The position is in the retained HIR list, not an AST token/recovery slot.
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

## Other bounded follow-ups

- **Attributes:** exclusive bare/value/arguments/malformed forms are implemented. Preserve
  missing values, empty lists, authored spellings and recovery. Attachment still uses
  target_span; design a typed declaration/member target using existing identities before
  changing it. A universal node table is not a prerequisite. Grouped imports need one
  authored target despite flattened leaves; no new syntax/attribute targets are authorized.
- **Inline identities:** audit generic parameters, local bindings and associated members
  for the same record-versus-source-slot confusion. Record concrete cases here before
  expanding implementation; they are not automatically included in the parameter slice.
- **Naming:** replace misleading Buffer aliases when their owning declarations are touched;
  do not run a standalone renaming migration or add wrappers without invariants.
- **Caches:** owned syntax/semantic records, weak graph/query indexes and split Native
  preparation are implemented. Leave their behavior unchanged in this cleanup. The old
  proposed session cutover is cancelled. Further retention-policy work needs a concrete
  warm-query/retained-result requirement, not an arena prerequisite. With no live graph,
  current caches force fresh lowering/checking; no performance improvement is claimed.

Native registration/Provider redesign, new tool crates/features, incremental parsing,
function-granularity reuse/reclamation and LSP scheduling remain deferred in the
[review notes](hir-model-review.md). This cleanup makes no new sharing/reclamation promise.

## Checkpoints and validation

These replace the withdrawn arena phases; earlier implementation commits remain in Git.

| Checkpoint | Scope and acceptance |
| --- | --- |
| HR00: responsibilities | Record current owners, the parameter defect and finite scope; documentation only. |
| HR01: parameters | Function-local ParamId positions, explicit source registration and checked structural lookup; update semantic/compiler/Native consumers and remapping together. |
| HR02: scoped follow-ups | After HR01, review concrete inline-identity/attribute-target findings; implement only agreed cohesive slices. This is not permission for a graph rewrite. |

HR01 should strengthen existing lowering, arena/owner-isolation, signature/body reuse and
checked-input scenarios. Assert actual parameter records, types, owner/slot rejection and
source ranges; preserve method/self, Native declaration and independent fresh-analysis routes.
Cover edit remapping and inline physical UTF-8/CRLF provenance without exhaustive matrices.
Sample compiler handoff and SDK analysis/replay. No bump allocation/lifetime tests are needed.

Run focused affected tests, structure, fmt, workspace all-target Clippy and diff checks at
implementation checkpoints. Full workspace tests and exhaustive backend/platform matrices
remain CI-only. HR00 requires only content/link/diff checks; this plan revision changes no
runtime behavior and establishes no new test-pass claim.

Known carried checks on rustc 1.100.0: workspace fmt has existing differences in
hir/ids.rs and kagari-syntax/src/ast/tokens.rs; workspace Clippy reports
needless_range_loop at kagari-compiler/src/bytecode/placement.rs:110. The prior transient
Windows derive-DLL linker failure cleared; three active SDK regressions passed afterward.
Do not silently fix unrelated findings or claim workspace acceptance.

## Reference boundary

[rustc HIR at 5f4effd5795](https://github.com/rust-lang/rust/blob/5f4effd5795/compiler/rustc_hir/src/hir.rs)
and [HirId](https://github.com/rust-lang/rust/blob/5f4effd5795/compiler/rustc_hir_id/src/lib.rs)
separate owner/local identity and attribute metadata. Borrow those responsibility boundaries,
not rustc's arena/reference allocation or complete query model. Kagari retains typed category
storage and current owned result APIs; this is a local correctness/readability cleanup.
