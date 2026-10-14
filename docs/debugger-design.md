# Interactive Debugger Design

Status: queued proposal; implementation has not started. The user confirmed
synchronous debugger waiting on the executing host thread. This document records
that decision and a bounded implementation proposal, not implemented APIs or an
instruction to start work. The [roadmap](implementation-roadmap.md#other-queued-designs)
owns activation. Recheck the inventory below when this work is activated.

## Decision and observable behavior

At a safe debug point, stop advancing the script and wait for debugger commands
inside the current synchronous call chain. Preserve the Rust stack, script frames,
host borrow scopes, GC roots and pinned code generations. Continuing exits the
debug wait and resumes the existing execution; it does not replay the entry.

For example:

```text
Host: begin skill operation
  -> Script: compute damage
       -> breakpoint: inspect damage, wait for continue
  -> Host: apply returned damage
  -> Host: broadcast result
```

Applying damage and broadcasting cannot happen while the nested script is paused.
The same rule applies to `script A -> Rust callback -> script B -> breakpoint`:
the Rust callback has not returned, so its local state and outstanding scopes remain
alive. Do not unwind the stack to implement a pause.

The stopped scope is the executing host thread and its synchronous call chain,
not the whole process. Other host threads, isolates and external services may
continue. Global application freezing requires host coordination and is outside
this proposal. Debug inspection is not a transactional snapshot of external state.

No whole-interpreter continuation conversion, fiber stack, general suspendable
host API or debugger-specific `DriveResult::Paused` is required by this design.
An owned execution pauses inside its current `drive()` call; that call returns
only after the debug wait ends and an ordinary drive boundary is reached.

## Existing implementation and gaps

Inventory reviewed at `2202c344`:

| Owner | Reusable foundation and current limit |
| --- | --- |
| [VM debug session](../crates/kagari-vm/src/debug.rs) | Source breakpoint resolution, safe-point/step decisions, execution identity, rooted frame/binding snapshots and binding-name watches. `record_pause` appends a snapshot and returns; it does not stop execution. |
| [Internal adapter](../crates/kagari-vm/src/debug_protocol.rs) | Rust requests/events and snapshot inspection. Requests currently require access to `Vm`; events can be flushed after execution. This is not a live transport or a DAP server. |
| [Runtime observer](../crates/kagari-runtime/src/observer.rs) | Runtime owns the observer; dispatch temporarily borrows the observer and session frames. These borrows constrain where interactive requests can run. |
| [Interpreter driver](../crates/kagari-vm/src/executor/driver.rs) | Existing instruction/trap observation and safe execution boundaries. Stop decisions must be consumed before the observed instruction proceeds. |
| [Owned execution](../crates/kagari-vm/src/vm/owned.rs) and [reentry](../crates/kagari-vm/src/reentry.rs) | Explicit script frames and bounded driving already exist. Synchronous reentry preserves a Rust call chain; no conversion to a resumable host callback is planned. |

The [debugger specification](spec/debugger.md) contains a broader target feature
set than the implementation. Conditional breakpoints, general expression watches,
interactive transport and complete value inspection are not established by the
current snapshot tests. Reuse current metadata and checks rather than introducing
a second debugger or a parallel execution engine.

## Ownership and communication

```text
IDE <-> protocol/transport worker
                  |
           command/event queues
                  |
       executing host thread
         VM safe debug point
           -> decide to stop
           -> process debug commands and wait
           -> resume the existing call chain
```

- VM debug control owns breakpoint/step policy and the paused command loop.
  Runtime continues to own execution frames, roots, host access validation and
  the installed observer. Protocol code does not define execution semantics.
- The transport worker parses messages, queues commands and sends detached
  responses. It never calls into `Vm`, borrows runtime state or dereferences script
  values. Runtime remains Send, not Sync; no ownership transfer occurs while paused.
- The SDK exposes a command endpoint for a host-selected debug session. Reconcile
  it with the [queued host facade](host-api-refactor.md) at activation; concrete
  public type names and crate placement are not fixed here.
- DAP and CLI code own transport, protocol IDs, source presentation and launch or
  attach orchestration. Start with modules in existing owners; a new crate needs
  a concrete dependency or distribution reason, not an empty placeholder.

Only one controller may issue resume/step decisions for an isolate. Commands and
responses carry session and stop identity so delayed requests cannot accidentally
inspect or resume a later stop. Network disconnection and queue closure must wake
the waiting execution thread. Bound pending inspection work; a full response queue
must not prevent cancellation or disconnect handling.

## Stop, inspect and resume

Conceptual sequence, not a proposed public API:

```text
reach existing safe debug point
  -> resolve breakpoint/step/manual-pause decision
  -> finish short observer/frame borrows
  -> establish current stop identity and inspection roots
  -> publish stopped event immediately
  -> wait for inspect / breakpoint update / continue / step / cancellation
  -> invalidate stop-scoped inspection handles and release their roots
  -> resume at the same program point or propagate cancellation normally
```

The current observer callback holds guards while inspecting frames. Do not simply
put a blocking loop inside that callback and call the existing adapter back into
the same borrowed observer. Prefer returning a stop decision to the driver, ending
dispatch borrows, then servicing commands on the same thread. The exact internal
handoff is an activation decision; it must retain one observer authority and never
hold a frame-bank, heap or observer guard across a wait or a reentrant operation.
Longer-lived host borrow scopes belonging to the preserved call chain remain live.

Resume consumes this occurrence of the stop check without executing an instruction
twice or immediately stopping at the same occurrence again. A loop revisiting the
same location must still hit the breakpoint. Stepping uses source points, selected
frame identity/depth and `ExecutionId`; other async roots cannot consume a step.
Repeated points on one line, recursive calls, frame return and temporary breakpoint
removal require explicit handling at the existing debug owner.

A pause request while running is cooperative: observe it at a valid debug point.
It cannot interrupt arbitrary Rust host code. A synchronous host call is opaque;
if it reenters script, the nested script may stop while the outer Rust frame stays
on the stack. Stepping never invents Rust source frames or Rust local inspection.

Frame and variable handles exposed to the adapter are valid only for their stop.
Responses contain display data and opaque references, not runtime `Value` handles
for another thread to use. Continue, detach, cancellation and session end release
inspection roots. Avoid retaining every stop snapshot indefinitely; tests or
diagnostic recording must have explicit retention ownership and limits.

The first watch subset remains binding reads. Add bounded expansion of available
script values through existing metadata/access rules, with cycle handling and
pagination where needed. Unavailable captures or host fields must be reported as
unavailable, never reconstructed by bypassing validation. Do not invoke arbitrary
script `Debug` implementations, getters or host callbacks merely to display a value.
General expressions, mutation and side-effecting evaluation are deferred.

## Host borrowing, cancellation and lifecycle

Pausing does not release application locks or host borrows owned by outer Rust
frames. Inspection must respect those scopes and reject conflicting access rather
than deadlock waiting for a resource held by the paused call chain. No arbitrary
host callback, task drive or general application event pump runs inside the debug
wait. In particular, waiting for a command must not require the blocked host thread
to execute its ordinary event loop.

The wait must wake on continue/step, explicit detach, disconnect and cancellation.
Cancellation exits through the normal execution error/cleanup path, preserving
committed effects and RAII cleanup; it does not kill the thread or force-return
through a Rust callback. A trap stop preserves the original failure and relevant
frames until inspection ends; continuing then propagates that failure rather than
retrying the failed instruction. Engine faults must retain quarantine behavior and
must not enter an inspection loop that assumes a valid runtime.

Lifecycle defaults proposed for activation review:

| Event | Proposed behavior |
| --- | --- |
| Explicit detach-and-continue | Release stop handles, disable interactive control and resume. Remove the installed observer only at a boundary where replacement is legal. |
| Unexpected transport loss | Cancel the debugged root through normal cleanup; do not silently continue a partially completed host operation. An alternative host policy must be explicit. |
| Debug stop/terminate request | Request cooperative cancellation, without terminating an embedding application's process. Standalone CLI process lifecycle is owned by the CLI. |
| Existing deadline/cancellation becomes effective while paused | Honor it and wake the wait. Any debug-specific deadline policy must be selected by the host, not silently imposed by the adapter. |

Only the synchronous pause model is confirmed; these lifecycle defaults and their
exact SDK configuration remain proposals. Resolve them before implementing waits,
including races between disconnect, cancellation, command delivery and resume.

## Async execution, reload and native execution

No additional root is driven on this runtime while its executing thread is paused.
External async completions may mark readiness but do not resume script execution
or end the debug stop. Existing owned-root and host-task cancellation/cleanup
contracts still apply. Inspection of unrelated parked roots is outside the first
interactive scope.

Retain the current frame's pinned module generation and corresponding source/debug
metadata. Never display current disk contents as the old frame's source without
checking their identity. Breakpoints resolve against each relevant loaded version;
missing locations remain pending/unverified. Continuation and nested calls keep
existing generation-pinned dependencies; later root admission follows ordinary
publication rules. Do not reload or publish through the paused command loop.
Retained source delivery and source path mapping belong to the SDK/tooling boundary.

Keep the current pre-entry interpreter fallback when an observer is attached:
Cranelift has no actual debug callbacks for this purpose. Attaching to already
executing native code, mid-function deoptimization and machine-code stepping are
outside scope. No new runtime dependency on source analysis is permitted.

## Adapter and command scope

After the SDK pause loop works, add DAP translation for initialization, launch or
host-authorized attach, source breakpoints, pause/continue/stepping, stack/scopes/
variables, binding watches and lifecycle events. Announce only implemented
capabilities. DAP source breakpoint requests replace that source's set; the current
append-only `SetBreakpoint` request is insufficient. Report verified locations and
later resolution changes. Do not wait for an execution-ending `FlushEvents` to
deliver the stop that the IDE must resume.

The [toolchain roadmap](implementation-roadmap.md#toolchain-distribution-and-modes) owns
`kg` and distribution naming. Candidate commands are `kg debug [file.kgr]` for a
user-facing entry and `kg dap` for an IDE-facing service; neither spelling nor a
terminal debugger UI is confirmed by this proposal. A stdio DAP endpoint must keep
script/log output separate from protocol output. Embedded attachment is explicitly
enabled by the host; no automatic listener is installed by the SDK.

## Proposed implementation order and acceptance

All stages are unstarted. They define a finite proposal, not active phase gates.

1. **Synchronous control:** settle lifecycle defaults and the borrow-safe driver
   handoff; implement stop/continue and cancellation with an in-memory command
   endpoint. Prove preservation of the Rust callback stack during nested reentry.
2. **Debug session completeness:** finish single-step and breakpoint replacement,
   stop-scoped inspection, bounded value expansion and generation-aware source
   presentation. Retain binding-only watches and current native fallback.
3. **Tool integration:** connect SDK endpoints to a DAP transport and the agreed
   CLI/host entrypoints; verify a real IDE session and disconnect handling.

Reuse [VM debug tests](../crates/kagari-vm/src/tests/execution/debugger.rs),
[protocol tests](../crates/kagari-vm/src/tests/debug_protocol.rs),
[reentry tests](../crates/kagari-vm/src/tests/reentry_debug.rs) and
[async SDK tests](../crates/kagari/tests/async_execution/debugger.rs).
Current snapshot tests alone cannot prove interactive stopping. Add distinct
coverage using command handshakes and bounded waits, not timing-dependent sleeps:

- After a stopped event, later script effects and the outer host continuation
  have not run; continuing performs each effect once, including loop revisits.
- Nested Rust reentry retains its scopes; inspection respects host borrow
  conflicts. Resume and cancellation preserve normal cleanup and trap ordering.
- Step requests stay with their execution and selected frame. Async completion
  does not resume a paused root, and cancellation can wake an idle debug wait.
- Stop-scoped roots keep inspected values valid; stale requests fail and roots
  are released on every exit. Preserve existing collection/reentry GC coverage.
- Source-free artifacts with debug metadata and old pinned generations remain
  inspectable. Stripped metadata reports unavailable capabilities. Attached
  observers still force actual interpreter execution before native entry.
- Disconnect and queued-command races do not strand the host thread. DAP reports
  current breakpoints, valid handles and actual supported capabilities.

Follow [repository validation policy](../AGENTS.md#verification-and-tooling): run
selected affected tests and a small regression set locally, with structural,
formatting and diff checks. Full-workspace and exhaustive feature/backend/platform
coverage belong to CI. Benchmark attached and unattached overhead only if making
performance claims; none are made here. This documentation checkpoint requires
only content/link/diff checks, not builds or execution tests.

## Reference implementations and fit

- [EmmyLua's debug loop](https://github.com/EmmyLua/EmmyLuaDebugger/blob/master/emmy_debugger/src/debugger/emmy_debugger.cpp):
  `Hook` reaches `HandleBreak` and `EnterDebugMode`; the executing thread waits on
  a condition variable and services queued evaluation. `ExitDebugMode` wakes it.
  Reuse the synchronous waiting concept, not its unrestricted evaluation behavior
  or its C++ ownership assumptions.
- [Java debugging architecture](https://docs.oracle.com/en/java/javase/25/docs/specs/jpda/jpda.html)
  and [JVMTI suspension](https://docs.oracle.com/en/java/javase/25/docs/specs/jvmti.html#SuspendThread):
  protocol and VM control are separate; suspension preserves execution instead of
  requiring application methods to return. Kagari does not need JVM-wide thread
  control, arbitrary OS thread suspension or Java's native/debug metadata machinery.
- [AngelScript debugging](https://www.angelcode.com/angelscript/sdk/docs/manual/doc_debug.html)
  documents both returning a suspended context and waiting inside a line callback.
  This proposal chooses the latter behavior for the host call chain.
- [DAP overview](https://microsoft.github.io/debug-adapter-protocol/overview):
  reuse protocol lifecycle, breakpoint replacement and suspended-state references.
  DAP transports existing semantics; it does not supply the pause implementation.

After implementation, move accepted lasting behavior into the debugger
specification and architecture, move remaining work to the roadmap/review, and
remove this completed proposal rather than retaining a second contract owner.
