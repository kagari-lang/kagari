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

Candidate initialization cannot perform external effects or access module state
outside its pinned program. This protects atomic publication and hot reload; it
is an execution-phase invariant, not a host privilege matrix.

## Execution control

`RuntimeConfig::limits` contains `RuntimeLimits::max_call_depth`, defaulting to
`Some(256)`; `None` disables that protection. `ExecutionOptions::cancellation`
provides cooperative cancellation. Nested calls, native callbacks and synchronous
reentry share the root token and sticky termination state. A handler cannot
swallow termination and resume that root. Cleanup always releases frame roots,
borrow leases and generation retention. A fresh root can execute after cleanup;
a reused cancelled token continues to reject execution.

The interpreter and supported JIT poll at valid execution boundaries. Long native
work uses `CallContext::poll` at appropriate chunks or callbacks. Indivisible
primitive bulk operations, including primitive slice sorting, complete before the
next poll. Prepared mutations commit their target and dirty record together.
Cancellation cannot forcibly preempt a blocking Rust callback or guarantee a
universal wall-time response bound. Host timers may request cancellation.

## Retained validation

Static typing, verified bytecode/native contracts, bounds and overflow checks,
GC roots, ownership, borrow validity, reload pinning and invariant-failure
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
