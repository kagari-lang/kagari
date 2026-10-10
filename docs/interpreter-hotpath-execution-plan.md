# Interpreter hot-path execution plan (HP00-HP06)

Status: planned; implementation is not started. The user requested this plan after
the post-VE09 diagnosis. Writing the plan does not activate implementation or resume
an earlier goal. The [roadmap](implementation-roadmap.md#interpreter-performance-follow-up)
owns activation; this document owns the finite phase order and progress ledger.

## Objective and evidence

Make repeated interpreter operations reuse checked execution facts, with short
dynamic checks at the boundaries where those facts can change. Preserve correctness
guarantees while replacing redundant validation and preparation paths. The completed
[VE00-VE09 record](interpreter-value-execution-plan.md) remains historical acceptance;
this work does not reopen Value representation or replay those phases.

The [VE09 measurements and hotspot diagnosis](performance-baseline.md) are the
starting evidence, not promised speedups:

| Workload | VE09 time / Lua | Requests per 5,000-iteration diagnostic | Evidence |
| --- | ---: | ---: | --- |
| Shared generic identity | 201.94 | 315,599 | Repeated method application, environments, metadata roots and allocation |
| String constants / string calls | 105.62 / 103.51 | 7 / 7 | No new string objects; frame/state checks and general dispatch dominate sampled paths |
| Map | 88.00 | 50,280 in the separate Map::get probe | VE09 removed repeated layout comparison; ordinary Option storage and pattern work remain |
| Interface | 76.78 | 70,040 | Argument vectors, rooted method selection, metadata validation and frame entry |
| Byte state | 62.72 | 36 | General execution boundaries are prominent despite low allocation |

Ordinary helper/concrete-generic loops request only seven allocations each.
Capture-cell has eleven requests; its CPU attribution is not established. Sampling
is diagnostic wall-clock evidence, not exact CPU percentages. The original arrays
workload must be profiled separately before attributing its time to byte-state costs.

## Scope and architecture

HP00-HP06 owns runtime-local prepared calls, transient interpreter access, managed
slot/constant operations, bounded primitive collection/string operations and enum
pattern reads. It may change internal Rust APIs and prepared execution records
directly. It does not add a second interpreter or a second language implementation.

Three layers own the proof:

1. Verification/linking establishes instruction types, physical transfers, declared
   access, layouts and target contracts from checked products, without source analysis.
2. Execution admission pins the exact runtime, program, frame window and metadata
   dependencies. Internal access reuses these facts only while their lifetime holds.
3. Individual operations check changing facts: indices, dynamic receiver selection,
   handle generation, borrow validity, checked arithmetic and termination state.

Admission is an internal lifetime boundary, not a new permission system. Prefer
ordinary scoped Rust borrows and checked identities; no new raw-pointer fast path
is planned. Do not replace a graph traversal with an equally expensive cache-key
construction on every call. Reuse the existing metadata/GC owners rather than
introducing a global cache or cross-crate forwarding layer.

| Fact or transition | Required release behavior | Permitted reuse |
| --- | --- | --- |
| Instruction/signature/layout | Validate executable products before publication | Sealed physical records within the pinned program |
| Foreign or stale host value | Check runtime identity, generation and declared access on entry | Internal transfers of values already admitted into live rooted slots |
| Dynamic interface receiver | Select its actual implementation and exact applied view | Prepared selection only when the complete identity/environment key matches |
| Generic application | Preserve scoped types, operation witnesses and associated outputs | Immutable prepared application under its owning runtime/version |
| Array index or alias mutation | Check current bounds/access and preserve mutation commit | Borrowed storage only while mutation/reentry cannot invalidate it |
| Allocation, native callback, observation or stack growth | Publish PC/live roots and release conflicting borrows | Re-admit afterward; do not reuse a stale cursor |
| Return, suspension and resumption | Root outgoing values before retiring frames; park exact dependencies | Resume only after checked admission |
| Trap, cancellation or call-depth failure | Preserve failure order, completed effects and full cleanup | No success-path-only cleanup or suppressed failure |

For every check moved or removed, record its invariant, proof owner, invalidating
events and remaining release enforcement in the phase ledger. Existing semantic
boundaries are defined by [execution](spec/execution.md),
[failure semantics](spec/failure-semantics.md), [runtime](spec/runtime.md),
[traits](spec/traits.md) and [collection access](spec/collection-access.md).
Preserve logical instruction/debugger positions, execution slicing and current
cancellation observation boundaries; polling cadence changes are outside this plan.

Debug-only fields/assertions may cross-check admitted facts, generations and window
identities under `cfg(debug_assertions)`. They must not supply missing release
validation, introduce roots, change collection eligibility or perform required
publication/cleanup. Follow the existing [diagnostic policy](interpreter-value-execution-plan.md#debug-only-invariant-diagnostics).

Excluded: further Value shrinking/NaN boxing, moving/generational GC, general enum
unboxing, escape analysis, global string interning, JIT expansion, generic language
redesign, async API redesign and a Java benchmark project. The known shared generic
Add lowering failure remains separately tracked; do not change the frozen shared
identity workload to conceal it. Closure-specific redesign requires new attribution
and is deferred; shared execution improvements may benefit closures incidentally.

## Phase order and acceptance

Once implementation is activated, execute these phases in order without requesting
permission again for work inside this scope. Each implementation checkpoint must
build and pass its selected contracts; no planned broken intermediate commits.
If a mechanism fails its gate, record the evidence and fix or discard that candidate.
Keep the failure visible if proceeding to an independent later phase; never mark an
unmet phase accepted or silently add a new architecture track.

### HP00 — Preserve a baseline and assign check ownership

- Preserve a release executable at `target/hp00/baseline-executable`, its revision,
  hash and measurement metadata. Rebuild from the recorded revision if VE09 artifacts
  are unavailable; do not compare a current rebuild with an undocumented old binary.
- Reuse the existing Lua driver and source-form bodies. Make the minimal allocation
  and boundary-count diagnostics reproducible through existing benchmark tooling;
  do not depend solely on untracked probe sources surviving under target.
- Count method-application preparation, metadata graph validation and slow-boundary
  entry separately from throughput. Include warmed repeated calls, changing receivers
  and type arguments, plus original arrays/maps attribution.
- Inventory the checks on interface application, root refresh, constant/managed
  transfers and native/array access. Record the proof ownership described above.

Acceptance: checked workloads, recoverable baseline, reproducible diagnostics and
a concrete invariant map. No performance improvement is claimed for this phase.
Do not rerun the historical full correctness suite merely to establish the baseline.

### HP01 — Reuse shared generic method applications

Owners: `runtime/objects/application.rs`, `objects/calls.rs`,
`execution_metadata/{applications,environments,interfaces}.rs` and prepared calls.

- Replace the method-local-generic exclusion from reuse with an exact applied-call
  identity. Include implementation/member, pinned program, scoped type arguments,
  receiver environment and operation bindings; equal printed types are insufficient.
- Start with bounded reuse for a verified call site and its observed application.
  A single-entry cache is sufficient initially; changed keys take the existing checked
  preparation path. Do not retain a receiver value in reusable code metadata.
- Publish prepared environments/signatures/result adapters only after complete
  validation. Keep cache edges in the existing traced runtime metadata graph, with
  checked generations, bounded retention and reclamation of unreachable cycles.
- Keep required cross-generation/scoped comparisons on misses. Do not use unchecked
  IDs, global process maps, negative-result caching or partially initialized entries.

Acceptance: the unchanged monomorphic shared-generic loop stops preparing a new
application/environment per iteration after its first preparation; remaining Rust
allocation sources are reported separately. Repeated calls improve in paired timing.
Different receivers/types/witnesses and old/new generations never share an invalid
application; forced collection, reload and retirement reclaim cache-owned cycles.

### HP02 — Prepare interface transfers and reuse metadata admission

Owners: `vm/executor/dispatch.rs`, `runtime/frame/{calls,arguments,transfer}.rs`,
`objects/{method,calls}.rs`, `gc/roots.rs` and `execution_metadata.rs`.

- Give script-internal interface calls physical argument/result transfer records,
  following the existing concrete-call window model. Eliminate both transient argument
  vectors and insert the receiver through the transfer description.
- Retain a selected call's dependencies through its active frame/admitted scope.
  Avoid rebuilding metadata-root vectors and walking the same immutable graph for
  every invocation. Root replacement must protect new edges before retiring old ones.
- Separate external host-handle admission from already-admitted internal dispatch
  inside the existing owner modules. Reuse implementation logic; do not weaken host
  entry or expose a public unchecked interface-call API.
- Scope reuse so collector reclamation, receiver changes, reentry and version changes
  cannot invalidate an admitted descriptor. Slow misses keep full validation.

Acceptance: warmed repeated interface/shared calls no longer allocate argument/root
metadata containers or walk unchanged dependency graphs per invocation. Verify this
with N versus 2N diagnostics, including result checks, rather than timing assertions
in unit tests. Paired interface/shared throughput improves; parameter order, result
adaptation, call depth, reentry and pinned descendants remain correct.

### HP03 — Extend scoped execution to managed transfers and constants

Owners: `runtime/frame/cursor*`, `frame/values.rs`, `module/{constants,execution}.rs`
and `vm/executor/{mod,loop_body,dispatch}.rs`.

- Extend prepared execution with managed register/local copies and already-materialized
  constant loads. Borrow validated windows/code once for a contiguous eligible region.
- Keep lazy string materialization as an allocating slow boundary. Cache hits use
  the pinned runtime-local constant storage without resolving the module again for
  every load; never put runtime heap identities in portable products.
- Reduce repeated current-frame/state acquisition and repeated invariant checks inside
  the admitted scope. Represent synchronous/runnable versus waiting/native states so
  ordinary progress does not repeatedly execute irrelevant await/native preparation.
- Keep dynamic termination/debugger/slice checks at their existing logical boundaries.
  Publish roots/PC and discard transient views before callbacks, GC, growth or parking.

Acceptance: managed-copy and warm constant instructions stop forcing a general
dispatch round trip, as shown by boundary counts. The frozen string workloads retain
zero new string objects and no per-iteration allocation. Paired string timings improve;
source-free constants, escaped strings, cancellation, observer stepping, async resume
and trap cleanup still pass. Do not cache a mutable object's value across aliases.

### HP04 — Short paths for proven primitive operations

Owners: existing prepared execution/cursor, stdlib string and collection bindings,
runtime collection storage. Contract/linking changes are allowed only for these cases.

- Use HP00/HP03 profiles to implement bounded primitive Vec index reads/writes and
  immutable String byte length in the admitted execution model.
- Recognize exact installed implementations using verified identity and physical
  contracts, never a method name or an untrusted host claim of purity. Keep arbitrary
  native functions, custom indexing and user callbacks on their normal boundaries.
- Share operation semantics with the existing runtime/stdlib owner. Preserve current
  bounds, collection access, handle validity and checked conversion behavior.
- Avoid per-operation host-style retention for already-rooted script values. Do not
  hold a storage borrow across allocation, cancellation handling, observation or reentry.

Acceptance: targeted primitive operations avoid general native/dispatch transitions
while preserving their logical observations and failure order. Frozen strings,
arrays and byte-state improve in paired timing without per-iteration allocation.
Custom/foreign implementations and alias mutations continue through valid checked
paths. Report any added descriptor size and cold preparation cost.

### HP05 — Remove Map result-pattern copying and repeated preparation

Owners: `vm/executor/aggregate_ops.rs`, runtime enum layouts/storage and existing
Map native result publication. Reuse VE09 layout compatibility facts.

- Prepare expected enum pattern descriptors within their exact lexical/program scope.
  Reuse them for repeated matching and payload extraction when the scope is unchanged.
- Replace owned enum snapshots used only for tag/payload reads with bounded borrowing
  or direct checked field copies. Keep the source rooted and release views before GC.
- Retain ordinary Option allocation/publication and nominal enum behavior. General
  unboxing, special Map::get return semantics and escaping-payload tricks are excluded.

Acceptance: attribution confirms that repeated pattern preparation/payload snapshot
allocation has disappeared on the frozen path; remaining Option allocations and GC
counts are reported honestly. Map timing improves. Nested traced payloads, cross-scope
layouts, cross-generation matching, invalid variants and source-free native enums pass.

### HP06 — Integrated correctness and unchanged Lua comparison

- Resolve all carried failures and remove temporary production instrumentation.
  Review changed module ownership, effective LOC and any obsolete alternate paths.
- Run the final local workspace checks below once the whole implementation is ready.
  Exercise changed lifetime/admission boundaries with debug assertions on and off;
  complete feature/backend combinations remain GitHub CI work.
- Measure the complete frozen original/source-form matrix against HP00 and Lua.
  Report per-workload medians, ranges, retained/preparation memory costs and remaining
  allocation/boundary sources. Reprofile the largest remaining gaps.
- Update current architecture and this ledger with results; keep the detailed tables
  in the existing performance report. Record a finite follow-up proposal if needed.

Acceptance has separate outcomes: implementation/local correctness, measured benefit,
complete CI and Lua parity. None implies the others. Reaching the end of HP06 without
parity leaves the performance objective open; it does not authorize unbounded work.

## Measurement and validation contract

Keep the existing 16 matched nontrivial workloads unchanged: arithmetic, arrays,
branches, calls, fibonacci, maps; source-form direct, helper, concrete_generic,
interface, shared_generic, capture_cell, field, byte_state, string_constants and
string_calls. Entry, host adapters and alternate numeric diagnostics remain separate.

Use workspace release settings, default target/build parallelism and existing
source/native SDK features. Record machine/OS/toolchain/Lua version, revisions,
binary hashes, cache state, inputs, warmups and process order. Exclude compilation,
verification/linking/setup from execution; include normal GC and host entry/return.
Use serial paired runs (three warmups, eleven samples per process, both process
orders as in VE09). Run instrumentation separately, with no concurrent builds/tests.

Full paired throughput commands, after HP00 preserves its baseline:

```sh
uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/hp00/baseline-executable
uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/hp00/baseline-executable
```

On the recorded macOS environment, prefix build commands with
`DEVELOPER_DIR=/Library/Developer/CommandLineTools`. HP00 records the actual commands
for its durable counting/sampling support; do not present those future tools as
already available. Report cold preparation and retained cache memory separately,
including repeated reload/retirement and changing application keys.

Phase measurements target affected workloads and a small unaffected control set.
An optimization requires its mechanism gate and a reproducible time reduction beyond
observed noise. Repeat a suspected >5% regression in a control; a repeatable regression
blocks performance acceptance until resolved or explicitly reported as an unmet gate.
Do not choose a best sample or count lower allocation as proof of lower elapsed time.

Final parity requires Kagari/Lua median <=1.0 for **each** of the 16 workloads, repeated
in independent paired runs, with uncertainty analysis near 1.0. No mean, JIT result,
changed fixture, semantic relaxation or lowered threshold replaces this gate.

Reuse existing contract owners; add cases only for uncovered semantic/lifetime rules:

| Phases | Existing focused contract owners |
| --- | --- |
| HP01-HP02 | runtime `objects/application/tests.rs`, `execution_metadata/*/tests.rs`; embed `generic_reload`, `generic_associated_types`; VM interface allocation and native-boundary interfaces/functions/GC contracts |
| HP03-HP04 | runtime frame values; VM execution frames/debugger/host-effects; embed string methods, collection access, async lifecycle/debugger; source-free artifact and native reentry/control contracts |
| HP05 | embed enum payloads/native enums; VM native-boundary enums, hash handles and GC payload failures |

At each implementation checkpoint run selected tests, affected Clippy, formatting,
`uv run --locked scripts/check_structure.py` and `git diff --check`. Keep tests focused;
do not split the whole suite across commands to run it at every phase. HP06 final
local acceptance runs `cargo test --workspace`, `cargo clippy --workspace --all-targets
-- -D warnings`, `cargo fmt --all -- --check`, structure and diff checks. Report actual
commands/results and GitHub CI status separately. Documentation-only checkpoints need
only content/link/diff checks. Use `Phase: HPxx` in implementation commit trailers.

## Progress ledger

- [x] Plan and scope recorded from VE09 evidence and post-VE09 diagnosis.
- [ ] HP00 — Baseline and invariant ownership.
- [ ] HP01 — Shared generic application reuse.
- [ ] HP02 — Interface transfers and metadata admission.
- [ ] HP03 — Managed execution regions and constants.
- [ ] HP04 — Primitive string and collection paths.
- [ ] HP05 — Map enum-pattern reads.
- [ ] HP06 — Local integration and full matched measurements.
- [ ] Complete GitHub CI acceptance.
- [ ] All 16 matched workloads reach Lua parity.

2026-10-10: plan authored against `e2f93e1b`; no implementation started, no new
build/test failures and no new performance result. Future phase entries record only
decisions, invariant proofs, actual checks, measurements and unresolved errors.
