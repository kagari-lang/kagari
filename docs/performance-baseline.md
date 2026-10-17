# Performance Measurements

Historical commands below describe the test layout at their recorded revisions.
TP05-TP09 moved ordinary integration suites into source; use the qualified library
filters in [the testing guide](testing.md) for current code. Old timing samples
are not measurements of the consolidated harnesses.

This page retains measurements needed for the open test-cost, interpreter
performance and architecture follow-up work. Each result applies to its recorded revision and environment,
not automatically to current HEAD. Superseded checkpoint reports live in Git
history; compiler, startup and execution measurements remain separate.
Historical result labels retain `kagari-embed`; current commands and source links
use the renamed `kagari` SDK. This naming update does not rerun measurements.

Keep workloads, hashes, environment and timing scope together. Allocation requests
are not RSS; compilation/setup and execution are separate. Small differences are
not significance claims, and local correctness does not imply GitHub CI or Lua parity.

## Interpreter follow-up evaluation, Windows, 2026-10-17

**Outcome: measured interpreter gains; final lifecycle performance acceptance is
outstanding. Lua parity remains unmet.** IP09 (`ebb22ec2`) retains shared call
admission/return ownership; IP10 (`af9051f7`) retains ordinary cross-crate inlining
of the cancellation predicate. IP11 (`c2e6fbbc`) rejects both collection candidates
rather than retaining complexity that misses predeclared gates. The
[active plan](interpreter-performance-plan.md) retains the unresolved IP12 gate,
phase decisions and exact focused local test selection; this is not complete CI or
performance acceptance.

### Environment and method

Same Windows 11/i9-12900K/24-logical-CPU/approximately 64 GiB machine, Rust/Cargo
1.99.0 and LLVM 23.1.1 as the investigation below. Ordinary workspace release
profile (opt-level 3), default target and Cargo parallelism, unchanged lockfile
SHA-256 `1a18e39bd7312c9ca332af0ebeb81aabb3aca293256f75c85b028a3873b7ba06`.
SDK source/native features were built, but only the interpreter executed; vendored
PUC Lua 5.4.9 through mlua 0.12.2, no JIT/native preparation or execution. No CPU
sampler/counting allocator, build or test overlapped ordinary timing. Process
placement, power management and desktop load were uncontrolled except in the
explicitly separate affinity diagnosis below. No current CPU-stack/hardware-counter
cost partition is available.

Baseline code: `88e87fd4`, frozen before production edits. Measured integrated
candidate: `c2e6fbbc` (production state is IP09 + IP10; IP11 changes were removed).
Original, source-form and lifecycle workload checksums are unchanged. Image SHA-256:

- Baseline: `2a6605f8368310cae43e4981c66a55d7df376dc088b4f66a432574c9bcf5286c`.
- Integrated candidate: `3e04133b0137fc518c5f9248aa195f9ee030a7b631eb31cda89493371cad002c`.

