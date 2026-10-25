# Program-owned generic instance worklist execution plan

Status: implementation and one commit per phase authorized by the user on
2026-10-25. R7-01 through R7-03 are implemented and locally checked; R7-04 final measurement
and documentation closure are next. The [roadmap](implementation-roadmap.md#program-owned-generic-instance-worklist)
owns activation. This plan owns R7-01 through R7-04, scope, decisions, acceptance
and the progress ledger. It is not a new incremental-compilation API.

## Task and outcome

Replace program-wide whole-module specialization-demand rebuild rounds with
compilation-owned persistent module state and incremental instance scheduling.
Keep the existing generic execution model, diagnostics, resource limits, update
evidence and independent executable verification.

The [R7 finding](review.md#architecture-findings) identifies repeated preparation,
not an absence of queues: module lowering already has instance deduplication and
a local worklist. The change must eliminate the outer reconstruction mechanism,
not add a cache around it or introduce a second generic lowering implementation.

Illustrative dependency chain, not a benchmark:

```text
main -> A::foo<int> -> B::bar<int> -> C::baz<int>

Current: generate modules -> discover new demands -> regenerate modules -> ...
Target:  generate foo<int> -> enqueue bar<int> -> enqueue baz<int> -> close demands
```

Queue exhaustion alone is not publication readiness. Layout/interface generation
and retained-demand coverage can discover more work. Preserve that fixed point
without regenerating completed function bodies.

## Baseline and implementation owners

Planning baseline: `61b3fe4f` (2026-10-25), clean working tree before this document.
Source inspection establishes the mechanism; no R7 timing or work-count baseline
has been collected, and no tests were run for this planning step.

| Owner | Current mechanism / required change |
| --- | --- |
| [Program lowering](../crates/kagari-compiler/src/source/program.rs) | `lower_program` recreates module outputs, classifications and remaining limits each round, invokes every module's lowering, scans products for demands, then repeats. Replace reconstruction with retained state and delta routing. |
| [Module lowering](../crates/kagari-compiler/src/source/lower/mod.rs) | `lower_to_mir_with_requests` initializes, seeds, drains, assembles and verifies in one call. Separate initialization, resumable processing and candidate assembly. |
| [Instance planner](../crates/kagari-compiler/src/source/lower/instances.rs) | Existing instance keys and enqueue operations deduplicate locally. Separate long-lived module data from temporary borrows of shared identities, options and budgets. |
| [Product requests](../crates/kagari-compiler/src/source/lower/instances/requests.rs) | Preserve function, layout, shared-application and interface semantics while accepting demand deltas. |
| [Update requests](../crates/kagari-compiler/src/source/program/update_requests.rs) | `RequestQueue` is an accumulated per-module demand inventory. Integrate routing and retained-demand feedback without treating historical headers as current proof authority. |

One-time inputs include the checked authoring modules, source declarations and
manifest, exports and retained-demand correspondence. The root aggregate catalog
covers the complete checked dependency closure, including caller-private generic
arguments. Replacing it with isolated per-module catalogs is not equivalent.

Current module finalization may append bridge functions. Current program closure
also constructs preoptimization update evidence, optimizes and verifies a candidate,
then expands publication/retained demands against candidate proof coverage. These
are explicit migration boundaries, not disposable bookkeeping.

## Scope and preserved contracts

Own compiler scheduling, associated state/lifetime changes and directly affected
contract tests. Preserve public lowering entrypoints where useful, but replace
obsolete private internals directly. No compatibility aliases or duplicate paths.
No format/ABI bump is required for this unpublished internal refactor.

Preserve:

- Existing ordinary-function, method, static-initializer, export/layout and
  interface roots, including diagnostics for obligations not reachable from main.
- Checked declaration/type/bound evidence and source-free executable contracts.
  Backends must not resolve source syntax or trait implementations again.
- Concrete instances versus shared bodies with canonical binder arguments. A new
  shared application is not automatically a new monomorphic function body.
- Authored body origin versus output placement, particularly foreign trait defaults,
  selected implementation methods and generated descendants.
- Native defaults, conditional bounds, interface construction/witnesses, static
  generic methods, closures, callable adapters and async-generated bodies.
- Stable in-compilation identities, deterministic output, complete dependencies,
  error attribution, cancellation and bounded compilation. Budget failure must not
  publish a partial candidate.
- Preoptimization update evidence, source-body/classification correspondence and
  retained demands even when their source call disappears.
- The current [reload contract](spec/module-activation.md#live-installations-and-call-selection):
  ordinary calls select current implementations; existing frames, closures and
  Futures retain their selected computation bodies. This task changes neither rule.

Exclude R8 semantic invalidation, artifact reachability pruning, the separately
recorded generated tuple-bridge update defect, GC/native collection migration,
inlining, new generic semantics, runtime/JIT redesign, process-global caches,
parallel compilation and reopening the deferred general startup-latency project.
If a separate defect blocks acceptance, record its reproduction and owner here;
do not silently absorb it or weaken coverage to proceed.

## Selected ownership and scheduling model

The following is a responsibility sketch, not required Rust type names or a new
framework:

```text
Compilation state (one lowering operation)
  shared checked catalog / module lookup / definition context
  bridge identities / compilation limits / cancellation / update budget
  deterministic pending demand routing
  module states
    admitted instance keys -> stable local IDs
    pending bodies and progress cursor/status
    completed unoptimized functions and source/update records
    layout, interface, native and generated-product inventories
    newly discovered dependency/demand deltas
```

1. Initialize each authoring module once. Register its initial obligations through
   the same mechanisms used by the single-module entrypoint.
2. Admit demands using existing canonical keys and semantic ownership. Reserve an
   instance's identity before exploring dependencies so recursive references reuse
   that identity. A pending/in-progress/completed distinction may use existing
   vectors and cursors; do not add redundant status maps without a consumer.
3. Process only pending work. Report new dependencies from each new body/product,
   either directly or through a one-time scan of that new output. Do not repeatedly
   scan all completed instructions for scheduling purposes.
4. Route new demands to the owning output module and deduplicate before generation.
   Function bodies, layouts, interface products, native targets and shared
   applications have distinct identities; retain their existing semantic handlers.
   Do not force all products into a universal function-instance key.
5. Let layout/interface/adapter discovery feed scheduling before declaring closure.
   Generated bodies must be admitted once under stable identities, even if candidate
   assembly or coverage checking runs again.
6. Assemble and independently verify a candidate. Preserve preoptimization update
   evidence and derive coverage from current candidate facts. If coverage expansion
   adds work, resume the retained state; reject an uncovered non-progressing demand
   rather than looping or accepting incomplete proof.
7. Return only a fully verified candidate with covered retained demands. Drop all
   scheduling state when this compilation ends, including on failure/cancellation.

Shared mutable resources must be borrowed for a bounded processing step, not held
by multiple persistent planners. Avoid a `RefCell`/global-cache workaround for
`InstancePlanner`'s existing `&mut BridgeIdentities` ownership. Maintain one
semantic implementation for single-module and program lowering.

Output accounting is compilation-owned: charge admitted products/instructions
according to the existing limits, not again for duplicate requests or candidate
reassembly. Include generated bridges and interfaces; do not reset allowance when
another module runs. Keep proof/update work budgets distinct from output counts.
Audit limits before choosing the accounting representation; preserve what each
public option bounds and its rejection behavior.

Choose deterministic root and demand ordering rather than iterating hash maps to
assign IDs. Exact old incidental numbering is not a compatibility requirement, but
all references, portable identity mappings and canonical-output contracts must
remain consistent. Preserve diagnostic obligations and deterministic attribution.

### Candidate assembly decision required in R7-01

Record exactly which operations can discover demands and which can consume or
mutate output: layout closure, ABI/interface bridges, update evidence, optimization,
module verification, program verification and retained coverage expansion.

Select an ownership path that retains unoptimized completed bodies and does not
append the same generated bridge twice. If a later demand requires a fresh candidate
or verification seal, invalidate the affected derived products explicitly. A bounded
candidate snapshot/reassembly is allowed where required by current verification;
re-lowering source bodies or cloning the whole program on every queue pop is not.
Measure candidate assembly separately so repeated preparation is not merely moved.
Do not claim that all verification can run exactly once.

### R7-01 lifecycle decisions and baseline

Retain owned instance state and completed unoptimized outputs; create short-lived
planner contexts borrowing shared identities and checked inputs. Keep planner
functions as a stable indexed prefix and store generated interface wrappers
separately. Wrapper semantic identities remain stable; their own numeric IDs are
assigned in the candidate tail. Existing wrapper calls target planner IDs or shared
semantic identities, so late planner bodies cannot collide with wrapper slots.
Generate wrappers once, and persist layout `seen`, binder scope and input cursors.
Prepare bridge dependencies before emitting a wrapper that needs their bodies.

Keep accumulated demand inventory for dynamic receiver enumeration, separately
from pending delivery. Extract callable/construction facts once per new product.
Reuse the contract-owned construction closure over retained summaries at barriers;
refresh method selection when tables/targets change. Do not introduce another
construction solver. Candidate assembly clones unoptimized products only at a
closure barrier; optimization consumes candidate copies. Refresh verification
seals and coverage when new retained demands arrive, without re-lowering bodies.

Budget review found unequal existing counters: planner admission counts shared
bodies/resumes/adapters while the program debit counts nonempty emitted argument
lists (including some closures). Preserve both checks rather than silently unify
their acceptance boundaries: retain per-module admission counters and validate them
against the original deterministic module-prefix allowance; count program output
products independently, once per retained output. Instructions retain generation
charges, including wrapper terminators. Type traversal and update/proof budgets
keep their current separate scopes. Recheck prefix allowances as preceding modules
grow, always reporting the original public limit.

Baseline command (39 samples, three repetitions per shape/update combination):

```text
cargo test -p kagari-compiler --lib tests::source_programs::worklist::bounded_worklist_measurements -- --ignored --exact --nocapture --test-threads=1
```

Environment: Apple M1 Max, arm64 macOS, 32 GiB RAM; rustc 1.99.1
(`58cf8f9c3`, LLVM 23.1.1), workspace test profile (optimized + debuginfo), default
features, default Cargo build parallelism/target, one measurement-test thread.
Fresh source databases/checking/lowering per sample; warm process/filesystem, no
output cache. Rust build time, history construction and MIR encoding are excluded
from lowering timers. Test-only counters add overhead. Source checking is timed
separately; this is not a release or end-to-end startup benchmark.

| Shape | Retained | Module initializations | Generated / final bodies | Candidate verifications | Median lowering ms | MIR bytes |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Tiny | No | 78 | 2 / 1 | 1 | 548.478 | 707124 |
| Chain 2 | No | 123 | 6 / 3 | 1 | 728.131 | 715472 |
| Chain 4 | No | 215 | 15 / 5 | 1 | 1136.301 | 723876 |
| Chain 8 | No | 423 | 45 / 9 | 1 | 1825.218 | 740684 |
| Chain 2 | Yes | 123 | 5 / 3 | 2 | 748.726 | 715085 |
| Chain 4 | Yes | 129 | 7 / 5 | 2 | 814.859 | 723489 |
| Chain 8 | Yes | 141 | 11 / 9 | 2 | 821.592 | 740297 |
| Fanout 2 | No | 126 | 8 / 4 | 1 | 737.450 | 720627 |
| Fanout 4 | No | 132 | 12 / 6 | 1 | 627.944 | 730951 |
| Fanout 8 | No | 144 | 20 / 10 | 1 | 614.063 | 751599 |
| Fanout 2 | Yes | 126 | 6 / 4 | 2 | 802.084 | 719321 |
| Fanout 4 | Yes | 132 | 8 / 6 | 2 | 683.766 | 727807 |
| Fanout 8 | Yes | 144 | 12 / 10 | 2 | 673.935 | 744779 |

The default test source context contributes 38 library modules. Ordinary chains
use 3/5/9 passes; fan-outs and retained cases use 3; tiny uses 2. The old callable
scan visits every generated body each pass, so this baseline body count also
bounds the repeated scheduling input, not individual witness-edge visits. Separate
scan/assembly timing and peak-memory attribution remain R7-04 measurement work.
Timings show warmup/noise (fan-out timings decrease despite more work); only the
counts establish the repeated-generation claim. The first build attempt timed out
at 120 seconds; the resumed command passed all 39 samples in 84.02 seconds of test
execution, with no known test failure.

### Mature implementation reference

Reviewed for this plan: [Rust 1.90.0 mono collector](https://github.com/rust-lang/rust/blob/1.90.0/compiler/rustc_monomorphize/src/collector.rs).
It discovers roots, marks instances visited before following uses, traverses call,
function-value and vtable-method edges, and produces deterministic final ordering.
Its used-versus-mentioned distinction demonstrates why output reachability must
not accidentally erase diagnostic obligations.

Reuse the graph-closure and operation-owned visited-state principles, not Rust's
exact root policy, parallel collector, linkage/drop-glue rules or full
monomorphization model. Kagari additionally needs shared generic bodies, checked
interface witnesses and live-update retained-demand coverage. Adapting the existing
planner is therefore preferable to copying a Rust collector or adding another IR.

## Ordered execution phases

### R7-01 — Freeze the baseline and lifecycle decisions

- [x] Recheck status, relevant diffs and current source; update the baseline if it moved.
- [x] Inventory root classes, demand producers, instance ownership/placement and all
  finalization feedback edges, including the candidate assembly decision above.
- [x] Record the shared-resource ownership split and budget charge points here before
  replacing the outer loop. Keep decisions concrete and limited to this task.
- [x] Measure a tiny control, a bounded cross-module generic chain and a converging
  fan-out at several recorded sizes. Include a retained-demand variant using the
  existing update harness. Confirm that the chain actually exercises repeated rounds.
- [x] Capture module initialization/body-generation counts, unique instances, demand
  scans, candidate assemblies/verifications and output sizes. Record lowering time
  separately from source checking, bytecode preparation and execution.

Gate: reproducible evidence identifies the repeated work and a finite replacement
path. If the expected mechanism is no longer present, report that discrepancy before
implementing a different optimization. Do not invent a speedup target after results.

### R7-02 — Separate retained module state from processing contexts

- [x] Split initialization, pending work processing and assembly at meaningful module
  boundaries. Keep facades small and follow repository effective-LOC/import rules.
- [x] Preserve instance deduplication, stable references, source records and generated
  descendants while moving shared mutable borrows to short-lived contexts.
- [x] Route single-module lowering through the same implementation; retain initial
  root/diagnostic obligations and run the focused lowering/limit checks.

Gate: affected compiler code builds and selected tests pass. The old program driver
may temporarily invoke the new lifecycle, but this is an explicitly temporary bridge
removed in R7-03, not a second long-term implementation.

### R7-03 — Replace program rebuild rounds with delta scheduling

- [x] Initialize module states once and route new demands without clearing outputs.
- [x] Integrate function/layout/interface/native/shared-body and generated-product
  dependencies, preserving catalog visibility and authored-versus-output ownership.
- [x] Integrate compilation-wide output accounting, cancellation and deterministic
  scheduling, including recursive instances and converging duplicate demands.
- [x] Reconnect candidate assembly, preoptimization update records, optimization,
  fresh seals and retained-demand coverage feedback. Keep non-progress rejection.
- [x] Delete the whole-module regeneration loop, superseded cumulative rescans and
  the R7-02 temporary driver bridge. Do not retain a fallback production path.

Gate: cross-module and retained-demand contracts pass, and work counters show no
repeat source-body generation for the same admitted instance during one compilation.
A necessary candidate proof pass is reported separately, not hidden in this count.

### R7-04 — Validate, measure and close the implementation

- [ ] Complete the selected contract coverage below; add only distinct missing cases.
- [ ] Repeat the R7-01 workloads under matched conditions. Verify module initialization
  once per module and body generation once per unique emitted body identity; check
  generated adapters as well as user functions.
- [ ] Compare tiny-input setup, chain/fan-out scaling, retained-demand compilation,
  candidate assembly and peak memory/output size. Explain regressions and residual
  repeated work; do not declare a timing win from work counts alone.
- [ ] Run local implementation acceptance checks, remove temporary probes and review
  changed ownership, visibility, imports and effective LOC.
- [ ] Move lasting architecture to `architecture.md`, measurements to
  `performance-baseline.md`, and unresolved limits/CI work to the roadmap/review.
  Resolve R7 only when its replacement and local gates are complete; remove this
  completed execution plan rather than retaining an in-tree phase archive.

## Verification selection

Start with existing compiler contract owners rather than adding a parallel suite:

| Contract | Existing owner / initial selection |
| --- | --- |
| Cross-module concrete demand | [Source programs](../crates/kagari-compiler/src/tests/source_programs.rs): `public_generic_dependency_functions_specialize_through_source_aliases` |
| Retained demands after source use disappears | [Update demands](../crates/kagari-compiler/src/tests/source_programs/update_demands.rs): `retained_cross_module_calls_compile_after_source_use_disappears` |
| Static/closure/generic obligations | [Static lowering](../crates/kagari-compiler/src/tests/source_programs/static_lowering.rs): `unused_initializers_retain_closures_generics_coercions_and_static_body_captures` |
| Caller-private generic layouts | [Type exports](../crates/kagari-compiler/src/tests/source_programs/type_exports.rs): `public_generic_layouts_keep_private_argument_contracts` |
| Interface target and layout closure | [Bytecode interfaces](../crates/kagari-compiler/src/tests/bytecode/interfaces.rs): `generic_interface_implementation_specializes_reachable_method`, `generic_interface_slot_requires_instantiated_method_layout` |
| Shared budget and cancellation | [Lowering](../crates/kagari-compiler/src/tests/lower.rs): `generic_interface_instances_share_the_instantiation_budget`, `lowering_honors_instruction_limits_and_cancellation` |
| Optimization failure isolation | [Passes](../crates/kagari-compiler/src/tests/passes.rs): `pass_limits_and_cancellation_do_not_publish_partial_modules` |

Run individual selections with `cargo test -p kagari-compiler --lib <test-name>`;
confirm each filter executes the intended test, not zero tests. Before R7-03,
select a small number of existing cases for foreign defaults/shared witnesses,
source-free admission and deterministic artifact emission, plus a directly affected
SDK update case if compiler tests do not establish publication behavior. Record
exact selections here. This is a bounded coverage-completion step, not permission
to run every compiler/SDK suite. Add missing recursive/converging scheduling or
late-coverage cases at the existing semantic owner only when not already covered.

Work-count instrumentation may be test-only or a temporary probe. Keep durable
assertions for the no-regeneration contract, not private container layouts. Record
measurement commands, workload construction/sizes, baseline and candidate revisions,
Rust toolchain, machine, workspace profile, features, default parallelism, cache
state and repetitions. Store temporary data under ignored `target/`; the durable
method/results must be understandable without those files. Separate Rust build time
from Kagari compilation. No universal linear-complexity or runtime-speed claim.

Local implementation acceptance:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Use workspace Cargo profiles, default `target` and default parallelism. Full-workspace
tests and the complete feature/backend/platform matrix remain CI-only; do not
reproduce them locally through many package commands. Use `--no-fail-fast` when
running multiple selected targets. Rerun only failed/affected checks after repairs;
previously passing checks remain evidence for unchanged code.

## Execution and recovery rules

Once implementation is explicitly authorized, proceed through the finite phases
without requesting permission for routine in-scope decisions. Update this ledger
at each checkpoint. Keep checkpoints buildable where practical; attempt the affected
build/tests at ownership boundaries and record any carried error, exact reproduction,
representative diagnostic and owning next phase. No production stubs, disabled
validation or lowered assertions. Final local acceptance permits no carried
integration error.

If a performance gate is missed, diagnose remaining ownership/repeated preparation
before another local optimization. Do not add phases indefinitely or absorb excluded
work. A genuine semantic/API decision or scope expansion needs user direction.
On interruption, leave the ledger sufficient for another agent to resume.

The user authorized implementation and one commit per phase, not branches, pushes
or deployment. Use coherent Conventional Commits and a `Plan-Phase: R7-0N` trailer;
disclose carried build failures in the body. Do not rewrite user work.

## Progress ledger

| Phase | State | Evidence / next action |
| --- | --- | --- |
| R7-01 | Complete | 39 baseline samples pass; retained-state, bridge-tail and dual-accounting decisions recorded above. |
| R7-02 | Complete | Owned instance/layout/bridge state, short-lived planner, shared input lookup and borrowed products implemented. Existing driver temporarily uses lifecycle entrypoint; R7-03 removes it. |
| R7-03 | Complete | Delta delivery/scans, retained construction summaries and candidate barriers implemented; obsolete driver/entrypoint removed. Five no-regeneration cases and selected contracts pass. |
| R7-04 | Not started | Depends on R7-03; focused local acceptance and matched measurements, CI scope separate. |

R7-01 local checks passed: structure (1719 Rust files, no violations), formatting,
workspace all-target Clippy with warnings denied, and diff whitespace checks.

R7-02 checks passed: `cargo check -p kagari-compiler`; compiler test filters
`generic_interface` (3 cases), `lowering_honors_instruction_limits_and_cancellation`,
aggregate deduplication/shared-budget, growing recursive-layout limit, recursive
instance reuse and retained cross-module source-use removal (8 distinct cases).
Structure, formatting, workspace all-target Clippy and diff checks passed.
The new lifecycle is exercised by the existing driver; multi-step/no-regeneration
acceptance belongs to R7-03/R7-04. No carried build failures.

R7-03: replaced driver and migrated four internal test calls to the lifecycle.
Initial test/Clippy builds failed on removed `lower_to_mir_with_requests` imports;
the direct test migrations resolved them. Five no-regeneration cases passed when
run explicitly with `--ignored`, including late bridge wrappers and shared layout
binders. Eight `source::program::update_` cases passed. Additional selected checks:
source generic aliases, both private-layout cases, unused static initializer
obligations, portable foreign-default origins and registration-order-independent
MIR/bytecode products all passed. The existing lower/interface budget selection
and new closed-table layout/debit regression passed. Structure (1728 files before
the final regression), formatting, workspace all-target Clippy and diff checks
passed; the final regression also passed those checks.

Independent review found closure-added bridge tables incorrectly becoming layout
roots. Fixed by recording their inventory positions separately and skipping only
those positions during layout discovery. They retain the old post-module table
debit; explicit layout requests still add/charge layouts. The concrete private
`Key<i32>` shared-tuple test reproduced the faulty-worklist minimum generic limit
of 241 versus fixed-worklist 240; this is not a historical-revision timing claim.

Selected SDK checks (one `cargo test -p kagari --lib --` invocation with four
filters) passed: `shared_helpers_are_materialized_in_their_declaring_module`,
`composed_tuple_operation_witnesses_are_verified_without_source`,
`shared_nominal_layout_applications_are_verified_without_source`, and
`new_primitive` (exactly the current-operation reload case). These cover source-free
proof rejection and actual publication beyond compiler-only coverage.

A discarded shared-layout fixture exposed a pre-existing bytecode rejection:
a root shared default method calls a foreign generic `wrap<T>` returning `Box<T>`.
Both source-only and retained compilation reject with `invalid collection access
flow`. An identical probe reproduced both failures at baseline `71e72b02` and the
current source snapshot; no production fix or weakened assertion was used. The
scheduler contract uses a provider-owned shared default that constructs `Box<T>`
directly and passes both source-only/retained bytecode validation. Preserve the
original defect and minimal source in review at R7-04 closure. A direct tuple-to-Hash
coercion fixture also hit `InvalidPublicAbi` independently of the closed-table
layout fix; it is not the validated construction-evidence fixture.

Known intermediate implementation failures: none outstanding in the selected
scope. Five no-regeneration tests remain explicitly ignored until R7-04 activation.
Full CI acceptance is unrun, not passed.
