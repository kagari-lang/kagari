# Async Script and Native Execution Design

Status: proposal only; no async syntax or runtime API described here is implemented
by this document. The proposed first release lets scripts await host-owned RPC,
database and timer operations without blocking the VM thread or exposing completion
callbacks to script authors. Rust remains responsible for IO and scheduling.

This design builds on current [native registration](spec/standard-declarations.md)
and [execution control](spec/execution.md). Implementation requires separate
activation through the [roadmap](implementation-roadmap.md) and resolution of the
design gates below. Existing specifications remain authoritative until an
implementation updates them.

The later [runtime ownership and host object design](runtime-ownership-and-host-api-design.md)
owns centralized stores, automatic retention and movement of an exclusively driven
runtime between host threads. References below to the owning VM thread mean the
current exclusive driver, not permanent OS-thread affinity. This async proposal
still owns suspension and completion protocols; GO alone does not implement them.

The [host task scope design](host-task-scope-design.md) defines synchronous handlers
spawning scope-owned work, admission and mailbox-driven resumption. Cold Futures
and scheduled Tasks have distinct lifetimes; neither requires script threads or
a mandatory Actor/Tokio dependency.

The execution contract owns the current access/protection model: installation
authorizes API use, root cancellation and a call-depth limit control execution, and the host
owns deadlines/service limits. Per-task permission matrices, precise allocation
attribution and hierarchical budget delegation are not async requirements.

## Current baseline

