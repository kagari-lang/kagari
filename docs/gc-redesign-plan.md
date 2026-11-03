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
  output family; the adapter publishes before region exit. Closed u64/Unit
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
5. The proof covers u64, Unit and Native object kinds, with one optional working-
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
- **Performance:** allocation, live scanning and reclamation still lag the
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
