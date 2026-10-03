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
this run; they are not estimates for all Kagari or Lua applications. BP01 did
not collect a CPU profile. The subsequent BP02 diagnosis below investigates
interpreter costs separately and does not justify removing runtime checks.

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

## Interpreter diagnosis (BP02), 2026-10-03

Reproduce on x64 Windows with 64-bit Python:

```powershell
uv run python scripts/profile_lua_comparison.py arithmetic branches calls fibonacci arrays maps
```

The driver builds the existing release profile with the child-process override
`CARGO_PROFILE_RELEASE_DEBUG=2`, retaining optimization level 3 and adding private
Rust PDB symbols. It saves the matching executable/PDB under
`target/lua-comparison/profile/symbol-bin`; `--no-build` reuses that pair. The
workspace profile and production sources are unchanged. This is a diagnostic
build, and its execution durations are not pooled into the BP01 speed ratios.
The first symbol build took 106.228 seconds; the recorded final invocation reused
those products and rebuilt the diagnostic harness in 3.158 seconds. Build and
setup costs are outside every sampling window.

After three checked warm calls, each fresh process repeatedly executes one VM
workload for ten seconds. No observer or Lua hook is enabled during this window.
The sampler selects the target thread with the highest accumulated CPU cycles,
briefly suspends it, reads its instruction pointer and always resumes it before
symbol resolution. Intervals have deterministic 1.5–2.5 ms jitter. Local PDBs
resolve optimized Rust symbols without ETW/system profiling privileges. Windows
CPU recording was unavailable (`wpr -start CPU -filemode`, policy error
`0xc5585011`); no system recording was active or left running.

The machine, Rust/Cargo versions, features, default Cargo parallelism and cache
conditions match BP01. Workload processes run sequentially, with no concurrent
agent-started build/test. CPU affinity/frequency/background activity remain
uncontrolled. The final sample has zero thread-context errors in all six cases.
These are instruction-pointer sample shares, not exact exclusive CPU times or
call stacks. Inlining folds some work into callers, brief suspension perturbs
execution, and C/system libraries without private PDBs can resolve to a nearby
export (for example `_NLG_Return2`); those labels are not evidence of unwinding.
An initial run with minimal release symbols was discarded because private Rust
functions also resolved to misleading adjacent public symbols.

| Workload | Samples | Frame access/checks | Termination state | Instruction fetch/clone | Dispatch/loop/report | Slots/value copies | Allocator exports |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 4,249 | 23.21% | 10.03% | 12.10% | 22.33% | 13.70% | 0.00% |
| branches | 4,269 | 22.75% | 9.53% | 12.56% | 22.00% | 12.60% | 0.00% |
| calls | 4,250 | 22.19% | 10.54% | 10.66% | 17.81% | 11.13% | 4.45% |
| fibonacci | 4,250 | 17.34% | 7.15% | 12.07% | 16.56% | 10.71% | 8.07% |
| arrays | 4,242 | 18.95% | 8.77% | 11.36% | 19.52% | 13.55% | 4.76% |
| maps | 4,231 | 17.06% | 6.85% | 8.53% | 14.13% | 9.60% | 11.84% |

Buckets are disjoint resolved symbol families; unlisted symbols account for the
remainder. Frame access includes `ExecutionStack::{validate_top,current,current_mut}`;
termination is `ResourceState::termination`; fetch is `ExecutionFrame::next_instruction`;
dispatch includes `Executor::{run,dispatch_instruction,report_operation}` and any
inlined handlers, so it is not all dispatch overhead. Slots include frame local/
register reads/writes, `RootSet::set`, and `Value` clone/drop. Allocator exports
are `RtlAllocateHeap`/`RtlFreeHeap`, without caller attribution. The ordinary
no-observer `Runtime::observe_execution` path additionally accounts for
2.20–3.01% of samples.

Independent counting passes use a Kagari execution observer and a Lua instruction
hook, outside the sampling window. Counts include the entry and script callees;
different opcode semantics prevent treating them as equivalent units of work.
GC counts and allocations come from the uninstrumented window, normalized by
complete calls. The live-object counter is adjusted by reclaimed objects to
recover total GC-heap object allocations; it does not count Rust allocations.

| Workload | Kagari instructions/call | Lua instructions/call | GC collections/call | GC-heap allocations/call |
| --- | ---: | ---: | ---: | ---: |
| arithmetic | 750,014 | 250,007 | 0 | 0 |
| branches | 960,016 | 309,968 | 0 | 0 |
| calls | 240,014 | 110,007 | 0 | 0 |
| fibonacci | 262,691 | 120,400 | 0 | 0 |
| arrays | 96,030 | 42,012 | 2 | 1 |
| maps | 81,030 | 24,012 | 5 | 2,001 |

The shared bottleneck is the per-instruction runtime protocol. The loop in
[`executor/mod.rs`](../../crates/kagari-vm/src/executor/mod.rs) accesses the current
frame repeatedly, and operand handlers access it again. Each access validates
execution state, active-session identity and the top frame scope in
[`frame.rs`](../../crates/kagari-runtime/src/frame.rs). Register writes also
validate the value's heap ownership through root slots. `next_instruction`
clones a bytecode enum on every dispatch; the measured x64 sizes are 264 bytes
per `BytecodeInstruction` and 104 bytes per `Value`. Even scalar slot operations
use the general value/root representation. Arithmetic alone performs 250,003
`LoadLocal`, 100,002 `StoreLocal` and 100,003 `LoadConst` instructions, in addition
to 200,001 binary operations and loop branches/jumps.

Calls/recursion add frame/root allocation costs. Map lookup also constructs
heap-backed `Option` results: [`map_get`](../../crates/kagari-runtime/src/native/foundation/hash.rs)
calls [`option`](../../crates/kagari-runtime/src/native/foundation.rs), which
allocates an enum object. This fixture performs 2,000 gets plus one map allocation,
matching the measured 2,001 allocations and five collections per call. The zero
GC collections in scalar/call workloads exclude GC pauses as the cause of their
large gaps. Both container workloads still spend substantial samples in the same
frame/check/fetch paths.

Measured priorities for a subsequent optimization are to consolidate repeated
frame/session access within safe execution boundaries, avoid cloning whole
instructions, and reduce redundant local/constant moves in verified bytecode.
Compact scalar/root storage and heap-backed `Option` results deserve separate
evaluation after that. Preserve cancellation, observer program points, host
reentry, generation checks, roots and trap cleanup. This checkpoint changes
diagnostic tooling only; no speedup is claimed.

Raw files are `target/lua-comparison/profile/{workload}/samples.json` and
`execution.log`, plus `metadata.json`, binary/PDB/source hashes and build logs.
The final six windows contain 25,491 samples and validate every returned checksum.
The diagnostic CLI and ordinary benchmark regression are checked independently.
Strict workspace/all-target Clippy, formatting, structure (659 files, zero
violations/exceptions), empty/single-element regression, all fifteen ordinary
release smoke routes, Python syntax/document links and diff checks pass. No
production source changed, and the full language-contract matrix was not rerun.
