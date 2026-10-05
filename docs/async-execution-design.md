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
launching Actor-owned async work, scope admission and mailbox-driven resumption.
It refines the original root-bound task proposal without introducing script threads
or a mandatory Actor/Tokio dependency.

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

The proposed release supports named async script functions, typed awaitable native
operations, nested async script calls, explicit host driving and cancellation.
Multiple independent host-started executions may wait in one runtime, but only one
may execute script at a time. This is cooperative interleaving, not parallel heap
access. A root has one active await chain. A host-provided scope may admit a cold
task as another managed execution; unrestricted global spawning is deferred.

Do not include script threads, a mandatory Rust async executor, implicit blocking,
detached background jobs, async generators, async closures or async trait methods
in the first release. Parallel combinators, streams, general user-defined awaitable
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

An async declaration's written return type is its completed output. Calling
`async fn f(...) -> T` produces a `Task<T>`; awaiting it produces `T`.
`await` is permitted only in an async body. Result and Option propagation keep
their existing meaning: `request().await?` first awaits a Result/Option, then
propagates its business value. Traps and execution termination are not Results.

Recommended initial task policy, to be ratified before implementation:

- Tasks are cold: receiver and arguments are evaluated once, left to right, at
  task creation, but the async body or native operation starts only when driven
  after first await or explicit host-scope launch.
  Capture and task allocation can fail; argument evaluation can have effects.
- Tasks use shared reference identity, not Rust ownership semantics. Re-awaiting a
  completed task in its owning root returns its cached output without restarting
  IO; reference-valued outputs retain ordinary shared-object semantics.
- Creation captures the checked target, owned arguments and pinned implementation
  generation. Await or scope admission validates installed bindings, ownership and
  execution protection before work.
  Creating several cold tasks does not start concurrent requests.
- An unstarted discarded task performs no IO. Unused task expressions should
  produce a diagnostic, so forgetting `.await` is visible.
- Awaiting an already running ancestor task is rejected as an await cycle, not
  allowed to deadlock. Multiple concurrent waiters are outside the first release.
- Cold tasks belong to one runtime and retain their creating context until started.
  Await binds one to the current async root; explicit scope admission binds it to a
  new scope-owned execution before the creating handler ends. Started tasks cannot
  be transferred or launched again. Scope admission retains captures independently
  of the old handler; it does not switch the captured code version or runtime.
- Tasks are not durable values, reload migration payloads or cross-root join handles.
  Stored aliases do not extend an execution's lifetime; awaiting a started task from
  another root or after its execution ends fails validation. Root cleanup releases
  its owned resources even when stale handles remain reachable. Successfully
  launched work is instead owned by the host scope until completion or cancellation.

Named async functions may call each other and ordinary synchronous helpers.
Callable metadata must distinguish a callback returning `T` from one returning
`Task<T>`; `sort_by` does not implicitly await an async comparator. Awaitable behavior
must not be inferred from a provider name, function ID or a runtime value shape.

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
| Awaitable native operation | Starts external work when an awaited or launched task is driven, then completes immediately or waits for a host event |

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

The adapter runs when the awaited or launched task is first driven, not when the
cold task is allocated or merely enqueued by launch. It also has
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
Synchronous script helpers may construct cold tasks, but cannot await them.
They may call a registered scope-launch method that enqueues a separate execution
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
cleanup. For admitted jobs, that owner belongs to the host scope, not the launching
handler or a script Job handle. Closing the scope cancels its jobs; dropping a Job
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

Parked frames, task captures, cached outputs, callback captures and native state
are explicit GC root sources. Waiting alone does not keep a raw `Value` alive.
Root release must be exactly once on every terminal path, including an abandoned
task, conversion failure, shutdown and a race with completion.

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
reload session or bypass its restrictions by returning an async task.

## Compiler and runtime responsibilities

| Layer | Proposed responsibility |
| --- | --- |
| Syntax and HIR | Async declarations, await expressions, completed-output typing, task types, diagnostics and source-level suspension restrictions |
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

- Ratify cold task creation, cached repeat-await, scope admission, execution
  ownership, escaping stale handles and task-type interactions with ordinary
  callable/generic APIs, including the companion's launch and dispatch contracts.
- Specify source grammar, offline declaration encoding, exact suspension effects,
  ephemeral-value analysis and artifact verification rules.
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
   Include a synchronous handler launching that flow into a host-owned scope.
4. Complete independent parked roots, accounting, GC, borrow restrictions, reload,
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
- Cold tasks do not start when discarded; repeated await never repeats IO; recursive,
  foreign-runtime, cross-root and expired task awaits fail correctly.
- Scope-admitted work outlives its launching handler, resumes only through its
  dispatcher, and cannot be launched twice; scope close cancels all admitted jobs.
- Immediate and deferred completions agree; completion-before-wait and wakeup races
  lose no result; duplicate/late/stale-slot events cannot resume another operation.
- A waiting execution consumes no drive loop; another root runs with independent
  cancellation state and pinned versions under the same installation. Host drive
  slices preserve call-depth and lifetime checks rather than resetting them.
- Cancellation before start, while waiting, after readiness and before conversion,
  plus owner drop and runtime shutdown, leave no retained frames, roots or leases.
- GC during every wait and completion conversion preserves captures and results;
  memory and queue limits fail without leaking or waiting indefinitely.
- Await through host borrows, prepared writes, guarded iteration and synchronous
  reentry is rejected; valid owned data survives and accesses are revalidated.
- Reload while RPC is pending resumes old code/contracts; new roots use the new
  publication; candidate execution cannot suspend or launch external work.
- Source and artifact paths, malformed async artifacts, source-disabled SDK use,
  synchronous API rejection and JIT pre-entry fallback satisfy the same semantics.
- A separate host consumer adds a second RPC by changing only its declaration,
  implementation, registration and tests, with no compiler/VM method cases.

Reuse existing [session tests](../crates/kagari-vm/src/tests/sessions.rs),
[native boundary tests](../crates/kagari-vm/tests/native_boundary.rs)
and [host interface tests](../crates/kagari-embed/tests/host_interfaces.rs), extending
their meaningful behavioral coverage. Final implementation acceptance includes the
repository structure, formatting, clippy, workspace-test and diff checks, plus the
source-free/native feature and dependency matrix. Measure runtime/parked memory,
allocation counts and drive overhead before making performance claims.
