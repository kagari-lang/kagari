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
below. The explicit constructor is implemented by SA20; broader SA18 changes
remain deferred.

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

## SA19 Use DefinitionId throughout semantic models

The agreed design now has an [execution plan](definition-id-plan.md), SI01-SI06.
Implementation is active (SI01 complete, SI02 in progress). The plan owns scope,
validation and the progress ledger; the [roadmap](implementation-roadmap.md#semantic-definition-ids)
places it before native collections.

Semantic records and algorithms will use DefinitionId without identity-representation
generics. DefinitionPath remains at authoring/display boundaries; portable local
references remain inside codecs. Reuse existing contexts, snapshots and explicit
imports, preserving verification, source revisions and pinned runtime generations.
SA9's full module-graph handle migration and SA21 registration unification remain
separate. This finding closes only when the implementation is complete.

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
