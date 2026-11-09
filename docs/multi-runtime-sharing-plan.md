# Runtime ownership, prepared reload and multi-Runtime sharing plan

## Status and authority

Status: execution authorized on 2026-11-09, including a commit per completed
phase. MS00 and MS01 are locally accepted; MS02-MS05 have not started. The user selected the
[Runtime target architecture](runtime-refactor-design.md), requested this phase
plan, then authorized its execution. Routine choices within the selected scope
do not require another design-approval round. The progress ledger records actual
acceptance; design/API descriptions are not implemented claims.

This updates the existing 2026-10-23 sharing plan in place. Its motivating workload
is one Actor/Runtime per player with a shared game project. The [roadmap](implementation-roadmap.md#runtime-execution-model-review)
owns activation and scope; this file is the sole MS00-MS05 implementation sequence,
acceptance and progress owner. Do not create a parallel reload or performance queue.
HM live handles and UP00-UP02 publication semantics are implemented foundations,
not prerequisites to rebuild. Preserve their declaration, namespace, state, exact
body/current-inner-call and source-free validation contracts while replacing the
ownership, API access and dispatch mechanism.

The selected design prepares one immutable transferable Program outside Runtime,
keeps each installation's current selection private to its exclusive Runtime,
prepares normal/old-body call tables before publication, and retains complete
shared Program versions for actual old computations. It does not freeze an old
transitive call graph, require Future drain or publish globally across players.
The new APIs/data structures are not implemented merely because this plan is ready.

## Outcome and finite scope

Task: replace Runtime-owned source preparation and Mutex-backed current selection
with shared immutable products, checked local publication and prepared final call
bindings, preserving observable execution/state contracts.

Context: the [prepared call-table design](runtime-refactor-design.md#prepared-call-tables-and-old-code-redirection)
owns the target; current [call](architecture/runtime/calls.md) and
[reload](architecture/runtime/reload.md) walkthroughs locate the implementation.

Expected behavior:

- Prepare/verify one candidate against an exact immutable base while recipients
  continue running; distribute it across owner threads without Runtime access.
- Share code, immutable metadata/recipes and retained MIR without whole-image
  copies per installation. Keep heaps, statics, host values/loans, schedulers,
  constants and local authority independent. Runtime is Send / !Sync.
- Publish with mutable Runtime access at an idle synchronous boundary. Parked work
  may remain. Missing coverage, stale base, host incompatibility or cancellation
  before commit leaves both current and every old call table unchanged.
- Retained frames/Futures/closures/queues keep exact selected bodies/providers,
  environments and completed effects. Their ordinary calls enter current code.
  Current-version direct calls stay on normal links, without reload-specific
  locks, version tests, retired lookups or adapter paths. Normal call performance
  beyond that boundary belongs to ordinary execution work.
- Refresh the whole parked stack's binding views on resume; repeated updates link
  old sources directly to latest without forwarding chains. Reclaim unused old
  records/tables through explicit retention and traced dependencies, preserving
  installation revocation and supported actual-native product lifetime.

Scope includes the minimal coherent Program/InstalledProgram/preparation/lookup/
publication API migration, portable bounded historical demands, checked identity
admission, prepared call tables/transfer recipes, existing native eligibility,
scoped correctness/measurement and direct consumer updates.

Explicit deferrals: a replacement GC or enum/value layout; the later logical slot
stack/public Native-adapter redesign and stdlib split; per-body reclamation/R13;
independent module linking; arbitrary base skipping/branch merge; automatic global
rollout/drain/Actor orchestration; optimizing native expansion, automatic JIT and
native continuation/deoptimization; unrelated compiler/performance migrations.

Validation: a small selected contract set per affected boundary, required local
structure/fmt/workspace Clippy/diff checks, and frozen bounded measurements. Full
workspace tests and exhaustive feature/backend/platform matrices remain CI-only.

Known intermediate errors: none carried from MS00's selected local baseline and
checkpoint checks. Complete workspace tests and exhaustive feature/backend/platform
matrices are unrun locally and remain CI acceptance. Record new phase failures with
command, representative diagnostic, cause and repair owner; a phase cannot be
accepted with its required integration failures open.

## Evidence and root-cause classification

The following is source inspection, not a new benchmark. Existing executed evidence
in [performance measurements](performance-baseline.md) belongs to its recorded
revision/workload. In particular, the 2026-10-03 tiny-program multi-Runtime figures
are historical, not a current large-project capacity estimate.

| Owner | Current mechanism | Required diagnosis or change |
| --- | --- | --- |
| [PreparedProgram](../crates/kagari/src/program.rs) | Rc/RefCell preparation/cache state is neither Send nor Sync; immutable source inputs and retained MIR already use Arc across publication associations | Preserve existing input/MIR sharing, split product/cache ownership and measure remaining duplication rather than fixing an obsolete by-value MIR path |
| [VerifiedProgram and ProgramDescriptor](../crates/kagari-runtime/src/module.rs) | Arc-backed execution preparation is shared; each installation owns its descriptor, layouts and linked modules | Retain existing instruction/frame preparation sharing; attribute remaining code-sized installation records |
| [Identity normalization](../crates/kagari-runtime/src/module/admission.rs) | A different definition scope with referenced IDs remaps modules/exports into new records while sharing execution and the version token | Separate immutable declaration facts from Runtime binding authority; avoid reconstructing complete images on each load |
| [Linked execution](../crates/kagari-runtime/src/module/linked_execution.rs) | Runtime-local per-function arrays and linked call/native/layout records | Separate reusable recipes from actual Runtime-local targets; do not share GC Values or authority merely because a record is immutable |
| [Layout linking](../crates/kagari-runtime/src/module/linked_execution/layouts.rs) | Scans original instructions per installation to construct site/operand tables and admitted layouts | Prepare site/operand recipes once; retain checked instance/application admission where necessary |
| [Native installation](../crates/kagari-runtime/src/native/installation.rs) | Prepared registration records/callbacks are shared; checked host maps and runtime state are copied | Attribute immutable registry facts versus local host identity/borrow records; different callback captures are not interchangeable |
| [Runtime constant pool](../crates/kagari-runtime/src/module/constants.rs) | Local constant slots and heap materialization | Keep local GC identity; measure slot and materialized-value cost without introducing shared-heap semantics |
| [Program native products](../crates/kagari/src/program/native.rs) | Cached compilation products can be shared; installation/leases remain local | Preserve actual native validity and product sharing; execution must not contend on the compile-cache lock |

The principal architectural issue is coupling code/metadata identity adoption to
per-Runtime reconstruction, plus mixed shared facts and local bindings. Necessary
per-player object/state/borrow costs are not defects. Already-shared prepared
instructions should not be replaced or credited as a new saving. Same-domain and
cross-domain loads must both be measured: favorable same-owner loading alone does
not prove the general sharing contract. Repeated loads into one shared destination
context must not repeatedly reconstruct equivalent full normalized products either.

### Retained MIR sharing boundary

The MS00 source recheck found that ProgramState and the former
RetainedPreparation both stored `Option<Arc<VerifiedMirProgram>>`; retained input
and publication associations cloned that shared owner. MS01 removes the separate
association model and keeps MIR in the shared [Program owner](../crates/kagari/src/program.rs).
The earlier by-value MIR description was already obsolete at MS00 and is not
a remaining task or a new saving to claim.

Preserve one immutable MIR backing per prepared product/configuration across
compatible installations and preparation lookups. Source-input snapshots, normal
and redirect tables, metadata normalization and native pages remain separate
memory categories. MS00 must inspect those actual owners rather than infer total
sharing from one Arc. A mutable edit/reverification candidate can still need an
explicit owned copy outside ordinary load and execution.

MIR-present source/encoded input and real native execution after compiler/Program
owner drop remain required regression samples. Failed local publication cannot
advance an input association or lose native availability. The last legitimate
owner releases retained inputs; separate versions or inherited type/initializer
retention must not be hidden in the measurements.

## Intended ownership and execution model

These are responsibilities, not a requirement for additional public types:

```text
Frozen program version: shared across compatible workers/installations
  code and prepared instructions, immutable contracts/export graph,
  optional sealed MIR/compiler input shared by program version,
  frame/layout recipes, debug data, eligible compiled products

Runtime installation: private to one player Runtime
  installation/module identity, current publication reference,
  validated host bindings, static cells, local constants and necessary links

Execution and player state: private to that Runtime
  GC objects/roots, frames, captures, pending native waits, Tasks and local caches
```

### Identity and linkage

MS00 selects immutable product-scoped IDs/tables with checked installation
binding records. The [consumer and API decision](runtime-refactor-design.md#ms00-representation-and-api-decision-2026-11-09)
specifies multi-scope type/native admission and the public migration map.
Do not add a per-instruction remapping layer, copy the image into a cache per player,
use names/hashes as authority or infer equality from identical numeric scoped IDs.

The existing installation-scoped identity writer is a measured retained foundation.
This plan does not reopen its lifetime policy without a concrete incompatibility
and explicit scope decision. Preserve accepted-prefix checks, exact version identity,
independent artifact verification, nominal/application equality, foreign-runtime
rejection and host type/path validation when relocating identity adoption.

Classify every affected link field as shared pure fact, instance authority/state or
selected computation retention. Prepare pure call/operand/layout recipes once;
instantiate only genuinely local references. A shared declaration/layout must not
make an object or host borrow valid in another Runtime. Preserve generic environment
and trait evidence without runtime type inference or a second semantic implementation.

### Thread sharing and caches

The frozen executable product used by workers must support safe Send + Sync through
its real fields. The compiler's mutable source/query state need not become shared.
Choose whether PreparedProgram directly exposes the frozen owner or retains a
separate preparation role; avoid redundant public wrappers solely for forwarding.
No unsafe trait implementation may paper over Rc/RefCell or owner-bound contents.

Mutable native/preparation caches need a clear preparation-side owner. Any reusable
cache has an exact product/configuration/provider validity key, an explicit lifetime
and a measured need. Host callbacks may deliberately share captured Rust services;
that does not permit sharing a player's mutable payload or borrow registry. A
prelinked product is reusable only when its captured bindings permit it.

### Deferred JIT delivery and Runtime-local installation

User direction recorded 2026-10-28: prepare finalized machine-code products outside
player execution, deliver them to the Runtime owner, and install them at an eligible
safe boundary before a subsequent function call. Sharing code is acceptable, but
ordinary execution must not acquire a shared compilation-cache lock to obtain or
execute it. This records a design direction, not implemented automatic JIT scheduling
or authorization to expand MS03 into a background compiler/Actor framework.

Separate immutable products from mutable entry selection:

```text
Preparation task: exclusively compile and finalize a product
  -> deliver an owning reference and exact product/version identity
Runtime owner: receive at a permitted safe boundary
  -> validate and install in Runtime-local entry selection
Subsequent call: read the local installed route
  -> invoke finalized machine code with this Runtime's context
```

- Publish only finalized immutable code and metadata. Multiple Runtimes may hold
  references to the same product; each owns its installation/binding choice and
  player state. Shared code does not require one globally mutable entry table.
  Keep local routing records bounded; this is not permission to copy all immutable
  dispatch/code metadata per player.
- A background task must never mutate a Runtime's entry table directly. Delivery
  may use synchronized queues and preparation caches; installation is serialized
  by the Runtime's exclusive owner. A queue notification alone does not establish
  permission to install, nor does `Arc` make concurrent mutation safe.
- A function-call boundary is a candidate acceptance point, not a claim that every
  nested call or host reentry is safe for installation. Define the permitted boundary
  and reentry/active-frame rules before implementation. Actor message boundaries or
  explicit owner-side processing may amortize delivery without polling a shared
  queue on every call. Preserve the current host-top-level publication contract;
  installing another execution strategy for the same verified version must not
  silently publish a different source version.
- Before installation, verify the exact function/program version, ABI/backend
  configuration and required Runtime bindings. If reload superseded the version
  while compilation ran, reject the stale delivery for the new version; never
  install by matching a name alone. Handle failed installation, cancellation and
  Runtime shutdown without leaking products or admitting execution effects.
- Switching local entry selection affects subsequent eligible calls only. Existing
  invocations retain their selected code owner until completion; do not patch or
  release machine code underneath them. After source publication, ordinary inner
  calls still obey current-call selection; this does not restore pinned dependency
  semantics or authorize automatic deoptimization/tier replacement.
- Normal invocation uses the retained installation and function address, not a
  shared-cache lookup or a lock held across execution. Reference-count changes at
  delivery/installation/retirement and bounded local dispatch costs are distinct
  from per-call cache synchronization. Do not promise zero atomic operations for
  the whole runtime or place shared Arc churn on every instruction.

Current evidence: the SDK explicitly prepares and executes native entries;
[Cranelift emission](../crates/kagari-codegen-cranelift/src/emit.rs) finalizes code
before publishing its owner, and [native invocation](../crates/kagari-runtime/src/backend/native.rs)
uses an installed entry address and an explicit Runtime context. Its code-memory
owner's Mutex is accessed through exclusive `get_mut()` during construction/drop,
not locked to execute finalized code. Automatic background delivery and ordinary
call-entry replacement are not implemented. Coordinate their future scope with the
[deferred native API review](implementation-roadmap.md#deferred-native-execution-api-boundary-review)
and [Cranelift expansion plan](cranelift-expansion-plan.md), rather than making hosts
manage version-specific machine-code handles for ordinary calls.

When that work is activated, require evidence for stale deliveries after reload,
independent installation by two Runtimes, in-flight code lifetime, failed/cancelled
installation and shutdown, plus absence of shared compilation-cache access on warm
calls. Measure delivery/installation separately from execution. This does not claim
that the current backend supports all calls or that any latency gate has passed.

### Hot path and current-code selection

Read-only sharing requires no reader lock. Frames retain needed owners at lifecycle
boundaries and borrow prepared data during dispatch; do not clone/drop shared Arcs
or acquire global locks per instruction. Any extra instance indirection must be
bounded and attributed. Binding resolves paths once; hot calls do not perform string
lookup or cross-domain normalization. No global mutable inline cache may create
cross-player contention or hide missing preparation.

The implemented host contract exposes latest metadata through live Program/Module
handles. MS03 moves that lookup through Runtime and leaves handles as identity-only
references or explicit immutable snapshots. Preserve latest-publication contents,
namespace continuity and stable callable selection without rebinding. Immutable versions and implementation tables may
be shared, but each installation owns its publication choice. Updating player A
must not switch player B. A shared image is not a shared global mutable dispatch table.

The [activation specification](spec/module-activation.md#publication-boundary)
owns host-top-level serialized publication, current-call dispatch and retained
Future/closure/resume semantics. Sharing must not add a concurrent publication
system or force a per-player full copy of the dispatch image. Retained old
computations keep needed bodies/state; their ordinary calls select current
implementations. Reuse these implemented contracts rather than replaying update
implementation or introducing state-replacement support.

Runtime owns installed state. Ordinary Program/Module Drop does not unload, cancel
or reset it; update/reload succeed with unit rather than replacement handles. Shared
products do not grant execution after Runtime shutdown. R13's type-only retention
of old executable payload remains separately owned; measure it honestly and avoid
new accidental retention without claiming this plan completes that broader split.

## Primary references and differing assumptions

Rechecked on 2026-11-09 for MS00. The linked Wasmtime API pages identify
themselves as 52.0.0-dev; this records the consulted documentation version, not
a Kagari dependency choice or a pinned release/source audit:

- [Wasmtime Module](https://docs.wasmtime.dev/api/wasmtime/struct.Module.html):
  immutable compiled representation, cheap shared clones and Send + Sync, separate
  from the stateful Instance. Reuse the code-versus-instance ownership principle;
  do not replace Kagari's tracing heap, typed host boundary or public Module naming.
- [Wasmtime InstancePre](https://docs.wasmtime.dev/api/wasmtime/struct.InstancePre.html):
  checks/import resolution can precede instantiation, but reuse across Stores is
  conditional on captured imports. Reuse that limitation when separating Kagari's
  pure link facts from Runtime-bound host authority. Wasmtime instantiation may run
  a start function; Kagari load/publication must retain its no-script-effects rule.

Neither reference supplies Kagari's live callable update semantics, scoped definition
admission or nominal generic/type contracts. A custom boundary here adapts those
requirements; it does not justify inventing another executor or unvalidated fast path.
[JVMTI 25.0 RedefineClasses](https://docs.oracle.com/en/java/javase/25/docs/specs/jvmti.html#RedefineClasses)
specifies that active frames keep their previous method bodies while subsequent
method invocations use the replacement, and static values survive redefinition.
Use that distinction between retained computation and current call selection.
Kagari additionally retains created-but-unstarted closure/Future bodies and parked
stacks; Java stack semantics do not supply Kagari's generic/witness transfer or
cooperative async ownership. The local call-table design adapts these differences
without requiring whole-old-call-graph pinning or new frame deoptimization.

## Measurement and acceptance contract

### Baseline selection

MS00 freezes a finite fixture set before implementation:

1. Tiny control for fixed Runtime/native-registration cost.
2. Representative game-shaped program with modules, structs, methods/traits,
   statics, constants and checked generic applications. Prefer a real supplied
   project; otherwise clearly label generated fixtures and retain their generator.
3. One bounded size series separating function/instruction growth from type/export
   and generic-application growth. LOC is descriptive, never the memory denominator.

Use one prepared product with 1, 32 and 128 simultaneously retained Runtimes as the
initial bounded scale series; adjust before measuring if machine capacity requires
it. Record actual counts and resource bounds. Include independent preparation as a
control, same-provider same/different definition domains, and post-MS03 worker-thread
installation. A synthetic result is not capacity certification for a real game.

Measure separately: compilation/preparation, empty Runtime construction, load-only,
player initialization, representative warm calls, suspended async work, compatible
reload with old work alive, and teardown. Do not instantiate active stacks for every
function merely to count code. Separate source/analysis/MIR retention from execution
products; report enabled features and intentional source-debug/native retention.
Include MIR-present and bytecode-only controls, reusing one prepared program across
players. Attribute retained MIR backing separately from compilation snapshots,
bytecode, machine code and local publication records. Count allocations/copy work
on load, reload and repeated preparation lookup; do not infer sharing solely from
`PreparedProgram` Rc equality. Include dropping the preparation handle while
installations remain and old/new-version coexistence during independent updates.

Report:

- Live requested heap bytes, allocation counts and cumulative requested bytes as
  different quantities; backing capacity/high-water retention and process RSS
  separately where available. Executable mappings are not ordinary Rust heap bytes.
- Shared code/metadata bytes and attributable per-installation/state bytes, including
  any new binding tables, cache storage and reference-owner allocations.
- Marginal cost `(retained(N2) - retained(N1)) / (N2 - N1)` for each measured stage;
  shared baselines, nonlinear growth and retained versions must remain visible.
- Load latency, warmed execution time/throughput and bounded multi-worker latency
  distributions. Use identical workloads/provider sets on baseline and candidate.
- Toolchain, revision, machine/OS, workspace profile/features, default build
  parallelism/target, worker counts, cache/warmup state and sampling method.

Do not overlap builds with measurements. Counting allocators perturb timings;
measure timing separately. Keep temporary output under ignored target/ and retain
methods, fixture identities, summaries and limits in tracked documentation.

### Required outcomes and gates

Structural acceptance requires one shared code/immutable-recipe backing per product
and relevant configuration, not one reconstructed image per player. Exact code-image
allocation evidence must accompany total retained-byte measurements; pointer equality
alone does not account for other duplicate records. Instantiation may still have
O(imports/statics/local-bindings) work, not necessarily constant time. Account for
remaining function-sized private arrays instead of claiming all installation cost
is proportional only to active player objects.

MS00 must freeze numerical memory-reduction and setup/steady-state performance gates
from the current baseline before MS01. The plan deliberately has no invented
KB-per-player target for an unavailable production project. Lack of a frozen gate
blocks implementation acceptance; gates cannot be relaxed after seeing a regression.
Use repeatable samples and an uncertainty rule: an inconclusive/noisy result is not
a pass, and average improvements cannot conceal a failed selected workload.

No new global read lock, string lookup or normalization in script dispatch is
acceptable. Measure hot Arc traffic/indirections where changed. Shared backing may
reduce working-set pressure but no throughput, RSS or tail-latency benefit is assumed.
NUMA replication, lazy constant slots and sparse caches require later evidence, not
speculative inclusion. A failed performance gate triggers ownership/cost diagnosis
and a bounded scope decision, not an unlimited sequence of local workarounds.

## Phases

Execute MS00 through MS05 in order. These are the sole implementation phases for
the selected Runtime ownership/prepared-reload work; the later slot/Native API
proposal is not another prerequisite. The existing phase IDs and heading anchors
are retained, with scope expanded in place on 2026-11-09.

| Phase | Main delivery | Requires |
| --- | --- | --- |
| MS00 | Consumer inventory, identity/API decisions, bounded baseline and gates | Selected target design |
| MS01 | Shared immutable Program and detached preparation/update | MS00 |
| MS02 | Prepared normal/old-body tables, atomic replacement and frame refresh | MS01 |
| MS03 | Runtime-owned live lookup, mutable publication API and current-lock removal | MS02 |
| MS04 | Actual-native integration, cross-thread isolation and old-owner retirement | MS03 |
| MS05 | Selected integrated acceptance, measurements and durable documentation | MS04 |

### MS00 — Freeze baseline, ownership and publication invariants

Task: close implementation choices and establish comparable evidence before
changing product ownership. No production cutover in this phase.

Owners: SDK program/source/preparation, runtime identity/store/dispatch, VM frame
activation and existing sharing/native measurement owners.

- Inventory every product/link field and affected consumer as shared immutable
  fact, local authority/state, exact computation retention or disposable compiler
  state. Include retained MIR/source inputs, callbacks, static cells, layout and
  witness descriptors, native pages, Module metadata and public callable handles.
- Choose one checked identity representation against those consumers: shared
  definition ownership or immutable product IDs with bounded local links. Preserve
  installation/generation and nominal identity; do not infer authority from names,
  equal numeric IDs or Arc identity. Record the mechanism and existing primary
  source/version comparison in the design before MS01.
- Freeze the minimal API surface and migration map: immutable SDK Program, local
  InstalledProgram identity, detached prepare/update against an exact base, live
  lookup through Runtime, immutable snapshots and mutable local replacement.
  Name collisions are resolved through explicit owner imports, not aliases that
  preserve removed APIs. Separate shared native products from compiler/cache state.
- Specify checked revision/base evidence and portable cumulative demand ownership.
  Include historical generic applications, semantic impl/binder correspondence,
  source-free verification and source-edit locator ownership. Keep the existing
  bounded contract solver; do not invent a Runtime source resolver.
- Select existing fixtures/test names and freeze the finite baseline and numerical
  gates below. Separate current-only calls, old-body adaptation, preparation,
  local binding/publication and shared/local retained memory. Use A/A controls to
  set uncertainty before comparing changes; no invented speedup or capacity target.

Acceptance: a concrete field/API/identity map, test-owner selections, reproducible
baseline and gates, with no unresolved sharing/authority/Send+Sync choice required
by MS01. Record remaining Rust layout details at their owning phase. A missing
production-sized project limits capacity claims, not the bounded implementation.

### MS01 — Share immutable code through identity admission

Task: replace the mixed preparation product and enable one detached update to be
distributed without borrowing any recipient Runtime.

Owners: SDK `program`/`engine::source`/source-update preparation;
runtime VerifiedProgram inventory / installation identity admission;
compiler/bytecode/contract handoff.

- Implement the MS00 immutable Send + Sync product and local InstalledProgram
  naming coherently across SDK, Runtime, VM, CLI, web adapter, examples, benchmarks
  and actual native consumers. Remove obsolete aliases/wrappers at this boundary.
- Apply the selected identity boundary so instructions, immutable contracts,
  operand/layout recipes and retained MIR are shared instead of reconstructing
  a complete identity-remapped image for each installation. Keep authority/state
  local; measure residual function-sized binding records explicitly.
- Extract source edits from Runtime::update/Runtime-scoped locators into detached
  base-Program preparation. Reuse captured immutable CompilationInputs and source
  package mappings, then lower with the base's cumulative historical demands.
  Failed/cancelled preparation leaves both base and all installations unchanged.
  Missing compilation context remains an explicit error for artifact-only input.
- Migrate callers to prepare once then use the existing local load/reload boundary;
  remove Runtime-owned source replay and its publication-associated capsule map
  where superseded. Do not retain a second update implementation or a forwarding
  Runtime::update compatibility entrypoint. Mutable replacement ownership follows
  in MS03; this phase already has one preparation path.
- Move mutable native/compiler cache work out of the shared product. Share verified
  MIR/finalized products where their actual fields permit it; native installation
  still uses the currently supported checked execution seam. No unsafe Send/Sync
  assertion may conceal a thread-bound backend, callback or memory owner.

Acceptance: one detached candidate can be prepared while a recipient runs and
then loaded/reloaded independently; same/cross-definition-domain installations
share product/MIR backing and preserve foreign-owner, provider and artifact
rejections. Source-free load executes without frontend inputs. A supported real
native entry still executes after preparation/compiler owner drop. These are
selected witnesses, not a full backend/platform matrix.

### MS02 — Separate reusable link facts from instance bindings

Task: materialize final call bindings and replace per-call version correspondence
without changing old-body/current-inner-call semantics.

Owners: runtime module execution/linked_execution/dispatch/loading/staging,
frame admission/calls/closures/future, execution-metadata transport, VM driver;
compiler/contract/bytecode own portable checked recipes.

- Introduce the design's RevisionRecord/CallTable/BoundCall responsibilities with
  checked generational keys. Shared recipes own identities and operand facts;
  local tables own final targets and admitted transfer/dispatch binding keys.
  Keep old bodies' constants, static-cell bindings and environment provenance.
  Move MS01's temporary per-use interface-upcast type closure out of
  `ExecutionFrame::resolve_type` into the linked function/environment's prepared
  operand descriptors; remove the superseded backend path at this checkpoint.
- Build normal tables for candidate code and direct-to-candidate tables for every
  retained source revision. Resolve declaration/application/binder correspondence
  during preparation/linking, including parameterized generic/witness recipes.
  Missing historical current coverage rejects before commit. Interface dispatch
  uses the current checked receiver/implementation catalog, including additions.
- Migrate explicit direct/shared calls, implicit witness/construction/protocol
  operations, named values, closures/Futures, static recipes and native reentry.
  Ordinary warm calls must not reconstruct PublicationMapping or fall back to the
  old route solver. Actual value conversion, loans, roots, cancellation and depth
  remain checked at their required execution boundaries.
- Make candidate normal tables, all old-body redirect tables, demand history and
  native eligibility one prepared publication. Complete allocations/checks first;
  commit all visibility changes without failure or callback. Rejection leaves
  current and every old table unchanged. Release superseded owners safely after
  the commit. The old current-cell Mutex may still protect cold metadata access
  until MS03; it is not an excuse to keep per-call correspondence.
- Separate persistent FrameState from the active code/call-table view. Release
  views at parking and refresh the complete parked frame stack on resumption,
  including parents resumed after child return. Preserve original body, PC,
  locals, captures, return destination and error origins. Normal Direct entry
  carries its normal binding locator without a legacy/revision-selection test.
- Repeated publication binds v1 directly to v3 and v2 directly to v3. Retired
  records/tables are not unconditional roots and do not form predecessor chains.
  Do not keep a selectable legacy execution mode after migrating consumers.

Acceptance: cold/parked old computations call new helpers, current code follows
normal BoundCall entries, and v1-to-v3 updates have no forwarding chain. Exercise
one complete parked parent/child stack, generic/witness/result adaptation, current
interface additions, callback reentry with GC, initializer helpers, atomic failure
and source-free evidence. Validate phase-local native eligibility with supported
products; optimizing native capability expansion is not required.

### MS03 — Deliver cross-thread frozen product reuse

Task: finish exclusive Runtime ownership of current/publication and remove the
shared current cell and its call-path liveness locks.

Owners: core Runtime/module installation/exports/publication; SDK Runtime/query/
invocation facades; VM entry and direct consumers.

- Store current and revision records under the exclusive Runtime owner. Make
  installed Program/Module handles identity-only; latest metadata lookup uses
  Runtime and explicit snapshots retain immutable evidence. Preserve namespace,
  alias-binding, close/unload authority and stale/foreign-generation checks.
- Require mutable Runtime access for load/replace/drive and relevant host entry
  surfaces selected in MS00; migrate all direct consumers. Retain dynamic
  synchronous/callback/observer/reentry/cleanup admission invariants. Do not remove
  a safety check just because an outer method now accepts &mut Runtime.
- Remove Installation.current's Mutex, its independent shared-live-query path and
  repeated capture-current/liveness reads inside bytecode frame admission. Reuse
  activation-scoped authority while preserving dynamic value/borrow revocation.
  Normal calls use MS02's already selected links, not a snapshot cache bolted onto
  the old shared pointer API. No atomic pointer replacement is needed.
- Complete the detached prepare/distribute/mutable-local-replace public workflow.
  Check exact base, installation history, host bindings and native eligibility per
  recipient; one failed recipient cannot advance another or a global baseline.
  Keep compiler work and hidden native compilation outside Runtime publication.
- Establish field-derived product Send + Sync and Runtime Send / !Sync evidence.
  Move Runtime ownership between workers outside synchronous scopes; share no
  script heap, callback payload, borrow registry or scheduler state.

Acceptance: one candidate crosses threads and is independently installed by at
least two Runtime owners, preserving distinct static/host state. Cover a rejected
recipient and stale base, live handle continuity, immutable snapshot behavior,
close revocation, Native/observer reentry rejection and Runtime movement across
awaits. All current-cell lock sites are removed; normal current direct calls have
no reload-specific comparison, retired lookup or adapter path.

### MS04 — Validate player isolation and publication lifetime

Task: close actual-native, asynchronous retention and multi-Runtime integration
on the final ownership model, without adding new native capabilities.

Owners: SDK program/native and AOT installation, runtime backend/native/GC/metadata,
VM prepared execution and existing async/publication test owners.

- Verify supported finalized native products and shared MIR outlive their compiler
  and preparation owner, then execute through each recipient's checked local
  bindings. Keep exact body/configuration/ABI/assumption keys and independent
  eligibility; product sharing must not share Runtime state or validity mutation.
- Preserve entry invalidation when an old product depends on changed ordinary
  callees. Fallback, where supported, uses the same old root body's verified
  bytecode with current inner links. Unsupported native calls/suspension decline
  before entry; failures never restart completed effects. Do not invent inlining,
  native continuation/deoptimization or automatic background JIT for this phase.
- Exercise independent publication, distinct roots/constants/statics/host loans,
  ordinary calls from cold Futures/retained closures, queued work, cancellation,
  traps and close. Producer notifications and cleanup must not revive retired
  authority or publish code from a background thread.
- Pair old-owner/table/compiled-page lifetime checks with actual execution values
  and cleanup. When all legitimate owners release a revision, its local tables and
  product references retire without touching another Runtime's owners. Attribute
  original initializer and inherited R13 retention instead of claiming Future-only
  or per-body reclamation. No new version history may root executable predecessors.

Acceptance: actual-native source/encoded-input samples, asynchronous old/new
selection, one-recipient failure and last-owner teardown pass with independent
oracles. No extra mutable state is shared and no supported native route silently
turns into interpreter-only acceptance. Full native/platform matrices remain CI.

### MS05 — Integrated measurement and documentation closeout

Task: accept the complete selected migration and retire temporary plan detail.

- Run the MS00 frozen bounded memory/setup workloads and normal-call samples with
  old work absent, alive across publication, and collected. Attribute old-code
  adaptation separately. Confirm normal calls never enter retired lookup/adapter
  code and introduce no reload-specific allocation or per-call owner churn.
- Compare shared product/code/MIR backing and local table/state bytes, preparation,
  local publication, steady execution and retained-version teardown. Freeze the
  workload/environment before comparing; normal call performance beyond reload
  machinery is separate work, not a reason to add slot/GC/Native API scope here.
- Satisfy selected correctness witnesses and required local structure/fmt/Clippy/
  diff checks. Resolve carried errors; distinguish pending CI from actual passing
  local results. Reuse unchanged passing evidence rather than rerun every package.
- Move final API/data ownership and behavior into architecture/module activation/
  embedding owners; retain measured methods/results/limits in performance-baseline.
  Put remaining work in the roadmap and remove completed execution-plan content.
  Git history owns completed phase logs; no compatibility aliases or obsolete
  owner/dispatch implementation remain.

Acceptance: all selected contracts, frozen sharing/performance gates and local
implementation checks pass with no carried integration error. No source-LOC-based
capacity, zero total call cost, optimizing-tier or unrun CI claim is made.

## Verification and checkpoint policy

Each phase is a buildable boundary with direct consumers migrated. The default
policy is no carried integration errors, production stubs, fake success, weakened
validation or disabled semantic tests. If an indivisible edit batch temporarily
breaks compilation, finish that batch and repair within the same phase before
acceptance. An unexpected larger build boundary needs an explicit scope decision,
not an implicit exception. Do not add compatibility aliases, duplicate executors
or a fallback to the superseded per-call route path to obtain a green build.

### Contract owners and selected commands

The following are current test-owner seeds, not commands to run at every phase.
MS00 resolves exact names using scoped `--list` where necessary and records the
selection/rationale. After an owner/API move, migrate its tests and update the
filter in this plan; a zero-test match is not passing evidence. Choose the small
affected set plus directly impacted regressions and reuse unchanged passing results.

| Contract | Existing owner | Main phases |
| --- | --- | --- |
| Shared backing, normalization and foreign installation rejection | [Runtime module tests](../crates/kagari-runtime/src/module/tests.rs), [engine sessions](../crates/kagari/src/engine/tests/engine_sessions.rs) | MS00/MS01/MS03 |
| Immutable source/preparation inputs | [Program source preparation](../crates/kagari/src/program/tests/source_preparation.rs), [source updates](../crates/kagari/src/program/update/tests.rs) migrated to the detached owner | MS01/MS03 |
| Current targets, aliases and impl binder correspondence | [Dispatch tests](../crates/kagari-runtime/src/module/dispatch/tests.rs), [generic reload](../crates/kagari/src/runtime/tests/embedding/generic_reload.rs) | MS02/MS03 |
| Publication guards, reentry and parked work | [Publication tests](../crates/kagari-runtime/src/session/execution_sessions/publication.rs), [async facade](../crates/kagari/tests/async_facade.rs) | MS02/MS03/MS04 |
| Whole-stack refresh, cold/queued work, cancellation and GC | [VM tests](../crates/kagari-vm/src/tests.rs), [async lifecycle](../crates/kagari/src/runtime/tests/embedding/async_execution/lifecycle.rs), [Runtime transfer](../crates/kagari/src/runtime/tests/embedding/runtime_transfer.rs) | MS02/MS03/MS04 |
| Runtime binding safety and scoped loans | [Typed calls](../crates/kagari/src/runtime/tests/embedding/typed_calls.rs), [scoped calls](../crates/kagari/src/runtime/tests/embedding/scoped_calls.rs) | MS01/MS02/MS03 |
| Bounded historical coverage/provenance | [Contract update tests](../crates/kagari-contract/src/updates/tests.rs), existing bytecode verifier tests selected in MS00 | MS01/MS02 |
| Genuine source-free consumer | [Artifact features](../crates/kagari/tests/artifact_features.rs) with a separately emitted fixture | MS01/MS02/MS04 |
| Actual native selection and product lifetime | [Cranelift preparation](../crates/kagari/src/program/tests/cranelift_preparation.rs), [VM prepared execution](../crates/kagari-vm/src/tests/prepared_execution.rs) | MS01/MS02/MS04 |

Initial command examples, to update with renamed owners rather than preserve old
modules for their names:

```text
cargo test -p kagari-runtime --lib module::dispatch::tests
cargo test -p kagari-runtime --lib session::execution_sessions::publication
cargo test -p kagari-contract --lib updates::tests
cargo test -p kagari --lib program::tests::source_preparation
cargo test -p kagari --lib runtime::update::tests
cargo test -p kagari --lib runtime::tests::embedding::generic_reload
cargo test -p kagari --test async_facade relative_typed_members_and_current_reload_workflow
cargo test -p kagari --lib runtime::tests::embedding::runtime_transfer
cargo test -p kagari --lib source_and_encoded_mir_use_real_native_code_after_backend_and_program_drop
cargo test -p kagari-vm --lib tests::prepared_execution
```

For the one genuine source-free boundary, emit the disposable feature fixture once
after affected contract changes, then run the selected no-frontend consumer:

```text
cargo run -p kagari --example regenerate_feature_artifact
cargo test -p kagari --no-default-features --test artifact_features portable_artifact_executes_without_source_compilation
```

This is one targeted source-free sample, not a locally expanded feature matrix.
Keep artifact limits, malformed/provenance rejection and actual native coverage at
their owners. Batch other necessary fixture refreshes without routine ABI/schema
identifier bumps; never weaken current validation to admit stale products.

Strengthen existing owner scenarios before adding new cases. Distinct additions
are justified for atomic multi-table failure, direct v1-to-v3 rebinding, the parked
parent/child resume boundary and final table lifetime. Keep fixed Kagari programs
in their owning `tests/fixtures/` and explicit lifecycle/value assertions in Rust;
do not add a universal runner or another test binary. Use `kg fmt` and
`kg fmt --check` for ordinary fixtures, preserving deliberate malformed/raw bytes.

### Measurements and mandatory local checks

Use the baseline protocol above and the existing [Kagari/Lua driver](../benchmarks/lua-comparison/README.md).
A bounded normal-call sample can start with:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --workload calls --runs 7
```

The original and source-form controls, old-body redirection probe, allocation and
memory fixture selections are frozen in MS00; the command above is not the entire
acceptance set. Keep builds outside timing, checked results outside timed regions,
environment/profile/features/parallelism and A/A uncertainty explicit. Temporary
probes/logs stay under ignored target; tracked conclusions must remain reproducible.
Do not change Cargo target directories, jobs or workspace profiles to obtain a win.

Before each implementation checkpoint, review changed imports/paths (including
nested scopes and macros), responsibility/visibility, re-exports and effective
Rust LOC. Use meaningful modules, no production globs/deep traversal or unchecked
visibility widening. Record narrow structural debt/owners in the design; every
changed re-export still needs its exact whitelist entry. Then run:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Workspace Clippy is mandatory locally for implementation phases, not replaced by
scoped Clippy. Full-workspace tests and exhaustive feature/backend/platform/native
matrices remain CI-only. Use package/target/name filters, `--no-fail-fast` for a
selected multi-target run, and rerun failing/affected checks after batched repairs.
Separate actual local results, known failures and pending CI; do not call unrun
work passed. Planning-only documentation edits need content/link/diff checks,
not Rust builds or Clippy; this does not waive MS00's actual baseline builds and
measurements. Use `uv run python` for Python and preserve line endings.

## MS00 frozen evidence protocol

MS00 selects the [product/consumer/API boundary](runtime-refactor-design.md#ms00-representation-and-api-decision-2026-11-09).
It adds no production cutover. The executable baseline is `1d66c45a` plus the
runtime-storage probe and documentation changes in this checkpoint. The probe
reuses the existing example and counting allocator; it adds no benchmark binary,
universal test runner or production counters.

### Finite workloads and attribution

Use `runtime_storage --sharing N F DOMAIN INPUT`, where `F=0` is the fixed tiny
control and `F=32/128` uses the fixed `sharing.kgr` struct/trait/static/generic
program plus exactly that many generated public scalar functions. Generation is
retained in `sharing.rs`. These are synthetic workloads with the default standard
library; they are not a production game or an estimate of maximum player count.
The size series holds types/generic applications fixed while growing functions.
Multi-module contracts, type/application growth and suspended/native lifetimes
are sampled at their existing semantic owners rather than multiplying this
storage matrix. A production-shaped capacity claim requires a supplied workload.

Freeze these seven sharing cases, in this order, for both ordinary timing and
diagnostic allocation builds:

```text
1   32  shared bytecode
32  32  shared bytecode
128 32  shared bytecode
32  0   shared bytecode
32  128 shared bytecode
32  32  fresh  bytecode
32  32  shared mir
```

Build each with `cargo build --release --locked -p kagari-lua-benchmark --example
runtime_storage`, adding `--features diagnostics` only for allocation evidence.
Save each executable before the other build. Run each case in a separate process,
serially with no overlapping builds/tests/timing processes. Each prepares once,
warms three complete lifecycles, then records eleven lifecycles with simultaneously
retained N Runtimes. Report per-phase median/min/max. `shared` reuses the same
engine/provider; `fresh` creates and drops a default engine per Runtime, so its
empty phase includes provider/engine preparation and is not pure Runtime creation.
All loads use the same prepared product. Assert return values and one independent
static increment per installation. The MIR control retains optional verified MIR;
neither case compiles native code. Root backing pointer counts diagnose repeated
module reconstruction; they do not measure all shared bytes or grant authority.

The allocation build reports allocation requests, cumulative requested bytes and
live requested-byte deltas separately. Load/initialize deltas exclude vector/set
buffers reserved before the phase snapshots; release includes freeing the Runtime
and installed-handle vector buffers. Consequently the sum of the four printed
deltas need not be zero. The enclosing warmed lifecycle measurement includes those
buffers and must balance to zero. These are requested Rust heap bytes, not process
RSS, allocator capacity or executable mappings. Shared compilation/source/MIR
backings are held once outside the N-Runtime snapshots and must not be presented
as newly eliminated duplication. Final measurement must separately attribute new
call/binding records and the retained shared products.

For independent preparation/setup and ordinary execution, freeze the existing
original `calls` workload and the source-form suite with:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --workload calls --runs 1 --freeze-baseline target/ms00/calls.exe
uv run python scripts/benchmark_lua.py --source-forms --runs 1 --freeze-baseline target/ms00/forms.exe
uv run python scripts/benchmark_lua.py --workload calls --runs 7 --baseline-executable target/ms00/calls.exe
uv run python scripts/benchmark_lua.py --source-forms --runs 7 --baseline-executable target/ms00/forms.exe
```

The last two are unchanged-image A/A controls before MS01 and paired baseline /
candidate comparisons at MS05. Preserve the driver's matched configuration,
alternating process order, three warmups/eleven samples and checked checksums.
Compare process medians, not all samples as independent observations. Setup rows
separate engine initialization, source/artifact preparation, Runtime creation and
program linking; they are diagnostics, not repeated steady-state compilation.
Old-body redirection performance is measured at MS02/MS05 using the existing
retained-body scenarios after final tables exist; the current-only row cannot
substitute for it. MS00 freezes semantics/owners, not an imaginary legacy fast path.

### Frozen acceptance gates

Apply all gates; a favorable average cannot offset a failed selected workload:

- Structure: exactly one shared immutable module/recipe backing per checked
  product/configuration for both same- and different-domain recipients. No full
  bytecode/export/source/contract-image remap retained per installation. Account
  for bounded local binding/call records and their allocation work explicitly.
- Memory: on the 32-function shared-domain series, reduce marginal load-retained
  requested bytes `(load(128)-load(32))/96` by at least 50% from MS00. Also reduce
  the 32-Runtime load-retained bytes by at least 50% for both the 128-function and
  fresh-domain cases. Optional MIR remains one shared backing, not per Runtime.
- Load: ordinary median load time for the 32-Runtime/32-function shared case and
  the 32-Runtime/128-function case must be at most 75% of MS00. Empty Runtime
  construction for the shared-domain tiny control must not exceed 110% of MS00.
  Preparation and final publication/redirect cost are reported separately.
- Steady-state current calls: use the existing comparison script's maximum
  paired-median candidate/baseline ratio of 1.05 for original `calls` and every
  selected source-form route. No claim of a speedup is required. Independently
  inspect the direct path for absence of reload locks/version tests/retired-table
  lookup/adaptation. Ordinary call overhead remains outside this reload change.
- Uncertainty: the unchanged-image A/A median ratio for each selected current-call
  route must lie within 0.95..1.05 before candidate comparison is interpretable.
  If it does not, keep that route inconclusive and repeat the paired measurement
  in a quiet environment; never increase the gate to absorb noise. For sharing
  setup timing, a result crossing its gate within observed process/lifecycle
  variation needs a second complete serial run; disagreeing runs are inconclusive.
  Memory gates use stable warmed requested-byte counts, not timing or RSS proxies.
- Correctness: preserve all selected authority/type/body/current-inner-call,
  state/effect, cleanup, independent-publication and actual-native assertions.
  A warmed diagnostic lifecycle must release its local allocations. These gates
  do not complete deferred R13/per-body reclamation or certify production capacity.

The 50% memory/25% load-time requirements make the change materially useful against
the observed repeated full-image reconstruction; merely moving the same copies
into another per-player owner cannot pass. They are finite workload requirements,
not predicted results or an unlimited optimization mandate. If a gate fails,
diagnose the owning cost and record the scope decision before further changes.

### Local baseline selection

MS00 runs the core dispatch and publication-guard owner filters, SDK source-update
owner filter, one retained generic closure scenario, one async-facade reload
scenario and actual Cranelift preparation/lifetime scenario. These establish the
specific boundaries being replaced. MS01 adds the sharing/identity, source-free and
scoped-host samples affected by its cutover; MS02 adds the full-stack/direct-latest
table witnesses. Other test-owner seeds above are selected by the phase touching
them, not a request for a full local matrix. Zero matching tests are an error.
Required structure/fmt/workspace all-target Clippy/diff checks apply to MS00's Rust
probe. Complete workspace tests and exhaustive matrices remain CI-only.

### Recorded results

Environment: Windows 10 Pro 10.0.19045, Intel Core i7-10700F (16 logical
processors), 67,017,248 KiB visible RAM; rustc 1.99.1 (58cf8f9c3,
2026-10-13), Cargo 1.99.1 (5f94df478, 2026-08-27), x86_64-pc-windows-msvc.
Workspace release defaults (opt-level 3), default target/build parallelism,
one measurement process/thread at a time. Ordinary and diagnostics builds
use the benchmark package's source/native dependency features; diagnostics
adds the existing allocation observer. No native compilation occurs in the
sharing rows. Builds do not overlap measurements; three warmed lifecycles
precede eleven recorded samples per row.

| N / extra functions / domain / input | Load median [min, max], ms | Load requests | Cumulative load requested bytes | Live load requested bytes | Root module backings |
| --- | --- | --- | --- | --- | --- |
| 1 / 32 / shared / bytecode | 42.954 [40.549, 47.944] | 334,166 | 33,238,323 | 13,335,904 | 1 |
| 32 / 32 / shared / bytecode | 1417.558 [1388.736, 1481.324] | 10,693,504 | 1,063,629,408 | 426,748,928 | 32 |
| 128 / 32 / shared / bytecode | 5756.381 [5633.608, 5835.458] | 42,773,376 | 4,254,507,392 | 1,706,995,712 | 128 |
| 32 / 0 / shared / bytecode | 1360.255 [1329.417, 1433.572] | 10,399,104 | 1,017,197,120 | 410,915,232 | 32 |
| 32 / 128 / shared / bytecode | 1537.477 [1479.487, 1557.957] | 11,393,216 | 1,205,816,608 | 475,100,608 | 32 |
| 32 / 32 / fresh / bytecode | 1432.275 [1407.445, 1483.043] | 10,695,008 | 1,071,255,840 | 428,487,296 | 32 |
| 32 / 32 / shared / mir | 1431.042 [1379.449, 1484.918] | 10,693,184 | 1,063,624,288 | 426,748,928 | 32 |

All load-retained byte/request counts were constant across the eleven samples;
the fresh-engine empty-phase bookkeeping varies with randomized map construction.
The 32-function shared-domain load marginal is exactly **13,335,904 requested
bytes per Runtime** between N=32 and N=128. This is the load phase, not the total
process footprint or a Program-size claim. Shared-domain N=32 initialization adds
269,312 live requested bytes (8,416 per player) for the game-shaped fixture;
static counters return one independently in every installation. The tiny control
has one root backing per recipient too: default-library metadata dominates this
baseline, so source LOC alone would be a misleading denominator.

The frozen numerical limits implied by these rows are: marginal shared load
`<= 6,667,952` requested bytes per Runtime; N=32 scale-128 load `<= 237,550,304`
live requested bytes; N=32 fresh-domain game load `<= 214,243,648`. The two load
time limits are 1,063.169 ms (32 functions) and 1,153.107 ms (128 functions).
The shared tiny-control empty-Runtime median is 0.3239 ms total for N=32, giving
the 0.35629 ms non-regression limit. These are gate arithmetic, not candidate
results. The fresh-domain empty phase takes a median 12,444.492 ms for N=32
because it repeatedly constructs providers/engines; do not attribute that cost
to plain Runtime creation or fold it into the load gate.

Measured source/MIR retention was held once outside the per-recipient snapshots;
the MIR-present and bytecode-only N=32 load-retained counts are both 426,748,928.
This does not quantify the shared capsule/MIR byte size, allocator RSS, high-water
capacity or executable pages. Final owner attribution and old/new coexistence
remain MS04/MS05 requirements; no production capacity or speedup is inferred.

Unchanged-image A/A used seven alternating process pairs for each frozen suite;
both reports verified identical candidate/baseline image hashes and matched
configuration, with no build/test process overlapping execution. All checksums
pass. Every selected paired-median ratio is inside the frozen 0.95..1.05
uncertainty window. These are noise controls, not measured improvements.

| Route | A/A median of pair ratios |
| --- | --- |
| calls | 0.9824 |
| forms_byte_state | 1.0032 |
| forms_capture_cell | 1.0180 |
| forms_concrete_generic | 0.9993 |
| forms_direct | 1.0010 |
| forms_field | 0.9917 |
| forms_helper | 0.9912 |
| forms_host_callback | 1.0121 |
| forms_interface | 1.0163 |
| forms_native | 0.9963 |
| forms_shared_generic | 0.9995 |
| forms_string_calls | 0.9952 |
| forms_string_constants | 0.9716 |

Individual pairs are noisier (for example the field route ranges 0.7701..1.0525);
do not treat individual in-process samples as independent process comparisons.
Original `calls` baseline medians across its seven independently prepared baseline
processes are: engine initialization 687.228 ms, source-to-artifact 842.442 ms,
artifact preparation 441.820 ms, empty Runtime creation 0.0379 ms, program link
39.2163 ms, warmed execution 8.5552 ms per complete workload. Preparation/link
costs are not included in warmed execution. No Cargo/Rust environment overrides
were recorded. These workload/phase definitions are those of the existing driver,
not the sharing probe's direct-source lifecycle.

The final diagnostic probe also passes the enclosing zero-net-byte teardown
assertion for N=1 in shared and fresh domains after three warmups and eleven
samples. Its load-retained counts match the frozen rows (13,335,904 / 13,390,228);
allocation-request counts can vary slightly across fresh processes with randomized
map construction. The larger matrix predates the added enclosing assertion and
provides phase/backing/behavioral evidence; it is not reported as a new execution
of that assertion at every N.

MS00 selected local regression results: **25 passed, zero failed**, with nonzero
matches for every filter:

- `cargo test -p kagari-runtime --lib module::dispatch::tests`: 4 (aliases,
  retained exact body/current inner calls, semantic impl/binder correspondence).
- `cargo test -p kagari-runtime --lib session::execution_sessions::publication`:
  5 (nested scopes, observers/native callbacks, parked work, borrows, foreign owner).
- `cargo test -p kagari --lib runtime::update::tests`: 12 (detached captured source
  behavior, atomic update/error paths and exact publication/source associations).
- `cargo test -p kagari --lib retained_generic_default_and_override_closures_use_current_operations_after_reload`:
  2 (default and explicit override with retained generic environments).
- `cargo test -p kagari --test async_facade relative_typed_members_and_current_reload_workflow`:
  1 (typed async/current reload integration).
- `cargo test -p kagari --lib source_and_encoded_mir_use_real_native_code_after_backend_and_program_drop`:
  1 (source/encoded MIR, actual native selection, backend/program drop and policy fallback).

Required local checkpoint checks pass: `uv run --locked scripts/check_structure.py`
(1,864 Rust files, no violations/exceptions), `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check`.
Both new `.kgr` files pass `kg fmt --check`; documentation links/fences and CRLF
preservation were checked. The initial structure probe identified three newly
qualified `std` paths; importing `env`/`Arc` resolved them before acceptance.
No production cutover, source-free migration or workspace-test pass is claimed.
Complete workspace tests and exhaustive matrices remain CI-only. MS01 owns the
source-free/shared-product and scoped-host samples affected by its implementation.

MS00 freezes identity/API choices, the finite baseline, numeric gates and selected
correctness evidence. Its checkpoint did not implement the shared product,
detached preparation or new call tables; later phase status appears below.


## Progress ledger

Execution is active. MS00 and MS01 are accepted locally with the evidence below.
Next: MS02's shared preparation facts, final call bindings and parked-stack views.
Current-selection ownership and public live-query cutover remain MS03 work.

| Phase | Status | Acceptance evidence / carried errors |
| --- | --- | --- |
| MS00 | Complete locally | Frozen identity/API map, sharing/A/A baseline and gates; 25 selected tests and required local checks pass; full matrices remain CI-only |
| MS01 | Complete locally | Shared immutable Program, detached preparation, local identity admission and native compiler split; selected integration/source-free cases and required local checks pass |
| MS02 | Not started | Ready after MS01 local acceptance |
| MS03 | Not started | Await MS02 |
| MS04 | Not started | Await MS03 |
| MS05 | Not started | Await MS04; final CI status remains separate |

At a checkpoint record the concrete change, changed contract/owner, selected
commands and actual assertions/results. For each failure record its representative
diagnostic, cause, repair owner and whether a selected test was skipped by build
failure. No phase is complete with required errors outstanding. Historical sharing
or current-cell measurements do not satisfy MS00's fresh baseline. Capacity for a
production-sized game remains unproven unless that workload is actually measured.

### MS01 local acceptance — 2026-11-09

The SDK product is `Program`, backed by immutable `Arc<ProgramState>`; the core
live handle is `InstalledProgram`. Source updates and source-free replacement
validation prepare a candidate against an exact immutable base outside Runtime.
Candidates retain a weak base token, not a strong predecessor payload.
`Runtime::replace_program` checks the exact base and cancellation before commit.
`Runtime::program` retrieves the SDK owner attached to installed verified code;
it does not reconstruct compiler inputs from runtime source metadata. Compiler
history preparation reports cancellation through the SDK cancellation boundary.
Obsolete Runtime source-update methods, locators and publication capsule maps
are removed; direct consumers use the new preparation/publication API.

Installed code retains the entire SDK product through `ProgramCodeOwner`.
The product holds ownerless verified code, avoiding an ownership cycle. Existing
static-cell type provenance (deferred R13) can retain the original inputs/MIR as
well as executable code; last-Future completion alone is not a reclamation promise.
The complete-package scenario checks that dropping both recipient Runtimes releases
the original compilation-input owner even while the prepared candidate remains.

`NativeCompiler` exclusively owns the mutable preparation cache, bounded to 4096
revision/function/configuration/ABI decisions per compiler. Revision tokens retain
no code or MIR payload. Immutable `NativeCode` retains finalized products;
`Runtime::install_native` checks exact revision/root and local eligibility. Compiler
drop releases cache references independently of installed entries. This is explicit
preparation, not automatic JIT or expanded native continuation support.

The whole-image normalizer is removed. `DefinitionAdmission` imports the complete
referenced identity/ancestor inventory into a metadata-only local binding. One
candidate-owned admission is reused across members, staging and publication
revalidation. Native/host contracts are checked against actual local registrations;
only registered providers grant shared native-trait authority. Different Runtimes
share original instruction, source, contract and execution backings without a
second executable seal or a reconstructed ID-remapped image.

Generic binders retain immutable lexical tables; transported parents use the
selected destination table. Closed type keys use an immutable accepted installation
prefix while origins retain original expressions/providers/environments. Registry
provenance rejects foreign nominal arguments; identity-free types retain their
portable behavior. Later accepted argument prefixes do not widen an old lexical
source prefix. Static links prepare canonical storage contracts once; reads retain
direct lineage/contract checks without mapping. Persistent compatibility walks
complete schemas and retains the introducing initializer/owner. Associated lookup
facts stay lexical, while interface descriptions and operation-slot indexes use
prepared closed keys. Applied transport preserves supplied closed arguments and
does not issue a portable single-scope verification seal for mixed metadata.

Selected validation and actual evidence:

- `cargo test -p kagari-runtime --lib module::tests` ran 100 matching cases.
  Initial failures identified static bindings keyed by source IDs and internal
  queries assuming source/canonical IDs coincide; the seven failing cases passed
  focused retests. Source-only inventory/cancellation and real native shared-code
  lifetime added two selected cases. Ten further cases covered immutable type
  origins/binders, corrupt static cleanup, current/exact source routes and nominal
  admission. Unchanged passing cases were not rerun for a fresh all-green transcript.
- SDK selection covered twelve source updates, ten native preparations, the real
  Cranelift source/encoded/drop scenario, 22 generic reload cases, five typed-call
  cases and two scoped-call cases. All selected cases passed across focused repair
  rounds. Repairs preserved exact closure/Future bodies and current helper calls
  through repeated publications. The directly affected bound-construction reload
  case passed again after the final associated-output fix.
- The existing complete-package scenario prepares one candidate on a worker thread
  while a recipient can execute its old publication, then distributes that product
  to two Runtimes with independent statics and immutable old source snapshots. The
  strengthened scenario passed; this proves worker transfer, not a throughput claim.
- `cargo run -p kagari --example regenerate_feature_artifact` independently emitted
  the disposable fixture. `cargo test -p kagari --no-default-features --test
  artifact_features --no-run` built the genuine source/native-disabled consumer.
  Exact cases for portable execution, structurally valid installed-binding mismatch
  rejection and forged-signature verification rejection passed. The installed
  mismatch now asserts SDK Load, ModuleValidation, the installed-contract diagnostic
  and root cleanup. The positive fixture exposed lexical associated-output lookup
  keys being confused with closed keys; the final fix passed both artifact execution
  and the affected source-enabled bound-construction regression.
- `cargo check -p kagari --all-targets` passed. Required local acceptance passed:
  `uv run --locked scripts/check_structure.py` (1866 files, zero findings/exceptions),
  `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `git diff --check`. Documentation links/fences/line endings are checked at
  this checkpoint. Intermediate compile and selected-test failures are resolved;
  no broken build is intentionally carried into MS02.

Remaining work is in the existing MS02-MS05 sequence. Temporary full-code copies
in contract comparison, repeated native dependency preparation, instance-sized pure
link/layout facts and per-use upcast type closure are MS02 preparation owners.
Current-call correspondence and the installation current-cell Mutex still exist
until their designated cutovers. Sharing/performance gates remain MS05 work;
functional witnesses are not allocation or speedup measurements. Full-workspace
tests and exhaustive feature/backend/platform matrices remain unrun locally and
belong to CI acceptance.
