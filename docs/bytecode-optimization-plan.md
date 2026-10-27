# Bytecode optimization execution plan

## Status, priority and ownership

**Status: active, implementation and one commit per phase authorized on 2026-10-19;
BO01-BO03 completed; BO04 is blocked after its bounded diagnosis/repair: four
owned-lifecycle timing gates remain above 1.05.**
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
- [x] BO02: coherent portable slot/placement replacement and selected contracts.
- [x] BO03: predicate branches and bounded constant-operand decision/integration.
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

### BO02 — 2026-10-19

Replaced bytecode Register/LocalSlot and local transport opcodes with checked u32
Slot identities, explicit parameter homes and a separate full semantic projection.
MIR's original semantic namespaces remain unchanged. Compiler placement conservatively
forwards block-local scalar reads and redirects eligible adjacent scalar assignments;
managed/cell/call/await snapshots remain explicit. Runtime preparation, frame access,
GC/await, debugger, canonical codec, disassembly and native/AOT consumers now use
this one format. No compatibility reader or second execution engine remains.

The sum probe changed from 17 operations/3 locals/12 temporary declarations to 13
operations/8 unified slots; both arithmetic updates write their source homes directly.
The loop-carried i snapshot and return move remain conservatively necessary under
the selected block/debug analysis. Do not advertise the seven-operation sketch as
achieved. The emitted artifact executed successfully without source input.

Independent review identified and resolved: convergence-time type expansion and
retained inferred-type budgeting; direct-argument intrinsic interface bridge
validation; generic parameter physical layout with omitted home annotation; exact
per-instruction origin choice after many-to-one mapping; absent native coordinates;
and public frame access/side-entry with reused physical storage. Source-free origin
validation requires an explicit actual-origin choice for ambiguous PCs. Inferred
types are bounded before combined cloning and charged conservatively on allocation.

Public API impact: Frame read_slot/write_slot/jump_to replace separate local/register
access. Access outside a reused logical lifetime and jumps that manufacture an
uninitialized reused value are rejected. The immutable reuse guards are function-owned;
no second local store, permanent mirror or blanket disabling of reuse is introduced.
The new format is unpublished: identifiers remain, stale artifacts/caches must be
regenerated. The phase commit is breaking and records this impact.

Focused correctness passed at the existing owners (counts refer to selected cases,
not workspace coverage): compiler analysis 8, codec 7, program points 1, lowering 19,
bytecode validation 19, bytecode artifacts 9 after its failing disassembly case was
fixed/retested; bytecode async verifier 3 and disassembly 40 after fixture repairs;
VM numeric 8, optimized execution 1, debugger 1, suspended caller 1, owned drive 2,
GC/capture/pinned reload 3, await storage 1; runtime storage 7 (manual timing ignored),
execution frames 14 before additional side-entry/provenance regressions; real native
overflow 1, AOT 2 and SDK owned native wait 1. Follow-up provenance/side-entry tests
at their existing owners passed; intrinsic protocol bridge and forged shared-generic
SDK boundaries each passed. No full-workspace test sweep was run or claimed.

Workspace Clippy, structure (1,384 Rust files, zero violations), fmt and diff checks
passed. The first structural check found two long use-site constant paths in the
verifier; explicit imports repaired them. No unresolved compilation/test failure is
carried. Earlier passing unaffected tests remain evidence rather than being rerun
for an all-green transcript.

Two fixed seven-pair ordinary comparisons against BO01 passed the predeclared gates:

- Arithmetic candidate/baseline median 0.7061318, pair range 0.4994513..0.8529578;
  checker target 0.90. Diagnostic artifact_prepare ratio 0.9884, source_to_artifact
  1.0021 and program_link 0.9781.
- Branches median 0.8391844, pair range 0.7047686..0.9190460; checker target 0.95.
  Diagnostic artifact_prepare ratio 0.9932, source_to_artifact 1.0118, program_link
  1.0025. Wide ranges are disclosed; do not infer precise speedup confidence.

Commands used the existing driver with `--interpreter-only --workload arithmetic`
and `--workload branches`, each `--baseline-executable target/bytecode-optimization/before`,
then check_benchmark.py with the corresponding fixed target. Reports are
`target/lua-comparison/20261019T163705Z-paired/results.json` and
`20261019T163914Z-paired/results.json`; they record the matched machine/toolchain,
cache/features/profile and raw samples. Candidate SHA-256
`e55b939f44a3d0e3a81f57f215d225d547aa38ce7f9a5e380c8b89139a5f9728`,
27,792,896 bytes (1.00466x BO01), without diagnostic instrumentation. Required fixture
and timed harness checksums stayed unchanged; only the non-timed profile consumer
was migrated. Setup totals, complete retained metadata/frame accounting and other
execution/lifecycle regression rows remain BO04 acceptance, not established here.