The [syntax specification](spec/syntax.md) excludes async and coroutine syntax.
[Host interoperability](spec/host-interop.md#suspension-boundaries) and
[runtime semantics](spec/runtime.md#suspension-and-ephemerality) anticipate suspension
but only establish restrictions on borrowed and ephemeral values. A `may_suspend`
metadata field does not define task execution, wakeup or cancellation semantics.

Current [host callbacks](../crates/kagari-runtime/src/host.rs) and
[native calls](../crates/kagari-runtime/src/native/mod.rs) run synchronously;
callbacks reenter the same execution stack. They do not yield an owned async
execution to the host. [Sessions](../crates/kagari-runtime/src/session.rs)
retain root options, frames, termination and resource baselines for synchronous
execution and reentry. These are reusable concepts, not a completed async runtime.
Re-audit their implementation at activation before selecting concrete API changes.

The product boundary follows [project goals](project_goal.md) and
[architecture](architecture.md): one thread executes a script heap at a time;
the host owns networking, external resources and application scheduling.

## First release scope

The proposed release supports named async script functions, explicitly marked
async closures, typed awaitable native operations, nested async script calls,
explicit host driving and cancellation.
Multiple independent host-started executions may wait in one runtime, but only one
may execute script at a time. This is cooperative interleaving, not parallel heap
access. A root has one active await chain. A host-provided scope creates a separate
managed execution through `spawn`; unrestricted global spawning is deferred.
Task result sharing across executions is part of the chosen direction, with exact
waiter, cancellation and result-retention contracts still requiring review.

Do not include script threads, a mandatory Rust async executor, implicit blocking,
detached background jobs, async generators or general async trait methods
in the first release. Closure-based spawn uses the Future-producing callable
contract below. Parallel combinators, streams, general user-defined awaitable
protocols and migration or serialization of suspended executions are later work.
Ordinary synchronous functions and standard-library callbacks retain their semantics.

## Script surface and evaluation

Proposed syntax, not an executable repository example:

```kagari
pub async fn load_profile(id: i64) -> Result<Profile, RpcError> {
    val user = rpc.get_user(id).await?;
    val account = rpc.get_account(user.account_id).await?;
    Ok(Profile::new(user, account))
}
```

### Selected direction (2026-10-08)

`async` is a reserved keyword. An async declaration's written return type is its
completed output. Calling `async fn f(...) -> T` produces a cold `Future<T>`;
`f(...).await` drives it in the current execution and produces `T`. A call without
await does not implicitly wait or create a separately scheduled execution.
Receiver and arguments are evaluated once, left to right, when the Future is
created; the async body or native operation starts only when driven. Capture and
allocation can fail, and argument evaluation can have effects.

`scope.spawn(...)` creates a separate scope-owned execution and exposes a
`Task<T>` handle after successful admission. Use this single task API rather than
separate `launch` and `async` methods. `Task<()>` covers Unit-producing work.
The exact admission wrapper is still open; `Result<Task<T>, SpawnError>` is the
current recommendation, not a finalized signature. Spawn enqueues work without
executing its body inside the caller's activation. Discarding the returned handle
does not cancel accepted work.

| Expression | Meaning |
| --- | --- |
| `f(...)` for `async fn f(...) -> T` | Create an unstarted `Future<T>` |
| `future.await` | Drive the Future in the current execution and obtain `T` |
| `scope.spawn(...)` | Admit a separate scope-owned execution and return its Task handle |
| `task.await` | Wait for an already scheduled execution's result |

Both Future and Task use explicit postfix `.await`, not an `await()` method.
Await is legal only in a suspension-capable body. Result and Option propagation
retain their existing meaning: `request().await?` awaits a Result/Option and then
propagates its business value. Traps and execution termination are not Results.
An unused cold Future should produce a diagnostic; dropping it without driving
it performs no body execution or IO.

### Future ownership and Task results

A cold Future belongs to one runtime and independently retains its checked target,
owned arguments, rooted captures and pinned implementation generation. It does
not belong to the creating handler's execution and does not expire when that
handler returns. Temporary Rust references, host leases and unrooted values cannot
be retained in it. This replaces the earlier creator-root-bound cold Task model.

First driving binds a Future to one execution. Shared aliases observe that state;
another execution cannot drive the same started Future. Runtime identity and
state checks enforce this without introducing Rust ownership syntax. Storing an
unstarted Future inside a closure or another Future is not itself forbidden:
it can be driven later by a spawned execution. Capturing a Future does not start
or recursively bind it. Actual use must still validate its state, including when
shared mutable captures have changed. Same-execution repeat-await after completion,
terminal Future aliases and wait-cycle detection remain explicit design gates;
there must never be an accidental restart or concurrent drive.

Task handles represent scope-owned work, not cold computations. The selected
direction permits same-runtime sharing and waiting across executions, including
multiple waiters and cached completed results. Waiting never reparents the target.
Reference-valued results keep ordinary shared-object semantics. Specify cancellation
propagation, terminal error observation and result retention before implementation.
Keeping a completed Task reachable must not retain its entire execution stack.
Neither Future nor Task is a cross-runtime value, serialized continuation or
state-replacement payload. Publication must not silently retarget retained code.

### Async closure syntax and evaluation

Use an explicit `async` modifier on the existing closure expression:

```kagari
val load = async |id: i64| load_profile(id).await;
val work = async || {
    val profile = load_profile(id).await?;
    player.apply_profile(id, profile);
    Ok(())
};
```

The proposed grammar is `async_closure_expr ::= "async" closure_expr`, reusing
the current pipe-delimited parameters and expression/block body. Zero parameters
use `async ||`; parameter annotations and contextual parameter inference follow
ordinary closure rules. There is no new `async { ... }` block, trailing-lambda form
or `move` modifier. The current syntax specification remains unchanged until
implementation; these examples specify the proposed extension.

An ordinary closure remains synchronous even inside an async function or when
passed to spawn. Its own body cannot contain `.await`; a nested async closure is
a separate body and can. Neither an expected type nor the presence of an await
silently inserts `async`. Conversely, an explicitly async closure stays async even
when its body contains no await. `return` and `?` inside it target its completed
output, not an enclosing function or the Future factory's return type.

Async closure evaluation has three distinct stages:

1. Evaluating `async |args| body` creates the callable and captures its environment
   under ordinary closure rules. It does not execute `body` or create a scheduled task.
2. Calling that callable evaluates receiver/arguments once, left to right, and
   creates a fresh cold Future retaining the arguments, environment and code.
   None of `body`, including statements before its first await, executes yet.
3. Driving the Future enters `body`. Each call has separate parameters, local
   variables, resume positions and completion state. A dropped unstarted Future
   never executes the body; capture and allocation effects are not rolled back.

Creating or calling an async closure is allowed from synchronous code; driving it
with `.await` requires an async body or the host execution driver. Creation/call
can fail checked allocation or capture validation without running the body.

### Callable types and generic calls

Use the existing function-value and `Fn` protocol for the synchronous Future
factory. An async closure with parameters `A` and completed output `T` has callable
type `fn(A) -> Future<T>` and implements `Fn(A) -> Future<T>`. Named async functions
have the same callable contract. No new `AsyncFn`, `FnOnce`, ownership syntax or
`async fn(...) -> T` function-type notation is needed for this first design.

For example, with the Result-returning `load_profile` above:

```kagari
val load: fn(i64) -> Future<Result<Profile, RpcError>> =
    async |id| load_profile(id).await;
val pending = load(42);
```

The expected function type supplies parameter types and its `Future<T>` result
supplies the async body's completed output `T`. Without context, existing rules
still require parameter annotations where types cannot be supplied; the body
determines the completed output. An expected `fn(A) -> T` does not implicitly
await a Future or convert a synchronous body into an async one.

The factory invocation itself does not suspend, so existing generic
`F: Fn(A) -> Future<T>` consumers can call it through the ordinary protocol.
Suspension belongs to the separately checked Future body/resume contract. This
does not make synchronous `Fn::call` or standard callbacks suspendable, and does
not enable general user-defined async trait methods. A synchronous comparator
expecting `fn(A, A) -> Ordering` rejects a Future-producing closure.

An ordinary `|| load_profile(id)` and `async || load_profile(id).await` can have
the same callable type. The former executes its ordinary body during invocation
and returns a Future; the latter creates a Future without running its async body.
Function type equality does not promise purity or identical effect timing.
`async || load_profile(id)` instead returns `Future<Future<Result<Profile, RpcError>>>`:
omitting the inner await does not flatten the returned Future.

### Captures, repeated calls and cleanup

Async closures reuse ordinary lexical capture semantics. Copied scalar captures
remain values; shared objects remain shared; captures classified as writable
bindings retain their shared environment slots. Each produced Future independently
retains the necessary environment, so dropping the closure value or returning from
its creating handler cannot invalidate that Future. Calls do not deep-copy objects,
snapshot shared slots or keep a temporary borrow of the closure alive across await.
Captured values must obey the Future retention and suspension restrictions above.

Repeated calls create distinct Futures but share the captured environment:

```kagari
var count = 0;
val next = async || { count += 1; count };
val first = next();
val second = next();
// count is still 0; each body runs only when its Future is driven.
```

If first and second are driven sequentially, their outputs are 1 and 2. Multiple
scope tasks may also drive separate Futures from the same closure; script execution
remains serialized, while shared data may change at suspension/scheduling points.
Separate invocation state does not imply isolated captures or a transaction.

Capturing an already-created Future is different from creating one per call:
`async || pending.await` retains the same pending Future on every invocation.
The new outer Futures do not clone/reset that inner Future; its execution-binding
checks still apply. Share a Task result when multiple executions need one result,
or create a fresh Future in the body when each call should do independent work.

Completed/cancelled execution frames release their capture references exactly once.
Other closures, unstarted Futures or retained outputs can still keep shared objects
alive. Cancellation of one invocation must not invalidate another invocation's
environment; runtime shutdown invalidates all runtime-owned execution identities.

### Spawn input contract

Spawn accepts a zero-argument Future-producing callable: `F: Fn() -> Future<T>`.
After successful admission it exposes `Task<T>` (with the admission wrapper still
to be finalized). Both forms are supported without name-based compiler behavior:

```kagari
ctx.tasks.spawn(|| load_profile(id));
ctx.tasks.spawn(async || {
    val profile = load_profile(id).await?;
    player.apply_profile(id, profile);
    Ok(())
});
```

Admission retains the callable without invoking it. When the host first drives
the new execution, it invokes the factory once under that execution's controls,
validates the returned Future and drives it there. Factory failure terminates
the accepted task, rather than retroactively changing admission to a failure.
A factory returning an already-bound Future cannot bypass ownership checks.

Spawn drives exactly the returned Future, not arbitrary nested Future/Task values.
For example, `spawn(async || load_profile(id))` yields a task whose output is itself
a Future. A bare Future argument and a synchronous closure returning Unit do not
satisfy this callable contract; use `async || { synchronous_work(); }` for the
latter. Direct-Future overloads and automatic wrapping are outside this design.
Dropping or awaiting the Task never reinvokes its factory.

Callable and native metadata must encode these types and effects. Frontend-free
verification validates the factory call and Future resume contract independently;
no special behavior may be inferred from `spawn`, a provider name or a value shape.

## Native registration boundary

An asynchronous native operation is registered through the common provider contract
and linked slot mechanism. Its contract carries typed parameters and completed
output, passing rules, creation/start effects and execution mode. Installed bindings
and execution-phase restrictions determine access without per-task capability bits.
No RPC name or standard-library method belongs in compiler or VM dispatch.

Separate three capabilities:

| Capability | State and behavior |
| --- | --- |
| Direct native entry | Returns during the current call; may support checked synchronous reentry |
| Resumable native callback entry | Retains algorithm state while the VM executes a supplied script callable |
| Awaitable native operation | Creates a cold Future; starts external work when that Future is driven, then completes immediately or waits for a host event |

A simple RPC does not require a handwritten native algorithm state machine. Its
start adapter submits a request and hands ownership of completion to the runtime.
The script execution carries the continuation after `.await`. A native algorithm
that itself waits and then continues needs resumable state, but that is a distinct
use case, not a requirement for every native function.

Conceptual registration sketch, with API names and adapter types still undecided:

```rust
registry.register_async(contract, move |start, owned_request| {
    let completion = start.reserve_completion()?;
    let operation = rpc.submit(owned_request, completion.sender())?;
    Ok(completion.waiting(operation))
});
```

The adapter runs when the native Future is first driven, not when the
Future is allocated or a containing callable is enqueued by spawn. It also has
an immediate-completion path. `reserve_completion` illustrates an ownership and
race requirement: install the wait identity before an operation can complete.
Its reservation rolls back on start failure; a late completion cannot reach a
reused slot. This is not a promise of a particular closure signature or ABI enum.

Only owned, declared inputs may survive the start call. The operation cannot retain
`HostCallContext`, runtime borrows, frame leases or unrooted script values. External
completion carries owned host data through a checked typed adapter; conversion to
GC-managed script values occurs on the owning VM thread under the execution's
checked allocation and GC ownership. Arbitrary host data is not portable executable metadata.

A Rust Future adapter can be added over this lifecycle, but the language and core
runtime must not require Tokio or a particular executor. A Future must not borrow
the runtime across `Pending`; a waker only schedules work and never enters the VM.
Cross-thread completion, if exposed, transfers owned thread-safe payloads or opaque
notifications, not `Value`, GC handles or the runtime itself. A local-only adapter
need not impose a blanket `Send` requirement.

## Host driving and session ownership

The embedding API needs an owned execution handle and a bounded drive operation.
Conceptual outcomes are completion, waiting for an external event, or a runnable
execution whose host-selected instruction slice has ended. Waiting is not a trap;
slice exhaustion is not total work exhaustion.

The host starts an async entry, drives runnable work, services IO, delivers owned
completions and drives newly runnable work. The core never spins on a waiting
execution, blocks on a socket, or recursively resumes script from a completion
callback. Completion of a native operation queues readiness; script runs only
when the host explicitly drives it. No automatic retry of an external operation
is performed by the execution protocol.

Existing synchronous `execute` and synchronous host reentry do not acquire hidden
`block_on` behavior. They reject entry into an async target before starting it.
Synchronous script helpers may construct cold Futures, but cannot await them.
They may call a registered scope-spawn method that enqueues a separate execution
without entering its async body. This is not suspension of the synchronous caller.
Synchronous callback/reentry boundaries remain non-suspendable; async does not
silently make existing native callbacks suspendable.

Current synchronous session lifetimes cannot simply be held open as Rust borrows
across an external wait. The future runtime must own each parked execution's
frames, roots, native states, ownership, pinned versions, cancellation state and
termination independently of an active driver call. A short-lived activation
guard grants exclusive driving access and restores runtime state when it exits.
Attempted concurrent or recursive driving of the same execution is rejected.

Independent roots must not be mistaken for synchronous nested reentry. Nested
calls share the caller's cancellation state and versions; a separate host-started
root has its own cancellation state. Scheduling does not add work charging or
runtime heap quotas. Host services own task admission and queue capacity.

## Waiting and completion lifecycle

Each awaited native operation has an opaque identity scoped to its runtime,
execution, operation slot and generation. Only the authorized completion endpoint
may submit a result for that identity. Duplicate and stale completions are safely
rejected or ignored with bounded diagnostics; neither resumes script twice.

1. Validate arguments, installed binding, suspension safety and capacity; reserve the operation
   and completion endpoint before starting external work.
2. Start once. Publish an immediate checked result, or install the owned waiting
   state without retaining temporary borrows.
3. Completion marks that operation ready and requests host scheduling. A completion
   arriving during start is retained; it cannot be lost before waiting is installed.
4. On drive, check termination, claim the result exactly once, convert it under
   the correct runtime/session context and continue after the await point.
5. Completion, failure or cancellation retires the endpoint and releases its
   resources. Late messages cannot revive the operation or a replacement execution.

Readiness registration and completion publication must also avoid lost wakeups
when integrated with a host Future. Completion queues, payloads, outstanding roots
and operation counts are bounded; overload must have a defined terminal outcome,
not silently drop the only completion and leave a task waiting forever. Exact
limits and failure classification are a pre-implementation design gate.

## Failure and cancellation

Preserve [failure semantics](spec/failure-semantics.md). RPC business errors are
ordinary declared Result values. Script traps, cancellation, resource exhaustion
and engine faults retain their separate classifications across await boundaries.
Original error origins and the logical async call chain remain available.

Root cancellation is sticky across all nested async calls and native operations.
It prevents further script execution, invalidates completion endpoints and releases
frames, roots, native state and retained code. Cancellation must wake a waiting
host driver; it cannot depend on the remote service eventually answering.
Dropping the host's execution owner requests cancellation and deterministic runtime
cleanup. For admitted tasks, that owner belongs to the host scope, not the spawning
handler or a script Task handle. Closing the scope cancels its tasks; dropping a Task
does not. Runtime shutdown cancels all parked executions before releasing the heap.

Cancellation requests host-owned external cleanup without waiting for it or running
script. Provider cleanup must be bounded, nonblocking and safe to invoke once;
failure to meet this obligation is not recoverable script behavior. Completed
network writes and other business effects remain completed. Stopping observation
of an RPC result does not guarantee that the remote server stopped processing it.

An RPC-specific timeout may be an ordinary `RpcError` if declared by that API.
The host owns task deadlines and triggers cancellation when one expires; it also
defines whether waiting counts toward that deadline. The engine does not add a
second wall-time budget. There is no script work allowance or heap quota. Scheduling slices preserve
cancellation and runtime call-depth limits.
A runtime invariant failure quarantines the
runtime and retires all its executions, not just the currently driven one.

## Values and resources across await

Cold Futures, parked frames, task captures, cached outputs, callback captures and
native state participate in explicit GC retention. Waiting alone does not keep a raw `Value` alive.
Root release must be exactly once on every terminal path, including an abandoned
Future, conversion failure, shutdown and a race with completion. Terminal execution
cleanup releases its frames and waiting resources independently of retained Task
outputs; the precise completed-result lifetime is a design gate.

No borrowed host handle, runtime/table borrow, host lease or prepared commit action
may survive a suspension boundary. Compiler liveness checks cover locals and
temporaries; runtime checks cover the complete active frame chain and native
resources. Artifact validation must enforce the same facts without trusting HIR.
Suspension checks apply even when a particular await happens to complete immediately.

Implicit resources matter too: iterator guards, writable path preparation and
collection mutation guards cannot accidentally remain locked during external IO.
The first release rejects suspension through such a live resource unless its
contract has an explicitly reviewed suspend-safe representation. It does not
silently release a guard and resume with weaker semantics. In particular, ordinary
prepared `retain`/`sort_by` operations remain synchronous, and a guarded loop may
need an owned snapshot traversed by index or another explicitly suspend-safe
iterator before awaiting in its body. Copying a container alone does not make an
iteration guard suspendable.

Owned host IDs and permitted stable path descriptors are not Rust borrows. Their
validity and authority are checked again on access after resumption; they do not
guarantee that the underlying entity still exists. Another root or host operation
may change shared objects while this execution waits. Await is neither a lock nor
a transaction: business code must revalidate relevant conditions, use versioned
host operations, or request an explicit host transaction.

## Reload and authority

The [update model](update-model-design.md) distinguishes compatible publication
from state replacement. The retention rules below describe compatible publication.
State replacement closes admission and drains or explicitly cancels all work that
can access the old state before snapshotting; cancellation must complete cleanup.
No task/continuation is serialized into the new runtime. Late completion identities
remain tied to the old scope/runtime and cannot mutate restored state. Cancelling
local work does not undo remote RPC effects or resurrect jobs if replacement aborts.

Cold Futures and callable captures retain their resolved code before execution;
spawn or first await must not resolve them again against the latest publication.
An execution pins its dependency program and native implementation owners through
all waits, following [module activation](spec/module-activation.md). Publication
affects new roots, not the continuation of old ones. Results are converted using
the waiting execution's types and provider contract, never the latest registration.
Suspended executions are not automatically migrated to new code.

Host retention policy, unfinished-job limits and deadlines bound old-generation retention. The host
may cancel old executions under an explicit deployment policy; publication does
not silently cancel them. Cancellation still does not roll back external effects.

The installed surface remains the access boundary. Completion cannot install APIs,
change execution ownership or reset its cancellation state. Candidate initialization
rejects async entries and external waits in the first release; do not park a staged
reload session or bypass its restrictions by exporting deferred external work.
The new independently retained Future model requires an explicit candidate-output
rule before implementation; creating a Future is not the same as starting IO.

## Compiler and runtime responsibilities

| Layer | Proposed responsibility |
| --- | --- |
| Syntax and HIR | Async declarations, await expressions, Future/Task types, callable suspension contracts, diagnostics and source-level suspension restrictions |
| Compiler and MIR | Explicit await control flow, captured/live values, effects, source origins and verified resume points; no RPC-specific lowering |
| ABI and bytecode | Portable async call contracts and bounded validation of task/result types, resume destinations, initialization, roots and non-suspendable resources |
| Runtime | Owned execution/task state, provider operations, completion identities, GC retention, runtime limits, cancellation and pinned generations |
| VM | Generic bounded driving, wait/resume transitions and logical async stacks |
| SDK and host | Execution handles, registration adapters, readiness integration, IO, timers and scheduling |
| Native backends | Recognize unsupported suspendable execution before entry and use checked interpreter fallback |

The interpreter can preserve explicit frames and resume positions; implementation
need not generate a separate Rust Future or heap object for every source local.
Representation and allocation choices require measurements, not performance claims.
JIT support for suspension is deferred. Never enter unsupported native code and then
restart it in the interpreter after effects have occurred.

Portable artifacts encode program behavior, not live tasks, OS resources, wakers
or runtime operation IDs. Retain ABI/format identifiers and invalidate affected
development products when contracts change. Establish a version boundary for a
published compatibility commitment; add no old-format reader for disposable caches.
Keep source-free validation and the existing crate dependency boundaries intact.

## Relationship to synchronous native execution

Synchronous direct entries and callbacks remain useful without async. Async work
must preserve their ordinary linked invocation and cleanup contract.

Reuse trusted provider contracts, linked identities, rooted callables,
sticky termination and generation retention when async is
implemented. Extend execution-mode metadata generically at that time, with matching
verification and runtime support. Callback resumption inside the current session
and external suspension back to the host are distinct capabilities. Async is not
a reason to reintroduce Engine-versus-Host method lists or privileged RPC paths.

## Design gates and implementation sequence

This is a queued design. Re-audit the implementation and resolve these gates
before activating async work:

- Lower the selected explicit async-closure/Future-factory contract through source
  declarations, generic `Fn` metadata and source-free callable/resume verification;
  finalize the native registration adapter without adding an async trait system.
- Specify Future same-execution repeat-await and terminal states; Task multi-waiter,
  cached-output and error-observation lifetimes; cancellation between waiters,
  targets and nested spawns; and self/transitive await-cycle handling.
- Finalize spawn admission/result types and the companion's dispatch contracts.
  Decide whether local structured scopes are deferred; do not infer task parenting
  merely from lexical nesting of spawn calls using a host scope.
- Specify source grammar, offline declaration encoding, exact suspension effects,
  ephemeral-value analysis, candidate-output restrictions and artifact verification rules.
- Specify owned execution APIs, activation/reentry rules, completion ownership,
  cancellation races, overload behavior and host lifetime/shutdown requirements.
- Define scheduling slices, host deadline cancellation,
  bounded completion storage and trace/debug behavior while several roots are parked;
  do not reintroduce a generic hierarchy of quotas or permissions.

Suggested vertical implementation order:

1. Update the relevant syntax, type, execution, host, failure, security, reload,
   artifact and embedding specifications; finalize executable contracts and examples.
2. Prove one registered async native operation with immediate and deferred completion
   through the generic provider path and an owned, cancellable execution handle.
3. Implement async functions and await through HIR, MIR and bytecode; validate
   encoded artifacts and run a two-RPC script with normal Result propagation.
   Include explicit async closures and a synchronous handler spawning that flow
   through the Future-producing `Fn` contract into a host-owned scope.
4. Complete independent parked roots, Task waiters, GC, borrow restrictions, reload,
   debugger origins and all cleanup/race cases before claiming supported async.
5. Validate an external embedding consumer, publish examples and run the final
   feature matrix. Convenience Future adapters may follow the executor-neutral API.

Each implementation checkpoint must cover a real producer-to-consumer path. Record
intermediate failures and their owner here after activation; do not weaken tests
or add production placeholder success. This proposal does not activate any phase.

## Acceptance evidence

Use a deterministic fake host service that can complete immediately, hold a request,
fail it or deliver late/duplicate messages. Core tests must not depend on live RPC
servers or timing sleeps. Required cases include:

- A two-request script returns correct results and propagates the first business
  error without issuing the second request; creation/await evaluation occurs once.
- Explicit async closures work with zero/typed/contextual parameters, expression
  and block bodies, generic `Fn` calls, and return/propagation boundaries. Await in
  an ordinary nested closure is rejected; no-await async bodies remain lazy.
- Closure construction, factory call and Future drive preserve their separate
  effect timing. Repeated calls have separate frames with ordinary shared captures;
  Futures survive closure drop/handler return and reject unsafe retained borrows.
- Spawn invokes both ordinary Future factories and async-closure factories once,
  only after admission and caller activation exit. Wrong return types are rejected;
  nested Futures are not implicitly flattened and captured Futures are not reset.
- Cold Futures do not run when discarded and survive creator-handler return when
  retained. Future aliases cannot restart work or drive a bound Future from another
  execution. Foreign-runtime access and invalid wait cycles are rejected.
- Scope-admitted work outlives its spawning handler, resumes only through its
  dispatcher, and executes once per admission; scope close cancels admitted tasks.
  A captured unstarted Future can first be driven in the admitted execution.
- Task waiters across executions observe one completion without duplicate work;
  retained results survive GC while completed execution frames are released.
  Waiter/target cancellation and terminal errors follow the finalized contracts.
- Immediate and deferred completions agree; completion-before-wait and wakeup races
  lose no result; duplicate/late/stale-slot events cannot resume another operation.
- A waiting execution consumes no drive loop; another root runs with independent
  cancellation state and pinned versions under the same installation. Host drive
  slices preserve call-depth and lifetime checks rather than resetting them.
- Cancellation before start, while waiting, after readiness and before conversion,
  plus owner drop and runtime shutdown, release execution-owned frames, roots and
  leases; only explicitly retained outputs/cold Futures keep their necessary roots.
- GC during every wait and completion conversion preserves captures and results;
  memory and queue limits fail without leaking or waiting indefinitely.
- Await through host borrows, prepared writes, guarded iteration and synchronous
  reentry is rejected; valid owned data survives and accesses are revalidated.
- Reload while RPC is pending resumes old code/contracts; new roots use the new
  publication; retained cold Futures keep their original target; candidate
  execution cannot suspend, spawn external work or export forbidden deferred work.
- Source and artifact paths, malformed async artifacts, source-disabled SDK use,
  synchronous API rejection and JIT pre-entry fallback satisfy the same semantics.
- A separate host consumer adds a second RPC by changing only its declaration,
  implementation, registration and tests, with no compiler/VM method cases.

Reuse existing [session tests](../crates/kagari-vm/src/tests/sessions.rs),
[native boundary tests](../crates/kagari-vm/tests/native_boundary.rs)
and [host interface tests](../crates/kagari-embed/tests/host_interfaces.rs), extending
their meaningful behavioral coverage. GitHub CI owns full implementation acceptance:
repository structure, formatting, clippy, workspace-test and diff checks, plus the
source-free/native feature and dependency matrix. Local iteration uses focused
checks under repository policy. Measure runtime/parked memory,
allocation counts and drive overhead before making performance claims.
