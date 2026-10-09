# Performance Measurements

Keep workload, baseline/candidate, environment, timing scope and reproduction
together. These are finite observations, not language-wide guarantees. Build,
preparation and execution times are separate; allocation requests are not RSS.
Older superseded tables and successful test logs remain in Git history.
Historical sections were not rerun by the documentation cleanup. The post-GO06
interpreter section is a new measurement on its explicitly recorded revision.

## Compact values and shared constants (VE04), 2026-10-10

VE00-VE03 reduce Value from 32 to 16 bytes and make it Copy without per-copy
ownership work. Paired execution shows string-constant and string-call workloads
at 29.7% and 41.9% of the preserved baseline time, respectively. Arrays improve
9.3%; most other mixed routes improve 2–9%. Numeric routes are 0.1–2.0% slower;
this migration does not improve the scalar execution loop. Lua parity is still
open: the first value/reference milestone is not final interpreter acceptance.

Candidate production revision: `511e40a2` (VE04 changes only integration fixtures
and documentation). Candidate executable SHA-256:
`88440efb52b9d16ea3f87ab38458a24c7896fd571269f4196651e3b261e693d0`.
The preserved VE00 executable remains
`60aa2c07334fb25123208c8c77a4008d1bc93d2f680da6ac21560838e7ebfc81`.
The benchmark sources, inputs, lockfile and driver are unchanged from VE00.

Environment: Apple M1 Max, 10 logical CPUs, 32 GiB RAM, macOS 26.6.2 arm64;
Rust 1.98.1 (`48a229cea`), LLVM 22.1.8, Cargo 1.98.1. Workspace release profile,
default target directory and Cargo parallelism; SDK source/native and mlua
lua54/vendored features, locked PUC Lua 5.4.8, interpreter-only execution.
`DEVELOPER_DIR=/Library/Developer/CommandLineTools`; no custom jobs or flags.
The first release rebuild took 28.173 s; subsequent driver builds reused it in
0.084/0.079 s. Builds and source/verification/link preparation are excluded from
execution, while public host entry/return and normal GC remain included.

Each matrix uses baseline,candidate,candidate,baseline sequential fresh processes,
three warmups and eleven checked samples per route/process; the second pair reverses
workload/engine order. There are 22 samples per variant/engine/workload and all
2,728 measured execution batches pass independent checksums. No build, test or
profiler ran concurrently with throughput timing. CPU frequency, affinity and
unrelated background activity remain uncontrolled. Allocation counts below come
from separate instrumented executions and are not used as throughput samples.

Tables show microseconds per complete workload; entry is per call from 1,000-call
batches. C/B is candidate VM / preserved baseline VM (lower is better); VM/Lua
uses Lua from the candidate process. Small differences are reported without a
statistical-significance claim. Numeric and branch/entry regressions are retained,
not discarded or hidden by the smaller Value.

### Original suite

| Workload | Baseline VM | Candidate VM | C/B | Candidate Lua | VM/Lua | Candidate VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 3472.312 | 3421.354 | 0.985 | 390.812 | 8.75 | 3407.625–3449.292 |
| arrays | 15550.416 | 14109.792 | 0.907 | 67.188 | 210.01 | 13987.083–14627.834 |
| branches | 4011.688 | 4061.624 | 1.012 | 819.438 | 4.96 | 4038.375–4102.083 |
| calls | 7369.188 | 7145.937 | 0.970 | 225.771 | 31.65 | 7127.583–7289.334 |
| entry | 1.450 | 1.462 | 1.008 | 0.029 | 50.07 | 1.457–1.495 |
| fibonacci | 15237.479 | 14654.854 | 0.962 | 350.583 | 41.80 | 14575.667–14700.666 |
| maps | 14263.875 | 13678.396 | 0.959 | 66.730 | 204.98 | 13573.875–13861.083 |

### Source forms

| Workload | Baseline VM | Candidate VM | C/B | Candidate Lua | VM/Lua | Candidate VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| forms_byte_state | 7748.229 | 7173.021 | 0.926 | 111.105 | 64.56 | 7086.333–7270.375 |
| forms_capture_cell | 11108.041 | 10127.521 | 0.912 | 143.458 | 70.60 | 10081.000–10459.291 |
| forms_concrete_generic | 3707.854 | 3533.229 | 0.953 | 102.770 | 34.38 | 3519.125–3704.708 |
| forms_direct | 481.896 | 464.250 | 0.963 | 75.084 | 6.18 | 460.959–536.250 |
| forms_field | 8018.729 | 7699.958 | 0.960 | 89.958 | 85.60 | 7685.834–7769.417 |
| forms_helper | 3673.000 | 3485.146 | 0.949 | 140.292 | 24.84 | 3471.500–3533.084 |
| forms_host_callback | 2088.021 | 1950.625 | 0.934 | 248.604 | 7.85 | 1946.334–1996.541 |
| forms_interface | 10820.854 | 10431.854 | 0.964 | 133.729 | 78.01 | 10312.292–10577.167 |
| forms_native | 2109.541 | 1949.104 | 0.924 | 140.250 | 13.90 | 1945.916–1971.458 |
| forms_shared_generic | 21832.312 | 21420.604 | 0.981 | 106.500 | 201.13 | 21227.750–21674.750 |
| forms_string_calls | 30389.375 | 12745.084 | 0.419 | 125.209 | 101.79 | 12679.459–13356.750 |
| forms_string_constants | 23347.500 | 6945.729 | 0.297 | 66.562 | 104.35 | 6922.000–7166.458 |

### Bounded numeric matrix

| Workload | Baseline VM | Candidate VM | C/B | Candidate Lua | VM/Lua | Candidate VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| numeric_f32 | 2566.271 | 2569.646 | 1.001 | 441.125 | 5.83 | 2543.875–2582.959 |
| numeric_f64 | 2522.104 | 2546.438 | 1.010 | 443.604 | 5.74 | 2503.334–2665.542 |
| numeric_i16 | 2838.229 | 2882.333 | 1.016 | 573.437 | 5.03 | 2854.625–2895.291 |
| numeric_i32 | 2836.688 | 2860.083 | 1.008 | 573.355 | 4.99 | 2834.209–2927.292 |
| numeric_i64 | 2801.791 | 2838.729 | 1.013 | 575.688 | 4.93 | 2823.333–2862.875 |
| numeric_i8 | 2863.876 | 2889.125 | 1.009 | 576.333 | 5.01 | 2867.125–2914.291 |
| numeric_isize | 2816.563 | 2842.021 | 1.009 | 573.771 | 4.95 | 2828.167–2868.083 |
| numeric_u16 | 2814.854 | 2846.001 | 1.011 | 573.458 | 4.96 | 2837.750–2895.584 |
| numeric_u32 | 2829.312 | 2853.209 | 1.008 | 574.083 | 4.97 | 2842.000–2883.750 |
| numeric_u64 | 2805.875 | 2859.709 | 1.019 | 573.854 | 4.98 | 2850.250–2959.333 |
| numeric_u8 | 2805.959 | 2850.376 | 1.016 | 574.542 | 4.96 | 2835.042–3026.375 |
| numeric_usize | 2808.021 | 2863.562 | 1.020 | 576.562 | 4.97 | 2847.500–2886.083 |

