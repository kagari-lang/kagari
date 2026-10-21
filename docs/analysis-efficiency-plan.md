# Bounded analysis and MIR compaction execution plan

## Status and authority

**Implementation and one commit per phase are authorized.** Requested on 2026-10-21 after
reviewing R16-R18 at `4baef38e`. This document owns AE01-AE03, their finite scope,
acceptance and progress ledger. The [roadmap](implementation-roadmap.md#bounded-analysis-and-mir-compaction)
owns activation; [review](review.md#engineering-a-compiler-chapter-review) retains
the unresolved findings. Writing this plan authorizes no implementation, commits,
pushes or deployment. The subsequent user instruction activates AE01-AE03 and
phase commits, but not push or deployment.

The compilation/startup work is already closed, including direct source preparation,
retained verification evidence and graph-owned proof preparation. Preserve those
[architecture contracts](architecture.md#compilation-pipeline); do not restart CS.
Its unmet 500 ms compile/load goal, BO04 lifecycle gates and IP12 acceptance are
not acceptance targets for this task. No evidence establishes R16-R18 as their cause.

Follow [project goals](project_goal.md), [repository rules](../AGENTS.md), the
[artifact contract](spec/artifacts.md) and [debugger contract](spec/debugger.md).
When implementation and commits are authorized, execute the three buildable phases
in order, one coherent commit per phase, with a `Plan-Phase: AExx` trailer.
Use Conventional Commits and explain any breaking API change. Final integration
and documentation handoff belong to AE03, not an open-ended fourth tuning phase.

## Outcome and exclusions

Remove three demonstrated sources of repeated work without changing language or
executable semantics:

1. R17: query debug binding availability without scanning every range at every PC.
2. R16: propagate initialization and liveness in their respective directions without
   recomputing both through repeated whole-graph forward sweeps.
3. R18: apply a block's dead-operation deletion set by stable compaction rather than
   repeatedly shifting its retained suffix.

R17 supplies location facts; R16 schedules block facts. These are complementary,
not interchangeable optimizations. R18 is independent MIR transformation work and
is grouped here only because it addresses the same bounded-preparation objective.

Excluded: R7/R8 specialization or incremental reuse, R13/R14 lifetime/parking work,
R19 storage-class coloring, a generic dataflow framework, new SSA/IR, managed slot
coalescing, new optimizer passes or deletion eligibility, removing verification,
artifact pruning, persistent caches, JIT/AOT expansion and browser delivery tuning.
Do not fuse runtime allocation's solver with the verifier or change its coloring
policy as a side effect of sharing range queries. No format/ABI bump is planned.

## Evidence and ownership

Source and selected tests were inspected, not executed, while writing this plan.
The mechanisms below remain at the baseline; elapsed impact is unmeasured.

| Finding | Owner and present mechanism | Replacement boundary |
| --- | --- | --- |
| R17 | [Bytecode flow](../crates/kagari-bytecode/src/verifier/flow.rs), `inputs(pc)`, scans `local_live_ranges` during graph construction and forward/backward point checks. [Runtime allocation](../crates/kagari-runtime/src/module/execution/allocation.rs) scans the same table at every PC. | Bytecode-owned function-local range-query capability, used by both consumers. |
| R16 | `Flow::fixed_points` combines forward definite initialization and backward liveness in ascending-PC sweeps. `FlowResult` also supplies CFG edges to access analysis. | Separate direction-aware fixed points inside the existing verifier; preserve CFG and downstream facts. |
| R18 | [MIR pass driver](../crates/kagari-mir/src/passes.rs), `optimize`, reverse-sorts deletion triples then removes individually from instructions/spans/scopes. | One stable block-local compaction using the existing dead-operation decisions and seal lifecycle. |

Examples are algorithmic, not timing claims:

- With P instructions and D debug ranges, allocation currently inspects P*D range
  records even when only one range is active at a time. The replacement should pay
  for index construction, queries/events and actual active ranges, not P*D filtering.
- For `B0 -> B1 -> ... -> Bn -> return parameter`, without debug ranges seeding uses
  everywhere, forward liveness sweeps move the last use backward one edge per sweep.
  R16 removes that scheduling artifact; it does not make every CFG analysis linear.
- D dead operations before L retained operations cause D*L suffix moves in each of
  three vectors. Stable compaction visits the block once per aligned column instead.

Current limits distinguish policies: bytecode flow charges a shared program-level
100-million work budget and bounds analysis state/suspension retention; runtime
slot preparation uses a shared four-million work budget and safely declines reuse
on exhaustion. MIR passes use `PassOptions::max_work` plus verification's own limits.
Keep these policies and limit values, not the accidental old cost model.

## Design contracts

### R17: immutable range ownership, explicit traversal

Place the shared capability with bytecode metadata, not in generic `common` or a
runtime-to-compiler dependency. It borrows or owns only the exact function's admitted
range records. Mutation invalidates it; it is not serialized, process-global or a
new trusted-input flag. Keep malformed range rejection before indexed access, including
records in unreachable code. Review actual validation ordering before construction.

Choose a bounded interval index or sorted event representation after enumerating
consumer access patterns in AE01. Define half-open membership `start <= pc < end`,
empty ranges according to existing admission rules, adjacent/overlapping ranges,
repeated slots and deterministic output. A slot must remain active while any of its
admitted ranges covers the PC; one ending range cannot erase another live range.
Do not rewrite serialized debug record order or remove debugger names/ranges.

Graph construction and allocation scan forward; point checks also scan backward
inside blocks and jump between blocks. A forward-only cursor must not silently
restart from zero for each reverse query. Use direction-aware traversal or bounded
seeks, or precompute only the block facts actually needed by the solver. Worklist
revisits must not rescan the entire range table. Do not materialize a dense
instruction-by-range/slot table or keep all per-PC active sets merely to avoid scans.
A compact query representation may be rebuilt once per independent operation;
sharing code does not require retaining it in every loaded program indefinitely.

Charge construction, sorting, queries, active-output traversal and scratch memory
before unbounded growth. Respect each caller's existing cancellation/error policy;
new verifier work uses caller cancellation, never a fresh uncancelled token.
Keep runtime allocation's safe fallback, but do not use it to hide malformed inputs
or convert a propagated cancellation into successful executable admission.

### R16: two fixed points over the existing CFG

Retain one checked CFG and reachability owner. Initialization intersects reachable
predecessor outputs, with the entry parameter boundary preserved even with backedges;
liveness unions successor inputs and applies the existing use/definition transfer.
Keep read-before-write order for instruction and debug uses, await facts and host
capability rejection. Access analysis continues to consume the same checked edges.

Use separate deterministic, duplicate-suppressing worklists. Changed initialization
outputs schedule successors; changed liveness inputs schedule predecessors. Establish
an initial graph order appropriate to each direction rather than relying on numeric
PC layout. Seed all required reachable nodes, including loops with no exit: seeding
only return blocks is unsound. Preserve initial lattice states and convergence rules;
queue membership is not proof that a node's transfer has been evaluated.

Account for graph-order construction, queue state, edges, bitset words and scratch
buffers within existing bounded ownership. Preserve cancellation through initialization
as well as convergence. Keep point validation in deterministic source/PC order so
worklist order need not become diagnostic order. Do not remove ordinary-function
liveness in this task; changing which facts are computed is separate from scheduling.
Do not replace independent bytecode validation with MIR facts after slot/CFG changes.

### R18: stable deletion with aligned provenance

Use `dead::find` as the deletion-policy owner. Group its deletion set by function
and block with bounded work, then compact instructions, spans and scopes using one
consistent keep decision. Preserve exact retained order and metadata correspondence,
not merely equal vector lengths. Do not use `swap_remove`, add cross-block code motion,
change constant/copy pass ordering or clone entire modules to simplify mutation.

Prefer ordinary safe Rust. Charge deletion-set preparation and traversal/moves,
including retained entries, and poll cancellation in bounded chunks. An error drops
the owned unpublished intermediate module; it must never return a partially modified
seal. Existing seals are consumed before editing and full required MIR verification
rebuilds offsets, roots, safepoints and debug facts afterward. Leave the no-removal
path cheap. Any temporary allocation has an explicit size/lifetime justification.

### Budget compatibility

Semantic rejection rules stay unchanged. Work-limit outcomes need not be identical:
removing redundant work can admit valid inputs that previously exhausted the budget,
and accounting previously uncharged work can reject a budget too small for actual
work. Document these differences rather than increasing limits or weakening tests.
Validate cancellation, exhaustion and safe cleanup separately from semantic validity.
No fallback may publish unchecked code, and no new allocation may escape accounting
merely because the old implementation did not allocate it.

## Mature references and adaptations

- [Rust compiler dataflow guide](https://rustc-dev-guide.rust-lang.org/mir/dataflow.html),
  rolling documentation reviewed 2026-10-21: analysis direction, lattice boundaries,
  fixed-point ownership and separate cursor/visitor consumption. Adopt those
  responsibilities for R16/R17, not rustc's full generic framework or borrow checker.
  Kagari's existing [MIR analysis](../crates/kagari-mir/src/verify/analysis.rs) already
  has bounded worklists and explicit scratch accounting; reuse its design lessons,
  not its verified conclusions for a different bytecode graph.
- [Rust `Vec::retain`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.retain)
  and [`remove`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.remove),
  rolling standard-library docs reviewed 2026-10-21: stable single-visit filtering
  versus suffix shifting. R18 adopts stable compaction, but a single-vector API does
  not itself solve Kagari's three-column alignment or fallible cancellation contract.
- Cooper and Torczon, *Engineering a Compiler*, second edition (2012): the existing
  [chapter review](review.md#chapter-map-and-architectural-implications) records
  dataflow direction (§9.2), mark/sweep deletion (§10.2.1) and representation/index
  tradeoffs (Appendix B). No private PDF or previous chat is required to execute this
  plan. A bounded function-local interval representation is the specific adaptation;
  the references do not mandate one universal data structure or promise speedups.

## Phases and acceptance

### AE01 — R17 shared debug availability queries

- Confirm range admission, access patterns, budget/cancellation plumbing and the
  narrow cross-crate API. Record the selected representation and lifetime here.
- Implement the bytecode-owned query capability and migrate both flow validation
  and runtime allocation. Remove their superseded full-table filter paths.
- Reuse lexical-scope/async coverage; add distinct endpoint, repeated-slot, reverse
  traversal, sparse-range work and budget cases at the appropriate owner.
- Compare query results to a simple scan oracle in tests only. Keep the oracle
  bounded and independent; do not retain a second production implementation.

Acceptance: exact admitted active-slot sets in both consumers, unchanged malformed
metadata rejection, no P*D full-table scanning for sparse ranges, bounded temporary
state, and unchanged safe runtime fallback. Include actual construction/query work,
not just a helper rename or a wall-clock-only assertion.

### AE02 — R16 direction-aware bytecode flow

- Separate fixed points with explicit initial states, direction-appropriate initial
  ordering and changed-node queues. Retain existing CFG/access and point-check owners.
- Exercise straight chains and block-permuted chains, diamonds, backedges, unreachable
  predecessors and no-exit cycles; include both ordinary and suspending functions,
  with and without debug ranges.
- Compare facts on bounded small CFGs with a test-only straightforward solver or
  independent expected facts, plus existing semantic rejection contracts.
- Count actual node/edge/word work, queue storage and cancellation boundaries.

Acceptance: initialization and await live sets remain correct; unrelated stable
forward facts are not recomputed because backward facts changed. Fixed-width chains
and their layout permutations avoid the old quadratic scheduling behavior. Cycles
converge within the existing budget or reject safely; no universal linear claim.

### AE03 — R18 stable compaction and focused handoff

- Replace repeated deletion shifts with stable aligned compaction; remove the old
  loop. Preserve deletion statistics, eligibility and fresh verification.
- Cover dead-prefix/live-suffix and alternating removals across multiple blocks,
  no/all removable instructions, exact provenance, checked traps and retained calls.
  Extend cancellation/limit coverage to reach compaction, not just an already-cancelled
  entry. Add only cases not established by existing pass contracts.
- Complete the small cross-phase regression selection and matched measurements below;
  repair related failures without reopening CS/BO/IP tuning.
- Move lasting ownership contracts to architecture/debug documentation and measured
  results to the performance baseline. Remove resolved R16-R18 entries and repair
  their direct references without removing R19 or other pending findings. Remove this
  completed plan after handoff; Git retains the ledger, roadmap retains pending CI.

Acceptance: compaction work is proportional to affected block entries plus deletion
set preparation, not dead-count times live-suffix length. All semantic/structural
checks in the selected local scope pass. Report actual remaining costs and CI limits;
no speed claim or completion claim follows solely from fewer instructions/moves.

## Focused verification and measurement

This is a selection menu, not a suite to rerun in every phase. The named commands
were source-inspected, not run for planning. Confirm filters select nonzero tests.

| Contract / phase | Existing command or test owner |
| --- | --- |
| AE01/AE02 suspension, debug-only host liveness | `cargo test -p kagari-bytecode --lib verifier::async_tests` |
| AE01 lexical endpoints | `cargo test -p kagari-compiler --lib debug_local_ranges_close_when_their_lexical_block_ends` |
| AE01 malformed metadata | `cargo test -p kagari-compiler --lib verifier_rejects_malformed_debug_metadata` |
| AE01 physical allocation/backedges | `cargo test -p kagari-runtime --lib module::execution::allocation::tests` |
| AE02 entry/join initialization | `cargo test -p kagari-compiler --lib ordinary_slot_initialization_requires_the_first_entry_and_every_join_edge` |
| AE02 source-free branch contracts | `cargo test -p kagari-compiler --lib predicate_branches_verify_operation_targets_and_initialization_without_source` |
| AE03 passes, effects, provenance and limits | `cargo test -p kagari-compiler --lib tests::passes` |
| Final debugger/source-artifact regression | `cargo test -p kagari-vm --lib optimized_breakpoints_preserve_branch_paths_and_binding_state` |

Add narrow owner-local tests for the uncovered algorithmic cases above; record their
actual names here when implemented. Source-free malformed bytecode coverage must
remain independent of HIR inference. Preserve existing roundtrip/debug coverage;
regenerate disposable fixtures only if affected, at a coherent checkpoint.

Each implementation checkpoint requires manual ownership/import/visibility review
and these local checks under repository rules:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Use ordinary modules and narrow cross-crate APIs; no forwarding re-exports or blanket
structure exceptions. Apply the 1200 effective-LOC rule. Full workspace tests and the
complete feature/backend/platform matrix remain CI-only. Select affected tests, use
`--no-fail-fast` for multiple selected targets, and rerun only failing/newly affected
checks after repairs. Documentation-only edits need content/link/diff checks, not builds.

Performance acceptance is algorithmic first, elapsed measurements second:

- Before changing each algorithm, capture a small bounded baseline from the same
  contract workload. No broad profiling campaign is required before implementation.
- Use three increasing sizes for sparse debug ranges, fixed-width CFG chains/layout
  permutations and dead-prefix/live-suffix compaction. Include one small ordinary
  control to expose setup overhead. Freeze sizes before comparison; record actual
  ranges, active overlap, blocks, edges, words and removed/retained entries.
- Count real visits/moves as well as charged work, so changing accounting alone
  cannot establish an improvement. State algorithmic bounds including sorting and
  output enumeration. Bound scratch/retained memory separately; dense overlap has
  unavoidable active-output cost.
- At final integration, run matched native release measurements for the targeted
  operations and one representative direct source/explicit artifact path. Separate
  lowering, verification/preparation and execution; build before timing, collect
  at least five samples per condition, record medians/ranges and explicit warmups.
  Record revision, toolchain, machine, profile/features, default parallelism and
  cache state. Keep probes under ignored `target/`, durable methods/results in docs.
- No absolute browser latency goal is introduced and no old CS result is reclassified.
  Full browser/backend benchmarking is not required for these local algorithms;
  platform/feature coverage stays CI-owned. If focused timing regresses materially,
  diagnose the changed path before claiming success; make substantial new scope
  explicit instead of continually extending this plan.

## Resumption and progress ledger

On another machine, use this plan and linked tracked files, not old chat state.
Inspect current HEAD, status and affected diffs before starting; the baseline hash
is not a reset instruction. Follow repository-managed toolchains, and remeasure
before/after on the same machine rather than comparing different machines' times.
Confirm authorization and resume the first incomplete phase. Update this ledger
with decisions, selected tests, actual commands/results and carried errors. No
parallel decision queue is needed.

| Phase | Status | Next action / evidence |
| --- | --- | --- |
| Plan | Drafted | Current source mechanisms and focused contract owners inspected; no implementation or measurements performed. |
| AE01 | Complete | Borrowed balanced max-end interval index; verifier and runtime allocation migrated. Focused query, async, allocation, lexical range and malformed metadata tests pass. |
| AE02 | Not started | Separate initialization/liveness scheduling over the checked CFG. |
| AE03 | Not started | Stable MIR compaction, focused integration and documentation handoff. |

AE01 decisions: `availability::DebugAvailability` owns O(D) scratch borrowed from
one function; balanced start ordering and subtree maximum ends prune arbitrary-PC
queries, including backward point checks. A fallible heapsort polls the caller's
budget/cancellation inside sorting. Source record order remains unchanged. Scratch
is checked against each consumer's existing state bound before construction.

AE01 evidence (deterministic counts, not elapsed speed claims): for D = 16/256/4096
sparse ranges and 2D reverse-order queries, the old filter inspects 512/131072/33554432
records. New build charges are 177/5700/139887 and query charges 181/5893/143365,
including actual visited nodes/output work; scratch is 384/6144/98304 bytes on x64.
The runtime's 10000-PC/500-range case now retains allocation reuse rather than
exhausting four million units; an exhausted budget still selects the safe fallback.

AE01 validation passed: `cargo test -p kagari-bytecode --lib availability::tests --
--nocapture` (3), `cargo test -p kagari-bytecode --lib verifier::async_tests` (3),
`cargo test -p kagari-runtime --lib module::execution::allocation::tests` (2 existing),
then `sparse_debug_ranges_preserve_reuse_and_budget_fallback` under the same package
(1). Compiler filters `debug_local_ranges_close_when_their_lexical_block_ends` and
`verifier_rejects_malformed_debug_metadata` passed (1 each). Structure check, format
check, workspace all-target Clippy with denied warnings and diff check passed.
An initial test-fixture compile error used `Default` for `LocalLiveRange`, which has
no such implementation; explicit fields repaired it before the successful runs.
No carried integration error remains. Full feature/platform/workspace tests remain
CI-only; AE02/AE03 and final matched timing are pending. Historical CI/BO/IP/CS limits
remain at their existing owners.
