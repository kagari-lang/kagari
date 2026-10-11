# Performance Measurements

This page retains measurements needed for the open test-cost and interpreter
performance work. Each result applies to its recorded revision and environment,
not automatically to current HEAD. Superseded checkpoint reports live in Git
history; compiler, startup and execution measurements remain separate.

Keep workloads, hashes, environment and timing scope together. Allocation requests
are not RSS; compilation/setup and execution are separate. Small differences are
not significance claims, and local correctness does not imply GitHub CI or Lua parity.

## Focused preparation recheck, 2026-10-11

After the user reported lower CPU load, nine selected scenarios ran twice: 18
successful executions, with no workspace sweep. Code is 294dce32 plus the working
TP01-TP03 implementation. The machine/toolchain are Windows x86_64 MSVC,
i9-12900K (16 cores / 24 logical CPUs), 63.7 GiB RAM, rustc 1.99.0 (b940084d7,
LLVM 23.1.1) and Cargo 1.99.0. Default `source,native` features, workspace test
profile (opt-level=1, debug info), target directory and build/test concurrency
were retained. Each group ran in a fresh process; groups/probes ran sequentially.

The selected incremental build took 23.28 s (Cargo reported 23.15 s), rebuilding
affected libraries and test binaries against existing artifacts. This is separate
from the following harness times and is not a cold-build measurement.

| Selected group (3 tests each) | Run 1 | Run 2 | Peak working set across runs |
| --- | ---: | ---: | ---: |
| Default methods | 23.09 s | 22.97 s | 410.9-412.5 MiB |
| Syntax examples | 17.14 s | 17.13 s | 413.8-414.3 MiB |
| Language contracts | 23.08 s | 23.53 s | 402.0-403.2 MiB |

Selections, passed together as libtest filters with `--exact`:

- `default_methods`: `default_body_calls_the_implementation_override_through_static_and_dynamic_dispatch`,
  `explicit_override_takes_precedence_over_the_default_body`,
  `generic_implementations_and_default_local_binders_are_specialized`.
- `syntax_examples`: `array_operations`, `collection_pipelines`, `callables`.
- Embed library: `runtime::language_contract::evaluation::scalar`,
  `runtime::language_contract::evaluation::host_reject`,
  `runtime::language_contract::types_and_dispatch::publication_pins_old_dependencies_and_rejects_stale_candidate`.

For example, build the three targets with `cargo test -p kagari-embed --lib
--test default_methods --test syntax_examples --no-run`. Reproduce the syntax
selection with `cargo test -p kagari-embed --test syntax_examples -- array_operations
collection_pipelines callables --exact`. The other targets accept their listed
filters in the same way. Report Cargo compilation separately from harness time.

Windows GetSystemTimes/GetProcessTimes indicated approximately 14.7-17.8% of
total logical CPU capacity was still used outside each test process. Thus this
is a reduced-load sample, not a dedicated idle-machine benchmark. Two similar
runs establish useful current timings, not statistical confidence or a whole-suite
speedup. The earlier interrupted 866.93 s aggregate is not a comparison baseline.

A retained pre-change probe linked against 294dce32 and a freshly linked current
probe were also run sequentially in this session. Both use `fn main() -> i32 { 42 }`,
three fresh engines followed by three compilations in one retained engine. Each
case compiles, encodes/decodes, prepares, creates a runtime, loads and executes,
with default options. The current probe additionally creates three fresh sessions
from the retained engine. The drivers use rustc's default optimization while
linking the workspace test-profile libraries. Values below are medians of the
three fresh-engine cases, except the explicitly separate session row.

| Stage | Before | Current |
| --- | ---: | ---: |
| Construct full engine | 692.255 ms | 720.409 ms |
| Compile source to artifact | 4.470 s | 4.580 s |
| Prepare decoded program | 2.726 s | 2.822 s |
| Create runtime | 222.275 ms | 0.016 ms |
| Load prepared program | 9.370 ms | 9.576 ms |
| Execute | 0.020 ms | 0.020 ms |
| New source session from prepared engine (current-only, 3 samples) | N/A | 0.852 ms |

The runtime-creation reduction is consistent with reusing validated installation.
Full engine construction still performs preparation; creating a session amortizes
that work but does not clone callback captures. Compilation and program preparation
remain the main costs. Later current samples varied to 5.775 s for compilation and
3.431 s for preparation; these sequential, background-loaded probes do not isolate
small regressions or prove those stages improved. The first full engine in each
process also pays cold standard registration initialization (1.94 s before,
1.74 s current), which is amortized by subsequent samples.

