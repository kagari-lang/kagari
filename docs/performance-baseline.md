# Performance Measurements

Current interpreter evidence is VE09, followed by the VE08 whole-track comparison
against the preserved VE00 binary. Representation tradeoffs and distinct compiler,
startup and platform observations remain below. Intermediate phase reports live
in Git history; the historical index identifies their checkpoints.

Keep workloads, hashes, environment and timing scope together. Allocation requests
are not RSS; compilation/setup and execution are separate. Small differences are
not significance claims, and local correctness does not imply GitHub CI or Lua parity.

## Native enum result layout reuse (VE09), 2026-10-10

The explicitly authorized follow-up is locally accepted. Native Map::get results
and their consuming patterns retain equal concrete enum layouts in different
module slots of one pinned program. EnumVariantRef::matches_layout now compares
those existing applied layouts directly when neither side has a lexical environment.
Runtime owner and variant checks precede the comparison; different generations or
lexical scopes retain the general type-graph comparison. Allocation, dynamic payload
checks, roots, traced enum storage, cancellation and return publication are unchanged.
No additional cache, public API, metadata field or unsafe code was introduced.

In the frozen map workload the median falls from 12,423.354 to 6,075.917 microseconds,
0.489x VE08 time (51.1% less). The isolated Map::get counting probe removes 90 Rust
allocation requests per iteration, with unchanged GC objects/collections. Other
workloads retain small increases and decreases below, including fibonacci +2.1%.
All 16 matched nontrivial workloads still fail Lua parity, at 3.66-201.94x Lua time;
this phase does not close the interpreter-wide performance goal. Full CI is unrun.

### Attribution and allocation

The unchanged `target/ve05/collections.kgr` probe performs 5,000 warmed iterations
and checks 325,000. Separate baseline counting and stack captures use VE08 production
code. Requests 100-199 in a diagnostic execution allocate 90 times in enum layout
compatibility, six in pattern descriptor admission, two in payload snapshots, one
for payload storage and one for allocation validation. This is a bounded stack
sample, not a distribution over every workload. Backtrace machinery is excluded
from counting. A temporary descriptor diagnostic confirms one ProgramDescriptor,
module slots 0/14, enum IDs 4/5, equal full applied layouts and no lexical environments.
Native result layout caching already hits; adding another cache would miss this cost.
All diagnostic instrumentation was removed before candidate verification and timing.

| Probe | VE08 requests | VE09 requests | VE08 requested bytes | VE09 requested bytes | Script objects / GC collections, both |
| --- | ---: | ---: | ---: | ---: | ---: |
| Map get / ordinary Option | 500280 | 50280 | 38753903 | 1353903 | 5001 / 10 |
| Map update + contains_key | 25045 | 25045 | 283323 | 283323 | 1 / 0 |

Requests count alloc/alloc_zeroed/realloc; requested bytes are neither RSS nor peak
live memory. Construction, public host entry/return and default GC remain included;
source compilation, loading and three warmups are excluded. Probe entry aliases
`string_constants` / `string_calls` retain their historical names but execute these
collection cases, not the frozen string timing fixtures. Value remains 16-byte Copy;
no backing-layout or persistent metadata size changed in VE09.

### Paired release measurements

Baseline: VE08 `06757739`, `target/ve08/candidate-executable`, SHA-256
`98a14ea576a1a4f616bb6573c8e7342f01148dbbeac70147753269d08edf2261`.
Candidate: that revision plus the EnumVariantRef layout comparison change,
`target/ve09/candidate-executable`, SHA-256
`a10ea34693c9a113b97cf2f1e2af1906c07a2b175e1bfcd209bb055c01cecc22`.

Apple M1 Max (10 logical CPUs, 32 GiB), macOS 26.6.2 arm64; Rust 1.98.1
(`48a229cea`, LLVM 22.1.8), Cargo 1.98.1; workspace release defaults and default
Cargo parallelism/target, source/native SDK, mlua lua54,vendored Lua 5.4.8. Only
interpreter execution is timed. Candidate build took 19.37 s; the paired drivers
reused it in 0.132 / 0.084 s, all excluded from execution. Each matrix runs
baseline,candidate,candidate,baseline sequentially with three warmups and eleven
samples per process/route, reversing initial order for the second pair. There are
22 samples per variant/engine/workload; all 1,672 measured batches pass checksums.
No tests, builds, allocation probes or profilers ran concurrently with throughput.

