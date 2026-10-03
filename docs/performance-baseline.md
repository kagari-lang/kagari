# Foundation Performance Measurements

These reproducible workloads establish a baseline for R18. The figures are
observations on one machine, not performance guarantees.

## Test bottleneck diagnosis, 2026-10-03

Baseline: clean `1b975a36`, Windows 11 Pro, Intel Core i9-12900K (16 cores,
24 logical processors), 63.7 GiB RAM, rustc 1.99.0 (`b940084d7`, LLVM 23.1.1),
`x86_64-pc-windows-msvc`. Use the workspace's O1 dev/test profile with debug
information, default target directory and Cargo build parallelism. SDK measurements
enable default source/native features. Builds and timed execution ran separately;
no competing agent build/test ran during measurement. Background host activity was
not controlled. Build caches were warm; each measured runtime was freshly created.

The previous workspace test log reports 50.78 seconds of compilation before
execution. Selecting common/HIR/runtime/bytecode subsequently recompiled shared
dependencies, including smallvec, and reported 102 seconds of compilation. Its
634 passing tests report **88.93 seconds summed across 25 test-target/doctest
summaries**, excluding process startup and Cargo overhead. HIR's 400 unit tests
took 25.35 seconds; typed_path_views took 17.55 seconds; runtime's 51 unit tests
took 8.00 seconds. These logs do not establish a complete workspace ranking.
Changing the selected packages can change Cargo feature unification and artifact
reuse; compilation must not be counted as script/test execution.

### Bounded language-contract measurement

Temporarily timed compile, direct/serialized preparation, runtime construction,
load, setup and execution in
[language_contract.rs](../crates/kagari-embed/src/runtime/language_contract.rs).
Build with `cargo test -p kagari-embed --lib --no-run --message-format=json`, then
run its reported test executable directly with
`runtime::language_contract::language_contract_routes_preserve_values_diagnostics_and_effects
--exact --nocapture --test-threads=1`. Apply a 90-second subprocess deadline.
This is a partial profiling run, not a passing/full test-suite result. The existing
assertions and four execution routes remained enabled.

Before the deadline, 14 executing cases completed all 56 routes; 13 diagnostic
cases also completed. The table includes only the 14 complete executing cases,
so intervals correspond to the same work. Each case compiles once and prepares
direct and serialized products once; each route gets a fresh runtime.

| Phase | Samples | Total | Median per sample |
| --- | ---: | ---: | ---: |
| Source check/lower/artifact construction | 14 | 4.769 s | 317.984 ms |
| Direct and serialized preparation | 14 | 10.226 s | 702.827 ms |
| Runtime construction | 56 | 71.110 s | 1,152.819 ms |
| Host registration and program load | 56 | 0.245 s | 3.580 ms |
| Execute/prepare-native and outcome assertion | 56 | 0.006 s | 0.049 ms |
| Complete executing cases, including cleanup | 14 | 87.638 s | 5.684 s |

Runtime construction accounts for **81.1%** of complete-case wall time. Its
observed range was 0.922-2.841 seconds, so these are noisy local measurements,
not universal latency promises. This prefix does not cover all later cases,
large workloads, native operations or the entire workspace.

### Initialization attribution and priorities

A separate temporary test constructed six fresh default runtimes on one thread;
discard the first warmup and report five samples. Added timers around foundation
build/install, algorithm build/install and the binding dependency/registration
loops in [lib.rs](../crates/kagari-runtime/src/lib.rs) and
[native/module.rs](../crates/kagari-runtime/src/native/module.rs).

| Initialization work | Median |
| --- | ---: |
| Complete Runtime::new | 931.420 ms |
| Foundation module build/check | 856.055 ms |
| Foundation installation | 64.892 ms |
| Collection algorithm module build | 2.924 ms |
| Collection algorithm installation | 7.031 ms |
| Per-binding dependency catalogs inside foundation build (91 bindings) | 639.155 ms |

