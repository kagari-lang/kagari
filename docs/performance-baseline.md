# Foundation Performance Measurements

These measurements accompany the foundation refactor; they do not close R18.
Compiler, reanalysis, shared-code memory and call-overhead measurements remain open.

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
