# Source-owned script tests and `kg test`

Status: design authorized on 2026-10-15; implementation is unstarted.
The commands, attributes and testing APIs below are proposed behavior. This work
is separate from the implemented [formatter/checker/linter](spec/toolchain-tools.md).
This document records the agreed direction and the decisions required before
implementation; it is not evidence that `kg test` exists.

## User contract

Most script tests need only the standard language environment. A user writes a
test beside the code and runs `kg test`, without a separate package, Rust runner,
public test function, `main`, or explicit test-entry registration.

The following illustrates the proposed test syntax and default assertion API:

```text
fn damage(attack: i32, defense: i32) -> i32 {
    attack - defense
}

#[test]
fn damage_reduces_defense() {
    assert(damage(10, 3) == 7);
}
```

`assert` here is part of the proposed default testing experience, not a claim
that a suitable unqualified standard assertion function is already installed.
Its final declaration/path and failure contract must be established at the
standard-library owner before documenting executable examples.

```text
kg test
kg test damage_reduces_defense
kg test -p combat
kg test --workspace
kg test --list
```

The optional positional argument is a case-name filter, not a source-file path.
Reuse existing manifest discovery, invoking/default-member selection, repeated
`-p`, `--workspace`, `--manifest-path`, `--input-root` and lock/offline semantics.
Package selection conflicts remain unchanged. Initial support targets projects;
standalone-file and stdin test modes are deferred.

Tests may call private code according to ordinary module visibility. This is
source-owned unit/scenario testing, even when a case exercises several business
modules. Testing public APIs from a distinct package remains a separate integration
boundary and is not a prerequisite for running ordinary tests.

## Baseline and semantic owners

The current package model has one root module per package and compiles it together
with its dependency closure into a Program. Loading does not execute module
initializers. Reuse [package](spec/packages.md), [module](spec/modules.md),
[execution](spec/execution.md) and [artifact](spec/artifacts.md) contracts.

Current gaps are concrete:

- CLI has no `test` command or test discovery. HIR rejects unqualified `test` and
  `cfg` attributes as unknown; preserving `tool::` metadata is not test execution.
- Source acquisition discovers external modules before HIR checking. Test-only
  module selection therefore cannot be implemented solely in late HIR filtering.
- [`ProgramEntry`](../crates/kagari-runtime/src/module/entry.rs) selects verified
  public exports or the separately verified root `main`. It cannot call arbitrary
  private functions; tests need a verified entry mechanism, not a lookup bypass.
- [`PreparedProgram`](../crates/kagari/src/program/mod.rs) uses `Rc`/`RefCell` and is
  neither Send nor Sync. Shared code preparation does not authorize sharing that
  object between worker threads.

| Owner | Responsibility |
| --- | --- |
| Syntax/source | Preserve attributes/CST; represent explicit source compilation mode and provenance at the appropriate existing input boundary |
| Package acquisition | Discover active module sources for each root/dependency mode, without a second module resolver |
| HIR | Interpret supported test/cfg attributes, select active declarations, check signatures, visibility and bodies, and collect test identities |
| Contract/compiler/verification | Retain test roots through lowering and establish executable entry evidence |
| Runtime/VM | Invoke verified entries with ordinary ownership, generation, roots, cancellation and cleanup checks |
| Standard library | Default assertion declarations and implementations; no application host setup required |
| SDK source/testing integration | Prepare/enumerate a suite and execute an individual case using explicit host environments |
| CLI | Default environment, selection, scheduling, reporting and exit status |
| Embedding host | Custom services, per-case fixtures, scheduling and lifecycle for host-dependent tests |

Use existing owners and ordinary modules first. Do not start with a new generic
testing framework crate, a second interpreter, or a duplicate semantic pipeline.
Test discovery/compilation remains source-enabled; runtime execution must not
depend on source analysis or interpret source attributes dynamically.

## Test declarations

First-release `#[test]` marks an ordinary module-level function with a body:

- Zero parameters, no receiver or generic parameters, synchronous, returning unit.
- Private by default; `pub` is permitted but not required for runner discovery.
- Associated methods, trait declarations, nested local functions, async functions,
  parameterized tests and Result-returning test functions are outside the initial
  signature contract and receive a clear diagnostic when marked as tests.