Probe executable SHA-256: baseline
`810e2c58711dfa8992c0c760b5236c552ef932a7fd49052c694fd03029b44da9`;
current `7cdf8065a27727152b0cbcfc958de10fcb740943d90067389fb6f758424cb536`.
Raw logs/binaries remain disposable local measurements. Current probe peak memory
cannot be compared directly with the baseline because it runs additional sessions.
Matched suite timing and retained-identity growth remain open in the
[test preparation plan](test-preparation-plan.md); these samples do not establish
a memory reduction.

## Workspace test-cost audit, 2026-10-11

The user explicitly requested one complete workspace audit after the targeted
sample below. This is a one-off diagnostic, not a change to the CI-only policy
for routine full-suite validation. No tests, assertions, production code or Cargo
profiles were changed, and no failed/final sweep was repeated.

Command: `cargo test --workspace --no-fail-fast --timings --color never`.
Source baseline: `35de1818`; working changes were documentation and CI policy.
Environment matches the sample below: Windows x86_64 MSVC, i9-12900K (16 cores /
24 logical CPUs), 63.7 GiB RAM, rustc 1.99.0 (b940084d7, LLVM 23.1.1), Cargo 1.99.0.
Workspace test profile uses opt-level=1 and debug info, default target directory,
default build/test parallelism and existing incremental artifacts. This workspace
selection rebuilt several targets; it is neither a cold build nor an unchanged
warm rerun. No other agent build/test/probe ran concurrently. The earlier sample
used serial tests; its wall times must not be mixed into this ranking.

| Measurement | Result |
| --- | --- |
| Complete command wall time | 2,210.66 s (36 min 50.66 s) |
| Start through Cargo's build-finished message | 179.50 s (2 min 59.50 s) |
| Remaining wall time | 2,031.17 s (33 min 51.17 s) |
| Sum of harness-reported test execution times | 2,009.15 s |
| Executed scope | 18 workspace members, 97 test executables and 16 doctest groups |
| Results | 1,952 passed, 0 failed, 2 ignored; Cargo exit 0 |

The gap between remaining wall time and harness totals includes process startup
and rustdoc preparation outside reported execution intervals. Cargo executes test
targets sequentially while each harness uses its default parallelism. Timings
come from one run; they identify expensive targets, not statistically established
performance changes. This covers the default workspace selection, not all feature
combinations, ignored manual measurements or the full CI matrix.

The audit covered all 113 targets; the slowest targets and package totals are
recorded below. Target wall intervals were measured from each Cargo announcement
to the next announcement (or command exit); doctest preparation can make them
much larger than harness time. Raw measurements are not part of the repository.

| Slowest targets | Harness time | Passed |
| --- | --- | --- |
| kagari-embed / lib: language_contract_routes_preserve_values_diagnostics_and_effects | 366.66 s | 1 |
| kagari-hir / lib | 204.36 s | 429, plus 1 ignored |
| kagari-embed / syntax_examples | 171.92 s | 9 |
| kagari-vm / native_boundary | 168.71 s | 160 |
| kagari-vm / lib | 94.47 s | 118 |
| kagari-compiler / lib | 86.83 s | 174 |
| kagari-embed / native_provider_reset | 56.30 s | 18 |
| kagari-stdlib / documentation | 41.24 s | 1 |
| kagari-embed / offline_nominal | 40.93 s | 12 |
| kagari-embed / source_modules | 38.62 s | 30 |
| kagari-lua-benchmark / binary unit tests | 37.53 s | 1 |
| kagari-embed / default_methods | 35.82 s | 32 |
| kagari-embed / standard_traits | 35.76 s | 20 |
| kagari-vm / prepared_execution | 33.45 s | 7 |
| kagari-embed / instantiation | 31.21 s | 25 |

The first six targets account for 54.4% of summed harness time. By package,
kagari-embed accounts for 1,270.15 s (63.2%), kagari-vm 320.15 s (15.9%),
kagari-hir 206.62 s (10.3%) and kagari-compiler 92.56 s (4.6%). In contrast,
runtime's 156 library tests take 1.04 s, syntax's 81 library tests 0.02 s and
bytecode's 32 library tests 0.12 s. Test counts alone do not explain elapsed time.

### Concrete follow-up candidates

