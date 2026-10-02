# Agent Instructions

Kagari is a statically typed, GC-backed scripting language implemented in Rust.
Its priorities include host embedding, explicit execution semantics, tooling and
hot reload. This document defines repository-wide engineering and workflow rules.

## Early Development Policy

Kagari is currently unpublished and in early development. Prioritize fast,
working implementation with a clear architecture. Do not apply release engineering
or compatibility workflows without an actual released consumer or an explicit
user requirement. This policy supersedes older plan requirements for routine
version bumps, repeated artifact regeneration and exhaustive checkpoint validation.

- Replace internal APIs and data models directly. Do not preserve obsolete
  callers, formats or artifacts for hypothetical compatibility.
- Keep format and runtime ABI identifiers, but do not increment them for each
  unpublished internal API or schema change. Establish a version boundary when
  publishing or making an explicit compatibility commitment, or when requested
  by the user. Ordinary native function additions and Rust implementation changes
  do not automatically require a format or ABI version bump.
- Treat development artifacts and caches as disposable. Invalidate or regenerate
  affected products when their layout or contracts change; do not add old-format
  readers. Batch necessary fixture updates at a coherent checkpoint instead of
  rebuilding all products after every incremental edit. Preserve meaningful
  source-free and behavioral coverage.
- Use focused checks for the changed behavior during development. Run full
  workspace checks and feature/backend matrices at final integration, or when
  broad impact or a concrete failure justifies them. Do not repeat unchanged
  successful checks or known failures at every small checkpoint.
- Keep the authorized scope finite. Do not silently expand a library task into
  unrelated architecture migration or continually add checklist items. Implement
  shared capabilities only for concrete requirements; record material gaps
  concisely in the existing plan and make substantial scope changes explicit.
- Keep correctness, static typing, ABI/schema validation, declared access, bounds,
  roots, cleanup and generation checks. Faster development does not permit fake
  success, weakened assertions or executing unvalidated input.

## Authority and Task Context

- Follow the user's current instructions and previously authorized scope. When
  they conflict with repository workflow defaults, the user's instructions win.
- Use [docs/project_goal.md](docs/project_goal.md) for product direction,
  [docs/architecture.md](docs/architecture.md) for architecture, and
  [docs/implementation-roadmap.md](docs/implementation-roadmap.md) to find the
  active execution plan. The relevant `docs/spec/` files define language behavior.
- The active execution plan owns phase order, scope, acceptance criteria and the
  progress ledger. Keep phase-specific instructions in that plan.
- Read the relevant documents when starting a task or changing direction. Reuse
  that context during the same task; reread when documents change or an unresolved
  design decision requires it. Inspect `git status` and the relevant diff before
  editing, and preserve unrelated user work.
- Inspect relevant implementation and tests before selecting a concrete unit of
  work. For bug fixes, use an existing failure or a focused reproduction when
  practical. Do not require a failing test for documentation or mechanical edits.
- Use the active plan's ledger and commit history when resuming work.

## Engineering Priorities

Correctness and observable semantics come first, followed by testability, host
boundary safety, hot reload correctness, maintainability and measured performance.
Prefer cohesive, reviewable changes with explicit responsibility boundaries.
For structural migrations, follow the active plan's phase size and intermediate
build policy.

### Code Structure

- Split code by crate and module responsibility. Keep `lib.rs` and facades focused
  on public entrypoints and orchestration, not accumulated feature implementations.
- Use 1200 effective lines of code (LOC) as the default Rust file size threshold.
  Exclude blank lines and comment-only lines, including Rust documentation comments
  and block comments. Count a line
  containing both code and a comment once. Recognize comments lexically: comment
  markers inside string literals are not comments. Do not use raw file line counts
  as effective LOC.
- Split oversized files at meaningful responsibility boundaries. A cohesive
  exhaustive table, generated source or fixture may justify keeping a larger file
  if splitting would reduce clarity or correctness. Document that reasoning and a
  bounded LOC exception under the structure-check policy; size alone is not proof
  that a split improves the design. Tests and tracked generated sources are checked.
- Review affected files as their responsibilities change and record unresolved
  structural debt in the active plan. Keep unrelated refactoring outside task scope.
- Use normal Rust `mod` boundaries for handwritten source. Reserve `include!` for
  generated code and `#[path]` for justified test or cross-target sharing.
- Follow the import/path rules and structural review below for handwritten Rust.
- Split functions when control flow or ownership becomes difficult to follow.
  Group growing argument sets into cohesive parameter types where that clarifies
  the contract. Prefer explicit enums and focused handlers over conditional chains
  that mix unrelated policies.
