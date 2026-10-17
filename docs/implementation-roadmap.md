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

GitHub CI acceptance remains pending for the Native/AOT setup and historical
contract-test repairs identified in runs 38568738772 through
[38697567439](https://github.com/kagari-lang/kagari/actions/runs/38697567439).
The current [applied-contract validation boundary](architecture.md#library-identities-and-representation-boundaries)
and [standalone AOT host contract](spec/aot.md) own the lasting behavior. Recheck
the complete CI matrix after publication of the repair commits; focused local
validation does not establish that matrix passed.

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
[AOT contract](spec/aot.md) describes the implemented scalar deployment route. NC remains an independent
migration; reuse implemented host registration contracts without expanding scope.

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

The first-release [formatter, checker and six-rule linter](spec/toolchain-tools.md)
are implemented. CLI owns arguments and IO; formatting uses lossless syntax without
dependency resolution, and check/lint share HIR analysis and strict language checks.
Required diagnostics cannot be disabled through optional-rule policy.

Deferred source-tool work: range/on-type formatting, comment reflow, import sorting,
skip regions, check/lint stdin overlays, watch/daemon, full IDE/LSP features,
per-path configuration, custom rules, autofix and persistent caches. Further lint
candidates include unused
imports, dead declarations, ignored Results, unreachable code, dead stores,
expression simplification and measured performance advice. Each requires its own
semantic justification for exports, trait lookup, traps and side effects; none is
part of the six-rule release.

The stdio analyzer and syntax adapters are implemented; current ownership belongs
in [architecture](architecture.md#language-server-foundation), and the
[editor guide](../editors/README.md) owns installation and selected local validation.
The complete editor-tooling CI job, full syntax-example inventory, clean Linux
installation and broader editor/platform matrix remain pending CI. Windows local
acceptance does not establish those results. Semantic IDE features, editor
server-launch integration and marketplace publication remain deferred.

Old analyzer repository retirement remains a separately authorized administrative
action. Its public tree at `4a05bc564dd56c5e32f68c7f0138ddc83840cec2` and public
issue/PR/release metadata were audited: only skeletons/design intent were present,
with no unique behavior to transfer. No code was copied; the old workspace declared
`MIT OR Apache-2.0` but contained no license texts. Its public wiki lookup returned
`Repository not found`. The owner must confirm preservation of inaccessible
private/deleted/wiki/project material before deletion; no remote mutation has been
performed and no build/test depends on that repository.

The [script testing design](script-testing-design.md) proposes `kg test` with
source-owned private `#[test]` functions, limited `#[cfg(test)]` selection,
verified test entries and per-case runtime isolation. Ordinary tests use the
default CLI environment; hosts orchestrate custom-service fixtures through shared
testing capabilities. Design is authorized; implementation is unstarted and
separate from TT01-TT05. Test-mode check/lint options belong to that later work.

`kg repl` remains a separate unimplemented mode. Explicit public-function selection
and argument forwarding, standalone dependency/host declaration policy and
file-versus-project discovery for the remaining execution modes
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

## Runtime storage redesign

RS01-RS06 are locally complete as of 2026-10-16. Receiver-independent interface
descriptions are shared, compact directory entries locate typed stores, and script
struct fields occupy a packed arena with direct scalar-bank transfers. General
Value transport, scalar frame banks, numeric buffers and full checked identities
remain in place. The implementation uses safe Rust; owner elision is deferred
because the measured node workload offers only a projected 0.49% benefit.

[Architecture](architecture.md#runtime-model) and the [runtime specification](spec/runtime.md)
own current contracts. [Matched measurements](performance-baseline.md#runtime-storage-2026-10-16)
record all frozen local memory/latency gates passing, including uncertainty near
GC thresholds. Focused tests and required local checks pass. The exhaustive
workspace and feature/backend/platform matrix remains pending CI; local results do
not establish that acceptance. Git history owns the completed execution ledger.

This completion does not activate downcast, NC migration or the proposed call-lifecycle
review. Universal 8-byte Value, new GC algorithms, state migration and native-backend
expansion were outside scope; interpreter/Lua throughput goals remain independent.

## Object storage overhead

The [object storage overhead execution plan](object-storage-overhead-plan.md)
records OM01-OM05; implementation is unstarted. It follows the completed local
RS storage work. The required next change attributes total storage and moves native
mutation revisions out of the common identity directory. Layout-grouped structs
and interface-record consolidation are conditional on additional measured savings,
sparse-layout capacity, unchanged semantics and non-compounding latency gates.
Value and full owner/slot/generation handles remain unchanged.

This is a recorded follow-up, not activation of implementation or a new dependency
that blocks DC/NC. Before activating another storage-dependent track, refresh its
inventory against whichever OM checkpoints have actually been accepted. OM owns
its decisions and ledger; downcast, collection-library migration and call-lifecycle
optimization remain separate scopes.

## Library-owned native collections

[NC01-NC06](native-collections-plan.md) are queued and unstarted.
Their [runtime storage predecessor](#runtime-storage-redesign) is locally complete;
reuse the resulting heap/descriptor contracts and refresh NC01's API/code inventory
against the current scoped identity APIs.
Baseline 9cd90557 includes the completed Iterable APIs, retained selected-call slots
and Vec/HashMap/HashSet FromIterator implementations.

Libraries should own concrete storage and algorithms; runtime should supply checked
dynamic access, accounting, GC, short operation protection, revision checks and pinned calls. NC01 identifies
missing public storage and constructor/host evidence capabilities before migration.
The plan covers Vec/HashMap/HashSet, their base cursors, host adapters and source-free
consumers, with an external collection proof. Builtin Array remains separate; reuse the
implemented host registration and scoped borrowing contracts.

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

[TP05-TP09 source ownership](test-preparation-plan.md#in-source-test-organization-tp05-tp09)
is implemented: integration targets fell from 97 to five, with ordinary cases
under source owners and six stdlib/runtime composition suites under stdlib to
preserve Rust crate identity. Focused local checks and external consumers pass;
the complete workspace/feature/platform matrix remains CI-owned.
The [matched pilot and lifetime control](performance-baseline.md#source-owned-test-pilot-2026-10-15)
observed a 124.23 s to 82.36 s median for 43 cases, with process peak rising from
956.7 to 1425.6 MiB under imperfectly isolated load. Repeated same-name sessions
stabilize the shared identity table; distinct names remain retained. Controlled
compile/link costs, broader retained-memory behavior and the earlier preparation
comparison remain in the existing plan. No whole-suite performance acceptance
is claimed.

## Interpreter performance follow-up

The finite compact-value, execution-architecture and IP01-IP07 interpreter work
have completed local acceptance. The [current matched evaluation](performance-baseline.md#interpreter-call-lifecycle-2026-10-17)
uses runtime f070a092 against IP01 60bfaacb and passes all 61 execution/lifecycle
gates. Calls, recursion, interface and shared-generic time fall approximately
23.0%, 22.6%, 23.2% and 24.9%. Selected correctness, allocation/teardown and required
local code-quality checks pass. Complete workspace and feature/backend/platform
validation remain pending CI; local acceptance does not establish that result.

The retained architecture has explicit reusable window identities, one prepared
call/argument/return lifecycle, shared checked operand decoding and live program
lease registrations. The [architecture](architecture.md#runtime-model) owns these
contracts and the [benchmark guide](../benchmarks/lua-comparison/README.md#beforeafter-workflow)
owns the fixed before/after procedure. The completed phase ledger is in Git history.
Setup and Lua times are diagnostics, with no independent calibration or report
extension. Historical failures are not reclassified as passes.

Interpreter/Lua parity remains unmet. The [Windows investigation](performance-baseline.md#interpreter-attribution-windows-2026-10-17)
at `88e87fd4` records 16 matched workloads at 2.48-41.94 times PUC Lua 5.4.9 time;
the preceding macOS evaluation is separate evidence. Acceptance still requires
<=1.0 for every unchanged matched workload on one machine, with uncertainty analysis
near parity. Entry, host adapters, unmatched native rows and JIT cannot replace that
gate. The investigation identifies excess logical transport, heavy warm call
transitions and repeated collection/result work, not measured CPU shares or a
promised speedup.

The active [interpreter performance plan](interpreter-performance-plan.md) owns
IP08-IP12, authorized for autonomous execution and phase commits on 2026-10-17:
freeze/attribute remaining costs, review shared call admission/return, select a
bounded instruction/operand decision, consolidate collection transactions and evaluate
the retained changes together. IP08 froze the ordinary baseline and declared scoped
gates; IP09 consolidated admission/return and IP10 retained a measured cancellation
code-generation correction. Both passed scoped local gates, not CI or Lua parity.
IP11 rejected map/Vec candidates after measured gates failed. IP12 integrated
original/forms execution gates passed, but lifecycle collection timing acceptance
remains outstanding after unrestricted and fixed-affinity runs. The
[follow-up evaluation](performance-baseline.md#interpreter-follow-up-evaluation-windows-2026-10-17)
records retained gains, failed candidates and the exact unresolved gate; Lua parity
remains unmet (2.20-41.07x in the integrated run). Do not add more local optimizations
before diagnosing collection work/retained shape versus measurement uncertainty.
The plan carries the design constraints,
versioned Lua/Rust/JVM references, applicability decisions, phase-specific gates and
the sole follow-up ledger rather than maintaining another decision list here. Keep existing prepared calls, direct
transfers and exact installed primitives; do not add workload recognition, a second
semantic path or unchecked boundaries.

Independent-session segmentation and abandoned-owner discovery remain deferred
outside IP08-IP12. The [earlier attribution](performance-baseline.md#conditional-decisions-and-remaining-attribution)
records non-tail retirement and owner-scan costs; that is not an explanation for
ordinary nested tail-window calls. A future lifecycle decision must preserve prompt
cleanup, suspended/reentrant roots, generation checks and bounded capacity. Existing
localized projection/publication inline hints remain supported by their recorded
assembly evidence; revisit ownership and footprint if those transfers regress,
not by spreading hints to unrelated handlers.

The [OM plan](object-storage-overhead-plan.md) remains the sole owner of object
storage changes and remains queued. IP preserved the accepted RS layout and its
memory savings. NC collection migration, enum unboxing, a new collector and native
backend expansion were outside IP scope.

Shared generic Add failed MIR lowering with
`MissingBinding("checked callable requirement")` at the measured revision. Its
[reproduction](performance-baseline.md#correctness-and-reproduction) remains an
open compiler follow-up; shared-identity benchmarks do not prove that capability.

## Host registration and interop delivery

Unified registration, Rust DTO/Serde conversion, runtime-owned Opaque and scoped
Ref/RefMut are implemented. Common NativeModule declarations, explicit bind and
atomic installation serve stdlib, application and live-host consumers. The former
host aggregate/format and parallel registration/invocation APIs are removed.
The HR01-HR03, RI00-RI05 and RF01-RF06 plans are retired; Git history retains their
phase decisions and validation ledgers.

Current contracts live in [host interop](spec/host-interop.md),
[the embedding guide](spec/embedding-api.md#typed-native-registration),
[runtime ownership](spec/runtime.md), [traits](spec/traits.md),
[artifacts](spec/artifacts.md) and [reload](spec/module-activation.md).
The [scoped SDK example](../crates/kagari/examples/scoped_host.rs) registers one
method set for Opaque, Ref and RefMut and demonstrates read/edit/take with old
alias invalidation. [DTO/Opaque](../crates/kagari/examples/value_interop.rs),
[optional Serde](../crates/kagari/examples/serde_interop.rs) and
[cold source-free registration](../crates/kagari-runtime/examples/offline_host.rs)
provide the other runnable boundary cases. No implicit Rust trait exposure or GC
object reinterpretation supplies host references.

Focused local declaration/proof/codec/installation, VM and SDK contract checks,
production lifetime compile-pass/compile-fail evidence and actual generated-field
Miri passed. The final Miri selection used nightly-2026-07-27 on
aarch64-apple-darwin with MIRIFLAGS unset and default target/profile/parallelism.
Full workspace, feature/backend/platform and broader Miri acceptance remains
CI-owned and unrun locally; focused checks are not complete CI acceptance.
Focused interop review coverage includes cross-generation scoped native calls,
borrow cleanup/ownership, source reflection diagnostics and applied reference
signatures, with ordinary reflection and propagation as adjacent regression cases.
No known selected local integration or safety failure remains.

Remaining scope stays separate: general view trait forwarding, borrowed returns,
reflective access to scoped views, retaining references across suspension,
packed/union/variant/index projections,
unrestricted editing/extraction of managed script-edge payloads and wider native
reference execution are not implemented. Current unsupported native reference
bodies select the interpreter before entry. Automatic derive/Serde supports the
finite subset documented in the embedding guide. [Call preparation costs](review.md#call-boundary-cost-follow-up)
remain a bounded follow-up with no speedup claim; NC and the Host API facade
remain independently queued.

## Other queued designs

These are proposals, not active execution plans. Activation and exact sequencing
require a concrete task; implemented foundations are not prerequisites to replay.

| Track | Scope and dependencies |
| --- | --- |
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
source interface impl targets, blanket impls, derive additions and NC
migration remain outside this work; their completion is not implied. Host
registration/interop is implemented separately as described above.

## Interface type tests and downcast

The [interface downcast execution plan](interface-downcast-plan.md) records the
accepted `is::<T>() -> bool` and `downcast::<T>() -> Option<T>` direction and owns
DC01-DC04. Implementation has not started. Reuse concrete interface metadata,
checked type environments, roots and pinned versions; no Any trait is added.
The [runtime storage predecessor](#runtime-storage-redesign) is locally complete.
Refresh DC01's implementation inventory against its shared descriptions before activation.
Ordinary read-only interfaces may explicitly recover their concrete storage,
while host access restrictions and scoped-reference exclusion remain intact.

DC01 closes intrinsic/member resolution, target eligibility and cross-version
recovery/call provenance before implementation. Later phases own source-independent
contracts/runtime execution, source/tooling integration and focused boundary
acceptance. Arbitrary trait queries, SA22 construction witnesses, NC, broader
reload redesign and native interface expansion remain separate. Recording this
plan does not activate those tracks or imply executable downcast support.

## Standard trait bounds

SA24's implementation is complete within its bounded scope as of 2026-10-17.
[Operation-specific bounds](spec/traits.md#operation-specific-bounds) and
[native declarations](spec/standard-declarations.md#explicit-registration) own the
language/registration contracts; [architecture](architecture.md#library-identities-and-representation-boundaries)
owns nominal bound storage and checked adapter identities. Special standard
constraints and their wire wrappers are removed without compatibility aliases.

Focused local evidence covers ordinary/custom and shared primitive operations,
bound/unbound associated outputs, malformed proofs/callables, canonical portable
records, native declaration installation, recovery, checked numeric traps/effects,
root cleanup, pinned reload and six-operator artifact execution in a build without
the source feature. Representative existing arithmetic tests require actual native
execution, not fallback. Full workspace tests and the complete feature/backend/
platform matrix remain unrun locally and CI-owned.

SA22 construction witnesses, SA23 scalar cleanup, SA25 method authoring, native
collection storage and broader native-backend work remain separate. No performance
improvement or completion of those tracks is implied.

## Default and struct update

The user selected [SA26](review.md#sa26-add-ordinary-default-construction-and-struct-update-expressions)
ahead of the default review queue on 2026-10-17. The
[Default and struct update execution plan](default-construction-plan.md) owns
DF01-DF05, the sole progress ledger, design decisions and focused acceptance.
Implementation and one commit per phase are authorized. DF01 froze the contract
and focused acceptance; DF02 ordinary registration/contextual calls passed focused
local checks. DF03 standard implementations passed focused value/fresh-storage
acceptance; DF04 struct update and editor syntax are next.

Deliver ordinary `core::default::Default` and prelude publication, contextual
static associated-call Self inference, a finite standard implementation inventory,
and general `Struct { field: value, ..base }` construction. The update base is
independent of Default. Keep nominal/generic/all-field visibility checks, once-only
source-order effects, a fresh outer struct with shared member identity, cleanup
and generation-pinned calls. Include source tooling and editor syntax in the
construction checkpoint; no compiler-only syntax acceptance.

Default is not a language protocol or special runtime constructor. Derive, automatic
enum selection, field declaration initializers, SA22, SA25, host/view decomposition,
new native instructions and collection storage remain separate. The pre-existing
SA16/SA17 editor-grammar gap was independently resolved before activation; reuse
that merged support rather than reimplementing it. Focused source/artifact/source-free and lifecycle evidence belongs
to this plan; complete feature/backend/platform/editor inventory remains CI-owned.

## Review follow-up order

The following is the proposed default order for the unresolved [SA review](review.md)
work, chosen to reduce replacement of newly implemented models. It does not
activate implementation, settle open language/API choices or combine unrelated
findings into one migration. Explicitly grouped scopes below share one plan; each
activated task retains a finite scope and focused gates. SA26 is explicitly
selected next under its [own plan](#default-and-struct-update); the table below
retains the default order for the other queued findings.
SA21 registration/interop is implemented as described above. Struct-pattern rest
and ordinary destructuring declarations are implemented; their current contracts
live in [syntax](spec/syntax.md#blocks-and-statements),
[patterns](spec/syntax.md#patterns) and the
[HIR guide](architecture/hir.md#pattern-based-declarations).
Focused local grammar, typing, execution, capture/tooling, lifecycle cleanup and
source-free artifact checks passed. Complete feature/backend/platform acceptance
remains pending CI; the other entries below remain queued.

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
| 1 | [SA15](review.md#sa15-hir-import-paths-should-retain-segments-instead-of-joined-text): structured import paths | Keep written path segments through normalization, traversal and prefix navigation before changing internal graph keys. |
| 2 | [SA9](review.md#sa9-module-paths-are-copied-into-internal-graph-keys-and-references): context-owned module handles | Reuse the existing definition-table owner where suitable; migrate graph owners/dependencies without conflating written paths, resolved modules and HIR-local ModuleId. |
| 3 | [SA1](review.md#sa1-inline-modules-are-parsed-again): inline AST reuse | Build child lowering/source attribution over the chosen module ownership and graph boundaries, preserving physical offsets and retained snapshots. |
| 4 | [SA14](review.md#sa14-function-bodies-cannot-declare-named-local-functions): named local functions | Define scope, forward references, recursion and captures; implement declaration ownership and allocation/storage order before reviewing redundant self IDs. |
| 5 | [SA11](review.md#sa11-hir-declarations-and-adt-members-carry-container-derived-identities): declaration identity review | Review container-supplied identities against the resulting function ownership model. Field removal is conditional on that review, not a predetermined cleanup. |

Important ordering constraints and limits:

- SA14's declaration ownership/allocation design must be settled before SA11
  changes function identity storage; the proposed default completes SA14 first.
- SA15 -> SA9 -> SA1 is a preferred sequence for overlapping import/graph/lowering
  code, not a semantic requirement. These tasks can be implemented independently
  if their owner interfaces remain suitable. Module handles do not replace written
  import segments or physical source identities.
- SA21 consumes ordinary trait declarations and checked interface operations.
  Collection readonly views and Rust host borrow capabilities remain distinct
  contracts; the removed trait marker must not be recreated by host adaptation.
- Frontend pattern/local-function work need not wait for the complete host track.
  Checked host initializers preserve their access contract; any future direct
  host/view destructuring requires an explicit scope decision and checked field
  projections rather than resolving/extracting patterns again in a backend.
- [Native collections](native-collections-plan.md) remain a separate NC01-NC06
  task. NC01 reuses the implemented content protocols/helpers and ordinary interface
  contracts while replacing concrete storage. Reconcile the shared
  NativeStorage/access API contract before NC changes to it. Reuse implemented
  host registration; do not require persistent borrowed returns for NC or expand
  its scope into host facade or call-cost work.
- SA21 typed Rust `&T`/`&mut T` inputs and generated field projections have an
  [execution contract](spec/host-interop.md#borrowed-host-values) for scoped
  handles and disjoint mutable fields. It also owns the passing matrix and explicit
  host method/trait registration. The bounded root/typed-call cost follow-up remains
  queued at [SA21](review.md#call-boundary-cost-follow-up). Persistent borrowed
  returns remain a separate extension.
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
