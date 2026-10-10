# Reading HIR Data and Analysis

`kagari-hir` owns source meaning, recoverable analysis and editor queries. It
contains more than the `hir/` node model: lowering, import/name resolution, type
checking and snapshot publication have separate owners. The
[architecture overview](../architecture.md#compilation-pipeline) describes the
compiler handoff; the [execution plan](../hir-documentation-plan.md) tracks
documentation completion. Detailed contracts and diagrams live beside the code.

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
| `DefinitionId` | Interpreted by its matching definition table; distinct from all lowering-local IDs. Portable authoring metadata uses `DefinitionPath`. |

Cloning/reusing one lowering retains its arena; fresh lowering creates a new one.
`SourceUnit` additionally records logical module, source file and revision. Logical
module identity is not the same as a physical filename. See
[source maps](../../crates/kagari-hir/src/source_map.rs) for byte ranges and
specialized name/member/path sites; synthetic nodes may have empty ranges.

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
| Trait's `fn read(self) -> Self::Item;` | `TraitMethod { has_default: false, id: m, name: "read", receiver: Value, function: f_trait }` | f_trait selects Function with kind TraitMethod, a self parameter, result syntax and no body. |
| Impl's method body | `ImplMethod { name: "read", function: f_impl }` | f_impl selects Function with kind ImplMethod; its body links to a block whose tail is a Field expression for self.value. |

The omitted impl constant uses the checked trait default; it does not create an
extra impl AssociatedConst record. The expanded impl/trait header, generic family
and where-clause examples in behavior.rs explain the remaining fields. An
associated type's optional assigned syntax does not enable trait type defaults:
current checking rejects them. Likewise, `Method`/`MethodOwner`/`Module.methods`
and the ADT member-link vectors are documented as unpopulated; current methods
belong to trait/impl collections. Review candidates remain in [the review](../review.md),
and documenting their current state does not activate their removal.

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
Stmt(s) = Binding { local: l, name: "y", initializer: sum, ... }
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
old snapshots retain their immutable catalog. The
[execution plan](../name-resolution-plan.md#implementation-refinements) details
closure, proof ownership and work accounting.

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
| Unchanged source and dependencies | A file result can retain its `Arc` when revision/arena, host revision, imports, reachable namespaces, imported types/functions and aggregate facts match. |
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
[host declarations](../../crates/kagari-hir/src/host.rs),
[native declaration import](../../crates/kagari-hir/src/native/api.rs),
[generated source views](../../crates/kagari-hir/src/native/render.rs),
[validated language roles](../../crates/kagari-hir/src/language/items.rs) and
[builtin bridges](../../crates/kagari-hir/src/builtin/mod.rs). The runnable semantic
examples install `kagari_stdlib::catalog::shared()` explicitly, as required by the
current foundation contract.

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
