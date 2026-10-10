# Agent Instructions

Kagari is a statically typed, GC-backed scripting language in Rust, prioritizing
host embedding, explicit execution semantics, tooling and hot reload. These rules
apply repository-wide.

## Early Development Policy

Kagari is unpublished. Prioritize working implementation and clear architecture;
use release/compatibility workflows only for released consumers or explicit user
requirements. This policy supersedes older plans' routine version bumps, repeated
artifact regeneration and exhaustive checkpoint validation.

- Replace obsolete internal APIs/models directly. Do not retain old callers,
  formats/readers, compatibility aliases, forwarding crates, duplicate public
  entrypoints or second semantic implementations for hypothetical compatibility.
- Keep format/runtime ABI identifiers, but bump them only for publication, an
  explicit compatibility commitment or a user request. Unpublished API/schema
  changes, ordinary native additions and Rust changes do not require a bump.
- Treat development artifacts/caches as disposable. Invalidate or regenerate affected
  products when layouts/contracts change; batch fixture updates at coherent checkpoints
  and preserve meaningful source-free and behavioral coverage.
- Keep scope finite: build shared capabilities for concrete requirements, record gaps
  in the existing plan and make substantial scope changes explicit. Do not silently
  expand tasks into unrelated migrations or continually add checklist items.
- Follow the focused/local-final/CI split under Verification and Tooling. Faster
  development never permits weaker correctness, validation or assertions.

## Authority and Task Context

- User instructions and previously authorized scope override repository defaults.
- [Project goals](docs/project_goal.md) define direction, [architecture](docs/architecture.md)
  defines ownership, [roadmap](docs/implementation-roadmap.md) locates the active plan,
  and `docs/spec/` defines language behavior. The active plan owns phase order, scope,
  acceptance, phase-specific instructions and the progress ledger.
- Read relevant documents at task start or direction changes; reuse context unless
  documents change or unresolved design questions require rereading. Before editing,
  inspect `git status` and relevant diffs; preserve unrelated work.
- Inspect implementation/tests before selecting work. For bugs, reuse a failure or
  focused reproduction when practical; documentation/mechanical edits need no failing
  test. Resume from the plan's ledger and commit history.

## Engineering Priorities

Order: correctness/observable semantics, testability, host boundary safety, hot reload
correctness, maintainability, measured performance. Make cohesive, reviewable changes
with explicit responsibilities; follow the active plan's migration size/build policy.

### Architecture Before Optimization and Feature Changes

- Before recommending or implementing optimizations/features, review architecture and
  implementation: responsibilities, data flow, ownership/lifetimes, invariants and
  execution paths. Distinguish architectural defects, local defects and unavoidable
  semantic costs before proposing fixes.
- Replace unsuitable boundaries/models coherently. Review earlier optimizations on
  affected paths: retain sound foundations, merge overlaps and remove superseded
  workarounds. Prior benchmark wins grant no exemption; temporary migration bridges
  need a removal checkpoint.
- Where useful, consult mature language/runtime designs through primary documentation
  or source. Explain their fit and differing assumptions for Kagari's typing, GC,
  host boundaries, reload and execution semantics; do not copy incompatible models
  or infer unmeasured speedups.
- Recommendations must state evidence, root cause/uncertainty, intended ownership/model
  changes, affected mechanisms and correctness/performance validation. Missed performance
  targets require renewed architectural diagnosis before more local optimization.
- Caches/specialized paths require a semantic owner, validity/lifetime contract and
  measured or concrete justification. They must not conceal repeated preparation,
  duplicate semantics or misplaced validation. Preserve guarantees when relocating checks.
- Scale review to the task: when architecture is sound, explain and make the local fix.
  Avoid speculative abstractions, empty future-use crates and unrelated rewrites;
  keep migrations finite and material decisions in the existing plan.

### Explaining Problems and Design Changes

- Include concrete examples when helpful. Assume readers have not read the source
  or have seen only fragments; explain context and causality without requiring them
  to reconstruct the architecture from names, links or isolated details.
- Use a small scenario: input/operation, steps through relevant layers, result/cost.
  Compare current/proposed behavior and preserved guarantees using the same example.
  Short code, pseudocode or diagrams may help; label simplifications/hypothetical
  numbers. Keep examples proportional and connect them to the reason. Analogies and
  source references support explanation, not replace it or serve as proof alone.

### Code Structure

- Split by crate/module responsibility. Keep `lib.rs`/facades for public entrypoints
  and orchestration. Keep feature policy out of generic loops; do not move mixed
  responsibilities into `common` merely to break a dependency cycle.