1. [SDK language contract](../crates/kagari-embed/src/runtime/language_contract.rs):
   one test contains a large serial collection of scenarios (146 Case::new source
   sites, not a claimed executed-case count). Successful scenarios traverse Source,
   Artifact, Jit and ArtifactJit routes. It already shares compilation/prepared
   inputs across routes; do not propose removing that duplication as if it still
   existed. Review distinct route coverage, prepared declaration reuse and
   individually filterable cases while retaining fresh mutable runtime state.
2. [Syntax examples](../crates/kagari-embed/tests/syntax_examples.rs): 45 examples
   inside one test each create an engine and compile a program, then execute
   direct/serialized artifacts. Default test parallelism cannot parallelize this
   inner loop. Preserve example validity and artifact coverage while reviewing
   repeated immutable setup and selective execution.
3. [Old ABI rejection](../crates/kagari-compiler/src/tests/bytecode/artifacts.rs):
   rejects_previous_runtime_abis_even_when_loader_requests_them loops over 28 old
   versions and recompiles the same `fn main() -> i32 { 1 }` in each iteration.
   The full-run log reports this individual case exceeding 60 s. Prepare the
   invariant program once when fixing this path; independently preserve each
   metadata mutation/rejection assertion. No exact standalone case time was
   measured during this parallel run.
4. [Provider cancellation](../crates/kagari-embed/tests/native_provider_reset/main.rs):
   cancellation_boundaries_unwind_nested_provider_callbacks constructs an engine
   and runtime for each cancellation point, stopping after normal completion or
   the 200-point bound. The program is already prepared outside the loop. Audit
   engine/declaration reuse independently of necessary per-attempt runtime state.
5. [Stdlib documentation](../crates/kagari-stdlib/tests/documentation.rs): one test
   deduplicates documentation examples and analyzes them sequentially. Review
   reuse of checked provider inputs without dropping documentation coverage.

The HIR and VM aggregate targets also require targeted attribution before changing
their tests. Libtest reported some cases running over 60 s, including HIR completion
and generic-recovery matrices, import graph cancellation, VM native cancellation
and retained selected calls. These are wall-time warnings under concurrent tests,
not a precise per-case CPU ranking. Do not remove boundary coverage merely because
its name appears in a slow target. The benchmark crate's 37.53 s is an ordinary
test over empty/single-element workload inputs, not a `cargo bench` run.

Cargo build timings separately identify the HIR library test build at 178.80 s,
compiler library tests at 127.50 s, bytecode library tests at 126.72 s, VM library
tests at 101.40 s and runtime library tests at 78.49 s. These build units overlap
and their times cannot be summed into wall time. Bytecode's expensive test build
and 0.12 s execution particularly illustrate why build/execution need separation.

Measurement method: timestamp Cargo's merged output for the command above,
separate the build-finished boundary, and record each harness summary. Cargo's
--timings flag generates build timing data on the machine running the command.
The temporary wrapper only captured output and timestamps, preserving default
parallelism. Its source and raw logs are not distributed; the tables preserve
the observed results, not a portable raw-data archive. No complete individual-test
timing is claimed. Use targeted follow-up measurements for proposed changes;
routine full sweeps remain with CI.

## Targeted test-cost audit, 2026-10-11

This is a diagnostic sample, not a workspace timing or a global ranking. No full
suite was run. Baseline: `35de1818`, with documentation/CI-policy edits only.
Machine: Windows x86_64 MSVC, Intel Core i9-12900K (16 cores / 24 logical CPUs),
63.7 GiB RAM. Toolchain: rustc 1.99.0 (b940084d7, LLVM 23.1.1). Existing default
target cache; workspace test profile inherits dev opt-level=1 with debug info;
default package features and Cargo build parallelism. No environment overrides
for build jobs, target directory, test threads, Rust flags or profile optimization
were present. No other agent build/test/benchmark ran concurrently with sampling.

Five integration targets were selected from source inspection: example batches,
collection execution matrices and allocation loops. Build them separately from
execution to distinguish Rust build time from work performed inside the tests:

```text
cargo test -p kagari-embed --test syntax_examples --test collection_interfaces --no-run --message-format=json
cargo test -p kagari-vm --test interface_allocations --test library_collections --test native_allocations --no-run --message-format=json
```

