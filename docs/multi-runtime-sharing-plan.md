# Multi-Runtime program sharing and per-player memory execution plan

## Status and authority

Status: planned; implementation not started. Requested after `ca3c4f3e` on
2026-10-23 for a game server with one Actor/Runtime per player and a shared game
project potentially containing hundreds of thousands of source lines. HM live
handles and UP00-UP02 compatible publication are now implemented; preserve their
[installation and dispatch contracts](spec/module-activation.md#live-installations-and-call-selection)
when activating. Cancellation of the state snapshot/migration/replacement framework
does not cancel MS or create a prerequisite for it. This request authorizes this
document, not implementation, commits or publication.

The [roadmap](implementation-roadmap.md) owns activation. This plan owns MS00-MS05,
its bounded design decisions, measurements, acceptance and progress ledger. Do not
create a parallel memory/performance queue. [Project goals](project_goal.md) and
[current architecture](architecture.md#reusable-preparation-and-independent-sessions)
remain authoritative for implemented behavior; the target below is not implemented
merely because a product uses Arc or has a shared version token.

## Outcome and finite scope

Prepare one game program and reuse its immutable executable products across many
independent Runtime installations and host worker threads. Each Runtime retains
its own heap, player state, static cells, host borrow state, scheduler and publication
selection. Adding a player must not require copying the complete code image or
repeating program-wide preparation of facts independent of that player.

Measure the complete marginal installation cost, not just the Program handle or
empty Runtime. Reduce identified duplicate code/metadata/link preparation without
moving compilation, identity resolution or synchronization into instruction dispatch.
Do not promise a bytes-per-player budget from source LOC or claim zero overhead
before measurement. Runtime remains Send / !Sync, with one exclusive heap driver.
Sharing immutable code neither shares mutable game state nor updates every player.

In scope:

- Program identity adoption without per-installation whole-code reconstruction.
- Shared immutable code, namespace/type facts and reusable execution/link recipes.
- Small, explicit Runtime-local binding/state records with complete validation.
- A safe cross-thread frozen-product ownership boundary and preparation-cache owner.
- Same-image multi-installation correctness, memory, setup and execution evidence.
- Preservation of implemented installation identities and publication selection.

Out of scope: cross-Runtime GC or object sharing; automatic global updates; Actor
frameworks; new unload/state-migration semantics; source-free temporary-script linking;
artifact pruning; general native expansion; a new identity interner/domain lifetime
project; blanket lazy initialization or sparse-cache rewrites; the full R13 type/code
retention split; SA21 conversion optimizations; unrelated BO/IP performance repairs.

## Evidence and root-cause classification

The following is source inspection, not a new benchmark. Existing executed evidence
in [performance measurements](performance-baseline.md) belongs to its recorded
revision/workload. In particular, the 2026-10-03 tiny-program multi-Runtime figures
are historical, not a current large-project capacity estimate.

| Owner | Current mechanism | Required diagnosis or change |
| --- | --- | --- |
| [PreparedProgram](../crates/kagari/src/program/mod.rs) | Clones share the Rc/RefCell preparation owner, optional MIR and native cache; it is neither Send nor Sync. Runtime publication association separately clones retained MIR by value | Share immutable MIR at the publication-retention boundary as well as the preparation handle; separate frozen products from mutable compiler/cache work, not a mutex around the whole owner |
| [VerifiedProgram and ProgramDescriptor](../crates/kagari-runtime/src/module.rs) | Arc-backed execution preparation is shared; each installation owns its descriptor, layouts and linked modules | Retain existing instruction/frame preparation sharing; attribute remaining code-sized installation records |
| [Identity normalization](../crates/kagari-runtime/src/module/normalization.rs) | A different definition scope with referenced IDs remaps modules/exports into new records while sharing execution and the version token | Separate immutable declaration facts from Runtime binding authority; avoid reconstructing complete images on each load |
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

### Confirmed MIR duplication at SDK publication retention

Inspection on 2026-10-28 identified a concrete gap for the one-Runtime-per-player
requirement. When native input is retained, sharing `PreparedProgram` does not
prevent MIR duplication during SDK load/reload:

```text
One PreparedProgram (Rc-shared state)
  Runtime::load_program / reload_program
    PreparedProgram::retained_inputs()
      self.state.mir.clone()             // Option<VerifiedMirProgram>, not Arc
    Runtime-local Publications::stage()
      retain the copied input under the exact publication token
```

Evidence:

- [SDK load/reload](../crates/kagari/src/runtime.rs) stages `program.retained_inputs()`
  alongside the candidate publication token before committing the installation.
- [RetainedPreparation](../crates/kagari/src/program/inputs.rs) holds
  `compilation: Option<Arc<CompilationInputs>>`, but `mir: Option<VerifiedMirProgram>`.
  The former shares its owner; the latter invokes the MIR program's derived Clone.
- [VerifiedMirProgram](../crates/kagari-mir/src/program.rs) owns a module Vec,
  binding HashMap and other records. [VerifiedMirModule](../crates/kagari-mir/src/verify.rs)
  owns MIR metadata and a Vec of analyses. These are value/container copies, not
  merely one shared-pointer increment; some nested identity owners can still share.
- [Publications](../crates/kagari/src/runtime/update.rs) retains these inputs per
  SDK Runtime/publication. Its `current()` lookup also clones `RetainedPreparation`,
  creating another MIR-copy opportunity independently of initial installation.

The duplication is confirmed structurally, not quantified. It applies when the
prepared input actually contains MIR, including native-enabled artifact preparation
with portable MIR; it is not evidence that bytecode-only players retain MIR or that
every nested allocation is duplicated. Recheck current call paths when MS00 starts.
The preparation owner's comment that a loaded Runtime does not retain its Rc/cache
must not be interpreted as saying the SDK retains no compiler input at all.

Required model: one immutable verified MIR backing per prepared program version,
shared by compatible Runtime publication associations and subsequent preparation
lookups. Adding players must not copy the whole MIR program. Retaining different
old/new versions during reload is legitimate; duplicating one version for each
player is not. Use an audited immutable shared owner (for example Arc at the sealed
program boundary), not an unsafe Send/Sync assertion or a global cache of copies.
If an editing/reverification path needs an owned mutable candidate, keep that copy
explicit and outside ordinary load/lookup. This does not itself settle identity
adoption, cross-thread compiler caches or all native machine-code reuse.

Preserve exact publication-token association, failed-publication rollback, independent
player state and deliberate MIR release when its last legitimate owner disappears.
Do not avoid duplication by silently disabling native-input retention or losing the
ability to compile supported functions later. Reconcile stale sharing/lifetime
comments with actual ownership during implementation. This is additional evidence
inside MS00-MS04, not a new phase or authorization to implement.

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

MS00 must choose a coherent identity strategy against the current implementation:
shared checked definition ownership, or program-local immutable IDs with bounded
installation binding records. Compare the actual consumers before selecting one.
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

The implemented host contract remains: Program/Module handles query their own
installation's latest published contents; stable callable handles select current
implementations without rebinding. Immutable versions and implementation tables may
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

Consulted on 2026-10-23; these are the published Wasmtime API documentation at the
linked site, not a version-pinned source audit or a dependency/version selection:

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
Recheck current primary source/API versions when MS00 freezes implementation choices.

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

### MS00 — Freeze baseline, ownership and publication invariants

- Inspect current implementation/consumers and record actual sharing by product,
  context, host-provider set and Runtime instance. Select fixtures and test owners.
- Run the bounded baseline and attribute code reconstruction, pure-link duplication,
  installation floor and real state; freeze numerical acceptance and uncertainty.
- Choose identity, frozen-product/cache ownership and Send/Sync representation.
  Preserve the deferred JIT delivery boundary above without implementing an automatic
  scheduler in this phase; shared owners must not require execution-time cache locks.
  Record validation relocation, field-by-field shared/local responsibility and
  publication selection against the implemented host/update contracts before
  changing shared owners.

Acceptance: reproducible baseline and finite gates, no unresolved identity/authority
or thread-safety choice, explicit preservation of installation/publication contracts.
No claimed large-game capacity from tiny or synthetic input. If current architecture requires a larger migration,
request that scope decision rather than adding hidden phases.

### MS01 — Share immutable code through identity admission

- Replace per-installation whole-image reconstruction through the selected identity
  boundary. Preserve verification, destination authority and source-free loading.
- Keep already-shared execution preparation and payload-independent version tokens;
  remove superseded normalization owners/adapters in this checkpoint.
- Replace by-value retained MIR duplication with the MS00 shared immutable owner
  across SDK load/reload, publication storage and current-input lookup. Preserve
  native-input availability, verified program/version correspondence and explicit
  editable-candidate boundaries; migrate direct consumers in the same checkpoint.
- Migrate direct consumers coherently; preserve nominal, foreign-runtime, layout,
  native contract and artifact rejection behavior.

Acceptance: same/cross-domain multi-load evidence demonstrates shared backing and
lower attributed retained bytes under MS00 gates; invalid identity inputs fail
before effects. MIR-present same-product installations and repeated preparation
lookups share one verified MIR backing without whole-program clone allocations.
Pair backing-identity checks with attributed allocation evidence, and preserve a
small existing actual-native preparation/execution case; bytecode-only fallback
is not proof of retained-input correctness. No per-instruction replacement for
former load-time validation.

### MS02 — Separate reusable link facts from instance bindings

- Move pure call/argument-location and layout-site recipes to their shared owner.
  Stop rebuilding those recipes by scanning all instructions on every installation.
- Keep local host targets, static/constant state, application authority and borrow
  validation at their actual owners. Account for function-sized local tables.
- Remove superseded duplicate preparations/caches rather than keeping two paths.

Acceptance: scoped object/generic/trait/native cases retain behavior; measured
linking work and private bytes improve as frozen, with no selected warm-call gate
failure and no new cross-player mutable cache.

### MS03 — Deliver cross-thread frozen product reuse

- Implement the MS00 product/preparation-cache split with safe field-derived bounds.
  Keep source analysis and compiler mutation out of shared execution objects.
- Load the same frozen product in independent worker-owned Runtimes; retain checked
  provider compatibility and explicit limits for Runtime-bound captured imports.
- Review lock/refcount placement and native-product reuse; remove obsolete ownership
  wrappers rather than add compatibility aliases or unsafe Send/Sync promises.

Acceptance: shared-product transfer and independent Runtime creation/execution/drop
pass; different host state and wrong providers do not cross-contaminate. Warm calls
never require preparation-cache locks. Preserve source-only/artifact-only support;
validate native product sharing only within currently supported actual-native cases.
Cross-thread acceptance must cover the MIR-present shared owner, not just a
bytecode-only product or same-thread Rc clones; immutable input sharing does not
require sharing mutable backend/compiler/cache state.

### MS04 — Validate player isolation and publication lifetime

- Exercise two independent installations sharing code with distinct players, static
  cells, constants, roots, native borrow state and suspended tasks.
- Update one installation while another stays unchanged; share immutable replacement
  products where valid. Failed publication changes neither installation's state.
- Verify retained MIR is shared within each old/new program version, not across
  incompatible versions. Dropping one player or the original preparation handle
  must not invalidate another installation's supported native preparation; the last
  legitimate retained-input owner must release its MIR backing. Failed publication
  must not leave a new retained input behind.
- Verify selected old computation cleanup and no new accidental lifetime extension.
  Preserve implemented current-call selection and exact retained-body semantics;
  MS does not close R13 without its own evidence.

Acceptance: independent publication and no state leakage, stale/foreign/native
rejection and cancellation/trap cleanup. Run the frozen memory/lifetime measurements;
report any inherited R13 retention separately rather than hiding it in teardown.

### MS05 — Integrated measurement and documentation closeout

- Run the frozen selected source/source-free and multi-worker scenarios, comparing
  complete bytes, setup, warm execution and tail measurements with MS00.
- Review all changed ownership/import/structure boundaries; satisfy local checks.
- Publish lasting ownership and measured limits in architecture/embedding and the
  existing performance report. Record deferred work and CI in the roadmap, then
  retire this plan; Git history retains the execution ledger.

Acceptance: all scoped correctness and frozen performance gates pass, no carried
integration failure, no unmeasured large-project/NUMA/native claims. Existing
host/update contracts remain mandatory regression boundaries, not unimplemented
prerequisites. Full CI/platform/backend acceptance remains separate until actually run.

## Verification and checkpoint policy

Each implementation phase is buildable with affected consumers migrated. No second
semantic path, placeholder success, skipped safety checks or production stubs.
Unexpected failures are recorded with cause and owning repair before proceeding.
Only authorized commits may be made; this document grants no phase-commit authority.

Reuse existing owners, adding cases only for distinct boundaries:

- [Runtime module tests](../crates/kagari-runtime/src/module/tests.rs): verified code
  sharing versus private linkage/constants/statics and foreign-runtime rejection.
- [Engine sessions](../crates/kagari/src/engine/tests/engine_sessions.rs): shared native
  installation, independent runtime state and provider behavior.
- [Public async facade](../crates/kagari/tests/async_facade.rs): ownership, cancellation,
  waking and selected retained work; do not duplicate the scheduler's whole suite.
- Existing generic/layout, reload and source-free artifact owners selected in MS00;
  current tests requiring local metadata IDs may change only with equivalent checked
  identity/authority coverage, never by removing foreign/stale-handle assertions.

Run small affected package/target/test-name selections plus representative directly
related regressions. After repairs rerun failing/affected cases, not every previously
passing check. Full-workspace tests and complete feature/backend/platform matrices
are CI-only, including final acceptance. Required local implementation checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Documentation-only work needs content/link/diff checks, not builds. Use workspace
Cargo profiles, default target and default build parallelism; use uv run python
for Python. No routine unpublished artifact/ABI bump or compatibility reader; retain
meaningful source-free/negative coverage when refreshing affected fixtures.

## Progress ledger

- [x] Plan requested and authored; source evidence and published references recorded.
- [x] 2026-10-28 inspection supplement: confirmed by-value MIR cloning through SDK
  load/reload publication retention and current-input lookup; added shared-MIR
  ownership, allocation, native-availability and lifetime acceptance to MS00-MS04.
  Documentation only; no implementation or new measurement. This finding does not
  mark MIR sharing or any MS implementation phase complete.
- [x] 2026-10-28 JIT direction supplement: background preparation may deliver
  immutable code for Runtime-owner installation at an eligible safe call boundary;
  mutable entry selection remains local and warm execution avoids shared-cache
  locks. Exact boundaries and automatic delivery/scheduling remain deferred, with
  no source changes or performance claims.
- [ ] MS00 — Baseline, numerical gates, identity/product decisions and publication invariants.
- [ ] MS01 — Shared code through checked identity admission.
- [ ] MS02 — Shared link recipes and private instance bindings.
- [ ] MS03 — Safe cross-thread frozen products and preparation-cache ownership.
- [ ] MS04 — Player isolation, independent publication and lifetime validation.
- [ ] MS05 — Integrated acceptance, durable contracts and plan retirement.

No new memory/throughput measurement, build or behavioral test is claimed. No
implementation has begun and no implementation error was introduced by this plan.
Historical measurements do not satisfy MS00. Production-project capacity, numerical
gates, exact frozen-product API and sequencing remain to be established. Record
changed contracts, selected checks/rationale, actual results, failures and pending
CI here at each authorized checkpoint; do not keep the only evidence in ignored logs.