The [existing paired procedure](../benchmarks/lua-comparison/README.md#beforeafter-workflow)
uses seven alternating baseline/candidate process pairs, three warmups and eleven
checked execution samples per route. Pass/fail uses the median of seven within-pair
candidate/baseline sample-median ratios, not the ratio of pooled medians. Pair
ranges are not confidence intervals. Workloads, algorithms, result checks and
cancellation/checked-arithmetic policies are unchanged. Compilation, verification,
preparation and linking are outside execution timers. Setup is diagnostic, measured
once per process; processes reuse warmed execution state.

Reproduction starts by freezing the old revision's ordinary executable and each
suite report, then uses:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/interpreter-ip08/before
uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/interpreter-ip08/before
uv run python scripts/benchmark_lua.py --execution-controls --baseline-executable target/interpreter-ip08/before
uv run python scripts/check_benchmark.py <paired-results.json>
```

The old image must actually contain the baseline revision; rebuilding current code
at its path is not a valid baseline. Reports below retain methods/results directly,
not a dependency on ignored local files. The integrated original/forms/control runs
started at 12:06:15, 12:17:02 and 12:18:53 UTC respectively.

### Retained mechanisms and phase evidence

- **Calls:** one admission transaction inspects candidate membership and heap
  identity together, but reports heap failure at the original late gate and completes
  subsequent candidate/window validation. No callback/collection/mutation invalidates
  that operation-local evidence. Nested return retirement and caller publication
  reuse one bank borrow; allocating result adaptation remains outside it, and caller
  locations resolve after possible compaction. Physical admission/copy, root windows,
  prepared calls, scalar return packets and error order remain checked. No cache,
  new frame field, stack ABI or additional allocation site was introduced.
- **Polling:** ordinary `#[inline]` on the existing cancellation check exposes its
  Acquire load/result across crate boundaries. Scalar release-object inspection
  found two outlined cancellation-check call relocations before, zero after, with
  direct flag reads/tests. The number/order of polls did not change. Numeric kernels,
  initialization checks and checked arithmetic remain in their existing owners.
- **Representation decision:** defer broad local/debug storage remapping, numeric
  kernel ABI replacement and fusion. Ordinary source-free bytecode flow verification
  is not a general initialization proof, and arbitrary in-range frame jumps are not
  a verified CFG transition. No safety defect or permission to remove checks follows
  from this investigation. See the plan's producer/consumer/validity inventory.

IP09's isolated calls ratio was 0.97725 (range 0.94334-1.05190), helper 0.96473,
recursion 0.99459. The 0.98 calls target passed narrowly; this does not establish a
stable 2% improvement. IP10 versus accepted IP09 gave arithmetic 0.83496
(0.81809-0.85699), branches 0.85936 (0.83999-0.87411), calls 0.94873
(0.90982-0.98592), passing its 0.98 arithmetic target and 1.05 controls. These are
complete-workload gains, not isolated per-poll costs. Do not multiply phase medians
to manufacture the independent integrated results below.

### Integrated ordinary execution

All checksums and the 1.05 execution regression checker gates passed for the original
and source-form suites. The 16 matched pure-interpreter workloads remain **2.20-41.07x
Lua time** in this run. Entry (0.99155 before/after), native (0.96859) and host callback
(0.98266) controls are outside matched-language parity.

| Matched workload | Candidate/baseline paired median | Pair range | Candidate/Lua time |
| --- | ---: | ---: | ---: |
| arithmetic | 0.83042 | 0.77824-0.96265 | 3.12 |
| branches | 0.88532 | 0.82278-1.35284 | 2.20 |
| calls | 0.97088 | 0.88663-0.99536 | 16.88 |
| fibonacci | 0.96830 | 0.92993-1.10564 | 25.69 |
| arrays | 0.99899 | 0.96155-1.04545 | 26.22 |
| maps | 0.98648 | 0.85948-1.05306 | 39.72 |
| direct | 0.85357 | 0.46357-0.88567 | 2.90 |
| helper | 0.93514 | 0.58978-0.97103 | 15.32 |
| concrete generic | 0.99503 | 0.52325-1.15236 | 16.81 |
| interface | 0.98029 | 0.91748-1.25348 | 27.60 |
| shared generic | 0.98563 | 0.94326-1.02766 | 41.07 |
| capture cell | 0.95953 | 0.92075-1.02769 | 36.25 |
| field | 0.99614 | 0.95376-1.05850 | 7.00 |
| byte state | 0.99482 | 0.97005-1.05788 | 22.18 |
| string constants | 0.93057 | 0.54098-0.97936 | 8.15 |
| string calls | 0.98510 | 0.79368-1.01230 | 19.43 |

The ranges include substantial placement/load outliers, especially direct/helper/
concrete generic and branches. They are disclosed, not filtered or replaced with
favorable reruns. Small median movements are not general speed claims. For scale,
candidate complete arithmetic took 1,171.4 us, calls 3,401.4 us and helper 1,679.6 us;
matching Lua medians were 375.3, 201.5 and 109.6 us respectively.

Total Kagari setup paired medians were 1.00141 (original) and 0.98867 (forms), within
the predeclared 1.10 diagnostic limit. Executable size is 26,782,208 versus 26,843,136
bytes (0.99773); no new prepared product or retained layout was introduced.

### Rejected collection experiments

Both transactions were prototyped through existing owners with shared semantics,
not private benchmark implementations. Sealed Vec reused nominal/element and mutation
admission while retaining polls, late borrowing, bounds checks and the final checked
write. Map carried occupied/vacant membership through fallible capacity/accounting
preparation and commit. Standard Entry was used only with spare capacity to avoid
infallible growth before the original gate; full occupied tables still needed two
lookups. SDK conversion/reentry validation, custom Hash/Eq, original keys and
collision tokens remained checked. No enum/Option representation changed.

The predeclared target was 0.98 for each affected workload, other controls 1.05:

| Candidate | maps | arrays | interface control | Disposition |
| --- | ---: | ---: | ---: | --- |
| Map + sealed Vec | 0.97634 | 0.98290 | 1.07466 | Reject Vec target miss and control regression. |
| Independent map only | 0.98530 | 0.98599 | 1.00075 | Reject map target miss. |

All selected semantic tests/checks passed, but performance did not. The first
reports started 11:53:49/11:55:51/11:57:26 UTC; the independently scoped map-only
reports 11:59:55/12:01:54/12:03:30. These are different candidates, not replacement
samples selected to pass. All prototype production code/tests were removed. The
second result clears the interface median regression but cannot establish its exact
cause; CPU/code-layout attribution remains missing. Further collection work needs
new attribution; repeating these near-threshold candidates or adding enum effects
is not the accepted next optimization.

### Lifecycle gate still outstanding

All owned admission/retirement median gates passed; explicit post-retirement
collection failed three unrestricted-scheduler rows: forward cancel 1.09524,
forward complete 1.21053, random cancel 1.06061. For scale, forward-complete process
medians were baseline 4.0 us versus candidate 4.6 us; pair range 0.39048-1.60000.
No collection/storage algorithm changed. This is not enough evidence to dismiss the
failure as noise or to claim a GC regression's root cause.

A separate bounded diagnosis was declared before execution: same unchanged images,
seven alternating pairs/3 warmups/11 samples, pin parent/workers to logical CPU 0
with process affinity mask 1, no priority/power changes or relaxed threshold. Windows
GetSystemCpuSetInformation reports group 0 CPUs 0-15 as EfficiencyClass 1, CPUs 16-23
class 0. The run started at 12:21:32 UTC. The original three failed rows then passed,
but random-complete collection **failed at 1.0512820513**, versus the exact 1.05
limit (range 0.82222-1.22500). Do not round that into a pass. All admission/retirement
rows passed again. This experiment reduces one source of variation but does not
establish why the collection gate fails. Both reports remain evidence; IP12 is not
complete, and no more unchanged-candidate acceptance reruns are planned.

To reproduce the constrained diagnosis on Windows after the ordinary image is built:

```text
uv run python -c "import ctypes,subprocess,sys; k=ctypes.WinDLL('kernel32',use_last_error=True); k.SetProcessAffinityMask.argtypes=[ctypes.c_void_p,ctypes.c_size_t]; assert k.SetProcessAffinityMask(ctypes.c_void_p(-1),1); raise SystemExit(subprocess.call([sys.executable,'scripts/benchmark_lua.py','--execution-controls','--baseline-executable','target/interpreter-ip08/before']))"
```

The affinity is local to that process tree and ends with it. It is not the default
benchmark environment or a new language/runtime execution policy. Next diagnosis
belongs to this existing lifecycle gate: distinguish collection work, retained-state
shape and microsecond-scale measurement uncertainty before changing runtime code.

### Correctness and allocation limits

Focused runtime window/frame and VM frame/depth/trap/reload/reentry/owned-drive tests
passed, as did cancellation subscription and observed-effect/cleanup tests. Prototype
collection contracts passed before rejection. Structure, format, workspace all-target
Clippy with warnings denied and diff checks passed at implementation checkpoints.
Unchanged passing checks retain their evidence; no full-workspace test sweep or
feature/backend/platform matrix was run locally. Those remain CI-only.

Separate counting-allocator builds (not throughput evidence) passed original/forms
checksums and all eight owned lifecycle diagnostic cases. Original warm diagnostic
rows exactly match the pre-edit investigation: calls/recursion still request seven
Rust allocations total, create no heap objects/GC, and initialize 360,090/1,182,132
bank bytes with zero moved bytes. Helper requests seven allocations; interface/shared
identity eleven total, one object each, no new environment/application/preparation
per call. Arrays/maps remain 51/2,216 requests and 1/2,001 objects; no collection
prototype benefit is retained. Owned controls assert a three-cycle reuse plateau
and **zero net requested bytes after complete runtime teardown** in every case.
This is neither RSS nor allocator-failure/OOM proof; a live returned report may
legitimately retain bytes before teardown.

```text
cargo build --release --locked -p kagari-lua-benchmark --features diagnostics
cargo run --release --locked -p kagari-lua-benchmark --features diagnostics -- --diagnostics
cargo run --release --locked -p kagari-lua-benchmark --features diagnostics -- --diagnostics --source-forms
cargo run --release --locked -p kagari-lua-benchmark --features diagnostics -- --diagnostics --execution-controls
```

Do not use the diagnostic binary for ordinary timing; restore the default-feature
ordinary build afterward. These local checks establish selected contracts and
bounded allocation behavior, **not** full CI, the outstanding lifecycle timing gate
or Lua parity.

## Interpreter attribution, Windows, 2026-10-17

This is a read-only execution investigation at `88e87fd4`, not a new optimization
checkpoint. It combines fresh pure-interpreter timing, independent logical/protocol
counts, source inspection and ordinary release-object disassembly. No production
code or workload was changed. The earlier macOS results below are not pooled into
this measurement; their sampling percentages are not current Windows attribution.
The [roadmap](implementation-roadmap.md#interpreter-performance-follow-up) continues
to own the bounded unresolved work, not a second optimization queue.

### Environment and method

- Intel Core i9-12900K, 24 logical CPUs, approximately 64 GiB RAM; Windows 11
  10.0.26300, x86_64-pc-windows-msvc.
- rustc 1.99.0 (`b940084d7`, LLVM 23.1.1), Cargo 1.99.0. Workspace release defaults
  (opt-level 3, no LTO override), default target and build parallelism, no RUSTFLAGS,
  jobs, target, CFLAGS or CC overrides. SDK source/native features are enabled;
  native preparation/execution is excluded through `--interpreter-only`.
- mlua 0.12.2, lua-src 551.0.2, vendored PUC Lua 5.4.9, not LuaJIT. The resolved
  dependency's `lua-5.4.9/lua.h` reports release 9.
- Two sequential fresh processes per suite, second reversing workload/initial engine
  order; three warmups and eleven execution samples per row/engine/process. Tables
  report the median of the two process medians, normalized by batch. Ranges are the
  two process medians, not confidence intervals. All checksums pass.
- Compilation, verification, preparation and linking are outside execution timing.
  Cargo cache was reused where available; the original release build took 103.110 s
  and the forms no-change build 0.396 s. Execution is warmed; these are not cold-start
  observations. No agent-started build, test, diagnostic or profiling command overlaps
  ordinary timing. CPU affinity, P/E-core selection, clocks and desktop load are not
  controlled. Two processes support diagnosis, not a small-change significance claim.
- Both suites use the same ordinary executable SHA-256:
  `4098796eb2770062ecb7ffe20fcd9e02696836fb2fe95e6dc7670c418c905237`.
  The Cargo.lock SHA-256 is
  `1a18e39bd7312c9ca332af0ebeb81aabb3aca293256f75c85b028a3873b7ba06`.
  Reports were collected at UTC 20261017T103032Z and 20261017T103359Z.

The paired sources use matching successful algorithms, not identical runtime
contracts. Kagari retains checked numeric behavior, typed results, scoped handles
and pinned executable dependencies. Vec is compared with dense Lua tables; map
lookup returns an ordinary Option versus Lua's value/nil. No semantic difference
is assigned an assumed fraction of the measured gap.

### Fresh execution observations

Microseconds per complete workload; entry is normalized per public call.

| Original workload | Kagari us | Kagari process range | Lua us | Lua process range | VM/Lua |
| --- | ---: | --- | ---: | --- | ---: |
| entry | 1.242 | 1.238-1.246 | 0.035 | 0.034-0.035 | 35.68 |
| arithmetic, 50,000 iterations | 1422.850 | 1420.100-1425.600 | 384.250 | 377.400-391.100 | 3.70 |
| branches, 30,000 iterations | 1690.150 | 1680.500-1699.800 | 681.700 | 679.200-684.200 | 2.48 |
| calls, 10,000 iterations | 3609.300 | 3561.500-3657.100 | 202.950 | 199.700-206.200 | 17.78 |
| fibonacci, n=20 | 7601.100 | 7484.300-7717.900 | 280.700 | 280.500-280.900 | 27.08 |
| arrays, 2,000 elements | 1975.350 | 1966.200-1984.500 | 76.050 | 75.800-76.300 | 25.97 |
| maps, 1,000 keys | 3033.200 | 3008.200-3058.200 | 74.800 | 72.200-77.400 | 40.55 |

The source forms each run 5,000 iterations. The matched rows are:

| Source form | Kagari us | Kagari process range | Lua us | Lua process range | VM/Lua |
| --- | ---: | --- | ---: | --- | ---: |
| direct | 194.800 | 193.200-196.400 | 57.100 | 57.100-57.100 | 3.41 |
| helper | 1819.050 | 1781.000-1857.100 | 110.550 | 109.100-112.000 | 16.45 |
| concrete_generic | 1833.100 | 1818.700-1847.500 | 105.800 | 105.400-106.200 | 17.33 |
| interface | 3469.650 | 3442.900-3496.400 | 122.550 | 122.500-122.600 | 28.31 |
| shared_generic | 4854.400 | 4833.300-4875.500 | 115.750 | 114.300-117.200 | 41.94 |
| capture_cell | 4721.250 | 4697.500-4745.000 | 126.450 | 124.200-128.700 | 37.34 |
| field | 743.950 | 741.800-746.100 | 103.650 | 103.200-104.100 | 7.18 |
| byte_state | 3191.450 | 3175.600-3207.300 | 138.000 | 137.100-138.900 | 23.13 |
| string_constants | 691.000 | 675.700-706.300 | 79.000 | 78.500-79.500 | 8.75 |
| string_calls | 2616.200 | 2593.500-2638.900 | 133.950 | 133.600-134.300 | 19.53 |

Separate controls are host_callback 914.300/257.800 us (3.55x) and unmatched native
917.450/110.400 us (8.31x). The former calls the same Rust helper through both host
adapters; the latter compares a Rust helper with a Lua script helper. Neither is
substitute evidence for script-call or interpreter parity. The 16 matched rows take
2.48-41.94 times Lua time on this machine.

### Instruction stream and scalar execution

Independent observer/hook counts, outside ordinary timing:

| Workload | Kagari logical instructions | Lua hook-counted instructions |
| --- | ---: | ---: |
| arithmetic | 550,015 | 250,007 |
| calls | 210,015 | 110,007 |
| fibonacci | 240,801 | 120,400 |
| arrays | 82,030 | 42,012 |
| maps | 63,030 | 24,012 |
| direct | 75,015 | 40,006 |
| helper | 105,012 | 65,007 |
| interface | 110,014 | 65,009 |
| shared_generic | 95,017 | 60,009 |

These count language VM operations, not CPU instructions or equally costly units.
Observer attachment changes Kagari's execution route, so its elapsed time is not
throughput evidence. Lua skips successful arithmetic's encoded metamethod fallback;
its comparison consumes/skips the following jump without a separate dispatch.

For `sum += i % 97; i += 1`, the successful Kagari iteration executes three local
loads, two local stores, four numeric/comparison operations, one branch and one
backedge: eleven logical operations. Lua executes five handlers: comparison,
constant remainder, addition, immediate increment and backedge. Both fixtures use
while, not Lua's specialized numeric-for. Kagari's 250,005 local loads/stores are
about 45.5% of this complete logical stream, not 45.5% of elapsed time.

[Copy propagation](../crates/kagari-mir/src/passes/copies.rs) already reuses and
hoists scalar constants; they are not loaded every loop iteration. Loop-header
forwarding declines an unprocessed backedge, and local stores retain debugger
availability. [Physical preparation](../crates/kagari-runtime/src/module/execution.rs)
turns local reads/writes into moves while
[register allocation](../crates/kagari-runtime/src/module/execution/allocation.rs)
reuses temporary slots but keeps named locals separate. Thus better slot allocation
does not itself eliminate local traffic or the comparison-result/branch pair.

The ordinary, unbounded
[scalar loop](../crates/kagari-runtime/src/frame/cursor/scalars.rs) was disassembled
from its release runtime object using llvm-objdump. It confirms that:

- Rust match already lowers to a jump table; dispatch is not a linear opcode chain.
- Fetch checks instruction range, publishes executing-PC state and advances the PC.
- Operand initialization/range checks and result initialization writes survive.
- The normal logical-boundary path calls the out-of-line
  [`CancellationToken::check`](../crates/kagari-common/src/cancellation.rs), then
  checks observer/collection/abandonment state. The cancellation check is not merely
  an inlined atomic load on this build. There is no active unbounded countdown.
- Numeric execution performs an indirect call through the prepared kernel pointer.
  The Windows ABI returns its Result through a stack-addressed output; the loop
  tests the status and reloads the payload. This is in addition to opcode dispatch.

The i32 Add, Rem and Lt
[kernels](../crates/kagari-types/src/payload.rs) were inspected separately. Carrier
range validation remains; addition uses native 32-bit add plus an overflow branch,
and remainder uses 32-bit idiv, not an i128 division helper. Source-level overlapping
numeric checks must not be counted twice without machine-code evidence. Prepared
instructions are 24 bytes, Value is 16 and canonical BytecodeInstruction is 136 on
this target; only the prepared representation describes the hot instruction stream.
Lua's regular instruction is four bytes. These are layout facts, not cache-miss or
speedup measurements.

This separates excess logical work from per-operation machinery, but does not assign
a fraction of time to dispatch, cancellation, initialization, ABI traffic or cache
behavior. The old macOS broader-loop attribution cannot supply those missing fractions.

### Calls: warm preparation is not a cheap transition

The [forms source](../benchmarks/lua-comparison/src/forms.rs) places the same
recurrence directly in a loop or in `step(i32) -> i32`. Helper takes about 9.34 times
direct time, while its logical instruction count rises only about 1.40 times.
Lua's corresponding time ratio is about 1.94. This is strong evidence to inspect
call execution, not a precise isolated per-call latency or a proof that all of the
difference is frame setup. Helper also executes more moves and callee constants.

Fresh warmed diagnostics record:

| Workload | Rust allocation requests | Script objects | GC collections | Frame admissions/returns | Initialized bank bytes | Moved bank bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| calls | 7 | 0 | 0 | 10,002 / 10,002 | 360,090 | 0 |
| fibonacci | 7 | 0 | 0 | 21,892 / 21,892 | 1,182,132 | 0 |
| helper | 7 | 0 | 0 | 5,001 / 5,001 | 180,072 | 0 |
| interface | 11 | 1 | 0 | 5,001 / 5,001 | 225,120 | 0 |
| shared_generic | 11 | 1 | 0 | 5,001 / 5,001 | 205,147 | 0 |

Warm method/interface/shared/native/operation preparation, environment allocation
and layout-comparison counters are zero in these cases. No per-call eight-byte
allocation from the historical HP06 observation remains. Metadata-validation counters
exclude cheap availability/ownership checks. Zero preparation does not imply zero
application-key lookup, argument validation or program/environment admission.

The current concrete path is: end cursor borrow ->
[`push_prepared_call`](../crates/kagari-runtime/src/frame/calls.rs) ->
[frame admission](../crates/kagari-runtime/src/frame.rs) ->
[transactional window allocation/transfer](../crates/kagari-runtime/src/frame/values.rs)
-> driver and new admitted region ->
[return/window retirement](../crates/kagari-runtime/src/frame/returns.rs) -> caller
region. Arguments still participate in candidate-value admission, heap-value
admission, physical admission and transfer. Scalar arguments avoid heap graph walks,
but source-window/initialization work remains. Reserve and zero-initialization can
cost work without a warmed allocator request. Ordinary nested calls retire tail
windows; independent-session compaction is not their bottleneck explanation.

Interface calls additionally select the current receiver/method and validate typed
arguments/results. The shared_generic fixture is a generic method through an
interface, not the separate statically selected Shared-call branch.
[Application processing](../crates/kagari-runtime/src/objects/application.rs) checks
arguments and builds/looks up a key even on a warm hit. Generic values use tagged
Value transport rather than the concrete all-scalar route; an i32 Value is not a
heap box. Shared_generic has fewer logical instructions than helper but about 2.67
times its elapsed time. This identifies materially different operation costs, not
an exact application-lookup percentage.

### Collections: concrete repeated work

Warmed arrays record 51 Rust allocation requests, 19,828 requested bytes, one script
object and two GCs. Maps record 2,216 requests, 1,490,978 requested bytes, 2,001 script
objects and five GCs. These are per-invocation allocation traffic, not RSS or retained
memory. Independent ordinary ten-second windows also observe one script object and two
GCs per array invocation, and 2,001 objects and five GCs per map invocation.

**Vec:** this fixture already uses compact `Vec<i32>` backing storage, not one heap
object per element. Index/set use the installed in-region primitive, not a general
native callback for every access. Push remains an allocating callback transition.
The 2,001 slow-boundary count corresponds to construction and 2,000 pushes; calls
and returns through the prepared script path are not counted as slow boundaries.

The [sealed Vec setter](../crates/kagari-runtime/src/native/primitive.rs) still uses
preflight followed by a separately checked commit through
[buffer operations](../crates/kagari-runtime/src/gc/buffers.rs): mutation exclusion,
bounds and element admission overlap. The general SDK separation permits conversion
and reentry; this particular sealed path has no such gap. Source shows five explicit
execution-poll call sites across primitive entry/exit, setter and preflight, in
addition to surrounding interpreter polling. This is a candidate for one checked
transaction owner, not authorization to delete dynamic validation or polls.

**Map insertion:**
[`GcHeap::map_insert`](../crates/kagari-runtime/src/gc/maps_sets.rs) calls contains_key
for growth/accounting, then
[`HashMapStorage::insert`](../crates/kagari-runtime/src/native/hash_storage.rs)
calls contains_key again before HashMap::insert. Each successful insertion or
replacement therefore makes three explicit table-search API calls, not one. The
fixture's 2,000 inserts issue 6,000 such calls, plus 2,000 gets; rehash work is extra.
This is verified duplicate membership work across accounting/storage ownership,
not a required language semantic. Its elapsed share is not measured.

**Map lookup and match:** the i32-key fixture takes the
[builtin-key branch](../crates/kagari-stdlib/src/bindings/hash.rs); it does not call
script Hash/Eq per lookup or allocate a key vector. Each get nevertheless constructs
an ordinary heap-backed Some with a payload buffer. The 2,000 successful gets explain
2,000 enum objects in addition to the map. They then execute exactly 2,000
TestEnumVariant and 2,000 ReadEnumPayload instructions. These remain canonical
[VM boundary handlers](../crates/kagari-vm/src/executor/aggregate_ops.rs), separately
borrowing/validating the enum. They are not in-region prepared enum operations.
Maps' 8,001 slow boundaries cover 4,001 native operations and 4,000 enum operations;
GC transitions add their own duties. This is a multi-stage result protocol, not
merely a slower hash function.

### Mature implementation comparison and follow-up limits

Primary reference is PUC Lua 5.4.9, consulted on 2026-10-17:

- [Instruction encoding](https://www.lua.org/source/5.4/lopcodes.h.html) and
  [VM](https://www.lua.org/source/5.4/lvm.c.html): compact register instructions,
  constant/immediate arithmetic and comparison-controlled jumps; dynamic numeric
  labels remain, but common arithmetic lives directly in handlers.
- [Call implementation](https://www.lua.org/source/5.4/ldo.c.html): supplied arguments
  already occupy the callee's stack area; CallInfo is reused when available. Lua-to-Lua
  execution switches frames within luaV_execute. The usual single-result return has
  a hook-disabled direct path. Lua still checks capacity and handles growth/errors.
- [Table helpers](https://www.lua.org/source/5.4/lvm.h.html) and
  [storage](https://www.lua.org/source/5.4/ltable.c.html): dense integer access can
  directly address the array part; sparse integer lookup uses a specialized hash
  path. Replacement reuses the found slot, and successful get returns its TValue
  without a separately allocated Option. New-key/growth/metamethod paths remain.

Lua's live stack/closure graph owns roots and code retention; it does not implement
Kagari's typed host loans, checked stale handles, versioned publication or method
application contracts. Numeric overflow/remainder rules differ too. Copying its
unchecked pointer access or dropping Kagari's boundaries is not an equivalent
implementation. Conversely, these differences do not prove the present amount of
repeated admission, polling, transport or result allocation is necessary.

The proposed [interpreter performance plan](interpreter-performance-plan.md) owns
the finite follow-up sequence, model constraints, attribution questions and acceptance:
internal call admission/return, uniform instruction/local/PC representation, then
bounded collection transactions and the enum-boundary decision. Its phases are
unstarted. This measurement section owns the evidence, not a parallel implementation
or decision ledger.

No current-HEAD CPU-stack/hardware-counter profile or removal experiment was collected.
The remaining time shares, branch/cache effects and achievable speedups are unknown.
The investigation establishes actual excess logical work, surviving machine-code
mechanisms and repeated source-level operations; it does not prove that one change
will close the whole gap or that dispatch is 81% of current Windows time.

### Reproduction and validation

```text
uv run python scripts/benchmark_lua.py --interpreter-only --runs 2
uv run python scripts/benchmark_lua.py --source-forms --interpreter-only --runs 2
cargo run --release --locked -p kagari-lua-benchmark -- --interpreter-only --profile=arithmetic
cargo run --release --locked -p kagari-lua-benchmark -- --interpreter-only --profile=calls
cargo run --release --locked -p kagari-lua-benchmark -- --interpreter-only --profile=fibonacci
cargo run --release --locked -p kagari-lua-benchmark -- --interpreter-only --profile=arrays
cargo run --release --locked -p kagari-lua-benchmark -- --interpreter-only --profile=maps
cargo run --release --locked -p kagari-lua-benchmark -- --source-forms --interpreter-only --profile=direct
cargo run --release --locked -p kagari-lua-benchmark -- --source-forms --interpreter-only --profile=helper
cargo run --release --locked -p kagari-lua-benchmark -- --source-forms --interpreter-only --profile=interface
cargo run --release --locked -p kagari-lua-benchmark -- --source-forms --interpreter-only --profile=shared_generic
cargo run --release --locked -p kagari-lua-benchmark --features diagnostics -- --diagnostics
cargo run --release --locked -p kagari-lua-benchmark --features diagnostics -- --source-forms --profile=helper --diagnostics
cargo run --release --locked -p kagari-lua-benchmark --features diagnostics -- --source-forms --profile=interface --diagnostics
cargo run --release --locked -p kagari-lua-benchmark --features diagnostics -- --source-forms --profile=shared_generic --diagnostics
```

Actual counting runs invoked one saved ordinary executable serially, using the same
arguments as the commands above; diagnostic runs used the separately built diagnostics
image. Profile mode supplies ten-second execution-only windows and counts afterward;
no sampler was attached. Diagnostic timings and hook/observer timings are excluded.
For assembly, run llvm-objdump --syms on the ordinary release runtime/type rlibs,
select ExecutionCursor::execute_slice<Unbounded> and the signed 32-bit Add/Rem/Lt
payload monomorphizations, then disassemble those symbols with relocations and Intel
syntax. Keep the ordinary object identity distinct from diagnostic artifacts.

All timing/counting/diagnostic results passed their existing independent result
assertions. Only documentation is changed; documentation link/content and diff
checks apply, not workspace builds/tests or new correctness acceptance. No production
performance improvement or full CI acceptance is claimed.

## Interpreter call lifecycle, 2026-10-17

IP01-IP07 completed local implementation/decision acceptance with runtime f070a092.
Baseline runtime is 60bfaacb, rebuilt with the same simplified benchmark harness
from b1d20e87. Earlier Windows runs and earlier macOS candidates are separate
historical evidence; they are not pooled into this result. Required window-slot
reuse and prepared calls are retained. Conditional session segmentation, typed/fused
instructions and additional library shortcuts are deferred for the reasons below.
Complete workspace/feature/backend/platform CI and interpreter/Lua parity remain
outstanding. The [roadmap](implementation-roadmap.md#interpreter-performance-follow-up)
owns further work; [architecture](architecture.md#runtime-model) owns current contracts.

Environment: Apple M1 Max, 32 GiB, 10 logical CPUs, arm64 macOS 26.6.2;
rustc 1.98.1 (48a229cea, LLVM 22.1.8), Cargo 1.98.1, workspace release profile
(opt-level 3), default target/build parallelism. Benchmark features are SDK
source/native and vendored Lua 5.4.9; native execution is excluded. Private-window
controls use the runtime's ordinary release test harness. Builds reuse Cargo cache;
workers run sequentially on one thread in fresh processes. No compilation, tests,
allocation instrumentation or sampling overlaps ordinary timing.

Each suite uses seven fixed process pairs, alternating B/C and C/B. Workload/engine
order reverses every two pairs. Every worker prepares once, then runs three warmups
and eleven execution samples per row, checking results outside timers. The sole
gate statistic is median(C_i/B_i), where each input is a process median after batch
normalization. Ranges below are min/max paired ratios, not confidence intervals.
Setup and Lua timings are diagnostics, outside regression gating. Workload, binary,
lockfile and build identities are checked; there are no per-source-file manifests,
reference calibration, report extensions or second acceptance estimator.

All **61/61** gates pass: 19 execution rows (16 matched language workloads plus
entry, host-callback and unmatched-native controls), 24 owned-root lifecycle rows
and 18 private-window rows. Calls, Fibonacci, interface and shared-generic targets
require <=0.90; every other row requires <=1.05. The four reports were collected at
UTC 20261017T072149Z (owned roots), 072334Z (forms), 072510Z (private windows) and
072624Z (original). They remain the final evaluation because the later conditional
reviews and documentation handoff change no production code. No rerun was used to
replace an unfavorable result from the same candidate.

Medians below are microseconds; the paired ratio is not the ratio of these two
aggregate medians. Lua ratios use candidate process medians for diagnostic context.

| Matched workload | IP01 us | Final us | Paired ratio | Pair range | Final / Lua |
| --- | ---: | ---: | ---: | --- | ---: |
| arithmetic | 1793.291 | 1731.458 | 0.972187 | 0.947320-0.980760 | 4.44 |
| arrays | 2611.166 | 2043.375 | 0.777376 | 0.772635-0.798497 | 28.70 |
| branches | 2212.666 | 2174.709 | 0.977975 | 0.973450-0.983599 | 2.66 |
| calls | 5119.458 | 3938.125 | 0.770502 | 0.762498-0.782246 | 17.28 |
| fibonacci | 10408.667 | 8067.459 | 0.774058 | 0.765708-0.781639 | 22.62 |
| maps | 3683.958 | 3020.375 | 0.826254 | 0.770090-0.851394 | 42.17 |
| byte_state | 3792.667 | 3210.500 | 0.846110 | 0.831384-0.858971 | 27.39 |
| capture_cell | 6214.166 | 5231.916 | 0.838625 | 0.827451-0.851557 | 35.96 |
| concrete_generic | 2579.916 | 1930.500 | 0.746617 | 0.736781-0.763291 | 18.42 |
| direct | 246.166 | 241.333 | 0.984530 | 0.965304-1.024987 | 3.15 |
| field | 724.208 | 698.458 | 0.966744 | 0.930720-0.989838 | 7.60 |
| helper | 2521.667 | 1940.708 | 0.770671 | 0.764262-0.782624 | 13.49 |
| interface | 4503.750 | 3452.209 | 0.767535 | 0.763770-0.791600 | 24.94 |
| shared_generic | 6434.917 | 4818.042 | 0.750595 | 0.732977-0.758593 | 43.68 |
| string_calls | 3151.958 | 2500.083 | 0.796388 | 0.775182-0.809549 | 19.52 |
| string_constants | 693.084 | 649.500 | 0.938492 | 0.910989-0.960564 | 9.59 |

The 16 matched workloads still take **2.66-43.68 times Lua time**. This work improves
call transitions; it does not meet parity. Additional control ratios are entry 0.876029, host_callback 0.629000, native 0.626827.
The largest accepted ratio in owned roots/private windows is 1.026663/1.034768.
The 256-forward-cancel collection ratio is 0.515312 (range 0.460761-0.559034),
with admission/retirement 0.809541/0.982578.

The earlier b1d20e87 comparison passed 60/61 and observed 1.068697 on that collection
row, with pair range 0.89118-1.28109. Identical temporary baseline/current GC probes
found equal graph work in 112 corresponding records: no value/metadata roots,
active windows, linked calls or live/reclaimed objects, and one 35-member program.
They also exposed 1/32/256 expired weak lease registrations in one member table,
matching the preceding session population. This was a shared bookkeeping cost;
it does not prove the precise cause of the isolated 6.9% observation.

The retained change counts live registrations with lazy per-kind Arc ownership
tokens. Clones share a registration, and only last drop releases it; whole-program
registrations belong to the program root, with local/member counts composed on
read. Collection no longer prunes expired registration lists. Complete member
validation, foreign/stale identity checks, generation-pinned dependencies and
cross-thread final drop are preserved. No custom atomic counter, finalizer queue,
unsafe storage or workload recognition is added. The new comparison above is a
changed implementation, not reclassification or extension of the earlier failure.

Allocation instrumentation runs separately. Seven original and 36 forms/protocol
warm records have no increased request count/bytes. All eight owned-root cases keep
identical warmed requests/bytes; their live/empty/reused increments are below IP01
(ratios 0.99618-0.99730), reuse plateaus are bounded and teardowns return to zero.
Eight artifacts over three setup cycles pass every +5% gate: prepared retained
1.00123-1.00208, additional loaded retained 1.00113-1.00138, preparation requested
0.99752-1.00335, additional load requested 0.81564-0.82255. Warmed teardowns are zero;
cold shared preparation is accounted separately. Unchanged IP01 setup observations
are reused; execution/root observations were collected for the new implementation.
The named registration wrapper preserves the measured single-Arc layout/allocation.

Ordinary original-suite setup ratios are source-to-artifact 1.0001-1.0042,
artifact preparation 0.9997-1.0063, runtime initialization 0.7264-0.9499 and linking
0.7414-0.8345. Forms-module ratios are 1.0015/1.0044/0.9066/0.7810 respectively.
These prepare-once diagnostics can mix first-workload and reused process state;
they establish no cold-start guarantee. Prepared instructions remain 24 bytes and
Value remains 16 bytes. Whole-image Mach-O __TEXT is 17,874,944 bytes at IP01 and
17,891,328 bytes at f070a092 (+0.092%); this is not isolated handler footprint.

The projection/publication review retains one shared operand decoder and window
allocator. Scalar operands require a temporary Value while managed operands can
be borrowed; a closure keeps both within the admitted borrow. The existing
with_value/with_operand inline boundary avoids materializing that projection
across calls. Windows::insert consumes publication after reserve; earlier MSVC
assembly showed an outlined 176-byte temporary. Existing hints remain localized
to these boundaries, with no struct-only decoder or handler-specific hint added.
Rust 1.90.0 [Option](https://github.com/rust-lang/rust/blob/1.90.0/library/core/src/option.rs)
and [Vec](https://github.com/rust-lang/rust/blob/1.90.0/library/alloc/src/vec/mod.rs)
illustrate borrowing/consuming ownership, but their ordinary inline hints are not
proof of Kagari's stronger hint requirements. ARM64 field assembly confirms the
projected path has no calls to either decoder layer; this is platform-specific.

### Conditional decisions and remaining attribution

Session segmentation is deferred. Four existing 256-root controls were sampled
sequentially for five seconds at one-millisecond intervals inside ten-second warmed
lifecycle loops, using temporary non-inlined park/retire wrappers. Results were
checked and sources restored. The denominator is retirement samples; movement is
a subset of release, and nested inclusive samples are not added twice.

| Retirement | Samples | Storage release | Release memmove | Abandoned-owner scan |
| --- | ---: | ---: | ---: | ---: |
| Forward complete | 2,137 | 20.82% | 8.33% | 28.64% |
| Forward cancel | 2,107 | 23.78% | 8.88% | 32.99% |
| Random complete | 1,876 | 14.02% | 5.38% | 31.56% |
| Random cancel | 1,822 | 15.20% | 5.60% | 36.94% |

Forward/random cycles move 2,350,080/1,237,464 bytes, with 18,432 initialized bytes
and 256 windows. The entire random-completion release share is below the proposed
15% retirement gain; segmentation cannot remove all release duties. Sampling and
compiler effects prevent a hard upper-bound claim, but the evidence does not justify
per-session banks, retained capacities and GC/reentry migration under the +5% memory
gate. The separate owner scan visits 131,328 slots during 256 terminal retirements
and the final drain (source-derived count). No owner-notification queue is added.
[Lua 5.4.9 state stacks](https://www.lua.org/source/5.4/lstate.h.html) and
[JVM SE 25 private stacks/shared heap](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-2.html#jvms-2.5.2)
are useful ownership references; Kagari sessions additionally require suspension,
host reentry, explicit roots and pinned reload. They are not Java threads.

Typed/fused additions are deferred. Existing ordinary-image `--profile=arithmetic`
and `--profile=branches` runs use three warmups and a ten-second execution window;
five-second/one-millisecond sampling precedes independent logical counting.
Numeric kernels account for 273/3,602 samples (7.58%) and 341/3,653 (9.33%);
the scalar loop directly accounts for 81.45%/81.00%. Logical counts are
550,015/600,022. The loop includes fetch, operand access, publication and polling;
the profile does not isolate indirect dispatch or predict a typed-opcode gain.
The five candidate i32 Add/Mul/Rem/Eq/Lt variants would add dispatch selection
beside already type-selected shared kernels. A uniform representation review is
preferred over that narrow addition; no prototype or speed claim is retained.
Fusion has no accepted interior-PC/resumption/root model and remains deferred.

The [Rust MIR guide](https://rustc-dev-guide.rust-lang.org/mir/index.html) informs
explicit operand/control-flow ownership. JVM SE 25
[iadd](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-6.html#jvms-6.5.iadd)
wraps instead of trapping; [Rust overflow behavior](https://doc.rust-lang.org/reference/expressions/operator-expr.html#overflow)
depends on compilation policy. Both differ from Kagari's checked numeric contract.
These sources, read on 2026-10-17, support design comparison, not copying host
operators or an unmeasured speedup.

Existing StringByteLength, VecIndex and VecSet/VecSetFluent bodies remain installed
by exact identity/signature and share their native/SDK semantic owners. No duplicate
stdlib body remains. Vec push is deferred because heap growth invalidates the
closed cursor's entry collection snapshot. Map lookup is deferred because selected
hash/equality may call script and the ordinary Option result allocates. The existing
native transition owns these effects; no name-based shortcut or second collection
engine is added. Lua 5.4.9's [VM](https://www.lua.org/source/5.4/lvm.c.html) saves state
around reentry and publishes live stack limits before relevant GC checks. Kagari
also preserves scoped host borrowing, checked handles, pinned calls and cancellation.

### Local validation and reproduction

The implementation passes eleven selected retention/generation, generic reload,
async lifecycle and source-free tests, plus post-wrapper clone/count checks.
Structure (1,284 Rust files, zero violations), formatting, workspace all-target
Clippy and diff checks pass. Earlier applicable window reuse, transfer, trap/cleanup,
scoped host/reentry and allocation checks remain evidence for unchanged code.
Final composition adds four passing tests: two deterministic frame isolation/depth
cases, nested host breakpoint/trap reentry, and the public owned native-wait contract.
The existing runtime-storage diagnostic also passes all sixteen zero-net teardowns.
Its 512-object live/empty/reused increments remain below the recorded RS values:

| Population | Live bytes | Empty bytes | Reused bytes |
| --- | ---: | ---: | ---: |
| Point | 57,036 | 39,460 | 65,644 |
| Pair<i32> | 57,812 | 39,500 | 66,004 |
| Narrow interface | 127,584 | 100,704 | 148,064 |
| Wide interface | 128,808 | 100,704 | 149,288 |
| Generic interface | 128,960 | 101,120 | 149,472 |
| One copied interface | 17,544 | 6,848 | 17,608 |
| Cyclic nodes | 413,380 | 71,828 | 429,764 |
| Numeric buffer | 9,176 | 5,896 | 9,208 |

Loaded setup is 5,196,147 requested bytes (+0.193% against the recorded RS setup).
The three 50,000-iteration field loops retain 98 requests/6,928 bytes/zero net bytes.
These are allocation observations, not fresh RS timing claims. They preserve the
RS savings; no OM heap-layout implementation has been introduced. Exhaustive CI is
not run locally and is not claimed passed.

The final selected commands are:

```text
cargo test --locked -p kagari-vm --lib -- tests::execution::frames::deterministic_frames_
cargo test --locked -p kagari-vm --lib -- tests::reentry_debug::nested_breakpoints_
cargo test --locked -p kagari --test async_execution -- sdk_owned_native_wait_contract
cargo run --locked --release -p kagari-lua-benchmark --example runtime_storage --features diagnostics
```

The [benchmark guide](../benchmarks/lua-comparison/README.md#beforeafter-workflow)
owns the simplified procedure. To reproduce this historical comparison, build the
same current harness on the baseline runtime before saving each suite's executable
and companion report, then compare the candidate on the same machine. Do not use
an old report format or freeze the candidate as its own baseline. For example:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --freeze-baseline <baseline>
uv run python scripts/benchmark_lua.py --source-forms --interpreter-only --freeze-baseline <baseline>
uv run python scripts/benchmark_lua.py --execution-controls --freeze-baseline <baseline>
uv run python scripts/benchmark_lua.py --window-controls --freeze-baseline <window-baseline>
uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable <baseline>
uv run python scripts/check_benchmark.py <original-results.json> --target calls=0.90 --target fibonacci=0.90
uv run python scripts/benchmark_lua.py --source-forms --interpreter-only --baseline-executable <baseline>
uv run python scripts/check_benchmark.py <forms-results.json> --target forms_interface=0.90 --target forms_shared_generic=0.90
uv run python scripts/benchmark_lua.py --execution-controls --baseline-executable <baseline>
uv run python scripts/check_benchmark.py <controls-results.json>
uv run python scripts/benchmark_lua.py --window-controls --baseline-executable <window-baseline>
uv run python scripts/check_benchmark.py <windows-results.json>
```

Freeze commands run on the baseline checkout; comparison commands run on the
candidate. Keep saved binaries unchanged throughout a comparison. For scalar
attribution, use the ordinary benchmark's `--profile=<workload>` and attach the
sampler only after PROFILE_READY. Allocation and temporary ownership probes are
separate diagnostics; their timing is not throughput evidence. Git history owns
the completed phase ledger and rejected implementation history.

## Runtime storage, 2026-10-16

Baseline runtime: 159c74ef, with the probe frozen in afec4a3b. Storage checkpoints:
086e422f (shared interface descriptions), ce56ae7f (compact directory), 7a8c85f4
(type-directed fields/direct scalar transfer), followed by the packed field arena
and sweep-count correction in d4b5a761. Final measured implementation: 35077323
(closed scalar admission and nonempty retirement batches). These are storage
microbenchmarks, not Lua/Java parity or a general interpreter speed claim.

Environment: Apple M1 Max, arm64 macOS 26.6.2 (25G83), rustc 1.98.1
(48a229cea 2026-09-01); workspace default release profile, target directory and
build parallelism. The benchmark enables SDK source/native features. Ordinary
latency runs do not enable diagnostics. Allocation runs separately enable the
counting allocator through diagnostics; their times are not performance evidence.

Reproduce with:

```text
cargo run -p kagari-lua-benchmark --example runtime_storage --release
cargo run -p kagari-lua-benchmark --example runtime_storage --release --features diagnostics
```

The [probe](../benchmarks/lua-comparison/examples/runtime_storage/main.rs) reports
compilation, program preparation and linking separately. Field/scalar loops run
50,000 iterations; traversals and lifecycle populations use 512 objects. Each
ordinary workload uses three warmups and eleven measured samples, reporting
minimum/median/maximum. Lifecycle samples use fresh runtimes and explicit live
and post-release collections; automatic collection is disabled equally. No test
or build runs concurrently with reported latency comparisons. Small controls use
one object. Allocation runs first warm shared immutable program preparation, then
measure setup, construction, live GC, sparse/released roots, empty GC, reuse and
runtime teardown. One Point initially populates 6,430 shared-program bytes, reported
separately; warmed teardown is zero in all sixteen population/control cases.

Requested-byte increments above each independently loaded setup:

| Population (512) | Baseline live | Current live | Baseline empty | Current empty | Current reused |
| --- | ---: | ---: | ---: | ---: | ---: |
| Point | 220,896 | 58,084 | 187,632 | 40,508 | 66,692 |
| Pair<i32> | 221,672 | 58,860 | 187,672 | 40,548 | 67,052 |
| Narrow interface | 834,960 | 128,616 | 613,056 | 101,752 | 149,096 |
| Wide interface | 1,437,072 | 129,840 | 613,056 | 101,752 | 150,320 |
| Generic interface | 1,314,192 | 129,992 | 666,304 | 102,168 | 150,504 |
| One copied interface | 18,776 | 18,576 | 9,472 | 7,896 | 18,640 |
| Cyclic nodes | 581,360 | 414,428 | 367,456 | 72,876 | 430,812 |
| Numeric buffer | 10,256 | 10,224 | 7,520 | 6,944 | 10,256 |

These totals include retained directory/store capacity and descriptions, not only
live field bytes. Current loaded setup is 5,186,139 requested bytes for this complete compiled
workload plus standard declarations; it is not a universal empty-runtime estimate.
Point/Pair live increments fall approximately 73-74%; interface populations fall
85-91%. The directory entry is 24 bytes instead of 168 on this target. Value stays
16 bytes and external identities stay 12 bytes. Each Point has eight field bytes;
its total cost also includes the directory, arena record and amortized descriptions.

The initial per-object byte buffers added an allocation after constructor staging.
A packed arena removes that boundary while preserving exact field widths. Relative
to 7a8c85f4, constructing 512 Points now requests 1,712 allocations instead of 2,214;
Pair construction requests 1,740 instead of 2,754 (canonical description keys also
avoid argument-vector copies). Repeated field loops retain the baseline 98 requests,
6,928 requested bytes and zero net bytes per invocation, including entry/exit.
Empty/sparse capacity and teardown are checked independently of those loop costs.

Owner elision is deferred. Removing four owner bytes from each Node's one managed
struct field would save 2,048 bytes, about 0.49% of the reference-heavy live increment.
Native edge-vector elements are outside that proposed encoding change. Retaining
complete identities avoids an additional owner-capability/reconstruction protocol;
no compressed handle or unsafe speedup is claimed.

Final latency comparison uses seven process pairs, alternating baseline/current
and current/baseline order. Baseline afec4a3b and final 35077323 use the identical
frozen probe; both were built with the toolchain above. Builds reused the ordinary
Cargo cache and were excluded from timing. Each process compiles/prepares the fixed
script once, then runs warmed execution samples; lifecycle setup loads each fresh
runtime outside its construction/GC timers. These are warm-machine measurements,
not cold filesystem/startup guarantees. Earlier exploratory runs are not combined
with the final samples.

The table gives the median of seven process medians, in nanoseconds. Ranges are
minimum/maximum **process medians**, not confidence intervals or individual-sample
bounds. Every gate also passes using the median of seven paired current/baseline
ratios. Frozen limits are at least 10% lower time for each field loop, at most 5%
regression for scalar/traversal/numeric execution, and at most 10% for each
construction/GC workload. All 26 aggregate latency gates pass.

| Workload / phase | Baseline ns | Current ns | Change | Baseline process range | Current process range |
| --- | ---: | ---: | ---: | ---: | ---: |
| Scalar loop: execute | 1,582,417 | 1,633,750 | +3.2% | 1,566,000–1,603,750 | 1,601,750–1,661,125 |
| Field read: execute | 4,565,250 | 3,170,917 | -30.5% | 4,480,167–4,580,958 | 3,117,542–3,298,125 |
| Field write: execute | 3,759,084 | 3,173,625 | -15.6% | 3,728,417–3,798,666 | 3,134,708–3,337,333 |
| i32 field RMW: execute | 9,595,584 | 6,070,625 | -36.7% | 9,573,416–9,642,417 | 6,066,000–6,143,666 |
| f64 field RMW: execute | 9,381,709 | 6,085,041 | -35.1% | 9,331,667–9,455,375 | 6,015,708–6,192,542 |
| Point traversal: execute | 216,083 | 184,167 | -14.8% | 214,583–222,625 | 183,209–188,250 |
| Pair traversal: execute | 219,208 | 187,542 | -14.4% | 217,958–225,583 | 186,000–191,208 |
| Numeric-buffer traversal: execute | 242,833 | 249,625 | +2.8% | 241,750–261,375 | 246,750–251,708 |
| Point: construct | 523,792 | 559,250 | +6.8% | 509,500–534,416 | 547,292–581,333 |
| Point: live GC | 106,417 | 103,250 | -3.0% | 101,417–107,458 | 99,250–106,291 |
| Point: release GC | 20,667 | 21,833 | +5.6% | 19,542–21,125 | 20,542–22,708 |
| Cyclic nodes: construct | 1,542,125 | 1,638,708 | +6.3% | 1,518,250–1,583,542 | 1,610,666–1,665,541 |
| Cyclic nodes: live GC | 219,375 | 219,625 | +0.1% | 210,667–224,875 | 217,667–225,834 |
| Cyclic nodes: release GC | 69,917 | 76,500 | +9.4% | 69,000–73,166 | 75,375–77,667 |
| Narrow interfaces: construct | 1,833,000 | 1,155,042 | -37.0% | 1,796,458–1,865,458 | 1,124,083–1,182,583 |
| Narrow interfaces: live GC | 454,042 | 302,333 | -33.4% | 441,334–464,458 | 298,250–309,167 |
| Narrow interfaces: release GC | 76,667 | 41,875 | -45.4% | 73,000–77,167 | 39,583–43,833 |
| Wide interfaces: construct | 2,686,209 | 1,158,458 | -56.9% | 2,623,834–2,832,291 | 1,136,458–1,181,750 |
| Wide interfaces: live GC | 466,083 | 305,666 | -34.4% | 445,167–488,000 | 295,959–314,916 |
| Wide interfaces: release GC | 108,458 | 42,500 | -60.8% | 104,583–117,583 | 39,583–44,292 |
| Generic interfaces: construct | 3,504,375 | 2,462,291 | -29.7% | 3,455,334–3,606,875 | 2,434,667–2,564,625 |
| Generic interfaces: live GC | 586,500 | 307,958 | -47.5% | 564,292–601,208 | 300,041–313,209 |
| Generic interfaces: release GC | 161,667 | 42,417 | -73.8% | 157,458–162,416 | 41,125–45,209 |
| Numeric buffer: construct | 322,625 | 318,292 | -1.3% | 317,333–339,791 | 310,583–327,084 |
| Numeric buffer: live GC | 6,375 | 5,959 | -6.5% | 5,708–6,833 | 5,625–6,792 |
| Numeric buffer: release GC | 3,958 | 3,875 | -2.1% | 3,584–4,042 | 3,667–4,833 |

Current field read/write/i32 RMW/f64 RMW cost is respectively 63.4, 63.5, 121.4
and 121.7 ns per loop iteration (about 15.8, 15.8, 8.24 and 8.22 million iterations/s).
An RMW iteration includes both field access and arithmetic; these are complete
benchmark iterations, not isolated instruction latency. Node release (+9.4%) is
close to the 10% gate. Numeric release is particularly noisy: its aggregate ratio
is -2.1%, while the paired-ratio median is +4.6%. Individual runs can exceed a
threshold; this result establishes the fixed aggregate local gate, not a guarantee
for every run or machine. Scalar (+3.2%) and numeric execution (+2.8%) are regression
controls, not evidence of faster general arithmetic.

Compilation/preparation/linking median times are 3.352 s / 1.744 s / 9.024 ms
baseline and 3.372 s / 1.749 s / 9.052 ms current, excluded from execution results.
The representation changes preserve all memory gates: Point/Pair live and empty
increments fall more than 50%; each interface population falls more than 40%;
node live storage does not grow; numeric retained storage stays within +5%; sparse
release/reuse and all sixteen teardowns remain bounded. Warm field-loop requests
do not grow. Nonempty sweep allocates one bounded list of changed payload kinds
(at most twelve), replacing transfer/disposal of empty typed stores; that temporary
allocation is included in lifecycle accounting and retains no bytes.

Focused local correctness covers GC ownership/generation reuse/relocation, every
scalar field domain and float bits, rejected initializer accounting, managed-field
nominal provenance, overflow side effects, pinned generic reload, source-free
nominal/reference rejection, scoped Rust loans, interface sharing/dispatch/retirement,
native extraction and trace/destructor panic cleanup. Final changed-owner reruns
pass 21 runtime and three native cleanup cases; earlier selected VM/host/source-free
results remain evidence for unchanged paths. Structure, formatting, workspace
all-target Clippy and diff checks pass. The exhaustive workspace and feature/backend/
platform test matrix remains CI-only and was not run locally. The
[architecture](architecture.md#runtime-model) and [runtime contract](spec/runtime.md)
own lasting storage behavior; the completed RS01-RS06 ledger remains in Git history.

## Focused preparation recheck, 2026-10-11

After the user reported lower CPU load, nine selected scenarios ran twice: 18
successful executions, with no workspace sweep. Code is 294dce32 plus the working
TP01-TP03 implementation. The machine/toolchain are Windows x86_64 MSVC,
i9-12900K (16 cores / 24 logical CPUs), 63.7 GiB RAM, rustc 1.99.0 (b940084d7,
LLVM 23.1.1) and Cargo 1.99.0. Default `source,native` features, workspace test
profile (opt-level=1, debug info), target directory and build/test concurrency
were retained. Each group ran in a fresh process; groups/probes ran sequentially.

The selected incremental build took 23.28 s (Cargo reported 23.15 s), rebuilding
affected libraries and test binaries against existing artifacts. This is separate
from the following harness times and is not a cold-build measurement.

| Selected group (3 tests each) | Run 1 | Run 2 | Peak working set across runs |
| --- | ---: | ---: | ---: |
| Default methods | 23.09 s | 22.97 s | 410.9-412.5 MiB |
| Syntax examples | 17.14 s | 17.13 s | 413.8-414.3 MiB |
| Language contracts | 23.08 s | 23.53 s | 402.0-403.2 MiB |

Selections, passed together as libtest filters with `--exact`:

- `default_methods`: `default_body_calls_the_implementation_override_through_static_and_dynamic_dispatch`,
  `explicit_override_takes_precedence_over_the_default_body`,
  `generic_implementations_and_default_local_binders_are_specialized`.
- `syntax_examples`: `array_operations`, `collection_pipelines`, `callables`.
- Embed library: `runtime::language_contract::evaluation::scalar`,
  `runtime::language_contract::evaluation::host_reject`,
  `runtime::language_contract::types_and_dispatch::publication_pins_old_dependencies_and_rejects_stale_candidate`.

For example, build the three targets with `cargo test -p kagari --lib
--test default_methods --test syntax_examples --no-run`. Reproduce the syntax
selection with `cargo test -p kagari --test syntax_examples -- array_operations
collection_pipelines callables --exact`. The other targets accept their listed
filters in the same way. Report Cargo compilation separately from harness time.

Windows GetSystemTimes/GetProcessTimes indicated approximately 14.7-17.8% of
total logical CPU capacity was still used outside each test process. Thus this
is a reduced-load sample, not a dedicated idle-machine benchmark. Two similar
runs establish useful current timings, not statistical confidence or a whole-suite
speedup. The earlier interrupted 866.93 s aggregate is not a comparison baseline.

A retained pre-change probe linked against 294dce32 and a freshly linked current
probe were also run sequentially in this session. Both use `fn main() -> i32 { 42 }`,
three fresh engines followed by three compilations in one retained engine. Each
case compiles, encodes/decodes, prepares, creates a runtime, loads and executes,
with default options. The current probe additionally creates three fresh sessions
from the retained engine. The drivers use rustc's default optimization while
linking the workspace test-profile libraries. Values below are medians of the
three fresh-engine cases, except the explicitly separate session row.

| Stage | Before | Current |
| --- | ---: | ---: |
| Construct full engine | 692.255 ms | 720.409 ms |
| Compile source to artifact | 4.470 s | 4.580 s |
| Prepare decoded program | 2.726 s | 2.822 s |
| Create runtime | 222.275 ms | 0.016 ms |
| Load prepared program | 9.370 ms | 9.576 ms |
| Execute | 0.020 ms | 0.020 ms |
| New source session from prepared engine (current-only, 3 samples) | N/A | 0.852 ms |

The runtime-creation reduction is consistent with reusing validated installation.
Full engine construction still performs preparation; creating a session amortizes
that work but does not clone callback captures. Compilation and program preparation
remain the main costs. Later current samples varied to 5.775 s for compilation and
3.431 s for preparation; these sequential, background-loaded probes do not isolate
small regressions or prove those stages improved. The first full engine in each
process also pays cold standard registration initialization (1.94 s before,
1.74 s current), which is amortized by subsequent samples.

Probe executable SHA-256: baseline
`810e2c58711dfa8992c0c760b5236c552ef932a7fd49052c694fd03029b44da9`;
current `7cdf8065a27727152b0cbcfc958de10fcb740943d90067389fb6f758424cb536`.
Raw logs/binaries remain disposable local measurements. Current probe peak memory
cannot be compared directly with the baseline because it runs additional sessions.
Matched suite timing and retained-identity growth remain open in the
[test preparation plan](test-preparation-plan.md); these samples do not establish
a memory reduction.

## Workspace test-cost audit, 2026-10-11

The user explicitly requested one complete workspace audit after the targeted
sample below. This is a one-off diagnostic, not a change to the CI-only policy
for routine full-suite validation. No tests, assertions, production code or Cargo
profiles were changed, and no failed/final sweep was repeated.

Command: `cargo test --workspace --no-fail-fast --timings --color never`.
Source baseline: `35de1818`; working changes were documentation and CI policy.
Environment matches the sample below: Windows x86_64 MSVC, i9-12900K (16 cores /
24 logical CPUs), 63.7 GiB RAM, rustc 1.99.0 (b940084d7, LLVM 23.1.1), Cargo 1.99.0.
Workspace test profile uses opt-level=1 and debug info, default target directory,
default build/test parallelism and existing incremental artifacts. This workspace
selection rebuilt several targets; it is neither a cold build nor an unchanged
warm rerun. No other agent build/test/probe ran concurrently. The earlier sample
used serial tests; its wall times must not be mixed into this ranking.

| Measurement | Result |
| --- | --- |
| Complete command wall time | 2,210.66 s (36 min 50.66 s) |
| Start through Cargo's build-finished message | 179.50 s (2 min 59.50 s) |
| Remaining wall time | 2,031.17 s (33 min 51.17 s) |
| Sum of harness-reported test execution times | 2,009.15 s |
| Executed scope | 18 workspace members, 97 test executables and 16 doctest groups |
| Results | 1,952 passed, 0 failed, 2 ignored; Cargo exit 0 |

The gap between remaining wall time and harness totals includes process startup
and rustdoc preparation outside reported execution intervals. Cargo executes test
targets sequentially while each harness uses its default parallelism. Timings
come from one run; they identify expensive targets, not statistically established
performance changes. This covers the default workspace selection, not all feature
combinations, ignored manual measurements or the full CI matrix.

The audit covered all 113 targets; the slowest targets and package totals are
recorded below. Target wall intervals were measured from each Cargo announcement
to the next announcement (or command exit); doctest preparation can make them
much larger than harness time. Raw measurements are not part of the repository.

| Slowest targets | Harness time | Passed |
| --- | --- | --- |
| kagari-embed / lib: language_contract_routes_preserve_values_diagnostics_and_effects | 366.66 s | 1 |
| kagari-hir / lib | 204.36 s | 429, plus 1 ignored |
| kagari-embed / syntax_examples | 171.92 s | 9 |
| kagari-vm / native_boundary | 168.71 s | 160 |
| kagari-vm / lib | 94.47 s | 118 |
| kagari-compiler / lib | 86.83 s | 174 |
| kagari-embed / native_provider_reset | 56.30 s | 18 |
| kagari-stdlib / documentation | 41.24 s | 1 |
| kagari-embed / offline_nominal | 40.93 s | 12 |
| kagari-embed / source_modules | 38.62 s | 30 |
| kagari-lua-benchmark / binary unit tests | 37.53 s | 1 |
| kagari-embed / default_methods | 35.82 s | 32 |
| kagari-embed / standard_traits | 35.76 s | 20 |
| kagari-vm / prepared_execution | 33.45 s | 7 |
| kagari-embed / instantiation | 31.21 s | 25 |

The first six targets account for 54.4% of summed harness time. By package,
kagari-embed accounts for 1,270.15 s (63.2%), kagari-vm 320.15 s (15.9%),
kagari-hir 206.62 s (10.3%) and kagari-compiler 92.56 s (4.6%). In contrast,
runtime's 156 library tests take 1.04 s, syntax's 81 library tests 0.02 s and
bytecode's 32 library tests 0.12 s. Test counts alone do not explain elapsed time.

### Concrete follow-up candidates

1. [SDK language contract](../crates/kagari/src/runtime/language_contract.rs):
   one test contains a large serial collection of scenarios (146 Case::new source
   sites, not a claimed executed-case count). Successful scenarios traverse Source,
   Artifact, Jit and ArtifactJit routes. It already shares compilation/prepared
   inputs across routes; do not propose removing that duplication as if it still
   existed. Review distinct route coverage, prepared declaration reuse and
   individually filterable cases while retaining fresh mutable runtime state.
2. [Syntax examples](../crates/kagari/src/runtime/tests/language/syntax_examples.rs): 45 examples
   inside one test each create an engine and compile a program, then execute
   direct/serialized artifacts. Default test parallelism cannot parallelize this
   inner loop. Preserve example validity and artifact coverage while reviewing
   repeated immutable setup and selective execution.
3. [Old ABI rejection](../crates/kagari-compiler/src/tests/bytecode/artifacts.rs):
   rejects_previous_runtime_abis_even_when_loader_requests_them loops over 28 old
   versions and recompiles the same `fn main() -> i32 { 1 }` in each iteration.
   The full-run log reports this individual case exceeding 60 s. Prepare the
   invariant program once when fixing this path; independently preserve each
   metadata mutation/rejection assertion. No exact standalone case time was
   measured during this parallel run.
4. [Provider cancellation](../crates/kagari/src/runtime/tests/embedding/native_provider_reset/mod.rs):
   cancellation_boundaries_unwind_nested_provider_callbacks constructs an engine
   and runtime for each cancellation point, stopping after normal completion or
   the 200-point bound. The program is already prepared outside the loop. Audit
   engine/declaration reuse independently of necessary per-attempt runtime state.
5. [Stdlib documentation](../crates/kagari-stdlib/src/tests/documentation.rs): one test
   deduplicates documentation examples and analyzes them sequentially. Review
   reuse of checked provider inputs without dropping documentation coverage.

The HIR and VM aggregate targets also require targeted attribution before changing
their tests. Libtest reported some cases running over 60 s, including HIR completion
and generic-recovery matrices, import graph cancellation, VM native cancellation
and retained selected calls. These are wall-time warnings under concurrent tests,
not a precise per-case CPU ranking. Do not remove boundary coverage merely because
its name appears in a slow target. The benchmark crate's 37.53 s is an ordinary
test over empty/single-element workload inputs, not a `cargo bench` run.

Cargo build timings separately identify the HIR library test build at 178.80 s,
compiler library tests at 127.50 s, bytecode library tests at 126.72 s, VM library
tests at 101.40 s and runtime library tests at 78.49 s. These build units overlap
and their times cannot be summed into wall time. Bytecode's expensive test build
and 0.12 s execution particularly illustrate why build/execution need separation.

Measurement method: timestamp Cargo's merged output for the command above,
separate the build-finished boundary, and record each harness summary. Cargo's
--timings flag generates build timing data on the machine running the command.
The temporary wrapper only captured output and timestamps, preserving default
parallelism. Its source and raw logs are not distributed; the tables preserve
the observed results, not a portable raw-data archive. No complete individual-test
timing is claimed. Use targeted follow-up measurements for proposed changes;
routine full sweeps remain with CI.

## Targeted test-cost audit, 2026-10-11

This is a diagnostic sample, not a workspace timing or a global ranking. No full
suite was run. Baseline: `35de1818`, with documentation/CI-policy edits only.
Machine: Windows x86_64 MSVC, Intel Core i9-12900K (16 cores / 24 logical CPUs),
63.7 GiB RAM. Toolchain: rustc 1.99.0 (b940084d7, LLVM 23.1.1). Existing default
target cache; workspace test profile inherits dev opt-level=1 with debug info;
default package features and Cargo build parallelism. No environment overrides
for build jobs, target directory, test threads, Rust flags or profile optimization
were present. No other agent build/test/benchmark ran concurrently with sampling.

Five integration targets were selected from source inspection: example batches,
collection execution matrices and allocation loops. Build them separately from
execution to distinguish Rust build time from work performed inside the tests:

```text
cargo test -p kagari --test syntax_examples --test collection_interfaces --no-run --message-format=json
cargo test -p kagari-vm --test interface_allocations --test library_collections --test native_allocations --no-run --message-format=json
```

The two incremental build commands took 7.78 s and 9.28 s wall time, respectively.
These are warm-cache observations, not cold-build estimates. Each emitted test
executable was then run once, from its crate directory, with `--test-threads=1`.
Suite time includes process startup/exit; individual times are approximate deltas
between completed libtest output lines, with startup included in the first case.
Serial execution attributes costs but does not predict normal parallel test wall
time. Do not sum these results into an estimate for the whole repository.

| Target | Passing cases | Serial execution wall time |
| --- | --- | --- |
| kagari-embed / syntax_examples | 9 | 159.96 s |
| kagari-vm / library_collections | 18 | 43.95 s |
| kagari-embed / collection_interfaces | 5 | 25.13 s |
| kagari-vm / interface_allocations | 3 | 7.46 s |
| kagari-vm / native_allocations | 2 | 3.88 s |

All 37 selected cases passed. Reproduce individual hotspots with package/target
and exact test-name filters; do not turn this diagnostic inventory into a routine
acceptance checklist. Raw logs and the temporary measurement helper are not
distributed; the method and measured results are recorded here.

The largest measured case was
`standalone_language_examples_execute_from_source_and_artifact` at 141.36 s,
approximately 88% of its target's serial time. The
[example loop](../crates/kagari/src/runtime/tests/language/syntax_examples.rs) runs 45 sources
inside one test, constructing a fresh engine for each and executing both direct
and serialized artifacts. This inner loop stays serial even when libtest runs
different tests concurrently. Other measured examples:

- `list_failures::active_iteration_rejects_list_edits_and_releases_the_guard_on_failure`:
  6.76 s in library_collections.
- `hash_collection_interfaces_use_custom_key_protocols`: 6.73 s.
- `user_containers_implement_the_same_storage_independent_contracts`: 6.04 s.
- `callback_alias_writes_and_nested_edits_are_rejected_without_changing_slots`:
  5.52 s in library_collections.
- `inherited_closed_dispatch_does_not_allocate_parent_wrappers_or_copy_unused_methods`:
  4.16 s in interface_allocations.
- `warmed_scalar_and_contiguous_i32_boundaries_add_no_allocations`: 2.40 s,
  including its 100,000 scalar calls and three 1,000-call bulk cases. Large loop
  counts alone did not identify the largest elapsed-time costs in this sample.

A separate disposable Rust probe linked the exact embed/source rlibs from the
first build, using rustc opt-level=1 and the existing dependency directory. Probe
build/link time was 2.41 s, excluded from the following measurements. Its input
was `fn main() -> i32 { 42 }`; each operation was timed with Instant. Three fresh
engines and then three uniquely named sources on one reused engine were measured
sequentially, without forced GC or JIT. Fresh-engine ranges:

| Operation | Wall-time range across three samples |
| --- | --- |
| KagariEngine::default | 0.700-1.401 s |
| compile_to_artifact | 1.199-1.437 s |
| artifact serialize + decode | 0.029-0.040 s |
| PreparedProgram::from_artifact | 0.402-0.511 s |
| engine.runtime | 0.230-0.259 s |
| runtime.load_program | 0.005-0.007 s |
| runtime.execute | 0.016-0.106 ms |

The temporary probe source and raw samples are not distributed. Its procedure
calls the listed APIs in order, drops the returned execution result,
then drops loaded module/runtime/program and engine with separate timing. This
is a scale diagnostic, not a precise interpreter throughput benchmark. Reusing
the engine removes another engine construction, but compile_to_artifact still
took 1.416-1.473 s on the three reused-engine inputs: engine reuse alone does not
remove the remaining compilation/preparation cost.

Source inspection explains repeated work, but does not yet attribute the time
inside those stages. [Engine declaration preparation](../crates/kagari/src/engine/declarations.rs)
renders/prepares/checks installed declarations; each fresh engine enters that
path. [Collection interface tests](../crates/kagari/src/runtime/tests/language/collection_interfaces.rs)
exercise direct, serialized and JIT-enabled routes with GC threshold 1. Similar
helpers exist in default_methods, type_inference and trait_inheritance; these
other targets were not timed and are candidates, not measured hotspots.

The next optimization review should separate ordinary semantic coverage from
the smaller set proving serialization/backend/GC boundaries, investigate repeated
immutable declaration and artifact preparation, and review the 45-case example
batch's isolation requirements. Preserve distinct source-free/backend/ownership
coverage and avoid sharing mutable engines globally or skipping validation merely
to shorten tests. No tests, runtime code, assertions or profiles were changed by
this audit. Complete test acceptance remains with CI.

## Execution architecture evaluation (HP06), 2026-10-10

HP00–HP06 implementation and final local correctness checks are complete. All 16
unchanged matched workloads improve against HP00, but **none reaches Lua parity**:
current interpreter medians are 2.57–51.11 times Lua. This closes the bounded
migration/evaluation, not the performance objective or complete GitHub CI acceptance.
Implemented ownership and execution boundaries are described in
[architecture](architecture.md); future activation belongs to the roadmap.

Runtime-owned executable identities and checked publication now own prepared facts;
active execution regions borrow admitted function/link views and disjoint PC/window
state. Internal calls/returns share one protocol, managed/scalar operations share
prepared execution, and canonical layout admission plus borrowed enum reads replace
the separate VE09 shortcut and snapshot consumers. External entry, real dynamic
boundaries, roots, bounds, generation checks, cancellation and observable failure order
remain. Value stays 16-byte Copy; ordinary Option still allocates traced enum objects.

### Frozen paired throughput

Baseline: HP00/VE09 f97b4095, SHA-256
`a10ea34693c9a113b97cf2f1e2af1906c07a2b175e1bfcd209bb055c01cecc22`.
Candidate: eca16497 (production code 375c727d;
eca16497 changes only an opt-in test and documentation), SHA-256
`5161d2859bb0e693960c7e7fa0b1b6e288f182eafc52ee6f804571494b9d3d33`.
The ordinary candidate binary is identical to the accepted HP04 region-view binary.

Apple M1 Max, 32 GiB, 10 logical CPUs, macOS 26.6.2 arm64; rustc 1.98.1
(48a229cea, LLVM 22.1.8), Cargo 1.98.1, vendored PUC Lua 5.4.8 through mlua.
Workspace release/opt-level=3, default target and Cargo parallelism; SDK source/native
features, only interpretation executed. No allocation counters, observer, debugger
or profiler runs in throughput. Normal GC/allocator and public host entry/return are
included; compilation, verification, loading and setup are excluded. Build cache
was warm; the two excluded build times were 0.094/0.134 seconds.

Both matrices run serial baseline/candidate/candidate/baseline, three warmups and
eleven samples per process/engine/workload (22 pooled). Workload and initial engine
order reverse in the second pair. All 1,672 measured batches pass checksums. Fixture
source/Lua bodies, sizes, expected results and ordinary loops are unchanged from HP00.
No agent-started build/test/profile or Rust edit overlaps timing. CPU placement,
frequency and desktop activity remain uncontrolled. No best samples are selected.

Microseconds per complete workload; C/B is candidate/HP00 elapsed time, so lower is
better. Ranges are sample extrema, not confidence intervals.

| Matched workload | HP00 VM | HP06 VM | C/B | Candidate Lua | VM/Lua | HP06 VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 2491.125 | 1711.479 | 0.687 | 387.584 | 4.42 | 1669.292–1754.792 |
| arrays | 5021.667 | 1403.771 | 0.280 | 67.292 | 20.86 | 1395.875–1472.250 |
| branches | 2953.709 | 2158.021 | 0.731 | 838.812 | 2.57 | 2096.459–7976.209 |
| calls | 6229.958 | 4980.124 | 0.799 | 222.417 | 22.39 | 4933.250–5085.416 |
| fibonacci | 12528.646 | 10268.604 | 0.820 | 353.416 | 29.06 | 10151.000–10640.209 |
| maps | 5991.374 | 3344.417 | 0.558 | 71.916 | 46.50 | 3227.041–11025.333 |
| byte_state | 6941.542 | 1341.688 | 0.193 | 109.250 | 12.28 | 1331.667–1394.917 |
| capture_cell | 10118.166 | 5943.458 | 0.587 | 138.292 | 42.98 | 5890.750–6086.166 |
| concrete_generic | 3073.312 | 2525.312 | 0.822 | 103.833 | 24.32 | 2497.958–2576.375 |
| direct | 348.708 | 239.792 | 0.688 | 75.312 | 3.18 | 233.833–253.041 |
| field | 3718.396 | 1017.646 | 0.274 | 90.166 | 11.29 | 1009.375–1088.208 |
| helper | 3056.749 | 2456.876 | 0.804 | 139.833 | 17.57 | 2431.625–2607.208 |
| interface | 10379.437 | 4334.583 | 0.418 | 134.480 | 32.23 | 4264.042–4394.583 |
| shared_generic | 21593.500 | 5460.146 | 0.253 | 106.834 | 51.11 | 5417.625–5576.791 |
| string_calls | 12899.812 | 3057.000 | 0.237 | 125.604 | 24.34 | 3018.125–3232.375 |
| string_constants | 7059.167 | 671.375 | 0.095 | 66.396 | 10.11 | 667.834–705.083 |

Branch and Map outliers remain in the table (7,976.209 and 11,025.333 us).
Their two candidate-process medians are 2,339.000/2,120.250 and 3,407.042/3,297.000 us;
HP00's are 2,940.208/2,954.875 and 5,974.875/6,007.375 us. Both process orders support
lower elapsed time and both remain far above Lua. Candidate/baseline Lua controls
range 0.978–1.042 in the original suite and 0.985–1.016 in the ten matched source
forms. No >5% control regression against HP00 is observed; uncertainty near parity
is not relevant because every row is far above 1.0.

Entry and adapters stay outside the 16-workload parity gate. Entry is normalized
per public call; native compares a Rust body with a script helper and is unmatched;
host_callback measures the same Rust body through each engine's host adapter.

| Diagnostic | HP00 VM | HP06 VM | C/B | Candidate Lua | VM/Lua | HP06 VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| entry | 1.430 | 1.336 | 0.934 | 0.028 | 47.00 | 1.328–1.423 |
| host_callback | 1913.396 | 1429.146 | 0.747 | 249.750 | 5.72 | 1416.625–1530.666 |
| native | 1896.417 | 1428.333 | 0.753 | 141.062 | 10.13 | 1418.125–1456.917 |

### Allocation, preparation and transitions

Separate diagnostic builds count one warmed script execution after three warmups.
The 86 final rows at production 375c727d are reused: eca16497 changes no production
code. Common HP00 rows are compared below. Original workloads each have their own
runtime in both versions; HP00 source forms shared one runtime, while final probes
use freshly linked runtimes per entry. Allocation traffic and protocol counts remain
useful, but GC/live/net deltas across the source forms are not equal-context retained
memory comparisons. Final cold/warm/changing-key probes stay separate from throughput.

| Workload | Rust allocation requests, HP00 -> HP06 | Requested bytes, HP00 -> HP06 | Metadata graph entries, HP00 -> HP06 | Region exits to ordinary dispatch, HP00 -> HP06 |
| --- | ---: | ---: | ---: | ---: |
| arrays | 12059 -> 59 | 180250 -> 20282 | 0 -> 0 | 16003 -> 2002 |
| maps | 30207 -> 2178 | 1313040 -> 1022792 | 0 -> 0 | 12003 -> 8002 |
| interface | 70040 -> 5039 | 3923832 -> 43832 | 5001 -> 1 | 10002 -> 5001 |
| shared_generic | 315599 -> 5050 | 26257537 -> 44785 | 20002 -> 2 | 20002 -> 5001 |
| field | 5009 -> 8 | 40231 -> 223 | 0 -> 0 | 10002 -> 1 |
| byte_state | 36 -> 38 | 2888 -> 3112 | 0 -> 0 | 25002 -> 1 |
| string_constants | 7 -> 7 | 170 -> 170 | 0 -> 0 | 25000 -> 0 |
| string_calls | 7 -> 7 | 166 -> 166 | 0 -> 0 | 45000 -> 5000 |

Shared-generic per-execution method preparation/environment allocation counts fall
5,000/10,000 -> 0/0. Alternating receiver/type probes keep distinct checked identities:
at 5,000 iterations their requests fall 315,726 -> 5,093 and graph entries 20,004 -> 4.
Their warm preparation does not scale with iterations. There is still roughly one
8-byte allocation per interface/shared invocation; zero preparation is not zero
call cost. Byte-state requests increase slightly; it is not an allocation win.

Scalar/direct/helper/concrete-generic/calls/fibonacci still request seven allocations
per complete warmed execution; no per-recursive-call Rust allocation is introduced.
Arrays still create one script object with two collections, Map 2,001 with five,
and both string cases zero objects/collections. Region exits are neither host
crossings nor opcode counts. Metadata counters exclude cheap Program ownership
validation and GC traversal; zero does not imply checks were removed.

### Preparation and retained memory

Preparation is intentionally visible. Across original fixtures, median program-link
time moves from 2.348–2.508 ms to 2.817–3.134 ms (six setup samples per workload/version).
For the source-form module (two samples/version), source-to-artifact is
1,141.288 -> 1,159.579 ms, artifact preparation 263.630 -> 268.355 ms and linking
2.779 -> 3.355 ms. Candidate runtime creation is 111.375 ms; the frozen HP00 forms
binary did not report that phase, so no comparative value is fabricated. This is
an execution improvement with preparation/storage costs, not a startup speed claim.

Release layout measurements: prepared instruction 24 bytes, ExecutionFrame 264,
LinkedFunction 56, ExecutionFunction 128, field/index operand 40/28, native primitive
slot 40, region exit/result 24/32. FunctionLayouts is 40, AppliedLayouts 16 (40 in
debug due to its extra environment check), AggregateLayout 56 and its lazy cell 64.
Linked constant cells are 24 bytes versus the previous 16-byte Option<Value>.
These are shallow type sizes, not total transitive metadata or bytes per instruction;
variable tables, shared Arc slices and lazily prepared descriptors add storage.

Isolated release probes compare identical runtime/input roots without versus with
preparation, with no script execution/VM frames. They cover cold preparation, two
warm repeats, 30 further repeats with GC, compatible reload/old-version retirement
and runtime teardown. Optional indices retain at most 128 keys; the 160-key case
also grows tuple widths from 1 to 160, so it is neither fixed-cost entries nor an
allocation-free steady state. Capacity can grow at eviction and remain after records
retire. Values below are extra **net allocated bytes after GC**, not RSS or peak.

| Prepared owner | Cold: 1 / 4 / 160 keys | After 30 repeats: 160 keys | After retirement: 1 / 4 / 160 keys |
| --- | ---: | ---: | ---: |
| native application | 1512 / 5824 / 1730312 | 1730312 | 0 / 0 / 0 |
| method application | 3408 / 7340 / 2633956 | 2699236 | 1376 / 1760 / 176128 |
| shared environment | 1464 / 4576 / 165896 | 167688 | 0 / 384 / 24576 |
| witness scope | 3200 / 13732 / 467856 | 496272 | 0 / 448 / 56896 |
| struct layout | 3185 / 11080 / 3851472 | 3868112 | 0 / 0 / 0 |
| enum layout | 3616 / 11488 / 3856600 | 3873240 | 0 / 0 / 0 |
| cross-version admission (two producers/key) | 900 / 1388 / 21148 | 26396 | 0 / 0 / 0 |

Setup deltas are zero; closed native preparation adds zero in this fixture. Every
control and prepared runtime returns to zero net allocation on teardown. Retirement
assertions verify old executable records/environments are reclaimed; a replacement
program's valid closed witness and reusable store capacity must not be mistaken for
old-version leaks. Small capacity differences may depend on hash/eviction order.
HP00 has no equivalent isolated owner probe, so these are final design costs, not
an invented HP00 memory speedup or retained-byte delta.

### Remaining architecture costs and bounded follow-up

Five ordinary-binary profiles use `/usr/bin/sample PID 5 1` within a ten-second
warmed execution window: three warmups for original fixtures, six for source forms.
The same candidate hash and workspace release settings apply; no allocation counters,
observer or Lua hook is active in that window. Logical instruction counting runs
separately afterward. All profile-window/counting checksums pass. The table
retains the observations; raw sampling reports are not distributed.

| Workload | Main-thread samples | Kagari / Lua logical instructions per execution | Observed remaining cost |
| --- | ---: | ---: | --- |
| shared_generic | 3657 | 95017 / 60009 | `push_prepared_call` 63.36% inclusive, return 11.18%; SipHash 9.76% and ModuleKey hashing 3.83% leaf |
| maps | 3652 | 63030 / 24012 | native invocation 53.72% inclusive; frame validation 9.53%; identified enum allocation/GC stacks 8.08% |
| fibonacci | 3703 | 240801 / 120400 | prepared call entry 36.94% inclusive, return 10.40%, frame validation 12.18%; value-window allocation 9.13% leaf |
| arrays | 3615 | 72030 / 42012 | native invocation 45.48% inclusive; scalar kernel 15.35% leaf; typed storage and access checks remain |
| string_constants | 3670 | 100012 / 52506 | scalar kernel 47.87% and region kernel 13.27% leaf; no objects or collections |

These are wall-clock stack samples, not exact CPU fractions. Inclusive families count
a sample once per family and include callees; families overlap and must not be added.
Leaf values use the sampler's collapsed-symbol list (entries >=5). Optimized inline
attribution is incomplete. The Map enum-allocation/GC family covers named
Runtime/GcHeap `alloc_enum` and collector stacks, not every representation-related
instruction. None of these percentages is a predicted speedup or an unboxing bound.

For example, the shared identity body merely returns its argument, but 5,000 calls
still select an applied invocation, check current dependencies/arguments, establish
frame slots, return and retire slots. Reused descriptors remove preparation, while
`frame/calls.rs`, `objects/invocation.rs` and frame admission still perform runtime
lookup/validation at those transitions. Fibonacci allocates no script objects and
only seven Rust allocations per complete execution: `ExecutionValues::allocate`
primarily means initializing/reusing window storage, not one heap allocation per
recursive call. Thus shrinking Value or changing GC alone cannot explain these costs.

For Map, each successful `get` still produces an ordinary Option followed by tag and
payload operations. The frozen workload executes 4,002 calls, 2,000 tag tests and
2,000 payload reads, creating 2,001 objects and five collections. Native invocation
also includes argument/result validation, installed storage authority, hashing and
mutation guards. Array indexing now uses prepared operands, but 2,000 growing
`push` operations still use native transitions; growth, type checks and real access
checks remain. Neither finding justifies deleting dynamic checks or calling the
remaining cost entirely allocation.

The next **proposed, unactivated** scope is one finite call-lifecycle migration:
review prepared invocation contracts and active-session frame ownership across
selection, argument admission, slot initialization and return. For the same identity
call, carry checked executable/type facts into a bounded internal transition rather
than rediscovering them through general module/type lookups, while validating real
receiver/version changes and retaining host/reentry boundaries. Avoid another
callsite cache, duplicated fast-call implementation or unchecked public API. Accept
only with changing receiver/type/version, eviction, source-free, GC/trap/cancel/reentry
contracts, measured call-window/lookup counts and paired shared/interface/capture/
fibonacci controls, including setup and retained memory.

After that bounded result, reconsider native collection contracts separately. Any
prepared collection operation must use exact installed binding/effect authority and
one storage implementation, preserving alias/growth guards and result publication;
ordinary Option representation should be changed only with evidence isolating its
material cost. A distinct future kernel/bytecode review can address excess logical
loads/stores/branches and general operand checks. It must retain observable trap
order, logical stepping and cancellation. These are remaining architectural questions,
not silently activated phases or proof that completing HP06 makes the design sufficient
for Lua parity.

### Correctness and reproduction

The measured revision passed its recorded local behavioral, structural and
release lifetime checks. Complete CI/feature/backend acceptance was unverified;
local success does not establish it. The implementation adds no JIT suspension,
collector replacement or general enum unboxing.

Shared generic Add failed MIR lowering with
`MissingBinding("checked callable requirement")` at the measured revision. Save
the following source and use `cargo run -p kagari-cli -- run <file>` to check it
on a selected revision. The shared-identity benchmark does not cover this capability.

```kagari
use core::ops::Add;
trait Forward {
    fn forward<T: Add<T>>(self, a: T, b: T) -> T::Output { a + b }
}
impl Forward for i32 {}
fn main() -> i32 { val receiver: Forward = 0; receiver.forward(1, 2) }
```

The tracked [benchmark driver and instructions](../benchmarks/lua-comparison/README.md)
provide current throughput, paired-baseline and diagnostic commands. For a
historical comparison, build both recorded revisions with the same toolchain,
features and fixture inputs; supply the resulting baseline binary explicitly.
Prebuilt binaries, logs and one-off probe sources from the original machine are
not distributed. Exact historical raw samples therefore cannot be recovered from
this document; the tables retain the measured results and their limits.

Reproduction commands for current measurements:

```text
uv run python scripts/benchmark_lua.py --interpreter-only
uv run python scripts/benchmark_lua.py --source-forms
uv run python scripts/profile_lua_macos.py arrays maps fibonacci
uv run python scripts/profile_lua_macos.py --source-forms shared_generic string_constants
cargo test --release -p kagari-runtime --lib --features execution-diagnostics diagnostics::memory -- --ignored --nocapture --test-threads=1
```

The sampling commands require macOS; use a suitable platform profiler elsewhere.
Keep allocation counters and profiling separate from ordinary throughput runs.
These are targeted measurement commands, not a full-suite validation requirement.

## Architecture review measurements, 2026-10-03

These historical measurements cover committed baseline
`9581f5ad2a754911d1edb92eb35e957a18d7b5df`. They preserve the evidence behind
[architecture review follow-ups](review.md#architecture-findings),
not current startup, allocation or interface-dispatch costs. Later work changed
interface descriptors, registration sharing and verification reuse; consult the
[current architecture](architecture.md) and newer measurements above before
selecting an optimization. The consolidation's source audit is recorded in the
review; no new measurement was performed and these numbers were not refreshed.

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
The foundation module carries 53 imports at this baseline. All are linked;
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

### Architecture measurement reproduction and limits

[Companion probes](review-support/architecture-2026-10-03/README.md) retain the
standalone workloads. From the recorded baseline, build the architecture_baseline
and foundation_baseline examples, then execute prebuilt binaries serially in
three independent processes. Raw logs under target/architecture-review are
disposable; baseline, environment and scope remain recorded above.

The original review ran 515 focused tests and dependency/structure checks, not
full workspace tests/Clippy or the complete feature matrix. Successful checks
are not a soundness proof. New implementation acceptance belongs to the roadmap;
these retained measurements introduce no additional active phase sequence.

## Source-owned test pilot (2026-10-15)

TP05-TP09 consolidated 97 integration targets into five justified external or
instrumented targets. [Testing ownership](testing.md#default-placement-and-integration-exceptions)
describes the resulting layout. This section records focused observations, not
whole-workspace speed or memory acceptance.

The matched execution workload contains the same 43 `associated_types`,
`collection_interfaces` and `conversion_traits` cases, with unchanged source,
artifact/JIT routes and assertions. Baseline products were built before TP05
(19d74c5b layout); the candidate has the ea58ebc3 pilot layout. Both products were
saved before later edits, then executed in alternating order three times without
compilation inside the timed process. Baseline wall/CPU times sum its three
sequential executables; candidate filters select only those cases in the library.

Environment: Windows x86_64-pc-windows-msvc, Intel i9-12900K, 24 logical CPUs,
rustc 1.99.0 (b940084d7, LLVM 23.1.1), workspace test profile (opt-level 1), SDK
default source/native features, default Cargo and harness parallelism, warm
filesystem/build caches and fresh test processes. Installation and later focused
builds overlapped some samples; these are not idle-machine acceptance runs.
Process CPU and peak working set use Windows process counters; sampling interval
was approximately 50 ms. The wrapper's polling adds up to one interval per exit.

| Workload | Three wall samples (s) | Median wall (s) | Median CPU (s) | Median process peak (MiB) |
| --- | --- | ---: | ---: | ---: |
| Three original executables | 116.96 / 127.32 / 124.23 | 124.23 | 820.22 | 956.7 |
| One filtered library harness | 84.23 / 81.30 / 82.36 | 82.36 | 882.09 | 1425.6 |

All six runs passed. More available cases reduced the observed wall time while
CPU time and process peak increased. More simultaneous source contexts are a
plausible contributor to peak memory, not an isolated causal measurement. The
final 20% of each candidate run averaged about 0.97 logical cores; the numeric
aggregation overflow scenario remained last. Consolidation does not parallelize
the steps inside that scenario. CPU deltas describe utilization, not a direct
count of active test workers.

Build observations were 24.76 s before and 6.50 s after the pilot, with different
dependency rebuild sets. The larger SDK checkpoint built in 37.03 s. These do
not establish matched compile/link costs or build-memory improvement; a controlled
comparison remains open. No full-workspace build or execution was used locally.

### Shared-baseline lifetime control

A temporary SDK engine unit probe used the existing shared execution helper and
128 fresh sessions: first 64 distinct filenames/types, then the same 64 again.
Each session compiled an artifact and released both artifact and engine. Inputs
were `retention-{key}.kgr` with this generated source (`key` is 0 through 63):

```text
struct Value{key} { val value: i32 }
fn main() -> i32 { Value{key} { value: 42 }.value }
```

At each checkpoint a new empty session inspected its analysis snapshot's retained
definition and symbol counts, then released the snapshot/session. Windows
GetProcessMemoryInfo sampled working set and private bytes during a 150 ms pause
after release. No other build or test ran concurrently with this probe; the
single selected test used the same profile/features and default harness settings.
The source probe was removed after its executable was built; it is not a new
permanent test or public inspection API.

| Released sessions | Definitions | Symbols | Working set (MiB) | Private bytes (MiB) |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 1825 | 335 | 82.7 | 75.5 |
| 16 | 1949 | 353 | 121.7 | 109.4 |
| 32 | 2013 | 369 | 93.7 | 81.2 |
| 64 | 2141 | 401 | 105.6 | 100.4 |
| 96 | 2141 | 401 | 125.7 | 113.1 |
| 128 | 2141 | 401 | 124.8 | 112.2 |

Distinct source identities remain in the shared baseline after session teardown;
replaying the same names did not further grow the table in this workload. Working
set/private bytes include allocator and other preparation state, so their deltas
are not the byte cost of the identity table. This controls same-name reuse over
a longer session sequence; it does not prove whole-harness memory bounds, compare
fresh-baseline lifetimes, or replace the earlier deferred preparation acceptance.
Keep the baseline bounded to the test process. The existing plan retains the
controlled compile/link and original preparation timing/memory follow-ups.
