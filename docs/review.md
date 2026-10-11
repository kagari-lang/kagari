# Repository review

Keep ongoing repository review findings in this document rather than creating
dated review files. Entries record observed behavior and possible follow-ups;
they do not activate implementation work.

SA1-SA3 were originally inspected against `c2f87c89`. SA2/SA3 and SA8 are now
implemented and locally accepted; their full CI acceptance remains pending.

## SA1 Inline modules are parsed again

[prepare_declarations](../crates/kagari-hir/src/analysis/declaration_queries.rs)
copies the enclosing source, masks bytes outside each inline module body and
parses that virtual file again. The parent AST already contains the module body;
[lower_module_decl](../crates/kagari-hir/src/lower/item.rs) only records its header.
This repeats lexing/parsing and allocates a full source buffer per child.

Consider lowering existing `ModuleBlock::items()` with an independent child
context. Preserve module ownership, visibility, stable child identities, original
UTF-8/CRLF offsets and diagnostic attribution. Reuse the existing inline-module
navigation tests. Keep declaration, signature and body checking separate.

## SA2 Import resolution rescans every module

Implemented in NR04 and locally accepted in NR05 under the
[name-resolution plan](name-resolution-plan.md). Full CI acceptance is pending.

The old builder rescanned all modules after each propagation step. The
[solver](../crates/kagari-hir/src/imports/solver.rs) now seeds declarations once,
replaces affected module contributions and schedules reverse namespace observers.
Observations include absent/pending names, namespace prefixes and glob membership;
watchers remain distinct from linking dependencies. New revisions rebuild from
current inputs, preserving removal, retargeting, access and ambiguity changes.

