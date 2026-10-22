# Kagari Documentation

Start with [project goals](project_goal.md), [architecture](architecture.md) and
[the pending roadmap](implementation-roadmap.md). Specifications describe current
behavior; queued designs do not imply implemented APIs.

The [language guide](../site/README.md) provides 25 guide/tool pages, including a
local browser Playground, and
local preview/build instructions. It is separate from the normative specifications
below; the public Pages site has not been activated by this work.

## Language and execution specifications

| Area | Documents |
| --- | --- |
| Source and types | [Syntax](spec/syntax.md), [traits](spec/traits.md), [builtins](spec/builtins.md), [value semantics](spec/value-semantics.md) |
| Numeric library | [Scalar method inventory](spec/builtins.md#numeric-scalar-methods), [integer policies](spec/value-semantics.md#explicit-integer-arithmetic), [float edge cases and precision](spec/value-semantics.md#floating-point-utilities-and-math) |
| Collections and native modules | [Collection interfaces/storage](spec/collection-access.md), [native declarations](spec/standard-declarations.md) |
| Failure and execution | [Failure semantics](spec/failure-semantics.md), [execution](spec/execution.md), [runtime](spec/runtime.md), [security/access](spec/security.md), [error reporting](spec/error-reporting.md) |
| Host integration | [Embedding and Rust DTO/Serde conversion](spec/embedding-api.md), [unified registration, Opaque and scoped Ref/RefMut](spec/host-interop.md), [typed path mutation](spec/typed-path-mutation.md) |
| Modules and reload | [Packages/workspaces](spec/packages.md), [Modules](spec/modules.md), [loading](spec/module-loading.md), [activation](spec/module-activation.md) |
| Executable products | [Bytecode](spec/bytecode.md), [artifacts](spec/artifacts.md), [backend contract](spec/codegen-backend.md), [JIT](spec/jit.md), [AOT deployment](spec/aot.md) |
| Tooling | [Formatter, checker, linter and KBC disassembler](spec/toolchain-tools.md), [Debugger and adapter](spec/debugger.md), [reflection](spec/reflection.md), [language conformance](spec/language-conformance.md) |

## Implementation references

- [HIR reading guide](architecture/hir.md), [syntax architecture](architecture/syntax.md),
  [grammar](kagari.ebnf) and [syntax coverage audit](syntax-coverage.md).
- [Runtime ownership and host object API](runtime-ownership-and-host-api-design.md),
  [async execution contracts](async-execution-design.md) and
  [host task scopes](host-task-scope-design.md).
- [Runnable examples](../examples/README.md), [structure checks](structure-checks.md)
  and [engineering rules](../AGENTS.md).
- [Native module authoring](native-registration.md): complete fluent definitions,
  forward references, sealed declarations and one-shot binding preparation.
- [Generic interface construction](spec/traits.md#current-interface-conversion-boundary)
  and its [proof/execution owners](architecture.md#interface-dispatch).
- [Test preparation and execution](testing.md).
- [Editor tooling and installation](../editors/README.md): standalone Zed/VS Code
  syntax adapters and the separate [stdio analyzer foundation](architecture.md#language-server-foundation).
- [Performance measurements](performance-baseline.md), including the completed
  [runtime storage evaluation](performance-baseline.md#runtime-storage-2026-10-16) and
  [object overhead evaluation](performance-baseline.md#object-storage-overhead-2026-10-18), and
  [Kagari/Lua benchmark instructions](../benchmarks/lua-comparison/README.md).
- [Repository and architecture review](review.md): unresolved findings and native
  expansion requirements, with historical measurements linked at their owner.

## Active and queued work

The [roadmap](implementation-roadmap.md) owns activation and dependencies.

- [Async Runtime CI acceptance](implementation-roadmap.md#async-blocks-structured-concurrency-and-runtime-api):
  async blocks, structured pair composition and the typed Runtime facade are
  implemented with focused local acceptance. Current contracts are in the
  [embedding guide](spec/embedding-api.md#typed-async-calls-scheduling-and-native-waits),
  [async execution](async-execution-design.md) and [task scopes](host-task-scope-design.md).
  Complete workspace/feature/backend/platform acceptance remains pending CI.

- [Compilation latency follow-up](implementation-roadmap.md#fast-compilation-and-eager-startup):
  the source-preparation implementation is closed with the 500 ms compile/load
  goal explicitly unmet. Further frontend preparation work is deferred; the
  [measurements](performance-baseline.md#direct-source-preparation-2026-10-21)
  retain the evidence and limits. Full incremental compilation stays separate.
- [Bounded analysis and MIR compaction acceptance](implementation-roadmap.md#bounded-analysis-and-mir-compaction):
  R16-R18 implementation and selected local acceptance are complete. Full CI remains
  pending; [measurements](performance-baseline.md#bounded-analysis-and-mir-compaction-2026-10-21)
  retain scale improvements, tiny-input setup regressions and current SDK timings.
  This does not close the separate CS/BO/IP latency gaps.
- [Runtime storage, parking and version identity](runtime-lifecycle-plan.md): planned
  RL01-RL03 address R19 scalar coloring, R14 transition-owned safety proofs and R13
  payload-independent version identity. Remaining R13 domain/type retention needs
  bounded measurements and explicit decisions, not automatic migrations.
  Documentation only; implementation has not started.
- [Interpreter performance](interpreter-performance-plan.md): active IP08-IP12
  cover attribution, internal calls, instruction/operand decisions and bounded
  collection transactions. IP09/IP10 changes passed scoped local gates; IP11
    candidates were rejected by their performance gates. IP12 execution/setup/size
      gates passed; lifecycle collection timing acceptance remains outstanding.
  IP01-IP07 completed local
  acceptance; preserve their call/window foundations. The roadmap owns activation,
  pending CI and the unmet pure-interpreter Lua-parity objective.
- [Bytecode optimization](bytecode-optimization-plan.md): independently prioritized
  BO01-BO04 cover unified typed slots, direct destinations, predicate branches and
  constant operands. Implementation and phase commits are authorized; BO01 baseline
  and BO02/BO03 implementation are complete; BO04 remains blocked by four lifecycle
  timing gates after its bounded diagnosis/repair. Its acceptance does not
  require the older IP12 lifecycle gate to pass first.
- [Cranelift synchronous execution](cranelift-expansion-plan.md): J01-J06 and
  the call-stack decision are deferred until the interpreter is stable; expand
  native ABI, control flow, calls, GC values and
  host/library integration while keeping explicit native coverage requirements.
- [Numeric library CI acceptance](implementation-roadmap.md#numeric-standard-library):
  integer policies and bounded f32/f64 utilities/math are installed; the lasting
  contracts are indexed above. Full workspace and feature/backend/platform acceptance
  remains pending CI. Associated constants, native-call const evaluation, a public
  RNG and full ray tracing are outside this delivery.
- [Library-owned native collections](native-collections-plan.md): NC01-NC06 are
  not started; reuse the implemented Iterable APIs while migrating storage.
- [Interface type tests and downcast](interface-downcast-plan.md): DC01-DC04 are
  not started; recover concrete values through checked identity and lifetime
  contracts without introducing Any.

- [Documentation website follow-ups](implementation-roadmap.md#language-documentation-website):
  CI/live Pages acceptance, upstream dependency patching and separately scoped
  translations. Build and content maintenance belong in the [site guide](../site/README.md).
- [Browser WASM follow-ups](implementation-roadmap.md#browser-wasm-execution):
  complete CI/platform and live Pages acceptance. Compilation/preparation latency
  remains with the compilation latency follow-up above; release product recipes
  are implemented. Permanent engine reuse and delivery-size optimization remain
  separately scoped. Current source/source-free adapter and Worker contracts belong in
  the [browser guide](../crates/kagari-web/README.md); measured Playground costs and
  exact-product builds belong in the [site guide](../site/README.md).
- [Host API unification](host-api-refactor.md).
- [Editor follow-ups](implementation-roadmap.md#toolchain-distribution-and-modes):
  complete CI/platform acceptance, semantic IDE features, server-launch integration,
  marketplace publication and separately authorized old-repository administration.
  The syntax-adapter implementation and selected local acceptance are complete.
- [KBC disassembly CI follow-up](implementation-roadmap.md#kbc-disassembly): the
  [offline command and selection contract](spec/toolchain-tools.md#kbc-disassembly)
  is implemented; full workspace and feature/backend/platform acceptance remains
  pending CI.
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
