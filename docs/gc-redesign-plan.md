# Standalone kagari-gc and Runtime integration

The standalone library implements typed Rust and fixed dynamic objects, scoped
access, host roots, direct execution-root adapters and incremental collection.
RR03's coordinated Runtime heap-owner/consumer cutover is complete locally.
Default workspace all-target Clippy and selected diagnostic/AOT/serde/JIT consumers
build/lint. Actual Runtime source/provider/cycle/reentry/failure proofs and
committed resource/session/diagnostic observations have selected passing evidence.
RR04 execution-space work and its local frozen call gate are complete. RR05
whole-cost/GC measured acceptance and required CI remain open;
local phase exit is not full integrated acceptance. This document owns the remaining GC
acceptance register and inherited measurements. The
[Runtime execution plan](runtime-refactor-plan.md#order-and-build-boundaries) is the
single coordinated implementation sequence and maps RR phases to the GCR groups
below; do not execute two parallel migration queues. The
[implementation design](gc-implementation-design.md) owns storage, borrowing,
tracing and mutation contracts, and the [crate guide](../crates/kagari-gc/README.md)
owns usage and fixture commands. The
[roadmap](implementation-roadmap.md#gc-library-redesign) locates this work.

RR05's public application-owned managed payload/callback and retained-callback
reload witnesses pass at their existing stdlib integration owners. The updated
real Native-scope integration passes strict-provenance Miri in 342.01 seconds,
including entry admission and cross-thread owner-drop retirement. Matched standalone old-kernel/current
characterization and original owned-root controls now have actual paired results
in the [performance record](performance-baseline.md#rr05-parked-root-admission-and-collector-characterization-macos-2026-11-14).
The latest ordinary root image passes all 24 original root admission/retirement/
collection gates, and the latest core image passes all ten original call gates.
Earlier small-root failures remain recorded as intermediate evidence. Paired original-control
setup/live/empty allocator snapshots pass their bounds, all measured Runtime
teardowns have zero net requested bytes, and repeated empty storage plateaus.
The later identical old/current allocator instrumentation observes transient
requested-byte peaks as well: 336 complete Runtime lifetimes/1,008 churn cycles
pass setup, post-setup and whole-lifetime memory bounds, with zero net teardown.
These are allocator-event owned requests, not allocator internals or RSS.
Standalone publication/collection regressions require diagnosis;
those new characterization rows are not retroactively frozen acceptance gates.
Original Vec-backed integrated recipes still fail source admission under the
authorized NC deferral; no replacement fixture or historical-time denominator
closes them. Historical intermediate ledgers below do not supersede these limits.

RR05's [local exit audit](runtime-refactor-plan.md#rr05-local-exit-and-rr06-handoff)
reconciles the current proofs, original latency/memory gates and these remaining
NC/CI limits. Local GCR04 Runtime work is complete within that authorized scope;
it does not establish the missing integrated collection denominators or CI.
The latest parking transition Miri proof passes in 570.28 seconds. Final call
diagnostics retain actual whole-entry allocations and show no callback-count
growth for raw/typed empty/scalar Native calls or fixed preparation rebuilds.
RR06 must transfer the lasting library/Runtime contracts and each unresolved
acceptance item before retiring this register; the historical proposed-state
paragraphs below are not current implementation descriptions.

## Scope and status

The library is a precise, nonmoving, nongenerational incremental mark-sweep
collector driven exclusively by its owner. It traces its own allocations from
host root leases and a complete driver-supplied execution root source. Semantic
IDs, language typing, execution policy, hot reload and code selection stay in
Runtime; host Rust state stays outside the script heap.

The standalone implementation is usable without language crates. Runtime cutover
has completed its selected local RR03 acceptance; full CI and measured acceptance remain pending. RR03 has
removed Native payload extraction and its invalidation-identity storage from
Runtime, rather than implementing extraction in the new GC.
The new fixed-array storage kernel also owns compact primitive buffers and traced
StoredValue cells, with scoped preparation and publish-before-commit bulk mutation.
Its three focused native and strict-provenance Miri tests pass; actual Runtime
array/nominal/Native acceptance remains incomplete in RR03.
RangeValue now uses the edge-free provider directly and scoped range/Bound
operations consume actual providers. Its existing scalar-domain witness passes
natively and under strict-provenance Miri; Runtime and Bound metadata regressions
still await selected integration runs, not a separate GC acceptance queue.
Native identity and concrete payload providers now share ordinary typed GC edges;
NativePayload selects registration instead of tracing old Value IDs. Fixed-data
and managed-field paths are moved to scoped services. One fixed-data error/panic
and cleanup witness passes natively and under strict-provenance Miri. Actual Native
argument/loan/factory/result, callable/Future/Task and cycle acceptance remain open
in the same RR03 build window; no integrated acceptance follows from that kernel.
Closed physical edge inspection now reuses each provider's scanner through a
read-only EdgeVisitor mode, with caller-owned reusable scratch and no marking/root
mutation. Runtime Task/group output and cold Future/capture/park admission use it
for actual transitive data,
including unknown private payload fields and mutable aliases. Scoped graph-owner
coverage and integrated Task/capture/queue scenarios are separate acceptance:
standalone inspector and marking regressions can pass while Runtime remains
non-building. Candidate allocations now carry Runtime-only immutable birth facts,
and the same physical walk applies candidate-specific mutable ownership and
Future/Task/scope rejection. Direct payload access and successful candidate output
also recheck their actual providers. The GC has no semantic owner table or new
root registration for this policy. Origin backing and traversal costs remain
unmeasured. Candidate/async integration, remaining consumers and publication
regressions remain RR03 obligations; this is not candidate-policy acceptance.
Parking now submits frame ranges in control/data order through one closed walk,
then checks auxiliary roots/dependencies. Runtime contains provider-scanner panics
at its data-inspection boundary before error/quarantine admission. One actual
Native-scratch integration case passes independent owned parking, collection,
retirement and resumed result reclamation. The migrated private-alias/order/panic
unit witnesses now pass, alongside selected data-walk, owned cleanup, publication,
static initialization and interface/application regressions. These default Runtime
samples do not close complete consumer acceptance, collector-phase failure proofs,
Miri or performance gates. See the
[RR03 parking evidence and limits](runtime-refactor-plan.md#rr03-owned-parking-order-and-provider-fault-containment).
Closure and ordinary function factories now hand captures/converted arguments to
actual rooted closure or Queued execution storage. The queue's linear owner survives
conversion cleanup, then retires only after frame publication. Exact rooted-closure
entry borrows the original graph directly, without keeping a redundant local closure
copy. An invocation dependency prefix retains the selected executable environment
through final typed conversion; a factory-returned Future moves directly to Pending.
Pair policies now read genuine roots and construct through actual tuple/enum owners,
preserving optional error origins. Task/group results share nominal and transitive
admission. Selected interpreter interface/closure reload and actual wait/retirement
owners now execute successfully. Complete Task/factory/queue and remaining consumer
acceptance stays open under RR03; no integrated GCR03, footprint, latency or CI
acceptance follows. Current evidence is maintained in the
[RR03 execution ledger](runtime-refactor-plan.md#rr03-selected-wait-parking-and-cleanup-acceptance).
Task result caches now use a concrete once-only provider and Scope capabilities
retain inert status instead of Host control callbacks. Wait results target source
coordinates, and typed argument packs use local source prefixes rather than root
packs. The strengthened source-compaction/alias/scalar witness passes natively
and under strict-provenance Miri. Actual Task, typed conversion, Native/queue and
async transitive validation acceptance remain blocked by the same Runtime consumer
cutover; this kernel result does not establish those contracts.
Borrowed Native adapters now keep protected argument coordinates and share a
closed GC/Host loan interval with poll-only resources, avoiding temporary owning
Opaque handles and per-call NativeType preparation. Their actual Native entry and
mixed error/unwind regressions remain unexecuted at the same build boundary;
zero warm allocation/root/latency acceptance still requires integrated measurement.
Host temporary scopes now own execution-source suffixes and revocable Host marks,
with linear result publication before callee cleanup and explicit owning escape.
Immutable Host path providers trace base and labeled dynamic fields directly;
registry checks traverse actual providers without invoking Host code under a loan.
The existing scope/unwind/cancellation/quarantine and actual-pointer scenarios are
migrated/strengthened, but remain unexecuted while Runtime does not build. Native
pending-return, SDK scoped entry and Host path callback consumers still require
the same coherent RR03 migration; no integrated acceptance follows from these APIs.
External-ID Host callbacks now borrow prepared Native/caller ValueViews and stage
managed results before Host locals retire. Optional Host traces capture bounded
detached data through closed provider reads without managed roots or payload/control
loans across reentry; error outcomes need no heap read after termination. The
existing Host registration, conflict/reentry, temporary/error/managed-return,
source-free binding and trace scenarios are migrated, but remain build-gated and
unexecuted. CLI/Web/AOT output consumers now borrow UTF-8 without old heap IDs;
Web retains its bounded-copy policy. Candidate allocation provenance/transitive
payload checks and async graph admission remain required RR03 repairs; the Host
adapter does not substitute nominal or storable matching for those checks.
Serde now consumes scoped providers and source-owned streaming/snapshot suffixes,
without old collector accessors or RootSet snapshots. Existing alias/GC/unwind/cycle
regressions use declared public mutable fields and genuine ownership; they and the
conversion-budget boundary remain unexecuted at the Runtime build boundary. This
consumer migration does not close frame/metadata, Native invocation or GCR03 gates.
Native input conversion now shares caller-source/prepared/scalar ArgumentValues,
and pending return protection is an execution-local cell owned by one Native mark.
The source-pressure witness now appends a Native result behind the captured root
frontier, removes its old root and checks survival/reclamation. Nested result-cell
and conversion retirement assertions pass natively; the smaller suffix witness
also passes strict-provenance Miri. Actual Native callback/Host/frame/backend/result
adapter integration is still non-building under RR03 and remains unaccepted.
Native shared capability aliases and ValueHandle now consume actual providers
and source coordinates rather than old collector IDs/RootSets. Payload reads use
the retained physical codec; operation guards reopen the source after reentry
and clean up even after cancellation. Inferred managed outputs carry their call
lifetime instead of the removed static Value/NativeOutput protocol. Existing Host
alias and Native registration cases are migrated, with no new test binary. They
and actual reentry/unwind/root acceptance remain unexecuted while Runtime does not
build; this does not close GCR03, warm allocation or footprint gates.
Raw Native enum/array allocation, enum payload reads and origin forwarding now
transfer results from short conversion suffixes into the enclosing Native source.
Public named-enum construction uses genuine Host roots. These services reuse
physical source compaction and actual aggregate providers; their consumers,
complete scoped matcher and integrated Native/Host/frame tests still require the
same RR03 build repair. Existing physical-kernel evidence is reused, not promoted
to a new Runtime, Miri or performance pass.
Native body admission now uses an explicit current/execution/Host revision origin,
checks exact program identity before growth and protects the body through source
dependencies. Same-execution children may borrow the admitted caller prefix;
independent sources copy an edge. Staged publication uses the same physical Host
root authentication. The existing stale-publication scenario additionally checks
old-body retention, current/retained mismatch rejection, root registrations and
reclamation after source retirement. It remains unexecuted at the Runtime build
boundary; frame/backend/Host and environment/provider integration are still open.
Host ProgramLease now owns an actual revision root; clones and authenticated
retention-kind changes share its registration. Execution sessions retain their
body through one source dependency and keep only a diagnostic lifetime token.
The Native source record exposes its innermost body coordinate for exact-origin
admission; full typed/program authentication remains in the closed Runtime scope.
Fresh ordinary selection excludes that generic candidate and uses current
publication unless a matching frame/root already protects the selected body.
The extended source-pressure witness passes natively and under strict-provenance
Miri: a newly entered execution beyond the captured root frontier keeps its body
after every old source is retired, and dropping its last dependency permits
reclamation. Nested Native retirement restores the parent's body coordinate.
Actual ProgramLease/session/backend/hot-reload owner tests remain unexecuted at
the Runtime build boundary. This closes no GCR03 or integrated performance gate.
Executable metadata now has actual environment, operation-group and application
providers with traced direct edges resolved from protected source graphs. Runtime
environment admission returns facts paired with an actual dependency; a monotonic
identity supply replaces its old record table and cannot recover Gc/Root by ID.
Physical source owner/slot/generation admission remains separate. Construction uses the
existing mutation/barrier protocol for cancellable, scanned adjacency prefixes.
The environment owner tests are migrated to real Runtime sources and Host roots,
including metadata-only collection pressure, but remain unexecuted. Generic
transport, dictionary/application producers, frame/callable consumers and old
store/walk deletion still require RR03 integration; no new native/Miri/CI or
performance acceptance follows from these non-building consumers.
Dictionary admission and receiver-table production now use explicit execution
sources; a monotonic identity supply replaces the strong dictionary record store.
Environment member lookup/transport reads pure snapshots from authenticated actual
graphs, with no ID-to-heap accessors. Conversion marks retire typed dependencies
and reject crossing a live child call before mutation. The strengthened actual-source
conversion witness passes natively and under strict-provenance Miri, and the affected
caller-bank publication witness passes natively. These are source-kernel results.
Dictionary/transport Runtime scenarios remain unexecuted at the same compiler
boundary; application/revision-cycle, full preparation cleanup and complete metadata
accounting remain RR03 owners. No integrated or performance gate is closed.
Application admission now publishes semantic/admitted record kinds through actual
execution sources and genuine escaping Host roots. Runtime's shared nonce supply
replaces the three metadata record stores without granting ID-based GC access.
Structural adapters and witness/construction preparation pass their actual source
candidates along the same preparation suffix. Bounded witness caches reuse facts
only with currently protected dictionaries, reconstruct missing optional backing,
and preserve errors for invalid original sources. Application lifecycle, retained
body and cache expiry tests are migrated but have not run at the Runtime compiler
boundary. Outer call/Host/frame/source propagation, other caches, cycles and total
metadata accounting remain RR03 requirements; no GCR03 or performance gate closes.
Shared-call preparation now publishes source-owned environments and checks its
bounded pure-fact cache against actual protected nodes. Source copying retains an
existing environment in the destination suffix without creating another object
or Host root; stale original inputs remain errors rather than cache misses. Host
entry preparation retires conversion scratch after retaining its escaping optional
environment root, alongside the selected body's actual ProgramLease. Same-version
relationship preparation shares that environment lease. Validation-only paths do
not prepare dictionary maps. Environment/shared-cache and Host route regressions
remain unexecuted at the Runtime compiler boundary. Native promotion now retains
its actual environment before scratch retirement. Native applications and stored
callback relationships use actual ApplicationObject edges, including a typed edge
from the Native header; no old Native selection table authorizes lookup. Closed
Ready signature facts prepare once per immutable linked record, while ordinary
body selection stays current. Fixed data and branded traced Native result
construction publish into the call's source suffix. The new independent-source
retirement/GC witness has not executed. Exact-origin frame transfer, general Native
type/revision backing, other providers and complete accounting remain RR03 requirements;
no new Miri, performance or integrated GC acceptance is claimed.
Frame banks now use linear suffix marks in the authoritative execution source;
ResourceState.frame_values and the independent window directory are removed.
Source retirement checks frame/Native nesting, and bounded initialization rolls
back every new bank length on polling rejection. Unit prepared operands allocate
no payload cells, including preparation-budget fallback. Four actual source tests
and eight actual allocation/layout selections pass; the nested frame witness also
passes strict-provenance Miri. Cold source inspection has a passing borrow-escape
compile-fail doctest. Diagnostic accounting reports source buffers/control and
collector/root-channel metadata separately. Runtime admission/cursor/return and
metadata/provider consumers still do not build, and their existing owner tests
remain connected for migration. Neither GCR03 nor memory/latency acceptance closes
from this isolated actual-module evidence; full footprint and CI remain unmeasured.
Cold Frame observations now return source coordinates/scalars tied publicly to
frame/Runtime loans; no stored edge or Host lease escapes inspection. Exclusive
FrameStack slot write/copy uses paired source publication before completion-PC
commit. The actual source witness covers cold queries and reclamation, and the
physical Unit operand witness checks zero storage, neighbor preservation and
expired execution rejection; both pass natively and under strict-provenance Miri.
Public FrameStack/debug/backend and managed metadata integration still require the
RR03 build repair. Their new lifetime rustdoc and full Runtime checks are unrun;
these kernel results do not close GCR03 or performance acceptance.
The interpreter cursor now opens the authoritative banks and exact revision under
one paired scope; scalar segments borrow their payload/init slices once. Literal,
field/index and sealed Native operations use actual providers, with full nominal
matching still an explicit missing owner. Non-scalar return snapshots occupy the
existing local bank, with pointer-free full-execution/index/sequence records for
current-frame/stale/reuse authentication. Marks retire those records; they create
no second value store or per-call Window directory. Source diagnostics account
for the backing (ExecutionStorage=232 bytes; ReturnValue=48 bytes on the measured
aarch64 harness; NativeSourceMark is now 64 bytes including its return suffix).
Ordinary returns publish into the caller before retirement; local-stack Native
reentry transfers its snapshot into the parent source prefix without a Host lease.
Native and conversion marks also retire these records, including preparation
records invalidated by result preservation/pack relocation, so reusing a local
coordinate cannot revive an old packet. Interpreter completion now consumes the
result before its activation/source retires: typed consumers decode, scoped
consumers publish to ResultPlace, and escaping Host consumers explicitly retain
once. ScriptInvoker uses the same argument pack/result publication protocol;
compiled scalar ABI results are inline retained carriers with no root registration.
Three return-owner tests and one ABI-domain test pass natively and under strict-
provenance Miri. These include Host activation retirement, no duplicate lease,
foreign-heap rejection, independent object/root counts and exact values after
collection/cleanup. Four unchanged source regressions retain their passing evidence.
Full matcher/adaptation/static/await/primitive/remaining Native consumers/metadata
and public body-retention/old ConstantPool consumers still require RR03 build repair. Physical
kernel evidence does not establish full Runtime, footprint, speed or CI acceptance.
The active plan records exact carried diagnostics and proof limits.
Actual ClosureObject and InterfaceObject providers have been added to the same
unaccepted Runtime cutover. Closure facts contain no captured Value pack; captures,
exact retained bodies and environments are traced edges. Ordinary named routes
select current code without retaining their original mutable revision. Interface
metadata lookup follows only its fixed dependencies/parent views, independently of
receiver tracing. The new closure lifecycle scenario checks independent-source
retention, named-current versus exact-body selection and old revision release, but
has not executed while Runtime does not build. Description preparation and parent
publication now consume actual InterfaceObject metadata/receiver sources. Immutable
descriptions moved out of the collector; weak cache hits cannot recover retired
binders and include operation/construction evidence. Declared/bound boxing, prepared
upcast and linked Native result boxing publish through the same source-based service.
Parent edges validate the exact receiver and checked relationship before a shaded,
fixed-position commit. Their sharing/failure/reload/generic-lifecycle owner cases
remain unexecuted at the same Runtime build boundary. Host method resolution,
semantic application and exact admission now use actual interface/application
leases and immutable facts, with no collector getter or root metadata setter.
Receiver dictionaries moved from shared description IDs into an optional scanned
InterfaceObject edge; actual graph copies preserve source retirement checks.
The admitted computation now owns the actual relationship/application/body graph;
Frame handoff rebinds that computation into the frame source and reads its receiver
directly. Native calls and returns use shared pack/result publication, and Interface
factories use the existing Queued bank with prefix-owned completion sources.
Escaping Interface metadata and generic member witnesses retain actual lexical
leases/environments. Their dictionary, frame handoff, exact-body and queue lifecycle
owner scenarios remain unexecuted. Bytecode interface calls now consume actual
caller sources; staged revisions own their finite closed-call dictionaries, and
FrameMark adoption bounds per-call preparation lifetime. Scoped call caches retain
pure facts and authenticate actual backing. Since admitted computations also own
receiver data, parking now inspects their typed dependency cells rather than
skipping every application as metadata. The new Host-receiver negative scenario
uses normal nominal boxing/admission and remains unexecuted. Candidate provenance,
remaining backend/SDK/reflection consumers and obsolete collector/store removal
remain required. Metadata publication now preserves the whole provider's
charge instead of replacing it with adjacency-only accounting; complete metadata
backing and whole-lifecycle footprint measurement remain open.

Scoped type matching now reads these actual providers and preserves lexical child
type scopes. Closed polling returns inert execution errors; Host/matcher/metadata
owners record cancellation only after source/control loans end. The existing sticky
cancellation scenario now checks this ordering, and remains unexecuted. Task output
now uses actual transitive data inspection. Cold providers, linear single/pair claims,
queued-claim rechecks, Script resume and Native/Task wait completion now consume actual
sources. Two fixed Pending cells protect each execution's awaitable/completion; source
snapshots protect frameless async returns before those cells clear. Submission and
pair handoff release captured data after the next genuine owner is installed. These
consumers and migrated owner tests remain unexecuted in the non-building Runtime;
Interface factory/dispatch and remaining async consumers still require migration
and cannot use top-level storability as a substitute. The focused
standalone inspector has strict-provenance Miri evidence, independently of still
missing integrated GCR03, Runtime, CI and performance acceptance.

Selected Native/retained Host frame entry now passes its sealed actual revision and
environment sources through FrameDispatch. Frame admission uses FrameSlots::allocate
over the authoritative banks; its environment coordinate is published under that
frame mark, rather than borrowed from a conversion suffix that is about to retire.
The existing environment-source lifecycle scenario now includes independent frame
publication, original-source retirement and frame-coordinate expiration without
extra Host roots. It remains unexecuted at the Runtime build boundary. Candidate
provenance checks and old closure/interface/Future producers still block admission;
no full frame, collector-cutover or end-to-end performance acceptance is claimed.

Actual closure frame/Native handoff now uses SelectedClosure and a source-backed
capture prefix. RevisionSource::Closure admits the supplying exact body edge;
ordinary named calls select current code. StoredCallable is now a concrete
single-edge GcObject constructed under NativeInit, with an explicit accounted
unit. Reading a stored child publishes its closure into the Native suffix before
payload loans end. Host conversion freezes named relationships only for genuine
retained computations; prepared Host roots/leases and the added stored provider
node still need whole-footprint measurement. Environment transport reads immutable
source identity facts while separately validating its actual graph and selected
live body, permitting scalar-generic named calls after old-body reclamation.
The migrated stored-provider/frame transfer and Host teardown scenarios plus the
new scalar-generic/frozen-computation scenario remain unexecuted while Runtime
does not build. Old task factories, async data-graph admission, native providers
and other consumers still require integration. No GCR03, Miri, CI or performance
gate is closed by these source changes.
The external artifact provider now constructs a real GcObject with a StoredValue
field and an Edge<StoredCallable>. NativeInit::value admits the original source;
public low-level StoredValue initialization/read/scan retain their existing unsafe
provider obligations. Native value/callback projection shares physical provider
admission and publishes the child before releasing loans; scalar field projection
adds no local cell. The existing stored-callback integration case now constructs
the actual provider and collects before invocation. Those Runtime-dependent tests
remain unexecuted, including the independent source-free consumer; no public API
or memory-safety acceptance is inferred from partial typechecking.

Cold interpreter tuple/array/repeat/struct/enum creation and enum pattern/payload
reads now use the actual providers and frame slots. Original inputs are admitted
before layout preparation, and incoming publication precedes destination/PC commit.
Host struct/enum constructors consume genuine roots, preserve initializer ScriptTrap
classes and do not require executable-body retention for immutable layout facts.
Enum diagnostic capture occurs after semantic field admission with all source/control
loans released. Temporary location/scoped-field vectors and Host output roots remain
real costs; no allocation or speedup gate is closed. Four existing interpreter
scenarios and the Host initializer/old-layout lifecycle owners are migrated, but
remain unexecuted while Runtime fails to build. Candidate allocation provenance,
async graphs and the other recorded RR03 consumers still require integration.

Capture-cell and range instruction consumers now read actual providers and publish
directly into the frame source. Cold instruction admission is shared with aggregate
handlers; zero/one/two-input location storage stays inline. Cell mutation preserves
input/target trap ordering and publication before the fixed field commit. Its charge
now includes the one logical stored field, and the existing physical lifecycle case
expects that unit to remain until the cell/cycle is retired. Range Native bindings
reuse the prepared type signature and canonical conversion/projection protocol,
without EnumSymbols preparation or the old collector. Updated captured-cell/GC and
wide/full-range bounds scenarios remain unexecuted at the Runtime build boundary.
Static readiness, initialization begin/commit/cycle failure, return/cleanup and
Host snapshots now follow actual managed cells. Attempts share only edge-free
completion facts; a dirty signal requests closed settlement before collection or
source retirement, without changing tracing from shared state. The exact initializer
body is admitted from the actual recipe edge and retired after value publication;
the parent does not retain a redundant recipe dependency. Public ready snapshots
own genuine Host roots. Legacy mutable static state has been removed from the pure
staging directory. These paths and their migrated real Runtime tests remain in the
RR03 non-building window; no initializer/collector integration acceptance is claimed.

Canonical scalar instructions now use protected frame slots and the actual scoped
comparison providers. They publish results directly without legacy Value packs or
comparison-local Host roots. Numeric kernels consume inline call values and no
longer own a heap or allocate an overflowing tuple; the installed integer Native
binding publishes that tuple through the shared conversion/provider path. Closed
comparison/hash/preview and tuple loops defer cancellation/trace admission until
source/payload loans end. Real numeric/native/source-free lifecycle tests remain
unexecuted at the same RR03 Runtime build boundary; performance is unmeasured.

Host paths now construct and inspect actual HostPathObject providers. Callback
receiver/index/RHS snapshots use the Host scope's execution suffix; reads publish
to the caller without intermediate Host result roots. Only durable dirty records
retain genuine roots for base/arguments/old/new values. Detached ledger snapshots
share those registrations and keep the actual base graph alive independently.
The old detached path handle and HostRegistry's foreign gc_roots expansion are
removed. Borrowed callback results still reject before escape, Host leases remain
scoped and commit/ledger publication stay atomic under the existing commit guard.
Remaining old collector, external Host adapter and path/native lifecycle consumers
still block RR03 integration. No path GC/performance acceptance is inferred.

Generated Host field snapshots/decoders now use source-backed ValueViews. Field
reads publish to caller ResultPlace before conversion/loan retirement; nested
struct views remain revocable Host capabilities rather than escaping Rust
references. Ready/cold script fields share one actual-provider implementation,
with scalar carrier/domain admission before payload erasure and no old frame-value
table or temporary field RootSet. The existing generated-field lifecycle test now
uses actual source destinations and checks unchanged Host root registrations,
collection survival, disjoint writes and alias restrictions. The added scalar
boundary case distinguishes unavailable/wrong carriers from zero. Neither test
executes at the Runtime compiler boundary; complete scoped matching and integrated
frame/provider validation remain RR03 requirements, with no new Miri/performance
or GCR03 acceptance claim.
Value operations now consume scoped providers rather than old heap snapshots;
primitive output uses ResultPlace before conversion cleanup. Comparison scratch
holds only branded handles during collection-free access; no per-member Root is
introduced. Physical preview labels delegate to the existing pointer carrier,
without a new directory/header field, and grant no persistent identity/retention.
Five selected Runtime value-semantics tests now pass: Char behavior, nominal enum
identity, NaN/unsupported object comparison, deep short-circuit comparison/format
limits and bounded String output/escaping. Hash/join and remaining value-operation
integration remain pending. The active execution plan records source mechanisms,
the outstanding hash progress obligation and exact current compiler/check results.
Prepared Native callback/entry results now use a borrowed frame ResultPlace as
well as source-cell places. The destination writes the authoritative caller bank
before the Native mark retires; discard validates its source without adding a local
cell or Host root. A focused actual-source/provider publication test passes natively
and under strict-provenance Miri via the bounded ordinary-module harness. It checks
collection survival, scalar mismatch without overwrite, expired discarded-source
rejection and reclamation after frame retirement. This excludes the non-building
Runtime wrapper, actual Native callback, managed application and full type matcher;
those integrated GCR03 gates remain open in RR03.

Concurrent/moving collectors, foreign graph stores or Domain bindings, generic
finalizers/resurrection, shared heaps across Runtimes, derive tooling and new
container libraries are outside this scope. The current fixed dynamic API does
not enforce language nominal types or implement all Runtime storage needs.

| Acceptance group | Current state and remaining exit condition |
| --- | --- |
| GCR00: contract closure | Typed/fixed dynamic access, typed schema keys, branded handles, provider mutation, cursor repair and direct root-source contracts are implemented. The old extraction API is removed. Close Runtime representation requirements and comparable performance gates. General safe custom-object construction remains an explicitly reviewed provider responsibility. |
| GCR01: standalone heap | Allocation, roots, mutation, resumable collection, metadata reclamation and opt-in safepoints are implemented. Complete the remaining representation/diagnostic requirements selected for integration; do not invent placeholder APIs. |
| GCR02: Runtime object-model proof | RR01 records/source contracts now have selected actual Runtime publication, reentry, cycle and retained-body/current-inner-call proofs at RR03 exit. Fake Native services and simulated revision/static graphs are removed; independent carrier-bit/destructor oracles remain at the value owner. Actual frame/control/source footprint and paired cost acceptance continue in RR04/RR05. |
| GCR03: coherent Runtime cutover | Complete locally under RR03: one production RootDriver owns execution storage; genuine Host/publication/revision roots and all actual providers share the graph. The replaced collector/root/metadata stores and Native extraction APIs are removed. Default workspace Clippy and selected diagnostic/AOT/serde/JIT consumers build/lint; selected actual lifecycle/fault/source-free proofs pass. Windows/full feature/backend CI and measured acceptance remain distinct GCR04 obligations. |
| GCR04: integration and performance closure | Complete selected integration regressions, paired standalone/integrated gates and CI; reconcile inherited obligations at their existing owners. |
| GCR05: documentation and cleanup | Move lasting behavior to architecture/spec/authoring owners, remove obsolete paths, transfer unresolved work and retire this plan. |

GCR01 and standalone GCR02 fixtures can coexist with Runtime because they own
separate heaps. RR00/RR01 of the execution plan list affected crates and any
finite temporarily non-building migration boundary, with repair inside RR03.
GCR03 acceptance closes at that coherent cutover. No
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

### RR03 rooted entry and Source progress

Host owned submission now copies genuine root-slice inputs directly into the first
callee's authoritative source. The caller may drop those roots after submission;
parked execution owns their managed fields. Interface Host calls use one argument
prefix and the admitted method's actual receiver graph. Callable Source observation
reads actual providers without claiming Futures or retaining them in diagnostic
tokens. Native task inputs and mutable binding/lifecycle boundaries no longer read
the obsolete collector. The existing owned-drive and Source regressions are migrated
and strengthened but still fail to reach execution because the affected Runtime
graph remains non-building. Reflection now publishes actual scoped provider outputs
directly to caller banks; Host reflection retains explicit escaping roots. Requested
debug binding snapshots are captured by Runtime before readonly observation, share
alias leases for that event and release temporary roots on callback unwind. Detached
diagnostic records keep independent genuine leases. The
[Runtime plan](runtime-refactor-plan.md#rr03-reflection-publication-and-requested-diagnostic-roots-2026-11-13)
owns the pinned tooling comparison and selected unexecuted lifecycle regressions.
Remaining raw consumers, collector removal and full integrated GCR acceptance remain
RR03 obligations; this source migration supplies no Runtime or GCR acceptance.


Host and interpreter constants now use the same actual RevisionState fields.
Host reads borrow an explicit current/admitted/retained origin and retain escaping
results, while internal loads publish to frame banks. The duplicate legacy
ConstantPool and unused frame mutation-root vector are removed. Actual Native
loan/source/Running-frame parking restrictions remain. The
[Runtime plan](runtime-refactor-plan.md#rr03-shared-revision-constants-and-obsolete-frame-guards-2026-11-13)
owns the migrated existing constant/foreign-frame/driver scenarios and their current
build gate. Old ProgramGraph/MetadataTrace and collector consumers still require
removal before integrated GCR03 acceptance; no acceptance group is closed here.


Prepared nominal matching now authenticates genuine roots/source views and reads
actual struct/enum providers, retaining only immutable admission facts. The existing
nominal/type-origin tests are migrated to actual managed dictionaries/environments
and source retirement, but remain build-gated with the rest of Runtime. The
[Runtime plan](runtime-refactor-plan.md#rr03-actual-prepared-nominal-matching-2026-11-13)
records that evidence; no GCR acceptance or performance gate is closed.


Runtime ownership/Host-boundary and Native-revision regressions now use genuine
roots and actual providers rather than the obsolete collector's naked IDs/RootSets.
Live Host capabilities are created in the consuming Runtime and still expire at
scope retirement independently of wrapper roots. Six focused current-core root,
foreign/shutdown/thread-release and execution-source checks pass locally; migrated
Runtime cases remain compile-gated. The
[Runtime plan](runtime-refactor-plan.md#rr03-runtime-root-host-boundary-and-native-revision-consumers-2026-11-13)
owns the finite old/current coverage mapping, exact commands and remaining audit.
No integrated GCR03, Miri, performance or CI acceptance is inferred.


The obsolete Runtime GcHeap, raw Value/ID transport, central RootSet/InterfaceStore
and MetadataTrace/ProgramGraph owners are now removed. Default Runtime production
builds against the sole actual provider/source graph; existing array and source-free
offline Native examples execute. The latter exposed and now verifies matching
candidate-body admission rather than reopening current publication. Seven current
core failure/history tests and four selected Runtime lifetime/NativeData doctests
pass locally. Remaining Runtime tests, stdlib/SDK/backend and diagnostic consumers
still block integrated GCR03 acceptance. The
[Runtime plan](runtime-refactor-plan.md#rr03-obsolete-collector-and-metadata-owner-removal-open-integration)
owns coverage consolidation, exact commands and limitations; no GCR group,
performance target or full CI acceptance is closed by this library cut.


Standard-provider production consumers now use actual sources. Lazy adapters own
resumably scanned source/callback edges, and nested Native regions reclaim each
step's scratch while a reduction replaces one protected state cell. A selected
actual-call test verifies collection between steps, alias reads, array child/bounds,
shared result staging, error/unwind/cancellation cleanup and fresh-context reuse;
a compile-fail witness verifies nonescape. Selected web owned-output and actual
Cranelift/thread-lifetime contracts also execute. The Host-object example verifies
that a queued old result can read/write through its real retained layout lease after
reload, while ordinary calls continue selecting current implementations. These
samples do not close GCR groups or substitute for the outstanding Runtime/stdlib/
SDK/benchmark/diagnostic consumers and accounting/measurement gates. The
[Runtime plan](runtime-refactor-plan.md#rr03-standard-providers-bounded-native-scratch-and-retained-object-access)
owns commands, initially failing witnesses, repairs and local/CI limits.


The source-owned frame test port now replaces the removed window directory's
scalar/argument and managed-lifetime witnesses. A Native case now checks selected
environments through an actual active application after pure-cache eviction;
Native stored-selection and JIT/fallback fixtures also use actual sources and
owning results. These authored cases remain unexecuted while the Runtime unit target fails
to build. The
[Runtime plan](runtime-refactor-plan.md#rr03-source-owned-frame-and-nativejit-test-consumers-open-integration)
records the old/current coverage mapping, removed-model-only assertions and the
new nested/independent storage controls. The controls are unrun and cannot serve
as matched WINDOW speedup evidence or close measurement/Runtime acceptance gates.


The independent stdlib native_schema target now supplies actual public-carrier
DTO/Serde evidence: child snapshot retention through alias mutation/GC/unwind,
recursive-enum cycle rejection, Char-domain checks, concrete opaque storage identity,
source-free installed schemas and bounded/failed conversion cleanup. Its 16 cases
have passing local evidence. This does not close the remaining stdlib/SDK consumer
build window or establish root-free conversion, complete GCR03, Miri, timing or CI
acceptance. See the
[RR03 schema/consumer evidence](runtime-refactor-plan.md#rr03-standard-library-schemas-and-basic-execution-consumers).


Selected Runtime formatting/comparison and Opaque::read loan proofs also have
actual local evidence: five value-semantics cases and two compile-fail rustdoc
cases pass. The migrated stdlib cyclic graph, transitive program/environment,
provider-fault and opaque construction tests remain unexecuted behind its consumer
build errors. Changed snapshots own facts only; actual links use script entries,
NativeInit and the sole source graph. Fault assertions distinguish stopped sweeping
from eventual shutdown disposal. This does not close GCR03, account/session or
performance gates. The
[RR03 cycle/payload evidence](runtime-refactor-plan.md#rr03-gc-cycles-provider-faults-and-opaque-construction-consumers)
records coverage migration, actual commands and the remaining build boundary.


Three focused Runtime array-conversion cases also pass: foreign/bulk rejection
without partial writes, copy-within bounds and preparation cleanup, and repeat
capacity rejection before publication with wide unsigned input. Managed-field,
recursive owned/retained conversion and collection iterator/sort consumers have
been migrated to source views and real Host ownership, but remain unexecuted
behind the stdlib unit build boundary. Immediate child snapshots are preserved;
post-collection retained charge is distinguished from transient replacement cost.
The [RR03 conversion/field evidence](runtime-refactor-plan.md#rr03-managed-fields-conversion-packs-and-collection-observers)
records scope, oracles and pending integration/accounting/diagnostic acceptance.

Typed Native, external Host adapter and static-state consumers now use source views,
genuine Host roots and explicit mutable snapshot services. The readonly initializer
observer uses admitted immutable frame facts. These scenarios remain unexecuted:
the stdlib library-test target still has 317 diagnostics, and workspace Clippy stops
at other unmigrated consumers. The
[RR03 Host/static evidence](runtime-refactor-plan.md#rr03-typed-native-external-host-and-static-state-consumers)
records preserved effects, foreign-owner rejection and publication-aware cleanup
baselines; it does not close integration, accounting, diagnostics or performance.

The generic candidate-array contract now executes at Runtime's array-operation
owner and passes: a preexisting protected view reaches each candidate read/write
guard, local candidate data remains usable through collection, and the old array
is unchanged and writable after session exit. The equivalent stdlib scenario and
sole-use source fixture are removed. Reentry/owned execution/interface-method/
Native-boxing stdlib consumers have migrated but remain unexecuted behind 212
unit build diagnostics. The
[RR03 execution-consumer evidence](runtime-refactor-plan.md#rr03-reentry-owned-execution-and-prepared-function-consumers)
records the actual command, retained oracles and unresolved consumer/accounting/
diagnostic gates. This does not close integrated GCR03 or performance acceptance.

Five provider-independent conformance scenarios now pass in Runtime, including
collected debugger binding/watch inspection through protected internal access.
Numeric PartialOrd and array-length scenarios remain at their actual stdlib owner.
Enum/Host-object/generic-application and storage graph consumers have migrated but
remain unexecuted behind 153 stdlib diagnostics. Graph assertions now use actual
identity, retained contents and collected narrowing/teardown, rather than rebuilding
the removed collector's traversal list. The
[RR03 enum/storage evidence](runtime-refactor-plan.md#rr03-enum-storage-and-host-object-consumers)
records preserved oracles, real owner dependencies and remaining integration/
accounting/diagnostic work. No complete GC or performance acceptance is inferred.

The actual Runtime Native Host owner now passes nested shared/unique borrow
protection through collection, Owned/retained escape rejection and scope expiry,
with exactly three source-protected objects reclaimed after teardown. Native
polling also passes with cancellation after three completed iterations and
post-call termination preserved despite a swallowed poll error. Composite Host
signature and logging/control stdlib consumers have migrated but remain unexecuted
behind 116 unit diagnostics; logging needs the real standard String representation
declaration, so no replacement provider is introduced in Runtime tests. The
[RR03 Host/control evidence](runtime-refactor-plan.md#rr03-complete-host-signatures-and-native-control-consumers)
records current owner boundaries, preserved oracles and remaining build/accounting/
diagnostic gates. These two local cases do not close integrated GCR03 or performance.

The no-standard core-entry contract now passes at Runtime's conformance owner:
source-free Unit entry and an explicit application Native echo return correctly,
and collected objects/provider units/roots match publication. Function/callback,
resource-termination, reflection, retained-interface and shared-application stdlib
consumers have migrated but remain unexecuted behind 68 unit build diagnostics.
Array callbacks keep immediate reads; ordinary/prepared/closure selection and actual
publication-owned preparation retain their independent lifecycle assertions. The
[RR03 function-lifecycle evidence](runtime-refactor-plan.md#rr03-function-callback-and-reflection-lifecycle-consumers)
distinguishes retained cleanup from transient costs and the still-open session
attribution, diagnostic and integration gates. No full GC acceptance is inferred.


The default stdlib unit build boundary is now repaired. Existing method/application,
callback, Host signature/nominal, scoped conversion, floating-carrier and Native
control oracles execute, with affected failure reruns recorded in the
[RR03 build recovery evidence](runtime-refactor-plan.md#rr03-consumer-build-recovery-and-actual-admission-regressions).
Public converted parameter packs now reject wrong scalar representations as input
errors while verified frame banks keep fault containment. Object binding reuses
scoped identity equivalence before its existing supplying-layout checks. Actual
private Host tokens cover both nominal mismatch and nested Owned escape, and
normal/unwind pointer revocation and frame-owned computation lifetime pass. Scalar
SharedCall preparation has no additional retained managed state; current String
constants retire with publication. These correct prior unexecuted environment/root
assumptions, not establish allocation or speedup measurements. Three Vec-dependent
scenarios still fail in analysis under the queued NC gap. Remaining allocation/SDK/
backend/diagnostic consumers, session attribution and complete production proofs
still gate RR03; CI/Miri and performance acceptance remain pending.


Standalone SDK async/decoded-artifact and CLI consumers now run through real
roots and source views; actual Native/fallback and Host scope/reentry samples
pass. The stdlib allocation target builds. Warm prepared scalar/array Native and
script argument/return windows pass their original zero-Rust-allocator assertions;
whole dynamic interface Host calls still allocate. The inherited method ID lookup
now uses its checked lexical-to-runtime parent correspondence. Runtime's private
parent-provider oracle verifies identity reuse across real calls and complete
retirement; the public width oracle no longer mistakes new admitted-computation
metadata for parent wrappers. The
[RR03 SDK/allocation evidence](runtime-refactor-plan.md#rr03-sdk-source-free-and-allocation-consumers)
records methods, toolchain/machine/profile, actual results and the explicitly
retired cumulative all-object assertion. These observations do not establish
whole-Host zero allocation, complete backing size or matched latency acceptance.
SDK unit, diagnostic/session accounting and remaining production proofs still gate
RR03. Vec-dependent benchmark/default-library routes and full feature/platform CI
remain pending under their existing owners.

Public SDK typed-handle/source and Tokio transfer consumers now pass six local
cases through genuine roots and protected reads, including actual old executable
retirement with readable detached source diagnostics and final array-root release
across a real worker await. The [SDK unit consumer evidence](runtime-refactor-plan.md#rr03-sdk-unit-consumers-and-retained-public-handles)
records the moved existing test owners, changed cache-lifecycle oracle and exact
selected commands. Async/cross-route unit consumers were ported before the SDK unit target recovered
compilation; the previously unexecuted cases remain unclaimed until selected runs. The
Task cache's Vec-dependent scenario, diagnostic/session attribution and complete
production proofs remain open. No CI/Miri or performance gate is closed here.

Host-interface and concrete managed Native-result consumers now pass twelve
selected tests in the existing artifact_features target. The protected tuple
constructor supports actual nested external-root results, and managed generic
payload fields replace raw Value tracing. The [RR03 Host/result evidence](runtime-refactor-plan.md#rr03-generic-reload-host-interfaces-and-managed-native-results)
records exact selected filters, the scoped method-ID fixture correction, retired
unrepresentable raw-ID witnesses, retained Runtime-owned loan proofs and the
foreign rooted-tuple sampling limit. The generic cold-graph release oracle remains unexecuted; its SDK unit target now
builds. No resource-attribution, diagnostic,
Miri/CI or performance gate is closed by these consumer results.

The existing decoded application-payload oracle now includes Runtime shutdown
while a Host lease remains: normal collection counts, forced payload destruction,
foreign import rejection and inert-lease drop preserve exactly-once disposal.
The [RR03 observer/shutdown evidence](runtime-refactor-plan.md#rr03-sdk-root-observers-callback-controls-and-shutdown)
records the passing selected test, removed duplicate unit teardown and the root
baselines at the earlier consumer checkpoint. Remaining composite/callback consumer
ports are not claimed merely because the SDK unit target now builds. Resource/session attribution, diagnostics,
complete production proofs, CI/Miri and measured acceptance remain open.

Default SDK build recovery now permits selected unit integration: 21 distinct local
cases pass for enum layout/data reclamation, error snapshot versus body lifetime,
interface reload, exact Host loan conflict/reentry, Host path effect ordering,
actual Native code/page ownership and safepoint progress. Workspace all-target
Clippy, structure, fmt and diff checks pass with default features. The
[RR03 default-build evidence](runtime-refactor-plan.md#rr03-sdk-default-build-recovery-and-selected-lifecycle-acceptance)
records filters, the repaired scalar-Debug/pacing assumptions, the retired raw-ID
point-in-time probe and its sampling limit. The additional shared-arithmetic case
fails at the inherited `Vec::from` source-analysis gap; its Runtime cleanup path
has not executed. Plain rooted enum data keeps immutable layout facts, not an old
executable lease. Resource/session attribution, diagnostic emitters, outstanding
actual production proofs and CI/Miri/performance acceptance remain open in RR03.

Committed heap/session attribution now uses readonly access/service interval
reports from the existing ledger. Actual allocation/mutation commits preserve
within-interval peaks; a borrowed Runtime guard attributes them after physical
loans close, including unwind. Current occupancy follows real retirement, and
conversion preparation has its own scoped component. The
[RR03 accounting evidence](runtime-refactor-plan.md#rr03-committed-heap-observations-and-session-attribution)
records 17 distinct passing selected cases, including fresh-root +3/+2 increments,
exact String unwind units/backing, generic grow/shrink/failure/forgotten-guard
behavior, Native/Host cleanup and unchanged zero-allocation warm boundaries.
Default workspace Clippy/structure/fmt/diff checks pass. The later
[RR03 diagnostic evidence](runtime-refactor-plan.md#rr03-diagnostic-sources-events-and-backing-consumers)
supersedes the 28 old consumer errors and unemitted-event warnings: actual sources,
storage events and nonallocating retained backing observations are wired. Five
selected release tests pass, including the twelve-row provenance probe; Runtime,
Lua and SDK diagnostic all-target builds/lints pass. Two ordinary allocation
contracts remain zero after instrumentation, and diagnostic arithmetic executes.
The original complete storage workload still fails inherited Vec source analysis
before Runtime execution. Complete actual production proofs and CI/Miri/matched
performance remain open in RR03; footprint accounting must include the new
interval records and wider counter/session fields. Retained buffer snapshots,
occupancy deltas and cumulative events are distinct measurements, not full RSS.
The subsequent [RR03 local exit audit](runtime-refactor-plan.md#rr03-production-proofs-and-local-phase-exit)
closes those production-proof/build gates: 19 additional selected cases pass,
including actual Native/script/Native staged-result collection, source-free
single/cross-program cycles, old marker/current helper selection and four provider
fault/shutdown cases. One Vec-dependent SDK scenario still fails before Runtime;
it remains NC-owned. The reviewed Windows AOT ports are unexecuted locally.
Earlier cutover paragraphs record intermediate evidence, superseded by this audit
and the current acceptance rows; RR04-RR06 and final CI/performance remain open.

## Remaining decisions and work

- **Runtime representations:** validate fixed layouts against packed Value fields,
  custom erased layouts and outstanding aliases. Remove Native take rather than
  adding extraction/tombstones to the collector. Do not
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

The [Runtime carrier proposal](runtime-refactor-design.md#runtime-reference-carriers-and-transfers)
reuses Gc for scoped access, Edge for traced slots/fields and Root for escaping
Host retention. Tagged wrappers and Native call-lifetime slot views are selected for the first
implementation, not implemented Runtime APIs; the standalone frame adapter supplies the underlying
get/publish/set mechanism. GCR02 must prove the actual Runtime tags, roots and
transfers before this can close representation acceptance. Do not preserve the
old unbranded Value: Copy API using raw pointers or a persistent Root per value.

The [selected Native/execution/GC flow](runtime-refactor-design.md#selected-native-execution-and-gc-control-flow)
closes the first implementation choices: slot-based Native entry/results, mutable
service reentry, sole execution-root storage, managed installation/state edges and
single-owner cleanup. GCR02 still owes real proofs and GCR03 the coherent cutover;
these design decisions do not establish compiler/GC/performance acceptance.

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
   final host-root drop and destructors on trap/unwind retain their
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
cancellation, parked reload and source-free boundaries. Do not dereference freed
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
