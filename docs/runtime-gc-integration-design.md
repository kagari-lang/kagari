# Runtime integration with kagari-gc

Status: design draft, 2026-11-03. Runtime still uses its existing collector.
This document records a proposed integration model for review and revision; it
does not authorize or report an implementation cutover. Proposed type and helper
names are illustrative. Unresolved choices are collected in
[open decisions](#open-decisions); they must not be mistaken for implemented APIs.

This document owns Runtime responsibilities, value representations and the
protocols that connect execution, objects, native providers and host handles to
`kagari-gc`. The [GC implementation design](gc-implementation-design.md) owns the
collector's physical storage, access, tracing, barriers and reclamation contracts.
The [GC redesign plan](gc-redesign-plan.md) remains the single owner of GCR00-GCR05
phase order, progress and acceptance. This is a design document, not another plan
or progress queue.

Current language, host, failure and reload behavior remains authoritative in the
[runtime specification](spec/runtime.md), [host interop](spec/host-interop.md),
[value semantics](spec/value-semantics.md) and
[module activation](spec/module-activation.md). Preserve those observable contracts
except for the user-directed pop/remove failure-order simplification under
[Option wrapping](#option-wrapping-and-removal-failures) and removal of
[cooperative execution cancellation](#execution-without-cooperative-cancellation-polling). Text fences are
Rust-shaped schemas or pseudocode, not compilable examples or declarations of
currently available APIs.

## Problem and bounded outcome

The existing `GcHeap` owns more than allocation and collection. It contains
`ResourceState`, executable environment/interface/application/group stores,
native operation guards and collection-specific policy. `ResourceState` in turn
contains sessions, publication state, execution windows, termination and both
execution and memory counters. As a result, a frame-slot read reaches through
the heap to find execution storage. Collection explicitly coordinates object,
program and executable metadata graphs and their separate retirement stores.

The relevant current owners are [GcHeap](../crates/kagari-runtime/src/gc.rs),
[ResourceState](../crates/kagari-runtime/src/resource.rs),
[joint collection](../crates/kagari-runtime/src/gc/collection.rs),
[metadata tracing](../crates/kagari-runtime/src/execution_metadata.rs) and
[program tracing](../crates/kagari-runtime/src/module/collection.rs). These links
describe the pre-cutover implementation, not the proposed destination.

Replacing the marking loop alone would preserve these dependencies and add an
adapter around them. The proposed outcome is one physical collector, one managed
reachability graph and explicit Runtime owners for execution and semantics.
Existing compact scalar banks, prepared field facts and closed interpreter
regions remain useful foundations. Their existence does not require keeping
execution state inside the heap.

This design includes value transport, direct execution roots, object operations,
managed module dependencies, native storage and host retention. It does not
activate the complete [native collections migration](native-collections-plan.md),
redesign source analysis, add script-visible dynamic typing, introduce concurrent
heap access or complete [R13 type/code separation](review.md#r13--type-and-layout-provenance-retains-retired-code).
The integration must avoid creating new instances of that retention defect.

## Responsibility and ownership model

| Owner | State and decisions |
| --- | --- |
| `kagari-gc` | Allocation, physical descriptors, host root leases, scoped identities, barriers, tracing, incremental phases, disposal, managed accounting and Faulted/Closed state |
| Runtime memory integration | Checked ownership of `RootDriver<ExecutionRoots>`, short access regions, value/root adapters and GC error/report conversion |
| Runtime execution | Session lifecycle, call depth, sticky terminal failure, frame lifecycle, parameter/result transfers, observation and execution admission; no cooperative cancellation polling |
| `ExecutionRoots` | Authoritative active/parked value banks and managed computation dependencies; complete direct-root scanning and controlled slot publication |
| Runtime objects/native providers | Nominal type and declared-access checks, representations, field/payload operations, collection operations and their commit rules |
| Runtime modules/reload | Installation/version identity, current-call selection, staged publication, semantic lookup indexes and genuine publication roots |
| Runtime host boundary | Rust-owned resources, installed interfaces, borrow scopes, typed conversion and independently retained script handles |
| Outer `Runtime` | Construction, orchestration, callback transitions, safepoint scheduling and ordered shutdown |

```text
Runtime {
    memory: checked exclusive access to RootDriver<ExecutionRoots>,
    execution: sessions + depth + termination + execution admission,
    modules: semantic indexes + publication policy + publication roots,
    host: Rust resource registry + checked borrow state,
    native: installed declarations, entrypoints and provider bindings,
    scheduler: scheduling/control state,
    ...
}

RootDriver<ExecutionRoots> {
    heap: generic physical collector,
    source: authoritative frame/value/computation-root storage,
}
```

`ExecutionRoots` is semantically execution-owned even though the driver physically
owns it to guarantee that every collection sees the complete source. It cannot
borrow `Runtime` persistently, omit parked executions or be replaced while
populated. It is not a mirrored root bank beside another authoritative value bank.

Split `ResourceState` by responsibility. Execution control and publication state
must no longer be obtained through `GcHeap`. GC-owned committed physical/indirect
charges are not copied into an independently mutable second ledger. Runtime may
combine those observations with its own frame, scheduler and other resource
figures, preserving documented exclusions and failure order.

Logical modules do not require new crates. A narrow checked interior borrow of
the memory driver can coexist with `&Runtime` APIs, but no whole-Runtime borrow
or driver loan may survive a user callback. Split access to sibling semantic and
execution services rather than store a Runtime reference inside GC Access.

## Value categories and retention forms

The current [Value](../crates/kagari-runtime/src/value.rs) is a copyable tagged
transport containing scalars or checked owner/slot/generation identities. It can
be copied into arbitrary Rust scratch and later rejected as stale. The proposed
scoped representation removes that permission for direct managed references:
crossing a collection boundary requires an explicit traced destination or root.

Three forms share one private representation family:

| Form | Managed carrier | Retention authority | Allowed lifetime and operations |
| --- | --- | --- | --- |
| `Value<'gc>` | `Gc<'gc, AnyObject>` | The current collection-excluded scope | Copyable temporary; cannot escape the region or cross a callback/collection |
| `StoredValue` | `Edge<AnyObject>` | Its live traced object or the complete driver root source | Private persistent storage; no public independent dereference, Copy or Clone |
| `RootedValue` | `Root<AnyObject>` | An independent host root registration | Immutable retained value; clone shares object-root registration; access checks Runtime ownership/lifecycle |

An edge is not itself a root. A standalone Rust `Vec<StoredValue>` does not keep
its targets alive unless that storage is included in a reviewed tracing owner.
An `Arc` containing such a vector does not establish that ownership either.
The [SpiderMonkey comparison](#spidermonkey-reference-categories) supports this
reference-role separation; the [gc-arena comparison](#gc-arena-scoped-access)
supports the proposed Rust access interval. Neither specifies our Repr layout.

### Shared representation family

```text
// Private representation; generic only over the reference carrier.
enum Repr<R> {
    Unit,
    Bool(bool),
    Char(char),
    I32(i32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),

    Str(R),
    Tuple(R),
    Range(R),
    Array(R),
    Struct(R),
    Enum(R),
    Interface(R),
    Closure(R),
    Cell(R),
    Native(R),
}

Value<'gc> { repr: Repr<Gc<'gc, AnyObject>> }
StoredValue { repr: Repr<Edge<AnyObject>> }
RootedValue {
    owner: existing Runtime admission identity,
    repr: Repr<Root<AnyObject>>,
}
```

The category inventory is a draft target, not an instruction to mechanically
rename today's enum. Array denotes the builtin fixed-length array; ordinary Vec,
HashMap and HashSet ultimately use Native. Their migration is coordinated with
NC, without changing script APIs or creating two permanent storage semantics.
Host roots, paths and ephemeral call carriers are addressed separately below.

The representation enum and wrapper fields stay private. An ordinary caller
cannot combine a Struct tag with an arbitrary erased handle. Reviewed allocation,
root import, field/slot reads and checked conversion establish the pairing.
Erasure removes a Rust static type, not physical descriptor or language checks.
Typed payload reads still downcast/check the actual representation; dynamic
records require their exact prepared layout evidence.

Only the scoped wrapper implements Copy. A conditional internal implementation
on `Repr<R>` must not accidentally provide public cloning for stored carriers.
Do not expose an unrestricted generic `map_reference` conversion between the
three forms: each conversion has a different liveness/publication proof.

One representation family shares tag/scalar handling, not a second language
semantics engine. Arithmetic, equality, hashing, trait operations and readonly
access rules remain with their existing semantic owners. In particular, Rust
pointer/tag equality is not script string, tuple or collection equality.

### Representation tags are not semantic types

Struct identifies a representation category, not a declaration such as Player.
The object's semantic metadata and prepared operation evidence determine nominal
identity, applied types, field contracts and supplying installation. Value does
not acquire a per-copy `Arc<Ty>`, executable lease or inferred dynamic type.

Integer carrier variants preserve the existing widened execution representations;
narrow integer domains remain checked by semantic/layout facts. Char must remain
a valid Unicode scalar. Floating-point bits, full u64 payloads and checked numeric
behavior must survive transport and packed-field codecs.

An enum's variant discriminator stays with its enum representation. Interface
receivers and closure environments remain real managed relationships. Do not
encode ordinary Option/Result branches or collection families as new special
Value variants merely to simplify dispatch.

### Option wrapping and removal failures

User direction, 2026-11-03: Option will later wrap its value directly rather than
require a separately allocated GC enum object for every result. Exact encoding
remains future work, including distinguishing Some(None) from None and tracing
the payload correctly; this document does not implement or freeze that layout.
Ordinary enum/Result representation is not changed by this decision.

Do not design script pop/remove around fallible allocation of an Option wrapper.
Use the ordinary removal result and publish it through the common return path.
In particular, no PreparedReturn, prepare_some/commit pair, preallocated Some,
mutation rollback or retry is required to keep the collection unchanged if
encoding/publication fails after removal. This intentionally supersedes the
current implementation's Option-before-removal allocation order, including any
intermediate implementation that still allocates the wrapper.

Runtime failures abort the invocation and propagate to the host; they are not
script-level None or Result::Err values for the script to handle. Preserve the
original error and completed mutations. Never turn a failure into normal absence
or apparent success. Bounds, access and borrow checks still reject an invalid
removal before it commits; an error does not imply that deletion must have occurred.

GC safety is separate: a removed Value<'gc> remains physically live in its closed
region even after the source edge disappears. Install any value needed by later
execution in a traced result/working slot or owning root before a callback,
collection or region exit. This requires protected transfer, not transactional
collection contents. Atomic tag/edge commits and valid storage after errors
remain mandatory. See the [authoring examples](#standard-library-authoring-proposal).

### Physical size and pointer representation

Start with ordinary Rust representations and the library's opaque carriers.
Check actual size/alignment on 64-bit native and wasm32 targets before selecting
a compact encoding. A 16-byte general transport is a 64-bit measurement target,
not an established result or a constraint on larger host-root wrappers.

This draft does not select NaN boxing, pointer compression, integer reconstruction
of managed pointers, hand-written unions or safe raw-address admission. Exact
layout is internal; it is not a portable artifact or native-call ABI. Any later
encoding must preserve Rust pointer provenance, all scalar bits and the carrier's
liveness/admission contracts, with focused strict-provenance validation.

Known scalar frame slots remain in the existing scalar bank. Concrete struct
scalar fields retain exact widths, and scalar containers retain specialized
storage. General tagged transport does not require one full StoredValue per
scalar field, per primitive collection element or per bytecode operand.

## Admission, reads and conversion capabilities

```text
live traced field or admitted frame slot --load--> Value<'gc>
RootedValue --checked import into this Runtime/scope--> Value<'gc>
Value<'gc> --publish and commit--> traced field or execution slot
Value<'gc> --retain--> RootedValue
```

There is no conversion from an arbitrary detached `StoredValue`, saved address
or previous-scope Value into a new scoped Value. Checking that an allocation
currently occupies an address is insufficient: the address could have been
reused, and the old carrier has no independent retention authority.

| Entry | Required evidence before constructing a scoped value |
| --- | --- |
| Fresh allocation | Same scope/heap, fully initialized representation and admitted tag |
| Host retained value | Existing Runtime identity/lifecycle checks, valid core root registration and representation pairing |
| Managed object field | Admitted live source object, declared/prepared field evidence and controlled stored carrier |
| Execution slot | Same-driver source, live frame/window generation, initialized slot and prepared representation |
| Typed native access | Admitted Native shell, exact registered Rust/application identity, payload still Present and valid loan scope |

Numeric IDs remain useful for frames, installations, host registries and lookup
indexes. Their owner/generation checks do not disappear. They are not replacements
for managed edges and cannot alone retain a target. Existing stale/foreign IDs
must be rejected before mutation or accounting. No public unchecked ID-to-Gc
conversion or duplicate permanent object directory is selected.

The brand protects managed references throughout a scope even if the source
field is cleared. Actual payload loans are shorter shared/exclusive borrows;
holding two aliases as Value does not grant overlapping mutable references.
Faulted/Closed gates still apply inside a scope after a caught error/panic.

## Persistent fields and prepared layouts

Value is the transport surface, not the mandatory physical layout of every
object. Runtime supplies checked field representation, access policy and nominal
facts; GC supplies physical allocation, exact-layout access and reference writes.

For a field with a prepared concrete reference representation, storage can hold
only the edge and reconstruct the transport tag from immutable prepared facts.
A general tagged slot retains its tag alongside the edge. Neither case permits
inferring nominal types again from source or a previous field value.

The fixed dynamic API currently offers scalars and nullable erased references.
It does not yet provide all of Runtime's semantic headers, tagged stored slots
or custom erased layouts. GCR00/GCR02 must determine which actual fields fit it
and which require a bounded provider/descriptor extension. Do not assume the
current dynamic builder is a complete Runtime object format, or add a second
per-object Box<Value>/field map solely to make the prototype connect.

Semantic descriptor provenance must keep required type/layout facts without
giving GC descriptors LoadedModule ownership. Placement of nominal metadata and
managed computation links is an open representation decision, separate from
physical field offsets and from completing the existing R13 work.

### Reference field write

For `player.target = enemy`, preserve the existing evaluation and trap order:

1. Evaluate receiver and right-hand value left to right, once each.
2. Perform the language checks at their specified points: initialization,
   declared access, concrete type, installation and any host borrow rules.
3. Admit the live receiver and exact prepared layout in a short access region.
4. Prepare any fallible storage/accounting work, then publish the incoming target
   through the core barrier before changing the destination.
5. Commit the tag and edge together in a suffix that cannot fail or call user
   code. On preparation/publication failure the old field remains intact.

The steps group responsibilities; they do not authorize moving a check across
an observable evaluation or side effect. A scalar write uses the same semantic
owner but needs no reference barrier. Completed scalar edits remain committed
if a later operation fails, as in the existing execution contract.

Replacing/removing references does not invalidate already admitted scoped Values.
A removed result must be installed in its next traced/rooted destination before
the interval ends. Container relocation additionally repairs scan coverage or
publishes relocated targets under the existing provider mutation protocol.

## Authoritative execution roots

Adapt the existing [execution windows](../crates/kagari-runtime/src/frame/values.rs)
and [closed interpreter cursor](../crates/kagari-runtime/src/frame/cursor.rs)
rather than introduce per-variable roots. ExecutionRoots owns scalar/managed
banks, initialized-slot state and managed dependency slots. RootSource scans only
the required managed carriers, charging inspected holes/slots as contracted.

Active frames, parked executions, queued computations, pending return/argument
transfers and driver-owned temporary storage must each have an identified genuine
root owner. A result stored in an ordinary managed Task/Future is an object edge;
an independently scheduled active task may also have an execution-owned root.
Do not root every internal result independently and thereby retain dead cycles.
Host registry retention of script values must likewise use core roots or the
complete source, not an omitted external graph callback.

### Frame assignment and slot reuse

```text
within one admitted execution region:
    v = frame.load(source)       // scoped Copy; no root registration
    frame.store(destination, v)  // publish, then commit tag/carrier
```

Bounds, initialization, frame owner/generation and prepared type checks remain.
New/reused frames publish initial values and computation dependencies before
becoming visible. There is one authoritative commit, not a write to a value bank
followed later by a write to a mirrored RootSet.

Source cursors retain stable coordinates/generations, never Vec addresses or
borrowed slices across service calls. Removing/compacting windows must preserve
coverage: an unscanned reference moved behind the cursor is published or the
cursor is repaired. A frame reused after a captured frontier cannot be mistaken
for its old occupant. Scalar banks are not conservatively scanned for pointers.

### Arguments, return values and suspension

Arguments move from protected caller slots into admitted callee slots with all
managed dependencies published before the callee is observable. Passing a scalar
does not allocate a root. Checked reference copies do not clone object contents.

A return transition adapts the result while the callee still supplies any needed
protection. Publish the caller destination, pending-result slot or host RootedValue
before releasing the last required source protection. A closed no-collection
transition may reorder physical retirement internally, but its scoped liveness
proof must cover the complete transfer. Root allocation failure cannot expose an
unprotected host result or silently drop required computation dependencies.
This safety requirement does not promise rollback of an already completed
collection mutation if later result handling fails; see
[removal failure policy](#option-wrapping-and-removal-failures).

Suspension retains slots and the selected computation body in the direct source;
it does not store a Value<'gc> in an async future. Resumption creates fresh scoped
values from retained slots. Trap, depth exhaustion and scheduler retirement of
queued/suspended or abandoned execution release windows/dependencies once through
their lifecycle owner; see the [execution policy](#execution-without-cooperative-cancellation-polling).

## Host retained values and replaceable slots

RootedValue is proposed as an immutable retained snapshot of a value. Scalars
need no managed target; object clones share the core Root lease. The wrapper's
owner uses existing Runtime identity/admission machinery, not a new global
identity registry. Runtime access still rejects foreign and closed/faulted owners
for scalar-only wrappers as required by the existing boundary. Handles must not
strongly retain Runtime, its frame store or the heap after shutdown.

RootedValue retains value reachability. Typed ScriptValue, prepared functions and
other host adapters still own their checked applied-type/access contracts and
any genuinely required computation roots. A general value copy must not retain
an entire module merely because its nominal type came from that module.

### Replaceable host slots

Today's RootedValue also exposes `set` through a shared root-table slot. This
draft separates that mutable-slot operation from an immutable value snapshot:

```text
HostValueSlot {
    owner: existing Runtime admission identity,
    roots: private one-slot RootSet,
    value: private scalar payload or object tag, // no detached object address
}

load(scope) -> Value<'gc>
replace(scope, Value<'gc>) -> Result
snapshot(scope) -> RootedValue
```

The baseline slot is not Clone and mutation requires its exclusive loan. Do not
expose its RootSet or clone it into wrappers with independent mutable tags. The
slot reserves one registration even while empty/scalar; replacing its target
does not allocate another registration. Snapshotting an object creates an
independent immutable retained value and can fail before any slot change.
Immutable snapshots plus a uniquely mutable slot are a Kagari draft API choice,
not a requirement imposed by the reference runtimes; see
[limits of the comparisons](#what-the-comparisons-do-not-decide).

Replacement prepares/adopts the incoming reference and publishes it first, then
commits tag/scalar data and target with no intervening failure, callback or
collection. Object-to-scalar replacement clears the target; scalar-to-object
replacement publishes before installing the object tag. Failed replacement
preserves the complete old value. The Runtime wrapper owns tag coupling; the
generic GC root table stores no Kagari Value.

If a concrete consumer requires shared replaceable slot handles, it needs one
shared authoritative tag/target protocol, not copied tag caches. The draft does
not select such an extension. Migrate existing `set` consumers deliberately;
do not keep duplicate compatibility entrypoints or lose foreign-owner and
borrow-conflict rejection coverage.

RootedValue cloning/dropping remains suitable for crossing host threads as inert
retention. Clone does not clone the script graph; last-root drop queues release,
not collection or a user destructor. Heap access remains exclusive and requires
the owning Runtime. Shutdown makes surviving handles inert without dereferencing
freed payloads. Public Send/Sync bounds of any new slot API require explicit review.

## Scoped host arguments and nonpersistent carriers

Existing HostRoot, HostPathView, HostRef/HostMut and RuntimeEphemeral transport
cannot all be mechanically converted to persistent Native objects. Follow the
current storable/borrowed-host contracts: some values are valid call arguments
but forbidden in persistent script storage or owning host return handles.

Use the existing argument/borrow owner to distinguish ordinary scoped Values
from nonpersistent call carriers. A schematic argument view may have both a
host-call lifetime and a GC-scope lifetime. Neither brand substitutes for the
other: GC membership does not prove a Rust loan or host access capability.

Preserve the allowed outer-tuple argument composition and existing typed native
conversion behavior. A borrowed member must not become persistable by nesting
it in a tuple, interface, native payload or rooted wrapper. Any GC-allocated
temporary argument aggregate needs complete tracing and an explicit nonpersistent
classification. Final carrier names/tuple representation remain open; do not
silently remove supported calls or make borrowed returns persistent in this work.

## Native payloads, provider safety and extraction

The current [NativePayload](../crates/kagari-runtime/src/native/storage.rs) is a
safe trait over checked-ID Values with whole-payload tracing. The new GC's
[GcObject](../crates/kagari-gc/src/object.rs) is an unsafe contract over direct
stored references. Missing an edge can now violate memory safety, rather than
only produce a later checked stale-ID error. A blanket unsafe implementation
delegating to every existing safe NativePayload implementation is not justified.

The provider boundary must establish complete tracing, controlled construction,
reference publication, scanner repair, accounting and unwind-safe owned storage.
Reviewed unsafe providers or a sufficiently restrictive safe schema can provide
that proof. Ordinary methods use their safe facade; no general mutable payload
access may permit inserting arbitrary references or hiding changes in interior
mutability. Existing derives/schema APIs must be audited at their actual owner.

EdgeFree and bounded/resumable scanners use the core's declared work model. A
large whole-container scan cannot be labelled bounded merely by wrapping today's
trace callback. An explicitly indivisible provider must disclose its pause cost;
that choice does not establish integrated incremental-latency acceptance.
Mutation-capacity growth and completed partial edits retain committed charge,
including on errors. Release never calls an untrusted provider to reconstruct an
already committed charge. Native operation/revision and callback mutation guards
remain Runtime/provider policy rather than generic collector state.

### Stable native shell for take

```text
Value::Native -> Gc<AnyObject> naming a stable NativeObject shell

NativeObject {
    checked registration/application facts,
    managed computation dependencies,
    payload: Present(erased owned Rust payload) | Taken,
    committed provider state/charge,
}
```

The baseline uses a stable shell for registered Runtime native payloads. It does
not require an extra allocation merely to transport a Value; exact shell/payload
placement is measured separately. Standalone typed GC objects are unaffected.
The [mlua comparison](#mlua-native-borrows-and-extraction) motivates checked
extraction and invalidated access through old aliases, not this exact Rust layout.

A successful take changes the shell state, never its physical GC representation
descriptor or allocation address. A scoped Native value continues to name a valid
shell; subsequent payload access reports moved/taken state. Do not expose a
long-lived `Gc<T>` to a takeable inner T and then remove that T in the same scope.
Typed native handles resolve and check the shell before lending T; active payload
loans and native operation guards reject take.

Take remains restricted by [NativeTake](../crates/kagari-runtime/src/native/payload/take.rs):
the original T is recursively free of managed edges, roots, executable metadata
and script callbacks. It is moved once, never cloned. Prepare all fallible checks
and any collector mutation bookkeeping first. Failure preserves the original
payload. Commit Taken, detach no-longer-needed managed dependencies and update
charge under the reviewed shell mutation protocol; drop detached metadata only
after storage loans end. Physical shell bytes remain charged until GC reclaims it.

Old fields/frames/roots may still contain aliases to the shell. Their tracing is
valid and does not fault collection; language payload access and fresh publication
through Runtime reject the taken value according to existing rules. No second
moved-ID registry is introduced. Once its genuine roots disappear, the ordinary
collector reclaims the shell. GC itself understands no Native or Taken state.

This replaces the earlier target sketch that changed an allocation's descriptor
to Taken. That sketch did not establish safety for already admitted typed Gc
handles. Generic representation-changing extraction is not assumed or required
by the stable-shell baseline.

## Module state, code dependencies and reload

Reachability-bearing module/static/environment/interface state becomes ordinary
Runtime-defined managed objects. Their outgoing references are real stored edges,
including relationships not carried by a general StoredValue. Runtime lookup
indexes keep semantic IDs and availability/version checks but do not become
permanent roots for every retired version.

Live installations remain Runtime-owned: dropping a public Program/Module
handle does not unload an installation or reset its state. Shared persistent
static cells keep their existing installation lineage across compatible reload;
moving their representation into GC must not duplicate cells per code version.
Pending initializer recipes, Ready values and failed-attempt state retain their
current behavior. Staged/abandoned candidates have only their actual candidate
retention, never ordinary-call authority merely because their managed node lives.

```text
object -> closure -> executable environment -> static cell -> object
```

After publication/host/execution roots to this cycle disappear, one collector
must reclaim it. Replacing any of these internal edges with an owning host Root
would keep the cycle alive. An internal Rust Arc does not replace managed graph
accounting for relationships that can lead back to script objects.
If a current installation still reaches the static cell/value, the cycle is
genuinely live; dropping an old host handle alone is not a reclamation condition.

Immutable code/resources without managed edges can remain Rust-owned backing
held by managed computation state. An admitted frame, closure, queued operation
or Future retains its exact selected body where required. An ordinary subsequent
call still selects the current implementation in the same live installation.
Data layout/type retention must not accidentally freeze ordinary dispatch or
resurrect executable admission for a retired registry record.

Module indexes need an explicit validity/pruning contract. The draft does not
select a public weak-reference feature, raw stale-pointer entries or a second
metadata collector. GCR02 must prove how a semantic lookup acquires live managed
evidence and how obsolete index entries are removed without rooting the graph.

## Closed regions, callbacks and failure

Interpreter execution obtains short, disjoint access to admitted frame slots,
memory and prepared semantic facts. Retain the existing region structure and
scalar fast paths. Do not acquire a driver RefCell borrow for every scalar
instruction, repeat type preparation per element or cache mutable field values
across aliases.

Before a callback or synchronous script reentry:

1. Finish the current mutation and install all surviving values/dependencies in
   traced slots or independent host roots.
2. Release driver, frame-bank and payload loans. No Value<'gc> crosses this point.
3. Invoke the callback with host borrow/depth and terminal-failure admission.
4. Let nested execution use the same driver and complete root source; outer
   parked/active frames remain retained.
5. Reacquire a new region and reload values after return. Publish returned values
   before releasing their final protection.

A native body is not one giant GC-excluded scope. Operations that allocate or
reenter must provide valid GC service boundaries. Runtime owns safepoint
eligibility and observation independently of the removed cancellation polls;
core work units do not define an instruction interval or a hard time bound.

Foreign input, wrong language types, bounds and moved native payloads return the
existing appropriate Runtime errors before unsafe physical access. A broken
provider/root-source proof or core Faulted state quarantines Runtime; catching
the error does not permit more execution. Scope branding does not remove the
core state gate. Shutdown-only cleanup remains available after quarantine.

Use the existing collector panic/disposal contract rather than silently changing
panic isolation, partial side effects or error order. No collection, user code or
destructor may intervene in a prepared no-fail tag/reference commit. A faulting
trace cannot be treated as an empty successful scan and followed by sweep.

Shutdown stops admission, retires execution/queued work and genuine Runtime roots
in the prescribed order, then closes the driver. Remaining host handles are
inert. Physical disposal does not trace or dereference removed peers; owned
payloads are disposed once under the established panic-containment policy.

## Execution without cooperative cancellation polling

User direction, 2026-11-03: remove Runtime's cooperative execution-cancellation
mechanism, including its token polls in interpreter/backend loops, script/native
call transitions, conversion traversals and long native operations. This is a
semantic change, not merely removing explicit cx.poll() from function bodies or
moving the same checks into next_into/invoke_into. Do not add a replacement
cancellation flag, polling interval, optional mode or hidden helper-loop check.
The current implementation still follows the existing specifications until
cutover; this section records the selected destination.

### Rationale and accepted tradeoff

The user accepts noninterruptible synchronous execution in favor of performance
and simpler Native authoring. Kagari primarily runs trusted application scripts;
universal in-flight cancellation is not a requirement of this selected model.
Remove its repeated execution checks and provider polling obligations directly,
while measuring the actual benefit separately. Nonreturning synchronous code is
an accepted execution limitation, not a missing cancellation checkpoint to add
back during implementation.

Runtime practices differ: Lua exposes explicitly installed instruction hooks;
[Wasmtime offers fuel/epoch interruption](https://docs.wasmtime.dev/examples-interrupting-wasm.html),
and [V8 exposes TerminateExecution](https://v8.github.io/api/head/classv8_1_1Isolate.html).
These official references were reviewed on 2026-11-03; the V8 link is its moving
head API documentation. They establish that interruption is an explicit runtime
capability with differing mechanisms, not that other languages universally omit
it. Neither a guest-code interrupt nor a hook establishes arbitrary native Rust
function preemption. Kagari does not adopt these interruption mechanisms.

### Observable execution contract

Synchronous script/native work runs until normal return, a real execution error,
or an existing suspension/driver handoff. Changing a cancellation token does not
interrupt it, including across nested script calls and synchronous host reentry.
A nonallocating infinite script loop or nonreturning Rust native function can
therefore keep the executing thread indefinitely. Returning to the scheduler has
no bounded latency. GC progress, debugger observation and existing scheduler
slicing retain their own contracts; none becomes a cancellation channel.

For example, a host cancellation request arriving halfway through a synchronous
fold does not stop the remaining callbacks or convert its result to Cancelled.
The fold completes or reports an actual callback/runtime failure. There is no
cancellation-token read on each iteration, callback or return transition. This
removes that category of work, not argument/result transport, GC publication,
borrow admission or error propagation; no speedup is claimed without measurement.

### Responsibilities that remain

- **Execution admission and failure:** reject closed/quarantined Runtime entry,
  illegal collecting/commit reentry and operations after a recorded terminal
  execution failure. Keep call-depth limits, allocation/checked arithmetic errors,
  first-failure provenance, cleanup and completed side effects. A host ignoring an
  inner terminal error cannot resume the failed execution. These checks belong
  at actual admission/invalidation boundaries, not cancellation polling loops.
- **GC:** Runtime memory integration schedules collection only at valid service
  boundaries after publishing roots and releasing loans. Core Faulted/Closed
  gates, barriers, accounting and root cleanup remain mandatory. A cancellation
  token is not needed to run the collector or prove scoped access safe.
- **Async lifecycle:** cancelling/dropping queued or suspended work, Task/scope
  retirement and external-operation cancellation hooks remain scheduler-owned
  control transitions. The driver handles them when it has control, releases
  owners exactly once and serializes retirement against result publication.
  They cannot unwind an actively executing script/native segment. Runtime::poll
  and Rust Future::poll still drive readiness; they are not cx.poll(). Existing
  scheduler handoffs are not an instruction-cancellation facility.
- **Compilation/tooling:** analysis, compilation and artifact-admission request
  cancellation are outside this execution change; keep their existing contracts.

### Cutover obligations

The current ResourceState::poll_execution combines Runtime phase admission,
sticky terminal failure and SessionState cancellation-token polling. Split the
first two responsibilities into their actual owners; do not delete this method
and lose their safety/error guarantees. Remove execution-token plumbing and the
Native/loan/conversion cancellation-poll API rather than retain inert compatibility
entrypoints. Preserve shared CancellationToken support needed by compilation;
audit SDK context fields and async control forwarding separately from it.

GCR03 must update all execution backends, stdlib providers, conversion/host paths,
SDK entrypoints and affected architecture/spec/example contracts coherently.
Replace tests requiring in-flight token interruption with the new synchronous
contract, while retaining distinct GC, borrow, depth, sticky-failure and async
retirement coverage. Use a finite operation with a token changed during a host
callback to demonstrate continued execution; never use an unbounded hang as a
test. Verify queued/suspended retirement and cancellation/completion ordering at
the scheduler owner. Do not retain an execution-token API solely for this test;
if a host context still carries a compilation/request token, verify that execution
does not inherit it. This documentation does not implement that migration.

## Native preparation and execution checks

User direction, 2026-11-03: use Kagari's static typing to establish stable Native
facts during Rust compilation, script checking and executable installation/linking
or generic-application preparation. Repeated execution consumes that evidence;
it does not rediscover signatures, Rust mappings or type/trait relationships.
Runtime source compilation means the engine's normal checked compilation pipeline,
not type inference or runtime specialization inside the Native execution adapter.
Independently loaded artifacts still pass bounded source-free validation.

The [Wasmtime typed-call comparison](#wasmtime-contexts-and-owned-roots) is a
direct reference for validating a signature before repeated typed invocation;
it does not remove ownership or changing-state admission from the call boundary.

### Existing preparation and remaining repeated work

The current [native registry](../crates/kagari-runtime/src/native/registry.rs)
validates registrations/imports, and
[linked bindings](../crates/kagari-runtime/src/native/binding.rs) already own
prepared scoped signatures and result adapters. Applied Native environments have
their existing immutable application owners. Reuse those owners and the
[prepared-type architecture](architecture.md#prepared-native-type-facts).

The current owning [typed invocation adapter](../crates/kagari-runtime/src/native/typed.rs)
nevertheless invokes result/argument Rust type-mapping checks per call, copies a
parameter pack and registers a root pack for conversion/callback lifetime. Some
custom conversion paths require protection; that does not justify repeating all
stable mapping checks or owning snapshots on every ordinary invocation. Review
each route rather than attribute this sequence to all Native calls.

### Earliest valid owner for each fact

| Stage | Facts established here | Execution consequence |
| --- | --- | --- |
| Rust compilation | Callback parameter/result shapes, scalar Rust types, typed receiver/value APIs, supported adapter traits and GC/call lifetimes | Missing or incorrectly shaped Rust returns fail compilation; ordinary entries need no manual result-slot completion protocol |
| Kagari static checking and checked lowering | Script parameter/result types, generic substitutions/bounds, selected trait/member evidence, declared access and checked call sites | Backends/native entries consume checked facts; no per-call trait search or type inference |
| Registration, artifact admission and executable linking | Actual installed declaration/provider identity, Rust-to-script mapping, physical ABI/storage compatibility, slot projections, result representation and selected-call routes | Reject mismatches before callback/converter effects; publish an executable adapter with exact supplying ownership |
| Concrete generic-application preparation | Closed applied signature, associated results, converters/layouts and callable witnesses for that application's immutable environment | Reuse the prepared application; shared generic code does not rebuild this evidence on every call |
| Invocation and short access admission | Current owner/installation, current target, active frame/storage, borrow permission and mutable lifecycle state | Admit changing state at its real boundary, then reuse the proof inside that closed operation |

This is staged proof, not a requirement to monomorphize all Native functions.
Generic code may remain shared behind a prepared application descriptor. If an
application first becomes available through an existing lazy preparation route,
finish its stable checks before entering its first body and reuse its ordinary
owner afterward. Do not turn missing preparation into a normal per-call fallback
that repeats the same work forever.

Conceptually a prepared entry supplies:

```text
existing linked binding / concrete Native application:
    exact installation, declaration and applied-type evidence
    typed Rust adapter and prepared argument/receiver projections
    prepared result encoding/publication and selected-call routes
    executable owner needed by an admitted computation
```

This summarizes facts on existing owners, not a second signature cache, global
proof table or extra prepared-entry layer. Ordinary calls select current code
and its matching prepared entry. An active call pins its admitted body as usual;
a cached type proof must not pin all subsequent calls to obsolete code. Reload
revalidates/replaces affected evidence before publication and never reuses it
merely because an ID, hash or memory address happens to match.

### Checks that remain at execution boundaries

| Check or operation | Why static typing alone is insufficient |
| --- | --- |
| Foreign/stale host handles, installation admission and untyped inputs | A correct declared type does not prove that this actual value belongs to the current Runtime or remains usable |
| Payload Present/Taken state and conflicting host/native loans | These can change through aliases and reentry; a checked type does not authorize access to removed or borrowed storage |
| Sticky terminal failure, depth and Faulted/Closed state | Execution state changes independently of signatures; keep required admission and the core's in-scope state gate, without cancellation-token polling |
| Data bounds, initialization, checked arithmetic and actual conversions | Types do not determine a collection's current length, a slot's initialization or a data-dependent range/parse outcome |
| Iterator revision and operation exclusion | Aliases can mutate the receiver between reads/callbacks |
| Allocation, capacity, publication, scanner coordination and charge commits | These protect physical storage and collection invariants, not script type inference |
| Enum branch and ordinary data reads | Choosing Some/None or Less/Equal/Greater inspects the result, not a redundant signature/type comparison |

A dynamic check is not automatically needed at every inner instruction. For
example, once a frame region and typed vector view establish owner, layout and
access, its scalar reads reuse those facts until an invalidating boundary. End
payload/slot loans before reentry and reacquire their changing-state evidence
afterward. Preserve foreign-input rejection and memory safety when relocating
checks; debug assertions may cover only invariants whose release proof is real.

### Certified typed paths and actual conversion boundaries

An erased Value proves neither a nominal script type nor an applied generic
identity. TypedValue<'gc, T> and Local<'call, T> below are proposed private-
constructor views backed by the entry's checked type application. Rust T markers
alone cannot distinguish arbitrary script types loaded later or two runtime
applications of a shared generic parameter. The prepared owner/application and
source admission supply that evidence; do not clone a TypeArgument per value.

For sealed/reviewed scalar and typed-value adapters, stable mapping validation
belongs to preparation and result shape follows from Rust's return type plus
the admitted constructors. Arbitrary user converters, reflection and erased/raw
provider values cannot acquire that proof by declaring a matching signature.
Validate their actual produced values at the conversion/admission boundary,
then let subsequent typed operations consume the admitted view. Data-dependent
conversion errors and user effects stay at their specified execution point.

Do not add trust flags, skip-check matrices, unchecked stdlib-only entrypoints or
blanket assumptions about safe IntoKagari implementations. Replace unsuitable
converter contracts coherently, separating stable mapping preparation from
value conversion, and keep one semantic conversion/storage implementation.

## Standard-library authoring proposal

These are proposed Rust-shaped function bodies, not implemented APIs or
compilable examples. They make the intended author experience reviewable before
GC cutover. Names can change; the separation between safe function code and
reviewed storage/call machinery is the acceptance target. The
[native collections plan](native-collections-plan.md) still owns actual
collection implementations, algorithms and their migration phases.

### Executable prototype and remaining boundary

The [orders 1-3 checkpoint](gc-redesign-plan.md#orders-1-3-executable-checkpoint)
records executable results and remaining closure work. The following components
now exist without activating them as production Runtime/stdlib entrypoints:

| Component | Concrete API and ownership |
| --- | --- |
| Memory and NativeCall | [Runtime call component](../crates/kagari-runtime/src/native/managed/call.rs); shared owner reference, short checked RefCell driver borrows, generative call locals and cleanup on error/unwind |
| Value forms | [Shared representation](../crates/kagari-runtime/src/native/managed/value.rs); scoped Copy Value, opaque non-Copy StoredValue and immutable RootedValue<T>, including owner checks for scalar snapshots |
| Typed arguments/results | [Arguments](../crates/kagari-runtime/src/native/managed/arguments.rs) projects supported outer tuples; [ScopedEntry](../crates/kagari-runtime/src/native/managed/entry.rs) publishes its region-dependent Rust output into a protected slot |
| Native payloads | [NativeObject/Leaf](../crates/kagari-runtime/src/native/managed/payload.rs); one EdgeFree declaration for simple Rust data, safe ordinary methods and reviewed generic provider construction/mutation |
| Vector views | [Stdlib provider](../crates/kagari-stdlib/src/managed/vector.rs); T is a sealed physical kind with a scoped View, not an arbitrary script generic marker; scalars and references share stored values |
| Callback-bearing fold | [Stdlib algorithm](../crates/kagari-stdlib/src/managed/fold.rs); revision-checked vector projection, reusable scalar slots and script callbacks with no live driver/payload loans |

The actual CallRegion<'access, 'gc> has two lifetimes. ScopedEntry uses a sealed
argument tuple and result-kind family; ordinary bodies return NativeResult<R>
with a real Rust tuple/scalar/Gc/Option shape. Explicit kind tuples currently
select the adapter. No manual return_unit or return_option protocol is needed.
The [provider examples](../crates/kagari-stdlib/src/managed/tests.rs) contain the
actual compilable push_scalar/pop_scalar and native payload bodies.

Object<P> is physical proof only. prepare_scalar checks supported closed scalar
FnDecl contracts once; it is not the eventual nominal/generic application binder.
The new scoped entry is not yet accepted directly by ModuleBuilder::bind. The
[VM fixture](../crates/kagari-vm/src/tests/native_boundary/managed_gc.rs) uses the
existing registration and typed script-call path as a scalar-only bridge; it
retains that path's current checking/conversion costs. No raw new-heap reference
is converted into an old Runtime Value. General prepared application/witness
ownership, declared write admission and untyped ingress must still be settled
before broad migration. Reuse the existing linker/application owners to do so.

The prototype deliberately supports only u64, Unit, native objects and a single
optional working-slot projection; it is not the full Value category inventory or
language Option ABI. Native read/publication can fail after take, so concrete
len/get/push/pop APIs return NativeResult where state can invalidate access.
There is no new public TypedValue<T> claiming full script identity merely from a
Rust P. The remaining function sketches below stay design targets; their generic
markers, scalar widths and Iterable coverage must not be read as implemented APIs.

### Call and storage capabilities

| Proposed capability | Contract |
| --- | --- |
| NativeCall<'call> | Current native invocation, prepared typed calls and bounded working slots; permits ordinary script calls with depth, terminal-failure admission and reload rules |
| CallRegion<'gc> | Short collection-excluded access region supplied by the adapter to a scoped entry; no script-call/collection capability; the adapter publishes the typed result before ending the region |
| TypedValue<'gc, T> | Scoped managed value with checked script application evidence; private constructors preserve the entry's type relationship without per-copy type resolution |
| Local<'call, T> | Typed coordinates of a protected slot, not a direct object pointer or independent Root; owner/generation/initialization checks remain where not already proved |
| Vector/VectorMut scoped handles | Library-defined typed storage access; the mutable handle proves declared write access without lending an unrestricted Rust &mut payload |
| next_into/invoke_into | Use prepared iterator/call routes, transfer arguments/results through authoritative frames, preserve execution admission/GC boundaries and release all payload loans before script execution |

These revised entries return NativeResult<R> with the real Rust output shape:
Unit is (), length is u64, and optional T is Option<TypedValue<T>> or an optional
typed Local. No public return_unit/return_option/return_from side channel is
required. The adapter writes the result; ordinary functions do not manually
complete a result slot and then separately return Ok(()).

There are two adapter shapes over one call/storage model. A callback-free scoped
entry returns its value to the adapter inside CallRegion; the adapter publishes
it before ending the region. A callback-bearing entry returns scalars or typed
call-owned Local results; the adapter transfers them before retiring the native
frame. No Value<'gc> escapes through an ordinary region callback. These are
proposed lifetime/type families, not currently implemented Fn trait bounds.

Rust catches a missing non-Unit result, a bool returned for u64, or () returned
for Option<TypedValue<T>>. Returning Ok(()) for a Unit function is complete and
correct; omission of the intended mutation is an algorithm bug for tests, not a
return-type error. Exact script declaration/provider compatibility is established
at checking/linking/application preparation, not by the Rust compiler knowing
every script loaded later. The erased backend entry may still return internal
status, but that is not the ordinary Native author's function signature.

Generic T/U/A/K/V below denote checked script-type markers; associated Rust
bounds are omitted. Iterator/callable/collection names in Local<T> denote linked
script views, not new script types. Mixed argument tuples are prepared projections
from the admitted invocation. Typed constructors/loads preserve their evidence.
No body repeatedly asks which argument type or result enum it is supposed to use.

local<T> obtains reusable working slots in the invocation, not independent
per-element roots. Adapter field projection uses fixed prepared fields, not
runtime strings or a retained payload loan. Actual initialization/borrow checks
remain where needed; read_scalar on Local<bool> does not repeat bool-vs-signature
matching. Operational failures propagate directly, following the
[user-selected removal policy](#option-wrapping-and-removal-failures).

The following capabilities are required underneath these bodies, not assumed to
exist because the examples are short:

- Library storage operations enforce source protection, publication, exact
  element contracts, scanner coordination, revision rules and committed capacity.
  Ordinary functions do not supply EditPlan, mark colors or scan cursors.
- Read/mutation facades restrict what can escape or change. They do not expose
  arbitrary &mut Vec<StoredValue> or merely wrap an unrestricted mutation in a
  safe-looking function. Standard and external providers use the same capabilities.
- The small provider kernel still needs reviewed unsafe construction/tracing
  proofs. Adding a normal method to an existing representation should not require
  another raw transport or liveness proof in that method's body.

### Length, emptiness and ordinary element operations

```text
fn len<'gc, T>(r: &CallRegion<'gc>, values: Vector<'gc, T>) -> NativeResult<u64> {
    values.len(r)
}

fn is_empty<'gc, T>(r: &CallRegion<'gc>, values: Vector<'gc, T>) -> NativeResult<bool> {
    Ok(values.len(r)? == 0)
}

fn get<'gc, T>(
    r: &CallRegion<'gc>,
    values: Vector<'gc, T>,
    index: u64,
) -> NativeResult<Option<TypedValue<'gc, T>>> {
    values.get(r, index)
}

fn set<'gc, T>(
    r: &mut CallRegion<'gc>,
    values: VectorMut<'gc, T>,
    index: u64,
    value: TypedValue<'gc, T>,
) -> NativeResult<()> {
    values.set(r, index, value)
}

fn push<'gc, T>(
    r: &mut CallRegion<'gc>,
    values: VectorMut<'gc, T>,
    value: TypedValue<'gc, T>,
) -> NativeResult<()> {
    values.push(r, value)
}
```

Vector len uses the fixed u64 script length contract. Storage accesses preserve
bounds and initialization checks: get returns normal absence, while invalid set
traps. Receiver/argument type correspondence was established before this entry;
values.push does not rediscover T from the payload on every call. set/push still
publish incoming references before their storage commit. Capacity
growth remains correctly charged if a later step fails; no blanket rollback or
whole-container copy is implied by the safe API.

### Pop and removal

```text
fn pop<'gc, T>(
    r: &mut CallRegion<'gc>,
    values: VectorMut<'gc, T>,
) -> NativeResult<Option<TypedValue<'gc, T>>> {
    values.pop(r)
}

fn remove<'gc, T>(
    r: &mut CallRegion<'gc>,
    values: VectorMut<'gc, T>,
    index: u64,
) -> NativeResult<Option<TypedValue<'gc, T>>> {
    values.remove(r, index)
}
```

The returned Rust Option<TypedValue<'gc, T>> holds a scoped value, so removing the
source edge does not expose an unprotected object inside the region. The adapter
publishes that typed result before closing the region. Empty/out-of-range results
become script None. An operational error propagates; completed removal
is not rolled back if later result handling fails. No PreparedReturn or Option
preallocation is part of this proposal. A result that must cross collection or
reentry still goes through the normal traced/rooted transfer before that boundary.

### Lazy map and filter

```text
fn map<'call, T, U>(
    call: &mut NativeCall<'call>,
    values: Local<'call, Iterable<T>>,
    mapper: Local<'call, Function<(T,), U>>,
) -> NativeResult<Local<'call, MapIterator<T, U>>> {
    let source = call.iter(values)?;
    MapState::create_local(call, source, mapper)
}

fn filter<'call, T>(
    call: &mut NativeCall<'call>,
    values: Local<'call, Iterable<T>>,
    predicate: Local<'call, Function<(T,), bool>>,
) -> NativeResult<Local<'call, FilterIterator<T>>> {
    let source = call.iter(values)?;
    FilterState::create_local(call, source, predicate)
}

fn map_next<'call, T, U>(
    call: &mut NativeCall<'call>,
    iterator: Local<'call, MapIterator<T, U>>,
) -> NativeResult<Option<Local<'call, U>>> {
    let (source, mapper) = MapState::fields(call, iterator)?;
    let item = call.local::<T>()?;
    let mapped = call.local::<U>()?;
    if !call.next_into(source, item)? {
        return Ok(None);
    }
    call.invoke_into(mapper, (item,), mapped)?;
    Ok(Some(mapped))
}

fn filter_next<'call, T>(
    call: &mut NativeCall<'call>,
    iterator: Local<'call, FilterIterator<T>>,
) -> NativeResult<Option<Local<'call, T>>> {
    let (source, predicate) = FilterState::fields(call, iterator)?;
    let item = call.local::<T>()?;
    let accepted = call.local::<bool>()?;
    while call.next_into(source, item)? {
        call.invoke_into(predicate, (item,), accepted)?;
        if call.read_scalar(accepted)? {
            return Ok(Some(item));
        }
    }
    Ok(None)
}
```

call.iter invokes the selected iter once outside a CallRegion and stores its
result in a protected typed Local. Construction does not invoke the mapper or
predicate. create_local opens a short region, initializes the prepared adapter
representation with traced source/callable edges and publishes its typed Local
before ending that region. It shares the reviewed fixed-field provider kernel,
not independently owning roots or a second object-construction implementation.

next_into invokes the selected next, decodes its ordinary Option and installs
the item in the supplied working slot. invoke_into may collect/reenter; it leaves
its result in a protected destination. Neither leaks Value<'gc> across the call.
Input consumption precedes mapper/predicate execution, aliases share progress,
and a callback failure does not restore consumed input. Structural revisions are
checked on the source's next affected read, preserving short-circuit behavior.

### For-each and fold

```text
fn for_each<'call, T>(
    call: &mut NativeCall<'call>,
    values: Local<'call, Iterable<T>>,
    action: Local<'call, Function<(T,), ()>>,
) -> NativeResult<()> {
    let source = call.iter(values)?;
    let item = call.local::<T>()?;
    while call.next_into(source, item)? {
        call.invoke_unit(action, (item,))?;
    }
    Ok(())
}

fn fold<'call, T, A>(
    call: &mut NativeCall<'call>,
    values: Local<'call, Iterable<T>>,
    initial: Local<'call, A>,
    reducer: Local<'call, Function<(A, T), A>>,
) -> NativeResult<Local<'call, A>> {
    let source = call.iter(values)?;
    let accumulator = call.local::<A>()?;
    let item = call.local::<T>()?;
    call.copy_into(initial, accumulator)?;
    while call.next_into(source, item)? {
        call.invoke_into(reducer, (accumulator, item), accumulator)?;
    }
    Ok(accumulator)
}
```

invoke_into explicitly permits a destination aliasing an argument slot: transfer
arguments into protected callee storage before publishing the result. Keep the
old accumulator protected for the invocation; do not overwrite it while arguments
are still being read. Iteration/call helpers do not poll cancellation tokens. No
per-item ScriptValue/transport/root round trip appears in the algorithm body.

### HashMap lookup with user Hash and Eq

```text
fn map_get<'call, K, V>(
    call: &mut NativeCall<'call>,
    table: Local<'call, HashMap<K, V>>,
    key: Local<'call, K>,
) -> NativeResult<Option<Local<'call, V>>> {
    let lookup = HashRead::begin(call, table)?;
    let (hash_fn, equal_fn) = lookup.key_functions();
    let hashed = call.local::<i64>()?;
    let candidate = call.local::<K>()?;
    let equal = call.local::<bool>()?;
    call.invoke_into(hash_fn, (key,), hashed)?;
    let hash = call.read_scalar(hashed)?;
    let mut search = lookup.bucket(call, hash)?;
    while let Some(entry) = search.next_key_into(call, candidate)? {
        call.invoke_into(equal_fn, (candidate, key), equal)?;
        if call.read_scalar(equal)? {
            let value = lookup.value_local(call, entry)?;
            return Ok(Some(value));
        }
    }
    Ok(None)
}
```

HashRead and the bucket cursor are library-private algorithm helpers. Their
operation protection permits GC but rejects conflicting receiver mutation under
the existing key-callback contract. Short source reads copy candidates into
working slots before callbacks; no borrowed table entry survives a call. An
entry coordinate is usable only while its operation remains valid. Publish its
selected value into an independently protected call slot before releasing the
operation; the entry adapter handles final result publication. Do not return a bare
EntryId after dropping the guard. Table layout and collision policy stay in the
library, while prepared Hash/Eq calls retain ordinary current-body selection.

### Sorting entrypoint and a merge step

```text
fn sort_by<'call, T>(
    call: &mut NativeCall<'call>,
    values: Local<'call, MutableVector<T>>,
    compare: Local<'call, Function<(T, T), Ordering>>,
) -> NativeResult<()> {
    with_sort_state(call, values, |call, state| {
        stable_merge_sort(call, state, compare)
    })
}

fn merge_run<'call, T>(
    call: &mut NativeCall<'call>,
    state: &mut MergeState<'call, T>,
    compare: Local<'call, Function<(T, T), Ordering>>,
) -> NativeResult<()> {
    let (left, right, ordering) = state.comparison_slots();
    while state.pair_into(call, left, right)? {
        call.invoke_into(compare, (left, right), ordering)?;
        let order = call.read_ordering(ordering)?;
        if order != Ordering::Greater {
            state.move_left(call)?;
        } else {
            state.move_right(call)?;
        }
    }
    state.finish_run(call)
}
```

This is an entrypoint and one merge step, not a complete sorting implementation.
with_sort_state, MergeState and the stable mergesort live privately in stdlib;
they are not new public SortSession APIs or collector algorithms. The remaining
code still implements splitting, scratch transfers and restoration explicitly.

The private operation owner establishes receiver protection and GC-visible
main/scratch state, reserves necessary backing and obtains reusable comparison
slots. Every initialized live element is traced through source/work storage or
protected argument slots at callbacks. pair_into leaves no payload loan alive;
move_left/right update storage and numeric recovery state through the shared
safe buffer operations. Equal elements choose the left run to preserve stability.

Before releasing operation protection, with_sort_state restores valid storage on
success, callback/runtime error and Rust unwind, without additional user
calls or fallible recovery allocation. Builtin Vec preserves the original element
multiset while allowing partial reordering and retaining completed callback
effects. Restore ownership/storage invariants, not the original order or business
state. This is distinct from adding rollback to ordinary pop/remove.

The algorithm must still meet stable worst-case O(n log n) comparisons/moves,
avoid an entry-time retention/index prepass. Callback-free transfers do not
need cancellation polls. Rooting only the receiver and comparator pair
does not make an ordinary Rust sort's invisible scratch safe for collecting
comparators. See [sorting contracts](spec/collection-access.md#foundation-algorithms)
and the [existing NC design](native-collections-plan.md#concrete-vec-sorting-api-proposal).

### Authoring acceptance before cutover

Use these concrete routes to refine the missing call/storage API under GCR00/GCR02.
No separate generic container framework or wholesale Native migration is selected
by these sketches. Inspect actual function and provider code: a short facade does
not establish simplicity if each helper repeats transport conversion, root packs,
type preparation or bespoke barrier/accounting protocols internally.

Prototype the ordinary reads, push/pop, lazy callback next, key lookup and sorting
transfer boundaries using existing contract tests. Normal methods on an existing
representation should contain no raw transport or manual root/scanner protocol.
Provider safety, independent external use, failure propagation, borrowing and
measured access/copy costs remain acceptance evidence, not assumptions inferred
from the example line counts.
Require positive and compile-fail prototypes for typed returns/argument tuples,
scoped-result publication and generic application provenance. Trace one prepared
call and repeated calls to distinguish stable mapping work from changing-state
admission. No steady-state argument/result signature walk, generic witness rebuild
or automatic parameter Vec/RootSet snapshot is acceptable on a route whose
prepared typed frame already supplies the same proof and retention. Keep genuine
custom-conversion work and report its cost separately.

## Mechanisms consulted and differing assumptions

Reviewed on 2026-11-03. The comparisons below select mechanisms for individual
boundaries, not one runtime to copy wholesale. Pinned releases/commits identify
the inspected source. Wasmtime's linked documentation identified itself as
51.0.0-dev when reviewed; it is a dated architectural reference, not a pinned
released API or a dependency selection. Existing GC algorithm/geometry research
remains at the [collector design](gc-implementation-design.md) and
[plan](gc-redesign-plan.md#reference-mechanisms-and-selected-boundaries).

| Kagari boundary | Main reference | Design section |
| --- | --- | --- |
| Reference roles and protected reads | [SpiderMonkey](#spidermonkey-reference-categories) | [Value forms](#value-categories-and-retention-forms), [admission](#admission-reads-and-conversion-capabilities) |
| Generative access regions | [gc-arena](#gc-arena-scoped-access) | [closed regions](#closed-regions-callbacks-and-failure) |
| Rust embedding contexts and retained handles | [Wasmtime](#wasmtime-contexts-and-owned-roots) | [host retention](#host-retained-values-and-replaceable-slots) |
| Stable Native mapping/signature preparation | [Wasmtime typed calls](#wasmtime-contexts-and-owned-roots) | [Native preparation](#native-preparation-and-execution-checks) |
| Actual frame roots and managed computation edges | [Lua](#lua-execution-storage-and-computation-edges) | [execution roots](#authoritative-execution-roots), [module graph](#module-state-code-dependencies-and-reload) |
| Execution hooks versus GC service | [Lua hook/GC separation](#lua-execution-hooks-and-separate-gc-control) | [execution policy](#execution-without-cooperative-cancellation-polling) |
| Native borrow exclusion and take | [mlua](#mlua-native-borrows-and-extraction) | [native providers](#native-payloads-provider-safety-and-extraction) |
| Runtime/collector responsibilities | [MMTk](#mmtk-runtime-and-collector-responsibilities) | [ownership](#responsibility-and-ownership-model) |
| Independent root registration and release | [OpenJDK OopStorage](#openjdk-host-root-storage) | [host slots](#replaceable-host-slots) |
| Old computations and current ordinary calls | [Java redefinition](#java-redefinition-and-retained-computations) | [reload](#module-state-code-dependencies-and-reload) |

### SpiderMonkey reference categories

Source: the official embedding examples'
[ESR78 GC Rooting Guide](https://github.com/mozilla-spidermonkey/spidermonkey-embedding-examples/blob/esr78/docs/GC%20Rooting%20Guide.md),
especially Handles, heap tracing and PersistentRooted. This is a versioned
mechanism reference, not a current SpiderMonkey API migration guide.

- **Mechanism:** Handle refers to an already protected location; Heap carries a
  reference that still needs tracing; PersistentRooted supplies independent
  retention. MutableHandle gives controlled access to a protected mutable slot.
  The guide warns that putting persistent roots inside otherwise collectible
  structures can keep cycles and their associated global alive.
- **Application:** separate scoped access, traced StoredValue edges and host
  roots. The object/closure/environment/static cycle must use internal edges;
  only actual host/publication/execution owners introduce roots.
- **Different assumptions:** SpiderMonkey moves objects and updates handles.
  Its C++ rooting/static-analysis discipline is not a Rust generative-lifetime
  proof. Handle is not literally Value<'gc>, and Heap is not our Edge API.
- **Kagari evidence:** exercise source clearing, protected field/slot reads,
  last-root release and cycle reclamation. An internal owning root must not be
  accepted as an implementation shortcut merely because retention tests pass.

### gc-arena scoped access

Source: gc-arena 0.7.0 at commit
`49c4c041816be2dc6ce8a928ace838e37a066813`,
[Arena](https://github.com/kyren/gc-arena/blob/49c4c041816be2dc6ce8a928ace838e37a066813/src/arena.rs)
and [Gc](https://github.com/kyren/gc-arena/blob/49c4c041816be2dc6ce8a928ace838e37a066813/src/gc.rs).

- **Mechanism:** generative invariant lifetimes constrain GC pointer use to
  arena callbacks; mutate excludes collection, while collection is explicitly
  driven outside those callbacks.
- **Application:** Value<'gc> is cheap temporary transport inside a closed region.
  Stored/rooted destinations protect anything needed after that region ends.
- **Different assumptions:** Kagari stores unbranded static carriers, has
  independently admitted host handles and requires resumable provider/root scans.
  Reentry ends the region; the callback's entire execution is not GC-excluded.
  The source does not prove those added adapter contracts.
- **Kagari evidence:** compile-fail scope escape/mixing cases, alias reads and
  writes, protected removal, region unwind and nested reentry through fresh
  admission. Retain the core Faulted gate after caught failures.

### Wasmtime contexts and owned roots

Sources: official [Rooted/OwnedRooted documentation](https://docs.wasmtime.dev/api/wasmtime/struct.Rooted.html),
[rooting source](https://docs.wasmtime.dev/api/src/wasmtime/runtime/gc/enabled/rooting.rs.html)
and [Store/context documentation](https://docs.wasmtime.dev/api/wasmtime/struct.Store.html),
version 51.0.0-dev as reviewed above.

For Native preparation, also consult
[Func::typed](https://docs.wasmtime.dev/api/wasmtime/struct.Func.html#method.typed)
and [TypedFunc](https://docs.wasmtime.dev/api/wasmtime/struct.TypedFunc.html),
the same documented version. Typed conversion checks the signature up front;
the resulting typed function carries parameter/result correspondence into later
calls. Store ownership and actual execution traps remain distinct runtime concerns.
Kagari applies that separation to prepared Native adapters, with its additional
generic application, host loan, reload and GC-root contracts.

- **Mechanism:** Store owns instance/host state and Caller lends access during
  host calls. Scoped Rooted references and independently owned roots have
  different unrooting lifetimes; operations use the associated context/store.
- **Application:** preserve Runtime identity at host admission, separate retained
  script values from short inspection, and require the current context to access
  payloads. Root ownership does not itself grant concurrent heap access.
- **Different assumptions:** Wasmtime's scoped roots can fail dynamically after
  their rooting scope ends; they are not our collection-excluded Gc brand.
  Its documented Wasm instance retention until Store drop is not Kagari's
  retired-version reclamation model and must not be adopted accidentally.
- **Kagari evidence:** wrong-Runtime and post-shutdown admission, scalar-only
  handle policy, root clone/drop across host threads, nested host calls and
  independently retained results after the creating invocation retires.

### Lua execution storage and computation edges

Source: Lua 5.4.8 [lgc.c](https://github.com/lua/lua/blob/v5.4.8/lgc.c),
particularly traversethread, traverseLclosure and traverseproto, and
[lstate.h](https://github.com/lua/lua/blob/v5.4.8/lstate.h).

- **Mechanism:** tracing a thread visits its actual live stack and open upvalues.
  A closure traces its prototype/upvalues; prototypes trace constants and nested
  prototypes. Computation and data dependencies participate in the managed graph.
- **Application:** scan authoritative execution storage instead of maintaining
  per-variable host roots. Represent managed closure/environment/static links as
  actual graph edges, with code backing retained by the computations needing it.
- **Different assumptions:** Lua has dynamic value/type and built-in layout
  policies. Its incremental stack treatment includes atomic revisiting; it does
  not prove Kagari's resumable direct source, incoming barriers, typed metadata
  or reload policy. An unreachable coroutine is not automatically a permanent root.
- **Kagari evidence:** active and independently scheduled parked computations,
  slot/window reuse and relocation, return transfer, bounded source progress and
  object/code-state cycle reclamation under genuine-root release.

### Lua execution hooks and separate GC control

Source: the [Lua 5.4 reference manual, lua_sethook](https://www.lua.org/manual/5.4/manual.html#lua_sethook)
and [lua_gc](https://www.lua.org/manual/5.4/manual.html#lua_gc), reviewed on
2026-11-03; this is a manual-version reference, not a pinned implementation.

- **Mechanism:** instruction-count hooks are explicitly installed and can be
  disabled; GC control is a separate interface. Count hooks apply to Lua code,
  not arbitrary running C function bodies.
- **Application:** execution interruption and GC service are separate policies.
  Kagari selects no cooperative execution-cancellation polling, while retaining
  the collector's required service boundaries and storage safety checks.
- **Different assumptions:** Lua hooks also serve debugging, and disabling a
  hook says nothing about Kagari's scheduler, typed calls or sticky failures.
  Do not copy Lua's hook API or remove independent observation machinery.
- **Kagari evidence:** a token changed during finite synchronous execution does
  not interrupt later work; GC/reentry and async retirement remain correct.

### mlua native borrows and extraction

Source: mlua 0.12.2 [AnyUserData::take](https://github.com/mlua-rs/mlua/blob/v0.12.2/src/userdata.rs#L747-L766)
and [userdata storage/borrow exclusion](https://github.com/mlua-rs/mlua/blob/v0.12.2/src/userdata/cell.rs).

- **Mechanism:** extraction checks exclusive access, moves the Rust value out
  and makes later operations on the userdata fail through a destructed state.
  Existing aliases do not authorize another payload access or extraction.
- **Application:** separate native object identity from the continued presence
  of its Rust payload. Take must honor active loans and invalidate old aliases'
  payload access without requiring them to disappear immediately.
- **Different assumptions:** mlua uses Lua metatable/userdata and Rust storage
  machinery, not our invariant Gc<NativeObject> shell. Its exact destruction,
  synchronization and payload-content policies are not selected. Kagari still
  requires NativeTake's recursively edge-free proof and its own panic policy.
- **Kagari evidence:** already admitted in-scope aliases, failed take with active
  loans/operations, moved-value errors, unchanged shell descriptor, scan restart,
  charge release and exactly-once disposal after aliases disappear.

### MMTk runtime and collector responsibilities

Source: MMTk core 0.33.0 documentation/source revision
`44bd8fe8a3d5e14a5a269e2b690c108aa0121ce9`,
[VMBinding](https://github.com/mmtk/mmtk-core/blob/44bd8fe8a3d5e14a5a269e2b690c108aa0121ce9/src/vm/mod.rs)
and [Scanning](https://github.com/mmtk/mmtk-core/blob/44bd8fe8a3d5e14a5a269e2b690c108aa0121ce9/src/vm/scanning.rs).

- **Mechanism:** separate VM object-model, scanning and collection interfaces;
  the runtime supplies object/root information to generic GC machinery.
- **Application:** keep language type/access/reload policy with Runtime, while
  the core owns physical marking, barriers and reclamation. Give roots and
  object scanning explicit, reviewed contracts rather than implicit access to
  every Runtime store.
- **Different assumptions:** this is a responsibility reference, not a decision
  to introduce MMTk-style VM bindings, worker scheduling or foreign-store graph
  nodes. Kagari's core owns its allocations; RootSource enumerates roots to them.
  The Runtime remains exclusively driven and Send, not Sync.
- **Kagari evidence:** the standalone core depends on no language crates, has
  no Program/Environment mark cases and cannot collect while omitting the paired
  source. Review the concrete ownership split alongside the existing graph tests.

### OpenJDK host root storage

Source: OpenJDK 21+35
[OopStorage](https://github.com/openjdk/jdk/blob/jdk-21%2B35/src/hotspot/share/gc/shared/oopStorage.hpp).

- **Mechanism:** independently allocated off-heap reference entries can be
  released and enumerated by GC without making each client a new collector.
  Occupancy and empty-block reclamation distinguish live registrations from
  historical storage capacity.
- **Application:** one core root registration/release protocol supports host
  retention and replaceable packs; Runtime supplies the coupled language tag.
- **Different assumptions:** HotSpot supports concurrent interaction and moving
  collectors. Its handle pointers and synchronization are not our root lease,
  exclusive driver or allocation-free release-notification protocol. It does
  not require immutable RootedValue or uniquely owned HostValueSlot.
- **Kagari evidence:** tag/target atomic replacement, failed publication,
  object-to-scalar transitions, final clone drop, stale/foreign leases and
  released-root history not inflating empty collection work.

### Java redefinition and retained computations

Source: Java SE 25
[Instrumentation.redefineClasses](https://docs.oracle.com/en/java/javase/25/docs/api/java.instrument/java/lang/instrument/Instrumentation.html#redefineClasses(java.lang.instrument.ClassDefinition...)).

- **Mechanism:** existing active frames continue their old method bytecodes;
  new invocations use the redefined method. Static values are preserved and
  class initializers are not rerun just because the class was redefined.
- **Application:** retain the body needed by an existing computation while
  ordinary calls select current code; code publication does not reset persistent
  state. Body retention and call selection need distinct owners.
- **Different assumptions:** this contract is not Kagari's module publication,
  cold Future/closure retention, generic environment or metadata layout. It does
  not prove type-only holders release executable backing, establish an index
  pruning algorithm or justify copying JVM class-loader lifetime wholesale.
- **Kagari evidence:** an old resumed body calls an updated helper, retained
  closures/Futures keep their selected body, shared static cells keep values,
  failed publication stays isolated and old type/layout holders acquire no new
  executable admission. R13 remains a separate unresolved ownership issue.

### What the comparisons do not decide

The references support role separation, protected access, explicit roots,
authoritative execution storage, controlled native extraction and computation
retention. The following concrete choices remain Kagari proposals:

| Draft choice | Why it still requires Kagari evidence |
| --- | --- |
| One private Repr<R> family | Reusing tag/scalar handling can avoid duplicate code, but the references do not establish its API, physical layout or performance. |
| Non-Copy StoredValue | This intentionally restricts escaped direct carriers in Rust; other runtimes use different tracing/analysis guarantees. Audit provider ergonomics and controlled copy operations. |
| Immutable RootedValue plus unique mutable slot | A deliberate API/aliasing model, not a prerequisite for safe rooting. Audit existing consumers and preserve required replacement behavior. |
| RootDriver physically owning ExecutionRoots | Makes complete-source inclusion structural, but actual slot publication, scan frontiers, scheduling and unwind still need proof. |
| Stable NativeObject shell | Preserves typed GC representation during take; its scanner, payload placement and charge policy remain our responsibility. |
| Managed module state with Rust-owned code backing | Requires an actual reachability/index model; neither Lua nor Java redefinition settles Kagari's nominal metadata and executable lifetimes. |

These mechanisms support access and retention separation, not an unmeasured
speed claim. Kagari's combination of packed dynamic fields, Rust payload
destruction, native extraction, exact computation retention and resumable
providers requires its own reviewed adapters. The generic collector must remain
independent of those language policies.

## Open decisions

The proposed baseline above is editable. The following choices remain explicit
GCR00/GCR02 inputs, not a parallel implementation queue:

| Decision | Baseline or unresolved point | Evidence needed before cutover |
| --- | --- | --- |
| Value family | Private Repr with scoped/stored/rooted wrappers; exact names/category inventory provisional | Actual frame, field, host conversion and native call prototypes; no duplicate semantic implementations |
| Execution cancellation | User-selected removal of cooperative Runtime token polling; async retirement and compilation cancellation are separate | Finite synchronous continuation after a token change, no hidden Native/backend polls, preserved admission/GC/failure gates and queued/suspended retirement |
| Native preparation/typed entries | Bounded scoped output/argument families and scalar preparation are executable; general script nominal/generic binding, direct bind integration and ergonomic kind inference remain open | Existing compile-fail results plus prepared application provenance, raw/custom-converter admission, steady-state check attribution and reload invalidation |
| Runtime object metadata | Nominal/type facts and managed dependencies must be reachable without executable ownership in core descriptors; exact header/descriptor placement open | Real struct/interface/native layouts, descriptor lifetime and type-only retention review |
| Tagged/packed storage | Ordinary opaque carriers first; exact encoding and any missing core provider API open | Size/alignment and provenance checks on native/wasm32; exact scalar/reference round trips and paired field/transfer measurements |
| Option wrapping | User-selected future direct-value wrapping; exact encoding remains later work; pop/remove require no wrapper-allocation transaction | Preserve None versus nested Some(None), payload tracing/type identity and direct propagation of Runtime errors |
| Host mutable slots | Immutable RootedValue plus uniquely mutable RootSet-backed slot proposed | Audit existing set/clone consumers, owner rejection, tag coupling, failed replacement and public Send/Sync behavior |
| Nonpersistent call values | Borrowed host values stay outside ordinary persistent Repr; exact argument/tuple representation open | Existing typed borrow and outer-tuple call coverage, scoped reentry and rejection of indirect escape |
| Native providers/take | Reviewed tracing boundary and stable Native shell proposed | Existing derives/payload consumers audited, in-scope take aliases, operation exclusion, scanner state, committed charge and exactly-once disposal |
| Semantic lookup indexes | No all-version roots or stale direct-pointer admission; validity/pruning protocol open | Real module/environment/static cycle and lookup-after-retirement proof |

Do not choose compact encodings or broaden generic provider APIs solely to
complete a hypothetical future use. Resolve missing capabilities using these
concrete Runtime paths and record implementation decisions in the existing plan.

## Validation and integration handoff

The [implementation order](gc-redesign-plan.md#implementation-order-for-runtime-integration)
first closes executable Native/value/provider proofs, then complete root/graph
coverage, Runtime ownership/policy separation and the coherent physical cutover.
This document supplies GCR00/GCR02 inputs, not a parallel implementation ledger,
and does not close those phases. GCR03 must enumerate affected crates and any finite intermediate build
boundary in the existing plan. Cutover removes the old production collector,
joint graph/metadata sweep and replaced root/representation owners; it does not
keep compatibility forwarding APIs or two collectors over the same heap.

Implementation acceptance reuses existing contract owners and adds cases only
for distinct uncovered boundaries:

| Boundary | Required behavioral evidence |
| --- | --- |
| Scoped/store/root forms | Scope escape rejected; source clearing does not invalidate a scoped handle; foreign/expired host admission fails; stored carriers cannot be independently revived |
| Frame transfers | Slot reuse, moved scan frontiers, argument/return publication and scalar-bank handling; no per-copy root registration |
| Native preparation | Stable type/mapping/witness facts prepared once under the actual owner, wrong Rust/script contracts rejected before effects, typed return mistakes rejected by Rust, and changing-state/custom-value checks preserved |
| Removal/error propagation | Empty/out-of-range remains None; operational errors stay errors, completed removal is not rolled back for later result failure, and removed values stay protected through the actual return boundary |
| Fields and slots | Access/type/trap order, alias-visible writes, complete old-value preservation on failed publication, object/scalar host-slot replacement and exact scalar bits |
| Reentry and async | Outer roots survive nested collection; suspended computations retain values/body; trap/depth and queued/suspended retirement cleanup disposes once |
| Native | Complete provider scanning, cursor repair, capacity/partial-edit accounting, take with existing aliases and loans, moved-value errors and destructor panic containment |
| Modules/reload | Object/code/static cycle reclamation, current ordinary calls, retained closure/Future bodies, lookup pruning and source-free/ABI admission |
| Teardown/threads | Inert handles after shutdown, safe cross-thread handle clone/drop, Runtime Send/not Sync and no peer dereference during disposal |

Useful existing evidence includes [root-source contracts](../crates/kagari-gc/tests/root_source.rs),
[incremental mutation](../crates/kagari-gc/tests/incremental.rs),
[Runtime ownership](../crates/kagari-runtime/src/gc/tests/gc_ownership.rs),
[host roots](../crates/kagari-runtime/src/gc/roots/tests.rs),
[native accounting/take](../crates/kagari-runtime/src/gc/native/tests.rs) and
[closed-region/reentry tests](../crates/kagari-runtime/src/frame/cursor/tests.rs).
Standalone results do not establish actual Runtime integration or safety of a
new representation. Preserve semantic coverage while replacing obsolete
ID/directory-specific assertions with their new contract owner.

Use focused local tests for changed contracts and a small directly affected
regression set, plus the repository's required structure/fmt/workspace-Clippy/diff
checks for implementation. Full workspace and complete feature/backend/platform
matrices remain CI-only. Documentation revisions need content/link/diff checks.

Measure actual field reads/writes, generic value/frame copies, allocation,
retained host results, mixed object/code cycles and collection latency under
comparable pressure policies. Record toolchain/machine/profile/features,
parallelism, cache state and workload; separate preparation from execution and
physical managed bytes from total Runtime/allocator/RSS costs. Preserve existing
GC performance gates at their plan owner rather than replacing them with a
smaller standalone benchmark.

This unpublished internal redesign does not itself require artifact/runtime ABI
identifier bumps. Preserve bounded validation and source-free execution. Change
wire/native contracts only when a concrete affected representation requires it,
then regenerate disposable development products at a coherent checkpoint.
