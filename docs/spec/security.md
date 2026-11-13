# Installation Access and Execution Control

Kagari primarily embeds trusted scripts. The host chooses the native functions,
types, objects and services it installs. Installation determines available APIs;
an offline signature never supplies an executable implementation. Binding still
validates identity, complete signatures, layouts, effects and generation data.

There are no language profiles, per-execution function allowlists, general
capability bits or script CPU/memory quotas. The CLI has no security profiles.
Debugger attachment and JIT selection are host tooling/backend decisions.

## Language and interface contracts

Member visibility, val/var, readonly views, declared reflection metadata and host
getters/setters remain enforced. These are static or runtime interface contracts,
not configurable permissions. Reflection cannot manufacture undeclared members,
bypass readonly fields or expose unrestricted Rust references. Typed host paths
preserve scoped borrows, no-escape, identity, lifetime and generation checks.

Candidate initialization cannot perform external effects, access any static cell,
or escape its staged installation through an external call route. Publication itself
requires an idle synchronous host boundary and cannot run inside callbacks/reentry.
These atomic-publication safeguards are execution-phase invariants, not a host
privilege matrix.

## Execution control

`RuntimeConfig::limits` contains nonzero `RuntimeLimits::max_stack_bytes`
(default 8 MiB) and `max_native_depth` (default 32). Each independent execution
owns a bounded account for committed frame, scalar/managed slot, outgoing
argument, Native/local and execution-source backing capacity. Empty frames count
toward it; active uncommon control leases its entry footprint. Fixed Runtime/
session directory headers and unassigned global pool spare capacity remain
separately reported storage. Synchronous reentry shares that execution account; parked independent
executions retain their own accounts. This bounds committed execution capacity,
not managed heap, transient reallocation backing, arbitrary Rust callback state
or process RSS. Exceeding the account reports `StackOverflow`; fallible allocator
failure within it reports `AllocationFailure`.

Native/compiled entry has a separate nested-entry guard shared across Runtime
sessions. On supported native hosts, the guard also queries remaining OS thread
stack and rejects entry below a 128 KiB reserve. It does not grow/switch stacks.
On unsupported hosts, Wasm and Miri only the configured nested-entry ceiling is
available. Arbitrarily small host thread stacks and recursion inside Rust
callbacks remain host-owned; the guard cannot recover an already exhausted
thread stack. Guard exhaustion reports `StackOverflow` with a distinct Native
admission reason and releases admitted guards during unwind.

Nonexecuting Host conversion/metadata preparation uses its own default 8 MiB
source account; it does not consume an idle execution's configured budget.
Active conversions use that execution's source/account.
`ExecutionOptions::cancellation`
provides cooperative cancellation. Nested calls, native callbacks and synchronous
reentry share the root token and sticky termination state. A handler cannot
swallow termination, stack exhaustion or allocation failure and resume that root.
Cleanup always releases frame roots,
borrow leases and generation retention. A fresh root can execute after cleanup;
a reused cancelled token continues to reject execution.

The interpreter and supported JIT poll at valid execution boundaries. Long native
work uses `NativeCall::poll` at appropriate chunks or callbacks. Indivisible
primitive bulk operations, including primitive slice sorting, complete before the
next poll. Prepared mutations commit their target and dirty record together.
Cancellation cannot forcibly preempt a blocking Rust callback or guarantee a
universal wall-time response bound. Host timers may request cancellation.

## Retained validation

Static typing, verified bytecode/native contracts, bounds and overflow checks,
GC roots, ownership, borrow validity, exact computation retention and invariant-failure
quarantine remain required. Structural decode and compiler work limits protect
analysis and artifact validation; they are independent of execution control.
GC occupancy and call/module counts serve collection and lifecycle diagnostics.
Optional tracing owns observation; there are no always-updated instruction,
cumulative-allocation, host-call or reflection quota counters.

Hosts own service admission, IO deadlines, rate limiting and isolation for
untrusted code. Kagari does not promise hard preemption, process isolation or
per-script CPU/memory guarantees. See [runtime](runtime.md),
[failure semantics](failure-semantics.md) and the
[execution contract](execution.md).
