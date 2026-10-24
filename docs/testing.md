# Test preparation and execution

[AGENTS.md](../AGENTS.md#verification-and-tooling) owns validation scope. Select
affected contract tests locally; CI owns complete workspace tests and exhaustive
feature/backend validation. Full-workspace Clippy runs locally for implementation
acceptance: `cargo clippy --workspace --all-targets -- -D warnings`.
This page defines source ownership, justified integration targets and reusable test setup.
The [repository-wide organization plan](test-organization-plan.md) owns the finite
follow-up phases and ledger; it does not replace these lasting contracts.

## Match the test to its semantic owner

Parser tests should not construct an SDK engine. Typing tests should use HIR;
runtime ownership and installation tests should exercise runtime boundaries.
SDK conformance tests retain the full checked source/artifact/backend path when
that path establishes the contract. Do not remove source-free, actual-native,
GC, cancellation or pinned-reload coverage to reduce execution time.

## Classify the contract before choosing a format

A case has three separate dimensions: its primary semantic owner, its execution
requirements (such as source, decoded artifact, actual-native, platform or process),
and its expression form. File-driven input is a format, not a new semantic owner.
Classify individual cases; one Rust file may contain ordinary return checks and
stateful cleanup scenarios.

| Primary contract | Suitable expression form | Assertions that remain explicit |
| --- | --- | --- |
| Lexing, parsing and recovery | Source files with diagnostic or selected-tree expectations | Token bytes, ranges, partial tree and recovery behavior |
| Static semantics and diagnostics | Source plus structured diagnostics; Rust scenarios for surviving facts | Eligibility, neighbor facts and inference/identity isolation |
| Ordinary language execution | `.kgr` input with typed result expectations | Existing routes, runtime settings and result/root cleanup |
| Text and presentation | Input/expected golden files | Options, exact bytes, ordering, idempotence and error behavior |
| Pure algorithms and numeric boundaries | Rust tables or narrowly parameterized cases | Independent reference values, bit patterns, allowed sets and traps |
| Lowering, IR, bytecode and artifact admission | Constructed inputs and focused Rust assertions | Typed operands, effects, forged-input rejection and source-free validation |
| Multimodule and package acquisition | Fixture trees plus Rust graph/filesystem scenarios | Locks, fingerprints, namespaces and retained snapshots |
| Incremental analysis, caches and scoped identity | Explicit Rust scenarios | Reuse, invalidation, owner identity and old-snapshot isolation |
| GC, roots, handles and resources | Explicit Rust scenarios; external source where useful | Foreign/stale rejection, accounting and release order |
| Host borrowing, conversion and reentry | Explicit Rust and compile-fail scenarios | Scoped access, alias rules, unwinding and synchronous reentry |
| Async, cancellation, statics and reload | Explicit Rust scenarios; external source where useful | Poll/drop/wake order, publication, version choice and cleanup |
| Products, processes and deployment | Existing dedicated process/platform runners | Real binaries, feature isolation, actual-native and source-free execution |

Prefer four forms: source plus typed expectations, stable text goldens, structured
Rust tables, and explicit Rust scenarios. A short inline input remains appropriate
when separating it from its assertion would make the contract harder to read.
Moving source out of Rust does not justify reducing a lifecycle test to its final
return value, or treating interpreter fallback as proof of native execution.

Fixtures stay under the owning crate; keep Rust orchestration in ordinary test
modules. Use explicit named registration and `include_str!` for fixed text inputs
when sufficient. Keep logical source filenames and byte-sensitive inputs stable.
Do not normalize whitespace, spans, float bits, effect order or identity distinctions
that the case is intended to test. Review golden changes as assertions, not output
to accept automatically; do not snapshot entire private `Debug` representations.

Use [`rstest`](https://docs.rs/rstest/0.28.0/rstest/) with descriptive
`#[case::name(...)]` inputs when independent cases share orchestration and
assertions but need explicit metadata. For a directory with one execution contract,
prefer `#[files(...)]`: each matched file becomes an independently scheduled test,
not one runtime directory loop. Keep unique stateful scenarios as ordinary
`#[test]` functions. Parameterization belongs to the existing library harness;
adding cases does not require a custom registration macro or integration target.
`rstest` is a workspace-managed dev dependency of the selected test owners, with
its default `async-timeout` and `crate-name` features disabled. Do not add it to
production dependencies or build a second registration DSL around it.

Inline setup/assertion logic into its parameterized test when no other caller
needs it. Keep helpers that have actual consumers or an independent responsibility,
such as the shared SDK execution runner; a new name is not itself a reason for
another forwarding function. Framework adoption does not inherently reduce
compilation/execution cost. Avoid uncontrolled operation/type/backend Cartesian
products and one serial test that walks an entire fixture directory. A custom
discovery harness needs a concrete justification for departing from ordinary
ownership rules.

### Compiler UI diagnostic tests

The [organization plan](test-organization-plan.md#file-based-ui-diagnostic-tests-planned)
owns the remaining CLI UI migration. Here UI means user-facing compiler
diagnostics, not graphical interfaces. Parser and HIR suites run in their existing
library harnesses; real-process CLI presentation follows separately.

Parser source inputs pair with ordered `.diagnostics.json` expectations for full
producer records; HIR uses the same test-only source-owned schema. CLI cases
will compare actual `.stdout`, `.stderr` and
`.exit` output through the existing process harness. The plan owns the TO00-frozen
schema, source/revision checks, narrow normalization and explicit update workflow.
Default test/CI runs must not rewrite expectations. Recovery trees, stronger typed
errors, healthy-neighbor facts and codegen eligibility remain explicit assertions;
a matching diagnostic snapshot cannot replace them. No separate compiletest
framework or production diagnostic renderer is planned.

### Existing file-backed patterns

Use the existing owners as templates, not a new universal runner:

- [Formatter layout cases](../crates/kagari-format/src/tests.rs) use named `rstest`
  cases for input/expected pairs with explicit width/idempotence settings. The
  parameterized test compares exact output and second-format/narrow-width results
  directly, without a custom registrar or single-caller assertion helper. Data
  lives under `kagari-format/tests/fixtures/layout`.
- [Parser error cases](../crates/kagari-syntax/src/tests/parser/errors.rs) retain
  typed delimiter kind/count/severity and separate partial-tree recovery checks.
  [Parser UI cases](../crates/kagari-syntax/src/tests/parser/ui.rs) discover paired
  `.kgr`/`.diagnostics.json` files under `kagari-syntax/tests/fixtures/ui/parser`.
  The real producer supplies all messages and labels. The
  [source-owned test schema](../crates/kagari-source/tests/support/diagnostics.rs)
  rejects malformed fields and validates captured identities/revisions and UTF-8
  ranges before projecting IDs to logical paths. Missing/orphan pairs fail the
  owner inventory; normal comparisons never update expectations.
- [HIR UI cases](../crates/kagari-hir/src/tests/ui.rs) group bare, installed-standard
  and local-function diagnostics without inferring policy from source comments.
  They bind final records to captured sources before projection. Stronger typed
  payloads, healthy-neighbor facts, query identity and codegen rejection remain
  explicit in their semantic owners; matching a golden does not replace them.
- SDK [syntax examples](../crates/kagari/src/runtime/tests/language/syntax_examples.rs)
  and [reload](../crates/kagari/src/runtime/tests/embedding/generic_reload.rs)
  load scripts from `kagari/tests/fixtures/test-fixtures`. Ordinary conversion,
  default-method and standard-trait execution uses directory-discovered inputs in
  `kagari/tests/fixtures/language/{conversion_traits,default_methods,standard_traits}`.
  Each owner keeps its logical source name, GC threshold `Some(1)`, result `42`
  and source/artifact/artifact-JIT routes. The default-method cursor case reuses
  the canonical `iterable-default-cursor.kgr` directly, preserving its CRLF bytes;
  admission tests reuse ordinary fixtures when they need the same program.
  The remaining types/dispatch, collections/iteration, operators, Result/Try and
  string owners follow the same representation rule, not one universal execution
  policy. Rejection, linking, forged-contract and callback-lifecycle scenarios keep
  their explicit assertions in focused modules. Singleton and lifecycle tests need
  not be parameterized. Independent programs remain independent named cases; a common
  final result does not make them duplicate coverage.
- [Numeric predicate programs](../crates/kagari/src/runtime/tests/language/numeric_checks.rs)
  share bounded source generation and retain the expression for each failure
  marker. Integer/float owners retain their own compilation, routes, result/root,
  depth and quarantine checks. Named `rstest` rows expose the existing integer and
  float width dimensions, while bounded inner predicates stay generated in Rust.
  Typed result carriers, exact float bits, NaN rules and allowed signed-zero sets
  remain explicit; no general expectation DSL is needed.

Fixed text extraction preserves the original Rust literal's value, including its
leading/trailing newlines or deliberate absence of a final newline. Multiline
fixture directories declare `*.kgr text eol=lf` locally so `include_str!` has stable
bytes under Windows checkout conversion. Do not normalize loaded fixtures. Tests
specifically about CRLF or literal bytes may continue constructing inputs inline.

### Directory-discovered syntax examples

`syntax_examples::source_examples` discovers `examples/syntax/*.kgr` at test
compilation through `rstest`. Ordinary files in that directory are admitted to the
same contract: standard installation, default GC settings, `i32` result `42`, and
source plus encoded-artifact interpreter routes. Adding an ordinary example needs
no Rust registration row. This is not permission to execute arbitrary repository
`.kgr` files under that contract.

Five existing files remain explicitly excluded, not silently skipped on failure:

| File | Different contract / existing owner |
| --- | --- |
| `async.kgr` | Cold async declarations without a synchronous `main`; async execution has its own owners. |
| `byte-literals.kgr` | Returns `1`, not `42`; formatter tests retain literal-byte and layout checks. |
| `char-literals.kgr` | Returns unit, not `i32`; character semantics and formatter tests have their own assertions. |
| `grammar-witnesses.kgr` | Parser/formatter grammar witness with unresolved external declarations, not a standalone executable. |
| `try-protocols.kgr` | Returns `42`, but the Try owner requires forced GC, encoded-only transport, reload retirement and heap cleanup; `try_protocols::ordinary_execution` selects this canonical input directly. |

The four separately selected root examples and two extracted programs stay explicit
in `additional_source_examples`: the repository root example directory has mixed
host/async/error contracts, and the grouped-glob case intentionally uses only the
artifact interpreter route. Do not relocate/copy public examples merely to make a
glob work, infer policy from source text, or gradually replace every case row with
an exclusion. New different-contract examples need an explicit owner and reviewed
admission decision; homogeneous test-only fixture directories should instead group
by contract.

The discovered test receives a physical path, reads its UTF-8 text without
normalization and retains the logical name `examples/syntax/<filename>` for source
origins. Missing files fail with the physical path. Existing scripts are not
rewritten and result-release/root cleanup remains in the shared execution runner.

The SDK build script watches the external syntax and language-fixture directories
when the source feature and each directory are present, including source-only
builds. File additions/removals therefore rerun macro discovery; do not rely on
`include_str!` content tracking to notice new files.
The missing-directory guard keeps standalone consumers from requiring the workspace
example tree. This hook adds no source-analysis or framework production dependency.

The separate [script testing proposal](script-testing-design.md) does not provide
an implemented `kg test` runner and is not a prerequisite for these patterns.

## Default placement and integration exceptions

Put ordinary tests beside their semantic owner under `src/`, using `#[cfg(test)]`
and ordinary modules. Split larger suites by contract; keep library roots for
declarations. Several crates, filesystem access or a backend do not by themselves
require a separate integration executable. Cargo schedules test executables
sequentially; the harness schedules cases within one executable concurrently.
Compilation parallelism (`-j`) does not remove those execution barriers.

The workspace keeps eight integration targets for concrete boundaries:

| Package / target | Reason |
| --- | --- |
| `kagari-cli` / `toolchain` | Executes the actual `kg` binary to verify stdout/stderr, JSON, exit status, project side effects and direct SDK agreement. |
| `kagari` / `artifact_features` | Standalone consumer checks ordinary public visibility and source-free feature/dependency isolation. |
| `kagari` / `async_execution` | Standalone native-wait/owned-execution consumer, including builds without source/native features. Source scenarios live in the runtime unit tree. |
| `kagari` / `async_facade` | External consumer of public typed async calls, direct await and externally driven execution; keeps public API visibility and lifecycle checks outside the library harness. |
| `kagari-analyzer` / `stdio` | Launches the actual analyzer process to verify framed stdio messages, synchronization, bounded transport and shutdown. |
| `kagari-runtime` / `native_schema` | External derive, `native_data!` and Serde expansion against ordinary runtime visibility. Manual rollback cases live beside schema registration. |
| `kagari-vm` / `allocation_contracts` | A single custom global allocator with thread-local measurement windows for the existing native/interface allocation assertions. |
| `kagari-vm` / `library_measurements` | Ignored manual timing workload with a separate allocator process; ordinary test success is not a performance result. |

Lifetime/compile-fail doctests, standalone AOT deployment and executable examples
retain their separate purposes. The feature script uses standalone manifests;
library unit builds cannot replace proof of the production dependency graph.

For the bounded [numeric sampling example](../examples/numeric-sampling.kgr), run:

```text
uv run --locked python scripts/check_features.py --numeric-only
```

This emits a fresh disposable KBC under `target/fixtures/` with the source-enabled
producer, then builds a separate no-default-features SDK consumer. Its normal/build
dependency graph must exclude source analysis, HIR, MIR, compiler and native codegen.
The consumer runs only `artifact_features/numeric.rs`, requiring the explicit
artifact path; it checks the example result, exact sampling checksum and execution
cleanup. That file is selected by the generated standalone manifest, not added as
another workspace integration target or silently skipped when a product is absent.
The full CI feature-script invocation includes this case once; `--numeric-only`
does not run the full feature matrix. `--async-only` remains the separate async
selection, and the two focused modes cannot be combined.

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
cargo test -p kagari --lib -- runtime::tests::language::syntax_examples::source_examples::path_01__UP__UP_examples_syntax_array_operations_kgr
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
2. Prepare each selected product once: direct for the source route; one
   encode/decode and decoded preparation for artifact routes. Artifact-only cases
   do not prepare an unused direct product.
3. Run the chosen source, artifact and artifact/JIT routes in separate runtimes.
4. Assert the result while it remains rooted in that runtime.
5. Release the scalar result and assert that the execution left no active roots.

JIT preparation still belongs to the loaded runtime/generation. The general
language-contract runner additionally preserves actual-native requirements,
ordered host effects, mutations, traps, cancellation and reload assertions.
Do not reuse a runtime, heap, host registry, cancellation token or mutable callback
capture between independent cases. Sharing prepared registration does not clone
Rust state captured by a callback.

Not every existing SDK test's first lane is the checked-source route. Arrays,
iteration, numeric operations, operator traits, Result/Option and interpolation
also test **unencoded compiled artifacts** before their decoded artifact lanes.
Keep this distinction when choosing a runner. Some owners require more than the
standard scalar runner: Result/Option observes actual collection on every lane;
String/Try retire materialized constants before checking final heap cleanup; native
enums use a fresh installed source-unaware consumer and repeat on its loaded heap.
These are separate contracts, not setup differences to discard for uniformity.

## Expose cases to the harness

Give independent scenarios descriptive `#[test]` names or named `rstest` cases,
as in [`syntax_examples.rs`](../crates/kagari/src/runtime/tests/language/syntax_examples.rs).
`rstest` produces qualified names with a group, ordinal and case description or
sanitized file path. Prefer `#[base_dir]` to keep directory prefixes from consuming
the framework's truncated file description. File ordering can change ordinals;
update affected filters rather than retaining forwarding aliases. Preserve semantic descriptions and use selected
`--list` output to confirm exact names when needed. Do not hide a growing
independent case list inside one serial test. Related
sequences such as trap-then-recovery or publish-then-call-old-generation remain
one scenario. Run its routes sequentially; let Rust's test harness schedule
scenarios without a nested pool or a separate executable per scenario.

The language-contract cases are grouped by types/dispatch and evaluation, with
one shared runner. Ordinary scalar SDK routes in `default_methods`,
`standard_traits`, `instantiation`, `associated_types`, `conversion_traits`,
`collection_interfaces` and `collection_access` use this one source-owned helper.
Ordinary `syntax_examples` use two interpreter routes; the grouped-glob case
explicitly selects only the artifact interpreter route. Keep each caller's runtime
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
