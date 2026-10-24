# Async Script and Native Execution

This document describes implemented async language and runtime contracts. Scripts
await host-owned RPC, database and timer operations without blocking the VM thread;
Rust owns IO and scheduling. [Specifications](README.md#language-and-execution-specifications)
define observable behavior; [architecture](architecture.md#owned-interpreter-activation)
defines implementation ownership.

[Runtime ownership](runtime-ownership-and-host-api-design.md) supplies centralized
stores, automatic retention and exclusive movement between host threads. The
owning VM thread below means the current exclusive driver, not OS-thread affinity.

The [host task scope design](host-task-scope-design.md) defines synchronous handlers
spawning scope-owned work, admission and mailbox-driven resumption. Cold Futures
and scheduled Tasks have distinct lifetimes; neither requires script threads or
a mandatory Actor/Tokio dependency.

The execution contract owns the current access/protection model: installation
authorizes API use, root cancellation and a call-depth limit control execution, and the host
owns deadlines/service limits. Per-task permission matrices, precise allocation
attribution and hierarchical budget delegation are not async requirements.

## Supported scope

The interpreter supports named async script functions, explicitly marked
async closures and blocks, typed awaitable native operations, nested async script calls,
cold two-input `join`/`try_join`, typed host calls, shared scheduling and cancellation.
Multiple executions may wait in one runtime, but only one may execute script at a
time. This is cooperative interleaving, not parallel heap access. Ordinary awaits
continue one chain; structured composition parks the parent and owns branch
executions. A host-provided scope creates an independent managed execution through
`spawn`; unrestricted global spawning is deferred.
Task result sharing across executions follows the cancellation and result-retention
rules below; the executable contracts define waiter storage and host completion boundaries.

Do not include script threads, a mandatory Rust async executor, implicit blocking,
detached background jobs, async generators or general async trait methods
in the first release. Closure-based spawn uses the Future-producing callable
contract below. Combinators beyond fixed two-input join/try_join, streams, general
user-defined awaitable protocols and migration or serialization of suspended executions
are later work.
Ordinary synchronous functions and standard-library callbacks retain their semantics.

## Script surface and evaluation

The following application-specific types illustrate the supported syntax. See the
[runnable host example](../crates/kagari/examples/async_tasks/main.rs) for a
self-contained two-provider flow and [its script](../crates/kagari/examples/async_tasks/script.kgr)
for ordinary `for` traversal with `.await`:

```kagari
pub async fn load_profile(id: i64) -> Result<Profile, RpcError> {
    val user = rpc.get_user(id).await?;
    val account = rpc.get_account(user.account_id).await?;
    Ok(Profile::new(user, account))
}
```

### Evaluation boundaries

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
The admission result is `Result<Task<T>, SpawnError>`; the concrete variants are specified in the executable contract below. Spawn enqueues work without executing its body inside
the caller's activation. Discarding the returned handle
does not cancel accepted work.

| Expression | Meaning |
| --- | --- |
| `f(...)` for `async fn f(...) -> T` | Create an unstarted `Future<T>` |
| `async { body }` | Capture now and create a cold `Future<T>` directly, without a callable |
| `join(left, right)` / `try_join(left, right)` | Create a cold structured composition of two Futures |
| `future.await` | Drive the Future and obtain `T`; composition owns internal branches |
| `scope.spawn(...)` | Admit a separate scope-owned execution and return its Task handle |
| `task.await` | Wait for an already scheduled execution's result |

Both Future and Task use explicit postfix `.await`, not an `await()` method.
Await is legal only in a suspension-capable body. Result and Option propagation
retain their existing meaning: `request().await?` awaits a Result/Option and then
propagates its business value. Traps and execution termination are not Results.
Dropping a cold Future without driving it performs no body execution or IO. The
ordinary unused-binding diagnostic applies; there is no separate Future must-use lint.

### Future ownership and Task results

A cold Future belongs to one runtime and independently retains its checked target,
owned arguments, rooted captures and pinned implementation generation. It does
not belong to the creating handler's execution and does not expire when that
handler returns. Temporary Rust references, host leases and unrooted values cannot
be retained in it.

First driving binds a Future to one execution. Shared aliases observe that state;
another execution cannot drive the same started Future. Runtime identity and
state checks enforce this without introducing Rust ownership syntax. Storing an
unstarted Future inside a closure or another Future is not itself forbidden:
it can be driven later by a spawned execution. Capturing a Future does not start
or recursively bind it. Actual use must still validate its state, including when
shared mutable captures have changed. A Future is driven once: a second await,
including through an alias after completion in the same execution, is an invalid
state and traps. Resuming its one active await after a host wait is not a second
await. Terminal aliases cannot restart execution or retrieve a cached result;
use Task for shared/repeated result observation.

Task handles represent scope-owned work, not cold computations. They support
same-runtime sharing and waiting across executions, including
multiple waiters and cached completed results. Waiting never reparents the target.
Reference-valued results keep ordinary shared-object semantics. Completed output
and terminal metadata remain available while the Task is reachable; GC can release
them when no handles or explicit host roots retain them. Scope bookkeeping must
not retain all completed tasks indefinitely. Keeping a completed Task reachable
must not retain its entire execution stack. Await cancellation/failure follows
the terminal-execution rules below, without adding an outer Result to the output.
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

The closure grammar accepts an optional `async` modifier before pipe-delimited
parameters and an expression/block body. Zero parameters use `async ||`;
parameter annotations and contextual inference follow ordinary closure rules.
There is no trailing-lambda form or `move` modifier. See the
[syntax specification](spec/syntax.md#async-syntax) for the supported forms.

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

### Async block expressions

`async { body }` directly creates a cold, one-shot `Future<T>` using the same
capture, resume and lifetime contracts as async closures. It does not construct
a callable or invoke an extra factory. The compiler shares capture lowering and
emits the existing `MakeFuture` instruction targeting a private resume body;
MIR and decoded-bytecode verification remain independent acceptance boundaries.

For example, `val pending = async { load_profile(id).await };` captures `id`
now and calls `load_profile` only when driven. `return` and `?` exit this body,
not its enclosing function; loop control cannot cross that boundary. A contextual
`Future<T>` supplies the body's expected output. `async { load_profile(id) }`
retains a nested Future output rather than flattening it. Ordinary closures
inside a block remain synchronous. Full source, capture and discard rules belong
to [async syntax](spec/syntax.md#async-syntax).

### Structured Future composition

`core::task` and its `std::task` facade export `join` and `try_join`; neither is a
prelude function. Both take two already-constructed Futures with ordinary
left-to-right argument evaluation and return another cold, one-shot Future.
Awaiting admits two internal branches, not public Tasks. Each branch drives exactly
one layer; output order follows input order, not completion order.

`join` returns a tuple and treats business Result values as data. `try_join` selects
the first observed Err, preserves its diagnostic origin and closes the sibling
before returning. Parent cancellation or branch execution failure also retires
structured descendants, without waiting for remote IO acknowledgement. Waiting on
an independent Task remains non-owning. See [signatures and examples](spec/builtins.md#structured-future-combinators)
and [atomic admission, dependency and cleanup rules](spec/execution.md#structured-two-input-composition).

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
Admission returns `Result<Task<T>, SpawnError>`. Both forms are supported without
name-based compiler behavior:

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

`ModuleBuilder::bind::<Async<A>, R>` binds a cold Future-producing declaration.
The adapter receives owned `A` and a reserved `Completion<R>`, and returns
`NativeStart::Ready(R)` or `NativeStart::Pending` with optional bounded cancellation.
It runs when the Future is first driven, not when allocated or when a containing
factory is enqueued. The runtime reserves the wait identity before invoking the
adapter, so immediate completion cannot race ahead of registration. Start failure
retires the reservation; late completion cannot reach a reused slot. See
[native producer registration](spec/host-interop.md#registered-cold-producers-and-vm-waits)
for the installed declaration and conversion contract.

Only owned, declared inputs may survive the start call. The operation cannot retain
`HostCallContext`, runtime borrows, frame leases or unrooted script values. External
completion carries owned host data through a checked typed adapter; conversion to
GC-managed script values occurs on the owning VM thread under the execution's
checked allocation and GC ownership. Arbitrary host data is not portable executable metadata.

A native Rust Future adapter can be added over this lifecycle without requiring
Tokio or a particular executor. A provider must not retain runtime borrows across
`Pending`; this differs from the SDK's host-side driving Call, which exclusively
borrows Runtime. A waker only schedules work and never enters the VM.
Cross-thread completion transfers owned `Send` payloads or opaque notifications,
not raw `Value`, heap access or the runtime itself. Conversion into script values
occurs on the exclusive driver.

## Host driving and session ownership

The SDK binds `AsyncFunction<A, R>` with `Runtime::async_function`. A named binding
selects the current implementation at invocation admission; the admitted queue then
retains that exact factory and output scope, even before its first poll.
`call_async` admits a cold invocation and returns `Call<'_, R>`, an exclusive
borrowing Future that drives the shared bounded scheduler when awaited. No second
pump is needed. `submit_async` instead returns `PendingCall<R>`, a nonborrowing,
result-only Future; the host continues polling Runtime to execute work. Both use
the same scheduler as managed scope Tasks and structured branches.

`Runtime::poll` registers a host Waker and performs a bounded turn. Open idle
runtimes remain Pending; clean closed runtimes return Ready(Ok(())). `close`
synchronously stops admission and acknowledges local cleanup without script
execution or remote IO acknowledgement. Dropping an unfinished Call provides the
same synchronous cleanup for its invocation and structured descendants, not
independent Tasks. PendingCall drop/cancel only requests retirement, completed by
Runtime polling, close or destruction.

The core never spins on a waiting execution, blocks on a socket, or recursively
resumes script from a completion callback. Completion queues readiness, not script
execution or an automatic retry. There is no hidden worker after a Call finishes.
See [typed SDK contracts](spec/embedding-api.md#typed-async-calls-scheduling-and-native-waits)
for exact bounds, context overrides, member calls and driver notification rules.

Existing synchronous `execute` and synchronous host reentry do not acquire hidden
`block_on` behavior. They reject entry into a suspendable resume body before starting it.
Synchronous script helpers may construct cold Futures, but cannot await them.
They may call a registered scope-spawn method that enqueues a separate execution
without entering its async body. This is not suspension of the synchronous caller.
Synchronous callback/reentry boundaries remain non-suspendable; async does not
silently make existing native callbacks suspendable.

Core session storage owns each parked execution's frames, roots, native states,
pinned versions, cancellation and termination independently of an active driver
call. No frame or native borrow spans an external wait. A short-lived activation
guard grants exclusive driving access and restores runtime state when it exits.
Attempted concurrent or recursive driving is rejected.

Independent roots must not be mistaken for synchronous nested reentry. Nested
calls share the caller's cancellation state and installation authority, but ordinary
calls select current implementations rather than a retained old dependency set.
A separate host-started root has its own cancellation state. Scheduling does not
add work charging or runtime heap quotas. Host services own task admission and queue capacity.

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
not silently drop the only completion and leave a task waiting forever. The admission/readiness contract below defines the limits and failure classes.

## Failure and cancellation

Preserve [failure semantics](spec/failure-semantics.md). RPC business errors are
ordinary declared Result values. Script traps, cancellation, resource exhaustion
and engine faults retain their separate classifications across await boundaries.
Original error origins and the logical async call chain remain available.

### Await output and terminal propagation

Both `Future<T>.await` and `Task<T>.await` produce `T` on successful execution.
There is no implicit `Result<T, TaskError>` layer and no script-catchable cancellation
exception. If `T` is `Result<U, E>`, a business Err remains that ordinary value;
await does not unwrap it or terminate the waiter. Existing `?` handles propagation.
Cancellation and traps return no value from await and do not run the continuation.

For an execution A awaiting a scope-owned Task B:

| Event | Required outcome |
| --- | --- |
| A is cancelled | Terminate A and detach its wait registration; B continues unless separately cancelled |
| B is cancelled | Terminate A as cancelled due to dependency B; do not continue after await |
| B traps or reaches another execution termination | Terminate A with the corresponding failure class and retain B's original failure provenance |
| B completes with a business Err | Return that Err as part of T; A may handle it normally |
| The owning scope closes | Cancel its unfinished tasks, irrespective of their wait relationships |

Await never reparents B. Cancelling A must not cancel B simply because A needs its
result, including when other executions also await B. When B is cancelled, its
registered waiters are scheduled for terminal cleanup, not resumed in script;
later awaits observe the same terminal outcome. Failure propagation can terminate
dependent waiters but does not cancel unrelated tasks in the scope. A caller that
is itself cancelled cannot use a completed target result to continue execution.

An ordinary directly awaited script/native Future runs in A, so its active native
waits and frames terminate with A. A structured composition owns internal branch
executions that also retire with A. Nested `scope.spawn` calls create independent
Tasks in the explicitly selected host scope, not implicit children of A.
Local structured scopes, parent-child Job trees, supervisor modes and script-level
async cleanup/non-cancellable regions are outside this release.

Reject self-waits and any new Task wait edge that would form a dependency cycle
before parking. Invalid Future reentry is likewise rejected. The exact diagnostics
and bounded dependency tracking are implementation contracts, not permission to
leave impossible waits pending forever.

### Cancellation delivery and reporting

Cancellation is a request until the runtime's exclusive driver has completed
cleanup. Queued work must not start after cancellation is observed; running work
observes it at execution checkpoints; waiting work must have a wake/cleanup path
that does not depend on the remote operation returning. Native code must cooperate
through bounded work/checkpoints; cancellation does not promise Rust preemption.
Only the driver accesses parked frames or the script heap. Cleanup must not invoke
arbitrary script code, and a failed dispatch must not strand cancellation forever.

Cancellation and successful completion have one terminal transition. Cancelling an
already completed Task does not replace its cached result; cancellation observed
before terminal completion prevents publishing a later success. A completion
payload queued by a provider is not by itself successful completion of the Task.
Late/duplicate completion cannot revive a cancelled execution. The endpoint
publication/driver protocol preserves both completion-before-cancel and
cancel-before-completion ordering.

Managed scope failures enter a bounded `TaskEvent` stream even when script has
already observed them. Events identify task/scope and failure origin; overflow
produces a suppressed count. Successful outputs, including business Err, are not
events. Ignoring events never delays admission-slot retirement. Manual backend
scopes instead retain explicit terminal reports and report-consumption ownership.
Both preserve original dependency failure provenance and logical spawn/await
locations without requiring source text. Scope close/drop requests cancellation;
`is_closed` acknowledges cleanup, while SDK Runtime close performs it synchronously.

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
outputs, whose lifetime follows the reachable result handles and explicit roots.

No borrowed host handle, runtime/table borrow, host lease or prepared commit action
may survive a suspension boundary. Compiler liveness checks cover locals and
temporaries; runtime checks cover the complete active frame chain and native
resources. Artifact validation must enforce the same facts without trusting HIR.
Suspension checks apply even when a particular await happens to complete immediately.

Ordinary `for` bodies support `.await`. Parked frames retain live cursor values
and their traced sources without a Rust borrow, storage pointer or iteration
lease. Another execution may structurally mutate a source while the loop is
parked; the resumed cursor traps before its next source read. Nonstructural
replacements remain visible. Suspension does not copy sources or restart iter/next.
Native adapters retain source and callback GC edges and checked selections.
Ordinary callback calls select current implementations; created closures retain
exact bodies.
Custom script iterators retain ordinary rooted state and own their consistency
rules. Their iter and next calls remain synchronous; awaiting in a loop body needs
no async iterator protocol.

Frame mutation guards, prepared writable paths, ephemeral host access and native
storage borrows remain non-suspendable. A mutation guard protects an unfinished operation
and cannot cross suspension. Ordinary `retain`/`sort_by` callbacks cannot contain await
under the selected callable rules: `Fn(T) -> Bool` is not `Fn(T) -> Future<Bool>`.
No additional user restriction or async collection algorithm is introduced.

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

Created closures, cold Futures and admitted frames/queued invocations retain their
exact body/provider and lexical type environment through waits. First drive or await
does not reselect an already admitted computation. Named function values and ordinary
typed bindings instead follow checked current-call routes without obsolete executable
leases. An old resumed body retains its instructions, while its ordinary inner calls
select current implementations; no rebinding is required for an existing named handle.
Results still convert using the admitted execution's types and provider contract.
Suspended state is not migrated or serialized by compatible publication.

[Module activation](spec/module-activation.md) owns the implemented publication
boundary: no active synchronous execution, driver, observer, callback/reentry or
cleanup scope may remain. Parked executions do not block publication. While the
publication guard is held, script entry/resumption, driver consumption and another
publication reject; producer-side completion notifications may still mark readiness.

Host retention policy, unfinished-job limits and deadlines bound old-generation retention. The host
may cancel old executions under an explicit deployment policy; publication does
not silently cancel them. Cancellation still does not roll back external effects.

The installed surface remains the access boundary. Completion cannot install APIs,
change execution ownership or reset its cancellation state. Candidate initialization
rejects async entries and external waits in the first release; do not park a staged
reload session or bypass its restrictions by exporting deferred external work.
The candidate rule below rejects construction and deferred-work output graphs
in candidate initialization, even though cold construction does not itself start IO.

## Compiler and runtime responsibilities

| Layer | Responsibility |
| --- | --- |
| Syntax and HIR | Async declarations, closures/blocks and await expressions, Future/Task types, capture and control-flow boundaries, diagnostics and suspension restrictions |
| Compiler and MIR | Explicit await control flow, captured/live values, effects, source origins and verified resume points; no RPC-specific lowering |
| ABI and bytecode | Portable async call contracts and bounded validation of task/result types, resume destinations, initialization, roots and non-suspendable resources |
| Runtime | Shared scheduler, structured ownership/dependencies, execution/task state, provider operations, completion identities, GC retention, limits, cancellation, exact computation ownership and current ordinary-call selection |
| VM | Generic bounded driving, wait/resume transitions and logical async stacks |
| SDK and host | Typed callable binding and exact invocation admission, driving Calls/result tickets, Runtime poll/close, managed scopes/events, registration, host readiness and IO integration |
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
sticky termination and generation retention across async suspension. Execution-mode
metadata carries suspension effects with matching verification and runtime checks. Callback resumption inside the current session
and external suspension back to the host are distinct capabilities. Async is not
a reason to reintroduce Engine-versus-Host method lists or privileged RPC paths.

## Executable contracts

Source compilation and source-free loading share these checked factory, resume,
admission and lifecycle contracts. Runtime installation does not recover source
semantics or infer missing suspension evidence.

### Semantic roles, factories and portable execution

Register `Future<T>` and `Task<T>` as nominal managed native types with exactly one
invariant output parameter and distinct checked language roles. The foundation
exports Future from `core::future` and Task from `core::task`, with `std` facades and prelude
names. Role checks use declaration identities and validated generic arity, not
spelling, member lookup or a user-defined `await` method. Physical representation
is the existing managed heap-object slot, with runtime-checked payload kind,
owner and concrete output type. No new scalar ABI category is required.

An async source declaration exposes a synchronous factory signature
`fn(A) -> Future<T>` plus a private resume-body signature with the same captured
inputs and output T. The factory allocates a traced payload retaining its exact
body/dependency generation and owned input environment; it executes no body code.
An async closure uses the existing environment/callable representation for its
factory. The resume body is never an ordinary callable target. Native factories
retain a checked registered producer identity and owned inputs instead of a script
body. Producer metadata declares output T and cold construction separately from
starting the external operation. Ordinary generic Fn dispatch still calls the
factory synchronously.

Portable contracts encode the factory/resume relationship, producer kind, output
type, suspension capability and await operand/destination. Await evaluates its
operand once. The next instruction is the resume PC; its destination becomes
initialized only on successful completion. MIR and bytecode verification derive
live initialized slots and balanced iteration/mutation scopes at this boundary,
validate the role and exact T representation, and reject direct resume-body calls,
forged producers, incompatible outputs and non-suspendable live resources. No
serialized `safe_to_suspend` boolean supplied by a producer is trusted. Linked
verification checks native declaration identities/effects; runtime validates the
actual reachable values and active resource chain before driving even an immediate
await. Artifacts contain no live runtime IDs or completion endpoints.

### Owned execution and driver interface

Runtime session storage owns stacks, roots, options, pins and terminal state.
`OwnedExecution` is a non-cloneable owner token carrying a checked session identity
and a retirement signal; it holds no runtime reference. Dropping it requests
retirement and wakes the host; cleanup is performed by the next owner-thread drive
or explicit drain, with runtime destruction as the final backstop. Scope-owned
Tasks retain their execution owner independently of script handles.

Backend VM `start` validates a pinned entry and owned arguments and returns this
owner without executing script. `drive` grants short-lived activation and returns
`Runnable`, `Waiting` or `Complete(Result<rooted output, execution failure>)`.
These are backend primitives, not SDK aliases; the SDK composes them through typed
calls and shared Runtime polling. Slice is a positive
instruction interval, not a work quota or native preemption guarantee. Slice exits
occur only at safe boundaries. Synchronous execute uses the same storage and runs
to completion without host-visible slices. Ordinary synchronous reentry joins the
current root; recursive drive and independent-root start/drive during an activation
are rejected. Internal checked scope admission remains possible during a handler.
Candidate entry keeps its existing isolated synchronous activation rules.

Execution, scope and operation IDs contain runtime owner, slot and generation.
Backend manual-scope ready notices also identify the execution incarnation;
unknown/stale notices return `Stale` without script execution, foreign owners
return an API error. SDK notification uses the shared Waker route, not per-Task
notice dispatch. Reused slots increment generations; exhausted slots retire
permanently. Host tokens cannot authorize a different runtime. Invariant
faults quarantine the runtime and retire all owned executions; script traps affect
only the execution and its dependency waiters.

### State transitions and retained ownership

| Object/state | Event | Next state and responsibility |
| --- | --- | --- |
| Future / Cold | first await | Claimed by one execution; create the script frame or reserve native operation before starting. |
| Future / Claimed | resume same await | Continue its existing drive; never repeat construction or submission. |
| Future / Claimed or Terminal | any new await | Script trap, including aliases and same-execution repeated await. |
| Future / Claimed | completion or failure | Terminal; release input/frame/operation state after moving the result into the caller. |
| Future / Cold | becomes unreachable | GC releases captured values and code pins; no external work started. |
| Execution / Queued or Runnable | drive | Running, after identity/cancellation validation. |
| Execution / Running | safe slice exit or incomplete await | Runnable or Waiting; retain owned stack and live cursor values. |
| Execution / Waiting | authorized completion or target terminal | Runnable; publication does not enter VM. |
| Execution / nonterminal | cancellation | Runnable for cleanup, then terminal Cancelled; no remote reply required. |
| Task / Queued | first drive | Invoke admitted factory once, drive its one returned Future; factory failure is Task failure. |
| Task / nonterminal | await | Register a checked dependency edge; execution waits without changing Task ownership. |
| Task / Terminal | await | Copy cached shared value or propagate terminal cause; no frame is recreated. |
| Task / nonterminal | final script handle dropped | Remains owned by scope; dropping a handle does not cancel. |
| Operation / Reserved | provider starts | Starting, with endpoint already installed; completion during start is retained. |
| Operation / Starting or Waiting | first completion | Ready with one owned payload; duplicates are rejected and disposed. |
| Operation / Ready | owner drive | Claim once, convert under pinned types, retire endpoint, resume. |
| Operation / nonterminal | start failure or cancellation | Retired; dispose payload and invoke bounded provider cleanup at most once. |
| Scope / Open | close | Closing, rejects admission and requests cancellation for unfinished tasks. |
| Scope / Closing | final driver cleanup | Closed; return cleanup acknowledgment independent of report consumption. |

A terminal transition happens once on the owner driver. Cancellation observed
before result conversion/publication wins over queued readiness; once a Task is
terminal, later cancellation cannot replace its outcome. Task waits and structured
parent-to-branch edges share one bounded dependency graph; registration rejects
self/transitive cycles before parking. Cancelling a Task waiter detaches its edge
only; cancelling or failing the target wakes dependents with original target/await
provenance. A successful business Result::Err remains a value.

Completed Task payloads retain only output and terminal metadata, traced from
reachable handles. Managed scopes automatically retire terminal admission records,
without waiting for handles to disappear or failure events to be read. Manual
backend reports retain their separate result/retirement contract. Direct Future
await and internal composition branches add no public Task ID.

### Admission, readiness and native endpoints

`SpawnError` has `ScopeClosed`, `CapacityExceeded` and `DispatchUnavailable` variants.
Cross-runtime/forged capability use, forbidden captures and restricted candidate
execution are traps or host API errors, not admission values. A caller already
terminated remains terminated; spawn cannot turn cancellation into an ordinary Err.
The scope capability is a registered opaque owned handle with runtime/scope
generation checks, safe to capture within its owning runtime.

Readiness is durable and coalesced, not stored only in a host mailbox.
`RuntimeConfig::async_limits` uses `AsyncLimits` with nonzero capacities for pending
operations, Tasks, scopes, Task waiters, scheduled invocations, failure events and
structured group count/depth. Each native operation reserves one payload slot before
provider submission. Managed terminal records retire automatically; a full failure
event buffer increments suppression rather than blocking admission. Manual backend
scope reports retain backpressure until consumed.

Storage allocation failure before spawn commitment maps to CapacityExceeded;
operation/dependency/group reservation failure while executing retains the
ResourceLimit terminal class. These are async bookkeeping/admission capacities,
not script heap or execution quotas.

Native registration uses a cold input-capture adapter and a start adapter. Capture
converts declared inputs into retained values without IO. Start receives owned
inputs and an already reserved typed completion endpoint, and returns either an
immediate owned result or an owned bounded cancellation hook. Failure rolls back
the reservation; no adapter retains NativeContext, frame borrows or raw Values.
Cross-thread endpoints require Send payloads and only touch synchronized endpoint
state. Driver-side conversion is the sole path to script heap values. Payload byte
limits and external request limits belong to the provider's declared admission
contract; the engine bounds endpoint count, not arbitrary host object sizes.

Publication stores the payload/ready bit before notifying the registered Waker;
registration rechecks readiness. A coalesced redundant wake cannot lose work. SDK
Waker notification cannot report channel disconnection: the host must reliably
service notifications or detect lost driver ownership and close Runtime. Backend
manual `TaskDispatcher` can report dispatch failure, marks admission unavailable
and retains readiness for its separate control/drain path. Neither shutdown path
relies on a remote reply or a failed mailbox. Admission commits only after capacity
and scheduling ownership are reserved; no factory runs then.

Failure diagnostics retain original class, cancellation cause, source task and
logical spawn/await sites. Source locations are optional; portable identities remain
available. Manual backend terminal reports additionally deliver rooted success
outputs. Cleanup acknowledgement means frames, leases and endpoints have retired,
not that every result handle has been dropped or remote IO rolled back.

### Suspension audit and candidate boundaries

Allowed retained state: owned scalar/managed values, code pins, checked stable IDs,
closure environments and rooted cursors. Prohibited retained state: Rust Ref/RefMut,
host scoped borrows/ephemerals, prepared writable paths, frame mutation guards and
an unfinished synchronous native/reentry activation. For-loop tests park real
executions, force GC, permit source edits and verify that subsequent stale reads
trap without repeating earlier effects. Nonstructural replacements remain visible.
Completion, cancellation and owner retirement release frame roots independently
of GC timing. No source snapshot is required.

Cold capture validation follows reachable managed values/cells with cycle detection,
not just static outer types. Reject ephemeral or scoped host resources at creation
and revalidate actual reachable mutable state on drive/suspension. Declared owned
host payloads must expose their traced edges; arbitrary raw host references cannot
hide in a Future. Source diagnostics reject known-invalid captures, and portable
verification/runtime checks protect artifacts and later alias writes.

For the first release, candidate initialization rejects Future construction,
spawn and await, and rejects candidate publication/output graphs containing Future,
Task or task-scope capabilities, including values hidden behind cells/interfaces.
This intentionally conservative rule prevents deferred work from escaping through
mutable aliases. It is checked at runtime/activation as well as in known source
contexts; no general effect/capability framework is introduced. Compatible reload
retains exact cold/parked computation bodies, types and provider contracts. New
admissions through retained named bindings and ordinary inner calls use current
implementations, as described under [reload and authority](#reload-and-authority).
State replacement remains outside this contract.

## Contract coverage

The [SDK async tests](../crates/kagari/tests/async_execution.rs) cover source,
artifact and host-driven execution. [Session tests](../crates/kagari-vm/src/tests/sessions.rs)
and [native boundaries](../crates/kagari-vm/src/tests/native_boundary.rs) own cleanup,
reentry and runtime safety. Use deterministic host completion/dispatch fixtures;
preserve cold-effect timing, single Future drive, shared Task results, cancellation,
GC, exact admitted-body retention/current ordinary-call selection across reload
and source-free rejection. CI owns the full feature/backend matrix.
