# Test preparation and execution

[AGENTS.md](../AGENTS.md#verification-and-tooling) owns validation scope. Select
affected contract tests locally; CI owns complete workspace tests and exhaustive
feature/backend validation. Full-workspace Clippy runs locally for implementation
acceptance: `cargo clippy --workspace --all-targets -- -D warnings`.
This page defines source ownership, justified integration targets and reusable test setup.

## Match the test to its semantic owner

Parser tests should not construct an SDK engine. Typing tests should use HIR;
runtime ownership and installation tests should exercise runtime boundaries.
SDK conformance tests retain the full checked source/artifact/backend path when
that path establishes the contract. Do not remove source-free, actual-native,
GC, cancellation or pinned-reload coverage to reduce execution time.

## Default placement and integration exceptions

Put ordinary tests beside their semantic owner under `src/`, using `#[cfg(test)]`
and ordinary modules. Split larger suites by contract; keep library roots for
declarations. Several crates, filesystem access or a backend do not by themselves
require a separate integration executable. Cargo schedules test executables
sequentially; the harness schedules cases within one executable concurrently.
Compilation parallelism (`-j`) does not remove those execution barriers.

The workspace keeps six integration targets for concrete boundaries:

| Package / target | Reason |
| --- | --- |
| `kagari-cli` / `toolchain` | Executes the actual `kg` binary to verify stdout/stderr, JSON, exit status, project side effects and direct SDK agreement. |
| `kagari` / `artifact_features` | Standalone consumer checks ordinary public visibility and source-free feature/dependency isolation. |
| `kagari` / `async_execution` | Standalone native-wait/owned-execution consumer, including builds without source/native features. Source scenarios live in the runtime unit tree. |
| `kagari-runtime` / `native_schema` | External derive, `native_data!` and Serde expansion against ordinary runtime visibility. Manual rollback cases live beside schema registration. |
| `kagari-vm` / `allocation_contracts` | A single custom global allocator with thread-local measurement windows for the existing native/interface allocation assertions. |
| `kagari-vm` / `library_measurements` | Ignored manual timing workload with a separate allocator process; ordinary test success is not a performance result. |

Lifetime/compile-fail doctests, standalone AOT deployment and executable examples
retain their separate purposes. The feature script uses standalone manifests;
library unit builds cannot replace proof of the production dependency graph.

Tests that combine stdlib-created runtime objects live in
`kagari-stdlib/src/tests/runtime_boundary`. A runtime unit harness and the ordinary
runtime dependency used by stdlib have distinct Rust type identities. Keep these
composition tests at the consuming layer instead of adding self aliases or
changing production dependencies to force the types together.

Shared fixtures serving actual external targets may use a narrow `#[path]`.
Declare them once per harness and import that owner. Preserve feature/platform
gates, ignored status, malformed-input checks, cleanup assertions and fixture
paths when relocating cases. Review global state and temporary paths for newly
concurrent execution. SDK standard preparation is shared, while runtimes, source
sessions and captured mutable host state remain case-local.

Select by package, library and qualified test filter, for example:

```text
cargo test -p kagari --lib -- runtime::tests::language::syntax_examples::array_operations
cargo test -p kagari-vm --test allocation_contracts -- native_allocations::
```

Do not combine these examples into a routine workspace sweep.

## Package contract owners

Select cases at the layer whose behavior changed; this table is not a local sweep.

| Contract | Existing owner |
| --- | --- |
| Manifest inheritance, namespace/path policy, declared module trees and workspace selection | `kagari-package/src/modules/input_tests.rs` and `src/workspace/tests.rs` |
| Shared locks, exact host integrity, cycles/diamonds, disjoint revisions and relocation | `kagari-package/src/graph/tests.rs` |
| Importer aliases, empty roots, visibility, unused bodies, implementation-only dependencies and retained scopes | `kagari-package/src/tests/analysis.rs` and the HIR import/signature owners |
| Canonical native contracts and provider/query-setting identity | `kagari-types` native compiler-input unit case and `kagari-hir` environment-identity unit case |
| Public export target/signature validation and malformed source-free artifacts | Compiler/bytecode export cases and runtime/VM `package_exports` unit modules |
| Exact SDK capture, affected roots, dependency constants/generic bodies and pinned reload through decoded artifacts | `kagari/src/engine/tests/package_inputs.rs` |
| CLI discovery/selection, shared products, locked edits and standalone artifacts | `kagari-cli` project and artifact-pipeline cases |

The [workspace/host example](../examples/package-workspace/README.md) is a runnable
user workflow. It complements boundary assertions rather than duplicating another
integration suite. CI owns the exhaustive feature/backend/platform matrix;
`scripts/check_features.py` also checks source-free production dependencies.

## Host interop contract owners

For host interop changes, select the affected owner: runtime `native_schema` for
DTO/derive/Serde, runtime `native::tests::host_registration` and stdlib
`tests::runtime_boundary::installation_access` for exact contracts,
VM `native_boundary` for Opaque take, scoped method/argument and portable reference
semantics, and SDK `scoped_calls` for execution-context integration. Production
scope and callback lifetime proofs live beside their API as rustdoc tests; the
standalone prototype is retired. A focused selection is:

```text
cargo test -p kagari-runtime --doc -- host_scope::scoped native::typed::loans
```

Raw projection changes also need actual-pointer Miri coverage. The runtime library
case `executable_inline_field_codecs` exercises generated nested access, replacement,
disjoint paths and rooted results without a frontend execution workload:

```text
cargo +nightly-2026-07-27 miri test -p kagari-runtime --lib executable_inline_field_codecs -- --nocapture
```

Use a supported runner, default Miri target/profile/parallelism and no flags that
disable alias validation. Select other pointer kernels when their mechanism changes;
do not run the VM or workspace suite under Miri as a substitute. Broader platform
and feature/backend acceptance remains CI-owned.

## Prepare once, execute with fresh state

Ordinary SDK tests can use
[`test_support/execution.rs`](../crates/kagari/src/test_support/execution.rs).
Its standard baseline initializes once in the SDK library test harness. A short mutex
protects copying the native-only analysis baseline; compilation and execution
take place outside it in a fresh `KagariEngine::new_session`. This is not a pool
of previously used engines. Registration, provider replacement, source-origin,
cache invalidation and custom captured-state tests construct their own engine.

For a single source case, the runner performs these steps:

1. Compile the source once using its own source session.
2. Encode/decode once, and prepare direct and decoded programs once each.
3. Run the chosen source, artifact and artifact/JIT routes in separate runtimes.
4. Assert the result while it remains rooted in that runtime.
5. Release the scalar result and assert that the execution left no active roots.

JIT preparation still belongs to the loaded runtime/generation. The general
language-contract runner additionally preserves actual-native requirements,
ordered host effects, mutations, traps, cancellation and reload assertions.
Do not reuse a runtime, heap, host registry, cancellation token or mutable callback
capture between independent cases. Sharing prepared registration does not clone
Rust state captured by a callback.

## Expose cases to the harness

Give independent scenarios stable `#[test]` names, or generate them with a small
macro such as the one in
[`syntax_examples.rs`](../crates/kagari/src/runtime/tests/language/syntax_examples.rs).
Do not hide a growing independent case list inside one serial test. Related
sequences such as trap-then-recovery or publish-then-call-old-generation remain
one scenario. Run its routes sequentially; let Rust's test harness schedule
scenarios without a nested pool or a separate executable per scenario.

The language-contract cases are grouped by types/dispatch and evaluation, with
one shared runner. Ordinary scalar SDK routes in `default_methods`,
`standard_traits`, `instantiation`, `associated_types`, `conversion_traits`,
`collection_interfaces` and `collection_access` use this one source-owned helper.
`syntax_examples` selects the two interpreter routes. Keep each caller's runtime
configuration, including collection thresholds; the helper does not choose policy
for a semantic owner. Specialized host, mutation, trap and root-lifetime assertions
remain with their existing runner.

## Put invariant preparation outside scenario loops

Reuse at the smallest sufficient lifetime. An artifact rejection test that varies
only an ABI field compiles its input once, then constructs and validates a separate
artifact for each field value. A cancellation-point sweep can reuse its provider
engine and prepared program when the exercised callbacks are stateless; each
attempt still creates a new runtime, observer and cancellation token. Captured
application state needs separate preparation when it is part of the scenario.

The standard documentation owner validates provider declarations once, then forks
that native-only analysis for each extracted example. Each example sees only its
own source snapshot. It retains documentation coverage, deduplication and complete
diagnostic reporting without converting checked facts back to an unused portable
representation. Runtime routes are not needed for this analysis-only contract.

Analysis forks retain an append-only identity context. Dropping a session releases
its query state, but its new interned identities can remain until the last owner
of that context is dropped. Test baselines are bounded by their executable's case
set; do not extend this pattern into an unbounded process-wide source service.
Such a service needs an explicit baseline lifetime or a separately designed
identity isolation boundary, preserving the validity of existing scoped IDs.

## Measure the actual bottleneck

Build selected tests first:

```text
cargo test -p kagari --lib --no-run
```

Then select the intended cases, rather than running the whole library harness:

```text
cargo test -p kagari --lib -- runtime::tests::language::syntax_examples:: runtime::tests::language::default_methods::
```

These filters are a workload example, not a required local validation set. Report
compilation separately from harness execution and
process peak working set. Preserve the workspace profile, features, default
parallelism and workload; record machine/toolchain and cache state. Compare current
measurements rather than assuming the historical workspace ranking is unchanged.
When another workload competes for CPU, continue focused correctness validation
and batch performance comparisons after the optimization work. Do not repeatedly
rerun successful tests waiting for a cleaner timing sample.

If preparation still dominates, measure native installation, source checking,
artifact preparation, loading and execution separately before adding another
cache. Reuse requires an explicit owner, input identity, invalidation rule and
bounded lifetime. Do not trade validation or state isolation for a timing result.

## Source-tool acceptance

Formatter syntax, comment attachment and layout tests live in `kagari-format`;
rule/level/partial-fact cases live in `kagari-lint`. HIR owns eligibility/reuse
contracts, package tests own recovery/completeness, and CLI tests own argument,
reporting and writeback behavior. Select affected filters locally. The
`toolchain` executable includes one representative workspace that proves formatter
idempotence, side-effect/order preservation through SDK execution, CLI/SDK checking
agreement and optional findings alongside required errors.

CI explicitly runs the formatter's ignored `complete_grammar_fixture_inventory`
matrix. Windows CI enables symbolic links and runs formatter replacement, hard-link,
read-only, symlink/junction and toolchain command tests. Production dependency guards
in `scripts/check_features.py` and `scripts/check_aot.py` prohibit source tools from
leaking into source-free deployment; the formatter graph excludes semantic/runtime
crates and the linter graph excludes executable lowering/runtime crates.