The native source-form row remains unmatched (Rust callback versus Lua helper).
The host_callback row is matched within each binary. Its Lua time changes from
383.417 to 248.604 us between binaries. Both adapters call the same Rust `host_step`,
which uses Kagari Value/numeric operations affected by this migration, so the Lua
row also includes changed Kagari Rust code. The callback's implementation source
is unchanged; this does not isolate a Lua VM or callback-body speedup. Report the
candidate ratio 7.85, including its current callback cost,
rather than combining the new Kagari measurement with the old Lua callback cost.
Shared-generic Add remains excluded for the pre-existing frontend error; the frozen
shared-generic identity/default-method row does not establish Add support.

### Representation and allocation

Current release/debug Value is 16 bytes and Copy, HeapObjectId is 12, HeapObject
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

### Reproduction and acceptance

Run sequentially with the environment above:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/ve00/baseline-executable
uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/ve00/baseline-executable
uv run python scripts/benchmark_lua.py --numeric-matrix --baseline-executable target/ve00/baseline-executable
```

Full raw samples, setup phases, process order, machine/toolchain, source/binary
hashes and build logs are in `target/lua-comparison/20261009T163933Z-paired/`,
`20261009T164241Z-forms-paired/` and `20261009T164314Z-numeric-paired/`.
The original suite records engine/runtime initialization, source-to-artifact,
artifact preparation and program linking as separate phases. Expanded matrices
prepare one module outside execution. None of these setup costs were subtracted
from only one engine's timed execution.

Allocation probes are `target/ve00/measure_source` (preserved baseline) and
`target/ve03/measure_source` (release-linked candidate); use the same `strings.kgr`
and `tuple-range.kgr` inputs under `target/ve00/`. The latter constructs `(i,65)`
and reads `pair[1]` 5000 times, then separately traverses `i..(i+1)` 5000 times,
adding 65 each time. Probe aliases string_constants/string_calls name tuple/range
in that input. Candidate logs are `target/ve03/string-allocations.log` and
`target/ve04/aggregate-allocations.log`; link commands are recorded by
`target/ve03/build_probe.py`/`probe-build.log`.

The [VE04 ledger](interpreter-value-execution-plan.md#ve04-completed-integration-and-performance-gate)
records focused correctness, lifetime, source-free and backend checks separately.
The compact value/reference gate passes locally: no material unexplained throughput
regression, reduced allocation traffic, and explicit accounting for added tuple/range
records. This does not establish a scalar speedup, complete GitHub CI acceptance or
Lua parity. VE05–VE07 still own prepared object/collection/call and execution-loop
costs; the end-to-end ratios above remain the full-plan performance obligation.

## Compact value baseline (VE00), 2026-10-09

Production revision: `19fe129d` (runtime unchanged from `f521b2f0`) plus the
VE00 benchmark additions. Frozen workloads and representation decisions belong to
[the execution plan](interpreter-value-execution-plan.md#2026-10-09-ve00-acceptance).
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
none were discarded. The allocation probe below is a separate instrumented run.

Medians and ranges below are microseconds per complete workload. Entry is per
call from 1,000-call batches and is a separate boundary diagnostic.

### Original suite

| Workload | VM median | Lua median | VM/Lua | VM min–max | Lua min–max |
| --- | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 3413.667 | 384.479 | 8.88 | 3371.500–3500.792 | 378.375–404.042 |
| arrays | 16184.771 | 74.250 | 217.98 | 15356.125–19968.792 | 67.375–105.208 |
| branches | 3961.291 | 806.958 | 4.91 | 3932.500–4020.417 | 791.417–856.792 |
| calls | 7318.688 | 223.625 | 32.73 | 7189.625–17463.750 | 220.666–235.083 |
| entry | 1.444 | 0.029 | 50.64 | 1.440–1.508 | 0.028–0.029 |
| fibonacci | 15410.584 | 353.021 | 43.65 | 14969.416–39779.666 | 343.875–805.500 |
| maps | 14228.104 | 74.020 | 192.22 | 14032.791–31550.583 | 65.834–93.167 |

### Source forms

| Workload | VM median | Lua median | VM/Lua | VM min–max | Lua min–max |
| --- | ---: | ---: | ---: | ---: | ---: |
| forms_byte_state | 7727.396 | 111.624 | 69.23 | 7682.375–7862.833 | 108.708–133.084 |
| forms_capture_cell | 11065.500 | 137.708 | 80.35 | 10971.417–11408.208 | 131.333–161.625 |
| forms_concrete_generic | 3663.229 | 103.042 | 35.55 | 3626.375–3715.709 | 98.667–105.625 |
| forms_direct | 467.521 | 75.083 | 6.23 | 465.375–542.625 | 74.916–94.417 |
| forms_field | 8015.750 | 90.355 | 88.71 | 7962.084–8259.042 | 89.792–94.041 |
| forms_helper | 3627.021 | 140.250 | 25.86 | 3610.417–3695.375 | 128.250–145.250 |
| forms_host_callback | 2091.624 | 383.791 | 5.45 | 2077.208–2156.625 | 383.250–390.666 |
| forms_interface | 10861.916 | 134.187 | 80.95 | 10786.625–10958.083 | 133.250–149.250 |
| forms_native | 2088.000 | 139.105 | 15.01 | 2071.125–2123.875 | 130.083–163.333 |
| forms_shared_generic | 21813.041 | 107.375 | 203.15 | 21722.500–22383.709 | 105.666–136.125 |
| forms_string_calls | 30471.750 | 125.395 | 243.01 | 30303.458–30570.292 | 122.792–130.250 |
| forms_string_constants | 23699.729 | 68.376 | 346.61 | 23331.583–24243.708 | 66.250–81.083 |

### Bounded numeric matrix

| Workload | VM median | Lua median | VM/Lua | VM min–max | Lua min–max |
| --- | ---: | ---: | ---: | ---: | ---: |
| numeric_f32 | 2559.646 | 444.354 | 5.76 | 2533.959–2700.167 | 440.500–477.042 |
| numeric_f64 | 2502.666 | 442.229 | 5.66 | 2491.500–2558.167 | 440.166–461.958 |
| numeric_i16 | 2827.083 | 575.188 | 4.92 | 2802.750–2989.917 | 572.375–663.458 |
| numeric_i32 | 2830.271 | 580.875 | 4.87 | 2779.875–3018.250 | 572.667–641.000 |
| numeric_i64 | 2785.771 | 573.938 | 4.85 | 2768.959–2981.833 | 572.708–615.791 |
| numeric_i8 | 2844.187 | 574.542 | 4.95 | 2810.625–2961.959 | 572.500–608.041 |
| numeric_isize | 2863.416 | 584.355 | 4.90 | 2794.083–2896.958 | 574.000–595.125 |
| numeric_u16 | 2808.312 | 590.042 | 4.76 | 2788.042–2898.250 | 573.875–618.917 |
| numeric_u32 | 2818.688 | 591.062 | 4.77 | 2780.209–2929.625 | 572.375–608.958 |
| numeric_u64 | 2822.916 | 575.542 | 4.90 | 2784.708–2878.583 | 572.333–594.708 |
| numeric_u8 | 2816.188 | 577.250 | 4.88 | 2783.625–2898.458 | 572.375–605.791 |
| numeric_usize | 2800.834 | 575.062 | 4.87 | 2786.708–2839.625 | 572.541–596.875 |

The `native` row remains unmatched (Rust callback versus Lua script helper).
The new `host_callback` row invokes the same Rust checked-arithmetic implementation
through both adapters. String cases alternate 62/68 ASCII bytes for 5,000
iterations, result 325,000; string_calls adds script argument/return transfer.
Numeric rows cover bounded exactly representable values, not full-domain Lua
compatibility. Shared generic Add default-method compilation was rechecked and
still fails MIR lowering with `MissingBinding("checked callable requirement")`;
it is excluded, not reported as supported by the identity default method.

### Representation and allocation diagnostics

A release-linked layout probe reports: Value 32 bytes, HeapObjectId 24,
RangeValue 24, EphemeralValue 16, RootedValue 32, HostRootHandle 40,
HostPathViewHandle 104 and FrameHostBorrowToken 48. These are headers, excluding
String/Vec/Arc backing allocations. Candidate enums with a u64 scalar and either
a direct reference or a 12-byte (u32 owner/slot/generation) index are both 16 bytes.

A bounded storage probe uses 8,192 72-byte nodes, eleven alternating samples,
two million permuted payload accesses, and 100 mark/sweep-scan passes retaining
every second node. Direct references borrow individually boxed stable nodes;
indices address a contiguous Vec. Host admission checks owner, bounds and generation
for both. The admitted-access case assumes those checks and retention were already
established; indexed access still uses safe bounds checking. Allocation includes
creation and destruction. Marking uses a per-node Cell; sweep is a scan/reset, not
the production collector, metadata graph or native destructor protocol.

| Probe | Boxed direct median µs | Compact index median µs |
| --- | ---: | ---: |
| Allocate/drop 8,192 nodes | 203.750 | 12.000 |
| Two million admitted reads | 1,900.833 | 2,304.125 |
| Two million checked host admissions/reads | 2,648.000 | 2,198.875 |
| 100 root-mark/sweep scans | 1,347.042 | 1,251.958 |

Direct access is cheaper in this probe; it does not establish that a stable slab
allocator or a whole direct-reference interpreter would have these costs. The
selected compact index avoids per-object boxes and new unsafe lifetime contracts,
while meeting the measured candidate value size. This is a design tradeoff,
not an interpreter speedup claim. VE04 must measure the actual implementation.
Probe source/results: `target/ve00/references.rs`, `references.csv`, `layout.log`;
compiled with `rustc --edition=2024 -O`, linked to the baseline release runtime.

A separate global allocator probe wraps one warmed public execution after three
warmups, counting alloc/alloc_zeroed/realloc calls and requested sizes. It uses the
same string function bodies and checks result 325,000. string_constants requests
480,007 allocations/reallocations and 50,155,179 bytes; string_calls requests
500,007 and 51,455,175 bytes. Both report zero script GC allocations/collections.
Counts include runtime, type/native conversion and root bookkeeping; they are not
counts of strings alone, retained memory, or RSS. Source inspection confirms each
string LoadConst clones owned text and native owned String conversion copies again.
The large non-GC allocation count makes shared string storage and read-only native
access separate requirements. Raw instrumented source/output:
`target/ve00/measure_source.rs`, `strings.kgr`, `string-allocations.log`.

Raw throughput metadata/samples: `target/lua-comparison/20261009T152106Z/`,
`20261009T152231Z-forms/` and `20261009T152327Z-numeric/`. The benchmark package,
lockfile, driver and binary hashes are included in each report. VE00 adds baseline
and diagnostic evidence only; no production speedup or Lua parity is claimed.

## Windows arithmetic comparison, 2026-10-07

Only the unchanged arithmetic workload was measured: 50,000 iterations of
`sum += i % 97; i += 1`, with result 2,398,830. Current worktree over `ef080344`
includes the focused benchmark selector and existing uncommitted frontend edits;
raw metadata and `worktree.diff` retain that state. This is a fresh Windows
observation, not a regression comparison with the M1 Max report below.

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

## Typed numeric execution result (NE05), 2026-10-06

NE01-NE05 implementation is complete; **Lua parity acceptance is not met**.
The candidate is the NE05 diff over `08afe47b`, following NE01 `631e4806`, NE02
`d0e80d68` and NE03 `1bb8a33d`. The original production executable remains SV01
`ba170cc7`; it was copied before changes and its SHA-256 is recorded in both reports.
No benchmark source from the original seven workloads changed. Canonical source
semantics, checked domains, roots, generation checks and cancellation remain active.

Reproduce after preserving the original release executable:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/lua-comparison/20261006T073128Z/baseline-executable
uv run python scripts/benchmark_lua.py --numeric-matrix
uv run python scripts/benchmark_lua.py --source-forms
```

