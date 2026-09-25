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
