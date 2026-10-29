# Runtime storage probes

The no-argument example retains the original field/access and storage lifecycle
probe. `--families` adds a compact PG00 selection; `--slab-model` is an independent,
deterministic occupancy calculation. `--script-keys` selects only the script-key
HashMap family for focused timing. None of these modes implements the proposed collector.
No Cargo/profile/dependency changes or temporary source generation on disk are needed.

## Reproduction

Run from the repository root. Create an ignored output directory first, and use
bounded commands in automation. Never overlap measurements with builds or other
performance probes. Use default Cargo profiles, target directory and parallelism.

```sh
mkdir -p target/pg00-storage
cargo build --release --locked -p kagari-lua-benchmark --example runtime_storage
cp -p target/release/examples/runtime_storage target/pg00-storage/ordinary
/usr/bin/time -lp target/pg00-storage/ordinary --families > target/pg00-storage/families-0.log 2> target/pg00-storage/process-0.log
/usr/bin/time -lp target/pg00-storage/ordinary --families > target/pg00-storage/families-1.log 2> target/pg00-storage/process-1.log
target/pg00-storage/ordinary --slab-model > target/pg00-storage/slab-model.log
cargo build --release --locked -p kagari-lua-benchmark --example runtime_storage --features diagnostics
target/release/examples/runtime_storage --families > target/pg00-storage/memory.log
uv run python benchmarks/lua-comparison/examples/runtime_storage/summarize.py target/pg00-storage/families-0.log target/pg00-storage/families-1.log --memory target/pg00-storage/memory.log --json target/pg00-storage/summary.json
```

`/usr/bin/time -lp` is the macOS process-resource command; its maximum RSS includes
compilation, prepared products, all workloads and probe state. It is not GC-only
peak memory. Record OS/CPU/RAM, `rustc -Vv`, `cargo -V`, revision and dirty paths,
lockfile/source/executable checksums, build settings/environment and cache treatment
alongside the outputs. Retain essential conclusions in the active plan, not solely
in these disposable logs. The summarizer validates the full selected row set,
raw sample medians/extrema, live counts, three diagnostic repetitions and zero-net
teardown. It is not the paired before/after benchmark acceptance checker.

## Frozen workload contract

| Workload | Operation and checked outcome |
| --- | --- |
| `sparse_types_32x4` | Generate 32 distinct nominal declarations, four instances each, and 128 interface wrappers in one Vec. Check each method returns 3 after full GC. These are actual distinct types, not repeated instantiations of one type. |
| `mixed_native_128x6` | 128 six-element tuples in a Vec. Each tuple owns native payloads with size/alignment 8/8, 24/8, 48/16, 1024/16, 1056/16 and 64/64. Ordinary native registration/factories allocate the payloads. Trace and Drop check address alignment and first/last initialized bytes; payloads contain no script edges or indirect backing. These constant checks are included in GC timings. |
| `scalar_65536` | Grow a Vec of 65,536 i32 values (256 KiB element backing); check every value after live collection. |
| `references_8192` | Grow a Vec containing 8,192 distinct two-i32 Points; check every field after live collection. This is 128 KiB of Value backing on the measured 16-byte Value target, plus pointed-to objects. |
| `sparse_cycles_4096_keep64` | Allocate 4,096 self-cyclic Nodes, each with an edges Vec; keep every 64th Node. Retain the 64 cycles while constructing/reclaiming three more equal 4,096-node populations. Verify cyclic aliases remain mutable and exactly 129 objects survive each churn collection, then release all. |
| `retained_reload_4x256` | Prepare four versions before timing. Retain one closure with 256 Items from each version across three actual hot reloads. Check original closure-body constants plus the current named helper result, all four versions' presence, old-version reclamation after dropping three closures, and final object cleanup. |
| `script_keys_256` | Insert 256 nominal keys, read/update each, then read/sum. Script Hash produces 16 buckets; script Eq checks key codes. Shared counters prove at least 1,024 Hash calls and more than 256 Eq calls. Check the sum, length and every value after live GC, then release the map and callback state. Its `construct` row is the complete map workload, not allocation alone. |