BO03 starts from this coherent replacement; no predicate-branch or constant-operand
instruction was added in BO02.

### BO03 — 2026-10-19

Added a general PredicateBranch retaining the original comparison and explicit
Boolean polarity, and BinaryOperands with slot/typed canonical scalar-constant
inputs. Compiler selection fuses only control-only, single-definition comparisons
(and eligible not chains) with compatible debug states; data bools/source bindings
and necessary snapshots remain. Constants are numeric, single-definition and
block-local with complete width facts. Runtime preparation resolves constant bits,
operand orientation and the existing numeric kernel once. No literal encoding,
per-execution constant lookup, duplicated numeric semantics or specialized loop is
introduced. Cross-block hoisted constants remain conservative, including the sum
probe's constant one; this is a bounded-domain deferral, not an immediate ABI bridge.

The sum now has 12 operations and seven slots, versus BO02's 13/eight and BO01's
17 operations/15 combined declarations. Its original less-than directly branches;
actual source-free execution passed. Residual moves are retained by block/debug
proof requirements. The complete seven-operation illustration is not claimed.

ExecutionInstruction remains 24 bytes; scalar sites are immutable function-owned
48-byte records with a 16-byte per-function Box header. These hold only prepared
scalar bits/slots/kernels/destinations, not GC handles. Full memory accounting includes
them; retaining the hot enum size alone is not a footprint acceptance argument.

Independent review caught primitive comparison protocol proof and static object
identity-category omissions on new predicate forms. Both now reuse ordinary
comparison restrictions, also covering BinaryOperands; 10 forged cases exercised
20 direct/decoded rejections at the existing portable comparison test owner.
The four selected protocol/identity/constant/NaN tests passed after the repairs.
No unresolved correctness/build error is carried.

Selected BO03 correctness: compiler validation 21, lowering 19, program points,
canonical native correspondence and artifact disassembly one each; bytecode
instruction disassembly 40 and async verifier 3; runtime execution frames 17 and
physical instruction budget 1; VM numeric 10, debugger 1, owned drive 2; real native
overflow 1 and AOT 2. Scope/repair runs reuse earlier unaffected evidence and are
not a workspace sweep. Final workspace Clippy, structure (1,388 Rust files, zero
violations), fmt and diff checks passed.

The ordinary integrated image SHA-256 is
`0be79d87ead754147538e8c5b16450f10da3c7475d6a8aa60a8c3a1bee9c7073`,
27,779,072 bytes (1.00416x BO01). All original/forms/control report checksums match
the same image and unchanged timed workloads. Seven-pair arithmetic and branches
medians passed fixed targets at 0.7693863 and 0.7666826; pair ranges were
0.5155460..1.0106615 and 0.6010461..1.2974597. These meet BO03's targeted gate, not
BO04's integrated regression gate.

BO04 carried timing failures, disclosed before committing: original maps 1.1428386;
forms byte_state 1.0840472, capture_cell 1.1036056, field 1.2150740, host_callback
1.3583237, interface 1.0519814, string_calls 1.2305812; nine of 24 owned lifecycle
rows also exceed 1.05. Reports are `20261019T181325Z-paired`,
`20261019T182609Z-forms-paired`, `20261019T182754Z-controls-paired` under
`target/lua-comparison`. All result checksums pass, but 16 of 43 timing gates fail.
Some pair ranges span approximately 0.35..3.08, so neither code regression nor
scheduling noise can be presumed away. These failed results remain evidence.

One bounded BO04 diagnosis is predeclared: use the unchanged image pair and fixed
seven pairs/three warmups/eleven samples on logical CPU 2 (affinity mask 0x4).
Windows GetLogicalProcessorInformationEx identifies it as efficiency class 1,
SMT P-core (mask 0xC); E-cores are class 0. Parent process affinity is inherited by
both engines and all workload processes. Record the change and run the same three
suites once; no threshold, fixture, sample count or favorable sample selection may
change. If failures remain, diagnose the changed execution model using this evidence
and record rejection/blockage or seek a scope decision; do not start a tuning queue
or claim unrestricted acceptance from a more favorable isolated sample.

