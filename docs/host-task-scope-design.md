# Host Task Scopes and Actor Dispatch Design

Status: conceptual proposal, not an implemented API. This document defines how a
synchronous script handler can spawn an async business flow, return immediately,
and have every later execution segment dispatched by its owning host scope. An
Actor mailbox is the motivating integration, not a language or VM dependency.

The [async execution proposal](async-execution-design.md) owns language-level await,
native completion, suspension safety and execution semantics. This companion owns
scope admission, spawn, scheduling notifications and the host drive protocol.
The selected model separates independently retained cold `Future<T>` values from
scope-owned scheduled executions observed through `Task<T>` handles.

Both documents remain design-only work built on current
[native registration](spec/standard-declarations.md) and
[execution control](spec/execution.md). The [roadmap](implementation-roadmap.md)
owns activation; the async proposal owns implementation sequencing.
The selected script names are `spawn`, `Future<T>` and `Task<T>`, with explicit
postfix `.await`. Explicit `async |args| body` closures produce Futures through
ordinary callable types. Admission/result wrappers and concrete host APIs remain
open; examples are illustrative, not implemented interfaces.

The later [runtime ownership and host object design](runtime-ownership-and-host-api-design.md)
owns the Send runtime and centralized retention model. Scope/Actor ownership stays
logical and serialized when its runtime moves between host workers. Completing GO
does not implement the async spawn/drive protocol in this document.

The execution contract supplies the baseline: installed APIs authorize
use, root cancellation and call-depth limits control execution, and the host manages task
admission, deadlines and service limits. Scope/operation identities below are
lifetime and ownership checks, not another boolean permission matrix.

## Intended script experience

Ordinary handlers remain synchronous. They explicitly spawn independent business
work into a host-owned scope. Add `async` to the existing closure expression when
its body needs to suspend; the parent proposal owns its evaluation/capture rules.
The Result wrapper below is the recommended admission API, not a final signature.

```kagari
fn on_message(ctx: PlayerContext, req: QueryRequest) {
    validate(req);
    val player_id = ctx.player_id;
    match ctx.tasks.spawn(async || query_and_apply(player_id, req).await) {
        Ok(task) => ctx.record_task(task),
        Err(error) => ctx.reject_spawn(error),
    }
}

async fn query_and_apply(
    player_id: i64,
    req: QueryRequest,
) -> Result<(), RpcError> {
    val user = rpc.get_user(req.user_id).await?;
    val account = rpc.get_account(user.account_id).await?;
    player.apply_account(player_id, account);
    Ok(())
}
```

Successful spawn retains a Future-producing callable without invoking it in the
handler. First drive invokes the factory once and drives its returned Future.
In this example, the call to `query_and_apply` happens later inside the async body;
it creates a Future, which the explicit `.await` drives in that execution. By contrast,
creating a Future before constructing the closure evaluates that call's arguments
in the handler. Captures retain the existing shared-object and shared-`var` rules;
spawn does not implicitly snapshot a captured object.

The handler returns, then the Actor can drive ready tasks. At an unresolved await,
driving returns to the mailbox loop. Completion makes work runnable but cannot
execute script itself. Spawn's admission result is distinct from the eventual
business result; Task is its observation/result/cancellation handle. There is no
separate script `launch`/`async` method or Job type in this direction. Exact failure
types, result reporting and generic registration remain pre-implementation gates.

## Ownership model

| Concept | Owner and purpose |
| --- | --- |
| Cold Future | Runtime-local checked target and independently rooted captures; not yet bound to an execution |
| Scope capability | Unforgeable, runtime-local script handle authorizing bounded task admission |
| Host scope owner | Host control handle binding lifecycle, policy and scheduling to an Actor or another host service |
| Execution | Engine-owned frames, roots, pinned code, cancellation, termination and waiting operations |
| Task | Script-visible result/observation/cancellation handle for an admitted execution; the scope owns its running lifetime |
| Ready notice | Opaque identity telling the host that an execution may need driving; no script frames or callable closure |
| Completion endpoint | Operation-specific authority to submit an owned result, not authority to enter the VM |

Scopes have Open, Closing and Closed lifecycle states. Closing rejects new spawns
and cancels admitted executions. Only the host can create or close the owner and
select its dispatcher/policy; scripts cannot construct a scope from an Actor name,
replace its dispatcher or use a leaked handle to reopen it.

