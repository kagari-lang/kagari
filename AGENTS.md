# Agent Instructions

Kagari is a statically typed, GC-backed scripting language implemented in Rust.
Its priorities include host embedding, explicit execution semantics, tooling and
hot reload. These instructions adapt Vela's engineering practices to Kagari;
Vela's language restrictions and milestone queue do not apply here.

## Authority and Task Context

- Follow the user's current instructions and previously authorized scope. When
  they conflict with repository workflow defaults, the user's instructions win.
- Use [docs/project_goal.md](docs/project_goal.md) for product direction,
  [docs/architecture.md](docs/architecture.md) for architecture, and
  [docs/implementation-roadmap.md](docs/implementation-roadmap.md) to find the
  active execution plan. The relevant `docs/spec/` files define language behavior.
- The active architecture track is
  [docs/mir-architecture-refactor.md](docs/mir-architecture-refactor.md).
  It owns crate migration, phase order, progress and known integration errors.
  The completed R01-R18 foundation track is historical, not a second work queue.
- Read the relevant documents when starting a task or changing direction. Reuse
  that context during the same task; reread when documents change or an unresolved
  design decision requires it. Inspect `git status` and the relevant diff before
  editing, and preserve unrelated user work.
- Inspect relevant implementation and tests before selecting a concrete unit of
  work. For bug fixes, use an existing failure or a focused reproduction when
  practical. Do not require a failing test for documentation or mechanical edits.
- Creating or updating a plan does not start goal mode. Start a goal only when
  explicitly requested. Use the plan's ledger and commit history when resuming.

## Engineering Priorities

Correctness and observable semantics come first, followed by testability, host
boundary safety, hot reload correctness, maintainability and measured performance.
Prefer cohesive, reviewable changes with explicit responsibility boundaries.
For an approved structural migration, follow its phase size and intermediate-build
policy rather than forcing every file move into a separately runnable checkpoint.

### Code Structure

- Split code by crate and module responsibility. Keep `lib.rs` and facades focused
  on public entrypoints and orchestration, not accumulated feature implementations.
- Treat 1200 lines as the ordinary handwritten implementation/test file review
  threshold. When a changed file exceeds it, split at meaningful boundaries or
  record a specific reason in the active plan's ledger or architecture docs.
  Generated output and cohesive exhaustive tables/fixtures can justify exceptions;
  exceptions do not authorize unrelated growth.
- Existing large files are structural debt, not a reason to stop an unrelated task
  or start a repository-wide split. During MIR migration, review affected files as
  their responsibilities move, and record unresolved debt for the final audit.
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

### Structural Review at Checkpoints

Review changed handwritten Rust modules before each implementation checkpoint:

1. Check production `use` and `pub use` declarations, including nested/local
   imports, for globs. Classify test-only/generated code explicitly rather than
   treating the entire file as exempt because it contains tests.
2. Check for repeated `super::` traversal and long qualified paths at use sites;
   replace them with explicit imports or short module qualification as appropriate.
3. Check module ownership, visibility/re-export growth, handwritten `include!`,
   unjustified `#[path]`, file size and large functions mixing responsibilities.
4. Record any justified exception or existing debt in the active plan with its
   reason and follow-up owner. Resolve new violations in the changed code; do not
   hide them with broad lint allowances or claim that formatting checks catch them.

Text searches are useful audit inputs, not a complete Rust-aware checker: imports
may span multiple lines, contain nested groups, or appear inside string fixtures.
Review results in context. Do not launch unrelated repository-wide source rewrites
for a documentation task. During the MIR migration, apply this review as modules
move and include a full structural audit in A05. Intermediate compilation failures
do not prevent import/path review and do not require compatibility scaffolding.

### Replacement and Migration

- Replace obsolete internal models directly. Do not add compatibility aliases,
  forwarding crates, duplicate public entrypoints, old artifact readers or a second
  semantic implementation solely to preserve superseded callers.
