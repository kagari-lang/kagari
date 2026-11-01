# Standalone kagari-gc and Runtime integration

The standalone library implements typed Rust and fixed dynamic objects, scoped
access, host roots, direct execution-root adapters and incremental collection.
Runtime still uses its existing collector. This plan owns remaining work and
acceptance; the [implementation design](gc-implementation-design.md) owns storage,
borrowing, tracing and mutation contracts, and the [crate guide](../crates/kagari-gc/README.md)
owns usage and fixture commands. The [roadmap](implementation-roadmap.md#gc-library-redesign)
owns activation of the integration phases.

## Scope and status

The library is a precise, nonmoving, nongenerational incremental mark-sweep
collector driven exclusively by its owner. It traces its own allocations from
host root leases and a complete driver-supplied execution root source. Semantic
IDs, language typing, execution policy, hot reload and code selection stay in
Runtime; host Rust state stays outside the script heap.

The standalone implementation is usable without language crates. Runtime cutover,
arbitrary erased providers and native extraction/tombstones remain deferred.
Concurrent/moving collectors, foreign graph stores or Domain bindings, generic
finalizers/resurrection, shared heaps across Runtimes, derive tooling and new
container libraries are outside this scope. The current fixed dynamic API does
not enforce language nominal types or implement all Runtime storage needs.

| Phase | Current state and remaining exit condition |
| --- | --- |
| GCR00: contract closure | Typed/fixed dynamic access, branded handles, provider mutation, cursor repair and direct root-source contracts are implemented. Close Runtime representation/extraction requirements and freeze comparable performance gates. General safe custom-object construction remains an explicitly reviewed provider responsibility. |
| GCR01: standalone heap | Allocation, roots, mutation, resumable collection, metadata reclamation and opt-in safepoints are implemented. Complete the remaining representation/diagnostic requirements selected for integration; do not invent placeholder APIs. |
| GCR02: Runtime object-model proof | Standalone frame adapters exist. Map every genuine root and managed relationship, prove object/code-state cycles and pinned computation lifetime, then measure actual frame/layout adapters. |
| GCR03: coherent Runtime cutover | Connect Value/layout/Native providers and direct frame scanning; rehome execution state and remove the replaced collector and graph owners. Not started. |
| GCR04: integration and performance closure | Complete selected integration regressions, paired standalone/integrated gates and CI; reconcile inherited obligations at their existing owners. |
| GCR05: documentation and cleanup | Move lasting behavior to architecture/spec/authoring owners, remove obsolete paths, transfer unresolved work and retire this plan. |

GCR01 and standalone GCR02 fixtures can coexist with Runtime because they own
separate heaps. Before GCR03, list affected crates and any finite temporarily
non-building migration boundary here, with its owning repair checkpoint. No
production stubs, disabled validation, duplicate semantic implementations or
compatibility facades are part of the transition.

## Remaining decisions and work

- **Runtime representations:** validate fixed layouts against packed Value fields,
  custom erased layouts, Native take/tombstones and outstanding aliases. Do not
  assume a transient Gc remains valid after adding an in-scope representation
  change. The [Runtime map](gc-implementation-design.md#runtime-integration-map)
  owns proposed responsibility changes.
- **Safe provider authoring:** GcObject, raw initialization and general mutation
  remain unsafe contracts. Opaque Edge is non-Copy/non-Clone; a higher-ranked Init
  callback alone cannot stop an unbranded owned edge escaping. Reviewed providers
  expose safe constructors and operations. A future builder/derive must prove
  field-complete tracing, source retention, publication, scanner repair,
  accounting and unwind behavior before it can accept arbitrary custom objects.
  It must not expose blanket safe mutable object access.
- **Complete execution roots:** adapt actual frames, parked tasks, return/argument
  transfers and host reentry to RootDriver. Validate slot reuse, changed frontiers,
  incoming publication, cursor repair and cleanup on every trap/cancellation path.
  No mirrored per-variable Root registration or foreign-store graph is selected.
- **Access intervals:** reuse prepared layout/offset facts only while their
  ownership and admission proofs hold. Release payload/frame loans before calls
  that may collect or reenter; root values that cross that boundary and reacquire
  transient loans afterward. Shared layout evidence never permits caching mutable
  field values across aliases. Retain existing cooperative polling and long-native
  operation cancellation; GC work allowance is not an instruction-count interval.
- **Failure policy:** Faulted permits shutdown only. Errors/panics can be caught
  inside Scope, so its state gate remains. Removing that gate would require a
  separate recovery/exit protocol, not merely replacing Result with panic.
- **Performance:** allocation, live scanning and reclamation still lag the
  gc-arena lifecycle fixture. Measure first-white-target barriers, mixed/sparse
  pages, multilayer directory access, real destructor tails and equivalent
  sustained policies before claiming broader parity. Prepared dynamic layout
  binding may amortize exact-layout checks, but requires a concrete validity owner.
- **Validation:** full CI/platform/backend acceptance, integrated pressure and
  cancellation behavior, coverage-guided fuzzing, automatic trace reduction and
  sanitizers remain open. Existing deterministic graph/Miri evidence is scoped.

### Integration obligations

Runtime should own memory, execution and semantic stores alongside each other.
Split ResourceState along actual responsibilities rather than move it intact into
the library. Runtime's focused object integration borrows required siblings
without putting field/type semantics or external-store callbacks in the collector.

| Existing mechanism | Target owner and preserved obligation |
| --- | --- |
| GcHeap object allocation, page marks, root releases and disposal | kagari-gc; generic physical/retention contracts |
| Value/GcRef and StoredValue transport | Runtime interprets tags; library owns opaque direct transport/admission; copied values still acquire no root |
| StructLayoutRef and prepared struct field access | Runtime object/layout owner supplies compact descriptor/access facts; scalar reads retain the prepared fast path |
| NativeStorage registration/application evidence | Native owner; generic scan/layout/charge adapters use the library protocol |
| ExecutionFrame/ExecutionValues and frame scopes | Execution owner; batched roots, scalar banks, return transfer and cleanup stay intact |
| Program, static, interface and environment state | Runtime defines representations; reachability-bearing state becomes ordinary GC objects, while lookup/version policy remains Runtime-owned |
| Runtime::gc_safepoint / collect_garbage | Runtime schedules core operations and maps reports/errors |
| Host typed handles and scoped inspection | Host API retains ownership and type admission, backed by core roots/access loans |

Four integration scenarios define the boundary:

1. **Field update:** the same `player.target = enemy` passes language checks once,
   publishes before physical commit and leaves the old field intact on failure.
   An admitted closed-region operation reuses layout/storage evidence only until
   a callback, collection or other invalidating transition.
2. **Host callback/reentry:** retain extracted values, release all payload/frame
   storage loans, then call Rust or script. Reentry shares the existing session's
   depth/termination contracts. Finish publication before releasing the last
   protection on a returned value. Native entry must not disable GC for the
   callback's entire duration.
3. **Object/code-state cycle:** object -> closure -> executable environment ->
   module static -> object uses ordinary managed nodes and is collected when its
   genuine roots disappear. Actual retained computations keep their selected
   bodies. Ordinary subsequent calls
   still select current implementations. An internal Arc is not a replacement for
   graph accounting of this cycle.
4. **Suspension and shutdown:** parked frames keep live values and required code;
   cancellation releases them through ordinary retirement. Runtime/heap transfer,
   final host-root drop, taken aliases, and destructors on trap/unwind retain their
   original safety and exactly-once cleanup contracts.

No bridge may serialize and reload all objects to reconnect Runtime, duplicate
statics, retain every module forever, or install two collectors over the same
heap. The old production implementation may remain while an independent library
is proved; the integration cutover must retire the old owners and algorithms.
Unpublished Rust API changes do not require format/runtime ABI bumps. Preserve
artifact/ABI validation and source-free loading; regenerate affected disposable
products at coherent checkpoints if executable contracts actually change.

Historical source audits and PG/NC obligations refer to
[0490f8a4](https://github.com/kagari-lang/kagari/tree/0490f8a4071f11ffcb98679f0eb3dd67eb8c9854),
not main's current Runtime. Reconcile the
[PG acceptance ledger](https://github.com/kagari-lang/kagari/blob/0490f8a4071f11ffcb98679f0eb3dd67eb8c9854/docs/paged-incremental-gc-design.md#pg05-integrated-driver-and-open-acceptance)
and [Native integration plan](https://github.com/kagari-lang/kagari/blob/0490f8a4071f11ffcb98679f0eb3dd67eb8c9854/docs/native-redesign-plan.md)
when activating the cutover; standalone results do not close their gates.
The existing [R13 code-retention finding](review.md#r13--type-and-layout-provenance-retains-retired-code)
remains at its current owner: library descriptors must not acquire LoadedModule
provenance, and extraction alone does not fix other type-only holders of code.

## Reference mechanisms and selected boundaries

| Primary source/version | Mechanism and applicability |
| --- | --- |
| [gc-arena 0.7.0 Gc](https://docs.rs/crate/gc-arena/0.7.0/source/src/gc.rs), [Arena](https://docs.rs/crate/gc-arena/0.7.0/source/src/arena.rs), [dynamic roots](https://docs.rs/crate/gc-arena/0.7.0/source/src/dynamic_roots.rs) | Generative invariant handles prove membership after external-root admission. Kagari brands transient handles, while its stored objects/edges remain unbranded and static. Root admission remains checked; no per-access heap ID is needed for branded Gc. |
| [gc-arena 0.7.0 Collect](https://docs.rs/crate/gc-arena/0.7.0/source/src/collect.rs), [containers](https://docs.rs/crate/gc-arena/0.7.0/source/src/collect_impl.rs), [derive 0.7.0](https://docs.rs/crate/gc-arena-derive/0.7.0/source/src/lib.rs) | Unsafe tracing and barrier-aware mutation support safe ordinary use. Its complete-container scans have no suspended element cursor; Kagari's resumable providers need additional mutation/cursor proofs. A tracing derive alone is insufficient. |
| [gc-arena 0.7.0 collector](https://docs.rs/crate/gc-arena/0.7.0/source/src/context.rs) | A DropGuard returns partially traced objects to gray_again on unwind. Kagari retains its different Faulted quarantine contract; direct reads cannot silently drop the state gate. |
| [Go 1.26.0 barriers](https://github.com/golang/go/blob/go1.26.0/src/runtime/mbarrier.go) | Bulk movement publishes pointers before copying. Concurrent Go marking cannot use Kagari's exclusive-driver ordered-Vec cursor repair; arbitrary container edits still need a provider proof. |
| [OpenJDK 21+35 OopStorage](https://github.com/openjdk/jdk/blob/jdk-21%2B35/src/hotspot/share/gc/shared/oopStorage.hpp) | Block allocation/occupancy permits empty-block reclamation. Kagari uses stable numeric IDs and a sparse radix directory rather than HotSpot's handle/iteration protocol. |
| [gc-arena 0.7.0 metrics](https://github.com/kyren/gc-arena/blob/49c4c041816be2dc6ce8a928ace838e37a066813/src/metrics.rs), [Lua 5.4.9 collection](https://www.lua.org/source/5.4/lgc.c.html) | Allocation debt schedules collection at safe boundaries. Kagari keeps explicit bounded-provider admission and opt-in driver safepoints; their coefficients are not interchangeable. |
| [HotSpot JDK 25 safepoint options](https://docs.oracle.com/en/java/javase/25/docs/specs/man/java.html), [Go 1.26.0 preemption](https://github.com/golang/go/blob/go1.26.0/src/runtime/preempt.go) | Poll placement trades throughput against responsiveness. Neither supplies a Kagari instruction interval or justifies weakening cancellation during extraction. |

These sources inform ownership and mechanisms, not predicted speedups. Kagari's
custom boundary combines compact dynamic layouts, Rust provider destruction,
resumable scans and explicit host/execution roots under one exclusive driver.
Current implementation invariants belong in the implementation design rather
than a duplicate API sketch here.

## Performance evidence

### Method and interpretation

Measurements below were taken on 2026-11-01: Windows 11 Pro 10.0.26300 x64,
i9-12900K (16 cores/24 logical), about 64 GiB RAM; rustc 1.99.1 (58cf8f9c3,
2026-10-13), Cargo 1.99.1, LLVM 23.1.1, x86_64-pc-windows-msvc. They use workspace
default release profile, target and build parallelism, default crate features,
sequential single-thread processes and warm workloads. Compilation/setup are
outside timers; no concurrent builds/tests or CPU affinity/frequency pinning.
gc-arena is 0.7.0 with std. Lua uses mlua 0.12.2, lua54/vendored, lua-src 551.0.2
and PUC Lua 5.4.9, without LuaJIT.

Unless identified otherwise, each row reports the median of seven process
medians, with three warmups and eleven measured samples per process. Scalar loops
perform 50,000 operations; cases and comparison orders rotate. Brackets are the
range of process medians, not confidence intervals. All measured batches check
results and reclamation. Keep all samples; tiny loop throughput depends on
compiler layout and execution conditions and is not individual access latency.
The [crate guide](../crates/kagari-gc/README.md) provides tracked fixture commands.
Source baseline is main `00254a09` plus the standalone GC implementation; the
field comparison identifies the successive check/handle variants below. These
are characterizations, not frozen regression gates or Runtime measurements.

### Current field access

The retained implementation removes the redundant scoped heap-ID check and
shrinks Gc from 16 to 8 bytes on x64. Faulted/Closed state checks and dynamic
layout admission remain. For attribution, three release variants used identical
fixtures: prior checks/16-byte handles, check removed/16-byte handles, and the
final pointer-sized handle. Seven rotated process groups ran field_access
(normal, --dynamic-borrow and --alternating) and arena_comparison. Build times
were 1.46/1.40/1.42 seconds, excluded from timing. All 12,642 CSV rows validated.

| Operation (ns/op) | Prior checks | Check removed | Pointer-sized Gc |
| --- | ---: | ---: | ---: |
| Matched static read | 0.450 [0.450, 0.450] | 0.368 [0.368, 0.368] | 0.306 [0.306, 0.338] |
| Matched static write | 0.614 [0.614, 0.614] | 0.614 [0.614, 0.614] | 0.614 [0.410, 0.836] |
| Matched static increment | 0.480 [0.460, 0.616] | 0.412 [0.412, 0.414] | 0.412 [0.410, 0.516] |
| Matched borrow each | 0.616 [0.614, 0.618] | 0.616 [0.614, 0.884] | 0.452 [0.452, 0.464] |
| Dynamic read | 0.864 [0.864, 0.864] | 0.750 [0.750, 0.752] | 0.760 [0.752, 0.766] |
| Dynamic write | 0.834 [0.834, 0.840] | 0.766 [0.764, 0.814] | 0.754 [0.754, 0.762] |
| Dynamic increment | 1.634 [1.632, 1.634] | 1.498 [1.496, 1.498] | 1.562 [1.534, 1.570] |
| Dynamic borrow each | 0.894 [0.892, 0.896] | 0.834 [0.834, 1.156] | 0.824 [0.822, 0.826] |

Batch-borrow medians remain 0.412 ns/op. Matched gc-arena read/write/increment
medians remain 0.204/0.614/0.412; compact-variant ranges are [0.204, 0.206],
[0.614, 0.692], [0.412, 0.452]. Its borrow-each median is 0.818. Separate
field_access typed reads are 0.410/0.308/0.308, not interchangeable with the
matched arena rows. Plain Rust controls remain 0.204/0.614/0.412.

The static matched read improves about 32%, but compact dynamic increment is
about 4.3% slower than the intermediate variant while remaining faster than the
original. The unchanged gc-arena root-copy control also varies (0.688 -> 0.750,
final range [0.686, 0.770]). No uniform speedup or isolated per-comparison CPU cost
is established. Machine-code inspection confirms the static read loop falls
from 15 to 12 to 11 normal-path instructions: the ID reload/comparison/branch
and then extra handle store disappear. Per-iteration state checks remain;
there is no page lookup, helper call or runtime borrow counter on that path.

DynamicLayout::pointer retains an inline annotation so cross-crate callers can
inline exact-layout validation and merge repeated read/write admission. Its prior
paired measurement reduced dynamic reads from 1.128 to 0.864 ns/op while retaining
all checks. Broad forced inlining was not retained; measured improvements do not
justify a blanket inline policy. External Root/RootSet/layout and unbranded edge
admission still validates membership before access.

### Lua context

The Lua fixture was measured before the final GC field optimizations; Lua itself
has not changed. Its single table uses existing keys, no metatables or loop
allocation, and collection is stopped during timing. One host call executes a
whole batch. The GC/Lua builds took 1.47/48.84 seconds and were excluded; the
seven-process refresh validated 5,096 CSV rows across its five suites.

| Operation (ns/iteration) | Named field | Integer index | Local control |
| --- | ---: | ---: | ---: |
| Read/sum | 4.644 [4.568, 4.714] | 4.156 [4.152, 4.240] | 2.808 [2.806, 2.930] |
| Write | 4.708 [4.700, 4.776] | 4.098 [4.092, 4.272] | 2.830 [2.760, 2.858] |
| Increment | 9.798 [9.648, 10.084] | 7.036 [6.944, 7.146] | 2.764 [2.756, 2.894] |

These include bytecode dispatch, lookup/tag handling, arithmetic and loop control;
the native Rust GC suites do not. Do not infer a Kagari-script/Lua ratio or
subtract the local control to claim isolated field lookup cost.

### Collection and retained capacity

The pre-final-field-optimization arena refresh measured 8192 leaves: Kagari /
gc-arena allocation plus frame store 387.9 / 193.3 us, live collection 81.6 / 27.2
us, dead collection 256.9 / 122.0 us. Different resumable-root/retirement contracts
remain; these figures identify remaining gaps, not whole-collector parity.

Metadata reclamation was measured independently of the later field changes.
The capacity fixture creates 8192 entries, retains the highest-ID survivor,
collects, then drops it and collects again. Requested directory bytes are:

| Directory | Peak | One survivor | Empty |
| --- | ---: | ---: | ---: |
| Physical pages | 200192 | 6752 | 0 |
| Root slots | 527872 | 16992 | 0 |
| Descriptors | 134656 | 4704 | 0 |

Partial leaves retain 256 slots. This excludes inline Heap/allocator overhead
and RSS; freeing allocations need not return OS memory. The paired radix change
cost ordinary allocation 373.5 -> 395.0 us, live collection 78.7 -> 81.2 us and
dead collection 232.2 -> 250.5 us for 8192 leaves. The retained memory fix has a
measured throughput tradeoff; do not describe it as a speedup. Work no longer
scales with released historical entries, but live multilayer tables need further
throughput and pause characterization.

The earlier sustained fixture exercised 128 batches, a 512-target live ring and
one default-policy safepoint per batch. Regular batches allocate 256 objects;
burst mode replaces every sixteenth batch with 4096. Timers include allocation,
root writes, scope transitions and service, excluding setup and final cleanup.
Each process observes 1408 measured batches; p99 below is nearest-rank per process,
then median across seven. This small-fixture tail is not a latency gate or SLO.

| Workload | ns/object including service | Batch p50/p99 (us) | Steps/assists/cycles per run | Peak objects | Peak managed bytes |
| --- | ---: | ---: | --- | ---: | ---: |
| Regular leaves | 60.61 | 9.05 / 45.3 | 100 / 25 / 25 | 2711 | 49192 |
| Regular self-cycles | 79.41 | 15.5 / 56.4 | 103 / 23 / 23 | 3296 | 90376 |
| Burst leaves | 58.75 | 9.0 / 221.6 | 108 / 17 / 17 | 8128 | 145288 |
| Burst self-cycles | 73.16 | 14.5 / 270.7 | 108 / 18 / 18 | 8192 | 224224 |

These preceded metadata and field changes and require refreshing before a current
performance gate. Peak managed bytes exclude metadata and source ring capacity.
There is no matched sustained gc-arena policy result. Lower burst amortized cost
does not imply better tails. Actual Runtime safepoint frequency remains unmeasured.

## Acceptance requirements

This specification defines remaining benchmark acceptance, not a claim that the
implemented fixtures have passed it. Evaluate throughput, latency and memory together. A smaller step is
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
  do not create another Runtime-dependent collector harness. GCR00 freezes the
  workload recipe and measurement method. The first correct GCR01 implementation
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

Implementation checks follow [AGENTS.md](../AGENTS.md): affected local tests plus
structure, fmt, workspace all-target Clippy and diff checks; exhaustive workspace,
feature/backend/platform tests belong to CI. Documentation-only changes require
content/link/diff checks. Passing selected tests does not close the complete plan.

### Current validation evidence

The final scoped-handle change passed 33 release library/ownership/root_source/
dynamic/graph-model tests and 20 doctests, including 18 compile-fail cases.
Six focused strict-provenance Miri cases cover field panic/quarantine, external
root rejection, scoped graph/root edits, paired source admission and erased
layout identity. Prior unchanged collector/metadata contracts retain their
focused native and graph/layout/failure Miri evidence. No known build failure
is carried; structure, fmt, workspace Clippy and diff checks passed at the
implementation checkpoint.

The independent graph oracle checks mixed cyclic nodes, roots, dynamic fields
and resumable vectors against ID-only reachability and destructor counts. The
[crate guide](../crates/kagari-gc/README.md#graph-and-memory-safety-validation)
owns deterministic seeds, replay and Miri commands. CI pins nightly-2026-10-22;
local interpretation used strict provenance on Linux x86-64. CI has not run for
this checkpoint. Miri is evidence for executed paths, not proof of arbitrary
unsafe providers. Broader fuzzing/sanitizer and Runtime acceptance remain open.
