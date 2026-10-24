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

Deferred discussion, recorded 2026-10-20: package-managed development should keep
source-project commands as the user entrypoint and use automatic cross-process
disk compilation caches without requiring a resident compiler service. The build
system would locate workspace cache records and validate compiler/configuration,
source, dependency and host-contract identities before reuse. Newly compiled
in-memory results should execute directly rather than be written and read back.
Development caches are disposable and distinct from explicitly exported bytecode
or platform-specific AOT deployment products; deployment loads the selected product,
not a guessed development cache. Input paths converge on shared runtime contracts,
while bytecode validation and trusted native-code loading retain different checks.
Cache units, invalidation, identity remapping, disk validation, atomic/concurrent
publication and cleanup remain undecided. This is a direction to revisit, not an
implemented capability or an approved cache design. It does not block or expand the
[completed compilation/startup work](#fast-compilation-and-eager-startup): the
direct source path and efficient eager verification do not require a persistent
cache. No cache implementation, daemon or AOT expansion is activated by this note.

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

## KBC disassembly

The delivered [offline `kg disasm` contract](spec/toolchain-tools.md#kbc-disassembly)
provides full instruction rendering, a compact function directory and exact
name/artifact-slot selection. [Bytecode output](spec/bytecode.md#disassembly-output)
owns scoped references, entry/physical/semantic facts, recorded nontransitive
effects and nonauthoritative debug annotations. [Artifact validation](spec/artifacts.md#offline-disassembly-validation)
and [library ownership](architecture.md#offline-bytecode-inspection) preserve bounded,
complete current-format verification without source files, runtime construction or
installed host providers. Paths show retained executable records, not reconstructed
source declarations; optional MIR remains opaque.

Full-workspace tests and the complete feature/backend/platform matrix remain
**pending CI and unrun**; delivered behavior and documentation do not establish
that acceptance. Source decompilation, CFG reconstruction, source-like output,
MIR/native browsing, raw-invalid inspection, JSON/machine-protocol output and
editor/web integration are outside the delivered scope.

## Language documentation website

The independent [Astro/Starlight language guide](../site/README.md) is implemented:
25 substantive English guide/tool pages, shared editor grammar, canonical source includes,
static-output checks and a default-disabled least-privilege Pages workflow.
Specifications remain the semantic authority; site maintenance and publication
setup belong in its README, not a second execution ledger.

Focused local acceptance passed on Windows with Node 22.21.1/npm 10.9.4 and
Chrome 155.0.8059.82: fresh 25-page/404 output, links/anchors/assets/base,
20 source includes, script contracts, and selected desktop/mobile browser coverage
for real search, themes, code/copy, nested routing, keyboard access and Playground.
Selected CLI and SDK examples/host-boundary tests passed. This is not complete
cross-browser/platform/accessibility or full Rust workspace acceptance.

Open acceptance and follow-ups:

- Run the new workflow on GitHub's Linux/Node 24 environment. Local Windows
  results do not establish CI acceptance or Linux/macOS browser discovery.
- Public settings and deployment at the proposed
  `https://kagari-lang.github.io/kagari/` remain unverified and disabled by default.
  Pushes, settings changes and activation require separate authorization. After
  authorization, follow the site README's Pages/environment/opt-in setup and
  check the live project base, nested routes, assets, search and 404 behavior.
- Track an upstream-compatible fix for
  [GHSA-rj75-hqrm-r3gf](https://github.com/advisories/GHSA-rj75-hqrm-r3gf).
  The current lock has 10 moderate package entries for that single selector-parser
  advisory; high-threshold audit passes, moderate-threshold audit does not.
  The narrowly accepted trusted build-time boundary is documented in the site
  README. Do not force incompatible overrides or treat this as a patched parser;
  re-review before dynamic CSS, remote theme/plugin or request-time inputs.
- Browser Playground/WASM ownership and remaining acceptance are described below.
  Translations, full API generation, versioned release sites, custom domains and
  analytics remain separately scoped. Source-build
  instructions remain necessary until a functional toolchain release exists.

## Browser WASM execution

The fixed-width migration and browser-hosting implementation are complete. Current
contracts belong to [architecture](architecture.md#browser-hosting), the
[browser adapter](../crates/kagari-web/README.md) and
[site guide](../site/README.md#playground-ownership-and-acceptance); completed phase
ledgers live in Git history, not a parallel plan.

Script isize/usize were removed, standard lengths/indices/counts use u64, and Rust
storage narrowing stays checked. The browser runs the existing frontend/interpreter
with default source support or an independent frontend-free `.kbc` product, not a
WASM code-generation backend. One lazy-loaded Playground route uses bounded Worker
execution and exact-product Pages builds. Selected source, source-free, Worker and
UI contracts passed in Chrome, including Unicode diagnostics, lossless integers,
ordered effects, cancellation and fresh-instance recovery. Native contract tests
remain evidence for precise root/loan and pinned-generation cleanup; labelled
browser fault wrappers are not proof of Rust panic recovery.

Remaining acceptance and explicitly deferred work:

- Run full workspace/backend/feature/platform acceptance in CI; no local full sweep
  was performed. Source-free graph and selected real execution are established,
  not every target or host-capability combination.
- Run the expanded Pages workflow on Linux/Node 24, including cold Cargo and CLI
  acquisition budgets. The nested Unix timeout/interrupt contract passed in WSL,
  but that is not cold GitHub Actions acceptance.
- Read-only live diagnosis on 2026-10-20 verified successful Run/repeat behavior,
  WASM MIME, gzip and ten-minute cache headers. First successful Runs took 25-36 s
  on the measured route, with transfer failures also observed; warm repeats made
  no requests yet took about 4 s. The served representation changed mid-probe and
  exact live revision/profile was unavailable. This is not stable-release or full
  deployment acceptance, nor authorization for pushes/settings changes.
- [Matched dev/release profiling](performance-baseline.md#browser-compilation-latency-2026-10-20)
  at 98f5c39d establishes that release alone is insufficient: minimal-program fresh
  construction/compile medians were 4.88 s dev and 3.65 s release; retained-session
  release recompilation remained 2.63 s. About 69% of one sampled compile was MIR/
  bytecode verification, with allocation-heavy type limits/trait proofs and three
  bytecode verification boundaries. Keep this separate from steady execution gates.
  Subsequent [compilation/startup work](#fast-compilation-and-eager-startup)
  implements the direct source/preparation path, efficient eager verification and
  release product recipe. Its matched results retain the unmet compile/load target
  separately from this historical diagnosis. Malformed-artifact, type-limit,
  trait-proof, source-free and lifecycle guarantees remain. Permanent engine reuse,
  artifact pruning and delivery-size work remain separate; no unbounded identity
  retention or relaxed validation is allowed.
- Other browsers/platforms, hard heap/compiler quotas, external async wake providers,
  threads/shared memory, WASI, arbitrary JS callbacks, browser live reload, i128/u128
  and a WASM code-generation backend remain outside this delivery. Existing native
  and interpreter optimization work remains separately owned.

## Fast compilation and eager startup

The implementation is locally closed as of 2026-10-21. The user accepted closure
with the 500 ms compile/load target explicitly unmet and declined an additional
optimization phase. The completed execution plan is removed; Git history retains
its phase decisions and validation ledger.

Source runs now use [direct sealed preparation](spec/embedding-api.md#direct-source-preparation),
with explicit artifact export/import and opt-in native-input retention. Shared
bounded type traversal, graph-owned proof resources and exact dependency views
reduce repeated preparation while preserving eager validation, host binding,
cancellation, debug/GC metadata and generation-pinned reload. Immediate CLI,
embedding and browser consumers use the direct route, and browser products use
release builds. [Architecture](architecture.md#compilation-pipeline) owns the
verification and evidence-lifetime boundaries.

[Matched Mac release measurements](performance-baseline.md#direct-source-preparation-2026-10-21)
reduce minimal first compile/load from 2748.8 to 708.9 ms and fresh construction
plus compile from 3554.6 to 1458.2 ms. The 1500 ms median startup target is met;
the 500 ms compile/load target remains unresolved, without changing its threshold.
Selected local correctness, source-free, real-browser/Worker and implementation
checks passed. Full workspace tests and complete feature/backend/platform matrices
remain CI-owned and unrun locally. The products were not pushed or deployed.

Deferred follow-up: source analysis and MIR lowering now dominate the sampled
remaining compile time. Review snapshot-owned signature input environments and
checked lowering facts before selecting another optimization; imported binding
construction precedes signature reuse, and native-default/ABI lowering still uses
HIR aggregate normalization. The performance record retains the sample weights,
workload growth, memory results and limits of that diagnosis. This is not proof of
an R7 specialization-round or R16 flow-scheduling cause. Subsequent bounded
analysis work below resolves R16 without reclassifying this latency goal. Further
frontend work requires a separately authorized scope. Full incremental compilation,
permanent engine reuse, R7/R8 changes, global caches, artifact pruning and AOT
expansion remain separate.

## Bounded analysis and MIR compaction

R16-R18 implementation and selected local acceptance are complete as of 2026-10-21.
[Architecture](architecture.md#bounded-flow-debug-availability-and-mir-deletion)
owns function-local debug availability, independent directional bytecode fixed
points and stable aligned MIR deletion. Required validation, cancellation, budget
exhaustion, debugger/GC facts and fresh seals after edits remain intact. The completed
AE01-AE03 execution plan is removed; Git retains phase decisions and validation.

[Bounded release measurements](performance-baseline.md#bounded-analysis-and-mir-compaction-2026-10-21)
record actual work reduction and native operation timing, including tiny-input setup
regressions. Current direct source and explicit artifact paths both execute the
representative input correctly; those current-only timings are not an end-to-end
before/after claim. Full workspace tests and feature/backend/platform matrices remain
pending CI; browser acceptance was not run. No push/deployment was authorized.
R19 coloring, R7/R8, runtime lifetime work and existing CS/BO/IP latency gaps remain
separate. Do not use these results as evidence that those performance gates passed.

## Runtime storage, parking and version identity

RL01-RL03 implementation and focused local acceptance are complete. The execution
plan is retired; Git history retains its phase ledger. Lasting ownership belongs to
[architecture](architecture.md), [execution transitions](spec/execution.md) and
[runtime version identity](spec/runtime.md). Scalar-only coloring preserves managed
locations and logical/debug liveness. Parking proves the actual retained graph once
per transition, retaining fresh post-poll checks and ready/waiting publication rules.
Private version tokens preserve exact preparation identity without retaining original
code merely for comparison; runtime authority and executable leases remain separate.

[RL01 capacity/work controls](performance-baseline.md#rl01-scalar-storage-class-allocation-2026-10-23),
[RL02 graph work and isolated timing](performance-baseline.md#rl02-transition-owned-parking-2026-10-23)
and [RL03 ownership probes](performance-baseline.md#rl03-exact-version-identity-and-bounded-retention-2026-10-23)
retain methods, results and limits. Focused contracts cover mixed decoded frames/GC,
debug/reused-frame access, pending completion/cancellation, source -> A -> B payload
release, exact-version native rejection and actual pinned native/AOT execution.
Private snapshot tests also verify backing release and the finite COW/capacity matrix.
Structure, formatting, workspace/all-target Clippy, diagnostics-feature Clippy and
diff checks passed. Full-workspace/backend/feature/platform acceptance remains CI-only;
these results do not close BO04/IP12/CS timing gaps or establish an end-to-end speedup.

R19/R14 and R13's version-marker subfinding are resolved. The measured decision is
to retain installation-scoped identity writers for the documented lifetime, not
promise cheap unbounded names/snapshots. Type/layout-only holders still retain retired
code at 1/16/128 generations; [review](review.md) keeps that R13 follow-up for a
separately scoped ownership split. No session/domain or type/code migration is
activated. The Windows AOT check used an existing process-local MSVC linker override;
no persisted toolchain changes, push or deployment were performed.

## Persistent module static

Module-level `static val`/`static var`, lazy initialization, compatible shared state,
source-free verification and source/editor tooling are implemented. Lasting ownership
belongs to [persistent module state](architecture.md#persistent-module-state),
[declarations](spec/modules.md#persistent-module-statics), [activation](spec/module-activation.md)
and [runtime cells](spec/runtime.md#persistent-static-cells). The
[runnable example](../crates/kagari/examples/persistent_static.rs) demonstrates shared
state versus pinned callbacks, untouched old recipes and fresh installations.
Completed phase ledgers are retained in Git history, not a second execution queue.

Focused local source/decoded tests cover full durable-value families, three-generation
publication, candidate isolation, cancellation, collecting reentry, failure poisoning
and retirement. Map/Set lookup now uses construction-pinned protocols; content
comparison remains caller-selected. Syntax/HIR/formatter/lint/disassembly tests,
grammar audits, Tree-sitter CLI corpus/query checks, TextMate checks, the runnable
example and required Rust quality checks passed. Full workspace/backend/feature
and platform validation remains CI-only; no performance acceptance is claimed.

Static syntax's native Node/capture tests remain blocked by pinned node-gyp 11.5.0
not recognizing the only installed Visual Studio 18.10.3 (no VS 2022 installation).
Actual WASM staging is blocked because the available Zed WASI SDK is 25.0 rather
than pinned 34.0; the version guard correctly rejected it before compilation.
Run those checks with supported toolchains, plus actual editor-host and complete
inventory acceptance, before claiming the editor matrix passed. Earlier editor-host
validation predates this syntax addition. No tool version was spoofed or upgraded.

Semantic LSP handlers and static-watch UI remain separately deferred tooling, not
implied by HIR facts or non-forcing runtime snapshots. The existing
[update proposal](update-model-design.md) still owns state replacement/migration;
static persistence does not implement it, disk recovery, callback rebinding, generic
associated statics or general additive reload. The pre-existing equality receiver
mismatch remains in [review](review.md#hash-lookup-equality-receiver-clarification).
RL02/RL03 must reuse the current initializer/frame/state owners rather than replaying
the replaced raw-slot model. Push and deployment remain unauthorized.

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

OM01-OM05 completed local acceptance on 2026-10-18 at runtime `65bbf477`.
Native mutation revisions now belong to runtime-owned native records; the measured
common directory entry falls from 24 to 12 bytes. Interface heap identities locate
the snapshot store directly while preserving independent checked heap/metadata
identities, metadata-only roots and alias lifetimes. Value, full handles and packed
field storage are unchanged. Current contracts live in
[architecture](architecture.md#runtime-model) and [runtime](spec/runtime.md).

The [matched evaluation](performance-baseline.md#object-storage-overhead-2026-10-18)
records Point/Pair live storage down 21.49%/21.20% against OM01, and interface live
storage down a further 19.61-19.88% against OM02. All 29 OM01 timing, 26 inherited RS
and twelve additional forms gates pass, alongside memory/teardown/reuse checks,
28 focused contracts and required local quality checks. The failed first interface
prototype remains documented. These results do not establish Lua parity or the
unrun complete workspace/feature/backend/platform CI matrix; that acceptance stays
pending CI.

Per-layout groups are deferred: the optimistic sparse model adds 9.69% live bytes,
exceeding the +5% budget. Keep one packed arena. Revisit only with a concrete group
model that bounds sparse overhead without a second fallback engine; no grouping
implementation is queued automatically. The measured model does not prove all
future designs impossible.

DC/NC inventories reflect the accepted storage owners without activating either.
Their activation must still recheck current APIs and freeze their own relevant
baseline. Downcast, collection-library migration, collector changes and call-lifecycle
optimization remain separate scopes. Git history owns the completed OM ledger.

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

The [repository-wide test organization plan](test-organization-plan.md) is the
authorized phased follow-up to the committed fixture pilot. TO00-TO11 cover all 24 workspace
members and the existing Node/Python/editor/browser/product families: semantic
ownership, proven duplicate cleanup, suitable parameterization/discovery,
file-based compiler UI diagnostics in TO03/TO04/TO09, and explicit retention of
stateful/source-free/native boundaries. TO00 has frozen the full family catalogue,
UI expectation contract and first selected harness inventory. TO01 conversions and
ordinary traits are complete locally (102 discovered cases preserved; focused
behavioral validation passed); TO02 remaining SDK language families is next. The user authorized implementation and phase commits. This creates no
new test binary/framework layer and does not replace the older measurement debt
below; complete CI acceptance remains separate.

Bounded contract-owned fixture pilots are implemented: formatter input/expected
pairs, parser recovery/diagnostic inputs, ordinary SDK source cases, shared bounded
numeric predicate generation and two stateful source extractions. The
[testing guide](testing.md#existing-file-backed-patterns) owns current examples,
classification and format rules. Cases retain independent names in existing library
harnesses. Named `rstest` cases now replace the custom formatter/source-example
registrars and same-flow parser/default-method wrappers. Homogeneous syntax
examples use `rstest` file discovery with explicit different-contract exclusions;
Cargo tracks directory additions/removals. The framework is dev-only;
no integration target or production dependency was added. Focused local acceptance
covers migrated cases and representative source examples; exhaustive
feature/backend/platform tests remain CI-owned.
There is no measured speed/memory claim or automatic whole-repository conversion.
This does not replace the older preparation/performance acceptance below.

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
remains unmet (2.20-41.07x in the integrated run). Within IP08-IP12, do not add more
local optimizations before diagnosing collection work/retained shape versus
measurement uncertainty. The independently prioritized bytecode migration below
has its own baseline and acceptance and does not require this gate to pass first.
The plan carries the design constraints,
versioned Lua/Rust/JVM references, applicability decisions, phase-specific gates and
the sole IP08-IP12 ledger rather than maintaining another decision list here. Keep existing prepared calls, direct
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

The separate [object storage evaluation](performance-baseline.md#object-storage-overhead-2026-10-18)
records the locally accepted OM changes; IP's measurements retain their original
storage baseline and are not rerun by that handoff. NC collection migration, enum
unboxing, a new collector and native backend expansion were outside IP scope.

Shared generic Add failed MIR lowering with
`MissingBinding("checked callable requirement")` at the measured revision. Its
[reproduction](performance-baseline.md#correctness-and-reproduction) remains an
open compiler follow-up; shared-identity benchmarks do not prove that capability.

## Bytecode optimization

The independent [bytecode optimization plan](bytecode-optimization-plan.md) owns
BO01-BO04: current baseline/design, unified typed slots and direct destinations,
predicate branches/constant operands, and integrated acceptance. On 2026-10-19 the
user explicitly requested separate ownership and priority before the earlier
interpreter work finishes. BO01 can proceed without completing IP12 or obtaining
another disposition of its collection timing gate. The user subsequently authorized
implementation and one commit per phase. BO01 froze the current baseline and
settled the executable-boundary/slot/proof design; BO02 replaced the slot contract
and passed its focused correctness/measurement gates. BO03 added general
predicate branches and prepared scalar-constant operands. BO04 remains blocked:
initial unrestricted timing failed 16/43 gates. One fixed-P-core diagnosis and a
bounded completion-publication ownership repair produced original 7/7 and forms
12/12 passes, but four of 24 lifecycle rows still exceed 1.05. The
[measurement report](performance-baseline.md#bytecode-slot-and-operand-evaluation-windows-2026-10-19)
records arithmetic/branches ratios 0.69181/0.71995, passing setup/size/retained-byte
budgets and the exact residual failures. Required local correctness/code-quality
checks pass; full CI is unrun. Do not mark the plan complete or add another tuning
phase without an explicit scope decision; earlier failed evidence remains visible.

Use a current-revision baseline and separate progress/acceptance ledger. The old
plan retains its outstanding results; BO acceptance neither closes IP12 nor requires
historical baseline reconstruction. Existing lifecycle/collection workloads remain
regression checks against BO01, not tasks to optimize collections. Cancellation
features, specialized loop instructions and numeric-kernel ABI changes remain out
of scope. Required verifier/debug/native/AOT mapping updates are part of the coherent
bytecode replacement. The independent plan can be completed and retired regardless
of whether IP12 remains open. BO phase commits use their own Plan-Phase trailers.

## Module-owned standard-library registration

Module-owned standard-library registration is implemented across core/alloc/std.
[Architecture](architecture.md#unified-library-registration), the
[native authoring guide](native-registration.md) and
[declaration specification](spec/standard-declarations.md) own the current contracts.
The SR01-SR06 execution plan is retired; Git history retains its phase decisions,
normalization policy and validation ledger.

Remaining acceptance is the full workspace test suite and complete
feature/backend/platform CI matrix. Focused local coverage passed authoring/default
proofs, public declarations/docs/navigation, collection and async behavior, source-free
loading, GC/host/reload boundaries and existing native allocation contracts. Final
checks used fresh current build products after invalidating stale shared-target
artifacts; the stale comparison result was withdrawn. Full semantic comparison
retained every declaration field with explicit reviewed identity/metadata mappings
and 15 documentation corrections, not only inventory counts. The frozen baseline
has matching captures and pinned source fingerprints but lacks its historical
producer rlib/source manifest; this is not a hermetic historical rebuild claim.

The existing [call-boundary cost follow-up](review.md#call-boundary-cost-follow-up)
remains separately queued. Registration now uses direct associations and one-shot
preparation; it does not eliminate invocation-time value/ownership/generation checks
or establish general zero-allocation execution or speedups. Collection storage,
host facade redesign and VM/backend expansion remain independent work.

## Numeric standard library

The installed numeric foundation delivers 17 integer-policy variants across eight
types (132 concrete bindings), 15 float-utility names and 11 extended-math names at
each of two widths (30 and 22 bindings). These are ordinary canonical `core::num`
methods using the completed module-owned registration, shared integer kernels and
width-matched float bindings. Ordinary checked operators remain unchanged.

Lasting contracts live in the [numeric API inventory](spec/builtins.md#numeric-scalar-methods),
[value semantics](spec/value-semantics.md#floating-point-utilities-and-math),
[native declaration boundary](spec/standard-declarations.md#numeric-declaration-boundary)
and [architecture](architecture.md#numeric-scalar-library-ownership). The
[library guide](../site/src/content/docs/library/overview.mdx#numeric-policies-and-sampling)
connects explicit overflow policies to script-owned sampling and numerical work.
Bit-exact transport is distinct from arithmetic NaN payloads and approximate math;
`sqrt` is correctly rounded, while extended-math precision is unspecified. Power
adapters enforce quiet/signaling-NaN unit cases and exact `i32` parity rather than
promising direct Rust agreement on every input.

Remaining acceptance is the full workspace test suite and complete
feature/backend/platform CI matrix. The Linux workspace suite and standalone feature
script schedule these numeric tests. Existing Windows/browser jobs do not yet select
the representative numeric contracts: wiring Windows and browser/WASM float acceptance
there remains an explicit follow-up, not merely a queued run. Focused local
source/source-free method and boundary coverage does not establish that matrix or
broader JIT/native execution. Native associated constants, native-call
const evaluation, public/global RNG APIs, full ray tracing and new numeric types
remain outside this delivery; existing scalar const evaluation and backend eligibility
are unchanged. Future work requires separate scope, not compatibility aliases or a
second free-function math catalog.

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

## Async blocks, structured concurrency and Runtime API

Implementation and focused local acceptance are complete. The completed AR01-AR06
plan is retired; Git history retains its phase decisions and validation ledger.
Lasting contracts belong to [async execution](async-execution-design.md),
[structured composition](spec/execution.md#structured-two-input-composition),
[host task scopes](host-task-scope-design.md) and the
[typed embedding API](spec/embedding-api.md#typed-async-calls-scheduling-and-native-waits).

The SDK Runtime supports pinned function/member bindings, object access, driving
call_async and result-only submit_async. Both use one single-threaded scheduler;
normal scope Tasks retire internally. Borrowed Call drop cleans local descendants
without another poll; ticket drop requests cancellation through runtime driving.
No public Driver, mandatory executor, compatibility Runtime alias or parallel heap
execution was added. Explicit backend-owned staging remains for tooling, low-level
contracts and the browser's sliced ordinary-main protocol, not normal SDK calls.

Focused local validation covers async syntax/capture/control flow, real wake routing,
structured IO/failure/cleanup, original Err provenance, Task dependency cycles,
public typed calls/objects/methods, pinned reload and frontend-free artifact execution.
Source-free composition checks both submitted and borrowed driving calls using real
Waker notifications; executable host examples cover independent scope work and late
completion rejection. Structure, formatting, workspace/all-target Clippy and diff
checks passed. No known selected local integration failure remains. Full workspace
and complete feature/backend/platform matrices are **pending CI**, not inferred from
these focused results; optional editor WASM/WASI-SDK validation was not run locally.

CI run [39883455884](https://github.com/kagari-lang/kagari/actions/runs/39883455884/job/120186018422)
exposed a stale editor rejection fixture for the now-supported `async {}` form.
The fixture now rejects unsupported `async move {}` instead; existing positive
async-block coverage remains. Local Tree-sitter CLI checks pass for the supported
form and all eleven rejection fixtures, all seventeen corpus cases and generated
asset consistency. The native Node test rerun is blocked by a missing Windows
addon for Node 24.15.0; passing CLI checks do not establish that suite or CI passed.

Remaining scope stays separate: native JIT/AOT suspension, Rust async-closure
registration, dynamic/race/timeout combinators, arbitrary Object-to-trait casts,
generated host views, and the [confirmed current-call update migration](update-model-design.md#calls-retained-objects-and-new-trait-implementations).
That migration retains existing Future/closure/resume bodies, but selects current
implementations for subsequent ordinary calls; it is not implemented by the completed
async work. The [Host Program/Module plan](host-api-refactor.md) scopes HM00-HM04 to
stable Program/Module handles, relative public lookup, checked type exports and
reload compatibility. Existing handles must query the latest published contents of
their installation; existing function bindings call current implementations without
reacquisition/rebinding. HM00 must settle representation, internal version retention
and the UP handoff. The user-prioritized
[alias prerequisite](host-api-refactor.md#transparent-generic-alias-prerequisite)
is locally complete. Joint HM00/UP00 remains authorized but paused and incomplete;
HM/UP implementation phases have not started.
Broader preparation/tooling remains deferred.

## Text literals and String traversal

TL01-TL05 implementation and focused local acceptance are complete; the execution
plan is retired. Git history retains the phase ledger. Lasting contracts belong to
[literal syntax](spec/syntax.md#literals), [character values](spec/builtins.md#character-scalar),
[explicit String traversal](spec/builtins.md#explicit-string-traversal) and
[architecture](architecture.md). Rust-style raw/multiline and byte literal spellings
retain Kagari's managed String and fresh mutable array ownership, not Rust borrowing.
Ordinary strings replace f-prefixed expression interpolation with identifier-only
holes; migrate effectful expressions using ordered intermediate bindings. Char
remains distinct from integers through source-free artifacts and typed host access.
Regenerate development artifacts affected by the changed builtin/operand schemas.

Focused evidence covers scalar/byte domains, source/artifact/JIT-fallback behavior,
ordered formatting effects/traps, literal freshness, persistent static reload,
host/Serde admission, lazy cursor GC/publication and native-wait retention. The
three character/byte/interpolation examples also pass CLI checking. Structure,
formatting, workspace/all-target Clippy, grammar products and affected local editor
checks passed. Final documentation review corrected obsolete syntax/tooling claims.
These checks do not establish full CI acceptance or wider native code generation.

Outstanding acceptance: full-workspace and exhaustive backend/feature/platform
matrices remain CI-only; native Tree-sitter Node/Zed/shared capture suites need the
missing Windows Node 24 addon or a working CI environment. Editor WASM requires
WASI SDK 34 (local SDK 25 was observed). Full browser/site and editor-host acceptance
were not rerun for this work. Run those checks in their designated environments;
CLI corpus/query success is not a substitute. No push or publication was performed.

Expression interpolation compatibility, a format API, broader Unicode APIs,
u8 range-pattern expansion, native backend expansion and unrelated migrations
remain outside this delivered scope.

## Host API and update execution

HM/UP implementation and phased commits are authorized on 2026-10-23. The user
subsequently prioritized transparent generic type aliases first; that prerequisite
is now locally complete. HM/UP remain paused. On resumption, the order remains the
incomplete joint HM00 + UP00 checkpoint, followed by HM01, UP01, HM02, UP02, HM03,
HM04, UP03, UP04 and UP05.
The [Host API plan](host-api-refactor.md) and
[update plan](update-model-design.md) retain their own ledgers and acceptance.
HM owns live Program/Module lookup and verified type exports; UP owns stable
current-call dispatch, source Update batches, exact-source inspection and later
explicit state replacement. Existing public semantics remain unchanged until the
owning migration. MS, wider native expansion and repair-program linking are separate.

The first audit found the missing transparent-alias language capability; the user
resolved that scope question by explicitly authorizing generic aliases before HM/UP.
The [finite prerequisite ledger](host-api-refactor.md#transparent-generic-alias-prerequisite)
is the single owner of scope, implementation progress, validation and compiler
resource limits. Alias implementation, review repairs and local acceptance are
complete. HM/UP remain paused and incomplete, not resumed or completed by alias work.
This prerequisite does not implement HM host export schemas, export-driven layout
roots or UP dispatch. The linked ledger retains concise focused evidence; full CI
remains unrun.

## Other queued designs

These are proposals, not active execution plans. Activation and exact sequencing
require a concrete task; implemented foundations are not prerequisites to replay.

| Track | Scope and dependencies |
| --- | --- |
| [Multi-Runtime program sharing](multi-runtime-sharing-plan.md) | MS00-MS05 planned, unstarted: measure current same-product multi-Runtime costs; eliminate whole-code identity remapping copies and duplicated pure link preparation; establish safe cross-thread frozen-product reuse. Keep player heaps/statics/host borrows and installation publication private. MS00 freezes numerical memory/performance gates and the HM/UP identity/dispatch handoff before implementation. R13 type-only retention, cross-installation repair linking, global updates and native expansion remain separate. No capacity or speed claim from source LOC or historical tiny-program data. |
| [Interactive debugger](debugger-design.md) | Confirmed synchronous waiting at safe debug points, preserving the executing host thread's Rust call chain. Pause control, inspection lifecycle and SDK/DAP integration are queued and unstarted; no whole-execution continuation migration or automatic process-wide freeze. Lifecycle defaults and CLI spellings remain proposals. |

## Open review and documentation work

The authorized R15 -> R11 -> R12 repairs are implemented. The
[debugger observation contract](spec/debugger.md#safe-debug-points) keeps scalar
constants on their control-flow paths; the [GC contract](spec/runtime.md#gc-heap)
owns committed backing pressure, release-driven roots and occupied-object discovery.
The unpublished native API replaces `NativePayload::units()` with fallible
`charge() -> NativeResult<HeapCharge>`; providers and configuration consumers have
been migrated without compatibility aliases. Attributed bytes are not RSS or a
quota, and the 64 KiB collection floor is policy, not a measured optimum.

Focused local validation selected optimized/unoptimized debugger behavior and
artifact round-trip, all 12 MIR-pass contract tests, committed native storage and
mutation/cleanup accounting, cross-thread/stale root leases, sparse relocation,
interface snapshot independence and native take/reuse. These passed, as did the
runtime `execution-diagnostics` feature check, structure, formatting, workspace
all-target Clippy and diff checks. The old loop-hoisting test was replaced with
original block/span/scope preservation while retaining forwarding/reuse assertions.
[Deterministic work counts](performance-baseline.md#root-release-and-occupied-discovery-work-counts-2026-10-20)
establish only the selected release/discovery algorithm bounds, not timing gains.

Full-workspace tests and the complete feature/backend/platform matrix remain
CI-owned and unrun locally. BO04/IP12 historical timing failures remain open;
their measured hoisted images do not validate the current constant-placement policy.
Other review findings and native/frontend expansions are not activated by these
repairs.

[The consolidated review](review.md) owns unresolved SA and R findings. SA entries
include inline AST reuse, compact module handles, HIR model cleanup, local
functions, patterns, generic interface construction and host-registration unification. Resolved definition-identity,
namespace/import/default-body/Array work is described by the current architecture,
specifications and reading guides, not retained as review tasks.

The scoped R9/R10 runtime repair is handed off to
[execution failure semantics](spec/failure-semantics.md) and the
[completion contract](spec/host-interop.md#completion-transport-foundation).
Shared ResourceState admission covers conversion, native, VM and frame exhaustion,
retaining the first termination and origin, including observer precedence.
Completion transport contains payload, waker and hook destruction; the first hook
is retained after a registry fault. Guarded retirement continues cleanup,
quarantine drains remaining owned executions, and Runtime Drop also drains
standalone retained owners. Local validation selected conversion/reentry and
observer precedence, atomic frame admission, Task business rejection, completion
transport and source-free owned-retirement contracts; those tests passed.
Structure, formatting, workspace all-target Clippy and diff checks passed.
Full-workspace tests and the complete feature/backend/platform matrix were not
run locally and remain CI-owned. This scoped handoff is not a
whole-runtime audit or performance acceptance. The
[runtime review](review.md#runtime-architecture-review) retains R13 type-only code ownership;
R7/R8 and unrelated open findings remain unchanged and are not activated here.

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

Source interface impl targets, blanket impls, derive additions and NC
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
acceptance. Arbitrary trait queries, extensions to generic construction, NC, broader
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

Method authoring is implemented through
[module-owned registration](native-registration.md#generic-contracts-defaults-and-selected-calls).
SA23 scalar cleanup, native collection storage and
broader native-backend work remain separate. No performance improvement or
completion of those tracks is implied.

## Default and struct update

Ordinary core-library [Default](spec/builtins.md#default-construction),
[contextual static factories](spec/traits.md#contextual-static-factories) and
[struct update expressions](spec/syntax.md#struct-update-expressions) are implemented.
[Value semantics](spec/value-semantics.md#struct-update-construction) owns evaluation
order, fresh outer identity and shared members; the [HIR guide](architecture/hir.md#struct-update-facts)
owns base/omitted-field facts. The completed plan is retired; Git history owns its
phase decisions and local validation ledger.

Focused local checks passed for registration/prelude, the finite scalar/String/Vec/
Option implementation inventory, generic/contextual calls, nominal/generic/access
rejection, recovery/navigation/reuse, source/editor syntax, effects/traps/cancellation,
GC cleanup, retained generic Default calls after reload and source-free artifact
execution. Scalar fixture entries still require actual native execution; new object
and library cases may fall back before entry and do not claim native expansion.
Structure, formatting, workspace all-target Clippy and diff checks passed. Full
workspace and complete feature/backend/platform/editor inventory remain CI-owned.

Updated Zed WASM loading/rendering remains unverified: the fixed official SDK 34
Windows archive failed to download twice with curl 56 connection errors. Native
parser/capture/incremental tests and staging passed, but are not host rendering
evidence. Retry the [existing pinned build/host workflow](../editors/README.md#syntax-change-workflow)
when SDK acquisition is available; do not substitute SDK 29 or infer host acceptance.

Derive, automatic enum selection, field declaration initializers,
host/view update, new native instructions and broader standard Default implementations
remain separate. Default is not a language protocol or special runtime constructor.

Two bounded investigation probes need separate triage, not silent feature expansion.
On the DF04 working tree, a direct closure call with projected parameter
`<Tag as Pick>::Item = Cell<i32>` rejected a concrete Cell<i32> argument with
ArgumentTypeMismatch; the checked Fn::call path passed. HIR's direct function-value
argument normalization is the follow-up owner; independence from update syntax was
inferred from that path, not established on a historical baseline. A generic identity
closure in `duplicate<T>(base: Cell<T>) -> Cell<T>` also reached MIR InvalidGraph
with no update expression; unused generic associated-type declarations remained in
the reproduction. Compiler generic-closure/environment lowering and whole-program
MIR graph validation own initial triage. Neither introduction revision nor precise
root cause was isolated; ordinary generic update functions passed.

## Review follow-up order

The following is the proposed default order for the unresolved [SA review](review.md)
work, chosen to reduce replacement of newly implemented models. It does not
activate implementation, settle open language/API choices or combine unrelated
findings into one migration. Explicitly grouped scopes below share one plan; each
activated task retains a finite scope and focused gates. Default/struct update is
implemented [above](#default-and-struct-update); the table below retains the default
order for the other queued findings.
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

SA22 generic interface construction is implemented through static and shared
execution. [Trait conversion](spec/traits.md#current-interface-conversion-boundary)
owns language behavior; [interface dispatch](architecture.md#interface-dispatch),
[bytecode](spec/bytecode.md) and [artifacts](spec/artifacts.md) own proof, generation
and source-free admission contracts. Methodless traits, parent and associated
bounds, generic implementation dependencies, imported helpers, retained closures
and supported host receivers use exact checked evidence without runtime impl search.

The follow-up to [CI run 39587942382](https://github.com/kagari-lang/kagari/actions/runs/39587942382)
composes symbolic tuple construction capabilities and preserves their shared adapter
bodies. Focused coverage includes nested tuple boxing, caller-owned types in imported
helpers, source-free rejection of missing/forged tuple evidence, and retained versus
evicted witness environments. The complete matrix still requires a new CI run.

Focused local coverage includes forged capabilities/instructions, default and
override execution, old-provider/new-caller applications, forced GC, synchronous
host reentry and trap/depth/cancellation cleanup. Canonical artifact emission and
one build without default features passed; existing actual-native scalar and
explicit fallback assertions remain intact. Workspace all-target Clippy, structure,
formatting and diff checks passed. Full workspace tests and the exhaustive
feature/backend/platform matrix remain pending CI; new native conversion support
is not claimed. The completed BI01-BI06 plan is retired; Git history owns its ledger.

Blanket interface self-conformance, broader associated-type solving and the other
review tracks remain separate. SA11 is still deferred by user instruction.

| Order | Review scope | Reason for placement |
| --- | --- | --- |
| Deferred | [SA11](review.md#sa11-hir-declarations-and-adt-members-carry-container-derived-identities): declaration identity review | Deferred by the user. Any later review must preserve the implemented function ownership model; field removal remains undecided. |

SA9 context-owned module handles are implemented. The
[identity architecture](architecture.md#definition-identity-ownership) and
[HIR guide](architecture/hir.md) own module-root scope, retained catalogs and
file/revision/arena checks. Focused identity/import/cache/publication, source-program,
canonical-output, source-free export and pinned-dependency reload cases pass,
as do structure, formatting, workspace all-target Clippy and diff checks.
The [matched graph measurements](performance-baseline.md#context-owned-module-graph-handles-2026-10-21)
include initial registration, publication, retained graphs and table growth;
their reductions apply only to that bounded graph workload. Full-workspace tests
and the complete backend/feature/platform matrix remain pending CI acceptance.
The completed MH01-MH05 plan is retired; Git history owns its phase ledger.

SA1 inline AST reuse is implemented. Physical text, line indexing and parse storage
are shared; logical modules retain separate identities/arenas and owned syntax
views. The [HIR query contract](architecture/hir.md#query-work-reuse-and-publication)
and [syntax guide](architecture/syntax.md) own recovery, diagnostic projections,
physical error gates and snapshot retention. Focused source/HIR/lint/compiler/SDK
checks, structure, formatting, workspace all-target Clippy and diff checks pass.
The [matched declaration measurements](performance-baseline.md#inline-module-ast-reuse-2026-10-21)
record allocation reductions and the unchanged nested-query timing limitation;
these do not establish end-to-end compile latency or finer cache invalidation.
Complete workspace and backend/feature/platform acceptance remain pending CI.
The completed IA01-IA05 plan is retired; Git history owns its ledger.
SA14 named local functions are implemented. The
[language contract](spec/syntax.md#blocks-and-statements) and
[HIR ownership model](architecture/hir.md#named-local-function-ownership) own
noncapturing block scope, forward references/recursion, independent generics,
function values and async behavior. Source-free artifact, imported default-body,
query/navigation/lint/formatting and retained-generation lifecycle cases pass.
Focused checks cover traps, depth exhaustion, cancellation and GC cleanup;
structure, formatting, workspace all-target Clippy and diff checks pass.
Complete workspace and backend/feature/platform acceptance remain pending CI.
The LF01-LF06 plan is retired; Git history owns the phase ledger. SA11 remains
deferred and unstarted. R8 owns finer invalidation for modules containing local
functions; this delivery conservatively rechecks changed revisions.

Structured import paths (SA15) are implemented. The
[HIR guide](architecture/hir.md#structured-import-paths-and-source-sites) and model
rustdoc own written segments, normalized lookup boundaries and physical prefix-site
contracts. Focused import/recovery/visibility/publication, navigation/cache/snapshot,
native declaration and compiler source-free export checks passed, as did required
local code-quality checks. Complete feature/backend/platform acceptance remains
CI-owned; no speed or memory improvement is claimed. The completed SA15 plan is
retired and Git history owns its phase ledger.

Important ordering constraints and limits:

- SA11 must preserve the implemented local-function reservation/finalization,
  source-map alignment and separate body ownership when reviewing self IDs.
- Inline syntax views preserve written import segments and physical source
  identities. Module handles do not replace either of those inputs.
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

The latest [CI run](https://github.com/kagari-lang/kagari/actions/runs/39520471271)
at `fa82bd48` passed structure, editor tooling and Windows AOT jobs. The Rust job
failed only the compiler enum-layout test: its conflicting dependency fixture also
invalidated a constructor, so local validation returned `InvalidOperation` before
graph layout comparison could run. The fixture now changes an unused variant,
preserving valid constructor operands and asserting the exact `InvalidHostInterface`
diagnostic from shared proof preparation, which rejects conflicting carried enum
records before the older layout comparison. This follows the existing
local-before-linked validation boundary; production validation is unchanged.

Focused local validation selects the repaired enum case, unused public enum
templates, applied nominal ABI and scoped layout arguments; all four pass.
Structure, formatting, workspace all-target Clippy and diff checks pass.
Complete workspace tests remain CI-owned. The failed Rust job skipped formatter
inventory, the async example, standalone SDK feature consumers and CLI native tests; those steps and
the complete repaired Rust run still require CI acceptance. Exact WASM-backed
Pages build acceptance remains separate.

[AGENTS.md](../AGENTS.md) owns verification policy. Local implementation checks
select affected contracts and a small dependent regression set; full workspace
and feature/backend matrices belong to CI. Documentation-only changes need
content/link/diff checks. Performance reports retain revision, workload, machine,
toolchain, profile/features, parallelism and cache state, separating compilation,
preparation and execution.