CPU frequency, affinity and background activity remain uncontrolled. Tables show
microseconds per complete workload, C/B median ratio and candidate sample extrema;
entry is normalized per call. Extrema are not confidence intervals; small changes
are not claims of significance. The 14,022.084 us string-constant sample is retained.
Raw reports retain both variants' ranges and separate setup measurements. Execution
includes host entry/return and ordinary GC; compile/verify/link/setup are excluded.
The seven original and twelve source-form fixtures are unchanged. Native is an
unmatched adapter diagnostic; host_callback compares the same Rust body separately.
The bounded numeric diagnostic matrix was not rerun for this enum-only change.

### Original workload results

| Workload | VE08 VM | VE09 VM | C/B | Candidate Lua | VM/Lua | VE09 VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 2488.500 | 2507.646 | 1.008 | 383.250 | 6.54 | 2440.833–2582.875 |
| arrays | 5005.250 | 5034.105 | 1.006 | 68.792 | 73.18 | 4971.166–5495.625 |
| branches | 2926.667 | 2952.624 | 1.009 | 806.792 | 3.66 | 2889.709–3013.667 |
| calls | 6127.688 | 6188.833 | 1.010 | 224.292 | 27.59 | 6146.750–6400.333 |
| entry | 1.412 | 1.412 | 1.000 | 0.029 | 49.00 | 1.404–1.452 |
| fibonacci | 12422.104 | 12688.541 | 1.021 | 355.584 | 35.68 | 12425.834–13253.875 |
| maps | 12423.354 | 6075.917 | 0.489 | 69.041 | 88.00 | 5975.333–6171.208 |

### Source form results

| Workload | VE08 VM | VE09 VM | C/B | Candidate Lua | VM/Lua | VE09 VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| forms_byte_state | 6964.188 | 6915.063 | 0.993 | 110.250 | 62.72 | 6865.083–6978.459 |
| forms_capture_cell | 10144.459 | 10129.708 | 0.999 | 140.500 | 72.10 | 10060.167–10484.417 |
| forms_concrete_generic | 3135.417 | 3078.604 | 0.982 | 103.084 | 29.87 | 3052.292–3126.583 |
| forms_direct | 345.916 | 348.812 | 1.008 | 75.146 | 4.64 | 335.500–398.709 |
| forms_field | 3749.501 | 3738.563 | 0.997 | 90.209 | 41.44 | 3704.917–3812.042 |
| forms_helper | 3132.875 | 3071.001 | 0.980 | 141.312 | 21.73 | 3046.250–3136.167 |
| forms_host_callback | 1912.896 | 1908.938 | 0.998 | 248.667 | 7.68 | 1891.167–1951.208 |
| forms_interface | 10710.438 | 10429.146 | 0.974 | 135.833 | 76.78 | 10378.250–10510.458 |
| forms_native | 1906.542 | 1900.250 | 0.997 | 140.688 | 13.51 | 1883.250–1945.833 |
| forms_shared_generic | 22307.730 | 21708.834 | 0.973 | 107.499 | 201.94 | 21553.542–22412.792 |
| forms_string_calls | 12905.834 | 12908.000 | 1.000 | 124.708 | 103.51 | 12804.000–13175.792 |
| forms_string_constants | 7070.312 | 7111.479 | 1.006 | 67.334 | 105.62 | 7017.875–14022.084 |

### Correctness and reproduction

All 38 affected existing contracts pass: enum payload identity/reload (4), source-free
native enums including nested traced payloads and rejected operations (2), generic
reload (2), native enum boundaries (6), hash handles/custom callbacks and cleanup
(8), GC ownership (6), native conversion (10). Runtime all-target Clippy, formatting,
structure (988 files, no violations/exceptions) and diff checks pass. No full
workspace suite was rerun for this bounded change; VE08's full pass is historical.
No carried local build/test failures remain. Complete GitHub CI is unrun.