The dependency interval is nested in module construction, not additive to it.
NativeModule::checked seeds each binding's dependencies with all owned traits and
module implementations, traversing and cloning those records repeatedly. Each
Runtime::new builds and validates that module again. SDK engine construction also
creates a validation Runtime; its runtime factory creates another fresh Runtime.
This magnifies the cost in SDK/VM tests, beyond this one matrix.

Recommended order, without activating an implementation migration:

1. Address the architecture review's R4: reuse checked immutable foundation
   registration blueprints and declaration/dependency storage. Preserve fresh heaps,
   host state, generation identities and atomic installation checks. Rc-backed
   handlers need an appropriate thread/engine lifetime, not unsafe Send/Sync claims.
2. Split the single language-contract test into separately named cases, retaining
   every route and assertion and the current compile/product sharing. Its internal
   serial loop prevents libtest from scheduling cases independently and hides
   progress. More test threads cannot parallelize that one test; measure memory
   and contention before increasing concurrency.
3. Address R6's repeated sealed-input verification at PreparedProgram boundaries.
   It is secondary here (11.7% in preparation); independently decoded/derived
   products must still be validated and compared.
4. Keep development test selections coherent and distinguish warm no-change builds
   from feature-set switches. Do not remove conformance routes or change build
   profiles based solely on these observations.

All diagnostic Rust edits were restored byte-for-byte; no production behavior or
test coverage changed. Raw logs, summaries, backups and initialization timer patch
are under ignored `target/test-bottlenecks/`. The tables and instrumentation
boundaries above preserve the conclusions and reproduction approach after cleanup.

## Foundation baseline, 2026-09-25

Environment: Windows x86_64, Intel Core i9-12900K, rustc 1.98.1
(`48a229cea`, LLVM 22.1.8), target `x86_64-pc-windows-msvc`, Cargo release profile.
Implementation: commit `e243943` plus the benchmark example below; KBC format 38
and runtime ABI v38.

Run `cargo run --release -p kagari-embed --example foundation_baseline` to measure
the compiler and VM. Five independent process runs each take five cold compilation
samples of a 33-function source module; the table reports each process's median.
The edit replaces the body of one function through a source overlay, then analyzes
the new snapshot. The VM measurement averages 10,000 root calls to `main`, each of
which makes one internal function call, after 100 warmup calls. The compiler time
includes construction of a fresh engine, analysis, lowering, verification and
artifact creation; it excludes serialization.

| Process | Cold compile median (µs) | Edit analysis (µs) | Root + internal call (ns) | Reused / checked bodies |
| --- | ---: | ---: | ---: | ---: |
| 1 | 860 | 470 | 1,651 | 32 / 1 |
| 2 | 840 | 481 | 2,201 | 32 / 1 |
| 3 | 730 | 408 | 1,597 | 32 / 1 |
| 4 | 773 | 416 | 1,647 | 32 / 1 |
| 5 | 810 | 409 | 1,649 | 32 / 1 |
| Median | 810 | 416 | 1,649 | 32 / 1 |

The executable image serializes to 24,513 bytes. Two runtimes loaded from one
`VerifiedProgram` have pointer-identical `Arc<BytecodeModule>` handles; the
shared module has three strong references during the measurement. The encoded
size is a stable proxy for code size, **not** the resident heap footprint.
Runtime-owned instance state is separate from this shared code allocation.

Run `cargo run --release -p kagari-runtime --example rooted_values` for the GC
workload described below. Five repetitions, each with a fresh runtime and 10,000
array links, produced the following collector-only pauses:

| Repetition | All-live collection (ns) | All-dead collection (ns) | Reclaimed units |
| --- | ---: | ---: | ---: |
| 0 | 1,145,100 | 361,400 | 20,000 |
| 1 | 1,142,700 | 320,200 | 20,000 |
| 2 | 1,360,000 | 313,500 | 20,000 |
| 3 | 1,281,700 | 319,900 | 20,000 |
| 4 | 1,024,300 | 295,400 | 20,000 |
| Median | 1,145,100 | 319,900 | 20,000 |

Neither workload measures server concurrency, process peak memory, nor optimized
JIT calls. The older GC series remains below for historical comparison.

## Mark-sweep baseline, 2026-09-12

