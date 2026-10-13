# Repository review

Keep ongoing repository review findings in this document rather than creating
dated review files. Entries record observed behavior and possible follow-ups;
they do not activate implementation work.

Only unresolved findings remain here; identifiers stay stable when resolved
entries are removed. Current contracts live in the specifications and architecture.
CI status belongs to the roadmap, independently of implementation completion.

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

## SA7 Routine fixes trigger expensive aggregate test suites

[Repository policy](../AGENTS.md#verification-and-tooling) limits local checks to
affected contracts and a small regression set; complete suites belong to CI.
The [test-cost audit](performance-baseline.md#workspace-test-cost-audit-2026-10-11)
identifies expensive SDK/VM/HIR targets and repeated preparation for investigation.
Optimization is deferred; confirm current costs before changing an owner.

The numeric fixtures already share compilation while keeping independent runtime
routes and successful calls after traps. Further work should reuse immutable setup
where safe and preserve cases requiring fresh snapshots or mutable runtimes.

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
through a host wrapper is separate from the [ordinary collection-interface model](spec/collection-access.md) and
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


## SA22 Generic bounds do not supply interface construction witnesses

The current checker rejects this conversion with KG_TYPE_RETURN_TYPE_MISMATCH:

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

## SA23 Mixed concrete and interface equality depends on operand order

Priority: P1. Reproduced against 7121a819 after the collection-interface migration.

```kagari
use std::collections::List;
fn main() -> bool {
    val storage = Vec::from([42]);
    val view: List<i32> = storage;
    storage == view
}
```

`view == storage` compiles and evaluates to true. Reversing the operands as above,
or using `storage != view`, passes source checking but fails bytecode lowering with
`Verification(InvalidOperation { function: FunctionRef(0), reason: "invalid collection access flow" })`.

[Operator checking](../crates/kagari-hir/src/typeck/body/operators.rs) accepts
operands with a common interface, but
[binary lowering](../crates/kagari-compiler/src/source/lower/expr.rs) selects
PartialEq using only the left operand's type. The concrete-left case consequently
passes an interface value to Vec's concrete comparison contract. The verifier is
correct to reject that call; weakening access/type validation is not a repair.

Follow-up owner: HIR checked comparison/coercion facts and compiler equality
lowering, coordinated with [SA26](#sa26-implement-the-accepted-equality-and-identity-design).
Under the accepted declarations, List has no automatic PartialEq, so both mixed
operand orders in this reproduction must fail source checking. Do not retain
receiver forwarding merely to make the old example execute. For comparisons
that remain legal, record checked operand adaptation and the selected operation
together and consume them consistently, preserving left-to-right once-only
evaluation. Reuse
[collection interface coverage](../crates/kagari-embed/tests/collection_interfaces.rs)
for concrete/interface and mutable/readable operand orders, both equality
operators, and same/different underlying receivers. Validate representative
source and encoded-artifact execution; preserve independent source-free rejection.

## SA24 Collection interface equality rejects valid enum receivers

Priority: P2. At 7121a819, the following legal interface construction reaches a
runtime trap instead of the enum's applicable value comparison:

```kagari
use std::collections::Set;
use std::iter::CollectionCursor;
enum Single { Item }
impl Iterable for Single {
    type Item = i32;
    type Iter = CollectionCursor<i32>;
    fn iter(self) -> CollectionCursor<i32> { Vec::from([42]).iter() }
}
impl Set<i32> for Single {
    fn len(self) -> usize { 1usize }
    fn is_empty(self) -> bool { false }
    fn contains(self, value: i32) -> bool { value == 42 }
}
fn main() -> bool {
    val a: Set<i32> = Single::Item;
    val b: Set<i32> = Single::Item;
    a == b
}
```

Compilation succeeds; execution traps with
`interface receiver does not support object comparison`.
[Comparison planning](../crates/kagari-compiler/src/source/lower/instances/receiver.rs)
returns no receiver-comparison evidence for non-object categories. The
[contract validator](../crates/kagari-contract/src/callable/receiver.rs) and
[runtime binding](../crates/kagari-runtime/src/objects/comparison.rs) impose the
same object-category restriction. Consequently, interface PartialEq forwarding
cannot use this enum's valid comparison. The identity restriction has also become
a restriction on retaining value equality.

**The replacement semantics are now agreed:** [SA26](#sa26-implement-the-accepted-equality-and-identity-design)
and the [accepted value contract](spec/value-semantics.md#accepted-comparison-design)
remove automatic collection-interface PartialEq forwarding and all identity
fallbacks. Both Set operands above remain legal interface values, but their `==`
must fail source checking because Set has no interface-level PartialEq. The fix
is not to extend the old retained receiver comparison to enums. Explicit content
helpers remain available under their declared bounds.

Value receivers remain eligible for checked interface conversion; object-only
admission was not adopted. All interface `===`/`!==` expressions are rejected
statically, replacing the current hidden-value trap contract. No Object type or
bound is introduced. User enum comparison becomes explicit rather than implicit.

Follow-up owner: source operator checking, receiver-comparison planning, portable
proof contracts and runtime binding, coordinated with SA26. Reuse collection and
[interface identity coverage](../crates/kagari-embed/tests/interface_identity.rs)
to distinguish explicit/conditional value equality, absent comparison evidence,
concrete-object identity and interface-operator rejection. Preserve meaningful
generic-method, GC and reload coverage currently exercised through value receivers
when changing fixtures.

## SA25 Common-interface inference includes dynamically incompatible traits

Priority: P2. Reproduced against 7121a819:

```kagari
trait Read { fn read(self) -> i32; }
struct A {}
struct B {}
impl Read for A { fn read(self) -> i32 { 42 } }
impl Read for B { fn read(self) -> i32 { 42 } }
impl PartialEq for A { fn eq(self, other: Self) -> bool { true } }
impl PartialEq for B { fn eq(self, other: Self) -> bool { true } }
fn main() -> i32 {
    val item = if true { A {} } else { B {} };
    item.read()
}
```

The checker reports KG_TYPE_AMBIGUOUS_COMMON_INTERFACE with `PartialEq, Read`.
PartialEq is not a valid dynamic-interface target under the current rules, so
Read is the only usable common interface under the current inference policy.
Adding an unrelated static comparison implementation changes its candidate set.
The accepted replacement below removes that inference policy entirely: the
unannotated example must be rejected even without the PartialEq implementations;
`val item: Read = ...` explicitly supplies the intended interface.

[Relationship queries](../crates/kagari-hir/src/aggregates/relationships.rs)
enumerate implemented traits and intersect/deduplicate candidates before applying
dominance, without filtering dynamic-interface eligibility. Later
[interface validation](../crates/kagari-hir/src/typeck/applications.rs) cannot
repair an ambiguity already reported. Keep the general implementation query usable
for static bounds and member lookup. Targeted conversions must use the shared
dynamic-compatibility owner; do not repair context-free joins by adding a filtered
candidate search that the accepted design no longer permits.

### Accepted explicit conversion contexts

Status: accepted on 2026-10-13; implementation unstarted. The
[trait specification](spec/traits.md#accepted-interface-conversion-context) owns
semantics; [TI01](trait-interface-comparison-plan.md#ti01-require-supplied-interface-conversion-targets)
owns implementation, focused acceptance and progress. Retain explicit target
contexts and remove context-free common-interface/common-parent inference,
including sibling-derived coercion. Identical interface types and ordinary
non-interface inference remain valid.

[Branch checking](../crates/kagari-hir/src/typeck/body/joins.rs) currently both
searches common interfaces and supplies inferred branch hints to conversion.
Removing only common_interfaces is insufficient. The plan owns migration of both
paths and their existing behavioral fixtures; SA22 remains separately scoped.

### Agreed direction for dynamic trait compatibility

Status: the complete structural rule was accepted on 2026-10-13; implementation
alignment is pending. The [trait specification](spec/traits.md#interface-compatibility-rules)
owns the rules, examples and diagnostics;
[TI02](trait-interface-comparison-plan.md#ti02-unify-structural-dynamic-compatibility)
owns execution and validation. Check applied signatures, method bounds, associated
outputs and every parent. One incompatible member rejects the entire interface
application while static use remains legal; add no member exclusion mechanism.
Keep shared generic methods; no-receiver functions, constants and GATs remain
static-only in the accepted scope.

The current [role exclusion](../crates/kagari-types/src/language/role.rs) rejects
every reserved role except Index, Iterable and Iterator, including Debug, Display
and Hash. Replace it with structural checking at all admission boundaries.
PartialEq remains incompatible because of other: Self; Eq is incompatible through
its parent. Value receivers still admit checked interface conversion. This does
not grant comparison or identity operators; [SA26](#sa26-implement-the-accepted-equality-and-identity-design)
tracks that separate semantic migration.

Implementation ownership: HIR compatibility and expected-type propagation,
source-independent declarations/contracts, lowering and executable validation.
The execution plan is the sole phase/acceptance ledger; retain these reproductions
until the corresponding TI outcomes are implemented and validated.

The three findings were confirmed with temporary focused SDK probes using
compile_to_artifact and interpreter execution where compilation succeeded.
The source examples and diagnostics above retain the reproduction evidence;
temporary harnesses are not required documentation dependencies. No complete
backend/feature matrix or full-workspace suite was run for this review. These
entries record unresolved work and the agreed design direction; no implementation
or new validation run is implied by the subsequent documentation decision.

## SA26 Implement the accepted equality and identity design

Status: design accepted on 2026-10-13; implementation unstarted. The
[value specification](spec/value-semantics.md#accepted-comparison-design) owns
semantics; [TI03](trait-interface-comparison-plan.md#ti03-replace-equality-and-identity-selection-end-to-end)
and TI04 own implementation, cleanup, validation and the progress ledger.

Require statically proven PartialEq, explicit user Struct/enum comparison and
conditional Array content equality. Remove object identity defaults and automatic
collection-interface receiver forwarding; preserve content helpers, interface
Debug and ordinary method dispatch. Identity operators admit known concrete
object types and reject either interface operand. Value receivers remain allowed
in interfaces; no Object type/bound or new downcast API is introduced.

Current code still implements identity equality defaults, implicit user-enum
comparison, collection-interface receiver forwarding and runtime rejection of
hidden-value identity operations. The plan maps their HIR, contract, stdlib,
compiler/runtime and codec owners and removes comparison-only proof/dispatch
machinery with its consumers. Ordinary checked calls, GC/host boundaries,
cancellation, cleanup and pinned generations remain mandatory. SA23/SA24 must be
resolved against the replacement semantics rather than extending the old model.

This finding is unresolved. Plan authoring and specification updates do not
establish implementation correctness, passing local gates or complete CI.