Raw paired reports are `target/lua-comparison/20261010T002035Z-paired/` and
`20261010T002247Z-forms-paired/`. Counting/trace sources, builds and focused logs are
under `target/ve09/`; baseline/candidate allocation executables remain preserved.
Temporary logs/binaries are ignored, with durable evidence recorded here.

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/ve08/candidate-executable
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/ve08/candidate-executable
uv run python target/ve09/build_probe.py target/ve00/measure_source.rs target/ve09/candidate-allocations
target/ve09/baseline-allocations target/ve05/collections.kgr
target/ve09/candidate-allocations target/ve05/collections.kgr
```

The build helper selects the current release rlibs. Rebuilding a historical probe
requires its corresponding production revision; do not overwrite a saved baseline.

## Compact value and interpreter final local evaluation (VE08), 2026-10-10

VE00-VE08 implementation and final local correctness evaluation are complete.
`Value` is 16 bytes and Copy in both debug and release, down from 32 bytes.
Runtime-local string constants share traced storage, concrete fields and calls use
prepared facts, selected collections use scoped access, and scalar segments retain
bounded code/bank borrows. Host retention, runtime ownership, generations, roots,
checked arithmetic and observable execution boundaries remain enforced.

The frozen Lua parity gate **fails all 16 matched nontrivial workloads**: candidate
medians take 3.65-201.48x Lua time. Improvements over VE00 include arrays taking
0.328x time, fields 0.469x, string constants 0.298x, arithmetic 0.720x and calls
0.823x. These are whole-track changes, not effects attributable only to Value size.
Full GitHub CI feature/backend acceptance remains unrun. The subsequently authorized
[VE09 follow-up](interpreter-value-execution-plan.md#ve09-native-result-layout-reuse)
is completed above; its improvement does not establish parity.

### Environment and measurement scope

Baseline is the preserved VE00 executable `target/ve00/baseline-executable`, SHA-256
`60aa2c07334fb25123208c8c77a4008d1bc93d2f680da6ac21560838e7ebfc81`.
Candidate is `target/ve08/candidate-executable`, SHA-256
`98a14ea576a1a4f616bb6573c8e7342f01148dbbeac70147753269d08edf2261`.
Its production revision is VE07 `0a6c34f0`; VE08 changes only integration fixtures
and documentation. All three matrices used this same candidate binary.

Apple M1 Max, 10 logical CPUs, 32 GiB, macOS 26.6.2 arm64; Rust 1.98.1
(`48a229cea`, LLVM 22.1.8), Cargo 1.98.1; workspace release defaults, default target
and Cargo parallelism, source/native SDK features, mlua lua54,vendored PUC Lua 5.4.8.
Only the interpreter is timed. Each matrix runs baseline,candidate,candidate,baseline
sequentially, with fresh process/state, three warmups per route and eleven measured
samples per process. Engine order rotates; the second pair reverses initial order.
Each variant/engine/workload has 22 samples. All 2,728 measured execution batches
across 31 workloads pass checksum validation; warmups are checked separately.

No compilation, tests, allocation probes or profilers ran concurrently with final
throughput. Builds reused the release cache and took 0.123 / 0.080 / 0.092 s for
original/forms/numeric, excluded from execution. Source compilation, verification,
runtime setup and linking are excluded; public host entry/return and ordinary GC
remain included. CPU frequency, affinity and background activity are uncontrolled.
Tables show microseconds per complete workload (entry normalized per call), C/B
candidate-to-baseline median ratio, and candidate sample extrema. Extrema are not
confidence intervals; small differences are not statistical significance claims.
Raw JSON/CSV retain both variants' ranges and separate setup samples.

### Final original workloads

The six nontrivial rows belong to the parity gate; entry measures the host boundary
separately. Algorithms, inputs and explicit while loops are unchanged from VE00.

| Workload | VE00 VM | Final VM | C/B | Candidate Lua | VM/Lua | Final VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 3438.229 | 2473.874 | 0.720 | 383.812 | 6.45 | 2448.333–2566.875 |
| arrays | 15296.376 | 5016.583 | 0.328 | 67.729 | 74.07 | 4969.792–5378.792 |
| branches | 3987.709 | 2925.938 | 0.734 | 801.917 | 3.65 | 2919.833–2945.292 |
| calls | 7417.396 | 6101.645 | 0.823 | 222.020 | 27.48 | 6059.500–6221.875 |
| entry | 1.455 | 1.409 | 0.968 | 0.028 | 49.60 | 1.403–1.415 |
| fibonacci | 15097.021 | 12413.729 | 0.822 | 352.021 | 35.26 | 12343.708–12730.042 |
| maps | 13961.312 | 12398.000 | 0.888 | 65.833 | 188.33 | 12324.584–12483.291 |

### Final source forms

The ten script forms belong to the parity gate alongside the six original cases.
`host_callback` compares the same Rust body through each host adapter separately;
`native` compares a Kagari Rust callback with a Lua script helper and is diagnostic.
All use 5,000 iterations. String routes alternate the unchanged 62/68-byte ASCII
constants and check 325,000 bytes; they do not compare Unicode character counts.
Shared generic/interface rows compare algorithms, not equivalent type systems.

| Workload | VE00 VM | Final VM | C/B | Candidate Lua | VM/Lua | Final VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| forms_byte_state | 7693.312 | 6851.208 | 0.891 | 109.604 | 62.51 | 6841.417–6942.167 |
| forms_capture_cell | 11002.834 | 10059.354 | 0.914 | 139.875 | 71.92 | 10025.958–10109.500 |
| forms_concrete_generic | 3622.916 | 3047.646 | 0.841 | 103.480 | 29.45 | 3037.667–3086.375 |
| forms_direct | 470.354 | 336.041 | 0.714 | 75.125 | 4.47 | 335.334–351.375 |
| forms_field | 7951.146 | 3725.791 | 0.469 | 90.145 | 41.33 | 3681.333–3849.625 |
| forms_helper | 3589.375 | 3030.062 | 0.844 | 139.896 | 21.66 | 3020.666–3049.084 |
| forms_host_callback | 2081.041 | 1892.250 | 0.909 | 253.167 | 7.47 | 1885.667–1901.709 |
| forms_interface | 10748.812 | 10429.708 | 0.970 | 134.480 | 77.56 | 10312.708–10779.500 |
| forms_native | 2081.166 | 1904.854 | 0.915 | 140.520 | 13.56 | 1892.625–1911.792 |
| forms_shared_generic | 21666.896 | 21386.646 | 0.987 | 106.146 | 201.48 | 21226.667–21479.916 |
| forms_string_calls | 30427.709 | 12702.312 | 0.417 | 123.063 | 103.22 | 12681.958–13175.917 |
| forms_string_constants | 23424.625 | 6974.895 | 0.298 | 66.438 | 104.98 | 6952.000–7161.708 |

### Final numeric diagnostics

The 12 numeric routes use bounded exact inputs with 20,000 iterations. Kagari uses
while and these Lua fixtures use numeric for; their ratios are diagnostic and are
excluded from the frozen matched parity gate. They do not establish equivalent
full integer/float domains or checked overflow. No fixture was changed to improve
these ratios. Numeric semantic/source-free contracts are checked separately.

| Workload | VE00 VM | Final VM | C/B | Candidate Lua | VM/Lua | Final VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| numeric_f32 | 2544.667 | 1879.708 | 0.739 | 443.125 | 4.24 | 1841.667–1937.125 |
| numeric_f64 | 2508.833 | 1826.458 | 0.728 | 443.688 | 4.12 | 1807.459–1939.542 |
| numeric_i16 | 2904.834 | 2088.417 | 0.719 | 581.625 | 3.59 | 2051.959–2144.584 |
| numeric_i32 | 2843.334 | 2062.625 | 0.725 | 574.229 | 3.59 | 2047.958–2077.917 |
| numeric_i64 | 2867.938 | 2056.062 | 0.717 | 574.375 | 3.58 | 2044.000–2088.542 |
| numeric_i8 | 2866.854 | 2087.791 | 0.728 | 574.104 | 3.64 | 2062.416–2115.792 |
| numeric_isize | 2865.708 | 2052.104 | 0.716 | 574.250 | 3.57 | 2043.958–2094.958 |
| numeric_u16 | 2833.188 | 2059.041 | 0.727 | 574.145 | 3.59 | 2056.333–2092.375 |
| numeric_u32 | 2815.292 | 2065.292 | 0.734 | 574.000 | 3.60 | 2056.333–2095.208 |
| numeric_u64 | 2793.709 | 2072.667 | 0.742 | 574.396 | 3.61 | 2064.125–2095.292 |
| numeric_u8 | 2840.812 | 2064.166 | 0.727 | 574.333 | 3.59 | 2056.375–2080.417 |
| numeric_usize | 2793.854 | 2089.604 | 0.748 | 576.812 | 3.62 | 2071.333–2172.166 |

### Memory, attribution and correctness

The 16-byte value does not halve total memory: strings, tuples, ranges and ephemeral
host payloads now have separately traced backing allocations; retained host handles
remain non-Copy. The representation section below retains the backing-layout and
allocation tradeoffs; intermediate VE05 probes remain in the historical report.
No whole-process memory reduction is claimed. VE07's independent counts still describe the identical final
production binary: arithmetic 550,015 Kagari / 250,007 Lua canonical operations;
direct 75,015 / 40,006; helper 105,012 / 65,007. These scalar probes allocate no
script GC objects. Generated-code and wall-clock sample evidence remain in the
historical VE07 report; counters/profiling are separate from throughput.

Final `cargo test --workspace --no-fail-fast` passes all unit, integration and doc-test
targets after correcting old fixture assumptions about inline strings and live
module constant caches. Exact final zero-root/object assertions remain. Workspace
all-target Clippy passed, followed by affected-target Clippy after fixture fixes;
formatting, structure (988 Rust files, no violations/exceptions) and diff checks
pass. Logs are under `target/ve08/`. This is local default source/native acceptance,
not an unrun GitHub CI/backend matrix result. No production API or semantic change
was needed for VE08 corrections; no carried build/test failures remain.

### Final reproduction and raw reports

Raw directories under `target/lua-comparison/`:

- `20261009T184853Z-paired/` (original).
- `20261009T185103Z-forms-paired/` (source forms).
- `20261009T185119Z-numeric-paired/` (numeric diagnostics).

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/ve00/baseline-executable
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/ve00/baseline-executable
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --numeric-matrix --baseline-executable target/ve00/baseline-executable
```

