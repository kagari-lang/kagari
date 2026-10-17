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
| Host integration | [Embedding and Rust DTO/Serde conversion](spec/embedding-api.md), [unified registration, Opaque and scoped Ref/RefMut](spec/host-interop.md), [typed path mutation](spec/typed-path-mutation.md) |
| Modules and reload | [Packages/workspaces](spec/packages.md), [Modules](spec/modules.md), [loading](spec/module-loading.md), [activation](spec/module-activation.md) |
| Executable products | [Bytecode](spec/bytecode.md), [artifacts](spec/artifacts.md), [backend contract](spec/codegen-backend.md), [JIT](spec/jit.md), [AOT deployment](spec/aot.md) |
| Tooling | [Formatter, checker and linter](spec/toolchain-tools.md), [Debugger and adapter](spec/debugger.md), [reflection](spec/reflection.md), [language conformance](spec/language-conformance.md) |

## Implementation references

- [HIR reading guide](architecture/hir.md), [syntax architecture](architecture/syntax.md),
  [grammar](kagari.ebnf) and [syntax coverage audit](syntax-coverage.md).
- [Runtime ownership and host object API](runtime-ownership-and-host-api-design.md),
  [async execution contracts](async-execution-design.md) and
  [host task scopes](host-task-scope-design.md).
- [Runnable examples](../examples/README.md), [structure checks](structure-checks.md)
  and [engineering rules](../AGENTS.md).
- [Test preparation and execution](testing.md).
- [Performance measurements](performance-baseline.md), including the completed
  [runtime storage evaluation](performance-baseline.md#runtime-storage-2026-10-16), and
  [Kagari/Lua benchmark instructions](../benchmarks/lua-comparison/README.md).
- [Repository and architecture review](review.md): unresolved findings and native
  expansion requirements, with historical measurements linked at their owner.

## Active and queued work

The [roadmap](implementation-roadmap.md) owns activation and dependencies.

- [Object storage overhead](object-storage-overhead-plan.md): OM01-OM05 are
  unstarted; attribute total costs, relocate native revisions, then conditionally
  group struct layouts and consolidate interface records under measured gates.
- [Interpreter performance](interpreter-performance-plan.md): active IP08-IP12
  cover attribution, internal calls, instruction/operand decisions and bounded
  collection transactions. IP09/IP10 changes passed scoped local gates; IP11
    candidates were rejected by their performance gates. IP12 execution/setup/size
      gates passed; lifecycle collection timing acceptance remains outstanding.
  IP01-IP07 completed local
  acceptance; preserve their call/window foundations. The roadmap owns activation,
  pending CI and the unmet pure-interpreter Lua-parity objective.
- [Cranelift synchronous execution](cranelift-expansion-plan.md): J01-J06 and
  the call-stack decision are deferred until the interpreter is stable; expand
  native ABI, control flow, calls, GC values and
  host/library integration while keeping explicit native coverage requirements.
- [Library-owned native collections](native-collections-plan.md): NC01-NC06 are
  not started; reuse the implemented Iterable APIs while migrating storage.
- [Interface type tests and downcast](interface-downcast-plan.md): DC01-DC04 are
  not started; recover concrete values through checked identity and lifetime
  contracts without introducing Any.
- [Struct patterns and destructuring declarations](pattern-binding-plan.md):
  active PB01-PB04 combine SA16/SA17 with phase commits. PB02 explicit struct rest
  and completeness passed focused checks; PB03 next unifies val/var patterns,
  checked extraction, captures and tooling. Full CI remains pending.
- [Host API unification](host-api-refactor.md).
- [Editor tooling](editor-tooling-plan.md): ET01-ET05 are unstarted; consolidate
  the LSP foundation into this workspace, then add Zed/VS Code basic highlighting
  independent of a server. Full semantic IDE features remain deferred.
- [Script testing](script-testing-design.md): proposed `kg test`, source-owned
  `#[test]` functions and host-orchestrated fixtures; design only, separate from
  formatter/checker/linter implementation.
- [Toolchain distribution and remaining modes](implementation-roadmap.md#toolchain-distribution-and-modes):
  SDK/CLI distribution and the separate REPL proposal; source packages are
  specified above.
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
