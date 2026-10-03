# Kagari and Lua comparison

Run from the repository root:

```text
uv run python scripts/benchmark_lua.py
```

The driver builds `kagari-lua-benchmark` in the existing release profile and
default target directory, then launches two sequential processes. It saves CSV
samples, JIT diagnostics, source/binary hashes, machine/toolchain metadata,
separate build duration and summarized JSON under ignored `target/lua-comparison/`.
No system Lua installation is needed: this benchmark-only package embeds standard
Lua 5.4 through `mlua` with `lua54,vendored`. Kagari production dependencies do
not gain Lua. The lockfile pins the measured dependencies.

For correctness checks without a performance claim:

```text
uv run python scripts/benchmark_lua.py --check --runs 1
cargo test -p kagari-lua-benchmark -- --test-threads=1
```

## Matching and timing

Kagari and Lua have different grammars. The paired `.kgr` and `.lua` files use
equivalent source constructs, loop bounds, algorithms, constants and inputs.
Neither language delegates the workload to a Rust helper. Both use explicit
`while` loops rather than comparing different iterator or numeric-for machinery.
All seven results are checked against independent Rust reference calculations.
The regression test additionally checks empty and single-element inputs.

| Workload | Input | Measured script work |
| --- | ---: | --- |
| entry | 1,000 calls per batch | Return `40 + 2`; primarily host entry overhead |
| arithmetic | 50,000 iterations | Sum `i % 97` |
| branches | 30,000 iterations | Bounded integer recurrence and two-way branch |
| calls | 10,000 iterations | Call a script arithmetic helper once per iteration |
| fibonacci | n = 20 | Recursive Fibonacci, without memoization |
| arrays | 2,000 elements | Append, mutate each indexed element, then sum |
| maps | 1,000 keys | Insert, read/update, then read/sum sparse integer keys |

Measured execution starts after source compilation, artifact verification,
runtime construction, linking and native preparation. Each supported route gets
three untimed warmup calls and 11 measured samples per process. Engine order
rotates each sample; the second process reverses the initial engine and workload
order. The reported execution median pools 22 samples, normalized by the batch
size. Every timed batch accumulates a checksum and validates it after the timer;
warmup results are also validated. The timer is Rust `Instant` on both routes.
Host API entry, return conversion and default GC during execution remain included.
Console output, source file I/O, expected-value calculation and setup are excluded.
No profiler or counting allocator is installed.

Setup has its own three samples per workload per process. Each uses a fresh
Kagari engine and fresh Lua state, so source analysis is not reused from an earlier
compilation. Kagari source-to-artifact includes static analysis, MIR, bytecode,
verification and portable MIR emission. Lua source-to-chunk compiles the source;
its separate module-init stage creates the script functions. These stages provide
startup cost context, rather than identical compiler responsibilities. Engine
initialization's first process-local sample also includes Kagari's cold fixed
foundation caches; later engine/runtime construction reuses those caches.

The native route requires `JitExecutionStatus::Native` on every execution. A
`PreparedNativeEntry::Unsupported` is logged and omitted from JIT timing, without
executing a fallback as if it were native code. Currently only the entry workload
fits Cranelift's zero-argument, straight-line i32 scalar subset. The other entries
call a script function and are rejected by the native compiler; their bodies also
use control flow or collections outside the subset.

## Semantic limits

