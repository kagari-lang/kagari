# Failure and Side Effects

This is the authoritative v1 failure contract. Execution cleanup is not business
rollback. A failed call stops further execution but preserves completed effects.

## Business-value propagation

`?` on built-in Option/Result returns None/Err from the nearest function or
closure. Result propagation converts the error payload through the destination's
`From` implementation when needed and preserves the original Err metadata.
It does not capture a second origin for the propagated Err, roll back prior
side effects, catch a trap, or clear root termination. Normal frame return
releases that frame's roots and host resources. Constructors,
explicit conversions and matching are defined in [builtins](builtins.md#option-and-result).
See [error origins and diagnostic stacks](error-reporting.md) for reporting rules.
A general propagation trait and Error/cause protocol are deferred.

## Failure classes

- Business rejection is an ordinary Result value that scripts can handle.
- Bounds errors, checked overflow, and invalid handles trap the current root call.
- Cancellation and resource exhaustion terminate the root call; ordinary Result
  handling cannot swallow them or clear root termination through reentry.
- An internal invariant failure makes recovery unsupported; isolate or discard
  the affected runtime rather than presenting it as an ordinary business error.

Ordinary traps and termination release frames, host leases,
temporary roots, and reentry state. Cleanup does not poll cancellation or resume
script execution. Owned native waits may invoke their bounded cancellation hooks
and destroy host-owned payloads, wakers and hook captures under the
[completion cleanup contract](host-interop.md#completion-transport-foundation).
The runtime may be reused after ordinary failure cleanup, but a cleanup invariant
failure quarantines it. Completed business mutations remain present. Rust panic
recovery is not a transaction mechanism.

The approved foundation collection algorithms likewise provide no operation-wide
transaction. Sort, reverse, retain and dedup propagate callback, receiver and
cancellation failures while preserving valid storage and completed effects.
Vec sorting retains every original element, but may change their order;
custom container writes may leave partial progress. This does not relax individual
write validation or the separate host-path and reload publication contracts.
The [collection contract](collection-access.md) defines these mutation guarantees.

Frame scopes share one session-owned stack. Each scope unwinds only its own suffix
on failure, preserving suspended callers that may handle an ordinary nested trap.
Frame roots and call counters are released after cancellation and quarantine too.
Invalid frame slots, mutation through a suspended scope and out-of-order scope
destruction are engine invariant failures and quarantine the runtime.
Missing linked function/module/path slots and unsupported verified call targets
also indicate broken execution state, not recoverable script traps. Quarantine
still unwinds frames and releases their roots and call-depth accounting.
Candidate access to any static cell is an execution-phase violation, including
Ready cells and indirect or host-reentrant access. It leaves the runtime usable
and the active entry unchanged; publication does not grant candidate execution
permission to read shared state.
Holding a mutable module-instance borrow across another runtime entry violates
the execution ownership contract; the second entry reports an engine fault and
quarantines without a Rust borrow panic.
An invalid reference in registered or module-state GC roots is likewise an
engine invariant failure: collection quarantines the runtime instead of
reporting a recoverable script trap.
The loader rejects bytecode functions that can fall through their final
instruction. Reaching the end of a verified function without a terminator is an
engine fault and quarantines the runtime rather than synthesizing a `Unit` return.
Host resource scopes apply the same unconditional cleanup to temporary roots and
borrow leases. Rejected argument preparation releases prior leases before returning;
expired/foreign tokens and borrow conflicts never invoke the target callback.
Quarantine and termination forbid new borrowing while still permitting guard Drop.

Candidate execution and ordinary calls release their version retention, frames,
temporary roots, and host resources on failure, including after quarantine.

## Static initialization failures

[Lazy static activation](module-activation.md#lazy-static-activation) distinguishes
failure to admit an attempt from failure after it starts. Admission failure before
the transition to Initializing leaves the cell Uninitialized. After that
transition, a trap, cancellation, depth exhaustion, observer/GC failure or dropped
execution stack must leave the cell Failed, never stranded Initializing. A
successful result is rooted through frame retirement and semantic validation
until the Ready commit.

Reentering an Initializing cell is an initialization cycle and poisons it
immediately. This includes indirect cycles and native callbacks that swallow the
reentry error: the outer initializer cannot subsequently commit success over the
poisoned cell. Dependencies that already completed remain Ready. Cleanup retires
all still-active initialization attempts without callbacks that resume script.

Failed cells retain a detached cause of at most 1024 UTF-8 bytes and an optional
origin span, not live frames or an executable recipe root. Later access reports
the retained initialization failure; it does not retry, reset the cell, or turn a
past cancellation into fresh session cancellation. Compatible reload does not
repair Failed cells or replace their recipes. A fresh installation has fresh cells.
Runtime quarantine, when required by the underlying failure, still takes precedence.

A business `Result::Err` returned by the initializer is an ordinary Ready value,
not Failed. Failure does not roll back completed host effects or mutations to
other cells/objects. For example, if A initializes B successfully and then traps,
A is Failed while B remains Ready. Static initialization is not a transaction over
all effects that occurred while computing its result.

## Modification guarantees

Standard container and typed-path mutations validate the target, types,
declared access, arithmetic, and capacity before committing. Rejection
leaves the target's logical contents unchanged by that operation; backing-capacity
reservations have the separate accounting rule below. This does not undo effects of
evaluating the receiver, indexes, or RHS, nor earlier operations in the call.

Heap allocations and container growth validate types, checked logical occupancy
and capacity before publishing logical contents. A rejected new object publishes
neither its identity nor its charge. `HeapCharge` separates logical units from
retained backing bytes; checked storage transitions commit the charge for storage
actually retained, not merely the requested growth. A multi-step reserve can grow
one backing allocation and then fail at a later step. The operation publishes no
new entry, but keeps and charges the successfully reserved capacity. For example,
a hash insertion can retain extra table capacity without inserting its key.

Removal, clear and truncate reduce logical occupancy without necessarily releasing
backing capacity. Library Vec main/scratch storage stays installed and charged
through callback failure and same-payload recovery. Sweep and native take release the runtime-recorded charge,
never call a provider's size hook again; take leaves the inert identity's logical
unit until its remaining aliases are no longer traced. Charge release does not
poll execution and still works after termination or quarantine. Sealed Task failure
publication likewise accounts its retained error message during cleanup; it grants
no permission for new execution or ordinary mutations. Accounting underflow is an
engine invariant failure, not a wrapping counter or business rejection.

These counters describe attributed occupancy/pressure, not cumulative allocations,
RSS or a heap quota. See [GC accounting](runtime.md#gc-heap) for byte coverage and
safepoint scheduling. Resource failure does not cause emergency collection inside
allocation or a scoped storage mutation.

Standard removals returning an Option prepare that result before removing the
entry. Result allocation failure leaves the entry present. Peak heap occupancy
includes the prepared result before the removed entry's units are released.
Resource exhaustion keeps its structured classification through builtins and
the VM; it does not become a script trap or a different resource limit.

A host path mutation prepares both the value update and its dirty record before
commit. The commit does not allocate fallibly or invoke arbitrary script code.
It cannot update the field and then report an ordinary failure because dirty
record creation failed. Custom adapters must satisfy this same contract.

HostPathAdapter uses a fallible `with_prepare_write` callback returning a
`PreparedHostPathWrite` action. Preparation must not change the target. It resolves
the stable host location, checks host invariants, and reserves anything the action
needs. Dropping an uncommitted action releases those reservations. The runtime
reserves ledger capacity before
running the action and appending the prepared record. There is no fallible write
callback or synchronous dirty hook. The host consumes the ledger after execution.

A provided old-value reader must succeed; its error cannot be ignored by Set.
Old/new heap values and path arguments remain rooted through preparation and
commit. Preparation may explicitly collect. Commit cannot execute script, mutate
script heap storage, allocate through the runtime, or collect. Unwinding commit
panics and attempts to enter execution quarantine the runtime, even when a host
action swallows the rejected nested operation. These return EngineFault (embedding
EngineInvariant), not a business Result or ordinary trap. There is no promise of
target rollback after a broken commit invariant. Other completed effects remain.

Cancellation and resource termination remain recorded for the active session and
cannot be swallowed to continue executing that root. `ResourceState` owns admission
of these failures: native conversion limits and frame preparation failures record
termination before returning, while native and VM boundaries admit propagated
errors before unwinding their frames. Constructing a detached error alone does not
terminate a session. Standalone conversion without an active session returns its
error without establishing root termination. Business-value admission rejection,
such as a rejected Task spawn, remains a value rather than resource termination.

Admission retains the first termination and its origin. For example, a host that
ignores a nested native output-conversion resource failure cannot make another
successful reentry or return success for that root. An ordinary Trap observer
failure cannot replace the recorded termination; EngineFault or quarantine still
takes precedence. Releasing the final session scope clears termination without
quarantining the runtime. New root calls use fresh execution state; a reused
cancelled token still rejects immediately. Commit actions do not poll cancellation,
so termination cannot split a field write from its dirty record.

External database writes, network messages, and other irreversible effects belong
to explicit host APIs or host transactions. Custom host functions may partially
succeed only under an explicitly documented result contract. Whole-call atomicity
requires a host-owned transaction; Kagari does not infer or emulate one.

## Acceptance examples

- Deducting gold and then failing to grant an item retains the deduction.
- A failed array insertion or checked path update leaves its target unchanged.
- Dirty-record preparation failure does not commit the corresponding field write.
- A trap during nested host reentry clears all execution resources exactly once.
- Reentry shares the outer version, cancellation state and call-depth limit.

## Async outcomes

Future/Task waiting, directional cancellation and Task failure provenance are
implemented. The
[async executable contracts](../async-execution-design.md#executable-contracts)
define the checked handoff.

Await returns T, without an implicit Result wrapper. Business Err is an ordinary
value; cancellation, traps and resource exhaustion terminate the waiting execution
with their original class and provenance. Cancelling a waiter detaches it without
cancelling its target; a target termination propagates to its waiters. Task
self/transitive dependency cycles trap before parking. One driver terminal
transition decides cancellation versus completion; queued payload is not success.

Task reports identify the reported task separately from the originating failed task.
Dependent cancellation reports use Dependency while task_origin retains the initial
task's cause. Ordinary owned-root SDK errors also expose task_origin when awaiting a
failed Task. Traces preserve the original failure snapshot. A host waiter-wake panic
quarantines the runtime after terminal publication; it cannot rewrite a committed
producer outcome or restore its already retired execution. Guarded owned-session
retirement continues releasing waits, frames and roots after a host cleanup panic;
a retirement drain that discovers quarantine also drains the remaining inactive
owned sessions, including those whose handles the host retains. Runtime destruction
uses the same guarded path for standalone owners as well as Task-owned executions.
