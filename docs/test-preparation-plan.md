# Test preparation, scheduling and source ownership

CM00-CM04 subsequently moved the VM package/tests into kagari-runtime. Historical
target counts and timing labels below retain their recorded names; current test
owners and commands are in the [testing guide](testing.md).


This finite task improves test preparation boundaries and selected contract owners.
The initial three SDK pilots and TP04's six additional SDK runners are complete.
TP04 also removes invariant preparation from compiler ABI, provider cancellation
and standard documentation loops. The user authorized TP05-TP09 on 2026-10-15;
source-owned test organization and focused local acceptance are complete. The
[TP10-TP13 directory readability follow-up](#directory-readability-follow-up-tp10-tp13)
was recorded on 2026-10-28; its direction is agreed, but implementation is deferred.
The user authorized [TP14 contract-test pruning](#contract-test-pruning-tp14) on
2026-11-07, independently of the deferred layout and measurement work.
Native collection migration and compiler pipeline redesign remain outside scope.
The user additionally authorized [persistent frontend benchmarks](#persistent-frontend-benchmarks)
on 2026-11-07: remove completed Rowan-migration tooling first, then establish HIR
analysis/lifecycle measurements. This adds measurement infrastructure, not a new
production optimization or acceptance claim for the deferred final comparison.
The user also authorized the [frontend source-fixture follow-up](#frontend-source-fixture-follow-up)
on 2026-11-07, starting with fixed syntax/HIR inputs rather than another test
framework or a production refactor.

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
- [x] TP14: remove evidenced duplicate contract tests and redundant preparation;
  validate retained owners without changing production behavior or test harnesses.
- [ ] Final measurement: matched timing and retained-memory comparison, deferred
  until optimization is complete and machine load permits a meaningful comparison.
  Consolidate measured results and retire this plan when complete.

## Persistent frontend benchmarks

Scope: retire AST migration-only dependency/acceptance rules and their tests;
retain workload/environment validation and neutral timing/memory comparisons.
Then retain a small HIR driver for cold staged/direct analysis, warm queries,
body/signature edits, selected-body queries and snapshot destruction. Production
analysis algorithms, visibility, caching and compiler/runtime backends do not change.

- [x] HB0: remove Rowan-only lockfile allowances, TA0 archive recovery and fixed
  migration speed/memory budgets. Ten focused Python tests pass. AST comparisons
  now require identical lockfiles and existing environment/toolchain checks.
- [x] HB1: freeze plain/generic scales (24/96/384 functions) and one dependency-edit
  workload, with semantic evidence and independent state for each measured route.
- [x] HB2: implement an uninstrumented timing target and sequential process runner;
  separate staged queries from fresh direct snapshot/checked-program routes.
- [x] HB3: add a separate allocation-instrumented target sharing the lifecycle
  recipe; never reset live allocation accounting across retained snapshots.
- [x] HB4: validate selected existing contracts and driver evidence, record a
  current-machine baseline and the limits of unrun CI/platform scope.

Measurement ownership: standard catalog construction and source preparation are
reported separately or excluded explicitly; a fresh analysis database still pays
provider analysis. Every cold/edit sample starts from its declared cache state.
Retained-result body counters describe result construction, not necessarily the
latest query. Time and allocation instrumentation run separately. Baselines require
identical workload/protocol/toolchain/environment and lockfile fingerprints; no
cross-machine or historical Rowan speedup is inferred. Raw samples belong under
`target/`; durable methods/results belong in the testing/performance guides.

The method follows the existing [HIR phase experiment](performance-baseline.md#hir-architecture-and-candidate-filtering-macos-2026-11-06)
and [rustc-perf collector](https://github.com/rust-lang/rustc-perf/blob/master/collector/README.md)
(consulted 2026-11-07): separate full, unchanged and edited work, isolate setup and
profile separately. Kagari measures in-process source databases and retained `Arc`
snapshots, not rustc's disk incremental cache. No general benchmark framework or
production instrumentation is needed. Complete workspace/platform tests remain
CI-only; ordinary test runs do not execute performance campaigns.

HB0-HB4 are complete. Durable operation/measurement contracts are in the
[frontend benchmark guide](frontend-benchmarks.md); the
[current Windows baseline](performance-baseline.md#persistent-hir-baseline-windows-2026-11-07)
records machine, source/harness identities, methods, results and limitations. The
older final test-preparation optimization comparison remains deferred, independently
of this completed infrastructure work.

Local acceptance used the 17 AST/HIR Python tooling tests, all three size-96
workloads in timing/memory correctness mode, and the existing HIR contracts
`native_declarations_are_shared_across_user_revisions_and_retained_by_snapshots`,
`imported_signature_changes_invalidate_a_cached_function_body`, and
`associated_definition_changes_invalidate_cached_body_and_keep_old_snapshot`.
These sample provider ownership, dependency invalidation and retained-result
correctness without replaying the complete language suite. Workspace Clippy,
formatting, structure (zero findings) and diff checks passed. CI now runs the
Python tooling tests and the small HIR correctness set, not performance campaigns;
complete workspace/platform/backend tests remain unrun locally.

The release campaign validates all 24/96/384 timing scales: 49 timing and 21 memory
samples. Its outer 1,200 s timeout occurred after all timings and 19 memory samples;
the last two memory processes were completed from the unchanged frozen image after
source/environment/archive validation. The baseline records this interruption;
no timing was resumed or substituted. A separate plain/24 same-image pilot passed
fresh AB/BA and saved-only comparison validation, without a speedup claim. An
initial release build exposed private `ModuleFacts::type_table` access in the new
assertion; switching to its public accessor resolved it before measurement. No
carried build errors remain. Existing AST performance numbers were not rerun.

The subsequent user-authorized layout cleanup moves both frontend drivers from
`examples/` to `benches/`, with explicit `[[bench]]`, `harness = false` and
`test = false` targets. Runners discover executable paths from Cargo artifacts;
AST's explicit `--executable` bypass remains available. Workloads, measurement
blocks and assertions are unchanged; only Cargo's injected `--bench` argument is
accepted in addition to driver options. Old HIR archives are not migrated across
changed target/harness/tooling fingerprints. Recorded results remain pre-relocation
evidence; collect a new baseline only for the next performance comparison.

Migration validation: 18 focused AST/HIR tooling tests pass, direct Cargo bench
AST correctness and HIR manifest invocation pass, and the HIR runner's plain/96
check passes in timing and memory modes. This samples both CLI entry paths and
both executable types; no production analysis changes require another full
performance campaign or exhaustive language-test matrix.

The baseline exposes substantial warm-query preparation despite retained body
results. Current declaration/signature/environment preparation precedes reuse
checks; no phase-level attribution or production optimization was performed.
Any follow-up must profile those owners before proposing new caches. This is an
observation for later diagnosis, not an additional acceptance phase here.

## Frontend source-fixture follow-up

The first bounded batch separates complete test programs from Rust orchestration.
It does not change semantic owners, test names, discovery, execution routes or
assertions, and does not reopen TP14's pruning work or TP10-TP13's directory moves.
The lasting input-placement rule is in [testing](testing.md#classify-the-contract-before-choosing-a-format).

- [x] Externalize eight syntax inputs: six positive language-conformance cases,
  the combined generic-item parser case and the attribute/trivia program. Keep
  their named cases, spec references, losslessness and structural assertions.
- [x] Externalize seven contextual-trait programs, four lowering programs and two
  static-initializer programs. Keep dispatch/receiver/type facts, recovery,
  codegen admission, local owners, capture and constant-table assertions in Rust.
- [x] Externalize the struct-update factory declarations and the fixed generic/
  projection program. Existing field-order, invalid-base, bound and never/recovery
  tables retain their Rust orchestration and short varying fragments.
- [ ] Later batch: diagnostic-focused parser/standard-library cases using existing
  UI admission and typed checks. The HIR UI owner is rejection-only; do not remove
  its error guard to accommodate positive fixtures.
- [ ] Later batch: remaining fixed trait/inference programs and selected long
  multimodule/revision inputs, after reviewing logical paths and span sensitivity.
  Preserve incremental/native/cancellation lifecycles; do not build a generic DSL.

The 23 source fixtures live under syntax `tests/fixtures/parser/conformance` and
`attributes`, and HIR `tests/fixtures/typeck/traits`, `lower`, `analysis/statics`
and `analysis/struct_update`. All inherit the repository-root LF rule. Initial
extraction verified 21 original runtime byte matches; the two non-byte-sensitive
statics programs removed incidental embedding indentation. The user then requested
normal source formatting: all 22 ordinary inputs now pass `kg fmt` and `fmt --check`,
without copied leading/trailing blank lines or Rust-string layout. The attribute/
trivia witness retains its authored delimiter comment and layout. Logical source
names, Rust assertions and execution routes remain unchanged, including factory
fragment consumers. The same 11 syntax and 20 HIR tests pass after formatting.
No load-time normalization, whitespace exceptions, new golden files or production
visibility changes were introduced.

Selected local validation passes: 11 syntax tests cover all moved inputs plus
neighboring rejection cases; 20 HIR tests cover all moved inputs, every consumer
of the extracted factory declaration and adjacent contextual-call rejection paths.
Before/after library test discovery matches exactly: 141 syntax and 637 HIR
entries, with unchanged names and multiplicities. Discovery is not execution;
no full-workspace suite or exhaustive platform/backend matrix was run locally.
Structure (1,858 Rust files, zero findings), formatting, workspace Clippy and
`git diff --check` pass, as do new-fixture whitespace and LF-attribute checks.
Review found no import, visibility, re-export or module-ownership changes. The
improvement is clearer Rust ownership, not a claim of fewer total repository
lines or faster compilation.

The user-authorized attribute cleanup consolidates ordinary `.kgr` LF handling in
root `.gitattributes`, removing 19 redundant local files and the `.kgr` line from
four mixed-rule files. Other-format rules and the dedicated literal fixture's
whitespace exception remain local. No source renormalization or index staging is
part of that cleanup; checkout bytes were preserved at that checkpoint. Verification
confirmed unchanged bytes for all 543 Kagari files and effective `text=set, eol=lf`
attributes for each; 89 paths newly inherit the policy. All other effective
attributes, including whitespace exceptions, are unchanged. `git diff --check`
passes. This configuration/documentation-only step does not rerun Rust builds or
tests; the preceding source-fixture validation remains separate evidence.

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
Full-workspace tests remain CI-only. Run workspace Clippy locally for implementation
acceptance (`cargo clippy --workspace --all-targets -- -D warnings`), affected SDK suites,
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

## Contract-test pruning (TP14)

### Scope and preservation

The 2026-11-07 audit found duplicate smoke tests, upper-layer repetitions of
lower-layer rules, historical/multiplicative matrices and unused preparation.
The user first authorized a new branch and a conservative implementation batch,
then explicitly requested completion of all three audited pruning directions:
remove evidenced weak/empty copies, migrate unique assertions to existing
scenarios, and reduce history/spelling/Cartesian matrices with explicit losses.
Earlier case-preservation requirements govern relocation, not this explicitly
authorized removal of redundant cases. Distinct assertions and execution
boundaries remain mandatory; lower test counts are not an acceptance target.

The initial batch owns:

- Parser duplicate rejection cases, an empty grammar-gap test and its unused
  payload model; inventory checks still reject undeclared gap IDs.
- Four HIR rebinding conformance copies already owned by typed diagnostic tests,
  and three malformed `let` inputs that could pass on syntax errors instead of
  their claimed annotation/type errors. Retain valid call/index admission cases
  and the existing valid-syntax annotation recovery owners.
- VM duplicate reflection, debugger-construction and DynamicCall rejection
  smoke tests, plus a runtime registry lookup copy. Retain initial field reads,
  reflection mutation/debugger control, host paths and publication rejection.
- Two SDK language smoke cases, and consolidation of runtime/helper ABI
  preparation rejection using one compiled artifact. Preserve both error codes
  and variants; actual reload failure/old-version assertions keep their owner.
- Compiler identical-input arithmetic/metadata preparation, a scalar foundation
  case already covered by the codec contract, and immediate revalidation of
  unchanged verified bytecode. Keep explicit Add selection assertions.

The authorized completion also owns the finite audit candidates below. It is
not a requirement to remove every similar-looking test: independent oracles,
public/dependency/product boundaries and lifecycle assertions remain protected.
Production code, dependency/feature/API/ABI changes, test layout moves, global
caches and new harnesses remain out of scope. Test-only support and browser/editor
test runners may change. No timing or memory improvement is claimed without
matched measurements.

### Acceptance and ledger

- [x] Inspect retained counterparts and record the finite deletion scope.
- [x] Implement the selected duplicate removals and assertion-preserving merges.
- [x] Run selected retained-owner tests and directly adjacent boundary regressions.
- [x] Complete structure, formatting, workspace Clippy and diff checks.
- [x] Reconcile local results, pending CI and any carried errors.

Local tests select the retained parser rejection/grammar inventories, four HIR
typed diagnostics plus valid annotation recovery and call/index admission,
compiler metadata/foundation/codec contracts, VM helper/debugger/publication
owners, runtime metadata, and SDK ABI preparation/language/real reload owners.
These representative cases directly establish the removed copies' contracts;
they are not a package-by-package workspace sweep. CI retains exhaustive
workspace, source-free, feature/backend/platform and product acceptance.

2026-11-07: completed the bounded implementation and local acceptance on
`test/prune-redundant-contract-tests`, starting from clean `a82d9c4b`. Static
registration comparison shows 13 fewer ordinary tests and five fewer named
parameterized cases (18 net, including the empty gap test). Three malformed
annotation inputs and one exclusively consumed scalar fixture were removed.
No production code, test harness, feature or dependency changed.

Independent diff review found no blocking coverage loss. The parser owner now
retains the deleted copy's first-diagnostic error assertion, not merely an
any-error check. The deleted combined VM smoke's zero-argument native path is
also exercised by the retained comparator/Probe lifecycle, separately from the
two-argument host-call test. The remaining installed-access mutation/debugger
control scenario is unchanged. Both preparation ABI errors retain their exact
codes and variants; real reload rejection and old-entry usability remain covered.

Focused local validation passed 36 distinct tests, with the strengthened parser
case rerun after its assertion was merged:

| Package | Passed | Selected owners |
| --- | ---: | --- |
| `kagari-syntax` | 5 | `invalid_syntax_diagnostics_reject_non_spec_forms` and the four `tests::grammar_coverage::` tests |
| `kagari-hir` | 7 | `ui::typed::{val_local_rebinding, parameter_rebinding, rejects_assignment_to_val_field, rejects_assignment_to_const}`, retained mutation acceptance, `composite_annotations_retain_structure_without_authorizing_codegen`, and `erroneous_calls_and_indices_never_enter_codegen` |
| `kagari-compiler` | 7 | `lowers_function_metadata_into_bytecode`, three `language_foundation::compile_success` cases, `portable_program_codec_rebuilds_seals_and_preserves_canonical_lowering`, `normalizes_ir_operands_as_typed_values`, and `flattens_branch_targets_to_instruction_offsets` |
| `kagari-vm` | 10 | Type-of/initial-read/get-set reflection, host helper/path results, live-locals/step-trap debugger, unsupported-call rejection, retained installed-access mutation/control, and `comparator_failure_stops_callbacks_and_preserves_original_elements` |
| `kagari-runtime` | 1 | `metadata_registry_carries_reload_and_path_validation_records` |
| `kagari` | 6 | `preparation_rejects_incompatible_runtime_and_helper_abis`, namespace-shadowing, applied-trait argument distinction/marker acceptance, per-execution cancellation, and actual prepared reload failure |

Structure (1,818 Rust files, no findings/exceptions), formatting, workspace
Clippy with `-D warnings` and diff checks passed. Initial formatting checking
found two extra blank lines left by deletions; both were corrected. No build,
test or lint failures remain. Full-workspace tests, exhaustive feature/backend/
platform and external product checks were not run and remain CI-owned; these
focused results do not establish complete CI acceptance or measured speedup.
TP10-TP13 and performance acceptance remain deferred.

### Completion of the three pruning directions

The follow-through removes the remaining audited weak copies in syntax/HIR,
SDK readonly/identity/Eq checks and basic interface calls, VM scalar native calls
and repeated missing-entry checks. The HIR mutation smoke and its now-empty
module are removed; typed-fact mutation owners remain. All decisions below are
implemented, not a new queue.

Unique assertions now run on existing prepared scenarios:

- Parser delimiter count/severity/typed-kind checks share their original UI
  fixture parse and full JSON comparison. New parser UI fixtures must declare
  their typed expectation rather than silently skip it. Both original inputs
  and JSON expectations are unchanged.
- Opaque declaration source-mode rejection shares the substantive declaration
  scenario; documentation/losslessness and top-level/nested distinctions remain.
- VM field reflection returns the executed initial and updated reads as `(1, 9)`.
  The weaker installed-access write is removed, but debugger control remains.
- Tuple `ScriptOwned` classification moves to the value owner. Trait method
  origin, trait type and path-access records move to metadata registration;
  the inherent method and a minimal `Runtime::types()` facade check remain.
- Full artifact-header and root-function-count equality move to the existing
  associated-interface MIR/KBC roundtrip; its verification remains independent.

Preparation is actually reduced, rather than moving old preparations into loops:

- HIR `lower_ok` lowers its already checked parsed tree. The standard hash/numeric
  checks also stop discarding a separate lowering for seven analysis inputs.
- Six SDK runners retain all 41 source scenarios and 123 direct-interpreter,
  decoded-interpreter and decoded-JIT route executions. Encodes/decodes fall from
  82 to 41 and artifact preparations from 123 to 82. Each route still creates a
  fresh context/token/runtime, uses its original GC threshold, drops the result
  before root checks, and retains the Result/Option collection assertion.
- TextMate still checks every one of five operands under both EOLs, but tokenizes
  two complete documents instead of ten isolated fragments. The first fragment
  starts fresh; the other four per EOL now inherit preceding document state.
  This relinquishes eight independent fresh-stack starts, not operand assertions.

#### Retained samples and relinquished combinations

| Owner | Before -> after | Explicitly relinquished coverage |
| --- | --- | --- |
| Compiler const ABI | 115 historical-format serializations -> 0 | The unrelated const-fixture x old-format combinations; const fingerprints/float bits remain |
| Runtime ABI rejection | 28 artifact roundtrips / 56 loader checks -> 1 / 2 | Runtime ABI v5-v31 x current/matching-old requirements; v32 retains both requirements and exact error kind |
| Helper ABI rejection | 2 artifact roundtrips / 4 loader checks -> 1 / 2 | Helper v3 x both requirements; v4 retains both and its exact error kind |
| KBC wire format | 115 old-version attempts -> 3 representative invalid formats | Exhaustive versions 1-114; wire and in-memory owners now check zero, previous (115) and future (117), relative to current 116; loader retains current and matching-invalid requirements |
| CLI unrelated options | 23 options x 3 positions = 69 -> 23 | Positions 1 and 2 for each illegal option (46 combinations); position 0 retains every option and escaping assertion; nonempty positive modes still cover all three positions |
| CLI empty option list | 3 identical argument vectors -> 1 | Only two duplicate executions of the exact default-mode input |
| Formatter rejection | 39 -> 5 upper-layer cases | Detailed malformed spellings listed below; retain surrogate char, interpolation expression, bad byte hex, unclosed raw bytes and generic malformed input |
| Formatter literal EOL/mode | 15 x 2 x 3 = 90 -> 60 | LF input with explicit LF and CRLF input with explicit CRLF for each literal (30 combinations); Preserve and opposite-forced EOL retain all literals and idempotence |
| Formatter CRLF/empty | Two duplicate table rows removed | Width-40 repetitions; exact bytes and second-format idempotence now share width-100 owner preparation; whitespace-only remains |
| Analyzer multiline edits | 8 edits x 2 EOLs = 16 -> 6 | Debug-format space, member interpolation, incomplete hole, Unicode byte escape and raw-byte hash mismatch under both EOLs; retain delimiter damage, non-BMP insertion, restore, coordinates and old snapshots |
| Common snapshot capacity | 3 cycle counts x 4 states = 12 -> 4 | Cycle counts 1 and 128 for repeated/distinct x drop/retain (8 combinations); 16 cycles retain all four states and existing COW/prefix/Weak assertions |
| Python unsigned CSV fields | 79 fields x 8 spellings = 632 -> 79 field checks + 8 grammar checks = 87 | Seven additional spellings per field (553 full-report combinations); all eight spelling classes remain at `_rows`, all 79 suite/field mappings remain on public parsing paths with exact field-error checks |
| Pages deploy guard | 288 -> 15 contexts | 273 cross-products, chiefly multiple disabled conditions at once; exact YAML conjunction, both normal event kinds, all opt-in classes and each independent failure remain |
| Native editing grammar | 19 byte corpus cases -> 5 tree goldens | 14 CLI-corpus copies of Node-owned rejection inputs; retained goldens include accepted trees and unterminated raw-byte EOF recovery; refreshed native binding proves retained negative owners |
| Grammar witness | 3 full-file parses -> 2 | The valid-only copy; alias/static owners retain whole-file validity plus node assertions |
| Direct WASM old integer spellings | 4 -> 2 compile inputs | Type-name `isize` and suffix `1usize` on the actual WASM product; retain type-name `usize` and suffix `1isize`; native owners still check both spellings in each category |
| Product recipe source matching | Two source-text assertions removed | Duplicate static checks of command spelling/order; actual command recording and bad-graph early-stop remain, as do top-level wiring and direct graph-negative checks |

The formatter no longer repeats these named character-rejection rows:
`empty`, `multiple_scalars`, `combining_sequence`, `physical_tab`, `physical_lf`,
`physical_cr`, `non_ascii_hex_escape`, `leading_underscore_unicode_escape`,
`overlong_unicode_escape`, `above_unicode_max`.
Its removed malformed-text rows are:
`obsolete_f_prefix`, `hole_member`, `hole_trailing_space`, `hole_leading_space`,
`hole_debug_space`, `hole_debug_comment`, `hole_unknown_format`,
`unmatched_close_brace`, `string_non_ascii_hex_escape`,
`string_leading_underscore_unicode`, `string_surrogate`, `cooked_bare_cr`,
`raw_bare_cr`, `raw_unclosed_hash`, `byte_non_ascii_scalar`, `byte_empty`,
`byte_multiple`, `byte_physical_tab`, `byte_unicode_escape`,
`bytes_unicode_escape`, `bytes_non_ascii`, `raw_bytes_non_ascii`,
`bytes_bare_cr`, `raw_bytes_bare_cr`.
These are deliberately fewer formatter entrypoint probes, not proof that every
removed spelling has an equivalent lower-layer test. Lexer/parser/literal owners
remain unchanged and keep responsibility for detailed language rules.
The removed analyzer character-only scenario similarly relinquishes its exact
escape spelling, while lifecycle and Unicode/CRLF snapshot observations remain.

The byte corpus removed pattern rejection for cooked/raw byte strings; direct
cooked, named raw and nested cooked/raw attribute values; three non-ASCII byte
forms; byte-char/string Unicode escapes; multi/empty byte chars; and bad byte hex.
Ten inputs exactly match Node counterparts; four differ only in attribute-to-item
whitespace. All 14 rejected with identical native error trees after the binding
refresh; none of the removed `:error` entries asserted that exact recovery tree.

These reductions are representative sampling, not a zero-loss or complete-matrix
claim. Deliberately retained audit candidates include the typed-operand manual
oracle (independent checks are not redundant merely because a verifier exists)
and HIR typed/golden companions (different assertions/environments; eliminating
preparation would require broader registration machinery or a cache). No such
machinery was added to make test counts smaller. GC, borrowing, cancellation,
source-free/real-native, compile-fail and actual-host boundaries are unchanged.

#### Final local acceptance

- [x] Complete the three directions across the finite audited candidate set.
- [x] Record actual preparation reductions and every sampled-out dimension.
- [x] Compare retained assertions and independently review Rust/product changes.
- [x] Execute the affected focused selections below and complete skipped targets.
- [x] Pass required structure, formatting, workspace Clippy and diff checks.

This completion ran 154 Rust tests across affected owners, all passing:

| Package | Passed | Selection |
| --- | ---: | --- |
| `kagari-common` | 6 | Four retained capacity states, backing release and module-prefix ownership |
| `kagari-hir` | 5 | Lowering, both typed mutation owners and standard hash/numeric analysis |
| `kagari-syntax` | 11 | Dedicated rejection and declaration owners, remaining conformance rows, both merged typed/golden delimiter cases, inventory and partial recovery |
| `kagari-format` | 22 | Retained rejection samples, literal byte/EOL preservation, idempotence and exact CRLF/empty output |
| `kagari-cli` | 54 | `args::tests::disassembly_` on the `kg` binary target |
| `kagari-analyzer` | 5 | `documents::tests::` on the analyzer binary target |
| `kagari-bytecode` | 2 | Independent wire and in-memory format admission |
| `kagari-compiler` | 9 | ABI/const samples, artifact assertion migration, stronger control flow, Add selection and independent typed operands |
| `kagari-runtime` | 5 | Value classification/payload, metadata records/duplicates and runtime facade |
| `kagari-vm` | 7 | Merged field lifecycle, debugger control, entry/trap recovery, checked scalar native and slot/JIT routes |
| `kagari` | 28 | Two source fixtures per migrated runner, LF/CRLF for both text runners, retained readonly/identity/Eq/provider/interface/declaration owners |

The initial multi-package `--lib` tooling selection executed only formatter;
CLI/analyzer binary selections were subsequently run explicitly. Three ignored
formatter full-inventory entries matched the broad literal filter and remain
CI-only, not passed. Five long SDK fixture names were truncated by harness
registration; shorter unique filters subsequently ran all five intended cases.
No selected local target remains unexecuted. Earlier passing evidence remains
valid for unchanged first-batch owners; it is not an unrun CI claim.

Additional focused validation passed four Python unittest methods (87 negative
subchecks plus positive schema/warmup checks); 27 site Node tests, with the Unix
process-group test skipped on Windows; two TextMate Node tests; five refreshed
native grammar Node tests; and five CLI corpus golden cases. Browser/Worker/UI,
WASI/actual editor hosts, full inventories and the feature/backend/platform matrix
were not run. The WASM runner syntax check passed, not actual WASM acceptance.

Native binding refresh initially failed Python discovery; retry with installed
Python succeeded and rebuilt committed parser/scanner sources without grammar
regeneration. Structure checked 1,817 Rust files with no findings/exceptions;
`cargo fmt --all -- --check`, workspace/all-target Clippy with `-D warnings`,
link/content and diff checks passed. There are no carried build/test/lint errors.

Against `a82d9c4b`, static registrations decrease by 125 Rust cases: 21 fewer
ordinary tests, 56 fewer explicit case rows and 48 additional eliminated
Cartesian repetitions. This excludes function-internal loops and backend routes;
Node/corpus/Python counts are separate. No speed or memory reduction was measured.
TP14's authorized implementation/local acceptance is complete; CI acceptance,
TP10-TP13 layout work and the independent measurement debt are not marked complete.

### Slow-test preparation follow-up

The user reported over-60-second harness warnings for the SDK interpolation
rejection matrix and VM observed-boundary cancellation sweep after `ea4e9d09`.
These were not among the preceding 154 selected tests. Isolated reproduction
passed both: 18.47 s for SDK rejection and 12.98 s for VM cancellation, excluding
build time. These are single diagnostic observations, not a matched benchmark or
a diagnosis of the user's concurrent/CI environment.

For example, rejecting `"{value + 1}"` previously built a fresh default SDK engine
for each of 16 malformed spellings. The SDK now samples that one expression hole,
uses the existing standard baseline with an independent source session, and
requires `EmbeddingError::Diagnostics` containing `KG_TYPE_INVALID_LITERAL`.
The 15 other SDK spellings (member/index/call/qualified/block holes, five whitespace
forms, empty/unmatched braces, and `f`/`fr` prefixes) are no longer repeated here.
Detailed parser rejection/recovery and incomplete-text cases remain at the syntax
owner; this is reduced upper-layer sampling, not unchanged spelling coverage.
The adjacent missing-protocol/type/const cases are outside this focused change.

The cancellation test retains direct and decoded programs and the same `0..80`
point sweep, observed-boundary termination and cancellation assertions. Only
invariants move out of the sweep: one decoded artifact, one function-owned
`NativeInstallation` for the standard library and stateless boundary callback,
and an unbound logging declaration/builder template. Each attempt clones that
unbound builder, binds its own callback, and creates a new runtime/heap,
cancellation token, observer and effect counter, with GC threshold `Some(1)`. For example,
cancelling inside the callback still checks completed effects, root/depth cleanup,
session-sticky cancellation, successful `ready` after session exit, version
retirement and zero live objects. Stateful logging registrations are deliberately
not shared. No production code, process-wide cache or cancellation points change.

- [x] Reproduce the two reported tests and inspect preparation/coverage owners.
- [x] Reduce the SDK matrix and move cancellation invariants outside the sweep.
- [x] Run the two affected tests and selected parser/installation/callback regressions.
- [x] Complete structure, formatting, workspace Clippy and diff checks.
- [x] Record actual diagnostic timings, local results and unrun CI scope.

Validation selects the renamed SDK propagation test, the original VM sweep,
syntax interpolation rejection/incomplete-text owners, runtime shared-installation
isolation, and VM callback depth/session cancellation neighbors. Use the same
workspace dev/test O1 profile, default features/parallelism and cached builds;
each exact test runs in its own process. The reproduction host is Windows,
Intel i9-12900K (16 cores/24 logical processors), Rust 1.99.1
(`58cf8f9c3`, 2026-10-13). Record build and harness execution separately; no
whole-suite speedup or retained-memory claim follows from these local samples.
Full workspace and exhaustive feature/backend/platform execution remain CI-only.

The final exact selections passed: SDK
`interpolation_syntax_errors_reach_the_sdk_compile_boundary` in 1.38 s and VM
`cancellation_at_observed_boundaries_cleans_callback_scopes` in 8.47 s. Their
preceding rebuilds took 23.44 s and 9.35 s respectively and are excluded from
those execution samples. SDK uses default `source,native` features; VM uses its
default features. Relative to the 18.47/12.98 s isolated reproductions, the SDK
also performs less upper-layer work (16 spellings -> one), while the VM retains
its two routes, point bound and all cleanup/recovery assertions. These are single
fresh-process diagnostics with a warm build cache, not statistically established
speedups or a guarantee against 60-second warnings under concurrent CI load.

All seven distinct focused tests passed: the two affected tests, syntax
`interpolation_rejects_expressions_whitespace_keywords_and_old_prefixes` and
`incomplete_interpolation_and_invalid_text_report_diagnostics`, runtime
`shared_registrations_keep_installation_and_failure_state_runtime_local`, and VM
`callback_depth_failure_precedes_effects_and_cleans_native_roots` and
`cancellation_in_callback_keeps_the_effect_and_is_sticky_only_in_its_session`.
Independent review confirmed that the shared installation callbacks are stateless,
the logging template is unbound, and its definition owner stays valid when cloned;
per-attempt captures are never shared. Repeated independent decoder contexts are
no longer sampled, but fresh runtime admission of the decoded artifact remains.
Structure (1,817 Rust files), formatting, workspace/all-target Clippy with
`-D warnings` and diff checks passed. No production code or dependencies changed;
no build/test/lint failures are carried. Full CI and performance acceptance above
remain unverified.

#### CPU utilisation investigation

The subsequent user report concerns low CPU during `cargo test`. No concurrency,
production code or runner configuration was changed for this investigation. The
four ordinary SDK runner files were temporarily instrumented, then restored
byte-for-byte from pre-probe copies, preserving the preceding uncommitted changes.

Environment: the same Windows 11/i9-12900K/Rust 1.99.1 host above, 24 available
logical CPUs, workspace O1 test profile, default package features and build/test
parallelism. `RUST_TEST_THREADS`, `CARGO_BUILD_JOBS`, `RUSTFLAGS` and encoded
Rust flags were unset in the probe environment; no project Cargo config or
repository-wide serial-test override was found. User-global configuration and the
original reported invocation were not established. Builds were cached; the
processes below ran one at a time, not concurrently with another test probe.

A temporary Windows sampler read each test process's kernel+user CPU time with
`GetProcessTimes` at approximately 0.2 s intervals. CPU seconds / wall seconds
estimates used logical cores; dividing by 24 gives normalized machine CPU, not
necessarily Task Manager's frequency-adjusted utilisation. Only the test process
is measured, not Cargo/builds or unrelated processes. It runs the executable from
Cargo's `--no-run --message-format=json` output in the owning package directory.

| Selected workload | Process wall | Process CPU | Average cores | Normalized CPU | Peak sampled cores |
| --- | ---: | ---: | ---: | ---: | ---: |
| Exact VM cancellation sweep | 8.64 s | 8.48 s | 0.98 | 4.1% | 1.07 |
| 21 ordinary SDK cases | 9.66 s | 98.81 s | 10.23 | 42.6% | 20.17 |
| Same 21, with completion timestamps | 9.91 s | 99.33 s | 10.02 | 41.8% | 19.76 |

The SDK selection is `runtime::tests::language::<owner>::ordinary_execution` for
`type_inference` (5), `numeric_operations` (4), `result_option` (7) and
`string_interpolation` (5). Each retains its three routes. Cached Cargo/build
wall time was 0.36-0.39 s and is excluded. All selections passed; these are
single-process diagnostic samples, not an exhaustive run or stable benchmark.

The first SDK trace used about 14-19 cores in its early 1 s buckets, then about
1-2 cores from 6 s onward. Completion timestamps rule out explaining this solely
as one final slow test: in the timestamped run only 10 of 21 cases had completed
by 8.07 s, and the final pair finished only 0.004 s apart.

Temporary phase markers narrowed the low-parallelism interval to cleanup:
engine construction completed around 1.34-1.41 s per case, source compilation by
3.25-4.03 s, and artifact preparation by 4.82-6.22 s. Explicitly dropping the
existing decoded/direct prepared products and engine at function exit took
0.10-2.20 s per case. Runtime teardown between lanes also grew while other cases
were finishing. This instrumented run passed all 21 cases (9.21 s harness time;
9.53 s total process time including captured diagnostic output); its 21.88 s
rebuild is excluded. The markers identify a teardown bottleneck, not which
individual owner, allocator, dependency or Windows synchronization causes it.
Native stack/wait attribution is required before changing locks or allocators.

The ownership review also rules out a misleading explanation: these ordinary
runners create independent engines, installation writers and analysis contexts.
The process-wide SDK baseline mutex only guards source-session creation when
that helper is used; it does not hold compilation/execution under a lock.
Shared-session definition writers do serialize imports/COW, but cannot simply be
blamed for the independent-engine batch measured here. Shared stdlib source-table
snapshot locks are short; their contribution is unmeasured.

Scheduling adds a separate structural limit. Cargo metadata currently lists 44
potential test-enabled targets (26 unit/binary and 18 integration), plus library
doctest owners; this is not a claim that all are nonempty or enabled on every
feature/platform. [Cargo](https://doc.rust-lang.org/cargo/commands/cargo-test.html)
runs ordinary test executables serially, while
[libtest](https://doc.rust-lang.org/rustc/tests/index.html#execution-options)
parallelizes cases inside the current executable. `-j` controls builds, not these
case threads. A single serial sweep can therefore consume one core (about 4.2%
on this host) while other executables have not started. Increasing the thread
limit cannot split that test body or eliminate serialized teardown.

[Nextest's execution model](https://nexte.st/docs/design/how-it-works/) can schedule
across binaries, but runs each test in a new process. Adopting it without matched
measurements would discard the current per-process OnceLock preparation reuse
and repeat cold setup. No runner migration, thread-count tuning, global-cache
change or full-workspace test execution is authorized by these diagnostic results.
The SDK-specific next measurement is teardown stack/wait attribution, followed
by a bounded ownership/allocation or scheduling decision at the responsible owner.
The workspace follow-up below establishes priorities without generalizing that
SDK mechanism to other owners.

#### Workspace scope and non-SDK sampling

The root manifest is a virtual workspace with no `default-members` override.
`cargo metadata --no-deps --format-version 1` confirms that all 26 members are
also default members. Thus bare `cargo test` **at the repository root** selects
the entire workspace; invocation from a member directory can select differently.
The initial SDK/VM samples were not a measurement of that whole command.

No CI run was available for `test/prune-redundant-contract-tests`. For historical
prioritization, inspect the successful [main CI run 42212573892](https://github.com/kagari-lang/kagari/actions/runs/42212573892),
SHA `f5fa12a4f9cbfc3c51b3f335f99cb07b2534d49f`, rust job `127874563006`,
created 2026-11-06. Its `Run tests` step executes
`cargo test --workspace --no-fail-fast`. Download without rerunning CI:

```text
gh api repos/kagari-lang/kagari/actions/runs/42212573892/jobs
gh api repos/kagari-lang/kagari/actions/jobs/127874563006/logs
```

Method: restrict logs to that step, pair each top-level `running N tests` with
its result and executable header, and sum harness elapsed times. Account for
stdout/stderr header interleaving; exclude the SDK's nested child harness
(1 test, 3.45 s) from additive totals. Individual harness log intervals agree
with reported elapsed within 0.008 s. This is elapsed-time attribution, not CPU
profiling. Later feature/inventory steps are excluded.

| Historical executable | Tests, including ignored | Harness seconds | Share of executable elapsed |
| --- | ---: | ---: | ---: |
| SDK `kagari` | 867 | 1230.32 | 48.10% |
| `kagari_hir` | 637 | 415.84 | 16.26% |
| `kagari_vm` | 371 | 403.43 | 15.77% |
| `kagari_compiler` | 261 | 252.54 | 9.87% |
| `kagari_stdlib` | 64 | 92.73 | 3.63% |
| Other 39 executables | 1686 | 162.80 | 6.37% |

The top four account for 90.01% of executable elapsed. All 44 total 2557.66 s
(3833 passed, 53 ignored); doctests add 65 passes and 2.23 s harness time,
or 8.25 s including their preparation/compilation and transitions. The test step
lasted 3209 s, including a separately reported 641 s build. Runtime's 604 tests
took only 1.82 s: test count alone is not a useful optimization ranking.
Two over-60-second warnings occurred in SDK offline-nominal effect ordering and
stdlib documentation analysis; both passed. No warning is not evidence of low
aggregate cost.

Limits: this predates pruning and the current working tree. It used Ubuntu
26.04.1, Rust 1.99.1, optimized test profile, restored Rust cache and
`CARGO_INCREMENTAL=0`; CPU model/count and effective harness thread count were
not logged. Neither these shares nor its 53-minute step duration describe the
current Windows run, and historical success is not current-branch CI acceptance.

Local follow-up selects HIR/compiler/VM paths from those high-cost owners, plus
small GC and CLI controls. Same Windows environment and default concurrency as
above; groups run sequentially, not concurrently with each other. Each process
has cold process-local initialization even when build artifacts are cached.

| Current selection | Passed | Process wall | Process CPU | Average cores | Peak sampled cores |
| --- | ---: | ---: | ---: | ---: | ---: |
| HIR body queries | 10 | 3.89 s | 12.09 s | 3.11 | 9.75 |
| Compiler foundation | 3 | 1.52 s | 3.89 s | 2.56 | 2.97 |
| VM optimized prepared execution | 8 | 4.14 s | 28.17 s | 6.81 | 8.12 |
| GC incremental target | 8 | 0.175 s | 0.016 s | not meaningful at this duration | no interval |
| CLI lint process contracts | 2 | 5.42 s | 0.094 s, parent only | not process-tree CPU | not comparable |

Build/Cargo wall times, excluded above: HIR 0.35 s, compiler 0.28 s, VM 0.28 s,
GC 3.13 s (dependency rebuild), CLI 6.60 s (rebuild). All 31 cases passed.
Exact scope, reproducible through Cargo (sampling used the resulting binaries):

```text
cargo test -p kagari-hir --lib analysis::body_queries::tests::
cargo test -p kagari-compiler --lib tests::language_foundation::compile_success::
cargo test -p kagari-vm --lib tests::prepared_execution::optimized_execution_preserves_results_traps_and_source_origins::
cargo test -p kagari-gc --test incremental
cargo test -p kagari-cli --test toolchain lint_
```

Distinct observations and next decisions:

- HIR completes nine cases by 1.64 s; only
  `assignment_member_receivers_survive_errors_and_snapshot_revisions` remains
  until 3.88 s, using about one core. Unlike the SDK trace, this **is** a single
  long-case tail. Its three source variants each build fresh analysis state and
  run ordered cache/edit/snapshot checks. Review invariant preparation there;
  preserve valid, unknown-member and incomplete-member recovery contracts and
  old-snapshot assertions rather than dropping variants just to reduce time.
- Compiler's three cases reach approximately three cores; VM's eight reach
  approximately eight, with no sustained one-core tail like the SDK batch.
  Small selected case counts cap available concurrency; their low percentage of
  24 CPUs is not evidence of a disabled harness or whole-crate serialization.
- GC's harness reports 0.00 s; process startup/exit and CPU accounting granularity
  dominate this sample. It does not justify a GC scheduling optimization.
- CLI's two cases launch nine `kg` children. `Project::run` waits for each child
  while draining pipes. Parent-only `GetProcessTimes` omits their work; measure
  cumulative process-tree CPU, including exited children, before attributing
  CLI utilization. The reported parent value is not machine utilization.

Prioritize SDK teardown attribution and high-cost HIR/VM/compiler preparation,
not thread-count increases or deletion of cheap tests. Cross-executable serial
scheduling remains a separate known limit. No runner, runtime, lock or test
behavior changed in this follow-up, and no full local workspace run was made.
Current Windows whole-suite shares, native wait attribution and current-branch
full CI remain unmeasured. Content/link review and `git diff --check` validate
this documentation-only update; prior implementation checks are not rerun.

#### Native stack attribution and immutable MIR retention

The user subsequently requested investigation of production-language costs, not
just test scheduling. Three process-scoped CDB 10.0.26100.7705 snapshots of the
unchanged 21-case SDK workload provide native evidence. Each fresh test process
was attached noninvasively after a requested delay (2, 6 or 7.5 s), with local
PDBs and no system-wide tracing. Attachment/symbol loading perturbs timing;
these runs are stack evidence only, not performance comparisons. All passed.
Reproduction: build the filtered SDK library executable as above, launch it from
`crates/kagari`, then use the installed x64 CDB with
`-pv -p <test-pid> -y <absolute-target/debug/deps> -c "~* kb 35; qd"`.
Offsets in Windows internal routines without private symbols are not exact
function attribution; the exported wait/critical-section/heap frames and Rust
ownership chains are the evidence used here.

| Requested snapshot delay | Active test threads | Free-path heap waits | Allocation-path heap waits, excluding free paths |
| --- | ---: | ---: | ---: |
| 2 s | 21 | 0 | 0 |
| 6 s | 15 | 11 | 3 |
| 7.5 s | 13 | 11 | 1 |

Late stacks contain `RtlFreeHeap -> RtlEnterCriticalSection ->
ZwWaitForAlertByThreadId`, reached through several independent production owners:
installation `DeclarationCatalog`, HIR `AnalysisDatabase`/`FileAnalysis`, lowered
native records retaining definition `TableData`, core runtime metadata and
`VerifiedMirProgram`. Allocation waits also include `VerifiedMirModule::clone`
under SDK publication retention. Thus heap synchronization during allocation and
release is directly observed; it is no longer merely an allocator hypothesis.
These snapshots do not measure its percentage of total time or establish one
owner as the dominant allocation source. They do not implicate Kagari's analysis
writer locks or JIT page unmapping as the explanation for these sampled waits.

A bounded production fix changes SDK `ProgramState.mir` and
`RetainedPreparation.mir` to `Option<Arc<VerifiedMirProgram>>`. Previously,
`Runtime::load_program -> retained_inputs` deep-cloned modules/functions/analyses,
even for interpreter execution; retrieving current publication inputs copied them
again. In the 21-case workload, three loads per case implied 63 full MIR clones.
Now each verified source/artifact product is wrapped once and shared by exact
publication token. Direct and decoded artifacts remain independently verified;
no validation, cancellation check, native cache or code lease is bypassed/shared.
This follows [Rust Arc ownership](https://doc.rust-lang.org/std/sync/struct.Arc.html)
(Rust 1.99.1): cloning shares immutable data, with destruction by the last owner.
Arc rather than Rc preserves movable runtime attachments; PreparedProgram itself
remains Rc/RefCell-owned and non-Send. Validity and generation guarantees remain;
the last destruction time/thread can change. No public API or artifact format
changes, new dependency, global cache or allocator replacement is introduced.

Matched comparison uses a preserved pre-fix executable and rebuilt current
executable, alternating before/after three times, default features/test threads,
same Windows/i9-12900K/24-logical-CPU/Rust 1.99.1/O1 profile as above. No debugger,
other probe or build overlaps the measured processes. Build artifacts and fixture
files are warm; each process starts its own initialization. External machine load
is not controlled. The current SDK rebuild took 30.93 s, excluded from execution.
All 21 cases passed in every run.

| Metric | Before, three runs | After, three runs | Median before -> after |
| --- | --- | --- | --- |
| Process wall seconds | 9.246 / 9.457 / 9.279 | 9.388 / 8.994 / 8.964 | 9.279 -> 8.994 |
| Process CPU seconds | 98.125 / 98.906 / 97.797 | 97.422 / 97.984 / 96.328 | 98.125 -> 97.422 |

The observed median wall reduction is 3.1%, CPU 0.7%; one pair is slower, and
three samples do not establish a stable or whole-suite speedup. A separate
post-change 6 s stack snapshot still shows 11 free-path heap waits among 16 active
tests. The redundant MIR allocation is removed, but the wider bottleneck remains.
Do not respond to this small gain with speculative additional local optimizations.

Next bounded diagnosis: measure `compiler::source::program::candidate::expand ->
update_requests::proof_catalog`. The 2 s snapshot reaches this path after
`verify_program` has already prepared proof records and validated a full-module
catalog for the same immutable candidate. Planning collects those declarations
and prepares/validates a catalog again. Other sampled stacks traverse
`collect_module_abi` and type/proof normalization. Quantify preparation versus
actual demand queries, candidate rounds, exact visibility sets and standard/user
record counts before redesigning this boundary. Not every module is the entire
stdlib, and not every validation is redundant: different visibility, new
candidates, raw artifacts, resource/cancellation limits and retained demands
still require their current guarantees. Late analysis-table destruction is a
separate retained-allocation source, not proof that temporary proof catalogs
caused those waits. No proof API redesign is included in this checkpoint.

Correctness scope reuses existing tests, without adding private-Arc assertions or
more test cases: four SDK contracts cover actual-native source/decoded lifetime,
reload/GC, source-update attachments and product destruction; one artifact-native
contract runs with default features to regenerate its disposable fixture and
again with `--no-default-features --features native` for the real source-free
boundary. All six executions passed, in addition to the 21-case comparison.
Commands:

```text
cargo test -p kagari --lib -- source_and_encoded_mir_use_real_native_code_after_backend_and_program_drop real_native_entries_keep_their_values_after_reload_and_collect_at_safepoints source_updates_retain_requested_native_input_without_retaining_prepared_caches caches_products_across_runtimes_and_retains_them_after_backend_drop
cargo test -p kagari --test artifact_features real_cranelift_compiles_portable_artifact_without_source
cargo test -p kagari --no-default-features --features native --test artifact_features real_cranelift_compiles_portable_artifact_without_source
```

Structural review: two small production files retain explicit feature-scoped
imports and private ownership, with no re-exports, new modules or size exceptions.
Formatting, structure and workspace Clippy (`--workspace --all-targets -- -D warnings`)
passed. Full-workspace tests and exhaustive feature/platform validation remain
CI-only and unrun for this working tree.

## Directory readability follow-up (TP10-TP13)

Status, 2026-10-28: documentation only. The user selected subsystem-local `tests/`
directories and explicitly deferred implementation. TP10-TP13 are queued, not
started. They refine physical layout without reopening TP05-TP09's completed
source ownership or the later fixture/contract organization. This section owns
this follow-up's scope and ledger; it does not create another test-cost project.

### Problem and intended result

Production modules and several independently named `*_tests.rs` files currently
share directories. Some owners also combine a `tests/` directory, `tests.rs`,
other test subdirectories and inline test modules. A reader expanding the source
tree must distinguish implementation from tests file by file.

For example, `kagari-runtime/src/native/assembly/` contains implementation files
such as `authoring.rs`, `draft.rs` and `seal.rs` alongside `default_tests.rs`,
`default_tests/`, `proof_tests.rs`, `transfer_tests.rs`, `tests.rs` and `tests/`.
The proposed layout gathers that owner's test groups under `assembly/tests/`,
so folding that directory leaves implementation visible. Child-module private
checks remain under their actual owner rather than moving to a sibling merely
for visual uniformity.

The objective is navigation and clear ownership, not fewer tests, faster builds
or a measured execution speedup. Production APIs, semantics, dependency boundaries,
harnesses and test setup policies remain unchanged.

### Placement rules

1. Prefer one `#[cfg(test)] mod tests;` entry for grouped tests belonging to the
   same module, with ordinary child declarations in `tests.rs`. Keep relevant
   feature/platform gates. Within this directory, use meaningful names such as
   `arena.rs` or `defaults/` rather than repeating `_tests` on every leaf.
2. Keep tests at the smallest correct semantic owner. Rust private items are
   accessible from their defining module and descendants, not arbitrary siblings
   ([Rust privacy rules](https://doc.rust-lang.org/reference/visibility-and-privacy.html)).
   For example, a private `analysis::body` check can stay in
   `analysis/body/tests.rs`, declared by `body.rs`; it must not
   require widening visibility to move under `analysis::tests`.
3. Preserve existing clear `src/tests/` and subsystem `tests/` trees. A small,
   single-entry `tests.rs` is acceptable; converting every such file to a directory
   is not a goal. Extract inline test modules only where separating a substantial
   test section improves implementation reading, preserving the logical parent.
4. Do not gather all tests at the crate root, create a shared test crate or turn
   unit tests into integration targets. Existing external/process/feature-isolation
   tests retain their distinct harness boundaries. Crate-root `tests/` fixture and
   support directories are not automatically integration targets.
5. Use ordinary Rust modules. Do not introduce handwritten `include!`, layout-only
   `#[path]`, forwarding modules, duplicate module declarations, new re-exports or
   wider production visibility to make moves compile. Keep helpers with their
   actual test consumers; do not duplicate stateful setup or preparation caches.

Illustrative HIR destination, not an implemented tree:

```text
src/
  analysis.rs
  analysis/
    body.rs
    declarations.rs
    tests.rs
    tests/
      arena.rs
      callable.rs
      generic_types.rs
      generic_types/
        ...
    body/
      tests.rs
      tests/
        ...
```

### Inspected scope and priority

The 2026-10-28 survey inspected paths and module declarations, not every private
access or fixture dependency. Recheck the actual owner before moving each group;
this is a bounded migration inventory, not a claim that every listed file must move.
Paths below are relative to `crates/`.

| Priority / owner | Observed layout | Intended treatment |
| --- | --- | --- |
| First: `kagari-hir` | Numerous `analysis/*_tests.rs`; similar groups under `imports/` and `native/api/`; existing local `tests.rs` and inline modules | Pilot `analysis/tests/`, then consolidate other mixed groups without crossing private ownership |
| Second: `kagari-runtime` | `native/assembly/` mixes several test files and directories; `assembly/seal/`, `gc/` and `scheduler/` also contain named test files | Consolidate at each actual owner; preserve existing `native/tests/` and `host/tests/` |
| Third: `kagari-contract` | `types/tests/` coexists with `inheritance_tests.rs` and `native_tests.rs`; additional groups under `types/proofs/`, `types/named/verify/` and `callable/wire/evidence/` | Use one grouped test entry per owner; do not flatten private child checks |
| Third: `kagari-bytecode` | `artifact/canonical_tests.rs`, `identity_tests.rs` and `string_iter_tests.rs` are beside implementation; `verifier/async_tests.rs` is separate | Consolidate artifact/verifier test groups while retaining local verifier ownership |
| Local follow-up: `kagari-types` | `ty/identity/tests.rs` plus `metadata_tests.rs`; `language/identity_tests.rs`, `declaration/verify/scoped_tests.rs` | Review and consolidate only mixed groups |
| Local follow-up: `kagari-compiler` | Existing `src/tests/`; `source/lower/abi/tests.rs` plus `host_tests.rs` | Preserve the main test tree and organize the ABI-local group |
| Local follow-up: `kagari-package` | Existing `src/tests/`; `modules/input_tests.rs` and local graph/manifest/workspace test files | Preserve semantic owners; avoid relocating clear single-entry modules unnecessarily |
| Lower priority: inline sections | Examples in `kagari-source`, `kagari-common`, CLI, web, format, analyzer, MIR, stdlib, syntax, VM and SDK implementation files | Inspect size and reading impact; extract only justified sections at their current module boundary |
| Already clear | SDK engine/program/runtime test trees; runtime host/native trees; analyzer/syntax/VM `src/tests/`; single test files in lint/format/Cranelift | Retain unless a concrete mixed group is found within the selected scope |

No conventional test declarations were found in the surveyed ABI, codegen and
derive crates. Do not create placeholder test directories for them. This survey
is not a test-coverage audit or authorization to add missing tests.

### Migration safety

Before each batch, record old/new physical paths and qualified test names, helper
consumers, feature/platform gates, ignored/panic attributes and fixture references.
Inspect `super::` imports, restricted visibility, macro-generated registrations,
`include_str!`/`include_bytes!`, `#[path]`, `file!()`/`module_path!()` and filesystem
paths. Moving a module can change names and relative paths even if its body stays
identical. Update current documentation, CI/script filters and any path-specific
structure exceptions in the same batch; label historical commands rather than
rewriting historical evidence as a current run.

Keep input bytes, golden expectations, assertions, source names, route matrices,
GC settings, cancellation behavior and setup isolation unchanged. Do not delete
or deduplicate cases, rewrite fixtures, replace runners, add smoke tests or change
production APIs as part of this layout work. Any necessary behavioral repair needs
its own diagnosis and explicit scope, not a hidden exception to a mechanical move.

### Queued checkpoints and acceptance

- [ ] **TP10: HIR pilot and remaining mixed HIR groups.** Freeze each group's
  mapping/private-access constraints, consolidate `analysis` first, then review
  `imports`, `native/api` and the remaining observed HIR groups. Record justified
  retained local layouts rather than forcing everything under `analysis/tests/`.
- [ ] **TP11: runtime groups.** Consolidate assembly and its privately owned child
  tests, then the observed GC/scheduler groups. Keep native/host trees and shared
  fixture consumers intact; do not reorganize production ownership.
- [ ] **TP12: contract, bytecode and smaller mixed groups.** Address the inventory
  above in coherent owner-sized batches, preserving existing test trees and
  external targets. Do not combine all crates into one unreviewable move.
- [ ] **TP13: scoped readability review and handoff.** Review the listed inline
  candidates, recording which substantial sections are extracted and why others
  stay. Reconcile current links/filters, document the lasting layout convention
  in `testing.md`, and report local evidence separately from pending CI. Do not
  expand this checkpoint into arbitrary production refactoring or every future
  inline test in the repository.

Each batch must compile in its affected scope; there is no planned broken-build
phase. Compare before/after harness discovery for selected moved groups, including
parameterized and ignored cases, using an explicit old/new qualified-name mapping.
Reject empty filters, lost cases, duplicate registration or changed attributes.
Discovery establishes preservation, not successful execution.

Run a small selected contract set for moved behavior plus directly affected
adjacent contracts. For HIR select representative analysis/identity/diagnostic
cases; for runtime select registration and ownership/cleanup cases; for contract
and bytecode select admission/codec rejection; for package select input/filesystem
contracts. Record exact nonempty filters and selection rationale at implementation
time. Compile affected harnesses as needed, but do not execute every package suite
as a disguised full-workspace sweep. Use default target/profiles/parallelism and
`--no-fail-fast` for multiple selected test targets.

Implementation checkpoints also require:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

CI owns complete workspace and feature/backend/platform execution. Preserve
existing case coverage even when only representative local cases are run. Verify
fixture discovery/path changes in their affected scope; a successful build alone
is insufficient. This documentation-only checkpoint needs content/link/diff checks,
not Rust builds or tests. No benchmark is required for a readability-only move;
the earlier preparation timing and retained-memory debt stays independent.

### Follow-up ledger

2026-10-28: recorded the agreed subsystem-local directory direction and the
cross-crate survey. TP10-TP13 remain unstarted; no Rust files, test registrations,
fixtures, APIs or harnesses changed. There are no migration build results or
carried migration errors yet. Resume by inspecting current Git status, affected
diffs and test owners, not by treating this survey as a verified move manifest.
On completion, keep lasting rules in `testing.md` and unresolved acceptance in the
roadmap. Retire this plan only after its independent preparation/measurement work
is also resolved or transferred to its durable owner.

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
test directory. Library roots and module entry files contain declarations/orchestration,
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
a successful build or name listing is not a passed test. Run workspace Clippy for
implementation acceptance, structure, formatting and `git diff --check`;
scoped Clippy remains optional during iteration.

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