All families use the interpreter and disable automatic collection. Explicit full
collection remains active. Compilation and artifact preparation are printed
separately. Each ordinary process performs three untimed warmup lifecycles followed
by eleven measured lifecycles per family, with a fresh Runtime per lifecycle.
`setup` includes load and the initial baseline collection, not source compilation.
`construct_reload` includes allocation and three reload publications, not compilation;
it is not an allocation-only throughput row. Result checking occurs outside the
reported action/GC timers. It can initialize caches before the subsequent release.
Current code retains `ScriptValue` owners while live, as ordinary SDK consumers do.

The reported median is per process. Combine processes by taking the median of their
sample medians, not pooling samples. Preserve process ranges and raw extrema; neither
is a confidence interval or a p99 estimate. For before/after conclusions use fixed,
alternating old/new processes and unchanged workloads/settings, rather than compare
unpaired historical medians. The existing Lua benchmark driver owns its paired
Vec/HashMap/root-churn controls; these storage probes do not replace them.

The diagnostics build installs the existing allocation counter and never reports
phase timings as performance evidence. Each family warms shared owners once and
measures three fresh Runtime lifecycles. Fixed-size observation records avoid
probe bookkeeping allocations inside snapshots. `net_bytes` records absolute
requested Runtime backing within the measurement; `net_delta_bytes` subtracts
that Runtime's post-load/post-GC setup. Keep both to detect setup regressions.
Allocation request/byte totals are cumulative since setup. Repetitions assert
identical per-phase net-byte deltas and exact zero-net Runtime teardown. First-use
shared cache retention is separate. `interval_peak_bytes` is the high-water signed
requested-byte balance since the previous capture; capture resets only the interval
peak, not cumulative counters. The setup interval begins before Runtime creation.
`family_peak` records setup peak, the maximum subsequent interval including final
teardown, that post-setup maximum minus the setup balance, and the whole-lifecycle
peak. Thus setup peaks cannot mask later operation/collector growth. Verification
and root drops between captures belong to the following interval; sparse churn
intervals include graph construction and collection, not GC alone. No output is
printed inside the measured scope.

No-argument diagnostics apply the same interval accounting to the original eight
storage scenarios at populations 1 and 512. Each of three fresh measured Runtime
lifecycles has its own preceding warm lifecycle and retains three equal reuse
cycles. `repetition` identifies the independent lifecycle; `memory_peak` includes
all intermediate reuse intervals and final teardown. The six printed phase
balances must agree across repetitions. Compare each process's maximum of three
values for every backing/peak metric, calculating setup-relative values within
each lifecycle before taking maxima. This replaces the older one-lifecycle probe
whose post-setup counter was not reset at setup; historical output cannot satisfy
the corrected method.

For seven alternating baseline/candidate process pairs, create a JSON manifest
with `pairs` entries containing `pair` (0 through 6), `baseline` and `candidate`
(distinct log paths relative to the invocation directory), then run:

```text
uv run python benchmarks/lua-comparison/examples/runtime_storage/storage_memory.py --pairs target/memory-pairs.json --output target/memory-summary.json
```

The evaluator requires complete three-lifecycle coverage, stable phase backing,
consistent peaks, zero-net teardown and all field controls. It retains repetitions
and input hashes and evaluates 144 metrics: six backing balances and three peaks
for each scenario/population. Setup uses 110% of paired baseline; later balances
and peaks use baseline plus the larger of 10% or 4,096 bytes. Acceptance uses the
median of seven candidate/bound ratios. Use the identical diagnostic harness on
both frozen runtimes, record both image hashes, and keep these runs separate from
ordinary execution timings.

High-water tracking occurs at every successful allocator event on the calling
thread, not by periodic sampling. It excludes pre-scope allocations, other threads,
allocator overhead and transient backing inside System::realloc (realloc atomically
replaces the old requested size in this model). These lifecycles enclose Runtime
allocation and release and warm shared owners first. General counter users must not
interpret a signed net balance as absolute process liveness. The measurements are
requested backing/peaks, not physical backing, process RSS or a disjoint accounting
of every collector category. Capacity remaining
after empty GC can be valid runtime identity/cache backing; empty objects and
Runtime teardown are independently checked. Reload deltas include retained program
metadata, not merely object payloads.

## Slab occupancy model

