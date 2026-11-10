# Reading the Runtime Execution Model

This map describes implemented ownership after the immutable Program and prepared
reload migration. The [architecture overview](../architecture.md),
[execution specification](../spec/execution.md) and
[activation specification](../spec/module-activation.md) own the contracts.
Read the [call walkthrough](runtime/calls.md) for entry/return costs and the
[reload walkthrough](runtime/reload.md) for preparation, publication and retention.
The [scalar execution guide](runtime/scalar-execution.md) maps the readable
instruction loop, explicit source generation and bounded structural exceptions.
Measurements and their limits belong in the [performance record](../performance-baseline.md).

## Physical ownership

```mermaid
flowchart TD
    PRODUCT["Immutable SDK Program<br/>Arc ProgramState, Send + Sync"] --> CODE["Verified bytecode, execution/layout facts<br/>source inputs and optional verified MIR"]
    SDK["SDK Runtime"] --> VM["Vm"]
    VM --> RT["Core Runtime<br/>Send, not Sync"]
    RT --> STORE["ModuleStore<br/>local current map and declaration allocator"]
    STORE --> RECORD["Local records, statics, final call bindings<br/>host/native authority and native eligibility"]
    STORE --> DESC["Current and retained exact ProgramDescriptors"]
    DESC -.-> PRODUCT
    HANDLE["InstalledProgram / Module<br/>identity only"] -.-> ID["Installation identity"]
    RT --> GC["GcHeap"]
    GC --> RESOURCE["ResourceState<br/>sessions, frames and value banks"]
    RT --> SERVICE["Scheduler, host registry, loans and caches"]
    COMPILER["Preparation-owned NativeCompiler"] --> PAGES["Immutable NativeCode / finalized pages"]
    PAGES -.-> RECORD
```

Solid arrows show containment; dashed arrows show shared references, not script GC
reachability. `ModuleStore.current` is Runtime-local `RefCell` state. Installation
handles contain no current pointer or executable lease. Live queries borrow the
supplying Runtime; immutable source snapshots can be copied independently.
Publication requires `&mut Runtime`, and synchronous admission excludes publication,
close and conflicting loans during active execution. Scoped backend/NativeContext
views retain dynamic admission and borrow checks; they do not permit concurrent
heap access. Moving Runtime between host threads is an ownership transfer.

Execution records and value banks remain physically below GcHeap through
ResourceState. Splitting these owners, redesigning slots and changing the collector
are separate work. Arc references keep Rust owners alive; explicit roots and joint
object/metadata traversal determine script reachability.

Sources: [SDK Program](../../crates/kagari/src/program.rs),
[SDK Runtime](../../crates/kagari/src/runtime.rs), [Vm](../../crates/kagari-vm/src/vm.rs),
[core Runtime](../../crates/kagari-runtime/src/lib.rs),
[ModuleStore](../../crates/kagari-runtime/src/module.rs),
[Installation](../../crates/kagari-runtime/src/module/installation.rs),
[ResourceState](../../crates/kagari-runtime/src/resource.rs).

## Error reasons and diagnostic context

[RuntimeError](../../crates/kagari-runtime/src/error.rs) owns one typed
[ErrorReason](../../crates/kagari-runtime/src/error/reason.rs) and optional boxed
context. Its category and stable KG_* code are derived from the reason. There is
no independently assignable `(kind, message)` constructor or cached formatted
message. Display produces text when a diagnostic consumer requests it.

```rust
use kagari_runtime::error::{RuntimeError, context::ErrorLocation, reason::ErrorReason};
use kagari_types::scalar::error::ScalarError;

let error = RuntimeError::from(ScalarError::ShiftOutOfRange)
    .at(ErrorLocation::Field("count".into()));
assert_eq!(error.reason(), &ErrorReason::Scalar(ScalarError::ShiftOutOfRange));
assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
assert_eq!(error.to_string(), "field count: integer shift out of range");
```