- Keep feature-specific policy out of generic execution loops. Do not move mixed
  responsibilities into `common` merely to make a dependency cycle disappear.
- Adjust an incorrect boundary instead of repeatedly patching around it. Avoid
  speculative abstraction and empty crates created only for future features.
- Write source comments, API documentation and repository documents in English.
  Use the user's language for conversation and progress updates.

### Imports and Module Paths

- Declare dependencies through explicit `use` statements at module scope. Import
  the type/function or a short, meaningful module name instead of repeating long
  `crate::...`, external-crate or `std::...` paths in signatures and function bodies.
  For example, import `std::sync::Arc` and `crate::error::RuntimeError`, then write
  `Arc<RuntimeError>` rather than
  `std::sync::Arc<crate::error::RuntimeError>` throughout the implementation.
- Group related imports from the same crate or module with nested braces, such as
  `use kagari_common::{Span, host_interface::HostTypeDeclaration};`, instead of
  repeating the same prefix in separate statements. Preserve distinct scopes,
  conditional attributes and comments when grouping; keep every imported item explicit.
- Root-qualified paths in imports are encouraged: `use crate::module::Type;` makes
  ownership clear. This rule limits verbose paths at use sites, not explicit paths
  in the import declarations themselves.
- Short qualification such as `fmt::Display`, `io::Result` or `hir::Expr` is useful
  when it clarifies ownership. Resolve collisions with meaningful aliases or module
  imports; do not replace one unreadable path with an opaque abbreviation.
- Do not use wildcard imports in production, including `use module::*`, grouped
  glob imports, function-local `use Enum::*`, and `pub use module::*`. Explicitly
  list imported/re-exported items; keep enum variants qualified where helpful.
- Test-only scopes may use wildcard imports, such as `use super::*` inside a
  `#[cfg(test)]` module or a dedicated integration-test target. Compiling ordinary
  production code with `cargo test` does not make its imports test-only. Examples
  and benchmarks should follow the production import style.
- Do not use repeated parent traversal such as `super::super::` or longer chains
  in production imports, signatures or bodies. Use an explicit `crate::...` import
  for the owning module instead. A single `super::` is acceptable for a direct
  parent relationship; tests should avoid deep traversal as well.
- Keep normal imports at module scope. Function-local imports need a concrete
  reason, such as feature/configuration scoping, rather than hiding a function's
  dependency list or enabling wildcard matching.
- Qualification required for correctness is allowed: ambiguous trait calls such
  as `<Type as Trait>::method`, macro hygiene, and clearly scoped derive/attribute
  paths such as `thiserror::Error`. Generated code may require absolute paths.
  These exceptions do not justify routine fully qualified paths in handwritten
  implementation code.
- Do not introduce forwarding modules, broad re-exports, widened visibility or
  compatibility aliases just to shorten imports. Import from the actual owner or
  fix the responsibility boundary. Keep intentional public facades explicit.
- Re-exports (`pub use`, including restricted visibility and test-only scopes)
  are forbidden by default. An intentional API boundary requires an exact
  file/declaration whitelist entry under `reexport-whitelist` in
  `scripts/structure-exceptions.toml`, with reviewed consumer/ownership evidence.
  Library roots and `mod.rs` receive no automatic exemption. Import from the
  actual owner; do not add forwarding modules or aliases to shorten paths.

### Structural Review at Checkpoints

Review changed handwritten Rust modules before each implementation checkpoint:

1. Check production `use` and `pub use` declarations, including nested/local
   imports, for globs. Classify test-only/generated code explicitly rather than
   treating the entire file as exempt because it contains tests.
2. Check for repeated `super::` traversal and long qualified paths at use sites;
   replace them with explicit imports or short module qualification as appropriate.
3. Check module ownership, visibility/re-export growth, handwritten `include!`,
   unjustified `#[path]`, effective LOC and large functions mixing responsibilities.
4. Run `uv run --locked scripts/check_structure.py`. Record existing debt in the
   active plan with its reason and follow-up owner. The check covers the whole
   repository without a grandfathering baseline. Resolve findings or justify a
   narrowly scoped LOC/re-export exception with evidence in code review; a generic
   debt entry is not an exemption. Never use blanket allowances to make CI green.

The [structure checker](docs/structure-checks.md) parses Rust syntax and checks
imports, paths, the re-export whitelist and effective LOC without building the
workspace. Its documented scope excludes macro expansion and semantic name
resolution. Review macro token trees, module ownership and unnecessary public
surface manually; a passing syntax check does not replace architectural review.
The [exception policy](scripts/structure-exceptions.toml) records justified cases;
the checker validates scope and limits but cannot prove the design rationale.
When changing the checker, run its `--self-test` suite as well.

