# Failure and Side Effects

This is the authoritative v1 failure contract. Execution cleanup is not business
rollback. A failed call stops further execution but preserves completed effects.

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

Initialization cleanup retains its authority after execution is quarantined.
An unfinished initialization transitions to Failed and releases its version
retention through its lifecycle guard; it does not request a new execution or
ordinary module-write permission while handling the original failure.

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
its own usage, shared by initialization, entry execution and nested scopes.

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