- Keep runtime ABI, schema, version, permission, bounds and handle checks. Removing
  compatibility support does not permit executing unvalidated input.
- Update examples, consumers and tests to the intended model. Preserve meaningful
  behavioral coverage; do not remove tests or weaken assertions to conceal failures.
- During MIR phases A01-A04, compilation/test failures are explicitly allowed at
  checkpoints. Record the command, representative diagnostics, cause and owning
  follow-up phase in the plan ledger; disclose a broken build in the commit body.
- Continue authorized structural work despite documented intermediate failures.
  Do not introduce adapters, fake success, disabled validation or production
  `todo!()` stubs to manufacture a green checkpoint. Do not seek approval merely
  because an intermediate build is broken.
- Final A05 acceptance requires all carried build/test errors to be resolved and
  all documented checks to pass. Track phase scope separately from build status.

## Kagari Semantic Boundaries

- Preserve existing static typing, generics, traits, checked numeric behavior,
  shared object semantics and Result/Option propagation under their specifications.
  Rust and Kotlin are references, not implicit permission to change semantics.
- Host-owned Rust state remains outside the script heap. Script access uses the
  declared host capabilities and typed paths; never expose unrestricted Rust
  references through script values or bypass scoped borrow validation.
- Preserve left-to-right, once-only evaluation, mutation commit guarantees, trap
  order and already-completed side effects. A readonly view does not prove that
  another alias cannot modify its referent.
- Keep runtime ownership/generation checks, explicit roots and cleanup on traps,
  cancellation, budget exhaustion and synchronous host reentry.
- Preserve generation-pinned calls and dependency versions across hot reload.
  Reflection must remain within declared metadata and permissions, without runtime
  type mutation or monkey-patching that bypasses versioned publication.
- Do not introduce unbudgeted infinite execution paths. Optimization must preserve
  the specified logical budget and termination behavior.
- Follow the active plan's production dependency constraints: executable contracts
  must not depend on source analysis, and backends must consume checked facts
  rather than resolve syntax or infer types again.
- The current track migrates the existing Cranelift subset. Expanded JIT comes
  later; LLVM is a late independent track. Async, moving GC and speculative
  optimization are not incidental additions to the crate migration.

## Verification and Tooling

- Run checks appropriate to the change. Reuse valuable subsystem and conformance
  tests; add tests for meaningful behavior or boundaries, not just implementation
  structure. Documentation-only edits need link/content and diff checks, not a
  full workspace rebuild.
- Run focused tests when affected units build. At intermediate architecture
  boundaries, attempt relevant checks and record failures honestly; avoid repeating
  unchanged known failures while their owning migration step is still pending.
- Use the workspace O1 development profile; tests inherit it. Use the default
  `target` directory and default Cargo parallelism. Do not routinely add a custom
  `--target-dir`, restrict builds to `-j 2`, or clean caches. Investigate concrete
  file-lock/build failures before changing those defaults.
- Python is managed through `uv`; use `uv run python` when Python is needed.
- Store temporary logs and generated measurement output under ignored `target/`.
  Record durable conclusions, reproduction commands and known errors in the active
  plan or relevant docs so cache cleanup does not erase resumption state.
- For performance work, record toolchain, machine, profile, features, parallelism,
  cache state and workload. Separate compilation time from execution time and
  avoid speed claims unsupported by measurements.

Final architecture validation includes the plan's feature/behavior matrix and:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Use `git diff --check` at every checkpoint. On Windows, an invocation-local
`git -c core.safecrlf=false diff --check` is acceptable; do not change global Git
configuration just to handle the repository's line-ending conversion.

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
- Mark breaking API/format changes with `!` and explain their impact. Include
  `Architecture-Step: Axx` for MIR phase commits; several commits may share a phase.
  Do not apply a phase trailer to unrelated work or mark planning as implementation.
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
