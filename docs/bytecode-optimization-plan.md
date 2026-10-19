# Bytecode optimization execution plan

## Status, priority and ownership

**Status: active, implementation and one commit per phase authorized on 2026-10-19;
BO01 baseline/design completed; BO02 is next.**
The user explicitly requested a separate plan that can proceed before the previous
interpreter performance work finishes. This document owns BO01-BO04: general typed
slots, direct destinations, predicate branches and constant operands. It replaces
the proposed instruction-model extension in the older plan, not its retained work.

BO01 may proceed without completing IP12 or obtaining another IP12 disposition.
The [previous interpreter plan](interpreter-performance-plan.md) retains ownership
of its outstanding lifecycle collection timing gate and historical results. This
plan neither diagnoses nor closes that gate. Freeze a new current-revision baseline
for this work; no old benchmark image, rerun or collection repair is an entry
requirement. Existing accepted call/window/numeric foundations remain in use.

Independence is in scope, scheduling, baseline, acceptance and progress ownership,
not permission to ignore regressions in shared code. Collection/lifecycle workloads
below check this migration against its own baseline, not the old IP12 baseline.
Do not add collection optimizations to pass them. Failure requires diagnosis of the
changed representation, repair/rejection, or an explicit scope decision here.

The [roadmap](implementation-roadmap.md#bytecode-optimization) owns priority;
[architecture](architecture.md#compilation-pipeline) and the
[execution specification](spec/execution.md) own current contracts. This plan owns
the only bytecode-optimization ledger. The user's subsequent instruction activates
BO01-BO04 and authorizes one coherent commit per completed phase, with the phase
trailer below. No additional phase activates automatically after BO04.

## Objective and scope

Remove mandatory local transport and control-only Boolean materialization through
a general interpreter slot/operand/control-flow model. Numeric-for specialization
is not the first step. Source inspection supports the work reduction; current
baseline timing and machine-code attribution must establish its elapsed benefit.

In scope: interpreter-target placement from verified MIR; portable operand, debug
and program-point contracts; their independent verification, encoding/disassembly
and canonical MIR correspondence; runtime physical preparation and all affected
frame/debug/call/await consumers. Existing numeric semantics, scalar/managed banks,
window authority, GC and invocation lifecycle remain foundations.

Out of scope: a new source type system or general SSA rewrite; loop recognition or
closed-form evaluation; JIT/AOT feature expansion; numeric-kernel ABI/dispatch
replacement; a universal Value layout; collection/Option changes; a second GC;
unsafe execution; cancellation or slice-policy changes. Required native input and
origin correspondence updates are integration work, not native loop support.

## Recommended design at a glance

**Recommendation: replace mandatory local/register transport with a unified typed
logical-slot model, then add general predicate branches and constant operands.**
Do not begin with numeric-for opcodes, byte packing or removal of runtime checks.
The current scalar cursor already reads prepared scalar operands and selects typed
kernels; the missing capability is better placement and control-flow representation,
not runtime type specialization from scratch.

The implementation pipeline remains:

```text
verified semantic MIR (locals, temporaries, effects, explicit commits)
  -> deterministic interpreter placement and instruction selection
  -> portable typed slots + instructions + binding/origin metadata
  -> independent bytecode verification
  -> runtime physical allocation and operand preparation
  -> existing interpreter driver and scalar/managed execution
```

These are responsibilities in existing owners, not six new IRs or subsystems.
Keep MIR's semantic local/temporary distinction where useful; remove the requirement
that bytecode and physical storage reproduce that distinction. Bytecode lowering
chooses value placement; runtime allocation chooses physical banks and reuse.

For the review's i64 sum, the intended executable shape is the following
**pseudobytecode**, not finalized Rust types, wire encoding or promised output:

```text
; slot 0: parameter n, slot 1: local s, slot 2: local i
constant       s1, i64(0)
constant       s2, i64(0)
check:
branch_compare i64.lt, s2, s0, body, exit
body:
binary_checked i64.add, s1, s1, s2
binary_checked i64.add, s2, s2, const(i64(1))
jump           check
exit:
return         s1
```

The two arithmetic operations read inputs before committing their destinations;
failed checked arithmetic leaves that destination unchanged. The original less-than
predicate with explicit successors also generalizes to float comparisons without
incorrectly rewriting the complement of a NaN comparison. A materialized comparison
remains necessary when its Boolean result is used as data. The seven displayed
operations are a structural illustration, subject to the observation rules below.

| Change | Removed work | What it does not imply |
| --- | --- | --- |
| Direct local operands and destinations | Eligible local loads and stores around assignments and arithmetic | No elimination of snapshots needed across writes, calls, captures or observation |
| Predicate branches | Temporary Boolean storage and a separate control-only branch operation | No change to evaluation order, NaN behavior or observable Boolean values |
| Prepared constant operands | Dedicated constant slots/loads where profitable | No per-iteration constant-table lookup, source inference or eager managed allocation |
| Unified physical placement | Storage separation solely because a value has a source name | No merging of overlapping lifetimes, semantic types or GC identities |

Start placement conservatively: prove local forwarding and direct destination
eligibility before removing a move; retain a scratch value when the proof is absent.
For `x = x + f()`, a call that mutates captured x must not change the already-read
left operand. For `s = s + i; i = i + 1`, a failing increment must retain the completed
s assignment. This is why replacing LoadLocal/StoreLocal with aliases mechanically
is insufficient. An across-the-board SSA rewrite is not required.

The principal difficulty is cross-crate contract migration, not writing arithmetic
handlers. BO02 therefore replaces one coherent format and its consumers; BO03 adds
operand/control-flow improvements on that foundation. Existing polling and slicing
contracts remain in force. A cancellation feature is a separate decision, and any
required interior continuation can limit fusion. A dedicated counting-loop opcode,
new numeric-kernel ABI or further check elimination requires later evidence and scope.

## Reference versions and evidence

These mechanisms were consulted for the earlier investigation on 2026-10-17 and
are retained as architectural references, not measurements of this migration.
Lua source pages track the 5.4 series; the inspected source identified itself as
5.4.9. Rust references are pinned to 1.90.0 and the JVM specification to SE 25.

| Reference | Applicable mechanism | Kagari-specific boundary |
| --- | --- | --- |
| Lua 5.4.9 [opcodes](https://www.lua.org/source/5.4/lopcodes.h.html) and [interpreter](https://www.lua.org/source/5.4/lvm.c.html) | Register destinations, constant operands and comparison-controlled jumps | Retain checked arithmetic, explicit roots and generation-pinned calls; do not copy raw-stack/upvalue ownership. |
| Rust 1.90.0 [MIR VarDebugInfo](https://github.com/rust-lang/rust/blob/1.90.0/compiler/rustc_middle/src/mir/mod.rs#L1231-L1289) | Source binding identity separated from execution location | Preserve Kagari local availability rather than introducing optimized-out values; no Rust borrow model or general SSA rewrite. |
| JVMS SE 25 [verification](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-4.html#jvms-4.10) and [typed instructions](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-2.html#jvms-2.11.1) | Establish type/initialization facts before execution and consume them within a valid boundary | Adapt to mutable slots, scoped host access and GC windows; neither JVM operand stacks nor wrapping arithmetic are the proposed model. |

## Target model and responsibility boundaries

Use the [versioned mature references](#reference-versions-and-evidence), especially
Lua 5.4.9 register destinations/constant operands and Rust 1.90.0 debug-location
separation. Borrow JVM verification discipline, not its operand stack or wrapping
arithmetic. Lua raw-stack/upvalue ownership and Rust optimized-out debug values do
not satisfy Kagari's window, root, reload or local-availability contracts. Record
any departure and its concrete requirement during BO01; citations are not proof of
correctness or speed.

| Owner | Proposed responsibility |
| --- | --- |
| MIR | Preserve checked types, once-only evaluation, effects, explicit commits, lexical bindings and source/program-point facts. Keep local loads/stores where useful to semantic analysis; do not make source lowering guess physical slots. |
| Compiler bytecode lowering | Deterministically map MIR locals/temporaries to typed interpreter value locations; introduce snapshots where writes/calls invalidate an earlier read; select direct destinations and control-only comparison branches from checked facts. |
| Bytecode/contract metadata | Own the portable slot, parameter-entry, operand, branch, binding-location and origin records at their existing boundaries. A logical slot is not a runtime address or an ownership lease. |
| Bytecode verifier | Independently validate shape, types, initialization, legal successors, debug availability and bounded analysis for the replacement model, including source-free inputs. |
| Runtime preparation | Assign physical scalar/managed locations with interference, debug and suspension liveness; prepare operand/kernel access and immutable point mappings once. Do not infer source semantics again. |
| Runtime/VM | Consume one prepared model for ordinary, observed, sliced, reentrant and resumed execution. Publish exact PCs, roots and pinned dependencies at existing effect boundaries. |

Prefer one typed logical slot space for interpreter parameters, local storage and
expression temporaries. Source binding identity and availability map to that space;
being named must not itself require a second execution copy. Maintain separate
physical scalar/managed banks, full managed identities and explicit capture cells.
The same model must represent every existing instruction consumer even when its
placement remains conservative. This is not permission to alias arbitrary reads
with future writes or aggressively coalesce managed values in the first checkpoint.

Operands conceptually read a slot or an admitted constant; destinations identify
writable slots. These are design concepts, not prescribed Rust type/opcode names.
Keep semantic width, operand order and comparison predicate explicit. Existing
arithmetic destinations may be sufficient: do not multiply type/local/immediate
opcode combinations before comparing complete prepared-record and mapping costs.
Keep portable encoding distinct from hot prepared representation; no four-byte
instruction requirement or stored-byte/dispatch-count equivalence is assumed.

Typed logical slots carry semantic facts, not only physical ValueType tags. Current
[SemanticSlots](../crates/kagari-contract/src/slots.rs) is shared with MIR and has
separate local/register maps; compiler lowering currently clones it. Define an
explicit projection from MIR facts to bytecode slot metadata and wire records.
Renumber full Ty/access facts and parameter-entry mappings together; equal physical
representations do not establish interchangeable semantic types. Do not accidentally
change semantic MIR merely to reuse its metadata layout for interpreter slots.

## Semantic decisions required before replacement

BO01 must resolve and document each row below, including proof producer, consumer,
validity interval and invalidation boundary. A statement that values are verified
or the interpreter is trusted is not sufficient.

| Contract | Required decision and representative failure |
| --- | --- |
| Snapshot versus mutable storage | Preserve an earlier operand across later writes, calls and reentry. In `x = x + f()`, if f changes captured x, the left operand remains the value read before f. Captured mutable variables retain cell identity. |
| Read-modify-write and commit | Read all inputs before overwriting an aliased destination. Checked `x + 1` failure leaves x unchanged. In `s = s + i; i = i + 1`, failure of the increment retains the completed s update. Do not merely alias a result to a local while leaving its canonical commit at a later store. |
| Debug identity and location | Separate binding identity, lexical availability and current execution location. Preserve shadowing, parameter values, initializer invisibility before success, values after their last computational use and rooted snapshots. No permanent mirror store or newly optimized-out locals. |
| Program points and observation | Distinguish MIR origins, portable instruction ordinals and prepared locations. Specify pre/post-commit state, traps, suspended caller points, branch targets and interior continuations. Renumbering storage ordinals is not permission to remove a contractual observation point. |
| Scheduling and cancellation | Inventory the actual observer, poll and nonzero-slice guarantees, including current Frame PC/write APIs. A mapping alone cannot preserve an interior pause. Remove only noncontractual transport points; retain required continuations/polls, or defer that transformation. Do not silently weaken polling, change slices into execution charging, replay effects or add a second debug interpreter. |
| Verification | Extend the existing bounded bytecode flow owner for ordinary replacement-model functions, not only resume bodies. Inputs to an in-place write must already be initialized on every incoming path, including first loop entry. Check typed constants, slot/count bounds, read-before-write, legal CFG and debug ranges. Resource exhaustion fails verification; it must not bypass checks. Migrate access-flow facts as well as initialization: writes invalidate stale constant/access facts, and CFG joins conservatively merge them. Retain runtime guards unless their full producer-to-consumer proof is separately established. |
| Roots and suspension | Include debug-visible values in interference/liveness. Preserve a shared physical managed location if any logical alias is live; dead-slot clearing must not erase a live local. Keep call operands rooted until result publication, including `x = make(x)`, callbacks, collection and await. |
| Predicates and constants | Branch on the original predicate and explicit polarity; `!(NaN < n)` is not `NaN >= n`. Keep materialized bools when used as values. Constants retain source-width overflow/shift behavior, exact float bits and runtime-local string materialization. |
| Canonical correspondence | A portable MIR round trip must deterministically reproduce bytecode, constants, metadata and origins. Native trap/poll origins map through the same authoritative facts; neither hashes alone nor independent validity replaces exact correspondence. |

If preserving an observation/commit contract requires a scratch value or a retained
move, retain it. The goal is to eliminate mandatory transport, not all temporary
values. The seven-instruction example in the review is an explanatory structural
target, not an exact instruction-count test or a reason to weaken a boundary.

## BO01 — Freeze a current baseline and settle the execution contract

Entry: the independently prioritized scope above; no IP12 completion dependency.
Before editing executable production code:

- Recheck the current source-to-MIR, bytecode, prepared-record and release machine
  code for the review's i64 sum and existing arithmetic/branches workloads. Record
  loads/stores, executed handlers, physical slots, hot-record/mapping bytes and
  checks actually surviving compilation. Do not relabel historical counts as HEAD.
- Freeze ordinary original/source-form/lifecycle baseline images with the existing
  harness, same lockfile/toolchain/features, workspace release profile, default
  target and build parallelism. Record image/workload identities, machine, cache
  state and missing attribution. Do not rebuild while timing or instrument ordinary
  measurements. Use bounded command deadlines; a timeout is incomplete evidence.
- Resolve the contract table and choose deterministic slot/parameter/debug/origin
  records and transfer rules. Review all consumers, not only the scalar loop.
  Compare direct-slot versus continued local/register operands; record why the
  selected model avoids duplicate storage and instruction-family growth.
- Fix the exact contract-test selection and acceptance budgets below before edits.
  If attribution or the program-point design does not justify this migration, stop
  at the design result; do not substitute a smaller unrelated optimization.

Deliverable: one reviewed decision in this ledger, baseline identities and the
finite affected-owner list. No new production IR/cache/profiling framework is
required for this checkpoint. Record only this migration's evidence and decisions.

## BO01 implementation decisions — 2026-10-19

### Executable boundaries, not historical micro-steps

Observer events, per-instruction cancellation/GC checks and nonzero instruction
slices consume the **final verified executable's actual instructions**. Deterministic
compilation may remove noncontractual transport and choose a direct-destination
instruction with its own real commit. Recompilation does not preserve internal PC
numbers, observer counts, cancellation event ordinals or drive counts. This is not
permission for runtime preparation to skip boundaries still present in its input.

The evidence is the current [observer contract](../crates/kagari-runtime/src/session.rs),
[driver](../crates/kagari-vm/src/executor/driver.rs),
[scheduling contract](spec/execution.md#owned-interpreter-driving) and
[debugger safe points](spec/debugger.md#safe-debug-points), which explicitly do not
require arbitrary pauses inside every VM instruction. Preserve statement/call/loop
states and exact failure/commit order, not one event for every old temporary read.
Multiple redundant debug records for the same source span may describe the new
boundary only when their visible binding state agrees. Otherwise keep the move.

This distinction avoids adding PendingStore/DecidedSuccessor phases solely to
simulate a deleted instruction. Existing call/await continuations remain necessary.
In particular, a suspended caller reports its call origin, an Await starts once,
and a checked result is committed only on success. No separate observed interpreter,
runtime fusion of existing instructions or cancellation-feature change is selected.

### Slot, placement and proof contract

- One checked logical slot ID space represents parameters, source storage and
  temporaries. Use a nontruncating ID/count capable of representing combined former
  local/register counts, with bounded decoding. A slot has a physical representation
  and all existing full semantic/access facts; representation equality alone never
  authorizes aliasing. Preserve capture-cell roles and readonly-weakening moves.
- Keep semantic MIR unchanged. Compiler bytecode lowering owns a separate full
  semantic-slot projection, parameter-entry map and binding locations. Callable
  generic/protocol/result facts retain their present owner. Synthetic missing Ty
  facts do not become unchecked Any or permission to drop access validation.
- Deterministic initial placement uses local homes followed by temporary IDs in
  emission order. Forward only block-local single-definition reads whose uses are
  proven unaffected by writes, calls/reentry, suspension and required debug states.
  Clear forwarding at block entries/barriers; retain snapshots if uncertain. Do not
  replicate MIR's existing general copy optimizer or introduce phi/SSA machinery.
- Redirect a scalar producer's sole adjacent store use only when full types agree
  and required debug states permit one atomic compute-and-commit instruction.
  Read inputs before writing; failure preserves the old destination. Do not redirect
  calls, awaits, managed/cell or compound path writes as part of this first pass.
- Compute final block PCs after selection. The canonical native-input reproduction
  invokes that same deterministic lowering, including constants, semantic projection,
  origins and debug metadata; retain exact canonical equality, not just hashes.
- Extend the existing bounded CFG verifier to ordinary synchronous functions. Entry
  initializes only admitted parameters; incoming initialization meets by intersection;
  check reads before defining writes. Access/constant facts are per-CFG-state: writes
  invalidate old facts and joins conservatively meet reachable inputs. Integrate
  this into existing verification owners, not a parallel verifier. Exceeding the
  validation work budget rejects input, whereas allocation may safely decline reuse.
- Runtime allocation includes binding availability and suspension liveness, retains
  scalar/managed banks and full identities, and conservatively avoids initial managed
  coalescing. Any live logical alias keeps its managed physical location rooted.
  No steady-state per-call map building, new semantic cache or duplicated numeric
  implementation is authorized.

### Origins, native products and external frame entry

Separate final executable PCs from semantic MIR origin IDs. Preserve function-local
MIR ordinals as origin identities for native polls/traps; put the required bounded
origin/PC correspondence in executable metadata, not only optional portable MIR or
strippable debug names. Many origins may share a selected instruction, but removed
origins cannot silently be interpreted as unrelated in-range PCs. Source-free AOT
must independently validate native manifest references against this same metadata
before loading code. Update actual generated poll arguments, runtime point validation
and error traces together; do not simply drop old count/continuity checks.

Migrate public Frame slot/PC APIs to the actual new executable, retaining initialized,
range, runtime/window and owner checks. No old PC or removed register is guaranteed
across compilation. A manually redirected frame does not acquire normal-entry
verification proofs; this migration does not remove defensive runtime reads. Keep
separate next-execution and suspended/executing location state.

### Fixed validation and budget selection

Retain the correctness selection and performance budgets below. BO02 selects compiler
analysis/codec/program-point and bytecode-validation owners, allocation, optimized
execution, debugger, suspended caller, owned-drive, numeric, representative GC/capture
and pinned-code tests. Add only distinct missing source-free flow/constant/access,
snapshot/failed-commit and changed-origin boundaries at those owners. Native/AOT
selection includes real native checked-overflow origins and source-free malformed
mapping rejection, not fallback. BO03 reruns changed numeric/branch/origin cases;
BO04 reuses unaffected passing evidence. Full workspace/backend/platform tests stay
CI-only. Required implementation structure/fmt/workspace Clippy checks remain local.

Budgets are now fixed before executable edits: arithmetic <=0.90, branches <=0.95;
other execution/lifecycle rows <=1.05, diagnostic setup <=1.10, executable bytes
<=1.02 and complete retained representation <=1.10 against BO01. Failed gates require
bounded attribution and a recorded disposition, not relaxed thresholds or unrelated
optimizations. Lua parity and historical IP12 remain separate.

## BO02 — Replace the slot/placement contract end to end

Replace the portable local/register execution split with the selected typed slot
model across compiler lowering, verifier, codec, disassembler, runtime preparation,
parameter/call/return/await destinations and debug/frame access. Keep MIR semantic
facts and one shared numeric implementation. Implement conservative placement for
all value categories; remove plain scalar local transport where snapshot liveness
and commit/observation rules permit it. Keep necessary managed/cell/scoped moves.

Update program-point/debug mappings and canonical native-input reproduction in the
same coherent checkpoint. Include these existing coupled consumers explicitly:

- [Access verification](../crates/kagari-bytecode/src/access.rs) currently assumes fixed
  register identities and tracks constants used for tuple indexing. Replace that
  assumption for mutable slots; preserve readonly/scoped-reference restrictions and
  invalidation/join rules, rather than relying on initialization verification alone.
- [Compiler emission](../crates/kagari-compiler/src/bytecode.rs) currently uses MIR
  block offsets and clones semantic slot metadata. Rebuild destination offsets and
  semantic maps after selection, and update both bytecode metadata wire records and
  canonical reproduction without changing semantic MIR's ownership.
- [Native emission](../crates/kagari-codegen-cranelift/src/emit.rs) uses MIR logical
  offsets for generated polls and trap/safepoint metadata. Update actual emitted
  origins as well as descriptors, and their
  [runtime validation](../crates/kagari-runtime/src/backend/native.rs).
- [AOT loading](../crates/kagari/src/aot.rs) and
  [manifest validation](../crates/kagari-contract/src/native_bundle.rs) currently
  constrain safepoints to bytecode instruction counts/continuous indices. Replace
  those one-to-one assumptions with the selected mapping contract; source-free
  deployment must retain and independently validate the needed mapping without MIR.

Extend the existing bytecode flow verifier as specified; do not delete physical
initialization or owner/generation checks as a side effect.
Preserve checked limits when replacing counts/IDs. Review frame public mutation/PC
entry points rather than assuming all execution starts at an untouched entry PC.

This is one cross-crate contract replacement, not a permanent scalar-only second
format. All retained checkpoints must build and pass their selected contracts.
Edits may span owners before a checkpoint, but no production stubs, disabled
validation, forwarding compatibility APIs or duplicate old execution engine may
be committed to make an intermediate state appear complete. If unexpectedly large
integration work is needed, record its cause and seek a scope decision here.

Regenerate affected disposable fixtures once the contract is coherent; preserve
source-free negative/behavioral coverage. Do not bump unpublished format/runtime
identifiers solely for the internal migration. Retain identifiers and invalidate
stale products according to repository policy.

Exit: source and decoded artifacts share the new representation; ordinary and
observed/sliced paths produce the required states; scalar examples lose redundant
transport without early commits. Record a focused arithmetic/branches paired
comparison and setup/footprint results against BO01, but reserve final optimization
acceptance for BO04.

## BO03 — Add general predicate branches and bounded constant operands

Use the same slot, numeric, origin and verifier model, not a scalar-loop recognizer.
Lower a comparison with only a control-flow use directly to a predicate branch;
keep comparisons that produce observable bools. Preserve operand evaluation order,
NaN semantics, fallthrough/target checks and all required observation boundaries.

Select slot/constant operand forms once for the affected numeric operations. Prefer
existing canonical scalar constants before introducing a new literal encoding;
add immediates only if BO01's bounded domain/encoding and footprint analysis support
them. Share semantic width checks and preserve exact float bits. No arithmetic
semantic copy, special increment opcode or exhaustive type/operand Cartesian product.
Record a justified immediate deferral if constants already address the concrete
requirement; direct destinations and predicate branches remain required scope.

Update use/def, CFG, initialization, codec/disassembly and mapping consumers together.
Recheck canonical reproduction, emitted native poll/trap origins and source-free AOT
mapping validation after fusion/constant-load removal; BO02 mapping acceptance alone
does not cover these further instruction-count changes.
Use the existing decode/prepare owner rather than per-execution operand resolution.
Exit: non-loop assignments/conditions benefit as well; zero/negative bounds, value
bools, mixed constant operands and effectful failures retain semantics. Compare the
new arithmetic/branches candidate with BO01 using unchanged fixtures; do not demand
an exact seven-opcode dump or claim a handler ratio is a timing ratio.

## BO04 — Integrated acceptance and handoff

Evaluate the retained BO02/BO03 model against the same BO01 baseline with original,
source-form and owned-lifecycle suites. Reuse passing unaffected correctness
results; complete the selected cross-boundary cases below. Report separately:
structural work removed, actual execution ratios, preparation/metadata/frame cost,
local correctness, pending CI and Lua comparison diagnostics. Neither historical
IP12 acceptance nor full Lua parity is a completion gate for this bounded migration.

A failed gate triggers one bounded diagnosis of the changed model, followed by a
recorded repair, rejection or explicitly authorized scope decision. Do not stack
handler/polling/collection shortcuts, relax limits after seeing results, extend
samples or create new phases until this migration has a disposition. Preserve user
work and earlier accepted foundations if rejecting only the new candidate.

After acceptance, move implemented location/point/verification contracts to their
architecture/spec owners and measurements to the performance document. Update the
roadmap/review rather than leaving an implemented proposal queued. Retire this plan
when its own required work is handed off; outstanding IP12 acceptance does not keep
it open. Authorized implementation commits use
`Plan-Phase: BO01` through `Plan-Phase: BO04` trailers for their respective checkpoints.

## Focused correctness selection

The following commands select existing owners, not a full workspace/package sweep.
BO01 records actual selected test counts; a filter matching zero tests or an early
build failure is not a pass. Run only the commands relevant to a checkpoint; batch
related repairs and rerun affected/failing cases rather than restarting all checks.

```text
# BO02: placement, origins, canonical/source-free contracts and physical liveness.
cargo test -p kagari-compiler --lib tests::analysis::
cargo test -p kagari-compiler --lib tests::codec::
cargo test -p kagari-runtime --lib module::execution::allocation::tests::
cargo test -p kagari-vm --lib optimized_execution_preserves_results_traps_and_source_origins
cargo test -p kagari-vm --lib debug_session_resolves_breakpoints_and_inspects_live_locals
cargo test -p kagari-vm --lib nested_breakpoints_and_traps_include_the_suspended_host_caller
cargo test -p kagari-vm --lib async_owned_drive_contract

# BO02/BO03: all numeric widths and source-free routes; new predicate/operand cases
# belong here, not in a parallel opcode-smoke suite.
cargo test -p kagari-vm --lib tests::numeric::

# Representative managed, captured-state and pinned-code dependents of placement.
cargo test -p kagari-vm --lib frame_roots_preserve_returned_objects_across_calls_and_collection_safepoints
cargo test -p kagari-vm --lib closures_keep_captured_objects_and_cells_alive_across_collection
cargo test -p kagari-vm --lib rooted_closure_retains_its_old_program_after_new_publish
```

Use [compiler verification](../crates/kagari-compiler/src/tests/verify.rs),
[numeric execution](../crates/kagari-vm/src/tests/numeric.rs),
[debugger](../crates/kagari-vm/src/tests/execution/debugger.rs) and
[bytecode flow](../crates/kagari-bytecode/src/verifier/flow.rs) as existing owners.
During BO01 name the exact additional source-free verifier, disassembly and native
origin/AOT cases selected after the affected records are known. Include overwritten
constant facts and incompatible access/type facts at CFG joins. Replace the existing
[program-point tests](../crates/kagari-compiler/src/tests/program_points.rs) that
assume equal MIR/bytecode ordinals with assertions of the actual mapping contract.
New negative cases must exercise the independent bytecode boundary without relying
on MIR rejection.
Do not mistake native fallback for native origin coverage or activate an exhaustive
backend matrix locally.

Extend those owners only for uncovered boundaries: first-entry/read-before-write
initialization and typed constants; snapshot survival across a later write/call;
failed direct assignment with prior completed effects; NaN branch polarity;
initializer/last-use debug availability; slice-one resume with no replay; and live
managed aliases across await/collection. Reuse existing cases for all equivalent
rules. Assert observable results, effects, state/origins and cleanup; do not mirror
private layout or prescribe one opcode count as semantic correctness.

For each implementation checkpoint run structure, formatting, workspace all-target
Clippy and diff checks under repository policy. Full-workspace tests, complete
feature/backend/platform coverage (including browser products) remain CI-only.
This planning-only edit requires content/link/diff checks, not Rust builds.

## Performance budgets and reproducible measurement

These are proposed go/no-go budgets, not predicted or measured gains. Confirm them
during BO01 before executable edits; document any evidence-based change before
measuring a candidate, never relax a failed gate afterward.

| Dimension | BO04 acceptance against the frozen BO01 baseline |
| --- | --- |
| Intended work | Demonstrably remove eligible local transport and control-only bool materialization in the review probe and existing workloads; report residual moves and why they remain. Inspect release code, not just emitted opcode names. |
| Warmed execution | Paired median candidate/baseline `arithmetic <= 0.90`, `branches <= 0.95`; every other compared execution and owned-lifecycle row `<= 1.05`. These are optimization investment thresholds, not Lua parity. |
| Setup | Matched diagnostic total setup median per process `<= 1.10`; separately report compilation, verification and preparation where available. Do not present the setup diagnostic as a controlled cold-start result. |
| Executable/retained representation | Ordinary executable bytes `<= 1.02`; combined retained instructions, operand/point/debug mappings and frame-bank capacity `<= 1.10` on each selected arithmetic/branches, source-form and owned-control fixture. Count the complete representation, not only the hot enum; report zero-baseline categories as absolute bytes. |
| Allocation/lifetime | No new steady-state per-call allocation, bounded reuse, all terminal roots/depth released and zero net diagnostic requested bytes after complete runtime teardown. Retained immutable mappings have an explicit function/program lifetime and are included above. No RSS/OOM claim follows. |

Use the [benchmark harness](../benchmarks/lua-comparison/README.md#beforeafter-workflow)
with fixed seven alternating pairs, three warmups, eleven execution samples and
the existing checker estimator. Show pair ranges and conflicting evidence; no
favorable reruns or unreported affinity changes. Instrumentation, profiling and
ordinary timing run separately, never alongside builds/tests. All ratios in this
plan use the BO01 baseline; historical results are context, not acceptance inputs.

Before production changes, from the repository root:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --freeze-baseline target/bytecode-optimization/before
uv run python scripts/benchmark_lua.py --source-forms --interpreter-only --freeze-baseline target/bytecode-optimization/before
uv run python scripts/benchmark_lua.py --execution-controls --freeze-baseline target/bytecode-optimization/before
```

After building/checking the changed ordinary image, run the same suites with
`--baseline-executable target/bytecode-optimization/before` instead of
`--freeze-baseline`. Validate the original-suite report with
`check_benchmark.py` targets `arithmetic=0.90` and `branches=0.95`; validate the other
reports with the default `1.05` gates. Use the actual report paths returned by the
driver. Setup, executable size and complete retained-byte budgets require their own
recorded calculations; checker success alone does not establish those gates.
Run separate diagnostic-feature counts afterward and restore the ordinary image
before any subsequent timing. Keep artifacts beneath ignored `target/`, but retain
methods, conclusions and limitations in this plan/performance document.

## Progress and decision ledger

- [x] Independent scope, priority and acceptance ownership established.
- [x] BO01: current baseline, attribution, slot/point/proof design and fixed gates.
- [ ] BO02: coherent portable slot/placement replacement and selected contracts.
- [ ] BO03: predicate branches and bounded constant-operand decision/integration.
- [ ] BO04: integrated performance/correctness acceptance and lasting handoff.

### Planning checkpoint — 2026-10-19

Moved the existing instruction-model proposal into this standalone plan at the
user's request. Retained its semantic safeguards, complete migration obligations,
focused test selection and proposed budgets. Removed the IP12 prerequisite and
shared phase ledger; the older plan retains its own outstanding acceptance.
Implementation, benchmark execution and phase commits have not started. Record
subsequent design decisions, measurements and carried errors only in this ledger.

### BO01 — 2026-10-19

Activated by the user's instruction to implement the independent plan and commit
each phase. No production executable code changed in this checkpoint. BO02 must
implement the contract above; a structural sketch is not a measured speed claim.

Frozen ordinary baseline: production revision
`06172d5e1aee6dca0b8f88fe46fc9726d8992b85` with documentation-only planning edits;
executable SHA-256 `ba2c5e44beba5fee14800275483585c7d3a862f02c48503433f08ed042afc740`,
27,663,872 bytes. Cargo.lock SHA-256
`6fe327a7801e48778ae27a74764b556250f9c9007a0528e4fa3d38d424099009`.
All three suite reports identify the same image. The baseline is a fresh ordinary
release build, not an archived checkout sharing compiled outputs.

Machine: Windows 11 10.0.26300, Intel Core i9-12900K, 24 logical CPUs, 66,843,692 KiB
visible memory; rustc 1.99.1 (`58cf8f9c3`, LLVM 23.1.1), Cargo 1.99.1
(`5f94df478`), x86_64-pc-windows-msvc. Workspace release/default target/default Cargo
parallelism, no build environment overrides; Kagari source/native, mlua lua54/vendored.
Build cache reused where valid; fresh benchmark process/state, three warmups and
eleven samples, sequential single-threaded runs. Compilation is outside execution.

Ran the three freeze commands below with bounded deadlines: all original seven,
twelve source forms and twenty-four lifecycle rows passed checksum validation.
Ordinary baseline artifacts live under `target/bytecode-optimization/before` with
`.original.json`, `.forms.json`, `.controls.json`. Selected single-process warmed
medians were arithmetic 1,173.3 us, branches 1,497.9 us, calls 4,450.6 us and
forms_direct 163.0 us. These are baseline observations, not paired improvement
estimates. Workload SHA-256 values:

- Original: `ce648a8dc75951a9f148ff3ce1fd0497c44b93e21ffb2b7324fc63f9e1eaaaee`.
- Forms: `63da18bd97ed4e11e9cdb6a3fb8d77c7903031559020bc452dc1d90018f8808f`.
- Controls: `8570e946697b691a2989188686033c8be32af26f4c2e7a35a017af464d227492`.

Separate current-image profiling/counting used:

```text
uv run python scripts/profile_lua_comparison.py arithmetic branches --executable target/release/kagari-lua-benchmark.exe --pdb target/release/kagari_lua_benchmark.pdb
```

Arithmetic executes 550,015 bytecode positions, including 150,003 local loads and
100,002 local stores; branches executes 600,022 positions, including 120,003 loads
and 90,003 stores. Arithmetic's loop frame has 14 logical temporaries, six physical
temporary colors, three local homes and nine scalar slots (72 payload bytes plus
nine initialization bytes). Branches has 35 temporaries, 13 colors, four homes and
17 scalar slots (136 payload plus 17 initialization bytes). Neither allocates script
heap objects in the sampling window. BytecodeInstruction is 136 bytes and prepared
ExecutionInstruction is 24 bytes; mappings/boxed operands are additional costs.

Sampling rejected 99.43%/99.59% of symbol attribution as unresolved. Do not turn
preceding public PDB symbols into CPU-share claims. Inspecting the dominant unwind
range at image RVA `0xf87090..0xf87899` with objdump shows the scalar dispatcher
shape: 24-byte record stepping, initialized/bounds-checked payload reads, inline
cancellation/state predicate, numeric indirect call and checked destination write.
This supports the observed repeated transport work, not a time share or projected
speedup. The existing fixed before/after gate remains decisive.

The current CLI emitted and source-free executed the review i64 sum probe. Its sum
body still has 17 instructions, three locals, 12 temporary declarations and no GC
root slots. Constants zero/one are outside the loop. Reproduce with the source in
[the review](review.md#interpreter-instruction-and-operand-model-follow-up), then
`kg emit`, `kg disasm --function '@18:0'` for this artifact's selected function and
`kg run-artifact`; artifact-local selectors are not stable identities. The initial
`cargo run -p kagari-cli -- --help` returned usage status 2; it was not a test failure.

Evidence limits: no exact CPU cost partition or complete retained-byte accounting
has been established; BO02/BO04 must report complete mapping/frame costs rather than
infer them from enum sizes. No Rust tests or CI acceptance are claimed by this
baseline/design checkpoint. Documentation links, CRLF and diff checks are its local
acceptance; production validation starts with BO02.
