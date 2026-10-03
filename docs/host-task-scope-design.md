# Host Task Scopes and Actor Dispatch Design

Status: conceptual proposal, not an implemented API. This document defines how a
synchronous script handler can launch an async business flow, return immediately,
and have every later execution segment dispatched by its owning host scope. An
Actor mailbox is the motivating integration, not a language or VM dependency.

The [async execution proposal](async-execution-design.md) owns language-level await,
native completion, suspension safety and execution semantics. This companion owns
scope admission, launch, scheduling notifications and the host drive protocol. It
refines that proposal's initial root-bound task policy: a cold task may be admitted
as a new scope-owned execution before its creating handler ends.

Both documents remain design-only work built on current
[native registration](spec/standard-declarations.md) and
[execution control](spec/execution.md). The [roadmap](implementation-roadmap.md)
owns activation; the async proposal owns implementation sequencing.
All API names and examples below are illustrative and require review before coding.

The execution contract supplies the baseline: installed APIs authorize
use, root cancellation and call-depth limits control execution, and the host manages task
admission, deadlines and service limits. Scope/operation identities below are
lifetime and ownership checks, not another boolean permission matrix.

## Intended script experience

Ordinary handlers remain synchronous. They explicitly launch a business flow only
when it needs to outlive the handler. Named async functions are sufficient for the
first release; async blocks and closures are optional later syntax.

