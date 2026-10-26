# Compilation cancellation polling plan

Status: proposed, not implemented or authorized for execution. The
[roadmap](implementation-roadmap.md#compilation-cancellation-polling) owns activation.
This plan owns CP01-CP04, decisions, acceptance and the progress ledger. Preparing
this document does not authorize commits or reopen runtime cancellation policy.

## Objective and scope

Reduce redundant cancellation polling and clarify its owner in compilation work,
without weakening resource limits, executable validation or failure isolation.
Prefer one responsible traversal context over repeated token reads in every helper.
Batch only bounded, cheap work; preserve explicit boundary checks.

Baseline for source inspection: `64e7b242`, clean working tree. No cancellation
cost or response-latency measurements have been collected for this proposal.
High call-site density is evidence of an ownership question, not proof of a
performance bottleneck. The [R7 measurements](performance-baseline.md#program-owned-instance-scheduling-2026-10-26)
do not attribute time to cancellation and cannot justify a claimed speedup here.

### Finite first delivery

Audit compilation/analysis/validation boundaries, then migrate at most these three
connected families where baseline evidence supports the change:

1. Syntax scanning: lexer whitespace/comments/strings and directly invoked literal
   decoding. Preserve Unicode boundaries, lossless tokens and malformed-input errors.
2. Compiler source lowering: planner/body/layout traversal and demand discovery.
   Preserve R7's retained state, per-product deduplication and candidate feedback.
3. Contract update evidence/coverage traversal: existing `updates::Budget` and its
   direct callers. Preserve exact charges, proof coverage and source-free ownership.

HIR query/cache owners, MIR/bytecode verification and codecs are audited as adjacent
boundaries and tested where directly affected. They are not additional wholesale
migration tracks. In particular, merely sharing `CancellationToken` does not bring
all callers into scope. If the three selected families are not profitable, retain
existing polling where appropriate and report the evidence; do not substitute an
unrelated optimization or keep expanding the list.

Exclude VM instruction boundaries, `ResourceState`, GC safepoints, native execution,
host callbacks/reentry, await/task wakeups, publication policy, runtime observers,
thread preemption, deadline APIs, token memory-order changes, execution charging,
process-global polling counters and caller-configurable polling intervals. No new
script syntax, public cancellation policy or artifact/ABI version is required.

## Current implementation and constraints

| Owner | Observed behavior | Implication |
| --- | --- | --- |
| [Common token](../crates/kagari-common/src/cancellation.rs) | `check()` is inline and loads `AtomicBool` with Acquire. Cancellation/wake registration, not normal polling, uses the subscriber mutex. | Keep token semantics and synchronization unchanged. Measure before assuming token reads dominate. |
| [Lexer](../crates/kagari-syntax/src/lexer.rs) and [string scanner](../crates/kagari-syntax/src/lexer/string.rs) | Poll inside character/token scans; string scanning can call cancellation-aware literal decoding. | Long single tokens require internal polling, not merely one check per token. |
| [Planner](../crates/kagari-compiler/src/source/lower/instances.rs) | `check()` observes cancellation and a latched compile-limit diagnostic. Admission helpers also call it. | Do not replace the whole method with a throttled check; sticky failure and resource accounting remain distinct. |
| [Update limits](../crates/kagari-contract/src/updates/limits.rs) | `visit()`, `records()` and `bytes()` each poll and charge different resources. | One logical visit may poll several times; deduplicate cancellation, never the required charges. |
| [HIR completion](../crates/kagari-hir/src/typeck/completion.rs) | Tests require cancellation not to become a completion fact and cancellation from iterator advancement to stop before another operand. | Preserve this exact traversal contract; it is not a batchable loop by default. |
| [Runtime poll](../crates/kagari-runtime/src/resource.rs), [VM driver](../crates/kagari-vm/src/executor/driver.rs) and [GC entry](../crates/kagari-runtime/src/lib.rs) | Execution polling checks admission/session termination, and GC safepoints poll execution too. | These are not interchangeable token reads. Leave them unchanged in this delivery. |

The [execution specification](spec/execution.md#common-executable-contract) includes
per-instruction cancellation/GC observations. Altering those points can change which
side effects complete before termination and requires a separate explicit design.
Compilation has no equivalent universal promise to stop at every source node, but
specific diagnostic, cache and iterator contracts must still be inventoried.

## Proposed model

### Three kinds of checks

| Kind | Placement | Frequency |
| --- | --- | --- |
| Boundary observation | Cancellable operation entry, successful cached return, relevant phase handoff, seal/cache/result handoff | Immediate token check, including empty inputs |
| Traversal polling | Long sequences of bounded cheap characters/nodes/edges | Operation-owned batch cadence |
| Required state/error check | Latched failure, allocation/record/node/depth limit, source identity, proof and publication validation | Existing required frequency; not throttled with cancellation |

A boundary check is an observation point, not synchronization with future `cancel()`
calls. If cancellation races after the final check, it does not retroactively revoke
a completed result. Preserve existing transactional cache/publication synchronization;
do not promise globally atomic cancellation-versus-success just by adding a check.
Once cancellation is observed, propagate the existing cancellation error rather
than returning a partial successful product or caching it as a semantic fact.

### Internal polling state

A small common polling utility is appropriate only for the selected concrete users.
The proposed responsibility sketch is not a public embedding API:

```text
CancellationPoll<'operation>
    token: borrowed CancellationToken
    remaining: local work units until next actual token read

operation entry:     token.check() immediately
before bounded unit: poll.step()
required boundary:   poll.check_now()
normal completion:   token.check() before result handoff
```

The token stays shared and thread-safe. Cadence state belongs to one active operation
or traversal, not to the token, thread-local global state, or an `Arc<AtomicUsize>`.
It requires no lock, allocation, clock read per item or runtime mutation counter.
Use ordinary explicit mutable borrowing through the owning context. Do not widen
visibility or introduce forwarding APIs solely to carry the counter.

Start with candidate intervals 64, 256 and 1024 cheap work units; 256 is a proposed
starting point, not an accepted universal value. CP01 freezes the definition of a
work unit and the experiment before comparing results. A simple local countdown
still costs an update/branch per unit and is not automatically cheaper than the
current inline atomic load. Compare the generated hot path and measured workload.
Where a natural bounded chunk exists, one check per chunk may be simpler.

The maximum gap is expressed in actual bounded units: after a successful read,
execute no more than the selected interval before the next read. Handle exact
thresholds and incomplete final batches. Do not add weights after an arbitrarily
large operation and call that responsive cancellation. Keep checks before large
allocations and preserve byte/count bounds before allocation.

### Ownership through nested calls

Simplified current path:

```text
visit a node -> caller checks token
             -> helper checks token
             -> budget.visit checks token
             -> budget.records checks token
```

Proposed path:

```text
one traversal owns the cadence
    -> admit bounded node work through that cadence
    -> helper uses the same traversal context
    -> budget charges every required resource independently
    -> return/phase boundary observes cancellation immediately
```

Do not create a fresh counter in every recursive/helper call: repeated short calls
could otherwise postpone polling indefinitely. Independent public operations still
check their own entry/exit. Nested operations that can run for a long time must
poll internally; an outer check cannot bound their duration. Retained compiler state
may store a cadence or create a new one at a processing boundary only if that
boundary checks immediately, so repeated short batches cannot starve observation.

A counter may be omitted from a short helper only when its caller establishes a
concrete maximum work gap. Record that invariant beside the owning traversal, not
as comments on every deleted check. Preserve cancellation at cache-hit paths and
standalone low-level entries; do not add unchecked public variants.

### Expensive or irregular work

One function, type instantiation or module is not a bounded cheap work unit. Keep
internal polling for trait search, nested substitution, graph closure and large
single-token scans. Before reducing checks around sorting, bulk copying, hashing,
decoding or destruction, account for the entire non-interruptible segment and its
existing bounds. Do not redesign these algorithms in this delivery merely to make
a desired interval pass. Leave the original cadence where no safe bound is known.

Iterator advancement or callbacks that can cancel are observable boundaries, not
ordinary array accesses. Preserve the HIR completion sequence test unchanged.
Runtime/native callback behavior remains entirely out of scope.

### Errors, budgets and successful results

All existing record/byte/node/depth/output charges remain exact and checked before
the same protected action. Polling cadence is not a resource allowance. Keep
latched errors visible even on steps that do not read the token. Inventory existing
cancellation-versus-limit/diagnostic precedence before splitting combined helpers;
where cancellation currently wins at a failure boundary, use an immediate check
on that error path rather than silently reversing precedence. Do not hide or delay
limit enforcement until the next cancellation poll.

Before returning a completed successful product or installing a successful cache
entry, preserve the owner's final cancellation/revision checks and existing commit
boundary. Cancelled construction must leave prior snapshots/caches usable. This
proposal does not permit skipping validation on a reused verified graph.

## Mature reference and differing assumptions

[.NET polling guidance](https://learn.microsoft.com/en-us/dotnet/standard/threading/how-to-listen-for-cancellation-requests-by-polling)
(article dated 2021-09-15, reviewed for this proposal) demonstrates checking an outer
loop when the inner loop is known to be sufficiently short. It also notes that the
underlying Boolean observation is cheap. Adopt the bounded-work placement principle,
not its example's workload or a presumed performance gain.

Kagari additionally has latched compile errors, explicit work limits, checked graph
seals and cache publication rules. Therefore retain immediate boundary observations
and separate cancellation cadence from accounting. Runtime safepoint placement is
not copied from .NET and is not changed here.

## Execution phases

### CP01 — Inventory and baseline

- [ ] Confirm the current revision/status and inspect relevant diffs.
- [ ] For the three families, record entry/exit, cache, error and traversal owners;
  identify nested duplicate reads and longest uninterruptible segments.
- [ ] Collect actual token-read counts separately from poller calls/work charges,
  normal execution timing and mid-operation cancellation latency. Do not infer
  reads solely from source call-site counts.
- [ ] Select a small fixed workload matrix: long single tokens and many short tokens;
  bounded generic chain/fan-out; a dense retained-demand/proof input. Include tiny,
  empty, already-cancelled and cached controls where applicable.
- [ ] Freeze candidate intervals, bounded-unit definitions, test insertion points,
  environment and acceptance gates below before implementation comparisons.

Gate: evidence supports a finite set of redundant reads and safe replacement
owners. If costs are insignificant or cadence overhead is worse, narrow or reject
the optimization rather than increasing the task scope.

### CP02 — Internal primitive and syntax pilot

- [ ] Implement the minimal operation-owned polling state alongside the unchanged
  token only if justified by CP01. No new external configuration or dependency.
- [ ] Prove interval boundaries and entry/final checks with deterministic unit tests.
- [ ] Apply it to the lexer/literal pilot without changing token/source semantics;
  share cadence where appropriate and retain nested expensive-operation checks.
- [ ] Compare polling overhead and mid-scan responsiveness before proceeding.

Gate: full/truncated batches, huge single tokens, UTF-8 and malformed inputs retain
their existing results; successful partial parses cannot escape cancellation.

### CP03 — Lowering and update-budget integration

- [ ] Separate planner latched-error checks and budget accounting from token cadence.
- [ ] Apply the policy only to the inventoried cheap-work paths; retain immediate
  entries, error-precedence checks and candidate/cache handoffs.
- [ ] Preserve R7 once-only body/extraction behavior, retained coverage and all limits.
- [ ] Audit HIR and executable verifier callers affected by signature changes;
  preserve exact cancellation tests rather than extending batching repository-wide.

Gate: selected semantic, budget, cancellation and failed-publication cases pass.
Do not accept a buildable pilot while leaving missing cancellation propagation in
its direct callers.

### CP04 — Acceptance and handoff

- [ ] Complete matched timing and response measurements; reject unprofitable cadence
  choices. Document retained immediate checks and remaining limits.
- [ ] Run focused contracts and required local implementation checks. Record CI scope
  separately; do not run a disguised full-workspace suite.
- [ ] Move lasting ownership/behavior to architecture and relevant API contracts,
  measurements to the performance document and actionable remaining work to review.
  Remove this plan on completion; Git history owns the phase ledger.

## Acceptance and measurement policy

Correctness gates are mandatory: unchanged ordinary outputs, exact limit enforcement,
existing error precedence, no partial success/cache poisoning, immediate pre-cancelled
entry/empty/cache-hit behavior, and bounded work between traversal observations.
A request after the final observation may race successful completion as before.

Use deterministic hooks or synchronization to request cancellation after work starts;
never rely only on `sleep` or the fact that a worker was spawned. Assert the work-unit
bound independently of wall-clock scheduling. Include cancellation during nested
helpers, just before cadence rollover and in a short final batch. The current syntax
concurrent test does not itself prove the worker reached its scanning loop before
cancellation; supplement it at its owner if the selected contract needs that proof.

Proposed local responsiveness targets to freeze in CP01: on each selected long-running
workload, at least 30 in-flight requests, p95 request-to-observation <= 10 ms and
observed maximum <= 50 ms. These are engineering acceptance measurements on the
recorded machine, not hard real-time API guarantees or timing assertions in ordinary
CI. Record request-to-return separately because unwinding/destruction may cost more.
Pre-cancelled calls must reject at entry rather than waiting for a batch. If baseline
already misses a target, diagnose and record it; do not quietly relax the gate or
claim batching fixed an unrelated uninterruptible operation.

Performance: compare the existing implementation and fixed candidate intervals with
identical output/charge counts. Interleave baseline/candidate runs, use multiple
fresh processes, distinguish warmup/cache states and separate source/build/execution
time. Choose a candidate only with a reproducible gain beyond observed variation on
at least one selected affected workload and no material regression on tiny/control
or other selected workloads. CP01 records the concrete noise-derived regression
threshold before tuning. If only read counts improve, report only that fact; do not
claim speedup. Do not introduce global tracing overhead to measure a local poll.

Store temporary samples/probes under ignored `target/`; retain workloads, commands,
revisions, toolchain/machine, profile/features, default parallelism, sample counts,
latency distribution and limitations in this plan/performance owner. Use workspace
profiles, default target and default build parallelism; use `uv run python` for
Python. CPU time/read counts and response latency are different evidence.

### Focused existing test owners

- [Syntax cancellation](../crates/kagari-syntax/src/tests/cancellation.rs):
  `cancelled_parses_never_return_a_partial_success` and UTF-8 losslessness; select
  affected lexer/string cases without running the whole workspace.
- [HIR completion](../crates/kagari-hir/src/typeck/completion.rs):
  `cancellation_is_not_a_completion_fact` and
  `cancellation_during_a_sequence_stops_before_the_next_operand` remain unchanged.
- [HIR analysis](../crates/kagari-hir/src/analysis/tests.rs):
  `snapshots_reuse_unchanged_files_and_cancellation_does_not_publish`; add a selected
  cache-hit case only if existing coverage does not establish its distinct rule.
- [Compiler lowering](../crates/kagari-compiler/src/tests/lower.rs):
  `lowering_honors_instruction_limits_and_cancellation` and shared instance budgets.
- [Compiler passes](../crates/kagari-compiler/src/tests/passes.rs):
  `pass_limits_and_cancellation_do_not_publish_partial_modules`.
- [R7 scheduling](../crates/kagari-compiler/src/tests/source_programs/worklist.rs):
  select retained-demand and once-only extraction cases; reuse the ignored bounded
  workload for attribution, not as evidence of a new cancellation speedup by itself.

CP01 selects exact contract update and source-free verifier tests from their
current owners after inspecting affected call paths. Keep that selection small;
add tests only for uncovered boundaries, not each removed check. Existing runtime
observer/cancellation tests must not be rewritten to accommodate this proposal.

Implementation acceptance commands:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Run affected package/target/name-filtered tests; use `--no-fail-fast` for multiple
selected targets. Full-workspace tests and complete feature/backend/platform
matrices are CI-only. Previously passing checks remain evidence for unaffected code.
Docs-only planning requires content/link/diff checks, not Rust builds.

## Progress ledger

| Phase | State | Evidence / next action |
| --- | --- | --- |
| CP01 | Not started | Source inspection establishes mechanisms only; measure cost and responsiveness after authorization. |
| CP02 | Not started | Depends on CP01 selecting safe, worthwhile pilot work. |
| CP03 | Not started | Depends on CP02; no repository-wide token rewrite. |
| CP04 | Not started | Focused acceptance, measured handoff and pending CI kept distinct. |

No implementation tests or benchmarks have run for this proposal. No known build
failure was established. Thresholds and candidate intervals are proposed, not measured
results. Update this ledger for decisions/failures; do not create a parallel queue.