- Arguments to `#[test]`, duplicate markers and unsupported attribute targets are
  diagnosed. Test discovery operates on resolved declarations, not textual names.
- Discovery lists each marked declaration once. Re-exports do not duplicate cases.
  Display names include the package, module path and function name.

A test passes only when its invocation returns unit normally. Assertion failures,
script traps and execution/host errors fail that case, with distinct diagnostics.
There is no automatic retry or expected-failure attribute in the first release.
Filtering selects execution, not compilation: an invalid unselected test still
prevents its package's test Program from being executed.

Unmarked helpers remain ordinary functions. Users group test-only helpers and
cases with the limited compilation condition:

```text
#[cfg(test)]
mod tests {
    fn expected_damage() -> i32 { 7 }

    #[test]
    fn damage_reduces_defense() {
        assert(super::damage(10, 3) == expected_damage());
    }
}
```

The first condition language supports only outer `#[cfg(test)]` on module-level
items. It does not introduce `cfg(not(...))`, feature predicates, cfg expressions,
inner attributes or a macro system. A cfg module recursively excludes its contents.
An external `#[cfg(test)] mod tests;` uses ordinary module-file resolution when
enabled. It is not opened or required to exist in normal mode.

## Compilation modes and source acquisition

Compilation mode is explicit input, not inferred from directory names or presence
of `#[test]`. It affects source discovery, active declarations and analysis reuse.

| Context | Selected package | Its dependencies |
| --- | --- | --- |
| Ordinary check/build/run | Normal | Normal |
| `kg test -p A` | Test for A | Normal |
| `kg test --workspace` | A separate test Program per selected package | Normal within each Program |
| Proposed check/lint test option | Normal and test variants for selected roots | Normal |

If A depends on B and both are selected, A's test Program contains normal B;
B's test Program separately contains B's tests. A cannot access B's private tests
or test-only helpers. A library under test does not need a `main`, and its existing
`main` is never executed automatically by the test runner.

In normal mode, exclude marked test functions and cfg-test items before name
collection, import/trait contribution, type checking and executable lowering.
Normal code cannot depend on these excluded declarations. Test mode includes the
selected package's active tests and ordinary code, using ordinary visibility rules.

Parsing and selection are separate: excluded inline items must still be
syntactically well formed, and mode-attribute shapes must be recognized reliably.
Their names and types need not resolve in normal mode. External excluded modules
are not read. Freeze the exact malformed-attribute versus excluded-item behavior
before implementing selection; do not use recovery placeholders to guess a mode.

Package discovery and HIR must consume the same selection semantics. Extract a
focused shared attribute/selection operation at a dependency-compatible owner;
do not independently match attribute strings in two different loaders. Preserve
module ambiguity, canonical-path containment, alias checks and resource limits.

Record mode per package analysis instance and include it in affected query,
captured-input and prepared-product validity keys. Normal B and test B must never
reuse incompatible lowering/signatures/bodies merely because their file text is
identical. Do not change B's permanent package family identity to encode a mode.
Retained snapshots keep their original mode and definition ownership.

The dependency graph and lock retain current semantics. This milestone introduces
no dev-dependency table or automatic dependency acquisition. Application services
are installed by the host, not inferred from test attributes.

## Verified test entries

Each test Program has a compiler-produced case inventory. A conceptual entry
contains its display name, source location, declaration identity and a reference
to a verified callable. Display strings are not executable authority.

Marked functions must be retained as compilation roots even when no ordinary
export calls them. Carry their exact identity through lowering, optimization and
function-slot remapping. Verify membership in the supplying Program, non-generic
unit signature and valid callable bounds before producing an executable test handle.
Invocation must still enforce runtime ownership and exact loaded generation.

The implementation design must select where this evidence is represented in the
contract/preparation layers. Private test entries must neither become ordinary
public exports nor expose an API for calling arbitrary function slots. Do not
execute all cases through one synthesized `main`; each case needs an independent
invocation, result and cleanup boundary.

Initially prepare test suites in memory. Persisted/distributable test artifacts,
new general CLI export invocation and source-free test discovery are deferred.
Normal build artifacts contain neither excluded test bodies nor test-entry data.
If executable records need extending, update their existing verifiers coherently;
do not add an unchecked sidecar or duplicate executable format. Follow the
repository's unpublished ABI/version policy rather than bumping identifiers by habit.

