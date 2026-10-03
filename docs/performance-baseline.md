# Performance Measurements

Keep workload, baseline/candidate, environment, timing scope and reproduction
together. These are finite observations, not language-wide guarantees. Build,
preparation and execution times are separate; allocation requests are not RSS.
Older superseded tables and successful test logs remain in Git history.
Nothing below was rerun by the documentation cleanup.

## Scoped identity pipeline, 2026-10-03

Reproduce with `uv run python scripts/measure_definition_identities.py`. The driver
creates/reuses an isolated worktree at clean `7857fd8a`, copies the identical
`definition_pipeline.rs` probe into it, builds both versions sequentially and runs
baseline/candidate/candidate/baseline. Candidate production code is `41b29c28`.
Raw logs, allocator passes, toolchain and probe hash are under ignored
`target/identity-measurements/`. The checked workload has 32 nominal `Player`
functions, one generic identity function and an entry constructing a Player and
returning 31. Both versions verify and execute that result.

Environment matches ID01: Windows 11 Pro, i9-12900K (16 cores/24 logical CPUs),
63.7 GiB RAM, rustc 1.99.0 (`b940084d7`, LLVM 23.1.1), x86_64-pc-windows-msvc.
Both use the workspace release profile, default SDK source/native features,
default Cargo parallelism and their worktree's default target directory. No builds
or tests run concurrently with measurements. Runtime operations are single-threaded.
One warmup precedes each operation in each process; fresh engines/runtimes are
created as specified below. Each timing result pools fourteen execution samples.
Destruction is outside the timed interval. An inactive forwarding System allocator
remains installed during timing; host background activity is uncontrolled. These
are finite workload observations, not whole-language performance guarantees.

Builds are excluded; differing target-cache states do not establish a build-speed comparison.

| Operation | Owned baseline median | Scoped candidate median | Observed change |
| --- | ---: | ---: | ---: |
| Fresh engine + complete analysis | 137.940 ms | 174.065 ms | +26.2% |
| Fresh engine + artifact compilation | 292.057 ms | 266.374 ms | -8.8% |
| Clone authored program + verify/adopt | 50.244 ms | 31.763 ms | -36.8% |
| Create runtime + install foundation | 57.595 ms | 63.345 ms | +10.0% |
| Import into an existing runtime | 2.275 ms | 2.012 ms | -11.6% |

`fresh_analysis` retains both engine caches and snapshot. Artifact compilation
retains the returned unsealed authoring artifact, including portable MIR.
`verified_program` clones its input inside measurement and retains the complete
runtime verification product. `runtime_creation` retains the runtime; for
`runtime_import`, both the input verified program and runtime construction are
outside timing/counting, and the returned loaded program remains installed.

Allocation counting uses a separate warmed pass. Calls include allocations and
reallocations; gross bytes sum requested sizes, including full realloc sizes.
Retained/peak bytes are net live-allocation deltas from entry, so import may also
free pre-existing runtime buffers. They exclude allocator bookkeeping, fragmentation,
pre-existing caches and input programs; they are not RSS or exclusive metadata size.
The analysis delta covers the complete retained cache graph; the verification delta
covers its full owned program/table graph. Counts varied slightly between processes;
the table reports the first counting pass, with both raw passes preserved.

| Retained operation result | Baseline bytes | Candidate bytes | Baseline allocation calls | Candidate allocation calls |
| --- | ---: | ---: | ---: | ---: |
| Fresh engine + complete analysis | 14,624,321 | 10,364,137 | 3,288,870 | 3,601,730 |
| Fresh engine + artifact compilation | 2,728,648 | 3,182,804 | 7,471,364 | 5,595,317 |
| Clone authored program + verify/adopt | 1,827,175 | 760,720 | 1,480,900 | 704,051 |
| Create runtime + install foundation | 1,417,415 | 1,221,122 | 1,485,456 | 1,562,887 |
| Import into an existing runtime | 53,176 | 1,163,897 | 15,859 | 23,447 |

