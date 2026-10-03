# Compiler and execution architecture review — 2026-10-03

This review covers committed baseline `9581f5ad2a754911d1edb92eb35e957a18d7b5df`.
It is historical source/measurement evidence, not a current defect inventory or
execution plan. Keep the compiler/runtime layering; fix concrete boundary issues
before broadening native coverage. Subsequent work addresses R3, R4 and R6.
Remaining findings require current-code confirmation before implementation.

## Outstanding findings

| ID | Baseline finding | Focused follow-up and acceptance |
| --- | --- | --- |
| R1 / P1 | Safe public KagariLanguage::kind_from_raw used unchecked u16-to-SyntaxKind transmute, reachable through external green trees | Check current syntax_node.rs conversion. Validate all raw kinds; valid round trips and invalid-input rejection through the public API. No invalid enum was executed by the review. |
| R2 / P2 | JitPolicy::Disabled did not govern execute_prepared; default-context tests asserted Native | Settle SDK tier-selection contract, then assert actual native/interpreter reports under enabled/disabled contexts. No fallback after entry. |
| R5 / P2 | Trait implementation_method/count wrappers replaced caller cancellation and finite search limits | Propagate one fallible search context; test pre-cancelled queries and tiny depth/candidate limits. No source hang was reproduced. |
| R7 / P2 | Cross-module specialization lowered all modules again for each new request round | Measure a bounded generic chain/fan-out; use concrete-instance work queues or input-keyed reuse. Preserve foreign defaults/witnesses and final verification. Scaling impact was not timed. |
| R8 / P2 | Imported signature/inherent-method equality included source revision/location | Separate semantic contracts from navigation identities; test signature-preserving versus contract-changing dependency edits. Conservative invalidation was correct; no stale result was found. |

Evidence owners: [syntax conversion](../crates/kagari-syntax/src/syntax_node.rs),
[SDK context](../crates/kagari-embed/src/context.rs),
[trait queries](../crates/kagari-hir/src/aggregates/implementations.rs),
[program lowering](../crates/kagari-compiler/src/source/program.rs) and
[analysis](../crates/kagari-hir/src/analysis/mod.rs). Links name owners; historical
line offsets are not current-code evidence. These findings remain review items,
not automatically activated tasks or new prerequisites for unrelated work.

## Findings addressed by subsequent work