Command: `cargo run --release -p kagari-runtime --example rooted_values`.
Environment: Windows x86_64, Intel Core i9-12900K, rustc 1.98.1
(`48a229cea`, LLVM 22.1.8), target `x86_64-pc-windows-msvc`, Cargo release profile.
Implementation: foundation owned heap/roots checkpoint, artifact v10 / runtime ABI v5.

Each of five repetitions creates a new runtime and a chain of 10,000 arrays,
each with one value. One rooted handle retains the entire chain during the first
collection; dropping it makes all objects unreachable for the second collection.
The latter must reclaim 20,000 accounting units. No compilation or allocation
time is included in the pauses. The reported collector interval includes root
snapshotting, graph tracing and sweeping; it excludes runtime root gathering and
resource-counter synchronization around the collector call. The live pass also
scans the slot table. No statistical performance guarantee is inferred from five runs.

| Repetition | All-live collection (ns) | All-dead collection (ns) | Reclaimed units |
| --- | ---: | ---: | ---: |
| 0 | 1,200,100 | 458,600 | 20,000 |
| 1 | 1,066,800 | 293,000 | 20,000 |
| 2 | 1,129,300 | 298,000 | 20,000 |
| 3 | 1,162,300 | 410,000 | 20,000 |
| 4 | 1,024,700 | 354,100 | 20,000 |

Observed medians: 1.1293 ms with all objects live, 0.3541 ms with all objects dead.
This workload provides a reproducible starting point for later GC comparisons;
it does not represent a server workload or include peak-memory accounting in bytes.


## MIR architecture baseline (2026-09-28)

This is the A05 baseline for the thirteen-crate implementation. Earlier Windows
release measurements above are historical and are not comparable to this run.
No clean-build speedup or change from those results is claimed.

Environment: Rust 1.98.1 (`48a229cea`, LLVM 22.1.8), aarch64-apple-darwin,
MacBookPro18,2, 32 GiB RAM, 10 logical CPUs; workspace O1 dev/test profile with
ordinary debug information, default Cargo parallelism and the normal `target/`
directory. Dependencies use repository `Cargo.lock`, including Cranelift 0.132.0.
Source/native SDK features are enabled; the CLI native smoke check uses `jit`.
Cargo caches are warm from correctness runs; no cache cleaning was performed.

Reproduction:

```sh
cargo run -p kagari-embed --example architecture_baseline
cargo run -p kagari-embed --example foundation_baseline
uv run python scripts/check_features.py
```

Compile the examples before timing their executable workload. The example reports
only in-process durations; Cargo's build time is separate. `architecture_baseline`
uses the fixed scalar source `fn main() -> i32 { 40 + 2 }`, one warmup followed by
21 compilation or 101 preparation samples and 10,001 execution samples. The source
measurement creates a fresh engine per sample; OS pages and process-wide tables are
warm. A counting allocator adds atomic bookkeeping to these timings. They are small
workload observations, not production throughput guarantees.

| Operation | Median | Scope |
| --- | ---: | --- |
| Fresh-engine source to portable artifact | 2,969.709 µs | 21 samples, parse/check/lower/verify/build and result disposal |
| MIR verification and analysis | 1.667 µs | 101 samples; input clone excluded, verification/sealing included |
| Decode and native-input verification | 64.416 µs | 101 samples; canonical correspondence and prepared-result disposal included |
| New runtime and shared-program linking | 4.000 µs | 101 samples; runtime creation/link/disposal included |
| Native compilation | 95.000 µs | 21 samples; existing host backend/link setup, product freeing excluded |
| Cached preparation and installation | 1.666 µs | 101 samples; cache hit, handle installation/disposal |
| SDK interpreter entry | 1.542 µs | 10,001 samples; result checks and ordinary session teardown |
| SDK native entry | 2.000 µs | 10,001 samples; requires Native report and equal result |

For this tiny four-point function, native entry is 0.458 µs slower than interpreter
entry in the measured medians. Both pay runtime/session and logical-helper overhead;
the measurement does not isolate which component explains the difference. It does
not justify a general native speedup or a regression claim against the old backend.
A later Cranelift coverage/performance track should use larger supported workloads
and profiling before changing the logical budget contract.