Ignored binaries/raw logs are local reproduction aids, not durable release assets.
The committed tables, hashes, environment and commands preserve the conclusions.

## Representation and allocation tradeoffs (VE04)

Measured VE04 release/debug Value is 16 bytes and Copy, HeapObjectId is 12, HeapObject
is 128, and a complete ObjectSlot is 168. TupleData is 32 plus 16 bytes per member;
String control storage is 24 plus text capacity; RangeValue is 24. RootedValue
remains a 32-byte owning lease. Host descriptors remain 40/104/48-byte payloads
behind compact checked IDs. These are implementation layout observations, not
portable ABI sizes, retained-memory measurements or RSS. Scalar banks already
stored raw 64-bit payloads and are not halved by the Value change.

The unchanged allocator probe runs one 5,000-iteration public execution after
three warmups and checks 325000. Counts cover Rust alloc/alloc_zeroed/realloc and
requested bytes, not just strings or heap nodes:

| Workload | Baseline calls → candidate calls | Baseline bytes → candidate bytes | Baseline heap allocations / collections → candidate |
| --- | ---: | ---: | ---: |
| string constants/length | 480007 → 7 | 50155179 → 163 | 0 / 0 → 0 / 0 |
| string function transfer/length | 500007 → 7 | 51455175 → 159 | 0 / 0 → 0 / 0 |
| tuple construction/member read | 35007 → 5315 | 1680179 → 1175451 | 0 / 0 → 5000 / 14 |
| one-element range traversal | 1540708 → 1480704 | 107233227 → 106005195 | 15000 / 25 → 20000 / 29 |

