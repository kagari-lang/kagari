# Kagari Documentation

Start with [project goals](project_goal.md), [architecture](architecture.md) and
[the pending roadmap](implementation-roadmap.md). Specifications describe current
behavior; queued designs do not imply implemented APIs.

## Language and execution specifications

| Area | Documents |
| --- | --- |
| Source and types | [Syntax](spec/syntax.md), [traits](spec/traits.md), [builtins](spec/builtins.md), [value semantics](spec/value-semantics.md) |
| Collections and native modules | [Collection interfaces/storage](spec/collection-access.md), [native declarations](spec/standard-declarations.md) |
| Failure and execution | [Failure semantics](spec/failure-semantics.md), [execution](spec/execution.md), [runtime](spec/runtime.md), [security/access](spec/security.md), [error reporting](spec/error-reporting.md) |
| Host integration | [Embedding](spec/embedding-api.md), [host interop](spec/host-interop.md), [typed path mutation](spec/typed-path-mutation.md) |
| Modules and reload | [Packages/workspaces](spec/packages.md), [Modules](spec/modules.md), [loading](spec/module-loading.md), [activation](spec/module-activation.md) |
| Executable products | [Bytecode](spec/bytecode.md), [artifacts](spec/artifacts.md), [backend contract](spec/codegen-backend.md), [JIT](spec/jit.md), [AOT deployment](spec/aot.md) |
| Tooling | [Debugger and adapter](spec/debugger.md), [reflection](spec/reflection.md), [language conformance](spec/language-conformance.md) |

## Implementation references

- [HIR reading guide](architecture/hir.md), [syntax architecture](architecture/syntax.md),
  [grammar](kagari.ebnf) and [syntax coverage audit](syntax-coverage.md).
- [Runtime ownership and host object API](runtime-ownership-and-host-api-design.md),
  [async execution contracts](async-execution-design.md) and
  [host task scopes](host-task-scope-design.md).
- [Runnable examples](../examples/README.md), [structure checks](structure-checks.md)
  and [engineering rules](../AGENTS.md).
- [Test preparation and execution](testing.md).
- [Performance measurements](performance-baseline.md) and
  [Kagari/Lua benchmark instructions](../benchmarks/lua-comparison/README.md).
- [Repository and architecture review](review.md): unresolved findings and native
  expansion requirements, with historical measurements linked at their owner.

## Queued work

The [roadmap](implementation-roadmap.md) owns activation and dependencies.

- [Cranelift synchronous execution](cranelift-expansion-plan.md): J01-J06 and
  the call-stack decision are deferred until the interpreter is stable; expand
  native ABI, control flow, calls, GC values and
  host/library integration while keeping explicit native coverage requirements.
- [Library-owned native collections](native-collections-plan.md): NC01-NC06 are
  not started; reuse the implemented Iterable APIs while migrating storage.
- [Unified host registration and API proposal](host-registration-plan.md):
  SA21 / HR01-HR03 are unstarted; merge the existing host interface into
  NativeModule, with complete by-value and Ref/RefMut API examples. The queued
  reference design covers generated paths and disjoint field borrowing;
  its implementation remains deferred, and DTO derives remain RI work.
- [Rust value/opaque interoperability](rust-interop-design.md).
- [Host API unification](host-api-refactor.md).
- [Toolchain distribution and remaining modes](implementation-roadmap.md#toolchain-distribution-and-modes):
  SDK/CLI naming, formatting, linting and REPL; source packages are specified above.
- [Compatible reload and state replacement](update-model-design.md).
- [Interactive debugger](debugger-design.md): synchronous waiting on the executing
  host thread, followed by SDK control and IDE/DAP integration; not implemented.

## Documentation lifecycle

Keep current contracts, reading guides, actionable open findings and bounded
active/queued plans. When implementation completes, move lasting contracts to
their architecture/specification owner and unresolved work to the roadmap or
review, then remove the execution plan. Pending CI alone does not justify retaining
a completed phase ledger. Keep only measurements needed for current decisions,
with their environment, workload and limits.

Git history owns completed plans, command transcripts and superseded measurements.
Use `git log --all -- docs/<name>` to locate their revisions. Do not create an
in-tree archive or depend on another developer's ignored logs and probe files.