The two incremental build commands took 7.78 s and 9.28 s wall time, respectively.
These are warm-cache observations, not cold-build estimates. Each emitted test
executable was then run once, from its crate directory, with `--test-threads=1`.
Suite time includes process startup/exit; individual times are approximate deltas
between completed libtest output lines, with startup included in the first case.
Serial execution attributes costs but does not predict normal parallel test wall
time. Do not sum these results into an estimate for the whole repository.

| Target | Passing cases | Serial execution wall time |
| --- | --- | --- |
| kagari-embed / syntax_examples | 9 | 159.96 s |
| kagari-vm / library_collections | 18 | 43.95 s |
| kagari-embed / collection_interfaces | 5 | 25.13 s |
| kagari-vm / interface_allocations | 3 | 7.46 s |
| kagari-vm / native_allocations | 2 | 3.88 s |

All 37 selected cases passed. Reproduce individual hotspots with package/target
and exact test-name filters; do not turn this diagnostic inventory into a routine
acceptance checklist. Raw logs and the temporary measurement helper are not
distributed; the method and measured results are recorded here.

The largest measured case was
`standalone_language_examples_execute_from_source_and_artifact` at 141.36 s,
approximately 88% of its target's serial time. The
[example loop](../crates/kagari-embed/tests/syntax_examples.rs) runs 45 sources
inside one test, constructing a fresh engine for each and executing both direct
and serialized artifacts. This inner loop stays serial even when libtest runs
different tests concurrently. Other measured examples:

- `list_failures::active_iteration_rejects_list_edits_and_releases_the_guard_on_failure`:
  6.76 s in library_collections.
- `hash_collection_interfaces_use_custom_key_protocols`: 6.73 s.
- `user_containers_implement_the_same_storage_independent_contracts`: 6.04 s.
- `callback_alias_writes_and_nested_edits_are_rejected_without_changing_slots`:
  5.52 s in library_collections.
- `inherited_closed_dispatch_does_not_allocate_parent_wrappers_or_copy_unused_methods`:
  4.16 s in interface_allocations.
- `warmed_scalar_and_contiguous_i32_boundaries_add_no_allocations`: 2.40 s,
  including its 100,000 scalar calls and three 1,000-call bulk cases. Large loop
  counts alone did not identify the largest elapsed-time costs in this sample.

A separate disposable Rust probe linked the exact embed/source rlibs from the
first build, using rustc opt-level=1 and the existing dependency directory. Probe
build/link time was 2.41 s, excluded from the following measurements. Its input
was `fn main() -> i32 { 42 }`; each operation was timed with Instant. Three fresh
engines and then three uniquely named sources on one reused engine were measured
sequentially, without forced GC or JIT. Fresh-engine ranges:

| Operation | Wall-time range across three samples |
| --- | --- |
| KagariEngine::default | 0.700-1.401 s |
| compile_to_artifact | 1.199-1.437 s |
| artifact serialize + decode | 0.029-0.040 s |
| PreparedProgram::from_artifact | 0.402-0.511 s |
| engine.runtime | 0.230-0.259 s |
| runtime.load_program | 0.005-0.007 s |
| runtime.execute | 0.016-0.106 ms |

The temporary probe source and raw samples are not distributed. Its procedure
calls the listed APIs in order, drops the returned execution result,
then drops loaded module/runtime/program and engine with separate timing. This
is a scale diagnostic, not a precise interpreter throughput benchmark. Reusing
the engine removes another engine construction, but compile_to_artifact still
took 1.416-1.473 s on the three reused-engine inputs: engine reuse alone does not
remove the remaining compilation/preparation cost.

Source inspection explains repeated work, but does not yet attribute the time
inside those stages. [Engine declaration preparation](../crates/kagari-embed/src/engine/declarations.rs)
renders/prepares/checks installed declarations; each fresh engine enters that
path. [Collection interface tests](../crates/kagari-embed/tests/collection_interfaces.rs)
exercise direct, serialized and JIT-enabled routes with GC threshold 1. Similar
helpers exist in default_methods, type_inference and trait_inheritance; these
other targets were not timed and are candidates, not measured hotspots.

The next optimization review should separate ordinary semantic coverage from
the smaller set proving serialization/backend/GC boundaries, investigate repeated
immutable declaration and artifact preparation, and review the 45-case example
batch's isolation requirements. Preserve distinct source-free/backend/ownership
coverage and avoid sharing mutable engines globally or skipping validation merely
to shorten tests. No tests, runtime code, assertions or profiles were changed by
this audit. Complete test acceptance remains with CI.