Raw metadata, source/binary hashes, all samples and preparation timings are in
`target/lua-comparison/20261006T100030Z-paired/`,
`target/lua-comparison/20261006T100218Z-numeric/` and
`target/lua-comparison/20261006T100224Z-forms/`. Environment: Apple M1 Max,
10 logical CPUs, 32 GiB, macOS 26.6.2, Rust/Cargo 1.98.1, aarch64/LLVM 22.1.8,
workspace release opt-level=3/default target/default Cargo parallelism, warm build
cache, SDK source/native features, native preparation/execution disabled, PUC Lua
5.4.8 via mlua 0.11.6/vendored. All workspace tests, standalone feature consumers
and CLI checks finished before timing. No measuring agent build/test/profile was
concurrent. Desktop scheduling, core placement and frequency remain uncontrolled.

The original suite uses four fresh sequential processes in baseline, candidate,
candidate, baseline order. Each variant has two processes, three warmups per route
and eleven samples per process, with rotating engine order and reversed second
pair. All **616 checked timed batches** pass. Numeric and source-form matrices use
two fresh processes each and pass **528/396 checked timed batches**. Build times
2.232/0.085/0.074 seconds are excluded, as are source compilation, verification,
physical preparation, runtime construction and linking. Public host entry/return
and default GC are included. Sampling, observer counting and allocation accounting
are separate from these unprofiled timings.

Times are microseconds per complete workload (entry per call):