- R3: immutable shared interface descriptors, selected ordinals and receiver
  preparation replace whole-method-table copies. Construction/fixed call checks
  and interpreted callback costs remain; see [interface architecture](architecture.md#interface-dispatch).
- R4: module dependency traversal/catalog storage and fixed registrations are
  shared at host-thread lifetime. Independent runtime installation/state checks
  remain. Artifact contract reachability pruning is a separate unresolved question.
- R6: immutable bytecode verification survives SDK/runtime adoption; native
  preparation checks MIR correspondence without repeating equivalent graph proofs.
  Open/changed inputs still require validation.

Results and retained costs are in [performance measurements](performance-baseline.md).
Do not read baseline allocation/startup figures below as present-day behavior.

## Profiling candidates

Instruction cloning, repeated frame/session access, root-map scans, scratch
allocation for leaf handles and interpreter-only callbacks were inspected but not
independently timed here. The later [Lua diagnosis](../benchmarks/lua-comparison/README.md#interpreter-diagnosis-bp02-2026-10-03)
contains actual interpreter sampling. Do not hold frame/heap RefCell borrows across
native calls, observers or reentry; preserve ownership/generation checks, tracing,
safe mutation boundaries and cancellation/debug behavior.

## Native expansion gates

The current narrow scalar backend is intentional and passed its focused tests.
The following gates apply when adding calls, GC-bearing values, compiled callbacks
or optimizations that depend on callee/memory facts. Wider scalar arithmetic and
scalar locals/control flow can progress independently with appropriate trap and
polling semantics. These are not bugs caused merely by lacking a larger backend.

| Gate | Current boundary | Required decision and evidence |
| --- | --- | --- |
| General calls and values | [native_call.rs](../crates/kagari-abi/src/native_call.rs), 38–46, transports runtime/result/status for zero arguments and Unit/Bool/i32 results | Define argument/result representation, helper calls, error propagation and cleanup once for both native backends |
| Native GC roots | [native.rs](../crates/kagari-abi/src/native.rs), 75–85, records logical Register/Local locations; no runtime physical map reader was found | Choose a shadow-root protocol or real physical frame/PC/location maps; prove objects survive collecting helpers, returned allocation, reentry and traps |
| Verified cross-module calls | [codegen/lib.rs](../crates/kagari-codegen/src/lib.rs), 12–45, accepts a module seal; full binding proofs live in `VerifiedMirProgram` | Pass program-scoped verified target bindings or a typed runtime trampoline; backends must not redo name/type resolution |
| Effect interpretation | Script calls carry `calls`/`may_trap`, not transitive allocation/write facts | Treat calls as conservative memory/GC barriers or compute conservative graph summaries before emitting LLVM `readonly`/`readnone`-style assumptions |
| Tier-neutral callbacks | Synchronous `ScriptInvoker` currently selects the interpreter | Preserve pinned generation, captures, argument/result checks, shared call depth/cancellation, observers and trap origins when selecting compiled callbacks |
| Code lifetime and hot reload | Installed handles retain exact versions and their dependency closure | Extend dependency assumptions to direct calls, inlining and module/SCC compilation units; reclaim code only when its owners and retained dependencies permit it |

The current poll helper records a logical offset and invokes ordinary registered-root
GC; it does not publish native stack roots. Cranelift correctly rejects heap-bearing
slots and emits empty root maps for its accepted subset. Do not mistake the logical
metadata schema for completed GC integration.

Effect caution is similarly forward-looking. In
[instruction.rs](../crates/kagari-mir/src/instruction.rs), lines 397–406, script calls
use `EffectSet::call`; [effects.rs](../crates/kagari-abi/src/effects.rs), lines
124–129, sets only `calls` and `may_trap`. A callee can still allocate and mutate.
Current constant propagation treats calls as barriers and dead-code elimination
requires an empty effect set, so no present miscompile was demonstrated. Individual
false effect bits must not become absence proofs in a future optimizer.

Recommended order: establish calls/roots and tier-neutral runtime services with
Cranelift, grow the semantic conformance matrix, then implement LLVM against the
same checked contract. Keep LLVM lowering and optimization local to that backend.
MIR need not become LLVM IR, and an additional SSA IR is justified only by concrete
optimization requirements. Simple baseline native compilation and optimizing
compilation may use different strategies without duplicating language semantics.

For each newly accepted native feature, use the same semantic cases through source,
serialized artifact, interpreter and **asserted actual native execution**. Tests
that successfully fall back do not establish native coverage. The important rows
are checked overflow/trap order; left-to-right once-only effects; nested calls and
call-depth failure; objects live across collection/reentry; cleanup after traps and
cancellation; old callbacks/interfaces surviving reload; and observer-triggered
pre-entry fallback. Add differential generated-program testing only within the
supported subset, with reproducible seeds and finite bounds.

## Semantic constraints

Keep recoverable HIR distinct from checked executable types. Readonly List does
not prove frozen storage, noalias or an exclusive native borrow. `[T]` already
denotes List and must not silently acquire Rust slice layout/lifetime semantics.
Preserve once-only evaluation, trap order, completed effects and pinned generations.
The contract/common cleanup remains [queued](architecture.md#contract-and-common-responsibility-cleanup);
renaming a mixed crate alone does not fix ownership.

## Measurements

### Environment and method

- Baseline above; Apple M1 Max / MacBookPro18,2, 32 GiB RAM, 10 logical CPUs,
  macOS 26.6.2 (25G83), `aarch64-apple-darwin`.
- Rust 1.98.1 (`48a229cea`, LLVM 22.1.8), Cargo 1.98.1; Cranelift 0.132.0 from
  the baseline lockfile. Workspace dev/test O1 profiles with debug information,
  default target directory and Cargo parallelism; default SDK source/native features.
- The isolated worktree initially needed compilation. Timed runs then used warm
  prebuilt executables, serially, with compilation excluded. The desktop was not
  reserved from user/OS activity. One brief standalone frontend harness compilation
  may overlap the first baseline process; it was never executed. New probe runs
  had no competing review build/test jobs.
- Three independent processes per original benchmark/probe. Table centers labeled
  median-of-medians summarize the three within-process medians, not confidence
  intervals. Foundation call means and single edit timings are stated separately.
  Ranges below expose variability rather than silently discarding slow runs.
- `architecture_baseline` uses an atomic counting allocator; the interface probe
  uses a thread-local counting allocator. Both perturb timings. The startup split
  and foundation baseline use ordinary allocation. Never subtract differently
  instrumented/scoped timings as an exact cost decomposition.
- No successful sampling profile was captured during this review. Code inspection
  identifies mechanisms, but does not provide per-function CPU percentages. No
  release-mode, cross-machine or historical-regression claim is made.

### Existing architecture baseline: `fn main() -> i32 { 40 + 2 }`

| Interval | Samples per process | Median of process medians | Range of process medians |
| --- | ---: | ---: | ---: |
| Fresh engine + source-to-artifact | 21 | 432.671 ms | 427.760–439.009 ms |
| Decode + native-input preparation/verification | 101 | 208.602 ms | 207.840–210.570 ms |
| Fresh runtime + shared-program link + disposal | 101 | 247.197 ms | 241.197–254.334 ms |
| Native compilation | 21 | 97.750 µs | 97.167–100.292 µs |
| Cached native preparation and installation | 101 | 1.750 µs | 1.709–1.833 µs |
| SDK interpreter call | 10,001 | 1.500 µs | 1.500–1.583 µs |
| SDK native call, actual `Native` asserted | 10,001 | 1.834 µs | 1.833–1.917 µs |

The tiny native entry does less useful work than its entry machinery; these numbers
do not predict loop or application speedups. Compilation/preparation and execution
are distinct intervals. Fresh-engine compilation is cold with respect to that
engine's analysis state, not a cold OS/registry cache.

Artifact bytes: **505,550**, with **267,717** for the bytecode-only artifact and
**237,805** in portable MIR. The remaining difference includes envelope metadata.
The startup probe uses a different source filename and therefore reports **505,536**
total bytes and **237,799** MIR bytes; source identities affect encoded size.

| Live runtimes | Shared prepared program: retained requested Rust heap bytes | Independently prepared programs: retained bytes |
| ---: | ---: | ---: |
| 1 | 11,782,678 | 11,782,678 |
| 8 | 65,612,636 | 94,261,424 |
| 32 | 250,172,492 | 377,045,696 |

These include retained prepared programs, runtimes and links. They exclude RSS,
allocator overhead and executable page mappings. The result demonstrates useful
program sharing and substantial independent per-runtime cost at the same time.

### Construction versus linking: separate System-allocator probe

One warmup and eleven samples per process; fresh runtime each sample, shared
prepared program. Engine creation, compilation, preparation, validation execution
and all drops are outside both timers.

| Interval | Run 1 median | Run 2 median | Run 3 median |
| --- | ---: | ---: | ---: |
| `engine.runtime(default_context)` | 239.872 ms | 239.077 ms | 234.404 ms |
| `runtime.load_program(shared_program)` | 0.658 ms | 0.656 ms | 0.643 ms |

The root module has one function/four instructions and no native imports of its own.
The foundation module carries the 53 imports described in R4. All are linked;
linking remains small beside construction in this workload.

### Interface width: identical selected method, 1,000 interpreted calls

Five warmups and 21 samples per width/process. Each sample includes SDK execution,
one interface construction, 1,000 calls and return-report creation; preparation,
linking and report destruction are excluded. Every result is checked as `1,000`,
and bytecode is checked to retain dynamic interface dispatch.

| Methods in interface | Allocation calls per sample, excluding realloc | Median cumulative requested bytes, same median in all runs | Median of process medians | Range of process medians |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 75,099 | 3,931,475 | 6.181 ms | 6.021–11.254 ms |
| 8 | 243,358 | 13,718,643 | 12.060 ms | 11.952–15.805 ms |
| 32 | 820,246 | 47,460,629 | 31.936 ms | 31.906–58.759 ms |

Allocation counts are stable across processes; this column counts `alloc` and
`alloc_zeroed`. Additional reallocations vary from 1–2, 9–10 and 35–36 per sample
for widths 1, 8 and 32 respectively; their requested sizes are included in bytes.
The table reports byte medians, not identical byte counts for every sample.
Timings show appreciable variability on an unreserved desktop; a width
32 process had a 102.6 ms maximum sample. Allocation counts and the inspected clone
path are the stronger evidence. Bytes are cumulative requests, not retained/peak
memory. The test includes one width-dependent construction, so it is not an exact
per-instruction allocation attribution or a before/after optimization comparison.


## Reproduction and limits

[Companion probes](review-support/architecture-2026-10-03/README.md) retain the
standalone workloads. From the recorded baseline, build the architecture_baseline
and foundation_baseline examples, then execute prebuilt binaries serially in
three independent processes. Raw logs under target/architecture-review are
disposable; baseline, environment and scope remain recorded above.

The original review ran 515 focused tests and dependency/structure checks, not
full workspace tests/Clippy or the complete feature matrix. Successful checks
are not a soundness proof. New implementation acceptance belongs to the roadmap;
this condensed report introduces no additional active phase sequence.
