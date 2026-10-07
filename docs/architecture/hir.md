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

math NameTable["sum"].strong -> Declaration(S), target Source(S), public
api  directive              -> Glob, resolved Namespace(math)
api  NameTable["sum"].globs  -> GlobImport(api_directive), target Source(S), public
app  directive              -> Named { alias: Some("add") }, resolved Source(S)
app  NameTable["add"].strong -> NamedImport(app_directive), target Source(S)
```

Each candidate also retains its owner, visibility and source site. Provenance says
which declaration/import introduced it; the target says where the declaration
actually lives. A glob's directive targets a namespace, while its contributed
bindings target eligible members. Public re-exports become candidates with public
visibility in that module's table; the catalog filters access for each importer.
Glob expansion is stored in these namespace candidates. Lowered `Module.exports`
retains explicit declaration/named-use exports; it does not contain expanded glob
members.

[NameTable](../../crates/kagari-hir/src/resolver/table.rs) selects the first nonempty
tier: strong, glob, then implicit. Conflicting strong entries are ambiguous;
equal-target glob candidates can coexist with their origins preserved. These are
precedence tiers, not separate type/value namespaces. See
[import records](../../crates/kagari-hir/src/imports/mod.rs) for the concrete fields.

The builder currently scans all supplied lowered modules each pass. With `N`
distinct logical module identities its inclusive loop allows at most `2N + 2`
passes, stopping early when facts/catalog contents stabilize. It publishes the
last pass if the bound is exhausted; this is not a dependency work queue.

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
