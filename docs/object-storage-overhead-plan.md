# Object storage overhead execution plan

Status: proposed execution plan recorded on 2026-10-16; baseline and measurement
policy refreshed on 2026-10-18. Implementation has not started.
The [roadmap](implementation-roadmap.md#object-storage-overhead) owns
activation and ordering. This plan owns OM01-OM05, conditional decisions,
acceptance and the progress ledger. Writing it does not activate implementation.

## Outcome and finite scope

Reduce per-object management cost on top of the completed RS01-RS06 storage
redesign. Preserve the [runtime model](architecture.md#runtime-model),
[runtime specification](spec/runtime.md) and [project priorities](project_goal.md).

The required change is to move native mutation revisions out of the common object
directory, with complete consumer migration and measured total-storage benefit.
Layout-grouped struct storage and interface-record consolidation are conditional
follow-ups: evaluate them in order and retain an implementation only when its
ownership model and measured benefits pass this plan's gates. Explicit deferral
is an accepted outcome for each conditional phase; directory migration is not
optional and cannot be marked complete merely because a benchmark is difficult.

Keep 16-byte Value transport, full owner/slot/generation handles, direct scalar-bank
field transfers, exact scalar widths, shared interface descriptions and typed
native storage. Do not change language value/alias semantics, introduce inline
value-type collections, compress owners, replace the GC algorithm, expand native
backends, implement downcast or perform the NC collection-library migration.
General arithmetic and call-lifecycle optimization belong to the separate
[interpreter follow-up](implementation-roadmap.md#interpreter-performance-follow-up).
IP01-IP07 completed local acceptance without activating OM. Future interpreter work
must reuse the accepted storage ownership and measurements rather than duplicate
this ledger or require conditional storage changes to be implemented.

## Evidence and cost ownership

OM01 has no frozen baseline yet. Freeze the current coherent implementation at
activation, including the accepted IP call/window/lease work and subsequent accepted
changes; record its exact revision, relevant working-tree changes and validation
status before any OM production edit. Neither the old RS revision nor the IP
checkpoint automatically represents the implementation at activation.

The [RS measurements](performance-baseline.md#runtime-storage-2026-10-16) at
35077323 (handoff 312e6b46) retain the original pre-RS comparison. The later
[IP storage checks](performance-baseline.md#interpreter-call-lifecycle-2026-10-17)
at f070a092 provide newer allocation observations. Both used Apple M1 Max,
arm64 macOS 26.6.2, rustc 1.98.1 and the workspace release profile; the IP allocation
run is not a fresh RS latency comparison. These are historical evidence for selecting
work, not fresh OM measurements or current-HEAD guarantees. Recheck ownership,
sizes, capacities and behavior in OM01 rather than attributing intervening changes
to OM or discarding their accepted improvements.

| Item | Recorded evidence | Implication |
| --- | --- | --- |
| Common directory entry | RS: 24 bytes on the measured target | Every kind pays for a u64 revision, generation and locator. Recheck the current layout in OM01. |
| Struct record plus its directory back-reference | RS: about 24 bytes by the reviewed 64-bit layout | Every instance repeats a description reference and byte offset. Verify sizes in OM01. |
| Point fields | Two i32 fields occupy eight bytes | Field payload compression is not the main remaining opportunity. |
| 512 Points | IP: 57,036 live / 39,460 empty / 65,644 reused requested-byte increments | Total population cost includes capacity and bookkeeping; dividing by 512 does not establish an object size. |
| 512 Pair<i32> values | IP: 57,812 live / 39,500 empty / 66,004 reused | Generic layout provenance must survive any grouping. |
| Narrow / wide / generic interfaces | IP: 127,584 / 128,808 / 128,960 live increments | Description sharing already works; remaining instance records need separate attribution. |
| Cyclic nodes | IP: 413,380 live increment; historical RS release GC +9.4% against pre-RS baseline | Reference-heavy retention and collection latency are sensitive controls; these observations come from separate runs. |

The common directory and per-store record have different responsibilities:
the directory owns stable identity/generation; each dense store's back-reference
allows relocation to update that directory. These indices are not automatically
redundant. Native revisions protect mutation observations; struct layout metadata
supplies field encoding and nominal provenance. Classify each byte by responsibility
before removing or moving it.

OM01 must reconcile requested-byte totals into live field bytes, occupied records,
unused directory/store capacity, container element storage, shared descriptions and
keys, roots/free lists, and other metadata. Distinguish requested allocator bytes
from logical GC units, allocator size classes, committed pages and RSS. Do not
double-count a shared description once per object or count retained capacity as
live payload. Explain any unattributed remainder rather than presenting an exact
per-object saving from shallow sizes alone.

## Ownership changes

Before selecting an ownership change, follow the
[architecture-first repository policy](../AGENTS.md#architecture-before-optimization-and-feature-changes).
OM01 must compare relevant Java/OpenJDK, Go or other established storage/GC
mechanisms with Kagari's checked handles, exact nominal layouts, host state and
pinned reload roots. Record source/version, applicable mechanism and reasons for
adoption or rejection here. Distinguish JVM specification contracts from concrete
collector/object-layout implementations. This is part of OM01's existing diagnosis,
not authorization to replace the collector or activate OM ahead of the roadmap.

### Common identity directory and native mutation state

Current owners are [ObjectStorage](../crates/kagari-runtime/src/gc/directory.rs),
[allocation](../crates/kagari-runtime/src/gc.rs),
[native access](../crates/kagari-runtime/src/gc/native.rs),
[managed native edits](../crates/kagari-runtime/src/gc/native/managed.rs),
[collection commits](../crates/kagari-runtime/src/gc/mutations.rs),
[sequence leases](../crates/kagari-runtime/src/gc/sequence_edit.rs) and
[iterator revision checks](../crates/kagari-runtime/src/gc/iter.rs).

Keep generation and kind/location in the common directory. Move revision state to
the runtime-owned native storage record, adjacent to the native payload, rather
than into a general per-object HashMap or a library-controlled Rust payload. All
native objects use this owner, including custom managed objects; the migration
must not assume that only Vec/Map/Set need revisions. Select the concrete record
shape after OM01 measures padding and access costs. Do not narrow or wrap the
existing revision counter to meet a size target.

Audit every revision read, increment, reset and restoration, including buffers,
map/set edits, native mutable views, callback-driven commits and native extraction.
The new owner must preserve:

- New identity initialization and revision exhaustion errors; relocation preserves
  the revision, whereas slot reuse creates a fresh identity and initial state.
- Existing structural versus element-replacement rules. Keep each operation's
  current revision commit point, including errors or callbacks that leave effects.
- Sequence lease restoration on success, traps and cancellation; temporarily
  detached storage must remain inaccessible through ordinary traversal.
- Candidate initialization isolation, scoped native borrow checks, synchronous
  reentry and operation protection before mutation or extraction.
- Native payload replacement without accidentally resetting the receiver's revision;
  moved/extracted identities remain inert and cannot alias a later allocation.

For example, an iterator observes revision r; a callback structurally edits its
source; the next read must reject the stale traversal after preserving the
callback's completed effects. Moving r from the directory to native storage must
not move that check, add another semantic implementation or retain a heap borrow
across the callback. Generation validation and mutation revision serve different
purposes and must not be substituted for each other.

### Conditional layout-grouped struct storage

Today [struct records](../crates/kagari-runtime/src/gc/directory.rs) each own an
Arc description and byte offset; all rows occupy one packed byte arena. The
[closed layout](../crates/kagari-runtime/src/module/layout_storage.rs) and
[field codec](../crates/kagari-runtime/src/gc/fields.rs) already own field semantics.

Evaluate blocks owned by one exact closed layout. A block can share its description
and derive a row's byte position from stride and row index. For 512 Points, this
could replace 512 description references and offsets with block-level metadata
and smaller row bookkeeping. This is a candidate design, not a promised size or
speedup; directory locators and relocation back-references still have a job.

Before implementation, close the following decisions in OM03:

- Group identity includes registry/module-generation and canonical layout identity,
  or the complete detached structural/provenance key. Equal stride or a compatible
  field shape does not authorize merging distinct nominal generations.
- One owner defines directory-to-group/row lookup, survivor relocation and reverse
  mapping. Do not retain the old per-row descriptor/offset alongside a new index
  indefinitely. Retain the existing codec and scalar transfer path as the only
  semantic implementation.
- Growth, sparse deletion, reuse and release policies bound slack for one instance
  per layout as well as large homogeneous populations. Empty groups release their
  descriptions; unused group metadata must not become an executable or data root.
- Zero-byte rows remain distinct identities. Mixed scalar/reference rows preserve
  initialization, NaN bits, signed zero, nominal checks and alias-visible mutation.
- Reserve all required storage before publication. No row view survives allocation,
  collection, reentry or suspension. Partial preparation never exposes an object.

Prefer safe byte storage and ordinary Rust ownership. Avoid a new fallback engine
for sparsely populated layouts; a single group owner may use bounded growth sizes.
If group overhead or collection costs outweigh savings, remove the prototype and
retain the current packed arena with an explicit measured deferral.

### Conditional interface-record consolidation

The current path is a Value's checked heap identity, directory locator, typed
interface payload containing InterfaceSnapshotId, and a separately checked
[interface snapshot store](../crates/kagari-runtime/src/execution_metadata/interfaces.rs).
The [snapshot](../crates/kagari-runtime/src/gc/interfaces.rs) owns the receiver,
shared description and receiver-specific parent-view caches.

Assess whether one record/identity owner can serve the actual consumers. Inventory
heap values, metadata graph edges, parent-view caches and construction paths before
choosing which identity survives. Two stores alone do not prove duplicated
semantics: snapshots can participate in executable metadata reachability beyond
an ordinary wrapper lookup. Do not remove a store until every such edge has a
checked replacement and defined lifetime.

A retained design must keep receiver data/private parent caches separate from
shared method/type descriptions, trace each receiver and each shared description
appropriately, and preserve applied generic witnesses and pinned executable
dependencies. Parent-view creation must not create immortal cycles or share a
receiver with another interface value. Preserve public identity/equality behavior,
host restrictions, tracing failure handling and disposal outside borrowed stores.
Old metadata identities must not resolve to reused records after GC or reload.

If consolidation passes its gate, replace the obsolete wrapper/store paths and
update all consumers in one coherent phase. Do not keep forwarding IDs, duplicate
roots or a second snapshot implementation for hypothetical compatibility. This
phase does not implement is<T>/downcast<T>; refresh that queued plan's inventory.

## Phases and checkpoints

### OM01 — Attribute costs and freeze measurements

Extend diagnostics beside the existing
[storage probe](../benchmarks/lua-comparison/examples/runtime_storage/main.rs) and
[allocation probe](../benchmarks/lua-comparison/examples/runtime_storage/memory.rs).
Keep the original script, counts and timing regions unchanged. New controls use
separate programs so extra declarations do not silently alter the original setup.
Diagnostic attribution must not add permanent counters or alter ordinary timing.

Record actual sizes/capacities, ownership/consumer inventories, environment and
the missing data for new controls. Freeze their exact sources, counts and gates
before modifying production storage. Use the current implementation selected at
activation, with only equivalent probe/diagnostic additions if required. Save its
ordinary executable and baseline report before the first production edit, and keep
that image fixed across OM comparisons. Collect allocation diagnostics separately.
Later conditional phases also compare against their preceding accepted checkpoint;
they do not replace the OM01 baseline or compound its allowances. Preserve the
original pre-RS comparison and RS obligations using matched evidence, not elapsed
times copied from a different machine or toolchain.

Acceptance: allocator totals are reconciled with documented limits; all revision
consumers and interface identity roots are accounted for; fresh matched controls
build and run. No implementation speedup is claimed by this phase.

### OM02 — Relocate revisions and shrink the directory

Implement the required directory/native-record ownership change and update every
consumer, including leases, native replacement/extraction and collection cleanup.
Remove the common revision field and obsolete access paths. Keep this checkpoint
buildable; split growing storage responsibilities into ordinary modules if needed.

Acceptance: native semantics and identity tests pass, the common directory is
smaller, and Point/Pair total live and empty increments each fall at least 10%
against the OM01 baseline. All regression gates below pass. Do not accept a nominal
header saving that simply shifts more bytes into another general side table.

### OM03 — Decide and conditionally implement layout grouping

Using the OM02 result, record the complete proposed group/row ownership model,
projected total saving and sparse-layout cost. Prototype only when the projection
can materially improve the measured remaining cost. Compare against OM02 as well
as OM01; retain grouping only with at least 10% further live-storage reduction for
both Point and Pair and all regression gates passing. A reasoned deferral completes
the decision phase without claiming that grouping was implemented.

### OM04 — Decide and conditionally consolidate interface records

Use the consumer/root inventory to close identity and tracing ownership before
prototyping. Compare against the preceding accepted checkpoint and OM01. Retain
consolidation only if narrow, wide and generic interface live increments each fall
at least 15% further and all correctness/regression gates pass. Otherwise retain
the existing store with measured/design evidence for deferral. Remove rejected
prototype code and temporary bridges before closing the phase.

### OM05 — Integrated validation and documentation handoff

Run the focused composition selection and matched final measurements. Resolve all
carried integration errors, review changed Rust structure, and report conditional
phase dispositions separately from implementation. Move lasting contracts to
architecture/specifications, measurements to their existing owner, queued CI/gaps
to the roadmap, and update affected DC/NC inventories without activating them.
Remove this completed plan; Git history owns its ledger. Complete local acceptance
does not mean the unrun exhaustive CI matrix passed.

## Workloads and acceptance

Retain all original 26 latency cases, the one/512-object allocation controls,
50,000-iteration scalar/field loops, numeric buffers, interface populations,
reference cycles and sparse release. Add only these directly affected controls:

- Directory/native revision: structural collection edits, iterator advancement,
  native mutable access, callback edits and sequence lease restoration. Reuse
  semantic fixtures; timed cases measure normal successful paths separately.
- Layout grouping: homogeneous objects, many distinct closed layouts with one/few
  live rows, mixed zero-byte/scalar/reference layouts, and overlapping old/new
  nominal generations. Count groups, used bytes and reserved capacity separately.
- Interface consolidation: repeated copies of one interface, distinct receivers,
  parent views, generic applications and metadata-only reachability across reload.

Each population follows setup, construction, live GC, sparse/released roots, empty
GC, and at least three equal create/release/reuse cycles, then runtime teardown.
Shared immutable program warmup is reported separately. Root and container costs
are included equally. Existing numbers do not establish future plateau behavior;
OM01 records it before judging a replacement.

The following limits apply to **each accepted checkpoint and the final result**:

- Unchanged execution/control medians may regress at most 5% against OM01.
  Construction/live-GC/release-GC medians may regress at most 5% against OM01.
  Original cases must also retain RS acceptance against a matched pre-RS baseline:
  field loops at least 10% faster, other execution at most +5%, lifecycle at most
  +10%. Do not compound allowances across phases or spend a fresh 10% per rewrite.
- Non-target live/empty/reused and small/diverse-layout requested-byte increments
  may grow at most 5% against OM01. Phase-specific target savings above apply in
  addition. Loaded setup must not grow more than 5%, and its metadata attribution
  must remain explicit. Compare complete native records, not directory size alone.
  A zero-byte baseline requires zero retained bytes rather than a percentage ratio.
- Warmed field-loop allocation requests/bytes must not increase. Total construction
  and collection allocation counts/bytes are reported separately, including any
  temporary batches; retained-byte wins cannot hide repeated preparation costs.
- Repeated equal populations reach a bounded capacity plateau rather than retaining
  additional per-cycle groups/metadata. Every warmed runtime teardown has zero net
  requested bytes; generation tombstones remain valid until runtime destruction.
- No correctness, trap ordering, cleanup, cancellation, host borrowing or pinned
  reload regression is acceptable. A smaller size does not offset a lost check.

Apply the simplified [before/after protocol](../benchmarks/lua-comparison/README.md#beforeafter-workflow)
to the storage probe's own rows; the guide's Lua-suite commands do not select this
probe. Keep the existing workload sizes, timing regions and checked results.
Use the same machine, toolchain, release profile, features, target and default
build parallelism for matched comparisons. Build first, then run old/new ordinary
executables sequentially. Separate compilation/preparation/linking, ordinary timing,
sampling and allocation instrumentation; run no builds, tests or other measurements
concurrently with timing. Compilation and preparation times are diagnostics;
construction and live/release GC remain separate gated lifecycle rows. Setup-memory
limits above remain in force.

Use seven fixed baseline/current process pairs by default, alternating B/C and C/B,
with three warmups and eleven samples per case. Choose any different pair count
before starting. If workload order is reversed, change it every two pairs,
independently of which executable runs first. For each row, let B_i and C_i be the
baseline and candidate process sample medians, normalized by batch size if needed:

```text
comparison ratio = median(C_i / B_i)
```

This is the sole timing threshold estimator, including inherited RS timing gates.
For example, <=1.05 meets the OM regression limit and <=0.90 meets a 10% reduction
target. Preserve raw samples, process medians and min/max paired ratios; the range
is not a confidence interval and a numerical pass is not a significance claim.
Zero baseline medians are inconclusive. There is no second aggregate-ratio gate,
fixed near-threshold margin, report extension or independent reference calibration.
Investigate visibly noisy results before claiming a speedup. A justified follow-up
is a separate, predeclared experiment that retains and discloses earlier conflicting
results, not an extension or repeated unchanged run until a pass. Historical RS
reports keep their original conclusions; this policy applies to new comparisons.

Validate complete rows/samples, pair identities, workload inputs/results and matched
build settings. Record one selected-workload checksum, the two executable checksums,
revision/status, dependency lock and environment/cache metadata once per run; do not
add per-source-file or per-worker manifests. Keep images unchanged throughout each
comparison. Diagnose ownership before further tuning when a gate fails. Keep raw
logs under ignored target/ and necessary methods/results/limitations in this plan
until handoff. This update changes measurement policy, not the storage savings,
correctness or regression limits above.

## Correctness and verification

Reuse [GC ownership](../crates/kagari-runtime/src/gc/tests/gc_ownership.rs),
[struct layout contracts](../crates/kagari-runtime/src/module/tests/struct_layouts.rs),
[native trace/drop failures](../crates/kagari-vm/src/tests/native_boundary/native_boundary_gc/payload_failures.rs),
[interface identity](../crates/kagari/src/runtime/tests/language/interface_identity.rs),
[host interfaces](../crates/kagari/src/runtime/tests/embedding/host_interfaces/mod.rs)
and [generic reload](../crates/kagari/src/runtime/tests/embedding/generic_reload.rs).
OM01 records exact focused filters after locating the current revision/iterator
contract owners. Add cases only for uncovered boundaries, such as revision
preservation through relocation/replacement or zero-byte rows in different groups.
Include representative source-free layout/nominal rejection and suspended/reentry
root coverage where changed; no production source-analysis dependency is permitted.

Prefer safe Rust throughout. The existing bounded-unsafe policy in
[architecture](architecture.md#runtime-model) still applies if a concrete layout
requires it: private safe APIs, enforced provenance/bounds/initialization/aliasing
invariants, measured benefit over the safe baseline for check elision, and focused
Miri/sanitizer validation. Unsafe is not an acceptance objective and cannot replace
owner/generation, borrow, artifact or reload validation.

Implementation checkpoints remain buildable and use Conventional Commits with
Plan-Phase: OMxx trailers. Record attempted focused commands, failures/skips, cause
and owning repair in the ledger; never mark an unrun selection as passed. Preserve
meaningful assertions and use coherent replacements rather than parallel models.
Run the required local checks at implementation checkpoints:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Select a small affected test set per phase under [AGENTS.md](../AGENTS.md) and the
[testing guide](testing.md). Full-workspace tests and the exhaustive feature/backend/
platform matrix remain CI-only. Documentation-only planning needs content/link/diff
checks, not workspace builds or tests.

## Progress and decisions

- [x] Planning: current owners, finite phases and numerical gates recorded.
- [ ] OM01: cost attribution, consumer audit and frozen current baseline/controls.
- [ ] OM02: common-directory/native-revision migration and acceptance.
- [ ] OM03: layout grouping decision; accepted implementation or explicit deferral.
- [ ] OM04: interface record decision; accepted implementation or explicit deferral.
- [ ] OM05: integrated local acceptance, CI scope and documentation handoff.

Current decisions: keep Value and full checked handles; make directory slimming
required; gate the two larger representation changes on additional total-storage
savings. No Java/Go object-size parity or universal speedup is promised. Baseline
revision state already protects native managed edits and sequence recovery, not
only collection iterators. Planning has not established a new implementation
failure or a new performance result. Prior exhaustive CI acceptance remains pending.

2026-10-18 planning refresh: replace the fixed pre-IP baseline with a fresh current
baseline at activation; distinguish historical RS and IP evidence; adopt the fixed
paired-ratio protocol above without dual acceptance or report extensions. Preserve
all numerical storage/latency targets and original RS obligations. OM01-OM05 remain
unstarted; no new measurement or implementation acceptance is claimed.

Refresh validation: phase/checklist and roadmap consistency, Markdown fences,
124 repository-relative links/anchors across the two changed documents and
whitespace checks pass. This documentation-only update requires no build, test or
Clippy run.
