# Foundation Performance Measurements

These reproducible workloads establish a baseline for R18. The figures are
observations on one machine, not performance guarantees.

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