Strings now materialize once per runtime/module version, and standard readonly
string inputs no longer decode to owned Rust Strings. Warmed loads/transfers copy
no text. The seven remaining allocations occur at the complete execution boundary.
Tuples/ranges now have GC records: this is a real added collector cost, not a hidden
zero-allocation claim. Tuple copying no longer duplicates membership, giving fewer
Rust requests/bytes; each range construction adds one record, not one per iterator
step. Range iterator/Option/native costs still dominate its allocation workload.
No new collector or general enum-unboxing work was added to explain these results.

The comparison is VE00 -> VE04 on the M1 Max/Rust 1.98.1 release setup recorded
above, using a separate counting allocator and serial execution with ordinary GC.
Baseline is `target/ve00/measure_source`, candidate `target/ve03/measure_source`
(the same runtime accepted in VE04); sources are `strings.kgr` and
`tuple-range.kgr` under `target/ve00/`. The latter repeats `(i,65)` member reads
and one-element range traversal 5,000 times, adding 65 per iteration. Counts include
public execution boundaries and exclude compilation/setup; requested bytes are not
RSS. Rebuild historical probes from their matching production revisions. The full
VE04 record at `0f6252f8` retains raw paths and intermediate comparison tables.

## Compact value baseline (VE00), 2026-10-09

