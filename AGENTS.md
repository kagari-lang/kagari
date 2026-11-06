# Agent Instructions

Kagari is a statically typed, GC-backed scripting language in Rust, prioritizing
host embedding, explicit execution semantics, tooling and hot reload. These rules
apply repository-wide.

## Early Development Policy

Kagari is unpublished. Prioritize working implementation and clear architecture;
use release/compatibility workflows only for released consumers or explicit user
requirements. These rules override older plans' routine compatibility,
regeneration and validation workflows.

- Replace obsolete internal APIs/models directly. Do not retain compatibility
  aliases, forwarding crates, old readers/callers, duplicate entrypoints or second
  semantic implementations for hypothetical consumers.
- Keep format/runtime ABI identifiers; bump them only for publication, an explicit
  compatibility commitment or a user request, not ordinary unpublished changes.
- Treat development artifacts/caches as disposable. Invalidate or regenerate affected
  products when layouts/contracts change; batch fixture updates at coherent checkpoints
  while preserving meaningful source-free and behavioral coverage.
- Keep scope finite and tied to concrete requirements. Record gaps and material
  decisions in the existing plan; make substantial scope changes explicit. Avoid
  speculative abstractions, empty future-use crates and unrelated migrations.

## Authority and Task Context

- User instructions and previously authorized scope override repository defaults.
- [Project goals](docs/project_goal.md) define direction, [architecture](docs/architecture.md)
  defines ownership, [roadmap](docs/implementation-roadmap.md) locates the active plan,
  and `docs/spec/` defines language behavior. The active plan owns phase order, scope,
  acceptance, phase-specific instructions and the progress ledger.
- Read relevant documents at task start or direction changes; reuse context unless
  it changes or unresolved questions require rereading. Before editing, inspect
  `git status` and relevant diffs; preserve unrelated work.
- Inspect implementation/tests before selecting work; resume from the plan's ledger
  and commit history. For bugs, reuse a failure or focused reproduction when practical;
  documentation/mechanical edits need no failing test.

## Engineering Priorities

Order: correctness/observable semantics, testability, host boundary safety, hot reload
correctness, maintainability, measured performance. Make cohesive, reviewable changes
with explicit responsibilities; follow the plan's migration size/build policy.

### Architecture Before Optimization and Feature Changes

- Review responsibilities, data flow, ownership/lifetimes, invariants and execution
  paths before recommending or implementing features/optimizations. Distinguish
  architectural defects, local defects and unavoidable semantic costs; when the
  architecture is sound, make the local fix.
- Replace unsuitable boundaries coherently. Reassess earlier optimizations on affected
  paths: retain sound foundations, merge overlaps and remove superseded workarounds.
  Prior benchmark wins grant no exemption; temporary bridges need a removal checkpoint.
- For architecture/performance decisions, consult mature implementations before designing
  mechanisms or local workarounds. Use primary sources: Rust for typed lowering/compilers,
  Java/OpenJDK for GC/runtime boundaries, or others with matching execution models.
  Record mechanism, source/version, ownership rules, differing assumptions and reasons
  for custom designs in the active plan. Syntax similarity is not runtime evidence;
  preserve Kagari's semantic boundaries. Routine local fixes need no comparative study.
- Recommendations must explain evidence, root cause/uncertainty, ownership/model changes,
  affected mechanisms and correctness/performance validation. Missed performance targets
  require renewed architectural diagnosis before further local optimization.
- Caches/specialized paths need a semantic owner, validity/lifetime contract and measured
  or concrete justification. They must not hide repeated preparation, duplicate semantics
  or misplaced validation. Preserve guarantees when relocating checks.

### Explaining Problems and Design Changes

Explain context and causality for readers unfamiliar with the source. When useful,
walk one small scenario through input, relevant layers and result/cost; use the same
example to compare current/proposed behavior and preserved guarantees. Keep examples
proportional; label simplified code, diagrams and hypothetical numbers. Links and
analogies support explanation, not replace it or serve as proof alone.

### Code Structure

- Split by crate/module responsibility. Keep `lib.rs`/facades for public entrypoints
  and orchestration; keep feature policy out of generic loops. Do not move mixed
  responsibilities into `common` merely to break a dependency cycle.
