# Agent Instructions

Kagari is a statically typed, GC-backed scripting language in Rust, prioritizing
host embedding, explicit execution semantics, tooling and hot reload. These rules
apply repository-wide.

## Early Development Policy

Kagari is unpublished. Prefer working implementation and clear architecture over
release/compatibility workflows unless released consumers or the user require them.
This overrides older plans' routine version bumps, artifact regeneration and
exhaustive checkpoint validation; correctness and assertions are not negotiable.

- Replace obsolete APIs/models directly. Do not retain hypothetical compatibility:
  old callers/readers, aliases, forwarding crates, duplicate entrypoints or semantics.
- Keep format/runtime ABI identifiers; bump only for publication, an explicit
  compatibility commitment or a user request, not ordinary unpublished changes.
- Artifacts/caches are disposable. Invalidate affected products when contracts/layouts
  change; batch fixture regeneration while preserving source-free/behavioral coverage.
- Keep scope finite and tied to concrete requirements. Record gaps in the existing
  plan; make substantial scope changes explicit, without unrelated migrations or
  continually expanding checklists.

## Authority and Task Context

- User instructions and authorized scope override repository defaults.
- [Goals](docs/project_goal.md) define direction, [architecture](docs/architecture.md)
  defines ownership, [roadmap](docs/implementation-roadmap.md) locates the active plan,
  and `docs/spec/` defines behavior. The active plan owns phases, scope, acceptance,
  phase-specific instructions and progress.
- At task start or direction changes, read relevant docs and implementation/tests;
  otherwise reuse context. Resume from the plan and commit history. Before editing,
  inspect Git status/relevant diffs and preserve unrelated work.
- For bugs, reuse a failure or focused reproduction when practical. Documentation
  and mechanical edits need no failing test.

## Engineering Priorities

Order: correctness/observable semantics, testability, host boundary safety, reload
correctness, maintainability, measured performance. Keep changes cohesive and
reviewable, with explicit ownership and the plan's migration/build boundaries.

### Architecture Before Optimization and Feature Changes

- Review responsibilities, data flow, ownership/lifetimes, invariants and execution
  paths before proposing or implementing features/optimizations. Distinguish
  architectural defects, local defects and unavoidable semantic costs; make local
  fixes when boundaries fit.
- Replace unsuitable models coherently. Retain sound earlier optimizations, merge
  overlaps and remove superseded workarounds. Benchmark wins grant no exemption;
  temporary bridges need a removal checkpoint.
- For architecture/performance decisions, consult mature implementations and primary
  sources: Rust for typed lowering/compiler organization, OpenJDK for managed-runtime/
  GC boundaries, or another runtime with a matching execution model. Record mechanism,
  source/version, ownership rules and differing assumptions in the active plan;
  justify custom designs. Syntax similarity is not runtime evidence. Preserve Kagari
  semantics and scale research to the decision; routine fixes need no new study.
- State evidence, root cause/uncertainty, proposed ownership/model changes, affected
  mechanisms and correctness/performance validation. Missed performance targets
  require architectural diagnosis before further local optimization.
- Caches/specialized paths need an owner, validity/lifetime contract and measured or
  concrete justification. Do not hide repeated preparation, duplicate semantics or
  misplaced validation; preserve guarantees when moving checks.
- Avoid speculative abstractions, empty future-use crates and unrelated rewrites.
  Keep material decisions in the existing plan and migrations finite.

### Explaining Problems and Design Changes

- Explain context and causality for readers unfamiliar with the source. Use a small
  input/operation scenario showing relevant layers and result/cost; compare old/new
  behavior and preserved guarantees on the same example.
- Keep examples proportional, label sketches/hypothetical numbers, and use code or
  diagrams when useful. Analogies and citations support explanation, not replace it
  or prove correctness/performance.

### Code Structure

- Split by crate/module responsibility. Keep facades/`lib.rs` for entrypoints and
  orchestration, feature policy out of generic loops, and mixed responsibilities
  out of `common` merely to break dependency cycles.
- Rust limit: 1200 effective LOC, including tests/tracked generated source. Exclude
  blank/comment-only lines; count mixed lines once. Parse comments lexically, not
  markers inside strings. Raw line counts are not effective LOC.
- Split oversized files at meaningful boundaries. Cohesive tables/generated source/
  fixtures may use documented, bounded exceptions when splitting harms clarity or
  correctness. Review changed responsibilities, record debt in the plan, and avoid
  unrelated refactoring; size alone does not justify a split.
