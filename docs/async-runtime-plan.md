# Async blocks, structured concurrency and Runtime API execution plan

## Status and authority

**Active: AR01 design closure complete; AR02 implementation next.** Originally
planned on 2026-10-22 against `2bfcc77c`; the user subsequently authorized AR01-AR06
implementation with one commit per completed phase. This document owns scope,
dependencies, acceptance and progress. The [roadmap](implementation-roadmap.md#async-blocks-structured-concurrency-and-runtime-api)
owns activation. Unimplemented examples describe target APIs, not current callable
interfaces. The AR01 decisions below resolve earlier proposal alternatives.

Follow [project goals](project_goal.md), [architecture](architecture.md),
[execution semantics](spec/execution.md), [failure semantics](spec/failure-semantics.md),
[runtime ownership](runtime-ownership-and-host-api-design.md) and
[repository rules](../AGENTS.md). Existing async execution is implemented foundation,
not a migration to replay. Current specifications remain authoritative until the
corresponding implementation and contract updates land.

This plan owns the invocation and object-interaction facade portion of the queued
[Host API plan](host-api-refactor.md). The user requested extending API planning
beyond call/call_async to reusable function/member handles and retained object
interaction. AR01 freezes the bounded facade contract below; implementation is
now authorized. This does not activate that plan's entire
preparation/loading facade or logical latest-version Function proposal. Keep their
remaining decisions there; do not create a second progress queue for this work.
Coordinate changes to parking and version retention with
[RL01-RL03](runtime-lifecycle-plan.md), without making unrelated storage optimization
or performance targets prerequisites for async functionality.

## Outcome and finite scope

1. Add script `async { ... }` expressions producing cold `Future<T>` values.
2. Add structured concurrent composition, initially two-input `join` and `try_join`.
3. Make `Runtime` the ordinary host execution entry: direct synchronous calls,
   Rust-awaitable asynchronous calls and internally managed script task progress.
   Design one facade for reusable function/field/method handles, argument conversion,
   struct/trait calls and retained object access, reusing implemented boundaries.
4. Reuse the existing interpreter, owned executions, GC roots, typed registration,
   completion transport, cancellation and generation pinning. Do not add a public
   Driver object or require hosts to schedule individual script Task IDs.

Already implemented: named async functions, explicit `async |args| body` closures,
one-shot cold Futures, scope-owned Tasks, typed native completion and parked
interpreter executions. An async block is a new expression, not a second async
closure implementation.

Excluded: automatic Rust async-closure registration, mandatory Tokio or Actor
integration, arbitrary user-defined awaitables/poll/Waker, async traits, streams,
async generators, public dynamic TaskGroup/supervisor APIs, race/select/timeout
combinators, unbounded batch combinators, script threading, native stack switching,
JIT/AOT suspension, continuation serialization and suspended-code migration.
Keep `TaskScope.spawn` factory input and independent scope ownership unchanged.
Do not add a direct-Future spawn overload merely for spelling symmetry.

## Baseline evidence and architectural diagnosis

This is source/test inspection, not a new behavioral run or performance result.

| Owner | Current mechanism | Work needed |
| --- | --- | --- |
| [Compiler Future lowering](../crates/kagari-compiler/src/source/lower/future.rs) | Async factory emits MakeFuture; private resume body carries may_suspend; await emits Await. | Reuse for async blocks and preserve portable verification. |
| [Future storage](../crates/kagari-runtime/src/native/future.rs), [claiming](../crates/kagari-runtime/src/gc/future.rs) | Script/native cold payload is claimed once; aliases cannot restart it. | Retain this contract for blocks and composition inputs. |
| [Frame waiting](../crates/kagari-runtime/src/frame/waiting.rs) | One execution has one pending native/Task wait; nested script awaits push resume frames. | Own multiple branch continuations without duplicating interpreter semantics. |
| [Task admission](../crates/kagari-runtime/src/task/admission.rs), [driving](../crates/kagari-runtime/src/task/drive.rs) | Scope retains factory, readiness and owned execution; first drive invokes the factory. | Move routine task scheduling behind the public Runtime facade. |
| [Stdlib tasks](../crates/kagari-stdlib/src/foundation/tasks.rs) | Registered generic methods call checked runtime services and encode Result values. | Keep join policy and Result interpretation at this library boundary. |
| [SDK owned API](../crates/kagari/src/runtime/owned.rs) | start/drive produces a cold Future; start_future/drive obtains its output. | Encapsulate both stages in one Rust-awaitable invocation. |
| [SDK facade](../crates/kagari/src/runtime.rs) | KagariRuntime wraps Vm; execute rejects nonempty argument slices. | Reuse existing typed calls instead of extending raw-Value plumbing as the default API. |
| [Task publication](../crates/kagari-runtime/src/task/outcome.rs) | Taking a report retires its admission record; script payload independently retains its result. | Internal result delivery/retirement must not depend on manual per-Task polling. |

The identified defect is host-facing orchestration responsibility: applications
currently reconstruct ready-task scheduling, factory/Future staging, report
retirement and cancellation drains. It is not evidence that VM-frame suspension is
unsuitable. Keep that foundation; introduce one runtime-owned scheduling path that
all public invocation and script-spawn paths use.

Example: a synchronous handler spawns a request, returns, and the request waits on
RPC. Today the host finds a TaskId, drives it, handles readiness and takes its
report. After this work the host calls the handler and gives the Runtime execution
opportunities through its selected wake/poll integration. Runtime schedules the
Task and delivers its outcome. The handler still returns before the async body
runs; completion threads still cannot enter script.

## Target contracts

### Async blocks

```text
val work = async || { request(id).await }; // fn() -> Future<T>
val first = work();                       // fresh Future<T>
val second = work();                      // another Future<T>
val pending = async { request(id).await };// directly Future<T>
```

- Creating a block captures its lexical environment but runs none of its body.
  Capture/allocation may fail; expressions outside the block retain ordinary
  left-to-right once-only evaluation and effects.
- Capture classification follows existing closures: scalar values remain values,
  shared objects remain shared, writable captured bindings retain their shared
  environment slots. No implicit deep copy, move syntax or Rust lifetime model.
- The completed output is the block's tail-expression type; contextual Future<T>
  supplies T. Empty/Unit bodies follow ordinary block rules.
- `return` and `?` target this async body. `break`/`continue` cannot escape into an
  enclosing loop outside it; nested ordinary closures remain non-suspendable.
- Synchronous code may construct a block. Await still requires an async body or
  the host's explicit asynchronous invocation path.
- Aliases share the existing one-shot Future state. Unstarted blocks perform no
  body effects when discarded; repeated await traps.
- `async { f() }` returns Future<Future<T>> when f returns Future<T>; there is no
  automatic flattening. `async { f().await }` produces Future<T>.
- Retain runtime identity, capture-graph checks, non-suspendable host resources and
  candidate/static-initialization restrictions. Syntax sugar is not an exemption.

Conceptually the block resembles `(async || { body })()`. Lower directly through
the shared capture/resume machinery where appropriate, without requiring an extra
observable closure invocation or promising an unmeasured allocation reduction.

### Structured composition

The selected input model is cold Futures, not factories. The target signatures
below describe normal output; AR01 freezes precise declaration and error surfaces:

```text
join<A, B>(left: Future<A>, right: Future<B>) -> Future<(A, B)>
try_join<A, B, E>(left: Future<Result<A, E>>, right: Future<Result<B, E>>)
    -> Future<Result<(A, B), E>>

val panel = task::try_join(
    async { load_inventory(id).await },
    async { load_guild(id).await },
).await?;
```

Construction returns a cold composition Future and retains inputs; it does not
start branches. First drive claims the inputs through the existing checked
one-shot contract and establishes ownership of their executions. Merely passing a
Future does not freeze its state: an alias may consume it before the composition
starts. Validate actual state at use, including duplicate operands. Invalid or
already-consumed inputs cannot cause double submission or strand a started branch.

The group owns branch lifetimes, not an application scope's independent jobs.
No public Task handle or independent host report is required for each branch.
Waiting on an existing shared Task inside a branch does not reparent that Task;
cancelling the branch removes its wait, not the target execution.

| Event | join | try_join |
| --- | --- | --- |
| Branch returns normal output | Retain in input-index order; finish when all return. | Retain Ok payload; finish with ordered tuple when all succeed. |
| Branch returns business Err | Ordinary output value; do not interpret it. | Select that business failure, stop further normal branch progress and close remaining branches. |
| Branch traps or is cancelled | Close the group and propagate original execution termination. | Same; do not encode termination as E. |
| Parent is cancelled/dropped | Terminate owned branches through runtime cleanup. | Same. |
| Group cannot reserve resources | No branch body starts; unwind reservations. | Same. |

When try_join changes the success type of a selected Err to the tuple output,
it preserves that Err's original diagnostic metadata, source and stack. Forward
failure provenance rather than extracting E and constructing a fresh Err at the
combinator; reuse the [Result propagation contract](spec/builtins.md).

A selected result is not published until all owned branches are terminal and local
cleanup has finished. Cleanup does not execute arbitrary script, depend on a remote
reply or require cancellation acknowledgement from an external service. Late
completion is invalidated; already completed external effects are not rolled back.

Observe all branch completions: a right-hand failure must not remain hidden behind
an indefinitely waiting left-hand branch. Choose the first failure committed by
the exclusive driver under a documented stable ready-processing order, not an
unobservable wall-clock "first failure". Parent cancellation and engine faults keep
their existing precedence and quarantine rules; cleanup failures cannot be hidden
by an already selected business Err.

Prepare and reserve the fixed-size group before starting any branch body. This
is not a transaction over argument evaluation or future external effects. Keep
invalid-state traps distinct from admission capacity and execution termination.
Default direction: internal resource exhaustion follows existing execution failure
classification; do not copy scope-specific SpawnError variants into join merely
because existing spawn machinery is reused. A recoverable capacity/degrade-to-serial
API would require a separate explicitly approved contract.

### Library policy versus execution machinery

Standard library owns names, generic signatures, tuple assembly and Result
interpretation. Runtime owns branch records, roots, validity, readiness, retirement
and cancellation. VM executes checked bodies using the existing executor. Compiler
and bytecode never infer special behavior from the spelling `join` or `try_join`.

A bounded runtime-managed composition service may report normal branch completion
to trusted library policy. Conceptually policy returns Continue or Finish(value);
Finish requests closing, not immediate publication. Policy uses checked type/enum
identities and a short-lived context; retained values are traced/rooted. It cannot
keep CallContext/heap borrows across suspension, recursively drive the VM, swallow
execution termination or run from a worker-thread completion callback.

The current external Completion<T> adapter is not sufficient to drive multiple
script branches. Extend the runtime-owned resumable-operation boundary rather than
sending script Values through a host executor or implementing a nested interpreter.
Do not invent an unrestricted user scheduler/plugin protocol for two combinators.

### Public Runtime API and wake/poll ownership

Runtime is the primary stateful host object. Engine remains preparation/configuration;
Vm remains an execution implementation detail. Target call-site shape:

```text
calculate = runtime.function(module, "calculate")?;
value = runtime.call(&calculate, args)?;
// load_profile is a prepared async callable.
value = runtime.call_async(&load_profile, args)?.await?;
// Optional separate submission; the host must continue driving Runtime.
pending = runtime.submit_async(&load_profile, args)?;
```

Exact Rust signatures, argument/return adapters and public exports are AR01 design
gates. Reuse typed conversion and retention, and the already confirmed outer-tuple
argument convention from the Host API plan. Do not require raw Value extraction,
manual factory staging or runtime.runtime() for ordinary calls, cancellation or
scope task progress. A facade name change replaces the obsolete name directly;
review intentional public exports under the structure whitelist. This does not
authorize unrelated Engine/Program/Script/Function redesigns.

- Synchronous calls retain synchronous semantics and never implicitly block_on.
  Calling a Future-returning function synchronously returns its cold value.
- Async invocation drives exactly one explicitly requested Future layer. Define
  accepted callable signatures and reject unsupported targets before body effects.
- call_async validates/registers an invocation without executing the async body
  inline. Directly awaiting the returned Future must drive the call to its result
  through the common Runtime execution machinery, without a separately required
  business-authored pump or background task. This is cooperative async waiting,
  not a hidden block_on or an OS-thread-blocking wait.
- call_async returns Call<'a, R>, which borrows &mut Runtime until completion/drop
  and drives through it. Other direct host access to that Runtime is unavailable
  during the borrow; unrelated host tasks can still run. submit_async briefly
  borrows Runtime and returns PendingCall<R>, a non-borrowing result ticket that
  never drives execution. It permits further host calls but requires Runtime progress
  from external polling or another driving invocation. These are distinct execution
  contracts over one implementation, not compatibility aliases or implicit modes.
- Call polling registers the awaiting host task's Waker and advances bounded
  work through the same Runtime driver used by explicit polling. Pending IO wakes
  the task so it can poll again. Progress includes structured branches and shared
  Task dependencies needed by the call; inline await must not deadlock those paths.
  Keep result conversion/retention under checked runtime access, not a raw-Value channel.
- Runtime execution is single-threaded. The external scheduler arranges another
  runtime turn on its execution thread when the runtime-level Waker is notified.
  Ordinary calls, async resumption and cleanup use serialized execution entries.
  Completion threads publish notifications only; they never enter the VM or access
  the script heap. No new parallel-execution model or global mutex is required.
- Runtime schedules ready calls, script Tasks and structured branches. Use a
  Context/Poll-style runtime driving entry; exact signatures remain an AR01 detail.
  Each turn has a bounded shared instruction slice. Remaining runnable work requests
  another turn; external waiting does not spin. Do not give each child a full outer
  slice or promise hard native-code preemption. Wakes received during a turn must
  remain observable in that turn or a subsequent scheduled turn.
- Runtime readiness and individual call-result readiness have distinct roles.
  Runtime readiness wakes the current driver (Call or the external adapter);
  PendingCall registers only a result waiter. Both use lost-wakeup-safe state.
  Driver handoff must preserve durable readiness and restore/notify the external
  registration without retaining a completed Call's Waker as the only progress path.
  One ticket's drop/completion must not remove another result waiter. No active
  frame/table borrow, native borrow or scoped host resource crosses suspension;
  Call's Rust facade borrow is not an active heap/native-resource lease.
- Independent scope Tasks retain their existing lifetime after a handler or root
  call returns. When the host wants background progress with no call being awaited,
  its external scheduler drives Runtime on notifications independently of the call.
  Root-call completion neither cancels these Tasks nor removes their progress path. No additional per-root run Future, public
  Driver object or host per-Task scheduling protocol is required. Progress still
  requires the host to service the registered runtime notifications.
- Dropping unfinished Call or PendingCall cancels the invocation, not detaches it.
  Call has exclusive Runtime access: its drop path must perform local cancellation
  and release owned execution state without requiring an uninstalled background pump.
  PendingCall publishes a cancellation request and wakes the driver; Runtime retains
  cleanup ownership after the ticket disappears. AR01 audits exact cleanup/error
  handling for both paths. Cancel structured child branches, but only remove waits
  on shared Tasks and do not cancel independent scope Tasks. Neither path waits for
  remote cancellation acknowledgement. Use scope.spawn explicitly for independent work.
- Routine Task reports are delivered/retired internally under a bounded outcome
  policy, not left occupying admission slots until a user manually takes each one.
  Preserve the script Task's awaitable result and a runtime-level host reporting
  path for unhandled execution failures. Define receiver-drop, overload and
  dispatcher-failure behavior without losing the only cleanup path.
- Explicit shutdown stops admission, cancels remaining executions and acknowledges
  internal cleanup. Dropping Runtime performs local fallback release and invalidates
  late notifications without relying on a future poll. Neither path waits indefinitely
  for external cancellation acknowledgement or rolls back completed external effects.
  Whether explicit close needs an async signature depends on its actual cleanup steps.
- Runtime remains Send, not Sync: ownership transfer is not concurrent execution.
  Cross-thread notification carries owned host data or checked identities, never
  unrestricted script Values or host borrows. Preserve existing synchronous host
  reentry rules separately; Waker notification must not introduce recursive driving.
  Core execution remains executor-neutral, without mandatory Tokio or Actor support.

For example, a synchronous login handler registers a scope Task and returns. The
registration requests a runtime turn; the external scheduler drives the Task until
it waits for RPC. RPC completion publishes readiness and wakes that same scheduler,
which drives Runtime again to resume the Task. The handler's return and the presence
of a host invocation Future are irrelevant to this independent Task's progress.

A serial Actor handler may directly await a script call and deliberately postpone
other mailbox messages. The call must still progress via invocation polling rather
than depend exclusively on a RuntimeReady message that the handler is not processing.
If its external IO depends on another message to that same Actor, that application
cycle still needs explicit handling; async syntax does not make an Actor reentrant.
For multiplexed/background operation, a coalesced wake can request a runtime turn;
no per-script-Task mailbox protocol is required. These are host integration choices,
not prerequisites for an ordinary direct-await call.

### Runtime driving API proposal

**Candidate signatures, pending AR01 review.** Keep explicit responsibilities:
call executes synchronously; call_async returns a borrowed, driving Call; submit_async
returns a non-borrowing, non-driving PendingCall; poll supports event-loop/background
driving. Direct call_async await requires no separate runtime.poll loop. Proposed
explicit driving signature:

```text
fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), RuntimeError>>;
```

This is an explicit Runtime method, not a requirement that Runtime implement Future
or be moved into a permanently borrowing run Future. A live Call's exclusive borrow
prevents simultaneous explicit poll or another direct host call. When that borrow
ends, the host can resume external driving. Both entries reuse one scheduler with
checked wake handoff; no second interpreter or competing task queues are introduced.

| Return | Proposed meaning |
| --- | --- |
| Pending | Runtime remains open or is still closing. Further progress is scheduled through its Waker. |
| Ready(Ok(())) | Explicit shutdown has completed internal cleanup. |
| Ready(Err(error)) | Runtime has reached an unrecoverable terminal condition under existing failure/quarantine rules. |

Idle is not terminal: an open Runtime with no tasks returns Pending and can accept
later calls. A bounded turn that still has runnable work requests another turn
before returning Pending. Genuine external waiting does not self-wake continuously.
Ordinary call traps/cancellation go to the call result; independent Task failures go
to the reporting path. Neither alone terminates runtime polling. After Ready the
host stops driving; AR01 must define closed-state behavior for subsequent API use
and complete or invalidate outstanding result receivers under shutdown/fatal rules.

Each turn updates the Waker, processes completion/cancellation notifications,
advances ready work within the shared slice and delivers outcomes/cleanup. Result
conversion that needs heap access runs under the owning Runtime context before
publication, including when the driver is entered from invocation polling. A bare
result receiver cannot independently access heap storage. Conversion failures retain
completed script effects; receiver abandonment releases unpublished result retention through
the same lifecycle. Exact conversion callback bounds remain part of AR01.

#### Host scheduling walkthrough

The ordinary workflow needs only the host executor to poll the invocation:

```text
let result = runtime.call_async(&load_profile, (id,))?.await?;
// Continue this business operation only after the script result is available.
```

The call's poll drives bounded Runtime work, returns Pending at suspension and
arranges a wake on readiness. The host executor polls it again; no manual Runtime
poll loop or spawned pump is required. This waits for the call, not all independent
scope Tasks. Inline waiting does not promise that unrelated host messages continue
being processed, nor does it freeze other script work the shared scheduler needs
to advance. Independent tasks remaining after the call need later Runtime execution
opportunities if background progress is desired.

The following optional multiplexed integration uses submit_async and non-borrowing
PendingCall tickets. It is not a requirement of the inline call_async workflow.
The host creates a Waker whose wake operation posts a coalesced RuntimeReady event
or marks its existing execution unit runnable. It holds notification state only,
not a mutable Runtime reference. This adapter is host integration, not a new public
Driver object or a mandatory messaging framework. Example event-loop pseudocode:

```text
runtime_waker = notify_this_host_loop_on_wake()
drive_once(runtime, runtime_waker) // first registration; handle terminal outcome

while runtime_is_active:
    event = wait_for_host_event()
    match event:
        BusinessMessage(message):
            handle_call_result(runtime.call(on_message, (message,)))
        AsyncRequest(arguments, reply):
            submission = runtime.submit_async(load_profile, arguments)
            handle_admission_error_or_hand_ticket_to_host_waiter(submission, reply)
        RuntimeReady:
            // Acknowledge the coalesced notification before driving, so a wake
            // during this turn can schedule another event.
            acknowledge_runtime_notification()
            cx = Context::from_waker(runtime_waker)
            handle_pending_or_terminal(runtime.poll(cx))
```

In this optional mode, the host waiter may be a local task; handing it a result
does not imply Send or require a thread pool. If continued host-message/background
progress is desired, keep servicing those events while a ticket is awaited. Simply
awaiting a PendingCall in the only driving loop cannot make progress by itself;
use call_async instead when intentionally awaiting inline. A Call drives without
depending on deferred RuntimeReady messages. The same scheduler serves both modes;
AR01 must show that wake routing and exclusive access remain correct across handoff.

A synchronous handler can spawn and return; the registration requests RuntimeReady.
The next turn runs the Task until IO waiting. IO completion records durable readiness
and wakes the host; another turn resumes the Task. On call completion, Runtime wakes
the separate PendingCall result waiter. Root-call return preserves the installed
external wake path; a driving Call releases its temporary driver registration safely.

The initial poll installs notification routing and observes work queued before any
Waker existed. Wakes can coalesce because readiness lives in Runtime, not solely in
messages. Waker replacement, notifications arriving during a turn, and acknowledgement
ordering must not lose progress. A full/closed notification destination must not be
silently treated as successful scheduling; AR01 defines adapter failure/teardown.
Late wakes after shutdown are harmless. Shutdown initiation remains a separate API
review item; poll reports its completion but does not initiate shutdown when idle.

### Public invocation and object API proposal

**Proposal, not implemented facade or approved final signatures.** The following
Rust-shaped examples are deliberately text blocks: names, generic placement and
error signatures must be frozen in AR01 and exercised by compiling consumer
examples in AR05. They assume an already loaded module and a host async executor
for await; external runtime-loop integration is optional. Loading and executor setup
are not redesigned here.

#### Implemented foundation and repeated-call costs

The [SDK typed calls](../crates/kagari/src/runtime/typed.rs) already accept
PinnedFunction<A, R>. The [host objects example](../crates/kagari/examples/host_objects.rs)
uses prepared fields/methods, retained aliases and checked mutation. Public bindings
currently often require runtime.runtime(), and object operations require a scoped
NativeContext through with_context. Unify the facade rather than replace these
semantic implementations.

| Existing capability | Proposed ordinary facade |
| --- | --- |
| PinnedFunction<A, R>, bind_function, SDK call | runtime.function followed by runtime.call |
| ObjectType and bind_associated_function | runtime.object_type and runtime.associated_function; invoke with call |
| bind_method and Object::call | runtime.method and runtime.call_method with an explicit receiver |
| Interface member binding and Interface::call | runtime.interface_method and call_method, preserving a distinct checked interface descriptor |
| bind_field, Object::get/set | runtime.field and runtime.get/set using retained Object handles |
| Owned conversion and retained collection/object mappings | Same argument adapter; document snapshot versus shared-object semantics |

A saved pinned function avoids repeated public-export lookup and semantic-signature
preparation. Name-based execution currently performs that work on each entry;
rebinding on each call is not equivalent to retaining the handle, since the binding
cache is consulted after some evidence preparation and mapping checks. Each actual
call still converts/roots arguments, validates ownership and actual values, enters
execution, and converts/protects its result. Current tuple conversion also repeats
some mapping checks and builds argument storage. Do not promise lookup-free binding,
allocation-free calls or validation-free execution.

No timings were measured in this review. If optimizing these paths, measure binding,
repeated named calls and retained-handle calls separately, using scalar, owned-data
and retained-object arguments. Distinguish immutable signature evidence from per-call
ownership, cancellation, version and root checks before relocating any validation.
A facade cleanup does not authorize a new cache or an unrelated optimization project.

#### Functions, arguments and results

Prefer explicit Runtime invocation as the normal entry, rather than introducing
both runtime.call and a second equivalent function.call facade:

```text
let calculate = runtime.function::<(i32, i32), i32>(&module, "calculate")?;
let first = runtime.call(&calculate, (10, 20))?;
let second = runtime.call(&calculate, (30, 40))?;
```

The handle retains a checked callable and generation, not a lifetime borrow of
Runtime. It can be stored in application state. Binding validates the Rust mapping
against the loaded declaration; Rust compilation alone cannot establish that a
separately loaded script has that signature. Wrong ownership/type/access must fail
before target effects. Existing retained script closures reuse the callable boundary.

Keep the confirmed outer-tuple argument convention and existing 0-12 convenience
arities; this is not a script/runtime argument-count limit:

| Host arguments | Script parameters |
| --- | --- |
| () | No arguments |
| (42,) | One integer |
| (42, true) | Two arguments |
| ((42, true),) | One tuple-valued argument |
| ((),) | One Unit argument |
| (player.clone(),) | One retained object, preserving identity |

Do not infer tuple flattening from a target signature. Returns are one value;
business Result remains data inside the outer execution-error channel. Owned Rust
DTO/Vec conversion constructs data rather than sharing arbitrary Rust memory;
Object and ScriptVec-style handles preserve script identity and aliases. A handle
clone retains the same object, not a deep copy.

#### Driving calls versus submitted result tickets

Both async entrypoints use a prepared callable. Candidate AsyncFunction<A, R>
represents parameters A and declared script output Future<R>, with awaited host
output R; it is not a PinnedFunction<A, R> pretending the factory returns R directly.
AR01 freezes binding names, trait bounds and error types. Proposed Rust-shaped
signatures (bounds omitted, not compilable declarations):

```text
fn call_async<'a, A, R>(
    &'a mut self,
    function: &AsyncFunction<A, R>,
    args: A,
) -> Result<Call<'a, R>, CallError>;

fn submit_async<A, R>(
    &mut self,
    function: &AsyncFunction<A, R>,
    args: A,
) -> Result<PendingCall<R>, CallError>;

Call<'a, R>: Future<Output = Result<R, CallError>>
PendingCall<R>: Future<Output = Result<R, CallError>>
```

| Contract | Call from call_async | PendingCall from submit_async |
| --- | --- | --- |
| Runtime ownership | Exclusive borrow until completion/drop | No Runtime lifetime borrow; result/cancellation state only |
| Future polling | Advance bounded work through the common Runtime driver and check this call's outcome | Check result/register result Waker; never execute script |
| Direct await without another pump | Supported ordinary workflow | Insufficient unless some other entry drives Runtime |
| Another direct host call while pending | Rejected by Rust borrowing | Permitted under ordinary Runtime access rules |
| Cancellation on unfinished drop | Use exclusive Runtime access for local cleanup; no external pump prerequisite | Publish cancellation and wake the runtime driver |
| Outliving Runtime | Prevented by lifetime | Resolve/invalidate safely on teardown; ticket must not own the heap |

Ordinary sequential business uses the first form:

```text
let result = runtime.call_async(&load_profile, (id,))?.await?;
runtime.call(&apply_profile, (result,))?;
```

For optional interleaving, use the second form:

```text
let pending = runtime.submit_async(&load_profile, (player.clone(),))?;
runtime.call(&update_position, (player.clone(), position))?;
// Keep driving Runtime; a separate host waiter may await pending.
```

Do not show call_async followed by another direct Runtime call before its Call is
completed/dropped; that contradicts the exclusive borrow. Unrelated host tasks can
run during Call.await, but direct access to this Runtime cannot. PendingCall.await
is still asynchronous waiting, not an alternate self-driving operation.

Both entrypoints share checked registration, argument conversion/roots, version
selection, execution records and result delivery. The first error channel is
registration/admission; awaiting yields execution/output-conversion errors. Retain
the selected target and converted inputs before returning; caller-side handles need
not remain borrowed. Neither entry runs the async body inline. A submitted invocation
can start on a later runtime turn even before its ticket is polled. A driving Call
wraps the same registered invocation with exclusive access to the common driver.

Call polling checks its outcome and advances bounded work using lost-wakeup-safe
registration; it returns Ready when that call is done, not when Runtime shuts down.
Pending IO wakes the active driver. Do not implement Call by awaiting the terminal
Ready of Runtime.poll. No separate interpreter, queue or per-call full child budget
is introduced. Previously submitted calls and independent Tasks can also progress
during a driving Call, without a guarantee they finish before that Call returns.

The external adapter resumes driving after Call releases its borrow. Durable
readiness and wake handoff must cover both directions; do not leave the completed
Call's Waker as the only notification target. Dropping an uncompleted Call must
retire local owned state even without another poll; audit cleanup failures and
quarantine rules without executing arbitrary script or waiting for remote IO in
Drop. Dropping PendingCall uses the advertised external-drive cancellation path;
Runtime teardown remains a local fallback for all remaining state.

Async-capable methods must use these same drive/submit semantics where already
supported by the language; freeze concrete method entry names in AR01. Do not add
async traits or a receiver-specific scheduler. Default execution configuration
comes from Runtime with explicit overrides where needed; preserve cancellation,
observers, reentry and backend-policy checks rather than dropping the current context.

#### Struct and trait calls

Associated functions have no receiver and use the ordinary function-call path:

```text
let player_type = runtime.object_type(&module, "Player")?;
let create = runtime.associated_function::<(i32,), Object>(&player_type, "new")?;
let player = runtime.call(&create, (100,))?;
let damage = runtime.method::<(i32,), i32>(&player_type, "damage")?;
let remaining = runtime.call_method(&player, &damage, (10,))?;
```

Method arguments exclude self. A prepared method may be reused across compatible
receivers; matching spelling or structural shape alone does not establish nominal,
applied-type or generation compatibility. Preserve the existing closed executable
evidence requirements for generic applications; a convenient lookup API cannot
invent specializations. Receiver-bound convenience handles are not required initially.

Trait invocation uses an explicit retained interface view and checked dispatch, not
a name search across all implementations on a concrete object:

```text
let behavior: Interface = runtime.call(&make_behavior, ())?;
let tick = runtime.interface_method::<(f32,), ()>(&behavior, "tick")?;
runtime.call_method(&behavior, &tick, (0.016_f32,))?;
```

make_behavior declares the relevant trait return. Inherent and interface descriptors
may share a public call_method spelling through a checked abstraction, but must not
erase their distinct validation/dispatch semantics. Existing inherited/default
methods are covered by the interface path. Converting an ordinary Object to a
specified trait view is a separate AR01 decision: either expose a checked conversion
using installed implementation evidence or explicitly require an existing interface
value in this release. Do not add runtime trait inference or imply arbitrary casts
already exist. Same-named methods in different traits remain distinguishable.

#### Retained objects, mutation and round trips

```text
let hp = runtime.field::<i32>(player.object_type(), "hp")?;
let previous = runtime.get(&player, &hp)?;
runtime.set(&player, &hp, previous - 10)?;
runtime.call(&save_player, (player.clone(),))?;
```

The host can keep player across calls and GC; field access uses prepared handles
rather than name lookup on each access. The write updates the original script
object, visible to aliases, and passing it back preserves identity. Object-valued
reads return retained handles. Reuse existing checked constructors and collection
handles; arbitrary Rust references into script storage are not part of this facade.

Preserve public-member access, writeability/readonly views, nominal type, runtime
identity, generation compatibility, active-borrow checks and controlled reference
writes. No conversion to unrestricted &mut RustPlayer or raw GC slot access is
implied. Failed preparation does not publish a field write; later failure does not
roll back earlier committed writes. Pending async calls do not freeze shared objects:
subsequent host mutations may be observed when those calls run/resume. Runtime
teardown invalidates access even if host handles remain; handles do not own Runtime.

#### Reload policy and complete usage review

Use existing pinned-version handles as the proposed initial facade foundation:
old handles keep their checked generation, and applications rebind current entries
after successful reload. A name resolved against an old LoadedModule is not a
latest-version lookup. Do not silently change the meaning of PinnedFunction.

Logical latest-version Function handles remain a separate proposal in the Host API
plan. Adopting them requires explicit compatibility, invalidation and selection-time
rules, not just another cache. Old objects cannot be passed to new functions merely
because their type names agree; retain existing nominal/layout/version checks and
make failure actionable. Queued and suspended calls keep their selected generations.

Before freezing this facade, review one coherent host workflow: load/bind, repeated
calls, retained object creation, field mutation and round trip, struct and trait
calls, async registration with another call while pending, and reload with old
handles/objects versus newly bound entries. AR01 owns the API review; AR05 supplies
a compiling public-consumer example and focused semantic coverage. Do not force
ordinary consumers through runtime.runtime(), raw Values or manual root management,
and do not add generated object views or rewrite preparation/loading for this scope.

## AR01 resolved design and implementation boundaries

Single-threaded Runtime execution, self-sufficient direct await, optional external
background driving and cancel-on-drop are settled requirements. The selected API
model separates borrowed driving Call from non-borrowing result-only PendingCall;
exact names, bounds and implementation details remain under AR01 review. Close the
remaining contracts here with API sketches, ownership diagrams and focused Rust
ownership prototypes where needed; AR05 compiles real public-consumer examples.
These details are not permission to defer correctness to later phases.

1. Exact invocation signatures: admission errors versus execution results, typed
   result retention/conversion, actual Send bounds and runtime identity/lifetime
   checks. Validate the Call/PendingCall split: borrowed direct await progresses
   without a separate pump, while submitted tickets permit interleaved calls under
   external driving. Audit local Call-drop cleanup without subsequent polling,
   ticket-drop cancellation with external driving, cancellation before first drive,
   and ticket completion/invalidation when Runtime is destroyed.
2. Exact Context/Poll driving and Waker registration/update interface, including
   wake coalescing, idle versus shutdown behavior and the external scheduling
   adapter. Review the proposed Pending/terminal distinction, first registration,
   notification acknowledgement and result conversion/publication ownership above.
   Specify bounded unhandled-failure reporting, receiver/dispatcher failure and
   shutdown acknowledgement; decide whether explicit close requires async.
   External driving supplies optional background progress after root-call completion;
   it must not be a prerequisite for the direct-await call itself.
3. Structured branch representation: reuse internal owned executions, or add branch
   continuations within an execution. Compare actual frame/pending-state reuse and
   cancellation ownership. No second interpreter, public Task/report inflation or
   unsupported claim that existing sequential await already supports fan-out.
4. Minimal composition-policy API, source-free callable/type validation, atomic
   reservation/claim order and error precedence. Provide checked Err forwarding
   across success types without losing or recapturing diagnostic metadata.
   Preserve bounded cycle detection
   across new parent/group dependencies and existing shared Task waits.
5. Review and freeze the invocation/object facade proposal: callable and async-output
   handle types, struct/trait member binding, field access, default execution options,
   explicit trait-view conversion scope and the complete host workflow. Reuse pinned
   generations and existing retention/conversion; bound obsolete facade API removal.
   Finalize join signatures too. Logical latest-version lookup, source preparation
   and native backend expansion remain with their existing owners.

### AR01 closure decisions

The preceding gates are resolved as follows. Earlier candidate names/shapes are
illustrations; this subsection is authoritative for implementation. Source inspection
covered owned-session admission/retirement, VM slice driving, task reports/dependencies,
prepared callable conversion and stored enum error origins. No runtime acceptance
or performance result is claimed by this documentation checkpoint.

1. **Typed calls.** Rename the SDK KagariRuntime to Runtime directly; distinguish the
   core runtime with a meaningful internal import alias. Keep PinnedFunction<A, R>.
   function/async_function bind once and use existing KagariArguments/KagariType
   checks. AsyncFunction<A, R> retains the exact callable plus Future<R> output scope.
   call takes &mut self and uses default_context; call_with_context preserves explicit
   policy. call_async and submit_async have the lifetimes shown above, but use the
   existing RunResult/EmbeddingError instead of adding CallError. Their arguments
   require IntoKagariArguments; outputs require FromKagari + Send + 'static, not Sync.
   Arguments convert during admission and need no new blanket Send bound. Binding
   uses 'static Rust type identities as existing prepared bindings do.
2. **Admission/completion.** Queue a checked prepared target and rooted arguments,
   without executing the factory. Reuse pinned-call entry authority, then await one
   Future layer in the same owned session. Extend the zero-argument queued-factory
   boundary rather than constructing an unchecked closure. Decode results against
   the retained output TypeArgument under the original execution context before
   retiring the session; publish only after cleanup. An erased Send completion sink
   prepares the result while active and publishes it after retirement. PendingCall
   owns only checked cancellation identity and Arc-backed synchronized result state;
   it never owns Runtime or a raw heap reference. Runtime teardown resolves unfinished
   tickets; completed results remain available, while object access requires a live
   owning Runtime. Non-Send custom async outputs are outside this initial API.
3. **One scheduler.** Runtime owns scheduling records/readiness and lifecycle; VM
   supplies one bounded turn using its existing Executor. Use stable round-robin
   work selection, fixed positive quanta and one shared outer allowance. Conservatively
   debit granted quanta even if a slice returns early; bound control/admission work
   too. No instruction-accounting subsystem or speed claim is needed. A notification
   hub retains durable readiness, persistent external Waker and temporary Call Waker.
   Scope signals notify this hub directly, not only host mailbox dispatchers. Register
   before checking readiness; run/drop Waker callbacks outside locks and preserve
   the existing panic/quarantine contract. Call completion/drop restores the external
   route and notifies remaining work. A Waker cannot report channel failure: host
   adapters must coalesce reliably and explicitly close on disconnection, not claim
   Runtime detects every discarded host notification.
4. **Retirement/reporting/close.** Call drop synchronously retires its invocation and
   descendants at an idle driver boundary, reusing guarded retirement rather than
   only dropping OwnedExecution. Cleanup faults quarantine while draining remaining
   owned state; expose the fault on subsequent runtime access. PendingCall drop
   requests cancellation, including before first factory execution. Retire Task
   admission records internally after terminal publication. Report independent Task
   execution failures through a bounded runtime event buffer, including failures also
   observed by script waiters; do not invent unreliable unhandled-error detection.
   Overflow retains an explicit suppressed-failure count and never blocks retirement.
   close() -> RunResult<()> is synchronous/idempotent: stop admission, cancel/retire
   local work, resolve tickets and invalidate notifications. Open idle poll is Pending;
   closed poll is Ready(Ok(())); quarantine returns a terminal error after cleanup.
5. **Groups.** A group owns two internal OwnedExecutions, never public Tasks/reports.
   Add a traced cold composition payload and a group wait/admission request. Parent
   await parks before the scheduler admits children, because owned-session creation
   cannot run inside a live parent activation. Validate left then right, reject duplicate
   identities, reserve group/session/root/dependency state, then atomically claim both
   cold inputs without allocation/callback between claims. Pre-commit failure consumes
   neither input; the outer awaited composition remains consumed. Start from claimed
   payloads, not a second take_cold_future. Preserve logical parent depth and invocation
   inputs/recording across branch sessions; siblings must not reset deterministic RNG.
6. **Composition policy.** A fixed-pair service invokes trusted stdlib normal-output
   policy through a short-lived restricted context. Runtime retains both original
   outputs; policy returns Continue or Finish(rooted_value), never drives script or
   retains untraced values. Continue after both outputs is a contract failure. Stdlib
   registers core::task::join/try_join with the signatures above and checked declaration
   identities. Rebind selected Err into the new success type while preserving the
   original optional trace, including None, without recapturing at the combinator.
   Runtime goes Running -> Closing -> Ready; finish descendant cleanup before output.
   Stable ready order selects failures; sticky parent termination and cleanup quarantine
   retain precedence. Resource exhaustion is execution termination, not SpawnError/E.
   Generalize the existing Task dependency registry to bounded multi-edge session
   dependencies, including parent -> branch edges. Shared Task waits remain non-owning.
7. **Facade scope.** Bind object_type/field/method/associated_function and expose get/set,
   call_method and call_interface_method using existing descriptors. Keep interface
   dispatch explicitly named rather than add a broad receiver-dispatch trait. Async
   method/submit entrypoints reuse checked callable admission for existing supported
   Future-returning members. Require an existing Interface value; arbitrary Object-to-
   trait casts are deferred. No latest-version handles, generated views, implicit
   specialization or unrelated loading/preparation redesign. Context override variants
   reuse ExecutionContext; do not drop observers, cancellation or backend checks.

Finite write set: AR02 owns syntax/HIR/compiler/formatter and corresponding guides;
AR03 owns runtime scheduler/session/task notification, retirement, completion boundaries
and VM turn driving; AR04 extends native Future/frame wait/group/GC origin/dependency
owners and stdlib tasks; AR05 owns SDK typed admission/facade and affected consumers;
AR06 owns focused integration and durable documentation. Core backend primitives
remain where necessary; superseded normal SDK per-Task drive/factory-stage entrypoints
are removed after migration, not kept as compatibility aliases. Existing re-export
whitelists must be updated for reviewed public declarations, never widened wholesale.

Focused acceptance follows the test owners below: AR02 block capture/control flow
and source-free execution; AR03 real wake routing, scope progress and admission-slot
reuse; AR04 dual IO, right-branch failure, atomic claims, cycles and Err provenance;
AR05 direct await with no pump, drop with no later poll, submitted ticket teardown,
object/member round trips and pinned reload. Each implementation phase runs structure,
fmt, workspace Clippy and diff checks; full test matrices stay CI-only. API prototypes
are not needed to prove Rust's ordinary exclusive-borrow rule; real compiling public
consumer examples belong to AR05 and behavioral claims require actual focused tests.

## Established implementation references

Research informs ownership and representation, not unmeasured speed claims. Recheck
mutable sources when implementing; the observations below were consulted on
2026-10-22.

| Source/version | Mechanism to reuse or compare | Kagari difference |
| --- | --- | --- |
| [Wasmtime TypedFunc](https://docs.rs/wasmtime/50.0.0/wasmtime/struct.TypedFunc.html), 50.0.0 | Typed reusable callable with explicit store access on invocation. | Reuse the handle/environment separation; review async borrowing against direct-await and optional interleaving requirements. Kagari handles retain GC roots and pinned generations; do not copy Wasmtime's execution machinery. |
| [Rust Future contract](https://doc.rust-lang.org/std/future/trait.Future.html), stable contract since 1.36 | Pending registers a wake; executor polls again; poll must not block. | Rust Future is the host adapter, not a requirement to expose Pin/poll to scripts. |
| [rustc coroutine pass](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_mir_transform/coroutine/index.html), rolling nightly documentation | Persist state and suspension-live locals; generate resume/poll and state-specific cleanup. | Retain existing VM frames; native async lowering is excluded. |
| [Kotlin coroutine specification](https://kotlinlang.org/spec/asynchronous-programming-with-coroutines.html#implementation-details), 1.9-rfc+0.1 | CPS plus state machines separates continuation from the current native call stack. | Preserve cold Future calls and explicit await, not Kotlin suspend-call semantics. |
| [Python asyncio TaskGroup](https://docs.python.org/3/library/asyncio-task.html#task-groups), consulted 3.15.1 documentation | Group exit accounts for children; failure cancels and joins remaining work. | No catchable cancellation exceptions or ExceptionGroup; preserve Result versus execution termination. |
| [OpenJDK JEP 444](https://openjdk.org/jeps/444), JDK 21 | VM-owned stack chunks and scheduler integration support transparent suspension. | Kagari does not control arbitrary Rust stacks/borrows or host blocking libraries; do not add fibers. |

The custom composition service is needed because Kagari's continuation state,
precise GC roots, verified bytecode and pinned generations belong to its runtime.
An off-the-shelf Rust executor cannot safely take over raw script continuations.
Use that executor at the wake/poll boundary, not as a replacement semantic owner.

## Implementation phases

Phases are buildable checkpoints. Sequence AR01 -> AR02 -> AR03 -> AR04 -> AR05
-> AR06. AR02 does not depend on the new scheduler; AR01 must nevertheless settle
the overall ownership before scheduling or public API changes begin. If commits
are separately authorized, use Conventional Commits and `Plan-Phase: ARxx` trailers;
mark breaking API changes and describe their impact.

### AR01 - Freeze execution ownership and public contracts

- [x] Re-audit the baseline and relevant typed-call/provider consumers.
- [x] Record single-threaded execution and the borrowed Call/non-borrowing PendingCall
      split, direct-await progress, external driving and mode-specific drop cleanup.
- [x] Close the five remaining interface/implementation gates above; validate Rust
      ownership with focused prototypes where needed. Keep real consumer compilation
      in AR05 and do not treat prototypes as execution-correctness evidence.
- [x] Review the proposed function/member/object facade with the complete usage
      scenario; freeze names, typed handles, async result mapping and trait-view scope.
- [x] Record the chosen runtime/VM/library boundaries and finite migration write set.
- [x] Identify existing semantic test owners and select focused acceptance cases.

Acceptance: direct await completes without a separate runtime pump; optional external
integration can drive background Tasks and cancellation cleanup after root-call
completion. Invocation polling and external polling share one execution scheduler,
without hidden recursive execution. Call's exclusive borrow and PendingCall's external
progress requirement are explicit; direct-await cancellation cannot depend on a pump
that was never installed. Continued host-business processing is optional.
This checkpoint may be documentation-only; it does not activate excluded features.

### AR02 - Implement async block expressions

- [ ] Extend grammar, syntax AST/parser recovery and HIR for async blocks while
      retaining named functions and async closure syntax unchanged.
- [ ] Check output inference, captures and return/?/loop-control boundaries.
- [ ] Lower through shared resume/MakeFuture machinery; preserve independent MIR
      and decoded-bytecode verification and existing effect timing.
- [ ] Update syntax/AST reading guides, formatter/editor support and diagnostics
      affected by the new expression; review unused cold-Future diagnostics at
      their existing owner without introducing a general lint framework.

Acceptance: blocks are cold, one-shot and source-free executable; nested returns
and captures preserve ordinary language semantics. Immediate, deferred and discarded
blocks reuse existing GC/cancellation/pinning coverage where contracts are identical.

### AR03 - Centralize runtime scheduling and lifecycle

- [ ] Internalize ready-task selection, safe bounded progress and wake publication.
- [ ] Reuse one executor for ordinary calls, queued factories and resumed work.
- [ ] Implement runtime-level wake/drive integration with the external scheduler,
      separate result waiters, result/report retirement and shutdown decisions.
- [ ] Exercise a minimal real host wake adapter now, before the final AR05 facade:
      synchronous root return, independent Task IO completion, bounded-turn rescheduling
      and cancellation cleanup must progress without busy polling or per-Task driving.
- [ ] Preserve independent scope Tasks, dispatcher failure cleanup and synchronous
      host reentry; coordinate parking edits with RL02 rather than duplicating proofs.

Acceptance: a spawned Task progresses without host per-Task driving; a parent
waiting on it cannot starve it. Durable readiness handles completion-before-wait,
completion-during-poll, duplicate wakes and cancelled/retired identities. Existing
low-level paths may remain until their callers migrate in AR05, not indefinitely.

### AR04 - Add structured groups and library combinators

- [ ] Implement the chosen bounded internal group representation and ownership edges.
- [ ] Reserve/validate before branch execution; handle alias/duplicate Future claims.
- [ ] Implement checked normal-output policy callbacks and closing-before-publication.
- [ ] Register two-input join/try_join in the standard library using ordinary types.
- [ ] Preserve Task wait cycle checks, nested-group depth/capacity limits, GC roots,
      source-free validation, async origins and reload pinning.

Acceptance: both branches can start IO before either completes; a right-hand Err
is observed while the left waits; results retain input order. Verify that try_join
preserves the selected Err's original source, stack and diagnostic metadata rather
than capturing the combinator location. Parent drop/cancel,
child termination and cleanup faults cannot leave live owned branches. Awaiting a
shared external Task does not transfer its ownership or cancel it on group exit.

### AR05 - Deliver the public Runtime invocation and object surface

- [ ] Expose synchronous call, borrowed driving call_async and result-only submit_async
      through one registration/execution implementation. Direct await needs no pump;
      submitted tickets allow additional Runtime calls with explicit driving.
- [ ] Expose reusable function/type/field/method bindings, struct/trait invocation
      and retained object get/set/round trips through the reviewed Runtime facade.
      Reuse existing conversion, dispatch, roots and version checks.
- [ ] Deliver the coherent public-consumer example reviewed in AR01, including
      old handles/objects versus rebound entries after reload.
- [ ] Hide factory/Future staging and internal task/report driving from normal use.
- [ ] Provide the AR01 runtime-level persistent progress/wake integration without a
      required public Driver object or mandatory executor dependency.
- [ ] Migrate affected SDK consumers/examples and remove superseded public entrypoints
      or names directly. Retain only justified low-level operations, not aliases.
- [ ] Exercise cancellation on Rust Future drop and concurrent-notification/reentry
      boundaries with the actual chosen ownership signatures.

Acceptance: a host can call a named async script with arguments and await its typed
output; a sync handler can spawn without exposing TaskId scheduling to the host.
Direct await works with no separate driver task and may intentionally pause the
calling business flow. Submitted tickets permit interleaved calls; validate wake
handoff between external driving and borrowed Call, including existing queued work.
Independent Tasks survive caller completion and can finish through external driving.
Call-drop cleanup works without subsequent polling; PendingCall-drop cleanup uses
external driving. Cover both before first drive and safe ticket teardown. No
per-task report leak, unexpected flattening or hidden block_on. Repeated calls reuse
prepared handles; object mutation and round trips preserve identity and GC retention.
Struct/trait calls preserve access and selected dispatch without raw-runtime plumbing.

### AR06 - Integrate contracts and complete local acceptance

- [ ] Run the selected end-to-end source and source-free boundary cases.
- [ ] Update current async/scope, syntax, execution, embedding and architecture owners;
      remove obsolete proposal wording and correct project-goal async status.
- [ ] Run required local structural/format/Clippy/diff checks; resolve carried errors.
- [ ] Report local results, known failures and pending CI separately.
- [ ] Move lasting contracts to their owners and remaining work to the roadmap;
      retire this plan when implementation/local acceptance is complete, preserving
      pending CI scope explicitly. Git history retains the phase ledger.

## Verification strategy

Documentation-only checkpoints run content/link review and git diff --check, not
builds or workspace tests. For implementation, reuse existing semantic owners:

- [SDK async execution](../crates/kagari/src/runtime/tests/embedding/async_execution/mod.rs):
  source semantics, captures, repeated await, nested outputs and owned execution.
- [Task waiters](../crates/kagari/src/runtime/tests/embedding/async_execution/task_waiters.rs):
  shared results, cancellation direction and dependency cycles.
- [Lifecycle tests](../crates/kagari/src/runtime/tests/embedding/async_execution/lifecycle.rs):
  parked/cold code retention and cleanup across reload.
- [External native waits](../crates/kagari/tests/async_execution.rs) and
  [artifact task tests](../crates/kagari/tests/artifact_features/async_tasks.rs):
  external consumer and frontend-free boundaries.
- [SDK typed calls](../crates/kagari/src/runtime/tests/embedding/typed_calls.rs) and
  [interface methods](../crates/kagari-vm/src/tests/native_boundary/native_boundary_functions/interface_methods.rs):
  reuse retained-object, context-policy and selected-dispatch coverage; add public
  facade cases only for distinct new usage/ownership boundaries.
- Parser/type-check/MIR and verifier owners get cases for new syntax or independent
  validation facts, not duplicate end-to-end smoke copies for each changed function.

Add only distinct new contracts: block control-flow boundaries, multi-branch
ownership and fail-fast observation, parent cleanup acknowledgement, shared Task
non-ownership, per-call wake routing, persistent background progress and typed
public-call behavior. Include a small directly affected synchronous-call/reentry
regression set. No speedup or memory-saving acceptance claim is made by this plan.

Available focused starting commands (choose affected filters per phase, do not run
all of these automatically at every checkpoint):

```text
cargo test -p kagari --lib sdk_script_future_and_for_await_contract
cargo test -p kagari --lib scoped_task_shared_wait_contract
cargo test -p kagari --lib lifecycle_reload_contract
cargo test -p kagari --test async_execution sdk_owned_native_wait_contract
```

Record newly introduced exact filters, selection rationale, results and skipped CI
scope in the ledger. New facade tests must exercise real wake/poll behavior, not
busy loops that accidentally hide lost wakeups. When several selected targets are
run together use --no-fail-fast; a failed build does not count tests as passed.

Every implementation checkpoint also performs the manual structural review and:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Full-workspace tests and complete feature/backend/platform matrices remain CI-only.
Use existing Cargo profiles, target and parallelism. Do not bump unpublished ABI
or artifact identifiers for this work alone; invalidate affected development
artifacts and batch fixture regeneration at a coherent checkpoint.

## Progress ledger

| Phase | Scope status | Evidence / remaining work |
| --- | --- | --- |
| Planning | Complete; implementation and one commit per phase authorized | Original baseline 2bfcc77c; scope remains AR01-AR06. |
| AR01 | Complete (design/documentation) | Closure decisions above resolve ownership, bounds, scheduler, groups, reports/close and finite migration. Source/link/diff review only; runtime acceptance remains in implementation phases. |
| AR02 | Not started | Async block syntax and shared lowering. |
| AR03 | Not started | Internal runtime scheduling and cleanup. |
| AR04 | Not started | Structured groups and stdlib join/try_join. |
| AR05 | Not started | Public synchronous/awaitable calls, reusable bindings and retained object/member facade. |
| AR06 | Not started | Integrated contracts and focused local acceptance. |

Known intermediate build/test errors: none established; no implementation checks
have been run for this plan. Existing unrelated findings and timing failures remain
with their current roadmap/review owners. Full CI acceptance is unrun.

Planning validation: 38 local Markdown file targets in this plan and added lines of
the three navigation/handoff documents were checked for existence; the roadmap and
API proposal heading anchors were checked separately. git diff --check and an
explicit whitespace check of the untracked plan passed. No code tests, formatting,
structure or Clippy runs are required or were performed for this documentation-only change.
