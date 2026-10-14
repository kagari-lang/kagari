# Kagari Implementation Roadmap

This is the queue for unfinished work and open acceptance. Detailed execution
plans own their finite implementation contracts; current behavior belongs in
[architecture](architecture.md) and [specifications](README.md#language-and-execution-specifications).
Completed phases and command logs belong in Git history, not this queue.

## Current baseline

The checked source-to-MIR/bytecode pipeline, source-free loading, explicit native
registration, nominal enums/propagation, runtime-owned roots, pinned reload,
typed host access and host-driven async execution are implemented. Builtin `[T]`
arrays have construction-time fixed length; Vec is distinct. Iterable defaults
provide lazy adapters, terminal operations and explicit FromIterator collection.
Collection cursors check structural revisions on reads; retaining or parking a
cursor does not block source edits.
The interpreter uses compact values and prepared execution/call/layout contracts.
Semantic processing uses scoped DefinitionId records from registration and HIR
through specialization, portable codecs and runtime adoption. Exact paths and
file-local references stay at their explicit boundaries; verification, retained
snapshots and generation-pinned execution remain separate contracts. See
[definition identity ownership](architecture.md#definition-identity-ownership).
These capabilities are foundations to reuse, not migrations to replay.

## Package acceptance and follow-ups

[Source packages and workspaces](spec/packages.md) define the implemented local and
immutable-host source model, one-root/multiple-export Programs, whole-workspace
locking and host-coordinated rebuild/reload. [Architecture](architecture.md#source-package-inputs)
owns the acquisition/analysis/executable boundaries. The runnable
[workspace and embedding examples](../examples/package-workspace/README.md) show
current entrypoints; package `kagari-cli` provides the `kg` executable.

The complete workspace/backend/feature/platform CI matrix remains unrun locally.
Focused Windows coverage includes junction escape/alias checks; Unix symlink
coverage and the exhaustive feature matrix remain CI responsibilities. Source-free
production dependency guards live in `scripts/check_features.py`. These are CI
acceptance items, not authorization to run a local full-workspace sweep.

CI run 38552068511 stopped during Clippy in a benchmark entry consumer, before
workspace tests or feature checks ran. The corrected consumers follow the
[verified benchmark entry contract](../benchmarks/lua-comparison/README.md#matching-and-timing).
The skipped workspace tests and complete feature/backend/platform matrix remain
pending a CI run with that correction.

Remote registry/Git acquisition and publication, simultaneous revisions of one
family within a Program, and explicit dependency-update CLI spelling remain queued.
Independent package binaries and multiple executable targets are outside the
current source-distribution model. Future persistent compilation caches must prove
compiler revision/options and provider/source validity before skipping work; current
shared output storage always checks and compiles inputs.

## Cranelift AOT deployment

[Scalar AOT deployment](spec/aot.md) is implemented for x86_64 Windows MSVC.
JIT and object emission share lowering and invocation context. Build hosts emit
an immutable bytecode/DLL/manifest bundle; compiler-free hosts load and install it
through existing runtime version and lifetime checks. Wider native coverage and
the call-stack decision remain deferred independently.

Focused local deployment, ABI, JIT preparation, VM and CLI checks passed, including
an independent loader process with no source/MIR/compiler/Cranelift dependencies.
The Windows AOT CI job and exhaustive workspace/feature/platform matrix remain
pending CI execution; focused local checks are not complete CI acceptance.

Linux AOT, cross-compilation, LLVM and wider native language coverage remain queued.
Before registry publication of an AOT-enabled Rust SDK, replace its conservative
workspace-source build fingerprint inputs with an explicit packaged build contract;
deployed host binaries already need no source checkout. Publication and that
packaging change are not activated by scalar deployment support.

## Cranelift synchronous execution

[The Cranelift expansion plan](cranelift-expansion-plan.md) owns J01-J06:
general native ABI, scalar/control-flow coverage, verified calls, rooted objects,
closures/interfaces and installed host/library operations. The execution plan is
recorded; implementation and the call-stack decision are deferred until the
interpreter is stable. Current behavior remains the narrow
scalar baseline in [JIT](spec/jit.md).

Native acceptance requires actual compiled script bodies and separately identified
runtime helpers/mixed callees, with source-free artifacts, GC/reentry, cancellation
and pinned reload evidence. Async suspension, native debugging, automatic hotness,
LLVM, AOT and machine-code persistence are outside J01-J06; the separate
[AOT contract](spec/aot.md) describes the implemented scalar deployment route. NC and HR/SA21 remain
independent migrations; reuse their current contracts instead of activating them.

## Toolchain distribution and modes

The current workspace product names are:

| Role | Package / command |
| --- | --- |
| Embedding SDK | Rust package `kagari`, imported as `kagari` |
| Installable toolchain | Rust package `kagari-cli`, executable `kg` |
| Internal components | Existing responsibility-based `kagari-*` crates |

The SDK package and directory are `kagari`, imported as `kagari`; the CLI package
remains `kagari-cli` and provides only the `kg` binary. Consumers, feature checks,
examples and current documentation use these names. No forwarding SDK package or
compatibility command is retained. The [Host API plan](host-api-refactor.md) owns
API evolution independently; naming changes do not activate that refactor.

Focused rename validation passed: SDK typed-call and Cranelift preparation cases,
CLI parsing/artifact execution, and the standalone source-free async consumer
(nine selected tests). These cover renamed dependency, feature and binary boundaries.
The `kg` binary also ran source with and without JIT; benchmark compilation,
production dependency guards, CLI-scoped Clippy, structure, formatting and diff
checks passed. External dependency versions are unchanged. Complete workspace,
SDK feature and backend/platform acceptance remains CI work.

Dependency-free `kagari` and `kagari-cli` reservation packages at `0.0.0-reserved`
were published on 2026-10-12 under separate authorization. The reservation SDK has
no API and its `kg` executable rejects script commands. They are not functional
releases. Before functional publication, verify component name ownership, add
registry versions alongside workspace paths and publish the required dependency
closure; omit unrelated implementation tools. Release automation, version policy
and functional publication require their own activation. No package publication
is authorized by the completed source-package implementation.

The target CLI extends current check/build/run/file/artifact workflows with `kg fmt`,
`kg lint` and `kg repl`. These modes remain unimplemented. CLI owns arguments and IO;
formatting uses lossless syntax and needs no semantic resolution/downloads; semantic
linting shares HIR checking inputs. Required language errors cannot be disabled as
optional lint rules. Explicit public-function selection and argument forwarding,
standalone dependency/host declaration policy and file-versus-project discovery
need concrete mode contracts before implementation. Current private `main` handling
remains the separate verified entry capability.

A proposed REPL keeps analysis/runtime state and explicit GC-owned bindings/results:
`val x = 10;` followed by `x + 2` displays `12`. Invalid syntax/types must not commit
input. Traps retain completed side effects; persistence is not transactional rollback.
Multiline input, expression results, declaration lifetime and reset behavior need
acceptance criteria. Rejecting duplicate declarations with `:reset` is the initial
proposal; redefinition/reload policy remains open. Reuse ordinary analysis, execution,
cancellation, roots and pinned generations, without a second interpreter or new
standalone top-level-statement rule. [Debugger tooling](debugger-design.md) owns its
separately proposed debug/DAP modes.

## Library-owned native collections

[NC01-NC06](native-collections-plan.md) are queued and unstarted.
Refresh their API/code inventory against the current scoped identity APIs.
Baseline 9cd90557 includes the completed Iterable APIs, retained selected-call slots
and Vec/HashMap/HashSet FromIterator implementations.

Libraries should own concrete storage and algorithms; runtime should supply checked
dynamic access, accounting, GC, short operation protection, revision checks and pinned calls. NC01 identifies
missing public storage and constructor/host evidence capabilities before migration.
The plan covers Vec/HashMap/HashSet, their base cursors, host adapters and source-free
consumers, with an external collection proof. Builtin Array and the full SA21
host-registration migration remain separate.

## Test execution cost

[The test preparation task](test-preparation-plan.md) has implemented reusable
native preparation, isolated source sessions and three SDK pilot migrations.
TP04 consolidates six more ordinary scalar SDK runners and removes invariant
preparation from ABI, cancellation and documentation loops; 19 focused tests and
the affected structural checks passed.
Final matched timing and retained-memory acceptance are deferred until after the
optimization work; concurrent CPU load does not block focused correctness work.
[A subsequent reduced-load sample](performance-baseline.md#focused-preparation-recheck-2026-10-11)
passed nine selected tests twice and confirmed cheaper runtime creation; it does
not establish whole-suite speedup or retained-memory reduction.
[The testing guide](testing.md) defines the reusable runner and case ownership. The
[test-cost audit](performance-baseline.md#workspace-test-cost-audit-2026-10-11)
is historical evidence at 35de1818; this task measures a fresh focused baseline.
Preserve fresh mutable state, source-free/backend/GC boundaries and assertions.

## Interpreter performance follow-up

The finite compact-value and execution-architecture migrations are complete.
[The latest retained evaluation](performance-baseline.md#execution-architecture-evaluation-hp06-2026-10-10)
reports 2.57-51.11x Lua time across 16 matched workloads; parity remains unmet.
Acceptance requires interpreter/Lua median <=1.0 for every unchanged matched
workload on one machine, with uncertainty analysis near parity. Entry, adapters,
alternate numeric diagnostics and JIT results cannot replace that gate.

The proposed, unactivated next scope is a bounded call-lifecycle review: prepared
invocation contracts, active-session admission, parameter/frame-window setup and
return. Preserve changing receiver/type/version, roots, cancellation, host/reentry
and trap order; measure unchanged workloads plus setup and retained memory. Native
collection optimization and kernel/bytecode design are separate decisions.

Shared generic Add failed MIR lowering with
`MissingBinding("checked callable requirement")` at the measured revision. Its
[reproduction](performance-baseline.md#correctness-and-reproduction) remains an
open compiler follow-up; shared-identity benchmarks do not prove that capability.

## Other queued designs

These are proposals, not active execution plans. Activation and exact sequencing
require a concrete task; implemented foundations are not prerequisites to replay.

| Track | Scope and dependencies |
| --- | --- |
| [Unified host registration](host-registration-plan.md) | SA21 / HR01-HR03 are proposed and unstarted. Unify HostInterface declarations/bindings under NativeModule; ordinary by-value structs use Kagari objects. HR01 bounds legacy live-host migration and records RI conversion/derive dependencies. The queued Ref/RefMut design covers generated paths and disjoint field loans; reference implementation remains outside HR01-HR03. |
| [Rust interoperability](rust-interop-design.md) | DTO derives, optional schema-backed Serde and retained host-owned opaque objects; reuse existing conversion/root contracts. |
| [Host API](host-api-refactor.md) | Preparation/load/reload facade and logical entry policy; reuse typed calls/roots. Freeze affected package/update contracts before dependent phases. |
| [Update model](update-model-design.md) | Compatible publication versus explicit state replacement; depends on the affected identity, value and host facade contracts. |
| [Interactive debugger](debugger-design.md) | Confirmed synchronous waiting at safe debug points, preserving the executing host thread's Rust call chain. Pause control, inspection lifecycle and SDK/DAP integration are queued and unstarted; no whole-execution continuation migration or automatic process-wide freeze. Lifecycle defaults and CLI spellings remain proposals. |

## Open review and documentation work

[The consolidated review](review.md) owns unresolved SA and R findings. SA entries
include inline AST reuse, compact module handles, HIR model cleanup, local
functions, patterns, generic interface construction and host-registration unification. Resolved definition-identity,
namespace/import/default-body/Array work is described by the current architecture,
specifications and reading guides, not retained as review tasks.

[Architecture findings](review.md#architecture-findings)
R1/R2/R5/R7/R8 remain supported by source inspection at `b71e0cdd`: raw-kind
safety, prepared-JIT policy, trait-search cancellation/limits, repeated
specialization and conservative semantic invalidation. Behavioral reproduction
and performance measurement remain task-specific follow-ups. Native expansion
requires checked call/root/effect contracts and actual-native conformance; LLVM
remains a separate choice.

Earlier identity measurements predate removal of semantic path projections.
Re-measure retained-prefix table-index copying and normalized metadata copying
across independent runtimes before optimization; compact IDs alone do not
establish whole-module sharing or universal speedups.

HIR model documentation is complete within its scope. The whole-crate
`cargo rustdoc -p kagari-hir --lib -- --document-private-items -D missing_docs`
audit had unsuppressed gaps in imports/namespace/solver APIs, including
`PerNamespace`, `LookupOutcome` variants and work/limit records. Complete those
docs at their owning documentation checkpoint; model examples do not establish
whole-crate missing-docs acceptance.

## Trait interfaces and comparison

The trait/interface comparison implementation is complete as of 2026-10-13.
Specifications own [supplied conversion contexts](spec/traits.md#accepted-interface-conversion-context),
[whole-trait dynamic eligibility](spec/traits.md#interface-compatibility-rules) and
[static comparison/identity rules](spec/value-semantics.md#accepted-comparison-design).
Architecture records their [proof and execution owners](architecture.md#library-identities-and-representation-boundaries).

Focused local checks cover branch contexts, dynamic admission, conditional and
explicit comparison, rejected interface identity, malformed portable contracts,
callback effects/cleanup, pinned reload and source-free loading. Full workspace
and exhaustive feature/backend acceptance remain unrun locally and belong to CI.
The completed plan and resolved findings are retained in Git history.

[SA22 generic construction witnesses](review.md#sa22-generic-bounds-do-not-supply-interface-construction-witnesses),
source interface impl targets, blanket impls, derive/downcast additions and NC/SA21
migrations remain outside this work; their completion is not implied.

## Review follow-up order

The following is the proposed default order for the unresolved [SA review](review.md)
work, chosen to reduce replacement of newly implemented models. It does not
activate implementation, settle open language/API choices or merge these findings
into one migration. Each activated task retains a finite scope and focused gates.

Ordinary collection-interface ownership is implemented. Current contracts live in
[collection semantics](spec/collection-access.md), [value semantics](spec/value-semantics.md)
and [architecture](architecture.md#library-identities-and-representation-boundaries).
Trait markers and receiver comparison forwarding are removed. Explicit stdlib
content protocols/helpers and pinned interface methods are validated by focused
source, artifact and reload tests.
Exhaustive workspace and feature/backend validation remain CI-owned; local focused
results do not claim those unrun checks. The queued items below are not activated
by completion of the collection-interface work.

The existing bound-to-interface erasure limitation is recorded as
[SA22](review.md#sa22-generic-bounds-do-not-supply-interface-construction-witnesses).
It has no assigned position in this queue and does not expand the collection or
host-registration scope.

| Order | Review scope | Reason for placement |
| --- | --- | --- |
| 1 | [SA21](review.md#sa21-unify-rust-registration-through-nativemodule-with-scoped-host-access): existing host registration unification | Follow [HR01-HR03](host-registration-plan.md): unify declaration/registration ownership and ordinary by-value binding. Inventory existing live-host consumers separately and bound retirement without replacing them with snapshots. The reference design is recorded in the same plan; its implementation remains queued. The full value example identifies its RI dependencies. |
| 2 | [SA15](review.md#sa15-hir-import-paths-should-retain-segments-instead-of-joined-text): structured import paths | Keep written path segments through normalization, traversal and prefix navigation before changing internal graph keys. |
| 3 | [SA9](review.md#sa9-module-paths-are-copied-into-internal-graph-keys-and-references): context-owned module handles | Reuse the existing definition-table owner where suitable; migrate graph owners/dependencies without conflating written paths, resolved modules and HIR-local ModuleId. |
| 4 | [SA1](review.md#sa1-inline-modules-are-parsed-again): inline AST reuse | Build child lowering/source attribution over the chosen module ownership and graph boundaries, preserving physical offsets and retained snapshots. |
| 5 | [SA16](review.md#sa16-struct-patterns-should-require-explicit-omission-with-): explicit struct-pattern omission | Establish the rest marker and field-completeness checking once, for existing patterns and subsequent declarations. |
| 6 | [SA17](review.md#sa17-ordinary-valvar-declarations-should-support-destructuring-patterns): pattern-based declarations | Replace simple bindings with the unified pattern model using SA16 rules; establish local visibility, writeability and checked extraction before local functions consume the binding/capture model. |
| 7 | [SA14](review.md#sa14-function-bodies-cannot-declare-named-local-functions): named local functions | Define scope, forward references, recursion and captures; implement declaration ownership and allocation/storage order before reviewing redundant self IDs. |
| 8 | [SA11](review.md#sa11-hir-declarations-and-adt-members-carry-container-derived-identities): declaration identity review | Review container-supplied identities against the resulting function ownership model. Field removal is conditional on that review, not a predetermined cleanup. |

Important ordering constraints and limits:

- SA16 rules must be established before SA17 struct destructuring is accepted.
  SA14's declaration ownership/allocation design must be settled before SA11
  changes function identity storage; the proposed default completes SA14 first.
- SA15 -> SA9 -> SA1 is a preferred sequence for overlapping import/graph/lowering
  code, not a semantic requirement. These tasks can be implemented independently
  if their owner interfaces remain suitable. Module handles do not replace written
  import segments or physical source identities.
- SA21 consumes ordinary trait declarations and checked interface operations.
  Collection readonly views and Rust host borrow capabilities remain distinct
  contracts; the removed trait marker must not be recreated by host adaptation.
- Frontend pattern/local-function work need not wait for the complete host track.
  Host-read destructuring must preserve the current checked access contract; if
  SA21 runs later, adapt that boundary rather than resolve/extract patterns again.
- [Native collections](native-collections-plan.md) remain a separate NC01-NC06
  task. NC01 reuses the implemented content protocols/helpers and ordinary interface
  contracts while replacing concrete storage. Reconcile the shared
  NativeStorage/access API contract before NC/SA21 changes to it. Do not require
  the full SA21 migration or persistent borrowed returns for NC implementation,
  or silently absorb host registration unification into NC scope.
- SA21 typed Rust `&T`/`&mut T` inputs and generated field projections have a
  [queued design](host-registration-plan.md#queued-ref-and-refmut-design) for scoped
  handles and disjoint mutable fields. Implementation follows registration
  unification and requires scope/adapter lifetime proofs; keep existing borrow/path
  checks during migration. The same plan owns the receiver conversion matrix,
  method/trait selection proposal and a bounded root/typed-call cost follow-up;
  these do not activate reference implementation or add performance gates to
  HR01-HR03. Persistent borrowed returns remain a separate extension.
- [Focused validation policy](../AGENTS.md#verification-and-tooling) applies
  throughout: reuse already implemented preparation. Coordinate any further
  preparation changes with the existing [test preparation task](test-preparation-plan.md),
  not a second implementation.
  Measure its finite changed scope after the relevant owners stabilize; do not
  postpone acceptance until every queued review feature is implemented or promise
  unmeasured speedups.

## Dependency maintenance

The 2026-10-13 refresh selects Cranelift 0.136.2, Rowan 0.17.0, mlua 0.12.2,
SmallVec 1.16.3 and Serde 1.0.229, with their resolved transitive updates.
Cranelift emission supplies the module target configuration at finalization, uses
`MemFlagsData` with the same empty flags, and explicitly sign-extends comparison
immediates to preserve checked i32 bounds. No runtime ABI or artifact version changes.
The benchmark retains `lua54,vendored`; historical performance results retain their
original dependencies and do not establish performance of the refreshed toolchain.

Bincode stays at 1.3.3 by user instruction. Evaluate its replacement separately at
the artifact/MIR/type codec owners, preserving bounded decoding and source-free
validation. The structure checker updates the Rust grammar to 0.24.2 but retains
Python binding 0.25.2: 0.26.0 crashes the full scan on Windows. The
[checker guide](structure-checks.md#dependency-upgrade-constraint) records the
reproduction and the conditions for retrying that upgrade.

Local validation selects backend execution/overflow/lifetime tests, SDK artifact
and native preparation contracts, parser interpolation/recovery/limits, and the
Lua benchmark's empty/single-element cases. These cover the changed dependency
boundaries without running the full workspace or performance matrix. All 36 selected
Rust tests and 43 checker regressions passed; the full structural scan reported
zero findings. Backend-scoped Clippy (`--lib --no-deps -- -D warnings`), workspace
formatting and diff checks also passed. A user-requested workspace/all-targets
Clippy follow-up removed 44 redundant DefinitionId clones in HIR tests and test
helpers, with field shorthand cleanup. `cargo clippy --workspace --all-targets --
-D warnings` now passes; test assertions and production behavior are unchanged.
Exhaustive feature/backend/platform acceptance remains pending CI; no performance
improvement is claimed.

## Verification and open CI acceptance

Completed implementations have recorded local acceptance with no carried local
integration failures. Complete GitHub CI feature/backend acceptance remains
unverified; local validation does not establish that matrix passed.

[AGENTS.md](../AGENTS.md) owns verification policy. Local implementation checks
select affected contracts and a small dependent regression set; full workspace
and feature/backend matrices belong to CI. Documentation-only changes need
content/link/diff checks. Performance reports retain revision, workload, machine,
toolchain, profile/features, parallelism and cache state, separating compilation,
preparation and execution.