| Complete encoded artifact | Baseline | Candidate |
| --- | ---: | ---: |
| KBC including portable MIR/debug metadata | 778,657 bytes | 279,003 bytes |

The artifact is **64.2% smaller**. Retained analysis state decreases **29.1%** and
verified-program state decreases **58.4%**, while the returned unsealed authoring
artifact retains **16.6% more** memory. Verification allocation calls decrease
52.5%; artifact compilation calls decrease 25.1%. Gross requested bytes increase
for analysis (272.5 -> 322.3 MB) and artifact compilation (539.2 -> 575.1 MB).
Source checking/proof validation still project transient exact paths, so shorter
stored records do not establish lower total allocation traffic. Analysis takes
26.2% longer and runtime creation 10.0% longer in this run; those regressions remain
explicit measured costs rather than claimed speedups.

Independent runtime imports retain the original immutable version identity but
normalize nonempty module metadata separately. The probe records full module Arc
sharing as true for baseline and false for candidate. Import retains 1.16 MB versus
53 KB and requests 3.23 MB versus 0.77 MB. Do not equate version sharing with whole
normalized-module sharing, or extrapolate the observed 11.6% import-time reduction
into a general loading improvement.

The common append probe measures one append to a 10,000-definition table,
excluding setup: 0.2 us/zero allocations without a retained snapshot versus
323.2 us/ten allocations with one. This isolates retained-prefix index copying,
not whole-pipeline cost.

Finite follow-up costs are source/proof authoring projections, table index copies
on retained-prefix appends, and normalized metadata copies across independent
runtimes. No interpreter-loop optimization, global identity interner, stable identity
hash, incremental disk cache, format version bump or compatibility reader is part
of ID01-ID05.


## Representation probes

Commands:

```text
cargo run -p kagari-common --example definition_identity --release --locked
cargo run -p kagari-abi --example definition_metadata --release --locked
```

Same Windows/i9/rustc 1.99.0 environment and release/default-feature settings as
above. Setup/builds are excluded. Header sizes exclude heap contents and tables:

| Header | Owned paths | Scoped IDs |
| --- | ---: | ---: |
| Definition identity | 72 bytes | 8 bytes |
| GenericParameterAbi | 80 bytes | 16 bytes |
| AbiType | 128 bytes | 64 bytes |
| NominalAbiType | 120 bytes | 56 bytes |

The identity probe copies 100,000 values, with one warmup and eleven timing samples;
allocation counting is separate. Owned clones perform 600,000 allocations with a
16,011,800 ns median; short-ID copies allocate zero with a 20,500 ns median.
Black-box barriers retain the operations. These isolated results do not imply
the same compiler/runtime speedup; full-pipeline costs above include owner tables.

## Matched Kagari/Lua baseline, 2026-10-03

The [benchmark package and report](../benchmarks/lua-comparison/README.md) compare
seven equivalent source workloads using standard PUC Lua 5.4.8 and the current
Kagari SDK. Release-profile execution excludes compilation, preparation and
initialization; two sequential processes provide 22 samples per actual route.
Every timed batch validates its result against an independent Rust checksum.
The six nontrivial workloads show Kagari VM/Lua median time ratios of
104.71–220.44 on this machine. Their JIT entries are unsupported and omitted,
not measured as interpreter fallback. Only the trivial entry route executes
natively. The report records setup costs, sample ranges, toolchain/machine,
container/safety differences and reproduction commands. These numbers describe
this finite suite rather than an overall language ratio; no CPU profile or
production optimization is part of BP01.

