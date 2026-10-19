# Reading HIR Data and Analysis

`kagari-hir` owns source meaning, recoverable analysis and editor queries. It
contains more than the `hir/` node model: lowering, import/name resolution, type
checking and snapshot publication have separate owners. The
[architecture overview](../architecture.md#compilation-pipeline) describes the
compiler handoff. Detailed contracts and source-to-field diagrams live beside
the code; [engineering rules](../../AGENTS.md#documentation-maintenance) define
the documentation conventions.

## Storage and IDs

Start with [LoweredModule](../../crates/kagari-hir/src/lower/mod.rs),
[Module](../../crates/kagari-hir/src/hir/item/mod.rs),
[Body](../../crates/kagari-hir/src/hir/body.rs) and
[the ID families](../../crates/kagari-hir/src/hir/ids.rs).

```text
LoweredModule
+-- source: Arc<SourceFile> -> text, identity, revision
+-- module: Module
|   +-- functions, consts, structs, enums, traits, impls, ...
|   `-- body: Body
|       +-- exprs:    Vec<(HirOwner, ExprData)>
|       +-- stmts:    Vec<(HirOwner, StmtData)>
|       `-- blocks, places, patterns, types: analogous owner-tagged vectors
`-- source_map: SourceMap -> matching ID slots and source byte sites
```

These arrows describe owned fields except the explicitly shared `Arc`. A body
node's child ID is a reference into a vector, not an owned subtree. `Body` is
shared by the module's declaration syntax and multiple function/constant owners.
Its name does not imply one allocation per function.

| Handle | Lookup and validity |
| --- | --- |
| `FunctionId`, `ConstId`, other plain declaration IDs | Index the matching collection in the same module; no arena is carried. |
| `ExprId`, `StmtId`, `BlockId`, `PlaceId`, `PatternId`, `TypeRefId` | Carry arena, owner and an allocation-wide index; `Body` checks arena/owner and indexes its vector. |
| `ParamId`, `LocalId` | Carry arena/owner and source-map-wide indices. Parameters live in function records; local bindings live in statements, patterns or closure parameters. Semantic maps use the complete IDs. |
| `FieldId`, `VariantId` | Carry arena, enclosing struct/enum ID and member slot; look up the owner's nested member vector. |
| `SourceDeclRef` | Carries a full `SourceUnit` plus a declaration handle for cross-module access. Do not index the importing module with it. |
| `DefinitionId` | Interpreted by its matching immutable definition-table prefix; distinct from all lowering-local IDs. Exact owned paths belong to explicit authoring/display queries. |
| `BindingId` | An analysis identity, scoped body definition and resolved local slot. Changing the definition context does not change the binding's owning analysis or source slot. |

Cloning/reusing one lowering retains its arena; fresh lowering creates a new one.
`SourceUnit` additionally records logical module, source file and revision. Logical
module identity is not the same as a physical filename. See
[source maps](../../crates/kagari-hir/src/source_map.rs) for byte ranges and
specialized name/member/path sites; synthetic nodes may have empty ranges.

HIR types, declarations, signatures and checked-call facts use fixed `DefinitionId`
fields. Their Rust models have no parameter selecting paths versus IDs. A
`DefinitionReferences` visitor covers every retained reference, including binding
body owners and attached Native/Host inputs. `DefinitionMetadata::checked` validates
those references against an immutable prefix; it does not validate language meaning.

Independently reusable facts implement `RemapDefinitions`: import their referenced
ancestor closure, then rebuild ID fields and unique keys in the destination scope.
For `fn read(p: Player) { val saved = p; }`, importing the `saved` declaration changes
its body definition to the destination's `read` ID while retaining the analysis and
local slot; a binding from another source analysis still cannot identify `saved`.
Collapsed keys, cancellation and identity/traversal limits reject conversion.

Whole analyzed/prepared/declaration bundles retain source arenas, name tables and
registration versions. They expose reference validation and editable scoped
extraction, not arbitrary representation conversion or whole-bundle import. Their
reusable constituent facts are the remapping boundary. Selected-body queries accept
scoped IDs and use the analysis context for cache keys; callers with an external
path explicitly look it up in the relevant declaration snapshot first. Source,
revision and dependency checks still govern reuse.

## Mapping source syntax to model fields

Read a model's source fragment together with its expanded payload. For example,
`type Item;` names a member but leaves its assigned type absent; `type Item = i32;`
supplies a type-syntax link. A stored ID is an allocated reference, not part of the
written program. Examples use symbolic IDs, show empty buffers and `None` values,
and label recovery, synthetic nodes and unpopulated models explicitly. Contextual
fragments still need suitable declarations and typing; HIR allocation is not proof
of accepted language semantics.

The complete mappings are beside their owners:

| Source question | Model and field examples |
| --- | --- |
| Which declarations does a file contribute, and where are their payloads? | [Module / Item](../../crates/kagari-hir/src/hir/item/mod.rs): source order, per-kind collections, imports and shared body storage. |
| Which function fields come from `pub`, `async`, generics, parameters, `where`, result and body? | [Function / Param / FunctionKind](../../crates/kagari-hir/src/hir/item/function.rs): full signature expansion, omitted annotations/bodies and method links. |
| How do field/variant declarations differ from constructor values and patterns? | [Struct / Field / Enum / Variant / OpaqueType](../../crates/kagari-hir/src/hir/item/adt.rs): complete declaration payloads, owner/member slots and native-provider surfaces. |
| What do trait/impl headers, associated items and generic bounds store? | [TraitDef / Impl / associated members / bounds](../../crates/kagari-hir/src/hir/item/behavior.rs): required versus defined members, member input versus output constraints and function/constant links. |
| Where do constant initializers, child module bodies and flattened use leaves go? | [Constants](../../crates/kagari-hir/src/hir/item/storage.rs) and [modules/imports](../../crates/kagari-hir/src/hir/item/module.rs): owner context, header-only children, alias/glob and leaf/root source ranges. |
| Is the final expression also a statement, and how is `+=` represented? | [BlockData / StmtKind](../../crates/kagari-hir/src/hir/stmt.rs): all statement payloads, tail absence, place/loop/pattern links. |
| How does `object.run(41)` differ from calling a named function or closure? | [ExprKind](../../crates/kagari-hir/src/hir/expr/mod.rs): Call-to-Field links, name qualification versus call type arguments, all expression shapes and inline helper records. |
| Which parts of `items[next_index()].count` are places and which are expressions? | [PlaceKind](../../crates/kagari-hir/src/hir/place.rs): root and projection expansion, once-only target structure and later validation. |
| Which names bind in a pattern, and what does shorthand synthesize? | [PatternKind / PatternField / PatternBound](../../crates/kagari-hir/src/hir/pattern.rs): all pattern forms, nested IDs and provisional bindings. |
| How do generic arguments, associated equalities and projections occupy type fields? | [TypeKind](../../crates/kagari-hir/src/hir/ty.rs): every type form, callable normalization, grouping and invalid argument-order retention. |
| Are literals, operators or `val`/`var` already runtime facts? | [Literals](../../crates/kagari-hir/src/hir/expr/literal.rs), [operators](../../crates/kagari-hir/src/hir/expr/ops.rs) and [writeability](../../crates/kagari-hir/src/hir/writeability.rs): spelling/category/policy versus later checked behavior. |
| Who allocates IDs, and can their numerical values change? | [IDs](../../crates/kagari-hir/src/hir/ids.rs) and [Body](../../crates/kagari-hir/src/hir/body.rs): full ID fields, row lookup, validity and fresh-lowering boundaries. |

For a concrete associated-member comparison:

```text
trait Reader {
    const LIMIT: i32 = 10;
    type Item;
    fn read(self) -> Self::Item;
}
struct Number { val value: i32 }
impl Reader for Number {
    type Item = i32;
    fn read(self) -> Self::Item { self.value }
}
```

| Fragment | Inline record | Referenced storage |
| --- | --- | --- |
| Trait's `const LIMIT: i32 = 10;` | `AssociatedConst { name: "LIMIT", name_ref: n, ty: t, initializer: Some(c) }` | n is a synthetic name-site type node; t is i32 syntax; c selects a ConstItem whose initializer expression spells 10. |
| Trait's `type Item;` | `AssociatedType { name: "Item", name_ref: n_item, ty: None, generic_params: [], parameter_bounds: [], bounds: [] }` | n_item is a synthetic name-site node; absence of ty requires an implementation definition. |
| Impl's `type Item = i32;` | Same member fields, with a distinct name_ref and `ty: Some(t_impl)` | t_impl selects i32 type syntax, not the trait member's name node. |
| Trait's `fn read(self) -> Self::Item;` | `TraitMethod { has_default: false, id: m, name: "read", function: f_trait }` | f_trait selects Function with kind TraitMethod, a self parameter, result syntax and no body. |
| Impl's method body | `ImplMethod { name: "read", function: f_impl }` | f_impl selects Function with kind ImplMethod; its body links to a block whose tail is a Field expression for self.value. |

The omitted impl constant uses the checked trait default; it does not create an
extra impl AssociatedConst record. The expanded impl/trait header, generic family
and where-clause examples in behavior.rs explain the remaining fields. An
associated type's optional assigned syntax does not enable trait type defaults:
current checking rejects them. Methods belong to trait/impl collections and link
to `Module.functions` through `FunctionId`. Receiver presence comes from the
function parameters; `self` uses ordinary value semantics without a separate
receiver-category tag. ADT `impls` vectors remain unpopulated; use `Module.impls`
and checked lookup facts to find associated implementations.

## One function through the tables

```text
fn add(x: i32) -> i32 {
    val y = x + 1;
    y
}
```

Read [item lowering](../../crates/kagari-hir/src/lower/item.rs), then
[statement lowering](../../crates/kagari-hir/src/lower/stmt.rs) and
[expression lowering](../../crates/kagari-hir/src/lower/expr/mod.rs).
The diagrams below abbreviate complete IDs; positions are illustrative.

```text
Function(f).params = [Param { id: p, name: "x", ty: t, ... }]
Function(f).body = Some(b)
Block(b).statements = [s]
Block(b).tail_expr = Some(result)
Stmt(s) = Binding { pattern: binding, initializer: sum, ... }
Pattern(binding) = Name { local: l, name: "y" }
Expr(sum) = Binary { lhs: x, op: Add, rhs: one }
Expr(x) = Name { name: "x", ... }
Expr(one) = Literal(Number, "1")       // literal record abbreviated
Expr(result) = Name { name: "y", ... }
```

The lowerer allocates source-map slots and node rows together under owner
`Body(Function(f))`. `p` and `l` are identities, not indices into `Body.exprs`.
The tail expression is separate from the statement list. Parentheses disappear
as wrappers; constructor shorthand and interpolation can create HIR nodes that
are not in one-to-one correspondence with AST nodes.

```text
stored HIR IDs      independent analysis tables / computed lookups
x                -> ResolvedNames.exprs -> Param(p)
result           -> ResolvedNames.exprs -> Local(l)
sum              -> TypeTable.exprs     -> semantic i32 type
l                -> TypeTable.locals    -> semantic i32 type
t                -> type syntax         -> resolved type facts
sum              -> SourceMap.expr_span -> bytes for the addition (possibly with trailing trivia)
```

[ResolvedNames](../../crates/kagari-hir/src/resolver/resolved.rs) records bindings;
[TypeTable](../../crates/kagari-hir/src/typeck/table.rs) records types and selected
operations. `TypeRefId` addresses type syntax; the semantic
[TypeId](../../crates/kagari-hir/src/types.rs) is an enum, not a node index.
Neither name resolution nor a successfully allocated HIR node proves the source
is valid. Facts can remain useful alongside diagnostics.

The runnable examples on `lower_module` and `analyze_source` verify storage/source
ranges and the checked arithmetic type. Declaration metadata lives in
[Declarations](../../crates/kagari-hir/src/declarations.rs); body checking builds
on [ModuleSignatures](../../crates/kagari-hir/src/typeck/mod.rs). Missing facts can
mean an inapplicable operation, a stage not requested or source recovery; each
table's accessor contract explains which applies.

### Pattern-based declarations

Every ordinary `Binding` owns one `PatternId`, declaration-wide writeability, an
optional whole-value annotation and its initializer; simple names have no separate
statement-level representation. Pattern leaves own local identities. For example,
`var (x, Point { y, .. }) = make_pair();` links a tuple pattern to a name leaf and
a struct pattern, whose field child owns the other local. Rest is a boolean on the
struct pattern, not a field or binding.

Resolution visits the initializer before publishing any leaf. Declaration bare
names always bind; other pattern contexts retain their constructor policy. The
shared checker records nominal field identities and leaf types, checks access and
completeness recursively, and requires conservative declaration irrefutability.
The whole annotation contextualizes the initializer rather than converting leaves.
Usage/navigation consume those same leaf identities; `Body::pattern_bindings`
selects canonical declaration leaves, with the first alternative representing an
Or pattern's consistent binding set.

Compiler lowering consumes checked facts through the shared pattern extractor.
Declarations have no failure continuation; refutable constructs require a real
match/condition/iteration failure edge, not an added declaration trap. Extraction
stages values in pattern source order, skips ignored projections, and selects the
successful alternative's values before allocating captured-variable cells. All
leaf stores precede debug-local publication. Captures, lexical scope exit and GC
roots retain their existing owners; backends neither re-resolve fields nor infer
leaf types. See [declaration semantics](../spec/syntax.md#blocks-and-statements).

### Struct update facts

For `Player { health: value, ..base }`, StructInit retains explicit fields in source
order and `base: Some(base_expr)`. The dedicated AST update-base node distinguishes
no marker from a marker with a missing operand; recovery retains the latter as a
missing HIR expression. Resolver, completion and constant validation visit the base
after explicit fields. The base is not a synthetic FieldInit or a range.

ResolvedStructInit.fields remains aligned with written labels, including recovery
holes; base_fields separately stores omitted checked definition IDs in declaration
order. Navigation must not pair omitted members with explicit source spans. Both
field lists participate in definition remapping, while base expression facts retain
ordinary body/snapshot identity. Constructor inference includes the base before
finalizing type arguments and checks all-field visibility and normalized nominal
identity. Compiler lowering evaluates explicit operands and the base, reads only
those checked omitted fields and emits the existing complete MakeStruct contract.
No backend resolves source fields or introduces a special update/default operation.

## Structured import paths and source sites

[Import](../../crates/kagari-hir/src/hir/item/module.rs) and
[ImportDirective](../../crates/kagari-hir/src/imports/mod.rs) retain flattened
written paths as `segments: Vec<String>`. For example, inside module `pkg::app`:

```text
use super::api::{sum as add, ops::*};

Module.imports[0]: segments = ["super", "api", "sum"], Named { alias: Some("add") }
Module.imports[1]: segments = ["super", "api", "ops"], Glob
```

[Item lowering](../../crates/kagari-hir/src/lower/item.rs) copies accumulated
segments and sites at group branches, preserving leaf order. Each leaf becomes
one directive identified by its `SourceUnit` and import slot. The alias is not a
path segment: `local_name()` selects it, otherwise the named terminal; a glob has
no local name and excludes `*` from its path. Glob expansion contributes bindings,
not additional HIR leaves or directives.

Keep three representations distinct:

- **Written:** HIR leaves and directives retain `super`, `self`, `crate` and package
  aliases exactly as written. Resolution does not overwrite these segments.
- **Normalized lookup input:** the
  [catalog](../../crates/kagari-hir/src/imports/catalog.rs) selects candidate packages
  and components relative to the importing module. Here `super::api::sum` selects
  package `pkg` with components `["api", "sum"]`. Components can name members, not
  only physical modules. `crate` starts at the package root, `self` at the current
  module, and repeated leading `super` ascends parents; root underflow fails lookup.
  Dependency and installed/native aliases follow the catalog's package-selection
  rules; ambiguity and physical module collisions are not resolved arbitrarily.
- **Resolved:** `ResolvedTarget`, `NamespaceId` and `SourceDeclRef` identify semantic
  destinations; binding origins independently retain how they were reached. An
  `api` re-export can target a declaration in `math`, not the written facade. See
  [the alias/glob example](#an-alias-through-a-public-glob).

The lowering-local `ImportPath` accumulates `segments` and `sites: Vec<(usize,
Span)>` together. [SourceMap](../../crates/kagari-hir/src/source_map.rs) stores the
sites in `import_paths`, keyed by the same flattened import slot:

| Site | Exclusive prefix end | Prefix selected from that leaf | Physical span |
| --- | --- | --- | --- |
| Both leaves' `super` | 1 | `segments[..1]` = `["super"]` | Original `super` token |
| Both leaves' `api` | 2 | `segments[..2]` = `["super", "api"]` | Original shared `api` token |
| Named leaf's `sum` | 3 | `segments[..3]` = `["super", "api", "sum"]` | Original `sum` token, excluding `as add` |
| Glob leaf's `ops` | 3 | `segments[..3]` = `["super", "api", "ops"]` | Original `ops` token, excluding `::*` |

Every stored end is nonzero and bounded by the leaf's **written** segment count,
never the normalized component count. Each span covers only that physical segment,
not the whole prefix. Shared group ancestors repeat per leaf; aliases and stars
add no site. The leaf `span` includes alias syntax (`sum as add`); `root_span`
covers the outer `super::api::{sum as add, ops::*}` tree, excluding `use` and `;`.
Ranges remain source byte offsets, including UTF-8/CRLF and inline-module routing.

[Import resolution](../../crates/kagari-hir/src/imports/resolve.rs) slices the written
prefix for each site. Intermediate navigation uses Type; a named terminal retains
all eligible Type/Value results, while a glob selects a Type namespace. Generated
native imports retain their validated category restrictions. Queries select the
most specific physical site; `source_targets_at` preserves categories and origins,
and `source_target_at` does not choose between dual-category hits.

Recovery retains only available syntax: no root tree means no leaf, and recovered
empty leaves can have no sites. An empty unaliased path has `local_name() == None`;
a usable explicit alias still takes precedence. Missing aliases, trailing separators
and empty groups retain their parser/lowering diagnostics and recovered structure,
not fabricated segments or unchecked terminal assumptions.

Import normalization, traversal and syntactic dependency-prefix matching consume
segments directly. Joined text belongs only at diagnostic payloads or necessary
existing text lookup boundaries; text callers adapt to the shared traversal rather
than defining another resolver. Syntactic facade dependencies remain distinct from
canonical target/provenance edges and failed/pending lookup observations. Reuse
compares written segments alongside kind, visibility, dependencies and outcomes;
equal canonical targets alone do not authorize reuse across source revisions or
HIR arenas. Retained snapshots keep their matching source maps and immutable catalogs;
see [query reuse and publication](#query-work-reuse-and-publication).

The segment separation follows Rust HIR's
[Path](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_hir/hir/struct.Path.html)
and [PathSegment](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_hir/hir/struct.PathSegment.html)
model (rolling nightly API documentation consulted on 2026-10-19). Kagari instead
owns its strings and snapshot resolution facts; this does not adopt Rust arena
lifetimes or symbol interning, or establish a measured performance improvement.

## An alias through a public glob

Bind these source units using `SourceDatabase::bind_module` with package
`PackageId("pkg")` and paths `["math"]`, `["api"]` and `["app"]`, then supply their
text using `SourceDatabase::set`. Filenames alone do not establish this layout.
The runnable example on `AnalysisSnapshot::check_program` in
[program.rs](../../crates/kagari-hir/src/program.rs) includes this setup.

```text
// math
pub fn sum(x: i32) -> i32 { x + 1 }
// api
pub use pkg::math::*;
// app
use pkg::api::sum as add;
fn main() -> i32 { add(41) }
```

[Item lowering](../../crates/kagari-hir/src/lower/item.rs) flattens each real `use`
leaf. The [import builder](../../crates/kagari-hir/src/imports/builder.rs) turns
these leaves into directives and binding candidates. The following is a lookup
outline; arrows to `S` denote equal canonical targets, not extra owned functions:

```text
S = SourceDeclRef { unit: math's SourceUnit, item: Function(sum_id) }

math NameTable["sum"][Value].strong -> Declaration(S), target Source(S), public
api  directive              -> Glob, resolved Namespace(math)
api  NameTable["sum"][Value].globs  -> GlobImport(api_directive), target Source(S), public
app  directive              -> Named { Type: Absent, Value: Resolved(Source(S)) }
app  NameTable["add"][Value].strong -> NamedImport(app_directive), target Source(S)
```

Each candidate also retains its owner, visibility and source site. Provenance says
which declaration/import introduced it; the target says where the declaration
actually lives. A glob's directive targets a namespace, while its contributed
bindings target eligible members. Public re-exports become candidates with public
visibility in that module's table; the catalog filters access for each importer.
Public members derive from these namespace candidates, including glob expansion.
There is no parallel HIR export list. Portable native `ModuleDecl.exports` remains
authoritative authored metadata, keyed by category and alias; generated native
imports retain its validated category. Ordinary source use leaves resolve both
categories.

[NameTable](../../crates/kagari-hir/src/resolver/table.rs) selects the first nonempty
tier: strong, glob, then implicit. Conflicting strong entries are ambiguous;
equal-target glob candidates can coexist with their origins preserved. These are
precedence tiers independently inside each Type/Value slot. Path prefixes select
Type and terminal lookups explicitly select their use-site category. Navigation
retains that category; a dual import has two `source_targets_at` results and no
arbitrary `source_target_at` winner. See
[import records](../../crates/kagari-hir/src/imports/mod.rs) for the concrete fields.

The [solver](../../crates/kagari-hir/src/imports/solver.rs) seeds declarations once
and replaces one module's contributions at a time. Namespace observations include
missing/pending lookups and glob membership; reverse watchers schedule affected
modules independently of linking dependencies. Draft named slots reserve their
category and pending glob membership blocks premature weak-tier selection.

At quiescence it closes the union of pending dependencies, releases justified
reservations and resumes propagation. Finite acyclic derivations prevent aliases
from proving themselves through cyclic forwarding; equal-target origins remain
available for navigation. Publication requires a drained queue and no pending
candidate or directive. Exact bounded state history detects repetition; a separate
input-sized work bound reports exhaustion. These are distinct from cancellation
and ordinary unresolved source diagnostics. No failed draft enters analysis caches;
old snapshots retain their immutable catalog.

Draft candidates retain the full LookupOutcome, so ambiguity and access failures
cannot become absence. Each selected candidate carries a finite derivation of
real `(DirectiveId, NameNamespace)` keys; lookup excludes a derivation containing
its own directive/category. Type and Value are distinct here: a Value lookup may
legitimately use a Type alias from the same import. Prefix/glob proofs compose;
equal-target origins remain available, with the shortest valid derivation retained.
Proofs are solver evidence, not definition identity or cache equivalence.

The work limit counts module evaluations and produced binding contributions,
derived from input modules, use leaves and seeded spellings. An eight-entry
history compares exact facts, observations, queue and closed keys rather than
trusting hash equality. Cancellation, exhaustion and non-convergence preserve
bounded physical import sites and never publish successful graph/cache entries.

At the call site, lexical resolution checks local bindings before module names:

```text
Expr(callee) = Name { name: "add", ... }
ResolvedNames.expr_resolution(callee) -> ResolvedName::Source(S)
ImportedFunctions.get(binding)        -> canonical signature + definition identity
Expr(call) = Call { callee, args: [argument], ... }
TypeTable.call_resolution(call)       -> ResolvedCall
  target = CallTarget::SourceFunction(definition identity for math::sum)
  signature = applied parameter/result types; type_arguments = []
TypeTable.expr_type(call)             -> semantic i32
```

The call target uses a semantic definition identity, not `SourceDeclRef` itself.
`FunctionId` in `S` is meaningful only in math's matching lowering. The checked
program's `source_function(&S)` verifies the complete source unit, finds math's
checked module, then selects its typed function. The example asserts this lookup.

For `use pkg::math as m; m::nested::value()`, the resolver selects `m`, obtains its
namespace with `NamespaceCatalog::namespace_of`, and follows each suffix with
`lookup_member`. This requires a visible `nested` module containing `value`;
entering it does not create another import directive. See
[namespace lookup](../../crates/kagari-hir/src/imports/catalog.rs) and
[body resolution](../../crates/kagari-hir/src/resolver/resolve.rs).

## Query work, reuse and publication

[AnalysisDatabase](../../crates/kagari-hir/src/analysis/mod.rs) owns mutable caches;
returned snapshots retain immutable source and facts through `Arc`. These are the
actual entrypoint dependencies, not phases repeated by every position query:

```text
declarations(source) -> prepare_declarations -> publish DeclarationSnapshot
signatures(source)   -> prepare_signatures
                         `-> prepare_declarations
                      -> publish SignatureSnapshot + declarations
body(source, owner)  -> prepare_signatures -> selected function + constant prerequisites
                      -> publish FunctionAnalysis + signatures
snapshot(source)     -> prepare_signatures -> check/reuse ALL file bodies
                      -> publish AnalysisSnapshot + signatures

snapshot.type_at / definition_at / completion queries -> read prepared facts
snapshot.check_program(root) -> validate reachable checked source closure
```

Read [declaration queries](../../crates/kagari-hir/src/analysis/declaration_queries.rs),
[signature queries](../../crates/kagari-hir/src/analysis/signature_queries.rs) and
[selected body queries](../../crates/kagari-hir/src/analysis/body_queries.rs) in that
order. Declarations include parse/lowering and import resolution; signatures check
type annotations and aggregate contracts; bodies add local bindings, expression
types and selected operations. Syntax errors can still leave useful facts and
diagnostics. Cancellation is an error, checked before publishing completed results.
Older requests do not replace newer published revision caches.

Inline modules currently receive a synthetic file ID and padded source text: bytes
outside the module body become spaces, preserving newlines and physical offsets.
That text is parsed/lowered again. The existing AST subtree is not reused for this
step. Position queries route physical offsets to the corresponding inline analysis.

| Change | Actual reuse boundary |
| --- | --- |
| Unchanged source and dependencies | A file result can retain its `Arc` when revision/arena, imports, reachable namespaces, imported types/functions and aggregate facts match. Installed native declarations participate through these ordinary facts. |
| Body-only edit | Changed source is lowered into a new arena. Signature reuse compares the source surface with eligible function bodies removed and remaps local IDs. Other bodies may reuse facts if environment tokens, diagnostics, body contents and source-map correspondence permit it. |
| Signature/export edit | Imports/namespaces and imported type/callable/aggregate contracts are compared again; failed reuse predicates force checking. This is not guaranteed minimal dependency invalidation. |
| Registry/policy change | Native registration clears dependent caches; parse policy clears parse-dependent caches; body/constant limits clear body/file caches. Host replacement is checked through registry revisions during later reuse decisions. |

[File cache predicates](../../crates/kagari-hir/src/analysis/cache.rs),
[signature remapping](../../crates/kagari-hir/src/typeck/signature_reuse.rs) and
[body remapping](../../crates/kagari-hir/src/typeck/reuse.rs) own these checks.
Retaining an old snapshot remains safe because its IDs still refer to its old
arena/definition tables; remapped facts must use the new owners.

## Checked handoff and registered inputs

[CheckedAnalysis](../../crates/kagari-hir/src/lib.rs) represents the checked
single-module boundary. [CheckedProgram](../../crates/kagari-hir/src/program.rs)
captures the root's reachable import closure from one snapshot, rejects error
diagnostics and retains each module once, including cycles/diamonds. Its order is
deterministic, not a topological execution order. These are inputs to the compiler;
HIR does not generate bytecode, verify MIR or run the functions.

For where non-source declarations enter this flow, follow
[native declaration import](../../crates/kagari-hir/src/native/api.rs),
[generated source views](../../crates/kagari-hir/src/native/render.rs),
[validated language roles](../../crates/kagari-hir/src/language/items.rs) and
[builtin bridges](../../crates/kagari-hir/src/builtin/mod.rs). The runnable semantic
examples install `kagari_stdlib::catalog::shared()` explicitly, as required by the
current foundation contract.

Native types and methods use the same source-unit names, declaration identities,
call checking and namespace lookup as source declarations. Analysis retains no
separate host callable/type catalog or host revision; shared schema and binding
facts remain attached to their installed native declarations.

Registered native defaults follow an additional checked path:

```text
NativeDefaultApplication recipe -> generated trait method with a tail call
FunctionId -> body BlockId -> tail ExprId -> ordinary ResolvedCall
TypeTable.native_default_call(FunctionId) -> that call after recipe/bound validation
source compiler -> selected native entry (no extra script frame)
```

The implementing type receives no synthetic HIR function. Signature queries expose
the method signature without claiming body validation; SDK preparation checks all
generated bodies, including unused defaults. A retained snapshot keeps its proof
and HIR arena together. Source-free linking validates the portable recipe without
depending on this HIR table.


Ordinary interface relationships are owned by
[AggregateCatalog](../../crates/kagari-hir/src/aggregates/relationships.rs).
Inference and member lookup enumerate applicable native/script implementations and
parent applications under caller bounds. If/match inference intersects candidates
across all normally completing arms before choosing a unique most-specific result;
ambiguous intermediate pairs cannot discard a candidate needed by a later arm.

Method selection and implementation counts use the catalog's shared bounded search.
`ImplementationSearchContext` carries the caller's generic assumptions, candidate/
depth limits and cancellation token. Searches check cancellation even when there
are no candidates, and distinguish no unique match from cancellation or exhaustion.
Application/body checking reports exhaustion as a compilation limit, while analysis
owners reject cancellation before publishing signatures, snapshots or body caches.
Lowering retains instantiated caller assumptions and propagates cancellation and
search exhaustion as distinct errors. Local method lookup in `TypeTable` maps the
catalog's selected implementation to a local function; it does not run another
recursive solver or substitute a fresh uncancellable context.

[TypeTable](../../crates/kagari-hir/src/typeck/table.rs) records interface
construction/upcast decisions and the exact applied interface/setter identity for
indexed writes. For `values[0] = next`, checking resolves Index's output and the
installed MutableList setter contract; lowering consumes that checked selection
without rediscovering members or storage capabilities. Builtin Array element context
and mutation stay independent. Registered readonly Map/Set occurrences describe
host/storage types only; trait signatures carry no collection-access field.

## Optional source-tool facts

`FileAnalysis::body_eligibility` returns positive evidence owned by the body checker,
not a judgment based on whether a retained diagnostic happens to overlap a function.
The checker records completed function IDs before diagnostic truncation. Restored
body facts must carry that evidence and satisfy the existing environment/arena
mapping contracts. Parser, declaration/import and shared-signature validity is
computed before bodies and travels with each retained file analysis. An invalid
shared snapshot disables semantic optional queries conservatively; a local body
error does not invalidate another successfully checked body. This evidence never
authorizes compilation: `AnalysisSnapshot::check_program` retains the strict gate.

`FileAnalysis::binding_usage` walks eligible resolved bindings, expressions, places
and closure captures once. Shadowed bindings keep distinct declaration identities;
member/index writes read the receiver without rebinding its slot, while direct and
compound assignments count slot writes. Rule consumers decide whether those facts
warrant a diagnostic. HIR does not own rule names, severity or suppressions.

`FileAnalysis::scoped_attributes` exposes uninterpreted declaration spans and logical
child-module identities. An attribute on `mod child;` therefore identifies the same
module as an external `child.kgr`, rather than relying on physical range containment.
Inline child contents belong to their own analysis. The linter owns supported policy
targets and argument/ID validation; ordinary language checking preserves tool metadata.