- Limit Rust files to 1200 effective LOC, including tests/tracked generated source,
  excluding blank/comment-only lines. Follow the [counting and bounded-exception rules](docs/structure-checks.md#effective-loc);
  split at meaningful boundaries, not merely to satisfy a raw line count. Cohesive
  tables/generated source/fixtures may justify an exception; record structural debt
  and its follow-up owner in the active plan.
- Use `foo.rs` with children in `foo/`, never `foo/mod.rs`, including tests/helpers.
  Keep Cargo entrypoints in their usual form. For helpers directly under `tests/`,
  set `autotests = false` and declare the actual `[[test]]` targets.
- Use ordinary modules. Handwritten `include!` is forbidden; reserve it for generated
  code and `#[path]` for justified test/cross-target sharing.
- Separate adjacent definitions with a blank line before attached comments/attributes:
  functions/methods, trait signatures, structs/enums/unions/traits/impls, inline modules,
  extern blocks and tests. Internal blanks do not count. Compact import, out-of-line
  module, type-alias and constant groups may be contiguous.
- Split functions when control flow/ownership is hard to follow. Group growing arguments
  into cohesive types; prefer explicit enums/focused handlers over mixed-policy chains.
- Write comments, API docs and repository docs in English; use the user's language
  for conversation/progress.

### Documentation maintenance

- Keep current contracts, guides, actionable findings and active/queued plans. On
  completion, move lasting behavior to architecture/specifications and unresolved work
  to the roadmap/review, then remove the completed plan. Git history owns phase ledgers,
  successful command logs and superseded reports.
- Use repository-relative links to tracked files or public sources. Record necessary
  methods, results and limitations in docs; do not depend on ignored logs, generated
  reports or local probes.
- Rustdoc starts with a short summary, then behavior, ownership/ID targets, errors and
  relevant examples. Map concrete syntax/HIR source examples to fields/children; AST
  diagrams distinguish nodes/tokens, filtered positions, optional children and recovery.
  Keep examples beside their owner and link shared contracts. Rust examples should
  compile; mark Kagari/tree sketches as `text`.

### Imports and Module Paths

- Prefer explicit module-scope imports and meaningful short module names over long
  paths in signatures/bodies; root-qualified imports such as `use crate::module::Type;`
  are encouraged. Group related imports with nested braces while preserving scopes,
  conditional attributes and comments. List items explicitly; use meaningful aliases.
- No production globs, including grouped/local enum imports and glob re-exports.
  Only test-only scopes may use globs; compiling production code with `cargo test`
  does not exempt it. Examples/benchmarks follow production rules.
- No repeated `super::super::` traversal in production; import from the owner through
  `crate::...`. One `super::` may name a direct parent; tests should avoid deep traversal.
- Function-local imports need a concrete reason, such as configuration scoping.
  Correctness-required qualification (ambiguous trait calls, macro hygiene, derive/
  attribute paths) and generated absolute paths are allowed.
- Do not add forwarding modules, broad re-exports, widened visibility or compatibility
  aliases to shorten paths. Import the owner or fix the boundary; keep facades explicit.
- Every `pub use`, including restricted/test-only re-exports, needs an exact file/
  declaration entry under `reexport-whitelist` in [structure-exceptions.toml](scripts/structure-exceptions.toml)
  with consumer/ownership evidence. Library roots and module entry files are not exempt.

### Structural Review at Checkpoints

Review changed handwritten Rust against the structure/import rules above, including
nested scopes and explicit test/generated classification. The [checker](docs/structure-checks.md)
is required under Verification; it audits the whole repository without a baseline.
Resolve findings or justify narrow LOC/re-export exceptions with review evidence;
debt entries and blanket allowances cannot waive violations. Review macro tokens,
ownership, function responsibilities and unnecessary public surface manually: passing
syntax checks or approved exceptions do not prove architectural soundness. Run the
checker's `--self-test` suite when changing it.

### Replacement and Migration

- Preserve ABI/schema/version validation, declared-access, bounds and handle checks.
  Update examples/consumers/tests; never weaken assertions or remove meaningful coverage
  to hide failures.
- Follow the plan's intermediate build/test policy. Attempt relevant checks at architecture
  boundaries and focused tests when affected units build. Record carried failures with
  commands, representative diagnostics, cause and owning follow-up phase in its ledger;
  disclose broken builds in commit bodies.
- Resolve integration errors through the intended architecture, never fake success,
  disabled validation, unvalidated input or production `todo!()` stubs.

## Kagari Semantic Boundaries

- Preserve static typing, generics/traits, checked numeric behavior, shared object
  semantics and Result/Option propagation.
- Host Rust state stays outside the script heap. Use installed interfaces/typed paths;
  never expose unrestricted Rust references or bypass scoped borrow validation.
- Preserve left-to-right once-only evaluation, mutation commits, trap order and completed
  side effects. Readonly views do not prevent mutation through another alias.
- Keep runtime ownership/generation checks, explicit roots and cleanup on traps,
  cancellation, depth exhaustion and synchronous host reentry.
- Follow [reload call selection](docs/spec/module-activation.md#live-installations-and-call-selection):
  ordinary calls select current implementations; created closures/Futures and admitted
  frames/queued invocations retain their selected bodies/providers and type environments.
  Their ordinary inner calls still select current implementations. Reflection stays within
  declared metadata/member access; no runtime type mutation or publication bypass.
- Preserve cooperative cancellation in loops, calls and long native operations under
  the [execution contract](docs/spec/execution.md). Do not reintroduce execution charging
  or generic permission matrices.
- Follow production dependency constraints: executable contracts cannot depend on source
  analysis; backends consume checked facts instead of resolving syntax or inferring types.

## Verification and Tooling

- Organize tests by semantic contracts: grammar, typing, observable behavior, artifact/ABI
  validation, GC cleanup, host borrowing, cancellation and reload. Reuse existing owners/
  fixtures; add cases only for distinct uncovered boundaries, not each changed function,
  past bug or spelling. Discard temporary reproductions once existing coverage suffices.
- Remove duplicate smoke tests, obsolete migration checks and assertions merely mirroring
  private structure; consolidate equivalent cases without hiding duplicate lists. Retain backend,
  source-free and ownership coverage. Test counts/coverage percentages are not targets.
- Locally run a small affected contract-test set plus representative dependent regressions,
  using package/target/test-name filters; scope standalone builds similarly. Record the
  selection rationale and uncovered CI scope in the active plan.
- Full-workspace tests and complete feature/backend/platform matrices are CI-only,
  including final acceptance; do not reproduce them locally through split package runs.
  This overrides older plans. Workspace Clippy remains required locally; scoped Clippy
  during iteration does not replace it at implementation acceptance.
- After failures, batch related repairs and rerun failing/affected cases. Complete skipped
  tests in the selected scope; build failures/early exits are not passes. Use `--no-fail-fast`
  for multiple selected targets. Reuse passing evidence for unaffected code and do not
  repeat unchanged known failures awaiting their owning phase. Broaden only for new impact.
- Documentation-only edits need content/link/diff checks, not builds or Clippy unless requested.
- Use workspace Cargo profiles, default `target` and default parallelism; use `uv run python`
  for Python. Store temporary logs/measurements under ignored `target/`, retaining necessary
  conclusions, reproduction commands and known errors in the owning plan/docs.
- Performance reports record toolchain, machine, profile, features, parallelism, cache state
  and workload; separate compilation/execution and make only measured speed claims.

Local implementation checkpoint/final checks (also run in CI), alongside selected tests:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Run `git diff --check` at every checkpoint; preserve repository line endings.

## Progress and Commits

- Update the existing plan's checklist/ledger when implementation, decisions or carried
  errors change; do not create parallel queues. Track phase scope separately from build status.
- Report changes, actual validation and remaining limits, separating local results, known
  failures and pending CI. Completion requires required work finished, carried/integration
  errors resolved and checks passed in their designated local/CI scope; unrun CI is never a pass.
- Use Conventional Commits: `<type>(optional-scope): <description>`, preferably imperative
  and under 72 characters. Use `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`,
  `ci` or `chore`. Mark breaking API/format changes with `!` and explain their impact;
  include active plan phase trailers on implementation checkpoints. Exclude unrelated
  changes; never amend/rewrite user commits unless requested.

## Task Description Template

```text
Task: Implement a concrete change.
Context: Active phase and relevant crate/module boundaries.
Expected behavior: Observable outcome and invariants to preserve.
Scope: What this task owns and what remains deferred.
Validation: Relevant commands and behavioral evidence.
Known intermediate errors: Cause, reproduction and owning follow-up phase.
```