- Use ordinary modules: no handwritten `include!`; reserve it for generated code.
  Use `#[path]` only for justified test/cross-target sharing.
- Separate adjacent definitions with a blank line before attached comments/attributes:
  functions/methods, trait signatures, types/traits/impls, inline modules, extern blocks
  and tests. Internal blanks do not count. Compact import, out-of-line module,
  type-alias and constant groups may be contiguous.
- Split hard-to-follow control flow/ownership. Group growing arguments cohesively;
  prefer explicit enums/focused handlers to conditional chains mixing policies.
- Write comments/API/repository docs in English; converse in the user's language.

### Documentation maintenance

- Keep current contracts, guides, actionable findings and active/queued plans.
  On completion, move lasting behavior to its architecture/spec owner and unresolved
  work to roadmap/review, then remove the plan. Git history owns completed ledgers,
  command logs and superseded reports.
- Link tracked files relatively or public sources. Do not depend on ignored logs,
  generated reports or local probes; retain necessary methods/results/limits in docs.
- Rustdoc: short summary, behavior, ownership/ID targets, errors and useful examples.
  For syntax/HIR, map source examples to fields/children; distinguish AST nodes/tokens,
  filtered positions, optional children and malformed recovery. Keep examples at their
  owner and link shared contracts. Rust examples should compile; use `text` for Kagari
  and tree sketches.

### Imports and Module Paths

- Use explicit module-scope imports and meaningful short module names; avoid long
  use-site paths. Group related imports with nested braces, preserving scopes,
  attributes and comments. Use short qualification/meaningful collision aliases.
- No production globs, including grouped/local imports and re-exports. Only test-only
  scopes may use globs; production built by `cargo test`, examples and benchmarks
  still follow production rules.
- No repeated `super::super::` in production; import from the owner through `crate::`.
  One `super::` may name a direct parent; tests should also avoid deep traversal.
- Local imports need a concrete reason, e.g. configuration scoping. Correctness-required
  qualification (trait ambiguity, macro hygiene, derive/attribute paths) and generated
  absolute paths are allowed, not a reason for routine long handwritten paths.
- Do not add forwarding modules, broad re-exports, widened visibility or compatibility
  aliases to shorten paths. Import the owner; keep intentional facades explicit.
- Every `pub use`, including restricted/test-only declarations, needs an exact file/
  declaration entry in `reexport-whitelist` in `scripts/structure-exceptions.toml`,
  supported by consumer/ownership evidence. Roots and `mod.rs` are not exempt.

### Structural Review at Checkpoints

Before each implementation checkpoint:

1. Review changed handwritten Rust imports/paths, including local/nested scopes and
   macro tokens. Classify test/generated scopes explicitly; a file containing tests
   is not wholly test-only.
2. Review ownership, visibility/re-exports, `include!`/`#[path]`, effective LOC and mixed
   function responsibilities; fix globs, deep traversal and long use-site paths.
3. Run the structure check listed below. It covers the repository without a baseline.
   Resolve findings or justify narrow LOC/re-export exceptions with review evidence;
   debt entries are not exemptions. Never add blanket allowances.

The [checker](docs/structure-checks.md) does not expand macros or resolve names;
passing it or an [exception](scripts/structure-exceptions.toml) does not prove sound
architecture. Record debt/follow-up owners in the plan. Run `--self-test` when changing
it.

### Replacement and Migration

- Preserve ABI/schema/version, declared-access, bounds and handle validation. Update
  consumers/examples/tests without weakening meaningful coverage to hide failures.
- Follow the plan's intermediate build/test policy and check affected architecture
  boundaries. Record failing commands, representative diagnostics, cause and owning
  follow-up phase; disclose broken builds in commit bodies. Run focused tests when
  affected units build; do not repeat unchanged failures awaiting their owning phase.
- Resolve integration errors through the intended architecture: no fake success,
  disabled validation, unchecked input or production `todo!()` stubs.
- Final acceptance requires carried errors resolved and required checks passing in
  their designated local/CI scope. Phase completion is not a passing-build or CI claim.

## Kagari Semantic Boundaries

- Preserve specified typing, generics/traits, checked numerics, shared objects and
  Result/Option propagation.
- Rust host state stays outside the script heap. Use installed interfaces/typed paths;
  never expose unrestricted Rust references or bypass scoped borrow validation.
- Preserve left-to-right once-only evaluation, mutation commits, trap order and
  completed effects. Readonly views do not prevent mutation through another alias.
- Keep ownership/generation checks, explicit roots and cleanup on traps, cancellation,
  depth exhaustion and synchronous host reentry.