| Workload | Paired original VM | NE05 VM | Candidate Lua | Original/NE05 | NE05/Lua |
| --- | ---: | ---: | ---: | ---: | ---: |
| entry | 1.352 | 1.365 | 0.029 | 0.99x | 47.12 |
| arithmetic | 20,866.270 | 3,446.146 | 385.479 | 6.05x | 8.94 |
| branches | 22,812.667 | 3,995.209 | 823.125 | 5.71x | 4.85 |
| calls | 12,333.188 | 6,430.917 | 228.520 | 1.92x | 28.14 |
| fibonacci | 18,441.166 | 12,931.021 | 350.541 | 1.43x | 36.89 |
| arrays | 14,907.104 | 14,522.625 | 68.500 | 1.03x | 212.01 |
| maps | 14,867.250 | 13,708.125 | 66.646 | 1.08x | 205.69 |

Arithmetic and branches improve 6.05x/5.71x against fresh paired originals;
call/Fibonacci improvement is 1.92x/1.43x. Collections show no comparable architectural
speedup: arrays remain 14.52 ms and maps 13.71 ms, with map sample ranges overlapping
the original. Do not attribute their small differences exclusively to this track.
All nontrivial ratios remain above 1.0; none is near parity. Candidate arithmetic
samples span 3.420-3.482 ms versus Lua 0.382-0.422 ms. The gap is much larger than
observed variation, so no near-parity uncertainty claim is needed.

Across the paired original suite, source-to-artifact medians are 603.538/599.215 ms,
artifact verification/preparation 261.401/259.213 ms and program linking
2.292/2.260 ms (original/candidate, pooled across workloads and setups). These include
foundation setup and are not attributed to a tiny function's kernel selection alone.
No preparation speedup is claimed. The first expanded-form process records source
compilation 923.954 ms, artifact preparation 255.836 ms and link 2.681 ms. Numeric
source/preparation are 1018.549/263.445 ms; its link is outside execution but not
separately instrumented.

The 20,000-iteration numeric matrix covers all ten integer domains, f32 and f64,
checked arithmetic, casts, comparisons/loop control and mixed u32 shift counts.
Integer medians span 2.734-2.828 ms with VM/Lua ratios 4.74-4.83; f32/f64 are
2.561/2.522 ms, ratios 5.69/5.60. This fixture has no type fallback cliff. Its float
values stay exactly representable, and integer values stay inside every narrow
source domain; Lua ratios describe that intersection, not arbitrary f32 rounding
or checked-overflow equivalence. Full edge-domain semantics have separate tests.

Source forms use 5,000 iterations of the same changing bounded recurrence, with
independent Rust checksums and nearest equivalent Lua bodies:

| Form | VM us | Lua us | VM/Lua |
| --- | ---: | ---: | ---: |
| direct | 455.875 | 75.125 | 6.07 |
| helper | 3,200.896 | 141.625 | 22.60 |
| concrete_generic | 3,170.709 | 101.709 | 31.17 |
| native | 1,862.458 | 139.688 | 13.33 |
| interface | 9,976.792 | 133.292 | 74.85 |
| shared_generic | 20,607.396 | 107.520 | 191.66 |
| capture_cell | 9,653.541 | 137.042 | 70.44 |
| field | 6,870.042 | 90.062 | 76.28 |
| byte_state | 6,559.584 | 109.354 | 59.98 |

Direct, helper and concrete generic bodies consume the same prepared numeric
kernels, but helper/generic calls still cost roughly seven times the direct Kagari
loop. These are **real remaining source-form performance differences**, not a
claim that unifying kernels equalizes total execution. Interface/shared/closure
and field paths remain substantially more expensive. Rust native vs Lua function
calls and generic erasure have distinct boundary implementations; those ratios
are observations, not an equivalent native-ABI parity test. Shared generic uses
an identity default body with arithmetic outside it. A generic Add default body
currently fails source lowering with `MissingBinding("checked callable requirement")`;
that frontend gap is recorded in the roadmap and was not hidden by weakening checks.

The byte state machine uses casts for u8 wrapping, XOR/shifts and 256 bounded
memory slots; it checks checksum 621716 against Rust and Lua. 6.560 ms per 5,000
steps is about 0.762 million steps/second, versus Lua's 45.7 million. This is a
synthetic storage/dispatch workload, not a NES instruction/frame benchmark or a
claim of emulator realtime suitability.

Independent counts (`target/ne05-count-*.log`) retain original logical origins.
Arithmetic falls from **600,015 to 550,015** instructions (50,000 redundant local
loads removed), versus Lua 250,007. It still has fourteen logical/six physical
temporaries and three fixed named locals: nine scalar slots, no managed slots,
72 payload plus nine initialization bytes. Shared Location metadata is eight bytes
per logical location and active Window metadata remains 96 bytes. Frame headers,
retained capacity and program metadata are separate. Value/canonical/prepared/return
packet sizes are 32/136/24/32 bytes. No fake smaller handle or lossy float storage
is used. The isolated ten-second arithmetic count/sampling window performs zero
script heap allocations/collections.

Form counts are 75,015 direct, 105,012 helper, 95,015 concrete generic, 65,012
native, 110,014 interface, 95,017 shared generic, 115,015 capture-cell, 90,016 field
and 160,021 byte-state instructions. The helper and concrete Add callee remain in
the counted route; their calls were not optimized away. The shared generic body
has two managed slots and one scalar slot. Direct/helper/concrete/native calls
allocate zero script objects and collect zero times per accounting run; interface,
shared generic and field each allocate one script object, closure/cell two and
byte storage one. Shared generic collects fourteen times, byte state once. Its
metadata/GC cost is visible rather than mislabeled scalar arithmetic. Live-object
deltas can include collection of earlier warmup objects and are not allocation counts.

The O1 counting allocator test
`cargo test -p kagari-vm --test native_allocations -- --nocapture` proves 1,000 warmed
repeated/reordered scalar frame entries, execution and caller return writes have
zero Rust allocations/reallocations/deallocations. Existing 100,000 scalar native
calls and bulk sequence boundaries keep zero allocation assertions. These exclude
public host entry/root-lease setup and do not establish zero-allocation dynamic
interface dispatch; the existing interface allocation regressions remain intact.

Final sampling uses `uv run python scripts/profile_lua_macos.py arithmetic calls
arrays maps`, with raw output under
`target/lua-comparison/20261006T100405Z-macos-profile/`. Arithmetic samples concentrate
in the closed region, with cancellation and raw kernels visible and boxed numeric
helpers absent. This does not separate every inlined bank/dispatch check. Calls
retain substantial stack/session authority and frame-entry/return overhead. Arrays
show module retention/hashing and native conversion/type validation; maps retain
allocation/free and type/layout comparison costs. Per original workload, arrays
allocate one script object/collect twice, maps 2,001 objects/collect five times.
Their counts fall to 72,030/63,030, versus Lua 42,012/24,012. These samples support
separate ownership/call and native-result/collection follow-ups; they do not justify
changing GC semantics or weakening admission to chase a timing.

Final acceptance: workspace tests pass 1,911 tests (one existing manual benchmark
ignored), strict workspace/all-target Clippy, formatting, structure (918 Rust files,
zero violations/exceptions), standalone artifact/source/native/source+native graphs
and consumers, CLI jit tests and diff checks pass. The bounded implementation is
complete. Lua performance acceptance and the documented shared-bound frontend gap
remain open; another architecture track requires explicit activation.