The portable artifact is 3,510 bytes versus 2,364 bytes for its bytecode-only export;
the MIR payload is 1,118 bytes (the envelope adds the remaining 28 bytes). One cached
native product retains 5,480 requested Rust heap bytes; retaining its installed
handle raises the measured delta to 5,600 bytes. These counters exclude executable
page mappings, allocator overhead and RSS. Cache entries are bounded at 4,096 per
prepared program; the measurements do not extrapolate a universal cache-entry size.

Shared preparation versus independently preparing identical bytes in each runtime:

| Runtimes | Shared preparation: retained Rust bytes | Independent preparation: retained Rust bytes |
| ---: | ---: | ---: |
| 1 | 20,350 | 20,350 |
| 8 | 43,324 | 162,800 |
| 32 | 122,092 | 651,200 |

The counter includes retained prepared MIR/bytecode/validation state and runtime/link
state, with result-vector capacity allocated before measurement. The example asserts
shared verified-version identity in the first route and distinct identities in the
second. It does not confuse serialized artifact size with live memory or claim that
all per-runtime state is shared. Native products are not compiled in this table.

The existing `foundation_baseline` (33 functions, one body edited) also ran in O1:
fresh-engine source compilation median 2,773 µs across five samples; edit analysis
1,449 µs with one checked and 32 reused bodies; portable code image 53,344 bytes;
root-plus-one-internal-call mean 2,093 ns across 10,000 calls, checksum 310,000.
That example has no counting allocator and a different workload; do not compare its
call number directly with the scalar SDK medians above.

Logs are reproducible under `target/a05-architecture-baseline-final.log` and
`target/a05-foundation-baseline.log`; these tables preserve the durable observations
if the ignored cache is removed. Workspace build/test timings and correctness
acceptance are recorded in the refactor plan's A05 ledger.

## Test preparation hotspots (2026-10-01)

Investigation starts at `adb5baae`, after the standard library/HIR migration's
integration checkpoint. The user requested focused test optimization and no
whole-workspace test execution. The previous incomplete workspace attempt took
2,107.435s; VM library tests (772.72s), standard declaration tests (404.61s),
embedding language-contract routes (318.70s), and compiler tests (157.63s) accounted
for approximately 79% of that wall time. Those are suite durations, not individual
test CPU times, and do not isolate Rust compilation from test setup.

Environment: Rust 1.98.1 (`48a229cea`, LLVM 22.1.8), Cargo 1.98.1, macOS 26.6.2
(`25G83`), aarch64-apple-darwin, MacBookPro18,2 / Apple M1 Max, 32 GiB RAM,
10 logical CPUs. Use workspace O1 dev/test profiles, default Cargo parallelism,
default `target/`, and the workspace's unified default features (SDK source/native).
Build and registry caches are warm; no cleanup occurs. Builds finish before timed
executions, and measured test processes run sequentially. An exact test filter
selects one test with the ordinary harness settings; the documentation target
uses its ordinary three-test harness.

The paired diagnostic runs use prebuilt test executables, one process per test,
with `sample <pid> 3 1 -file <path>` after one second. The table uses the harness's
execution interval, including fixture setup/teardown and sampler interference,
excluding Cargo compilation, process launch and tool reporting. Wrapper wall times
are retained separately in JSON. These are single paired observations, not
statistical performance guarantees. A preliminary measurement using an obsolete executable
was stopped and discarded; rebuilding with `cargo test --workspace --no-run`
restores the baseline dependency configuration without executing any tests.

The three-second worker-thread samples locate preparation hotspots:

- Required-method budget sweep: 1,288 / 1,992 samples include
  `compile_test_bytecode`, 422 include `Runtime::load_program`, and only 20 include
  `Vm::execute`. This window identifies setup cost rather than a slow script loop.
- Language-contract routes: 2,276 / 2,279 samples include the fixture's `compile`
  function, including source checking, lowering and portable-input preparation.
- Required-method lifecycle sweep: 1,288 / 2,401 samples include
  `Runtime::load_program`; serialization/decoding also occurs inside every scenario.

