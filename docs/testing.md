# Test preparation and execution

[AGENTS.md](../AGENTS.md#verification-and-tooling) owns validation scope. Select
affected contract tests locally; CI owns complete workspace/feature/backend runs.
This page describes how to avoid repeating expensive setup inside those tests.

## Match the test to its semantic owner

Parser tests should not construct an SDK engine. Typing tests should use HIR;
runtime ownership and installation tests should exercise runtime boundaries.
SDK conformance tests retain the full checked source/artifact/backend path when
that path establishes the contract. Do not remove source-free, actual-native,
GC, cancellation or pinned-reload coverage to reduce execution time.

## Prepare once, execute with fresh state

Ordinary SDK tests can use
[`support/execution.rs`](../crates/kagari-embed/tests/support/execution.rs).
Its standard baseline initializes once per integration binary. A short mutex
protects copying the native-only analysis baseline; compilation and execution
take place outside it in a fresh `KagariEngine::new_session`. This is not a pool
of previously used engines. Registration, provider replacement, source-origin,
cache invalidation and custom captured-state tests construct their own engine.

For a single source case, the runner performs these steps:

1. Compile the source once using its own source session.
2. Encode/decode once, and prepare direct and decoded programs once each.
3. Run the chosen source, artifact and artifact/JIT routes in separate runtimes.
4. Assert the result while it remains rooted in that runtime.

JIT preparation still belongs to the loaded runtime/generation. The general
language-contract runner additionally preserves actual-native requirements,
ordered host effects, mutations, traps, cancellation and reload assertions.
Do not reuse a runtime, heap, host registry, cancellation token or mutable callback
capture between independent cases. Sharing prepared registration does not clone
Rust state captured by a callback.

## Expose cases to the harness

Give independent scenarios stable `#[test]` names, or generate them with a small
macro such as the one in
[`syntax_examples.rs`](../crates/kagari-embed/tests/syntax_examples.rs).
Do not hide a growing independent case list inside one serial test. Related
sequences such as trap-then-recovery or publish-then-call-old-generation remain
one scenario. Run its routes sequentially; let Rust's test harness schedule
scenarios without a nested pool or a separate executable per scenario.

The language-contract cases are grouped by types/dispatch and evaluation, with
one shared runner. The SDK `default_methods` suite is the ordinary runner pilot.
Other suites remain candidates for measured migration, not implicit obligations
of this change.

## Measure the actual bottleneck

Build selected tests first:

```text
cargo test -p kagari-embed --test syntax_examples --test default_methods --no-run
```

Then run the selected binaries or corresponding Cargo test commands. Report
compilation separately from harness execution and
process peak working set. Preserve the workspace profile, features, default
parallelism and workload; record machine/toolchain and cache state. Compare current
measurements rather than assuming the historical workspace ranking is unchanged.

If preparation still dominates, measure native installation, source checking,
artifact preparation, loading and execution separately before adding another
cache. Reuse requires an explicit owner, input identity, invalidation rule and
bounded lifetime. Do not trade validation or state isolation for a timing result.
