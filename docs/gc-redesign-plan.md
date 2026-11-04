# Standalone kagari-gc and Runtime integration

The standalone library implements typed Rust and fixed dynamic objects, scoped
access, host roots, direct execution-root adapters and incremental collection.
Runtime still uses its existing collector. This plan owns remaining work and
acceptance; the [implementation design](gc-implementation-design.md) owns storage,
borrowing, tracing and mutation contracts, and the [crate guide](../crates/kagari-gc/README.md)
owns usage and fixture commands. The [roadmap](implementation-roadmap.md#gc-library-redesign)
owns activation of the integration phases.

The [Runtime integration design draft](runtime-gc-integration-design.md) records
proposed Runtime responsibilities, the scoped/stored/rooted Value family,
direct frame transfers, host slots and stable native shells. It is an editable
GCR00/GCR02 design input, not a second execution plan or a completed proof.

## Scope and status

The library is a precise, nonmoving, nongenerational incremental mark-sweep
collector driven exclusively by its owner. It traces its own allocations from
host root leases and a complete driver-supplied execution root source. Semantic
IDs, language typing, execution policy, hot reload and code selection stay in
Runtime; host Rust state stays outside the script heap.

The standalone implementation is usable without language crates. Runtime cutover,
arbitrary erased providers and Runtime native extraction remain deferred; the
selected extraction model preserves a stable shell rather than replacing its
descriptor with a tombstone.
Concurrent/moving collectors, foreign graph stores or Domain bindings, generic
finalizers/resurrection, shared heaps across Runtimes, derive tooling and new
container libraries are outside this scope. The current fixed dynamic API does
not enforce language nominal types or implement all Runtime storage needs.

| Phase | Current state and remaining exit condition |
| --- | --- |
| GCR00: contract closure | Typed/fixed dynamic access, typed schema keys, branded handles, provider mutation, cursor repair and direct root-source contracts are implemented. Close Runtime representation/extraction requirements and freeze comparable performance gates. General safe custom-object construction remains an explicitly reviewed provider responsibility. |
| GCR01: standalone heap | Allocation, roots, mutation, resumable collection, metadata reclamation and opt-in safepoints are implemented. Complete the remaining representation/diagnostic requirements selected for integration; do not invent placeholder APIs. |
| GCR02: Runtime object-model proof | Standalone frame adapters and the bounded Native/value/provider trial exist; production nominal/generic entry admission remains open. Map every genuine root and managed relationship, prove object/code-state cycles and pinned computation lifetime, then measure actual frame/layout adapters. |
| GCR03: coherent Runtime cutover | Connect Value/layout/Native providers and direct frame scanning; rehome execution state and remove the replaced collector and graph owners. Not started. |
| GCR04: integration and performance closure | Complete selected integration regressions, paired standalone/integrated gates and CI; reconcile inherited obligations at their existing owners. |
| GCR05: documentation and cleanup | Move lasting behavior to architecture/spec/authoring owners, remove obsolete paths, transfer unresolved work and retire this plan. |

GCR01 and standalone GCR02 fixtures can coexist with Runtime because they own
separate heaps. Before GCR03, list affected crates and any finite temporarily
non-building migration boundary here, with its owning repair checkpoint. No
production stubs, disabled validation, duplicate semantic implementations or
compatibility facades are part of the transition.

Dynamic schemas use DynamicLayoutBuilder and typed field keys; the former raw
index registration/binding API is replaced. A non-Clone builder and its keys share
one Arc identity, transferred into the registered shape. Binding checks identity
once, then uses the existing field accessor. This follows the external-owner
identity check in gc-arena 0.7.0 DynamicRootSet::fetch (linked below), without its
lifetime rebinding: keys retain only schema identity, and accessors retain actual
descriptors. Named accessors belong to Rust consumers or Runtime metadata. The
[implementation contract](gc-implementation-design.md#runtime-defined-fixed-records)
owns details. This physical-schema builder does not resolve the separate unsafe
custom-object construction question below.

Builder field declarations return keys directly and use ordinary Vec growth.
Registration/binding still return Result. Registration buffers and collector
preparation (root release credits, mark jobs, active lists, radix slots, scanner
workspace and staged provider growth) retain their failure/commit guarantees;
ordinary Arc/Box allocation still means OOM recovery is partial. See
[allocation semantics](gc-implementation-design.md#allocate-and-publish).

Reference writes now admit lifecycle state once per closed operation: typed
receiver admission, dynamic exact-layout admission, or the direct-source entry.
An internal admitted publication core shares the existing barrier algorithm with
the checked wrapper used by other callers. The core retains debug assertions;
publication failure still quarantines before error/panic and before field commit.
The [field contract](gc-implementation-design.md#concrete-entrypoint-shapes)
defines the proof lifetime; no public unchecked API is added.

Mutable-loan entrypoints likewise own their lifecycle check. The shared private
kernel retains a debug assertion after all three callers (rooted callback,
scoped callback and scoped loan) were audited to admit state before invoking it.
Its physical/type/scanner checks and failure quarantine remain intact. Existing
quarantine coverage rejects all three entries after a caught publication failure,
including an Access acquired before that failure; callbacks must not run.
Ten selected native contracts pass (field units, failure/forget/accounting and
scoped cycle loans), with both field units also passing in release and strict-
provenance Miri. Structure, fmt, workspace all-target Clippy, document/link and
diff checks pass. Full workspace/platform/backend acceptance remains CI-only.
No new API, state cache or performance claim is introduced.

## Implementation order for Runtime integration

Implementation checkpoint, 2026-11-03: the user authorized orders 1-3 for an
executable authoring trial. Their bounded value/Native/provider components and
selected proofs are implemented; the general nominal/generic binding contract is
not closed. Orders 4-8 remain pending. The checkpoint below owns concrete results
and remaining limits; this is not GCR03 activation or a second progress queue.
Standalone capabilities already implemented above remain available evidence.
Contract clarification, a demonstrated missing core capability and its Runtime
proof may iterate across GCR00/GCR01/GCR02; do not restart completed standalone
work or begin wholesale Runtime replacement while Native ergonomics are unproved.

| Order | Existing phase and responsibility | Concrete result and exit condition |
| --- | --- | --- |
| 1. Close the minimum authoring contract | GCR00; Runtime Native, value and provider owners | Specify what a normal function, an ordinary Native type and a managed container provider each define. Settle parameter/return adaptation, short access versus callback capabilities, typed application provenance, initialization, host borrowing and Present/Taken admission. Vector/VectorMut are proposed library facades, not assumed core features. List real gaps and freeze baseline workloads before execution-policy changes. |
| 2. Build the minimum value/access foundation | GCR01 gaps with GCR02 Runtime adapters | Implement only the scoped/stored/rooted forms, checked region/slot transitions and stable native-shell/storage primitives needed by the next proofs. Reuse existing collector tracing, roots, barriers and accounting. Demonstrate lifetime escape rejection, protected removal and typed result publication with actual Rust signatures, not pseudocode. |
| 3. Prove Native authoring end to end | GCR02; Runtime Native and provider owners, NC-owned collection logic | Run an ordinary edge-free Native payload plus a scoped host borrow, real Vec push/pop, and fold with a collecting/reentrant script callback. Use real storage and registration/application adapters. Function authors define bodies and bindings, not root packs or per-method tracing; provider authors implement one reviewed storage boundary. Compile-time return mistakes fail, declaration/application mismatches fail before effects, and stable facts are not rebuilt per call. |
| 4. Prove the complete retention model | GCR02; execution, module and host owners | Exercise real frame transfers, reusable Local slots, parked execution and host roots through collection. Prove object/environment/static/code cycles, ordinary current-code selection and retained closure/Future bodies. Check a collecting Hash/Eq callback and sort scratch recovery at their existing semantic owners. Settle lookup-index validity and metadata placement before production cutover. |
| 5. Separate Runtime owners and change execution policy | GCR03 preparation; Runtime execution/memory/modules/host, backends and SDK | Rehome ResourceState responsibilities without moving the same mixed state into a renamed owner. Remove cooperative execution-token polling and its API coherently, preserving admission/sticky-failure gates, GC service, compilation cancellation and queued/suspended async retirement. Make this a separately reviewable change from physical GC replacement. |
| 6. Perform the coherent representation/collector cutover | GCR03; Runtime plus all actual consumers | Replace Value/storage/frame roots, managed object/module relationships, Native adapters/providers and host retention together with their backend consumers. Migrate existing stdlib/host paths needed to preserve supported behavior through the shared provider API. Remove the old production collector, joint graph/metadata sweep, obsolete adapters and privileged collection paths replaced by this cutover. No selectable old/new execution path. |
| 7. Close correctness and performance acceptance | GCR04; existing contract/benchmark owners | Complete focused local boundary coverage and paired integrated workloads, then CI's exhaustive matrix. Compare field/value/frame work, Native calls, allocation, live/cyclic collection and pressure latency under documented settings. Attribute cancellation removal separately from collector and Native-model changes; diagnose architecture again if gates fail. |
| 8. Consolidate the lasting contracts | GCR05; architecture/spec/authoring owners | Move implemented ownership and API contracts to their lasting documents, update examples, retain unresolved work at its roadmap/review owner and remove the completed plan only after required acceptance. |

### Native prototype boundary

User direction, 2026-11-04: the public contract must center on one value-exchange
model, not separate models per user role. Parameters, returns, fields, container
elements and callback transfers share the same checked type/application and
protected-location publication rules. Keep scalar direct transport, explicit
retention where values cross access/callback lifetimes and internal storage-edge
discipline. Reduce ordinary-author concepts; responsibility separation does not
justify exporting Kind/View, Object/Optional, entry preparation and several
manual conversion protocols. The [unified exchange contract](runtime-gc-integration-design.md#one-exchange-model)
owns the design criterion. Existing production From/IntoKagari/CallSlots and
candidate Kind/View/Local adapters have not yet converged end to end. Close this
boundary with actual nominal/generic bindings and compiling lifetime-safe return/
callback examples before expanding public API families or advancing cutover.
This refines the existing orders 1-3 acceptance, without creating another API
layer, proof registry, implementation queue or collector mode.

The [host interaction proposal](runtime-gc-integration-design.md#host-interaction-api-proposal)
makes this criterion concrete: common NativeContext call/get/set operations;
ordinary SDK operations open that same interaction automatically; explicit enter
groups operations; Local protects call-owned locations and retain/load crosses
the host lifetime boundary through RootedValue. Function/member descriptors own
prepared contracts, not another exchange representation. Preserve nonpersistent
Rust-loan restrictions and current named-call/closure reload semantics. This is a
reviewable surface proposal, not implemented generic-family/lifetime inference or
authorization to skip the existing retention/cutover prerequisites. Next proofs
must bind nominal/generic values and demonstrate host -> script -> Native ->
script -> host with GC/reentry and source-free admission on the shared owners.

Design review, 2026-11-04: the requested complete Native call model is specified
as the [activation and lifecycle proposal](runtime-gc-integration-design.md#native-activation-and-call-lifecycle).
One existing execution-frame owner retains selection, frozen parameters, result,
locals and cleanup; typed adapters consume its protected storage. The proposal
assigns checks to their invalidation boundaries and distinguishes sealed typed
transport from arbitrary effectful conversion. It preserves parked completed
returns, current-code selection, host loans and collecting reentry. Lua 5.4.8,
Wasmtime 51.0.0-dev and OpenJDK 21+35 mechanisms and differing assumptions are
recorded with the design. This is a GCR00/GCR02 review input, not implemented
replacement or authorization to advance orders 4-8. Real nominal/generic binding,
Rust lifetime proofs, selected behavioral coverage and unchanged-source paired
measurements remain required. No new timing result or performance gain is claimed.

User direction, 2026-11-04: standard-library efficiency is a design/acceptance
constraint on that same exchange model. The [hot-path cost contract](runtime-gc-integration-design.md#standard-library-hot-path-cost-contract)
requires prepared stable facts, direct scalar/protected-location transport,
reuse of genuinely frozen callee storage and no owning host promotion or
per-element root packs for internal work. Borrow/state/barrier/cleanup guarantees
remain at their semantic owners. The current extra prepared input/result snapshot
is not declared redundant until callee-source lifetime and converter-effect/trap
ordering are proved. Freeze the bounded small-call, len/get, spare-capacity
push/pop and existing fold controls before the owning implementation change;
reuse existing benchmark/Lua comparison owners, not a parallel harness. Fold's
once-per-2048 Native entry does not measure small Native-call overhead. No
cross-language speed/parity claim or absolute ns target is established here;
the unresolved reference delta and inherited performance gates remain open.

The immediate next work is orders 1-3, not migrating every standard-library
function. Resolve the public parameter/adaptation shape with executable examples:

- **Ordinary Native type:** define its Rust payload, script declaration and method
  bindings; show the actual amount of user-written mapping/borrow code. Also show
  a host-owned resource accessed through the installed scoped interface. Do not
  move arbitrary host state into the script heap.
- **Vec storage and view:** implement the real push/pop path beneath the proposed
  Vector/VectorMut facades. Show provider construction, controlled mutation,
  removed-value protection, scanner repair, capacity accounting and return
  publication. A short wrapper delegating to an undefined storage helper is not
  evidence of a maintainable API. Do not require unrestricted mutable payloads.
- **Script callback:** implement fold using a fixed reusable slot set; its callback
  can allocate, collect and synchronously reenter. No scoped value/payload loan
  survives that boundary. Use the normal typed return path with no return_unit,
  PreparedReturn, per-element Root/RootSet construction or cancellation polls.

Keep prototypes bounded and attached to the actual owning modules/test fixtures.
Before production activation they may use a separate fixture heap, but must reuse
the intended storage/adaptation components; do not create a second miniature
Runtime or a disposable duplicate collection semantics implementation. A normal
method must not require new unsafe tracing code. If each new Native type needs
handwritten read/write wrappers and repeated conversion glue, revisit the generic
admission/borrow boundary before freezing the API. Exact trait/lifetime names are
chosen by these proofs, not by making the illustrative names compile at any cost.

The Hash/Eq and sort cases in order 4 validate the harder callback/exclusion and
scratch-liveness boundary before committing to the model. They do not authorize
reimplementing the entire collection library during the prototype checkpoint.
NC remains the owner of collection storage/algorithms and its inherited gates;
record the exact shared migration checkpoint there when implementation starts.
Do not make full NC completion, derive tooling, compact Option encoding, full R13
resolution or a general-purpose container framework prerequisites for these
proofs. Do not silently drop supported collection behavior at cutover either.

### Native call-model change sequence

Planning update, 2026-11-04: the user selected short payload borrows, release before
script callbacks and on-demand reacquisition, and requested this change breakdown.
The [activation design](runtime-gc-integration-design.md#native-activation-and-call-lifecycle)
owns the contract. This sequence refines orders 1-3; it does not restart completed
preparation/storage work or authorize the later whole-Runtime cutover. All
implementation states are tracked in the ledger below. The sequence remains open;
documentation changes alone establish no implementation or performance acceptance.

| Sequence / state | Change and existing owners | Exit evidence |
| --- | --- | --- |
| 1. Partial: production binding and baseline complete; scoped binder open | Reuse native binding/application preparation and the actual ModuleBuilder::bind route. Prove scalar, nominal/generic and scoped result shapes plus callback-bearing Local results; keep one public exchange model. Reproduce the existing native_zero/len/get/fold controls before implementation, retaining source/image hashes and environment. | Stable mapping failures precede converter/body effects; generic/nominal identity survives source-free linking and GC; ordinary authors need no Kind/View or result-slot protocol. Existing diagnostic counts are reproduced or discrepancies explained. |
| 2. Implemented and locally validated for production representation | Evolve frame admission, call_slots, Native entry/return, typed/conversion adapters and their argument projections. The Native frame owns the frozen complete input pack, protected result and bounded scratch. Typed conversion borrows these locations rather than making a second full window. Use common scalar codecs; instantiate recursive conversion state only for conversions that require it. | Effectful converters cannot observe later inputs changing through caller slots; every managed intermediate is protected before collection; malformed/custom results still fail. Input/result aliasing, unwritten Unit, failed conversion and unwind preserve error order and cleanup. Warm scalar entry has one logical Native frame/window and no independent root registration or recursive conversion context. |
| 3. Partial: parameter roles and shared short reads implemented; write/fold provider consolidation open | Native context/managed access, host-loan adapters and stdlib provider operations share short checked access. Precompute parameter roles in the existing binding/application; apply actual narrowing/borrow checks at admission. Reuse bounded fold locals and the actual provider storage. | len/get/set admit one closed access without repeated family/payload wrappers. Payload/bank/driver loans end before script calls. Collecting callbacks keep outer roots alive; alias mutation/take is detected on subsequent access. A return-only continuation does not probe an unused payload. Existing Rust host loans remain valid and reject conflicting reentry. |
| 4. Partial: Native entry/return admission consolidated | frame calls/admission/native/returns and the VM driver consume established selection and frame authority inside callback-free transitions. Ordinary calls still select current code; use the same activation protocol for SDK entry and Native script reentry. | No second session or call kernel; repeated module/state checks are removed only with a documented validity interval. Depth, observer/trap order, completed parked results and sticky inner failures retain their semantics. Named calls follow current code while closures retain exact bodies. No publication lock or raw storage loan crosses arbitrary user code. |
| 5. Partial: obsolete routes removed; parity remains open | Update affected SDK/stdlib consumers and existing boundary tests; remove the replaced snapshot orchestration and unused stale callback route, keeping no compatibility forwarding API or selectable alternate semantics. Compare unchanged production workloads when their owning path is integrated. | Focused contracts and required local quality checks pass. Count allocations, roots, windows, preparation and admissions separately from ordinary timing; record cold preparation, warmed throughput and remaining regressions. Host-per-operation measurements expose outer SDK cost. Complete workspace/backend/platform acceptance remains CI-owned. |

Implementation owners:

- Binding and conversion: native/binding.rs, native/typed/preparation.rs,
  native/typed.rs, native/conversion and native/context/references.rs. Retain
  actual value/capability validation while relocating stable parameter-role work.
- Activation and transport: frame/admission.rs, frame/call_slots.rs,
  frame/native.rs, frame/returns.rs and kagari-vm's executor/driver.rs. Use the
  existing execution banks and the single common return/cleanup protocol.
- Scoped access and provider proof: native/managed/{call,entry,slots}.rs,
  native/context operations, existing host borrow owners and kagari-stdlib's
  managed vector/fold. Current-collector corrections can land while it remains
  the only production collector; candidate-heap references remain confined to
  their reviewed prototype until orders 4/6 close retention and cutover.

Reuse the existing VM prepared-entry tests for prepared mappings, actual-result
rejection and pre-effect source-free mismatch rejection, and the managed_gc
collecting/reentrant fold proof. Select existing host-borrow, native frame/return,
module dispatch and cleanup cases for each changed boundary. Add only distinct
missing contracts (for example, a newly exposed alias/take or lifetime escape),
not another parallel smoke suite. Relevant initial filters are:

```text
cargo test -p kagari-vm --lib prepared_entry
cargo test -p kagari-vm --lib managed_fold_calls_real_script_with_collection_and_synchronous_reentry
```

These filters are a starting subset, not complete acceptance for later frame/host
changes. Choose the remaining small affected set after implementation inspection;
run structure, fmt, workspace all-target Clippy and diff checks at implementation
acceptance. Do not run the full workspace/backend matrix locally. Keep each
current-representation checkpoint building; if a later representation change
requires a broken intermediate boundary, record its exact failures and repair
owner under the existing GCR03 policy before beginning that cutover.

Cancellation removal stays in order 5/GCR03 and is measured independently. Core
GC scheduling, sticky failure, host loans and bounds are not cancellation polls.
Complete collection migration, new Option encoding, global target caches,
thread/concurrency changes and broader JIT support are outside this sequence.
Actual ABI/backend consumers of any changed internal contract must still be
updated at its owning checkpoint. A missed timing gate requires renewed path
attribution, not a container-specific unchecked API or weaker assertions.

### Native activation implementation ledger

2026-11-04: implementation of the ordered Native change sequence is now requested.
The user sets Lua parity as the performance objective; no parity is established.
The initial comparable gate is the unchanged native_zero same-Rust-helper route,
with len/get/fold and host entry retained as distinct checks rather than substitutes.
The production binding/application mapping proofs are reused and parameter-access
classification now belongs to those immutable owners.
Remaining nominal/generic scoped binding and lifetime work is not marked complete.

Baseline: 01f5edbb production sources, with only AGENTS/design/plan edits; Windows
11 10.0.26300, i9-12900K, 24 logical CPUs, 64 GiB; rustc 1.99.1 (58cf8f9c3),
LLVM 23.1.1, x86_64 MSVC. Workspace release/default target/default parallelism,
source/native and mlua lua54/vendored; interpreter execution, no automatic GC,
untimed full collection per batch. Seven sequential fresh processes, three
warmups and eleven samples; compilation/preparation excluded. Build took 31.407 s.
All checksums and mutation postconditions passed. Median batch microseconds:

| Workload | Kagari | Lua | Ratio of reported medians |
| --- | ---: | ---: | ---: |
| native_zero (5000 calls) | 4976.4 | 108.4 | 45.91 |
| loop_control | 37.2 | 23.9 | 1.56 |
| vec_len | 4186.2 | 50.3 | 83.22 |
| vec_get | 4389.3 | 58.5 | 75.03 |
| fold_scalar | 16742.8 | 43.6 | 384.01 |
| fold_references | 20148.1 | 48.0 | 419.75 |

Reproduce using the existing driver: `uv run python scripts/benchmark_lua.py
--stdlib-ops --runs 7 --freeze-baseline NEW_IMAGE_PATH`. The local frozen image is
under target/lua-comparison/native-activation-before; executable SHA-256
`eedf41d4f0679a34508283e81368f617b1205820d244b83bb8739d6cd9cc2071`,
workload SHA-256 `55898e479498738810ca8ab429daf33be7133edf85ddd04ceb21fd52b540278b`.
These timings include loop and outer SDK entry; they are not isolated FFI latency
or directly comparable to the earlier i7-10700F measurements. No concurrent
build/check ran during the timing processes. Final focused local and CI acceptance
are reported below; the baseline measurement alone was not implementation evidence.

Production Native checkpoint:

- Replaced typed callback snapshots with a projection of the admitted Native
  frame. Parameters remain frozen; concrete interface-adapted results use one
  generic scratch slot in that same window. Host-to-script conversion still owns
  its temporary window. This is not the candidate-heap cutover.
- Prepared shared-access parameter roles once per closed/applied binding; narrowed
  actual scoped capabilities in callee slots without replacing caller capabilities.
- Added engine-issued scalar conversion evidence backed by the existing sealed
  NativeScalar codec. Scalar packs retain physical/narrowing checks; custom packs
  retain their effectful conversion. Scalar output is published by the common
  Native return transition. Recursive conversion state is created only on use.
- Reused admitted Native-frame owner authority for conversion. Route selection
  already checks the installation and available/current publication, so it no
  longer repeats executable installation validation within that closed operation.
  Frame admission still validates metadata before publishing roots. Native return
  consumes the completed result under one common frame-retirement admission.
- Removed the unreachable retained-native cursor path and its linked descriptors;
  removed the direct scalar callback bypass. Selected callbacks use the normal
  execution frame, current-route selection, depth, observer and cleanup protocol.
- No payload reference is held across user conversion or reentry. The candidate
  managed short-region/reentrant fold proof remains intact; nominal/generic Local
  binding and production provider consolidation remain open steps 1/3. The whole
  call-transition consolidation in step 4 is not complete.

Focused validation already observed: 38 VM tests selected by Native conversion,
callback and control owners, the collecting/reentrant managed fold, and the admitted
Native/current-callback regression; 11 runtime tests for cursor return publication,
stored hash selections, and argument admission precedence. Initial runtime test
compilation exposed a missing ScriptInvoker argument in a migrated cursor fixture;
the fixture was repaired and the selected tests rerun successfully. No assertion
was weakened. Four dispatch-selection tests and six additional VM boxed-Native/host-boundary
tests also passed, including completed Native results surviving parked execution
and collection. Workspace all-target Clippy found five stale execute_region
arguments in the allocation-contract harness; those callers were migrated without
changing assertions. Both selected allocation-contract tests then passed, retaining
zero warmed allocation/release requirements. Workspace all-target Clippy passed after that harness repair. The 61 selected
contract tests, repository structure check, cargo fmt check and git diff check
passed. Production imports/ownership were reviewed: no new unsafe code, re-exports,
compatibility entrypoints, handwritten include or structural exception was added.
The complete feature/backend/platform matrix remains CI-owned and unrun locally.

Initial three-pair throughput probe passed every checksum. Native zero was
3570.5 us / 5000 calls versus Lua 101.2 us (35.28x); it predates the final duplicate
entry/return-validation consolidation and is not the final timing result. Other
stdlib workloads showed no broad improvement. This falsifies any claim that
removing typed snapshots alone closes the gap: repeated general activation,
selection, return and conversion work still dominates. Reattribute the remaining
path before selecting further changes; do not add a separate unchecked callback
kernel or claim Lua parity from this checkpoint.

Final ordinary timing, seven interleaved baseline/candidate process pairs, same
machine/toolchain/profile/features/workload hash as above; three warmups and eleven
samples per route; no compilation or tests overlapped timing. Build: 29.001 s.
All checksums and mutation postconditions passed. The frozen baseline was rerun,
so the comparison below uses paired-run medians rather than the first baseline:

| Workload | Baseline Kagari us | Candidate Kagari us | Candidate Lua us |
| --- | ---: | ---: | ---: |
| loop_control | 38.6 | 36.0 | 26.0 |
| native_zero | 4993.9 | 2878.6 | 106.2 |
| vec_len | 4254.9 | 3935.2 | 50.2 |
| vec_get | 4400.3 | 4159.7 | 58.3 |
| vec_set_get | 9661.3 | 9194.9 | 80.7 |
| vec_pop_push | 13160.1 | 12744.1 | 377.1 |
| fold_scalar | 16646.6 | 16768.0 | 43.7 |
| fold_references | 20364.0 | 20283.3 | 48.3 |

Native zero workload time decreased 42.36%, but remains 27.11x Lua. Fold has no
established improvement; the small scalar-fold median regression is retained
rather than presented as a win. Neither the Native sequence nor Lua parity is
complete. Candidate executable SHA-256:
`d9295da68d0054afeb7c910283ab24165b8564487018e207e8a20664df3d1a90`.
Reproduce with `uv run python scripts/benchmark_lua.py --stdlib-ops --runs 7
--baseline-executable FROZEN_BASELINE_EXE` and its matching stdlib report.

Separate diagnostic build: `cargo run --release -p kagari-lua-benchmark --features
diagnostics -- --stdlib-ops --diagnostics --check`. Subtracting warmed loop-control
counts from native_zero and dividing by 5000 gives one selection/admission/return,
one window, one module validation, nine polls, ten state guards and two GC service
points per inner invocation. Added recursive conversion contexts, Rust allocation
requests, roots, heap objects, collections, mapping/value matchers and preparations
are all zero. Whole-batch outer SDK cost remains 35 allocation requests, 2256 requested
bytes, two conversion contexts and one root-pack registration, also present in the
loop control. The diagnostic build supplies counts, never throughput evidence.

This attribution leaves a concrete next owner: the shared selection/frame/driver
activation lifecycle still performs nine polls and ten guards for each empty body.
The conversion layer is no longer the source of those counts. Sequence len/get
still admit family/contract twice per access; fold still crosses the full callback
conversion/entry protocol. Finish the pending scoped binding/provider work and
consolidate those established activation boundaries before any collector cutover;
benchmark the same production workloads again. No unchecked special entry,
changed workload, removed cancellation contract or permanent global target cache
is justified by the missed target.

Continuation: current-target selection now prepares binder transport only when a
consumer requests it. Immediate frame admission consumes the selected authority
inside a private, callback-free transaction; exact host/queued entries retain
their independent metadata validation. The current publication resolves its own
member ordinal directly, while candidates and retained old bodies still consult
record availability. Registry checks remain; an installation already obtained
from this Runtime's table does not repeat the same table lookup for Arc equality.

The runtime now owns the shared instruction-entry prelude (poll, diagnostic PC,
GC service, observation). The VM preserves prelude errors outside the operation's
Trap notification boundary. Resumed/parked slices still poll, as do Native entry,
post-callback and return boundaries. This consolidates duplicate polling without
activating GCR03's separately scoped cancellation-policy removal. A first
three-pair probe before the member-lookup change passed all checksums: native_zero
2531.0 us versus Lua 111.2 us, 22.76x. Same machine/toolchain/profile and workload
as above; build 30.059 s excluded. It remains a missed performance gate.
Separate counts now show six polls and zero extra module validations per empty
call; ten execution guards remain. These counts do not establish each guard's
individual timing cost.

Buffer family/layout/payload inspection now shares one internal borrowed record.
Length/snapshot read under that borrow; contract-only admission no longer clones
an Arc merely to check availability. Host TryInto index conversion retains its
original receiver-admission order and runs outside all payload borrows, followed
by reacquisition. Fixed-array/nominal-family rejection and detached-edit semantics
remain unchanged. That internal kernel introduces no unchecked library path.
Seven runtime buffer/accounting tests and seven VM partial-edit, detached-cursor,
alias extraction and conversion tests passed. The corrected instruction prelude
passed fourteen Native control/boxing/preparation tests. All twelve selected
dispatch/application tests passed after the member-lookup change.

The next bounded investigation is the common failure carrier: successful Native
and frame APIs still return a full inline RuntimeError, whereas two older local
workarounds box that same error in primitive and region paths. Measure its actual
ABI width before deciding whether to centralize cold error ownership and remove
those wrappers. This concerns successful boundary transport, not weaker checks or
changed failure behavior. A representation change must preserve codes/messages,
trace/task provenance, first-failure precedence and task heap accounting. Relevant
mature reference: [anyhow 1.0.104 Error](https://docs.rs/anyhow/1.0.104/anyhow/struct.Error.html)
keeps the error carrier one word; Kagari can use an ordinary owned concrete Box
instead of its dynamic type erasure, backtrace policy or unsafe pointer machinery.
No timing improvement is inferred from that reference.

The ABI probe measured RuntimeError / Result<(), RuntimeError> /
Result<Value, RuntimeError> at 72/72/72 bytes before the carrier change and
8/8/16 afterward on the same x86_64 toolchain. The probe was removed after use.
RuntimeError now owns its complete concrete diagnostic record through Box;
clones still independently own messages and share only the existing trace Arc.
The primitive-only result alias and region error Box were removed. Task charge
includes the diagnostic record's backing as well as message capacity; its
running/terminated/quarantined cleanup test retains exact accounting assertions.
Seven runtime error/buffer/accounting tests and thirteen VM cancellation, observer,
reentry and failure-precedence tests pass. A new buffer-owner case establishes
that arbitrary host index conversion runs after family admission, outside a loan,
and its completed mutation is seen by the subsequent read.

Three interleaved process pairs against the immediately preceding frozen image
passed all checksums. Native zero was 2080.8 us versus Lua 102.8 us (20.24x);
build 31.216 s excluded. The original environment/profile/workload applies.
This modest result does not close the lifecycle gap. The carrier is a shared
transport correction, replacing the two local wrappers, not an empty-call bypass.

Production library length/get and primitive indexing now consume one short
SequenceRead loan through CallContext::read_sequence and the shared native-record
borrow owner. That access checks generation, nominal/application type, storage
family, payload and detached state once. The loan cannot escape or permit script
reentry/collection; later access reacquires and checks the actual object. Values
copied out still need explicit protection before alias-mutating callbacks.
Write/edit operations retain their existing mutation/commit protocol; arbitrary
host TryInto conversion retains its separate admission/reacquisition boundary.
Eight selected VM detached-buffer/partial-edit/extraction/trace tests pass with
this read path. General scoped nominal bindings and write/fold consolidation
remain open; this is not acceptance of the entire Native sequence.

Latest ordinary gate: seven interleaved process pairs against the original frozen
baseline, same machine/toolchain/default release profile/features/workload hash,
three warmups and eleven samples per process. No builds/checks overlapped timing.
All checksums and mutation postconditions passed in all fourteen processes;
build 31.162 s excluded. Median complete-workload microseconds:

| Workload | Baseline Kagari us | Candidate Kagari us | Candidate Lua us |
| --- | ---: | ---: | ---: |
| loop_control | 38.7 | 36.0 | 31.7 |
| native_zero | 5018.8 | 2079.6 | 110.8 |
| vec_len | 4297.9 | 2842.3 | 51.6 |
| vec_get | 4372.7 | 3013.9 | 59.7 |
| vec_set_get | 9546.6 | 6791.5 | 79.3 |
| vec_pop_push | 13128.4 | 10430.8 | 356.7 |
| fold_scalar | 16571.2 | 15843.7 | 44.8 |
| fold_references | 20406.0 | 19406.5 | 50.6 |

Native zero is 58.56% lower than the rerun baseline but remains 18.77x Lua.
Candidate image SHA-256:
`eee26f6e1dc0cf5ea777d901f0f940baa169d518b0a131f857c2de2012b71ace`.
Reproduction remains `uv run python scripts/benchmark_lua.py --stdlib-ops --runs 7
--baseline-executable FROZEN_BASELINE_EXE`. Ratios include loop/outer host entry;
no isolated FFI latency, full Lua parity or collector-cutover win is claimed.

Separate diagnostic run confirms empty-call deltas of six polls, ten state guards,
zero module validations, two GC service points and one admitted frame/window.
Warm allocation requests, independent roots and conversion contexts add zero.
Length/index reads now have one family admission, one payload read and no contract
Arc clone. Their eleven state guards include the real short-loan entry gate.
Whole-batch outer SDK cost remains 35 allocation requests, now 2064 requested
bytes, two conversion contexts and one root-pack registration. Those outer costs
are also present in loop_control and are not per-inner-call costs.

Remaining architectural diagnosis: the empty body still crosses current selection,
frame admission, observed Native entry and common retirement; duplicate conversion
storage is no longer the cause. The six polls/ten guards belong to that distributed
control path, not to payload access. Fold still crosses full callback conversion
and activation for each element. The existing sequence's shared activation and
scoped binding/write/fold work remains required; neither more unchecked per-method
variants nor cancellation removal under this checkpoint is justified. No global
target cache, changed workload or unrun CI result is used to claim acceptance.

Latest local acceptance: workspace all-target Clippy with denied warnings, fmt,
whole-repository structure (1814 files, zero violations) and diff checks passed.
The new short-read compile-fail example rejects escaping its payload lifetime.
Both selected scoped-host capability/revocation tests and both warmed Native
allocation-contract tests also passed after the shared borrow-owner change.
All selected runtime/VM tests described above passed. Full workspace tests and
the complete feature/backend/platform matrix remain CI-only and unrun locally.

### GCR03 migration and build boundary

Expected affected owners are kagari-gc, kagari-runtime, kagari-stdlib, kagari-vm,
kagari and the actual ABI/codegen/Cranelift consumers of changed value/call
contracts. Audit kagari-derive, compiler/artifact metadata, CLI and web/example
consumers for concrete affected contracts before listing them as implementation
work. Frontend/type analysis is not to acquire a collector dependency, and the
backend must continue consuming checked source-free executable facts.

Prefer building checkpoints for responsibility separation and execution-policy
removal while the old collector is still the sole production collector. Update
the affected execution specifications/examples with that behavior change; do not
leave them promising in-flight interruption after its implementation is removed.
The physical Value/root/provider/backend migration is one coherent cutover even
if its edits need several batches. Do not promise every batch independently
builds: before the first incompatible edit, record its exact affected crates,
expected diagnostics and the next repair checkpoint here. Attempt the affected
checks at each meaningful boundary, preserve distinct behavioral coverage and
carry any real failures in this plan until resolved. There are no new carried
build errors from this documentation checkpoint.

Orders 1-4 must close the actual representation, generic Native adaptation,
root completeness and provider-access questions before order 6 starts. Use a
small correctly connected execution path first within the cutover, then finish
all supported consumers before claiming GCR03 complete. No production stubs,
ignored validation, old-ID compatibility adapters or permanent parallel banks
are accepted ways to obtain a green intermediate build.

Local implementation acceptance uses affected contract tests, structure/fmt,
workspace all-target Clippy and diff checks. Full workspace tests and the complete
feature/backend/platform matrix remain CI-only. Reuse passing evidence unless a
change affects it. Documentation-only sequencing changes need content/link/diff
checks and do not establish implementation or performance acceptance.

### Orders 1-3 executable checkpoint

The user-authorized first trial is implemented in these existing owners:

- [Runtime managed components](../crates/kagari-runtime/src/native/managed/mod.rs):
  one private Repr family, scoped Value, opaque non-Copy StoredValue, immutable
  typed host snapshots with scalar owner identity, sealed physical kinds,
  generative Locals, a complete direct slot source and short driver access.
  NativeCall retains a shared Memory reference, not a driver/payload loan across
  callbacks. Same-driver nested entry is checked dynamically while an access
  region is live and succeeds after it ends.
- [Typed scoped entry](../crates/kagari-runtime/src/native/managed/entry.rs) and
  [argument projections](../crates/kagari-runtime/src/native/managed/arguments.rs):
   supported outer tuples are (), (A,) and (A, B). Rust checks the real scoped
   output family; the adapter publishes before region exit. Closed immediate-scalar
  declarations can be checked once with prepare_scalar. There is no signature
  walk in invoke and no manual return completion. new constructs a Rust-only
  body; it is deliberately not a script nominal/application proof.
- [Stable Native shell](../crates/kagari-runtime/src/native/managed/payload.rs):
  Present/Taken preserves the Rust descriptor and typed-handle validity. Ordinary
  edge-free payloads implement the reviewed EdgeFree contract once; methods use
  safe read/edit/take. Reference-bearing providers use GcObject and generic unsafe
  construction/mutation once at their storage boundary. Missing tracing is never
  justified by the old safe NativePayload trait.
- [Stdlib value vector](../crates/kagari-stdlib/src/managed/vector.rs): the same
  provider stores unboxed u64 and managed references, publishes before commit,
  stages amortized capacity growth at its actual charge, repairs incremental
  tail-scan state and owns a revision. pop protects a scoped value, removes the
  source, then admits/returns it; later failure does not roll removal back.
  Taken Native values cannot be newly published. All operations use Runtime/core
  public generic services, without crate-private collection hooks.
- [Stdlib fold](../crates/kagari-stdlib/src/managed/fold.rs) and the
  [real VM proof](../crates/kagari-vm/src/tests/native_boundary/managed_gc.rs):
  one source slot and two reused scalar working slots survive script -> Native
  -> script reentry and collection from inside the script callback. The fixture
  uses the real compiler, installed native modules, typed callable routes and VM.
  A test-only scoped host service selects the same Memory owner; no managed
  references cross between the old Runtime heap and the candidate heap.

The representative [provider tests](../crates/kagari-stdlib/src/managed/tests.rs)
cover real ordinary payload access, typed push/pop, removal/error order, take,
capacity and incremental scanning, host snapshot ownership, unwind cleanup,
borrow rejection and scalar declaration preparation. With max_roots=0 the normal
vector/fold paths run without host root registrations. The scalar vector stores
128 values in one managed container object, not 128 boxed scalar objects.
These are behavioral/ownership observations, not throughput results.

Important boundaries found by this trial:

1. CallRegion needs both access and GC-brand lifetimes. Returning scoped values
   works through a higher-ranked output family only when publication happens
   inside the adapter's region. Ordinary region callbacks cannot return Gc.
2. Holding &mut Memory for a whole Native call prevents independently driven
   synchronous reentry. The driver is now exclusively borrowed only inside each
   region/collection operation; Memory itself is shared by active nested calls.
3. Present/Taken can invalidate an admitted physical object without changing its
   type. Read helpers can fail, and language-value publication must check state;
   typed physical handles do not authorize removed-payload access.
4. Explicit Rust kind tuples are currently required to instantiate ScopedEntry.
   Object<P> proves a physical Native shell, not an arbitrary loaded script type
   or generic application. Scalar preparation is intentionally restricted. Do
   not generalize it into a nominal-type trust flag or infer the script type from
   Rust P. Direct ModuleBuilder::bind integration for the new scoped families,
   application/witness ownership and custom-value ingress remain orders 1-3
   closure work before advancing to the complete retention/cutover checkpoints.
5. The proof covers immediate scalar and Native object kinds, with one optional working-
   slot projection. It does not implement the language Option encoding or nested
   Option transport. fold covers revision-checked vector projection and a u64
   accumulator, not all Iterable/generic reducer combinations. Other language
   representations, host mutable slots and complete module/async roots remain at
   their existing design/phase owners.

The production collector, Value ABI, NativeEntry dispatcher, execution cancellation
and ordinary stdlib registrations are unchanged. The scalar callback bridge uses
those existing production adapters, including their current conversion/checking
costs; its success does not prove a new production Native ABI or speedup. No
second Runtime, provider registry, production compatibility adapter or selectable
collector mode was introduced. These results do not close GCR00/GCR02 as a whole.

Focused local evidence for this checkpoint:

- cargo test -p kagari-stdlib -p kagari-vm --lib managed --no-fail-fast:
  7 provider tests and 8 relevant VM tests pass, including the real collecting
  reentry route and existing managed-payload/code-retention regressions.
- cargo test -p kagari-runtime --doc native::managed --no-fail-fast:
  4 compile-fail cases pass for missing/wrong returns and GC/call-local escape.
- cargo test -p kagari-vm --lib registered_struct_loans_use_original_host_storage_and_common_methods --no-fail-fast:
  the existing source-free scoped-host-loan contract passes; the prototype reuses
  that host boundary rather than define a second borrowing mechanism.
- cargo test -p kagari-stdlib --lib fold_reports_alias_mutation --no-fail-fast:
  the additional provider case passes, checking the next-read trap even when the
  final callback mutates the source. Eight distinct provider cases now pass.
- The final managed_gc VM case passes with original script-trap propagation and
  candidate-source cleanup on the failing path. Its earlier successful collection
  and reentry assertions remain in the same case. This is 17 distinct focused
  behavioral cases across the commands above, plus the 4 compile-fail cases.
- Structure, fmt, workspace all-target Clippy, content/link and diff checks pass.
  The content check covers 6 affected documents and 539 local links/anchors.
  Full workspace/backend/platform tests remain CI-only and were not run locally.
  Performance workloads and historical gates above remain unchanged; none was
  rerun to claim a speedup.

Resolved development failures: the first lifetime prototype let the view/region
brands differ; explicit invariant lifetimes and the region-dependent result family
resolve that. Provider tests initially lived inside Runtime's unit-test copy,
creating mismatched Runtime types through its stdlib dev-dependency; moving them
to the stdlib owner exercises the actual public boundary. A global From<GcError>
implementation also widened unrelated error inference; conversion now lives only
at the managed memory boundary and unrelated production code is restored. The
first VM source probe used Rust let/named-function-value syntax; the fixture now
uses Kagari val and an ordinary closure. These failures are resolved; no carried
build failure is selected as an acceptable intermediate state.

### Candidate fold measurements

Measurement checkpoint, 2026-11-04: the user requested timing of the current
bounded fold before extending its API. The
[fixture and workload guide](../crates/kagari-vm/examples/managed_fold/README.md)
and [runner](../scripts/benchmark_managed_fold.py) reuse the actual candidate
fold/provider and existing interpreter/typed Function adapter. No production
algorithm, collector, cancellation mechanism or Native ABI was changed.

Reproduce with a new output directory:

```text
uv run python scripts/benchmark_managed_fold.py --output target/managed-fold/baseline
```

Environment: Intel Core i7-10700F (8 cores/16 logical CPUs), Windows 10 build 19045,
rustc 1.99.1 (58cf8f9c3, LLVM 23.1.1), x86_64-pc-windows-msvc, workspace release
profile, kagari-vm default features/example dev-dependencies, bytecode interpreter
without JIT, default Cargo target/parallelism and no recorded build/Rust flag
overrides. Revision ccb63194 plus the new fixture/runner; production sources were
unchanged. The cached release rebuild took 5.361 seconds, separately from execution.
The initial release dependency build took 2m02s. Five benchmark processes ran
sequentially without overlapping builds/tests/profiling. Each route/process used
three warmups and eleven measured samples; order rotates across samples/processes.
Inputs and source hashes were checked before/after. CPU frequency, affinity,
power state and background activity were not controlled. This is warm execution,
not cold-cache or production-tail acceptance.

Each fold sums 1..2048. Direct Rust batches contain 256 folds, candidate/Rust
batches 16, and script batches one. Data construction, checking and pre-batch
collections are outside timing; call admission, callback execution and callback
collections are inside. Reported estimates are medians of five process medians;
ranges contain those process medians, not confidence intervals.

| Route | ns/item | Process-median range, ns/item | us/2048-item fold |
| --- | ---: | ---: | ---: |
| Direct Rust Vec, Rust reducer | 0.315 | 0.314-0.323 | 0.646 |
| Candidate scalar vector, Rust reducer | 69.043 | 68.411-70.444 | 141.400 |
| Candidate reference vector, Rust reducer | 89.075 | 87.976-89.548 | 182.425 |
| Direct Rust Vec, existing typed script callback | 7857.178 | 7794.873-8122.998 | 16091.500 |
| Candidate scalar vector, same script callback | 8156.934 | 7956.494-8487.744 | 16705.400 |
| Candidate reference vector, same script callback | 8285.156 | 8034.961-8799.951 | 16968.000 |
| Production Vec/Iterable fold, script callback | 13784.814 | 13559.033-14593.848 | 28231.300 |
| Direct Rust Vec, allocating/collecting script callback | 12562.646 | 12281.543-13383.057 | 25728.300 |
| Candidate reference vector, same allocating/collecting callback | 13224.072 | 12573.975-13689.941 | 27082.900 |
| Production Vec/Iterable fold, same allocating/collecting callback | 19804.004 | 19372.900-20372.559 | 40558.600 |

Setup medians, excluded from traversal: candidate data 0.459 ms, standard/native
declarations plus source checking/lowering 2253.086 ms, Runtime installation/linking
314.018 ms, and production Vec construction plus initial collection 3.394 ms.
The last two data recipes construct different representations and are not a
paired allocation-throughput comparison.

Interpretation and architectural follow-up:

- The candidate is not zero-cost: its scalar traversal is about 218 times this
  deliberately simple Rust/black_box loop; reference projection costs about 29%
  more than scalar traversal. Candidate timing includes short regions, revision
  and payload admission, slot transfers and fixed per-fold call setup/cleanup.
  These measurements do not isolate the contribution of each check or region.
- The existing typed script callback route alone already costs about 7.86 us/item
  in this trivial-reducer workload. Candidate scalar/reference routes add median
  process-paired costs of 4.3%/3.8% relative to that direct Rust callback loop.
  Process ranges overlap; do not interpret small differences as stable wins or
  independently measured per-check CPU costs. Current callback execution and
  its existing adaptation dominate end-to-end time; the split inside that path
  still requires profiling.
- The candidate scalar/script route takes about 41% less time than production
  Iterable::fold here (median paired production/candidate ratio 1.699). This
  changes specialized vector traversal versus generic cursor calls, language
  Option allocation, root operations, polls and adapters together. It is not an
  isolated collector improvement, cancellation-removal result or proof that a
  future full Iterable/generic accumulator has the same performance.
- Pressure callbacks allocate a two-element script array on every item, collect
  both heaps every 256 items, then read the array again. All routes retain/scan
  the same 2050 candidate objects through two host snapshots; the managed route
  also has active call slots. Exact eight-cycle progress, scalar result, object
  count and root-registration checks pass outside each batch timer. Candidate
  reference/production pressure folds take 27.083/40.559 ms; the median paired
  production/candidate ratio is 1.498. This finite, explicitly collected two-heap
  fixture does not measure new-heap allocation throughput, automatic pressure
  pacing, individual pause percentiles or final teardown.
- Before optimizing this path, profile the existing Function::call/reentry and
  conversion/preparation work separately from candidate region/slot admission.
  Close the already-owned nominal/generic prepared entry contract rather than
  inventing a second callback cache or removing state-invalidatable checks. No
  optimization is implemented or selected solely from these aggregate timings.

The runner preserves raw samples, setup records, source/executable hashes and
per-process estimates under ignored target output. The table and recipe above
remain usable without those artifacts. All 700 fixture rows passed checksum,
root/object and scheduled collection assertions; parsing verified the complete
route/sample matrix. Workspace all-target Clippy and the focused example check
pass; structure/fmt/link/diff checks pass after correcting one import qualification.
The first fixture source used unsupported Vec::<u64>::new syntax; a typed
Vec::from([]) initializer resolves it. No carried errors remain. No full workspace
tests or backend/platform matrix was run; those remain CI-only. Historical GC
gates and GCR00/GCR02 closure remain open.

### Native callback architectural diagnosis

Diagnosis checkpoint, 2026-11-04: investigate ownership/design before optimizing
the measured callback path. The same fixture now supplies `--diagnose` timing,
separate opt-in `--counts` and Windows instruction-pointer sampling through the
existing sampler. Production Runtime/VM/stdlib code is unchanged. The VM's new
diagnostics feature only forwards the existing Runtime diagnostics capability;
ordinary timing uses default features without the counting allocator/counters.

Reproduce each mode separately, with new output directories:

```text
uv run python scripts/benchmark_managed_fold.py --diagnose --output target/managed-fold/diagnosis
uv run python scripts/benchmark_managed_fold.py --counts --output target/managed-fold/counts
uv run python scripts/benchmark_managed_fold.py --sample bridge --sample bridge_raw --sample managed_rust_scalar --output target/managed-fold/profile
```

Timing uses the preceding checkpoint's machine/toolchain/release profile, default
features, warmups, process/sample counts and 2048-item inputs. All builds,
instrumented counts and sampling run separately from throughput. The normal
diagnostic matrix has 700 checked batches and a complete route/sample matrix.
The callback is the same anonymous, exact-body scalar addition closure; these
measurements do not characterize named-route reload selection or general
converters. Script-loop controls include their own loop instructions and are not
exact subtractions for callback overhead.

| Diagnostic route | ns/item, median | Process-median range | Allocator requests per complete 2048-item fold |
| --- | ---: | ---: | ---: |
| Owning typed Function::call over Rust Vec | 7915.967 | 7821.729-8044.434 | 90458 |
| Same Function prepared once before the loop | 7706.543 | 7675.537-7798.926 | 90458 |
| Borrowed CallableHandle scalar call over Rust Vec | 1808.643 | 1795.752-1824.854 | 4203 |
| Borrowed CallableHandle general Value call | 3341.260 | 3275.342-3364.795 | 43116 |
| Script loop calling the same-shape closure | 924.121 | 913.281-933.447 | 35 |
| Script addition loop without closure calls | 15.186 | 14.600-16.602 | 33 |
| Candidate scalar fold, Rust reducer | 69.174 | 68.808-71.033 | 0 |
| Candidate reference fold, Rust reducer | 88.583 | 87.805-90.952 | 0 |
| Candidate scalar fold, owning typed script callback | 8054.541 | 7981.543-8278.662 | 90456 |
| Candidate reference fold, owning typed script callback | 8140.625 | 8077.734-8311.230 | 90456 |

Allocation counts come from an independently built release executable with the
existing counting allocator and execution counters. They cover one warmed full
fold, including outer entry/closure work, not exactly N identical callback-only
events. The two-allocation difference between candidate and bridge rows is not a
per-item advantage. Counting build time was 5.236 seconds and is excluded. Typed
and borrowed scalar rows both have 2049 driver admissions, 2050 frame admissions/
returns/windows and 4097 argument transfers. The script closure loop has one
driver admission, 2049 frame admissions/returns/windows and 4096 transfers.
There are zero per-run native/layout/shared preparation events in these warmed
routes. A window-allocation event means logical window admission, not necessarily
a new heap allocation: the banks/windows already reuse capacity.

Two isolated, untimed allocation probes localize stable Rust mapping work:

- 2048 checks of an already supplied u64 TypeArgument request 16384 allocations
  and 917504 bytes: exactly eight requests/448 bytes per check in this build.
- 2048 encodings of a two-u64 argument pack request 69698 allocations and 3916240
  bytes. This includes four mapping checks per pack, conversion, a protected
  argument pack and release bookkeeping; it is not merely copying two integers.

The typed callback path checks its Rust result mapping once, its two argument
mappings before conversion, and both argument mappings again in encode_value:
five stable mapping checks per callback. The isolated result accounts for about
40 allocation requests per callback, consistent with the approximately 44 total
requests/item observed in the complete typed route. This is allocation evidence,
not an assertion that those checks consume the same proportion of CPU time.

Source-level responsibility diagnosis:

1. [Function::call](../crates/kagari-runtime/src/native/function_handle/mod.rs)
   erases its A/R correspondence into PreparedFunction and then calls that
   untyped prepared object's generic invocation method. The latter accepts new
   arbitrary Rust mappings, so its checks are necessary under its present public
   contract. Moving preparation out of the loop changes paired time by only
   -2.4% and leaves allocation counts unchanged. The anonymous closure already
   carries its exact prepared body; a new global function cache is not the fix.
2. [ConversionContext](../crates/kagari-runtime/src/native/conversion/context.rs)
   checks a Rust declaration by constructing a new actual TypeArgument through
   [native_type_argument](../crates/kagari-runtime/src/native/context/enums.rs),
   then comparing it with the supplied expected argument. This repeats immutable
   declaration/scope preparation even for u64. The
   [argument adapter](../crates/kagari-runtime/src/native/conversion/arguments.rs)
   checks the complete pack and then invokes individually checked encoders.
   The root cause is losing/reconstructing proof across adapter boundaries, not
   a need for a faster allocator or a scalar-only validation bypass.
3. That same argument adapter returns an owning RootSet, extracts another
   Vec<Value>, and copies those values into already-owned execution windows.
   The conversion scope also independently retains the program. Owned Rust
   conversion is appropriate for actual host data and potentially allocating
   custom converters; synchronous script-to-native-to-script value transfer
   should use the authoritative call/frame slots and existing execution owner.
   Moving all callers to the raw scalar API would leave two semantic paths and
   would not solve generic managed values.
4. Runtime already has one shared frame stack and reusable scalar/managed banks;
   [ExecutionStack](../crates/kagari-runtime/src/frame.rs) is a nested scope over
   that stack. The
   [backend invoker](../crates/kagari-vm/src/executor/native.rs) enters another
   stack scope, not another Runtime/heap. Typed invocation also begins a pinned
   session before conversions and again through backend admission. For this
   exact closure, PreparedFunction::validate is reached five times along
   prepare/call/session/backend/push paths. Some dynamic revalidation after a
   custom converter is required; consolidate admission ownership instead of
   deleting checks mechanically. Active logical depth, root publication,
   sticky failure, borrow exclusion and error cleanup must remain.
5. Candidate Rust-reducer folds allocate nothing when warm, but their three
   per-item access intervals and provider operations repeatedly resolve the
   source object. Vector view construction, revision, slot admission, payload
   state and stored-value extraction overlap physically. This is a separate
   provider/access-granularity question, not the explanation for 7.9 us typed
   callbacks. A callback-free provider next_into operation should combine
   revision/read/publication, then release all loans before invocation and
   reacquire after it. It must not cache mutable payload addresses/state across
   callbacks or assume a region prevents alias mutation/take.

Three ten-second default-release RIP samples yielded 3892/3898/3887 observations
for typed bridge/borrowed bridge/candidate scalar fold, with zero context-read
errors. Strict symbol-range verification leaves 78.06%/84.68%/15.23% unresolved.
The typed/borrowed recordings cannot support a complete hotspot percentage or
per-function CPU-time attribution. Candidate resolved samples include physical
heap resolution and region/invoke/slot work, consistent with the separate
zero-allocation diagnosis; inlining and merged monomorph symbols preclude exact
component costs. No profile instrumentation, debug profile override or PDB-based
nearest-symbol guess was used to claim throughput improvements.

Architecture direction and next bounded checkpoint:

- Keep immutable script/application evidence on the existing linked owner;
  preserve the Rust parameter/result adapter proof through typed preparation.
  The erased host entry remains checked, but the ordinary typed path must not
  erase its proof and rediscover it on every invocation. Validate custom external
  values after their converters, not by trusting an open Rust trait or a flag.
- Connect the already-designed NativeCall Local/invoke_into path to the shared
  backend call protocol. Transfer arguments from protected caller storage into
  callee windows and publish the result into a protected caller destination.
  Owning IntoKagari/FromKagari conversion and independent host roots remain at
  actual host data/escaping-value boundaries. Do not pass candidate-heap objects
  into the old VM as an intermediate shortcut; this requires the existing
  representation/cutover checkpoint.
- Make execution own one invocation-admission/cleanup protocol shared by typed,
  erased and borrowed entry adapters. Retain current-code selection for named
  routes, exact bodies for closures, call depth, phase, sticky failures, borrow
  state and GC roots. Immutable mapping proof is reusable; lifecycle admission
  can be invalidated by converter/callback reentry and must be checked there.
- The next work belongs to the already-owned orders 1-3 callback/prepared-application
  boundary, then feeds orders 4/6 retention/cutover. The implementation proof must run
  scalar and managed-reference accumulators through actual bind/application
  adapters, including allocating callbacks, same-Runtime reentry, alias mutation,
  mismatched/foreign values, ignored inner failures, trap/depth cleanup and named
  versus exact closure reload behavior. Reuse existing boundary tests and paired
  fixtures; no promise of a particular final ns/call follows from the raw control.

The [design amendment](runtime-gc-integration-design.md#callback-proof-and-transfer-ownership)
records the intended boundary and mature references. No production optimization
or expanded migration phase is implemented by this diagnosis. Historical
performance gates and final generic ABI acceptance remain open.

Local validation: the diagnostic timing matrix, separate allocation/event probes
and three independent sampling windows completed successfully. Workspace all-target
Clippy, the example's diagnostics-feature Clippy, structure, fmt, runner syntax,
document links and diff checks pass. Full workspace/backend/platform tests remain
CI-only and were not run. The first raw fixture binding omitted its CallableHandle
parameter and was correctly rejected for declaration/codec arity; the final fixture
uses the ordinary borrowed-argument adapter. No production bypass or carried build
failure was introduced.

### Full-symbol sampling follow-up

Tooling checkpoint, 2026-11-04: the user authorized preserving release optimization
while generating full debug information to resolve the preceding sampling gap.
The fold runner now adds `--config profile.release.debug=2` only in `--sample`
mode, records that setting and archives the matching executable/PDB. It checks
source and archive stability. Ordinary timing/counting builds keep their existing
settings; the workspace Cargo profile and production code are unchanged. This
explicit user-authorized diagnosis setting supersedes the default-profile rule
only for these sampling builds. Cargo's [debug setting](https://doc.rust-lang.org/cargo/reference/profiles.html#debug)
controls debug information independently of the release optimization setting.

The earlier unresolved samples all had candidate symbol names; strict address
range checks could not confirm their ownership. The new run uses the same
sampler and the same system DbgHelp 10.0.19041.5848, with no tool installation or
relaxed range validation. Full symbols also expose readable private function
names. The archived PDB grew from 8491008 to 341569536 bytes. The full-symbol
dependency rebuild took 188.256 seconds, outside the three sequential ten-second
sampling windows. Machine/toolchain/features and warmups match the diagnostic
checkpoint; release optimizations remain enabled, and no throughput estimate is
computed from this build.

| Route | Earlier unresolved | Full-symbol samples | Full-symbol unresolved | Context-read errors |
| --- | ---: | ---: | ---: | ---: |
| Owning typed bridge | 78.06% | 3902 | 0.205% | 0 |
| Borrowed scalar bridge | 84.68% | 3886 | 0.103% | 0 |
| Candidate scalar fold, Rust reducer | 15.23% | 3900 | 0.000% | 0 |

The pipeline now reliably attributes over 99% of sampled addresses in all three
routes. These remain instruction-pointer shares, without inclusive call stacks,
source-line attribution or exact individual check timings. Inlining and merged
identical functions can select a representative monomorph's symbol; do not infer
that its displayed concrete type was actually passed by this workload.

The resolved typed recording includes allocation (RtlAllocateHeap 11.97%), memset
(6.10%), definition snapshots (5.10%), an execution-entry dependency predicate
(4.89%), mutable module-record lookup (4.79%) and module-member iteration (4.41%).
Borrowed scalar callback samples include module-member iteration (10.71%), the
same execution-entry dependency predicate (9.32%) and executable-installation
validation (4.30%). These exclusive symbol observations support investigating
the already-recorded preparation/admission ownership, but are not complete
inclusive cost estimates or attainable speedup predictions. Candidate scalar
sampling still points to invoke/region/physical resolution work, consistent with
its zero-allocation count. Existing normal-release timing and allocation evidence
remain independent and unchanged.

Reproduction uses the existing --sample command above. The fixture guide owns
build-mode separation. The archived hashes/source checks and focused normal-mode
fixture check pass; runner syntax, document links and diff checks pass. No Rust
implementation changed, so the previously passing Rust checks retain their
evidentiary scope. Full CI/backend/platform acceptance is still pending.

### Typed callback mapping implementation

Implementation checkpoint, 2026-11-04: implement the first bounded
step of the callback diagnosis, retaining typed Rust mapping proof through
preparation and invocation. This belongs to the existing orders 1-3 contract;
slot/frame transfer, unified execution admission and production collector cutover
remain at their recorded owners. No cancellation or Value ABI change is included.

- Function::prepare_call now returns an opaque PreparedCall<A, R>. Its private
  construction carries the checked Function mapping into the exact selected
  signature, including compatible current-code selection. PreparedFunction keeps
  its actual backend/queued-factory role; its untyped generic Rust invocation
  entry is removed. Rust rejects attempts to rebind a prepared call's result.
- PreparedArguments pairs one Rust argument-list type with a borrowed immutable
  script signature. Its public constructor checks the complete mapping before
  converters; internal typed calls reuse their existing Function proof. Standard
  tuples and retained-receiver packs consume that proof through one common
  conversion implementation, without repeating each top-level mapping check.
  Actual values, converter errors, limits, temporary protection and GC service
  remain in the conversion scope. Custom argument packs delegate to their own
  conversion by default and may explicitly consume the supplied proof.
- Proof stays on the typed call rather than a global mapping cache or an unsafe
  flag. Function constructors still validate mappings; frozen declaration/reload
  contracts transport them into current selections. No added persistent scope
  or executable owner pins obsolete named-function code. Custom mappings must
  describe immutable declaration correspondence; mutable admission stays in
  conversion/execution. Nested custom/composite conversion may still perform its
  own mapping work; this checkpoint does not claim every converter is prepared.
- The existing source-free conversion test now proves mapping reuse across
  repeated prepared calls, rejects deliberately invalid converter output before
  script effects, and preserves completed effects on return-conversion failure.
  Existing reload/generic/reentry/depth/GC tests and SDK call consumers are being
  checked. The former SDK attempt to re-ascribe a prepared result is rejected at
  binding/compile time; the same case now executes its correctly typed prepared
  call through the SDK context and retains its returned object.

The pre-change normal-release executable was archived before implementation.
The existing runner's --baseline comparison reruns old/new images sequentially
in alternating order, matching fixture/build/machine metadata and validating
each route/sample matrix. Capture before changing implementation, then compare:

```text
uv run python scripts/benchmark_managed_fold.py --diagnose --runs 1 --output target/managed-fold/proof-before
uv run python scripts/benchmark_managed_fold.py --diagnose --baseline target/managed-fold/proof-before --output target/managed-fold/proof-after
uv run python scripts/benchmark_managed_fold.py --counts --output target/managed-fold/proof-counts
```

The first command validates/archives the image; its single-process timing is not
the before estimate. The comparison reruns that image in five current pairs.
Settings match the diagnosis above: i7-10700F/Windows 10, rustc 1.99.1, release
defaults, bytecode interpreter, default features/target/Cargo parallelism, empty
recorded compiler/build overrides, 2048 items, three warmups and eleven samples
per route/process. Candidate compilation took 56.327 seconds, separately from
execution. All builds/counts/tests/profiling ran outside throughput windows.
Frequency, affinity and background load remain uncontrolled. Ranges are process
medians; paired change is the median of process-index-matched new/old ratios
minus one, not the ratio of the two aggregate medians.

| Route | Rerun old ns/item | New ns/item | New process-median range | Paired time change |
| --- | ---: | ---: | ---: | ---: |
| Owning typed Function callback | 7949.707 | 4625.684 | 4473.291-5067.773 | -43.23% |
| Same callback prepared once per fold | 7901.953 | 4477.832 | 4343.848-4943.652 | -43.86% |
| Candidate scalar fold, script callback | 8184.229 | 4794.385 | 4617.480-5190.430 | -42.36% |
| Candidate reference fold, script callback | 8230.029 | 4863.770 | 4697.070-5269.092 | -42.31% |
| Borrowed scalar callback control | 1883.838 | 1912.744 | 1808.252-2189.746 | -0.30% |
| Candidate scalar fold, Rust reducer control | 69.559 | 70.520 | 68.866-74.448 | +1.13% |

Old typed process medians ranged 7870.264-11109.326 ns/item; one process was
substantially slower across controls too. Small control changes are not wins or
regressions established by this run. All 1400 old/new fixture batches checked
results, candidate object/root counts and absence of unexpected collections. Existing
source-free GC/reentry tests independently cover collecting callbacks; these
diagnostic timing rows do not establish pressure/tail or complete backend gates.

Separate instrumented whole-fold counts use the existing diagnostics-feature
allocator/events, excluded from timing. The counting rebuild took 74.362 seconds.

| Route/probe | Old allocation requests | New allocation requests | Old requested bytes | New requested bytes |
| --- | ---: | ---: | ---: | ---: |
| Typed Function callback, 2048 items | 90458 | 8538 | 4965172 | 377652 |
| Candidate scalar/reference fold, script callback | 90456 | 8536 | 4964076 | 376556 |
| Borrowed scalar callback control | 4203 | 4203 | 215436 | 215436 |
| 2048 standalone u64 mapping checks | 16384 | 16384 | 917504 | 917504 |
| 2048 unprepared two-u64 argument encodings | 69698 | 36930 | 3916240 | 2081232 |

The typed delta is exactly 81920 allocation requests and 4587520 requested
bytes: 2048 callbacks times five removed mapping constructions, each eight
requests/448 bytes in this build. Whole-fold requests fall 90.56%; requested
bytes fall 92.39%. These are allocator requests/attributed requested bytes, not
peak live memory or RSS. Unprepared tuple conversion still checks both mappings
once, but no longer repeats them inside each already-admitted element encoder.
The standalone check's cost is unchanged; the proof model changes where it runs.
Driver/frame/window/argument-transfer event counts match the pre-change recording,
and candidate/Rust reducer folds still allocate nothing when warm. This confirms
the intended staging change without asserting identical instruction layout or
attributing all remaining CPU time to a particular owner.

Local evidence: 24 focused behavioral tests pass (10 stdlib conversion contracts;
12 VM cases for pre-effect rejection, conversion failure, generic/native
applications, compatible reload, retained closures, methods, GC/reentry and
depth/cancellation cleanup; one SDK prepared-call case; one async custom-pack
panic/retirement case). One compile-fail doctest rejects prepared-result
re-ascription. Workspace all-target Clippy, diagnostics-feature example Clippy,
structure, fmt, document links, runner syntax and diff checks pass. Previously
passing Rust checks retain their scope after documentation-only updates. Full
workspace/backend/platform tests remain CI-only and were not run. No carried
build/test failures remain.

This first mapping slice is complete. RootSet/Vec marshalling, conversion scope
ownership, repeated execution admission, complete prepared Native-entry adapters
and provider access coalescing remain as recorded above. There is no new
production slot-to-frame transfer or collector switch, and orders 1-3/GCR02 are
not declared complete by this checkpoint.

### Scoped argument-window implementation

Implementation checkpoint, 2026-11-04: replace synchronous typed
RootSet/Vec marshalling with a scoped argument/result window in the existing
ExecutionValues bank. Reuse its generational identities, metadata roots, capacity
reuse and out-of-order release/compaction. No second root mirror, new heap,
collector switch or unchecked scalar-only entry is selected.

Conversion publishes each argument before its next safepoint; the same window
receives the returned value before result conversion. Backend retirement and the
caller result-slot publication remain a closed transition with no GC/reentry;
this does not claim that the callee directly writes the caller slot before its
frame retires. Backend invocation carries
read-only source coordinates rather than a borrowed bank slice. No bank/payload
loan may cross converters, GC or script reentry. Ordinary tuple and receiver
conversion share one writer implementation. ArgumentWriter checks bounded,
once-only source-order publication; incomplete custom packs cannot accidentally
admit an initial Unit slot as a real Unit argument. Only short copies cross bank
access. The guard releases its generational window on error/unwind as well as
success. Conversion limits and outer/receiver scope nesting are preserved, and
call boundaries still admit terminal errors before unwinding. Actual
async/escaping ownership continues to materialize an owned pack at that boundary.
The contiguous primitive builtin ABI, enum payload construction and outer typed
host ingress may still materialize Vec values; owning Native-entry input
adaptation also remains. No claim of eliminating every adapter/allocation is made.

Affected owners are Runtime argument conversion, frame arguments/windows and
backend invocation, the VM invoker, existing queued-factory/enum/host consumers
and their focused tests. IntoKagariArguments now owns a canonical scoped-window
builder and requires write_prepared_arguments; custom adapters use the bounded
writer instead of constructing RootSet packs. ScriptInvoker receives the same
InvocationArguments source used by selected/closure/interface frame admission.
No old RootSet-returning argument implementation is retained. Mechanical callback
signature errors in VM allocation tests and Runtime scalar-target tests were
found by workspace Clippy and repaired under the same protocol. The previously
near-limit interface test file now puts its frame-selection/retention case in a
focused child module; no LOC exception was added.

The initial slot-only run removed most allocator requests but did not establish
a throughput improvement: typed callback time rose 2.09% and the script-only
closure loop rose 11.33% in five alternating pairs. Inspection identified two
remaining ownership/preparation problems. Internal iteration had acquired
Option-returning per-item source discovery despite its constructors already
establishing the complete range. It now retains the original bounded-indexing
iteration, while public single-value reads check their index. The contiguous
source only models this call window's actual zero-based argument prefix; no
unused arbitrary subrange protocol is retained. Separately, synchronous
ConversionContext construction was registering a fresh ProgramLease even though
PreparedFunction already owns the exact version throughout conversion and
reentry. A selection-bounded constructor now borrows that lease, sharing the
same private initializer/admission with in_native_call. Public independent
contexts still register their own lease. No lifecycle check is removed and
async/escaping retention is unchanged. The initial results motivated these
boundary corrections; they are not claimed as speedups before remeasurement.

The final source keeps one canonical bounded iterator for both bulk admission and
public checked nth reads; it does not introduce a second element lookup body or
force inlining. The remaining script-only cost is not explained by allocator or
admission/event counts. In particular, reducing heap requests does not eliminate
window publication, metadata/state admission or backend entry work. Do not infer
an exact CPU attribution from these counts or add another cache/inline workaround
without isolating the shared-path/code-generation effect.

The previous typed-proof executable is the archived paired baseline. Reproduction
uses the same runner, without editing Rust/build inputs during measurement:

```text
uv run python scripts/benchmark_managed_fold.py --diagnose --baseline target/managed-fold/proof-after --output target/managed-fold/slots-after
uv run python scripts/benchmark_managed_fold.py --counts --output target/managed-fold/slots-counts
```

Final normal-release comparison retains the i7-10700F/Windows 10, rustc 1.99.1,
bytecode interpreter, default features/target/build parallelism and empty build
overrides recorded above. Five alternating old/new process pairs each use 2048
items, three warmups and eleven samples per route. The final build took 47.322
seconds, outside execution timing. All 1400 old/new batches validated results
and fixture ownership/collection expectations. No test/build/profiling overlapped
throughput. Frequency, affinity and background load remain uncontrolled. The
table uses medians of process medians; changes use medians of paired ratios.
Ranges describe process medians and are not confidence intervals.

| Route | Rerun old ns/item | New ns/item | New process-median range | Paired time change |
| --- | ---: | ---: | ---: | ---: |
| Typed Function callback | 4407.373 | 3208.887 | 3191.455-3300.098 | -27.13% |
| Callback prepared once per fold | 4269.141 | 3091.064 | 3060.254-3162.305 | -27.65% |
| Candidate scalar fold, script callback | 4566.699 | 3314.209 | 3293.262-3388.867 | -27.20% |
| Candidate reference fold, script callback | 4584.180 | 3360.596 | 3317.627-3455.713 | -26.17% |
| Borrowed scalar callback control | 1787.207 | 1824.414 | 1807.178-1865.625 | +2.00% |
| Borrowed general-Value callback control | 3303.076 | 3338.818 | 3293.018-3445.117 | +1.08% |
| Script-only closure loop | 871.045 | 936.816 | 924.707-946.338 | +7.61% |
| Script-only addition loop | 14.648 | 15.137 | 14.648-16.162 | +4.73% |
| Candidate scalar fold, Rust reducer | 68.469 | 69.049 | 68.323-71.271 | +1.85% |
| Candidate reference fold, Rust reducer | 87.430 | 89.200 | 87.643-92.126 | +2.07% |

The typed improvement is measured for this script-closure callback workload,
not every Native function or the final new-collector ABI. At this checkpoint the
script-only closure loop remained slower; the preceding selection-borrowing
recording also observed +5.69%. That finding required shared-path diagnosis
rather than being hidden behind the typed win. The following argument-source
and closure-admission follow-up addresses this bounded regression; broader GC,
backend and performance acceptance is separate. No new per-item cache, unsafe
scalar route or trust flag is selected.

Separate diagnostics-feature allocation/event recording took a 45.452-second
build, excluded from timing. These are whole-2048-item-fold allocator requests
and requested bytes, not peak live memory, RSS or collection latency.

| Route/probe | Old allocation requests | New allocation requests | Old requested bytes | New requested bytes |
| --- | ---: | ---: | ---: | ---: |
| Typed Function callback | 8538 | 278 | 377652 | 15684 |
| Callback prepared once per fold | 8538 | 278 | 377748 | 15780 |
| Candidate scalar fold, script callback | 8536 | 278 | 376556 | 16588 |
| Candidate reference fold, script callback | 8536 | 277 | 376556 | 15588 |
| 2048 unprepared two-u64 argument encodings | 36930 | 32768 | 2081232 | 1835008 |

Typed whole-fold requests fall 96.74% and requested bytes fall 95.85%. After the
initial slot-only recording, selection borrowing removes exactly another 2048
requests/49152 bytes, matching one ProgramLease registration per callback. This
does not make the whole route allocation-free: other execution/retention work
remains. The standalone u64 mapping probe is unchanged at 16384 requests/917504
bytes for 2048 checks; unprepared packs still perform both required mappings.
Warm candidate folds with Rust reducers still request zero allocations.

Typed driver admissions (2049), call admissions/returns (2050) and argument
transfers (4097) remain unchanged. Recorded window allocations per fold grow
from 2050 to 4099 and cumulative initialized bank bytes from 55346 to 153666:
2048 three-Value callback windows
plus one outer result window replace independently allocated argument retention.
The bank-compaction and window-slots-examined counters are zero in this workload.
This is an explicit transfer of root ownership to reusable bank capacity, not
elimination of all rooting work or a claim about peak memory. The script-only
closure loop retains the same admission/window/transfer counts, with 35 -> 34
allocator requests; its remaining time regression therefore needs CPU-path/code
generation diagnosis rather than more allocation-count reductions. Pressure,
tail latency, new-collector integration and the complete backend matrix remain
unmeasured by these diagnostic rows.

Local correctness evidence covers 39 distinct focused cases: ten stdlib
conversion contracts, 22 VM callback/GC/reentry/reload/generic/interface/error
cases, one frame-selection/retention case, two Runtime selected/scalar-target
cases, one SDK prepared call and three async success/drop/panic cases. After the
final iterator-only correction, the directly affected managed argument/result,
interface GC/reload and reentry termination cases were rerun successfully; prior
unaffected cases retain their scope. Workspace all-target Clippy, the diagnostics
example Clippy, structure, fmt, document links and diff checks pass. Whole-workspace
tests and complete feature/backend/platform validation remain CI-only and unrun;
no build/test failure is carried. The shared-path regression has the follow-up
below. Orders 1-3/GCR02, broader admission-loop consolidation and complete
new-collector integration remain open.

### Shared argument-source performance investigation

Follow-up, 2026-11-04: isolate the recorded script-only closure
regression before changing more admission or transfer mechanisms. Current
counts do not explain the time delta. The bounded first experiment removes the
new Contiguous variant from the shared FrameArguments carrier and represents
CallSlots through the original mapped-window source. A temporary per-window
owned Location table preserves lifetime, bounds, owner/generation validation and
the same transfer loop; it is diagnostic only, not a proposed per-call allocation
or an accepted final implementation. Compare against the archived scoped-window
image in alternating pairs with the unchanged fixture/profile. Restore the
original sources before choosing the final model; keep conclusions here rather
than depending on local probe files. No trust flag or disabled validation is used.

The first five-pair experiment kept the closure loop at 937.744 ns/item versus
951.367 for the rerun scoped-window image (-2.50% paired); the borrowed scalar
control also moved -2.24%. Typed callback time rose 3.41% with the intentionally
allocating location-table adapter. The closure result remains close to the
recorded slow image, rather than the earlier approximately 871 ns/item baseline.
This does not identify the third carrier variant as the dominant cause. Both
temporary source files were restored byte-for-byte, including removal of the
per-call Location Vec. No generic source specialization or persistent location
cache is justified by this result.

The second bounded experiment isolates outer host ingress retention. Vm's typed
entry already materializes a contiguous Vec for Executor::new, but retains its
CallSlots argument/result window throughout execution even though its reserved
result slot is unused by that ingress. Install owning argument roots at that
actual contiguous-ABI boundary and release the scratch window before executor
entry. Inner Native callback windows and the common argument carrier/transfer
checks stay unchanged. This tests the effect of the extra live window independently
of the rejected carrier rewrite, without weakening managed argument retention.

That second experiment observed 957.861 versus 952.930 ns/item for the closure
loop (+0.21% paired), with borrowed scalar control -0.35%; it did not repair the
regression. The extra rooting/early-retirement code was removed and typed ingress
restored byte-for-byte. Neither probe is retained as production behavior.

The existing full-symbol sampler was then run for script_loop against the saved
pre-window full-symbol image and the current full-symbol image, with identical
fixture hashes. Both produced approximately 3890 samples with less than 0.16%
unresolved and zero context errors. Their exclusive hot-symbol distributions are
similar: frame allocation/admission, executable-installation checks, shared
execution state and memmove remain visible; they do not establish one new dominant
operation or inclusive call costs. These full-symbol images are diagnosis only;
normal-release paired timing remains the throughput evidence.

MSVC 14.51.36260 dumpbin with /DISASM:NOBYTES and PDB-resolved ranges showed a concrete
representation cost: the old FrameArguments count field is at offset 0x38 and
the new one at 0x40, consistent with 64 -> 72 bytes for this Rust build. The
ExecutionValues::allocate code grew 5022 -> 5342 bytes and its stack reservation
0x2c8 -> 0x308 (712 -> 776 bytes); push_exact_closure also reserves a larger
stack frame. These observations identify larger carrier/copy/code-generation
work, not an exact attribution of all 7.61% or a stable Rust layout guarantee.

Selected implementation: separate caller-owned source coordinates from the
by-value argument view. WindowArguments owns the FrameSlots key plus a mapped
prepared-location slice or dense-prefix description. FrameArguments borrows that
description, or a raw Value slice, while retaining its existing prefix and count.
Script calls create the source on their caller's stack; CallSlots owns its source
for the complete conversion/invocation scope. No bank/payload reference escapes,
no per-call Location Vec or persistent metadata cache is added, and no parameter
or owner/generation check moves or disappears. Keep the same validation/transfer
loop for all three physical sources. This implements the existing borrowed-view
ownership rule; it does not introduce another call executor, scalar-only path or
capability waiver. The public InvocationArguments wrapper now measures 40 bytes
with the current toolchain/default-feature library in the dev profile, versus
the previous compiled carrier's 72 bytes. This is a diagnostic observation, not
an ABI/layout assertion or a new private-structure regression test. Twelve
focused Runtime window/argument-precedence cases and eight VM managed
conversion/reentry/generic/interface/reload cases pass; workspace all-target
Clippy, structure, fmt and diff checks pass. Its performance comparison and the
remaining admission overlap are recorded below.

The first five-pair comparison of the smaller view against the recorded slow
window image measured 931.543 -> 904.102 ns/item for the script-only closure loop
(-2.40% paired), with borrowed scalar callback control -1.88%. Typed Function
callbacks measured 3267.432 -> 3208.740 ns/item (-1.80%). This preserves the
allocation-free source description and reduces its physical footprint, but
does not establish that carrier size alone explains the full original regression.

The same source review identifies an existing admission ownership overlap on
that path: select_call validates the exact selected owner, push_exact_closure
immediately validates it again, and shared frame admission validates it before
publication. No user converter, collection, suspension or callback occurs
between selection and the immediate closure-entry body. Immediate selection now
enters one private push_selected_closure implementation directly; the independently
queued exact-body entry still validates before using that same implementation.
Final common frame admission, environment/candidate/depth/state checks and actual
argument checks remain unchanged. This is reuse within the closed transition
already specified by the callback-proof design, not a persistent trusted token,
waiver flag or unchecked callback path. Named-call selection/transport remains
unchanged; a queued computation never reuses a validation across its wait.

Final comparison reruns the archived pre-window typed-proof image and the final
implementation in five alternating pairs. It uses the same i7-10700F/Windows 10,
rustc 1.99.1, default release/features/target/parallelism, 2048 items, three
warmups and eleven samples per route/process as above. The build took 47.555
seconds outside execution. All 1400 old/new batches passed fixture validation;
no build, test or profiling overlapped throughput. Background load, frequency
and affinity remain uncontrolled. Values below are medians of process medians;
changes use paired ratios and ranges are not confidence intervals.

```text
uv run python scripts/benchmark_managed_fold.py --diagnose --baseline target/managed-fold/proof-after --output target/managed-fold/call-admission-after
uv run python scripts/benchmark_managed_fold.py --counts --output target/managed-fold/call-admission-counts
```

| Route | Rerun pre-window ns/item | Final ns/item | Final process-median range | Paired time change |
| --- | ---: | ---: | ---: | ---: |
| Script-only closure loop | 886.182 | 841.553 | 828.564-863.281 | -6.06% |
| Typed Function callback | 4526.953 | 3248.291 | 3231.104-3288.525 | -28.27% |
| Callback prepared once per fold | 4379.688 | 3149.609 | 3129.932-3202.637 | -27.02% |
| Candidate scalar fold, script callback | 4651.611 | 3371.289 | 3348.193-3412.891 | -26.89% |
| Candidate reference fold, script callback | 4707.520 | 3402.539 | 3369.580-3418.311 | -27.72% |
| Borrowed scalar callback control | 1822.314 | 1773.633 | 1754.639-1783.105 | -3.71% |
| Borrowed general-Value callback control | 3326.416 | 3265.771 | 3247.412-3360.742 | -1.82% |
| Script-only addition loop | 15.186 | 16.113 | 15.234-16.797 | +1.62% |
| Candidate scalar fold, Rust reducer | 68.170 | 68.396 | 67.896-69.644 | -0.01% |
| Candidate reference fold, Rust reducer | 88.638 | 88.947 | 86.737-89.102 | +0.10% |

The previously recorded closure-loop regression is absent in this final matrix;
that route is now faster than the pre-window reference. Typed callback/fold gains
remain. This supports the smaller borrowed source view and reuse of immediate
selection admission, without claiming to assign every cycle of the earlier
7.61% to one mechanism. The two rejected probes were removed, not retained as
extra adapters or modes. No absolute universal call-time guarantee or final
GC/backend acceptance follows from this Windows workload.

Separate final diagnostics took a 51.658-second build, outside timing. All ten
whole-fold allocation/event rows and both mapping probes are exactly unchanged
from the scoped-window recording: typed callbacks still request 278 allocations/
15684 bytes per 2048-item fold, script-only closure loops 34/1876, and warm
candidate Rust-reducer folds zero. Call/window/transfer counters are unchanged.
The MetadataValidation counter excludes direct Program checks, so unchanged
events do not imply that repeated executable validation work stayed unchanged.
These are allocator requests and cumulative events, not peak memory or pause
latency. The borrowed view adds no per-call Location table or root snapshot.

Local validation passes 23 distinct selected cases: twelve Runtime identity,
growth/repeated-source, scalar/tag/initialization and failure-precedence cases;
nine VM conversion, generic/interface, GC/reentry, termination and reload cases;
and two async admission-panic/driver cases, including scoped cold-factory spawn.
After admission consolidation the directly affected VM reentry/retention cases
and both async cases were rerun; earlier unaffected boundary cases retain their
scope. Workspace all-target Clippy, structure, fmt, document links and diff checks
pass. No build/test failure is carried. Full workspace/feature/backend/platform
tests remain CI-only and unrun. This bounded regression follow-up is complete;
broader admission consolidation, complete prepared Native adapters, collector
cutover and GC performance gates retain their existing owners and remain open.

### Prepared Native-entry conversion and input slots

Implementation checkpoint, 2026-11-04: complete the shared binding/
application preparation slice before the physical collector cutover. Typed
ModuleBuilder::bind/declared bindings attach their Rust mapping preparation to
the existing LinkedNativeFunction signature cell. Closed entries validate during
linking before candidate publication; shared entries validate after their actual
application and selected operations are prepared. Failure must not publish a
partially admitted binding/application. No parallel signature/proof cache or
permission flag is selected.

Replace repeated whole-pack Vec/RootSet conversions on the typed entry path with
a complete frozen CallSlots input snapshot and a bounded ArgumentReader. Stable
tuple/receiver mappings consume the admitted signature; actual values, custom
conversion, receiver loans, limits, sticky failures and GC service remain checked.
Preserve full-pack retention and pre-conversion value checks, including suffix
checks after a receiver converter reenters. FromKagariArguments custom packs move
to one required prepared-reader body, sharing the unprepared wrapper rather than
keeping a second conversion implementation. Receiver/loan implementations use
that reader; no intermediate compilation failure remains.

The first scoped-host regression run rejected a legitimate ephemeral receiver
while publishing it into an ordinary Generic slot (ModuleValidation: argument
slot publication). The fix belongs to input preparation: the same signature cell
also owns one execution-only root-slot layout derived from the declared receiver
and reference positions. CallSlots borrows those prepared locations; its bank
window retains the layout Arc. This admits scoped input at the declared positions
without loosening ordinary value storage or adding another WindowArguments
source variant. Dynamic storage, scope, overlap and loan validation remain in
the existing receiver/access operations. The failed scoped-host case now passes.

Three source-free VM cases establish closed mapping preparation before execution,
reuse for actual scalar/nominal generic applications through forced GC, and
pre-publication rejection of a wrong Rust result mapping before body effects.
The first also deliberately returns the wrong actual value: invocation rejects
it after the callback's completed effect and a later valid call still succeeds.
These cases measure mapping-hook invocations, not private slot representation.

Focused local validation passes 24 distinct cases: four prepared-entry/scoped-host
cases; eight VM conversion, receiver/pair, host ingress, generic object and
collecting/reentrant fold cases; ten stdlib conversion cases; two async
admission-panic/direct-await cases, including scoped cold-factory spawn. This
selection exercises the changed input conversion and its direct lifetime/error
boundaries. Workspace all-target Clippy, structure (1813 Rust files, zero
violations), fmt and diff checks pass. No build/test failure is carried. The
complete workspace/backend/feature/platform matrix remains CI-only and unrun.

The paired normal-release regression matrix reruns the preceding call-admission
image against this checkpoint on the same Windows 10/i7-10700F, rustc 1.99.1,
default workspace release/features/target/parallelism. Each of five alternating
old/new pairs uses 2048 items, three warmups and eleven samples per route.
Compilation took 56.823 seconds outside execution. All 1400 old/new batches
passed fixture checks; no build, tests or profiler overlapped throughput.
Execution is warm; background activity, frequency and affinity are uncontrolled.
Reported estimates are medians of process medians, changes are paired ratios,
and ranges are not confidence intervals.

```text
uv run python scripts/benchmark_managed_fold.py --diagnose --baseline target/managed-fold/2026-11-04-call-admission-after --output target/managed-fold/2026-11-04-native-entry-after
uv run python scripts/benchmark_managed_fold.py --counts --output target/managed-fold/2026-11-04-native-entry-counts
```

| Route | Rerun preceding ns/item | Current ns/item | Current process-median range | Paired time change |
| --- | ---: | ---: | ---: | ---: |
| Typed Function callback | 3320.947 | 3214.844 | 3179.639-3289.697 | -2.64% |
| Callback prepared once per fold | 3191.553 | 3088.086 | 3079.834-3206.982 | -2.72% |
| Candidate scalar fold, script callback | 3457.080 | 3317.285 | 3251.416-3376.221 | -3.43% |
| Candidate reference fold, script callback | 3499.365 | 3437.402 | 3285.547-3501.367 | -1.71% |
| Script-only closure loop | 897.510 | 837.451 | 822.705-850.391 | -6.69% |
| Borrowed scalar callback control | 1805.811 | 1780.273 | 1753.223-1788.965 | -2.05% |
| Borrowed general-Value callback control | 3334.961 | 3375.977 | 3306.738-3429.150 | -0.74% |
| Script-only addition loop | 16.650 | 15.967 | 15.527-17.920 | -3.05% |
| Candidate scalar fold, Rust reducer | 68.140 | 68.549 | 67.868-70.682 | +0.36% |
| Candidate reference fold, Rust reducer | 88.388 | 88.379 | 88.098-89.914 | -0.33% |

This matrix shows no observed callback/closure regression in the selected
workload. The typed Native entry is crossed once per fold, not once per element;
the script-only loop does not cross it. These deltas therefore do not establish
a per-Native-call speedup or attribute all timing movement to preparation.

Separate diagnostics built in 64.685 seconds outside timing. Whole 2048-item fold
allocator requests/bytes fall from 278/15684 to 123/6780 for typed callbacks,
278/15780 to 123/6876 for callbacks prepared once per fold, 278/16588 to 122/6684
for candidate scalar folds and 277/15588 to 122/6684 for reference folds. This is
aggregate entry/owning-conversion/preparation work, not 155 allocations removed
from every element callback. Those routes add one scoped input window per fold
(4099 to 4100 allocations of execution windows, 32 additional initialized bank
bytes); argument transfers stay 4097 and no bank move is recorded. Script-only
closure/addition allocator and event counts are unchanged; candidate Rust-reducer
folds still request zero allocations. The borrowed scalar control changes by one
allocator growth request (4202/215380 to 4203/216380); the general-Value control
and both mapping probes are unchanged. These are allocator requests/cumulative
events, including capacity history, not peak memory, collection pauses or universal
per-call costs. The measured entry path removes repeated mapping and whole-pack
RootSet/Vec construction, while actual escaping handles still retain their roots.

This reuses production binding/application owners and the scoped execution bank.
The candidate managed heap still cannot exchange object references through the
production Value ABI. Do not add cross-heap IDs/roots or claim full nominal
ScopedEntry binding from this preparation slice: the physical scoped-family
adapter and complete generic/reference transport remain orders 1-3 closure work.
The existing Wasmtime preparation and Lua shared-frame references apply to these
ownership boundaries; no new runtime, collector mode or broad stdlib migration
is introduced. Validate concrete nominal/generic native entries, mapping rejection
before effects, collecting/reentrant conversion, scoped loans and return/error
cleanup. Reuse the current paired fold fixture for regression evidence and keep
the full workspace/backend matrix in CI.

### Complete scalar transport in the managed Value family

Implementation checkpoint, 2026-11-04: production Value replacement
cannot precede the ordered nominal/application and complete-retention proofs.
Complete the candidate family's concrete scalar transport first, replacing the
u64/Unit-only restriction with Bool, Char, widened I32/I64/U64 and F32/F64
carriers. Narrow Rust integer adapters use the existing widened execution
convention and checked narrowing; do not add arithmetic, casts or a second
language numeric policy here. Never and String are not immediate scalar values.

Keep the persistent/scoped tag inventory flat in Repr. Stored/scoped/rooted
transitions copy scalar fields directly; ScalarValue is only a temporary codec
boundary carrier and is never nested in a stored/scoped value. Reference
transitions remain separate, each retaining its existing liveness/publication
proof; no generic map-reference conversion is introduced. Extend the existing
sealed Kind adapters and scalar declaration preparation, not the production
dispatcher or collector mode. Validate widths, Unicode, NaN/signed-zero bits,
mixed Native arguments/results, vector field storage, root owner rejection and
cleanup through existing owners. Full nominal binding, managed metadata and
complete root/retention proofs remain prerequisites for the coherent cutover.
This is routine completion of the already selected scoped/stored/rooted model;
the existing representation/reference-category research still applies.

The shared carrier lives at value::scalar::ScalarValue; it contains only Unit,
Bool, Char, I32, I64, U64, F32 and F64. NativeScalar's existing public interface
adapts production Value at ingress/egress and delegates to the same internal
ScalarCodec as managed Kind. The original codec owns carrier checks, exact narrow
integer ranges and machine-size errors. No managed reference can pass through
this scalar adapter. Fixed-width Kind supports (), bool, char, i8/i16/i32/i64,
u8/u16/u32/u64 and f32/f64. isize/usize retain their production host conversion
contract; they are not additional language kinds and are not admitted as managed
Kind in this checkpoint. Present/Taken and physical object admission are unchanged.

The initial Runtime test build failed because the fixture assumed Type::i16 and
Type::f32 conveniences; it executed no test cases. It now uses Type::scalar with
the corresponding BuiltinType. An initial attempt to place private-carrier codec
methods on the public sealed supertrait emitted private-interface warnings; the
codec instead belongs to one crate-private trait, preserving NativeScalar's
public surface. Both intermediate issues are resolved in this checkpoint.

The first executable Runtime run passed raw scalar admission but two entry/root
cases failed with MetadataConflict (missing native implementation binding): the
test's authored module had declarations without implementations. The fixtures now
provide matching real scalar bindings; both failed cases pass. These are fixture
repairs, not relaxed preparation or publication validation.

Local validation passes 19 distinct selected cases: three Runtime scalar-domain,
entry/root and mixed-signature cases; nine managed stdlib provider/entry/reentry/
failure cases plus the existing production scalar-width conversion case; six VM
prepared-entry, machine-range, composite and collecting/reentrant callback cases.
The entry/root case covers all 13 fixed scalar kinds, integer endpoints, Unicode
boundaries, exact NaN/infinity/subnormal/signed-zero bits, foreign-owner rejection
and scope cleanup. The stored-value case exercises all kinds through the real
vector provider with zero host roots. Existing collection cursor repair,
capacity charging, Present/Taken, removal effects and unwind assertions remain.
Workspace all-target Clippy, structure (1814 Rust files, zero violations), fmt and
diff checks pass. The two failed Runtime cases were rerun after their fixture
repair; unaffected passing cases retain their evidence. Full workspace tests and
the complete feature/backend/platform matrix remain CI-only and unrun. No
build/test failure is carried. Throughput follow-up is recorded below.

The first five-pair normal-release comparison against the preceding Native-entry
image exposed a regression: candidate Rust-reducer scalar folds measured
68.631 -> 95.734 ns/item (+40.59% paired), and reference folds
89.990 -> 114.236 (+27.25%). Script callback folds did not regress, so their
larger callback cost concealed this smaller inner-loop loss. This result failed
the selected performance gate; it is not accepted as the completed implementation.

Architectural diagnosis: the new reuse boundary carried NativeScalar's complete
RuntimeError result into repeated physical Kind admission. That error includes
host execution diagnostics/policy which the storage codec does not own. The
current rustc 1.99.1/MSVC build reports Result<u64, RuntimeError> at 72 bytes,
versus 24 bytes for Result<u64, GcError> and 16 for a diagnostic Result<u64, u8>
comparison. StoredValue and scoped Value both remain 16 bytes, as does a diagnostic
copy of the preceding bounded Repr; Edge is eight. These measurements reject
the initial storage-width hypothesis and identify a concrete result-transport
cost, without attributing every measured nanosecond from sizes alone.

ScalarCodec now returns a small private ScalarError describing representation or
signed/unsigned host range failure. The production NativeScalar boundary alone
materializes the original RuntimeError kind/message; managed fixed-width Kind
admission converts physical representation failures to GcError. Neither success
nor failure needs a rich host result through the physical codec. No forced
inlining, cache, trust flag, duplicate numeric semantics or waived check is added.
This follows the standard Rust scalar conversion distinction between a compact
conversion failure and its formatted diagnostic; the applicable primary reference
is [TryFromIntError](https://doc.rust-lang.org/std/num/struct.TryFromIntError.html).
The [Rust Reference layout contract](https://doc.rust-lang.org/reference/type-layout.html#the-rust-representation)
(consulted 2026-11-04) leaves these Rust layouts unspecified; the diagnostic sizes
are current-build observations, never ABI promises or private-layout assertions.
The revised codec keeps Kagari's distinct carrier/range errors and user-visible
host policy instead of copying an incompatible runtime error model. Correctness
reruns cover raw/narrow/bit-exact transport, stored vectors, machine-size errors
and collecting callback execution; a fresh paired matrix must pass before this
follow-up is accepted.

The small-error revision reduced the loss but did not close the gate: another
five-pair run measured 72.238 -> 76.172 ns/item (+7.99%) for scalar Rust folds and
93.277 -> 102.676 (+9.76%) for references. No callback-fold regression was seen.
Review the remaining representation normalization rather than declaring that
the rich error explains the entire original loss. A bounded flat-Repr experiment
removed the nested ScalarValue tag and used scalar-only extraction/construction;
reference transitions retained their separate liveness/publication paths. Its
five-pair recording was noisier (scalar process medians 83.536-113.202 ns/item,
reference 106.973-170.889) and still failed the gate (+21.09%/+8.08% paired).
This does not establish a flattening benefit. The flat inventory and extra
normalization helpers were removed, restoring the shared ScalarValue carrier.

A small independent downstream code-generation probe gives concrete boundary
evidence: lower a known u64 through Kind and immediately admit it using a
CallRegion's scope. The normal release library's optimized consumer still calls
the external u64 Kind::admit and reserves a 104-byte stack frame, even though that
known scalar round-trip needs neither scope access nor a failing conversion.
The thin, concrete trait adapter no longer exposed sufficient optimization
information across the Runtime/provider crate boundary. Do not replace it with
a u64-only execution path. ScalarCodec's primitive encode/decode and all 13
scalar Kind lower/admit adapters now use ordinary #[inline] hints, making their
bodies available for cross-crate optimization. No inline(always), removed check,
trust token or alternate numeric semantics is introduced. The reference mechanism
is the [Rust core conversion implementation](https://doc.rust-lang.org/src/core/convert/num.rs.html)
(std 1.99.1, consulted 2026-11-04), whose primitive conversions likewise expose
their bodies for inlining; Kagari uses ordinary hints rather than copying its
forced annotations. A fresh probe must show that the known-value adapter can
disappear, while raw-value/narrowing tests retain rejection. Throughput still
requires an independently paired measurement; the probe is not a speed claim.

The downstream known-value probe then reduced to movq/retq, confirming elimination
of that adapter boundary. However, its five-pair matrix still regressed for
actual Rust-reducer folds: 69.556 -> 80.499 ns/item (+11.47%) and
91.861 -> 104.587 (+12.83%). Known-value construction is insufficient evidence
for loading an unknown tag from erased storage. An actual CallRegion::read(Local<u64>)
consumer probe shows the remaining work: the nested model first classifies the
outer scalar category using subtract/cmov/indirect jump, then compares the inner
u64 tag. Its emitted function has 77 assembly lines with the current toolchain.

The final representation selection is flat Repr with direct scalar-field copies
at every slot, stored-field and host-root transition. Unlike the rejected flat
experiment, those transport operations do not call scalar normalization helpers;
only Kind's Rust conversion boundary projects to/from ScalarValue. This restores
the intended responsibility split: storage transports tags/bits and handles
reference liveness; the shared codec performs Rust conversion; Native owns rich
errors and signature preparation. No per-copy scalar-category conversion is
required. The revised actual-slot probe has 68 assembly lines and no outer
category normalization or extra u64 comparison after dispatch. Owner identity,
bounds, initialization and object-source state checks remain. These counts are
code-generation diagnostics, not a private-layout test or an ABI/speed guarantee.
The known-value probe still compiles to movq/retq. Fresh field/root/GC tests and
paired throughput must confirm the selected model below.

To reproduce the diagnostic independently, build the normal release library and
compile a small downstream Rust library with --edition=2024 -C opt-level=3
--crate-type=lib --emit=asm, -L dependency=target/release/deps and --extern
kagari_runtime=<matching-release-rlib>. Inspect these two public function bodies
(imports shown; no benchmark-specific implementation is needed):

```rust
use kagari_runtime::native::{
    binding::NativeResult,
    managed::{call::CallRegion, slots::{Kind, Local}},
};

pub fn known(region: &CallRegion<'_, '_>, value: u64) -> u64 {
    u64::admit(region.scope(), u64::lower(value)).unwrap()
}

pub fn loaded(region: &mut CallRegion<'_, '_>, local: Local<'_, u64>) -> NativeResult<u64> {
    region.read(local)
}
```

Final local correctness checks pass. After the selected direct-transport change,
rerun the three Runtime scalar/entry/root cases, all nine managed stdlib
provider/entry/error cases and the real VM collecting/reentrant fold (13 cases).
Earlier production machine-range/conversion and prepared-entry results retain
their scope: the flat Repr change affects only the candidate family. Workspace
all-target Clippy, structure, fmt and diff checks pass; no compilation/test failure
is carried. Full CI/platform/backend tests remain unrun.

Two final five-pair matrices compare the unchanged archived pre-expansion
Native-entry image and the selected implementation. Both use Windows 10,
i7-10700F, rustc 1.99.1, default workspace release/features/target/parallelism,
2048 items, three warmups and eleven samples per route/process. The first build
takes 65.188 seconds outside execution; the confirming cached build takes 0.304.
Each matrix passes all 1400 old/new fixture batches. No build, test or profiler
overlaps timed execution. Execution is warm; background load, frequency and
affinity are uncontrolled. Estimates below are medians of process medians;
changes are median paired ratios and ranges are not confidence intervals.

```text
uv run python scripts/benchmark_managed_fold.py --diagnose --baseline target/managed-fold/2026-11-04-native-entry-after --output target/managed-fold/2026-11-04-scalar-transport-after
uv run python scripts/benchmark_managed_fold.py --diagnose --baseline target/managed-fold/2026-11-04-native-entry-after --output target/managed-fold/2026-11-04-scalar-transport-confirm
uv run python scripts/benchmark_managed_fold.py --counts --output target/managed-fold/2026-11-04-scalar-transport-counts
```

| Route | Confirming rerun baseline ns/item | Confirming current ns/item | Current process-median range | First paired change | Confirming paired change |
| --- | ---: | ---: | ---: | ---: | ---: |
| Candidate scalar fold, Rust reducer | 72.495 | 64.764 | 61.490-67.728 | -11.01% | -11.84% |
| Candidate reference fold, Rust reducer | 94.424 | 98.932 | 91.669-104.178 | +2.70% | +2.27% |
| Typed Function callback | 3397.852 | 3481.934 | 3252.539-3656.348 | +2.41% | -0.05% |
| Callback prepared once per fold | 3229.443 | 3318.994 | 3160.205-3504.395 | -0.55% | -2.00% |
| Borrowed scalar callback control | 1862.207 | 1904.199 | 1817.871-1983.252 | +2.28% | +0.15% |
| Borrowed general-Value callback control | 3482.715 | 3578.613 | 3359.717-3682.471 | +5.25% | -3.53% |
| Script-only closure loop | 865.869 | 903.271 | 862.793-972.852 | -1.08% | +4.73% |
| Script-only addition loop | 18.701 | 18.164 | 17.480-19.873 | -7.24% | -6.53% |
| Candidate scalar fold, script callback | 3493.164 | 3563.916 | 3413.574-3696.924 | +2.97% | +0.86% |
| Candidate reference fold, script callback | 3465.283 | 3582.666 | 3415.625-3677.100 | +2.05% | +0.96% |

The large scalar expansion regression is removed; scalar Rust traversal improves
in both final paired recordings. Reference traversal retains a measured +2.27%
to +2.70% delta. Its cause is not resolved by these data; do not call it zero,
discard it as noise or add more local inline overrides without renewed reference
path diagnosis. The other controls also vary between recordings, so these results
do not establish a universal no-regression claim. Scalar functionality and the
large-loss repair are complete; small reference-path performance acceptance
remains open with the existing GCR02/GCR04 reference/frame investigation, before
production cutover acceptance. Full integrated GC/pressure/latency acceptance and
the nominal/generic object ABI remain pending at their existing owners.

Separate final diagnostics build in 59.882 seconds outside timing and reproduce
all ten allocation/event rows and both
mapping probes exactly from the preceding Native-entry checkpoint. Warm candidate
Rust scalar/reference folds still request zero allocations; typed callbacks
remain 123 requests/6780 requested bytes per 2048-item fold and candidate script
folds remain 122/6684. Call/window/transfer counters and initialized bank bytes
are unchanged. These counts do not measure peak memory or GC latency. The scalar
expansion introduces no new per-element roots or allocator requests in this
fixture; it does not settle the remaining reference time delta.

### Bounded public-operation Lua baseline (2026-11-04)

The existing [Lua comparison owner](../benchmarks/lua-comparison/README.md)
now provides `--stdlib-ops`: eight workloads using the **current production**
Runtime/stdlib, not the candidate managed heap or proposed unified host API.
This closes workload selection/freezing for the small-operation proof; it does
not close GCR02 production binding, reference regression or GCR04 acceptance.
Use the same scripts after the owning integration change; do not replace the
public operations with fixture-only fast paths or Rust kernels.

Measured environment: Windows 10 build 19045, i7-10700F, 16 logical CPUs,
64 GiB RAM; rustc 1.99.1 (`58cf8f9c3`, LLVM 23.1.1), x86_64 MSVC;
workspace release defaults, default Cargo target/parallelism; Kagari
`source,native`, interpreter execution only; mlua 0.12.2 embedding vendored
PUC Lua 5.4.9, no LuaJIT. Source state was `ccb63194` plus the uncommitted
Runtime/Native work and this benchmark addition, not clean HEAD performance.
The final invocation reused compiled artifacts (0.299 s build wall time,
excluded). Processes ran sequentially without builds/checks/profiling alongside
timing: seven fresh processes, three warmups and eleven measured batches per
route, alternating engine order per batch and workload order per process.

Both engines disable automatic collection and perform an explicit full collection
before each batch outside timing. Inputs are created/retained before timing;
each batch includes one typed host entry and returns an integer. The Kagari
input is ScriptValue passed by identity, not imported from an owned Rust Vec.
Function lookup/binding is untimed. Integer values share the signed i64 domain
and stay within range. Result checksums and mutation length/weighted contents
are verified outside timing. The raw execute API rejects argument passing;
the fixture uses existing bound typed Function calls instead. No production
entry contract was changed to make the fixture run.

The table reports the median of seven process medians divided by iteration count.
The ratio column is the median of the seven within-process Kagari/Lua ratios;
the range is the observed minimum/maximum process ratio, not a confidence interval.
One mutation pair is one iteration; fold consumes one item per iteration.

| Operation | Iterations/batch | Kagari ns/iteration | Lua ns/iteration | Kagari/Lua time (process range) |
| --- | ---: | ---: | ---: | ---: |
| loop control | 5000 | 11.22 | 10.08 | 1.11 (1.06-1.19) |
| zero-argument Native, same Rust helper | 5000 | 1456.32 | 32.68 | 44.69 (43.63-45.25) |
| Vec.len / dense table length | 5000 | 1327.86 | 15.32 | 86.67 (84.68-93.31) |
| indexed Vec/table read | 5000 | 1432.54 | 24.86 | 57.82 (57.45-58.38) |
| indexed write then read | 5000 | 3252.82 | 38.24 | 84.10 (78.31-88.11) |
| pop then push, no capacity growth | 5000 | 4727.84 | 90.70 | 51.81 (44.99-54.98) |
| public scalar fold / Lua reducer loop | 2048 | 14888.33 | 32.81 | 453.74 (399.87-527.68) |
| public object fold / Lua reducer loop | 2048 | 18873.00 | 40.19 | 469.65 (451.26-535.46) |

All rows include loop control, dispatch and arithmetic; these are not isolated
call/access latencies. The first six use 64-element containers. Kagari indices
include checked i64-to-u64 conversion; Lua uses one-based integer indices.
The Native control uses the same Rust helper through Kagari typed Native and
mlua callback adapters, so it includes each embedding adapter's real contract.
Pop includes the current allocated Some and match, while Lua uses
table.remove/table.insert. Kagari's public Iterable.fold includes cursor.next,
Option, erased callback and root machinery; Lua uses direct table access with
one script reducer call per item. Both create their reducer inside the timed
entry. These are comparisons of useful public operations, not equal low-level
instruction sequences or proof that an Option allocation explains the gap.

Architectural reading: loop control alone is close, while a zero-work Native
body and tiny accessors already show large fixed-path cost. Public fold adds
the iterator/callback chain on every element. This supports prioritizing shared
Native admission/transport and iteration/callback ownership before container
algorithm tweaks. It does not identify the exact split among admission, roots,
conversion, dispatch, cancellation polling and allocation. Inspect actual
call/allocation counts at those existing owners before changing them; preserve
the prepared-fact and dynamic-borrow invariants above. Do not subtract the loop
control time and label the result an exact Native cost. The current candidate
managed-fold measurements remain separate and cannot establish integrated
stdlib speed. GC pressure/latency, cold entry, broader stdlib workloads and the
unchanged-source post-integration comparison remain open under GCR02/GCR04.

Reproduce with `uv run python scripts/benchmark_lua.py --stdlib-ops --runs 7`
and optionally `--freeze-baseline NEW_IMAGE_PATH`. The selected local checkpoint
is `target/lua-comparison/20261104T102624Z-stdlib`; its frozen executable/report
are under `target/lua-comparison/2026-11-04-stdlib-baseline-clean`.
An earlier exploratory run overlapped the structural check and is excluded.
Selected image SHA-256:
`aef072c83c0f5940e21b9ca2c962963838d5350c3db35912309d1060bc5b8cbf`;
workload SHA-256:
`da2230be77522f413e05e77993be6285a629238d2d60a950d553a51772cc58db`.
These ignored artifacts are reproduction aids; the method, results and limits
above remain available without them.

Local validation: all eight cross-engine checksum routes and both mutation
postconditions pass in the focused check and all seven ordinary processes;
the existing benchmark-evidence suite passes 15 tests. Workspace Clippy with
all targets and denied warnings, fmt, structural checking (1815 files, zero
violations) and diff checks pass. Exhaustive workspace/backend/platform tests
remain CI-only and unrun. No GC/Native implementation optimization is included
in this measurement checkpoint.

### Small Native and Vec access diagnosis (2026-11-04)

This is the first bounded follow-up to the public-operation baseline: locate
fixed costs in `native_zero`, `vec_len` and `vec_get` before choosing an entry
model. `vec_get` means indexed `v[i]`, not Option-returning `v.get(i)`. No
optimization, cancellation-policy change or production Value/GC cutover is
included. The eight ordinary workload sources and useful work are unchanged.

The existing benchmark now accepts `--stdlib-ops --diagnostics` for these three
rows and loop_control. The opt-in Runtime counters include execution polls/state
guards, module checks, Native entry/argument reads, conversion checks, root-pack
registration attempts and sequence access. They are compiled out of ordinary
builds. See the [benchmark guide](../benchmarks/lua-comparison/README.md) for
separate counting and full-symbol sampling commands.

Counting uses the preceding machine/toolchain/release settings with the benchmark
diagnostics feature. Each row has 5000 iterations, retained input and explicit
collection outside counting. It includes one outer typed SDK call. A first batch,
two intervening warmups and two recorded warm batches produced identical warmed
counts. Subtracting loop_control's fixed protocol counts and dividing by 5000
gives the per-operation entries below. This is count attribution, not subtraction
of timings or an estimate of individual checks' CPU cost. Several counters are
nested: a GC service point itself calls execution polling, for example.

| Additional entries per operation | Empty typed Native | Vec.len | Indexed Vec read |
| --- | ---: | ---: | ---: |
| Current-target selection / frame admission / return | 1 / 1 / 1 | 1 / 1 / 1 | 1 / 1 / 1 |
| Logical execution windows | 2 | 1 | 1 |
| Argument transfers into the callee | 0 | 1 | 2 |
| Execution polls | 15 | 9 | 9 |
| Execution-state guard entries | 20 | 14 | 14 |
| Executable-module validation entries | 4 | 2 | 2 |
| GC service points (actual collections: zero) | 3 | 2 | 2 |
| Conversion contexts | 1 | 0 | 0 |
| Rust conversion mapping checks | 0 | 0 | 0 |
| Scoped type-argument validation | 1 | 0 | 0 |
| Value/type matcher entries | 2 | 2 | 2 |
| Native parameter-role scans | 0 | 1 | 2 |
| Native argument reads | 0 | 1 | 2 |
| Sequence-family/contract checks | 0 | 2 | 2 |
| Actual sequence payload reads | 0 | 1 | 1 |
| Independent root-pack registrations | 0 | 0 | 0 |
| Allocator requests / script objects | 0 / 0 | 0 / 0 | 0 / 0 |

Every warmed complete row, including loop_control, requests 35 allocations / 2256
bytes and registers one independent root pack at the outer SDK boundary; these
do not scale with the 5000 inner operations. Method/interface/shared/Native
application preparation, layout preparation/comparison, environment creation,
graph metadata validation and call-descriptor creation counters all remain zero
inside these batches. Window creation is logical storage lifecycle in a reused
bank, not a fresh heap allocation. The empty typed call initializes a 9-byte
scalar result window plus a 16-byte general conversion-result window; len uses
one 25-byte callee window and indexed read one 34-byte window. Those are physical
bank payload/initialization bytes, excluding window descriptors and Rust stack
temporaries. No bank relocation occurs. The initially cold indexed-read row has
one additional 64-byte allocator request; the recorded warm rows agree exactly.

The actual paths explain those counts:

- [Call lowering](../crates/kagari-runtime/src/module/execution/calls.rs) routes
  Native imports through ordinary current-target selection and exact admission,
  rather than the former retained Native fast path (removed at the activation
  checkpoint above). That path was not used by these scripts. This was an ownership correction for
  reload: a caller must not silently keep using an obsolete callback. Reusing
  that old retained callback would lose the current-selection guarantee.
- [Prepared calls](../crates/kagari-runtime/src/frame/calls.rs),
  [frame admission](../crates/kagari-runtime/src/frame/admission.rs), the
  [VM driver](../crates/kagari-vm/src/executor/driver.rs),
  [Native entry](../crates/kagari-runtime/src/frame/native.rs) and
  [return publication](../crates/kagari-runtime/src/frame/returns.rs) each own
  parts of the synchronous call lifecycle. Every tiny body traverses that full
  path; repeated polls, session/state admission and owner validation occur at
  its internal transitions. An ordinary result remains rooted in its Native
  frame across slicing, and return cleanup/publication must retain that guarantee.
- [Typed conversion](../crates/kagari-runtime/src/native/typed.rs) correctly
  reuses prepared Rust mappings, but still builds a general ConversionContext
  and an additional CallSlots snapshot/result window. Even the empty-argument
  scalar callback enters conversion depth/node/poll/GC-service machinery and
  checks the actual result, followed by the linked binding's result check.
  The callee already owns frozen arguments and a rooted return slot. Prove
  their lifetime and converter-effect ordering before unifying the storage;
  caller operand locations alone do not establish that proof.
- [Sequence admission](../crates/kagari-runtime/src/native/context/operations.rs)
  checks nominal type and sequence family. Then
  [sequence_len/sequence_element](../crates/kagari-runtime/src/gc/sequences.rs)
  repeats family/contract admission before the
  [buffer access](../crates/kagari-runtime/src/gc/buffers.rs) checks payload
  availability and reads it. No callback or safepoint separates these short
  stages. The unchecked lifetime of a long-lived facade would still be invalid:
  extraction, aliases, active borrows, detachment and index bounds remain dynamic.

Full-symbol sampling ran sequentially after builds/checks completed, using the
same machine/toolchain and ordinary feature set, with `profile.release.debug=2`
and unchanged release optimization. That rebuild took 304.323 s outside sampling.
Each route warmed three times, collected once before the window, then repeated
the existing typed batch for ten seconds with no observer/counters or automatic
GC. Every return was checked; script-object allocation and collection deltas
were zero. The existing Windows main-thread RIP sampler used jittered 1.5-2.5 ms
intervals and strict PDB symbol/unwind-range resolution, without inclusive stacks.

| Route | Samples | Unresolved | Context errors |
| --- | ---: | ---: | ---: |
| Empty typed Native | 3819 | 0% | 0 |
| Vec.len | 3808 | 0% | 0 |
| Indexed Vec read | 3821 | 0.079% | 0 |

Representative exclusive symbol shares corroborate the shared lifecycle path:

| Sampled symbol | Empty typed Native | Vec.len | Indexed Vec read |
| --- | ---: | ---: | ---: |
| Sip13Rounds::write | 6.15% | 5.25% | 4.61% |
| ResourceState::termination | 4.56% | 3.89% | 2.83% |
| ModuleStore::validate_executable_installation | 4.43% | 2.63% | 2.12% |
| ExecutionStack::validate_top | 3.53% | 3.76% | 3.53% |
| ExecutionValues::allocate | 2.64% | 2.91% | 3.35% |
| memmove | 3.04% | 3.99% | 4.21% |

Target selection, Native entry, admission closures and return transitions also
appear throughout the recordings. There is no single dominant container kernel.
These samples do not provide inclusive subsystem shares or prove every observed
hash belongs to module lookup. Source inspection independently shows installation
validation performing repeated installation-map lookups and a current-publication
lock; module-record lookup also checks the staged-program map. Session lookup is
a generational slot lookup, not a hash map. Preserve those distinctions before
proposing a different hasher or a cache. Likewise, memmove samples do not identify
the copied Rust structure; bank relocation counters are zero. Inlined/merged
monomorphs and out-of-line helpers limit attribution, so these percentages are
not estimates of the speedup from deleting a check.

The next ownership proof remains in existing orders 1-3: one selected Native
activation owns its current target, depth/trace/cleanup state and protected
argument/result storage; common NativeContext and standard-library views consume
that established authority. Prepare immutable parameter-role and conversion facts
with the binding/application; retain actual-value validation for arbitrary Rust
converters, runtime ownership, borrow/state checks and result publication at their
real boundaries. A short admitted sequence access should combine the repeated
family/contract/payload access under one valid borrow. Standard-library authors
must not author separate unchecked variants. Stable preparation, existing bank
storage and current-code selection are sound foundations to retain. Any replaced
legacy fast path or duplicate window protocol must have a removal checkpoint.

Execution polling removal remains the separately reviewable GCR03 policy change.
Do not treat all execution-state guards or GC service as cancellation checks, or
remove required post-callback sticky-failure/borrow/publication gates. This diagnosis
does not yet establish how much faster the ownership change will be; the frozen
ordinary workloads and the full retention proof remain acceptance requirements.
The fixed 35-allocation outer typed host call remains relevant to host embedding;
amortizing it across these loops does not establish that grouped host access is
cheap. Keep it visible in the unified host/Native proof without adding another
public exchange model or extending this diagnostic into an unrelated SDK rewrite.

Mature implementation cross-check, reviewed 2026-11-04:
[Lua 5.4.9 ldo.c](https://www.lua.org/source/5.4/ldo.c.html), `prepCallInfo`,
`precallC` and `luaD_poscall`, reuses CallInfo records and the actual argument/result
stack while preserving Native call/return bookkeeping and error/hook contracts.
It does not eliminate the activation or prove Kagari's static nominal types,
Rust loans or current-code reload semantics. Wasmtime 51.0.0-dev's
[typed.rs](https://docs.wasmtime.dev/api/src/wasmtime/runtime/func/typed.rs.html),
`TypedFunc::call_raw` and `WasmParams::store`, carries prepared type evidence,
uses one parameter/result storage region and keeps store ownership and concrete
reference checks distinct from scalar transport. Its fixed Wasm ABI/unsafe trait
contract cannot authorize trusting arbitrary Kagari Rust converters. These are
ownership references, not cross-runtime speed predictions.

Reproduction/evidence: counts are archived locally under
`target/lua-comparison/2026-11-04-native-costs` (12 checked rows, source hashes,
machine/toolchain metadata and instrumented executable). Count-image SHA-256 is
`b8d598dd9bb7a6b3a636eb4a362de1accdf9ed5770f8ce7df386eacd86716455`.
Sampling is under `target/lua-comparison/profile/windows-20261104T104853Z`;
ordinary sampling image SHA-256 is
`68d8df1fe516b643262224f612f149ad68efe2252aa7e0ea436b68b45240fc21`
and matching PDB SHA-256 is
`07c4b36276f6a4d3d961553449288d656942fd209a5e8c6932b3fbba2a17f097`.
Image/PDB and benchmark source hashes were checked after all sampling completed.
The method, counts, sampled evidence and interpretation above do not depend on
preserving these ignored local artifacts.

Adding diagnostic modes changes the harness digest even though the timed scripts
are unchanged. A fresh ordinary, counters-disabled seven-process baseline was
therefore frozen **after** sampling and all builds/checks, using the original
three-warmup/eleven-sample policy. This is the baseline for the next implementation
checkpoint; the earlier archive remains historical evidence. Normal release
rebuild time was 54.364 s, excluded. All eight checksums and both mutation
postconditions pass in every process. Median batch times in microseconds and
median within-process time ratios are:

| Workload | Kagari batch us | Lua batch us | Kagari/Lua |
| --- | ---: | ---: | ---: |
| loop_control | 53.7 | 50.3 | 1.07 |
| native_zero | 7447.3 | 162.6 | 45.62 |
| vec_len | 6399.1 | 76.5 | 83.00 |
| vec_get | 6686.1 | 125.3 | 53.28 |
| vec_set_get | 15090.7 | 191.7 | 78.87 |
| vec_pop_push | 21959.9 | 461.8 | 47.22 |
| fold_scalar | 30272.8 | 66.4 | 457.98 |
| fold_references | 38766.7 | 86.1 | 464.45 |

The local report is `target/lua-comparison/20261104T105657Z-stdlib`; frozen image
and report are under `target/lua-comparison/2026-11-04-stdlib-diagnosis-baseline`.
Image SHA-256 is
`42cd7a284e10e9b7778c82bd17d6705d34d454176292c322be0bdfaed81ee232`;
workload SHA-256 is
`55898e479498738810ca8ab429daf33be7133edf85ddd04ceb21fd52b540278b`.
These measurements are not paired old/new implementation evidence and establish
no optimization win; the production algorithms and validation behavior are unchanged.

Focused local validation covers the actual counting/sampling routes and the
three existing prepared-Native-entry tests with VM diagnostics enabled: mapping
reuse across scalar/nominal applications and GC, result-value rejection, and
source-free mismatch rejection before callback effects. All three pass. Workspace
all-target Clippy with denied warnings, diagnostic-feature benchmark Clippy,
fmt, structural checking (1815 files, zero violations), Python sampler syntax
and diff checks pass. No full workspace/backend/platform test matrix ran locally.

## Remaining decisions and work

- **Runtime representations:** validate fixed layouts against packed Value fields,
  custom erased layouts and the
  [Value/native-shell draft](runtime-gc-integration-design.md). The draft separates
  scoped values, traced carriers, immutable host roots and replaceable host slots;
  it proposes stable shell state for take instead of changing a typed Gc target's
  representation. Exact metadata placement, argument carriers, encoding and index
  validity still need concrete proofs. The draft owns proposed Runtime boundaries;
  the [Runtime map](gc-implementation-design.md#runtime-integration-map) summarizes
  the physical collector boundary.
- **Safe provider authoring:** GcObject, raw initialization and general mutation
  remain unsafe contracts. Opaque Edge is non-Copy/non-Clone; a higher-ranked Init
  callback alone cannot stop an unbranded owned edge escaping. Reviewed providers
  expose safe constructors and operations. A future builder/derive must prove
  field-complete tracing, source retention, publication, scanner repair,
  accounting and unwind behavior before it can accept arbitrary custom objects.
  It must not expose blanket safe mutable object access.
- **Complete execution roots:** adapt actual frames, parked tasks, return/argument
  transfers and host reentry to RootDriver. Validate slot reuse, changed frontiers,
  incoming publication, cursor repair and cleanup on every trap/retirement path.
  No mirrored per-variable Root registration or foreign-store graph is selected.
- **Access intervals:** reuse prepared layout/offset facts only while their
  ownership and admission proofs hold. Release payload/frame loans before calls
  that may collect or reenter; root values that cross that boundary and reacquire
  transient loans afterward. Shared layout evidence never permits caching mutable
  field values across aliases. Follow the user-selected
  [execution policy](runtime-gc-integration-design.md#execution-without-cooperative-cancellation-polling):
  remove cooperative cancellation polling without removing GC service or required
  admission/failure gates. GC work allowance is not an instruction-count interval.
- **Failure policy:** Faulted permits shutdown only. Errors/panics can be caught
  inside Scope, so its state gate remains. Removing that gate would require a
  separate recovery/exit protocol, not merely replacing Result with panic.
- **Performance:** resolve the retained +2.27% to +2.70% reference Rust-fold delta
  from the scalar-transport checkpoint before integrated performance acceptance;
  its cause is still uncertain and requires reference-path architectural diagnosis.
  Allocation, live scanning and reclamation still lag the
  gc-arena lifecycle fixture. Measure first-white-target barriers, mixed/sparse
  pages, multilayer directory access, real destructor tails and equivalent
  sustained policies before claiming broader parity. Prepared dynamic layout
  binding may amortize exact-layout checks, but requires a concrete validity owner.
- **Validation:** full CI/platform/backend acceptance, integrated pressure and
  async retirement and revised execution behavior, coverage-guided fuzzing, trace reduction and
  sanitizers remain open. Existing deterministic graph/Miri evidence is scoped.

### Integration obligations

Design checkpoint, 2026-11-03: the
[integration draft](runtime-gc-integration-design.md#open-decisions) now records
the proposed Value/ownership model, existing API changes and unresolved choices.
The collector design no longer proposes descriptor-changing native take or a
generic core take entrypoint. This is documentation only; GCR00 contract closure,
GCR02 actual Runtime proofs and GCR03 cutover remain incomplete. Documentation
acceptance checks local links/anchors, consistency and diff/line endings; no new
Runtime test or performance result is established.

The draft's [mechanism comparisons](runtime-gc-integration-design.md#mechanisms-consulted-and-differing-assumptions)
now record source versions, applicable ownership rules, differing assumptions and
Kagari-specific evidence for SpiderMonkey, gc-arena, Wasmtime, Lua, mlua, MMTk
and OpenJDK/Java. Repr reuse, host-slot mutability, stored-carrier copy policy,
direct-source ownership, native-shell layout and module/index lifetimes remain
proposals requiring actual Runtime proofs; the references do not close them or
activate another migration. Their detailed open decisions remain in the draft.

User direction, 2026-11-03: the draft now includes
[typical standard-library function bodies](runtime-gc-integration-design.md#standard-library-authoring-proposal)
and their call/storage capability requirements. Script pop/remove use ordinary
scoped removal and result publication; no preallocated Some, PreparedReturn,
rollback or retry is required to preserve contents on a later Runtime failure.
Propagate the original error and preserve completed effects, without mapping it
to None or a script Result. The user selected future direct-value Option wrapping;
exact encoding remains later work, not an implementation added by this design
update. This overrides preserving today's Option-before-removal allocation order.
GC liveness, checked storage commits and standalone owning-root API guarantees
remain intact. Authoring prototypes/proofs belong to GCR00/GCR02 and collection
algorithms remain NC-owned; no code or phase completion is claimed.

The user's Native preparation requirement is recorded in the draft's
[staged-check model](runtime-gc-integration-design.md#native-preparation-and-execution-checks):
Rust compilation, Kagari static checking and existing link/application owners
establish stable signature, mapping, layout and witness facts before body entry.
Typed execution consumes those facts, with actual owner/borrow/liveness,
terminal-failure admission, bounds, mutation and untrusted-value checks retained at their
changing boundaries. Function sketches now use typed parameters and ordinary
NativeResult<R> returns; the adapter publishes scoped/call-local results, rather
than requiring manual return_unit/return_option completion. Reuse existing
signature/application records, not a new proof cache or a parallel Native plan.
GCR00/GCR02 must prove the concrete adapters and attribute repeated-call work;
no implementation or measured reduction is established by this documentation.

User direction, 2026-11-03: remove the cooperative Runtime execution-cancellation
mechanism, not just explicit Native poll calls. The draft's
[execution policy](runtime-gc-integration-design.md#execution-without-cooperative-cancellation-polling)
records uninterrupted synchronous segments, independent GC/admission/failure
responsibilities, retained queued/suspended async retirement and unaffected
compilation cancellation. This explicitly supersedes preservation of in-flight
execution-token interruption in the prior integration requirements. GCR00/GCR02
close the boundary design; GCR03 owns the coherent backend/Native/SDK/spec cutover
and replacement of obsolete interruption assertions with the selected contract.
No active-loop or call-boundary token polling, inert compatibility API or alternate
execution mode is selected. Runtime code and current specifications are unchanged
by this design checkpoint; no performance or phase-completion claim is made.
The user explicitly accepts noninterruptible synchronous execution for performance
and simpler authoring; the draft records this rationale and differing Lua,
Wasmtime and V8 mechanisms without claiming that all runtimes omit interruption.

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
| [HotSpot JDK 25 safepoint options](https://docs.oracle.com/en/java/javase/25/docs/specs/man/java.html), [Go 1.26.0 preemption](https://github.com/golang/go/blob/go1.26.0/src/runtime/preempt.go) | GC safepoints and preemption have different responsibilities. Neither supplies a Kagari instruction interval; the user-selected removal of execution cancellation is a separate semantic decision, not a GC safety relaxation. |

These sources inform ownership and mechanisms, not predicted speedups. Kagari's
custom boundary combines compact dynamic layouts, Rust provider destruction,
resumable scans and explicit host/execution roots under one exclusive driver.
Current implementation invariants belong in the implementation design rather
than a duplicate API sketch here.

## Performance evidence

### Durable standalone benchmark tooling

The [benchmark guide](../crates/kagari-gc/BENCHMARKS.md) owns the persistent
standalone suite, output schemas/estimators, commands and metric limitations.
`scripts/benchmark_gc.py` reuses existing checked fixtures; it builds once,
archives exact executables/raw CSV and alternates saved-baseline/candidate process
order. Reports retain process estimates, paired ratios, tails and separate
managed/metadata memory observations. Missing evidence, incompatible recipes or
changed inputs during measurement fail rather than produce a successful report.
No collector algorithm, Runtime policy or numerical acceptance gate is changed.

This closes repeatable collection/reporting for the existing scalar, lifecycle,
reference, incremental-stage, regular/burst-ring and metadata-capacity fixtures.
It does not close GCR00/GCR04 comparative acceptance: first-white barriers,
mixed/sparse page behavior, real destructor tails, complete allocation/RSS,
long-duration stability and integrated Runtime results remain open. Small-fixture
tails are characterization, and a lower reported step time is not a pass without
matching throughput, progress and memory evidence. Historical tables below keep
their original revision and timing boundaries; buffered instrumentation and new
cleanup rows do not retroactively refresh them.

Focused tooling validation passes 58 parser/runner tests, all ten suites with
seven sequential process repetitions, and seven-pair archived-binary A/A runs.
The A/A run validates evidence plumbing, not a speedup or a noise-free timing gate.
The focused release example build, structure, formatting, workspace all-target
Clippy and diff checks pass. The IDE build endpoint timed out; Cargo supplied the
actual build evidence. Full workspace/platform/backend tests remain CI-only.

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

These measurements precede the typed schema-builder API. Registration/binding
changed outside the field timers; no post-builder timing result is claimed.

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

### Reference-write state-check diagnostic

The reference_access fixture was measured after the direct-return schema builder,
using main c5567989 plus the current standalone builder edits. Environment,
profile, features, warm-cache and single-thread conditions match the method
above. rustc 1.99.1/LLVM 23.1.1, workspace release/default target/default build
parallelism; all builds complete before sequential timing, with no tests or
profiling running concurrently. No compiler inlining or profile flags change.

Compare the original source, removal of only EdgeField::set's initial check, and
that removal plus a diagnostic release-only omission of Marking::publish's state
gate. The last variant is only for valid, healthy inputs: it is not a reviewed
replacement for the generic publication contract. Scope/receiver state checks,
physical lookup, reference marking and all fixture assertions remain active.
An identical original executable is also run as an A/A control. Build each image
with `cargo build --release --locked -p kagari-gc --example reference_access` and
archive it before changing source. Builds took 1.61/1.34/1.23 seconds, excluded.
Run seven groups rotating/reversing four image orders. Each invocation has three
warmups and eleven samples of 50,000 writes for typed/dynamic Idle/Mark/Sweep.
Values below are medians of seven process medians, with their range, in ns/write.
All 2352 CSV rows validate; process execution took 0.969 seconds. These alternate
two retained targets; this is not a fresh-white target or null-write distribution.
Scope entry, registration, collection and result/reclamation checks are untimed.

| Typed reference write | Original | Remove outer check | Also omit inner gate |
| --- | ---: | ---: | ---: |
| Idle | 1.838 [1.838, 1.848] | 1.636 [1.634, 1.746] | 1.636 [1.634, 1.788] |
| Mark | 6.586 [6.532, 6.870] | 6.326 [6.228, 6.348] | 5.958 [5.934, 6.094] |
| Sweep | 6.940 [6.736, 7.006] | 6.376 [6.326, 6.488] | 5.862 [5.796, 5.974] |

Original A/A control medians are 1.840/6.588/6.972, versus original
1.838/6.586/6.940; control ranges are [1.838, 1.932], [6.530, 8.036],
[6.736, 7.400]. Dynamic controls (same order of three source variants) are Idle
1.840/1.838/1.838, Mark 6.868/6.790/6.512, Sweep 6.938/6.906/6.468. Removing only
the typed outer check leaves dynamic source unchanged; small control movement
shows that whole-image layout/execution variation is not eliminated by pairing.

Final-binary inspection uses LLVM 16 objdump/PDB tools. At the typed set function
(original and outer-removed address 0x1400075c0), LLVM already merges the three
Scope checks into one phase load/control path. The original Idle path skips
Faulted/Closed comparisons once phase==Idle is established. Deleting the initial
source check changes branch ordering/register allocation rather than removing
three independently executed state checks. The active path still calls the
non-inlined Marking::publish, whose original entry at 0x1400088f0 checks phase
again; outer-removed entry is 0x1400088b0. Diagnostic entry 0x14000cdb0 removes
that gate, also changes saved registers and moves the function. Thus the observed
0.368/0.514 ns additional Mark/Sweep saving includes generated-code effects; it
is not an isolated cycle cost for two comparisons. Original-to-diagnostic total
savings are about 0.20/0.63/1.08 ns, or 11%/10%/16% of these writes, not script
speedups or whole-GC gains. Optimizing generic publication requires keeping an
authoritative state check at each admitted entrypoint, including caught failures.

All experiment source edits are restored byte-for-byte to the saved original
fields/scope/marking files. Force a rebuild after restoration because copied old
timestamps can otherwise leave Cargo's experimental artifact cached. The normal
release example is rebuilt from restored source. No production change, new unsafe
API, test/assertion removal or timing gate is retained. Previous implementation
validation remains applicable; this documentation update needs link/diff checks.
CI and failure-path acceptance are not claimed by healthy-input benchmark runs.

### Admitted reference publication

The retained implementation moves repeated lifecycle checks to each closed field
operation's admission and shares one publication algorithm with checked callers.
Measure reference_access on main c5567989 plus the current builder/admission
changes, under the environment and release/default-profile/feature/parallelism
conditions above. Baseline is saved immediately before this change. Seven groups
rotate/reverse baseline, an identical baseline A/A control, and the candidate;
each image runs three warmups plus eleven 50,000-write samples per typed/dynamic
Idle/Mark/Sweep case. Build before timing; no concurrent build/test/profile.
Setup, descriptor/Root admission, full collection and result checks are untimed;
all 1764 CSV rows pass. Candidate build took 1.26 seconds, excluded; sequential
process execution took 0.704 seconds. Values are ns/write, median of seven process
medians with their minimum/maximum. Targets alternate between two retained values;
no fresh-white-target latency or Runtime speedup is established.

| Operation | Baseline | Retained candidate |
| --- | ---: | ---: |
| typed Idle | 1.878 [1.838, 2.042] | 1.838 [1.634, 1.838] |
| typed Mark | 6.600 [6.552, 6.736] | 5.162 [5.114, 5.190] |
| typed Sweep | 6.790 [6.734, 6.870] | 5.096 [5.056, 5.114] |
| dynamic Idle | 1.838 [1.838, 1.840] | 1.838 [1.838, 1.840] |
| dynamic Mark | 6.794 [6.758, 6.940] | 5.692 [5.564, 5.892] |
| dynamic Sweep | 6.838 [6.764, 6.948] | 5.714 [5.558, 5.726] |

A/A typed control medians are 1.846/6.578/6.758; dynamic controls are
1.838/6.804/6.938. Static active-phase medians improve about 22%/25%, dynamic
active phases about 16%. Idle static ranges overlap; claim no material Idle gain.
Assembly confirms one state gate in EdgeField::set (0x1400075c0) and a direct
call to the shared admitted core (0x140010d40). The checked wrapper remains for
other callers. Compared with the earlier omission-only diagnostic, the compiler
also inlines physical location into the shared core; this whole-path improvement
is not a cost attribution for individual comparisons or proof of full-collector
throughput. No release-mode Faulted/Closed checks repeat on the field write path.

A separate attempt to inline the thin checked wrapper regressed typed Idle
1.898 -> 2.058 ns/write in its paired run; Mark/Sweep were 6.736 -> 6.050 and
6.892 -> 5.878. It is rejected, retaining default inlining and the result table
above. All samples are preserved. After rebuilding the retained source, the full
instruction stream matches the measured retained image; no favorable rerun of
unchanged code replaces these results. Other provider/scanner callers keep the
checked entry; broader collection/performance acceptance remains open.

The existing quarantine unit test now exercises typed panic, dynamic error and
paired-source error publication failures, then rejects reads, Some/None writes
and source publication while preserving old fields. The generic checked wrapper
also rejects Faulted. An empty RootSource fixture supplies a valid driver without
installing additional managed edges. Debug/release unit tests, the selected
mixed-graph/source/dynamic/failure contracts and strict-provenance Miri pass.
Local acceptance passes structure (1791 Rust files, zero findings), workspace
fmt and all-target Clippy with warnings denied, document/link and diff checks.
The selected native set has 22 distinct tests; both field units also pass in
release. Three strict-provenance Miri cases cover the expanded quarantine unit
and paired dynamic roots. Previous unchanged doctest evidence remains applicable.
No carried failure; exhaustive workspace/platform/backend tests remain CI-only.
No commits.

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
uninterrupted synchronous execution, async retirement, parked reload and source-free
boundaries. Do not dereference freed
memory merely to test rejection or add duplicate smoke suites.

Implementation checks follow [AGENTS.md](../AGENTS.md): affected local tests plus
structure, fmt, workspace all-target Clippy and diff checks; exhaustive workspace,
feature/backend/platform tests belong to CI. Documentation-only changes require
content/link/diff checks. Passing selected tests does not close the complete plan.

### Current validation evidence

Direct-return builder methods pass the seven dynamic contracts, all 22 doctests
and the named-layout example. Structure, fmt, workspace all-target Clippy,
document/link and diff checks pass. Existing Miri evidence remains applicable:
this simplification changes Vec growth/error propagation, not key identity,
layout binding or unsafe access. No new timing or full CI result is claimed.

The schema-builder migration passes 23 focused native tests across dynamic,
metadata, graph, execution roots, shared reads and pacing; release identity and
metadata-lifetime cases also pass. All 22 doctests pass, including scalar-key
and reference-key type rejection, and the named-layout example runs in release.
These selections cover field packing, cross-builder rejection, orphan keys,
descriptor recycling and existing mixed graph/borrow behavior. Broader CI remains
pending. Four strict-provenance Miri cases pass for key identity, packed fields,
scalar domains and metadata lifetime. Structure, fmt, workspace all-target Clippy,
document links and diff checks pass; no carried failure or new benchmark claim.


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