## Default runner and isolation

The default route is the existing interpreter with standard declarations and the
documented CLI services. No user host injection is necessary. Reuse compilation
and prepared code within a package suite rather than compiling each case.

```text
select packages and capture mode-aware inputs
    -> analyze and validate test declarations
    -> prepare test Program and verified case inventory
    -> for each selected case:
         create a fresh runtime and case context
         load the prepared Program
         invoke the verified test entry
         record outcome and release case-owned state
    -> report suite results
```

Each case owns its heap, loaded instances, cancellation token and mutable fixture
state. Preserve ordinary scoped borrowing, explicit roots and trap cleanup. A trap
fails one case; it does not undo completed host effects. Dropping a runtime is not
rollback of a database transaction or shared native callback state.

Provide serial execution and a bounded jobs setting. Before implementing parallel
execution, prove how thread-confined preparation hands off executable state to
workers under existing ownership contracts. Do not add unsafe Send/Sync, hold a
global mutex around all test execution, or rebuild the compiler baseline per case
to simulate reuse. Start with the serial path; parallel support is accepted only
after proving both isolation and the preparation handoff, with focused measurements.

Cooperative cancellation remains available per case and for the run. Cancellation
of the run stops scheduling new cases; finish cleanup for cases that return.
Timeouts, if exposed initially, request cancellation and must not claim hard
preemption of blocking host calls. Hard deadlines need process-level supervision
and are deferred. Do not reintroduce execution charging or permission matrices.

## Assertions and reporting

Provide a default boolean assertion facility backed by a normal checked standard
declaration, with an identifiable assertion-failure outcome and source call site.
Inventory existing trap/assertion support before adding it; do not simulate failure
with division by zero, rely on an application callback, or add Rust-style macros.
Richer equality/diff helpers can follow after the basic assertion contract works.

Capture output from documented CLI-owned logging services per case and show it
with failures; optionally show successful output explicitly. Do not claim to
capture arbitrary process stdout emitted by host callbacks. A host can supply its
own log sink. Keep source frames, case identity and host/setup errors distinguishable.

First-release reporting includes case name, pass/fail/cancelled/not-run status,
elapsed execution time, failure location/trace, captured logs and a summary.
Report compilation/preparation failures at package scope rather than inventing a
passed or failed invocation. Continue independent packages where inputs permit it;
do not execute a package with invalid test signatures or language errors.

The filter matches qualified case names deterministically. `--list` checks and
enumerates tests without loading a runtime, invoking a test or setting up fixtures;
it need not generate executable code. List order and final summary order are
stable even if execution is parallel. A project with no tests reports zero cases
explicitly; an explicitly supplied filter matching nothing should fail to catch
typos. Fix exact filter, jobs and output option spelling before implementation.

Exit status proposal: 0 only when selected execution/listing completes successfully;
1 for compilation, preparation, case, cancellation or operational failure; 2 for
invalid command/configuration. A failed case does not prevent later independent
cases. An incomplete run must not be reported as all passed. Reuse the toolchain's
structured diagnostics and machine-output conventions when available; do not print
ordinary progress into machine-readable stdout.

## Host-orchestrated tests

Hosts use the same discovery, signature checking, preparation and individual-case
invocation capabilities with their own installed declarations and implementations.
They do not have to invoke the `kg` process or accept CLI scheduling policy.

For a test requiring a game world, the host prepares the suite against that declared
interface, creates a fresh world per case, installs/binds matching services, invokes
the case, inspects events and cleans up. Installed contracts must match those used
for compilation. A missing service is a clear setup/compilation error; `kg test`
does not invent stubs or search for native plugins.

Keep the API composable: prepare/enumerate, construct fixture/runtime, invoke one
case, then report and dispose. Avoid mandatory inheritance from a testing framework.
Test metadata/entry handles are validated products, not unrestricted VM references.
Shared native closures can retain mutable Rust state even with independent script
runtimes; the host owns fixture freshness and external resource isolation.

Host-managed async execution, reload and external-system tests can use ordinary
SDK operations. They are not automatically supported by the initial synchronous
`#[test]` signature contract. Do not add CLI injection flags, native plugin loading
or an application service container to accommodate those cases.

