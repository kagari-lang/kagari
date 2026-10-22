# Persistent module static execution plan

## Status, authority and scope

**Implementation authorized; PS01-PS02 locally complete, PS03 next.** The user accepted the behavior
and authorized sequential implementation with one commit per phase on 2026-10-22.
Baseline: `b744f131`, with only this plan and its roadmap entry uncommitted.
This document owns PS01-PS05, semantic decisions, acceptance and progress. The
[roadmap](implementation-roadmap.md#persistent-module-static) owns activation.
Use buildable checkpoints in order and `Plan-Phase: PSxx` commit trailers.
Push, deployment and unrelated changes remain unauthorized.

User-confirmed requirements:

- Module-level persistent state survives compatible hot reload without resetting.
- Existing calls, closures and waiting tasks keep their pinned code/dependencies.
- Generic associated statics and per-type-instantiation state are not required.
  `const` remains a separate compile-time facility; this plan adds no associated
  constant feature or new const-evaluation rules.

The decisions below are the accepted execution contract, not claims that later
phase behavior is already implemented. PS01 preserves the agreed scope. Current
[modules](spec/modules.md), [activation](spec/module-activation.md), [value semantics](spec/value-semantics.md),
[failure semantics](spec/failure-semantics.md), [architecture](architecture.md) and
[repository rules](../AGENTS.md) remain authoritative until their implementation
owners are updated.

Deliver module-level `static val` and `static var`, typed runtime initialization,
per-installation state identity, compatible publication, source-free verification,
interpreter execution, GC ownership and normal source tooling. Persistence means
within the live runtime/installation, not recovery after process restart.

Excluded: function-local/associated/generic/thread-local statics, shared mutable
heaps across runtimes, automatic schema migration, rename mappings, resetting live
state, automatic callback rebinding, async initializers, disk snapshots, state
replacement/cutover, new reflection APIs, general additive reload, wider native
backend coverage, a new GC family and unrelated review/performance work.

## Baseline and architectural evidence

This inventory is source/test inspection, not a new behavioral or performance
result. Current source rejects static declarations. Low-level module slots exist,
but they are not a source-language static implementation.

| Owner | Existing boundary | Required change |
| --- | --- | --- |
| [Syntax items](../crates/kagari-syntax/src/parser/grammar/item.rs), [AST items](../crates/kagari-syntax/src/ast/item.rs) | Module declarations admit const; static is currently an identifier/rejected item. | Dedicated declaration syntax with recovery, explicit type and initializer; no reuse of const semantics. |
| [HIR lowering](../crates/kagari-hir/src/lower/item.rs), [declarations](../crates/kagari-hir/src/hir/item/storage.rs), [resolution](../crates/kagari-hir/src/resolver/collect.rs), [body queries](../crates/kagari-hir/src/analysis/body_queries.rs) | Const bodies and function bodies have explicit owners; selected body analysis includes const prerequisites. | A checked initializer body owner, static Value-space resolution, read/write facts, imports and incremental dependencies. |
| [Source lowering](../crates/kagari-compiler/src/source/lower/mod.rs), [public ABI](../crates/kagari-compiler/src/source/lower/abi.rs) | Source lowering emits no module slots. | Emit checked static descriptors, initializer bodies and explicit access targets; retain otherwise-unused initializer dependencies. |
| [MIR slots](../crates/kagari-mir/src/function.rs), [bytecode slots](../crates/kagari-bytecode/src/module.rs), [bytecode lowering](../crates/kagari-compiler/src/bytecode.rs) | Module slots carry name, physical ValueType and mutability; LoadModule/StoreModule address the current module. | Complete semantic contracts, verified cross-module operands and initializer ownership through codecs/adoption. |
| [State access](../crates/kagari-runtime/src/module/state.rs), [records](../crates/kagari-runtime/src/module/records.rs) | Slots belong to each ModuleInstance; reads/writes check representation and runtime validity. | Runtime-owned persistent cells and per-generation bindings, with complete type admission and explicit initialization states. |
| [Reload](../crates/kagari-runtime/src/reload.rs), [exports](../crates/kagari-runtime/src/reload/exports.rs), [loading](../crates/kagari-runtime/src/loading.rs) | Staging is isolated; public ABI lists/module sets are compared strictly. | Transactional cell-binding publication and a narrowly scoped static compatibility projection, without weakening other reload checks. |
| [VM dispatch](../crates/kagari-vm/src/executor/dispatch.rs), [frame admission](../crates/kagari-runtime/src/frame/admission.rs), [returns](../crates/kagari-runtime/src/frame/returns.rs) | Module access is a checked storage operation. | First access becomes an ordinary rooted execution transition with initializer return/unwind ownership. |
| [Program collection](../crates/kagari-runtime/src/module/collection.rs), [GC collection](../crates/kagari-runtime/src/gc/collection.rs) | Object and executable reachability are already traced together. | Trace state cells and initializer recipes in the same graph; do not create permanent roots for every historical version. |

The existing [cross-generation layout test](../crates/kagari-runtime/src/module/tests/struct_layouts.rs)
`objects_retain_old_layouts_and_require_equal_schemas_across_generations` demonstrates
a compatible layout accessing an old object and an incompatible layout being
rejected. [TypeView](../crates/kagari-runtime/src/frame/types/compatibility.rs) compares
scoped type/layout graphs. Reuse these owners; they do not by themselves establish
persistent collections, interfaces or native payload acceptance.

## Proposed language and runtime contract

### Declaration and ordinary value behavior

```text
static var requests: i64 = 0;
static val cache: HashMap<String, String> = HashMap::new();
pub(crate) static var selected: Option<String> = None;
```

- Require one identifier, explicit closed type and initializer. No declaration
  patterns, missing initializer, implicit Default or unresolved generic parameter.
- Admit declarations only at module scope, including inline modules. Use existing
  visibility, Value namespace, duplicate-name and import/re-export rules. Aliases
  identify the original declaration; they do not allocate another state cell.
- `val` forbids rebinding, not mutation through an object or another writable alias.
  `var` permits ordinary checked writes. Static is not a const, deep freeze, atomic
  variable or transactional object. Checked arithmetic and evaluation order remain.
- Scalar reads copy values; object reads preserve identity. Rebinding a static does
  not retarget an object reference already stored in a local or another object.
- A function/closure referring to a static accesses its bound cell when executed;
  this does not implicitly snapshot the current value. An explicitly read local
  captured by a closure follows ordinary local capture/value semantics.
- Import/name/type resolution, loading, publication and non-executing inspection
  never force initialization. Compiled access uses checked linked bindings, not a
  string-key lookup or latest-code lookup on each execution.

### State identity and isolation

The semantic key is `(runtime installation lineage, module logical identity,
static declaration identity)`. The lineage is the existing installed root's reload
family, not a process-global package name, artifact hash or code epoch. Select its
concrete private representation in PS02 from actual installation ownership. Raw
DefinitionId/ModuleSlot numbers from different contexts are not stable keys.

Two runtimes and two independently loaded root installations in one runtime must
not share state, even when they share prepared code or dependency packages. Within
one Program installation, repeated imports of the same declaration share one cell.
Hot reload carries that installation's lineage forward. Physical source paths,
declaration order and instruction indices do not determine state identity.

Separate three owners:

1. Immutable executable descriptors: declaration/type/access/initializer contracts.
2. Runtime installation state: checked cells and their initialization/value state.
3. Generation bindings: verified declaration-to-cell references under that code's
   dependency context; no mutable data in shared PreparedProgram/code descriptors.

For example, v1 increments `requests` to 100, then v2 publishes. A fresh call enters
v2 and sees 100; a retained v1 call still executes v1 and accesses that same cell.
A second installation starts its own cell. Old code does not imply an old snapshot
of mutable state. Business invariant changes, such as seconds becoming milliseconds,
are not automatically proved safe by equal types.

### Synchronous first-access initialization

Use explicit `Uninitialized`, `Initializing`, `Ready` and `Failed` states. Unit is
a valid value, never an initialization sentinel. The recipe is fixed to the
introducing generation when a new cell is successfully published. Reload does not
replace that recipe, even when no access has occurred yet. Reordering declarations
or which version first accesses a cell cannot change its initializer.

- First executing read or write ensures initialization. Forward references between
  declarations are allowed; initialize dependencies on demand, not in source or
  import order. Cyclic imports without initialization remain legal.
- Evaluate initialization at target access: direct `S = rhs()` ensures S before
  evaluating RHS; `S += rhs()` ensures and reads S before RHS. For `S[index()] =
  rhs()`, ensure/read S, then evaluate index and RHS once under ordinary place rules.
  Typed-path commit, bounds and trap ordering must be specified and tested together.
- The initializer is a hidden zero-argument synchronous checked body returning the
  declared type. Execute it through normal frame admission/return and shared root
  cancellation/depth policy, not a nested interpreter or an independently reset
  execution session. No storage borrow survives script/native callbacks or reentry.
- Allocate a transition owner before executing the body. Protect partial values,
  the selected recipe and the return value through all collecting boundaries; commit
  Ready only after full type/owner validation and successful return admission.
- Same-cell access while Initializing, including indirect cycles and synchronous
  host reentry, fails explicitly without waiting or exposing a default value. Mark
  the affected attempt failed so a host callback cannot swallow the cycle error and
  let its enclosing initialization publish success. Independent dependencies already
  initialized successfully remain Ready.
- Initializers may call synchronous script/native functions and allocate ordinary
  values. No await, spawn, cold Future creation or new async scope is allowed while
  an initializer is active, including through indirect/native reentry paths. A
  retained async closure is a value; invoking it to create a Future is different.
  Reuse explicit execution-phase checks, not a generic permission matrix.
- Initializer execution cannot park/yield an owned execution while its transition
  is active. Cooperative cancellation and synchronous debugger pauses still work;
  slice exhaustion continues to the next valid park boundary. Native polling that
  would suspend must fail before starting such work. This is not hard preemption.
- Trap, cancellation, depth/resource failure or contained provider failure after
  the attempt begins leaves Failed, never a stranded Initializing state. Preserve
  sticky session termination and quarantine precedence. Pre-entry admission failure
  with no started attempt leaves Uninitialized. Cleanup must work after termination.
- Later access to Failed produces a static-initialization failure with retained
  cause/origin, without replaying the initializer or pretending an old cancelled
  session is the current one. Bound retained diagnostics; no frame/borrow retention.
  There is no retry/reset API in this scope. Business `Result::Err` returned as the
  declared value is a successful initialization, not a runtime initialization trap.
- Initialization is not a transaction over other state or host effects. Completed
  writes/calls remain completed on failure. Do not claim exactly-once external IO.

After Ready/Failed, release executable recipe ownership unless needed by actual
retained values or owned diagnostic metadata. An unaccessed cell intentionally keeps
its old initializer executable; that retention is a documented cost of fixed recipe
semantics. Publishing code with a different initializer does not repair a Failed
cell. Explicit state replacement/new installation remains a different operation.

No new SDK preinitialization API is required: hosts can explicitly invoke a script
function that accesses chosen statics. A debugger/diagnostic snapshot reports
Uninitialized/Initializing/Ready/Failed without executing code. A ready-only runtime
accessor must not silently force a script initializer outside the execution driver.

### Values, type compatibility and pinned behavior

Support ordinary closed, durably storable values: scalars, strings, tuples, enums,
Option/Result, structs, arrays, Vec/Map/Set, function values, closures, eligible
interfaces and registered runtime-owned native payloads. Keep installed host identity,
scoped borrow restrictions, traced edges and resource revocation authoritative.
No unrestricted Rust reference or ephemeral/path/scoped loan becomes durable merely
because it is nested in an aggregate or interface.

Require complete semantic admission, not just ValueType, at initialization and
replacement. Compare scoped nominal identities, recursive layouts, applied generic
arguments, access and native/interface contracts with existing checked owners.
All generations bound to a cell must accept the same storage contract. Checking
only today's value is insufficient: `None` in `Option<T>` does not prove two T
schemas compatible, and an old frame may write a different value tomorrow.

Mutations through aliases continue through existing field/container/native checks;
do not add a competing static-only object semantics or a cached whole-graph safety
flag. Preserve any concrete provider/comparator/witness generation retained by
collections and interfaces; publishing new code does not rebuild map hashes or
retarget their operations. PS05 must validate these boundaries, not infer them from
one old-struct test.

Cold Future and Task handles may be assigned during ordinary execution when the
existing durable-storage/scope contract permits it, for example into an initialized
Option. This plan does not extend scopes, revive completed/cancelled tasks, permit
reconsuming a Future or start async work during initialization. Store/await after
reload retains original body/provider dependencies. Cover this explicitly in PS05;
do not call async persistence complete if only scalar tests pass.

A stored closure/interface keeps its original implementation. Explicit replacement
with a new compatible value changes subsequent reads only. Already-read aliases and
currently executing calls remain valid. Function signatures alone do not authorize
rewriting captures, dispatch tables or native layouts.

### Reload, candidate isolation and deletion policy

Publication preserves cell identity and current state; it does not copy old slot
values into new per-epoch slots. Stage a complete binding plan and uninitialized
new cells without executing initializers. At commit, recheck the expected base and
installation authority, then publish code bindings and new cells atomically with
no callbacks/fallible user work in the switch. A failed/abandoned candidate must
leave existing cells, initialization state and active bindings unchanged.

Candidate execution rejects static access, including ready-value reads and new
candidate-only statics, in this first version. Reusing a live object through a
readonly view or shallow snapshot would not isolate its writable aliases. Enforce
this before obtaining values or forcing initializers, including host/indirect paths.
Candidate functions that do not access static state retain existing permissions.
This is stricter than ordinary published initialization and is intentionally separate.

| Update | First-version rule |
| --- | --- |
| Function body or static declaration order | Allowed subject to ordinary reload checks; preserve cells. |
| Existing initializer expression | Allowed if the declared contract stays equal; existing Ready/Failed/Uninitialized cells keep their value/failure/recipe. A fresh installation uses the new expression. |
| Add a static in an existing module | Allow a new identity/cell after complete checking; initialization remains lazy. |
| Existing static type, mutability, visibility or relevant storage schema | Require compatibility; first version conservatively requires unchanged contracts. |
| Delete, rename or move an existing static | Reject compatible reload; no implicit reset or name guessing. |
| Unrelated module/function/type/trait changes | Preserve existing checks; this feature does not activate general additive reload. |

This deliberately narrows the earlier exploratory deletion suggestion. The
[update proposal](update-model-design.md#compatible-change-rules) already excludes
deleting/renaming existing definitions. First implement persistent state without
also introducing declaration retirement/reintroduction identities. Existing ordinary
writes can release an old value; installation retirement/runtime destruction releases
the state domain. Later removal/rename policy needs its own explicit scope.

Public static ABI records contain identity, declared type and access, not initializer
code, source locations or live values. Exact artifacts still contain/hash initializer
code. Private statics participate in storage compatibility too. Extend public export
and ABI comparison narrowly to admit static additions while preserving every prior
public binding and all non-static rules. Do not globally relax fingerprint equality,
module-count checks or source-free validation to make the positive case pass.

Code rollback is another checked publication, not state rollback. Rolling back to a
program missing a newly added static is rejected under the same no-deletion rule.
A rollback build retaining those declarations can preserve state, subject to normal
contract checks. Do not silently discard recently created state to permit rollback.

### GC, retirement and bounded access

Integrate state into the existing joint graph:

```text
active installation / retained executable / active initializer
    -> bound state cell
    -> Ready value and its object/program edges
    -> Uninitialized/Initializing recipe's pinned program
```

An active installation retains its declared cells. Old code retained by a callable,
frame or pending task retains the cells it can access, without making all historical
versions permanent roots. Immutable type metadata alone must not acquire new mutable
state/execution ownership. A state-to-closure-to-program cycle with no external root
must be collectible when the installation is retired; clearing a callback must allow
otherwise unreachable executable ownership to expire. Reuse existing module retirement
and runtime teardown, not a new forced-unload/reset API.

Keep cell identities checked for owner/generation, candidate abandonment, stale
bindings and reuse. Trace only actual graph edges and validate before detachment;
release native resources outside table borrows with existing panic containment.
Read/access preparation must not scan all historical generations or all cells.
Prove this with structural/work-count evidence before making any timing claim.

## Executable and tooling integration requirements

- Replace the obsolete per-epoch raw slot model coherently where it represents
  module state. Do not retain two source-visible state systems or compatibility
  entrypoints. Existing physical operands may be extended or replaced at their owner.
- Serialize declaration identity, complete scoped type/access, supplying module and
  initializer contract; initializers are private verified entries, not host-callable
  exports or forgeable public initialization authority. Ordinary stores cannot write
  val; only the runtime-owned initializer return transition publishes its first value.
- Cross-module access operands must identify verified targets and imports. Relocate
  and adopt all identity-bearing records through existing context/wire mechanisms.
  Source-free verification rejects wrong types, scopes, targets, signatures and
  immutable writes independently of source checking. Hash matches are not proofs.
- A first static access can call, allocate, trap, mutate and reenter. Update MIR,
  bytecode, prepared execution and optimizer effects coherently. Do not treat lazy
  access as the old pure module read, hoist it onto untaken paths, delete unused reads
  or reuse a Ready observation across an invalidating execution boundary.
- Preserve source origins for the access and initializer, distinct required binding
  states, rooted returns and checked writes after RHS effects. The initializer is an
  ordinary debuggable frame with a meaningful declaration origin, not fake source.
- Native backends must reject unsupported static access before entry and use the
  existing interpreter fallback policy. No mid-execution fallback, silent native
  semantic stub or new general native coverage is part of this task. Preserve AOT
  validation and source-free rejection of forged native-input correspondence.
- Update grammar, syntax audits, formatter, navigation/references, completion, lint
  body participation, editor highlighting, disassembly and runnable guide examples.
  Static initializer references are real semantic dependencies, even if not forced
  in a particular run. Compile/check all declarations under existing package rules.

## Mature references and deliberate differences

- [Rust 1.90.0 LazyLock](https://doc.rust-lang.org/1.90.0/std/sync/struct.LazyLock.html):
  separate cell construction from first-access evaluation and keep an initialization
  owner. Kagari uses a single-driver traced runtime, not Rust static lifetime,
  process-global ownership, blocking thread synchronization or unrestricted references.
  Runtime teardown must release state; Rust static destruction behavior is not copied.
- [Java SE 25 JLS 12.4.1-12.4.2](https://docs.oracle.com/javase/specs/jls/se25/html/jls-12.html#jls-12.4.2):
  explicit uninitialized/in-progress/initialized/erroneous states, separation of
  preparation and execution, and a persistent failed-initialization outcome. Kagari
  initializes individual declarations, rejects recursive observation rather than
  exposing default values, and neither locks threads nor initializes entire classes
  in declaration order. Cancellation/quarantine must use Kagari's existing failures.
- [Erlang/ERTS persistent_term](https://www.erlang.org/doc/apps/erts/persistent_term.html)
  (ERTS 17.1.1 documentation consulted in the design discussion): a separately owned
  long-lived keyed store illustrates separating state from code. Its global,
  read-mostly store, table-copy updates and global-GC replacement behavior are not a
  fit for frequently mutated per-runtime Kagari objects and are not proposed here.

These are mechanisms and contrasting ownership models, not a compatible-hot-reload
implementation to copy or evidence of speed. Kagari-specific generation binding,
typed cells and joint GC edges are necessary to preserve its established contracts.

## Execution phases

### PS01 — Freeze semantics and introduce checked source declarations

- Confirm the contracts above, especially initializer timing/poisoning, candidate
  rejection, async-value boundaries and no-deletion rollback behavior. Record any
  authorized adjustment here rather than creating a second design queue.
- Add syntax/AST/HIR ownership and explicit initializer bodies; resolve static names,
  visibility, types, reads/writes and initializer dependencies through ordinary queries.
- Cover malformed declarations, modules/import aliases, immutable rebinding, closed
  types and initializer diagnostics. Update the static-rejection lexer/parser fixtures
  without weakening rejection of local/associated forms. Preserve contextual/name
  behavior deliberately when selecting the keyword treatment.
- Source compilation beyond the new checked declarations may temporarily report one
  explicit unsupported-static-lowering diagnostic until PS03. Do not emit fake consts,
  Unit slots or incomplete executables. Remove this temporary gate in PS03.

Acceptance: complete source checking and recoverable tooling facts; existing sources
remain buildable. This phase alone does not claim executable static support.

### PS02 — Checked executable records and runtime state ownership

- Migrate MIR/bytecode/ABI/wire records, cross-module targets, verifier rules, identity
  adoption and public/private static contracts together. Update existing raw module
  slot fixtures and consumers; do not leave obsolete public storage APIs in parallel.
- Implement installation lineage, checked persistent cells, generation bindings,
  staged allocations, full semantic value admission and joint GC edges. Ready-only
  runtime inspection exposes state explicitly and cannot execute user code.
- Implement compatible binding/publication with static-only additions, fixed old
  recipes, no-deletion checks and candidate-access rejection. Failed staging must
  leave active cells unchanged; trace recipe ownership before initializer execution.
- Reuse storage/GC/reload fixtures to validate internal state transitions. Until PS03,
  existing backend admission must reject execution needing unimplemented initializer
  transitions before entry. Keep this temporary capability gate local and remove it
  when interpreter support lands; never fall back after side effects.

Acceptance: coherent source-free metadata, owner/type/access validation, state sharing
and rollback/GC ownership. No execution success is claimed through temporary gates.

### PS03 — Lazy execution and complete source-to-interpreter path

- Add a runtime-owned initializer continuation/admission/return/unwind transition;
  use existing driver/frame/session machinery and update all effect consumers.
- Connect checked source lowering and compiled target resolution. Remove both
  temporary gates from PS01/PS02; native-only unsupported cases retain their genuine
  pre-entry capability restriction, not a fake implementation.
- Implement first read/write ordering, cycle detection, reentry, cancellation/depth
  cleanup, durable Failed state, fixed recipes, source origins and no-suspension rules.
- Demonstrate source -> runtime and serialized source-free execution: counter survives
  reload, untouched static does not initialize, added static initializes independently,
  old/new calls share storage while their ordinary function calls stay version-pinned.

Acceptance: an end-to-end usable feature, including negative/error paths. A source
or artifact containing valid statics must not encounter an unimplemented runtime stub.

### PS04 — Cross-generation values, lifecycle and backend boundaries

- Validate the supported value matrix: object identity/aliases, collections with
  recursive nominal arguments, closure/interface/provider pinning, durable native
  handles, and retained cold Future/Task values assigned outside initialization.
- Cover three generations, old writes after new publication, untouched old recipes,
  changed initializer on a fresh installation, Failed state across reload, separate
  roots/runtimes and multiple imports. Include type-equal-value-empty negative cases.
- Exercise candidate indirect access, competing/stale publication, runtime transfer,
  forced GC during initialization/reentry, last external owner release, program-state
  cycles, task cancellation and destructor failure. Never reset state to make cleanup
  tests pass or retain whole runtime state through an immutable descriptor.
- Verify actual interpreter selection for unsupported native statics before entry,
  unaffected scalar native execution and portable native-input validation. Do not
  broaden JIT/AOT coverage or claim all backend/platform combinations passed locally.

Acceptance: no scalar-only shortcut behind the general declaration surface; all
promised storage families have focused evidence and explicitly retained restrictions.

### PS05 — Tooling, documentation and integration handoff

- Finish formatter/navigation/completion/reference/lint/editor/disassembly surfaces;
  use the existing syntax staging/host workflow without substituting unverified tools.
- Add concise runnable examples of persistent counter, mutable static val object,
  retained callback and new versus reused initializer. Separate const from static.
- Update modules/activation, syntax/EBNF, runtime/GC, failure, artifacts/bytecode,
  debugger, architecture and user guides to their lasting ownership. Reconcile the
  update proposal's explicit state-root boundary: static persistence is not automatic
  snapshot inclusion or implementation of state replacement.
- Run final local checks and selected missing boundary cases only. Record actual
  results, failures, work-count limits and unrun CI scope; no performance claim from
  passing tests. Remove the completed plan after durable contracts and unresolved
  acceptance move to their existing owners. Git history owns the completed ledger.

Acceptance: implementation, source-free artifacts, examples and tooling describe one
contract; all carried implementation errors resolved, local checks passing and CI
acceptance explicitly separate. No unrequested commit/push/publication.

## Verification and checkpoint policy

Use a small selected contract set per phase, not a workspace sweep. Extend existing
semantic owners; add cases for distinct new boundaries, not one smoke test per API.
Run multiple selected targets with `--no-fail-fast` where applicable. Exact filters
are chosen from the implementation/test inventory before each phase and recorded
below; this plan does not invent future test names or claim baseline test results.

| Boundary | Existing test owner to extend | Distinct required evidence |
| --- | --- | --- |
| Grammar/resolution | [parser items](../crates/kagari-syntax/src/tests/parser/items.rs), [source imports](../crates/kagari/src/engine/tests/source_modules/imports.rs) | Static forms, negative local/associated forms, aliases/visibility, cyclic imports without execution. |
| Checked storage | [module state](../crates/kagari-runtime/src/module/state/tests.rs) | Full types/access/owner, failure atomicity, no Unit sentinel, candidate restrictions. |
| Execution/reload | [VM frames](../crates/kagari-vm/src/tests/execution/frames.rs), [layout compatibility](../crates/kagari-runtime/src/module/tests/struct_layouts.rs) | Shared cells versus pinned code, assignment order, semantic cross-generation checks. |
| Reentry/cleanup | [native functions](../crates/kagari-vm/src/tests/native_boundary/native_boundary_functions/mod.rs), [program cycles](../crates/kagari-vm/src/tests/native_boundary/native_boundary_gc/program_cycles.rs) | Cycle/failure poisoning, GC during initialization, cancellation/depth and code-state reclamation. |
| Retained execution | [generic reload](../crates/kagari/src/runtime/tests/embedding/generic_reload.rs), [async lifecycle](../crates/kagari/src/runtime/tests/embedding/async_execution/lifecycle.rs) | Old closures/factories/waiters preserve code while compatible shared state changes. |
| Source-free contracts | [package exports](../crates/kagari-runtime/src/module/tests/package_exports.rs), [portable features](../crates/kagari/tests/artifact_features.rs) | Direct/decoded parity, forged semantic types/imports/initializer entries, public/private reload checks. |

At implementation checkpoints, inspect changed Rust imports, visibility, re-exports,
module responsibilities, effective LOC and borrowing across callbacks manually. Run:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Use default workspace profiles, target and parallelism. Batch affected artifact
fixture regeneration at coherent wire checkpoints; keep format/ABI identifiers but
no unpublished-version bump or legacy reader without an explicit requirement.
Avoid new dependencies and keep generic state policy out of execution-loop kernels.

Full-workspace tests and complete feature/backend/platform matrices belong to CI,
including exhaustive source-free/native/browser/editor variants. No local package
sequence may recreate that sweep. Previously passing checks remain evidence for
unaffected code. If an intermediate boundary fails, record the command, diagnostic,
cause and owning phase; do not repeat unchanged failures or mark skipped tests passed.
Documentation-only preparation runs content/link/diff checks, not workspace builds.

Dependencies: PS02/PS03 touch owners also named by [RL02/RL03](runtime-lifecycle-plan.md).
Neither plan requires the other to implement its semantics; do not execute overlapping
frame/parking/module-ownership migrations concurrently. Rebase the source inventory
and reuse whichever owner model lands first. Do not absorb R13/R14 repairs or the
broader [update proposal](update-model-design.md) into this feature without authorization.

## Progress ledger

| Phase | Scope status | Validation / carried errors |
| --- | --- | --- |
| PS01 | Locally complete | 36 selected tests passed; structure, formatting, workspace all-target Clippy and diff checks passed. |
| PS02 | Locally complete | Typed descriptors, persistent cells, reload/GC and source-free negative cases pass; required local checks pass. |
| PS03 | Locally complete | Lazy source/decoded execution, poisoning, reentry and assignment ordering pass; required local checks pass. |
| PS04 | Not started | No implementation checks run. |
| PS05 | Not started | No implementation checks run. |

Planning baseline: clean worktree at `b744f131`; inspected current owners and tests,
existing update/runtime-lifecycle plans and the references above. This request edits
only this plan and its roadmap entry. Independent read-only review found no
high-impact contradiction. Planning validation checked 182 local links/anchors
with no errors, CRLF in both documents, trailing whitespace and `git diff --check`.
Those planning checks are not implementation acceptance; phase evidence follows.

### PS01 checkpoint

Implemented dedicated syntax/AST/HIR static declarations, initializer body ownership,
closed storage signatures, imported visibility/writeability, body queries, navigation
and capture behavior. Runtime initializer expressions use ordinary contextual
coercion, not const evaluation. Full source-program compilation rejects every static
in its checked closure with `KG_SYNTAX_UNSUPPORTED` until PS03; check-only analysis
works. Single-module MIR remains a non-closed product and does not need a duplicate
HIR closure inventory solely for that temporary gate.

Selected validation: 10 syntax/formatter cases, 10 static HIR cases, 10 body-query
cases, 2 local-function regressions, 2 compiler gates, 1 lint and 1 SDK check/emission
boundary (36 distinct tests). Two review findings were reproduced red and repaired:
scoped Ref/RefMut storage was incorrectly accepted, and unbraced interface initializers
missed ordinary coercion. Callable signatures accepting views remain permitted.
Initial dependency integration and a filter-map Clippy warning are resolved.
Workspace all-target Clippy, formatting, structure (1455 files, zero violations)
and diff checks passed. IDE build requests timed out; Cargo supplies compilation
and test evidence. Cargo lock-contention timeouts were completed with exact case
runs/current built test binaries; no skipped selected case is counted as passed.
No full-workspace tests, CI matrix or performance acceptance is claimed. No carried
implementation error remains; executable static behavior is intentionally gated.

### PS02 checkpoint

Replaced raw per-epoch module slots with StaticContract/StaticDescriptor records,
complete scoped types, supplier-local initializer identities and checked generation
bindings to installation-owned cells. Read/write APIs are Ready-only and cannot
initialize val/var; ordinary execution remains conservatively rejected before entry
until PS03. Staging reuses compatible cells without altering their state, preserves
introducing recipes, rejects deletion/type/access changes and admits static-only
public additions. Candidate access remains rejected. Joint tracing covers Ready
values and pending recipes without permanent historical roots.

Selected checks passed: runtime state tests (18 after review repairs), session
execution cases (11), isolation/snapshot cases, VM program-cycle cases (4), pre-entry
static rejection, native disposal and stored-key retention, MIR descriptor codec,
bytecode descriptor presentation, compiler visibility/initializer forgery (9) and
one source-free provider-binding regression. Earlier broader module selection had
70 passes and one stale pointer-identity fixture expectation; the corrected shared
preparation/runtime-local adoption assertion passed its targeted rerun.

Independent review exposed public inherent/trait initializer aliases and comparison
against unrelated ordered generic specialization inventories. Reproductions failed
before repair. Initializer identity/export checks now reject those aliases and a
separate diagnostic-name forgery; storage comparison follows relevant applied type
schemas, allowing unrelated specializations/reordering while rejecting changed
recursive/private/interface contracts. A missing SDK fixture enum arm is fixed.
An MSVC link failure cleared after package-local runtime products were cleaned;
stale incremental output was suspected, not proven. IDE compilation timed out;
Cargo tests/compilation supply evidence.

Workspace all-target Clippy, formatting, structure (1463 files, zero violations)
and diff checks pass. No selected failure remains. The final full CI/backend matrix
is unrun; PS03 initializer execution and PS04 full retained-value coverage remain
separate acceptance, not implied by internal cell fixtures.

### PS03 checkpoint

Connected checked initializer bodies and imported descriptors to ordinary compiler
lowering and the interpreter frame driver. Removed both temporary gates. Initializer
frames enter the retained recipe module directly, protect the return through frame
retirement and semantic admission, and retry only the accessing instruction. Shared
attempt completion poisons cycles immediately, including swallowed native reentry;
unwind/drop settles started attempts. Shared session phase checks deny async work
and parking during initialization. Native unsupported instructions still reject
before entry and select the interpreter without replaying side effects.

Selected validation passed: four compiler lowering cases, one SDK source preparation
case, five runtime initializer cases, six storage contract cases, twelve VM static
cases, one Ready-only frame case, one nested-session regression and one Cranelift
pre-entry rejection case. Source and decoded cases cover reload sharing/fixed recipes,
imports, lazy access, side-effect ordering, business Result errors, managed return
GC, cancellation, observer failure, cycle poisoning, future rejection and slicing.
Independent review reproduced premature intermediate projection capture (writing a
displaced array and trapping before RHS effects). Static roots now remain captured,
while intermediate locations resolve at ordinary commit time. Focused source/decoded
regressions cover root rebinding, intermediate replacement and bounds-trap ordering.

Required workspace all-target Clippy, formatting, structure (1469 files, zero
violations) and diff checks pass. The missing MIR slot constructor and doc-comment
spacing are repaired. Package-local stale linker output required cleaning runtime
and SDK products; targeted reruns passed. IDE build requests timed out; Cargo
supplies compilation evidence. No carried selected failure remains. Full CI and
PS04 retained-value/lifecycle acceptance are not implied by these local results.