- Preserve generation-pinned calls/dependencies across reload. Reflection stays within
  declared metadata/access; no runtime type mutation or monkey-patching around
  versioned publication.
- Keep cooperative cancellation in loops, calls and long native operations. Do not
  reintroduce execution charging or generic permission matrices; follow the active
  execution-policy plan's trusted-script boundary.
- Follow plan sequencing/dependency constraints: executable contracts cannot depend
  on source analysis; backends consume checked facts, not re-resolve syntax/types.

## Verification and Tooling

### Test additions and consolidation

- Inspect the semantic owner, fixtures and existing coverage before adding tests.
  Identify a contract and plausible failure not already detected. A new function,
  bug fix, alternate spelling or expected constant alone does not justify a new case.
- Prefer strengthening existing scenarios. Add cases for distinct uncovered rules,
  boundaries or lifecycles; explain non-obvious additions in the plan/change summary,
  not a separate report. Discard temporary reproductions after checking coverage.
- Assert relevant values, error kinds, state transitions and cleanup, not merely
  success/any error when those cannot distinguish the rule. Negative inputs must
  reach the intended layer, not pass on unrelated syntax/setup/admission failures.
- Keep detailed matrices at their owner; upper layers sample integration/propagation.
  Expand combinations for distinct mechanisms or concrete interaction risks, not
  mechanical products of spellings, positions, sizes, historical versions or backends.
- Do not permanently test removed behavior merely because it once existed. Verify
  migrations temporarily; retain old API/syntax/model/version rejection only for an
  uncovered current contract or safety boundary, using representative invalid inputs.
- Consolidate equivalent cases at their owner; do not hide duplicates in one test.
  Retire weaker smoke/migration copies after preserving unique assertions. Share
  invariant preparation, but keep fresh mutable state and required routes; wrapping
  duplicate preparations in a loop is not consolidation.
- Preserve independent oracles even when production verification checks the invariant.
  Avoid assertions that merely mirror private structure or reproduce implementation.
- Preserve distinct backend, source-free, actual-native, ownership/GC, borrowing,
  cancellation, reload and process/platform contracts. Record sampled-out combinations;
  sampling is not zero-loss coverage. Counts/coverage percentages are not targets.

### Validation and execution

- Locally run a small affected contract set plus directly impacted regressions, using
  package/target/name filters and scoped builds. For cross-cutting changes, sample
  representative boundaries; record selection rationale and unrun CI scope in the plan.
- Full-workspace tests and exhaustive feature/backend/platform matrices are CI-only,
  even for final acceptance or split into package commands. This overrides older plans.
  Workspace Clippy is required locally for implementation checkpoints/final acceptance;
  scoped Clippy is optional, not a substitute.
- After failures, batch repairs and rerun only failing/affected cases. Complete selected
  tests skipped by build failure/early exit; use `--no-fail-fast` for multiple targets.
  Reuse passing evidence for unchanged code, not a fresh all-green transcript. Broaden
  checks only when new changes/failures show more impact.
- Report actual local results, known failures and pending CI separately; unrun checks
  are not passed. Documentation-only changes need content/link/diff checks, not builds
  or Clippy unless requested.
- Use workspace Cargo profiles, default `target`/parallelism and `uv run python`.
  Put temporary logs/measurements under ignored `target/`; retain conclusions,
  reproduction commands and known errors in the plan/relevant docs.
- Performance reports include toolchain, machine, profile, features, parallelism,
  cache state and workload. Separate compile/run costs; claim only measured speedups.

Local implementation acceptance (also CI):

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Run `git diff --check` at every checkpoint and preserve line endings. Full-workspace
`cargo test --workspace --no-fail-fast` and exhaustive matrices remain CI-only.

## Progress and Commits

- Update the existing plan when scope, design, implementation or carried errors change;
  do not create parallel queues. Report actual validation and remaining limits; do not
  mark goals complete with required work/integration failures outstanding.
- Commit only when requested. Use Conventional Commits, imperative subjects preferably
  under 72 characters: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`,
  `chore`, with optional scope. Mark breaking API/format changes with `!` and explain
  impact; include active-plan phase trailers on implementation checkpoints.
- Exclude unrelated work; never amend/rewrite user commits without permission.

## Task Description Template

```text
Task: Concrete change.
Context: Active phase and module/crate owners.
Expected behavior: Observable outcome and preserved invariants.
Scope: Owned work and explicit deferrals.
Validation: Selected commands and behavioral evidence.
Known intermediate errors: Cause, reproduction and owning follow-up phase.
```