A scope belongs to one runtime and one serialized host execution domain. The first
release does not transfer task captures or execution state between Actors or heaps.
Exposing a handle to another scope requires an explicit future authority model,
not an assumption that possessing an integer scope ID grants access.

## Future driving and Task ownership

Future creation retains its target program/provider generation and owning runtime.
Captures are ordinary values or independently retained handles, never temporary
Rust references, host leases or unrooted Values. An unstarted retained Future
survives the handler that created it; no admission before handler return is needed
to keep the cold value valid. This does not start IO or a background execution.

First `.await` drives a Future in the current execution and binds it there.
Nested async work shares that execution's cancellation and call-depth controls.
`scope.spawn(...)` instead creates a separate execution owned by the selected
scope. A Future captured by its closure can first be driven there. Shared aliases
do not permit a started Future to be driven from a different execution. Runtime
checks at actual use remain necessary for mutable capture graphs; capturing a
value is not evidence that its later state is safe.

Task handles can be captured and awaited in other executions in the same runtime;
that does not transfer task ownership. Multiple-waiter/result retention contracts
belong to the parent proposal. Decide cancellation of waiting versus target tasks
and nested spawns explicitly. Using the same host scope inside another task does
not by itself specify a parent/child cancellation tree or a local join scope.

Spawn does not re-resolve captured callable/Future targets against the latest code.
If a target is no longer permitted by the scope's deployment policy, admission or
use fails rather than silently switching versions. Reusing a callable for another
spawn creates another execution and invokes the factory once there; it does not
restart Futures captured by that callable. Repeated calls of an async closure
create separate outer Futures sharing the ordinary captured environment.

Successfully admitted tasks survive a later handler trap: spawn is an already
completed effect, not a transaction. Actor shutdown or explicit scope cancellation
can terminate them. Unstarted Futures retained elsewhere remain cold and valid
within their runtime; unreferenced ones release their retained resources through GC.

## Script registration and host setup

Two integration surfaces are required. The script-visible `spawn` is an ordinary
registered Native method; the SDK supplies the generic admission operation it calls.
Registering an arbitrary Rust closure alone cannot create safe resumable execution.

Conceptual binding sketch:

```rust
registry.register(spawn_contract, |cx, scope, callable| {
    cx.spawn_task(scope, callable)
});
```

`spawn_contract` declares concrete checked parameter/output types and effects
through the common provider system. Task handles and admission are generic
engine capabilities, not per-provider or per-method compiler branches. Its input
is `F: Fn() -> Future<T>`; the admitted execution's output is `T`. An ordinary
Future-producing closure and an explicit async closure both satisfy that contract.
Spawn does not accept a bare Future, silently wrap a Unit-returning callable or
recursively flatten Future/Task-valued outputs. A synchronous body can be explicitly
deferred with `async || { ... }`, even when it contains no await. Existing
host generic-registration restrictions still apply; a concrete task-output binding
can prove the first vertical path, but generic script-facing `spawn` needs an
explicit checked registration contract before general acceptance. Adding another
RPC must not require method cases in the compiler or generic driver.

At Actor creation the host binds its scope to a scheduling adapter:

```rust
let scope_owner = runtime.create_task_scope(
    task_policy,
    move |notice| actor_dispatcher.request_turn(notice),
    task_outcome_sink,
)?;
```

The host exposes a restricted scope capability in the script context. It retains
the scope owner and an exclusive driver capability. A ready notice is not sufficient
authority to drive an execution: the runtime also verifies scope ownership and
exclusive activation. Async admission is forbidden during restricted candidate
initialization and other contexts that disallow externally scheduled work.

## Spawn admission protocol

Admission is a bounded synchronous operation; it cannot wait for mailbox capacity
or start the task body reentrantly.

1. Validate the scope, Future-producing callable and capture contracts, permitted
   execution phase and installed binding.
2. Reserve execution/root storage, unfinished-job and initial dispatch capacity,
   and establish cancellation/call-depth controls. Spawn cannot bypass a terminated
   caller or scope admission limits.
3. Bind a generation-checked execution identity and retain its callable captures
   independently of the handler. Captured Futures are not recursively started.
4. Publish its runnable state and initial scheduling notice, then return a Task.