Numeric leaves, fixed Runtime traps, cancellation, stale handles, index failures
and quarantine invariants retain typed reasons. Existing loading/declaration/borrow/
resource diagnostics still carry owned details within their named families; their
finer owner-specific error migration is queued in the
[refactor design](../runtime-refactor-design.md#typed-errors-and-diagnostic-ownership).
HostError retains the current custom text/trace contract and implements Error;
Native/library providers can supply trap or index diagnostics through their explicit
provider variants. Runtime does not acquire Vec/String implementation policies.

Field, variant and element coordinates own their labels/data. Positions attach
from inner to outer and display in reverse order. Core builtin bridges append a
RuntimePrimitive coordinate to the original error. Reflective read/write wrapping
preserves the original RuntimeError as a typed source, including its positions and
execution provenance. VM builtin admission normalizes directly to its RuntimeError
route; obsolete BuiltinError/ReflectionError variants are removed from VmError.
The internal Runtime helper wrappers and SDK diagnostic mapping still exist; this
is not the queued VM/crate consolidation.

Trace capture stays at the failing program point, independently of eventual text
formatting. The first populated trace/task origin survives annotation and transport;
async provenance and bounded/incomplete trace rules are unchanged. Diagnostics own
no script values or program leases. Retained-task accounting includes owned reason
payloads, nested source errors, context boxes and position backing/labels; shared
trace metadata remains excluded. A context-free scalar failure allocates neither
a message nor a context box. Rich failure context is allocated on failure only.

A 2026-11-10 standalone layout probe used the same Windows i9-12900K machine and
rustc 1.99.1 toolchain recorded in the
[scalar guide](runtime/scalar-execution.md), default Runtime features, workspace dev
opt-level 1 library products, warm default target cache and default Cargo parallelism.
The probe itself used `rustc --edition 2024 -O` and size_of, with no timed workload.
The previous kind/String/trace/task-origin field model occupies 72 bytes under this
toolchain; ErrorReason is 40 bytes and RuntimeError is 48 bytes. Corresponding
Result<u64, E> shallow sizes are 72 and 48 bytes. The old value is a reconstruction
of the previous fields, not an old execution image. Rich context/source allocations
are additional owned storage and are counted separately; this is not a total error
memory or speedup claim. Rust layouts are not a stable ABI. Reproduce with size_of
for these types and the former field record; no ignored probe file is required.

## From a program product to execution

1. Source services check/lower a package, or artifact preparation independently
   verifies portable executable contracts. Optional retained source is diagnostic
   metadata; it does not replace verification or supply a compilation capsule.
2. SDK `Program` shares immutable bytecode, checked call/layout facts, bounded
   historical demands, optional immutable source inputs and optional verified MIR.
   Preparation has no Runtime, script effects or installed Rust captures.
3. Loading adopts declaration identities into the recipient scope without remapping
   a complete executable image. Runtime checks its own installed providers and builds
   local state, type admissions and final call bindings. Lazy statics stay lazy.
4. Publication commits current and prepared function links together. Host entry or
   resume admits authority; ordinary direct bytecode calls consume final entries.
5. Each Runtime retains independent heap, statics, providers, scheduler and native
   eligibility even when products, native pages or preparation inputs are shared.

Sources: [identity admission](../../crates/kagari-runtime/src/module/admission.rs),
[load/publication](../../crates/kagari-runtime/src/loading.rs),
[dispatch facts](../../crates/kagari-runtime/src/module/dispatch/facts.rs),
[linked execution](../../crates/kagari-runtime/src/module/linked_execution.rs).

## Core data structures

### Program product, installation and publication

```text
Program { state: Arc<ProgramState> }              SDK immutable shared product
ProgramState {
    bytecode: VerifiedProgram,                    no owner back-reference
    history: Arc<RetainedDemands>,                metadata, not executable ancestry
    expected_base: Option<Weak<ProgramState>>,
    compilation: optional Arc<CompilationInputs>,
    mir: optional Arc<VerifiedMirProgram>
}
InstalledProgram { installation: Arc<Installation> }
Module { installation: Arc<Installation>, namespace: ModuleIdentity }
Installation { registry_owner, id, root }         identity only
ModuleStore {
    current: RefCell<HashMap<InstallationId, Arc<ProgramDescriptor>>>,
    next_declaration: Cell<u64>,
    inner: RefCell<ModuleStoreInner>, ...
}
ExecutableModule { program: Arc<ProgramDescriptor>, slot: ModuleRef }
ProgramDescriptor {
    code: VerifiedProgram,                       attaches shared SDK owner, if any
    definitions: Arc<DefinitionAdmission>,
    dispatch: Arc<PublicationDispatch>,
    retained_demands, layouts, layout_admissions, source,
    installation, root, modules, ...
}
PreparedSelection { owner: ModuleKey, target, mapping: Arc<PublicationMapping> }
```

These sketches omit fields and are not binary layout promises. `ExecutableModule`
retains an exact descriptor; physical registration, owner/generation and installation
liveness still govern execution. A surviving descriptor cannot revive a closed
Runtime. `Runtime::program(&InstalledProgram)` recovers the exact shared SDK product;
a low-level publication without that owner cannot fabricate source preparation.

Product DefinitionIds remain lexical. `DefinitionAdmission` supplies checked
product/local correspondence; closed Runtime types and lexical binders keep explicit
provenance. Shared pure dispatch/layout facts grant no recipient provider authority.
Operation requirements and associated-output lookup compare admitted Runtime keys;
their lexical witnesses and supplying type environments remain separate. Native
selected calls retain their prepared scoped signatures, including native types
without script nominal origins. Enum value equality and hashing normalize the same
nominal arguments and variant IDs; payload/layout admission still checks provenance.
Local call entries contain generational keys and metadata, not strong predecessor
code references. The weak route registry lets saved host bindings follow current
implementations without retaining obsolete executable bodies. Expired identities are
pruned at the next publication; their metadata can remain until then, but current
Direct calls never consult this registry.

### Cache ownership

| Owner | Validity and lifetime |
| --- | --- |
| VerifiedProgram | Shared pure execution/layout/dispatch facts and successful initial historical demands, exact immutable product lifetime; cancellation does not poison preparation |
| Module records | Local linked calls, applied function/environment views, layouts and installed Native bindings; prepared/replaced with publication and traced with their actual owners |
| Native dependency validation | Temporary exact immutable required-catalog map identities, one callback-free link/revalidation pass against one product/recipient; registry owns keyed Arcs and each pass starts empty |
| NativeCompiler | At most 4096 exact revision/function/configuration/runtime-ABI/helper-ABI decisions; no Runtime access or execution-time cache lookup |
| NativeCode and installed entries | Immutable finalized page owners; each recipient checks exact revision and local policy/authority before installation |

A successful publication does not mean all old payload is immediately collectible.
Whole versions remain for actual closures, Futures, frames, queues, roots and type/
layout provenance. The unresolved [R13 finding](../review.md#r13--type-and-layout-provenance-retains-retired-code)
can retain an initial whole Program for inherited statics/types. There is no strong
executable predecessor chain, per-body unloading guarantee or Future-only lifetime.

### Session, active scope, frame and value window

```text
SessionStore {
    records: RefCell<generational SessionState slots>,
    frames: RefCell<Vec<Vec<ExecutionFrame>>>, ...
}
SessionState {
    id: SessionId,
    root: ExecutableModule,
    options: ExecutionOptions,
    owner: RefCell<Option<Arc<ExecutionOwner>>>,
    pending: RefCell<Option<PendingWait>>,
    queued_future, queued_factory,
    host_scopes, frame_scopes, termination, ...
}
ExecutionSession<'runtime> {
    id: SessionId,
    resources: &'runtime ResourceState,
    _activation: SynchronousActivation<'runtime>
}
ExecutionStack<'runtime> {
    session: ExecutionSession<'runtime>,
    base: usize, id: u64, parked: bool
}
ExecutionFrame {
    loaded: ExecutableModule,
    target: CallableTarget,
    environment: Option<TypeEnvironment>,
    links: Option<Arc<LinkedFunction>>,
    native_entry: NativeEntryState,
    ip, executing, completed,
    slots: FrameSlots,
    slot_layout: Option<Arc<FrameLayout>>,
    return_to: ReturnDestination,
    invocation, mutations, initializer, ...
}
FrameSlots { owner: u64, index: usize, generation: u64 }
ExecutionValues {
    payloads: Vec<u64>,
    initialized: Vec<bool>,
    values: Vec<Value>,
    windows, allocation_order, ...
}
Window {
    generation, ranges,
    program: ExecutableModule,
    environment, invocation, native_application,
    slot_layout, ...
}
```

The persistent session record owns the computation's state; ExecutionSession and
ExecutionStack are active scoped access to it. Stack `base` identifies the suffix
that this scope must unwind. An ordinary synchronous nested call adds a frame to
the existing execution; synchronous host reentry shares that execution where
admitted. An owned asynchronous execution can keep its frames after the active
scope ends. Spawned Tasks have their own scheduling/ownership relationships.

FrameSlots is a checked locator, not a pointer into Vec backing. FrameLayout maps
logical slots to physical scalar/managed locations, including scalar slot reuse.
Window records carry dependencies alongside protected values. Frame and window
metadata both retain relevant owners; these shared references and publication
steps must remain visible when reviewing per-call costs.

Do not read ExecutionState as a per-task Running/Waiting enum. Its actual phase
is Ready/Closed/Committing/Collecting/Quarantined. NativeEntryState is separately
Script/Pending/Running/Complete, while TaskState and execution-owner readiness
describe scheduling. Current execution status is distributed across these owners.

Sources: [sessions](../../crates/kagari-runtime/src/session.rs),
[session storage](../../crates/kagari-runtime/src/session/store.rs),
[frames/scopes](../../crates/kagari-runtime/src/frame.rs),
[value windows](../../crates/kagari-runtime/src/frame/values.rs),
[physical layouts](../../crates/kagari-runtime/src/module/execution/layout.rs),
[runtime phase](../../crates/kagari-runtime/src/execution_state.rs).

## Values and roots

Value, storage location and object identity are separate concepts:

```text
Value::I32(7)             scalar transport value
FrameSlots + Location    where a logical frame value is currently stored
HeapObjectId             owner + slot + generation of a shared heap object
RootedValue              lease on a runtime-owned root-table entry
```

Current Value is a Copy tagged transport. Scalars can stay as bits in the scalar
bank; managed locations use Value records. Struct, enum, string and other managed
variants refer to heap identities. Option/Result enums still use heap objects;
the deferred enum-layout model has not changed production storage.

Copying Value does not copy its object graph or register a root. Arc cloning
similarly concerns Rust ownership and does not, by itself, publish a script root.

```mermaid
flowchart LR
    ACTIVE["Active and parked<br/>execution windows"] --> ROOTS["Joint object and<br/>metadata traversal"]
    HOST["Host RootTable<br/>leases"] --> ROOTS
    INSTALL["Current installations<br/>and retained programs"] --> ROOTS
    PENDING["Waits, tasks and<br/>protected operations"] --> ROOTS
    ROOTS --> OBJECT["Object identities<br/>and payload edges"]
    ROOTS --> META["Type environments, applications<br/>and program edges"]
    OBJECT --> META
    META --> OBJECT
```

The root graph can cross object/program/metadata stores. The current collector
marks the joint graph before detaching unreachable storage; it does not scan
arbitrary Rust locals. NativePayload tracing reports internal edges, while public
Object/ScriptValue handles establish explicit retention. One external root per
collection element is not the native collection ownership model.

An ordinary return can retire the callee window before writing the caller only
inside a closed, non-collecting transition. A root return must be protected by its
caller/driver before any later collecting operation. These ordering guarantees,
not a temporary Rust Value local, prevent a retention gap.

ObjectStorage uses checked directory identities and concrete payload stores.
Mutable Rust host state stays outside the script heap and is accessed through
installed host scopes; registered Native payloads have a separate traced-object
contract. Temporary payload loans prohibit conflicting execution/collection.

Sources: [Value](../../crates/kagari-runtime/src/value.rs),
[roots](../../crates/kagari-runtime/src/gc/roots.rs),
[collection](../../crates/kagari-runtime/src/gc/collection.rs),
[native storage](../../crates/kagari-runtime/src/native/storage.rs),
[host ownership](../runtime-ownership-and-host-api-design.md#gc-boundaries-and-extension).

## Execution lifetime

The following states are explanatory lifecycle labels, not a proposed Rust enum.

```mermaid
stateDiagram-v2
    [*] --> Admitted: create owned execution
    Admitted --> Active: drive or resume
    Active --> ParkedReady: safe slice ends
    Active --> Waiting: await reports Pending
    ParkedReady --> Active: resume
    Waiting --> ParkedReady: completion notification
    Active --> Terminal: result or error
    ParkedReady --> Retiring: cancellation or owner abandonment
    Waiting --> Retiring: cancellation or owner abandonment
    Active --> Retiring: termination observed
    Terminal --> Retiring: transfer result and finish
    Retiring --> [*]: drain and release owned resources
```

| Boundary | Concrete work and retained state |
| --- | --- |
| Enter/resume | CHECK idle driver, runtime/owner identity and no conflicting native loan; enter SynchronousActivation; select the active session and restore depth |
| Drive | VM polls termination, dispatches Script/Await/NativeEntry/NativeReturn actions, invokes safepoints/observers and executes a closed region |
| Park | CHECK owned root, no conflicting scopes/initializers/mutations or Running Native frame, and storable retained values; keep frames/windows, end the active scope |
| Wait | Session PendingWait owns the native/Task/group wait, destination, output type and awaitable root; it is not a saved Rust stack frame |
| Wake | ExecutionOwner readiness/cancellation atomics and Waker notify the driver; the notifying thread does not enter Runtime |
| Return | Adapt while dependencies remain protected; retire/publish within a closed transition; root an escaping result through the driver/SDK |
| Error | Preserve trace/termination and completed effects; unwind the active scope's frame suffix and release its windows/guards |
| Abandon owned handle | Drop requests retirement; the Runtime driver later drains it. Drop is not immediate arbitrary-thread heap destruction |
| Close/drop Runtime | Revoke live installation authority, close task/scheduler ownership and drain resources; surviving descriptors/handles do not grant post-close execution |

Synchronous Native callbacks cannot park arbitrary Rust locals. Parking is denied
while a Native frame is Running. Async Native providers instead retain explicit
Future/wait state. Publication also counts callbacks/reentry/cleanup as active
even when an active-session pointer is temporarily hidden.

The VM driver carries established actions directly across internal calls/returns;
it does not necessarily perform a fresh general action lookup at every frame
transition. Closed scalar regions reuse admitted bank borrows and release them
before allocating preparation or callbacks. This is existing optimization to
preserve, not another universal entry check to introduce.

Sources: [VM driver](../../crates/kagari-vm/src/executor/driver.rs),
[runtime actions](../../crates/kagari-runtime/src/frame/driver.rs),
[closed regions](../../crates/kagari-runtime/src/frame/cursor.rs),
[parking](../../crates/kagari-runtime/src/frame/owned.rs),
[waits](../../crates/kagari-runtime/src/frame/waiting.rs),
[owned execution](../../crates/kagari-runtime/src/session/owned.rs),
[close](../../crates/kagari-runtime/src/scheduler/lifecycle.rs),
[Runtime drop](../../crates/kagari-runtime/src/task/cleanup.rs).

## Remaining review scope

Slot/window initialization, value transfer, interface computation metadata, typed
Native conversion and collection still have ordinary execution costs. Their owners
and follow-up scope are in the [roadmap](../implementation-roadmap.md#runtime-execution-model-review).
Removing reload-specific locks/selection does not establish zero call cost or
complete the separate slot, Native API, collector, enum-layout or R13 work.
