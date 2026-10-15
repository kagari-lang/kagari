# Test preparation, scheduling and source ownership

This finite task improves test preparation boundaries and selected contract owners.
The initial three SDK pilots and TP04's six additional SDK runners are complete.
TP04 also removes invariant preparation from compiler ABI, provider cancellation
and standard documentation loops. The user authorized TP05-TP09 on 2026-10-15;
source-owned test organization and focused local acceptance are complete. Native collection migration and compiler pipeline
redesign remain outside scope.

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
Historical reproduction commands at the TP01-TP03 checkpoint (the current
module filters are documented in [testing.md](testing.md)):

```text
cargo test -p kagari --lib runtime::language_contract::
cargo test -p kagari --test syntax_examples --test default_methods --no-fail-fast
cargo test -p kagari --test engine_sessions --test registration_sources
cargo test -p kagari --test native_provider_reset structural_agreement
cargo test -p kagari --test native_provider_reset forged_source_signature
cargo test -p kagari --test native_provider_reset callbacks_abort
cargo test -p kagari-runtime --lib native::module::tests::
cargo test -p kagari-hir --lib changing_parser_budget_invalidates_queries_without_changing_old_snapshots
cargo check -p kagari --no-default-features --lib
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

At the TP04 revision, reproduce the SDK selections with `cargo test -p kagari --test <target> --
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

Other historical TP04 checks:

```text
cargo test -p kagari-compiler --lib tests::bytecode::artifacts::rejects_
cargo test -p kagari-stdlib --test documentation
cargo clippy -p kagari --test standard_traits --test instantiation --test associated_types --test conversion_traits --test collection_interfaces --test collection_access --test native_provider_reset --test syntax_examples --test default_methods -- -D warnings
```

## In-source test organization (TP05-TP09)

Status: authorized on 2026-10-15 after the proposal at `e98064da`. TP05-TP08
implementation is complete; TP09 records focused acceptance and remaining limits.
TP01-TP04 above are implemented; their outstanding measurement remains open.
This section extends the same task rather than creating a parallel test-cost plan.

### Objective and observed cause

Default to tests inside the owning crate's source tree. Keep an integration target
only when its separately compiled consumer or process boundary establishes a
specific contract that an in-source test cannot establish equivalently.
Moving a test does not change the behavior it covers: SDK source/artifact/native
end-to-end cases remain SDK tests, even when they are now library unit tests.

