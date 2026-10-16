# Interpreter performance execution plan

Status: activated on 2026-10-16 by the user's execution request. IP01 is complete;
IP02 is next. IP02-IP07 implementation is unstarted.
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