This is a throughput comparison of the current embedding APIs on these inputs.
It does not claim equivalent runtime safety or identical machine instructions.
Kagari performs checked i32 arithmetic and retains bounds, generation, root,
linking and cooperative cancellation checks. Standard Lua uses dynamic values;
its integer overflow semantics differ. Every benchmark intermediate fits i32,
and the fixtures avoid floating point, division, overflow and invalid accesses.
See the [Lua 5.4 manual](https://www.lua.org/manual/5.4/manual.html#3.4.1).

Kagari arrays use `ArrayList<i32>`; Lua uses a table's dense 1-based sequence.
Lua adds one to the logical index where Kagari uses zero-based indexing. Sparse
integer keys prevent the map workload from merely reusing Lua's dense sequence
case. Kagari map lookup returns `Option`, while Lua returns a value or nil; both
scripts explicitly handle absence. The containers and their checks remain each
language's normal implementation. Default GC stays enabled in both runtimes;
their policies are not identical.

Lua is PUC Lua, not LuaJIT. This initial suite does not cover host callbacks,
strings, object graphs, hot reload, memory consumption or concurrent throughput.
The tiny entry case is not representative of numeric or collection throughput,
and its JIT compilation may fold constants. Do not extrapolate one overall
language speed ratio from these seven microbenchmarks.

## Measured baseline: 2026-10-03

Base revision: `45aa927d` plus this benchmark checkpoint; language/runtime code
is unchanged. Intel Core i9-12900K, 24 logical CPUs, 63.7 GiB visible RAM,
Windows 11 Pro 10.0.26300, x86_64-pc-windows-msvc. Rust/Cargo 1.99.0
(`b940084d7` / `5f94df478`), LLVM 23.1.1; workspace default release profile
(`opt-level=3`), Cargo default build parallelism. PUC Lua 5.4.8 is statically
compiled by `lua-src 550.0.0` through `mlua 0.11.6`; this is not a debug C build.
Cranelift 0.132.0 uses the unchanged `CraneliftBackend::for_host()` defaults.
Kagari enables source/native; Lua enables lua54/vendored. No custom RUSTFLAGS,
CFLAGS, CC, build jobs or target directory override was set.

The benchmark is single threaded, with two sequential fresh processes and warm
execution as described above. No other agent-started build or test was running
during measurement. CPU affinity, frequency and background Windows activity
were not controlled; the observed sample ranges below remain part of the result.
All 330 measured batches pass their checksums. Nothing was discarded as an
outlier. Times are per complete workload in microseconds, lower is better.

| Workload | Kagari VM median | Lua median | VM / Lua | Kagari VM min–max | Lua min–max |
| --- | ---: | ---: | ---: | ---: | ---: |
| entry | 1.175 | 0.035 | 33.32 | 1.055–1.866 | 0.032–0.058 |
| arithmetic | 88,043.150 | 399.400 | 220.44 | 76,093.0–116,175.8 | 351.1–631.3 |
| branches | 106,745.300 | 692.500 | 154.14 | 98,787.6–120,912.7 | 659.9–861.4 |
| calls | 35,110.550 | 203.450 | 172.58 | 28,237.2–40,947.8 | 194.1–439.2 |
| fibonacci | 39,424.350 | 282.700 | 139.46 | 37,306.3–45,540.0 | 274.5–548.7 |
| arrays | 12,864.000 | 122.850 | 104.71 | 10,488.7–16,382.9 | 74.1–171.4 |
| maps | 13,128.000 | 87.400 | 150.21 | 11,298.0–15,708.9 | 71.4–149.4 |

The entry workload's actual JIT median is 1.845 microseconds, with a
1.627–3.153 microsecond range. This includes the SDK/native entry protocol and
the per-call native-status assertion, and does not establish native arithmetic
throughput. JIT results for the other six workloads are unsupported, not zero
time or interpreter fallback measurements.

The two process medians independently show the same large VM gap. For arithmetic,
VM/Lua is 231.63 in the forward process and 212.42 in the reverse process. Arrays
show more noise: 132.77 and 104.27 respectively. The pooled ratios summarize
this run; they are not estimates for all Kagari or Lua applications. No CPU
profile was collected, so this comparison does not attribute the gap to a
specific interpreter routine or justify removing runtime checks.

Separate setup medians for the arithmetic fixture, six samples each:

| Stage | Kagari milliseconds | Lua milliseconds |
| --- | ---: | ---: |
| Engine/state initialization | 70.790 | 0.085 |
| Source to artifact/chunk | 277.291 | 0.015 |
| Artifact verification/preparation | 109.586 | — |
| Runtime initialization | 69.848 | — |
| Program link / Lua module initialization | 3.135 | 0.001 |

The compiler responsibilities differ as described above. These values are not
included in execution times. The first release build was reported by Cargo as
1 minute 45 seconds; the timed driver invocation reused those products and
recorded a 2.677-second build/rebuild step. Both build observations are excluded
from execution. Whole process durations (17.062 and 15.316 seconds) include setup,
warmups, measurements and output, and are not script-only execution times.

Raw data: `target/lua-comparison/20261003T060227Z/results.json`, accompanying
`run-0.csv`, `run-1.csv`, JIT diagnostics and `build.log`. The earlier
`20261003T055918Z-check` run was a single-sample correctness smoke check and is
not pooled into the performance result. The driver records source and binary
hashes for reproducibility; target data can be regenerated with the command above.

Validation passes: all default-input checksums, the empty/single-element regression
test, strict workspace/all-target Clippy, formatting, the structure checker
(658 Rust files, zero violations/exceptions), diff checks and the eight production
dependency boundaries plus the ABI build graph. Lua remains confined to the
benchmark package. The unchanged language conformance matrix was not rerun.