- Default Rust limit: 1200 effective LOC, including tests/tracked generated source.
  Exclude blank/comment-only lines (including doc/block comments); count mixed
  code/comment lines once. Parse comments lexically: markers in strings are not
  comments. Raw line counts are not effective LOC.
- Split oversized files at meaningful boundaries. Cohesive exhaustive tables,
  generated source or fixtures may warrant a documented, bounded LOC exception when
  splitting harms clarity/correctness; size alone does not prove a better design.
  Review changed responsibilities, record structural debt in the active plan and
  keep unrelated refactoring out of scope.
- Use ordinary Rust modules; handwritten `include!` is forbidden. Reserve `include!`
  for generated code and `#[path]` for justified test/cross-target sharing.
- Separate adjacent definitions with a blank line: functions/methods, trait signatures,
  structs/enums/unions/traits/impls, inline modules, extern blocks and tests. Put it
  before the next item's attached comments/attributes; internal blanks do not count.
  Compact import, out-of-line module, type-alias and constant groups may be contiguous.
- Split functions when control flow/ownership is hard to follow. Group growing arguments
  into cohesive parameter types when useful; prefer explicit enums/focused handlers
  over conditional chains mixing policies.
- Write comments, API docs and repository docs in English; use the user's language
  for conversation/progress. Handwritten Rust follows the import/review rules below.

### Imports and Module Paths

- Use explicit module-scope imports of types/functions or meaningful short module
  names; avoid long qualified paths in signatures/bodies. For example, import `Arc`
  and `RuntimeError`, then use `Arc<RuntimeError>`. Root-qualified imports such as
  `use crate::module::Type;` are encouraged.
- Group related imports with nested braces, preserving distinct scopes, conditional
  attributes and comments; list each item explicitly. Use short qualification such
  as `fmt::Display` and meaningful aliases for collisions, not opaque abbreviations.
- No production globs, including grouped globs, local `use Enum::*` or `pub use module::*`.
  Test-only scopes (`#[cfg(test)]` modules/integration tests) may use globs; building
  production code with `cargo test` does not exempt it. Examples/benchmarks follow
  production rules.
- No repeated `super::super::` traversal in production imports/signatures/bodies;
  import from the owner through `crate::...`. One `super::` may name a direct parent;
  tests should also avoid deep traversal.
- Function-local imports need a concrete reason, such as configuration scoping.
  Correctness-required qualification (ambiguous trait calls, macro hygiene, derive/
  attribute paths like `thiserror::Error`) and generated absolute paths are allowed;
  they do not justify routine long paths in handwritten code.
- Do not add forwarding modules, broad re-exports, widened visibility or compatibility
  aliases to shorten paths. Import the actual owner or fix the boundary; keep
  intentional public facades explicit.
- All `pub use`, including restricted/test-only re-exports, require an exact file/
  declaration entry under `reexport-whitelist` in `scripts/structure-exceptions.toml`
  with reviewed consumer/ownership evidence. Library roots and `mod.rs` are not exempt.

### Structural Review at Checkpoints

Before each implementation checkpoint, review changed handwritten Rust:

1. Check all production imports/re-exports (including nested/local scopes) for globs;
   classify test/generated scopes explicitly, not whole files containing tests.
2. Replace repeated parent traversal/long use-site paths with explicit imports or
   short qualification. Review ownership, visibility/re-exports, `include!`, `#[path]`,
   effective LOC and functions mixing responsibilities.
3. Run `uv run --locked scripts/check_structure.py`. It checks the whole repository
   without a grandfathering baseline. Resolve findings or justify narrow LOC/re-export
   exceptions with review evidence. Record debt's reason/follow-up owner in the plan;
   a debt entry is not an exemption. Never add blanket allowances to pass CI.

The [checker](docs/structure-checks.md) parses imports, paths, re-export whitelists and
LOC without building; it excludes macro expansion/semantic name resolution. Review
macro tokens, ownership and unnecessary public surface manually. A passing check or
validated [exception](scripts/structure-exceptions.toml) does not prove architectural
soundness. Run the checker's `--self-test` suite when changing it.

### Replacement and Migration

- Preserve ABI/schema/version, declared-access, bounds and handle validation while
  replacing obsolete models. Update examples/consumers/tests; never remove meaningful
  coverage or weaken assertions to hide failures.
- Follow the plan's intermediate build/test policy. Record failing commands,
  representative diagnostics, cause and owning follow-up phase in its ledger;
  disclose broken builds in commit bodies. Attempt relevant checks at intermediate
  architecture boundaries; run focused tests when affected units build. Do not
  repeat unchanged known failures pending their owning phase.
