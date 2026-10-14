# Repository review

Keep ongoing repository review findings in this document rather than creating
dated review files. Entries record observed behavior and possible follow-ups;
they do not activate implementation work.

This is the single review queue for unresolved findings and scoped follow-ups.
SA identifiers retain the repository review numbering; R identifiers retain the
architecture review numbering. Neither series is renumbered when resolved entries
are removed. Current contracts live in the specifications and architecture; CI
status and activation belong to the [roadmap](implementation-roadmap.md).

- [Repository findings](#sa1-inline-modules-are-parsed-again): unresolved SA entries.
- [Architecture findings](#architecture-findings): retained R findings checked
  against the current implementation; validation limits are recorded below.
- [Native expansion gates](#native-expansion-gates): requirements for future native
  coverage, not defects caused by the intentionally limited backend.
- Historical timings, allocation data and reproduction details are retained under
  [performance measurements](performance-baseline.md#architecture-review-measurements-2026-10-03).

The consolidation audit inspected code and existing tests at `b71e0cdd`; it did
not rerun behavior tests or performance probes. The remaining SA entries still
have their described representation or missing feature. Test preparation progress
and pending measurements belong to the [existing task](test-preparation-plan.md),
not a separate review finding. Reconfirm affected behavior when activating a task.

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

Keep this follow-up separate from completed identity and namespace work. Measure
graph build allocations and time before/after on the same module workload,
separating compilation time; reuse identity/snapshot and import tests for focused validation. Code migration is
not activated by this review.

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
uses; removing it requires updating those consumers.
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

## SA21 Unify Rust registration through NativeModule with scoped host access

The next scope is merging the existing HostInterface into NativeModule.
The [implementation plan and API proposal](host-registration-plan.md) owns
HR01-HR03, registration API decisions, migration acceptance and progress.
Implementation is unstarted. The public proposal uses ordinary by-value
conversion into Kagari objects. The plan also records a queued Ref/RefMut design;
its adapters and new field-view behavior remain unimplemented and outside HR01-HR03.
Their metadata is not a required part of by-value usage.

### Registration unification

Standard and application native modules use shared declarations, Native bindings
and Engine installation. Host APIs separately use
[HostInterface](../crates/kagari-types/src/host_interface/mod.rs), host type/function/
path bindings and runtime registration. Their overlapping declaration and install
paths should become one NativeModule model.

Move general type/member/function declarations to shared semantic and executable
owners. Ordinary struct values use checked converters and normal Kagari fields.
Inventory existing live-host/root/path consumers separately; preserve their checked
execution while migrating declaration/install ownership. Freeze a bounded removal
map before retiring the old aggregate, format and entrypoints, explicitly recording
any dependency on deferred reference work. Reuse
[unified library registration](architecture.md#unified-library-registration).

Runtime-local host identities, scoped borrow checks and prepared path commits
remain focused services. Preserve existing field/path semantics, roots, alias and
escape checks, cancellation, schema ownership, atomic installation and pinned
reentry/reload. Unification does not move external Rust state into the GC or
require new script wrappers or unrestricted Rust references. Converting existing
live host access to snapshots would change semantics and is not implied by this
migration. DTO schema/derive dependencies remain owned by the interoperability plan.

### Queued borrowing and field-view design

The [reference proposal and complete example](host-registration-plan.md#queued-ref-and-refmut-design)
use `Ref<T>`/`RefMut<T>` path handles and derive-generated field projections.
Register the target descriptor once; reference capabilities reuse its explicit
host members and any validated projection schema. Native payload and ordinary DTO
representations remain distinct; a data schema alone does not expose Rust methods.
Native calls batch-check loans before constructing short-lived `&T`/`&mut T`;
disjoint fields of one host object may be borrowed mutably at the same time.
Users do not write path-to-field lookup callbacks. Existing root-level checks
remain in force until their coherent path-aware replacement is implemented.

The plan owns overlap rules, scope cleanup, reentry, replacement behavior and
the finite inline-struct projection boundary. Its
[receiver proposal](host-registration-plan.md#method-receivers-and-trait-selection)
adds the conversion matrix and shared/mutable host receiver requirements. Methods
are registered once for a host target type. Rust and Kagari traits are independent;
script trait exposure requires explicit declaration/impl/binding registration,
without imposing three separate view impls for ordinary method calls. Safe scope
signatures and callback lifetimes still require compiling proofs before activation.
Generated projections must not create overlapping ancestor references. Implementation remains queued;
the proposal is not evidence that current typed adapters support Rust references.

Persistent returns borrowed from an input are a separate optional extension.
They require origin/lifetime records, conflict protection, deterministic cleanup
and a policy for alias invalidation; ordinary independent outputs do not establish
that borrow relationship. Do not use GC timing to release Rust borrow protection.

The runtime-owned `Opaque<T>` proposal remains owned by the
[interoperability plan](rust-interop-design.md#opaque-ownership-and-access), using
the shared registration model and existing NativeObject storage/roots. The host
moves T into runtime, accesses it through checked read/edit, or explicitly takes it
back with atomic alias invalidation. Ref/RefMut instead lend host-owned storage.
The new facade, extraction and method adapter capabilities remain unimplemented;
this does not activate RI03 or expand HR01-HR03.

The plan also owns the bounded
[call-boundary cost follow-up](host-registration-plan.md#call-boundary-cost-follow-up):
per-insertion root-table scans and repeated typed argument packing are confirmed
by static inspection, without measured speed claims. Registration must preserve
existing low-level allocation contracts. Reference implementation and performance
optimization are separate from HR01-HR03 acceptance.

## SA22 Generic bounds do not supply interface construction witnesses

The current coercion path requires a selected construction witness; the retained
[typing test](../crates/kagari-hir/src/tests/typeck/traits.rs) expects rejection of
bound-only erasure to `std::fmt::Debug`. The original minimal reproduction was
rejected with KG_TYPE_RETURN_TYPE_MISMATCH:

```text
trait Child {}
fn erase<T: Child>(value: T) -> Child { value }
```

[Interface coercion](../crates/kagari-hir/src/typeck/body/methods.rs) requires a
selected concrete implementation identity/arguments or an existing interface
upcast. A bound permits method calls but does not produce that construction record.
The same boundary predates the collection-interface migration (baseline 53949725).
Generic receiver shapes with a selected implementation, such as Vec<T> -> List<T>,
already work, including shared generic methods and retained closures.

Follow-up owner: HIR coercion facts, contract construction witnesses and compiler
lowering, with runtime consuming checked facts. A future implementation must carry
the bound's actual implementation and associated outputs through static/shared calls,
verify it without source, and preserve receiver category, GC roots and supplying
generations. Validate scalar and object receivers, parent adaptation, cancellation,
forged/missing witnesses and reload. Do not infer an object upper bound from an
arbitrary trait or bypass conversion checks. This is a recorded existing limitation,
not an activated feature, Any/downcast design or part of host registration unification.

## Architecture findings

These IDs originated in the 2026-10-03 review of
`9581f5ad2a754911d1edb92eb35e957a18d7b5df`. Source inspection at `b71e0cdd`
confirms the mechanisms below remain; the old report's resolved findings and
superseded implementation details are not retained as tasks. This is not a fresh
runtime reproduction, cancellation stress test or performance measurement.

| ID | Current finding | Focused follow-up and acceptance |
| --- | --- | --- |
| R1 / P1 | Safe public `KagariLanguage::kind_from_raw` still uses unchecked `u16`-to-`SyntaxKind` transmute; the public green-tree wrapper also performs no kind validation. | Validate raw kinds before constructing the enum. Cover valid round trips and invalid-input rejection through the public API. The audit did not execute an invalid discriminant. |
| R2 / P2 | SDK `execute_prepared` passes `context.runtime_options()` but does not consult `context.jit_policy`; those runtime options contain no JIT selection policy. The native path can proceed despite `Disabled`, contrary to the documented fallback rule. | Enforce the [JIT policy contract](spec/jit.md#preparation-and-fallback) consistently across preparation and execution; assert actual native/interpreter reports under enabled/disabled contexts. No fallback after entry. |
| R5 / P2 | `implementation_method` still creates a default cancellation token with maximal search/depth bounds and discards search errors; `implementation_count` uses the same unbounded/default-cancellation wrapper pattern. | Propagate the caller's fallible search context. Cover pre-cancelled queries and tiny depth/candidate limits. No source hang was reproduced by this audit. |
| R7 / P2 | `lower_program_to_mir` still loops over all authoring modules for each new specialization-request round. | Measure a bounded generic chain/fan-out before choosing an instance work queue or input-keyed reuse. Preserve foreign defaults/witnesses, compilation limits and final verification. Current scaling cost is unmeasured. |
| R8 / P2 | Body reuse compares imported functions using derived equality, including source/navigation identity; aggregate equality also compares inherent methods with their source sites. Signature-preserving edits can therefore prevent semantic reuse. | Separate semantic contracts from navigation identities. Validate signature-preserving versus contract-changing dependency edits without dropping source revision/arena validation. This is conservative invalidation, not evidence of a stale result. |

Evidence owners:

- R1: [syntax conversion](../crates/kagari-syntax/src/syntax_node.rs).
- R2: [SDK execution](../crates/kagari/src/runtime.rs),
  [context options](../crates/kagari/src/context.rs),
  [native dispatch](../crates/kagari-vm/src/vm/native.rs) and
  [SDK native tests](../crates/kagari/tests/cranelift_preparation.rs).
- R5: [trait searches](../crates/kagari-hir/src/aggregates/implementations.rs).
- R7: [program lowering](../crates/kagari-compiler/src/source/program.rs).
- R8: [body reuse](../crates/kagari-hir/src/analysis/body_queries.rs),
  [imported functions](../crates/kagari-hir/src/imports/functions.rs),
  [source identities](../crates/kagari-hir/src/imports/mod.rs) and
  [aggregate equality](../crates/kagari-hir/src/aggregates/mod.rs).

These findings remain review items, not automatically activated tasks or new
prerequisites for unrelated work.

## Artifact reachability follow-up

Whether executable artifacts should prune unreachable contracts remains an open
question separate from completed registration-sharing work. Review artifact
construction and verified dependency closure ownership before proposing pruning;
measure artifact size and preparation costs on a current baseline. Preserve
source-free validation, declared native contracts and generation-pinned dependencies.
Historical startup costs do not establish the benefit of this change.

## Native expansion gates

The mechanism inventory below comes from the same 2026-10-03 baseline and needs
current-code confirmation before implementation. The acceptance requirements are
future expansion constraints, not a new active phase sequence.

The narrow scalar backend is intentional. The following gates apply when adding
calls, GC-bearing values, compiled callbacks or optimizations that depend on callee/memory facts. Wider scalar arithmetic and
scalar locals/control flow can progress independently with appropriate trap and
polling semantics. These are not bugs caused merely by lacking a larger backend.

| Gate | Boundary observed at the reviewed baseline | Required decision and evidence |
| --- | --- | --- |
| General calls and values | [native_call.rs](../crates/kagari-abi/src/native_call.rs) transports runtime/result/status for zero arguments and Unit/Bool/i32 results | Define argument/result representation, helper calls, error propagation and cleanup once for both native backends |
| Native GC roots | [native.rs](../crates/kagari-contract/src/native.rs), logical stack-map declarations, records logical Register/Local locations; no runtime physical map reader was found | Choose a shadow-root protocol or real physical frame/PC/location maps; prove objects survive collecting helpers, returned allocation, reentry and traps |
| Verified cross-module calls | [codegen/lib.rs](../crates/kagari-codegen/src/lib.rs) accepts a module seal; full binding proofs live in `VerifiedMirProgram` | Pass program-scoped verified target bindings or a typed runtime trampoline; backends must not redo name/type resolution |
| Effect interpretation | Script calls carry `calls`/`may_trap`, not transitive allocation/write facts | Treat calls as conservative memory/GC barriers or compute conservative graph summaries before emitting LLVM `readonly`/`readnone`-style assumptions |
| Tier-neutral callbacks | Synchronous `ScriptInvoker` selected the interpreter | Preserve pinned generation, captures, argument/result checks, shared call depth/cancellation, observers and trap origins when selecting compiled callbacks |
| Code lifetime and hot reload | Installed handles retain exact versions and their dependency closure | Extend dependency assumptions to direct calls, inlining and module/SCC compilation units; reclaim code only when its owners and retained dependencies permit it |

At the reviewed baseline, the poll helper recorded a logical offset and invoked
ordinary registered-root GC; it did not publish native stack roots. Cranelift rejected heap-bearing
slots and emitted empty root maps for its accepted subset. Do not mistake the logical
metadata schema for completed GC integration.

Effect caution is similarly forward-looking. In
[instruction.rs](../crates/kagari-mir/src/instruction.rs), reviewed script calls
used `EffectSet::call`; [effects.rs](../crates/kagari-contract/src/effects.rs)
set only `calls` and `may_trap`. A callee can still allocate and mutate.
The reviewed constant propagation treated calls as barriers and dead-code
elimination required an empty effect set, so no miscompile was demonstrated. Individual
false effect bits must not become absence proofs in a future optimizer.

Recommended order: establish calls/roots and tier-neutral runtime services with
Cranelift, grow the semantic conformance matrix, then implement LLVM against the
same checked contract. Keep LLVM lowering and optimization local to that backend.
MIR need not become LLVM IR, and an additional SSA IR is justified only by concrete
optimization requirements. Simple baseline native compilation and optimizing
compilation may use different strategies without duplicating language semantics.

For each newly accepted native feature, use the same semantic cases through source,
serialized artifact, interpreter and **asserted actual native execution**. Tests
that successfully fall back do not establish native coverage. The important rows
are checked overflow/trap order; left-to-right once-only effects; nested calls and
call-depth failure; objects live across collection/reentry; cleanup after traps and
cancellation; old callbacks/interfaces surviving reload; and observer-triggered
pre-entry fallback. Add differential generated-program testing only within the
supported subset, with reproducible seeds and finite bounds.

Keep recoverable HIR distinct from checked executable types. Readonly collection
interfaces do not prove frozen storage, noalias or an exclusive native borrow;
use the current [collection contracts](spec/collection-access.md) rather than
historical type spellings. Preserve once-only evaluation, trap order, completed
effects and pinned generations. The [contract/common responsibility review](architecture.md#contract-and-common-responsibility-cleanup)
remains separate; renaming a mixed crate alone does not fix ownership.
