# Host Task Scopes and Actor Dispatch

A synchronous script handler can admit independent async business work and return
before it starts. A host-owned Task scope controls that work's lifetime; the shared
Runtime scheduler executes it only when the host polls Runtime or awaits a driving
Call. An Actor mailbox is one integration, not a language or VM dependency.

[Async execution](async-execution-design.md) owns Future/Task semantics, suspension
safety and native completion. The [embedding contract](spec/embedding-api.md#typed-async-calls-scheduling-and-native-waits)
owns exact SDK call, poll and shutdown APIs. This document owns scope admission,
host notification integration and scope lifetime.

## Script admission

```text
fn handle(tasks: TaskScope, id: i32) -> bool {
    match tasks.spawn(async || query_and_apply(id).await) {
        Ok(_) => true,
        Err(_) => false,
    }
}
```

`TaskScope.spawn<T>(fn() -> Future<T>) -> Result<Task<T>, SpawnError>` is an
ordinary registered `core::task` method. Callable coercions accept async closures,
ordinary Future-producing closures, user `Fn` implementations and generic forwarding.
`|| async { query_and_apply(id).await }` is also a factory; a bare async block is a
Future, not a callable, and cannot be passed directly to spawn.

Admission validates runtime/scope identity, callable/capture contracts and the
execution phase, reserves bounded ownership/readiness state, then publishes the
Task without invoking its factory. First scheduled activation invokes that factory
once and drives exactly one returned Future layer. A later factory or execution
failure is a Task outcome, not a failed admission. Reusing a factory creates another
execution; it does not restart Futures already captured by that factory.

`SpawnError::{ScopeClosed, CapacityExceeded, DispatchUnavailable}` describes
admission rejection. Forged/foreign capabilities, unsafe captures and prohibited
candidate execution are traps or host API errors. A terminated caller cannot turn
cancellation into an ordinary spawn Err. Failed admission leaves no hidden runnable
Task; prior argument/handler effects are not rolled back.

## Ownership model

| Value or owner | Contract |
| --- | --- |
| Cold Future | Runtime-local, one-shot computation retaining captures and pinned code; creation starts no body or IO |
| `TaskScopeOwner` | Host control of an Open/Closing/Closed scope; close/drop requests cancellation |
| Typed `TaskScope` | Cloneable, rooted capability from `scope.handle()`; does not keep a closed/dropped owner open or grant driver access |
| Script `Task<T>` | Shared observation/cancellation handle; dropping it does not cancel admitted work |
| Runtime scheduler | Owns readiness, execution selection, terminal retirement and bounded failure events |
| Host Waker | Requests another host turn; carries no authority to access the heap or execute script |
| Native completion endpoint | Publishes owned typed result data for a checked operation identity |

Successfully admitted Tasks survive handler return or a later handler trap. Nested
spawns belong to their explicitly selected scope, not to the spawning invocation.
By contrast, `join`/`try_join` own internal branch executions: parent termination
closes those branches. A branch waiting on a shared Task removes only its subscription
on cancellation, never the Task. See [structured composition](spec/execution.md#structured-two-input-composition).

Task await returns T or propagates execution failure/cancellation, without adding an
outer Result. Business Err remains ordinary data. Repeated/multiple awaits observe
the cached outcome with ordinary shared-object aliasing. Terminal frames, IO state
and admission slots retire independently of retained Task handles.

## SDK setup and host driving

Create the SDK runtime with its default `ExecutionContext`, load a program, then use:

- `runtime.create_task_scope(&loaded)` or
  `create_task_scope_with_context(&loaded, &context)` -> `RunResult<TaskScopeOwner>`.
- `runtime.function::<(TaskScope, ...), R>(&loaded, name)` to bind the handler.
  `TaskScope` here is `kagari_runtime::task::value::TaskScope`; pass `scope.handle()`
  as an ordinary typed argument. The outer tuple represents the argument list.
- `runtime.call(&handler, arguments)` for the synchronous handler.
- `runtime.poll(&mut Context<'_>)` on host notification for continued background work.

The [runnable async_tasks host](../crates/kagari/examples/async_tasks/host.rs)
implements this sequence, including a capacity-one coalescing mailbox and two
independent native providers. Its artifact consumer needs no source frontend.

`Runtime::poll` performs one bounded shared turn across managed Tasks, submitted
invocations and structured branches. It returns Pending while open, including when
idle; Ready(Ok(())) means closed, not temporarily idle. Ready(Err(_)) reports terminal
runtime failure after cleanup. Individual invocation errors reach their Call/ticket;
ordinary Task failures become events rather than failing an unrelated invocation.
Native callbacks remain cooperative and can overrun a quantum.

An Actor can interleave business messages with notifications that request one
Runtime poll. Register its Waker before waiting for messages and preserve durable
notification delivery. A full capacity-one mailbox may coalesce a duplicate wake
because a turn is already requested. Waker cannot report channel disconnection:
the application must detect loss of its driver and explicitly close Runtime rather
than silently discard the only notification. Runtime stores readiness/result data;
the mailbox is not result storage. Completion threads never resume script.

For a request whose whole result may be awaited, `call_async(...)?` returns a
borrowed driving `Call`: awaiting it needs no second pump and also advances shared
work. Its exclusive borrow prevents other direct Runtime access until released.
An Actor that must keep receiving business messages should instead use
`submit_async(...)?` and keep polling Runtime. The returned `PendingCall` only
receives a result; polling that ticket cannot drive its producer. A direct Call
restores any external Waker registration when it completes or drops, notifying
remaining work. There is no implicit background thread after the Call finishes.

## Completion, reporting and shutdown

Native producers reserve a result slot before submission. Immediate/deferred
completion stores owned host data and notifies readiness; the exclusive driver
claims it once and converts it under retained type/session authority. Duplicate
or late completion cannot restart an operation. Operation identities include
runtime ownership and slot generations; reincarnated scopes cannot accept old work.

`runtime.take_task_event() -> Option<TaskEvent>` drains detached managed-Task
failures from `kagari_runtime::scheduler::tasks`:

- `TaskEvent::Failed` carries task/scope identities, spawn origin and `TaskFailure`,
  including cancellation cause and original dependency-failure provenance.
- `TaskEvent::Suppressed { count }` reports overflow after buffered failures drain.

Failures observed by script waiters are still reported. This is not an unhandled-error
detector. Business Err is a successful output, not a failure event. There is no success
result stream here: retain a script Task or use typed invocation results when the host
needs output. Ignoring events never blocks terminal retirement or admission-slot reuse.

`scope.close()` requests cancellation and rejects new admission; `scope.is_closed()`
acknowledges completed local cleanup. Continue polling Runtime until cleanup, or use
`runtime.close()` to synchronously stop all admission and retire local work. Dropping
the scope owner requests the same lifecycle transition; retained capabilities cannot
reopen it. Runtime close is idempotent and does not execute script bodies or wait for
remote cancellation acknowledgement. Cleanup faults quarantine while draining other
owners; already completed result values remain available.

Dropping an unfinished borrowed Call synchronously retires its invocation and
structured descendants even without a later poll. Dropping or cancelling a
PendingCall only requests cancellation; Runtime polling, close or destruction must
perform cleanup. Neither action cancels independent scope Tasks. Remote effects
already completed are not rolled back.

## Threading, policy and reload

Runtime is Send and not Sync. It may move between workers outside active synchronous
borrow scopes, but one heap always has one exclusive execution domain. IO services
may run elsewhere and publish owned results. Task scopes do not transfer captured
values between runtimes or provide script threads.

Installed APIs, checked host borrows, call-depth limits and cooperative cancellation
remain mandatory. Async bookkeeping limits bound admission/storage; they are not
instruction charging or heap quotas. Hosts own aggregate IO concurrency, admission
rates and deadlines. Ordinary nested awaits and structured branches retain parent
execution controls; independent Tasks use their scope's context.

Cold Futures, admitted factories and parked frames retain their selected bodies and
matching type environments. Compatible reload does not cancel or retarget those
computations. Ordinary named typed handles and calls from resumed bodies instead
select current implementations without rebinding; see the
[publication contract](spec/module-activation.md#live-installations-and-call-selection).
Shared mutable state may change while execution is parked, so business logic must
revalidate assumptions on resumption. Suspended executions are not migrated or
serialized by publication.

## Backend integration boundary

Core runtime manual scopes and `Vm::{start, start_future, drive, drive_task}` remain
available for backend tooling and low-level contract tests, not as normal SDK aliases.
Manual scopes use checked `ReadyNotice`/`TaskDispatcher`, `ready_tasks`,
`take_task_report` and explicit retirement draining. Their dispatcher can report
DispatchUnavailable and their report consumption controls record retirement.
Managed SDK scopes instead join Runtime's scheduler and retire automatically; they
do not accept a replacement per-Task dispatcher. Do not mix the two ownership models.

A backend `OwnedExecution` token's drop requests retirement, rather than promising
the SDK Call's synchronous cleanup. Both paths reuse the same guarded runtime
retirement and interpreter; neither introduces a second execution implementation.
