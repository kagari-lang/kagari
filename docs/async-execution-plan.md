# Async execution implementation plan (AX00-AX06)

Status: planned, not activated. The user requested this execution plan after
agreeing the language direction; no implementation phase has started. The
[roadmap](implementation-roadmap.md#async-execution-ax00-ax06-planned) owns activation,
phase checkboxes, validation evidence and carried failures. This file owns phase
boundaries, dependencies and acceptance. The [async design](async-execution-design.md)
and [host task scopes](host-task-scope-design.md) own the agreed behavior.

## Outcome and finite scope

Deliver a source-to-artifact-to-host path in which synchronous handlers spawn
scope-owned tasks, await typed host IO without blocking the driver, and resume
only when the host drives them. A second message can run while the first task
waits. Cancellation, GC, failure provenance and code generations remain valid
through every wait and terminal path.

Implement these agreed contracts without reopening their language choices:

- Reserved `async fn` and explicit `async |args| body`; ordinary closure syntax
  never becomes async through inference or its receiving API.
- Async calls produce cold runtime-local `Future<T>` values. Creation evaluates
  arguments once; first drive runs the body. Cold values independently retain
  captures beyond the creating handler. One await owns the drive; a second await
  through any alias traps, including after completion.
- Async callables use `fn(A) -> Future<T>` / `Fn(A) -> Future<T>`. Calls have separate
  Future state and ordinary shared captures; no AsyncFn or ownership syntax.
- `scope.spawn` accepts `Fn() -> Future<T>`, returning
  `Result<Task<T>, SpawnError>`. It retains the factory at admission and invokes
  it once on first drive. It does not flatten nested outputs or accept bare Futures.
- Future and Task use postfix `.await`, returning T without an extra Result.
  Business Err remains a value; cancellation/traps terminate the waiting execution.
- Tasks belong to the selected host scope, can have multiple same-runtime waiters,
  and retain results for reachable handles without retaining completed stacks.
  Cancelling a waiter does not cancel its target; cancelling a target terminates
  dependent waiters. Scope close cancels unfinished tasks. Reject wait cycles.
- Completion only publishes readiness; the exclusive host driver runs script and
  performs cleanup. Cancellation acceptance and completed cleanup are distinct.
  Reports retain task/scope identity, cancellation cause and dependency provenance.

Exclude parent-child task trees, local structured scopes, supervisor modes, async
trait methods, generators/streams, user-defined awaitables, join/race/select APIs,
script cancellation handlers, async cleanup, suspended-state serialization, native
JIT suspension and mandatory Tokio integration. Do not activate the package,
embedding-facade or state-replacement proposals. Preserve their existing boundaries
without implementing those tracks. Rust Future convenience adapters can follow later.

Use existing crates and registration boundaries. Keep format/ABI identifiers;
invalidate affected unpublished products instead of adding compatibility readers
or routine version bumps. Do not introduce execution charging or heap quotas.

## Confirmed baseline and owners

Planning inspected the current synchronous path, not a running async prototype:
`ExecutionSession` borrows runtime resources, `Executor` owns a borrowed execution
stack, native registration accepts synchronous Rust entries, and SDK `execute`
opens a synchronous session. GO already provides checked session IDs, centralized
retention and Send/non-Sync runtime ownership. Reuse those facilities without
assuming that a parked execution is already supported.

| Owner | Concrete implementation boundary |
| --- | --- |
| `kagari-syntax/src/{lexer.rs,token.rs,parser/grammar,ast}` | Async declarations/closures, postfix await, recovery and source spans. |
| `kagari-types/src/{ty,declaration}`, `kagari-stdlib/src/catalog` | Checked Future/Task semantic roles, completed-output/factory declarations and generic registration. |
| `kagari-hir/src/{hir,lower,typeck,native,analysis}` | Async body context, Fn inference, capture/suspension facts, generated declarations and cache/tooling consumers. |
| `kagari-compiler/src/source/lower`, `kagari-mir/src/{function.rs,instruction.rs,verify}` | Factory/body lowering, explicit resume flow and checked liveness/effects. |
| `kagari-contract/src`, `kagari-bytecode/src/{instruction.rs,verifier,program}` | Portable call/resume/type contracts, bounded codec and independent verification. |
| `kagari-runtime/src/{session.rs,session/store.rs,frame,resource.rs,native,gc}` | Owned execution records, activation, Future/Task state, operation endpoints, roots and cleanup. |
| `kagari-vm/src/{executor,vm}` | Bounded driving, wait/return transitions, suspension-safe frame access and backend entry selection. |
| `kagari-embed/src`, `kagari-runtime/src/native` | Host execution/scope handles, registration adapters, ready/report integration and checked payload conversion. |
| `kagari-codegen`, `kagari-codegen-cranelift`, runtime backend selection | Reject unsupported suspension before native entry; never restart after effects. |

These paths identify responsibilities, not required new modules. Before editing
each phase, inspect its implementation/tests, status and diff. Split growing
responsibilities with normal modules; do not accumulate this implementation in
facades. Runtime/VM/bytecode remain frontend-free; HIR depends on semantic types,
not runtime/ABI contracts. Backends consume checked facts rather than infer types.

## Phase order and checkpoint policy

Execute AX00 through AX06 in order after implementation activation. Each phase
must have a real producer/consumer boundary and pass its focused local exit gate.
No phase is permitted to commit known compilation or focused-test failures for
the next phase to repair. Record unexpected failures with command, diagnostic,
cause and owner, repair them before marking that phase locally complete, and do
not claim an unrun build succeeded.

Internal interfaces can evolve directly across checkpoints, but all affected
callers must migrate in the owning checkpoint. Not-yet-supported source or encoded
operations must reject explicitly before execution. An intermediate rejection is
not final acceptance; no fake success, executable placeholders, disabled validation
or old parallel semantic implementation is allowed.

### AX00 - Concrete contracts and specification handoff

Own the implementation contract, not a new product-design round. Resolve names,
layouts and finite limits autonomously within the agreed semantics and record
them in the owning design/specifications before the relevant code is enabled.

- Specify registered identities for Future/Task and how the compiler recognizes
  their checked roles without recognizing application method names. Define Fn
  factory versus resume-body signatures, physical representations and wire facts.
- Specify owned execution/scope/operation identities, activation guards and drive
  outcomes: completed, waiting, runnable after a slice, and harmless stale notice.
  Define how the host enters an async factory without holding a runtime borrow.
- Write transition tables for Future single drive, Task result/waiter retention,
  operation reservation/readiness, scope admission/close and one terminal outcome.
  Include factory failure after successful spawn, self/transitive cycles, stale IDs,
  generation exhaustion, runtime quarantine and cancellation-versus-completion.
- Specify SpawnError variants, queue capacity/reservation and dispatch-failure
  cleanup. Choose durable bounded readiness storage and its wakeup protocol;
  do not let mailbox failure lose accepted work. Define terminal report identity,
  provenance, payload retention and cleanup acknowledgment.
- Audit implicit live resources: writable path preparation, iteration/mutation
  guards, synchronous reentry, GC/frame leases and candidate initialization.
  Define cold-Future capture/escape and candidate-output validation, including
  shared mutable captures; no HIR-only proof may authorize unsafe artifact input.
- Record draft additions to syntax/traits, execution/runtime, host/embedding,
  failure/reporting, bytecode/artifact and activation/backend specifications.
  Clearly distinguish scheduled behavior from currently executable support.

Exit: every later phase has concrete input/output/state contracts and an owner;
no pending language decision is hidden in a placeholder type. Document review,
local links/anchors and diff checks suffice; do not run a Rust build for prose.

### AX01 - Owned execution lifetime and bounded driver

Depends on AX00. Replace the borrowed-call-only lifetime where necessary with
runtime-owned execution records and short-lived exclusive activation. Adapt the
existing synchronous entry/reentry path to the same storage, preserving its behavior.

- Retain frames, register windows, roots, code owners and cancellation state across
  driver calls. Do not retain Ref/RefMut guards or a borrowed Executor between turns.
- Drive existing synchronous verified programs to completion or a safe runnable
  slice exit; restore driver state and preserve once-only side effects at each exit.
  Waiting operations are enabled in AX02, not simulated with success in this phase.
  A slice exit must not park non-suspendable resources anywhere in the active chain;
  defer it until a valid boundary rather than dropping/reacquiring a guard silently.
- Preserve nested synchronous reentry, call-depth protection, observer/debugger
  ownership, exclusive runtime transfer and candidate-session separation.
- Implement queued/running cancellation, owner-drop cleanup, generation-checked
  access and concurrent/reentrant-drive rejection. Scheduling slices are not work
  budgets and cannot preempt arbitrary Rust native bodies.

Exit: the same existing program has identical values/effects under synchronous
entry and successive bounded drive calls; cancellation/drop releases its roots.
Relevant existing reentry coverage passes. No script async syntax is exposed yet.

### AX02 - Checked native Future and source-free wait/resume

Depends on AX01. Prove one typed registered native IO operation end to end using
a deterministic fake service and verified, encoded executable input.

- Add cold native-Future creation and explicit await/resume contracts through
  semantic declarations, contract/MIR/bytecode, codecs, verifiers, runtime and VM.
  Validate result types, initialized live slots, resume destinations, roots and
  non-suspendable resources. Reject unimplemented producer kinds until supported.
- Extend the shared native registration path with owned input capture, operation
  reservation before submission, immediate/deferred completion, typed conversion
  on the driver and bounded once-only provider cancellation. No RPC-specific VM case.
- Add the owned SDK start/drive/completion path and its real readiness handshake.
  Exercise completion during start, duplicate/late messages and cancel while waiting.
  Root retention, external cleanup and stale-slot checks are mandatory now, not AX05.
- Ensure synchronous execute/reentry rejects attempts to drive suspendable bodies;
  synchronous creation of a cold Future is distinct from executing that body.
  Native selection must fall back before entering unsupported async execution.

Exit: a source-free program creates a cold native Future, waits without spinning,
accepts an owned result, resumes once and returns the checked value. Immediate and
deferred paths agree. Cancellation retires the endpoint and releases its execution.
A malformed async artifact is rejected without invoking host work.

### AX03 - Source async functions, closures and explicit await

Depends on AX02. Connect source producers to the verified execution path instead
of building a separate source-only coroutine implementation.

- Implement `async fn`, `async |args| body` and postfix `.await` through lexer,
  parser/AST/HIR, declaration projections, typing and tooling. Preserve recovery,
  generic substitutions, Fn selection, `return`/`?` boundaries and source origins.
- Lower the synchronous Future factory separately from its lazy async body. Handle
  arguments/captures once, independent invocation locals, shared writable captures,
  nested calls and Future-valued outputs without implicit flattening.
- Enforce async-body await restrictions, cold capture safety and suspension liveness
  including temporaries/implicit guards. Encode proofs that independent validation
  can check; runtime checks still protect actual mutable/host resource state.
- Complete Future single-drive/alias behavior, cold values retained past creator
  return and GC tracing of captures. Calling a no-await async body remains lazy.
  Update generated native declaration views and incremental signature/body reuse.

Exit: source and encoded artifacts execute a two-RPC async function and an async
closure through the host-owned root API. Early business Err skips the second RPC.
Ordinary closures with await reject; explicit closures work through generic Fn;
repeated Future await traps without repeated IO. Scope spawn is added in AX04.

### AX04 - Scope spawn, Task results and multiple waiters

Depends on AX03. Deliver the motivating synchronous-handler/Actor workflow.

- Register `spawn` through the ordinary provider flow using `Fn() -> Future<T>`
  and `Result<Task<T>, SpawnError>`. Complete generic declaration/binding support
  for this contract without unrelated generic-registration redesign.
- Reserve capacity and durable scheduling responsibility before admission succeeds.
  Do not invoke either ordinary or async closure factories during admission.
  First drive invokes once; factory failure becomes that Task's terminal outcome.
- Implement scope/task identities, Task await, bounded waiter/dependency storage,
  cached output/terminal metadata and cycle rejection. No parent-child task tree.
- Implement directional cancellation and failure propagation with origin identity:
  waiter cancellation detaches without cancelling its target; target cancellation
  terminates dependents; business Err stays a value. Scope close reaches all tasks.
- Keep completed results rooted independently of execution frames and release
  completed bookkeeping when no handles/report roots need it. Drop is not cancel.
  Emit bounded terminal reports even when nobody awaits a Task.

Exit: a synchronous handler spawns and returns before its factory runs; another
handler executes while RPC waits; a ready notice alone never executes script.
Multiple waiters observe one result, failed admission leaves no job, and cancellation
and wait-cycle rejection preserve unrelated tasks and completed effects.

### AX05 - Lifecycle, reload and diagnostic integration

Depends on AX04. Close cross-boundary integration gaps, not safety deferred from
earlier phases. Reuse the deterministic fixtures and extend only distinct cases.

- Exercise cancellation before start, during wait, after readiness and before
  conversion/publication, including closure, Future, Task and host-owner drop.
  Distinguish cancellation acceptance from completed driver cleanup; cover failed
  dispatch, scope close/restart and runtime shutdown without a remote reply.
- Verify GC retention/release for cold Futures, multiple waiters, cached shared
  outputs and error reports. Check foreign/stale handles and operation generations.
  A native invariant fault quarantines the runtime and retires all its executions.
- Preserve cold callable/Future target pins and waiting execution types/providers
  through compatible reload. New root resolution sees new publication. Enforce
  candidate async/spawn/deferred-output restrictions and existing quiescence hooks;
  do not implement the separate state-replacement proposal.
- Complete logical async origins, spawn/await provenance and Task/scope/cause
  reporting without mandatory source text. Debugger driving must not confuse
  independent parked roots with nested synchronous reentry or deadlock the driver.

Exit: adversarial completion/cleanup and reload scenarios have one terminal outcome,
no retained execution frames/leases, correct retained outputs and usable unrelated
executions where failure policy permits. Focused diagnostics/reload checks pass.

### AX06 - Embedding example, products and CI acceptance

Depends on AX05. Deliver documented embedding use and finish integration.

- Add a small external host/feature consumer and a runnable example showing
  synchronous handler, spawn, two native requests, bounded driving, cancellation
  and terminal reporting. Use deterministic fake IO, not live RPC or timed sleeps.
  A second provider uses the same registration/driver path; a non-Actor dispatcher
  must also work. This does not require a production Actor or Tokio dependency.
- Refresh affected source-free fixtures once at this coherent checkpoint; preserve
  meaningful no-source coverage. Update grammar/syntax audit, current specifications,
  architecture and example documentation to implemented behavior.
- Verify pre-entry interpreter fallback for suspendable bodies and keep existing
  synchronous actual-native coverage. Do not claim JIT suspension or performance
  improvements. No benchmark program is required absent a performance claim.
- Wire new contract owners into the existing GitHub CI feature/backend workflow.
  Record full CI status separately; local success is not full integration acceptance.

Exit: focused example/artifact/fallback evidence passes with no carried local error;
all planned implementation is present. Full acceptance remains pending until the
required GitHub CI run passes, with the run/commit recorded in the roadmap.

## Focused validation and contract owners

Reuse existing session/reentry, native boundary, closure, generic callable, artifact,
GC and reload fixtures. New coverage is justified for async execution as a distinct
contract, not one regression test per edited function. Use a small shared fake
provider/dispatcher fixture with manual completion ordering; test loops are finite.

The following are selected phase checks, not cumulative commands to run after every
edit. Proposed `async_*` test names/targets do not exist at this planning checkpoint;
create them at their owning phase, or map to an existing contract owner and record
the exact replacement filter. A command selecting zero tests is not a passing gate.

| Phase | Suggested focused local commands / evidence |
| --- | --- |
| AX00 | Local link/anchor/content checks and `git diff --check`; contract transition-table review. |
| AX01 | `cargo test -p kagari-vm --lib async_owned_drive_contract`; `cargo test -p kagari-vm --lib host_reentry_cannot_swallow_root_termination_and_releases_borrows`. |
| AX02 | `cargo test -p kagari-vm --lib async_native_completion_contract`; `cargo test -p kagari-bytecode --lib async_artifact_validation_contract`. |
| AX03 | `cargo test -p kagari-hir --lib async_callable_contract`; `cargo test -p kagari-embed --test async_execution source_future_contract`. |
| AX04 | `cargo test -p kagari-embed --test async_execution scoped_task_contract` (admission, sharing, directional cancellation and cycles). |
| AX05 | `cargo test -p kagari-embed --test async_execution lifecycle_reload_contract`; select one existing affected synchronous boundary if implementation changed it. |
| AX06 | `cargo test -p kagari-embed --no-default-features --test artifact_features source_free_async_execution_contract`; run the new deterministic example and one selected async pre-entry-fallback test. |

For AX01-AX06 also perform manual ownership/import/visibility review, then
`uv run --locked scripts/check_structure.py`, `cargo fmt --all -- --check` and
`git diff --check` at implementation checkpoints. Use affected-crate `cargo check`
or targeted Clippy when needed; retain default profiles, parallelism and target.
Do not repeat successful unchanged tests or run entire package suites as substitutes
for selecting contracts. No local `cargo test --workspace`, split full suite or
complete feature/backend matrix is permitted.

[GitHub CI](../.github/workflows/ci.yml) owns full workspace tests, strict
workspace/all-target Clippy, standalone SDK features, CLI JIT and complete
source/native/backend matrices. New async cases must execute under the appropriate
feature lanes; source-disabled coverage must not silently compile out. Full source,
portable-artifact, malformed-input, synchronous-regression and backend acceptance
in the parent design remains required in CI. No remote push or CI success is implied
by creating this plan.

## Progress, commits and resumption

The roadmap is the sole mutable checklist/ledger; do not duplicate it here or in
the companion design. At activation, start at the first unfinished AX phase and
read its ledger plus commit history. Record changed contracts, actual commands,
selected test counts, failures and follow-up owner; keep temporary logs in ignored
`target/`. Planning validation establishes no runtime correctness baseline.

Commit each coherent authorized implementation checkpoint with a Conventional
Commit and `Async-Phase: AX00` through `Async-Phase: AX06`. Mark breaking internal
API/format changes with `!` and explain impact without a routine version bump.
The planning commit carries no implementation trailer. Do not amend user commits.

Implementation adjustments within these boundaries are autonomous. Record their
reason in the owning document/ledger. A change to agreed observable behavior or an
excluded feature is a scope change, not an excuse to silently grow the plan. Report
phase implementation, local acceptance and GitHub CI acceptance separately.