The pre-migration `cargo metadata --no-deps --format-version 1` inventory contained
97 integration targets, excluding library/binary unit harnesses and doctests.
The SDK owns 51 and runtime 22. The ordinary Cargo runner executes binaries
sequentially and schedules tests within each binary concurrently
([Cargo execution model](https://doc.rust-lang.org/cargo/commands/cargo-test.html)).
A binary with seven short cases and one long case cannot fill freed workers with
cases from the next binary. More build jobs do not remove this execution barrier.

Before TP05, the SDK integration execution helper owned a
`OnceLock<Mutex<KagariEngine>>`. Compiling it separately into multiple targets created independent
baselines. Its lock protects session creation, not source compilation/execution.
A single source-owned helper can share preparation across the migrated SDK cases.
Do not assume every current case uses that helper.

These are scheduling/setup mechanisms, not a measured attribution of the user's
whole run. The earlier timing tables are historical. Library tests still have
per-crate barriers, and a single long case still runs serially. Neither a speedup
percentage nor full CPU utilization is an acceptance promise.

### Destination inventory

The implemented layout has five integration targets: two external SDK consumers,
one external runtime macro/Serde consumer, one VM allocation-contract target and
one ignored VM measurement target. This is an outcome of the exception inventory,
not a quota that permits deleting required coverage. TP05 must record any further
concrete exception here before changing that estimate.

| Crate | Before migration | Remaining targets | In-source owner |
| --- | ---: | ---: | --- |
| kagari | 51 | 2 | Engine, prepared-program and runtime contract test modules |
| kagari-runtime | 22 | 1 | Native, host, module, frame, session, GC and resource owners |
| kagari-vm | 7 | 2 | Existing `src/tests/` for ordinary execution contracts |
| kagari-bytecode | 1 | 0 | Program/portable identity tests |
| kagari-contract | 3 | 0 | Instantiation, native-default and public-wire contract tests |
| kagari-types | 2 | 0 | Language/metadata identity tests |
| kagari-stdlib | 2 | 0 | Documentation and registration tests |
| kagari-compiler | 3 | 0 | Existing source/program contract tests |
| kagari-hir | 2 | 0 | Type display and language-contract tests |
| kagari-package | 4 | 0 | Input, graph, workspace and analysis contract tests |
| **Total** | **97** | **5** | **92 fewer integration targets; cases retained** |

SDK target destinations below cover all 49 migrated targets. Keep
the old target basename as a leaf module and retain test function names, making
the old-target/new-filter mapping explicit. These are source module owners, not
four replacement integration binaries.

| Source module | Existing target names |
| --- | --- |
| `engine::tests` | `engine_sessions`, `package_inputs`, `registration_sources`, `source_modules`, `source_snapshots`, `standard_declarations` |
| `program::tests` | `cranelift_preparation`, `native_artifacts`, `native_preparation` |
| `runtime::tests::embedding` | `conformance`, `embedding_api`, `error_traces`, `generic_reload`, `host_interfaces`, `native_provider_reset`, `offline_composite`, `offline_functions`, `offline_nominal`, `runtime_transfer`, `scoped_calls`, `typed_calls` |
| `runtime::tests::language` | `array_operations`, `associated_constants`, `associated_types`, `callable_traits`, `collection_access`, `collection_callbacks`, `collection_interfaces`, `conversion_traits`, `default_methods`, `enum_payloads`, `generic_associated_types`, `instantiation`, `interface_identity`, `iteration_traits`, `library_namespaces`, `list_algorithms`, `native_enums`, `never`, `numeric_operations`, `operator_traits`, `result_option`, `standard_traits`, `string_interpolation`, `string_methods`, `syntax_examples`, `trait_inheritance`, `try_protocols`, `type_inference` |

The ordinary language cases belong under `src/runtime/tests/language/`; this
does not replace the existing `runtime::language_contract` runner or its route
assertions. Common stateless preparation belongs under `src/test_support/`,
declared once by the library, with source/native gates on the relevant modules.
Local tests can stay beside their implementation instead of entering a central
test directory. Library roots and `mod.rs` contain declarations/orchestration,
not the accumulated test bodies.

Runtime placement is similarly responsibility-based:

| Owner | Existing target names |
| --- | --- |
| Native registration/call contracts | `native_builder`, `native_conversion`, `native_execution`, `native_registration`, `host_registration`, `installation_access`, `offline_host`, `offline_types`, `host_nominal` |
| Host scopes and access | `host_borrows`, `host_objects`, `host_scopes`, `typed_path_views` |
| Module/layout contracts | `enum_declarations`, `package_exports`, `struct_layouts` |
| Frame/session owners | `execution_frames`, `execution_sessions` |
| GC/resource owners | `gc_ownership`, `mutation_resources` |
| Existing cross-subsystem runtime tests | `runtime_substrate` |

Six standard-provider composition suites from this table are owned by
`kagari-stdlib/src/tests/runtime_boundary`: `host_objects`, `host_nominal`,
`installation_access`, `native_builder`, `native_conversion`, `offline_types`.
They must use the same ordinary runtime crate instance as stdlib. The TP07
execution ledger records the type-identity failure that motivated this correction.

Move ordinary VM `library_collections`, `native_boundary`, `package_exports`
and `prepared_execution` into the existing `src/tests/` tree. Keep the native
boundary's current submodules rather than flattening its cases into one file.

The smaller crates' complete target inventory is: bytecode `identity`;
contract `instantiation`, `native_defaults`, `public_abi_wire`; types
`language_identity`, `metadata_identity`; stdlib `documentation`,
`registration`; compiler `language_foundation`, `package_exports`,
`source_programs`; HIR `identity_display`, `language_contracts`; package
`analysis`, `graph`, `inputs`, `workspace`. Move each into its existing
semantic owner, or a focused source test module when it spans several owners.
Using a filesystem, a backend or several crates is not by itself a reason for
an integration executable.

### Retained external/process boundaries

| Retained target | Required distinction and planned treatment |
| --- | --- |
| SDK `artifact_features` | `scripts/check_features.py` compiles this exact source in a standalone consumer outside workspace dev-feature unification. Keep its real imports, malformed-artifact/linking checks and source-free execution. Preserve the independent async artifact consumer and shared emitter providers. |
| SDK `async_execution` | The same script compiles this source without source/native features. Retain the native-wait/owned-execution consumer; move its source-only child suites into the SDK runtime test tree. Shared fixture construction must still compile in the external consumer without private SDK access. |
| Runtime `native_schema` | Preserve representative existing derive, `native_data!` and Serde cases compiled from another crate against ordinary runtime visibility. Move manual schema/conversion-only cases to their source owner when they do not establish that external boundary. Keep the existing Serde feature gate and lifetime/compile-fail doctests. |
| VM `allocation_contracts` | Combine existing `native_allocations` and `interface_allocations` as ordinary submodules of one integration target. Both use `native_allocations_counter` with a custom global allocator and thread-local measurement windows. Declare that allocator once; retain counter self-checks, warmup boundaries and exact assertions. Keep instrumentation outside the ordinary VM unit-test binary. |
| VM `library_measurements` | Keep the ignored manual timing workload and its custom allocator separate from ordinary correctness execution. Preserve its inputs, profile and explicit measurement invocation; do not count ignored timing cases as passed tests. |

The three VM targets currently include the same allocator implementation; that
does not justify three separate binaries. The two allocation-contract modules
can share it because their counters are thread-local. Manual timing remains
separate to avoid treating a correctness run as performance evidence.

Retain standalone AOT deployment under `scripts/check_aot.py`, executable
examples, and Rust lifetime/API compile-pass/compile-fail doctests. These are
distinct contracts, not extra integration targets to eliminate. Existing SDK
external consumers also exercise ordinary dependency builds of lower layers.
Do not replace proof of a production dependency graph with a library-unit build.

### Migration rules and risks

1. **Preserve cases before reorganizing semantics.** Record each old binary/test
   name, new qualified name, feature/platform gates, ignored status and exercised
   routes. Compare harness discovery where buildable and review macro-generated
   cases. Preserve fixtures, negative checks, GC thresholds, cancellation points,
   ordering and cleanup assertions. Do not merge tests into a single loop, remove
   cases, or add replacement smoke tests merely to compensate for a move.
2. **Audit the changed compilation boundary.** A library unit harness builds its
   own crate with `cfg(test)`; the old integration target tested the library as
   a normal dependency. Review conditional hooks, macros and feature availability.
   For example, HIR declaration queries have a test-only import-solver limit;
   migrated ordinary cases must not accidentally acquire different limits.
   Keep concrete external visibility/lifetime proof at the retained consumers or
   existing doctests. Never call a test-only bypass to make a moved case pass.
3. **Use real ownership and ordinary modules.** Rewrite imports of the tested
   crate to `crate::...`; keep other crates as dependencies. No new self aliases,
   broad re-exports, public test helpers, production dependencies, handwritten
   `include!`, or new umbrella test crate. Preserve the runtime's existing
   macro-required self alias. Use `#[path]` only for reviewed fixtures that really
   serve distinct external targets, not as a permanent way to leave every migrated
   test under `tests/`.
4. **Share preparation at the correct scope.** Declare SDK `test_support::execution`
   once and update all migrated consumers to that module; moving many copies of
   the helper would not unify their statics. Share sealed standard preparation,
   then create fresh sessions/runtimes. Custom registration, captured host state
   and invalidation cases retain independent setup. Do not unify the separate
   language-contract/analysis runner merely because both prepare declarations.
5. **Preserve consumer fixture ownership.** Inventory all `#[path]`,
   `include_str!`/`include_bytes!` and `CARGO_MANIFEST_DIR` uses before moving
   files. The feature consumer, example emitters and VM currently share some
   runtime/SDK fixtures. Move ordinary helpers into source; retain a narrow shared
   fixture location for actual cross-target consumers, with corrected relative
   paths. Do not copy providers or register the same cases twice.
6. **Check newly concurrent state.** Review static counters, temporary paths,
   generated artifact writes, environment/current-directory changes and callbacks
   previously separated by processes. Package scratch directories already use
   unique identifiers; preserve that property. Keep a mutable runtime, heap,
   cancellation token and captured application state per case. Process isolation
   is a justified exception only where scoped state cannot preserve the contract.
7. **Keep feature/platform coverage intact.** Place source/native/Serde/AOT gates
   on the smallest correct owner. A no-default-features library selection must
   still discover source-free cases and compile without source-only imports.
   Dev dependencies remain dev dependencies; standalone graph guards remain the
   authority for production isolation.
8. **Control the larger harness cost.** Fewer binaries can reduce barriers and
   duplicate preparation, but a larger library test binary may increase compile,
   link and peak-memory costs. Shared analysis retains interned IDs longer. Measure
   these effects and lock/session-fork contention; do not add a global runtime pool,
   unsafe Send/Sync, a nested worker pool or a whole-suite serial lock.
9. **Do not broaden into test deduplication or runner replacement.** Long serial
   scenarios such as the 4096-entry native cache test still need their stateful
   sequence. The stdlib documentation example loop also remains a separate future
   scheduling question. Keep current assertions and scenario boundaries in this
   layout migration. Nextest adoption, whole compiler optimization and collection
   migration are outside TP05-TP09.

Review changed Rust structure at each implementation checkpoint, including
effective LOC and shared-fixture ownership. An existing large test file is not
permission to create an oversized source module; split at scenario boundaries
as required, keeping each scenario's assertions together.

### Checkpoints and validation

- [x] **TP05: freeze inventory and pilot.** Confirm the 97-target mapping, external
  exceptions, feature gates and `cfg(test)` differences. Use SDK
  `associated_types`, `collection_interfaces` and `conversion_traits` as the
  first migration batch: all already use the standard preparation helper.
  Declare one source-owned helper and migrate those three targets to
  `runtime::tests::language`. Keep the remaining old targets runnable until
  their owning batch; remove the pilot's obsolete roots immediately. Compare
  matched pilot execution, compile/link costs and memory before expanding to
  TP06; record uncertain measurements without claiming performance acceptance.
- [x] **TP06: finish SDK ownership.** Migrate the other 46 ordinary targets in
  coherent engine/program/runtime batches. Move source-only async suites while
  retaining the external native-wait target. Resolve all emitter/consumer fixture
  paths in the same batch, with no forwarding integration entrypoints.
- [x] **TP07: runtime and VM.** Migrate the 21 ordinary runtime targets and manual
  schema cases, retaining external derive/Serde coverage. Migrate the four ordinary
  VM targets and combine the two allocation targets. Reconcile shared fixtures
  used by runtime unit cases, VM and retained consumers without widening APIs.
- [x] **TP08: smaller owners and references.** Migrate the remaining 17 targets,
  preserving existing unit-test ownership. Update affected CLI invocations in
  guides/scripts/CI and the testing guide's default placement rule. Do not silently
  activate unrelated queued plans or reclassify historical performance numbers.
- [x] **TP09: focused acceptance and handoff.** Verify mapping/discovery, selected
  correctness and representative external consumers. Record matched pilot timing,
  compile/link effects and memory limits together with the earlier TP measurement
  debt. CI performs the complete workspace/feature/platform matrix. Transfer lasting
  placement/exception rules to `testing.md`; retire this plan only when its required
  acceptance is satisfied, retaining unresolved follow-ups in the roadmap.

Each batch must build in its selected scope; this layout task has no planned
broken-build phase. Record failures and repair them before continuing to a new
owner. Conventional implementation commits use `Test-Preparation-Phase: TPxx`
trailers; this proposal's documentation commit is not an implementation checkpoint.

Local validation stays focused. For the TP05 pilot, list/build only the three
original targets and the new filtered library harness, compare the full selected
case-name inventory, and run their existing cases with `--no-fail-fast`.
The intended post-migration selection is:

```text
cargo test -p kagari --lib --no-fail-fast -- runtime::tests::language::associated_types:: runtime::tests::language::collection_interfaces:: runtime::tests::language::conversion_traits::
```

For later batches select moved behavior plus a small adjacent regression set:
registration/install rejection and GC cleanup for runtime; native callbacks and
pinned reload for VM; identity/codec rejection for the small contract owners;
filesystem alias rejection for package tests. Check each selected scope finishes;
a successful build or name listing is not a passed test. Use scoped Clippy when
imports/modules change, structure, formatting and `git diff --check`.

The external checks retain their own selection: SDK artifact/native-wait consumers
through the standalone manifest, runtime external derives with the relevant
Serde mode, the combined allocation target's existing allocation assertions,
and AOT only when its paths/contracts change. Do not run all package tests in
sequence as a disguised workspace sweep. Exhaustive no-default/source/native/
Serde/AOT/platform combinations and all doctests stay in CI.

Separate performance from correctness: build the selected old/new products first,
then compare exactly the pilot case set under the same toolchain, machine, profile,
features, default concurrency and fixture/cache state. Alternate at least three
old/new runs on an idle machine, record wall time, process CPU time, active-worker
tail and peak memory; report compile/link time separately. The old result is the
sum of its three sequential binaries, the new result is the same cases within
one harness. Keep scratch binaries/logs under `target/` and record conclusions
here. A broader shared-baseline lifetime sample must also account for retained
identity growth; a small pilot alone cannot prove whole-suite memory behavior.

Structural acceptance is the justified target inventory and preserved discovery/
contracts. Performance acceptance needs repeatable evidence; if results overlap
noise or compilation/memory worsens materially, record uncertainty and investigate
the relevant owner before claiming improvement. Local passing cases do not mark
CI acceptance complete.

### Original proposal ledger

2026-10-15: inventoried 97 integration targets at `e98064da`; inspected SDK shared
preparation, standalone feature/AOT consumers, runtime derive/Serde coverage,
VM allocator instrumentation, existing source test modules and CI commands.
Proposed 92 target removals/consolidations with five justified remaining targets.
No Rust implementation changes, migrations or performance runs have been made.

### Execution ledger

2026-10-15, TP05 started: preserved target metadata, source inventory and the
three old pilot executables under ignored `target/`. The pilot moves 43 cases
into source. The old external execution helper remains temporarily for pending
TP06 consumers; TP06 removes it after their imports move to the single source
helper. This bounded bridge preserves runnable intermediate checkpoints.
Baseline selected build: 24.76 s, warm dependencies with some source rebuilds;
this is not a clean compile/link comparison. Machine: i9-12900K, 24 logical CPUs,
rustc 1.99.0 (b940084d7), x86_64-pc-windows-msvc, default source/native features,
workspace test profile (opt-level 1), default Cargo/harness parallelism.
Concurrent cargo install-update work prevents treating samples as idle-machine
performance acceptance.

TP05 checkpoint: all 43 original case names map exactly to the three new leaf
modules; old and new selected runs passed. Structure (1231 files), SDK Clippy
including test targets, formatting and diff checks passed. Candidate warm build
was 6.50 s versus baseline 24.76 s with different rebuilt dependencies, so these
are build observations, not a compile/link speedup. First alternating sample:
old 116.96 s / 820.22 CPU s / 961 MiB peak working set; new 84.23 s / 882.53 CPU s /
1419 MiB. Increased simultaneous source sessions explain a higher process peak;
this requires retained-memory follow-up rather than a whole-suite claim. Saved
products continue the three-round comparison while TP06 proceeds; later build
load is recorded as interference. No execution semantics or assertions changed.

TP06 checkpoint: SDK integration targets reduced from 51 to 2. All 483 explicit
SDK cases preserve names and ignore/should-panic attributes; the syntax-example
macro and its invocation remain intact apart from relocated fixture paths. The
default library harness discovers 662 total cases, including pre-existing unit
contracts. Source-only library compilation also passes. Five selected session,
forged-result, async lifecycle and pinned-reload contracts passed; three retained
consumer cases passed in the workspace. SDK Clippy, structure (1234 files),
formatting and diff checks pass. Shared cancellation/route helpers have one
source owner; the temporary external execution helper is removed. Native enum,
provider, residual and async fixture files remain shared with real consumers.
The standalone source-free consumer is being checked separately; the initial
network-index wait was stopped and restarted offline, with no source change.

Three alternating TP05 runs all passed: old wall 116.96/127.32/124.23 s, new
84.23/81.30/82.36 s; median CPU 820.22 vs 882.09 s, median peak working set
956.7 vs 1425.6 MiB. Concurrent installation/build work makes these exploratory
samples. The final 20 percent of each new run averages about 0.97 logical cores;
the numeric-aggregation overflow case remains the last serial workload. More
concurrent sessions are consistent with the higher peak, but retained identity
growth and allocator retention are not isolated by these process-peak samples.
No whole-suite performance or retained-memory acceptance is claimed.

TP07 ownership correction (2026-10-15): the first runtime library build exposed
E0308/E0271: `kagari-stdlib` returns ordinary-dependency runtime types, which differ
from the runtime unit harness's own types. The six affected suites (`host_objects`,
`host_nominal`, `installation_access`, `native_builder`, `native_conversion`,
`offline_types`) therefore belong under `kagari-stdlib/src/tests/runtime_boundary`.
Their standard-provider/runtime composition remains unchanged and the other 15
ordinary runtime targets keep their runtime source owners. Add only the compiler
source dev dependency needed by these existing cases to stdlib. No production
dependency, self alias, type conversion, assertion or integration target is added.
This replaces the original six destination entries; the five-target acceptance
inventory is unchanged. The same build also exposed missed grouped imports of
the relocated layout fixture; repair those at the actual source owner before the
TP07 checkpoint. HIR's test-only solver limit defaults to None, matching its
ordinary build; runtime lifetime probes add observation, not validation bypasses.

TP07 checkpoint: all 184 explicit runtime and 208 explicit VM cases preserve
names and ignore/panic attributes. Runtime now retains only native_schema; VM
retains allocation_contracts and library_measurements. Two manual registration
rollback cases moved beside schema.rs; external derive/Serde cases remain.
Selected runtime rollback/GC cases (3), stdlib composition cases (2), empty-data
scope plus external derive/Serde (3), VM callback/generation cases (2) and all
five allocation assertions passed. The ignored measurement workload was compiled,
not executed. Scoped Clippy found a duplicate program fixture in the larger VM
harness; one declaration at tests::native_program resolves it, and VM Clippy
now passes. Runtime/stdlib Clippy had no findings. The empty native_data marker
also exposed an unused generic check for zero fields; emit each field check
inside its repetition, preserving the type bound and compile-fail contract.
Structure (1243 files), formatting and diff checks pass. Shared native fixtures
remain narrow cross-target consumers. SDK standalone source-free checks finished:
forged native-import rejection and owned native wait both passed offline.

TP08 checkpoint: migrated all remaining 17 targets (89 explicit cases) to
bytecode/contract/types/stdlib/compiler/HIR/package source owners. Case names and
ignore/panic attributes match the pre-migration inventory. Updated current guides,
source links and queued-plan commands; historical timing commands remain labeled
with their original layouts. No CI or feature-script test target was removed.
Selected artifact decoding and scoped identity cases passed (2); compiler export
rejection, HIR readonly rejection, filesystem alias rejection and stdlib installation
passed (4). The ABI bound fixture failed both its original integration target and
its new module: stale bytes omitted host policy and receiver-access fields, causing
an invalid enum tag (1684104562) before the intended count reader. Updated only
the valid header using typed HostCallPolicy/MethodPolicy values, retaining both
oversized counts and the exact limit-error assertion. The repaired bound case and
portable ABI roundtrip passed (2). The temporary original-target reproduction was
removed. Scoped Clippy for the seven affected libraries/test harnesses passed.
The native_data compile-pass and compile-fail doctests also passed after the TP07
empty-field warning repair. All 13 production dependency graph guards and the
ABI/contract build-graph guards passed; stdlib compiler use remains dev-only.
Current Cargo metadata confirms exactly the five documented integration targets.

TP08 lightweight checks: structure reports 1245 Rust files, zero violations and
zero exceptions. Changed documentation has no broken relative links. Formatting
and diff checks pass. All seven smaller owners compile in the selected scope;
CI still owns exhaustive feature/platform cases and workspace execution.

TP09 local acceptance: Cargo metadata contains the five documented targets;
92 integration executables were removed/consolidated. The preserved source
inventory contains 964 explicit cases (including platform/feature-gated and
ignored cases), plus 45 syntax macro cases whose names, logical filenames and
resolved fixture paths match. Syntax-aware comparison of 931 relocated test
bodies is identical after reviewed own-crate/path rewriting and rustfmt. The
manual schema/async split bodies also match; the only intentional test-body
repair is the independently reproduced stale ABI header described under TP08.
Case preservation is not a claim that every case was locally executed.

Focused local correctness, scoped Clippy, source-only SDK compilation, standalone
source-free consumers, selected Serde/derive and allocation boundaries, macro
compile-pass/compile-fail doctests and dependency graph guards passed as recorded
above. No build failure remains. Final lightweight checks cover structure,
formatting, relative documentation links, CRLF and diff hygiene. CI still owns the
full workspace, complete features/backends/platforms and remaining doctests.

The [pilot and lifetime report](performance-baseline.md#source-owned-test-pilot-2026-10-15)
records all three alternating runs and the completed 128-session lifetime control.
Same-name replay stabilizes retained IDs; it does not eliminate distinct-name
retention or establish whole-suite memory acceptance. Exploratory wall-time gains
come with a higher process peak. Controlled compile/link comparison and the older
TP01-TP04 matched preparation/retained-memory acceptance remain open; the plan is
retained for those explicitly deferred measurements, not for unfinished layout
implementation. Durable placement/exception rules are in testing.md. Temporary
probes and the original-target failure reproduction have been removed.

CLI follow-up: reproduced the project artifact test failing at runtime loading
with `native declaration differs from its installed contract`. CLI builds install
the host logging contract, but the test loaded their artifacts with the default SDK
engine. The test now uses the CLI runtime engine, preserving all artifact, export,
lock and edited-dependency assertions. Both `project::tests::` cases pass;
CLI binary/test Clippy, structure, formatting and diff checks pass. This focused
fixture repair does not change production behavior or the pending CI scope.
