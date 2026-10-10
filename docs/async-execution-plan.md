# Async execution acceptance record (AX00-AX06)

Status: implementation and local integration repairs complete at `f521b2f0`;
complete GitHub CI acceptance remains unrun. The
[roadmap](implementation-roadmap.md#async-execution-ax00-ax06-ci-pending) owns open
acceptance and future activation. [Async execution](async-execution-design.md) and
[host task scopes](host-task-scope-design.md) retain the detailed language/lifecycle
contracts. This file summarizes completed work, not phases to execute again.

Historical phase contracts and validation narratives are available with
`git show 9dfeba3c:docs/async-execution-plan.md` and
`git show f521b2f0:docs/implementation-roadmap.md`.

## Implemented behavior and scope

The implemented source-to-artifact-to-host path lets synchronous handlers spawn
scope-owned tasks, await typed host IO without blocking the driver, and resume
only when the host drives them. A second message can run while the first task
waits. Cancellation, GC, failure provenance and code generations remain valid
through every wait and terminal path.

The accepted contracts are:

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

## Completed phases and retained boundaries

| Phase | Implemented result |
| --- | --- |
| AX00 | Semantic/executable, state, admission, driver and verification contracts. |
| AX01 | Owned execution, safe slicing, independent roots and synchronous reentry. |
| AX02 | Checked native completion, portable wait/resume and source-free execution. |
| AX03 | Source async functions/closures, explicit await and ordinary for-body await. |
| AX04 | Host scopes, spawn admission, shared Task outputs and multiple waiters. |
| AX05 | Cancellation, GC/reload retention, detached provenance and debugger isolation. |
| AX06 | Host example, source-free products and local integration repairs. |

Instruction slices preserve observable boundaries. Await in ordinary for traversal
retains owned iteration leases across waits: structural writes stay excluded,
nonstructural replacement stays visible, and traps/cancellation release the lease.
No snapshot conversion substitutes for this behavior. Native input borrows end
before suspension; late/duplicate/foreign completion rejects through checked state.
Candidate graphs reject prohibited async resources. Resumption retains original
code, provider contracts, types and execution dependencies across reload.

## Validation and open acceptance

Existing contracts cover owned/sliced drive, synchronous reentry, native completion,
explicit source async, for-await iteration, task admission/waiters/cycles,
directional cancellation, GC cleanup, pinned reload and source-free artifacts.
The deterministic host example is
[async_tasks](../crates/kagari-embed/examples/async_tasks/main.rs). Native preparation
must decline unsupported suspension before entry rather than execute a hidden fallback.

The AX final workspace sweep ran all default targets/doc tests, found nine stale
fixture failures, and resolved them with focused reruns. Workspace Clippy followed
by affected Clippy, structure, formatting and local links/diffs passed. That phase
did not claim a single all-green workspace rerun. The later
[VE08 integration](performance-baseline.md#compact-value-and-interpreter-final-local-evaluation-ve08-2026-10-10)
passed the full workspace command. No carried local failures remain.

- [ ] Complete GitHub CI feature/backend acceptance, including standalone SDK
  feature consumers, source-disabled async contracts and actual backend lanes.

These local records do not establish CI success. The
[CI workflow](../.github/workflows/ci.yml) and [repository policy](../AGENTS.md)
own the current checks; historical per-phase command lists are not a new test queue.
Do not reopen agreed language choices or activate excluded async/package/facade
features as part of closing the remaining acceptance item.
