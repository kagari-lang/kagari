# Test preparation and scheduling

This finite task improves test preparation boundaries and selected contract owners.
The initial three SDK pilots are complete. The authorized next slice consolidates
six ordinary SDK runners and removes invariant preparation from compiler ABI,
provider cancellation and standard documentation loops. Native collection
migration, compiler pipeline redesign and the remaining suites are outside scope.

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
- [x] TP04: migrate the scalar route runners in `standard_traits`, `instantiation`,
  `associated_types`, `conversion_traits`, `collection_interfaces` and
  `collection_access`; preserve route, root-cleanup and GC assertions.
- [x] TP04: prepare invariant compiler ABI input and cancellation providers once
  per scenario; reuse native-only analysis for independent documentation examples.
- [x] TP04 validation: focused existing contracts and structural checks.
- [ ] Final measurement: matched timing and retained-memory comparison, deferred
  until optimization is complete and machine load permits a meaningful comparison.
  Consolidate measured results and retire this plan when complete.

## Evidence and validation

The historical audit is in [performance-baseline](performance-baseline.md).
Initial inspection at 294dce32 found `KagariEngineBuilder::install_all` discarded
the validated runtime registry, then `KagariEngine::runtime` repeated installation.
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
No whole-suite speedup, zero-copy or memory-reduction claim is established.

The user authorized TP04 without waiting for idle-machine measurements. The next
attempt completed both 33-case default-method suites, but background CPU differed
(approximately 71% versus 45% of total capacity), so their times are not speedup
evidence. The old syntax aggregate and remaining retention controls were stopped
on request. Retained baseline/current binaries and logs stay under ignored
`target/`; final measurement is deferred, not a gate on the TP04 changes above.

A partial retention diagnostic completed before measurement was deferred: 60
sessions with 32 uniquely named functions each grew the baseline identity table
from 1,104 to 3,084 entries. After dropping each session, tracked live Rust
allocation requests grew by 435,060 bytes in total. Dropping the baseline released
its ownership; process-global standard registrations remained. This confirms the
append-only lifetime contract, not a matched RSS or leak comparison. Same-name
and fresh-engine controls were not completed. The bounded test fixtures can use
this ownership model; an unbounded shared source service requires a separate
lifetime/identity design, outside TP04.

TP04 implementation and correctness are complete. Six SDK scalar runners now use
the existing helper; their test bodies were compared exactly with 7e7cb90f and are
unchanged. Original GC thresholds and all three routes remain. The shared helper
also preserves the migrated owners' result-release/root-cleanup assertion. Custom
host, artifact mutation and invalidation paths retain their existing preparation.

The ABI sweeps compile once per test while still building, serializing and checking
each incompatible artifact. The cancellation sweep retains one scenario-owned
engine and creates a fresh runtime/token/observer for every point; only stateless
`from_fn` callbacks execute in that scenario. Documentation examples fork a checked
native-only analysis and inspect its diagnostics directly, with all documentation
and example coverage retained. No additional global cache or worker pool was added.

Local validation passed 19 tests: two representatives for each migrated SDK owner,
the cancellation sweep, two prior helper consumers, three artifact rejection
contracts and the full documentation contract. All nine affected SDK test targets
compiled, scoped SDK Clippy passed, and structure (1,036 files, no violations),
formatting, documentation links and diff checks passed. No full workspace run or
performance acceptance was attempted for TP04; CI remains unverified.

Reproduce the SDK selections with `cargo test -p kagari-embed --test <target> --
<filters> --exact` using these space-separated filters:

| Target | Filters |
| --- | --- |
| `standard_traits` | `standard_trait_methods_and_generic_bounds_execute structural_and_identity_keys_execute_through_artifacts` |
| `instantiation` | `contextual_phantom_layouts_execute_from_source_and_encoded_artifacts trait_calls_use_checked_parameter_context` |
| `associated_types` | `generic_implementations_normalize_nested_associated_types imported_associated_types_keep_trait_identity_and_interface_bindings` |
| `conversion_traits` | `aliased_and_generic_from_bounds_evaluate_once iterable_terminals_delegate_and_fold_allocated_values_in_source_order` |
| `collection_interfaces` | `native_collection_views user_containers_implement_the_same_storage_independent_contracts` |
| `collection_access` | `constructors_and_live_views access_is_shallow_and_preserved_by_calls_closures_and_branch_joins` |
| `native_provider_reset` | `cancellation_boundaries_unwind_nested_provider_callbacks` |
| `syntax_examples` | `array_operations` |
| `default_methods` | `generic_implementations_and_default_local_binders_are_specialized` |

Other focused checks:

```text
cargo test -p kagari-compiler --lib tests::bytecode::artifacts::rejects_
cargo test -p kagari-stdlib --test documentation
cargo clippy -p kagari-embed --test standard_traits --test instantiation --test associated_types --test conversion_traits --test collection_interfaces --test collection_access --test native_provider_reset --test syntax_examples --test default_methods -- -D warnings
```
