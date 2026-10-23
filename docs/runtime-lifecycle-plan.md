# Runtime storage, parking and version identity execution plan

## Status and authority

**Implementation and per-phase commits authorized.** Execution resumed at
`289bee0b`; RL01 is complete with focused local acceptance and RL02 is next.
Originally requested on 2026-10-21 against `fd34f689` after local R16-R18 completion. This document owns RL01-RL03, their order,
finite scope, acceptance and progress. The [roadmap](implementation-roadmap.md#runtime-storage-parking-and-version-identity)
owns activation; [review](review.md) retains R19, R14 and R13 until their respective
acceptance. Execute buildable phases in order, with one coherent commit per phase
and a `Plan-Phase: RLxx` trailer. Push and deployment remain unauthorized.
Use Conventional Commits; explain breaking API changes rather than adding bridges.

Follow [project goals](project_goal.md), [architecture](architecture.md),
[runtime semantics](spec/runtime.md), [host interop](spec/host-interop.md) and
[repository rules](../AGENTS.md). The completed CS and AE work is a foundation to
preserve, not replay. BO04/IP12 lifecycle timing gaps and the CS 500 ms compilation
goal remain separate; this plan neither claims their cause nor requires an unbounded
tuning effort to close them.

## Outcome and finite scope

1. R19: scalar colors are chosen independently of values that ultimately occupy
   distinct managed locations, with common liveness and debugger facts preserved.
2. R14: one successful parking transition performs one actual retained-graph safety
   proof across all frame roots, without an immediate duplicate proof at commit.
3. R13: exact compiled-version identity no longer owns original bytecode/snapshots
   merely to distinguish versions after cross-context normalization.
4. Measure and decide the remaining R13 identity-domain and type-only retention
   questions. These decisions are required; implementing either large split is not
   implicitly authorized by the version-token phase. Retain unresolved findings at
   their existing review/roadmap owners rather than marking all of R13 fixed.

Excluded: managed-slot coalescing, a new allocator/collector family, bytecode flow
solver replacement, persistent safety caches, arbitrary fast-path flags, permanent
engine reuse, compilation caches, R7/R8/SA21 work, artifact pruning, wider JIT/AOT,
new execution charging/permission policies and automatic Pages publication.
No language, portable format or runtime ABI version bump is planned.

## Evidence and ownership inventory

This is source/test inspection, not a new reproduction, timing or memory result.
The cited mechanisms are present at the baseline.

| Finding | Current owner and mechanism | Intended replacement |
| --- | --- | --- |
| R19 | [Slot allocation](../crates/kagari-runtime/src/module/execution/allocation.rs) colors all logical intervals; [frame layout](../crates/kagari-runtime/src/module/execution/layout.rs) uses colors only for scalars and allocates managed locations separately. | One physical-storage classification and scalar-only color interference; retain unified logical facts. |
| R14 | [VM driver](../crates/kagari-vm/src/executor/driver.rs) calls `can_park` at slice exhaustion, then [slice exit](../crates/kagari-vm/src/executor/mod.rs) invokes `park`, which calls it again. [Stack parking](../crates/kagari-runtime/src/frame/owned.rs) validates each frame with a fresh [GC retention walk](../crates/kagari-runtime/src/gc/future.rs). | A runtime-owned proof/commit transition with one shared visited set across its actual roots. |
| R13 version | [VerifiedProgram](../crates/kagari-runtime/src/module.rs) stores both `modules` and payload-backed `version`, plus `version_definitions`. [Normalization](../crates/kagari-runtime/src/module/normalization.rs) remaps current records but retains original records/snapshot through version fields. | A private immutable shared version token, independent of payload and definition scope. |
| R13 domains/types | [Identity writer](../crates/kagari-common/src/identity/map.rs), installation/session owners and loaded-module-backed type/layout provenance have different lifetimes. | Bounded churn and retention probes followed by explicit decisions, not one global cache flush. |

Small examples explain the changes:

- A scalar `a` dies, a managed `m` uses a color, and a later non-overlapping scalar
  `b` receives another color because of the shared free pool. Layout cannot merge
  different scalar colors, even though `m` never shares their bank. R19 removes
  that false interference, not real CFG/debug liveness or managed roots.
- D frames can refer to one G-object graph. Today's successful slice boundary can
  repeat about 2*D*G object work. R14 should validate frame-local conditions for each
  frame but traverse the shared graph once for the entire transition. Root/edge
  enumeration and disjoint graphs still have real costs.
- A prepared program P loads into foreign identity context A: current code is
  remapped, but the original graph remains via `version`. A token lets P and A
  remain the same exact version without retaining that original payload solely for
  comparison. Equal bytes prepared independently must still be different versions.

## Design contracts

### RL01: storage classes before coloring

Use the physical bank decision already owned by frame layout, including its
`scalar_type` classification, as the shared classification owner. A semantic scalar
type alone does not establish scalar storage: managed capture cells and other
representations keep their actual storage class. Do not duplicate a primitive-type
match in the allocator or introduce a public type classification API for this task.

Retain all logical ranges, parameter entry uses, debug availability, CFG liveness,
closed interval endpoints and read-before-write semantics. Build interference/free
colors only for scalar locations. Managed locations remain individually allocated
and rooted; how a managed logical row is represented in the internal allocation
result must be explicit, not an accidental scalar color consumed by another caller.
Update `SlotAllocation`/`FrameLayout` consumers coherently, keeping assertions valid
for mixed, all-scalar, all-managed, Unit and unused-slot functions.

Preserve public reused-frame access, initialized entry and side-entry admission,
including backward joins and loops. Keep bounded allocation preparation and safe
fallback; neither fewer colors nor the completed R17 index permits ignoring a debug
use. Charge any new preparation and bound scratch. Do not change solver scheduling,
expand reuse eligibility or weaken initialization/GC checks in this phase.

### RL02: one proof owned by one parking transition

Inventory all current `can_park` callers, not just the duplicate slice path:
[await/future activation](../crates/kagari-runtime/src/frame/future.rs),
[waiting/polling](../crates/kagari-runtime/src/frame/waiting.rs) and
[queued factories](../crates/kagari-runtime/src/frame/factory.rs) also ask about
parkability. Preserve their preconditions and checks on newly introduced values.
A check before a host poll cannot authorize a later commit after that poll mutated
state. Do not mechanically remove every repeated call with the same name.

Choose a runtime-owned atomic attempt or an unforgeable, strictly scoped transition
proof during RL02 API review. A declined slice attempt must leave the activation
usable so the driver can continue to a safe boundary; consuming the stack on a
normal non-parkable outcome is not acceptable. A successful attempt must reach the
commit without script execution, host reentry, user conversion, mutation or an
unprotected tracing callback in between. No serialized/trusted flag, freely retained
permit or public unchecked-park entrypoint is allowed. Do not store a proof in a
long-lived `DriveOutcome` that can outlive its validity.

The runtime validates runtime/session identity, scope shape, absence of native
borrows/host scopes, outstanding mutations, native-entry state and all actual retained
managed values. Share one GC-owned traversal/visited set across all retained frame
roots for that operation. Preserve edge/value admission, cycle handling, invalid or
foreign handles, erased receivers, native trace hooks and the collection guard that
prevents mutation/reentry while tracing. No object-level async-safe result survives
this transition: an alias can introduce a scoped resource before the next slice.
Do not keep a heap-sized scratch set alive across unrelated drives without a separate
measured justification and lifetime decision.

Ready slice parking and pending-wait parking retain different publication rules.
`park` currently marks ready; `park_waiting` deliberately does not overwrite a
concurrent completion/cancellation notification. Preserve pending checks, sticky
cancellation, wake ordering, trace failures, quarantine, roots and cleanup on all
exits. A tracing error is not ordinary non-parkability and must not be swallowed as
"continue executing". Use existing runtime/GC guards and caller cancellation rules;
do not introduce new script execution quotas to justify the optimization.

### RL03: exact identity without executable ownership

Replace the payload-backed `version` marker with a private shared allocation/token
created at verified-program construction. Clones and normalization retain that token;
independent preparations, even with identical bytes/fingerprints, create distinct
ones. Preserve root equality in `same_version` and the current version-comparison
contract; token equality is not ABI compatibility or permission to execute.

Delete `version_definitions` and original-only payload retention once the consumer
inventory confirms they serve no remaining scope contract. Keep destination
`definitions`, exact referenced IDs, remapping, exports/targets, prepared execution
facts and immutable accepted-prefix validation. Source -> A -> B normalization must
map from the current scope, not look up IDs through the original context. Same-context
and reference-free fast paths may intentionally share current records.

Concrete external consumers of `same_version` include SDK
[native preparation](../crates/kagari/src/program/native.rs) and
[AOT installation](../crates/kagari/src/aot.rs). Preserve runtime authority, descriptor
and epoch checks, actual executable/code leases, candidate rollback and pinned old
callbacks. Token survival alone must not revive retired code; actual callables and
native installations still retain their executable dependencies. No hash identity,
process-global token table or public forgeable version constructor is proposed.

Use `Weak` probes to the original module allocations to distinguish true payload
release from executable registry retirement. On cross-context loads with references,
release original preparation/other owners and demonstrate that original payload
expires while the remapped program still resolves/executes. Repeat source -> A -> B.
Do not assert release while another owner intentionally still uses the original as
its current graph. Check token-only retention in a private test scope without adding
a public token accessor. Keep scope lifetime and old-version execution assertions at
their existing semantic owners.

### Remaining R13 decisions, not hidden migrations

Run two finite probes after token replacement:

1. **Identity domains:** reused versus distinct nominal names under retained versus
   independent installations/sessions, with retained/released snapshots. Separate
   definition records/capacity, append-time snapshot copy/peak cost and bytecode
   ownership. Dropping a runtime need not clear a deliberately longer-lived writer;
   no process-global identity leak has been established. Document intended lifetime.
2. **Type-only provenance:** retain only an old layout or nominal `TypeArgument`
   through replacement/collection after releasing actual old callables. Distinguish
   required old type facts from retained immutable instructions. Compare a control
   that intentionally retains a callable/native product. Executable retirement is
   not proof of bytecode destruction, and detached type readability is not executable
   admission.

Use small frozen counts, for example 1/16/128 churn cycles, and record the concrete
workload/owner lifetime that makes retention material. Report findings in the
existing R13 entry and measurement owner. Choose either "retain present model for
this lifetime" or "separately scope a specific split" with evidence. A provider
baseline plus session/runtime-local writers is not preselected; never fork mutable
identity builders under the same table ID. Type/layout-code separation likewise
requires a distinct approved design if justified. No further implementation phase
is silently activated. Only the version-marker subfinding can be closed by the token
change; unresolved measured subfindings remain visible.

## Mature references and deliberate adaptations

- Cooper and Torczon, *Engineering a Compiler*, second edition (2012), chapter 13
  (§§13.1-13.4): allocate according to actual storage/interference classes. The
  [existing chapter review](review.md#chapter-map-and-architectural-implications)
  records the relevant pages. Kagari colors reusable VM backing slots, not CPU
  registers; there is no hardware spilling, managed coalescing or custom machine
  allocator project here.
- [Tokio 1.48.0 task harness](https://github.com/tokio-rs/tokio/blob/tokio-1.48.0/tokio/src/runtime/task/harness.rs),
  reviewed in the [runtime reference inventory](review.md#primary-implementation-references):
  establish task state before notification and keep cancellation/cleanup under a
  transition owner. RL02 borrows that ownership discipline, not Tokio's threaded
  scheduler, JoinError or a proof that Kagari mutable object graphs are safe.
  Kagari's GC traversal/guard is the existing semantic owner of that proof.
- [Rust 1.90.0 type context](https://github.com/rust-lang/rust/blob/1.90.0/compiler/rustc_middle/src/ty/context.rs)
  ties interned identities to a context owner; [Java SE 25 JLS 12.7](https://docs.oracle.com/javase/specs/jls/se25/html/jls-12.html#jls-12.7)
  ties unloadability to defining-loader reachability (rechecked 2026-10-21).
  These motivate explicit lifetime owners, not Rust arena borrowing across reload,
  Java lazy loading, guaranteed immediate unloading or Java-sized domains. The
  private token is a small Kagari-specific fit to its existing shared exact identity;
  it must be independent of payload while keeping executable leases separate.

## Phase order and acceptance

### RL01 — R19 scalar storage-class allocation

- Freeze physical classification and `SlotAllocation` result invariants, then update
  coloring and frame layout together. Remove superseded managed interference.
- Add a focused mixed-storage case that demonstrates actual scalar capacity reduction,
  not only different color numbers. Reuse CFG/debug/public-access and GC contracts.
- Compare frozen mixed, all-scalar and all-managed controls; record scalar/managed
  capacities, preparation work and retained frame bytes independently of timing.

Acceptance: managed locations remain distinct, scalar non-overlap can reuse the
same backing storage without managed interference, and public access/debug/GC
semantics remain unchanged. Allocation exhaustion still uses safe fallback. No
claim that every function gets smaller or executes faster is required.

### RL02 — R14 transition-owned parking proof

- Freeze the call inventory and runtime-owned attempt/proof API, including how an
  ordinary declined slice remains executable and how waiting publication differs.
- Share actual retention traversal across all roots for one transition and eliminate
  the immediate successful query/commit duplicate. Remove superseded wrappers, not
  independent checks required after mutation or host polling.
- Reuse owned-drive, pending completion/cancellation, graph safety and cleanup
  contracts. Add distinct shared/disjoint graph visit counts and alias-mutation,
  trace-hook failure or declined-transition cases only where coverage is missing.

Acceptance: a successful slice transition proves the reachable graph once rather
than once per frame and again at commit. A later slice rechecks mutated aliases;
scoped/foreign/invalid values still fail. No lost wake, readiness overwrite, leaked
activation or externally reusable safety proof. Waiting/factory transitions retain
their own admission, roots and cleanup. Measure parking/drive costs separately from
ordinary script work and GC collection; do not call this an IP12 fix by inference.

### RL03 — R13 version token, bounded retention decisions and handoff

- Replace payload-backed exact identity coherently in construction/normalization
  and all native/AOT comparison consumers. Delete unused original-only snapshot
  retention; preserve actual immutable scope and executable owners.
- Add original-graph `Weak` release assertions to the existing identity test owner,
  covering referenced source -> A and source -> A -> B plus intentional sharing
  controls. Preserve independent-equal-versus-shared version tests.
- Execute the two finite R13 probes above and record explicit decisions. Stop at
  token acceptance and those decisions; any larger split needs separate scope.
- Finish the selected cross-phase tests and required local checks. Move current
  contracts to architecture/spec owners and methods/results to the performance
  baseline. Remove resolved R19/R14 and only resolved R13 subfindings, leaving
  identity-domain/type-only questions visible where still material or undecided.
  Reconcile roadmap/index links and remove this plan on completion; Git owns its
  phase ledger and the roadmap retains pending CI.

Acceptance: exact versions survive cloning/import without identity-only payload
retention, independently prepared equal programs remain distinct, and runtime/code
leases still pin executable generations. No in-scope errors remain. The two other
R13 decisions are recorded without claiming their migrations implemented. Report
local results and pending CI independently; no completion claim from registry
retirement counters or pointer equality alone.

## Focused verification

This is a phase-selection menu, not an all-packages sweep or a suite to rerun after
every edit. Names were source-inspected, not executed while drafting. Check that
filters select nonzero tests and confirm feature requirements before invoking them.

| Phase / contract | Existing focused command or owner |
| --- | --- |
| RL01 intervals, debug ranges and fallback | `cargo test -p kagari-runtime --lib module::execution::allocation::tests` |
| RL01 backward join / layout | `cargo test -p kagari-runtime --lib backward_join_inputs_are_live_at_the_interval_start` |
| RL01 public reused-frame access | `cargo test -p kagari-runtime --lib public_frame_access_preserves_reused_binding_lifetimes_and_initialized_entry` |
| RL01 scalar/managed roots | `cargo test -p kagari-runtime --lib scalar_windows_preserve_bits_initialization_and_managed_roots` |
| RL02 graph safety | `cargo test -p kagari-runtime --lib async_retention_graph_contract` |
| RL02 slice/reentry/cancellation | `cargo test -p kagari-vm --lib async_owned_drive_contract` (select the exact core case where appropriate, not every similarly prefixed case) |
| RL02 pending wait roots | `cargo test -p kagari-vm --lib async_wait_live_storage_contract` |
| RL02 concurrent ready/cancel publication | Select affected cases from `tests::native_wait` in `kagari-vm`; do not equate only a runnable slice test with waiting acceptance. |
| RL03 version identity | `cargo test -p kagari-runtime --lib shared_verified_program_identity_survives_independent_runtime_linking` |
| RL03 referenced/repeated normalization | `cargo test -p kagari-runtime --lib scoped_programs_import_exact_identities_without_replacing_versions_or_epochs` |
| RL03 scope snapshots | `cargo test -p kagari-runtime --lib scoped_type_inputs_preserve_snapshots_and_compare_complete_identity` |
| RL03 wrong native version | `cargo test -p kagari --lib rejects_equal_but_distinct_versions_before_compiling` |
| RL03 pinned native reload | `cargo test -p kagari --lib reload_reuses_candidate_code_and_keeps_old_native_entries_version_pinned` |
| Cross-phase retired old types | Existing `nominal_type_origins_keep_bindings_without_retaining_execution_parents` in `kagari-runtime`; inspect its retained owners rather than assuming it proves instruction release. |

Native checks must assert the actual compiled/installed path where supported,
not accept interpreter fallback as proof. If the token consumer in AOT changes,
select `cargo test -p kagari --features aot-build --lib
installed_images_pin_versions_share_bindings_and_release_after_last_owner` on its
supported Windows x86-64 MSVC setup. It requires the configured linker; missing
platform/tool prerequisites remain explicit pending acceptance, not a pass.
Full native/platform/feature matrices remain CI-only.

Add narrow owner-local coverage for distinct missing boundaries, not a new smoke
suite per helper. Preserve decoded/source-free mixed values, old callbacks and
malformed inputs where affected. Source-disabled acceptance, if needed by the
changed boundary, runs in a separate Cargo feature invocation; produce its ignored
fixture with the maintained producer first. Native-host web tests are not browser
acceptance. No complete browser benchmark matrix is required for this work.

Each implementation checkpoint requires manual review of ownership, lifetime,
imports/re-exports, visibility, ordinary modules and effective LOC, plus:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Use narrow internal APIs and one semantic owner; no forwarding re-exports,
production globs, public bypasses or blanket structure exceptions. Keep the
1200-effective-LOC rule. Use focused tests, `--no-fail-fast` for multiple selected
targets and rerun only failing/newly affected checks after repairs. Full-workspace
tests and complete backend/feature/platform sweeps remain CI-only. Documentation
steps need content/link/diff checks, not builds.

## Measurement and stopping policy

Before each implementation, freeze a small local baseline at its actual owner; no
broad attribution campaign is a prerequisite. Capture correctness and real work,
not only renamed helpers or timing:

- R19: mixed storage versus scalar-only/managed-only controls, capacities and
  requested/retained frame bytes, including debug-extended intervals and fallback.
- R14: shared and disjoint graphs at fixed depth, then varied depth/root aliases;
  count transition proofs and actual unique object/edge visits, including cycles.
  Include a small scalar-only activation to expose setup overhead. Shared aliases
  remove repeated object traversal, not necessary per-frame/root enumeration.
- R13: original graph/snapshot retention versus normalized current ownership, with
  retained-callable controls and token-only probes. For domain/type decisions use
  the bounded counts above; do not create a production profiler or new GC family.

Use workspace release, default target/build parallelism/features unless a consumer
requires a documented feature. Build before sampling; use at least five retained
samples and one explicit warmup for elapsed comparisons. Record revision, toolchain,
machine, workload size/shape, profile, features, parallelism and cache state, with
medians/ranges and all samples. No concurrent Cargo/test workload during timing.
Separate preparation, execution, park/resume, collection and reclamation. Requested
bytes, retained bytes, allocator traffic and RSS are different metrics; a failed
`Weak::upgrade` establishes ownership release, not OS page return.

No unmeasured speed/memory target or universal percentage is promised. Success
requires the stated structural improvement, preserved semantic contracts and honest
measurements, including tiny-input regressions. Investigate a material regression on
the changed path before completion, with one bounded diagnosis/repair batch; request
new scope if needed rather than endlessly adding phases or lowering safety checks.
Do not rerun old BO/IP gates as an implicit acceptance expansion or claim them passed.
Temporary probes/logs belong under ignored `target/`; retain necessary sources,
methods, results and limits in tracked documentation so another machine can resume.

## Resumption and progress ledger

On resumption, inspect current HEAD, status and affected diffs; `fd34f689` is an
evidence baseline, not a reset instruction. Read this document and linked owners,
confirm implementation/commit authorization and start the first incomplete phase.
No previous chat, ignored probe or adjacent checkout is required. Follow the
repository-managed toolchain/dependency files and re-establish matched baselines
on a changed machine instead of comparing times across machines. Transfer uncommitted
plan/index/roadmap/review edits explicitly; a clone/pull does not carry them.

Update this single ledger with decisions, actual tests/commands, selected-scope
rationale, carried errors and remaining CI. Significant scope changes require an
explicit decision; no parallel progress queue is needed.

| Phase | Status | Next action / evidence |
| --- | --- | --- |
| Plan | Drafted | Baseline source/test owners inspected; mature ownership references recorded; no implementation or measurement claimed. |
| RL01 | Complete locally | Scalar-only coloring, explicit managed locations and dense scalar layout; focused safety and capacity evidence below. |
| RL02 | Not started | Freeze all parkability consumers and implement transition-owned graph validation. |
| RL03 | Not started | Private version token, payload-release assertions, two bounded R13 decisions and final handoff. |

### RL01 checkpoint

Shared physical `scalar_type` classification now selects scalar-only intervals;
`SlotColor` distinguishes managed locations explicitly. Layout consumes dense scalar
colors directly, deleting its obsolete remapping tree. Logical ranges, debug/CFG
liveness, closed endpoints, parameter uses and distinct-slot fallback remain intact.

Frozen before/after work/capacity probes at `289bee0b` and this checkpoint cover
mixed, all-scalar, all-managed, debug-extended and exhausted-budget controls at
window depths 1 and 8. Mixed scalar locations decrease 2 -> 1, managed stay 3;
debug-extended scalar locations decrease 3 -> 2, managed stay 3. All-scalar work
increases 34 -> 40 for explicit classification; fallback mandatory output work is
accounted separately from an exhausted optimization budget. No elapsed comparison
or speed claim was made. For these deterministic count-only probes the workspace
test profile was used deliberately; the release/warmup/five-sample policy remains
mandatory for any elapsed comparison. See [measurement methods and complete results](performance-baseline.md#rl01-scalar-storage-class-allocation-2026-10-23).

Selected tests passed: allocation (4), layout (4), public reused-frame access (1),
scalar/managed root windows (1), and the existing VM
`frame_roots_preserve_returned_objects_across_calls_and_collection_safepoints` (1).
The VM case supplies source/decoded mixed-storage execution through collection,
not merely hand-built layout assertions. Structure, formatting, workspace/all-target
Clippy and diff checks passed; ownership review found no remaining correctness issue.
Initial probe/formatting issues and a test build timeout were resolved. Anonymous
MSVC link failures cleared after cleaning only kagari-runtime and rerunning affected
tests. IDE scoped builds timed out; Cargo provides successful compilation evidence.

No in-scope failure remains. Full-workspace/backend/feature/platform CI, CS/BO/IP
performance limits and unrelated review items remain with their existing owners.