### BO04 — 2026-10-19: evaluated, acceptance blocked

Completed the integrated evaluation and one bounded architecture diagnosis/repair;
**BO04 is not checked complete**. The lasting
[measurement report](performance-baseline.md#bytecode-slot-and-operand-evaluation-windows-2026-10-19)
records environment, image identities, fixed estimators, complete retained-memory
scope/results, failed unrestricted evidence and exact outstanding gates without
requiring ignored report files.

Fixed-P-core diagnosis of the unchanged BO03 image passed original seven but left
forms_native at 1.052035 and single-cancel admission/retirement at 1.090909/1.10.
Source inspection isolated added duplicate frame admission: execute_region reacquired
a frame solely to set completed just after an existing result-publication borrow.
The one repair moves completion into the owning admitted closed-region/native/
constant/tuple/field publication borrow. No frame borrow survives a callback; errors
do not mark completion. Cancellation, dynamic value/window checks, source origins,
PC/slice rules and public side-entry guards are unchanged. No new field, enum layout,
collection optimization or scalar-kernel semantic path was added.

Affected regression selection passed: 21 runtime frame/cursor/budget cases (one new
Unit-native fixture corrected and only that failure rerun), 14 VM native-control/
reentry/owned/debug/field/tuple cases, two SDK nested/shared layout cases. A temporary
fixture compile error was fixed before tests; no carried compilation/correctness
failure remains. Required workspace Clippy, structure (1,388 files, zero violations),
fmt and diff checks passed. Earlier passing unaffected numeric/source-free/native/AOT
coverage remains evidence; exhaustive workspace/backend/platform checks are CI-only.

The final ordinary image is 27,816,448 bytes (1.00552x BO01), SHA-256
`7992458ac9914ffc0869daf69f442727b3d86989077ae9edcd15644300e3b9b8`.
All three post-repair suites use that same image, original BO01 baseline, unchanged
workload/lock hashes and fixed CPU2/0x4 procedure. Reports:

- `target/lua-comparison/20261019T185615Z-paired/results.json`: original 7/7 pass;
  arithmetic 0.6918111 (range 0.6769153..0.7130965), branches 0.7199509
  (0.5044933..0.8225882), meeting the fixed 0.90/0.95 targets.
- `20261019T190822Z-forms-paired/results.json`: forms 12/12 pass;
  forms_native 0.9616903 after repairing its repeated publication admission.
- `20261019T191001Z-controls-paired/results.json`: controls 20/24 pass. Remaining:
  admit/roots_1_tail_complete 1.0769231; collect/roots_256_forward_cancel 1.0681818;
  collect/roots_256_random_cancel 1.0677966; retire/roots_1_tail_cancel 1.10.
  Checker returns failure. All result checksums pass, not all performance gates.

The single-root differences are about 0.1 us, but no complete cost attribution proves
that all residual failures are noise. No favorable retry, reduced limit or expanded
local tuning is authorized after this bounded disposition. Further implementation
requires an explicit scope decision rather than another automatic phase.

Diagnostic total setup passes (original 1.015981, forms 1.010372, limit 1.10).
Complete affected representation plus actual frame-bank storage passes all eight
selected rows, ratios 0.852970..1.050934; stricter no-session-padding maximum
1.068820. All sixteen diagnostic lifecycles end at zero net bytes and selected
warmed native/script-window allocation contracts show no new allocations. Temporary
memory instrumentation used an isolated BO01 archive/default target and was removed;
ordinary builds were restored. Its later completion-owner repair does not change
measured storage layouts or capacity policy. This is not a full retained workload
matrix, RSS/OOM proof or permission to treat timing failures as passes.

Implemented contracts have been transferred to architecture/specification owners.
The plan remains active solely for its unresolved integrated acceptance and finite
scope decision; it is not retired, historical IP12 remains separate, and CI/Lua
parity are not claimed. The BO04 commit records the repair and blocked evaluation,
not a completed acceptance checkpoint.

### CI correctness repair — 2026-10-20

[Run 39345496193, Rust job 118376967059](https://github.com/kagari-lang/kagari/actions/runs/39345496193/job/118376967059)
at `503de5c9` exposed 27 failures across four library targets. The user authorized
this correctness repair separately from further performance tuning. Causes and fixes:

- Physical intervals incorrectly treated their lowest instruction ordinal as a
  first definition. A match join can appear before its defining predecessors.
  Immutable reuse guards now distinguish output-only definitions from live-in/read
  starts; output-only publication, explicit side-entry checks and actual physical
  reuse remain intact. Frame access errors now identify the slot, PC and access.
- Access flow omitted the fixed `i64` result of `ValueHash`, rejecting its join with
  a typed constant in generated enum hash adapters. Restore that fact without
  relaxing conflicting-type/access checks. Add a source-free serialized join case.
- Numeric tests assumed only the old `Numeric` instruction, and reference-corruption
  tests assumed a parameter required a defining `Move`. Assert the same numeric
  widths/operations across constant operands and select the authoritative parameter
  slot before corrupting storage; retain all rejection assertions.
- The native execution fixture omitted mandatory executable origins. Populate them
  without changing native execution/ABI/ownership assertions.
- Async provenance tests expected stripping optional debug records to remove spans
  now retained in executable origins. Assert exact source text in both forms.
  Block-bodied waiter fixtures supply an explicit await span; the existing bare
  closure-body span gap in source lowering is not repaired or claimed resolved here.

Local validation passed all 27 originally failing cases: SDK iterator, binding
patterns, async provenance, three custom-key/hash cases and the standard-traits
example; two compiler numeric cases; all 16 native execution cases; VM key GC and
source-free reference corruption. Additional focused coverage passed 22 compiler
bytecode-validation cases, shared-associated-output numeric behavior, the new
backward-join layout case, public reused-frame access and native publication.
Selections target the changed flow/representation/publication boundaries rather
than reproducing a workspace sweep. The execution specification clarifies that
manual fetch APIs do not execute skipped instructions or establish their effects.

The additional regression command below did not finish within a 240-second limit
including build-lock wait and compilation; it is not counted as passed. Earlier
selected attempts also timed out. No assertion failure was observed for this case.

```sh
cargo test -p kagari-compiler --lib tests::numeric::narrow_arithmetic_retains_source_width_without_assert_expansion -- --exact
```
IDE build validation timed out; successful Cargo tests and Clippy provide the
compilation evidence instead.

Required structure (1,388 Rust files, zero violations), formatting, workspace
Clippy (`--all-targets -- -D warnings`) and diff checks passed. Full workspace,
Linux/backend/platform acceptance remains CI-owned and has not been rerun here.
No timing or retained-memory measurements were repeated after the reuse-guard
metadata change: earlier image/layout measurements remain historical evidence,
not measurements of this repair. BO04's four timing gates remain blocked.

### Direct target instruction shape, 2026-10-27

The user requested the ideal output before discussing implementation. This is a
proposed target and comparison, not another activated implementation phase or a
change to BO04 acceptance. The [actual direct listing](performance-baseline.md#direct-bytecode-inspection-2026-10-27)
contains fifteen instructions, eight logical slots/five prepared scalar locations,
and 40,008 dynamic operations. The current source is unchanged by this review.

A general register-style target uses state, sum and i homes plus one scalar scratch
location, constant operands, success-only arithmetic destinations, direct return
and a conditional backedge. The following is pseudobytecode: typed names explain
contracts, not required new opcodes; constants may use the existing prepared pool
operands. The initial guard retains zero-iteration behavior without relying on
benchmark-specific bounds. Each conditional evaluates the original i32 less-than
and applies polarity; the other successor is the following instruction.

```text
00  const.i32 state, 1
01  const.i32 sum, 0
02  const.i32 i, 0
03  branch_false.i32.lt i, 5000, exit
body:
04  mul_checked.i32 scratch, state, 3
05  add_checked.i32 scratch, scratch, 7
06  rem_checked.i32 state, scratch, 251
07  add_checked.i32 sum, sum, state
08  add_checked.i32 i, i, 1
09  branch_true.i32.lt i, 5000, body
exit:
10  return.i32 sum
```

This target has eleven stored instructions and 30,005 dynamic operations for
5,000 iterations: three initializations, one initial guard, six operations per
iteration and one return. The prior 35,005-operation proposal retains the original
loop-header/backedge arrangement; it is a separate, smaller placement change.
The conditional-backedge form eliminates a dispatched unconditional jump on every
successful iteration without merging arithmetic operations. If ordinary constant
propagation proves the first condition true (here i=0 and bound=5000), the entry
guard can additionally disappear: ten stored instructions / 30,004 operations.
These are work counts for proposed layouts, not measured runtime/retained-memory
improvements or evidence that the existing pipeline can emit/verify them unchanged.
Four scalar locations are a target, not a new observation of physical allocation.

Success-only destination publication is essential: state must remain unchanged if
multiply/add/remainder fails, sum must retain its old value if its own addition
fails, and i must remain unchanged until its own increment succeeds. scratch can
be overwritten after each successful operation because its earlier intermediate
has no later use. Do not algebraically combine the multiply/add/remainder or
reorder checked operations based only on this fixture's safe inputs.

Existing constant operands and PredicateBranch already supply the operation model.
Multi-use result-to-home forwarding and physical scratch reuse have different
owners: bytecode placement consumes verified MIR facts, while runtime preparation
owns location allocation. Loop rotation is an additional compiler CFG/layout
proposal beyond the implemented block-local placement rules. Duplicating/moving
condition code requires explicit semantic origins/canonical correspondence,
statement/loop debug states, checked initialization and source-free validation;
it must not be implemented by skipping final bytecode instructions in the runtime.
Final executable boundaries still own cancellation, slicing and observation.
The target does not replace the recurrence with a closed-form result or a
workload-specific opcode. Retain the whole semantic validation model even if its final
physical code has fewer operations.

Primary references checked on 2026-10-27: Lua 5.4.9
[lopcodes.h](https://www.lua.org/source/5.4/lopcodes.h.html) uses register destinations
and immediate/constant arithmetic operands;
[lvm.c](https://www.lua.org/source/5.4/lvm.c.html) consumes comparison-controlled
jumps in its comparison handler. These support direct operand/control transport,
not Lua's wrapping arithmetic, raw value pointers or an identical code encoding.
The rolling Rust [MIR guide](https://rustc-dev-guide.rust-lang.org/mir/index.html)
describes typed CFG operations and constant/place operands; Kagari retains the
separate verified semantic MIR, interpreter placement and physical allocation
owners. Its checked commits, source origins and cancellation boundaries require
those adaptations. Before implementation, freeze same-machine baseline images
and select focused numeric overflow/trap order, debug/origin, empty-loop,
cancellation/slice and source-free contract coverage. Existing failed lifecycle
performance gates remain open; no unmeasured speedup is claimed.

The [exact Lua direct dump](performance-baseline.md#direct-lua-instruction-comparison-2026-10-27)
confirms four Lua registers, nineteen stored 32-bit words and 40,006 successful-input
hook dispatches. Its loop reloads 5000 into the scratch register, and still has a
separate backedge: eight dispatched handlers versus the proposed six. Arithmetic
metamethod fallback words are skipped on integer success. Lua instruction storage
is denser; no new Kagari encoding or per-handler/elapsed performance claim follows.

### General implementation ownership for the direct target, 2026-10-27

The user asked which changes the ideal output requires and whether they generalize.
This records a bounded design review, not implementation activation, another BO phase
or disposition of the outstanding gates. These are general scalar placement, CFG
and allocation rules; no source/function-name, literal-count or recurrence matching
is justified. Existing MIR BasicBlock/Terminator, bytecode PredicateBranch/constant
operands and prepared scalar kernels are the foundations; no second IR or execution
semantics is proposed.

| Required behavior | Current owner / concrete gap | General adjustment |
| --- | --- | --- |
| Final arithmetic result writes directly to its local home | compiler bytecode/placement.rs requires the temporary's sole use to be the following Move | Extend bounded scalar destination selection using definition/use and clobber facts. Rewrite additional uses only while the home still carries the same value; retain snapshots across writes, calls, cells, managed access or uncertain debug states. |
| Constants initialize homes; return consumes the original result | The same placement owner retains shared-zero initialization and exit transport in direct | Fold eligible constant-to-home initialization and forward terminal copies using existing full type, initialization, debug/origin and lifetime facts. Duplication of a cheap scalar literal must not duplicate managed materialization or side effects. |
| No jump to the next emitted instruction | compiler bytecode.rs lowers every MIR Jump; placement already repacks PCs | Add bounded final fallthrough cleanup with shared target/origin/debug remapping. Protect entry, target and completion/inspection contracts instead of merely deleting a word. |
| Conditional backedge replaces separate test and jump | MIR passes.rs has scalar/copy/dead passes but no loop rotation; MirFunction::emission_order is entry then block-ID order | A bounded MIR CFG transformation produces guarded rotated loops before fresh verification and origin emission. Start with simple reducible loops and closed scalar conditions, preserve break/continue and condition evaluation count/order, and decline uncertain/suspending/reentrant shapes. Bytecode's existing predicate selection then emits the conditional backedge. |
| Entry guard disappears only when proven true | MIR constants pass owns scalar/branch simplification | Feed the transformed CFG into the existing verified simplification pipeline where adequate facts exist. Keep guards for unknown/false bounds; proving this fixture's first test true is optional after the general layout works. |
| Three homes plus one physical scratch location | runtime module/execution/allocation.rs uses inclusive first/last-PC intervals and expires an active color only when last < start | Review read-before-write reuse for eligible scalar operations. A dying input may share its successful output's physical location, but named binding availability, suspended/native views and public inspection must retain correct before/after values. This is separate from removing bytecode Move dispatches. |
| Preserve complete executable correspondence | compiler native_input.rs re-lowers portable verified MIR and compares all canonical facts | Use the same deterministic lowering/selection for source compilation and source-free comparison; regenerate transformed MIR analyses, origins, roots, debug ranges and actual-origin selections. Do not bypass equality or independently recreate optimization semantics in verifier/runtime. |

A multi-use temporary is not automatically a home alias. In the sequence t=expr;
x=t; x=other; use(t), forwarding the final use to x changes its value. Likewise,
x=x+f() may require preserving the pre-call x if f mutates an alias. Eligibility
must be established over the exact interval, not by removing the single-use guard
without replacing its proof. Arithmetic inputs are consumed before destination
publication; failed arithmetic retains the previous local home.

The four-location goal needs an explicit allocation decision. Current intervals
record both an operation's last input use and first output definition at the same
PC, then require last < start for reuse, so an otherwise dying scalar input and
output cannot share a color under that rule. Simply changing it to <= would affect
all instructions and public reused-slot inspection. Retain the current conservative
rule unless operation-specific read/write phases, definition/lifetime guards and
actual handler behavior establish safe reuse; managed roots/callbacks are outside
the first bounded scalar review. Six loop operations can be delivered independently
of the four-location target. Logical slot count need not equal physical bank count.

Loop rotation is an additional CFG mechanism beyond implemented block-local BO
selection. The existing nondecreasing semantic-origin-to-PC contract makes arbitrary
post-hoc block movement inappropriate. Prefer transforming/reverifying MIR before
assigning its canonical emission origins, and let the single deterministic bytecode
lowering consume those facts. Each duplicated condition site must receive correct
verified program-point/debug correspondence; do not widen artifact rules just to
hide a mapping failure. Existing Terminator::Branch already carries two successors;
compare-plus-branch selection can remain bytecode-owned rather than adding another
MIR predicate terminator solely for this example.

Suggested review order is destination/copy/fallthrough placement, guarded loop
rotation with correspondence, then any justified scalar allocation refinement.
Bytecode packing and the 24-byte prepared instruction / scalar-site footprint are
separate design work: matching the six-operation target does not require an encoding
migration, numeric-kernel ABI change, collector change or more native backend support.
No promised end-to-end speedup follows from these operation counts.

References checked on 2026-10-27: Lua 5.4.9
[lcode.c](https://www.lua.org/source/5.4/lcode.c.html) directs a local assignment's
expression into its target register and manages temporary registers. Kagari must
add its checked commits, lexical/type/ownership facts and debug availability rather
than copy that source frontend's mechanism. LLVM 22.1.0
[rotated loops](https://releases.llvm.org/22.1.0/docs/LoopTerminology.html#rotated-loops)
use a guard when a first iteration is not proven, supporting the proposed control
shape. LLVM's SSA/LoopInfo, undefined-overflow assumptions and backend ownership are
not Kagari contracts. The rolling Rust
[MIR optimization guide](https://rustc-dev-guide.rust-lang.org/mir/optimizations.html)
supports applying typed transformations before codegen; Kagari's existing optimizer
consumes its old seal and rebuilds verification rather than accepting edited facts.

Implementation validation must reuse focused owners: checked arithmetic/trap and
completed-assignment semantics; clobbered scalar snapshots; zero/one/multiple loop
iterations plus break/continue; float predicate polarity where shared selection is
affected; debug actual origins and reused-slot before/after inspection; cancellation,
slicing, traps and source-free native-input canonical correspondence. Add only
uncovered contracts. Compare full retained instruction/site/layout/origin storage,
not opcode counts alone, then run a frozen same-machine original/forms comparison
and the selected dependent lifecycle controls without changing old gate outcomes.
Required implementation structure/fmt/workspace Clippy/diff checks and CI-only full
matrix policy remain unchanged. This review has no implementation tests or new
measurements to report.