## Typed numeric execution baseline (NE01), 2026-10-06

Before numeric execution changes, run
`uv run python scripts/benchmark_lua.py --interpreter-only` on `ba170cc7`.
Raw metadata and two-process/308-batch checked results are under
`target/lua-comparison/20261006T073128Z/`. The pending roadmap was the only changed
file when the command began; the compiled production baseline is unmodified SV01.
Machine is Apple M1 Max (10 logical CPUs), 32 GiB, macOS 26.6.2, Rust/Cargo 1.98.1,
aarch64/LLVM 22.1.8. Use workspace release opt-level=3, default target and build
parallelism, warm compilation cache, SDK source/native features with native
preparation/execution disabled, and PUC Lua 5.4.8. Three execution warmups and
eleven samples per route/process use rotating order and reversed second-process
order. Source compilation, setup, linking and the 49.384-second build are excluded;
host entry/exit and default GC are included. Desktop/core/frequency activity and
editor-triggered background checks are not isolated, so final acceptance requires
fresh interleaved baseline/candidate trials rather than assuming these initial
observations establish a small speed difference.

Times are microseconds per complete workload (entry per call):

| Workload | NE01 baseline VM | Lua | VM/Lua |
| --- | ---: | ---: | ---: |
| entry | 1.353 | 0.029 | 46.52 |
| arithmetic | 20,301.688 | 393.833 | 51.55 |
| branches | 22,160.708 | 833.042 | 26.60 |
| calls | 12,576.208 | 230.146 | 54.64 |
| fibonacci | 18,405.854 | 360.688 | 51.03 |
| arrays | 15,046.000 | 75.125 | 200.28 |
| maps | 14,743.396 | 71.312 | 206.74 |

Retain the original compiled executable as ignored
`target/lua-comparison/20261006T073128Z/baseline-executable` for later paired trials;
its hash is the `binary_sha256` in results.json. Independent counting uses
`target/lua-comparison/20261006T073128Z/baseline-executable --interpreter-only --profile=arithmetic`
and writes `arithmetic-counts.log` in the same directory. Arithmetic still executes
600,015 Kagari instructions versus 250,007 Lua instructions, uses six physical
temporary slots plus three fixed locals, allocates no script heap objects and
collects zero times. Value is 32 bytes and prepared instruction is 24 bytes.
This counted/profile route is separate from the timing above.

NE01 changes narrow integer lowering and adds numeric source/source-free contract
coverage. It does not claim a post-change speedup or Lua parity.

## Prepared native facts and integration (IP04), 2026-10-06

IP04 prepares runtime-local native signatures once and shares immutable TypeArgument
facts, derived parameters and enum layout applications. Exact prepared enum
layout/payload-scope identity reuses checked evidence; other applications/generations
retain structural compatibility checks. Typed callbacks borrow the enclosing call's
program retention, while standalone conversion scopes and escaping handles retain
independent leases. Values still snapshot before custom conversion, and user mapping
checks still run before effects. No enum representation, collector algorithm,
allocation/commit order or validation boundary was removed.

Reproduce with `uv run python scripts/benchmark_lua.py --interpreter-only`.
Candidate is the IP04 diff over IP03 `f1372288`; original baseline production is
`97804fe7`. Environment: Apple M1 Max (10 logical CPUs), 32 GiB, macOS 26.6.2,
Rust/Cargo 1.98.1, aarch64/LLVM 22.1.8, workspace release opt-level=3/default target
and Cargo parallelism, warm build cache, SDK source/native features with native
preparation/execution disabled, PUC Lua 5.4.8. All paired workload source hashes
match IP03. Two fresh sequential processes, three warmups and eleven samples per
route, rotating route order and reversed second-process order complete all 308
checked timed batches and warmups. Build (19.663 seconds), source compilation,
setup and linking are excluded; host entry/return and default GC remain included.
No concurrent agent build/test/profile ran during timing. Phase comparisons are
sequential observations, not interleaved trials; desktop load, core placement and
frequency remain uncontrolled. Raw metadata, hashes, ranges and setup data:
`target/lua-comparison/20261006T030014Z/`.

Times are microseconds per complete workload (entry per call):

| Workload | IP00 VM | IP03 VM | IP04 VM | IP04 Lua | IP04 VM/Lua |
| --- | ---: | ---: | ---: | ---: | ---: |
| entry | 3.097 | 1.342 | 1.360 | 0.029 | 46.85 |
| arithmetic | 274,713.312 | 20,017.959 | 20,194.230 | 389.812 | 51.80 |
| branches | 348,287.125 | 21,955.666 | 22,161.583 | 814.021 | 27.22 |
| calls | 94,889.042 | 12,386.229 | 12,380.958 | 228.583 | 54.16 |
| fibonacci | 110,080.500 | 17,980.500 | 17,948.709 | 353.688 | 50.75 |
| arrays | 66,748.750 | 33,781.125 | 14,733.271 | 67.834 | 217.20 |
| maps | 47,415.375 | 20,364.729 | 14,361.583 | 66.646 | 215.49 |

The native preparation phase records 2.29x lower array and 1.42x lower map median
time than IP03. Scalar/call medians are close to IP03; no improvement is claimed
for their small fluctuations. Against IP00, recorded arithmetic time decreases
13.60x, branches 15.72x, calls 7.66x, Fibonacci 6.13x, arrays 4.53x and maps 3.30x.
The six nontrivial IP04 VM/Lua ratios remain 27.22–217.20: **Lua parity acceptance
is not met**. These finite workloads do not establish general language performance.

Separate sampling/counting used
`uv run python scripts/profile_lua_macos.py arithmetic arrays maps`, with raw data
under `target/lua-comparison/20261006T030129Z-macos-profile/`. Timing above has no
profiler or allocator-counting overhead. Logical counts are 600,015 / 78,030 /
66,030, versus Lua 250,007 / 42,012 / 24,012. Run functions use 6 / 7 / 9 physical
temporary slots plus 3 / 4 / 9 fixed locals, versus 14 / 43 / 70 logical temporaries.
Arithmetic still allocates no script heap objects and collects zero times. Arrays
allocate one object and collect twice per workload; maps allocate 2,001 objects
and collect five times. Type/layout preparation does not eliminate ordinary Option
objects or change observable enum semantics.

Remaining sampled costs are distinct:

- Arithmetic still spends many leaf samples in the VM loop, cursor authority checks,
  operand reads/writes and collection eligibility. The prepared representation has
  not established a narrower internal execution interface that can reuse region-level
  validation while preserving observer, cancellation and boundary checks.
- Arrays show module-key hashing, program retention and allocator work. Typed
  collection handles and their access/conversion scopes still acquire independent
  retention. Borrowing a native callback's top-level program lease removes only one
  part of this cost; escaping values still require durable leases.