Initial publication must either succeed with durable scheduling responsibility or
roll back admission completely, leaving no hidden runnable task after a spawn Err.
The concrete reservation protocol is an SDK design gate. No task body can run until
the current activation ends, so rollback never needs to undo child script effects.
Argument evaluation and other completed handler effects are not rolled back.

Once admitted, first drive invokes the factory once under the task's cancellation,
call-depth and generation controls. Validate the Future's runtime and binding state
before driving it. A factory trap/allocation failure is a task outcome, not a failed
admission. Subsequent drive turns resume saved state rather than invoking it again.

## Actor drive protocol

The Actor owns serialization. Business messages invoke synchronous handlers; ready
notices request a bounded synchronous drive, not an await of the whole workflow.

```rust
match message {
    Message::Business(request) => {
        engine.call_handler(request)?;
    }
    Message::ScriptReady(notice) => {
        match scope_owner.drive(&mut engine, notice, instruction_slice)? {
            DriveResult::Completed(outcome) => report(outcome),
            DriveResult::Waiting => {},
            DriveResult::Runnable(next_notice) => dispatcher.requeue(next_notice)?,
            DriveResult::Stale => {},
        }
    }
}
```

Error handling is abbreviated. In particular, failed requeue must trigger the
dispatch-failure policy below, not leave an execution permanently parked. The
driver verifies runtime, scope and generation, claims ready work and acquires an
activation guard. It releases that guard before returning. Duplicate notices for a
waiting or retired execution do not restart it; concurrent/reentrant drive attempts
are rejected. The API should distinguish harmless stale notices from engine faults.

`Waiting` means an external event is needed. `Runnable` means the host's slice ended
at a safe point; cancellation and call-depth controls remain in force. A slice cannot preempt
arbitrary trusted Rust code: native handlers and progress steps still owe bounded,
cooperative work. The host must drive its ready queue fairly with ordinary messages.

Do not implement the Actor as a receive loop that awaits one entire script business
flow before receiving again. Such a loop may release an executor thread while still
preventing the Actor from servicing other messages. This design releases both the
driver activation and the Actor turn at the wait boundary.

## Completion and wakeup protocol

An awaited RPC reserves its operation identity and completion endpoint before
external work can answer. On completion, the endpoint accepts owned typed host data
into bounded completion storage and requests a turn from the scope dispatcher.
No completion thread accesses the VM heap or directly resumes an execution.

When the Actor drives the notice, the engine checks cancellation and operation
identity, claims the result once, converts it on the owning thread under runtime
checked allocation and GC ownership, and continues at the saved await position. Program counters,
locals and callback state remain engine-owned; the mailbox carries no raw frame
pointers and no executable script closure.

Completion data and scheduling are separate concerns. A ready notice may be
coalesced, but the corresponding completed result must remain durably available
until claimed or cancelled. The adapter must cover completion-before-wait, completion
during a drive, duplicate wakeups and cancellation racing with publication without
lost wakeups or double execution.

The required dispatcher contract is either accepted scheduling responsibility or
an explicit closed/overloaded failure. On later dispatch failure, terminate the
affected execution or close the scope under a documented host policy. Record this
for owner-thread cleanup and wake the host control path; a failed mailbox send must
not be the sole mechanism for requesting cleanup through that same failed mailbox.
Scope shutdown is the final cleanup backstop. The choice between reserved mailbox
capacity and a bounded ready set with a coalesced wake signal remains open.

Actor restarts create a new scope incarnation. Old jobs, operation completions and
ready notices cannot target the replacement Actor even when its application ID or
mailbox address is reused.

## Threading boundary

The engine sees a serialized execution domain, not an Actor implementation or a
Tokio thread pool. IO may run elsewhere, but only owned host payloads and opaque
notifications cross threads. The runtime, script heap, captured values and parked
frames remain owned by the same logical driver. Current Runtime is Send and not
Sync after GO; the driver may move it between workers outside active synchronous
borrow scopes. Completion threads still cannot directly drive or access it.
Async suspension and transfer of parked executions additionally require the
contracts in the async proposal.

A host may run Actor/VM instances on local executors and use other workers for IO.
Executing on the same OS thread is not sufficient isolation: unrelated local tasks
still cannot bypass the Actor's driver entry. Conversely, the scheduling concept
does not require one dedicated OS thread per Actor; several Actors may share a
worker while retaining distinct ownership and mailbox serialization.

## Lifetime and policy

