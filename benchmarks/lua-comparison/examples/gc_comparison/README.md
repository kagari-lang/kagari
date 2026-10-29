# Kagari / PUC Lua graph and GC comparison

Run `uv run python scripts/benchmark_gc.py` from the repository root. The default
is seven sequential processes for ordinary timing and seven for allocator accounting.
`--check` builds and validates both images without a performance claim. Output and
raw samples live under ignored `target/gc-comparison/`; lasting measured results
belong in `docs/performance-baseline.md` and the existing GC plan.

The ordinary binary and the `diagnostics` binary are separate. Both builds and
semantic checks finish before timing. Each explicit timing process uses three
warmup lifecycles and eleven measured lifecycles, each with a fresh runtime/state;
engine order alternates by lifecycle and process. The estimator is the median of
within-process Kagari/Lua ratios across processes. Raw medians/ranges are retained;
this is not a confidence interval. There is no newly invented cross-language pass
threshold, and it does not replace Kagari's frozen before/after gates.

## Matched work

| Case | Graph |
| --- | --- |
| points_512 | 512 distinct two-integer records in one growable sequence |
| points_8192 | 8192 such records |
| cycles_4096 | 4096 records, each with an edge sequence containing itself, in one outer sequence |

Kagari uses nominal structs and Vec; Lua uses ordinary record/array tables. Both
scripts build the graph, preserve aliases and use while loops with equal bounds.
No Rust helper performs allocation, traversal or mutation for either script. All
integer inputs fit both numeric domains. Lua indexing is one-based. Immutable
field-name keys do not imply equal physical representations or type-system costs.
The lifecycle separately measures construction, full live GC, keeping every 64th
record, full sparse GC, and full GC after dropping the last root. Checks outside
timers verify every value and mutation through cyclic aliases before and after
sparsification. Kagari additionally checks exact managed-object counts. Lua has no
matching public object-count API; its memory balance and complete state teardown
are measured independently. Check mode also exercises empty/singleton graphs.

Compilation, loading/linking, Lua chunk initialization and the initial collection
are outside operation timers. Host entry, result conversion/rooting and ordinary
call boundary costs are included in script operations; explicit GC timers contain
only the public full-collection call. Roots are dropped before the release-GC timer.
There are no weak tables, finalizers, host resources, reloads or JIT routes. One
full GC call is timed in each phase; the workload does not rely on finalizer-driven
resurrection or multi-cycle finalization.

## Automatic collection

Each engine uses its ordinary automatic collection policy in a separate workload:
128 calls allocating 256 records each, retaining the last eight batches. Points
and self-cyclic Nodes are both covered. Three full warmup workloads precede eleven
measured workloads. Each measured process therefore has 1408 batch samples per
engine/case. Report upper median, nearest-rank p99, maximum, and summed batch time.
The maximum is an observed maximum, not a latency guarantee. **These are allocation
call latencies, not isolated GC pause measurements.** Lua's public API provides no
matching automatic-pause observer. The final explicit drain is outside these batch
timers; after-batch garbage and peak backing are retained in the memory report.

Kagari keeps its default 1024 logical-unit trigger and runtime policy. Lua 5.4 uses
incremental mode with pause=200, step multiplier=100, step size=13. These are the
Lua defaults, not equivalent work budgets. Explicit-GC cases disable automatic
collection in both engines. See the primary [Lua 5.4 GC contract](https://www.lua.org/manual/5.4/manual.html#2.5)
and mlua 0.12.2 `state.rs` (`gc_set_mode`, `gc_collect`). No claim of generational
Lua or LuaJIT parity follows from this incremental configuration.

## Memory boundary

The existing diagnostic global allocator counts requested bytes in both engines:
mlua 0.12.2 `memory.rs` routes its Lua allocator through Rust `std::alloc`, so Lua
allocations are included alongside wrapper/runtime metadata. Three complete
lifecycles per process record setup, live/sparse/empty deltas, post-setup peak delta,
whole-lifecycle peak and exact zero-net teardown. Automatic cases additionally
record backing after the batches, before a forced collection, and after live/empty
collection. Use per-process maxima of the three repetitions.

The scope encloses creation and destruction of Runtime/Lua state; output and
observation bookkeeping are outside it. Kagari's shared compiled/prepared program
is outside the scope and warmed first; Lua's loaded chunk belongs to its state.
Therefore setup and whole-lifecycle bytes describe the different embedding owners,
not identical compiler/code costs. Primary graph comparisons use deltas from each
engine's setup, but those deltas still include workload-induced runtime metadata.
The counter excludes allocator overhead, other threads and transient backing inside
realloc. It is not RSS. Report retained garbage, empty-state metadata and peaks,
not only compact live payload bytes. No automatic-pause or total-process-memory
claim can be inferred from these rows.