- Maps still show allocator traffic and TypeView normalization/compatibility.
  Prepared native result layouts do not yet share all evidence with bytecode
  pattern access across lexical owners. Generic native application rebuilding is
  also unchanged; this suite makes no new claim about unmeasured generic workloads.

Samples identify follow-up areas, not exact CPU cycle shares; optimized/inlined
and deduplicated symbols limit attribution. Further work should define reusable
execution/type/retention evidence at these boundaries, with bounded preparation and
explicit escape behavior. No collector replacement or special Option shortcut is
justified by the scalar profiles. A broader parity claim additionally requires
strings, objects, closures, traits and host-callback workloads.

Final integration passes 1,896 workspace tests across 113 summaries, zero failures,
one existing ignored manual benchmark; strict workspace all-target Clippy; structure
(905 Rust files, zero violations/exceptions); formatting and diff checks. Independent
artifact-only/source/native/combined consumers pass 9/10/11/12 tests, 13 production
crate dependency boundaries and ABI/contract build graphs pass, CLI JIT passes five
tests, and the host_objects example passes. The fallback fixture explicitly retains
unoptimized MIR to test unsupported instructions; the native fixture covers default
constant-remainder folding. Neither fallback assertions nor production checks were
weakened. The low-level native allocation probe now includes cached declared-type
and element-type access and remains zero-allocation after warmup; this does not
imply that the complete high-level typed native path allocates nothing.
Final logs: `target/ip04-workspace-final.log`, `target/ip04-clippy-final.log`,
`target/ip04-features.log`, `target/ip04-cli-jit.log`, and related `target/ip04-*`.

## Register allocation and call windows (IP03), 2026-10-06

The SDK now enables bounded, reverified MIR copy/constant simplification by default.
Physical preparation allocates temporary registers from checked CFG liveness;
canonical logical identities and fixed debugger locals remain intact. Compact
instructions contain physical operands. Direct script calls copy arguments between
runtime-owned windows after capacity growth, preserving source generations and
repeated/reordered arguments without a temporary argument vector.

Candidate is the IP03 diff over `d8404fe4`. Reproduce with
`uv run python scripts/benchmark_lua.py --interpreter-only`. Environment remains
Apple M1 Max (10 logical CPUs), 32 GiB, macOS 26.6.2, Rust/Cargo 1.98.1,
aarch64/LLVM 22.1.8, workspace release defaults, default target/Cargo parallelism,
warm build cache, SDK source/native features with native preparation/execution
disabled and PUC Lua 5.4.8. Sources and inputs are unchanged. Two fresh sequential
processes use three warmups and eleven samples per route, with rotating route order
and reversed second-process order. All 308 timed batches and warmups pass checksums.
Build (25.743 seconds), compilation/setup/linking are excluded; host entry/return
and default GC remain included. No concurrent agent build/test/profile ran during
measurement. Desktop load, frequency and core placement remain uncontrolled; these
are sequential phase observations, not interleaved baseline/candidate trials.
Raw metadata, hashes, ranges and CSVs are under
`target/lua-comparison/20261006T021400Z/`.

Times are microseconds per complete workload (entry per call):

| Workload | IP02 VM | IP03 VM | IP03 Lua | IP03 VM/Lua |
| --- | ---: | ---: | ---: | ---: |
| entry | 1.384 | 1.342 | 0.029 | 46.37 |
| arithmetic | 23,974.625 | 20,017.959 | 388.146 | 51.57 |
| branches | 30,183.291 | 21,955.666 | 817.229 | 26.87 |
| calls | 13,350.021 | 12,386.229 | 226.958 | 54.57 |
| fibonacci | 19,677.709 | 17,980.500 | 349.938 | 51.38 |
| arrays | 34,875.874 | 33,781.125 | 68.730 | 491.51 |
| maps | 21,161.395 | 20,364.729 | 68.604 | 296.84 |

Every recorded VM median decreases, but Lua collection medians also decrease;
ratios alone do not indicate a VM regression. Nontrivial ratios of 26.87–491.51
still miss parity. Separate macOS sampling/counting with
`uv run python scripts/profile_lua_macos.py arithmetic calls maps` records:

| Diagnostic | Arithmetic | Calls | Maps |
| --- | ---: | ---: | ---: |
| IP02 logical instructions | 750,014 | 240,014 | 81,030 |
| IP03 logical instructions | 600,015 | 220,015 | 66,030 |
| Lua instructions | 250,007 | 110,007 | 24,012 |
| Run function logical temporaries | 14 | 15 | 70 |
| Run function physical temporaries | 6 | 5 | 9 |
| Separate fixed local slots | 3 | 3 | 9 |

Value remains 32 bytes and execution records 24 bytes. Sampling still identifies
cursor checks and dispatch on scalar/call paths, and substantial allocator/type
normalization on maps. Arithmetic/calls allocate no script heap objects; maps still
allocate 2,001 objects and collect five times per complete workload. No Option
representation or collector semantics changed. Raw samples/counts are under
`target/lua-comparison/20261006T021521Z-macos-profile/`; optimized/inlined and
deduplicated symbols limit attribution.

The separate native_allocations regression records zero Rust allocations,
reallocations, deallocations or requested bytes across 1,000 warmed direct scalar
frame entries/exits, including reordered/repeated arguments. This is a call-window
probe, not a claim that complete script execution or host entry allocates nothing.
Focused MIR equivalence, CFG backedge/liveness and frame-growth tests pass;
297 final VM tests pass (one existing manual benchmark ignored), along with strict
affected all-target Clippy, structure, formatting and diff checks. The earlier
runtime/VM suite passed before final physical operand encoding; final cross-backend
and full-workspace acceptance remains IP04 work.

## Compact execution (IP02), 2026-10-06

IP02 moves immutable host descriptors out of Value's inline layout (104 to 32 bytes
on aarch64) and derives Copy execution records within a tested 24-byte budget,
versus the canonical 136-byte bytecode enum. The canonical verified code remains
immutable and retains variable-length semantic/call data. Runtime identity
normalization shares the physical product; logical PCs still map one-to-one.
The interpreter holds one cursor across non-reentrant instructions, releasing it
before collection, observation, calls and reentry. Boundary dispatch borrows
canonical records instead of cloning wide instructions. Scalar precision, full
handle identities and checked numeric semantics remain intact.

Reproduce with `uv run python scripts/benchmark_lua.py --interpreter-only`.
Candidate is the IP02 production diff over IP01 `38140f16`. The same M1 Max,
32 GiB, macOS 26.6.2, Rust/Cargo 1.98.1, default target/Cargo parallelism and
workspace release profile are used, with SDK source/native features but native
preparation/execution disabled, PUC Lua 5.4.8, unchanged workload sources and
three warmups/eleven samples per route in two sequential fresh processes.
All 308 timed batches and warmup checksums pass. Compilation (19.961 seconds),
setup and linking are excluded; host entry, return and default GC are included.
No concurrent agent build/test/profile ran during timing. These are sequential
same-day observations, not interleaved baseline/candidate trials; desktop load,
core placement and frequency remain uncontrolled. Raw ranges, metadata and hashes:
`target/lua-comparison/20261006T014054Z/results.json` and sibling CSVs/logs.