## Relationship with fmt, check and lint

- Fmt operates on all written syntax, including excluded test declarations; it
  preserves test/cfg attributes and does not need compilation-mode selection.
- Check/lint default to normal mode. A proposed explicit `--tests` option checks
  both normal and test variants of selected roots without executing cases. Checking
  only test mode could conceal normal-mode references to test-only declarations.
- Test mode uses the same required language checks as check/build. Running tests
  does not implicitly run optional lint rules or format source.
- Test-only names, fixtures and standard assertion contracts obey ordinary lint
  suppression and source ownership. Test entry use is explicit evidence for any
  future dead-code rules, not a reason to publish private names.

The `--tests` spelling and mode plumbing belong to this design's implementation,
not the completed formatter/checker/linter work. When activated, reuse the
existing diagnostics/recovery contracts without making LSP or the complete
optional lint set a prerequisite for script testing.

## Implementation gates and acceptance

Before coding, freeze these bounded decisions in this document:

1. Active-item selection, excluded-source diagnostics, external module discovery
   and mode-aware identity/reuse; demonstrate normal/test variants cannot mix.
2. Verified private entry representation and lowering retention, with a concrete
   plan for compiler, verifier and runtime owners and no unchecked private dispatch.
3. Default assertion API/failure mapping and per-case log attribution.
4. Runner/SDK handoff, fixture lifecycle and the supported worker model for current
   non-Send preparation; exact filter/list/jobs/exit behavior.

Suggested implementation order is mode-aware acquisition/HIR, verified entries
and assertions, serial CLI/SDK execution, then bounded scheduling and toolchain
integration. Record substantial boundary changes before expanding implementation.

Focused acceptance must cover:

- A private `#[test]` runs without `pub`, `main`, a separate package or host setup.
- Normal mode excludes test helpers and does not open excluded external modules;
  normal code cannot resolve excluded names. Test mode includes them correctly.
- Syntax errors in inline excluded bodies remain syntax errors; excluded names
  and types are not semantically checked in normal mode.
- Invalid signatures/attribute forms fail checking; filtering cannot hide them.
- A depending package sees normal dependency code; selecting that dependency as
  a root produces its separate test suite. Mode changes invalidate incompatible
  query/products without changing permanent package identities.
- Uncalled private tests survive lowering; wrong-program, stale-generation and
  wrong-signature entries are rejected before execution.
- Passing, assertion-failing and trapping cases each get independent state;
  later cases still run and roots/borrows are cleaned up. Failed compilation never
  becomes successful execution of a partial suite.
- Default services work without injection; custom host fixtures follow the same
  entry path and preserve contract validation and per-case host state.
- Listing does not run tests/fixtures, no-match filters are visible, cancellation
  is reported honestly, and parallel execution does not share case state.
- Normal production artifacts/source-free consumers remain free of test source
  discovery dependencies. Check/lint mode selection agrees with test compilation.

Tests for implementation live beside the relevant Rust owners, with CLI coverage
for actual argument/output/filesystem contracts. Select focused local tests and a
small regression set; run workspace Clippy for implementation acceptance:
`cargo clippy --workspace --all-targets -- -D warnings`. Structure, formatting and
diff checks follow [AGENTS.md](../AGENTS.md#verification-and-tooling). Full workspace
tests and exhaustive feature/backend/platform validation remain CI-only. This
documentation-only checkpoint requires content/link/line-ending/diff checks.

## Deferred scope and status

Separate integration targets, automatic `tests/` scanning, dev dependencies,
parameterized/async/Result-returning tests, expected-failure/ignore attributes,
benchmarks, property testing, coverage, snapshots, retries, process isolation,
native backend test matrices and persisted test artifacts are deferred. A host may
already compose some scenarios through existing SDK APIs without extending `kg test`.

- [x] Record the source-owned `kg test` experience and host-orchestration boundary.
- [ ] Resolve implementation gates and document the concrete entry/selection APIs.
- [ ] Implement compilation modes, test discovery and validated execution.
- [ ] Complete runner, focused acceptance and toolchain integration.

No implementation or validation result is claimed. On completion, move lasting
contracts to language/package/CLI specifications and architecture, move unresolved
work to the roadmap, and remove this design's completed progress ledger with the
plan according to the repository documentation lifecycle.