On the bounded 48-module named chain, module visits fell from 2,304 to 86. Adding
96 unrelated modules now adds 96 visits (182 total), compared with 6,912 total
visits previously. Diamond and removed-seed workloads show small timing regressions;
closure/proof overhead and measurement limits are documented in the
[measurement ledger](implementation-roadmap.md#name-resolution-sa8-sa2-sa3-ci-pending).
This demonstrates reduced unrelated rescan work, not universal speedup.

## SA3 Iteration exhaustion is not distinguished from convergence

Implemented in NR04; focused failure/publication acceptance passes. Full CI remains
pending. The old `2N + 2` loop could publish its last draft without convergence.

The solver now distinguishes cancellation, work exhaustion and non-convergence.
Queue exhaustion triggers pending-dependency closure; successful publication also
requires settled directives and binding tiers. Category-qualified acyclic proofs
prevent a named/glob cycle from manufacturing an order-dependent target. An exact,
eight-state history detects repetition and a separate input-sized work bound
limits remaining work. No mathematical completeness claim is made for that bound.

The core failure fixture forces both work limits, exercises a non-convergent
cycle and cancellation, checks old cache/snapshot retention and then successful
recovery. Existing cycle tests cover seeded circulation, unseeded unresolved
aliases, cross-category dependencies and source input order. Failures propagate
through HIR and SDK preparation; no partial graph is admitted to checked execution.

## SA4 Import records also represent namespace lookup state

Resolved by the [IR01-IR03 execution plan](import-resolution-plan.md).
Validation and phase commits are recorded in the
[roadmap ledger](implementation-roadmap.md#import-and-namespace-resolution-ir01-ir03-complete).

The previous `ResolvedImport` model combined named bindings, glob roots, module
headers, package/prelude bindings and empty-alias namespace entries. Import-vector
positions served as declaration and namespace identities.

The implementation now separates real directives and their source provenance from
named, tiered candidates and snapshot-qualified targets. A shared
[namespace catalog](../crates/kagari-hir/src/imports/catalog.rs) resolves nested
members with importer visibility and explicit lookup states. Qualified source
units include arena identity; aliases/re-exports share canonical declarations.
Imported signature consumers, compiler handoff, navigation, dependencies and
snapshot reuse use the new boundary. Deep aliases, grouped paths, strong/glob
precedence, unused imports, stale arenas, retained snapshots and source-free/native
execution are covered by behavioral acceptance. See
[implemented ownership](architecture.md#source-namespaces-and-import-ownership).

SA1 remains separate and unactivated. SA2/SA3 were subsequently implemented by
NR04; their bounded measurements and CI status are recorded above.

## SA5 Incremental reuse misses transitive namespace changes (P1)

Resolved; validation is recorded in the [SA5 follow-up ledger](implementation-roadmap.md#import-and-namespace-resolution-ir01-ir03-complete).

Reproduced against `c3294fad`. Signature reuse in
[check_signatures](../crates/kagari-hir/src/lib.rs), full-file/body reuse in
[snapshot](../crates/kagari-hir/src/analysis/mod.rs) and function-body reuse in
[body](../crates/kagari-hir/src/analysis/body_queries.rs) previously compared local
import facts and canonical imported contracts without comparing reachable namespace
bindings. Declaration reuse already performed that comparison, but later reuse
could still retain stale signatures, name resolution or checked body facts.

Setup: `library` declares the items; `exports` publicly aliases them; unchanged
`facade` contains `pub use pkg::exports::*;`; `root` imports
`use pkg::facade as m;`. Change only the aliases in `exports`:

- Rename `value as old` to `value as new`: incremental analysis still accepts
  `m::old()` and retains its navigation target; fresh analysis reports `UnknownName`.
- Swap `number as selected, flag as spare` to `flag as selected, number as spare`,
  where returns are `i32` and `bool`. Add a comment to `root` to exercise body
  remapping: incremental analysis reuses the body of
  `fn main() -> i32 { m::selected() }` and misses the return-type error.
- Swap `A as Selected, B as Spare` to `B as Selected, A as Spare`: incremental
  signature analysis still types `fn accept(x: m::Selected) {}` as `A`; fresh
  analysis returns `B`.

All affected reuse paths now compare reachable namespace bindings. Retaining a
complete file requires exact tables; reusing checked facts permits only the current
module's arena rebasing through the existing remappers, including validated local
variant owner/slot identities. Foreign namespace targets retain complete identities.
[Five regression tests](../crates/kagari-hir/src/imports/cache_tests.rs) cover the
three reproductions, single-function reuse, old snapshots, unrelated file sharing
and local nominal namespace remapping. All five tests pass, as do the HIR suite,
workspace tests completed in segments and the source-free/native feature matrix.

## SA6 Generated cache names differed from analyzed paths on Windows

Resolved at the SDK's [cache publication boundary](../crates/kagari-embed/src/engine/declarations.rs).
Windows canonicalization produced `\\?\F:\...`, which was published verbatim while
the source database normalized separators and drive spelling. The analyzed name
lost the usable drive prefix and differed from the published declaration source.
Four existing `registration_sources` tests failed their absolute-path assertion.

Publication now converts canonical drive paths to ordinary paths and applies the
same source-name normalization as analysis. The unchanged six-test suite passes,
including actual file reads, matching source views, content validation and retained
snapshot navigation. This bounded integration correction preserves all assertions.

## SA7 Routine fixes trigger expensive aggregate test suites

SA5's five focused regressions executed in 1.07s, while unrelated aggregate
language-contract, syntax-example and VM suites each took several minutes in that
validation run. These are observed suite timings, not isolated benchmarks.
Some runners serialize many scenarios inside one test and repeatedly create fresh
analysis/standard-library environments; Cargo cannot schedule those cases separately.

Routine follow-ups now use focused validation under [AGENTS.md](../AGENTS.md).
The [numeric fixture follow-up](implementation-roadmap.md#focused-test-harness-optimization-sa7-numeric-fixtures-complete)
retains all 90 cases and both artifact routes, batches 90 compilations into 4,
and installs/loads 8 runtimes instead of 180. Only scalar locals are shared within
each route; a successful call after every case checks trap cleanup. The four
numeric tests pass in an observed 4.51s; timing conditions and limitations are in
the ledger. Source-free validation and route isolation remain covered.

SDK/other aggregate runners and selectively executing their individual cases
remain separate follow-ups. Reuse immutable setup where safe and preserve tests
that specifically require fresh snapshots or independent mutable runtimes.

## SA8 Separate type/value lookup and unify export information

Implemented by NR01-NR03 and locally accepted with the NR04 solver integration;
full CI acceptance remains pending. The [execution plan](name-resolution-plan.md)
and [roadmap ledger](implementation-roadmap.md#name-resolution-sa8-sa2-sa3-ci-pending)
retain contracts, measurements and phase validation.

Name tables, directive outcomes, host queries and semantic lookup now select Type
or Value explicitly, with independent conflicts and `strong > glob > implicit`
precedence. Prefixes select Type; terminal use sites select their syntactic category.
One ordinary use leaf can introduce both bindings. Dual imports expose both tooling
targets, and a single-target query does not choose an arbitrary winner. Cache
comparison/remapping preserves both categories and qualified declaration ownership.

HIR `Module.exports`, `Export` and `ExportItem` were removed. Resolved bindings
provide public namespace members, including glob re-exports. Portable authored
`ModuleDecl.exports` uses category-aware `ExportName` keys; generated native aliases
retain their declared category and independent native ABI symbol validation.

Existing language/import/native/query/cache fixtures were consolidated around
these contracts. Focused SDK validation covers direct and encoded source-free
execution; the selected Cranelift preparation used `InterpreterFallback`. One
new normal test covers solver failure/publication as a distinct core boundary.
Macros, new constructors, SA1 and SA9 remain outside this implementation.

## SA9 Module paths are copied into internal graph keys and references

[ModuleIdentity](../crates/kagari-common/src/identity.rs) is a portable package/path
value containing owned strings. It is also embedded in `SourceUnit`, binding owners,
namespace keys and dependency sets. Cloning these identities copies strings; hashing
and ordering examine path content. NR04 removed whole-catalog rounds, but retained
portable keys in namespace watchers and derivation sets. This establishes remaining
representation overhead, not a measured bottleneck.

Separate portable module identity from a compact, context-owned module handle.
Intern each identity once; use handles for catalog keys, owners and dependencies,
and borrow the portable identity for diagnostics, registration and artifact boundaries.
The existing [definition table](../crates/kagari-common/src/identity/table.rs) already
interns module roots and shares their paths; investigate extending/reusing that
owner before introducing a second interner. Do not reuse HIR `ModuleId`: it addresses
a local module declaration, not a module in the analysis universe.

The design must define handle ownership and retained-snapshot validity, remap across
independent contexts, retain source revision/arena checks, and preserve deterministic
artifact ordering independently of allocation order. Parent/package/visibility checks
need table queries in place of editing copied path vectors. `Arc<ModuleIdentity>`
alone reduces clone costs but still compares/hashes content with ordinary traits.

Rust reference:

- The resolver's [`Module<'ra>`](https://doc.rust-lang.org/stable/nightly-rustc/rustc_resolve/struct.Module.html)
  wraps `Interned<'ra, ModuleData<'ra>>`: a `Copy` handle with identity-based
  equality, while module data is stored separately. It does not copy a full
  string path into every module reference.
- Definitions, including modules, use [`DefId`](https://doc.rust-lang.org/stable/nightly-rustc/rustc_hir/def_id/struct.DefId.html),
  a `Copy` pair of `CrateNum` and `DefIndex`, for compiler queries.
- Session-local numeric IDs can change after source edits. The
  [incremental cache](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html)
  stores `DefPathHash` and remaps it to the current session's `DefId` when loading.

Apply the same separation in Kagari: portable `ModuleIdentity` at boundaries,
shared identity data in the context, compact handles inside the graph. This does
not require introducing Rust's crate model or adopting its arena lifetimes.

Keep this a separate follow-up from SA8 and SA2/SA3. Measure graph build allocations
and time before/after on the same module workload, separating compilation time;
reuse identity/snapshot and import tests for focused validation. Code migration is
not activated by this review.

## SA10 Generated trait declarations hide native default bodies

Resolved by [ND01-ND04](native-default-bodies-plan.md): generated traits own real
forwarding bodies, parsed `has_default` must agree with registration, and source
compilation consumes checked calls. SDK preparation checks unused defaults before
publishing their source files. Private native helpers and source-free validated
recipes remain; direct dispatch adds no script frame. Broader CI acceptance is
reported separately in the plan.

## SA11 HIR declarations and ADT members carry container-derived identities

[Struct](../crates/kagari-hir/src/hir/item/adt.rs) stores its own `StructId`,
although that ID also identifies its position in `Module.structs`. Consumers such
as [signature checking](../crates/kagari-hir/src/typeck/check.rs) iterate over
`&Struct` and use `structure.id` to query semantic declarations and constraints.
[Item lowering](../crates/kagari-hir/src/lower/item.rs) also uses the stored ID to
publish `Item::Struct` before appending the declaration row. These are concrete
convenience uses, not evidence that the identity must live inside the payload.

All five ADT records store a self ID, but their reconstruction contexts differ:

| Record | Stored identity | Container-derived context |
| --- | --- | --- |
| `OpaqueType` | `OpaqueTypeId` | Index in `Module.opaque_types`. |
| `Struct` | `StructId` | Index in `Module.structs`. |
| `Enum` | `EnumId` | Index in `Module.enums`. |
| `Field` | `FieldId` | Arena, enclosing `StructId` and field slot. |
| `Variant` | `VariantId` | Arena, enclosing `EnumId` and variant slot. |

The first three duplicate their declaration-vector index. Fields and variants
are nested records: a slot alone cannot identify a member across owners. Their
IDs could also be reconstructed by a container, but only with the full arena,
owner and slot context. Review these two categories separately, including their
source-map and semantic-table consumers; this observation does not establish
that any of the five fields should be removed.

[Function](../crates/kagari-hir/src/hir/item/function.rs) has the same
container-derived self identity as the top-level ADT declarations above:
`module.functions[index].id.index() == index`. The
[source map](../crates/kagari-hir/src/source_map.rs) allocates `FunctionId` from
its function-span vector, and current item/method lowering appends function
records in the corresponding order. This applies to free functions and the
trait/impl methods stored in the same collection.

The field has active consumers: lowering publishes `Item::Function` and links
trait/impl method records; signature checking uses `function.id` to index typed
functions and source locations. It is a redundant identity with convenience
uses, not an unused declaration model like the registry discussed in SA13.
Review retaining the field versus supplying `(FunctionId, &Function)` from the
container and passing the identity explicitly to consumers. `FunctionId` itself
must remain available for method links, name resolution, source maps and
`BodyOwner::Function`; only its duplication inside `Function` is under review.
Any change must preserve allocation/storage order and snapshot identity mapping.
Future local function support (SA14) must also preserve that relationship if
parent and child declarations are allocated and appended at different times.

Expression storage uses a different interface:
[Body::expressions](../crates/kagari-hir/src/hir/body.rs) yields `(ExprId,
&ExprData)`, reconstructing each ID from the container's arena, the row's owner
and its index. `ExprData` contains no self ID. Both declarations and expressions
have identities; the difference is where that identity is supplied to consumers.

For structs, the duplicated invariant is
`module.structs[index].id.index() == index`, with matching source-map slots.
Reordering rows or changing the stored ID independently can break those links.
No current invariant violation or measured performance bottleneck was established
by this review. `StructId` is lowering-local and carries only an index; retaining
the field does not make it stable across modules, revisions or snapshots.

Follow-up owner: HIR declaration storage and its consumers. Compare retaining
self IDs with container-owned iteration returning `(ID, &record)` for the five
ADT records and `Function` above; review other declarations only where the same
issue applies.
The identity interface and any field removals remain undecided.
Preserve item order, source-map alignment, member ownership, semantic declaration
mapping and snapshot/reuse remapping. Do not conflate inline binding/member IDs
with redundant self IDs, or widen visibility/add forwarding APIs to hide the
ownership question. Reuse existing declaration, navigation and snapshot contracts
if implementation is activated; measure any claimed memory or speed benefit.
This entry records a design review only and does not activate a migration.

## SA12 HIR receiver category records only one language model

[ReceiverKind](../crates/kagari-hir/src/hir/item/behavior.rs) has only `Value`.
Both `Method` and `TraitMethod` carry a `receiver: ReceiverKind` field;
[trait-method lowering](../crates/kagari-hir/src/lower/item.rs) always assigns
`ReceiverKind::Value`. The inspected handwritten consumers do not select behavior
based on this category. The enum and fields therefore record a constant policy,
not a current distinction between receiver forms.

The [receiver contract](spec/traits.md#receiver-model) provides only `self` and
uses the ordinary parameter value model, without Rust-style `&self` or `&mut self`
receivers. The [value contract](spec/value-semantics.md#values-and-identity)
distinguishes value semantics from shared object identity: scalars, strings,
tuples and enums have value semantics; tuple/enum members retain their own
semantics. Structs, arrays, maps and sets share identity when passed. GC-backed
storage alone does not imply mutable identity semantics or a separate receiver
category. Passing `self` neither deep-copies a shared object nor introduces
exclusive ownership transfer.

Follow-up owner: HIR method surfaces and lowering. Under the current language
contract, the enum and both fields are candidates for removal together with their
imports and constant initialization; no replacement marker or speculative receiver
variants are needed. Preserve receiver parameters, method selection, ordinary
argument semantics and writeability checks. Host-boundary passing styles and
scoped borrow validation have separate responsibilities; retain
[HostPassingStyle](../crates/kagari-types/src/host_interface/type_declaration.rs)
and its checked consumers.

Reuse existing method/trait and host-boundary contracts for focused validation
if removal is activated. No correctness failure or measured performance benefit
was established here. This entry records the simplification candidate only;
implementation remains deferred.

## SA13 Unused HIR method registry duplicates the active ownership model

The unified method registry in [behavior.rs](../crates/kagari-hir/src/hir/item/behavior.rs)
provides `MethodOwner`, `Method` and `MethodBuffer`, with `MethodId` in
[ids.rs](../crates/kagari-hir/src/hir/ids.rs). [Module.methods](../crates/kagari-hir/src/hir/item/mod.rs)
and [Struct.methods / Enum.methods](../crates/kagari-hir/src/hir/item/adt.rs)
expose storage and links for that registry. Repository inspection found no
construction or consumption of these HIR method records; normal
[item lowering](../crates/kagari-hir/src/lower/item.rs) leaves both ADT method
lists empty and does not populate the module registry.

The active model already assigns methods to traits and impls. For example,
`impl Example { fn run(self) { ... } }` lowers to an `Impl` containing an
`ImplMethod { name, function }`; its `FunctionId` identifies the signature and
body in `Module.functions`. Trait declarations similarly contain `TraitMethod`
records linked to functions, including default bodies. This is the ownership
model to preserve, including method lookup and trait implementation checking.

This division of responsibilities resembles rustc HIR: its
[ItemKind](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_hir/hir/enum.ItemKind.html)
separates struct, trait and impl declarations, and an
[Impl](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_hir/hir/struct.Impl.html)
owns associated-item IDs together with its target type and optional trait.
Kagari currently uses inline trait/impl method records plus `FunctionId`, so the
storage representations differ. The reason to consider removal is the unused
parallel model and existing responsibility owner; similarity to Rust is only a
design reference.

Follow-up owner: HIR method storage and lowering. Consider removing
`MethodOwner`, `Method`, `MethodBuffer`, `MethodId`, `Module.methods`,
`Struct.methods` and `Enum.methods` together with their imports and initializers.
Retain `TraitMethod`, `TraitMethodId`, `ImplMethod`, the active trait/impl method
collections and their `FunctionId` links. Receiver-category simplification is
tracked in SA12; removing the unused `Method` record would also remove its
receiver field. Other crates' runtime/host method models are outside this scope.

Before activating cleanup, reconfirm consumers and update affected documentation;
use focused existing method/trait contracts and relevant compile/structure checks.
No behavior defect or measured performance benefit was established here. This
entry records a future cleanup candidate only; implementation remains deferred.

## SA14 Function bodies cannot declare named local functions

Named functions cannot currently be declared inside another function's body.
For example, the following requested source form is rejected:

```kagari
fn outer() -> i32 {
    fn inner(x: i32) -> i32 { x + 1 }
    inner(41)
}
```

[Block parsing](../crates/kagari-syntax/src/parser/grammar/stmt.rs) accepts
bindings, control flow and expressions, but has no function-declaration branch.
[HIR statements](../crates/kagari-hir/src/hir/stmt.rs) and
[statement lowering](../crates/kagari-hir/src/lower/stmt.rs) likewise have no
local function declaration form. Both the current
[syntax specification](spec/syntax.md#blocks-and-statements) and
[EBNF](kagari.ebnf) exclude function declarations from block statements, so this
is a missing language capability rather than an implementation violation of the
current grammar.

A closure binding such as `val inner = |x: i32| x + 1;` is supported, including
lexical captures, but does not provide the requested named declaration form.
The user requests that named local function support be added in later work.

Follow-up owner: language syntax, HIR declaration ownership and lexical name
resolution. Define block visibility, forward references, recursion, shadowing
and access to enclosing locals/generic parameters before implementation; capture
behavior must be specified explicitly rather than inferred from closures.
Extend the grammar, AST, HIR/lowering and declaration/resolution model together,
then carry checked local callable identities through typing, executable lowering
and tooling. Local declarations must not leak into module exports. Review whether
`FunctionKind` and function ownership need changes instead of automatically treating
local declarations as module-level `User` functions.

When activated, validate the example through execution and add focused coverage
for the chosen scope, recursion and capture rules, including diagnostics and
navigation. Preserve existing module functions, methods and closure contracts.
This entry records the requested future feature only; semantic choices and
implementation remain deferred, with no change to the currently accepted grammar.

## SA15 HIR import paths should retain segments instead of joined text

The requested follow-up direction is to replace
[`Import.path: String`](../crates/kagari-hir/src/hir/item/module.rs) with owned
`segments: Vec<String>`. Implementation remains deferred. For example:

```kagari
use pkg::math::sum as add;
```

```text
Current: Import { path: "pkg::math::sum", kind: Named { alias: Some("add") }, ... }
Proposed: Import { segments: ["pkg", "math", "sum"],
                   kind: Named { alias: Some("add") }, ... }
```

[Item lowering](../crates/kagari-hir/src/lower/item.rs) currently joins syntax
segments and stores each accumulated prefix with its physical source site in
[SourceMap](../crates/kagari-hir/src/source_map.rs). Import analysis copies the
joined path into `ImportDirective`. Its consumers primarily need structure:
[normalization](../crates/kagari-hir/src/imports/resolve.rs) splits relative paths
to process `self`, `super` and `crate`; the
[namespace catalog](../crates/kagari-hir/src/imports/catalog.rs) splits paths for
package/module/member lookup and dependency-prefix matching. Prefix navigation
resolves `pkg`, `pkg::math` and `pkg::math::sum` separately. Diagnostics and host
lookup interfaces also consume complete path text. `ModuleIdentity.path` already
uses `Vec<String>`; it is a resolved module identity, distinct from written import
syntax.

Follow-up owner: HIR import syntax/lowering and import analysis. Carry segments
through the affected `ImportDirective`, normalization, catalog traversal and
prefix-dependency consumers, with text rendering at diagnostics or existing
text-based host interfaces. Update `local_name()` to use the terminal segment
when no alias is written. Adapt source-map prefix sites and navigation together,
retaining their source-map ownership and exact physical spans. Avoid introducing
a second complete-path field merely to preserve the former internal API.
Preserve flattened grouped imports, aliases, glob paths excluding the final `*`,
relative-path behavior, namespace-specific resolution, visibility checks,
canonical targets, dependencies and retained-snapshot validity. An omitted alias
and a glob must keep their existing local-name behavior.

Rust's HIR provides a reference for structured paths:
[`Path`](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_hir/hir/struct.Path.html)
stores a slice of path segments, a span and resolution information; each
[`PathSegment`](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_hir/hir/struct.PathSegment.html)
retains its identifier and other segment facts. Kagari should keep resolution
facts under its existing analysis owners rather than copying Rust's entire model.
Owned `Vec<String>` needs no lifetime parameter. Rust's `'hir` describes borrowed
HIR storage; its interned `Symbol` is an index without a lifetime parameter.
Symbol interning is a separate decision and is not part of this follow-up.
The portable module-identity handle review in SA9 remains separate.

When activated, reuse existing import, relative-path, grouped/alias/glob,
visibility, dependency and navigation contracts for focused validation. Review
all affected callers together and update source-to-field documentation. The
motivation is to retain structure used by current consumers and reduce conversion
steps; no correctness failure or measured performance bottleneck was established.
Measure allocations/time before claiming a memory or speed improvement. This
entry records the future representation change only and does not activate code
migration.

## SA16 Struct patterns should require explicit omission with `..`

The requested follow-up is to align named-field struct patterns with
[Rust's field-completeness rule](https://doc.rust-lang.org/reference/patterns.html#struct-patterns):
without `..`, every field must be listed; with a terminal `..`, unlisted fields
are ignored. For a `Point` with fields `x` and `y`:

| Pattern | Current Kagari behavior | Requested behavior |
| --- | --- | --- |
| `Point { x: n, y: _ }` | Accepted; bind `x`, ignore `y`. | Preserve. |
| `Point { x: n, .. }` | Rejected by parsing. | Accept; bind `x`, ignore remaining fields. |
| `Point { x: n }` | Accepted; unlisted fields are implicitly ignored. | Reject because `y` is omitted without `..`. |
| `Point { .. }` | Rejected by parsing. | Accept; ignore all fields. |

[Struct-pattern parsing](../crates/kagari-syntax/src/parser/grammar/expr.rs)
and the [EBNF](kagari.ebnf) accept only named fields or named subpatterns.
The [AST view](../crates/kagari-syntax/src/ast/expr.rs),
[pattern lowering](../crates/kagari-hir/src/lower/expr/pattern.rs) and
[`PatternKind::Struct`](../crates/kagari-hir/src/hir/pattern.rs) have no rest marker.
[Pattern checking](../crates/kagari-hir/src/typeck/body/patterns.rs) validates
listed fields and subpatterns but does not check for omitted fields. This is a
requested syntax and checking change, not just an additional spelling of the
currently accepted pattern.

Follow-up owner: syntax/AST, HIR pattern lowering and pattern checking. Accept
at most one `..`, at the end of a named-field struct pattern; retain its presence
in HIR, for example with `has_rest: bool`. The exact field name and source-site
representation can be chosen during implementation. The marker belongs to the
whole struct pattern, not to `PatternField` and not to a synthesized wildcard
subpattern. It may ignore zero remaining fields. Without the marker, diagnose
missing fields against the resolved struct declaration. Update the EBNF,
syntax specification and source-to-field documentation together.

Preserve shorthand `y` as `y: y`, nested subpatterns, existing unknown/duplicate
field diagnostics, field-access rules, binding types and checked field identities.
The rest marker creates no bindings or extra field reads. Carry checked pattern
facts through existing analysis/executable lowering and tooling consumers as
needed. This scope does not adopt Rust borrowing, moves or unrelated slice/tuple
rest patterns.

When activated, reuse existing pattern contracts and add focused coverage where
absent for complete fields, explicit omission, missing-field diagnostics, empty
and nested rest patterns, and malformed duplicate/nonterminal markers. Check
accepted examples through executable lowering as well as analysis. This entry
records the future rule alignment only; implementation remains deferred.

## SA17 Ordinary `val`/`var` declarations should support destructuring patterns

The agreed follow-up design generalizes a local declaration to
`val/var pattern [: Type] = expression;`. Ordinary declarations currently accept
only one identifier: [binding parsing](../crates/kagari-syntax/src/parser/grammar/stmt.rs)
reads a name, and [`StmtKind::Binding`](../crates/kagari-hir/src/hir/stmt.rs)
retains one `name`/`LocalId`. Patterns already exist for match arms, loops and
binding conditions, but this does not provide ordinary destructuring declarations.

Proposed source forms (not currently accepted as ordinary declarations):

```kagari
val (x, y) = pair;
var Point { x, y } = point;
val Point { x: n, .. } = point;
val (id, Point { x, .. }) = result;
val (x, y): (i32, String) = make_pair();
```

Use one pattern-based binding model for simple and destructuring declarations:

```text
Binding {
    pattern: PatternId,
    writeability: Writeability,
    ty: Option<TypeRefId>,
    initializer: ExprId,
}

val Point { x: n, .. } = make_point();
statement -> Binding { pattern: p0, writeability: Val, ty: None, initializer: call }
p0 -> Struct { path: "Point", fields: [PatternField { name: "x", pattern: p1 }],
               has_rest: true } // illustrative rest-marker name; see SA16
p1 -> Name { name: "n", local: n_local }
```

The leaf patterns retain each binding's name and `LocalId`; replace the statement's
single-name fields rather than keeping a parallel simple-binding implementation.
`val n = 1;` becomes the same binding record with a `Name` pattern. Bind names
according to the declaration context and preserve constructor resolution where
appropriate; do not treat every provisional pattern name as an established binding
before resolution and checking.

The initial contract is:

- The outer `val` or `var` applies to every local introduced by the pattern.
  Mixed per-leaf writeability is outside this scope. `var` permits rebinding the
  extracted locals, not assignment back into the original object's fields.
- A type annotation describes the complete initializer value; member types
  determine the leaf binding types. Require an initializer for destructuring.
- Ordinary declarations require an irrefutable pattern after type and constructor
  checking. Reuse/review existing pattern classification rather than introducing
  runtime match-failure traps. Reject refutable literal/variant subpatterns;
  existing `if val`/`while val` and `match` handle conditional matching. A new
  declaration `else` form is outside this scope.
- Evaluate the initializer exactly once in the existing scope, then extract
  required members in pattern source order. `_` and `..` create no bindings and
  cause no extra reads of ignored members. New locals become visible after the
  declaration; `val (x, y) = (x + 1, 3);` uses the previous `x` on the RHS when
  shadowing an existing binding. Retain completed effects and existing trap/root
  cleanup if initialization or a required host read fails.
- Preserve Kagari's value and shared-object identity semantics. Binding an integer
  copies its value; binding a shared object preserves identity. `val` does not
  freeze objects, and destructuring adds no Rust move/borrow or `ref` semantics.
- Apply SA16's explicit `..` and field-completeness rules recursively to struct
  subpatterns. Preserve field access and unknown/duplicate-field checks, and
  reject duplicate local names within a single declaration.

The structural model, whole-value annotation and ordinary irrefutability rule are
similar to [Rust let statements](https://doc.rust-lang.org/reference/statements.html#let-statements).
Rust allows per-binding `mut`; the proposed Kagari form applies `val`/`var` to the
whole declaration and keeps its existing GC-backed value model.

Follow-up owner: binding syntax/AST, HIR lowering, lexical resolution and typing,
then checked executable lowering and tooling. Update the EBNF/specification and
model examples together. Reuse pattern traversal/type facts, assign declared
writeability to every bound local, preserve source-map identity/navigation and
closure capture behavior, and carry checked member identities to executable
lowering. Backends must not resolve source names or infer the pattern again.

When activated, reuse existing pattern/value/assignment/scope contracts and add
focused missing coverage for nested tuple/struct binding, annotations, both
writeability forms, refutability diagnostics, duplicate names, shadowing and
once-only initializer effects. Validate shared identity, ignored fields and
checked host reads where applicable through execution, with navigation/capture
coverage at their existing owners. Coordinate with SA16 without expanding into
slice patterns, tuple rest syntax or unrelated pattern features. This entry records
the agreed future design only; implementation remains deferred.

## SA18 Collection operations should be constrained by trait interfaces

The requested follow-up direction uses a Kotlin-like interface/object model:
trait members and checked implementation/inheritance relationships determine
available operations. A shared object's storage does not acquire a separate
mutable/immutable state when viewed through another interface. Preserve binding
and field `val`/`var` assignment rules, which have a different responsibility.
The current [collection contract](spec/collection-access.md) already separates
List/MutableList, Map/MutableMap and Set/MutableSet and permits shared aliases.
The concern is the extra semantic axis and unrelated rules attached to it.

For an installed Vec implementation, the intended interface model is shown
below. The constructor follows SA20's agreed future API; its implementation is
deferred together with the fixed-length builtin Array migration.

```kagari
val concrete = Vec::from([1, 2]);
val writable: MutableList<i32> = concrete;
val readable: List<i32> = writable;
writable.push(3);
readable.len(); // 3: both interfaces retain the same underlying object.
```

`List<T>` exposes read operations; `MutableList<T>: List<T>` adds write operations.
Calling `readable.push(...)` or assigning an element through List must fail because
the static interface lacks the required operation. Interface conversion retains
the object; it does not copy, freeze or snapshot storage. Other aliases can mutate
it, and referenced elements retain their own access rules. A readonly API is not a
purity proof. Native and script implementations should use the same ordinary
interface checking and dispatch model.

[`TraitBuilder::storage_view`](../crates/kagari-runtime/src/native/builder/trait_builder.rs)
currently sets [`TraitDef.storage_access`](../crates/kagari-types/src/declaration/mod.rs).
Inspection found that this field influences more than method availability:
implementation matching/receiver access weakening, common readonly branch types,
automatic Eq/Hash/Debug support, interface identity unwrapping, implementation
ownership and exact installation checks. The
[installation test](../crates/kagari-runtime/tests/installation_access.rs) constructs
a methodless marked trait and verifies those installation privileges even without
native calls. This coupling makes a read/write marker imply unrelated semantics.
It is not evidence of a current correctness failure or measured bottleneck.

Follow-up owner: shared types/declarations, HIR interface analysis, executable
contracts and runtime interface/storage boundaries. The target is to remove
`TraitDef.storage_access` and `TraitBuilder::storage_view` after moving each
consumer's guarantees to its semantic owner:

| Current responsibility | Intended owner/model |
| --- | --- |
| Available read/write operations and parent upcasts | Trait members, explicit implementations and ordinary interface inheritance. |
| Receiver matching and common branch types | General checked interface/type relationships; preserve valid existing cases and diagnose ambiguity without selecting by a storage tag. |
| Equality/hash/debug support | Explicit protocol selection and specified defaults, independent of read/write access. |
| Underlying identity through interface wrapping | Interface value representation and object identity semantics, retaining pinned method-table versions. |
| Native installation and implementation validity | Exact required declaration, entry, layout and witness validation at their existing source-free owners. |

An interface value conceptually retains its underlying value plus a checked method
witness/table. List and MutableList views of one Vec share the underlying object
while exposing different declared methods. The witness/table is executable
metadata, not a second mutability authority. Actual Rust storage still needs its
registered layout, tracing, ownership and scoped borrow validation; removing the
trait marker does not remove concrete native storage registration or runtime
integrity checks. Do not extend identity operations to scalar/tuple/enum values
merely because they are boxed, or to unrelated interfaces as an incidental change.

Review `CollectionAccess` in internal Array/Map/Set types as part of tracing the
same model, rather than retaining a parallel access system beneath ordinary
interfaces. SA20 retains a distinct builtin fixed-length Array and separates it
from Vec and the List interfaces; it does not require deleting that builtin type.
Choose representation changes only for affected concrete paths;
operators such as indexed assignment must still select and validate the required
write operation. Keep generic arguments invariant during this migration and
preserve the current collection identity-based equality/hashing and one-way
implicit parent conversions. General variance, structural collection equality,
new downcasts and a universal object superclass are separate language decisions.

Kotlin's [collection interfaces](https://kotlinlang.org/docs/collections-overview.html)
provide the reference for method surfaces, parent relationships and val/var
independence. Kotlin's List covariance and
[element-based equality](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/-list/)
differ from Kagari's current contract; this follow-up does not silently adopt them
or introduce Kotlin/JVM runtime representation requirements.

When activated, migrate the shared declarations, analysis, checked artifacts and
runtime consumers coherently and update specifications/model documentation. Reuse
focused contracts for read/write method and index access, shared alias visibility,
custom implementations, generic matching, common-interface inference, equality/
hash identity, interface dispatch and pinned reload. Adapt marker-specific tests
while retaining meaningful source-free rejection of forged declarations/layouts/
witnesses and host borrow/GC cleanup coverage. Measure any claimed memory or speed
benefit. This entry records the future architectural direction only; code migration
and specification changes remain deferred.

## SA19 Consider one definition-ID representation inside semantic models

The proposed follow-up is to simplify definition references by using
`DefinitionId` consistently inside semantic models and keeping path/wire
conversion at explicit boundaries. This is an evaluation direction, not a final
representation decision or an activated migration.

Currently, [DefinitionReference](../crates/kagari-common/src/identity/reference.rs)
is a sealed trait implemented by `DefinitionPath`, `DefinitionId` and
`PortableDefinitionRef`. Identity-bearing records such as
[TypeId and NominalType](../crates/kagari-hir/src/types.rs) are generic over that
representation. [CheckedAnalysis](../crates/kagari-hir/src/lib.rs) maps authoring
paths into scoped IDs; record mapping and portable encoding reuse the same shape.
This avoids duplicate record definitions, but propagates identity-representation
parameters through semantic APIs. The portable implementation cannot directly
describe or resolve itself through `DefinitionTable`; those operations return
errors until the dedicated decoder maps its references.

For a type application such as `Box<i32>`, the candidate model is:

```text
Authoring path for Box -> definition context -> DefinitionId for Box
Semantic type         -> declaration: DefinitionId, arguments: [Builtin(I32)]
Diagnostics/display   -> look up the definition's path through its owning table
Artifact boundary     -> encode/decode through a validated portable definition table
```

References to the same canonical declaration share its ID in one context. The
path remains useful for registration, debugging and exact cross-context import;
the proposal does not remove it. Source aliases must still resolve to canonical
definitions. Distinct contexts may assign different IDs to an equal path and
require explicit remapping. These IDs are not runtime generation/version IDs.

Rust provides a useful reference: [DefId](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/def_id/struct.DefId.html)
combines a crate number and definition index;
[DefKey](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_hir_id/definitions/struct.DefKey.html)
stores a parent and disambiguated path segment. Semantic
[ADT definitions](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/struct.AdtDefData.html)
retain a DefId, while [incremental compilation](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html)
maps stable path hashes to current-session IDs. This separation is the reference,
not a requirement to copy rustc's interner generics, hash identities or metadata
codec. Kagari's exact portable identities and pinned reload ownership remain
separate contracts.

Follow-up owner: common identity tables/mapping, semantic types/declarations,
analysis publication and checked artifact boundaries. Evaluate whether authoring
inputs can be adopted before semantic analysis so internal records no longer
need path/ID variants. Remove obsolete semantic representation parameters and
their mapping/projection machinery where this is viable; decide separately
whether boundary records still benefit from generic mapping. Language generic
parameters, type arguments and associated-type semantics are unaffected. Reuse
the existing definition context; do not create a parallel registry or merge HIR
arena IDs with semantic definition IDs. Coordinate module-handle changes with
SA9 rather than extending this review into that migration.

When activated, preserve owning-table and index validation, immutable snapshots,
append-only identity stability, canonical bounded artifact decoding, cancellation,
cross-context import and generation-pinned dependencies. Reuse identity,
alias/navigation, generic/associated-type and source-free artifact rejection
coverage. Assess adoption/projection consumers before deciding scope and measure
any claimed memory or speed improvement. Implementation remains deferred.

## SA20 Separate builtin fixed-length arrays from library lists

The requested future direction is to retain a builtin Array whose length cannot
change after construction. Array literals such as `[1, 2, 3]` should construct
that builtin type, not select mutable Vec storage. Array remains distinct from
List/MutableList and does not implicitly convert to or implement those interfaces.
Its supported core protocols should cover indexing and iteration, with exact
protocol membership and element-write behavior reviewed before implementation.
Fixed length does not imply immutable elements or a change to binding `val`/`var`.

The agreed collection construction direction uses associated functions on the
concrete implementation, following Rust-style naming. This supersedes the earlier
`list_of`/`mutable_list_of` and Set factory-name candidates; do not retain a second
factory surface as hypothetical compatibility. The initial API accepts one
builtin Array rather than requiring macros or variadic parameters:

| Entry | Result/responsibility |
| --- | --- |
| `Vec::new()` | Construct an empty concrete Vec. |
| `Vec::from(array)` | Construct an independent concrete Vec from the array's elements, retaining their order and duplicates. |
| `HashSet::new()` | Construct an empty concrete HashSet. |
| `HashSet::from(array)` | Construct an independent concrete HashSet and deduplicate according to checked element Eq/Hash. |

The following examples are future API sketches, not currently supported programs:

```text
val array = [1, 2, 3]; // builtin Array; length stays 3
val concrete = Vec::from(array);
val readable: List<i32> = Vec::from([1, 2, 3]);
val writable: MutableList<i32> = Vec::from([1, 2, 3]);
val unique = HashSet::from([1, 2, 2, 3]);
val readable_set: Set<i32> = HashSet::from([1, 2, 3]);
val empty: Vec<i32> = Vec::new();
```

Construction selects a concrete implementation; a type annotation or ordinary
checked upcast selects the exposed interface. List/MutableList and Set/MutableSet
remain operation contracts without constructors that choose default storage.
Constructing a Vec yields its concrete mutable API; annotating that value as List
does not freeze the object or create an object-level mutability flag. HashSet's
construction requirements are `T: Eq + Hash`, not requirements imposed on every
possible Set implementation. Its iteration order remains unspecified; adopting
Kotlin's ordered set factories is a separate decision.

`from(array)` is an explicit construction operation, not implicit Array/List
interoperability. It creates independent collection storage and leaves the input
Array usable. Copy scalar/value elements according to Kagari value semantics;
shared-object elements retain their identities without deep copying. Do not steal
or move the input Array as if Kagari had Rust ownership semantics. Array element
expressions evaluate left-to-right exactly once before the constructor executes;
Set deduplication must not suppress evaluation of duplicate input expressions.

These are ordinary library declarations and associated calls. Resolve `from`
through the applicable declaration/implementation model; the naming choice does
not authorize extra implicit conversions or compiler recognition of factory names.
General variadic parameters and spread syntax remain deferred. If later added,
evaluate lowering packed arguments through the same builtin Array construction
path and define existing-array spread alias/copy behavior explicitly.

Collection implementations and their constructor selection stay in library
declarations/implementations. Semantic analysis consumes checked declaration IDs,
generic arguments and trait facts; it should not need a dedicated Map/Set type
variant for every library implementation. Retain Array as an intrinsic semantic
type with its own contract. Review the current special Vec/HashMap/HashSet mapping
in [NativeTypeKind::apply](../crates/kagari-hir/src/native.rs) coherently with SA18;
do not rename the existing resizable Array representation and consider the
separation complete. No new Slice interface or implicit array/List coercion is
selected; the explicit associated constructors above remain future work.

Length in the type remains an open decision:

| Candidate | Meaning | Main consequence |
| --- | --- | --- |
| `[T; N]` | N participates in type identity. | Exact-size parameters can reject wrong lengths statically; constant-length rules and possible const-generic binders need a separate bounded design. |
| `[T]` (or `Array<T>`) | Each object has a fixed construction-time length, which is not part of type identity. | One parameter type accepts arrays of different lengths; lengths may be computed at runtime and indexing remains checked. |

The proposed `fn sum(values: [i32; 4])` uses an ASCII semicolon and would require
exact length four if that design is adopted. It is not a commitment to length
in types or to general Rust const generics. The initial review favors evaluating
runtime-stored fixed lengths for scripting simplicity, while leaving the choice
to the later design decision. An omitted length must have one defined meaning;
do not silently mix builtin arrays, borrowed slices and the List interface.

Current [collection access](spec/collection-access.md),
[value semantics](spec/value-semantics.md#repeat-arrays-and-bulk-replacement) and
[architecture](architecture.md#language-contracts-and-native-implementations)
instead specify `[T]` as List and literal/repeat construction as Vec. Activating
this direction deliberately replaces those contracts and requires coordinated
syntax, type, library, artifact/runtime, example and tooling updates. Preserve
left-to-right once-only literal evaluation, checked element/index behavior,
shared-object/GC rooting and cleanup. Decide repeat-count compatibility with the
length model explicitly; preserve repetition's existing shared-identity safety
rule unless separately changed. Do not import Rust move/borrow semantics, Kotlin
variance or structural collection equality as incidental changes.

Primary references: Rust's [array types](https://doc.rust-lang.org/reference/types/array.html)
have type-level constant lengths; Kotlin's [arrays](https://kotlinlang.org/docs/arrays.html)
are fixed-size objects without a size type parameter, and its
[collection factories](https://kotlinlang.org/docs/constructing-collections.html)
separate list construction from arrays. Rust's
[Vec](https://doc.rust-lang.org/std/vec/struct.Vec.html) and
[HashSet](https://doc.rust-lang.org/std/collections/struct.HashSet.html) provide
`new` and array-based `From` construction using associated-call syntax. These
support naming and responsibility comparisons, not copying either language's
runtime ownership model.

Follow-up owner: syntax/type semantics, foundation declarations and collection
constructors, checked executable contracts and runtime array storage. Reuse
focused literal/repetition, indexing/iteration, interface matching, generic,
aliasing and source-free artifact/GC tests when activated. Keep the length-in-type
decision and remaining constructor signature details explicit before selecting
migration scope. This entry
records a future design direction; implementation and specification changes
remain deferred.

## SA21 Unify Rust registration through NativeModule with scoped host access

The requested future direction is to register all Rust-side functions and types
through NativeModule, including application host APIs. Keep the existing field,
assignment and function-call syntax. Host access still needs explicit typing and
runtime lifetime/borrow checks; unified registration does not make Rust state
part of the script heap or import a full Rust borrow checker into Kagari.

Current registration has two paths. Standard and application native modules use
ModuleDecl, NativeBinding/storage and Engine installation. The bundled standard
library supplies [modules()](../crates/kagari-stdlib/src/lib.rs), and the
[Engine builder](../crates/kagari-embed/src/engine/builder.rs) installs them by
default. Host APIs separately use
[HostInterface](../crates/kagari-types/src/host_interface/mod.rs),
HostFunction/type/path bindings and HostRegistry. The runtime retains both host
and native registries. Ordinary function and method registration overlaps; the
Host path additionally owns external object identity, scoped borrows, declared
field/path access and mutation adapters. Its capabilities justify a host boundary,
not necessarily a second declaration and installation system.

The target reuses [unified library registration](architecture.md#unified-library-registration):
one declaration/binding/install path for standard and application APIs, with
host object/access adapters under the same module. Move relevant Host contracts
into the shared model and replace obsolete entrypoints/records coherently rather
than retaining forwarding APIs or dual signature authorities. Runtime-local host
objects, borrow leases and path commit state may retain focused internal owners;
this does not require flattening every registry into one data structure. Preserve
source-free declaration/binding validation, generated tooling views, root and
runtime ownership checks, schema/generation validation and pinned dependencies.

### Candidate host access types and field semantics

Ref<T> and RefMut<T> are candidate script-visible wrapper names for shared and
writable host access; names, conversion rules and exact representation remain
open. Scripts use ordinary type annotations, field access and Native calls rather
than new `&`/`&mut`, move or lifetime syntax. A wrapper is a checked handle with
host provenance, access capability and a valid scope, not a freely storable naked
Rust reference. It still needs semantic support for lifetime/escape restrictions;
a plain generic struct alone cannot establish these guarantees. Writable access
through a host wrapper is separate from SA18's collection-interface model and
from binding `val`/`var`; shared access does not imply deep object immutability.

The intended distinction is:

| Field content | Result of reading the field |
| --- | --- |
| Scalar or other value-semantic data | An ordinary value; local copies do not alias the host field. |
| A shared Kagari-managed object | Its checked object identity, following ordinary alias and rooting rules. |
| A Rust-owned nested object accessed in place | A controlled field view retaining host provenance, access and scope. |

These examples illustrate future wrapper behavior, not implemented APIs:

```kagari
// Assume player.hp is i32.
var hp = player.hp;
val number_alias = hp;
hp = 20; // Changes only the local; player.hp and number_alias are unchanged.
player.hp = 20; // Writes the host field through its checked adapter.

// Assume player.stats is a Rust-owned nested object.
val stats = player.stats;
val alias = stats;
alias.hp = 20; // Accesses the same host field view.
reset_stats(stats); // Native registration declares writable access to Stats.
```

Reuse [host path views](spec/typed-path-mutation.md): they retain a root and typed
projection rather than a persistent Rust field borrow. Copying a controlled view
must not duplicate a raw mutable reference. Field replacement, indexed-element
removal and relocation need an explicit view contract: decide whether an old view
resolves the current location or becomes invalid. Never silently treat a path
view as a stable borrowed object identity or retain a dangling field address.

### Initial scope: borrowed inputs and independent outputs

The proposed first implementation accepts shared/unique Rust access during a
Native invocation, returning ordinary values or independent owning handles.
Acquire checked access before invoking Rust and release it on every exit. An
outer host-to-script borrowed scope remains the lifetime bound for all derived
access; per-operation access must be authorized by that scope rather than create
an independent conflicting lease. Checks cover aliases, multiple parameters,
parent/child overlap and synchronous reentry. Distinct script names do not prove
distinct host data. For a callback requiring two mutable inputs, two handles to
the same or overlapping data must fail before Rust receives overlapping `&mut`s.

The existing [typed conversion traits](../crates/kagari-runtime/src/native/conversion/mod.rs)
primarily transfer owned data or owning handles; safe borrowed input adapters
must be designed explicitly rather than assume ordinary FromKagari can return
arbitrary Rust references. Reuse existing host ownership/borrow validation and
checked native contracts. Do not activate unrestricted borrowed returns as an
incidental part of registration unification.

### Optional extension: returns borrowed from an input

A Native function returning a reference tied to its input requires a persistent
borrow relationship. For example:

```rust
fn stats_mut<'a>(player: &'a mut Player) -> &'a mut Stats {
    &mut player.stats
}
```

The following future script sketch must reject the conflicting replacement while
the returned borrow remains active:

```kagari
val stats = stats_mut(player);
player.stats = new_stats(); // Trap: a live derived borrow protects the input.
stats.hp = 20;
```

Returning shared `&Stats` also prevents conflicting writes while it remains valid.
Returning an independent value/handle, or a reference independent of the input,
does not establish this particular input/output relationship. Registration must
declare the return's borrow origin(s), or use a dedicated checked return adapter;
input/output type names alone cannot identify that relationship. Multi-input
origins and composite borrowed results need a bounded design if later included.

The candidate runtime model records the host owner/scope, parent borrow, mode and
active/released state, and associates the returned handle with that record. Keep
the originating borrow protection after the Native function returns. Authorize
access through the derived handle while blocking conflicting use of the parent
or other aliases. Check active conflicts, not whether an object was ever borrowed;
a live shared borrow also blocks writes. An initial conservative whole-root check
is acceptable to evaluate; field-disjoint borrow analysis is not required. Report
conflicts as runtime traps rather than introduce a complete static borrow checker.
Preserve once-only evaluation, trap ordering, committed effects and path mutation
validation/commit guarantees when choosing the check location.

Explicit release is a candidate, not a finalized API:

```kagari
val stats = stats_mut(player);
val alias = stats;
stats.release(); // Candidate: closes the shared borrow record for all aliases.
player.stats = new_stats();
alias.hp = 20; // Trap: this borrowed handle is now invalid.
```

Closing the record invalidates all associated borrowed aliases; merely removing
one variable cannot restore input access while another valid alias remains.
Do not release protection while an active Rust callback still holds a reference.
Nested derived borrows also need a defined release/invalidation policy before
support is added. Explicit release must be backed by automatic scope cleanup on
normal exit, traps, cancellation and depth exhaustion. Releasing a child does not
end an independent outer host borrow. Borrowed handles cannot outlive their host
scope or cross suspension without a separately established safe contract. Do not
use nondeterministic GC collection as the borrow-release mechanism.

Runtime checking avoids full static borrow analysis but still needs safe storage,
provenance and cleanup. Wrapper copying, readonly adaptation, scope escape,
release behavior and diagnostics remain decisions for activation. Genuine borrowed
returns and re-resolving path views must remain distinct contracts. Rust's
[lifetime relationships](https://doc.rust-lang.org/book/ch10-03-lifetime-syntax.html)
and [RefCell runtime borrow checks](https://doc.rust-lang.org/std/cell/struct.RefCell.html)
are references for these boundaries, not a requirement to copy Rust syntax,
ownership semantics or RefCell's panic API.

Follow-up owner: shared declarations and executable contracts, Engine module
registration, runtime host identity/path/borrow adapters and Native conversion.
Suggested order: unify registration; integrate host types and field views; support
borrowed inputs with ordinary/owning outputs; then decide whether persistent
borrowed returns justify their additional lifecycle model. When activated, reuse
focused registration/linking, source-free rejection, alias/overlap, borrowed-return
lifetime, reentry, stale-view, suspension, cleanup and pinned-reload contracts.
Do not weaken tests or retain a second semantic implementation to hide migration
gaps. This review entry records the future direction and unresolved choices only;
implementation, specifications and roadmap phase activation remain deferred.
