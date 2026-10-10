# Kagari Documentation

Start with [project goals](project_goal.md), [architecture](architecture.md) and
[the pending roadmap](implementation-roadmap.md). Current behavior and queued
designs are separate. Completed execution logs live in Git history rather than
parallel migration documents.

## Language and execution specifications

| Area | Documents |
| --- | --- |
| Source and types | [Syntax](spec/syntax.md), [traits](spec/traits.md), [builtins](spec/builtins.md), [value semantics](spec/value-semantics.md) |
| Collections and native modules | [Collection interfaces/storage](spec/collection-access.md), [native declarations](spec/standard-declarations.md) |
| Failure and execution | [Failure semantics](spec/failure-semantics.md), [execution](spec/execution.md), [runtime](spec/runtime.md), [security/access](spec/security.md), [error reporting](spec/error-reporting.md) |
| Host integration | [Embedding](spec/embedding-api.md), [host interop](spec/host-interop.md), [typed path mutation](spec/typed-path-mutation.md) |
| Modules and reload | [Modules](spec/modules.md), [loading](spec/module-loading.md), [activation](spec/module-activation.md) |
| Executable products | [Bytecode](spec/bytecode.md), [artifacts](spec/artifacts.md), [backend contract](spec/codegen-backend.md), [JIT](spec/jit.md) |
| Tooling | [Debugger and adapter](spec/debugger.md), [reflection](spec/reflection.md), [language conformance](spec/language-conformance.md) |

## Implementation references

- [Syntax architecture](architecture/syntax.md), [grammar](kagari.ebnf) and
  [syntax coverage audit](syntax-coverage.md); the TSV files are audit data.
- [Structure checks](structure-checks.md) and [engineering rules](../AGENTS.md).
- [Performance measurements](performance-baseline.md),
  [architecture review/open findings](architecture-review-2026-10-03.md) and
  [Kagari/Lua benchmarks](../benchmarks/lua-comparison/README.md).
- [Review findings](review.md): the shared document for ongoing repository reviews.
- [Runnable examples](../examples/README.md).
- [HIR reading guide](architecture/hir.md) and the completed
  [HIR](hir-documentation-plan.md) / [syntax](syntax-documentation-plan.md)
  documentation contracts.
- [Runtime ownership and host object API](runtime-ownership-and-host-api-design.md).
- [Async contracts](async-execution-design.md), [host task scopes](host-task-scope-design.md)
  and the [AX00-AX06 acceptance record](async-execution-plan.md).
- [Compact values/interpreter acceptance (VE00-VE09)](interpreter-value-execution-plan.md).
  Implementation is complete locally; Lua parity and full CI remain open.
- [Interpreter execution architecture and prior-optimization review (HP00-HP06)](interpreter-hotpath-execution-plan.md).
  Migration and final local evaluation are complete; full CI and Lua parity remain open.
- Completed semantic contracts: [nominal enums/propagation](enum-propagation-plan.md),
  [imports and namespaces](import-resolution-plan.md),
  [Type/Value namespaces and import solving](name-resolution-plan.md), and
  [native default bodies](native-default-bodies-plan.md). The roadmap distinguishes
  implementation completion from pending CI; these are not queued migrations.

## Queued designs

These documents do not imply implemented APIs or active execution. The roadmap
owns activation, dependencies and phase order.

- [Rust value/opaque interoperability](rust-interop-design.md).
- [Host API unification](host-api-refactor.md).
- [Packages and dependency resolution](package-design.md).
- [Compatible reload and state replacement](update-model-design.md).

Historical per-phase logs are recoverable with
`git show 9dfeba3c:docs/implementation-roadmap.md` and the corresponding plan/report
path. Current reading guides, specifications and design contracts remain in-tree.