Times are microseconds per complete workload (entry per call):

| Workload | IP01 VM | IP02 VM | IP02 Lua | Recorded VM improvement | IP02 VM/Lua |
| --- | ---: | ---: | ---: | ---: | ---: |
| entry | 1.820 | 1.384 | 0.029 | 1.32x | 47.32 |
| arithmetic | 115,614.167 | 23,974.625 | 392.709 | 4.82x | 61.05 |
| branches | 150,262.604 | 30,183.291 | 847.125 | 4.98x | 35.63 |
| calls | 41,442.812 | 13,350.021 | 230.667 | 3.10x | 57.88 |
| fibonacci | 50,078.229 | 19,677.709 | 353.146 | 2.54x | 55.72 |
| arrays | 45,807.938 | 34,875.874 | 80.041 | 1.31x | 435.72 |
| maps | 30,704.541 | 21,161.395 | 72.709 | 1.45x | 291.04 |

The 35.63–435.72x nontrivial ratios still miss Lua parity. A separate warmed macOS
sample pass (`uv run python scripts/profile_lua_macos.py arithmetic calls maps`)
used the same release binary, with no profiling overhead in the timing table.
Raw evidence: `target/lua-comparison/20261006T014244Z-macos-profile/`.
Arithmetic's 4,012 main-thread samples attribute 31.51% exclusive samples to the
VM run loop, 20.84% to cursor authority/termination checks and 7.30% to collection
eligibility. Session hashing and wide instruction cloning no longer dominate that
scalar path. Optimized/inlined frames and deduplicated symbols limit attribution;
these are wall-clock stack samples, not exact CPU cycle costs.
Calls still show frame entry/authority costs. Maps show substantial Rust allocator
and repeated type-normalization traffic, in addition to execution; they still
allocate 2,001 GC objects and collect five times per complete workload. Arithmetic
and calls have no script heap allocations or collections in the measured window.
Logical instruction totals remain 750,014 / 240,014 / 81,030 respectively, exactly
matching IP00. The compact product has not yet coalesced locals/registers or removed
redundant logical instructions. IP03 owns this work and script argument windows;
IP04 owns prepared native type/retention boundaries. No collector algorithm or
semantic/ownership checks were removed to obtain these timings.

Validation: 296 runtime unit/integration tests and 296 VM tests pass; one existing
manual performance test remains ignored. The value migration also passed workspace
all-target compilation and host tests. Strict runtime/VM all-target Clippy, structure,
formatting and diff checks pass. Full cross-backend integration remains IP04 work.

## Execution windows (IP01), 2026-10-06

IP01 replaces per-frame host root leases with a reusable runtime-owned value arena,
indexed session frame storage and checked operand cursors. GC traces frame windows
and their program/environment edges; native views retain checked window identities.
Persistent host roots and safepoint/observer locations remain unchanged. The cursor
pins its session record and reads sticky termination directly. Root diagnostics count
both storage classes; trap/quarantine cleanup assertions remain intact.

Reproduce with `uv run python scripts/benchmark_lua.py --interpreter-only`.
Candidate is the IP01 production diff over `b99702b2`; baseline production is
`97804fe7` in the post-GO06 measurement below. Both use the same matched sources,
M1 Max/32 GiB/macOS 26.6.2, Rust/Cargo 1.98.1, workspace release defaults, SDK
source/native features with JIT preparation/execution disabled, PUC Lua 5.4.8,
default target and default Cargo parallelism. These are sequential same-day runs,
not interleaved baseline/candidate trials. Desktop background activity, core
placement and frequency remain uncontrolled. Ratios below describe the recorded
medians, not universal speedup guarantees.

Two sequential candidate processes, three warmups and eleven samples per route per
process, reverse second-process order: all 308 timed batches and warmup checksums
pass. No concurrent agent build/test/profile during timing. Compilation, setup and
linking are excluded; the reused release build took 19.633 seconds. Raw metadata,
hashes, setup timings and all sample ranges are under ignored
`target/lua-comparison/20261006T011517Z/results.json` and its sibling logs/CSVs.

All times are microseconds per complete workload (entry per call):

| Workload | IP00 VM | IP01 VM | IP01 Lua | Recorded VM improvement | IP01 VM/Lua |
| --- | ---: | ---: | ---: | ---: | ---: |
| entry | 3.097 | 1.820 | 0.029 | 1.70x | 62.41 |
| arithmetic | 274,713.312 | 115,614.167 | 383.438 | 2.38x | 301.52 |
| branches | 348,287.125 | 150,262.604 | 830.188 | 2.32x | 181.00 |
| calls | 94,889.042 | 41,442.812 | 229.437 | 2.29x | 180.63 |
| fibonacci | 110,080.500 | 50,078.229 | 355.729 | 2.20x | 140.78 |
| arrays | 66,748.750 | 45,807.938 | 79.020 | 1.46x | 579.70 |
| maps | 47,415.375 | 30,704.541 | 74.541 | 1.54x | 411.91 |

IP01 does not change opcode counts or the 104-byte Value/136-byte instruction
headers. Repeated instruction fetch/clone and between-instruction frame/session
checks remain, along with native retention/type and container allocation costs.
The remaining 140.78–579.70x nontrivial ratios do not meet Lua parity. IP02 owns
compact execution representation; IP03 register allocation/calls and IP04 prepared
native boundaries follow. No new collector, JIT expansion or disabled check accounts
for the observed improvements. Focused acceptance passed 134 runtime tests and
296 VM/unit/integration tests; one existing manual performance test stayed ignored.

## Post-GO06 interpreter baseline, 2026-10-06

