# Standalone GC benchmarks

The durable entrypoint is [`scripts/benchmark_gc.py`](../../scripts/benchmark_gc.py).
It reuses this crate's checked example workloads rather than implementing another
collector adapter. The suite characterizes the standalone library, not Kagari
Runtime. Runtime now uses this collector through ManagedMemory/RootDriver, but
standalone results do not establish script/Host throughput. The
[roadmap](../../docs/implementation-roadmap.md#gc-library-redesign) owns remaining
work; this guide retains comparable workload recipes and inherited numerical gates.

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
regression pass/fail. Scenario-specific acceptance is retained below; noisy tiny-loop ratios and small-fixture p99 are not production SLOs.

This initial durable suite does **not** cover first-white-target barrier latency,
large realistic mutable containers, broad mixed-size/alignment or sparse-page
fragmentation, resource-owning destructor tails, complete requested-allocation/RSS
accounting, reclamation-age distributions, long-duration production stability,
equivalent sustained gc-arena pacing, or Runtime integration. Those remain at the
[GC follow-up owner](../../docs/implementation-roadmap.md#gc-library-redesign)
rather than silently expanding the standalone suite or claiming Runtime coverage.

The fixture/parser and runner contracts are tested independently:

```text
uv run python -m unittest discover -s scripts/tests -p "test_gc_benchmark_*.py"
```


## Acceptance requirements

This protocol defines comparative acceptance, not a claim that every fixture
or inherited integrated workload has passed it. Evaluate throughput, latency and memory together. A smaller step is
not a win if equivalent useful work takes longer, assists grow, or unreclaimed
memory accumulates. Correct results, retention and cleanup are prerequisites for
every measured sample, with no tolerance for semantic failures.

The current [PG00 frozen gates](https://github.com/kagari-lang/kagari/blob/0490f8a4071f11ffcb98679f0eb3dd67eb8c9854/docs/paged-incremental-gc-design.md#pg00-baseline-handoff--measured-baseline-and-frozen-gates-2026-10-26)
remain the numerical authority for inherited workloads. Reuse their scenario
associations and later explicitly reviewed refinements; do not copy a historical
time from another machine or replace an existing denominator with the first new
library result. A passing standalone benchmark cannot close integrated PG/NC gates.

### Measurement boundaries and baseline ownership

Use two complementary layers:

- **Standalone library:** a Rust-only consumer measures allocation, roots, access,
  provider mutation, incremental tracing and disposal without compiler/VM/module
  overhead. Its fixtures belong with kagari-gc;
  do not create another Runtime-dependent collector harness. Freeze the
  workload recipe and measurement method before optimization. A correct implementation
  can establish an additional standalone regression baseline before optimization;
  it does not reset inherited Runtime obligations. Until a comparable standalone
  baseline and limits are frozen, new rows report characterization, not a pass.
- **Integrated Runtime:** reuse [runtime_storage](https://github.com/kagari-lang/kagari/blob/0490f8a4071f11ffcb98679f0eb3dd67eb8c9854/benchmarks/lua-comparison/examples/runtime_storage/README.md)
  and the existing [GC comparison](https://github.com/kagari-lang/kagari/blob/0490f8a4071f11ffcb98679f0eb3dd67eb8c9854/benchmarks/lua-comparison/examples/gc_comparison/README.md)
  fixtures, drivers and assertions. Measure the complete script/host operation,
  including its required root transfers, reference publication and GC assistance.
  Compilation and artifact preparation remain separate. Go/Lua/gc-arena comparisons
  are optional contextual evidence with matched semantics, not substitutes for
  before/after Kagari acceptance or isolated measurements of our collector.

Separate collector-inactive, active incremental and sustained automatic-service
cases. Suppressing collection is only a bounded access/allocation diagnostic;
its accumulated memory and eventual cleanup must remain visible. Do not present
uncollected allocation throughput as sustainable GC performance. When comparing
architectures, fix logical graph/liveness, root ownership, seed, work amount and
service/pressure policy; report physical representation differences rather than
forcing incompatible layouts or silently giving one side more memory.

### Standalone workload set

Keep this finite set representative. It is not a full product of every size,
phase, type and feature. Reuse compatible existing fixtures rather than add a
second semantic implementation. Freeze exact recipes before candidate timing.

| Workload | Required shape and operation | Measurements and distinct purpose |
| --- | --- | --- |
| Allocation and initialization | Edge-free two-i64 points at 512/8192 objects; reference-bearing cyclic nodes at 4096; protect equivalent live results | Construction ns/object and bytes/object; include initialization/publication and necessary result roots, separate descriptor/setup cost |
| Repeated access and writes | 50000 scalar reads/writes and reference replacements under a protected owner; compare inactive marking with active marking | ns/operation and full-loop time; separate host Root admission from a reused Access interval; integrated VM row uses directly scanned frame slots |
| Host roots and root packs | 1/32/256 owning handles, clone/drop and fresh registration measured separately; tail/forward/seeded completion; replace existing fixed root slots | Admission/release ns, allocations, retained table capacity and root-scan work; distinguish shared-lease clone from a newly registered Root |
| Vec mutation | Reference Vec lengths 1024/8192/65536 backed by a fixed pool of 64 protected leaf targets; remove front/middle/tail with the receiver unstarted, partially scanned or complete | Whole remove time including returned Root; separate untimed setup; diagnostic repair work versus suffix length; verify contents and survivors after collection |
| Map mutation | Existing indexed-reference provider with 1000 scalar keys; insert/replace/remove with protected reference values | Operation time, publication and scan work; preserve its covered-move proof, without extending Vec repair to map buckets |
| Dense and sparse tracing | 512/8192 leaf graphs; 4096 cyclic nodes retaining 64, followed by three equal churn rounds | Live/dense/sparse/release collection time, visited slots/edges, committed backing and post-setup peak; detect retained-high-water scanning |
| Sustained allocation and reclamation | 128 batches of 256 allocations with a 512-target live ring, default automatic service; leaf and self-cyclic nodes have no links retaining older batches. Reuse the existing separately identified pressure-burst control | End-to-end batch throughput/latency, normal steps, assist frequency/time, cycle completion and retained-memory trend |
| Sweep and disposal | Existing mixed size/alignment pairs 8/8, 24/8, 48/16, 1024/16, 1056/16 and 64/64; cheap and explicitly identified resource-owning destructors | Total retirement/disposal time, maximum detached owners, allocator/backing release, actual destructor time and final zero-net owned teardown |

The sustained recipe must last multiple completed cycles and reach a documented
repeatable live-set regime before reporting steady-state throughput. Preserve an
allocation-only and an end-to-end lifecycle measurement so setup, final draining
or accumulated garbage cannot hide the cost. A continuously mutating workload
also reports eventual cycle completion under the specified cooperation policy;
short service steps without progress do not meet acceptance.

For the selected Vec removal, record graph position/cursor scenario and measure
the full ordered remove, not just integer adjustment. The expected mechanism is
constant scanner repair work and no suffix publication for pure removal; the
contiguous shift remains linear. Use a separate diagnostic run to explain work
counts and a timing run to measure the actual provider API. Counts alone cannot
prove a speedup. New targets still require publication and a returned host value
still needs retention. Do not compare this host result with an unrooted value or
silently substitute swap_remove. Insertion/reordering keep their separate costs.

### Metrics and acceptance rules

| Dimension | Required evidence | Acceptance rule |
| --- | --- | --- |
| Semantic validity | Checksums/contents, live survivors, collected cycles, result retention and complete teardown | Every sample correct; validation remains outside the operation timer but inside lifecycle/peak accounting where applicable |
| Mutator throughput | Construction/access/mutation time per useful operation, plus whole-workload elapsed time | Existing Vec/Map/root rows retain 1.05 x paired baseline; other inherited construction/access rows retain 1.10 x. New standalone limits must be frozen from a valid comparable baseline before optimization |
| Completed collection | Whole collection invocation and summed service time/work, including attributed assists, retirement, disposal and transition maintenance | Inherited limit max(1.20 x paired baseline F, F + 10 us); match the completion contract, including finish-active-plus-fresh behavior where required, and report cycles completed |
| Normal-step latency | Each process's step count, maximum and observed distribution; normal budget remains 1024 work units | Inherited median-of-process-maxima limit max(50 us, frozen corresponding F/4); preserve existing scenario associations. Zero observed steps is unobserved, not a pass; this is not a universal 50 us guarantee |
| Operation and assist latency | Complete allocation/removal/host-call or script-batch durations; assist count/total/max; indivisible provider/Drop/allocator durations | Report separately from normal steps; apply any existing operation gate. New percentile/absolute limits remain pending a fixed reference workload and supported hardware budget, not guessed from the work-unit count |
| Retained and peak memory | Absolute setup, live/empty backing, post-setup peak, whole-lifecycle peak, metadata/root/workspace/retired capacity; RSS separately | Inherited setup <= 1.10 x paired baseline; other applicable backing/peak bounds B + max(0.10 B, 4096 bytes), applied independently as defined by their existing owner |
| Reuse, progress and release | Equal live-set churn rounds, completed cycles, release notifications and scoped owned allocations after teardown | No unbounded growth at a fixed workload; stable repeated backing and zero-net owned teardown. Measure eventual reclamation under the declared cycle contract, not immediate destruction when an edge is removed |

The numerical rows summarize PG00 categories, not replacements for scenario-specific
bounds or later reviewed gates at the linked owner. Apply that owner's precise
mapping and estimator. For zero-net teardown, drop the heap and every test-owned
Root/RootSet or inert lease; separately identify intentional shared caches warmed
outside the scope. Keeping an external handle alive is not a leak-free complete
teardown measurement, even if its heap has already shut down.

Keep normal steps, pressure assists, explicit collect and indivisible work as
separate observations, all included in their enclosing workload/cycle totals.
Use exclusive attribution for totals: an assist already inside an allocation's
elapsed time is not added again to end-to-end time, and a disposal nested in a
step is not added again to that step. For a yielded cycle, separately retain the
sum of all its service invocations and its elapsed completion interval, including
intervening mutator work. A final explicit drain's timer alone does not measure
the cost of work performed by previous steps.
The existing 32-total detached-owner cap remains required, but neither this count
nor 1024 work units bounds arbitrary Rust destructor time. Likewise, Vec removal
and arbitrary host callbacks are not bounded by the collector step budget. Slow
operations need diagnosis at their actual owner rather than reclassification to
make a pause table pass. Do not exclude the paused disposal suffix from a step's
observed wall duration; retain its separate attribution too.

### Sampling and verification

For gate comparisons preserve seven alternating baseline/candidate process
pairs, three warmups and eleven measured lifecycles. Normalize against each
paired baseline first, then take the median of ratios; ratios of aggregate
medians are not gate results. Preserve every sample and maximum. Memory uses
the maximum of three measured diagnostic lifecycles per process before paired
normalization. Separate ordinary timing from instrumentation and report its cost.
Record revision/diff, lockfile, toolchain, machine, features/profile, heap settings,
cache state and workload; never overlap compilation/profiling with measurements.

Freeze a tail event stream before measuring p99 and target at least 10000 events
per process for an acceptance row. Publish N, per-process nearest-rank p99/max
and the largest maximum; eleven lifecycle samples cannot establish p99. Each
result needs an explicit pass/fail/unobserved/not-comparable status. No aggregate
score may hide a failed throughput, memory or latency gate.

Use existing semantic owners for live graphs, cyclic reclamation, mutation behind
scan cursors, root reuse, sweep admission, alignment, allocation/scan/destructor
failure and shutdown. Integration adds actual Value/Native/frame/reentry,
cancellation, parked reload and source-free boundaries. Do not dereference freed
memory merely to test rejection or add duplicate smoke suites.

Implementation checks follow [AGENTS.md](../../AGENTS.md): affected local tests plus
structure, fmt, workspace all-target Clippy and diff checks; exhaustive workspace,
feature/backend/platform tests belong to CI. Documentation-only changes require
content/link/diff checks. Passing selected tests does not close the complete plan.


### Current applicability and remaining owners

Runtime refactor RR00-RR05 records original call/root latency and allocator-event
peak gates in the [performance owner](../../docs/performance-baseline.md#rr05-ordinary-call-regression-check).
The separate [standalone old-kernel/current comparison](../../docs/performance-baseline.md#rr05-parked-root-admission-and-collector-characterization-macos-2026-11-14)
reports active and sustained collection plus publication regressions without
inventing a new frozen limit. No historical Windows time is a current denominator.

The original runtime_storage workload still fails source analysis for Vec before
execution; original GC-comparison/arrays/forms recipes also depend on withdrawn
providers. NC01-NC06 owns restoring those exact public collection routes and their
inherited PG00 Vec/Map/construction/access/collection/step/memory gates. Keep their
assertions, work amounts, policies and paired estimators; fixed-array substitutes
or partial-source extraction do not close them. [NC](../../docs/native-collections-plan.md)
records the concrete handoff. The old PG sequence is superseded by the integrated
library; its numerical history remains the authority linked above.

The [GC follow-up owner](../../docs/implementation-roadmap.md#gc-library-redesign)
retains first-white-target publication, mixed/sparse occupancy, destructor tails,
full allocation/RSS and longer sustained policies, plus provider-authoring proof
and broader fuzz/sanitizer/CI work. Existing small step streams do not meet the
10,000-event per-process percentile requirement. These are explicit open items,
not passing rows or a second Runtime integration migration.