These short windows are not whole-run time percentages. Deeper stacks identify
HIR `prepare_signatures` / `TypeCatalog::bindings` and bytecode
`verify_trait_bounds`. `linked_bounds_match` rebuilds a `ProofCatalog` and checks
the closure's declarations, ancestry and parent implementations for each module.
These validation costs also occur in source lowering and artifact/native-input
preparation. Forgery tests change their inputs and must retain fresh validation;
their remaining costs require profiling the verifier itself rather than reusing
an already verified result.

A native-enabled `PreparedProgram::from_artifact` currently triggers four complete
bytecode graph verifications: loader validation, native-input bytecode validation,
canonical lowering of decoded MIR, and `VerifiedProgram::new`. These are separate
checks of open or newly produced values, not evidence that arbitrary input can be
trusted. Together with per-module proof-catalog construction, this is a concrete
remaining optimization target for a later change to the checked-input boundary.

The retained optimizations change only test fixtures:

- Compile each language-contract case once. Prepare direct and serialized products
  independently, then reuse each immutable input for interpreter/JIT execution.
  All four execution routes, fresh runtime/host state, native/fallback assertions,
  trap/effect/budget checks and generation-pinned reload scenarios remain.
  Frontend rejection cases still assert their original diagnostics once; there is
  no executable route for a rejected source.
- In the shared native lifecycle fixture, serialize/decode and verify once per
  input route, then load cloned `VerifiedProgram` handles into a fresh runtime at
  every cancellation/allocation cut. Each runtime still links its own host bindings,
  checks permissions/ownership/generations, and owns its heap and execution session.
  Every original cut, reentry, side-effect and GC cleanup assertion remains.

No production cache, validation bypass, artifact version change, reduced scenario
count or test-thread/profile adjustment is part of this optimization. Raw malformed
artifact tests are unchanged. Logs, environment, samples and timing JSON are under
ignored `target/test-performance/`; final observations are recorded below and in
the active integration plan's ledger.

| Focused test | Before (s) | After (s) | Observed reduction |
| --- | ---: | ---: | ---: |
| Embedding language-contract routes | 319.51 | 108.50 | 66.0% |
| Required-method lifecycle allocation/cancellation cuts | 19.86 | 5.19 | 73.9% |

Both paired tests pass with their original fixtures and behavioral assertions.
The other three callers of the shared lifecycle fixture also pass: fallible
destinations (4.67s), partition (5.31s), and lazy iterators (10.80s). These are
post-change verification intervals without sampling, not paired performance claims.
These measurements do not establish a new whole-workspace duration. The earlier
404.61s standard declaration suite is a historical reference, not a fresh paired
documentation benchmark with identical sampling conditions.

The documentation experiment replaced a fixed source in one engine, preserving
fresh runtime state and all direct/encoded/panic assertions. All three tests pass,
but its 414.56s harness interval shows no demonstrated improvement against the
historical 404.61s reference. The experiment is withdrawn; the original isolated
engine fixture remains. Its sample still locates signature preparation and complete
artifact/native-input verification. No documentation speedup is claimed.

To reproduce behavioral verification without executing other workspace tests:

```sh
cargo test -p kagari-embed --lib language_contract_routes_preserve_values_diagnostics_and_effects
cargo test -p kagari-embed --test standard_declarations
cargo test -p kagari-vm --lib required_entries_reenter_cancel_and_exhaust_every_allocation_cut
cargo test -p kagari-vm --lib fallible_destinations_reenter_cancel_and_exhaust_every_allocation_limit
cargo test -p kagari-vm --lib partition_destinations_reenter_cancel_and_exhaust_every_allocation_limit
cargo test -p kagari-vm --lib lazy_iterators_reenter_cancel_and_exhaust_every_allocation_cut
```

Separate build time from execution time. The paired runs above build with
`cargo test --workspace --no-run`, then invoke only the corresponding prebuilt
executable with `--exact <fully-qualified-test-name> --nocapture`. Its hash depends
on the dependency configuration; do not time an obsolete executable left in `target/`.
