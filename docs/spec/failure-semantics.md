# Failure and Side Effects

This is the authoritative v1 failure contract. Execution cleanup is not business
rollback. A failed call stops further execution but preserves completed effects.

## Business-value propagation

`?` on built-in Option/Result returns the original None/Err value from the nearest
function or closure. It preserves the origin captured when Err was constructed. It does not capture
a second stack, wrap the error, roll back prior
side effects, catch a trap, or reset an execution budget. Normal frame return
releases that frame's iteration guards, roots and host resources. Constructors,
explicit conversions and matching are defined in [builtins](builtins.md#option-and-result).
See [error origins and diagnostic stacks](error-reporting.md) for reporting rules.
A general propagation trait and Error/cause protocol are deferred.

## Failure classes

- Business rejection is an ordinary Result value that scripts can handle.
- Bounds errors, checked overflow, and invalid handles trap the current root call.
- Cancellation and resource exhaustion terminate the root call; ordinary Result
  handling cannot swallow them or reset the root budget through reentry.
- An internal invariant failure makes recovery unsupported; isolate or discard
  the affected runtime rather than presenting it as an ordinary business error.

Ordinary traps and termination release frames, host leases, iteration guards,
temporary roots, and reentry state. Cleanup does not consume script fuel or invoke
arbitrary user code. The runtime may be reused after cleanup; completed business
mutations are still present. Rust panic recovery is not a transaction mechanism.

Frame scopes share one session-owned stack. Each scope unwinds only its own suffix
on failure, preserving suspended callers that may handle an ordinary nested trap.
Frame roots and call counters are released after cancellation and quarantine too.
Invalid frame slots, mutation through a suspended scope and out-of-order scope
destruction are engine invariant failures and quarantine the runtime.
Missing linked function/module/path slots and unsupported verified call targets
also indicate broken execution state, not recoverable script traps. Quarantine
still unwinds frames and releases their roots and call-depth accounting.
Candidate access to module state outside its pinned program is a capability
denial instead; it leaves the runtime usable and the active entry unchanged.
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

## Modification guarantees

Standard container and typed-path mutations validate the target, types,
permissions, arithmetic, and resource availability before committing. Rejection
leaves the target unchanged by that operation. This does not undo effects of
evaluating the receiver, indexes, or RHS, nor earlier operations in the call.

Heap allocations and container growth share the runtime's resource counters.
Validation, live-heap and cumulative-allocation limit checks, and capacity
reservation precede the content and counter commit. Failure does not charge
either counter. Replacing a map entry or adding an existing set key consumes no
growth units; duplicate constructor keys count only once. Removal and GC reduce
live occupancy, but do not refund allocation usage within the root call. Runtime
allocation counters remain cumulative across roots; each root's limits apply to
its own usage, shared by the entry execution and nested scopes.

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
reserves ledger capacity and checks `ResourcePolicy::max_dirty_records` before
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
cannot be swallowed to continue executing that root. Releasing its final scope
clears termination without quarantining the runtime. New root calls use new
budgets; a reused cancelled token still rejects immediately. Commit actions do not
poll cancellation, so termination cannot split a field write from its dirty record.

External database writes, network messages, and other irreversible effects belong
to explicit host APIs or host transactions. Custom host functions may partially
succeed only under an explicitly documented result contract. Whole-call atomicity
requires a host-owned transaction; Kagari does not infer or emulate one.

## Acceptance examples

- Deducting gold and then failing to grant an item retains the deduction.
- A failed array insertion or checked path update leaves its target unchanged.
- Dirty-record preparation failure does not commit the corresponding field write.
- A trap during nested host reentry clears all execution resources exactly once.
- Reentry shares the outer version, permissions, and remaining resource budget.
