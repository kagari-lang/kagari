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

## Target responsibilities

| Data | Responsibility |
| --- | --- |
| Import directive | Named/glob syntax, path, optional explicit alias, visibility, spans, resolution status and direct dependency provenance. Grouped leaves remain separate directives. |
| Scope binding | Nonempty local name, resolved/unresolved/ambiguous state and provenance: explicit import, glob, module declaration, package or prelude. |
| Namespace catalog | Shared immutable module/associated-namespace member tables, qualified targets, visibility and canonical re-export links. Direct lookup by member name retains multiple candidates for diagnostics. |
| Resolved source target | Actual module/declaration identity, independent of import-vector positions. Existing host IDs remain host targets. |

During graph construction, qualify lowered source handles by file/revision and
their owning HIR arena where applicable. Reuse the existing definition context and
table mapping for published semantic facts; do not introduce a second global ID
registry. A declaration identity alone does not validate a source revision.

Publish the namespace catalog with the analysis snapshot. Resolver contexts share
it alongside local bindings; the catalog must not own a back-reference to its
resolver contexts. Lookup receives the importer context, so shared tables never
reuse another importer's visibility-filtered result. Keep direct dependency and
navigation edges separate from canonical targets. Move array-interface metadata
to the existing catalog/declaration environment rather than coupling it to imports.

## IR01: Replace namespace lookup and target identities

- Build the snapshot namespace catalog from the existing source catalog/module
  graph. Include source modules, module re-exports and existing enum/associated
  member behavior; keep declared host lookup through its existing provider.
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
- Build dependency edges from actual declarations/directives and implicit package
  bindings, including unused imports. Preserve deterministic reachable ordering.
- Update `source_import_at`, definition/navigation queries and public re-export
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
