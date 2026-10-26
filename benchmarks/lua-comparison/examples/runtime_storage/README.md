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

The model uses the proposed 4,096-byte payload slabs, alignment at most 16 and strides:

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

These probes measure the current synchronous full collector only. They establish
comparison baselines, not future pause measurements. The active PG plan owns the
numerical acceptance decision and work-unit contract. Future instrumentation must
separately report normal steps, escalation/assists, completed-cycle work and time,
and indivisible provider scan/drop or allocator operations. A pause gate on normal
bounded steps cannot silently omit an entire metadata graph or container traversal
as one unit, nor classify the whole sweep as an indivisible destructor. Exemptions
must identify actual operations, their duration/size/count and their inclusion in
full-cycle cost. Explicit `collect_garbage` still means completed collection, not
one incremental step. No hard real-time guarantee, native backend proof, exhaustive
size/platform coverage or new semantic test-suite acceptance is claimed here.