## Execution architecture evaluation (HP06), 2026-10-10

HP00–HP06 implementation and final local correctness checks are complete. All 16
unchanged matched workloads improve against HP00, but **none reaches Lua parity**:
current interpreter medians are 2.57–51.11 times Lua. This closes the bounded
migration/evaluation, not the performance objective or complete GitHub CI acceptance.
Implemented ownership and execution boundaries are described in
[architecture](architecture.md); future activation belongs to the roadmap.

Runtime-owned executable identities and checked publication now own prepared facts;
active execution regions borrow admitted function/link views and disjoint PC/window
state. Internal calls/returns share one protocol, managed/scalar operations share
prepared execution, and canonical layout admission plus borrowed enum reads replace
the separate VE09 shortcut and snapshot consumers. External entry, real dynamic
boundaries, roots, bounds, generation checks, cancellation and observable failure order
remain. Value stays 16-byte Copy; ordinary Option still allocates traced enum objects.

### Frozen paired throughput

Baseline: HP00/VE09 f97b4095, SHA-256
`a10ea34693c9a113b97cf2f1e2af1906c07a2b175e1bfcd209bb055c01cecc22`.
Candidate: eca16497 (production code 375c727d;
eca16497 changes only an opt-in test and documentation), SHA-256
`5161d2859bb0e693960c7e7fa0b1b6e288f182eafc52ee6f804571494b9d3d33`.
The ordinary candidate binary is identical to the accepted HP04 region-view binary.

Apple M1 Max, 32 GiB, 10 logical CPUs, macOS 26.6.2 arm64; rustc 1.98.1
(48a229cea, LLVM 22.1.8), Cargo 1.98.1, vendored PUC Lua 5.4.8 through mlua.
Workspace release/opt-level=3, default target and Cargo parallelism; SDK source/native
features, only interpretation executed. No allocation counters, observer, debugger
or profiler runs in throughput. Normal GC/allocator and public host entry/return are
included; compilation, verification, loading and setup are excluded. Build cache
was warm; the two excluded build times were 0.094/0.134 seconds.

Both matrices run serial baseline/candidate/candidate/baseline, three warmups and
eleven samples per process/engine/workload (22 pooled). Workload and initial engine
order reverse in the second pair. All 1,672 measured batches pass checksums. Fixture
source/Lua bodies, sizes, expected results and ordinary loops are unchanged from HP00.
No agent-started build/test/profile or Rust edit overlaps timing. CPU placement,
frequency and desktop activity remain uncontrolled. No best samples are selected.

Microseconds per complete workload; C/B is candidate/HP00 elapsed time, so lower is
better. Ranges are sample extrema, not confidence intervals.

| Matched workload | HP00 VM | HP06 VM | C/B | Candidate Lua | VM/Lua | HP06 VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 2491.125 | 1711.479 | 0.687 | 387.584 | 4.42 | 1669.292–1754.792 |
| arrays | 5021.667 | 1403.771 | 0.280 | 67.292 | 20.86 | 1395.875–1472.250 |
| branches | 2953.709 | 2158.021 | 0.731 | 838.812 | 2.57 | 2096.459–7976.209 |
| calls | 6229.958 | 4980.124 | 0.799 | 222.417 | 22.39 | 4933.250–5085.416 |
| fibonacci | 12528.646 | 10268.604 | 0.820 | 353.416 | 29.06 | 10151.000–10640.209 |
| maps | 5991.374 | 3344.417 | 0.558 | 71.916 | 46.50 | 3227.041–11025.333 |
| byte_state | 6941.542 | 1341.688 | 0.193 | 109.250 | 12.28 | 1331.667–1394.917 |
| capture_cell | 10118.166 | 5943.458 | 0.587 | 138.292 | 42.98 | 5890.750–6086.166 |
| concrete_generic | 3073.312 | 2525.312 | 0.822 | 103.833 | 24.32 | 2497.958–2576.375 |
| direct | 348.708 | 239.792 | 0.688 | 75.312 | 3.18 | 233.833–253.041 |
| field | 3718.396 | 1017.646 | 0.274 | 90.166 | 11.29 | 1009.375–1088.208 |
| helper | 3056.749 | 2456.876 | 0.804 | 139.833 | 17.57 | 2431.625–2607.208 |
| interface | 10379.437 | 4334.583 | 0.418 | 134.480 | 32.23 | 4264.042–4394.583 |
| shared_generic | 21593.500 | 5460.146 | 0.253 | 106.834 | 51.11 | 5417.625–5576.791 |
| string_calls | 12899.812 | 3057.000 | 0.237 | 125.604 | 24.34 | 3018.125–3232.375 |
| string_constants | 7059.167 | 671.375 | 0.095 | 66.396 | 10.11 | 667.834–705.083 |