The scope owner, not the handler stack or a script Task alias, retains admitted jobs.
Dropping a Task does not cancel it. Explicit cancellation stops that job; closing the
scope cancels all jobs. Shutdown cleanup runs on the runtime's owning thread, is
bounded by retained resources, releases roots and code owners, and never executes
script. External cancellation remains best-effort with respect to remote effects.

Failure of one ordinary job is reported to the scope's outcome sink and does not
cancel unrelated message jobs by default. Business Err, script trap, cancellation,
resource termination and engine fault remain distinct. Outcome reporting itself is
bounded and cannot synchronously reenter the VM. Retaining a heap output in host
state requires an explicit root or conversion, not copying a raw Value. Completed
Task results need independently retained storage for later/multiple awaits; do not
keep completed frames and waiting resources alive with the result handle. Exact
failure observation and completed-result release policies remain open.

The installed runtime surface authorizes API use; there is no child permission
intersection. The host supplies each admitted job's cancellation and call-depth
policy. Ordinary nested Future awaits share the same execution controls;
spawn creates a separate execution, not a budget transfer tree. Scope admission
limits unfinished jobs and refuses new work from an already terminated caller.
RPC concurrency, start-rate limits and deadlines belong to the host. If aggregate
load protection is required, the host must enforce it; unfinished-job limits alone
do not bound repeated short spawns. Queue capacities remain implementation safety
bounds, not a general per-task accounting hierarchy.

The [update model](update-model-design.md) adds an explicit state-replacement
lifecycle. It gates business and child-job admission, reaches writer quiescence,
then restores state into a new ownership domain. During draining, the dispatcher
must still service admitted work and completion/cleanup notices without blocking
the Actor thread. Cancelled jobs are not revived on abort; late old-scope completions
cannot enter the replacement runtime. Compatible publication remains non-cancelling.
An admission gate used for draining does not invoke Closing, since Closing cancels
admitted jobs. Abort/reopen and explicit cancel/recreate paths must remain distinct.

Pinned program generations survive spawn and every RPC wait; ordinary publication
affects new target resolution, not an admitted task. Scope policy may bound retained
generations or explicitly cancel old jobs. Await remains a business interleaving
point: messages may change shared state before resumption, so business code must
revalidate conditions or use version-checked host operations.

## Design gates and acceptance

Resolve exact admission/result wrappers, callable/Future capture verification, scope
capability passing, queue reservation, drive outcomes, Task waiter/result lifetimes
and shutdown ownership under the parent async proposal before implementation. This companion introduces
no independent phase ledger or new current implementation requirement.

Acceptance must demonstrate:

- A synchronous handler spawns a task and returns before the task body starts;
  GC after handler return preserves the admitted task's captures.
- Ordinary `Fn() -> Future<T>` factories and explicit async closures are invoked
  once on first drive, never during admission. Body/capture effects preserve their
  specified timing; factory failures are reported and never leave orphan tasks.
- While its RPC waits, another message handler runs; completion cannot run script
  until the Actor processes its ready work.
- A cold Future retained past its creating handler is usable in the new execution;
  driving a started Future from a different execution is rejected. Task sharing
  does not transfer ownership. Cross-runtime scopes, expired capabilities and
  unsafe captured resources are rejected at their specified boundaries.
- Admission failure leaves no orphan job; later handler failure does not silently
  roll back a successful spawn; missing outcome observers do not hide failures.
- Queue saturation, immediate completion, duplicate wakeups, stale IDs, scope close
  and Actor restart neither lose accepted work silently nor resume it twice.
- Task drop preserves scope ownership; cancel and shutdown release execution roots,
  waiting endpoints and execution-owned generation retention without remote-result
  dependencies. Retained results and unstarted Futures keep only necessary roots.
- Interleaved roots retain independent cancellation controls; resume turns do not
  reset cancellation or call-depth limits, nested Future awaits share them, and
  scope spawn obeys host admission limits without introducing execution charging.
- A second RPC and a non-Actor dispatcher use the same registration/drive protocol
  without changing generic compiler, verifier or VM method dispatch.

Use a deterministic fake dispatcher and fake RPC provider, not live services or
sleep-based ordering assertions. Follow the parent async proposal's source/artifact,
GC, failure, reload and feature matrix. These are future acceptance requirements,
not a claim that implementation or tests already exist.
