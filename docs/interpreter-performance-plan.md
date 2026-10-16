# Interpreter performance execution plan

Status: activated on 2026-10-16 by the user's execution request. IP01 is complete
at 60bfaacb; IP02 is implemented with two carried timing failures. IP03's ordinal,
callee-layout, runtime-contract, parameter-publication, action-continuation and
prepared native-callback changes are integrated;
focused validation and complete lifecycle/performance acceptance are in progress.
IP04-IP07 are unstarted.
The [roadmap](implementation-roadmap.md#interpreter-performance-follow-up) owns
activation. This plan owns interpreter implementation decisions, acceptance and
the progress ledger.

## Outcome and relationship to object storage

Reduce interpreted call and frame-management overhead, then evaluate bounded
changes to execution storage and instruction dispatch. Preserve the
[project goals](project_goal.md), [architecture](architecture.md#runtime-model)
and observable [execution contract](spec/execution.md).

The companion [object storage plan](object-storage-overhead-plan.md) remains the
only owner of OM01-OM05: cost attribution, native revision relocation, conditional
layout grouping and conditional interface-record consolidation. Its numerical
gates and ledger are not duplicated here. The default sequence is to finish a
coherent OM checkpoint before freezing IP01, then avoid interleaving production
storage changes with interpreter measurements. IP does not require conditional
OM changes to be accepted. Refresh interface consumers against the actual accepted
storage model before implementing IP03.

Required IP work is explicit window-slot reuse and one prepared call lifecycle.
Session-owned storage, instruction specialization and selected library transitions
are conditional experiments with finite scope. A documented rejection completes
a conditional decision; it does not satisfy an unfinished required phase.

Retain 16-byte Value transport, full checked identities, packed struct fields,
shared interface descriptions, scalar/managed banks and the admitted execution
cursor. Do not add a second call engine or retain obsolete implementations as
compatibility paths. Universal enum unboxing, NaN boxing, handle compression,
new GC algorithms, compiler specialization policy, native-backend expansion,
downcast and the [NC collection migration](native-collections-plan.md) are outside
scope. No routine ABI/format version bump is required for this unpublished work.

## Evidence and uncertainty

Source inventory is at 26f035d7, whose latest production storage change is
35077323. Refresh it at activation. The following historical measurements select
investigation targets; they are not fresh IP baselines or promised speedups.

| Evidence | Architectural implication |
| --- | --- |
| [RS storage measurement](performance-baseline.md#runtime-storage-2026-10-16): 512 Points retain 58,084 live and 40,508 empty requested-byte increments; each Point has eight field bytes | OM must attribute records, capacity and descriptions before shrinking more field data. |
| [HP06 sampling](performance-baseline.md#remaining-architecture-costs-and-bounded-follow-up): shared-generic call entry has 63.36% inclusive samples; Fibonacci entry/return have 36.94%/10.40% | Prepared selection does not eliminate frame admission and return costs. Inclusive samples overlap and are not predicted gains. |
| HP06 Fibonacci requests seven Rust allocations per complete warmed execution | Window allocation primarily includes reusable storage initialization; it is not one allocator call per recursive call. |
| HP06 Map native invocation has 53.72% inclusive samples; its workload constructs 2,001 objects | Call transitions and ordinary Option results need separate attribution. Object count alone does not justify enum unboxing. |
| HP06 string constants has 47.87% scalar-kernel leaf samples, without script object allocation | Instruction handling remains an independent cost after allocation is removed. |

HP06 used Apple M1 Max, arm64 macOS 26.6.2, rustc 1.98.1 and the workspace
release profile. Its interpreter/Lua ratios of 2.57-51.11 are historical.
The dependency refresh and later storage changes require a new matched baseline;
never compare elapsed times across the old Mac and a new Windows runner.

Current source exposes additional mechanisms to measure:

- [Prepared calls](../crates/kagari-runtime/src/module/execution/calls.rs) retain
  argument sources and result destinations, while
  [call entry](../crates/kagari-runtime/src/frame/calls.rs) looks up a PC-keyed
  record and rereads canonical instructions on some paths.
- [Frame creation](../crates/kagari-runtime/src/frame.rs) admits arguments,
  executable metadata and environments, obtains links, and publishes a separate
  value window. Classify checks by guarantee before relocating them.
- [Window allocation and release](../crates/kagari-runtime/src/frame/values.rs)
  scan for an empty identity slot. Non-tail release drains global banks and fixes
  surviving ranges; ordinary tail return already truncates them cheaply.
- [Scalar execution](../crates/kagari-runtime/src/frame/cursor/scalars.rs) calls a
  selected numeric kernel through a function pointer and checks operand state.
  Whether dispatch, checks, code footprint or instruction count dominates is an
  IP01 measurement question, not an assumption that every check is redundant.

## Ownership and target design

### Prepared calls and active transitions

For an identity function called 5,000 times, the body returns an argument, but
each call still selects a target, admits arguments, creates a window and retires
it. Preparing the signature once only helps if the transition consumes those
facts through parameter transfer and result publication.

| Owner | Responsibility after replacement |
| --- | --- |
| Verified program and runtime execution preparation | Own each caller's call ordinal, argument source locations and return destination without source analysis. |
| Runtime-linked function or exact generic/method application | Bind the target generation, environment, callee parameter layout, conversion template, result adapter and dependency edges; exclude caller-specific offsets. |
| Active execution scope | Admit the current runtime/session and enforce cancellation, depth, quarantine and real dynamic boundaries. |
| Frame/window owner | Publish parameter and executable roots transactionally, retain them through suspension and adapt/transfer results before retirement permits collection. |
| VM driver | Drive transitions and observation using runtime-owned contracts; do not resolve types or own another semantic implementation. |

Replace the PC-keyed call tree with a dense descriptor sequence addressed by a
prepared call ordinal. Keep source/debug PCs for diagnostics. A dynamic call
descriptor carries its checked contract directly; it must not rediscover it by
decoding the original call instruction. Canonical bytecode remains the verified
portable/debug owner, not a duplicate call-policy engine.

Static calls can bind caller sources to callee parameter locations and required
representation conversions during preparation. Interface and shared-generic calls
first select an exact application, then combine its callee template with the
current call site's sources and return destination. Receiver/capture prefixes and
external argument slices remain explicit argument sources, not assumed frame slots.

For example, callers A and B can invoke the same application from scalar slots 1
and 7 respectively. The application shares its callee layout and conversions;
it must not retain A's source offset for B. Prefer composing borrowed descriptions
without another cache. If a retained combined plan is justified, its identity must
bind the caller's pinned program, function, call ordinal and physical layout to the
exact callee/application identity, including environment-dependent conversions.
Version, nominal arguments, witnesses and installed binding authority remain in
the relevant identities. Never retain active window IDs, values or raw bank views
in reusable plans. Do not cache by receiver shape or method name alone.

Preparation belongs to the supplying program/application and retires with it.
Use traced IDs or scoped borrows for cyclic executable dependencies; adding strong
references between linked functions must not create immortal program cycles.
Warm repeated calls must not allocate another descriptor or grow a side cache.

Audit each existing check into one of three owners: immutable verified/prepared
facts, active-region admission, or dynamic operation boundaries. Record the old
check, replacement proof and invalidation point in IP03's ledger. Host input,
changed receiver/type/version, owner/generation, candidate isolation and scoped
borrows still require their real validations. A private transition may reuse
authority only while no callback, suspension or other invalidating boundary has
occurred; no public unchecked entrypoint is introduced.

Reserve storage and admit arguments before publishing a frame. Callee parameters
and executable dependencies become roots together. Result adaptation runs while
callee roots remain live; retirement and caller publication permit no collection
or callback gap. Preserve rollback, call-depth accounting, native return state,
error traces and completed effects on every failure path. Keep safe Rust as the
baseline; removing initialization writes or checks needs a proof of definite
initialization and all observable read paths, not trust in well-behaved scripts.

### Window identity and storage lifetime

First replace linear empty-slot search with explicit reusable indices under the
existing window owner. Reuse preserves complete owner/index/generation checks;
exhaustion cannot wrap. Update allocation, release, trimming and unwind together,
so the reuse structure cannot contain stale or duplicate indices. Retain the
existing scalar/managed banks and non-tail release policy in this required phase.

Separately evaluate storage lifetime. Suppose execution A suspends, execution B
allocates later windows, then A completes. The current global banks may move B's
values and update unrelated ranges. A session-owned segment could make A's
retirement independent of B's retained data while preserving stack-like calls
within each segment.

Before such a change, decide segment identity, growth, free capacity, GC enumeration,
debug access and synchronous reentry ownership. Same-session nested host callbacks
must retain their caller; independent roots must remain independently reclaimable.
No bank view survives growth, collection, callbacks or suspension. Zero-width
windows, out-of-order cancellation and stale frame views retain their contracts.
Use one storage model; a spare segment pool, if justified, needs an explicit bound
and must not retain roots or executable dependencies after release.

### Instruction and library execution

Evaluate explicit typed operations for the hottest measured scalar patterns,
reusing the existing checked arithmetic implementation. A finite opcode set may
replace indirect numeric dispatch and avoid temporary moves. Cold operands may
remain in side tables. Decide encoding from measured footprint and decode cost;
do not add a third retained execution representation beside the existing canonical
and prepared products.

IP05 may prototype at most six typed scalar forms and two fused forms, selected
and frozen before implementation. Typed operations may preserve the existing
one-to-one logical PC mapping. Before prototyping fusion, record these decisions:

- Map every constituent logical PC to its execution entry and successor, including
  branches, debugger positions, trap attribution and observation. Exclude fusion
  across an interior branch target unless that target has a correct entry path.
- Define slice accounting and resumable state for exits at every internal boundary.
  Completed suboperations must neither replay nor disappear after resumption.
- Store intermediate results and initialization state in owned frame storage when
  they must survive an exit. Describe and trace managed intermediates at each such
  point; no cursor borrow or handler-local temporary survives as resumable state.
- Preserve cancellation and collection opportunities, observer events, completed
  effects and mutation commit order through the same semantic handlers.

For example, after the read in a fused field read/add/write, a one-instruction slice
can return to the host, which may mutate the field through another alias. Resuming
must add to the saved read value, not reread the field or skip the write. Overflow
still traps before write publication. Exercise internal exits, intervening GC and
alias mutation, stepping, cancellation and valid interior control-flow entries.
If this model is not closed before the prototype, limit IP05 to typed operations
and defer fusion. Reject fusion if preserving these semantics removes its benefit.

IP06 may evaluate at most two installed library operations. Reuse the existing
[prepared native primitive path](../crates/kagari-runtime/src/frame/cursor/primitives.rs)
and one storage implementation. Admission depends on exact installed callable,
representation and effect contracts, never a function-name whitelist. Callback,
allocation, mutation revision, lease, root publication and reentry behavior decide
whether a transition can stay in a closed region. Dynamic operations still use
the common call protocol. Preserve ordinary Option representation in this plan.
Coordinate any changed storage/access API with NC; do not move collection algorithms
into the VM or activate the collection-library migration indirectly.

## Reference designs and applicability

These primary references informed the design review on 2026-10-16. They provide
mechanisms, not directly comparable benchmark targets.

| Reference | Useful mechanism | Kagari-specific constraint |
| --- | --- | --- |
| [Go allocator source](https://go.googlesource.com/go/+/refs/tags/go1.24.0/src/runtime/malloc.go) | Size-class spans and batch-managed allocation metadata | OM can evaluate amortization; exact nominal layouts and checked handles remain Kagari responsibilities. Copying Go's concurrent allocator hierarchy is unnecessary for a single driven heap. |
| [Luau performance design](https://luau.org/performance/) | Page-based sweeping, specialized builtin calls and 16-byte tagged values | Supports evaluating management overhead and transitions without assuming an eight-byte Value is necessary. Builtin shortcuts must consume installed effects and borrow authority. |
| [Lua call implementation](https://www.lua.org/source/5.4/ldo.c.html) | Function prototypes supply frame size and calls reuse stack arguments | Prepared frame transitions are relevant; Lua's dynamic typing and version model do not discharge Kagari's nominal/generation checks. |
| [Pulley design principles](https://github.com/bytecodealliance/wasmtime/blob/main/pulley/README.md) and [execution model](https://docs.wasmtime.dev/examples-pulley.html) | Typed register execution, compact opcode decoding and superinstructions | Kagari must preserve checked arithmetic, GC roots, logical observation and cancellation. Its existing preparation product is the replacement boundary. |

## Phases and checkpoints

### IP01 Attribute costs and freeze the baseline

Record the exact production revision, accepted OM checkpoint and relevant source
inventory. Reuse the [Lua comparison driver](../benchmarks/lua-comparison/README.md)
and runtime storage probe. Add diagnostics only for missing attribution: call
selection/admission/transfer/return, window slots examined, bank bytes initialized
or moved, descriptor preparation and instruction/transition counts. Counters run
separately from throughput and do not become permanent ordinary-path overhead.

Freeze baseline binaries and the exact sources, inputs, checksums, operation counts,
timing regions and targeted rows for all later phases before production edits.
If a later conditional hypothesis needs another diagnostic, run it equivalently
against the frozen baseline; do not rewrite the original workloads. Record exact
focused correctness filters and the check-to-owner inventory to complete in IP03.
Extend the existing benchmark reporting with the process/pair aggregation and gate
checker defined below; do not substitute the previous pooled-sample summary. Verify
the estimator with synthetic unequal process distributions, paired-order reversal,
missing/duplicate records, zero baselines and passing/failing thresholds. These
checks validate measurement tooling, not runtime performance.
Acceptance: all selected controls build/run, the gate checker rejects incomplete
evidence and reports both required ratios, allocation/profiling limits and separate
setup sampling are documented, and the baseline is reproducible on the selected
machine. Preserve each accepted checkpoint's revision, binary hash and rebuild
inputs for later comparisons against the preceding accepted checkpoint.

### IP02 Replace window slot scanning

Implement explicit free-slot management without changing bank ownership. Migrate
trimming, failure rollback and all releases. Acceptance: allocation no longer
searches live windows, stale identities remain rejected, and the common regression
gates pass. Measure shallow/deep calls and retained/empty window capacity; no
minimum speedup is claimed from the asymptotic change alone.

### IP03 Replace the prepared call lifecycle

Implement call ordinals, complete transfer contracts and admitted internal
transitions with one return protocol. Cover static, closure, interface, shared
generic and native callers, including result adapters. Review earlier HP prepared
facts and region admission first; preserve their useful owners and remove any
superseded call lookup/preparation path in this checkpoint.

Validate that distinct call sites with different physical source/return locations
can reuse one application without sharing caller offsets. Include repeated/reordered
arguments, receiver/capture prefixes and pinned caller/callee generations in the
focused transfer selection. Record which owner retains each half of the plan and
how any combined plan becomes invalid or retires.

Acceptance: each of calls, fibonacci, interface and shared_generic is at least 10%
faster than IP01, common gates pass, and diagnostics show which repeated work was
removed. Warm closed script calls must add no per-invocation allocator requests;
repeated applications reach a bounded metadata plateau. This is a chosen target,
not a forecast. If it fails, diagnose the remaining ownership/transition cost and
record the unresolved required phase before proposing a scope change.

### IP04 Decide session storage ownership

Using the frozen multi-root controls, attribute non-tail movement separately from
ordinary call entry. Prototype segmentation only if the projected benefit justifies
the lifetime migration. Acceptance for retaining it: at least 15% lower retirement
time on each preselected out-of-order workload, no movement proportional to another
session's retained banks, bounded reuse and all common gates passing. Otherwise
retain the existing banks and record measured or architectural reasons for deferral.

### IP05 Decide typed instruction specialization

Freeze the finite operation set and target rows from IP01 profiles before editing
the execution representation. Close the logical-PC/resumption and intermediate-root
model above before any fused prototype; otherwise record fusion as deferred and
evaluate only typed operations. Retain the change only with at least 10% improvement
on every selected target row against the preceding accepted checkpoint, improvement
against IP01, and all common gates passing. Report instruction count, execution
product size and setup cost together. Rejected experiments leave no extra opcode
engine or unused product behind.

### IP06 Decide selected library transitions

Identify the exact one or two operations and their effect/authority owner before
implementation. Use unchanged arrays/maps/string workloads as applicable; select
target rows before editing. Retain only with at least 10% improvement on every
selected row against the preceding accepted checkpoint, improvement against IP01,
and all common gates passing. Unsupported effect contracts are a valid reason to
defer; implementing a second collection engine is not a substitute.

### IP07 Integrated acceptance and documentation handoff

Run the focused composition selection and final matched workload evaluation.
Report each conditional disposition independently. Required integration failures
must be resolved; a missed required performance gate remains open. Move lasting
contracts to architecture/specifications and results to the existing performance
document. Move remaining scope and pending CI to the roadmap/review, then remove
the completed plan. Git history retains its ledger and rejected design history.

## Workloads and performance acceptance

Keep the original 16 matched interpreter/Lua workloads unchanged. Use affected
rows during iteration and all 16 at IP07; this bounded performance experiment is
not a full-workspace test sweep. Entry and host_callback remain separate diagnostics;
the unmatched native row, numeric matrix and JIT cannot satisfy the parity gate.

Add only the following distinct controls, with exact sizes frozen in IP01:

- Window depth and fragmentation: shallow/deep calls, zero-width windows, mixed
  scalar/managed arguments, released slots reused among still-live windows.
- Independent execution roots: a fixed suspended population, tail and non-tail
  completion, reverse/random completion order, cancellation and three equal reuse
  cycles. Freeze the random seed and report moved bytes separately from latency.
- Call validity and lifetime: repeated and alternating receiver/type/application,
  old/new publications, cold/warm preparation, retirement and runtime teardown.

Use the same machine, toolchain, lockfile, features, workspace release profile,
default target and build parallelism for matched pairs. Build first; never overlap
timing with compilation, tests or another agent-started workload. Separate ordinary
timing, sampling, allocation instrumentation and logical counters. Report setup,
execution, collection and retirement separately, with GC policy fixed per pair.

For execution and lifecycle acceptance use at least seven alternating
baseline/current process pairs, three warmups and eleven samples per timed row.
Setup has a separate fresh-state protocol: the current driver samples each setup
phase three times per process for the original suite and once for source forms.
Freeze those counts and cold/warm shared-preparation treatment in IP01, retain all
seven process pairs and report setup separately. Do not describe setup as eleven
warmed samples or mix Cargo build duration with script compilation/preparation.

For each phase/workload/engine row and matched process pair i, compute B_i and C_i
as that process's baseline and candidate sample medians after batch normalization.
Report all process medians/ranges and both ratios:

```text
aggregate ratio = median(C_i) / median(B_i)
paired ratio    = median(C_i / B_i)
```

Both must satisfy each applicable gate; a 10% improvement means <=0.90 and a 5%
regression limit means <=1.05. Pooling samples across processes is not equivalent.
The IP01 checker must validate pair/variant identity, expected rows and sample
counts, checksums, fixture hashes and matched configuration before computing gates.
Missing, duplicate, mismatched or failed evidence cannot pass. A zero timing median
requires a predeclared longer batch and a new matched measurement, not division by
zero. Report pass/fail/inconclusive per gate and fail automated acceptance when any
required evidence is missing or any gate is not passed. Setup uses these same
process-level estimators with its separately declared sample count.

Near-threshold noise is inconclusive: predeclare additional pairs and retain all
results. Keep raw files under ignored target/ and the methods, hashes, conclusions
and limits in this ledger until handoff. Do not select favorable samples.

The [driver](../scripts/benchmark_lua.py) builds before timing and treats --runs as
the number of matched process pairs when a baseline is supplied. Freeze the binary
and each matrix report with --freeze-baseline before production edits. A successful
measurement is not acceptance: the [checker](../scripts/check_benchmark.py) validates
both process-level estimators and the complete frozen matrix contract. IP01 also
records separate storage, session-retirement and allocation measurements.

```text
uv run python scripts/benchmark_lua.py --interpreter-only --runs 7 --baseline-executable <baseline-executable> --baseline-report <baseline-original-report>
uv run python scripts/benchmark_lua.py --source-forms --runs 7 --baseline-executable <baseline-executable> --baseline-report <baseline-forms-report>
uv run python scripts/benchmark_lua.py --execution-controls --runs 7 --baseline-executable <baseline-executable> --baseline-report <baseline-controls-report>
uv run python scripts/check_benchmark.py <paired-results.json>
```

Common gates apply to each retained checkpoint and the final result against IP01;
allowances do not compound across phases:

- Every unchanged execution/control median remains within +5%; phase-specific
  improvement gates apply in addition and remain satisfied at later checkpoints
  and IP07. Construction, live/release GC and retirement controls stay within +5%.
- Compilation, preparation and linking are each within +5%. Separately attributed
  prepared-code/metadata and loaded-setup requested bytes are each within +5%.
  Report the cost of added descriptors rather than hiding it in runtime setup.
- Live, empty and reused requested bytes for every workload, including performance
  targets and segmented-session controls, remain within +5%. Include all retained
  segment/free-pool capacity; a bounded plateau alone does not satisfy this gate.
  Warm request counts/bytes cannot increase on changed call paths. A zero byte
  baseline requires zero, not a percentage comparison. Report capacity separately
  from live payload. There is no implicit target exemption. Any proposed exception
  requires an explicit plan change before its prototype, identifying the workload,
  lifecycle state, absolute byte cap, reason and acceptance tradeoff; it cannot
  waive correctness, zero-net teardown or inherited OM/RS obligations.
- Repeated equal workloads reach bounded capacity/metadata plateaus. Warm runtime
  teardown returns to zero net requested bytes. Old versions retire after their
  actual roots disappear; intentional current roots are accounted for separately.
- If OM is accepted before IP01, its storage savings and original RS controls must
  still hold at IP07. Preserve matching baseline evidence; a fresh IP baseline does
  not erase earlier regression obligations.
- Correctness, cleanup, access, cancellation, pinned reload and logical debugger
  behavior permit no regression. A speedup does not compensate for a lost guarantee.

These gates are local project acceptance targets. The broader Lua-parity objective
still requires interpreter/Lua median <=1.0 on every unchanged matched workload
on one machine, with uncertainty analysis near parity. Completing IP does not
automatically establish parity or exhaustive CI acceptance.

## Correctness and implementation verification

Select a small affected set from these existing owners; this inventory is not a
command to run every suite at every phase:

| Contract | Existing test owner |
| --- | --- |
| Window identity, reuse and root publication | [Runtime frame values](../crates/kagari-runtime/src/frame/values/tests.rs) |
| Caller isolation, depth failure and pinned frame reload | [VM execution frames](../crates/kagari-vm/src/tests/execution/frames.rs) |
| Closed cursor and logical execution boundaries | [Runtime cursor](../crates/kagari-runtime/src/frame/cursor/tests.rs) |
| Nested host calls, breakpoints and trap stacks | [VM reentry debugger](../crates/kagari-vm/src/tests/reentry_debug.rs) |
| Generic/interface dependency generations | [SDK generic reload](../crates/kagari/src/runtime/tests/embedding/generic_reload.rs) |
| Suspended roots, reload and independent cleanup | [SDK async lifecycle](../crates/kagari/src/runtime/tests/embedding/async_execution/lifecycle.rs) |
| Public source-free loading and owned execution | [Artifact consumer](../crates/kagari/tests/artifact_features.rs) and [async consumer](../crates/kagari/tests/async_execution.rs) |

IP01 records exact filters after inspecting the current test layout. Add cases only
for uncovered contracts, such as reused free slots after trimming or another root's
retirement while a host callback retains its caller. Reuse numerical overflow and
ordered-effect conformance for IP05 and native mutation/borrow owners for IP06.
IP05 fusion additionally requires targeted internal-boundary resume/GC/observer
coverage from its declared PC model, including alias mutation between slices;
successful uninterrupted execution alone cannot establish fused correctness.
Changed pointer kernels, if any, require focused Miri/sanitizer evidence under the
architecture's bounded-unsafe policy. A faster unchecked path is not acceptance.

Two existing focused commands for IP02 and the direct-call part of IP03 are:

```text
cargo test -p kagari-runtime --lib -- frame::values::tests::
cargo test -p kagari-vm --lib -- tests::execution::frames::deterministic_frames_
```

These do not cover the complete selected scope: IP03 must also select interface,
generic reload, cancellation, host reentry and a source-free boundary; IP04 adds
the affected async lifecycle cases. Use --no-fail-fast for multiple selected test
targets, preserve default Cargo parallelism and complete any skipped selections.

Keep each implementation checkpoint buildable. Attempt focused checks at changed
ownership boundaries; record failures, diagnostics, skipped cases and their repair
owner without repeating unchanged known failures. No stubs or weakened assertions.
Run the repository's local implementation acceptance checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Review imports, visibility/re-exports, effective LOC and module ownership manually
as required by [AGENTS.md](../AGENTS.md). Do not turn this into unrelated cleanup.
Full-workspace tests and exhaustive feature/backend/platform validation remain
CI-only. Documentation-only planning needs content, link and diff checks.
Use Conventional Commits with Plan-Phase: IPxx trailers for implementation phases.

## Progress and decisions

- [x] Planning: owners, phase order, finite experiments and acceptance recorded.
- [x] IP01: fresh baseline, attribution, frozen controls, gate checker and test selection.
- [ ] IP02: explicit window-slot reuse and local acceptance.
- [ ] IP03: unified prepared calls, transfer and return with measured acceptance.
- [ ] IP04: session storage decision and accepted implementation or deferral.
- [ ] IP05: typed instruction decision and accepted implementation or deferral.
- [ ] IP06: selected library transition decision and implementation or deferral.
- [ ] IP07: integrated local acceptance and documentation handoff.

Current decisions: OM retains object-storage scope; IP02/IP03 are required once
activated; IP04-IP06 are conditional. No new production cache, second semantic
engine or universal representation change is authorized by a conditional proposal.
IP01 starts from b8f83cde, with production storage unchanged since 35077323.
OM remains queued; the accepted predecessor is RS, not an invented OM checkpoint.
This explicitly selects the stable existing storage baseline instead of interleaving
an unstarted OM migration. No heap-storage optimization is included in IP scope.

Activation environment: Intel Core i7-10700F (8 cores/16 logical processors),
Windows x64 MSVC target, rustc 1.98.1 (48a229cea, LLVM 22.1.8), Cargo 1.98.1.
The initial ordinary release benchmark build passed (3m18s), with default target
and parallelism; build time is excluded from execution measurements. Baseline
tooling, diagnostics, frozen controls and correctness selection are recorded below.

IP01 tooling now aggregates process medians and validates alternating pair order,
complete row/sample identities, checksums, frozen source/binary/configuration and
both timing ratios. The fixed inconclusive band is 0.01 below each gate.
Estimator/schema/invalid-evidence coverage passes (twelve Python tests).
Focused window contracts pass (six tests); the manual timing probe is intentionally
ignored in correctness runs and was run separately in release mode.

IP03 admission inventory, refreshed against the current implementation:

| Current check/work | Guarantee owner and intended treatment |
| --- | --- |
| `push_prepared_call`: runtime/top-frame validation, PC-key lookup and canonical instruction reread | Active session retains runtime/quarantine/borrow checks. Verified caller preparation owns ordinal and closure/interface/shared call facts. |
| `push_admitted_arguments`: candidate-argument isolation, suspension entry and call-depth admission | Keep candidate isolation and active execution policy at the session boundary. A sealed transfer must not bypass candidate or owned-resume restrictions. |
| `ExecutionFrame::new`: target/arity, environment shape and native layout selection | Callee/application preparation owns reusable signature/layout facts; dynamic selected application and generation must still match. |
| `ExecutionFrame::new`: heap arguments, executable/environment metadata validation and linked-function lookup | Explicitly prove the live pinned owner before relocating repeated checks. Keep heap ownership at external ingress and transactional parameter/root publication. |
| `ExecutionValues::allocate`: argument range/representation, generation/capacity and initialization | Frame storage owns checked identities, bounds, rollback and initialized roots. Prepared scalar transfers may reuse checked representation facts, not skip dynamic range requirements. |
| `finish_return`: adapted result while callee is rooted, retirement, caller transfer | Preserve one return protocol and adapter failure/GC behavior; caller destination remains separate from reusable callee/application templates. |

Selected correctness commands, run only in their affected phase (not one broad
local sweep): IP02 uses the six frame-value contracts above. IP03 adds the three
VM filters `tests::execution::frames::deterministic_frames_`,
`tests::execution::frames::reload_preserves_active_old_epoch_` and
`tests::reentry_debug::nested_breakpoints_` with `cargo test -p kagari-vm --lib --`;
SDK `runtime::tests::embedding::generic_reload::retained_generic_` with
`cargo test -p kagari --lib --`; and `cargo test -p kagari --no-default-features
--test artifact_features -- source_free_try_carriers_execute_with_selected_residual_calls`.
IP04 additionally selects SDK `runtime::tests::embedding::async_execution::lifecycle::lifecycle_reload_contract`
and the public `cargo test -p kagari --test async_execution -- sdk_owned_native_wait_contract`.
These cover caller isolation/depth unwind, pinned applications, callback reentry,
source-free adapters and waiting/cancellation cleanup. IP05/IP06 selections remain
conditional on their concrete retained changes; exhaustive matrices stay in CI.

Frozen control design (before production changes): private windows use
depth/width/order `(1,0,tail)`, `(32,8,tail)`, `(256,128,tail)`, `(256,0,random)`,
`(256,8,forward)`, `(256,128,random)`. Widths alternate scalar i32 and managed
Generic operands carrying i32 values; fragmentation retires/replaces even slots.
Owned-session controls park populations 1 and 32 in tail order, and 256 in forward
and random order, at a one-instruction slice boundary, then independently complete
or cancel them. Random order uses xorshift seed `0x4b4147415249`. Ordinary timing
uses three warmups/eleven samples; diagnostics run three equal reuse cycles.
Private storage timing batches 32 cycles per sample, summing the three timing
regions separately to avoid zero medians for the smallest windows.
These are fixed mechanism controls, not additional Lua-parity rows. Diagnostic
bank bytes count logical initialized/copied bank payload, excluding allocator
growth and window/order metadata; allocation accounting includes retained capacity.

Initial IP01 attribution (Windows environment above; diagnostic builds, no latency
claim) confirms that ordinary tail calls move zero bank bytes:

| Warm complete execution | Frame admissions | Slots examined | Bank bytes initialized | Rust allocation requests / requested bytes |
| --- | ---: | ---: | ---: | ---: |
| calls | 10,002 | 20,001 | 360,090 | 7 / 151 |
| fibonacci | 21,892 | 306,001 | 1,182,132 | 7 / 155 |
| forms helper | 5,001 | 5,000 | 180,072 | 7 / 152 |
| forms interface | 5,001 | 5,000 | 225,120 | 5,011 / 40,618 |
| forms shared_generic | 5,001 | 5,000 | 205,147 | 10,011 / 120,628 |

All five warmed rows have zero preparation counters for method/interface/shared
applications and zero environment allocations. Thus direct-call allocation is
already bounded, while interface/shared entry still allocates despite prepared
selection; IP03 must attribute those requests rather than add another target cache.
The 256-root random completion and cancellation controls each move 3,712,392 bank
bytes across three equal cycles and examine 97,920 slots. All eight owned-root
cases reach a stable empty capacity by cycles two/three and return to zero net
requested bytes on runtime teardown. Compilation/artifact preparation is outside
this lifecycle counter; runtime creation/linking is its separately recorded setup.
An initial overbroad diagnostic counted compilation too; it was stopped before
baseline freezing and replaced by this scoped measurement.

Implementation validation: twelve Python evidence tests, the six window
contracts, the ordinary owned-root checksum/cancellation run, full original/forms
diagnostic runs and both default-workspace and diagnostic runtime/benchmark Clippy
pass. Clippy initially found one missing `execution_controls` field in the existing
benchmark test initializer (E0063); it was repaired and both Clippy scopes completed.
Windows emitted an informational `linker_messages` warning for the derive DLL;
Clippy returned success. The release private-window/count probes, three existing
metadata lifecycle probes, new prepared-execution retention probe and original
storage timing/allocation controls also pass. The added preparation probe is
separate from compilation: five call descriptors retain 43,794 executable/metadata
bytes; runtime creation/linking adds 52,583 bytes; warmed teardown is zero.
The 512-Point storage control retains 58,092 live, 40,516 empty and 66,700 reused
bytes above a 5,186,413-byte loaded setup on Windows. All sixteen existing storage
population/control teardown checks return zero. These are current-machine controls,
not elapsed-time comparisons with RS's Apple measurements.

### IP01 frozen evidence and reproduction

This checkpoint contains the complete fixture/tooling source; its production
semantics are unchanged from b8f83cde/35077323. The frozen ordinary matrices each
use one fresh process to verify/preserve the contract (not acceptance), with three
warmups and eleven samples; original setup has three samples and forms setup one.
All later acceptance reruns both saved baseline and candidate in at least seven
alternating pairs. No production storage/call change preceded freezing.

Saved binaries under ignored `target/interpreter-performance/`:

| Binary | SHA-256 |
| --- | --- |
| `ip01.exe` (original/forms/owned controls) | `69f82baf0ad67534fc605576cfd935f04cc6e1671b2c4887818402cbe8fed327` |
| `ip01-windows.exe` (ordinary private storage) | `f8a5d3c477a1fe533b9607bbe22d37322365dea43b977e27826607341a620997` |
| `ip01-storage.exe` (unchanged RS probe) | `f1558dfcafc7c800a889f0e56c2d2618d4158a5b69570db1016895eb4ccea6f9` |
| `ip01-diagnostics.exe` (protocol/allocation counts) | `8f60f25257f26452e8f9ee80cade593204ed72d721affa391d91da769cb9dc64` |
| `ip01-runtime-counts.exe` (window/metadata counts) | `d16136cf73c645a36f19f962ad2892012d93282bd2e596655e1595f4f431762a` |
| `ip01-preparation-counts.exe` (added executable preparation probe) | `bdd5d7fd75535c4d9776e89fb539464b457f6bb7f73aaa0e69f1a7b1fa32c954` |
| `ip01-storage-counts.exe` (RS allocations) | `742b2e60ba5681b9775edb0f61e951a4b8ee63374c8570786546ef729c8e3c62` |

The preparation-retention probe was added afterward without changing any frozen
fixture or production semantics; its separate diagnostic binary is also retained.
Do not use diagnostic binary times. Four frozen JSON reports preserve exact
machine/toolchain/features/lockfile/fixture hashes and raw checksums. The tracked
driver recreates those reports; this ledger retains the method and conclusions if
ignored files are cleaned. Preserve the accepted binaries before a production edit.

Baseline VM/Lua execution medians in microseconds (single-process context only):

| Matched workload | VM | Lua |
| --- | ---: | ---: |
| arithmetic | 2,042.7 | 884.9 |
| branches | 2,430.3 | 1,433.1 |
| calls | 6,749.9 | 438.6 |
| fibonacci | 14,225.4 | 473.0 |
| arrays | 4,060.6 | 137.5 |
| maps | 5,564.9 | 136.6 |
| forms direct | 283.2 | 118.1 |
| forms helper | 3,403.6 | 198.5 |
| forms concrete_generic | 3,596.3 | 204.9 |
| forms interface | 7,311.1 | 216.1 |
| forms shared_generic | 9,116.8 | 207.3 |
| forms capture_cell | 8,433.5 | 217.5 |
| forms field | 1,149.8 | 184.0 |
| forms byte_state | 6,898.2 | 223.4 |
| forms string_constants | 1,045.2 | 146.2 |
| forms string_calls | 4,916.7 | 218.1 |

Entry, host_callback and the unmatched native row remain separate controls.
No speedup, significance or Lua parity is claimed from this freeze.

Logical profile passes retain Value/ExecutionInstruction sizes of 16/24 bytes
(canonical instruction 136 bytes, return packet 24 bytes). Observed logical
instruction totals: arithmetic 550,015; branches 600,022; forms direct 75,015;
string_constants 100,012; byte_state 175,022. These use the existing observer and
ten-second profiling window separately from throughput. No fresh CPU sampling
profile was collected on Windows; logical counts do not establish inclusive CPU
time or predict a specialization win. IP05 must close that uncertainty before
retaining an instruction experiment. IP06 must account for byte_state's 15,001
logical calls but only one admitted frame: installed primitives already consume
these calls inside the admitted region.

Reproduction (default Cargo target/profile/parallelism, same machine and toolchain):

```text
uv run python scripts/benchmark_lua.py --interpreter-only --runs 1 --freeze-baseline target/interpreter-performance/ip01.exe
uv run python scripts/benchmark_lua.py --source-forms --runs 1 --freeze-baseline target/interpreter-performance/ip01.exe
uv run python scripts/benchmark_lua.py --execution-controls --runs 1 --freeze-baseline target/interpreter-performance/ip01.exe
uv run python scripts/benchmark_lua.py --window-controls --runs 1 --freeze-baseline target/interpreter-performance/ip01-windows.exe
cargo build --release --locked -p kagari-lua-benchmark --features diagnostics
target/release/kagari-lua-benchmark.exe --diagnostics
target/release/kagari-lua-benchmark.exe --diagnostics --source-forms
target/release/kagari-lua-benchmark.exe --diagnostics --execution-controls
cargo test --release -p kagari-runtime --lib --features execution-diagnostics diagnostics::memory -- --ignored --nocapture --test-threads=1
cargo test --release -p kagari-runtime --lib --features execution-diagnostics frame::values::tests::measurements::window_storage_counts -- --exact --ignored --nocapture --test-threads=1
cargo run --release --locked -p kagari-lua-benchmark --example runtime_storage
cargo run --release --locked -p kagari-lua-benchmark --example runtime_storage --features diagnostics
target/interpreter-performance/ip01.exe --interpreter-only --profile=arithmetic
target/interpreter-performance/ip01.exe --interpreter-only --source-forms --profile=direct
```

Use a fresh destination when recreating the frozen snapshots; the driver refuses
to replace existing reports or a different binary. Other recorded profiles use
`branches`, or source forms `string_constants`/`byte_state`. The gate checker is
`uv run python scripts/check_benchmark.py <paired-results.json>`; IP03 adds targets
`calls=0.90`, `fibonacci=0.90`, `forms_interface=0.90`, `forms_shared_generic=0.90`
to their respective matrix runs. Focused original comparisons can select a single
workload against the full frozen original report. Correctness/byte gates remain
separate from this timing checker. All broader feature/backend/platform CI remains
unrun and is not implied by IP01 completion.

Review decisions: caller locations and callee templates have separate owners;
IP01 supplies validated process/pair statistics and explicit setup sampling; memory
limits cover performance targets; fusion requires a complete internal-PC resume
and root model before prototyping. These requirements refine the existing phases
without changing OM scope.

### IP02 work in progress

Window identity storage now owns an intrusive free chain in vacant slots, with
backlinks so trailing holes can be removed regardless of release order. Allocation
selects the free head or reserves a new slot; it no longer scans live windows or
reserves extra window capacity when a hole is available. Generation/owner validation,
transactional argument admission, bank initialization/compaction and root retirement
remain with their existing owners. No session-bank segmentation is included.
One additional contract case combines non-head trailing-hole removal, failed
admission, later reuse and stale-identity rejection at zero and nonzero widths.
The seven focused frame-value contracts and structure check (1,278 Rust files,
zero violations) pass. The release allocation probe covers all six frozen cases;
every live/empty/reused snapshot has no increase in retained bytes or allocation
requests/bytes, warm cycles add zero requests, and teardown remains zero. Bank
initialization/movement counts are unchanged; identity-table search probes are zero.

| Window depth/width/order | IP01 retained bytes after reuse | IP02 candidate |
| --- | ---: | ---: |
| 1 / 0 / tail | 736 | 736 |
| 32 / 8 / tail | 14,720 | 9,088 |
| 256 / 128 / tail | 501,760 | 456,704 |
| 256 / 0 / random | 92,160 | 47,104 |
| 256 / 8 / forward | 117,760 | 72,704 |
| 256 / 128 / random | 501,760 | 456,704 |

Compare all six snapshots (live/empty over three cycles) from the frozen and current
`window_storage_counts` outputs, including cumulative requests/requested bytes and
zero teardown. The savings remove unnecessary table growth during hole reuse;
they do not come from smaller Value payloads or session-bank segmentation.
The initial seven private-window process pairs did not pass: six timing gates
failed and one was inconclusive. For example, 256/8 forward retirement had
aggregate/paired ratios 1.1971/1.0731, and 256/128 random retirement 1.1403/1.0409.
The 256/0 allocation gate passed at 0.5884/0.5733. Do not summarize the mixed result
as an accepted speedup. Process drift is large (random wide-window baseline
retirement medians 568,916-765,850 ns); several paired medians are near 1 while
aggregate medians fail. Code review confirms unchanged bank movement and no new
allocation; free-link maintenance and changed occupied/vacant iteration remain
possible retirement costs. Further measurement must distinguish those costs from
drift before another local optimization.

Predeclared continuation: add fourteen pairs to the same candidate (twenty-one
total), preserving all initial samples and alternating order. The driver now
supports `--extend-report <initial-results.json> --runs 21`; it rejects candidate,
baseline or configuration changes and retains the prior metadata/report hash.
No production change is allowed between those measurements. Other matched
timing/allocation and local checkpoint acceptance remain pending; IP02 is not
accepted yet.

The unchanged-candidate continuation completed with all 21 pairs retained. Seventeen
of eighteen private-window timing gates now pass, including forward and random
retirement; single zero-width admission still fails at aggregate/paired
1.0769/1.0385. This isolates a small fixed admission cost instead of supporting
the initial broad retirement-regression hypothesis. Review the extracted window
table's reserve/publication boundary and generated code for repeated branching or
Window movement before changing it. Assembly emission is diagnostic only; its
build/run output cannot replace the frozen ordinary benchmark configuration.

Generated MSVC assembly confirms out-of-line calls to both `reserve_slot` and
`insert`; admission constructs a 176-byte Window stack temporary and `insert`
copies it with `memcpy`. The free-chain ownership itself remains sound. Inline
hints on these two admission/publication methods expose the existing checks and
construction to the compiler without another cache, semantic path or model change.
This is a new candidate: collect a fresh seven-pair comparison, retaining the
earlier failed candidate's 21-pair report separately. No earlier sample is merged
with a different binary.

The inline-hint candidate's seven pairs still fail single-window admission
(aggregate/paired 1.0769/1.0769), with mixed retirement results. A detached IP01
checkout and ordinary library assembly show that the baseline also constructs
and copies a Window through Vec publication; copying alone is not a newly
introduced cost. The hint removes `reserve_slot`'s call, but `insert` remains
out of line. A bounded follow-up forces this single-caller publication helper
inline and collects a separate seven-pair report. Assembly emission is not the
test-harness benchmark binary and only supports the boundary diagnosis.
The continuation tool's thirteen validation tests pass, including rejection of
changed configuration/binaries and missing earlier rows. All earlier failed
reports remain available; none establishes IP02 acceptance.

Forced publication inlining passes seventeen of eighteen window gates, including
all retirement/fragmentation rows. Single zero-width admission remains 1.0800
on both measures. This rejects the hypothesis that the helper boundary alone
explains that regression. Admission also re-reads its just-published ranges via
the generational handle; exclusive storage access already retains the same facts.
The next candidate keeps the locally constructed ranges for argument publication,
removing that redundant lookup without changing consumer identity checks, argument
validation or transaction boundaries. It requires a fresh comparison.

Keeping publication ranges locally resolves the shallow admission gate in seven
pairs (0.9643/0.9630); deep zero-width admission is 0.5308/0.5358. Seventeen gates
pass, but 256/128 random retirement is 1.0873/1.1028. That path was unchanged by
the range edit and passed the preceding candidate, so retain these results and
extend this unchanged binary to twenty-one pairs before diagnosing another
retirement change. This is not a passing checkpoint yet.

The twenty-one-pair continuation passes all eighteen private-window timing gates.
Selected aggregate/paired ratios are: single admission 1.0000/0.9643; 32/8 tail
admission 0.8320/0.8289; 256/0 random admission 0.5361/0.5344; 256/128 random
retirement 1.0276/1.0042; 256/8 forward retirement 1.0106/0.9975. All original
seven pairs are included. These are window-control results, not interpreter/Lua
parity or IP03 call-lifecycle acceptance. Focused calls/fibonacci, owned-root
controls, allocation diagnostics and local checkpoint checks follow before IP02
acceptance; all sixteen unchanged workloads remain the IP07 integration matrix.

Read-only IP03 preparation during the sequential timing run confirms two distinct
issues. The VM decodes a canonical call after region exit, then runtime call entry
looks up the PC tree and decodes dynamic contracts again. A prepared call transition
must reset the driver's cached action just as today's Call progress does; treating
it as an ordinary safepoint would mishandle a newly entered native frame. Ordinary
synchronous native calls currently consume one logical instruction using caller
roots, while generic native frames have explicit entry/return actions. Preserve
that distinction in the unified protocol rather than adding observable steps.
Separately, ApplicationKey construction allocates construction identities for each
applied call, and interface compatibility wraps a cloned nominal expression in Ty.
Both occur even when preparation counters remain zero. Attribute their measured
allocation deltas before replacing them; immutable identity preparation and borrowed
type comparison must preserve pinned provenance and avoid untraced strong edges.

The first seven real calls pairs pass execution (aggregate/paired 0.9778/1.0032),
compilation, artifact preparation and program linking. Two initialization rows
fail: Kagari runtime initialization is 1.1236/1.1624 (17.8 us versus 20.0 us
aggregate medians), and unchanged Lua module initialization is 1.2500/1.1765
(1.6 us versus 2.0 us). Neither is waived or evidence of an accepted checkpoint.
After the already queued fibonacci and root-control comparisons finish, extend
the unchanged calls binary to twenty-one pairs with all initial samples retained.
The initialization change has no established cause yet; broadening samples must
precede a new production change. Final allocation diagnostics, focused tests,
workspace Clippy, formatting and structural review are still pending.

Memory reduction must compare root lifecycle increments after subtracting each
process's initial loaded-setup snapshot, as well as checking setup separately.
Comparing only absolute snapshots would let the multi-megabyte fixed program
cost hide a growing window/session footprint. Compare cold live/empty and both
reuse cycles against IP01, with zero teardown, stable warm empty capacity and
nonincreasing warm request/count deltas. Private-window probes already exclude
loaded setup from their accounting region.

Fibonacci's seven pairs pass all ten timing gates. Execution aggregate/paired
ratios are 0.9665/0.9707; runtime initialization is 0.8592/1.0000 and unchanged
Lua module initialization 0.9375/1.0000. The opposite initialization result from
the same candidate reinforces the need for the predeclared calls continuation;
it does not erase the failing calls evidence. Owned-root timing is still running.

The seven owned-root pairs pass 21 of 24 gates, including all admissions and
256-root completion/cancellation retirement. Three small gates fail narrowly:
single-root retirement is 1.0526/1.0526 (cancel) and 1.0588/1.0588 (complete),
and collection after forward cancellation is 1.0522/0.9871. Predeclared extension:
retain all seven pairs and continue the same candidate to twenty-one root pairs,
after the calls continuation. No production edit occurs between these runs.

While those sequential measurements run, an isolated checkout at IP01 is used for
an unbuilt IP03 draft. It replaces the PC tree with dense caller descriptors and
a private runtime call transition, retains closure/interface/shared contracts in
preparation, and keys scoped interface preparation by function/call ordinal. The
VM's duplicate prepared-call dispatch entry is removed, with explicit action
invalidation on callee entry. This is preparatory work only: callee transfer/admission
and allocation work remain, the draft is not merged into the IP02 candidate, and
no build or performance claim is attached to it. Build/test work waits until all
ordinary measurements finish.

IP03 draft review found an ownership constraint: VerifiedProgram normalization
shares physical ExecutionModule data while remapping canonical definition IDs.
Therefore shared/interface contracts and scoped closure signatures belong to
runtime-local LinkedFunction records, populated from the normalized module once.
The shared caller table retains only ordinals, physical facts, module/function
slots and source PCs for linking/debugging. Linked contracts retire with their
supplying program, and closed interface dependencies keep their existing traced
edges. No pre-normalization Ty or callable contract may enter the shared table.

The final IP02 timing continuations retain every earlier pair. Calls passes all
Kagari rows; execution is 1.0132/1.0065 and runtime initialization 1.0374/1.0223.
Unchanged Lua module initialization remains failing at 1.0588/1.0556 (1.7 versus
1.8 us). Roots passes 23/24 rows; only single-root cancellation retirement remains
1.0526/1.0526 (1.9 versus 2.0 us). Both differences are one reported 100 ns quantum;
that observation does not prove absence of a real regression. The seven release
frame-value contracts pass, with the manual timing test intentionally ignored.

Checkpoint decision: after IP02's selected correctness, allocation and code-quality
checks finish, integrate the required IP03 replacement into the same unpublished
implementation checkpoint. IP02 is implemented but remains unaccepted, and neither
remaining timing failure is waived or changed into a pass. Preserve its frozen
candidate/reports and carry both gates into the combined IP02/IP03 measurement.
This groups acceptance instead of repeatedly optimizing an intermediate binary's
single-clock-quantum rows. Phase requirements, the IP01 baseline, all numerical
thresholds, and the final sixteen-workload matrix remain unchanged. No production
checkpoint is accepted until its complete applicable gates pass.

The isolated IP03 draft passes `cargo check --locked -p kagari-vm --test
allocation_contracts` and the structure checker (1,278 files, zero violations).
The existing warm script-window allocation contract now enters through the region's
Call exit and rewinds the logical PC for repeated measurement, instead of using
the removed public prepared-call entrypoint; its zero-allocation assertions remain.
These are compile/static results only; behavioral tests and merged IP02/IP03
measurement remain pending. The draft patch applies cleanly to the current worktree.

IP02 local implementation checks now pass: seven release frame-value contracts,
thirteen benchmark-evidence tests, allocation diagnostics (14 original records,
72 source-form/protocol records, eight root cases and six private-window cases),
formatting, structure (1,278 files), workspace all-target Clippy and whitespace.
All root/window teardown and warmed capacity checks pass; warm requests/bytes do
not increase. For diagnostic net-negative GC deltas, compare against the unchanged
baseline delta (no increase), not baseline * 1.05, which incorrectly demands extra
deallocation even for identical results. The reducer was repaired and the original
raw results reused; no runtime failure was hidden or measurement rerun.
The preserved ordinary IP02 candidate SHA-256 is
`24fea08dbb88b3581769791df3bca4e7d9d40534fb6f72626fcde79c08896125`.
The two stated timing gates still fail, so IP02 acceptance and all exhaustive CI
remain pending. Proceed with the declared combined IP02/IP03 implementation checkpoint.

The ordinal/runtime-contract changes are now integrated. Focused behavioral checks
pass: the two VM deterministic-frame contracts (caller isolation and depth unwind),
the runtime witness-preparation/retirement contract, and the VM allocation contract
for warm reordered/repeated window arguments and scalar returns (zero requests).
Commands use `cargo test --locked` with the package/library or allocation_contracts
target and exact named filters; no workspace test sweep was run. The ownership
split survives these selected runtime/definition-scope paths. Full callee admission,
parameter-transfer templates, remaining warm application allocations and the required
IP03 speed gates are still outstanding, as are its broader selected boundary tests.
Evidence continuation tooling is committed separately at 530ef082; that commit
contains no runtime implementation or phase-acceptance claim.

The next IP03 integration replaces per-entry canonical parameter placement with
`CalleeLayout`, prepared for script and native targets. It owns register/local
storage, parameter offset/count and generic/suspension flags; normalized binder
identities remain in the runtime's immutable bytecode contract. Caller argument
locations and return destinations remain separate. Admission composes these facts
by borrowing, without storing caller offsets or active window identities in the
callee. Scalar/managed conversion continues through the existing checked operand
implementation, including narrow integer ranges and initialized-source checks.
All argument checks and capacity reservations precede mutation. Parameters are
written into the newly reserved bank ranges, then window identity and metadata
roots are published together. The exclusive bank borrow excludes collection and
callbacks throughout this transition. The original frozen window fixture is unchanged.

Two repeated call-time constructions now have preparation owners:
ApplicationArguments owns an immutable construction scope and its shared identity
sequence, so each ApplicationKey shares that prepared identity through Arc instead
of rebuilding a Vec. InterfaceDescription stores its lexical interface expression
as Ty directly; compatibility checks borrow it instead of cloning a NominalTy to
construct a temporary Ty. Existing provenance comparison, dependency tracing and
bounded application retention remain authoritative. These changes have not yet
been measured against the warm allocation or retained-memory gates.

The private call transition now reuses execute_region's runtime/session admission:
the sealed cursor selects an ordinal and drops its borrows immediately before
the transition, with no intervening callback, collection or suspension. It does
not waive callee availability, candidate isolation, argument/heap/metadata checks,
depth or environment generation validation. Those checks retain their existing
dynamic owners. Public entries still admit runtime/session authority. Existing
finish_return remains the single adaptation/retirement/publication protocol;
method/application adapters run with the callee rooted and the caller's prepared
destination remains frame-local.

Focused layout/identity results: seven frame-value tests pass (manual timing test
ignored); all 27 selected application/interface/group tests pass. The VM's two
deterministic-frame tests, nested breakpoint/trap reentry test and warm argument
window allocation test pass. A mistyped reload filter in this invocation matched
no test; the correct filter already recorded above must be run explicitly, not
counted as covered. Structure passes over 1,280 Rust files. The subsequent private
admission reuse still needs its affected VM reentry/authority checks; SDK retained
generic, source-free carrier checks, workspace Clippy, allocation diagnostics and
ordinary paired measurements remain pending. IP02/IP03 acceptance is unchanged.

The subsequent private-admission checks pass: the VM old-epoch call, nested
breakpoint/trap reentry and foreign/stale reentry-input tests (three tests), plus
the SDK retained generic default/override and try-carrier reload contracts (two
tests). Workspace all-target Clippy with `-D warnings` passes. The source-free
try-carrier test fails before execution at artifact decoding with
`module identity path segment count limit exceeded`. Its input is the disposable
`target/fixtures/feature_artifact.kbc`; regenerate it using
`cargo run --locked -p kagari --example regenerate_feature_artifact`, then rerun
the same no-default-features named test. This is an outstanding local check until
that rerun succeeds; no compatibility reader, version bump or assertion change
is warranted. The initial failed command and its diagnostic are retained here.
The fixture was regenerated successfully and the identical source-free test now
passes (one test, unchanged source/assertions). This closes the local artifact
decode failure; it was a stale disposable product, not a call execution failure.
The diagnostic benchmark build is next; no IP03 timing claim is yet available.

The first integrated layout diagnostic candidate passes the unchanged memory
reducer against IP01: 14 original records, 72 source-form/protocol records, eight
root controls and six private-window controls. Warm allocations do not increase;
capacity reuse plateaus and teardown is zero. Isolated executable retained bytes
fall from 43,794 to 43,011; runtime-linked bytes rise from 52,583 to 52,983 (0.76%,
within the 5% gate). Physical instructions remain 24 bytes; the execution frame
is 240 bytes. These are release allocation diagnostics, not throughput timings.

For 5,000 warm iterations, shared_generic requests fall from 10,011 / 120,628 bytes
to 5,011 / 40,628 bytes. Interface remains 5,011 / 40,618 bytes and helper remains
7 / 152 bytes. Helper's 5,000 slow call boundaries disappear; interface and
shared_generic each retain only their one non-call slow boundary. Driver admission
counts remain 10,001, so removing canonical call dispatch is not equivalent to
removing all driver transitions. The remaining eight-byte per-call request is
attributed to TypeView::closed invoking Ty::is_concrete's temporary worklist on
the lexical interface expression. Its already-substituted closed type belongs to
InterfaceDescription. The next edit stores that existing closed nominal type as
Ty too and supplies both expression and checked closed type to TypeView::prepared,
preserving the original implementation/environment provenance. This replaces the
repeated concrete-type discovery without changing the common type comparer.
Repeat affected interface/provenance tests and allocation checks for this edit;
the preceding passing memory report does not yet validate the newer representation.

The checked-interface representation now passes those 27 tests and the source-free
try-carrier test, plus workspace all-target Clippy. Two multiline test/diagnostic
consumers initially failed to compile after the field changed from NominalTy to
Ty; both now use the nominal accessor, without changing their assertions. No
integration error remains. The final 5,000-iteration interface/shared_generic warm
counts are respectively 11 / 618 bytes and 11 / 628 bytes, versus IP01's
5,011 / 40,618 and 10,011 / 120,628. Closed script helper remains 7 / 152 bytes.
Prepared/linked retention remains 43,011 / 52,983 bytes with zero teardown.
The 14 original and 72 source-form/protocol allocation records pass again. The
unchanged root/window storage paths reuse the preceding eight/six passing controls;
this representation-only follow-up does not rerun their identical diagnostic cases.
All these measurements use the recorded release diagnostic configuration and are
allocation evidence only. Ordinary timing will collect seven alternating pairs
for source forms, calls and fibonacci against the frozen IP01 executable, using
the predeclared 0.90 target and common 1.05 gates. Preserve every pair and record
failures before deciding further changes; IP02's two carried gates still apply.

The seven-pair source-form timing run fails acceptance. Aggregate/paired ratios
are interface 0.9016/0.9081 (target 0.90, fail), shared_generic 0.8916/0.8847
(inconclusive inside the fixed band), byte_state 1.0802/1.0782 (common-gate fail),
and runtime initialization 1.1712/1.1273 (fail). Other rows pass; helper is
0.9445/0.9452, concrete_generic 0.9525/0.9473 and artifact preparation
1.0088/1.0116. All checksums pass. These are the ordinary release configuration,
seven alternating pairs, three warmups/eleven execution samples and the frozen
source-form setup sampling. Removing per-call allocation is therefore insufficient
to accept IP03. Keep this candidate and all seven pairs, complete the already
declared calls/fibonacci measurements without production edits, then diagnose the
remaining transition and byte-state regression before another optimization.

Read-only diagnosis while the calls/fibonacci pairs run: byte_state still has
zero prepared call selections, one admission/window, 231 initialized bank bytes,
15,054 allocation requests / 124,340 bytes, and two slow boundaries, identical
to IP01. Its regression is therefore not attributable to additional call/frame
work on that workload. The new standalone Call arm changes the shared scalar
dispatch loop even for call-free code; generated-code impact remains a hypothesis
to isolate after measurement. Calls still return to the VM to classify each new
frame and restored caller, although the runtime transition just established that
state. Review carrying the established next action from admission/return and
keeping non-scalar call transitions outside the scalar dispatch table. Any such
replacement must preserve await/factory/native return states, reentry, slicing,
per-instruction cancellation/observation and the existing single return protocol.
Do not implement or build during the in-progress ordinary measurements.

The seven-pair calls run is complete, with every checksum passing. Its execution
ratio is 0.9488/0.9497, below IP01 but short of the required 0.90. All Kagari
setup rows pass; unchanged Lua module initialization still fails at 1.0625/1.0625.
This preserves the carried small-timer failure rather than waiving it. The declared
fibonacci run follows using the identical candidate; it is still in progress.
Source-form and calls reports were collected at UTC 20261016T122303Z and
20261016T122833Z respectively; the fibonacci run began at 20261016T123606Z.
No production edits or concurrent builds/diagnostics occurred during these timings.

Fibonacci completes with all checksums passing: execution is 0.9213/0.9178,
short of the 0.90 target; all its setup/Lua rows pass. The ordinary candidate is
preserved with SHA-256
`1267e889575c2bbf386ce31449befa96450aa22409fecd3c0d1b090b399c27aa`.
All three declared runs finished before the next production edit.

The next replacement carries the action established by frame creation and return
directly to the VM. Callee construction returns Script or NativeEntry alongside
the frame; the common return protocol reports either a completed value or the
restored caller's checked state. Factory completion explicitly reports Await after
publishing its queued Future. Initial/resumed activations and post-await transitions
still use next_action; cancellation polling, instruction observation/slicing,
native result rooting and asynchronous readiness are unchanged. No second return
protocol or public unchecked entry is added. Native return consumers and examples
move to the explicit ReturnAction contract instead of retaining an Option wrapper.
Prepared calls now use the non-scalar operation handoff; the standalone Call arms
are removed from ExecutionInstruction and ScalarExit, restoring the scalar loop's
prior dispatch shape while retaining dense call ordinals.

Validation for this replacement extends the existing focused selection with
`generic_native_calls_return_retained_objects_collections_and_closures` and
`frame_roots_preserve_returned_objects_across_calls_and_collection_safepoints`
in VM lib tests, and SDK `sdk_owned_future_factory_contract`,
`sdk_script_future_and_for_await_contract` plus the public async_execution target's
`sdk_owned_native_wait_contract`. These cover the newly carried native/factory/wait
states; they are a bounded affected selection, not a full local matrix. Runtime
application retirement and the warm argument-window allocation assertion retain
their exact result/root expectations with the explicit return action.

This action-carrying replacement passes the six selected VM lib contracts and
the warm argument-window allocation test, both selected SDK factory/script-Future
tests and the public native-wait contract. The three runtime checks also pass:
physical instruction budget, borrowed-operand quarantine and applied-method
retention/eviction with an active return. Workspace all-target Clippy, formatting,
structure (1,280 files) and whitespace pass; the transient unused Value import
reported by the first build was removed. The scalar-loop source is now identical
to IP01. Allocation diagnostics will rerun original/source-form/root matrices;
unchanged private-window and prepared-retention evidence is reused. If those
pass, collect a fresh seven-pair source-form comparison before more production
changes. The previous failed candidate/reports remain preserved, and IP03 is
still unaccepted. Full workspace/backend/platform tests remain CI-only and unrun.

The action-carrying candidate passes the repeated allocation diagnostics: 14
original, 72 source-form/protocol and eight root controls; unchanged six private
window controls retain their previous passing evidence. Warm helper/interface/
shared-generic requests and bytes remain 7/152, 11/618 and 11/628. Driver admissions
fall from 10,001 to one in each of those call workloads; transfers and returns
are unchanged. The ordinary executable SHA-256 is
`0b41e6c2f2a3f4c3507f7399d9a6592b07ee852269db679409131aafb841007c`.
Seven alternating source-form pairs collected at UTC 20261016T130012Z pass
interface (0.8272/0.8246), shared_generic (0.8479/0.8455) and byte_state
(1.0279/1.0228). All checksums pass. Runtime initialization alone fails this
run's common gate (1.0950/1.0769); calls/fibonacci and combined IP02 controls
have not been measured for this candidate. IP03 remains unaccepted.

The remaining IP03 native-call review found that ordinary fixed native calls
discard their linked binding/operand facts and reread canonical instructions at
execution. Replace that fallback with a prepared callback descriptor owned by
the runtime-local linked function, alongside the existing sealed primitives.
The callback remains one logical instruction using the caller's roots, with no
extra native frame. Complete the private transition after cursor/frame/bank
borrows end; preserve installed signature checks, cancellation, reentry, pinned
bindings and checked result publication. This completes existing call preparation
scope, not IP06 library specialization. Keep the scalar dispatch unchanged.
Validate the existing zero-allocation native boundary, callback/reentry and
deterministic stepping contracts, then repeat affected retention/allocation
checks before timing. Runtime initialization still needs diagnosis and a passing
matched comparison; no failed gate is waived or baseline replaced.

Prepared native callbacks pass both existing warm zero-allocation boundary tests,
the two runtime physical-budget/borrow-quarantine checks, and seven of eight
selected VM contracts (deterministic frames, callback reentry, native result
validation, sequence borrowing, collecting/trapping hash callbacks and pinned
reentry). The native-safepoint cancellation test fails again in a focused rerun:
its second invocation reports EngineFault. Inspection finds an existing fixture
ABI mismatch, also present in HEAD: constant_i32 accepts a Runtime pointer while
the installed ABI passes NativeInvocationContext. Fix the fixture to extract its
runtime from that context and type-check the entry as JitCompiledFunction before
erasing the address; keep all cancellation/result assertions. The added affected
host-effect cancellation contract already passes. This is a test-fixture repair,
not an ABI/version change or relaxed validation. Rerun the failed native case;
workspace Clippy and affected allocation/retention evidence still follow.

The repaired native cancellation case passes. Nested callback breakpoint/trap
traces also pass, completing this native follow-up's selected correctness scope.
Workspace all-target Clippy succeeds (the previously documented Windows linker
informational warning remains), structure reports 1,280 files with no violations,
and formatting/whitespace pass. Repeated diagnostic original/source-form records
(14/72) pass the unchanged allocation gates; unchanged root/window paths reuse
their preceding eight/six controls. No new local failure remains.

The new callback record adds a boxed physical-source list at link time, with the
binding retained by the existing runtime-local linked-function owner; no new
per-call heap request is introduced. Recheck the existing runtime_storage allocation
probe's separately attributed loaded setup and 16 lifecycle controls, since the
call-only preparation fixture does not exercise fixed native arguments. This
reuses the frozen workload and baseline, not a new acceptance fixture.
After those gates pass, collect fresh seven-pair source-form, calls and fibonacci
comparisons with the ordinary release build before further production edits.
The required targets remain 0.90, all common gates 1.05, and every pair is retained.
Runtime initialization still has no established root cause: its last paired raw
measurements range 21.3–35.7 us for IP01 and 23.3–34.7 us for the candidate. The
native-installation cloning/host registration path is unchanged; only the empty
window-table state gains a free-head word at construction. This source review
does not establish that the observed regression is noise or acceptable.

The unchanged runtime_storage probe passes all 96 phase records, three warm field
loops and 16 zero-net teardown assertions against IP01. The 512-Point loaded
setup retains 5,187,053 bytes versus 5,186,413 (+640, about 0.012%); subtracting
setup leaves live/empty/reused storage unchanged at 58,092/40,516/66,700 bytes.
Both requested-byte and request-count checks pass, including setup and each
separately attributed lifecycle increment. The declared seven-pair timing sequence
now follows with no further production edits or overlapping builds/diagnostics.

The native fixture ABI repair is committed separately at e0bb3601. It does not
accept IP03 or change the ordinary benchmark executable. The current native-call
candidate is preserved with SHA-256
`38319cdec068c20d39ace105faf2b82b3e7fac8fca6eaae583d54d3293aaa120`.
Its seven-pair source-form run (UTC 20261016T133211Z) passes all checksums and
the interface/shared_generic targets: 0.8482/0.8482 and 0.8515/0.8501.
Helper is 0.8944/0.8977, native 0.7864/0.7864 and host_callback 0.7713/0.7759;
the latter two remain separate controls, not additional parity workloads.
Byte_state passes at 1.0148/1.0148. Two common gates fail: field execution
1.1960/1.1957 and runtime initialization 1.1422/1.1778. All other rows pass.

Field's diagnostic work is identical to IP01: no calls, one admission/window,
179 initialized bytes, eight requests/214 bytes and one slow boundary. The new
callback descriptor is therefore not doing extra per-field work. Review generated
code and the boundary between the closed cursor loop and callback transition
after the declared timing sequence finishes; altered code generation/inlining is
a hypothesis, not yet an established cause. Do not hide this regression with
another cache or remove callback safety checks. Calls timing began at UTC
20261016T133630Z with this same executable; fibonacci follows before any
production edit/build beyond the driver's unchanged ordinary build. Preserve
all seven pairs even if another row fails. Combined IP02/IP03 acceptance remains
open, including the carried window/root/setup gates and pending complete CI.

The seven-pair calls run completes with all checksums passing. Execution is
0.9027/0.8968, still short of the required 0.90 target and fixed uncertainty band.
Runtime initialization also fails at 1.0777/1.0777; all other rows, including
Lua module initialization, pass. The initialization regression therefore appears
in both the single-sample source-form setup and the three-sample original setup;
it still needs attribution rather than an assumption of noise. The declared
fibonacci comparison is now running with the same binary and frozen workload.
No ordinary throughput measurement has overlapped builds, tests or diagnostics.

Fibonacci's seven pairs (UTC 20261016T134451Z) complete with all checksums
passing. Execution passes at 0.8721/0.8734; only runtime initialization fails
(aggregate 0.9947, paired 1.0862). The two estimators disagree on that setup row,
so the evidence does not establish a uniform initialization slowdown, but it
still fails the declared gate. All three planned ordinary runs are now complete;
compiler assembly diagnostics start afterward. Required calls, field regression
and setup gates remain open; the passing fibonacci target is not full acceptance.

Compiler inspection finds that the native invoker parameter remains outside the
closed kernel; the field handler itself has the same 448 machine instructions
as the retained IP01 diagnostic assembly, apart from metadata offsets and exit
tags. The closed kernel changes register/stack placement, but this alone does not
prove the cause of the 20% field regression. Both versions eagerly copy 24 bytes
of general CursorExit payload after every field operation, including completion.
That handler only needs to report completion or request preparation for the
already-known ordinal. Replace its general optional exit packet with those two
states; the kernel remains the sole owner of constructing the transition. This
local boundary repair preserves field storage, ordering, traps and scoped-layout
preparation. Do not expand this into a second field engine or opcode experiment.

Calls remain below their target. Reviewing argument admission shows repeated
owner/index/generation lookup for the same caller window on each argument and
again during transfer, although one exclusive bank borrow covers the entire
transaction. Resolve its ranges once for this borrow, retaining per-operand
initialization, bounds, representation and checked numeric validation. Ranges
are indices, not pointers, and remain valid through reserve/growth because this
transaction cannot retire/compact another window or call back. Managed admission
uses an immutable borrowed operand view with the same lifetime guarantee. External
Value arguments and receiver/capture prefixes retain their existing checks.
Validate reordered/repeated arguments, stale/foreign windows, failed admission,
checked field mutation and callback/debug boundaries using the existing owners.
Runtime initialization is still unresolved; constructor assembly has the same
524 instructions (stack grows 5,064 to 5,080 bytes), and NativeInstallation::runtime
is identical. No initialization speed claim follows from instruction counts.

The field continuation and borrowed argument changes pass eight runtime storage/
quarantine tests, seven selected VM contracts (deterministic frames, checked scalar
fields, nominal/scoped fields and nested callback debugging), and both warm argument/
native zero-allocation tests. Workspace all-target Clippy, structure, formatting
and whitespace pass for those changes. Existing stale-source admission and failed
narrowing assertions remain unchanged. The private storage timing test remains
intentionally ignored during correctness checks, with its earlier timing evidence
owned separately.

Diagnostic assembly confirms the 24-byte field exit copy is removed. The closed
kernel stack shrinks from 456 to 424 bytes; argument allocation's emitted body
shrinks from 997 to 933 instructions while its stack grows 568 to 600 bytes for
the validated source ranges. These are attribution facts, not measured speedups.
The retained IP01 assembly predates modifications in the old scratch checkout;
that checkout now contains earlier experiments and must not be rebuilt as an IP01
baseline. Frozen executables/reports and commit 60bfaacb remain authoritative.

Initialization diagnosis adds the bounded
[runtime_initialization example](../crates/kagari/examples/runtime_initialization.rs).
It compares bare Runtime, installed Runtime and the SDK wrapper, with destruction
outside each timed construction. Each process records first use plus three warmups
and eleven 128-construction batches; it neither changes nor replaces frozen setup
acceptance. Build the identical example with the default workspace release profile
at clean 60bfaacb and current production sources, then run seven alternating pairs
(reverse route order on odd pairs), with no concurrent build/test/diagnostic work.
The fresh diagnostic checkout contains only that added example, unlike the older
scratch checkout. Reproduce with `cargo build --release --locked -p kagari --example
runtime_initialization` and run the resulting example binary, optionally `--reverse`.
Ordinary acceptance still uses the unchanged workload driver and gates.

The initialization probe's seven alternating pairs complete after both builds and
all other checks stop. Warm aggregate/paired ratios are bare 1.0162/1.0162,
installed 1.0123/1.0244 and SDK 1.0228/1.0008 (baseline medians about 1.058,
2.986 and 3.046 us). First-use ratios are 0.9766/0.9766, 0.9391/0.9508 and
0.9593/0.9821, with substantial first-use ranges. This does not reproduce a
general >5% constructor regression. The probe omits the frozen driver's preceding
source compilation/artifact preparation, so working-set/allocation-state effects
remain unmeasured; it cannot clear those setup gates. No native-registration model
change is justified by these results. Keep the frozen setup measurements open.
The example's initial structure failure was a qualified import; it is corrected,
and the final check covers 1,281 files with no violations. Workspace all-target
Clippy also passes with the example. Repeated original/source-form allocation
diagnostics for the argument/field candidate are in progress; unchanged storage
layout, prepared metadata, root/window and loaded-setup evidence is retained.

The repeated 14 original and 72 source-form/protocol allocation records pass.
Source-window views and field continuations add no retained metadata or heap
capacity; unchanged root/window and loaded-setup lifecycle evidence remains valid.
The next ordinary candidate collects seven source-form pairs followed by seven
calls pairs before further production edits, with the unchanged 0.90 targets and
1.05 common gates. Preserve all rows, including setup failures; fibonacci and the
combined storage/root controls remain required before checkpoint acceptance.

The borrowed-argument/field-continuation candidate is preserved with SHA-256
`7d77eaa036103e573aac946704d5adb53649e403d937e5a00011eba580253ed3`.
Its seven source-form pairs (UTC 20261016T143822Z) pass interface/shared_generic
at 0.8172/0.8180 and 0.8412/0.8474. Field improves from the preceding candidate's
regression to 1.0358/1.0435, still inconclusive under the fixed band; runtime
initialization fails at 1.1405/1.1659. All other source-form gates pass.
Seven calls pairs (UTC 20261016T144208Z) pass the required execution target at
0.8832/0.8823 and runtime initialization at 0.9424/1.0287. Only Lua module
initialization fails (1.0625/1.1111). All checksums pass in both runs.

Before further production edits, extend each of these two reports to exactly
21 pairs, retaining all seven original pairs and the same candidate. This resolves
the near-threshold field and small Lua setup rows; it does not excuse the larger
source-form setup failure. Fibonacci and combined window/root controls remain
pending. First extend the existing construction diagnostic with `--compiled`:
fresh engine, the frozen calls source at N=10,000, and artifact preparation precede
each bare/SDK runtime construction. Compare seven alternating process pairs, with
one construction per route and no warmups. The initial eleven-sample diagnostic
was stopped during its first process, before any paired result, after observing
that hundreds of fresh compilations would dominate this bounded investigation;
its incomplete output is not timing evidence. Formal acceptance counts and the
21-pair extension are unchanged. This supplementary probe omits the forms module's custom native installation
and does not replace either setup gate. Its source feature is declared explicitly;
no ordinary benchmark fixture or production constructor changes. Build/check work
finishes before diagnostic timing, which finishes before the ordinary extensions.

The bounded compiled-construction probe completes all seven pairs. Bare Runtime
is slower at 1.6018/1.5413 (median 11.3 -> 18.1 us), while the SDK path is
0.9531/1.0333 (21.3 -> 20.3 us). Wide process ranges remain: bare 8.5–21.8 vs
12.3–33.6 us and SDK 17.7–24.9 vs 19.4–35.8 us. Unlike the earlier warm probe,
this reproduces a cold-construction difference, but the installed SDK path does
not show the same regression. It does not attribute the forms setup failure or
justify replacing native registration. Formal setup gates remain authoritative.

The first extension attempt stops before any timing process: a rebuild after the
diagnostic example/manifest update produces a different executable hash
(`8331ea79e125dfeed3b1fc87277a51cd9d72578b74045b7bd580722de82f0287`).
The validator correctly rejects mixing it with the saved `7d77eaa0...` candidate.
Add the driver's bounded `--candidate-executable` option for extensions only;
the saved binary must still match the previous report and all original fixture/
configuration checks. No measurements are discarded or rebased. Existing thirteen
evidence tests pass, including changed-binary rejection and prior-pair retention.
Resume the declared 21-pair extensions with the saved executable after tooling
checks; do not interpret the failed launch as a completed comparison.

The saved-candidate extension support is committed at dcc50977. The source-form
extension (UTC 20261016T150918Z) retains all 21 pairs and passes all checksums.
Interface/shared_generic pass at 0.8222/0.8175 and 0.8482/0.8500. Field remains
inconclusive (1.04035/1.03288), and runtime initialization fails at 1.1718/1.1718.
All other source-form rows pass. Calls extension is running at UTC 20261016T151518Z;
finish its declared pairs before another production edit. More repetitions alone
are not the next remedy for the source-form setup regression.

Further construction review finds a concrete ownership defect on that path:
NativeInstallation::prepare validates and prepares host type/path registration,
discards the prepared result, and each Runtime repeats it against an empty host
registry. That includes a second temporary HostRegistry, cloning reflection rows,
checking installed paths and reconstructing their descriptors. Runtime::new also
performs this work for a known-empty native registry. This repeated preparation
predates IP, so it does not by itself explain the new cold-cache slowdown; it is
nevertheless avoidable work in the failed construction boundary, unlike a request
to change unrelated native-object storage.

The next bounded IP03 setup repair makes the existing immutable NativeInstallation
own the prepared host facts alongside its native registry. Runtime construction
clones those value-only type/path maps into a fresh registry with fresh host-root
identity, empty roots and dirty-path state. Keep runtime-local dynamic registration's
transactional preparation, declared access and conflict validation unchanged.
Empty Runtime construction needs no input validation beyond its trusted empty
registries. No process-global cache, heap payload sharing or OM storage change is
introduced. Native installation identity/catalog scope owns template validity;
later registration affects only that Runtime, never the supplying installation.
Use the existing runtime module tests for runtime-local failure state and external
path/foreign-root isolation, plus the SDK independent-runtime contract. Recheck
loaded setup retention/allocation and frozen ordinary gates before retaining this
repair. Field's unresolved common gate remains separate; no speedup is inferred
from this diagnosis.

Field review finds another preparation gap: the instruction already carries a
prepared field ordinal, but every access converts its stored PC back to a linked
layout site using binary search. Replace the field descriptor's PC with the
physical layout-site ordinal; preserve PCs in the shared site table for cold
canonical aggregate operations and diagnostics. A single instruction classifier
must supply the ordered aggregate sites to both physical preparation and runtime
linking, so normalized definition scopes cannot disagree on ordinal meaning.
The runtime-linked owner still owns nominal identities, deduplicated layouts and
environment-specific OnceLock publication. This replaces lookup work without a
second cache, extra per-site retained word, new opcode, changed field storage or
weakened receiver/access checks. Exercise the existing checked scalar, scoped
generic/nominal, host borrow and trap-order contracts. This is the bounded repair
for the carried field regression; IP05's instruction experiment remains separate.

The calls extension completes all 21 pairs and checksums. Execution passes at
0.88307/0.88323; runtime initialization is inconclusive at 1.03261/1.04455 and
Lua module initialization fails at 1.0625/1.0500. No other calls row fails.
All declared ordinary comparisons have stopped before the next implementation.
The host-registration template and physical layout-site changes are now integrated
for focused correctness/build checks. The template shares its existing installation
allocation, with no new process-global cache; host::installation is crate-visible
because NativeInstallation now consumes that actual prepared-fact owner directly.
AggregateOperand is the single classifier used by physical and normalized layout
preparation. Field descriptors replace one usize PC with one LayoutSite ordinal;
runtime nominal/layout validation and cold-PC lookup remain with their old owners.
No timing acceptance is claimed for this new candidate.

The prepared-owner repair passes two runtime installation/path contracts, five
VM scalar/scoped field and nested-debug contracts, the SDK independent-runtime
contract, and the source-free borrowed-pack/overlap contract (nine selected tests).
Existing overflow, foreign-root, nominal-generation and committed-effect assertions
are unchanged. Structure reports 1,282 Rust files with no violations; workspace
all-target Clippy, formatting and whitespace pass. Allocation diagnostics now repeat
original/forms, owned roots, loaded storage lifecycle and prepared-execution
retention; private window allocation/layout is unchanged and retains its previous
measurement. Ordinary timing waits until all tests/builds/diagnostics stop.

Prepared-owner allocation validation passes: 14 original records, 72 source-form
records, eight freshly measured root controls and the six unchanged private-window
controls. All 297 loaded-storage comparisons and 16 zero-net teardown cases pass;
512-Point setup retains 5,187,053 bytes and its live/empty/reused increments remain
58,092/40,516/66,700 bytes. Prepared-execution retention is unchanged at 43,011
prepared and 52,983 linked bytes, with zero teardown. Its first command omitted
`--ignored` and ran no probe; the corrected command with `--ignored --nocapture`
actually ran and passed. Instrumented timings are not throughput evidence.

Commit the combined IP02/IP03 integration as a buildable intermediate checkpoint,
not phase acceptance. All selected local checks pass; previously carried timing
gates and unrun exhaustive CI stay open. The next ordinary release candidate runs
exactly seven source-form pairs, seven calls pairs and seven fibonacci pairs,
sequentially with no other builds/tests/diagnostics during timing. Keep the original
0.90 required targets, 1.05 common gates and 0.01 inconclusive band, preserve the
binary and all results, and do not edit production code until that sequence ends.
Combined window/root timing and the remaining IP04-IP07 work are still required.

The buildable intermediate integration is committed at 91ad3b48. Its ordinary
candidate SHA-256 is `0d59d1c4c499e8aafbf79550e179f345d038e289d2ddced6f4ebb20fdd786ffb`.
All three declared seven-pair runs finish with passing checksums. Source forms
(UTC 20261016T155703Z) pass interface/shared_generic at 0.8366/0.8352 and
0.8289/0.8276; field now passes at 0.9765/0.9785. Runtime initialization still
fails at 1.1318/1.1261, and byte_state regresses to 1.1036/1.1107. Other forms
rows pass. Calls (UTC 20261016T160042Z) passes execution at 0.8709/0.8709 and
runtime initialization at 0.9538/0.9591; only Lua module initialization fails
(1.0000/1.0625). Fibonacci (UTC 20261016T160814Z) passes every row, including
execution 0.8483/0.8495 and runtime initialization 0.9257/0.9000. These results
do not close the required combined checkpoint or its remaining control timings.

Before another production edit, reuse the existing Windows main-thread RIP sampler
for byte_state. Add source-form selection, saved executable/PDB input and isolated
timestamped output; remove its old debug-profile override so it can sample this
exact ordinary candidate. This is leaf-address sampling, not an inclusive call-stack
profile. Preserve the matching PDB before later builds. Its instrumented ten-second
execution window is attribution only, never throughput acceptance. All ordinary
runs have stopped before sampling. Constructor/code-generation inspection follows
the same separation; repeated local optimization without attribution is not the
response to the new byte_state regression.

The first exact-candidate RIP report (UTC 20261016T162150Z) has 3,896 samples and
zero context errors, but nearest-public-symbol lookup misattributes addresses
up to 23,794 bytes beyond a name. Do not use its named fractions as evidence.
The sampler now checks explicit symbol sizes or matching x64 unwind starts,
retains rejected candidates in raw rows, and reports unresolved samples instead
of silently attributing them. The repeated exact-candidate report (UTC
20261016T163855Z) has 3,884 samples, zero context errors and 37.18% unresolved.
Verified leaf shares include buffer_contract 6.44%, execute_primitive 5.69%,
TypeView::normalized 5.18%, readable_location 4.20% and TypeView::compare 3.91%.
These identify current work, not the cause of a regression against IP01; no
baseline PDB was saved. Warm counters show 15,054 allocation requests with no
metadata preparation, so a claimed new warm descriptor-preparation cost is false.
Do not introduce an IP06 collection transition merely from these incomplete
IP03 samples. The current source-form checksum and zero live-object delta pass.

Workspace-release assembly inspection completes separately from profiling.
NativeInstallation::runtime now has a 1,680-byte stack frame and copies its
448-byte registry plus a 296-byte HostRegistry into constructor arguments;
the old wrapper had 20 instructions, while this one has 115. The constructor
then copies the host registry again. Its frame shrank from 5,080 to 4,792 bytes,
but preparation moved into the caller instead of yielding a single construction
owner. The bounded follow-up passes the immutable prepared host facts into
Runtime construction and constructs fresh host identity/roots there. Avoid
default-constructing then replacing the four prepared maps. This preserves the
installation-owned checked facts and runtime-local mutable state; it does not
change the host registration contract or add a cache. Assembly demonstrates
the redundant moves, not their share of the failed setup timing. Recheck the
existing installation/isolation contracts and setup gates before retaining it.

Known external follow-up: shared generic Add failed lowering at the historical
HP06 revision with MissingBinding("checked callable requirement"); its
[reproduction](performance-baseline.md#correctness-and-reproduction) remains a
compiler-owned issue to recheck if it blocks selected work. It is not a newly
reproduced HEAD failure, and shared identity cannot stand in for Add coverage.
No new local integration failure is known; exhaustive prior CI acceptance remains
unverified.

Planning validation: repository-relative links/anchors across this plan and its
three companion documents, Markdown fences, whitespace, all seven phase/checklist
entries and roadmap/index/OM links pass. This documentation-only checkpoint does
not run Rust builds, tests or Clippy.
