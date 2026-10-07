# Type/value namespaces and import solving (SA8, SA2, SA3)

Status: active; NR01-NR04 checkpoints are complete locally and NR05 is in progress. The
[roadmap](implementation-roadmap.md#name-resolution-sa8-sa2-sa3-active) owns
activation, phase checkboxes, validation evidence and carried failures. This file
owns the implementation contracts and NR01-NR05 phase boundaries. The user has authorized execution of NR01 through NR05 in order.

## Outcome and scope

Implement two source name spaces, Type and Value, with independent precedence,
conflicts and import outcomes. Derive public namespace members from resolved
bindings. Once those semantics are locally accepted, replace whole-graph import
rescans with dependency-driven work and explicit completion/failure handling.

This implements [SA8](review.md#sa8-separate-typevalue-lookup-and-unify-export-information),
[SA2](review.md#sa2-import-resolution-rescans-every-module) and
[SA3](review.md#sa3-iteration-exhaustion-is-not-distinguished-from-convergence).
The completed [SA4 plan](import-resolution-plan.md) remains historical context;
its qualified identities, provenance and ownership contracts remain in force.
This plan supersedes its single-space APIs and solver policy only in this scope.

Include HIR lowering/resolution/imports, actual declaration/registration consumers,
source queries and reuse, affected compiler handoffs, and validation of changed
portable declaration records. Preserve static typing, access checks, enum variant
ownership, source-free execution and generation-pinned calls.

Exclude SA1 inline AST reuse, SA9 compact module handles, macros, new import syntax,
new constructor forms, broader enum-glob eligibility, package discovery, trait
solver redesign and runtime/JIT optimization. Keep current collection types and
portable definition identities. Do not replace maps for speculative speedups.
No new crate, compatibility aliases or old-artifact reader is required. Keep
format/ABI identifiers without a routine unpublished-version bump; regenerate
only affected disposable fixtures at the SA8 integration checkpoint.

## Starting point and implementation owners

The current `NameTable` keys candidates by spelling alone. Host resolution tries
functions before types. A directive has one outcome, while HIR `Import` stores
`alias`, `alias_explicit` and `glob`. Lowering duplicates public declarations and
imports in `Module.exports`; the builder also consumes its variant entries.
`ModuleGraph::build` rebuilds every module for at most `2N + 2` rounds and publishes
the last catalog even when equality has not established convergence.

A concrete additional consumer matters: portable `ModuleDecl.exports` is a
`BTreeMap<String, I>`. It cannot represent a type and value exported under the same
alias. SA8 must migrate that key and its real consumers, not just HIR lookup. This
is the bounded registration/schema consequence of the agreed two-space contract.
Do not delete portable registration exports when removing redundant HIR exports.

| Owner | Required responsibility |
| --- | --- |
| [HIR import syntax](../crates/kagari-hir/src/hir/item/module.rs), [lowering](../crates/kagari-hir/src/lower/item.rs), [export storage](../crates/kagari-hir/src/hir/item/storage.rs) | Explicit named/glob leaves; preserve physical source spans; remove redundant HIR exports after variant consumers migrate. |
| [Name tables](../crates/kagari-hir/src/resolver/table.rs), [catalog](../crates/kagari-hir/src/imports/catalog.rs), [import facts](../crates/kagari-hir/src/imports/mod.rs) | Category-aware candidate selection, path lookup, directive results and derived public views. |
| [Graph builder](../crates/kagari-hir/src/imports/builder.rs) | Draft ownership, work scheduling, observed dependencies, cycle closure and immutable publication. Split solver responsibilities through normal modules as needed. |
| [Portable module declarations](../crates/kagari-types/src/declaration/module.rs), [native builder](../crates/kagari-runtime/src/native/builder.rs), [native module](../crates/kagari-runtime/src/native/module.rs) | Category-aware exported aliases and registration validation; preserve canonical targets and native symbol uniqueness. |
| [Host lookup](../crates/kagari-hir/src/host.rs), [native rendering](../crates/kagari-hir/src/native/render.rs) | Explicit category selection and complete generated declaration views; preserve checked native default bodies. |
| `resolver/`, `typeck/`, `imports/functions.rs`, `imports/types.rs`, `declarations.rs` | Use-site category selection, checked declarations and imported signatures, including constructors and associated members. |
| `analysis/` and [compiler source](../crates/kagari-compiler/src/source) | Tooling provenance, cache comparison/remapping, checked handoff and stable executable identities. |

Read current [AGENTS.md](../AGENTS.md), [architecture](architecture.md),
[HIR guide](architecture/hir.md), [modules](spec/modules.md),
[syntax](spec/syntax.md), [standard declarations](spec/standard-declarations.md)
and the roadmap ledger before implementation. Check status, relevant diffs and
phase commits; preserve unrelated work. Current code and tests must confirm this
inventory before the first migration edit.

## Required semantic contract

### Binding categories and identity

| Category | Members / use sites |
| --- | --- |
| Type | Structs, enums, traits, opaque/alias types, source/host modules and package aliases; scoped generic type parameters and `Self`. Type annotations, type applications, trait bounds, struct literals and struct patterns select this category. |
| Value | Functions, constants, locals, parameters and existing enum unit/payload constructors. Expressions, calls and enum constructor patterns select this category. |

A struct declaration introduces only a type; do not invent a value constructor
for it. Enum variants retain their declaration/owner identity while introducing
value bindings through the already-supported paths/imports. Ordinary binding
patterns still introduce local values; a two-space split must not reinterpret
every bare identifier pattern as a constructor. Document the existing syntactic
binding-versus-constructor distinction in the updated syntax spec.

The same spelling may exist once in each category. Conflicts within a category
remain errors: two strong bindings conflict even when their targets are equal;
equal-target glob contributions retain all origins. Apply
`strong > glob > implicit` separately. A local value named `Option` cannot mask
the prelude type `Option`; a type binding named `Option` can. Type parameters and
`Self` participate only in their existing lexical/type scopes.

For `prefix::terminal`, resolve prefixes through the Type category, then require
a namespace-capable target. Choose the terminal category from its use site.
A value named `m` cannot hide module `m` in `m::member`; a non-namespace type named
`m` blocks that prefix with an appropriate diagnostic. Bare `m` in an expression
still selects a value. Preserve root rules for `self`, `super`, `crate` and
installed packages. Do not retry another category after missing, inaccessible,
ambiguous or non-namespace results. Dot member access and trait/inherent selection
remain the type checker's responsibility.

Associated types select Type; associated constants/methods select Value. Preserve
receiver type inference and qualified associated projections. Classification does
not grant arbitrary namespace traversal through trait or generic type targets.

The intended source behavior can be observed without internal table assertions:

```text
mod api {
    pub struct Token { pub val value: i32 }

    pub fn Token() -> i32 { 40 }
}

use self::api::Token as Item;

fn main() -> i32 {
    val instance: Item = Item { value: 2 };
    Item() + instance.value
}
```

Here one use leaf introduces both bindings. The annotation/literal select the
struct, the call selects the function, and execution returns 42. Two functions
named `Token` still conflict; a local value `Item` shadows only the value binding.
This example specifies intended behavior for NR01, not behavior already available
at the planning checkpoint.

Canonical source/host/definition identities remain authoritative. Category is a
binding/query discriminator, not a new definition ID format. Audit all maps keyed
only by spelling, including generated names and diagnostic aggregation. Native
linkage symbols and callable ABI identities retain their independent uniqueness
requirements even when a source type and function have the same spelling.

### Imports, visibility and errors

Keep one directive per actual use leaf. A named import resolves both terminal
categories independently and binds every applicable target under its explicit
alias or default final path segment. A glob resolves one namespace container and
enumerates both categories. Preserve existing namespace eligibility for globs.

Per-category draft outcomes must distinguish pending, proven absence, resolved,
inaccessible and ambiguous. Keep malformed/non-namespace/stale path errors
explicit; never convert them to absence to unlock fallback.

| Named import outcome | Required behavior |
| --- | --- |
| Resolved in both categories | Bind both canonical targets under the same local spelling. |
| Resolved in one, proven absent in the other | Valid import; the absent category does not reserve a local name. |
| Absent in both | One unresolved-import diagnostic at the leaf. |
| Pending in a category | Reserve the strong local name in that category; suppress premature glob/prelude selection until settled. |
| Inaccessible or ambiguous in either category | Preserve that diagnostic/blocking outcome even if the other resolves; no successful category hides the failure. |

Failure of a shared path prefix blocks the directive; per-category absence applies
to a successfully reached terminal container. At final publication, no Pending
outcome is allowed. Stable invalid imports remain recoverable analysis facts with
diagnostics and cannot enter checked code generation.

Check visibility at every explicit path component. Re-exports can expose an
otherwise public target through a private implementation module, but cannot widen
the target's permitted visibility. Preserve facade edges, target dependencies,
unused imports, grouped leaf/root spans and selected binding origins. Globs omit
members disallowed at their requested export visibility, as today.

### Required representation and APIs

Use explicit categories at every semantic lookup boundary. The following shapes
specify distinctions, not mandatory public names or new re-export facades:

```rust
enum NameNamespace { Type, Value }

struct PerNamespace<T> { types: T, values: T }

// Each NameEntry still owns strong/glob/implicit candidates.
type NameEntries = BTreeMap<LocalName, PerNamespace<NameEntry>>;

enum ImportSyntax {
    Named { explicit_alias: Option<String> },
    Glob,
}

enum ImportResolution {
    Named(PerNamespace<LookupOutcome>),
    Glob(NamespaceLookupOutcome),
}

struct ExportName { namespace: NameNamespace, name: String }
```

Because portable registrations also need the category, own the minimal shared
`NameNamespace`/`ExportName` under `kagari-types` declaration semantics. HIR owns
candidate state, lookup outcomes and source provenance. Keep `PerNamespace<T>`
with its actual owner; do not move a generic convenience abstraction to common.
Use `BTreeMap<ExportName, I>` for portable exported aliases and validate that each
key's category matches its resolved target. Audit mapping, rendering, encoding,
registration, foundation providers and source-free verification consumers.

Require a category argument on `lookup`, `lookup_member` and terminal path lookup.
Provide an explicit two-category operation for imports and tooling. Remove
unqualified first-match entrypoints rather than defaulting them to Value or Type.
`NamespaceId` continues to identify the container; it is not `NameNamespace`.
Preserve catalog/table Arc ownership without graph or snapshot back-references.

Derive HIR public-member views from resolved, visibility-bearing bindings,
including named and glob re-exports. Migrate enum variant seeding directly from
owning enum/variant facts before removing `Export`, `ExportItem`, `ExportBuffer`
and `Module.exports`. Portable registration aliases remain authored declarations;
they project into the same HIR catalog instead of creating another source export
truth. Do not flatten child namespaces or fabricate use directives.

### Tooling and incremental reuse

Resolution sites retain the category and selected origins separately from the
canonical target. Type and value use sites navigate to their corresponding
identity. At a use leaf introducing both categories, expose both targets to
multi-result tooling; any existing single-result query returns no arbitrary
winner. Completion can show both with distinct kind/identity; references to one
must not attach to the other. Prefix navigation remains attached to its physical
source range.

Compare both binding categories, import outcomes, visibility, provenance,
dependencies and provider revision in declaration/signature/file/body reuse.
Adding/removing just one category must invalidate affected queries while retaining
the other's correct identity. Preserve SA5 transitive namespace comparisons.
Current-module arena rebasing uses existing explicit remappers; foreign source
units retain full file/revision/arena identity. Old snapshots keep their own
immutable catalog and observations. No pointer-sharing assertion substitutes for
incremental-versus-fresh semantic agreement.

## Solver contract (after the SA8 local gate)

### Ownership and scheduling

The builder owns mutable drafts for one immutable input snapshot. Key drafts and
work items by complete `SourceUnit`, preserving duplicate logical modules and
their diagnostics. Seed declarations and installed namespaces once. Maintain a
stable queue plus a membership set; queue order cannot choose a binding winner.
Recompute one affected module's contributions, replacing its old contributions
instead of appending candidates on each visit. Publish only changed catalog entries.

Record lookup observations during resolution, including unsuccessful and pending
lookups, every namespace prefix traversed, and namespace membership enumerated by
globs. Maintain reverse watchers independently of the final linking dependency
set. Missing names need watchers on the container where they could appear; missing
module/package prefixes need a watched input-universe dependency. Replace stale
watchers when paths retarget. Register observations before processing subsequent
updates, so no arrival can be missed.

A changed target, candidate tier, ambiguity, accessibility, absence, origin or
public-member set schedules affected dependents. Conservative namespace-level
watchers are sufficient; per-name optimization is optional. Stable unrelated
modules must not be rescanned on every propagation step. Provenance is a finite
set of real directive/declaration edges; cyclic forwarding must not grow nested
origin chains without bound.

A new source/provider revision starts from current authoritative inputs and new
drafts. Do not carry stale contributions or watchers across revisions as a new
incremental graph cache. Existing snapshot reuse remains responsible for validated
reuse. Deletion, visibility narrowing, duplicate modules and removal of one name
category must therefore be visible to the solver and downstream caches.

### Completion, cycles and bounded failure

Queue exhaustion alone does not prove absence or completion. Pending explicit
imports can suppress globs that would otherwise reveal names. A closed-cycle
analysis must distinguish a valid seeded cycle from a cycle with no target and
from a temporary lack of candidates. Use an explicit pending-dependency graph;
SCC analysis is permitted for closing these groups, without imposing SCC scheduling
on every acyclic update.

At quiescence, settle only absences/unresolved cycles justified by the closed
pending dependency graph and known namespace contents. Any released reservation
or changed selection must enqueue observers and resume propagation. Absence may
be final only after all possible contributors for that category are settled; do
not choose it from insertion order or one empty pass. Valid cycles with declaration
seeds and acyclic alias/glob chains must resolve. An unseeded unresolved cycle is
an ordinary import diagnostic, not successful lookup and not solver exhaustion.

Before implementing this closure step, write its state transitions and argument
for progress beside the solver. Candidate discovery and precedence selection are
not automatically monotone: releasing a reservation can activate a weaker tier.
Do not claim termination merely because candidates or modules are finite. Explain
how pending groups settle or why another round has distinct justified work. Keep
an explicit work bound as a safety backstop, with documented accounting for draft
visits/candidate work and an input-size-based default; it is not a proof that all
valid programs fit. The old `2N + 2` round count is not that proof.

Normal completion requires drained work, settled directives, and consistent
published bindings/dependencies. Exceeding the work bound returns a distinct
structured solver-limit failure; detecting a non-progressing state returns a
non-convergence failure. Choose one bounded detector (for example, verified
repetition of pending-group semantic state); hash equality alone is insufficient.
Do not retain unbounded full-catalog histories. Cancellation remains a separate
control-flow outcome and is polled inside scans, propagation and cycle handling.

Migrate the current `Result<ModuleGraph, Cancelled>` boundary and callers to carry
these outcomes explicitly. Never return the latest partial draft as a completed
graph, seal it as checked input or install it into successful reuse caches. Analysis
must report the failure at the responsible imports; retained older snapshots remain
valid. Stable graphs containing ordinary source diagnostics are distinct from a
solver that failed to establish a fixed point.

## Implementation refinements

NR02: generated native aliases retain a validated category per use-leaf slot.
The renderer emits authored exports first in key order; exact syntax validation
binds these slots to registration metadata before attaching them to lowering.
Ordinary source use leaves still import both categories. This prevents an authored
Type-only native alias from accidentally exporting a same-spelled Value target.
The category participates in signature reuse. Portable alias eligibility remains
its existing traits/types/variants set; SA8 does not add new registration kinds.
The old HIR `ExportItem::Variant` branch had no producer: registered variant exports
already render ordinary named use leaves. Removing that branch preserves seeding
through those leaves and canonical variant identities.

NR04: namespace observations form the explicit dependency graph, with a reverse
watcher set separate from linking dependencies. At a drained queue the union of
all pending directive/category owners is closed: every observation outside it is
stable in this immutable input universe. Close this union at once rather than
materializing individual SCCs. Closing permits pending lookups to become absent;
resolved seeds remain eligible, and changed selections resume watcher propagation.
This avoids repeated whole-input rounds while preserving closure requirements.

Draft candidates now retain the complete `LookupOutcome`, not `Option<Target>`:
ambiguity and access failures must never be reinterpreted as pending or absence.
Pending glob membership is a draft barrier to weak tiers. Strong conflicts settle
both directives and their candidate records only after participating reservations
settle; a pending category may still prove absent and release its slot. Publication checks both layers for
remaining pending state.

A candidate additionally retains one finite acyclic derivation (a set of real
`(DirectiveId, NameNamespace)` keys). A lookup for a directive excludes derivations already containing
that directive/category; equal-target origins remain separate and complete.
The category is essential: Value resolution may legitimately traverse an already
resolved Type alias introduced by the same use leaf. This is needed
because closing mutually blocking named slots can expose globs, then manufacture
a self-supporting strong target whose winner depends on schedule. Prefix and glob
container/member derivations compose, and the shortest available equal-target
proof is retained. Proof state is solver evidence, not a new canonical identity or
cache-equivalence requirement. A genuine fixed point succeeds; oscillation returns
non-convergence rather than an arbitrary target. Unseeded absence remains ordinary
source diagnostics. Existing seeded-cycle coverage also checks reversed input order.

The safety bound counts module evaluations and produced binding contributions;
its default derives from modules, use leaves and seeded spellings. An eight-entry
history compares exact facts, observations, queue and closed keys, without trusting
hash equality. Cancellation, exhaustion and non-convergence propagate through HIR
and the SDK; failures retain bounded physical import sites and never publish a
successful graph or declaration cache entry. This refines implementation mechanics,
without changing import syntax, precedence, access or the SA1/SA9 exclusions.

## Ordered execution phases

### NR01: Define two-space contracts and replace storage

- Update module/syntax/registration specifications with the category table, examples
  and error rules above. Keep architecture descriptions clearly staged until code lands.
- Introduce shared category/export keys, per-category name tables and directive
  results, and explicit HIR named/glob syntax. Migrate registration/export schema
  consumers and declaration/variant collection directly.
- Replace semantic lookup signatures and host first-match policy. Keep the existing
  whole-round solver schedule temporarily; do not mix scheduling changes into SA8.
- Update affected existing declaration/import/host contract fixtures. Attempt
  `cargo check -p kagari-hir` and the narrowest affected registration checks.

Exit: the intended model is the sole storage/API model. Unmigrated consumer build
errors may be carried only to NR02 under the checkpoint policy below; no adapters.
Suggested commit: `refactor(hir)!: separate type and value name bindings`.

### NR02: Migrate semantic consumers, exports, tooling and reuse

- Migrate type/body/pattern/associated queries, imported contracts, compiler
  handoff and all remaining consumers to explicit category selection.
- Derive public members from resolved bindings; delete redundant HIR export storage.
  Preserve variant ownership and portable authored exports.
- Migrate navigation/completion/reference provenance, native generated views and
  all cache comparison/remapping paths together. Preserve checked native defaults.
- Resolve every NR01 build failure and run selected import, host and snapshot
  contracts. Update HIR architecture diagrams/Rustdoc at changed boundaries.

Exit: affected consumers build; focused SA8 semantics and cache behavior pass.
Suggested commit: `refactor(hir)!: migrate namespace consumers and export views`.

### NR03: Accept SA8 locally before changing the solver

- Review the focused contract matrix below against actual existing owners; replace
  obsolete cross-category conflict assertions instead of accumulating test wrappers.
- Verify cross-space source/native aliases, deterministic identities, tooling and
  incremental-versus-fresh behavior. Exercise one affected SDK source/artifact path
  and one relevant native/fallback consumer; do not run the complete matrix locally.
- Regenerate affected disposable declaration/artifact fixtures once if their layout
  changed. Retain source-free validation evidence through the designated CI jobs.
- Close all local SA8 failures and record the remaining SA2/SA3 round-policy debt.

Exit: SA8 implementation/local acceptance complete; CI acceptance reported
separately. NR04 may start after this local gate without claiming unrun CI success.
Suggested commit: `test(hir): consolidate two-space resolution contracts`.

### NR04: Replace rescans and make solver completion explicit

- Capture a baseline for the bounded module workloads below at NR03.
- Implement draft ownership, reverse observations, deduplicated scheduling and
  closed pending-group handling. Update affected graph-build error consumers.
- Remove the all-module round loop and silent last-pass publication. Preserve
  deterministic visibility, conflicts, identities, origins and direct dependencies.
- Reuse chain/diamond/cycle/edit/cancellation contracts. Force the work bound low
  through a scoped internal test option to verify failure and cache isolation;
  this fixture represents the solver boundary, not each implementation branch.

Exit: SA2/SA3 focused behavior passes, no pending graph is published, stable
unrelated modules are not repeatedly processed, and no carried build errors remain.
Suggested commit: `refactor(hir)!: resolve imports with dependency-driven work`.

### NR05: Integrate, measure and record acceptance

- Check phase removals, module responsibilities, error propagation, public surfaces
  and documentation against the final implementation. Resolve remaining local failures.
- Compare NR04 with the recorded NR03 workloads; report work counts and timing
  separately. Investigate concrete regressions within the solver scope.
- Ensure existing GitHub CI covers the changed contracts and feature consumers;
  record actual CI run/commit/results when available. Fix attributable failures.
- Update review findings and roadmap status, distinguishing implemented/local
  acceptance from pending or passed CI acceptance. Retain SA1/SA9 as separate work.

Exit: no local carried failures; full acceptance only when the required CI checks
pass on the final implementation. Without an available CI run, record CI pending
rather than pretending the entire acceptance gate is complete.
Suggested commit: `test(hir): verify import solver boundaries and integration`.

## Focused acceptance and measurement

Reuse existing fixtures under `imports/`, `analysis/`, `host/`, SDK `source_modules`,
`syntax_examples`, `registration_sources` and artifact/native consumers. Add a row
only for a distinct uncovered language/core boundary. This is a coverage map, not
a demand for one new test per cell or every phase.

| Contract | Required distinguishing observations |
| --- | --- |
| Categories/precedence | Type+value coexist; same-category strong conflict; equal-target glob origins retained; prelude type survives value shadowing. |
| Paths/construction | Type prefixes; type literal/pattern vs value call/enum constructor; local binding patterns; associated category selection and unchanged access checks. |
| Named/glob imports | Both targets; one-category absence; both absent; pending reservation; inaccessible/ambiguous category; grouped alias spans, facades and unused dependency edges. |
| Host/registration | Same spelling across categories; per-category duplicate rejection; portable alias validation; ABI symbol uniqueness; generated default calls still checked. |
| Queries/reuse | Category-correct navigation and completion; add/remove/retarget one category through a transitive facade; incremental equals fresh; old snapshots and foreign arenas remain valid. |
| Solver | Long named/glob chains, fan-out/diamond, valid seeded cycles, unresolved cycles, changes/removals, duplicate modules and order independence. |
| Failure/publication | Cancellation and forced exhaustion do not publish/cache drafts; non-convergence is distinct from unresolved source names; healthy later analysis succeeds. |
| Executable boundary | Checked lowering retains canonical targets; encoded products validate and execute without source; selected native path verifies actual-native execution where supported, fallback otherwise. |

During development select a few exact existing tests/contract filters in affected
crates. Use targeted Clippy/check commands where needed, plus structure, formatting
and `git diff --check` at implementation checkpoints. Record exact commands and
results in the roadmap; do not repeatedly run unchanged successful checks.

GitHub CI owns `cargo test --workspace`, strict workspace/all-target Clippy,
`scripts/check_features.py`, CLI JIT and the complete backend/load matrices under
[the current workflow](../.github/workflows/ci.yml). Run none of those full matrices
locally and do not split them into package runs to bypass this boundary. Document
an unobserved CI result as pending. Plan-only edits need local links/content and
diff checks, without a workspace build.

For solver measurements use the same generated source set at NR03 and NR04:
long alias/glob chains, a fan-out/diamond, a seeded cycle and unrelated modules
alongside one active propagation chain. Include removal/retarget analysis in a
fresh revision. Record input sizes, toolchain, machine, profile, features, default
Cargo parallelism/target directory and cache state. Separate Rust compilation,
source preparation and graph solving; report draft visits, changed entries and
candidate work alongside elapsed time. Keep logs/workloads under ignored `target/`
and durable generation commands/conclusions in the roadmap. Instrumentation is
scoped test/measurement support, not a new production telemetry subsystem. The
required claim is less unrelated rescan work; speedup magnitude requires measured
evidence and is not a predetermined acceptance percentage.

## Checkpoints and resumption

On activation, start at the first unfinished NR phase in the roadmap. Each phase
updates that ledger and produces one coherent Conventional Commit with trailer
`Resolution-Phase: NR01` through `Resolution-Phase: NR05`. Use `!` and explain
changed internal APIs/portable layouts when applicable. No remote push is implied.
The documentation-only planning commit carries no implementation phase trailer.

Only NR01 may end with explicitly listed mechanical consumer compilation failures
owned by NR02. Record the command, representative diagnostic, cause and exact
consumer in the ledger and disclose the broken build in the commit body. NR02
must repair them before NR03; later phases do not carry known local build/test
failures. A successful single filter is not evidence that other attempted failing
checks recovered. Never add production stubs, ignore tests, relax validation or
retain the old semantic implementation to manufacture a green checkpoint.

Routine naming/container choices are autonomous. Record any adjustment to these
semantic, ownership or query contracts with its cause in this plan and the
roadmap. A real blocker involving new syntax, SA1/SA9, package policy or another
excluded architecture requires a concrete scope decision; do not silently expand
this migration. Keep CI acceptance status independent of implementation progress.
