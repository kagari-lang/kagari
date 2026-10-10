# Kagari and Lua comparison

Run from the repository root:

```text
uv run python scripts/benchmark_lua.py
```

To measure only the arithmetic loop, without native preparation or other workloads:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --workload arithmetic
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

The latest completed compact-value/interpreter report is recorded in the
[VE09 enum result layout report](../../docs/performance-baseline.md#native-enum-result-layout-reuse-ve09-2026-10-10).
The seven original workloads remain unchanged. VE08 local workspace integration
and VE09 focused checks passed; the latter reduces map time to 0.489x VE08. All
16 frozen matched nontrivial original/source-form cases still take 3.66-201.94x
Lua time. Parity remains unmet and full GitHub CI is unrun. Historical reports
below retain their distinct baseline scope. HP ownership and execution measurements
and remaining architecture work are tracked in the [active plan](../../docs/interpreter-hotpath-execution-plan.md).

## Matching and timing

Kagari and Lua have different grammars. The paired `.kgr` and `.lua` files use
equivalent source constructs, loop bounds, algorithms, constants and inputs.
For the seven original workloads, neither language delegates the workload to a
Rust helper. Both use explicit `while` loops rather than comparing different
iterator or numeric-for machinery.
All seven results are checked against independent Rust reference calculations.
The regression test additionally checks empty and single-element inputs.

The VE00 follow-up also freezes `--source-forms`: direct/helper/concrete generic,
interface/shared generic identity, captured cell, field, byte state, native,
host_callback, string_constants and string_calls. The last three were added before
the VE00 baseline executable was preserved. All use 5,000 iterations and checked
reference results. String routes alternate 62-byte and 68-byte ASCII constants;
string_calls additionally passes each value through a script identity function.
The result is 325,000 bytes in both engines. These fixtures compare byte length,
not Unicode character-count semantics or global string interning.

`host_callback` calls the same checked Rust arithmetic helper through both engines'
host adapters. The older `native` row compares a Kagari native callback with a Lua
script helper and remains diagnostic only. Shared generic identity and interface
dispatch compare observable algorithms, not equivalent language type systems.
The shared generic Add default-body lowering failure remains separately tracked.
The numeric matrix uses bounded exact inputs, with Kagari while and Lua numeric-for
loops. Its ratios are diagnostic, outside the frozen matched parity gate; it does
not claim Lua implements Kagari's entire integer/float domain or checked-overflow
semantics.

Saved VE00 binaries can be paired with either unchanged matrix as well as the
original suite. For example:

```text
uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/ve00/baseline-executable
```

Both binaries must contain the same selected fixtures. Results, binary/source hashes
and process order are recorded; setup, allocation instrumentation and throughput
runs remain separate. See the [VE00 report](../../docs/performance-baseline.md#compact-value-baseline-ve00-2026-10-09).

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

Kagari arrays use `Vec<i32>`; Lua uses a table's dense 1-based sequence.
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

## Interpreter baseline after GO06, 2026-10-06

The current interpreter does **not** meet Lua parity. Across the six nontrivial
paired workloads, its median elapsed time is **308.28–764.12 times** PUC Lua's.
This is a fresh baseline, not a measured regression against the Windows results
below: machine, OS, compiler and runtime implementation differ. No production
optimization was made in this measurement checkpoint.

Reproduce execution and independent diagnostic sampling, in that order:

```text
uv run python scripts/benchmark_lua.py --interpreter-only
uv run python scripts/profile_lua_macos.py arithmetic branches calls fibonacci arrays maps
```

The new `--interpreter-only` flag skips both native preparation and native
execution. The binary still has SDK `source,native` features enabled; artifact
preparation still verifies its normal contracts outside the execution timer.
It never calls `execute_prepared` on this route. This measures pure interpretation,
not JIT fallback. The original combined-route command remains available.

### Environment and scope

Production revision: `97804fe71f0e82a460eeef24b11e1a9ef8bb2aa7`, with only
benchmark tooling changes. Apple M1 Max, 10 logical CPUs, 32 GiB RAM, macOS 26.6.2
(25G83), aarch64-apple-darwin. Rust 1.98.1 (`48a229cea`, LLVM 22.1.8), Cargo 1.98.1
(`797e8a9bc`). Workspace release defaults (optimization level 3), default target
directory and Cargo parallelism; no RUSTFLAGS/CC/CFLAGS/jobs/target override.
Lua 5.4.8 is vendored by lua-src 550.0.0 through mlua 0.11.6.

Two sequential processes, three warmups per route and eleven samples per process;
second process reverses the workload order and initial engine order. The table
pools 22 samples, normalized by batch size. All 308 timed batches and all warmup
results matched independent checksums. Every setup phase has six separate samples.
Build cache was reused where available; the release build rebuilt affected crates
and took 66.356 seconds, excluded from execution. No agent-started build/test/profile
ran concurrently with timing. Desktop applications remained active; CPU placement,
frequency, temperature and background activity were not controlled. Sample ranges
and per-process ratios are retained rather than removing outliers.

Sources use matching algorithms and explicit while loops, not identical opcodes.
Kagari uses checked i32 operations; Lua uses its dynamic integer operations. These
fixtures stay within i32 range. Default GC, runtime checks and host entry/result
conversion are included. Compilation, verification, linking and initialization are
excluded. Arrays compare Vec operations with dense Lua tables; maps compare sparse
integer keys and explicit missing-value handling. These are useful end-to-end
collection workloads, not identical container or return-value representations.

### Execution results

All times are **microseconds per complete workload**, except entry which is
normalized to one call from a 1,000-call batch. Parentheses show min–max.

| Workload | Kagari VM (us) | Lua 5.4 (us) | VM/Lua |
| --- | ---: | ---: | ---: |
| entry | 3.097 (3.051–3.183) | 0.030 (0.028–0.046) | 104.03 |
| arithmetic | 274,713.312 (271,800.875–285,643.959) | 390.188 (375.750–416.583) | 704.05 |
| branches | 348,287.125 (342,549.458–353,006.541) | 821.312 (789.375–885.167) | 424.06 |
| calls | 94,889.042 (93,781.042–105,275.750) | 227.791 (220.208–255.041) | 416.56 |
| fibonacci | 110,080.500 (108,577.500–111,504.458) | 357.083 (344.750–374.334) | 308.28 |
| arrays | 66,748.750 (65,727.208–85,898.084) | 87.354 (68.959–109.750) | 764.12 |
| maps | 47,415.375 (46,634.000–49,760.958) | 78.083 (68.042–95.875) | 607.24 |

Per-process VM/Lua ratios, in forward/reverse order: entry 104.12/100.94,
arithmetic 692.30/717.73, branches 431.51/409.19, calls 417.07/419.25,
fibonacci 310.43/307.29, arrays 754.83/772.86, maps 570.20/653.45.
The tiny entry workload measures the public host-call protocol, not arithmetic
throughput. Collection timings have more variation; that does not explain the
hundreds-fold gap in both independent processes.

For arithmetic, separate setup medians are: Kagari engine 190.511 ms,
source-to-artifact 589.760 ms, artifact preparation 270.256 ms, runtime creation
107.742 ms and linking 2.429 ms. Lua state creation is 0.065 ms, source-to-chunk
0.015 ms and module initialization 0.002 ms. The setup products do different work;
these numbers are not part of the interpreter throughput ratios.

Raw evidence: `target/lua-comparison/20261005T230702Z/results.json`, with its two
CSV files, build log and metadata/source/binary hashes. Raw samples are disposable;
this report and paired sources preserve the durable result and reproduction.

### Execution diagnosis

`profile_lua_macos.py` builds the same release configuration without a debug/profile
override, then runs each workload in a fresh process. After three warm calls it
samples a ten-second execution window using `/usr/bin/sample PID 5 1` (five seconds,
one-millisecond requested interval). It rejects windows that may overlap the
subsequent instruction-counting pass. No execution observer or Lua hook is enabled
in the sampled window. Instruction counts use separate checked observer/hook calls;
GC counts cover the complete ten-second window, divided by completed calls.

The six reports contain 24,143 main-thread samples and no additional sampled
threads. These are wall-clock stack samples, not exact CPU times. Optimized inline
attribution is incomplete; duplicate linker symbols remain explicitly unattributed.
The table sums the sampler's collapsed leaf symbols reported at least five times,
so small omitted symbols can undercount a family. Families are disjoint; percentages
must not be added to inclusive parent-stack percentages. Sampled execution durations
are never pooled into the throughput results.

| Workload | Samples | SipHash leaf | Frame/session access | Termination/allowed checks | Fetch/clone | Root get/set | Duplicate symbols, unattributed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 3,998 | 13.78% | 22.81% | 15.56% | 12.76% | 5.30% | 10.63% |
| branches | 4,028 | 12.56% | 21.65% | 15.44% | 14.35% | 5.76% | 11.47% |
| calls | 4,000 | 13.88% | 22.00% | 14.42% | 11.95% | 5.67% | 10.15% |
| fibonacci | 4,060 | 13.42% | 22.41% | 12.81% | 11.43% | 4.14% | 10.71% |
| arrays | 4,027 | 9.83% | 11.55% | 8.57% | 8.10% | 2.81% | 7.55% |
| maps | 4,030 | 10.35% | 12.70% | 9.53% | 8.41% | 3.28% | 9.85% |

Frame/session includes `ExecutionStack::{current,current_mut,validate_top}` and
`SessionStore::{frames,frames_mut}`. Termination includes
`ResourceState::{termination,ensure_execution_allowed}`. Fetch is
`ExecutionFrame::next_instruction`; roots are `RootSet::{get,set}`. Hash is only
the `Sip13Rounds` leaf, not all hashing. In arithmetic, its parent stacks resolve
to session-frame lookup, even though the script has no map. Collections additionally
show module-key hashing and allocator work; not all their hashing belongs to frames.

| Workload | Kagari instructions/call | Lua instructions/call | GC collections/call | GC object allocations/call |
| --- | ---: | ---: | ---: | ---: |
| arithmetic | 750,014 | 250,007 | 0 | 0 |
| branches | 960,016 | 309,968 | 0 | 0 |
| calls | 240,014 | 110,007 | 0 | 0 |
| fibonacci | 262,691 | 120,400 | 0 | 0 |
| arrays | 96,030 | 42,012 | 2 | 1 |
| maps | 81,030 | 24,012 | 5 | 2,001 |

GC object counts exclude Rust allocator traffic, root records, vectors and leases.
Thus zero script allocations does not mean zero host allocation. Arithmetic spends
only 41/3,998 (1.03%) reported leaf samples in `numeric::binary`; shared arithmetic
helpers are not the primary measured cost. Dynamic instruction counts have different
meanings across VMs and are not themselves a speed ratio.

Current arm64 headers are `Value = 104 bytes` and
`BytecodeInstruction<DefinitionId> = 136 bytes`, excluding owned payloads. Arithmetic
executes 250,003 LoadLocal, 100,002 StoreLocal and 100,003 LoadConst instructions:
450,008/750,014 instructions (60.0%) are those transfers/constant loads. The code
uses registers but still transports most local values through separate local slots.
Instruction-count inflation is about 3x here, while elapsed time is 704x; both the
number of operations and their execution protocol need attention.

Code inspection explains the measured paths:

- `kagari-vm/src/executor/mod.rs` reacquires the current frame several times per
  instruction, polls GC/cancellation and enters the observer lookup even when absent.
- `kagari-runtime/src/session/store.rs` stores frame vectors in
  `HashMap<SessionId, Vec<ExecutionFrame>>`. Repeated frame access rechecks the session
  and hashes its identity. The samples confirm this cost in scalar code.
- `kagari-runtime/src/frame.rs` routes registers and locals through `RootSet`,
  validating execution again for each access. `gc/roots.rs` checks owner/generation
  and lease identity; its entry lookup temporarily creates a Weak through
  `Arc::downgrade`. That exact atomic cost was not isolated by this sampling run.
- Fetch clones the instruction enum. Operations with owned vectors/type records can
  additionally clone payloads; header size alone is not a full traffic measurement.
- Script frames allocate slot vectors/root leases/metadata. Collection samples also
  reach module retention, type normalization and Rust allocation paths. The arrays
  report has only one GC object allocation per call, despite substantial allocator
  samples. Maps allocate an ordinary Option result for each of 2,000 lookups.

Raw diagnostic evidence is under
`target/lua-comparison/20261005T230954Z-macos-profile/{workload}/`:
`sample.txt`, `execution.log`, sampler/error logs and top-level metadata. This evidence
supports priorities below; it does not isolate the speedup of any proposed change.

### Architectural direction

The central runtime ownership and checked host API remain the right boundary.
The problem is that internal instruction execution repeatedly traverses services
intended for externally retained values and reentrant calls. Keep host leases,
generations and validation, but give verified execution its own efficient storage
and access discipline.

Two primary-source references inform the proposal:

- [Lua 5.4 opcodes](https://www.lua.org/source/5.4/lopcodes.h.html) use compact 32-bit
  instructions. [Lua's interpreter](https://www.lua.org/source/5.4/lvm.c.html) keeps
  current code/constants/base locally, gates tracing through a trap flag and
  publishes/restores state around operations that can error, collect or relocate
  the stack. [GC thread traversal](https://www.lua.org/source/5.4/lgc.c.html) visits
  the execution stack directly. These public pages currently show 5.4.9; the
  measured dependency is 5.4.8, whose vendored sources contain the same mechanisms.
- [V8's Ignition design](https://v8.dev/blog/ignition-interpreter) describes a compact
  register/accumulator interpreter with fewer unnecessary register transfers.
  The relevant lesson is execution-oriented representation and lowering, not adding
  a JavaScript-style speculative JIT to this interpreter task.

Apply those ideas to Kagari's static typing and existing checked contracts:

1. **Execution storage and access.** Runtime owns a reusable contiguous value stack
   with frame windows. The VM acquires a validated execution cursor instead of
   resolving session identity for every operand. GC traces active and suspended
   frame windows directly; persistent host roots remain in the checked lease table.
   Publish PC/roots and release transient borrows before allocation, observation,
   native calls and reentry; reacquire and validate on return. No references may
   survive stack growth, GC or nested execution. Preserve current cancellation and
   observer program points initially, making disabled paths cheap. Changing their
   frequency requires a separate explicit semantic decision.
2. **Compact executable code and slots.** Lower verified facts into small typed
   operations and indexed constant/type/call tables. Dispatch does not clone owned
   semantic records. Use checked i32 operations for statically known i32 registers;
   generic values retain tags and validated boundaries. Choose compact slot layout
   together with tracing/stack maps and exact integer/float/handle requirements;
   do not mandate NaN boxing or truncate generations. Keep wire validation and
   debug origins; do not add a second semantic implementation.
3. **Register allocation and script calls.** Keep ordinary locals in their assigned
   register window; eliminate redundant copies/constant traffic through a general
   lowering pass. Calls use stack windows plus return destinations rather than
   per-call root tables and metadata vectors. Preserve aliased/captured cells,
   left-to-right evaluation, checked-arithmetic traps and generation-pinned callees.
   Verify debug-variable locations and observable points after coalescing.
4. **Prepared native and collection boundaries.** Reuse linked call descriptors,
   instantiated type evidence and pinned dependency scopes through the execution
   scope. Typed native ergonomics stay intact; temporary in-call views should not
   manufacture durable host roots repeatedly. Only escaping values need persistent
   leases. Profile again before selecting ordinary enum/Option representation work;
   preserve allocation failure, identity and mutation commit semantics. Do not
   introduce a HashMap-specific shortcut in generic VM dispatch.

The [roadmap](../../docs/implementation-roadmap.md#interpreter-performance-follow-up)
owns these proposed implementation checkpoints. This benchmark task does not claim
that one stage, a GC replacement, a different hash function or inlining alone will
close a hundreds-fold gap. Collector algorithm replacement, JIT expansion and source
compilation optimization are outside this measured interpreter work.

Acceptance target: interpreter/Lua median time <= 1.0 on each nontrivial paired
workload in repeatable same-machine release runs, with setup reported separately
and no hidden native execution. Near parity, expand samples/processes and quantify
uncertainty before declaring success; do not hide a regression in a geometric mean.
Track host-entry latency separately and retain its own improvement target. Extend
coverage to strings, objects, closures, traits and real host callbacks before making
any whole-language parity claim. Required correctness includes traps/side effects,
roots under forced collection, cancellation, observer PCs, synchronous reentry,
call depth, old-generation calls and source-free verification.

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
heap-backed `Option` results: [`map_get`](../../crates/kagari-stdlib/src/bindings/hash.rs)
calls [`option`](../../crates/kagari-stdlib/src/bindings.rs), which
allocates an enum object (links follow their current stdlib owners). This fixture performs 2,000 gets plus one map allocation,
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

## Execution architecture diagnostics (HP00)

Allocation and protocol counters are opt-in and separate from timing:

```sh
cargo build --release --locked -p kagari-lua-benchmark --features diagnostics
target/release/kagari-lua-benchmark --diagnostics
target/release/kagari-lua-benchmark --diagnostics --source-forms
```

On the development macOS machine, prefix Cargo commands with
`DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Diagnostic builds reject
throughput mode. Rebuild without `--features diagnostics` before timing; the
ordinary driver does this automatically. Neither allocator nor runtime counters
are compiled into the default build.

Each diagnostic entry reports its first execution (`phase=cold`), checks two more
warmups, then reports a fourth execution (`phase=warm`). Each source-form/protocol
probe now starts with a freshly linked runtime; preparation/linking stay outside
these counters. Host entry/result retention and ordinary GC are included. Counts cover successful system
allocation/reallocation requests, bytes requested and net allocated bytes on the
current thread; net bytes may be negative when GC frees earlier allocations.
They are not peak memory or bytes retained after collection. Heap object counts
include reclaimed objects; environment/application deltas report live records.
Runtime counters report method/shared preparation attempts, environment allocation
attempts, non-program metadata graph validation entries and prepared-region exits
to ordinary dispatch. They include same-thread synchronous reentry. Cold counts include first-call descriptor preparation; they do not measure
instruction cost or time saved. The HP00 warm baseline used one shared runtime
for source forms, so differences in GC/live deltas require that context.

Source-form diagnostics also run a separate 2,500/5,000-iteration protocol matrix:
fixed versus alternating interface receivers, and fixed versus alternating receiver
and generic argument types. The alternating receiver changes at the same callsite;
i32/i64 arguments use two statically typed callsites and all four receiver/type
combinations. Independent Rust checksums check receiver selection and results.
A separate `T: Ord` comparison probe measures witness preparation that unbounded
identity calls do not exercise. These probes do not replace or modify the frozen
Lua comparison fixtures.

On macOS, use the ordinary release binary for stack sampling:

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/profile_lua_macos.py arrays maps
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/profile_lua_macos.py --source-forms shared_generic interface byte_state string_calls
```

The driver records hashes/environment and samples five seconds inside a ten-second
warmed execution window (three Kagari warmups for original workloads, six for
source forms including their paired warmup pass). Observer instruction counts are collected afterward;
Lua hooks run before the window. These are wall-clock stack samples, not exclusive
CPU percentages. Run sampling, counters and throughput serially, without concurrent
builds/tests. Keep generated output under ignored `target/` and durable conclusions
in the active execution plan or performance report.
