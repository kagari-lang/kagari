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

## Modification guarantees

Standard container and typed-path mutations validate the target, types,
permissions, arithmetic, and resource availability before committing. Rejection
leaves the target unchanged by that operation. This does not undo effects of
evaluating the receiver, indexes, or RHS, nor earlier operations in the call.

A host path mutation prepares both the value update and its dirty record before
commit. The commit does not allocate fallibly or invoke arbitrary script code.
It cannot update the field and then report an ordinary failure because dirty
record creation failed. Custom adapters must satisfy this same contract.

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