Production revision: `19fe129d` (runtime unchanged from `f521b2f0`) plus the
VE00 benchmark additions. Frozen workloads and representation decisions belong to
[the execution plan](interpreter-value-execution-plan.md#implemented-representation-and-boundaries).
The baseline executable was preserved before runtime edits at
`target/ve00/baseline-executable`, SHA-256
`60aa2c07334fb25123208c8c77a4008d1bc93d2f680da6ac21560838e7ebfc81`.
The final benchmark import-style repair does not alter any workload or runtime.

Environment: Apple M1 Max, 10 logical CPUs, 32 GiB RAM, macOS 26.6.2 arm64;
Rust 1.98.1 (`48a229cea`), LLVM 22.1.8, Cargo 1.98.1, default workspace release
profile, target and Cargo parallelism. SDK source/native and mlua lua54/vendored
features; locked PUC Lua 5.4.8; interpreter execution only. Command environment:
`DEVELOPER_DIR=/Library/Developer/CommandLineTools`; no custom compiler flags,
CPU target or jobs. Initial cold release build took 69 seconds; warm driver
builds took 0.083–0.104 seconds. Builds are excluded from execution.

Reproduce with `uv run python scripts/benchmark_lua.py --interpreter-only`,
then separately with `--source-forms` and `--numeric-matrix`, using the environment
above. Each route has three warmups and eleven checked samples in each of two
fresh sequential processes; the second reverses order. Ordinary GC remains enabled.
Compilation, verification and linking are outside execution; host entry/return
remain inside. No agent-started builds/tests/profilers ran during throughput
measurement. CPU frequency, affinity and unrelated background activity were not
controlled. All 1,364 measured execution batches passed independent checksums;
none were discarded. Allocation observations use separate instrumented runs.

The final VE08 report above compares this same preserved binary with the completed
track. Original one-version tables and representation probes remain in the historical
report at `29a5df9b`; entry is a separate boundary diagnostic.

## Historical interpreter checkpoints

Intermediate tables, rejected candidates, assembly/profiler paths and successful
command logs are retained in Git history, not repeated in the current report.
Use `git show 9dfeba3c:docs/performance-baseline.md` for the complete pre-cleanup
measurement record, or the phase commits below for their original context. These
are different revisions/workloads, not additional measurements of today's tree.

| Track | Historical evidence and checkpoint |
| --- | --- |
| IP00-IP04 | Post-GO06 baseline, execution windows, compact instructions, register/call windows and native preparation; final `3d3fb624`. |
| NE01-NE05 | Typed numeric baseline, closed regions, scalar banks/kernels and final numeric acceptance; final `da2a2d5c`. |
| VE00 | Preserved executable and representation choice, `29a5df9b`; current baseline identity retained above. |
| VE04 | Value/string/GC tradeoffs, `0f6252f8`; selected allocation evidence retained above. |
| VE05 | Prepared fields and scoped collections, `263499b5`; separate Vec/Map allocation probes. |
| VE06 | Prepared calls/frame retirement, `13c151ef`; call counts and added metadata cost. |
| VE07 | Borrowed scalar segments, `0a6c34f0`; code/sample evidence and unchanged canonical instruction counts. |

The final VE08/VE09 reports retain all matched workloads, regressions, measurement
conditions and open parity gates. Historical experiments never establish current
performance or unrun CI acceptance.

## Windows arithmetic comparison, 2026-10-07

Only the unchanged arithmetic workload was measured: 50,000 iterations of
`sum += i % 97; i += 1`, with result 2,398,830. Current worktree over `ef080344`
includes the focused benchmark selector and existing uncommitted frontend edits;
raw metadata and `worktree.diff` retain that state. This is a fresh Windows
observation, not a regression comparison with the M1 Max measurements.

Environment: Intel Core i9-12900K, 16 cores/24 logical CPUs, about 64 GiB RAM,
Windows 11 10.0.26300, Rust/Cargo 1.99.0, LLVM 23.1.1,
x86_64-pc-windows-msvc, workspace release/default target/default Cargo parallelism,
opt-level=3/default CPU target. SDK source/native features and mlua 0.11.6
lua54/vendored are enabled; native preparation/execution is disabled. The initial
release build is cold and takes 128.739 seconds, excluded from execution.
Lua source is the locked `lua-src` product recorded in `Cargo.lock`.

| Route | Median us per complete loop | Sample range us |
| --- | ---: | ---: |
| Kagari interpreter | 8648.300 | 5697.700-17582.000 |
| PUC Lua 5.4 | 630.650 | 597.800-3807.700 |
| Rust ordinary release | 19.639 | 18.445-21.227 |
| Rust explicit checked additions | 22.371 | 20.916-25.814 |

Kagari/Lua pooled median ratio is **13.71**. Two fresh sequential processes
perform three warmups per route and eleven single-call timed samples per route,
with rotating engine order and reversed second-process order. All 44 timed results
pass the independent Rust reference checksum. Per-process Kagari/Lua medians are
7,491.900/623.300 and 8,766.600/655.900 us. Source compilation, artifact preparation,
runtime construction and linking are outside timing; public host entry/return and
default GC remain included. No agent build/check/profile runs concurrently with
execution. Desktop activity, core placement and frequency are uncontrolled;
large sample ranges prevent treating the reported ratio as a precise constant.

Rust is rerun afterwards in two sequential processes: 20 warmups per route,
eleven batches of 1,000 calls per route/process, non-inlined functions and runtime
input/result `black_box`, all batch checksums validated. No dependencies/features;
standalone `rustc -C opt-level=3`, default CPU target, existing executable/cache.
Its public function boundary differs from the embedding APIs; all intermediates
fit i32. Earlier standalone medians were 19.148/22.201 us on the same machine.

Reproduce the focused Kagari/Lua run:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --workload arithmetic
```

Raw CSV, hashes/environment, setup timings, summary and Rust rerun samples are in
ignored `target/lua-comparison/20261007T091754Z/`; Rust source/executable and original
methodology remain under `target/rust-loop-baseline/`. The driver's original final
printing failed after saving all samples/JSON because it still expected seven
workloads; focused printing was corrected and replayed against the retained report.
No timed batch was rerun for that correction. Release build, Rust formatting,
structure audit (921 files, no violations), focused CLI/summary checks and diff
checks pass; no workspace test suite was run.

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
