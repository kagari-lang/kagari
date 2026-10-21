# Context-owned module handles execution plan

Status: planning requested on 2026-10-21; implementation has not started.
The [roadmap](implementation-roadmap.md#review-follow-up-order) owns activation
and queue placement. This plan owns SA9's MH01-MH05 phases, design decisions,
acceptance and progress ledger. Recording the plan does not start implementation.

## Outcome and scope

Replace repeated owned module paths inside the source/semantic module graph with
compact, context-owned module handles. Reuse the existing definition identity
owner. Preserve portable paths at input, diagnostic and executable product
boundaries, and retain all source, visibility and identity validation.

For a module such as `game::combat::damage`, source units, namespace owners,
dependency edges and compiler request buckets currently carry copies of its
package/path. After migration, the identity context stores that portable identity
once and these records carry a checked handle. Display/export borrows the path
from the retained table. Initial path lookup still compares content; internal
handle copying, equality and hashing do not traverse strings.

The finite scope includes:

- The common identity table/context and checked module-root handle operations.
- HIR source units, namespace keys/owners, binding owners, dependency facts,
  import observations, module graph construction and snapshot consumers.
- Checked source-program dependency/export lookup and directly affected compiler
  source-lowering indexes, including specialization request bucket keys.
- Necessary registration, tooling, SDK and test callers of those changed APIs.
- Boundary conversion, deterministic output and matched graph-build measurement.

Source database/package acquisition inputs may retain portable `ModuleIdentity`.
Executable MIR/bytecode module records, artifact wire formats and runtime slots
remain their existing boundaries; SA9 does not require replacing every occurrence
of `ModuleIdentity` in the repository. MH01 must classify retained uses explicitly.

SA1 subtree lowering, SA14 local functions, SA11 declaration self IDs, SA22 interface
witnesses, SA23 scalar cleanup, SA21 call costs, R7 specialization scheduling, R8
semantic invalidation and R13 identity-domain retention remain separate. No global
interner, persistent compilation cache, new package model or runtime version-token
migration is introduced. No compilation-latency target or measured speedup is
claimed by this plan.

## Inspected baseline and responsibilities

Planning baseline: `6fe2cbe6`, inspected on 2026-10-21 with a clean working tree.
Planning inspected implementation and test owners; it ran no Rust tests or
performance probes. The [project goals](project_goal.md),
[identity architecture](architecture.md#definition-identity-ownership) and
[SA9 finding](review.md#sa9-module-paths-are-copied-into-internal-graph-keys-and-references)
remain the governing context.

| Owner | Current evidence and migration responsibility |
| --- | --- |
| [Portable identity](../crates/kagari-common/src/identity.rs) | `ModuleIdentity` owns a package string and path strings; clone/equality/hash/order operate on those values. Keep this exact portable locator. |
| [Definition table](../crates/kagari-common/src/identity/table.rs), [context](../crates/kagari-common/src/identity/map.rs) | Modules are already interned as roots over shared `Arc<ModuleIdentity>` records. `DefinitionId` carries table and node identity; snapshots retain immutable prefixes and imports remap roots. Extend this owner. |
| [Source database](../crates/kagari-source/src/source_database.rs) | External source/module binding is a portable input boundary; preserve file/revision ownership without forcing a compiler context into acquisition. |
| [Import model](../crates/kagari-hir/src/imports/mod.rs) | `SourceUnit.module`, installed prefixes, binding owners, dependency facts and graph keys carry paths. Replace their internal module identity while retaining file/revision/arena checks. |
| [Catalog](../crates/kagari-hir/src/imports/catalog.rs), [builder](../crates/kagari-hir/src/imports/builder.rs), [solver](../crates/kagari-hir/src/imports/solver.rs) | Namespace membership, access checks, observed changes and dependency propagation need one owning identity context. Existing build entrypoints do not yet take that context. |
| [Analysis](../crates/kagari-hir/src/analysis/mod.rs), [declaration queries](../crates/kagari-hir/src/analysis/declaration_queries.rs) | Analysis already owns a shared `DefinitionContext`; snapshots retain a `DefinitionTable`. Thread that ownership through graph construction and freeze after required module identities are registered. |
| [Checked program](../crates/kagari-hir/src/program.rs), [exports](../crates/kagari-hir/src/program/exports.rs) | Dependency maps and namespace lookup retain module paths. Adopt the graph's scope; preserve deterministic checked-member order and public export spelling. |
| [Compiler source program](../crates/kagari-compiler/src/source/program.rs) | Specialization request buckets use `ModuleIdentity`. Migrate keys and boundary lookups without changing whole-module request rounds or specialization semantics. |

## Chosen model and design gates

### A module root with an explicit owner

Use a dedicated `ModuleHandle` newtype around the existing module-root
`DefinitionId`; the name is provisional until MH01 closes the API inventory.
Keep construction private and checked. Conversion from a generic definition ID
must validate table membership, retained-prefix bounds and the root-node kind.
A function/field ID cannot be accepted as a module handle. This is a semantic
type distinction, not a compatibility alias or second identity table.

The existing eight-byte table/node representation is the intended storage basis.
Handles are Copy and have numeric equality/hash within the scoped identity model.
They contain no Arc, source body or runtime generation. They have no direct Serde
implementation. The enclosing graph, catalog or metadata owner retains the table;
cloning individual references must not acquire a fresh table snapshot or writer lock.

The common owner supplies checked interning, borrowed identity resolution,
exact-path comparison, lookup and explicit import/remapping. Module parent,
child-prefix and same-package queries belong with identity data, while access
policy and actual namespace membership remain with HIR/catalog.

Definition ancestry and module-path ancestry are different: existing
`DefinitionTable::parent` returns no parent for a module root. Do not reinterpret
that API or make `contains` silently mean lexical module containment. MH01 chooses
bounded module-relation queries, reusing interned prefixes where appropriate.
Do not add a second copied path tree or cache without a concrete query need.

### Snapshot and publication contract

1. Graph construction receives the analysis definition context. Source paths,
   installed inputs and necessary namespace prefixes are registered through it.
   Native records from another context are explicitly adopted before storing keys.
2. Freeze the published table after collecting all handles used by the graph.
   Published catalogs and queries are immutable and never intern on a read.
   A late unknown path produces the existing missing/unresolved result.
3. A handle denotes logical module identity only. `SourceUnit` retains FileId,
   Revision and HirArenaId, and consumers validate the complete unit before using
   local HIR IDs. Relowering the same revision can still make an old arena stale.
4. Equal logical module identities do not merge duplicate source units. Preserve
   ambiguity, strong/glob precedence, provenance and duplicate-module diagnostics.
   Interning a module or prefix grants neither existence nor visibility.
5. Older snapshots keep their original prefixes and facts. A handle appended later
   must be rejected by an older table. Context clones share one append-only writer;
   never fork mutable builders under the same table identity.
6. Separate contexts reject each other's raw handles even for equal paths. Import
   maps exact identities once and rebuilds affected map/set keys. Preserve limits,
   cancellation and collision checks; reject accidental collapse where duplicates
   are forbidden. Deliberate namespace ambiguity continues to retain all sources.

Graph construction may leave unused identity entries in the append-only writer
after cancellation, as identity allocation does not publish a graph. It must not
publish partial namespace facts, successful checked programs or executable proof.
Identity-table reclamation and domain lifetimes remain R13's separate concern.

### Paths, ordering and executable boundaries

Source import segments, aliases, physical locations and package dependency aliases
retain their existing owners. HIR `ModuleId` still names a local declaration;
bytecode `ModuleRef` still names a program-local slot. Neither becomes this handle.

Path-taking authoring/query entrypoints resolve once at their owning boundary.
Internal graph operations use handles throughout; avoid reconstructing a
`ModuleIdentity` per edge or per solver lookup. A boundary that returns an owned
portable record may materialize it deliberately. MH01 records the finite inventory
of such boundaries and the table lifetime from which each path is borrowed.

Handle allocation order is not semantic order. Audit ordered map/set iteration,
diagnostic selection, module traversal, export assembly and specialization seeding.
Use explicit portable-identity ordering where observable or needed for canonical
output. Preserve expression/source order and binding provenance; do not globally
sort operations merely to stabilize serialization.

An internal source-export namespace index should use handles; exported names and
portable executable namespace records retain paths. MIR/bytecode boundaries must
receive existing checked records in their correct definition scope. No backend
gains a source-analysis dependency, and no handle becomes a persisted module ID.
Generation-pinned calls, native installation and reload checks remain independent
of logical module equality. Existing format/ABI identifiers stay unchanged under
the unpublished-development policy.

### Primary implementation references

Reviewed on 2026-10-21:

- Rust `rustc_resolve` documentation for 1.99.0 (`b940084d7`, 2026-09-28):
  [`Module<'ra>`](https://doc.rust-lang.org/stable/nightly-rustc/rustc_resolve/struct.Module.html)
  wraps an interned module-data reference with identity-based equality and Copy
  use. Reuse the separation of owned data and cheap references. Rust's resolver
  arena and pointer identity do not supply Kagari's retained snapshot or explicit
  cross-context import contract.
- [Rust incremental compilation guide](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html)
  (unversioned guide, accessed on the same date): session-local IDs are distinct
  from persistent identity and are remapped when loading cached results. Kagari
  already has exact-path canonical encoding and definition remapping; reuse those
  mechanisms without adding a persistent cache or adopting hash equality as exact
  module identity.

The typed root wrapper is a small adaptation to Kagari's existing scoped IDs.
It avoids a second interner and preserves independent tables, source revisions,
bounded decoding and reload provenance. These references justify the ownership
model, not a performance claim.

MH01 must settle the precise relation-query API, graph/table publication point,
standalone analysis/test construction paths, cross-context reuse rules and retained
portable-field inventory before MH02. Record decisions here. Change the chosen
model only with concrete implementation evidence and an explicit scope explanation.

## Phases and coherent checkpoints

| Phase | Work | Exit condition |
| --- | --- | --- |
| MH01: inventory and baseline | Trace all source graph identity flows and standalone entrypoints; close the design gates above; select bounded contract tests and a fixed graph workload. Record baseline allocations, retained bytes and stage timing. | Every affected field/query has an owner and destination; retained paths have a boundary reason; baseline and commands are reproducible. No production migration yet. |
| MH02: checked identity operations | Add the module-root type and necessary table/context/remapping operations. Use it at a real module registration or graph-input preparation boundary and document ownership. | Root-kind, foreign-table, prefix lifetime and remapping contracts pass. No second interner, unchecked constructor or unused forwarding facade. Graph-field migration is explicitly owned by MH03. |
| MH03: graph and direct consumers | Migrate SourceUnit/catalog/owners/dependencies/solver observations together with checked programs, source exports, compiler request keys and required tooling/SDK callers. Thread the existing context through builders and publish retained tables. | Changed producers and consumers build together; no dual path/handle graph implementation. Focused import, visibility, ambiguity, snapshot and compiler dependency cases pass. R7/R8 algorithms remain unchanged. |
| MH04: boundary and determinism audit | Complete cross-context adoption, portable output, canonical ordering and negative identity checks; remove superseded path materialization on migrated internal paths. Review public surface and documentation. | Equivalent fresh/reused contexts produce equivalent diagnostics/navigation and deterministic products; source-free verification and representative pinned execution still pass. All carried integration failures are resolved. |
| MH05: measurement and handoff | Repeat the MH01 workload; account for table growth, snapshots and boundary conversion as well as graph storage. Complete required local checks and publish lasting contracts/results. | Scoped correctness and local checks pass; matched evidence and limits are recorded. Remaining CI scope and unrelated findings are explicit; retire this plan after completion. |

Once implementation is activated, commit coherent checkpoints with Conventional
Commits and `Plan-Phase: MH0N` trailers. Mark breaking public API changes with `!`
and explain caller migration. Do not retain compatibility entrypoints for this
unpublished change. MH03 may be a cohesive multi-crate edit; it must include its
direct consumers rather than leaving them to an unrelated future task.

Intermediate edits may temporarily fail compilation. At an architecture boundary,
attempt the affected check and record command, diagnostic, cause and owning phase
below. No fake validation, disabled assertion or production stub is permitted.
Do not repeat an unchanged failure before its owning repair. No phase is accepted
with required integration errors outstanding; any deliberately committed broken
intermediate state must be disclosed in its commit and ledger.

## Validation and measurement

Reuse the existing contract owners, adding cases only for distinct new handle
boundaries. MH01 selects a small set from these owners, not their combined full
suites:

| Changed boundary | Existing owner and focused coverage |
| --- | --- |
| Context, prefix and canonical identity | [Identity tests](../crates/kagari-common/src/identity/table/tests.rs): explicit scopes, foreign tables, immutable prefixes, explicit import, insertion-order-independent encoding and depth limits. Add checked non-root rejection if no equivalent case exists. |
| Import semantics and source qualification | [Provenance tests](../crates/kagari-hir/src/imports/provenance_tests.rs): duplicate logical modules, same-revision relowering, equal glob targets, unused imports and physical prefix sites. |
| Namespace lookup and navigation | [Namespace tests](../crates/kagari-hir/src/analysis/namespace_tests.rs), [identity tests](../crates/kagari-hir/src/analysis/identity_tests.rs): ambiguity, shadowing, installed/source identity, retained snapshots and diagnostics. Preserve cancellation/limit coverage at the existing solver owner. |
| Checked dependency and output boundaries | [Source-program tests](../crates/kagari-compiler/src/tests/source_programs.rs): transitive calls, missing dependencies, same-named imported types, checked snapshot ownership and inline physical origins. Reuse existing canonical artifact/source-free and pinned-reload contracts at their owners. |

Examples of narrow commands, to finalize against the activation revision:

```text
cargo test -p kagari-common --lib identity::table::tests::
cargo test -p kagari-hir --lib imports::provenance_tests::
cargo test -p kagari-compiler --lib source_program_keeps_module_identity_and_resolves_transitive_call_contracts
cargo test -p kagari-compiler --lib dependency_diagnostics_and_function_targets_belong_to_the_checked_snapshot
```

Use name/target filters for the selected additional boundaries; use `--no-fail-fast`
when selecting multiple test targets. A build failure does not count skipped tests
as passed. Reuse passing evidence for unaffected code. Full-workspace tests and
the complete backend/feature/platform matrix remain CI-only, including final
acceptance. Local implementation checkpoints require:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Review imports, visibility/re-exports, module responsibility, effective LOC and
mixed functions manually. Prefer ordinary modules at meaningful boundaries;
record necessary structural debt here without blanket exceptions. Planning-only
documentation edits require content/link/diff checks, not Cargo validation.

Measurement uses a fixed, bounded family with nested module paths, shared imports,
diamonds and cycles, including a retained-snapshot rebuild. Freeze module/edge
counts, path lengths and input sources in MH01. Keep setup/registration and source
parsing separate from graph construction, checked-program/compiler work and final
encoding. Count all new interning and snapshot publication costs rather than
excluding them from the comparison. Reuse existing test preparation facilities;
do not create a second preparation cache or benchmark-only unchecked graph path.

Record baseline/final revisions, toolchain, machine, workspace profile/features,
default target/parallelism, repetitions, cache state and machine load. Measure
allocation count/requested bytes, retained graph plus identity-table memory and
elapsed time; distinguish physical RSS if measured. Show existing-versus-fresh
context behavior and the cost of keeping an old snapshot alive. Temporary probes
and raw logs belong under ignored `target/`; retain reproduction method, workload
definition, results and limitations in tracked documentation.

Acceptance requires removal of the targeted path-bearing internal representation
and a measured comparison, not an invented speedup threshold. If allocation or
time regresses materially, diagnose table copying, retained ownership, repeated
boundary conversion and ordering work before further tuning. Record unresolved
results explicitly; unrelated optimizations cannot substitute for SA9 acceptance.

## Progress ledger and completion

- [ ] MH01: close API/ownership inventory and record matched baseline.
- [ ] MH02: implement checked module-root operations with direct usage.
- [ ] MH03: migrate graph and direct consumers coherently.
- [ ] MH04: complete boundary, deterministic-output and correctness acceptance.
- [ ] MH05: measure, document and hand off.

Planning entry, 2026-10-21: the requested execution plan records the scoped model
and phases against `6fe2cbe6`. Implementation, Rust validation and measurements
have not started. No new integration failure was observed during read-only
planning; this is not evidence that unrun tests pass. Existing CI status remains
with the roadmap. Implementation authorization is not inferred from this entry.
Planning validation: content/scope review, all 22 relative plan links and three
incoming document links passed; `git diff --check` passed. No Cargo checks were
required for this documentation-only checkpoint.

Future entries record changed decisions, focused commands/results, selection
rationale, structural debt and carried errors with their owning phase. Keep this
as the sole SA9 execution ledger; do not create parallel review/measurement queues.

On completion, move lasting ownership/query contracts to architecture and the HIR
guide, measured results to the performance baseline, and unresolved work/CI to
roadmap or review. Remove the resolved SA9 finding, reconcile incoming links and
delete this completed plan. Git history retains phases and successful command logs.
