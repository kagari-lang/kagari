# Standalone GC benchmarks

The durable entrypoint is [`scripts/benchmark_gc.py`](../../scripts/benchmark_gc.py).
It reuses this crate's checked example workloads rather than implementing another
collector adapter. The suite characterizes the standalone library, not Kagari
Runtime: Runtime still uses its existing collector. The
[active integration plan](../../docs/gc-redesign-plan.md#acceptance-requirements)
owns open performance gates and broader workload coverage.

## Run and compare

Run from the repository root, with Cargo/Rust and `uv` available:

```text
uv run python scripts/benchmark_gc.py --output target/gc-benchmark/before
```

After changing GC implementation code:

```text
uv run python scripts/benchmark_gc.py --baseline target/gc-benchmark/before --output target/gc-benchmark/after
```

Both commands use seven sequential process repetitions per suite. The second
command rebuilds the candidate once, then alternates archived-old/new and
new/archived-old process order by repetition. It measures the old executable
again under current conditions; it does not divide today's timings by a historical
report. Each matching pair receives the same process index, and suite order rotates.
No source checkout, Git reset, stash or branch operation is needed. A comparison's
candidate archive can itself become the baseline for a later change.

For a focused loop, select the same suites when creating and comparing a baseline:

```text
uv run python scripts/benchmark_gc.py --suite sustained --suite burst --output target/gc-benchmark/allocation-before
uv run python scripts/benchmark_gc.py --suite sustained --suite burst --baseline target/gc-benchmark/allocation-before --output target/gc-benchmark/allocation-after
```

Use `--runs 1` for a functional/exploratory run, not a performance conclusion.
`--timeout` bounds each fixture process (default 180 seconds);
`--build-timeout` bounds the focused release build (default 600 seconds).
Timeouts/nonzero exits/schema failures retain logs and a failed `state.json`,
return a nonzero status and do not publish a successful report. Output directories
must be new; evidence is never overwritten. Omitting `--output` creates a UTC
timestamp directory under ignored `target/gc-benchmark/`.

No dependencies beyond Python's standard library are added by the runner. It uses
workspace release/default features, target and Cargo build parallelism; overrides
to profiles/target directory/build target are rejected. Other relevant compiler
and build environment overrides are recorded and must match for comparisons.
Keep the machine idle: do not run builds, tests, profiling or another benchmark
concurrently. CPU affinity, frequency, power state and background activity are not
controlled. Matching metadata is necessary, not proof of identical conditions.

## Workloads

| Suite | Existing fixture | Contract measured |
| --- | --- | --- |
| `access` | `field_access` | Warm plain Rust, typed/dynamic scalar access; per-operation/batch loans |
| `access-borrow` | `field_access --dynamic-borrow` | Dynamic scalar loans |
| `access-alternating` | `field_access --alternating` | Scalar updates across 32 objects |
| `lifecycle` | `arena_comparison` | Kagari/gc-arena scalar/root copies and allocation, live collection, root clearing, dead collection at 512/8192 leaves |
| `lifecycle-prepared` | `arena_comparison --prepared` | Same lifecycle with Kagari descriptor registration outside allocation timing |
| `references` | `reference_access` | Typed/dynamic reference replacement in Idle/Mark/Sweep; two already-retained targets |
| `collection` | `collection_profile` | 8192 unique leaves, duplicate roots or one-root chain; individual steps, stage aggregates and independent full collection |
| `sustained` | `sustained_allocation` | 128 batches of 256 allocations, 512-slot live ring, one automatic safepoint per batch; leaf/self-cycle cases |
| `burst` | `sustained_allocation --burst` | Same ring, with 4096 allocations replacing every sixteenth batch |
| `capacity` | `metadata_capacity` | 8192 page/root/descriptor entries, highest-ID survivor, then empty; requested table/buffer capacity |

The fixed timing fixtures validate three warmups (samples 0–2), then use eleven
measured samples (3–13). Capacity is an exception: one high-low observation per
process, without internal warmups. Existing result/liveness/reclamation assertions
remain active in release builds. CSV validation checks the complete expected
matrix, sample identities and workload sizes, not just whichever rows arrived.

These are intentionally different contracts. In particular, gc-arena does not
have identical resumable-root or retirement semantics. Its rows are context, not
a collector-wide parity score. Repeated access is warm native Rust, not individual
hardware instruction latency or script throughput.

## Outputs and estimates

Each output directory contains:

- `report.md`: all candidate metrics, process-estimate ranges, old medians and paired changes.
- `summary.csv`: the same estimates for spreadsheet/chart tools.
- `report.json`: metadata, commands, raw-file hashes, per-process estimates and summary.
- `raw/`: every process's complete CSV, including warmups, and stderr log.
- `bin/`: the exact candidate executables with recorded SHA-256 hashes.
- `build.jsonl` / `build.stderr.log`: focused Cargo build output; compilation is separately timed.
- `state.json`: running/failed/complete state and accumulated evidence.

The runner records revision, dirty status/diff hash, GC source hash, fixture and
analysis-code hashes, build configuration, Cargo.lock, compiler, OS/CPU identity
and relevant build flags, including `CARGO_BUILD_*` settings. Cargo configuration
files in the repository, its ancestors and Cargo home are fingerprinted without
copying their potentially sensitive contents into reports. Comparison rejects
changed workload/analysis recipes,
suite selections, manifests/configuration, lockfile, toolchain or recorded machine
settings. Ordinary GC source changes are expected and allowed. Start a new baseline
when a measurement recipe changes; do not disguise it as the same measurement.
Archived raw CSV and binaries are hash-checked and raw data is revalidated before
use. Source/environment stability and archive hashes are checked again after a run.
These files are local development artifacts: copy a whole result directory if it
must survive `target/` cleanup; successful command histories do not belong in docs.

Estimates follow the existing process-boundary approach of
[`benchmark_lua.py`](../../scripts/benchmark_lua.py):

- Access/lifecycle/reference times are median measured batch ns/operation **within
  each process**, followed by the median of process estimates.
- Collection stage/full times and work counts are per-process sample medians.
  Stage durations sum incremental slices, not an independently timed full collect.
  Transition slices remain separate; a slice can cross collector phases.
- Step and batch p50/p95/p99 use nearest-rank observations within a process; the
  report summarizes process percentiles. It never pools different processes into
  one artificially large latency distribution. `max` rows also retain the range
  of process maxima, whose upper end is the overall observed maximum.
- Sustained throughput is total timed batch work divided by total allocations,
  rather than an unweighted mean of differently sized ordinary/burst batches.
  `gc_service_fraction` uses the same window and excludes final cleanup.
- Step/assist counts and total times are medians of per-run totals. Unobserved
  event latencies are `null`/`unobserved`, never synthetic zero-cost events.
- Paired change is the median of index-matched candidate/baseline ratios, minus
  one. A zero baseline or missing observation yields `n/a`, not infinity or a pass.
  Negative changes favor lower-is-better metrics; positive changes favor throughput.
  Work, cycle and event counts have neutral direction: fewer cycles alone is not a win.
- `observations` identifies the estimator's inputs: 11 samples for typical batch
  medians/counters, pooled measured events for process tails, one for capacity.
  Raw samples and process values remain available for inspecting outliers.

## Sustained timing and memory boundaries

Normal batch `allocation_ns` includes the allocation scope, initialization and
required frame writes. `total_ns` includes that work and its safepoint. Diagnostics
and CSV output are outside the timers; sustained/profile output is buffered so
printing does not interrupt measured batches. Buffers and instrumentation still
perturb cache/process memory and are not production overhead measurements.

`service_ns` already includes its maintenance/destruction suffix. Do not add
`maintenance_ns` again. After the window, live-ring values are checked and frame
roots released outside the timer; `cleanup_ns` then measures only `collect()`.
`lifecycle_ns_per_object` adds this final drain to the batch totals. It does **not**
include initialization, validation, frame release, heap Drop or complete teardown.
The final drain may finish an active cycle and a fresh cycle; reports preserve
actual cycle counts. It is not interchangeable with the total cost of prior steps.

`before_objects` is pre-batch occupancy. Since no collection occurs inside the
allocation scope, `observed_peak_objects` includes pre-batch occupancy plus that
batch's allocations. `peak_managed_bytes` is the library's backing-plus-indirect
high-water mark across the measured samples (including the drain), excluding
root/source storage, metadata, Arc/control allocations and allocator overhead.
Quarter-window managed-byte medians and end-window bytes expose trends but do not
prove indefinite stability. Capacity rows separately report actual requested
page/root/descriptor/queue storage; there is no invented combined RSS counter.
Peak-stage collection timing columns in the capacity CSV are untimed zero
placeholders, not zero-latency collections.

## Interpretation and remaining coverage

Read throughput, normal-step/assist tails, memory peaks and cycle progress together.
The command reports measurements, not an arbitrary aggregate score or automatic
regression pass/fail. Existing scenario-specific acceptance belongs to the active
plan; noisy tiny-loop ratios and small-fixture p99 are not production SLOs.

This initial durable suite does **not** cover first-white-target barrier latency,
large realistic mutable containers, broad mixed-size/alignment or sparse-page
fragmentation, resource-owning destructor tails, complete requested-allocation/RSS
accounting, reclamation-age distributions, long-duration production stability,
equivalent sustained gc-arena pacing, or Runtime integration. Those remain at the
existing plan owner rather than silently expanding this benchmark checkpoint.

The fixture/parser and runner contracts are tested independently:

```text
uv run python -m unittest discover -s scripts/tests -p "test_gc_benchmark_*.py"
```