Branch and Map outliers remain in the table (7,976.209 and 11,025.333 us).
Their two candidate-process medians are 2,339.000/2,120.250 and 3,407.042/3,297.000 us;
HP00's are 2,940.208/2,954.875 and 5,974.875/6,007.375 us. Both process orders support
lower elapsed time and both remain far above Lua. Candidate/baseline Lua controls
range 0.978–1.042 in the original suite and 0.985–1.016 in the ten matched source
forms. No >5% control regression against HP00 is observed; uncertainty near parity
is not relevant because every row is far above 1.0.

Entry and adapters stay outside the 16-workload parity gate. Entry is normalized
per public call; native compares a Rust body with a script helper and is unmatched;
host_callback measures the same Rust body through each engine's host adapter.

| Diagnostic | HP00 VM | HP06 VM | C/B | Candidate Lua | VM/Lua | HP06 VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| entry | 1.430 | 1.336 | 0.934 | 0.028 | 47.00 | 1.328–1.423 |
| host_callback | 1913.396 | 1429.146 | 0.747 | 249.750 | 5.72 | 1416.625–1530.666 |
| native | 1896.417 | 1428.333 | 0.753 | 141.062 | 10.13 | 1418.125–1456.917 |

### Allocation, preparation and transitions

Separate diagnostic builds count one warmed script execution after three warmups.
The 86 final rows at production 375c727d are reused: eca16497 changes no production
code. Common HP00 rows are compared below. Original workloads each have their own
runtime in both versions; HP00 source forms shared one runtime, while final probes
use freshly linked runtimes per entry. Allocation traffic and protocol counts remain
useful, but GC/live/net deltas across the source forms are not equal-context retained
memory comparisons. Final cold/warm/changing-key probes stay separate from throughput.

| Workload | Rust allocation requests, HP00 -> HP06 | Requested bytes, HP00 -> HP06 | Metadata graph entries, HP00 -> HP06 | Region exits to ordinary dispatch, HP00 -> HP06 |
| --- | ---: | ---: | ---: | ---: |
| arrays | 12059 -> 59 | 180250 -> 20282 | 0 -> 0 | 16003 -> 2002 |
| maps | 30207 -> 2178 | 1313040 -> 1022792 | 0 -> 0 | 12003 -> 8002 |
| interface | 70040 -> 5039 | 3923832 -> 43832 | 5001 -> 1 | 10002 -> 5001 |
| shared_generic | 315599 -> 5050 | 26257537 -> 44785 | 20002 -> 2 | 20002 -> 5001 |
| field | 5009 -> 8 | 40231 -> 223 | 0 -> 0 | 10002 -> 1 |
| byte_state | 36 -> 38 | 2888 -> 3112 | 0 -> 0 | 25002 -> 1 |
| string_constants | 7 -> 7 | 170 -> 170 | 0 -> 0 | 25000 -> 0 |
| string_calls | 7 -> 7 | 166 -> 166 | 0 -> 0 | 45000 -> 5000 |

Shared-generic per-execution method preparation/environment allocation counts fall
5,000/10,000 -> 0/0. Alternating receiver/type probes keep distinct checked identities:
at 5,000 iterations their requests fall 315,726 -> 5,093 and graph entries 20,004 -> 4.
Their warm preparation does not scale with iterations. There is still roughly one
8-byte allocation per interface/shared invocation; zero preparation is not zero
call cost. Byte-state requests increase slightly; it is not an allocation win.

Scalar/direct/helper/concrete-generic/calls/fibonacci still request seven allocations
per complete warmed execution; no per-recursive-call Rust allocation is introduced.
Arrays still create one script object with two collections, Map 2,001 with five,
and both string cases zero objects/collections. Region exits are neither host
crossings nor opcode counts. Metadata counters exclude cheap Program ownership
validation and GC traversal; zero does not imply checks were removed.

### Preparation and retained memory