The model preserves PG00's historical 4,096-byte payload-slab proposal, alignment
at most 16 and the strides below. PG02's measured allocator correction selects
independent 1,024-byte slabs; this model is intentionally not retuned and must not
be mistaken for the current physical layout. Actual metadata-inclusive measurements
and unchanged acceptance gates belong to the
[PG ledger](../../../../docs/paged-incremental-gc-design.md#pg02-physical-model-correction-and-checkpoint-evidence).

Historical model strides:

```text
8 16 24 32 40 48 56 64 80 96 112 128 160 192 224 256
320 384 448 512 640 768 896 1024
```

Selection is the first stride at least the requested size and divisible by its
alignment. Size above 1,024 or alignment above 16 uses a separate requested allocation.
Placement follows allocation order within `(stride, scan-capability)` pools, does
not move surviving slots, and releases an empty page immediately. A comparison
isolates the cost of one pool per nominal type for the sparse-type case.

For every retained scenario, the output checks:

```text
live backing = live requested payload + class rounding + vacant slot bytes + page tails
```

Separate live requested allocations are included on both sides. Empty variants
exercise complete release. The finite valid size/alignment domain checks class
selection, and all class capacities/tails are printed. These are synthetic
**payload-only lower bounds**, not a replay of the complete runtime allocation
stream: Point fields use eight bytes, the sparse nonmoving scenario uses explicit
24-byte slots, and mixed native payload sizes use actual Rust size/alignment.
Interface wrappers, Vec records, struct headers, identity directories, descriptors,
page headers, mark bitmaps, collector queues and allocator overhead are excluded.
Adding those structures or reserving an in-page header changes capacity and must
be measured separately. In particular, the synthetic 24-byte sparse-slot case is
not claimed as the future Node representation.

The table alone does not guarantee low waste: small requests round to at least eight
bytes, stride 896 leaves a 512-byte page tail, and nonmoving sparse survivors can
pin nearly all backing despite immediate empty-page release. Do not infer actual
memory savings or pause improvements from these modeled byte counts.

## Incremental acceptance boundary

The frozen modes above retain explicit full-collection comparisons. The separate
`--automatic-families` mode below measures actual automatic activity. The active PG
plan owns the numerical acceptance decision and work-unit contract. Observations
separately report normal steps, escalation/assists, completed-cycle work and time,
and indivisible provider scan/drop or allocator operations. A pause gate on normal
bounded steps cannot silently omit an entire metadata graph or container traversal
as one unit, nor classify the whole sweep as an indivisible destructor. Exemptions
must identify actual operations, their duration/size/count and their inclusion in
full-cycle cost. Explicit `collect_garbage` still means completed collection, not
one incremental step. No hard real-time guarantee, native backend proof, exhaustive
size/platform coverage or new semantic test-suite acceptance is claimed here.

## Automatic-family observations

Use the same example in an **ordinary build**, without the benchmark `diagnostics`
feature or runtime execution-event instrumentation. No new Cargo feature or product
is required. The mode rejects the benchmark's counting-allocator build.

```sh
cargo build --release --locked -p kagari-lua-benchmark --example runtime_storage
# One lifecycle per family; validate behavior and observations, not timing gates.
target/release/examples/runtime_storage --automatic-families --check
target/release/examples/runtime_storage --automatic-families --check --pressure
# Only after measurement authorization: 3 warmups and 11 measured lifecycles.
target/release/examples/runtime_storage --automatic-families
target/release/examples/runtime_storage --automatic-families --pressure
```

`default` retains the production trigger floors (1024 logical units, 64 KiB).
`--pressure` is a separately labeled, predeclared 64-unit/4096-byte trigger-floor
profile, not a change to the 1024-work-unit service allowance or 32-item disposal
cap. The normal post-collection doubling rule still applies. Never pool policies
or substitute the pressure result for the default result. Pressure runs require
normal automatic activity in every lifecycle; default runs report inactivity rather
than manufacturing allocations or fake zero-cost step samples. The offline validator
requires real activity in every selected family for both policies.

Both policies reuse the seven existing family sources, populations, native payloads
and reload versions. Construction, verification/access and root-release safepoints
are observed as `Automatic`; live/release cleanup remains explicit and is separately
labeled `Explicit`. The original frozen modes, output rows, object/alias/key/reload
assertions and disabled-auto settings are unchanged. Those seven families introduce
no per-element roots or additional payload population. A separate, absolute-only host
pressure control is described below. Retained reload checks original
closure bodies with current named helpers; cycles still check 129 sparse survivors
and final cleanup. External-provider/native-backend proof remains with its tests.

Every measured phase starts/resets the public heap observation before its wall timer.
The first start's fixed observation allocation, later resets, snapshots, formatting
and final buffer release are outside that action timer; backing is not claimed zero
or included in the old 55-memory comparison. Runtime-owned invocation clocks exclude
only final telemetry publication and return-to-caller instructions. This opt-in
instrumentation still has overhead; record the exact build/source provenance.

Output retains:

- `auto_phase`: scope, action wall time (including mutator work), before/after object
  counts and whether a collector cycle was already active on entry.
- `auto_work`: separate `normal`, `pressure_assist`, `explicit`, `cycles` and
  `carried_cycles` count/failures, total/last/max work and duration. Action wall time
  is not GC pause. Carried-cycle totals include earlier windows and must not be
  attributed entirely to the current phase or added again to service durations.
- `auto_slices`: structural work only, including slices inside assists/explicit calls;
  these are not mislabeled as normal invocation pauses.
- `auto_indivisible`: all eleven operation kinds, including zero counts; items, work,
  known bytes, unknown-size count, duration sum and maximum, plus the slowest sample's
  owner/native TypeId where available. Known bytes are local/shallow, not necessarily
  freed bytes or transitive destructor cost. TypeId debug text is not a stable ID
  across builds. Raw normal pauses include indivisible operations: never subtract
  this maximum or total, or exempt the entire containing service.
- `auto_summary`: all eleven per-lifecycle normal maxima/counts and their **maximum**
  over the process, not their median. `auto_check` proves semantic assertions and
  actual automatic normal activity separately from explicit cleanup.

Validation also checks failure counts, work/time extrema, disjoint service scopes,
service/slice work equality, service time within action wall time, completed-cycle
counts against ordinary heap statistics, and unchanged final observation on release.
Zero assists/carried cycles are honest absence of that observation, not evidence of
assist latency. Explicit active-plus-fresh semantics are never divided into an
invented average cycle time.

Validate complete logs with the existing summary entrypoint:

```sh
uv run python benchmarks/lua-comparison/examples/runtime_storage/summarize.py --automatic --check target/default-check.log target/pressure-check.log --json target/automatic-check.json
uv run python benchmarks/lua-comparison/examples/runtime_storage/summarize.py --automatic target/default-0.log target/default-1.log --json target/automatic-summary.json
```

The parser rejects check logs as timing evidence unless `--check` is explicit,
missing/duplicate phases or categories, incorrect summary maxima and missing family
activity. It preserves every process maximum, their median and the absolute maximum;
it does **not** claim paired acceptance without corresponding frozen full-cycle
baselines. For each measured scenario, use fresh seven-pair baseline `F_i`, compute
`M_i / max(50000 ns, F_i / 4)` from that process's normal maximum, and take the median
of seven normalized values. Publish every `M_i` and the absolute maximum. Windows
with zero normal invocations have no latency sample, not a passing zero. The frozen
association below selects only full-cycle GC rows, never constructor/action wall time
or whichever phase happens to have the largest baseline. Keep full-cycle/assist/
indivisible results and the separate original 55-memory gates alongside the pause report.

### Frozen normal-step association

`automatic_baselines.py` is the executable owner of this premeasurement association.
It binds the unchanged logical family population, not an assertion that every earlier
incremental slice sees the final graph or that PG00 and PG05 have identical physical
object counts. Transient object counts, partial cycles and carried work remain in
raw output. Normal maxima are per phase, not a lifetime maximum reassigned afterward.
The same association applies separately to default and pressure profiles.

| Family / unchanged logical population | Automatic window | PG00 full-cycle row used for F |
| --- | --- | --- |
| 32 nominal declarations x 4 instances; 128 interface entries | `construct`, `access` | `live_gc` |
| 128 tuples x 6 native payloads | `construct`, `access` | `live_gc` |
| One scalar Vec of 65536 i32 values | `construct`, `access` | `live_gc` |
| One reference Vec of 8192 distinct Points | `construct`, `access` | `live_gc` |
| 256 script keys / 16 hash buckets, unchanged insertion/update/read work | `construct`, `access` | `live_gc` |
| Each of the preceding five families after its returned root is dropped | `release_boundary` | that same family's `release_gc` |
| 4096 cyclic Nodes | `construct` | `dense_gc` |
| Same dense graph reduced to 64 retained Nodes and their edge Vecs | `sparsify` | `sparsify_gc` |
| 64 retained cycles plus a new 4096-node population in round i | `churn_construct_i` | `churn_gc_i`, same i, never the slowest round |
| Verification of the retained 64 cycles after round i cleanup | `churn_access_i` | `sparse_live_gc` |
| Final 64-cycle root release | `release_boundary` | `release_gc` |
| Four retained generations x 256 Items, with three reload publications | `construct_reload`, `access` | `retained_gc` |
| Drop the first three closure generations, retain the fourth | `release_old_boundary` | `release_old_gc` |
| Drop the final retained closure generation | `release_all_boundary` | `release_all_gc` |

Explicit observation windows are not normal-step gates. Their slices cannot inflate
normal counts or replace the original full-GC throughput checks. The new host control
has **no PG00 association**: its distinct population must not borrow a reference-Vec
or root-control baseline. Assists are measured absolutely, never divided by a normal
step's 1024 units or put under a normal-pause gate.

For paired evaluation, `--automatic-pairs manifest.json` accepts a manifest containing
`association_sha256` (the canonical association fingerprint) and `pairs`. Each pair
contains `policy`, zero-based `pair`, `candidate`, `baseline_families` and
`baseline_script_keys` log paths. It must exactly cover the input candidate logs,
provide seven distinct process logs for each baseline role, and use the same
baseline pair for both policies. `baseline_families` supplies the six non-script-key
families; `baseline_script_keys` exclusively supplies script keys. PG00's family
mode also emits incidental script-key rows: preserve them in the source log, but
exclude them by declared role, never by their measured duration. The report records
every excluded row and rejects missing scenarios, missing associated full-cycle
phases or a process reused across roles. Baseline F is validated against eleven
raw samples and three warmups, and
is recomputed for each fresh process from the preserved PG00 images. The parser
rejects a changed association, duplicate pair, check-mode log or policy mismatch.
No historical median, constructor duration, summed family timings or PG04 result can
substitute for F. Output includes all paired F/M/limits/ratios, absolute maxima,
input hashes, and `passed=null` when a window has no normal observations in any pair.

### Forced pressure-assist and carried-cycle control

Both automatic policies additionally run `host_pressure_burst_2048x2048`. This is a
small host scenario in the same example, not a new benchmark product or an added
population inside a frozen family. It deliberately creates 2048 independent retained
host String roots, then one burst of 2048 unrooted Strings through public allocation
APIs with no intervening safepoint. The roots are explicit workload state, not
per-element rooting of a library container's internal edges; all are released.

- Start from a fresh Runtime with zero completed cycles and no explicit setup GC.
  Thus the first captured unit threshold is the configured floor, which is asserted
  below 2048 in both profiles; no previous cycle has doubled it.
- Admit the 2048 roots without collector service. One public safepoint must produce
  exactly one normal slice with `0 < work <= 1024` and no cycle completion. The root
  extent cannot fit in that slice; no private phase inspection or test-only budget
  is used.
- Reset observation during that active cycle. The burst must add exactly 2048 objects
  and more positive heap units than the first-cycle threshold, with zero collector
  calls and no intervening completion. Reset again immediately before the safepoint.
- That safepoint must produce exactly one pressure assist with work greater than 1024,
  zero normal/explicit invocations, one carried completion and zero newly begun cycle
  completions. Snapshots before/after prove classification and work; action wall time
  is not substituted for service pause. Carried work must equal first-slice work plus
  assist work exactly once.
- Read every retained String through public rooted handles. Explicit fresh cleanup
  must leave exactly the 2048 retained Strings; after root release, final explicit
  cleanup must return to baseline. Indivisible destruction remains separately reported.

`auto_work` retains absolute assist duration/work; summary JSON adds per-process assist
counts, total work/time, maxima, their median and absolute maximum. The control's
first normal slice is also reported, but is absolute-only rather than a fabricated
PG00 ratio. This makes absence of escalation in the natural families distinguishable
from actual forced-assist evidence. The mode and parser fail if the intended assist
or carried cycle does not occur; no pressure threshold is retuned after timing.