The subsequent [BP02 execution diagnosis](../benchmarks/lua-comparison/README.md#interpreter-diagnosis-bp02-2026-10-03)
samples optimized VM execution in six fixtures and counts instructions separately.
Arithmetic spends 23.21% of instruction-pointer samples in frame access/checks,
10.03% in termination-state lookup and 12.10% in instruction fetch/clone. It
executes 750,014 Kagari instructions versus 250,007 Lua instructions and performs
no GC collections. Map lookup constructs heap-backed Option results: this fixture
allocates 2,001 GC objects and collects five times per complete call. The report
records symbol-resolution and sampling limitations; runtime checks remain intact.

## Preparation and runtime construction

Both comparisons use Windows 11/i9-12900K/63.7 GiB/rustc 1.99.0, workspace O1,
default target/parallelism and warm build caches. Execution is single-threaded,
builds excluded, with no concurrent measuring jobs; host background activity is
uncontrolled. These are historical paired observations, not current timings.

| Workload/change | Baseline | Candidate | Scope |
| --- | ---: | ---: | --- |
| First Runtime::new on a host thread, registration sharing | 1,124.628 ms | 481.447 ms | Baseline 1b975a36 diagnostic executable, stage-print overhead included; default runtime features |
| Next five fresh runtimes, median | 1,079.989 ms | 76.996 ms | Same thread's immutable registrations; destruction excluded |
| Complete language-contract matrix, sealed bytecode reuse | 155.36 s | 105.71 s | Clean 145b34b1 versus TO02, one serial process each; default SDK source/native, unchanged 149 cases/408 routes |

Reproduce construction with `cargo run -p kagari-runtime --example runtime_construction`.
For the matrix, build with `cargo test -p kagari-embed --lib --no-run` and run the
matching executable with the exact language-contract test filter and
`--test-threads=1 --nocapture`; an obsolete executable is not a valid baseline.

Sharing checked registrations preserves fresh heap/host/generation state and
installation checks. Sealed-input reuse reduces native preparation from four
equivalent bytecode verifications to one, while independently verifying MIR,
canonical correspondence and raw/changed inputs. Later identity migration changes
the workload representation; these figures must not be labeled current startup
performance. The matrix result is a suite observation, not script throughput.

## Interface dispatch and sorting

The source/allocation finding was measured on the Apple M1 baseline documented in
[the architecture review](architecture-review-2026-10-03.md). Subsequent interface
sharing uses that review's O1/default SDK features, prebuilt serial processes and
counting allocator. Five warmups and 21 samples include construction plus 1,000
interpreted calls; widths 1/8/32 retain the same selected method/result.

| Interface width | Baseline allocation calls | Shared descriptors/receiver cache | Candidate median |
| --- | ---: | ---: | ---: |
| 1 | 75,099 | 41,111 | 4.781 ms |
| 8 | 243,358 | 41,370 | 4.888 ms |
| 32 | 820,246 | 42,258 | 4.891 ms |

The removed cost is whole-table copying. One-time construction/cache allocation
remains. Existing-view regressions isolate 1,000 warmed calls: 16,000 allocations
for direct methods and 26,000 for closed generic inherited methods at every width,
with zero GC publications. No zero-allocation dispatch claim is made.

Reproduce with `cargo test -p kagari-vm --test library_measurements -- --ignored
--nocapture`. The bounded sorting workload uses one warmup/three reset samples,
compilation/input construction excluded, O1 and explicit roots. At 4,096 elements,
primitive concrete/interface sorting medians were 95.917/104.250 us (58/439 Rust
allocations, zero GC publications). Script comparator sorting still made 53,392
comparisons and 53,393 GC allocations, at 162.905/162.114 ms. The recorded earlier
callback baseline was 159.662/159.655 ms: no callback-loop speedup was demonstrated.
Use profiling evidence in the Lua report before selecting interpreter work.

## Reproduction boundaries

Committed probes and benchmark reports preserve workloads. Temporary executable
copies/logs belong under ignored target directories. Use the recorded commit when
reproducing historical comparisons; rebuilding the current tree alone is not a
before/after measurement. Mac O1, Windows O1 and Windows release observations are
not interchangeable. Source-free checks, roots, storage access, cancellation and
generation validation cannot be removed to improve a timing.