Preparation is intentionally visible. Across original fixtures, median program-link
time moves from 2.348–2.508 ms to 2.817–3.134 ms (six setup samples per workload/version).
For the source-form module (two samples/version), source-to-artifact is
1,141.288 -> 1,159.579 ms, artifact preparation 263.630 -> 268.355 ms and linking
2.779 -> 3.355 ms. Candidate runtime creation is 111.375 ms; the frozen HP00 forms
binary did not report that phase, so no comparative value is fabricated. This is
an execution improvement with preparation/storage costs, not a startup speed claim.

Release layout measurements: prepared instruction 24 bytes, ExecutionFrame 264,
LinkedFunction 56, ExecutionFunction 128, field/index operand 40/28, native primitive
slot 40, region exit/result 24/32. FunctionLayouts is 40, AppliedLayouts 16 (40 in
debug due to its extra environment check), AggregateLayout 56 and its lazy cell 64.
Linked constant cells are 24 bytes versus the previous 16-byte Option<Value>.
These are shallow type sizes, not total transitive metadata or bytes per instruction;
variable tables, shared Arc slices and lazily prepared descriptors add storage.

Isolated release probes compare identical runtime/input roots without versus with
preparation, with no script execution/VM frames. They cover cold preparation, two
warm repeats, 30 further repeats with GC, compatible reload/old-version retirement
and runtime teardown. Optional indices retain at most 128 keys; the 160-key case
also grows tuple widths from 1 to 160, so it is neither fixed-cost entries nor an
allocation-free steady state. Capacity can grow at eviction and remain after records
retire. Values below are extra **net allocated bytes after GC**, not RSS or peak.

| Prepared owner | Cold: 1 / 4 / 160 keys | After 30 repeats: 160 keys | After retirement: 1 / 4 / 160 keys |
| --- | ---: | ---: | ---: |
| native application | 1512 / 5824 / 1730312 | 1730312 | 0 / 0 / 0 |
| method application | 3408 / 7340 / 2633956 | 2699236 | 1376 / 1760 / 176128 |
| shared environment | 1464 / 4576 / 165896 | 167688 | 0 / 384 / 24576 |
| witness scope | 3200 / 13732 / 467856 | 496272 | 0 / 448 / 56896 |
| struct layout | 3185 / 11080 / 3851472 | 3868112 | 0 / 0 / 0 |
| enum layout | 3616 / 11488 / 3856600 | 3873240 | 0 / 0 / 0 |
| cross-version admission (two producers/key) | 900 / 1388 / 21148 | 26396 | 0 / 0 / 0 |

Setup deltas are zero; closed native preparation adds zero in this fixture. Every
control and prepared runtime returns to zero net allocation on teardown. Retirement
assertions verify old executable records/environments are reclaimed; a replacement
program's valid closed witness and reusable store capacity must not be mistaken for
old-version leaks. Small capacity differences may depend on hash/eviction order.
HP00 has no equivalent isolated owner probe, so these are final design costs, not
an invented HP00 memory speedup or retained-byte delta.

### Remaining architecture costs and bounded follow-up

Five ordinary-binary profiles use `/usr/bin/sample PID 5 1` within a ten-second
warmed execution window: three warmups for original fixtures, six for source forms.
The same candidate hash and workspace release settings apply; no allocation counters,
observer or Lua hook is active in that window. Logical instruction counting runs
separately afterward. All profile-window/counting checksums pass. The table
retains the observations; raw sampling reports are not distributed.

| Workload | Main-thread samples | Kagari / Lua logical instructions per execution | Observed remaining cost |
| --- | ---: | ---: | --- |
| shared_generic | 3657 | 95017 / 60009 | `push_prepared_call` 63.36% inclusive, return 11.18%; SipHash 9.76% and ModuleKey hashing 3.83% leaf |
| maps | 3652 | 63030 / 24012 | native invocation 53.72% inclusive; frame validation 9.53%; identified enum allocation/GC stacks 8.08% |
| fibonacci | 3703 | 240801 / 120400 | prepared call entry 36.94% inclusive, return 10.40%, frame validation 12.18%; value-window allocation 9.13% leaf |
| arrays | 3615 | 72030 / 42012 | native invocation 45.48% inclusive; scalar kernel 15.35% leaf; typed storage and access checks remain |
| string_constants | 3670 | 100012 / 52506 | scalar kernel 47.87% and region kernel 13.27% leaf; no objects or collections |