```kagari
fn on_message(ctx: PlayerContext, req: QueryRequest) {
    validate(req);
    match ctx.tasks.launch(query_and_apply(ctx.player_id, req)) {
        Ok(job) => ctx.record_job(job),
        Err(error) => ctx.reject_launch(error),
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

`query_and_apply(...)` creates a cold task and evaluates its arguments once; it
does not start RPC work. Successful `launch` queues the task without executing its
body inside the current handler. The handler returns, then the Actor can drive the
new execution. At an unresolved await, driving returns to the mailbox loop. RPC
completion makes the execution runnable again, but does not execute script itself.

The launch Result reports admission, not the eventual business result. A launched
task's output goes to the scope's configured completion policy. Exact script types
and names such as `Job`, `LaunchError` and `PlayerContext` are not standardized by
this sketch. The first host adapter can accept only tasks producing its declared
business Result type; it need not support arbitrary output reporting immediately.

## Ownership model

| Concept | Owner and purpose |
| --- | --- |
| Cold task | Engine-managed checked target and rooted captures; no execution has started |
| Scope capability | Unforgeable, runtime-local script handle authorizing bounded task admission |
| Host scope owner | Host control handle binding lifecycle, policy and scheduling to an Actor or another host service |
| Execution | Engine-owned frames, roots, pinned code, cancellation, termination and waiting operations |
| Job | Script-visible observation/cancellation handle for an admitted execution; not its lifetime owner |
| Ready notice | Opaque identity telling the host that an execution may need driving; no script frames or callable closure |
| Completion endpoint | Operation-specific authority to submit an owned result, not authority to enter the VM |

Scopes have Open, Closing and Closed lifecycle states. Closing rejects new launches
and cancels admitted executions. Only the host can create or close the owner and
select its dispatcher/policy; scripts cannot construct a scope from an Actor name,
replace its dispatcher or use a leaked handle to reopen it.

A scope belongs to one runtime and one serialized host execution domain. The first
release does not transfer task captures or execution state between Actors or heaps.
Exposing a handle to another scope requires an explicit future authority model,
not an assumption that possessing an integer scope ID grants access.

## Binding a cold task to an execution

Task creation retains the target program/provider generation and owning runtime.
Captures are ordinary values or explicitly
retained handles, never temporary Rust references, host leases or unrooted Values.
Before admission, the creating execution owns these temporary task resources.

There are two permitted ways to start a cold task:

- `await` binds it to the current async root. Nested work shares that root's
  cancellation, version set and remaining work allowance.
- `scope.launch` admits it as a new execution owned by that host scope. Its roots
  and lifecycle no longer depend on the synchronous handler remaining alive.

Shared task aliases observe the same started/admitted state. A second launch or a
launch after await has started is rejected. First-release Job handles provide
status/cancellation, not cross-root joining; aliases in the old handler cannot await
the newly launched execution as though it still belonged to that handler.
Repeat-await of a completed ordinary nested task retains the async proposal's
same-root cached-result policy.

Launch does not re-resolve a target against the latest code. If the captured code
is no longer permitted by the scope's retention/deployment policy, admission fails.
It does not silently switch versions. Captures must be independent of the creating
root's ephemeral resources. Nested task or execution handles hidden in captures
also need an escape policy; until safe transfer is specified, reject captures that
would retain another root's execution-bound resources.

On handler termination, cold tasks not admitted elsewhere are invalidated and their
owned resources released. Successfully admitted jobs remain in their scope even
if the handler later traps; launch is an already-completed effect, not a transaction.
Actor shutdown or explicit scope cancellation can still terminate them.

## Script registration and host setup

Two integration surfaces are required. The script-visible `launch` is an ordinary
registered Native method; the SDK supplies the generic admission operation it calls.
Registering an arbitrary Rust closure alone cannot create safe resumable execution.

Conceptual binding sketch:

```rust
registry.register(launch_contract, |cx, scope, cold_task| {
    cx.start_task(scope, cold_task)
});
```

`launch_contract` declares concrete checked parameter/output types and effects
through the common provider system. Task handles and admission are generic
engine capabilities, not per-provider or per-method compiler branches. Existing
host generic-registration restrictions still apply; a concrete task-output binding
is sufficient for the first adapter. Adding another RPC must not require changes to
launch registration, the compiler or the generic driver.

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

## Launch admission protocol

Admission is a bounded synchronous operation; it cannot wait for mailbox capacity
or start the task body reentrantly.

1. Validate the scope, task state, captured resources, target contract, permitted
   execution phase and installed binding.
2. Reserve execution/root storage, unfinished-job and initial dispatch capacity,
   and establish work controls. Starting a child cannot bypass a cancelled or
   exhausted caller.
3. Bind a generation-checked execution identity and transfer retained captures to
   the scope-owned execution. Mark all aliases of the cold task as admitted.
4. Publish its runnable state and initial scheduling notice, then return a Job.

Initial publication must either succeed with durable scheduling responsibility or
roll back admission completely, leaving no hidden runnable job after a launch Err.
The concrete reservation protocol is an SDK design gate. No task body can run until
the current activation ends, so rollback never needs to undo child script effects.
Argument evaluation and other completed handler effects are not rolled back.

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
at a safe point, not that the root's work allowance was reset. A slice cannot preempt
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
notifications cross threads. The runtime, script heap, captured Values and parked
frames stay with their owner. The current Runtime uses thread-local ownership
structures; this design does not make it Send or permit moving it between workers.

A host may run Actor/VM instances on local executors and use other workers for IO.
Executing on the same OS thread is not sufficient isolation: unrelated local tasks
still cannot bypass the Actor's driver entry. Conversely, the scheduling concept
does not require one dedicated OS thread per Actor; several Actors may share a
worker while retaining distinct ownership and mailbox serialization.

## Lifetime and policy

The scope owner, not the handler stack or a script Job alias, retains admitted jobs.
Dropping a Job does not cancel it. Explicit cancellation stops that job; closing the
scope cancels all jobs. Shutdown cleanup runs on the runtime's owning thread, is
bounded by retained resources, releases roots and code owners, and never executes
script. External cancellation remains best-effort with respect to remote effects.

Failure of one ordinary job is reported to the scope's outcome sink and does not
cancel unrelated message jobs by default. Business Err, script trap, cancellation,
resource termination and engine fault remain distinct. Outcome reporting itself is
bounded and cannot synchronously reenter the VM. Retaining a heap output in host
state requires an explicit root or conversion, not copying a raw Value.

The installed runtime surface authorizes API use; there is no child permission
intersection. The host supplies each admitted job's default work allowance and
cancellation state. Ordinary nested calls and awaits share the same root controls;
launch creates a separate execution, not a budget transfer tree. Scope admission
limits unfinished jobs and refuses new work from an already terminated caller.
RPC concurrency, start-rate limits and deadlines belong to the host. If aggregate
load protection is required, the host must enforce it; unfinished-job limits alone
do not bound repeated short launches. Queue capacities remain implementation safety
bounds, not a general per-task accounting hierarchy.

The [update model](update-model-design.md) adds an explicit state-replacement
lifecycle. It gates business and child-job admission, reaches writer quiescence,
then restores state into a new ownership domain. During draining, the dispatcher
must still service admitted work and completion/cleanup notices without blocking
the Actor thread. Cancelled jobs are not revived on abort; late old-scope completions
cannot enter the replacement runtime. Compatible publication remains non-cancelling.
An admission gate used for draining does not invoke Closing, since Closing cancels
admitted jobs. Abort/reopen and explicit cancel/recreate paths must remain distinct.

Pinned program generations survive launch and every RPC wait; ordinary publication
affects new target resolution, not an admitted task. Scope policy may bound retained
generations or explicitly cancel old jobs. Await remains a business interleaving
point: messages may change shared state before resumption, so business code must
revalidate conditions or use version-checked host operations.

## Design gates and acceptance

Resolve exact admission/result types, cold-task capture validation, scope capability
passing, queue reservation, drive outcomes, default work limits and shutdown ownership
under the parent async proposal before implementation. This companion introduces
no independent phase ledger or new current implementation requirement.

Acceptance must demonstrate:

- A synchronous handler launches a task and returns before the task body starts;
  GC after handler return preserves the admitted task's captures.
- While its RPC waits, another message handler runs; completion cannot run script
  until the Actor processes its ready work.
- First launch binds once; second launch, launch-after-await, cross-runtime scopes,
  expired capabilities and unsafe captured resources are rejected.
- Admission failure leaves no orphan job; later handler failure does not silently
  roll back a successful launch; missing outcome observers do not hide failures.
- Queue saturation, immediate completion, duplicate wakeups, stale IDs, scope close
  and Actor restart neither lose accepted work silently nor resume it twice.
- Job drop preserves scope ownership; cancel and shutdown release all roots,
  waiting endpoints and generation retention without remote-result dependencies.
- Interleaved roots retain independent work/cancellation controls; resume turns do
  not replenish them, nested calls share them, and scope launch obeys host admission
  limits without introducing parent/child budget accounting.
- A second RPC and a non-Actor dispatcher use the same registration/drive protocol
  without changing generic compiler, verifier or VM method dispatch.

Use a deterministic fake dispatcher and fake RPC provider, not live services or
sleep-based ordering assertions. Follow the parent async proposal's source/artifact,
GC, failure, reload and feature matrix. These are future acceptance requirements,
not a claim that implementation or tests already exist.