[The current Lua comparison](../benchmarks/lua-comparison/README.md#interpreter-baseline-after-go06-2026-10-06)
measures production revision `97804fe7` on Apple M1 Max/macOS 26.6.2 with Rust
1.98.1, workspace release/default Cargo parallelism, PUC Lua 5.4.8 and explicitly
disabled native execution/preparation. Two sequential processes provide 22 samples
per route; all 308 timed batches pass independent checksums. Setup is excluded.
Six nontrivial paired workloads show VM/Lua median elapsed-time ratios of
308.28–764.12. Arithmetic is 274.713 ms versus 0.390 ms; arrays are 66.749 ms
versus 0.087 ms. The separate entry benchmark is 3.097 us versus 0.030 us.
This is not a controlled regression comparison with the earlier Windows baseline.

Independent macOS stack sampling collects 24,143 main-thread samples. Arithmetic
has no GC object allocation/collection: reported leaf shares include 13.78% in
SipHash reached through session-frame lookup, 22.81% frame/session access, 15.56%
termination/allowed checks and 12.76% instruction fetch/clone. Samples are diagnostic,
with optimized/duplicate-symbol attribution limits, not exact CPU accounting.
Arithmetic executes 750,014 Kagari versus 250,007 Lua bytecodes. Current Value and
instruction headers are 104 and 136 bytes. Collection paths additionally show
native retention/type work and Rust allocation; map lookup allocates ordinary
Option results. The report retains all sample ranges, GC/instruction counts,
environment, primary-source comparisons, limitations and raw-output locations.

Reproduce with `uv run python scripts/benchmark_lua.py --interpreter-only`, followed
by `uv run python scripts/profile_lua_macos.py` on macOS. The
[performance follow-up](implementation-roadmap.md#interpreter-performance-follow-up)
proposes direct execution-stack access/tracing, compact typed instructions/slots,
register allocation/call windows and prepared native boundaries, in that order.
No production speedup is implemented or inferred from these measurements.

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
cargo run -p kagari-contract --example definition_metadata --release --locked
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

## Closed execution region (NE02), 2026-10-06

Use `uv run python scripts/benchmark_lua.py --interpreter-only` after correctness
checks finish. The same M1 Max/toolchain/profile/features and two-process protocol
as NE01 produce 308 checked batches; raw data is under
`target/lua-comparison/20261006T081153Z/results.json`. Build time (17.795 seconds),
source compilation, preparation and initialization are excluded from execution.
Host entry/return and default GC policy remain included. Desktop background work
and core frequencies are uncontrolled; final acceptance needs interleaved original
and candidate executables rather than attributing every sequential difference.

| Workload | NE02 VM (us) | Lua (us) | VM/Lua |
| --- | ---: | ---: | ---: |
| entry | 1.332 | 0.029 | 46.35 |
| arithmetic | 10,540.688 | 390.292 | 27.01 |
| branches | 12,494.688 | 811.270 | 15.40 |
| calls | 8,773.729 | 226.980 | 38.65 |
| fibonacci | 15,303.084 | 350.541 | 43.66 |
| arrays | 16,861.854 | 67.834 | 248.58 |
| maps | 15,603.896 | 66.459 | 234.79 |

The closed scalar region removes per-operand and per-instruction authority checks;
cancellation, observer and collection eligibility still retain original PCs.
Value storage and boxed numeric helpers remain for NE03. Compared with NE01,
arrays/maps regress: managed moves/returns now exit to general ownership handling.
NE03 storage and NE04 typed transfers own this integration cost, with final paired
checks retaining all seven workloads. No parity claim is supported.

Independent `uv run python scripts/profile_lua_macos.py arithmetic` sampling and
instruction counting is saved under
`target/lua-comparison/20261006T081335Z-macos-profile`. The route retains 600,015
Kagari versus 250,007 Lua logical instructions, six physical temporary slots and
three locals, Value size 32 bytes and prepared instruction size 24 bytes, with no
script allocations or collections. The optimized call tree attributes remaining
work to the region, collection eligibility, boxed numeric helpers and Value drop;
authority admission is no longer a per-instruction callee. Sampling is outside
timing and is not an exact cycle-share measurement; inline attribution is limited.

## Scalar frame banks and prepared kernels (NE03), 2026-10-06

The same release/default-feature M1 Max protocol, interpreter-only, runs after all
tests and Clippy finish. Two fresh processes validate 308 timed batches. Raw data:
`target/lua-comparison/20261006T085814Z/results.json`. The warm build takes 17.533
seconds and is excluded; source compilation/preparation/linking stay separate.
As before, desktop background work is uncontrolled and these checkpoint timings
are sequential observations. Final acceptance retains the saved original binary
for an interleaved comparison.

| Workload | NE03 VM (us) | Lua (us) | VM/Lua |
| --- | ---: | ---: | ---: |
| entry | 1.319 | 0.028 | 46.30 |
| arithmetic | 4,808.000 | 379.646 | 12.66 |
| branches | 5,519.979 | 793.083 | 6.96 |
| calls | 7,181.062 | 220.750 | 32.53 |
| fibonacci | 14,101.938 | 344.812 | 40.90 |
| arrays | 14,287.834 | 67.229 | 212.52 |
| maps | 13,600.729 | 66.125 | 205.68 |

The arithmetic frame uses nine scalar slots and no managed slots: 72 payload bytes
plus nine initialization bytes versus 288 boxed-slot bytes in NE01/NE02. Shared
location records cost eight bytes each; an active Window header costs 96 bytes on
this target. Fixed frame/arena headers, retained capacity, program/environment
metadata and prepared instructions are additional costs, not included in the
81-byte slot figure. Reproduce sizes and boundary regressions with
`cargo test -p kagari-runtime --lib scalar_windows_preserve -- --nocapture`; the
independent profiler reports actual bank counts. Prepared instructions remain
24 bytes and public Value remains 32 bytes. Scalar GC root scanning is eliminated,
while managed/capture-cell roots and suspended program/environment roots remain.

The new `uv run python scripts/benchmark_lua.py --numeric-matrix` runs twelve
20,000-iteration mixed arithmetic/conversion fixtures, with integer bit/shift
steps. Values vary within every narrow checked domain; independent Rust loops
check every warmed/timed result. Lua uses integer division and bit operators for
integer cases and floating division for float cases. All float values are exactly
representable, permitting matched f32/f64 checksums without adding Lua rounding
emulation. This measures supported successful execution, not equal overflow/type
semantics or equal instruction counts. Source/verification setup is excluded and
logged separately. Two fresh processes produce 528 checked timed batches under
`target/lua-comparison/20261006T085924Z-numeric/results.json`.

| Domain | NE03 VM (us) | Lua (us) | VM/Lua |
| --- | ---: | ---: | ---: |
| i8 | 3,754.834 | 591.480 | 6.35 |
| i16 | 3,758.709 | 591.583 | 6.35 |
| i32 | 3,675.208 | 574.562 | 6.40 |
| i64 | 3,621.938 | 572.875 | 6.32 |
| isize | 3,622.562 | 573.020 | 6.32 |
| u8 | 3,633.749 | 574.458 | 6.33 |
| u16 | 3,634.563 | 574.333 | 6.33 |
| u32 | 3,642.500 | 572.625 | 6.36 |
| u64 | 3,613.666 | 575.542 | 6.28 |
| usize | 3,615.876 | 573.771 | 6.30 |
| f32 | 3,206.583 | 441.209 | 7.27 |
| f64 | 3,172.875 | 441.375 | 7.19 |

Numeric type changes show no fallback cliff in this bounded matrix; they do not
prove equal costs across arbitrary source forms. Calls, native adapters and
managed transfers remain NE04 work. Lua parity is not achieved.


Independent `uv run python scripts/profile_lua_macos.py arithmetic` output is in
`target/lua-comparison/20261006T085930Z-macos-profile`. Counting retains 600,015
logical Kagari instructions versus 250,007 Lua instructions and no script allocations
or collections. The sampled optimized route uses prepared types payload kernels,
with no boxed numeric helper in the arithmetic path. Collection eligibility and
region dispatch remain visible costs. This is sampled attribution, not exact
cycle percentages; preparation and observer counting remain outside throughput.

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