These are wall-clock stack samples, not exact CPU fractions. Inclusive families count
a sample once per family and include callees; families overlap and must not be added.
Leaf values use the sampler's collapsed-symbol list (entries >=5). Optimized inline
attribution is incomplete. The Map enum-allocation/GC family covers named
Runtime/GcHeap `alloc_enum` and collector stacks, not every representation-related
instruction. None of these percentages is a predicted speedup or an unboxing bound.

For example, the shared identity body merely returns its argument, but 5,000 calls
still select an applied invocation, check current dependencies/arguments, establish
frame slots, return and retire slots. Reused descriptors remove preparation, while
`frame/calls.rs`, `objects/invocation.rs` and frame admission still perform runtime
lookup/validation at those transitions. Fibonacci allocates no script objects and
only seven Rust allocations per complete execution: `ExecutionValues::allocate`
primarily means initializing/reusing window storage, not one heap allocation per
recursive call. Thus shrinking Value or changing GC alone cannot explain these costs.

For Map, each successful `get` still produces an ordinary Option followed by tag and
payload operations. The frozen workload executes 4,002 calls, 2,000 tag tests and
2,000 payload reads, creating 2,001 objects and five collections. Native invocation
also includes argument/result validation, installed storage authority, hashing and
mutation guards. Array indexing now uses prepared operands, but 2,000 growing
`push` operations still use native transitions; growth, type checks and real access
checks remain. Neither finding justifies deleting dynamic checks or calling the
remaining cost entirely allocation.

The next **proposed, unactivated** scope is one finite call-lifecycle migration:
review prepared invocation contracts and active-session frame ownership across
selection, argument admission, slot initialization and return. For the same identity
call, carry checked executable/type facts into a bounded internal transition rather
than rediscovering them through general module/type lookups, while validating real
receiver/version changes and retaining host/reentry boundaries. Avoid another
callsite cache, duplicated fast-call implementation or unchecked public API. Accept
only with changing receiver/type/version, eviction, source-free, GC/trap/cancel/reentry
contracts, measured call-window/lookup counts and paired shared/interface/capture/
fibonacci controls, including setup and retained memory.

After that bounded result, reconsider native collection contracts separately. Any
prepared collection operation must use exact installed binding/effect authority and
one storage implementation, preserving alias/growth guards and result publication;
ordinary Option representation should be changed only with evidence isolating its
material cost. A distinct future kernel/bytecode review can address excess logical
loads/stores/branches and general operand checks. It must retain observable trap
order, logical stepping and cancellation. These are remaining architectural questions,
not silently activated phases or proof that completing HP06 makes the design sufficient
for Lua parity.

### Correctness and reproduction

The measured revision passed its recorded local behavioral, structural and
release lifetime checks. Complete CI/feature/backend acceptance was unverified;
local success does not establish it. The implementation adds no JIT suspension,
collector replacement or general enum unboxing.

Shared generic Add failed MIR lowering with
`MissingBinding("checked callable requirement")` at the measured revision. Save
the following source and use `cargo run -p kagari-cli -- run <file>` to check it
on a selected revision. The shared-identity benchmark does not cover this capability.

```kagari
use core::ops::Add;
trait Forward {
    fn forward<T: Add<T>>(self, a: T, b: T) -> T::Output { a + b }
}
impl Forward for i32 {}
fn main() -> i32 { val receiver: Forward = 0; receiver.forward(1, 2) }
```

The tracked [benchmark driver and instructions](../benchmarks/lua-comparison/README.md)
provide current throughput, paired-baseline and diagnostic commands. For a
historical comparison, build both recorded revisions with the same toolchain,
features and fixture inputs; supply the resulting baseline binary explicitly.
Prebuilt binaries, logs and one-off probe sources from the original machine are
not distributed. Exact historical raw samples therefore cannot be recovered from
this document; the tables retain the measured results and their limits.

Reproduction commands for current measurements:

```text
uv run python scripts/benchmark_lua.py --interpreter-only
uv run python scripts/benchmark_lua.py --source-forms
uv run python scripts/profile_lua_macos.py arrays maps fibonacci
uv run python scripts/profile_lua_macos.py --source-forms shared_generic string_constants
cargo test --release -p kagari-runtime --lib --features execution-diagnostics diagnostics::memory -- --ignored --nocapture --test-threads=1
```

The sampling commands require macOS; use a suitable platform profiler elsewhere.
Keep allocation counters and profiling separate from ordinary throughput runs.
These are targeted measurement commands, not a full-suite validation requirement.
