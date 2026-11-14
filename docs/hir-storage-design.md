# HIR Storage and Attribute Design

**Status:** HA00's remaining ownership work and HA01's production graph/consumer cutover
are approved as one combined migration. Neither phase is complete. Owned syntax preparation
and selected-body fact ownership are implemented; stage-cache/session/arena integration
remains pending. HA02 retains final acceptance.
**Baseline:** `main` at `43d55706a`; working branch `design/hir-native-model`.
[Roadmap](implementation-roadmap.md#hir-storage-and-attribute-refactor) owns activation.

**Model:** the caller owns the arena; HIR and analysis results borrow it. Immutable nodes have
direct structural children; owner-local indexes provide identity, source sites and attributes.

Replace the current HIR tables/APIs directly, without compatibility aliases or dual storage.
Native declaration/Provider redesign remains in [review notes](hir-model-review.md).

## 1. External arenas and module-version reclamation

**Reclamation unit: one logical module's lowering instance**, not a whole project/database
or a never-ending stream of revisions. An unchanged lowering can be reused; a new lowering
uses a new region. Logical owners within the module share that region.

```text
External analysis storage
├─ A@lowering1: HirArena ← borrowed LoweredModule for A1
├─ A@lowering2: HirArena ← borrowed LoweredModule for A2
└─ B@lowering1: HirArena ← unchanged B, reused without rebuilding

No remaining user of A1 → its entire region becomes reclaimable, without releasing B.
```

The caller owns each arena and keeps it alive for its borrowers. Inline child modules remain
separate logical lowerings with physical source offsets preserved; shared physical parse
storage is not the same as shared HIR reclamation storage.

```text
struct HirArena {
    bump: Bump,                            // Private; allocation only through typed helpers.
}

struct LoweredModule<'hir> {
    source: Arc<SourceFile>,
    snapshot: SnapshotId,
    module: &'hir Module<'hir>,
    owners: Vec<OwnerInfo<'hir>>,
    // Existing Native registration fields remain outside the bump.
}

fn lower_module<'hir>(
    arena: &'hir HirArena,
    source: Arc<SourceFile>,
    syntax: ModuleSyntax<'_>,
    cancel: &CancellationToken,
) -> Result<Arc<LoweredModule<'hir>>, LowerError>;

impl<'hir> LoweredModule<'hir> {
    fn node(&self, id: HirId) -> Option<NodeRef<'hir>>;
}
```

These sketches describe borrowing one module, not a completed multi-module cache API.
The lifetime comes from its external arena, not the AST borrow. A direct bumpalo dependency
is proposed; no self-reference library, lifetime erasure or manual Sync override is approved.

**Allocation policy is decided.** HA00 must implement and validate the external ownership/API
boundary for cached modules, old requests and independent release. This is the remaining
implementation-design obligation, not a request for another arena-granularity decision.
Do not claim Arc owns borrowed memory or bypass lifetime safety; validate at the combined
HA00/HA01 cutover.
If the chosen constraints require a model change, present the concrete conflict for review.

### Lifetime, threads and reclamation

- HIR-bearing views must remain bounded by the corresponding external regions. Exact persistent
  cache/snapshot handle APIs are open at HA00; a single whole-database lifetime must not prevent
  independent module retirement. Arc does not extend arena lifetime. Owned AnalysisInputs,
  CompilationInputs, prepared compilation/MIR/Program and source/debug packages do not borrow
  HIR. Retaining source text alone does not add an arena lifetime.
- Request owners retain the required module regions while borrowed work runs. Scoped workers
  can read immutable graphs; background work needs an explicit owner/borrow boundary or owned
  query responses/products, never a fabricated static reference.
- `Bump` is Send, not Sync. Keep allocation/database access on the owning analysis thread;
  published results contain node references, never `&HirArena`/`&Bump`. Assert Send/Sync for
  the actual immutable result types and maps; use normal Rust auto traits, not an unsafe override.
- Allocated nodes/slice elements are audited `Copy + Send + Sync`: scalars, immutable references,
  `&str` and slices. No owned String/Vec/Arc, interior mutability or surviving `&mut` aliases.
  Mutable state lives in temporary builders; committed graph/index data is immutable.
- Dropping a result releases owning tables/resources, not bump nodes; release the external
  region only after its final borrower. Errors/cancellation/unwind publish nothing and release
  the abandoned lowering's region once builders/borrowers end. Abort cannot promise Drop.
- Never append changed module versions to one persistent arena. No cross-region structural
  pointers: new A does not borrow obsolete A's nodes. Reuse a complete unchanged lowering or
  rebuild into a new region and remap/revalidate reusable semantic facts. No automatic version
  count/memory bound is claimed; requests and caches still need finite retention policies.

HA00 owns the concrete cache/borrow API and release proof; no further product-level choice
is needed to start that checkpoint. No new incremental query algorithm is included.
Runtime/verified executable products remain owned and arena-independent.

### Persistent caches versus borrowed sessions

The lifetime probe confirms that deleting A from a mixed `Map<Key, Arc<&Node>>` does not
permit dropping A's storage while continuing to use B in that map (rustc E0505). Do not
reproduce that container as the long-lived analysis database.

```text
Persistent owned cache
├─ Parse + exact source/version/limits and logical module manifest
└─ Semantic facts + scoped IDs, diagnostics and dependency/publication evidence (pending split)

External module graphs
├─ current A2/B1 → short borrowed analysis session → result bound only to current graphs
└─ previous A1  → temporary reuse/remapping input, never retained by new result/cache
```

Unchanged B1 is supplied from its original independent owner, not recovered from a shortened
borrow in an old mixed session. Persistent cache entries must not contain whole declaration/
signature/body bundles that retain HIR. The selected-body sub-checkpoint now strongly caches
only owned facts and weakly names the graph/query context. Other stage caches still need this
split. Remapping needs the old graph supplied temporarily, or must reject reuse.

### SDK boundary

| Entry/owner | Target boundary |
| --- | --- |
| Engine's source analysis caches | Keep arena ownership external; HA00 must define cache/module handles without pinning all module regions through one permanent borrow. |
| analyze/recheck returning frontend results | Require valid external region ownership for every retained HIR view; final signatures follow HA00, not a hidden local arena. |
| compile/update returning owned products | May create finite local module regions and finish owned products before those regions are released. |

This adapts the concrete Engine caches in `engine.rs` and replay entrypoints in
`engine/source/inputs.rs`; the owned product boundary in `engine/source/compilation.rs`
remains. Do not keep an owning analysis API through lifetime erasure or a hidden self-reference.

## 2. Structural graph

```text
struct Module<'hir> {
    id: ModuleRootId,
    items: &'hir [Item<'hir>],              // Source order, including authored use declarations.
}

enum Item<'hir> {
    Function(&'hir Function<'hir>),
    Struct(&'hir Struct<'hir>),
    Enum(&'hir Enum<'hir>),
    Trait(&'hir Trait<'hir>),
    Impl(&'hir Impl<'hir>),
    Const(&'hir Const<'hir>),
    Static(&'hir Static<'hir>),
    TypeAlias(&'hir TypeAlias<'hir>),
    OpaqueType(&'hir OpaqueType<'hir>),
    Module(&'hir ModuleDecl<'hir>),         // Header; child analysis is separate.
    Use(&'hir UseDecl<'hir>),
}

struct Function<'hir> {
    id: FunctionId,
    name: &'hir str,
    params: &'hir [Param<'hir>],
    return_type: Option<&'hir TypeRef<'hir>>,
    body: Option<&'hir Block<'hir>>,
    // Existing visibility, async, generic/bound and origin information.
}

struct Param<'hir> {
    id: ParamId,
    name: &'hir str,
    ty: &'hir TypeRef<'hir>,
    // Existing writeability.
}

struct Expr<'hir> {
    id: ExprId,
    kind: ExprKind<'hir>,
}

enum ExprKind<'hir> {
    Binary { lhs: &'hir Expr<'hir>, op: BinaryOp, rhs: &'hir Expr<'hir> },
    Call { callee: &'hir Expr<'hir>, args: &'hir [&'hir Expr<'hir>], /* type arguments */ },
    // Other existing expression forms, with the same reference/slice rule.
}
```

The rule also covers blocks, statements, places, patterns, type syntax, fields, variants,
generics, associated members and local binding records. Pattern binders and closure parameters
reference canonical Binding records; resolved name/type/trait links remain semantic identities.
Trait/impl methods point to the canonical Function; do not duplicate its signature/body.
Condition, MatchArm, FieldInit, PatternField, TraitRef/Bound and Literal remain inline dropless
payloads without new identities; their child nodes/bindings carry identity where needed.

`Module.functions`, other owning category vectors and the mixed-owner `Body` warehouse go
away. Typed traversal walks items/members; owner indexes support category-filtered iteration.
Do not retain another permanent category index without a concrete consumer/cost justification.
`*Buffer` aliases become explicit slices or construction Vecs, not renamed wrappers.

```text
Before: function.body → BlockId → Body.blocks → tail ExprId → Body.exprs
After:  function.body → &Block → tail &Expr → lhs/rhs &Expr

Read a parameter: function.params[0].ty
Find it by identity: snapshot.node(param.id.hir_id()) → NodeRef::Param(&Param)
```

Source and typed facts remain separate: `TypeRef` is written/recovered type syntax, not `Ty`.
Direct child references stay within one snapshot; cross-module/revision edges retain scoped
source/definition identities. Handles retain `Arc<LoweredModule<'hir>> + typed ID` and remain
bounded by the caller's arena lifetime.

## 3. Identity, owners and origins

```text
struct HirId {
    snapshot: SnapshotId,
    owner: OwnerId,
    local: LocalNodeId,
}

struct FunctionId(HirId);                   // Likewise ParamId, ExprId, FieldId, etc.

struct OwnerInfo<'hir> {
    parent: Option<HirId>,                 // Direct containment parent of the root.
    nodes: Box<[NodeEntry<'hir>]>,
    attributes: BTreeMap<LocalNodeId, &'hir [Attribute<'hir>]>,
    source_map: SourceMap,
}

struct NodeEntry<'hir> {
    node: NodeRef<'hir>,
    parent: Option<LocalNodeId>,           // Root uses OwnerInfo.parent instead.
}

enum NodeRef<'hir> {
    Module(&'hir Module<'hir>),
    ModuleDecl(&'hir ModuleDecl<'hir>),
    Function(&'hir Function<'hir>),
    Const(&'hir Const<'hir>),
    Static(&'hir Static<'hir>),
    Struct(&'hir Struct<'hir>),
    Enum(&'hir Enum<'hir>),
    Trait(&'hir Trait<'hir>),
    Impl(&'hir Impl<'hir>),
    TypeAlias(&'hir TypeAlias<'hir>),
    OpaqueType(&'hir OpaqueType<'hir>),
    AssociatedType(&'hir AssociatedType<'hir>),
    GenericParam(&'hir GenericParam<'hir>),
    Field(&'hir Field<'hir>),
    Variant(&'hir Variant<'hir>),
    Param(&'hir Param<'hir>),
    Binding(&'hir Binding<'hir>),
    Block(&'hir Block<'hir>),
    Stmt(&'hir Stmt<'hir>),
    Expr(&'hir Expr<'hir>),
    Place(&'hir Place<'hir>),
    Pattern(&'hir Pattern<'hir>),
    TypeRef(&'hir TypeRef<'hir>),
    Use(&'hir UseDecl<'hir>),
    Import(&'hir ImportLeaf<'hir>),
}

struct SourceMap {
    spans: Box<[Option<Span>]>,            // Aligned with this owner's nodes.
    details: HashMap<LocalNodeId, SourceSites>,
}
```

`SnapshotId` replaces lowering identity, not source revision or the shared physical arena.
`OwnerId` indexes `LoweredModule.owners`;
`local` indexes that owner's node entries across kinds. Owner-local counting is real, not a tag
on a module-wide vector. Local slot zero is the owner root; reserve it before lowering children.
Typed IDs wrap the same full identity and validate the node kind; constructors stay private.
Checked counters reject identity/index overflow. Local node numbers are never parameter positions.

| Special identity | Target |
| --- | --- |
| ModuleRootId / ModuleId | Module root / child ModuleDecl header respectively |
| LocalId | Binding, uniformly for pattern binders and closure parameters; not semantic BindingId |
| FunctionId | Canonical function/method; no separate TraitMethodId |
| ConstId | Canonical free/associated const, including requirements without initializers |
| AssociatedTypeId | Associated type declaration, not TypeAliasId or synthetic TypeRefId |
| Remaining typed IDs | Corresponding NodeRef kind: ParamId, GenericParamId, FieldId, VariantId, BlockId, StmtId, ExprId, PlaceId, PatternId, TypeRefId, UseId, ImportId and ordinary declaration IDs |

| Owner | Contents |
| --- | --- |
| Module root | Source-ordered item links, authored uses/leaves and child module headers |
| Each function, including local and trait/impl methods | Signature, parameters and body; closures stay within this owner |
| Each const/static declaration, including associated constants | Signature and optional initializer, even when no initializer exists |
| Each struct/enum/trait/impl/type declaration, including associated types | Header, generics and non-owner member nodes; nested owner roots have explicit parent links |

Fields/variants belong to the enclosing type owner. Retain the local-function Stmt wrapper:
function root → declaring Stmt → Block is its containment chain. Child module contents are
not traversed through the parent's header. Associated declarations gain real IDs; synthetic
type-name IDs no longer impersonate members.

Only written generics are structural children of a method. Inherited impl/trait binders stay
in their declaring owner; enclosing-declaration IDs supply semantic scope. Do not copy binder
records with old IDs into methods. Build independent method-local synthetic receiver syntax,
resolving its enclosing impl through ordinary context; do not reuse the impl's TypeRef as a
second containment child. This replaces current sharing in `lower/item.rs`, not binder meaning.

Before publication, validate:

- Each node's own ID equals its `{snapshot, owner slot, local slot}`; typed kind agrees.
- Only local zero is an owner root; register it only in its own owner, never again in the parent.
- Only the module root has no containment parent; all parent chains reach it without cycles.
- Spans/nodes have equal lengths; every attributes/details key names a valid local node.

Lookup checks snapshot, owner bounds, local bounds and expected kind before returning a node.
Semantic facts check the same complete ID. Context-specific SourceUnit/DefinitionId checks
remain additional boundaries; a local HIR identity cannot authenticate a semantic context.
Unchanged lowering reuse keeps identity; edits create a new snapshot. Semantic IDs are not
replaced with HirId, and no HIR ID becomes persistent/serialized executable identity.

SourceMap no longer allocates IDs. The owner builder reserves identity, then records nodes
and origins together. `SourceSites` preserves existing name/reference/path-prefix/terminal,
constructor-field, import-root/leaf and full static-initializer locations. Synthetic sites
are explicitly absent; physical UTF-8/CRLF offsets remain unchanged. Spans may still order
attributes and delimit lexical lint scopes; they never recover attachment identity.

## 4. Attributes

```text
struct Attribute<'hir> {
    name: &'hir str,
    form: AttributeForm<'hir>,
    span: Span,
}

enum AttributeForm<'hir> {
    Bare,
    Value(AttributeValue<'hir>),
    Arguments(&'hir [AttributeArgument<'hir>]),
    Malformed { recovered: Option<AttributePayload<'hir>> },
}

enum AttributePayload<'hir> {
    Bare,
    Value(AttributeValue<'hir>),
    Arguments(&'hir [AttributeArgument<'hir>]),
}

struct AttributeArgument<'hir> {
    name: Option<&'hir str>,
    value: AttributeValue<'hir>,
    span: Span,
}

enum AttributeValue<'hir> {
    Literal(&'hir str),
    Path(&'hir str),
    List(&'hir [AttributeArgument<'hir>]),
    Missing,
    Malformed,
}
```

The recovery payload retains one recognizable form when outer syntax is damaged; it cannot
contain another recovery wrapper. `Missing` distinguishes absent required values from malformed
value syntax. Keep original literal/path spellings; semantic consumers interpret them later.

| Input | Form |
| --- | --- |
| `#[a]` | Bare |
| `#[a()]` | Arguments([]) |
| `#[a = []]` | Value(List([])) |
| `#[a = ]` | Value(Missing), not Bare |
| `#[a(key = )]` | Arguments([key: Missing]) |

The AST already has exclusive payload forms. Lower from that payload and required-token state,
not the current accessor that conflates no assignment with an assignment missing its value.
Retain malformed syntax only where the parser actually retains it; do not invent extra nodes
for discarded tokens or treat AST error values as empty lists.

Attachment is `owner.attributes[target.local]`, with list order preserved. Every target has
one canonical node, including fields, local functions, associated types and constants without
initializers. A method's surface and function view share one target. Remove the flat root list,
`target_span`, the second attribute-collection walk and span-based target searches.

```text
struct UseDecl<'hir> {
    id: UseId,
    leaves: &'hir [ImportLeaf<'hir>],
    // Visibility and authored use structure/sites.
}

struct ImportLeaf<'hir> {
    id: ImportId,
    declaration: UseId,
    // Flattened written path, alias/glob and leaf source sites.
}
```

`#[a] use p::{x, y};` attaches once to UseDecl; both leaves point back to it. Even a recovered
use with no valid leaves keeps its retained declaration target. Resolution still consumes
flattened leaves; this adds no new import semantics.

Lint owns eligible target kinds and inheritance. It reads NodeRef instead of rebuilding AST
span sets. Module headers carry parent-snapshot attributes; child inheritance follows the
resolved ModuleIdentity, not byte containment. Local lexical application and diagnostic ranges
may still use source spans. `#[lang]` and alias validation query the canonical declaration.

No new attribute syntax/targets: variant nodes can have identity/index support, but the current
parser does not accept variant attributes. Do not add parameter/expression attributes, inner
attributes or new effects/macros. Keep Native attribute admission checks and language diagnostics.

## 5. Construction, cutover and acceptance

```text
AST + exact SourceFile
  → reserve snapshot/owners/node IDs
  → lower children into immutable nodes; attach attributes/origins immediately
  → fill reserved root/index entries; validate parents, kinds and complete tables
  → check cancellation; finish immutable module/indexes
  → publish Arc<LoweredModule<'hir>> borrowing the caller's arena
```

Use temporary owning vectors for reservations and lists, then allocate final dropless slices.
For inline Param/Field/GenericParam records, register NodeRef only after final slice allocation;
never index Vec elements or copy already indexed nodes into another allocation. Traversal and
ID lookup must return the same entity. No permanent holes, rewiring or structural-pointer cycles.
Recursive declarations resolve through IDs later. Recoverable syntax can publish structural HIR;
cancellation cannot publish a completed result. Old snapshots remain readable while their
external arena and borrowed analysis scope remain alive.

| Checkpoint | Required result |
| --- | --- |
| HA00 + HA01: combined cutover | Owned syntax/semantic records → scoped old/new graph sessions → real arena nodes, IDs, owners, origins and attributes. Migrate resolver/typeck/reuse/import/query/compiler/lint consumers together. Prove A1 release with A2/B1 results still usable. No project arena, hidden self-reference, forwarding accessors or dual authoritative graph. |
| HA02: acceptance | Strengthen existing semantic tests, remove superseded structural assertions, update HIR guide and retire this design after completion; Native work stays deferred. |

All checkpoints must build their affected targets. Existing Native facts stay normally owned;
their HIR IDs and borrowed API lifetimes adapt without changing registration semantics. This does
not activate Native redesign, optional source text, per-body arenas, a new incremental query
algorithm or Runtime changes.

### Approved checkpoint sequencing

HA00's production-arena proof requires HA01's actual graph and consumer replacement; the
remaining work therefore proceeds together, with buildable sub-checkpoint commits. This
changes sequencing, not scope or acceptance: parser → real borrowed HIR → semantic analysis
must permit independent A1 arena release while A2/B1 results remain usable. HA02 stays separate.

A toy AST, allocated references to the old owned graph, PhantomData lifetimes or a second
unconsumed HIR model cannot satisfy that proof. Cache detachment alone is not phase completion.

### Current checkpoint and carried checks

- HA00 first sub-checkpoint: SourceParseCache owns exact physical parses independently of HIR;
  prepare_syntax returns an owned logical module manifest before existing lowering runs.
  Source/limit checks, non-older publication, cache pruning and cancellation guard its lifetime.
  Temporary owned prior-syntax inputs preserve the last published parse identity when a newer
  syntax batch completes but semantic analysis is cancelled; they retain no HIR/history map.
  SourcePreparer uses explicit mutable cache/inline-ID borrows, not RefCell; declaration and
  signature preparation require mutable database access.
- Selected-body sub-checkpoint: FunctionFacts owns no HIR; BodyCacheEntry keeps strong facts
  and weak file/query handles. Returned FunctionAnalysis owns its exact graph/context. The
  existing lifecycle scenario proves both unrelated A and selected B graphs can release while
  cached facts remain; missing graphs trigger fresh checking and reject stale expression IDs.
- Attribute payload checkpoint: one exclusive form (bare, assigned value, arguments or malformed
  recovery) replaces independent value/argument options. Missing values/list slots and authored
  path spelling survive lowering. Language roles and lint policies reject recovered malformed
  forms. Target attachment still uses spans until the identity cutover; no parser syntax expands.
  Nine lowering, six language-item and nineteen lint tests pass; existing scenarios are strengthened
  and a lint recovery witness checks that damaged policy syntax cannot suppress findings.
- Six preparation tests, three arena/revision regressions, the deterministic cancellation
  scenario and eleven selected-body tests pass. Structure/diff, changed-file rustfmt and HIR
  all-target Clippy pass. No Bump graph or SDK arena cutover is claimed yet.
- On rustc 1.100.0, required workspace fmt is blocked by unchanged `hir/ids.rs` macro arguments
  and `kagari-syntax/src/ast/tokens.rs` enum spacing. Workspace Clippy is blocked by unchanged
  `kagari-compiler/src/bytecode/placement.rs:110` (`needless_range_loop`). No generated allowance,
  unrelated repair or passing-workspace claim; these remain integration acceptance blockers.
- Full workspace tests and exhaustive feature/backend/platform matrices remain CI-only.

Validation selection:

- Preserve [arena/revision isolation](../crates/kagari-hir/src/analysis/tests/arena.rs), staged
  query reuse, inline physical coordinates, static/local-function ownership and checked handoff.
- Strengthen [lowering tests](../crates/kagari-hir/src/tests/lower.rs) for distinct payload forms,
  canonical grouped-use targets and associated-member attachment. Keep parser recovery and
  exact diagnostic ranges; consolidate obsolete table-shape assertions rather than duplicating them.
- Preserve [import provenance](../crates/kagari-hir/src/imports/tests/provenance.rs), language
  role/alias checks and [lint policies](../crates/kagari-lint/src/tests.rs), including external
  module inheritance, malformed policies and local scope isolation.
- New ownership evidence: full typed-ID rejection, scoped parallel immutable reads, result/table
  release on a scoped worker, and compile-fail arena escape/reset/drop while borrowed. Assert
  Send/Sync and dropless constraints; verify cancellation/unwind publishes nothing, external
  resources drop and region bytes release when their final users end, independently of unchanged modules.
- Sample compiler lowering from admitted HIR and existing source-free execution; executable
  products keep their own verification, not borrowed arena references.
- Run structure, fmt, workspace all-target Clippy and diff checks at implementation checkpoints;
  focused tests locally, exhaustive workspace/features/platforms in CI only.
- Compare existing frontend workloads and snapshot retention/release before/after; report compile
  time and memory separately. No speedup target or claim is established by adopting an arena.

## Mechanism references

- [rustc HIR at 5f4effd5795](https://github.com/rust-lang/rust/blob/5f4effd5795/compiler/rustc_hir/src/hir.rs):
  direct child references, OwnerNodes indexes of references and owner-local AttributeMap.
  [HirId](https://github.com/rust-lang/rust/blob/5f4effd5795/compiler/rustc_hir_id/src/lib.rs)
  separates owner/local identity. Kagari also validates each retained lowering's identity.
- [rustc context](https://github.com/rust-lang/rust/blob/5f4effd5795/compiler/rustc_middle/src/ty/context.rs)
  borrows WorkerLocal arenas through an external context lifetime. Adopt the external-owner
  mechanism, not rustc's complete query/thread model. Kagari also needs independent module-region
  retirement for long-lived tools; that ownership API remains open. Reviewed against the local checkout.
- [bumpalo 3.20.3](https://docs.rs/bumpalo/3.20.3/bumpalo/struct.Bump.html): Send/!Sync,
  stable arena allocations and no automatic element Drop. External ownership avoids needing
  to make the allocator Sync; only immutable allocated node graphs are shared.
