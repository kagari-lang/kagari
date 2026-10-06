# Import and namespace resolution (SA4)

Status: ready for implementation; IR01-IR03 have not started.
The [roadmap](implementation-roadmap.md#import-and-namespace-resolution-ir01-ir03-planned)
owns activation, phase order, checkboxes and the progress ledger. This document
owns the implementation contract for [SA4](review.md#sa4-import-records-also-represent-namespace-lookup-state).

## Starting point

`ResolvedImport` currently represents real imports, module declarations, implicit
package/prelude bindings and auxiliary namespace entries. `ModuleImports` stores
these in one vector; `ResolvedName::SourceItem { import, item }` and
`resolve_member(import_index, path)` use its positions to identify namespaces.
Entering a child module therefore requires another synthetic import with an empty
alias and `internal_namespace`, although the child is already in the module catalog.

The migration separates local name introduction from namespace traversal and
declaration identity. Importing a module as `m` introduces only `m`; resolving
`m::nested::value()` enters the imported module and its child directly. Another
alias of that module must reach the same declaration while retaining its own
navigation and direct dependency provenance.

| Starting implementation | Responsibility |
| --- | --- |
| [imports/mod.rs](../crates/kagari-hir/src/imports/mod.rs), [catalog.rs](../crates/kagari-hir/src/imports/catalog.rs), [bindings.rs](../crates/kagari-hir/src/imports/bindings.rs) | Module graph, mixed records, exports and member lookup. |
| [resolver/resolved.rs](../crates/kagari-hir/src/resolver/resolved.rs), [table.rs](../crates/kagari-hir/src/resolver/table.rs), [resolve.rs](../crates/kagari-hir/src/resolver/resolve.rs) | Resolved identities, scope names and body path resolution. |
| [imports/functions.rs](../crates/kagari-hir/src/imports/functions.rs), [types.rs](../crates/kagari-hir/src/imports/types.rs), [declarations.rs](../crates/kagari-hir/src/declarations.rs) | Imported signatures/types and declaration consumers. |
| [analysis/mod.rs](../crates/kagari-hir/src/analysis/mod.rs), [declaration_queries.rs](../crates/kagari-hir/src/analysis/declaration_queries.rs) | Snapshot/cache integration and source queries. |

Read [AGENTS.md](../AGENTS.md), the module specification and the roadmap ledger
before execution. Inspect the current status/diff and relevant implementation/tests;
preserve unrelated edits. Start at the first incomplete phase, using the ledger and
phase commits to resume. An assignment to execute this plan covers IR01 through
IR03 in order; record bounded implementation decisions in the ledger and follow
the checkpoint rules below.

## Outcome and scope

Resolve `m::nested::value()` by looking up `m` in the current scope, entering its
module, finding `nested` in that module and finding `value` in the child module.
No auxiliary import or empty alias is created to represent the child namespace.
Keep existing language behavior under [modules](spec/modules.md).

Own HIR import/name resolution, declaration/signature consumers, source queries
and affected compiler handoff consumers. Preserve source-free execution boundaries.
SA1 inline AST reuse, SA2 scheduling, SA3 iteration termination, new language
features and Rust macro/multiple-namespace machinery remain separate work.
Retain the current fixed-point scheduling and round limit in this migration;
doing so does not resolve SA2 or SA3. No format/ABI bump or new crate is required.

## Design references

The structural references are pinned to Rust 1.90.0; they guide responsibility
boundaries, not Kagari language rules:

| Rust reference | Adopted distinction |
| --- | --- |
| [ModuleData](https://github.com/rust-lang/rust/blob/1.90.0/compiler/rustc_resolve/src/lib.rs#L572), [NameBindingKind](https://github.com/rust-lang/rust/blob/1.90.0/compiler/rustc_resolve/src/lib.rs#L802) | A module has member resolutions; a binding distinguishes its resolved definition from an import provenance layer. |
| [ImportKind](https://github.com/rust-lang/rust/blob/1.90.0/compiler/rustc_resolve/src/imports.rs#L62), [ImportData](https://github.com/rust-lang/rust/blob/1.90.0/compiler/rustc_resolve/src/imports.rs#L149) | Import syntax/state retains named/glob distinctions, source ranges and the introducing scope. |
| [NameResolution](https://github.com/rust-lang/rust/blob/1.90.0/compiler/rustc_resolve/src/imports.rs#L244) | Resolution retains strong and glob candidates rather than choosing whichever was inserted last. |

The [Rust compiler guide](https://rustc-dev-guide.rust-lang.org/name-resolution.html)
provides broader context. Here, a namespace means a member container. Rust's
type/value/macro namespace partition, macros, arena-pointer ownership and solver
scheduling are not imported into this migration. Kagari visibility and shadowing
remain governed by its module specification.

## Required data model

The following Rust-shaped declarations specify keys, ownership and enum cases;
imports/derives are omitted. Implement these distinctions directly, with no old
model adapter. Naming/container refinements must retain these contracts and be
recorded in the roadmap ledger. Keep visibility private unless an actual existing
consumer requires otherwise; no forwarding modules or new re-exports.

### Qualified targets

```rust
struct SourceUnit {
    module: ModuleIdentity,
    file: FileId,
    revision: Revision,
    arena: HirArenaId,
}
enum SourceItem {
    Function(FunctionId), Const(ConstId), Struct(StructId), Enum(EnumId),
    OpaqueType(OpaqueTypeId), Trait(TraitId), Variant(VariantId),
}
struct SourceDeclRef { unit: SourceUnit, item: SourceItem }
enum NamespaceId {
    Module(SourceUnit),
    Associated(SourceDeclRef),
    Host(HostModuleId),
    InstalledPrefix(ModuleIdentity),
}
enum ResolvedTarget {
    Namespace(NamespaceId), Source(SourceDeclRef),
    HostFunction(HostFunctionId), HostType(HostTypeId),
}
```

`SourceUnit` identifies one immutable lowering, using `LoweredModule.source` and
`module.body.arena()`. Inline-module targets use the child's lowered source identity;
physical navigation uses its source map. Validate module/file/revision/arena before
dereferencing an item. A declaration reference cannot contain `Import(usize)` or a
module-declaration index. Source type namespaces derive from the existing checked
enum/type-alias/inherent-member rules; type-relative resolution stays in type checking.

`InstalledPrefix` represents only prefixes admitted by current installed-package
lookup, including a package root without a physical source module. It is a catalog
node, never an import directive. Keep existing host IDs/providers and host-revision
invalidation. Reuse `DefinitionContext`/`DefinitionTable` to map source declarations
to published semantic identities; no second global ID registry is introduced.

Replace `ResolvedName::SourceImport`/`SourceItem` with source/namespace target cases.
Local function/type and lexical local/parameter cases remain; qualification uses
their owning `SourceUnit`. Module path bindings resolve directly to `NamespaceId`;
the local `ModuleId` survives only as declaration provenance. Update Copy-dependent
consumers to borrow/clone these qualified values rather than inventing unqualified
numeric handles. No target stores an alias, directive ID or copied member map.
At the resolver boundary, a source target from the current lowering projects to
its existing local `ResolvedName` case; foreign targets retain the qualified ref.
Both map to the same published declaration identity. Store use-site `LookupHit`
provenance alongside existing reference/source-map facts, including resolved path
prefixes; it is not part of the canonical target or the shared declaration cache.

### Directives and scope bindings

```rust
struct DirectiveId { unit: SourceUnit, slot: u32 }
struct LocalName(String); // Private field; checked nonempty single-component name.
enum ImportKind { Named { alias: Option<LocalName> }, Glob }
enum DirectiveResolution {
    Pending, Resolved(ResolvedTarget), Unresolved, Ambiguous,
}
struct ImportDirective {
    id: DirectiveId,
    path: String,
    kind: ImportKind,
    span: FileSpan,
    root_span: FileSpan,
    visibility: Visibility,
    resolution: DirectiveResolution,
    direct_dependencies: BTreeSet<ModuleIdentity>,
}
enum BindingOrigin {
    Declaration(SourceDeclRef),
    ModuleDeclaration { unit: SourceUnit, module: ModuleId },
    NamedImport(DirectiveId), GlobImport(DirectiveId),
    Package(PackageId), Prelude(NamespaceId),
}
struct BindingCandidate {
    target: Option<ResolvedTarget>,
    origin: BindingOrigin,
    owner: ModuleIdentity,
    visibility: Visibility,
    location: Option<FileSpan>,
}
struct NameEntry {
    strong: Vec<BindingCandidate>,
    globs: Vec<BindingCandidate>,
    implicit: Vec<BindingCandidate>,
}
struct ModuleImportFacts {
    directives: Vec<ImportDirective>,
    scope: Arc<NameTable>,
    diagnostics: Vec<Diagnostic>,
    dependencies: Vec<ModuleIdentity>,
}
```

Each flattened syntactic import leaf gets a directive, preserving its path roots
(`self`/`super`/`crate`), leaf range and enclosing use-tree range. An absent named
alias derives its binding name from existing lowering rules. A glob has no alias
field; it introduces admitted immediate members, not a name for the glob itself.
Use existing identifier rules for `LocalName`; a path such as `core::ops` is not a
scope key. Extend lowering/source-map facts if root ranges or explicit-alias presence
are lost.

Extend the existing `NameTable` to map `LocalName` to `NameEntry`; keep its impl
inventory and lexical `ScopeBinding` separate. Strong candidates are declarations,
module declarations and named imports; globs are weaker, package/prelude candidates
are weakest. Presence in a stronger tier blocks fallback even when unresolved or
ambiguous. Strong collisions remain errors even for equal targets. Equal canonical
glob targets resolve once while retaining all origins; distinct targets are ambiguous.
Filter glob admission/re-exports under existing visibility rules, not insertion order.

Published facts contain no `Pending` resolution: retain unresolved/ambiguous
diagnostics when the existing solver stops. A missing target reserves the known
binding name. Package/prelude/module bindings have no fabricated directive.
Directive IDs address source/provenance only. Dependencies include syntactic facade
edges and unused imports, not just final canonical targets; module declarations and
implicit installed bindings contribute their existing edges independently.
Installed-prefix bindings retain the currently admitted source-module dependency
set; a synthetic prefix is not a registered graph node or a replacement for those edges.

### Shared catalog and lookup

```rust
struct NamespaceTable {
    owner: ModuleIdentity,
    names: Arc<NameTable>,
    glob_allowed: bool,
}
struct NamespaceCatalog {
    modules: BTreeMap<ModuleIdentity, Vec<SourceUnit>>,
    namespaces: HashMap<NamespaceId, NamespaceTable>,
    package_aliases: BTreeMap<String, BTreeSet<PackageId>>,
}
struct LookupHit { target: ResolvedTarget, via: Vec<BindingOrigin> }
enum LookupResult {
    Found(LookupHit), Missing, Unresolved,
    Ambiguous(Vec<BindingCandidate>), Inaccessible(Vec<BindingCandidate>),
    NotNamespace, StaleSource,
}
enum NamespaceResult { Found(NamespaceId), NotNamespace, StaleSource }
```

Module namespace tables share the same `Arc<NameTable>` as their module facts.
Associated tables store immediate member spellings, not flattened `Enum::Variant`
strings. Preserve existing enum-glob eligibility and associated-method policy.
Duplicate logical module identities retain diagnostics/candidates instead of being
silently overwritten. Host namespaces delegate to `HostDeclarations`.

`ModuleGraph` owns `Arc<NamespaceCatalog>` alongside nodes containing
`Arc<ModuleImportFacts>`; the analysis snapshot owns that graph. Declaration/body
resolver contexts borrow/share the catalog and local names. Catalogs contain no
resolver, snapshot, import-facts or parent-Arc back-reference. Member tables are
unfiltered: each lookup receives the importer's module and current host provider.
Array-interface metadata remains in the existing declaration environment.

Required lookup interfaces, with importer/hosts passed in a `LookupContext`:

```rust
fn lookup_member(&self, ctx: &LookupContext, ns: &NamespaceId, name: &str,
                 cancel: &CancellationToken) -> Result<LookupResult, Cancelled>;
fn namespace_of(&self, ctx: &LookupContext, target: &ResolvedTarget,
                cancel: &CancellationToken) -> Result<NamespaceResult, Cancelled>;
fn resolve_path(&self, ctx: &LookupContext, root: LookupResult, suffix: &str,
                cancel: &CancellationToken) -> Result<LookupResult, Cancelled>;
```

`LookupContext` contains the importer `ModuleIdentity` and a borrowed current
`HostDeclarations`. The caller resolves the first component with existing
lexical/module/root rules and passes that result plus the remaining path to
`resolve_path`; unresolved/ambiguous/local-value prefixes block package/helper
fallback. For each remaining component, enter `namespace_of(target)` and do a keyed
member lookup, checking access and cancellation. A non-namespace stops with
`NotNamespace`; existing type-relative recovery remains with the type checker. Return the
canonical target plus selected provenance; ambiguity/inaccessibility stay diagnostic
states, not a generic missing result. Runtime field/method access remains outside
this namespace-path API.

During the existing fixed-point build, use mutable draft tables and explicit pending
re-export edges keyed by directive IDs. Finalize them to canonical targets with
cycle guards and publish immutable facts; never recursively canonicalize unchecked
cycles. Preserve the current round policy, including its documented SA3 limitation.
A public facade entry checks its own visibility and the original target's permitted
export visibility; do not re-check a hidden implementation path as if the caller
had spelled it. Preserve direct export edges in origins/dependencies.

### Concrete lookup and consumer replacement

For `use pkg::m; fn main() -> i32 { m::nested::value() }`, with public `nested`
and `value`, the stored facts are:

| Location | Stored fact |
| --- | --- |
| Root directives | D0: named `pkg::m`, no explicit alias, resolved namespace M. |
| Root name table | `m` -> namespace M, origin `NamedImport(D0)`. |
| Catalog M | `nested` -> namespace N, origin `ModuleDeclaration`. |
| Catalog N | `value` -> qualified function F, origin `Declaration(F)`. |

Lookup is `scope["m"] -> M["nested"] -> N["value"] -> F`. The root has no
`nested` binding or helper import. `use pkg::m::nested::value as v` binds `v` to
the same F with its own directive/provenance; both calls use one checked function
signature keyed by F, without copying namespace members into either directive.

| Old representation/API | Required replacement |
| --- | --- |
| `SourceImport { item, members, ... }`, `SourceItem { import, item }` | Qualified source/namespace targets; source item kind excludes imports. |
| `module_aliases`, `namespace_entries`, `internal_namespace` | Module declaration bindings and direct catalog lookup; delete helper entries/maps. |
| `glob_root`, `implicit_module` | Explicit directive kinds and `BindingOrigin` cases. |
| `resolve_member(import_index, path)`, `resolve_export(SourceImport)` | Catalog lookup and canonical final targets; builder export edges remain provenance only. |
| Imported function/type maps keyed by import-dependent `ResolvedName` | Qualified declaration keys; local alias lookup belongs only to `NameTable`. |
| `SourceFunctionId`/`SourceTypeId` | Reuse/project qualified source refs, validating their unit before signature/type access. |
| `source_import_at` | Replace with `source_target_at` returning the canonical hit plus provenance; migrate navigation/docs/tests directly. |

Keep existing DefinitionId-keyed nominal/method projections; replace only their
alias/import-dependent resolution keys. Place target/directive/catalog facts under
`imports/`, name-entry policy under `resolver/table.rs`, query adaptation under
`analysis/` and semantic identity mapping under `declarations.rs`.

Cache comparisons include target units, namespace surfaces, visibility, ambiguity,
dependency edges and host revision. Conservatively invalidate changed dependents;
retained snapshots keep their own catalogs. Existing arena/table remapping is the
only route for reusing facts against another lowering. Never compare aliases alone.

## IR01: Replace namespace lookup and target identities

- Build the snapshot namespace catalog from the existing source catalog/module
  graph. Include source modules, module re-exports and existing enum/associated
  member behavior; keep declared host lookup through its existing provider.
- Introduce qualified targets, candidate-based `NameTable`, binding origins and
  catalog lookup; migrate the consumer replacements above. Real-import records may
  remain for IR02 syntax production, already storing qualified targets and syntactic
  directive provenance. They cannot act as namespace identities.
- Replace `resolve_member(import_index, path)` with namespace-based lookup.
  Resolve each path component with visibility, ambiguity and cancellation checks.
- Migrate `ResolvedName`, declaration collection, body resolution, imported
  function/type catalogs and affected compiler lowering together. Imported
  signatures remain independently checked; callers do not re-analyze callee bodies.
- Remove internal namespace entries, `namespace_entries`, `internal_namespace`
  and member lookup's dependence on `module_aliases`. Reuse real module bindings.
- Update snapshot/cache comparisons conservatively as part of the cutover;
  changed namespace surfaces or target revisions must invalidate dependent facts.

Acceptance: deep paths and multiple aliases reach the same qualified declaration;
calls, constants, types and enum members use target identities, not import-vector
indices. Unresolved imports remain queryable diagnostics. No second active
namespace resolver or compatibility facade remains.

Primary owners: the HIR files above, `crates/kagari-hir/src/imports/members.rs`
and affected lowering under `crates/kagari-compiler/src/source/`.

## IR02: Separate directives, bindings and provenance

- Replace remaining mixed `ResolvedImport` records with explicit directive kinds
  and named scope bindings. Remove `glob_root`/`implicit_module` flag combinations.
- Explicit named imports create bindings; globs produce bindings for admitted
  members; module declarations and installed package/prelude names create bindings
  with their own provenance. Keep local/explicit precedence over globs.
- Keep directive IDs only for source provenance and export edges, never for
  namespace/declaration identity. Preserve grouped-leaf and root source ranges.
- Cut over the syntax/state producer to `ImportDirective` and publish
  `ModuleImportFacts`; retain the shared tables introduced in IR01 and remove the
  remaining `ResolvedImport` model.
- Build dependency edges from actual declarations/directives and implicit package
  bindings, including unused imports. Preserve deterministic reachable ordering.
- Complete `source_target_at`, definition/navigation queries and public re-export
  consumers to the intended model, then remove obsolete record types and branches.

Acceptance: every scope binding has a real name; every directive represents real
import syntax. Auxiliary namespaces, module declarations and package/prelude
bindings are not fabricated directives. Dependencies and diagnostics retain
existing semantics and source attribution.

## IR03: Verify snapshots, tooling and executable consumers

- Reuse existing import, facade, signature, overlay and arena tests. Add focused
  behavioral coverage where the matrix below is missing, rather than tests that
  mirror the new struct layout.
- Verify dependency edits/removals and transitive visibility/signature changes
  invalidate unchanged callers, while retained snapshots still answer old queries.
- Check definition, type and function navigation through nested module aliases,
  grouped imports and re-exports, including inline-module physical offsets.
- Exercise imported calls through checked source lowering and artifact execution;
  verify source-free and native consumers do not depend on the namespace catalog.
- Update architecture documentation, mark SA4 resolved and complete the roadmap
  ledger only after final checks pass. Performance remains unmeasured unless a
  separate reproducible measurement is made.

## Behavioral acceptance matrix

| Boundary | Evidence |
| --- | --- |
| Deep/grouped paths | Direct leaf import and qualified access agree; aliases share targets; a child namespace is not added as a local name. |
| Access and exports | Check each direct path component; public facade access through private modules retains current rules. |
| Globs and implicit names | Local/explicit precedence, conflicting globs, prelude shadowing and installed host/package lookup. |
| Associated namespaces | Enum variants, constructors, type aliases and existing associated method resolution. |
| Dependencies and recovery | Unused imports remain reachable; legal module cycles, unresolved re-export cycles and cancellation terminate as before. |
| Snapshots and tooling | Overlay add/remove, stale target rejection, unchanged/transitive callers, old snapshots and physical navigation locations. |

Extend existing `definition_queries_distinguish_modules_and_follow_source_facades`,
`associated_methods_respect_owner_visibility_and_type_aliases`,
`shared_facade_resolution_rejects_stale_targets_and_terminates_cycles` and
`facade_signature_changes_invalidate_unchanged_transitive_callers` in the import
test modules. Add missing deep-alias coverage: qualified and direct leaf calls
agree, an unqualified child name remains unresolved, and different owners' same-named
leaves remain distinct. Keep unresolved strong imports blocking a valid glob/prelude
candidate. These are observable identity/lookup tests, not struct-layout assertions.

## Checkpoints and validation

Complete IR01, IR02 and IR03 in order, with a compiling checkpoint and one
Conventional Commit per phase carrying `Import-Phase: IR01` (or IR02/IR03).
Update only the roadmap ledger with phase status and material decisions/failures.
A failed required check keeps the phase incomplete; record command, cause and
owning follow-up, then resolve it before committing that checkpoint.

Focused checks: `cargo test -p kagari-hir imports::`,
`cargo test -p kagari-hir analysis::`, and
`cargo test -p kagari-compiler --test source_programs` when consumers change.
At each implementation checkpoint run the structure checker and diff check.
IR03 additionally runs `cargo test -p kagari-embed --test source_snapshots`,
`cargo test -p kagari-embed --test syntax_examples`,
`cargo test -p kagari-embed --test cranelift_preparation` and
`cargo test -p kagari-cli --features jit`, followed by:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p kagari-embed --no-default-features --test artifact_features
cargo test -p kagari-embed --no-default-features --features source --test artifact_features
cargo test -p kagari-embed --no-default-features --features native --test artifact_features
git diff --check
```

Native acceptance must execute the existing applicable native tests, not only
compile that feature. Preserve source-free artifact behavior. Documentation-only
changes require local-link/content and diff checks, not Rust builds.