### Replacement and Migration

- Replace obsolete internal models directly. Do not add compatibility aliases,
  forwarding crates, duplicate public entrypoints, old artifact readers or a second
  semantic implementation solely to preserve superseded callers.
- Keep runtime ABI, schema, version, declared-access, bounds and handle checks. Removing
  compatibility support does not permit executing unvalidated input.
- Update examples, consumers and tests to the intended model. Preserve meaningful
  behavioral coverage; do not remove tests or weaken assertions to conceal failures.
- Follow the active plan's policy on intermediate compilation/test failures.
  Record the command, representative diagnostics, cause and owning follow-up phase
  in its ledger; disclose a broken build in the commit body.
- Resolve integration errors through the intended architecture. Do not introduce
  fake success, disabled validation or production `todo!()` stubs to pass checks.
- Final acceptance requires all carried build/test errors to be resolved and all
  documented checks to pass. Track phase scope separately from build status.

## Kagari Semantic Boundaries

- Preserve existing static typing, generics, traits, checked numeric behavior,
  shared object semantics and Result/Option propagation under their specifications.
- Host-owned Rust state remains outside the script heap. Script access uses the
  installed host interfaces and typed paths; never expose unrestricted Rust
  references through script values or bypass scoped borrow validation.
- Preserve left-to-right, once-only evaluation, mutation commit guarantees, trap
  order and already-completed side effects. A readonly view does not prove that
  another alias cannot modify its referent.
- Keep runtime ownership/generation checks, explicit roots and cleanup on traps,
  cancellation, call-depth exhaustion and synchronous host reentry.
- Preserve generation-pinned calls and dependency versions across hot reload.
  Reflection must remain within declared metadata and member access rules, without runtime
  type mutation or monkey-patching that bypasses versioned publication.
- Preserve cooperative cancellation through loops, calls and long native operations.
  Do not reintroduce execution charging or generic permission matrices; the active
  execution-policy plan defines the trusted-script boundary.
- Follow the active plan's production dependency constraints: executable contracts
  must not depend on source analysis, and backends must consume checked facts
  rather than resolve syntax or infer types again.
- Implement features within the active task's scope and the roadmap's sequencing.

## Verification and Tooling

- Run checks appropriate to the change. Reuse valuable subsystem and conformance
  tests; add tests for meaningful behavior or boundaries, not just implementation
  structure. Documentation-only edits need link/content and diff checks, not a
  full workspace rebuild.
- Run focused tests when affected units build. At intermediate architecture
  boundaries, attempt relevant checks and record failures honestly; avoid repeating
  unchanged known failures while their owning migration step is still pending.
- Use the build profiles defined in the workspace `Cargo.toml`, the default
  `target` directory and Cargo's default build parallelism.
- Python is managed through `uv`; use `uv run python` when Python is needed.
- Store temporary logs and generated measurement output under ignored `target/`.
  Record durable conclusions, reproduction commands and known errors in the active
  plan or relevant docs so cache cleanup does not erase resumption state.
- For performance work, record toolchain, machine, profile, features, parallelism,
  cache state and workload. Separate compilation time from execution time and
  avoid speed claims unsupported by measurements.

Final architecture validation includes the plan's feature/behavior matrix and:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Use `git diff --check` at every checkpoint and preserve repository line-ending
conventions.

## Progress and Commits

- Update the active plan's checklist and ledger when implementation advances,
  a design decision changes, or validation discovers a carried error. Use existing
  documents rather than creating parallel progress/decision queues.
- At task completion, state what changed, validation actually performed, and any
  remaining limitations. Do not report a goal complete while its required work or
  final integration failures remain.
- Commit coherent authorized checkpoints using Conventional Commits:
  `<type>(optional-scope): <description>`. Prefer imperative subjects under 72
  characters, with `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`
  or `chore` as appropriate.
- Mark breaking API/format changes with `!` and explain their impact. Use the
  phase trailers required by the active plan on implementation checkpoints.
- Keep unrelated changes out of the checkpoint. Do not amend or rewrite user
  commits unless requested.

## Task Description Template

```text
Task: Implement a concrete change.
Context: Active phase and relevant crate/module boundaries.
Expected behavior: Observable outcome and invariants to preserve.
Scope: What this task owns and what remains deferred.
Validation: Relevant commands and behavioral evidence.
Known intermediate errors: Cause, reproduction and owning follow-up phase.
```