- Resolve integration errors through the intended architecture, never fake success,
  disabled validation, unvalidated input or production `todo!()` stubs.
- Final acceptance requires all carried errors resolved and documented checks passing
  in their designated local/CI scope. Track phase scope separately from build status;
  never equate local validation with unrun CI acceptance.

## Kagari Semantic Boundaries

- Preserve specified static typing, generics/traits, checked numeric behavior, shared
  object semantics and Result/Option propagation.
- Host Rust state stays outside the script heap. Use installed interfaces/typed paths;
  never expose unrestricted Rust references or bypass scoped borrow validation.
- Preserve left-to-right once-only evaluation, mutation commits, trap order and
  completed side effects. Readonly views do not prevent mutation through another alias.
- Keep runtime ownership/generation checks, explicit roots and cleanup on traps,
  cancellation, depth exhaustion and synchronous host reentry.
- Preserve generation-pinned calls/dependencies across reload. Reflection stays within
  declared metadata/member access; no runtime type mutation or monkey-patching around
  versioned publication.
- Preserve cooperative cancellation in loops, calls and long native operations.
  Do not reintroduce execution charging or generic permission matrices; the active
  execution-policy plan defines the trusted-script boundary.
- Follow the plan's production dependency constraints: executable contracts cannot
  depend on source analysis; backends consume checked facts instead of resolving
  syntax or inferring types again. Follow task scope and roadmap sequencing.

## Verification and Tooling

- Organize tests by grammar, static typing, observable semantics and core boundaries:
  artifact/ABI validation, GC ownership/cleanup, host borrowing, cancellation and pinned
  reload. Changes do not automatically need new tests. Reuse the existing contract
  owner/fixtures; add cases only for distinct uncovered rules/boundaries, not merely a
  past bug, changed function or alternate spelling. Discard temporary reproductions
  after checking existing coverage where appropriate.
- Remove duplicate smoke tests, obsolete migration checks and assertions mirroring
  private structure. Consolidate equivalent cases at their semantic owner, without
  hiding a growing duplicate list in one test. Keep backend/source-free/ownership
  cases that establish distinct contracts; counts/coverage percentages are not targets.
- Routine feature iteration, bug fixes and phase follow-ups run a small affected
  contract-test set and lightweight checks. Reuse subsystem/conformance tests; do not
  run full workspace suites, even split across commands, at incremental checkpoints.
  Documentation-only edits need content/link/diff checks, not workspace builds.
- Local full-workspace validation is allowed only after an entire large task (such
  as a multi-phase feature/migration), not a single intermediate phase. Batch it at
  final acceptance; fix failures with focused tests and repeat the full run only to
  establish acceptance, never after every fix or after unchanged successful checks.
- GitHub CI owns complete architecture and feature/backend acceptance. This policy
  overrides both older blanket bans on local full runs and unconditional full-suite
  lists. Report focused local checks, final local runs and CI status separately.
- Use workspace Cargo profiles, the default `target` and default build parallelism.
  Use `uv run python` for Python. Put temporary logs/measurements under ignored
  `target/`; retain conclusions, reproduction commands and known errors in the active
  plan/relevant docs so cleanup does not erase resumption state.
- Performance reports record toolchain, machine, profile, features, parallelism,
  cache state and workload; separate compilation/execution and make only measured
  speed claims.

CI checks below may also serve local final acceptance of a completed large task;
they are not a routine iteration checklist. A local pass does not establish CI or
its full feature/backend matrix passed.

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Run `git diff --check` at every checkpoint; preserve repository line endings.

## Progress and Commits

- Update the active plan's checklist/ledger when implementation, design decisions or
  carried errors change; do not create parallel progress/decision queues.
- On completion, report changes, actual validation and remaining limits. Do not mark
  goals complete with required work or final integration failures outstanding.
- Commit coherent authorized checkpoints using Conventional Commits:
  `<type>(optional-scope): <description>`. Prefer imperative subjects under 72 characters
  and `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci` or `chore`.
- Mark breaking API/format changes with `!`, explain their impact and include active
  plan phase trailers on implementation checkpoints. Exclude unrelated changes;
  never amend/rewrite user commits unless requested.

## Task Description Template

```text
Task: Implement a concrete change.
Context: Active phase and relevant crate/module boundaries.
Expected behavior: Observable outcome and invariants to preserve.
Scope: What this task owns and what remains deferred.
Validation: Relevant commands and behavioral evidence.
Known intermediate errors: Cause, reproduction and owning follow-up phase.
```
