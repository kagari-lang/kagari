# Test preparation and scheduling

This finite task improves test preparation boundaries and three SDK test owners.
Native collection migration and the remaining workspace suites are outside scope.

## Contract

- Runtime owns reusable, validated native installations. Validation still uses
  the existing transactional installer. Each runtime owns independent registries,
  heap, host borrows, modules, execution state and cancellation. Registered Rust
  callbacks retain their existing captured-state semantics; sharing an installation
  does not promise to clone application state captured by callbacks.
- HIR owns analysis snapshot forks. The SDK retains its native-only baseline and
  creates fresh source sessions from it, never from another session's user files,
  host declarations or mutable query settings. Query invalidation remains intact.
- Test support shares only sealed standard preparation. Compile once per case;
  prepare direct and decoded artifacts once each; execute every existing route in
  a fresh runtime. Registration/invalidation tests continue to prepare fresh inputs.
- Expose independent language-contract and syntax examples as named harness tests.
  Related reload/trap sequences stay together. No nested thread pool or one binary
  per scenario. Preserve all cases, assertions, actual-native requirements and GC
  settings. Migrate `default_methods` as the normal SDK runner pilot.

## Checkpoints

- [x] TP01: retain validated native installation and native-only analysis baseline;
  prove independent sessions and unchanged registration rejection.
- [x] TP02: migrate language-contract, syntax-example and default-method owners;
  compare scenario inventories and preserve their route matrix.
- [x] TP03 correctness: focused tests, source-free build and structural checks;
  durable contracts are in architecture and the testing guide.
- [ ] TP03 measurement: idle-machine timing and retained-memory comparison;
  consolidate measured results and retire this plan when complete.

## Evidence and validation

The historical audit is in [performance-baseline](performance-baseline.md).
Current inspection found `KagariEngineBuilder::install_all` discards the validated
runtime registry, then `KagariEngine::runtime` repeats complete installation.
Bundled module construction already uses OnceLock; do not add another module cache.
The language-contract owner hides its scenarios in one test, while the syntax owner
hides 45 source/artifact examples in one test. This prevents harness scheduling.

Baseline and final measurements use the same focused `syntax_examples` and
`default_methods` binaries, default test concurrency and workspace test profile.
Separate compilation from execution; record machine/toolchain/cache state and
process peak memory. Stage probes distinguish engine, compilation, preparation,
runtime construction and execution. Temporary data belongs under ignored `target/`.
No whole-workspace test or Clippy run is required locally. Run affected SDK suites,
language contracts, provider reset/registration and session-isolation checks;
source-free compilation plus structure, formatting and diff checks. CI owns the
complete backend/feature matrix.

## Current status and remaining acceptance

Implementation is based on 294dce32. Native preparation/session isolation and the
three pilot migrations are complete. Token comparison preserved fixture sources,
expectations and modifiers, allowing only formatting changes; five structural
write operations are now individually scheduled. No build failures are carried.
The final fixture-module layout compiled after a naming-only reorganization.

Local validation passed 253 tests: language contracts (149), syntax examples and
related contracts (53), default methods (33), runtime registration (6), registration
sources (6), selected provider rejection/cleanup (3), session isolation (2), and
HIR fork/parser invalidation (1). Source-free SDK library checking, focused SDK
Clippy, formatting, structure and diff checks passed. Full CI remains unverified.
Relevant reproduction commands:

```text
cargo test -p kagari-embed --lib runtime::language_contract::
cargo test -p kagari-embed --test syntax_examples --test default_methods --no-fail-fast
cargo test -p kagari-embed --test engine_sessions --test registration_sources
cargo test -p kagari-embed --test native_provider_reset structural_agreement
cargo test -p kagari-embed --test native_provider_reset forged_source_signature
cargo test -p kagari-embed --test native_provider_reset callbacks_abort
cargo test -p kagari-runtime --lib native::module::tests::
cargo test -p kagari-hir --lib changing_parser_budget_invalidates_queries_without_changing_old_snapshots
cargo check -p kagari-embed --no-default-features --lib
```

The user confirmed concurrent CPU-intensive tests from another project. This
session therefore does not establish a performance comparison. In particular,
stopping the old serial syntax aggregate after 866.93 seconds proves neither
deadlock nor regression; all 53 reorganized tests subsequently passed. Rebuild
baseline and final binaries at their recorded revisions and compare them when the
machine is idle, without overlapping compilation/probes or other test projects.

Include retained metadata in that acceptance: forks share an append-only definition
identity allocator for the baseline's lifetime. Query state is independent, but
discarded sessions' interned identities can remain until their owner is dropped.
The [reduced-load recheck](performance-baseline.md#focused-preparation-recheck-2026-10-11)
ran nine selected tests twice, all passing, and compared the retained pre-change
preparation probe with current code. Runtime construction fell from a 222.275 ms
median to 0.016 ms; a new prepared source session took 0.852 ms. Compilation and
program preparation remain multi-second costs. Background CPU usage remained
observable, and matched whole-pilot timing/retained-identity growth are still open.
No whole-suite speedup, zero-copy or memory-reduction claim is established. Do not
migrate additional suites merely to extend the scope before completing that check.
