# HIR Responsibility and Identity Cleanup

**Status:** scope narrowed by user decision. Keep the current owned, ID-linked HIR and
category tables. The external-arena/direct-reference HA00/HA01 migration is withdrawn,
not completed. Its outstanding lifetime/session proofs are no longer acceptance requirements.
HR00 is recorded; HR01 parameters and HR02 attribute attachment are implemented with focused
validation, but required workspace checks remain blocked as noted below. [Roadmap](implementation-roadmap.md#hir-storage-and-attribute-refactor)
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
| HR00: responsibilities | Recorded current owners, the parameter defect and finite scope; docs/comments only. |
| HR01: parameters | Implemented function-local ParamId positions, explicit source registration, checked structural and typed lookup, and signature remapping. Focused checks pass; required workspace acceptance remains blocked below. |
| HR02: attributes | Implemented Module-owned facts and typed attachment during lowering; migrate role/alias/lint consumers and preserve recovery, grouped uses and inheritance. Inline-identity review remains separate, not permission for a graph rewrite. |

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
