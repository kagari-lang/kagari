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

Registration, runtime-owned Opaque and scoped Ref/RefMut are implemented.
Common NativeModule declarations, one bind entrypoint and atomic installation
replace the former host aggregate, standalone format and parallel registration.
Existing live-host consumers retain their checked root/path behavior. The completed
HR/RF/RI plans are retired; Git history retains their phase ledgers.

The [host interop specification](spec/host-interop.md) owns the storage/passing
matrix, scoped loans, generated inline field paths and explicit method/trait
registration. The [embedding guide](spec/embedding-api.md#typed-native-registration)
owns DTO/Serde conversion and runnable consumers. Rust traits and Kagari traits
remain independent. Borrowed returns, general view trait forwarding, async
retention and wider native reference execution remain outside this implementation;
[the roadmap](implementation-roadmap.md#host-registration-and-interop-delivery)
records those limits and pending exhaustive CI.

### Call-boundary cost follow-up

Static review confirms the following remaining preparation costs. These are
code-derived costs, not benchmark results or speedup claims:

- [RootTable::insert](../crates/kagari-runtime/src/gc/roots.rs) calls `prune` over
  every slot before each insertion, including when reusable slots already exist.
  [Vec conversion](../crates/kagari-runtime/src/native/conversion/composites.rs)
  calls `encode_value` per element; the
  [conversion context](../crates/kagari-runtime/src/native/conversion/context.rs)
  retains temporary roots for heap-backed results until the enclosing scope ends.
  For n such elements and initial slot high-water mark R, scanning can cost
  O(nR + n^2) when live temporary roots force table growth. Existing free capacity
  can avoid the quadratic growth term, but not the repeated high-water scan.
- [Typed native invocation](../crates/kagari-runtime/src/native/typed.rs) builds a
  `Vec<Value>`; [argument decoding](../crates/kagari-runtime/src/native/conversion/arguments.rs)
  copies it into rooted storage and creates a root lease even for scalar packs.
  Mapping checks recur at pack and element conversion boundaries. Value/handle
  copying here is not necessarily a deep object copy. Individual scalar conversion
  avoids an object root, which does not eliminate the pack-level allocation.

The GC root owner should evaluate scope/batch storage or bounded amortized reclaim;
the typed invocation owner should evaluate borrowed argument windows and reuse of
checked mapping facts. Preserve snapshot/retention across user conversion and
reentry, collection during conversion, owner/generation/value checks, cancellation
and cleanup on every exit. Separate stable declaration proofs from invocation-time
facts before relocating validation. Root reclamation must still handle last-lease
drop and runtime transfer safely. Do not remove roots merely because the simple
scalar case does not need GC protection.

Keep owned conversion, scoped borrowing and explicit raw/primitive implementations
behind one validated binding entrypoint without forcing one data-movement path.
The [VM allocation tests](../crates/kagari-vm/tests/allocation_contracts/native_allocations.rs) establish
warm zero-allocation scalar/SequenceHandle paths; they do not establish that the
general typed adapter is allocation-free. Preserve those existing contracts during
registration migration.

This is a queued, bounded follow-up, separate from completed registration/interop
delivery. Before optimization, measure equivalent raw/typed/owned cases and the
implemented Ref/RefMut adapters. Vary argument/payload size, root-table high-water
mark, path depth and active loans where applicable. Separate compile,
installation, conversion and warm-call costs; report allocations, retained memory
and timing with revision, machine, toolchain, profile/features, parallelism and
cache state. Include representative bulk calls and small frequent calls without
assuming one is faster. Reuse focused correctness tests for GC/reentry/trap cleanup;
no retained performance conclusion can be inferred from registration unification.

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

## SA23 Builtin type specifications duplicate scalar properties

[BuiltinTypeSpec](../crates/kagari-types/src/surface.rs) stores `family`,
`const_safe` and `heap_backed` beside `ty` in a fixed builtin table. The requested
follow-up is to remove these stored properties and query the scalar semantic
owner instead. This is a deferred cleanup, not activated implementation.

`family` duplicates facts already available through
[BuiltinType::number_type and integer_layout](../crates/kagari-types/src/scalar.rs).
Its current consumers in [HIR scalar checks](../crates/kagari-hir/src/builtin/surface.rs)
only classify numeric types and signed-integer/float negation. Reuse scalar-owned
facts for primitive checks and remove `BuiltinTypeFamily` when it has no remaining
consumers. Generic operation requirements already use
[ordinary trait bounds](spec/traits.md#operation-specific-bounds); do not move
their source-level semantics into scalar classification methods.

For the current builtin types, only `String` is heap-backed. Constant eligibility
is a separate language policy: `Never` and `String` are not const-safe, while the
other builtin types are. In particular, `Never` is neither heap-backed nor
const-safe, so constant eligibility cannot be derived as `!is_heap_backed()`.
Move both queries to explicit `BuiltinType` methods; define constant eligibility
with an exhaustive match so new builtin variants require a deliberate decision.
Update the HIR constant checks and representation classification to use those
methods, preserving existing behavior and the remaining name lookup/display table.

Follow-up owner: `kagari-types` scalar semantics and their HIR consumers. Keep
numeric admission, constant eligibility and representation facts distinct; do not
expand this cleanup into runtime reflection, declaration registration or a type
model migration. The table is fixed data, not per-object metadata. No memory or
speed improvement has been measured, and removing fields does not establish a
byte saving without checking padding and the resulting layout.

When activated, reuse the existing [standard type checks](../crates/kagari-hir/src/tests/typeck/standard.rs)
and constant-check contracts, with focused numeric/negation and builtin
name/display coverage. Verify all consumers migrate without changing accepted
programs or diagnostics; add coverage only for distinct missing rules.


## SA25 Standard-library method authoring scatters contracts across name-based passes

Standard-library methods should declare their signatures, generic bounds and
native callable dependencies together at their definition owner. The current
construction has nonlocal, name-driven policy that makes a method's complete
contract difficult to read and change. This is a recorded authoring-model
follow-up, not activated implementation or an observed execution failure.

In [list method authoring](../crates/kagari-stdlib/src/catalog/list_methods.rs),
`declare` enumerates method names and derives callback shapes from suffixes;
`comparison` assigns Eq/Ord bounds; `calls` reconstructs comparison, iteration
and mutation dependencies; `configure_overrides` scans another method-name list
and attaches concrete implementations' requirements and result metadata.
[Foundation assembly](../crates/kagari-stdlib/src/catalog/mod.rs) first declares
these methods, projects concrete implementations, then runs that override pass.
[Hash collection defaults](../crates/kagari-stdlib/src/catalog/defaults.rs) also
attach key-operation requirements through a name whitelist after selecting the
most recently appended implementation. Adding or renaming a method requires
coordinating these rules, not merely changing its declaration.

The issue is construction cohesion, not late inference of static constraints.
Bounds already enter `FnDecl` during declaration construction. A
[NativeCallableRequirement](../crates/kagari-types/src/declaration/requirement.rs)
is a symbolic dependency on a receiver type's trait member, not a bound,
function pointer or runtime argument list. Its `arguments` are the method's own
type arguments; trait arguments belong to `interface`. The
[compiler](../crates/kagari-compiler/src/source/lower/instances/callables.rs)
substitutes and selects or forwards these operations. Native execution consumes
prepared calls rather than searching for an implementation by method name.

For example, `dedup` requires `T: Eq`, but its callable dependency is
`T`'s `PartialEq::eq`. A generic MutableList default additionally needs receiver
iteration, iterator advancement and removal; the Vec override operates directly
on storage and needs only the element comparison dependency. The
[Vec binding](../crates/kagari-stdlib/src/bindings/vectors.rs) retrieves that
requirement's declaration-scoped `SelectedCall` and invokes the selected equality
operation. Deriving bounds solely from callbacks would lose the stronger Eq
contract; copying the default's dependencies into every override would retain
operations its body does not need.

Proposed direction: introduce a cohesive per-method authoring record or a small
builder, initially for these collection methods. Keep one public signature/bound
owner and attach explicit implementation recipes to it:

```text
MutableList<T>::dedup
  public contract: (Self) -> Unit, T: Eq
  generic native default:
    equal -> T's PartialEq::eq
    iter / next / remove -> declared receiver operations
  Vec<T> native override:
    equal -> T's PartialEq::eq
    direct Vec storage operations
```

Create owner-scoped operation handles when declaring dependencies, and let Rust
bindings consume those same handles rather than reconstructing intent from
method names or independently maintained slot numbers. Names may label methods
and operations for presentation, but must not silently select their contracts.
Instantiate default/override recipes with checked identity and generic mappings;
later passes may allocate identities, project implementations and validate them,
but should not rediscover method semantics from strings. The exact API remains
a design question; this does not prescribe a new macro system or global DSL.

Lower the authoring description into the existing
[FnDecl/NativeDeclaration](../crates/kagari-types/src/declaration/mod.rs) and
[ModuleDecl](../crates/kagari-types/src/declaration/module.rs) records. Identity-keyed
side maps can remain normalized storage without being a manually coordinated
authoring interface. Preserve distinct bounds, symbolic requirements and selected
executable operations; Rust closures stay outside portable declaration records.
Retain owner/scope validation, callback signature and slot correspondence,
source-free verification and generation-pinned calls. Shared generic operations
must keep their existing checked forwarding path, not become runtime trait search.

Follow-up owner: `kagari-stdlib` declaration/binding authoring, with narrowly scoped
shared registration or operation-handle support only where required. Keep this
separate from the implemented [ordinary trait-bound semantics](spec/traits.md#operation-specific-bounds),
Host API facade changes and collection storage redesign. Start with the list/default/Vec path and hash-key dependencies;
remove superseded name-based attachment passes instead of retaining a second
source of method policy. No performance improvement has been measured.

When activated, reuse existing list/default and hash-collection contract tests.
Select coverage for Eq versus PartialEq admission, generic default versus concrete
override dependencies, scripted/native callbacks and generic substitution. Check
rejection of missing, mismatched or foreign-owner operations and representative
source-free execution and pinned reload. Confirm generated declarations and
observable behavior remain unchanged; add tests only for distinct uncovered
contracts. Exhaustive workspace/feature/backend validation remains CI-owned.

## SA26 Add ordinary Default construction and struct update expressions

The agreed direction is a core-library `Default` trait plus general struct
update syntax, using Rust's behavior as the reference except for its ownership
rules. This records a future feature; implementation is not activated. The
[current trait inventory](../crates/kagari-stdlib/src/catalog/key.rs) has no
standard Default declaration. The [grammar](kagari.ebnf) accepts only named or
shorthand struct fields, and [struct checking](../crates/kagari-hir/src/typeck/body/constructors.rs)
reports omitted fields; there is no struct-update base expression.

Default is a core-library ordinary trait, not a compiler language protocol.
Declare it through standard registration and make it available through the
ordinary default prelude. Use the existing associated-function and trait-bound
mechanisms for `Type::default()` and `T: Default`; do not add Protocol/LangRole
variants, a `default` keyword, hidden name recognition or a special runtime
constructor. A private registration key may identify the standard declaration.
Resolve applications by their actual declaration identities, not spelling.
Rust's Sized bound belongs to its type system and does not require a new Kagari
Sized mechanism. The proposed source contract is:

```text
trait Default {
    fn default() -> Self;
}

impl Default for Player {
    fn default() -> Self {
        Player { health: 100, level: 1 }
    }
}

val player = Player::default();
val inferred: Player = Default::default();
val changed = Player { health: 200, ..Default::default() };
val updated = Player { health: 200, ..existing_player };
```

The examples are proposed syntax/behavior, not currently supported standard APIs.
A default is an ordinary implementation-defined value, not zeroed storage.
Choose primitive/container implementations deliberately at activation; do not
infer one for every type. Ordinary associated functions without `self` remain
static-only under the [trait contract](spec/traits.md), so this feature does not
make Default an interface value. Automatic `#[derive(Default)]`, enum default
selection and field declaration initializers are separate extensions, not
prerequisites or implicitly authorized work.

Implement `..base` as general struct construction, independent of Default:

- Permit one base expression at the end of a named-field struct initializer.
  It must have the same nominal struct type and type arguments as the result;
  structural similarity or an interface conversion is insufficient.
- Explicit fields provide their values; only omitted fields are read from the
  base. Existing duplicate/unknown-field checks remain. All fields must be
  accessible, including those supplied through the base, as in Rust; update
  syntax must not bypass private construction boundaries.
- Supply the target struct type as context to the base expression. In
  `Player { health: 200, ..Default::default() }`, ordinary trait-call inference
  selects Self = Player. Support contextual `Default::default()` consistently
  with annotated bindings, not through a struct-only hardcoded Default path.
- Evaluate explicit field expressions in source order, then the base expression,
  each once. A default call constructs its whole returned value; it is not
  separate per-omitted-field defaulting. Preserve its effects even when some or
  all returned fields are overridden. On a trap or cancellation, later
  expressions do not run, completed effects remain and temporary roots are
  cleaned up under existing execution rules.
- Allocate a new outer struct and copy remaining field values using Kagari's
  existing value semantics. Do not move fields out of or directly overwrite the
  base object, invoke Clone implicitly or deep-copy referenced objects. Object
  fields retain shared references; the base remains usable.

For example, updating only `health` in a Player with an `items` collection creates
an independent outer Player, leaves the base's health unchanged and shares the
unchanged items collection. Mutation through either collection alias remains
observable through the other. Rust instead moves or copies remaining fields and
may partially invalidate the base; borrow, move, Copy and Drop restrictions must
not be imported into Kagari to implement this feature.

Primary references: [Rust Default](https://doc.rust-lang.org/std/default/trait.Default.html),
[struct updates](https://doc.rust-lang.org/reference/expressions/struct-expr.html#functional-update-syntax)
and [operand evaluation order](https://doc.rust-lang.org/reference/expressions.html#evaluation-order-of-operands).
Follow-up owners: core-library registration/prelude, syntax/AST/HIR construction
and ordinary associated-call inference, with compiler/backend lowering consuming
checked field/base facts. Update grammar and syntax/trait/value contracts when
implemented; do not make executable loading depend on source analysis. This is
independent of SA16's pattern rest marker and SA25's authoring cleanup, and does
not activate a general cloning, storage or host-registration migration.

When activated, reuse existing trait/associated-call and struct-construction
coverage for concrete and generic defaults, contextual Self inference and nominal
identity/shadowing. Select cases for base type/field visibility, update placement,
left-to-right once-only evaluation, default-call effects, traps/cancellation,
outer-object independence and shared nested fields. Include representative
source-free execution and pinned reload of selected default implementations.
Complete workspace/feature/backend coverage remains CI-owned. No performance
improvement is claimed.

## Architecture findings

These IDs originated in the 2026-10-03 review of
`9581f5ad2a754911d1edb92eb35e957a18d7b5df`. Source inspection at `b71e0cdd`
confirms the mechanisms below remain; the old report's resolved findings and
superseded implementation details are not retained as tasks. This is not a fresh
runtime reproduction, cancellation stress test or performance measurement.

| ID | Current finding | Focused follow-up and acceptance |
| --- | --- | --- |
| R7 / P2 | `lower_program_to_mir` still loops over all authoring modules for each new specialization-request round. | Measure a bounded generic chain/fan-out before choosing an instance work queue or input-keyed reuse. Preserve foreign defaults/witnesses, compilation limits and final verification. Current scaling cost is unmeasured. |
| R8 / P2 | Body reuse compares imported functions using derived equality, including source/navigation identity; aggregate equality also compares inherent methods with their source sites. Signature-preserving edits can therefore prevent semantic reuse. | Separate semantic contracts from navigation identities. Validate signature-preserving versus contract-changing dependency edits without dropping source revision/arena validation. This is conservative invalidation, not evidence of a stale result. |

Evidence owners:

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
