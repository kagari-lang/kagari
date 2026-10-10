# Performance Measurements

Current interpreter evidence is the HP06 execution-architecture evaluation below.
VE09 and VE08 are historical baselines; their mechanism descriptions refer to those
revisions and may have been replaced by HP00–HP06. Compiler, startup and platform
observations remain separate.

Keep workloads, hashes, environment and timing scope together. Allocation requests
are not RSS; compilation/setup and execution are separate. Small differences are
not significance claims, and local correctness does not imply GitHub CI or Lua parity.

## Execution architecture evaluation (HP06), 2026-10-10

HP00–HP06 implementation and final local correctness checks are complete. All 16
unchanged matched workloads improve against HP00, but **none reaches Lua parity**:
current interpreter medians are 2.57–51.11 times Lua. This closes the bounded
migration/evaluation, not the performance objective or complete GitHub CI acceptance.
The [execution plan](interpreter-hotpath-execution-plan.md#mandatory-review-of-earlier-optimizations)
records the final disposition of earlier IP/NE/VE mechanisms; the implemented owners
are described in [architecture](architecture.md).

Runtime-owned executable identities and checked publication now own prepared facts;
active execution regions borrow admitted function/link views and disjoint PC/window
state. Internal calls/returns share one protocol, managed/scalar operations share
prepared execution, and canonical layout admission plus borrowed enum reads replace
the separate VE09 shortcut and snapshot consumers. External entry, real dynamic
boundaries, roots, bounds, generation checks, cancellation and observable failure order
remain. Value stays 16-byte Copy; ordinary Option still allocates traced enum objects.

### Frozen paired throughput

Baseline: HP00/VE09 f97b4095, SHA-256
`a10ea34693c9a113b97cf2f1e2af1906c07a2b175e1bfcd209bb055c01cecc22`,
`target/hp00/baseline-executable`. Candidate: eca16497 (production code 375c727d;
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
is not relevant because every row is far above 1.0. Phase-level regressions and
their recovered gates remain recorded in the execution ledger.

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
separately afterward. All profile-window/counting checksums pass. Reports are
`target/lua-comparison/20261010T123134Z-macos-profile` (arrays/maps/fibonacci) and
`20261010T123211Z-macos-profile` (shared_generic/string_constants).

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

One final local workspace run passes: 1,948 tests, zero failures, two ignored;
strict workspace/all-target Clippy, formatting, structure (1,025 files, zero
violations/exceptions) and diff checks pass. Release verification additionally
passes 56 admission/lifetime/control contracts, the metadata-size check and three
manual memory probes. The memory probe initially assumed globally zero witness
groups after reload; eca16497 instead checks all old groups reclaimed and exactly
one current-program group alive, matching the existing ownership contract. Affected
diagnostic Clippy/format/structure checks pass after that test-only fix. No production
error is carried; the successful default full suite was not needlessly repeated.

GitHub has no run for the measured revision; complete CI/feature/backend acceptance
is pending and is not implied by local success. Shared generic Add still fails
MIR lowering with MissingBinding("checked callable requirement"); the unchanged
shared-identity benchmark does not establish that separate compiler capability.
The following source reproduces it with `cargo run -p kagari-cli -- run <file>`
(on macOS, use the DEVELOPER_DIR prefix below); it is outside this migration's scope.
No JIT, collector replacement or general enum unboxing was silently added to the task.

```kagari
use core::ops::Add;
trait Forward {
    fn forward<T: Add<T>>(self, a: T, b: T) -> T::Output { a + b }
}
impl Forward for i32 {}
fn main() -> i32 { val receiver: Forward = 0; receiver.forward(1, 2) }
```

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/hp00/baseline-executable
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/hp00/baseline-executable
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/profile_lua_macos.py arrays maps fibonacci
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/profile_lua_macos.py --source-forms shared_generic string_constants
DEVELOPER_DIR=/Library/Developer/CommandLineTools cargo test --release -p kagari-runtime --lib --features execution-diagnostics diagnostics::memory -- --ignored --nocapture --test-threads=1
DEVELOPER_DIR=/Library/Developer/CommandLineTools cargo test --release -p kagari-runtime --lib --features execution-diagnostics module::execution::tests::physical_instruction_budget -- --nocapture
```

Raw throughput: `target/lua-comparison/20261010T122909Z-paired` and
`20261010T122855Z-forms-paired`, retaining all samples, checksums, setup phases,
source/driver/binary hashes and environment. Diagnostic rows are in
`target/hp00/` and `target/hp04/region-views/`; final validation/memory logs in
`target/hp06/`. [Diagnostic instructions](../benchmarks/lua-comparison/README.md#execution-architecture-diagnostics-hp00)
keep counting/sampling separate from throughput. Raw target files are disposable;
this report and the plan retain the durable results and reproduction contracts.

## Native enum result layout reuse (VE09), 2026-10-10

The explicitly authorized follow-up is locally accepted. Native Map::get results
and their consuming patterns retain equal concrete enum layouts in different
module slots of one pinned program. EnumVariantRef::matches_layout now compares
those existing applied layouts directly when neither side has a lexical environment.
Runtime owner and variant checks precede the comparison; different generations or
lexical scopes retain the general type-graph comparison. Allocation, dynamic payload
checks, roots, traced enum storage, cancellation and return publication are unchanged.
No additional cache, public API, metadata field or unsafe code was introduced.

In the frozen map workload the median falls from 12,423.354 to 6,075.917 microseconds,
0.489x VE08 time (51.1% less). The isolated Map::get counting probe removes 90 Rust
allocation requests per iteration, with unchanged GC objects/collections. Other
workloads retain small increases and decreases below, including fibonacci +2.1%.
All 16 matched nontrivial workloads still fail Lua parity, at 3.66-201.94x Lua time;
this phase does not close the interpreter-wide performance goal. Full CI is unrun.

### Attribution and allocation

The unchanged `target/ve05/collections.kgr` probe performs 5,000 warmed iterations
and checks 325,000. Separate baseline counting and stack captures use VE08 production
code. Requests 100-199 in a diagnostic execution allocate 90 times in enum layout
compatibility, six in pattern descriptor admission, two in payload snapshots, one
for payload storage and one for allocation validation. This is a bounded stack
sample, not a distribution over every workload. Backtrace machinery is excluded
from counting. A temporary descriptor diagnostic confirms one ProgramDescriptor,
module slots 0/14, enum IDs 4/5, equal full applied layouts and no lexical environments.
Native result layout caching already hits; adding another cache would miss this cost.
All diagnostic instrumentation was removed before candidate verification and timing.

| Probe | VE08 requests | VE09 requests | VE08 requested bytes | VE09 requested bytes | Script objects / GC collections, both |
| --- | ---: | ---: | ---: | ---: | ---: |
| Map get / ordinary Option | 500280 | 50280 | 38753903 | 1353903 | 5001 / 10 |
| Map update + contains_key | 25045 | 25045 | 283323 | 283323 | 1 / 0 |

Requests count alloc/alloc_zeroed/realloc; requested bytes are neither RSS nor peak
live memory. Construction, public host entry/return and default GC remain included;
source compilation, loading and three warmups are excluded. Probe entry aliases
`string_constants` / `string_calls` retain their historical names but execute these
collection cases, not the frozen string timing fixtures. Value remains 16-byte Copy;
no backing-layout or persistent metadata size changed in VE09.

### Paired release measurements

Baseline: VE08 `06757739`, `target/ve08/candidate-executable`, SHA-256
`98a14ea576a1a4f616bb6573c8e7342f01148dbbeac70147753269d08edf2261`.
Candidate: that revision plus the EnumVariantRef layout comparison change,
`target/ve09/candidate-executable`, SHA-256
`a10ea34693c9a113b97cf2f1e2af1906c07a2b175e1bfcd209bb055c01cecc22`.

Apple M1 Max (10 logical CPUs, 32 GiB), macOS 26.6.2 arm64; Rust 1.98.1
(`48a229cea`, LLVM 22.1.8), Cargo 1.98.1; workspace release defaults and default
Cargo parallelism/target, source/native SDK, mlua lua54,vendored Lua 5.4.8. Only
interpreter execution is timed. Candidate build took 19.37 s; the paired drivers
reused it in 0.132 / 0.084 s, all excluded from execution. Each matrix runs
baseline,candidate,candidate,baseline sequentially with three warmups and eleven
samples per process/route, reversing initial order for the second pair. There are
22 samples per variant/engine/workload; all 1,672 measured batches pass checksums.
No tests, builds, allocation probes or profilers ran concurrently with throughput.

CPU frequency, affinity and background activity remain uncontrolled. Tables show
microseconds per complete workload, C/B median ratio and candidate sample extrema;
entry is normalized per call. Extrema are not confidence intervals; small changes
are not claims of significance. The 14,022.084 us string-constant sample is retained.
Raw reports retain both variants' ranges and separate setup measurements. Execution
includes host entry/return and ordinary GC; compile/verify/link/setup are excluded.
The seven original and twelve source-form fixtures are unchanged. Native is an
unmatched adapter diagnostic; host_callback compares the same Rust body separately.
The bounded numeric diagnostic matrix was not rerun for this enum-only change.

### Original workload results

| Workload | VE08 VM | VE09 VM | C/B | Candidate Lua | VM/Lua | VE09 VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 2488.500 | 2507.646 | 1.008 | 383.250 | 6.54 | 2440.833–2582.875 |
| arrays | 5005.250 | 5034.105 | 1.006 | 68.792 | 73.18 | 4971.166–5495.625 |
| branches | 2926.667 | 2952.624 | 1.009 | 806.792 | 3.66 | 2889.709–3013.667 |
| calls | 6127.688 | 6188.833 | 1.010 | 224.292 | 27.59 | 6146.750–6400.333 |
| entry | 1.412 | 1.412 | 1.000 | 0.029 | 49.00 | 1.404–1.452 |
| fibonacci | 12422.104 | 12688.541 | 1.021 | 355.584 | 35.68 | 12425.834–13253.875 |
| maps | 12423.354 | 6075.917 | 0.489 | 69.041 | 88.00 | 5975.333–6171.208 |

### Source form results

| Workload | VE08 VM | VE09 VM | C/B | Candidate Lua | VM/Lua | VE09 VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| forms_byte_state | 6964.188 | 6915.063 | 0.993 | 110.250 | 62.72 | 6865.083–6978.459 |
| forms_capture_cell | 10144.459 | 10129.708 | 0.999 | 140.500 | 72.10 | 10060.167–10484.417 |
| forms_concrete_generic | 3135.417 | 3078.604 | 0.982 | 103.084 | 29.87 | 3052.292–3126.583 |
| forms_direct | 345.916 | 348.812 | 1.008 | 75.146 | 4.64 | 335.500–398.709 |
| forms_field | 3749.501 | 3738.563 | 0.997 | 90.209 | 41.44 | 3704.917–3812.042 |
| forms_helper | 3132.875 | 3071.001 | 0.980 | 141.312 | 21.73 | 3046.250–3136.167 |
| forms_host_callback | 1912.896 | 1908.938 | 0.998 | 248.667 | 7.68 | 1891.167–1951.208 |
| forms_interface | 10710.438 | 10429.146 | 0.974 | 135.833 | 76.78 | 10378.250–10510.458 |
| forms_native | 1906.542 | 1900.250 | 0.997 | 140.688 | 13.51 | 1883.250–1945.833 |
| forms_shared_generic | 22307.730 | 21708.834 | 0.973 | 107.499 | 201.94 | 21553.542–22412.792 |
| forms_string_calls | 12905.834 | 12908.000 | 1.000 | 124.708 | 103.51 | 12804.000–13175.792 |
| forms_string_constants | 7070.312 | 7111.479 | 1.006 | 67.334 | 105.62 | 7017.875–14022.084 |

### Correctness and reproduction

All 38 affected existing contracts pass: enum payload identity/reload (4), source-free
native enums including nested traced payloads and rejected operations (2), generic
reload (2), native enum boundaries (6), hash handles/custom callbacks and cleanup
(8), GC ownership (6), native conversion (10). Runtime all-target Clippy, formatting,
structure (988 files, no violations/exceptions) and diff checks pass. No full
workspace suite was rerun for this bounded change; VE08's full pass is historical.
No carried local build/test failures remain. Complete GitHub CI is unrun.

Raw paired reports are `target/lua-comparison/20261010T002035Z-paired/` and
`20261010T002247Z-forms-paired/`. Counting/trace sources, builds and focused logs are
under `target/ve09/`; baseline/candidate allocation executables remain preserved.
Temporary logs/binaries are ignored, with durable evidence recorded here.

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/ve08/candidate-executable
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/ve08/candidate-executable
uv run python target/ve09/build_probe.py target/ve00/measure_source.rs target/ve09/candidate-allocations
target/ve09/baseline-allocations target/ve05/collections.kgr
target/ve09/candidate-allocations target/ve05/collections.kgr
```

The build helper selects the current release rlibs. Rebuilding a historical probe
requires its corresponding production revision; do not overwrite a saved baseline.

### Post-VE09 hotspot diagnosis, 2026-10-10

At `5791c6f3` (production code unchanged from VE09), separate allocation and stack
probes used the same M1 Max/macOS/Rust environment above, existing workspace release
rlibs, source/native SDK and default target/parallelism. Temporary harnesses used
`rustc -O -g`; compilation/setup were excluded. The ten non-native source-form
bodies were extracted unchanged from `benchmarks/lua-comparison/src/forms.rs`.
Each performs 5,000 iterations; three warmups precede a checked counting execution.
All checksums passed. Counting includes host entry/return and normal collection.
Requests count alloc/alloc_zeroed/realloc; requested bytes are not live memory.

| Source form | Rust requests | Requested bytes | Script objects | Collections |
| --- | ---: | ---: | ---: | ---: |
| direct / helper | 7 each | 153 each | 0 | 0 |
| concrete_generic | 7 | 163 | 0 | 0 |
| interface | 70,040 | 3,923,825 | 1 | 0 |
| shared_generic | 315,599 | 26,257,530 | 1 | 14 |
| capture_cell | 11 | 3,147 | 2 | 0 |
| field | 5,009 | 40,224 | 1 | 0 |
| byte_state | 36 | 2,881 | 1 | 1 |
| string_constants | 7 | 163 | 0 | 0 |
| string_calls | 7 | 159 | 0 | 0 |

Five separate serial stack probes covered shared_generic, interface,
string_constants, string_calls and byte_state. Each used `/usr/bin/sample PID 3 1`
inside a seven-second warmed checked execution window with allocation counting
disabled. No builds or other probes ran concurrently. These are wall-clock stack
samples with incomplete optimized-symbol attribution, not CPU counters or new
throughput comparisons; background activity/frequency remain uncontrolled.

Shared generic samples prominently reach malloc/free, method application,
root metadata validation and type arguments. Method-local generic applications
currently miss the closed-method reuse condition in `objects/application.rs`;
preparation builds environments and signatures again. Interface samples also show
argument Vec construction, metadata traversal and frame admission. These support
reusing generation-scoped prepared applications and avoiding transient argument/root
metadata construction as the first bounded optimization proposal.

String samples instead concentrate in frame validation, termination checks,
`poll_await`, dispatch and register access. Warm string bodies allocate no string
objects; `len` borrows text. Cached constant loads still validate/resolve their
module, and managed transfers/native calls leave scalar regions. Byte-state samples
show the same boundary machinery with only 36 allocation requests. Reducing these
repeated transitions is a distinct proposal; removing required cancellation, roots,
owner/generation or reentry checks is not justified. Capture-cell allocation counts
alone do not establish its CPU attribution. Map attribution remains the VE09 probe
above; no new map CPU profile was collected here.

Probe sources, extracted workload, library/binary hashes, counts and sample stacks
are under ignored `target/hotpath-analysis/`. Reproduce counting by linking
`counts.rs` with `target/ve09/build_probe.py` and running it against `forms.kgr`;
`sample.rs` takes the same source plus the selected form name. This diagnosis changes
no production code and establishes no additional Lua parity result.

## Compact value and interpreter final local evaluation (VE08), 2026-10-10

VE00-VE08 implementation and final local correctness evaluation are complete.
`Value` is 16 bytes and Copy in both debug and release, down from 32 bytes.
Runtime-local string constants share traced storage, concrete fields and calls use
prepared facts, selected collections use scoped access, and scalar segments retain
bounded code/bank borrows. Host retention, runtime ownership, generations, roots,
checked arithmetic and observable execution boundaries remain enforced.

The frozen Lua parity gate **fails all 16 matched nontrivial workloads**: candidate
medians take 3.65-201.48x Lua time. Improvements over VE00 include arrays taking
0.328x time, fields 0.469x, string constants 0.298x, arithmetic 0.720x and calls
0.823x. These are whole-track changes, not effects attributable only to Value size.
Full GitHub CI feature/backend acceptance remains unrun. The subsequently authorized
[VE09 follow-up](interpreter-value-execution-plan.md#ve09-native-result-layout-reuse)
is completed above; its improvement does not establish parity.

### Environment and measurement scope

Baseline is the preserved VE00 executable `target/ve00/baseline-executable`, SHA-256
`60aa2c07334fb25123208c8c77a4008d1bc93d2f680da6ac21560838e7ebfc81`.
Candidate is `target/ve08/candidate-executable`, SHA-256
`98a14ea576a1a4f616bb6573c8e7342f01148dbbeac70147753269d08edf2261`.
Its production revision is VE07 `0a6c34f0`; VE08 changes only integration fixtures
and documentation. All three matrices used this same candidate binary.

Apple M1 Max, 10 logical CPUs, 32 GiB, macOS 26.6.2 arm64; Rust 1.98.1
(`48a229cea`, LLVM 22.1.8), Cargo 1.98.1; workspace release defaults, default target
and Cargo parallelism, source/native SDK features, mlua lua54,vendored PUC Lua 5.4.8.
Only the interpreter is timed. Each matrix runs baseline,candidate,candidate,baseline
sequentially, with fresh process/state, three warmups per route and eleven measured
samples per process. Engine order rotates; the second pair reverses initial order.
Each variant/engine/workload has 22 samples. All 2,728 measured execution batches
across 31 workloads pass checksum validation; warmups are checked separately.

No compilation, tests, allocation probes or profilers ran concurrently with final
throughput. Builds reused the release cache and took 0.123 / 0.080 / 0.092 s for
original/forms/numeric, excluded from execution. Source compilation, verification,
runtime setup and linking are excluded; public host entry/return and ordinary GC
remain included. CPU frequency, affinity and background activity are uncontrolled.
Tables show microseconds per complete workload (entry normalized per call), C/B
candidate-to-baseline median ratio, and candidate sample extrema. Extrema are not
confidence intervals; small differences are not statistical significance claims.
Raw JSON/CSV retain both variants' ranges and separate setup samples.

### Final original workloads

The six nontrivial rows belong to the parity gate; entry measures the host boundary
separately. Algorithms, inputs and explicit while loops are unchanged from VE00.

| Workload | VE00 VM | Final VM | C/B | Candidate Lua | VM/Lua | Final VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 3438.229 | 2473.874 | 0.720 | 383.812 | 6.45 | 2448.333–2566.875 |
| arrays | 15296.376 | 5016.583 | 0.328 | 67.729 | 74.07 | 4969.792–5378.792 |
| branches | 3987.709 | 2925.938 | 0.734 | 801.917 | 3.65 | 2919.833–2945.292 |
| calls | 7417.396 | 6101.645 | 0.823 | 222.020 | 27.48 | 6059.500–6221.875 |
| entry | 1.455 | 1.409 | 0.968 | 0.028 | 49.60 | 1.403–1.415 |
| fibonacci | 15097.021 | 12413.729 | 0.822 | 352.021 | 35.26 | 12343.708–12730.042 |
| maps | 13961.312 | 12398.000 | 0.888 | 65.833 | 188.33 | 12324.584–12483.291 |

### Final source forms

The ten script forms belong to the parity gate alongside the six original cases.
`host_callback` compares the same Rust body through each host adapter separately;
`native` compares a Kagari Rust callback with a Lua script helper and is diagnostic.
All use 5,000 iterations. String routes alternate the unchanged 62/68-byte ASCII
constants and check 325,000 bytes; they do not compare Unicode character counts.
Shared generic/interface rows compare algorithms, not equivalent type systems.

| Workload | VE00 VM | Final VM | C/B | Candidate Lua | VM/Lua | Final VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| forms_byte_state | 7693.312 | 6851.208 | 0.891 | 109.604 | 62.51 | 6841.417–6942.167 |
| forms_capture_cell | 11002.834 | 10059.354 | 0.914 | 139.875 | 71.92 | 10025.958–10109.500 |
| forms_concrete_generic | 3622.916 | 3047.646 | 0.841 | 103.480 | 29.45 | 3037.667–3086.375 |
| forms_direct | 470.354 | 336.041 | 0.714 | 75.125 | 4.47 | 335.334–351.375 |
| forms_field | 7951.146 | 3725.791 | 0.469 | 90.145 | 41.33 | 3681.333–3849.625 |
| forms_helper | 3589.375 | 3030.062 | 0.844 | 139.896 | 21.66 | 3020.666–3049.084 |
| forms_host_callback | 2081.041 | 1892.250 | 0.909 | 253.167 | 7.47 | 1885.667–1901.709 |
| forms_interface | 10748.812 | 10429.708 | 0.970 | 134.480 | 77.56 | 10312.708–10779.500 |
| forms_native | 2081.166 | 1904.854 | 0.915 | 140.520 | 13.56 | 1892.625–1911.792 |
| forms_shared_generic | 21666.896 | 21386.646 | 0.987 | 106.146 | 201.48 | 21226.667–21479.916 |
| forms_string_calls | 30427.709 | 12702.312 | 0.417 | 123.063 | 103.22 | 12681.958–13175.917 |
| forms_string_constants | 23424.625 | 6974.895 | 0.298 | 66.438 | 104.98 | 6952.000–7161.708 |

### Final numeric diagnostics

The 12 numeric routes use bounded exact inputs with 20,000 iterations. Kagari uses
while and these Lua fixtures use numeric for; their ratios are diagnostic and are
excluded from the frozen matched parity gate. They do not establish equivalent
full integer/float domains or checked overflow. No fixture was changed to improve
these ratios. Numeric semantic/source-free contracts are checked separately.

| Workload | VE00 VM | Final VM | C/B | Candidate Lua | VM/Lua | Final VM min–max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| numeric_f32 | 2544.667 | 1879.708 | 0.739 | 443.125 | 4.24 | 1841.667–1937.125 |
| numeric_f64 | 2508.833 | 1826.458 | 0.728 | 443.688 | 4.12 | 1807.459–1939.542 |
| numeric_i16 | 2904.834 | 2088.417 | 0.719 | 581.625 | 3.59 | 2051.959–2144.584 |
| numeric_i32 | 2843.334 | 2062.625 | 0.725 | 574.229 | 3.59 | 2047.958–2077.917 |
| numeric_i64 | 2867.938 | 2056.062 | 0.717 | 574.375 | 3.58 | 2044.000–2088.542 |
| numeric_i8 | 2866.854 | 2087.791 | 0.728 | 574.104 | 3.64 | 2062.416–2115.792 |
| numeric_isize | 2865.708 | 2052.104 | 0.716 | 574.250 | 3.57 | 2043.958–2094.958 |
| numeric_u16 | 2833.188 | 2059.041 | 0.727 | 574.145 | 3.59 | 2056.333–2092.375 |
| numeric_u32 | 2815.292 | 2065.292 | 0.734 | 574.000 | 3.60 | 2056.333–2095.208 |
| numeric_u64 | 2793.709 | 2072.667 | 0.742 | 574.396 | 3.61 | 2064.125–2095.292 |
| numeric_u8 | 2840.812 | 2064.166 | 0.727 | 574.333 | 3.59 | 2056.375–2080.417 |
| numeric_usize | 2793.854 | 2089.604 | 0.748 | 576.812 | 3.62 | 2071.333–2172.166 |

### Memory, attribution and correctness

The 16-byte value does not halve total memory: strings, tuples, ranges and ephemeral
host payloads now have separately traced backing allocations; retained host handles
remain non-Copy. The representation section below retains the backing-layout and
allocation tradeoffs; intermediate VE05 probes remain in the historical report.
No whole-process memory reduction is claimed. VE07's independent counts still describe the identical final
production binary: arithmetic 550,015 Kagari / 250,007 Lua canonical operations;
direct 75,015 / 40,006; helper 105,012 / 65,007. These scalar probes allocate no
script GC objects. Generated-code and wall-clock sample evidence remain in the
historical VE07 report; counters/profiling are separate from throughput.

Final `cargo test --workspace --no-fail-fast` passes all unit, integration and doc-test
targets after correcting old fixture assumptions about inline strings and live
module constant caches. Exact final zero-root/object assertions remain. Workspace
all-target Clippy passed, followed by affected-target Clippy after fixture fixes;
formatting, structure (988 Rust files, no violations/exceptions) and diff checks
pass. Logs are under `target/ve08/`. This is local default source/native acceptance,
not an unrun GitHub CI/backend matrix result. No production API or semantic change
was needed for VE08 corrections; no carried build/test failures remain.

### Final reproduction and raw reports

Raw directories under `target/lua-comparison/`:

- `20261009T184853Z-paired/` (original).
- `20261009T185103Z-forms-paired/` (source forms).
- `20261009T185119Z-numeric-paired/` (numeric diagnostics).

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/ve00/baseline-executable
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/ve00/baseline-executable
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --numeric-matrix --baseline-executable target/ve00/baseline-executable
```

Ignored binaries/raw logs are local reproduction aids, not durable release assets.
The committed tables, hashes, environment and commands preserve the conclusions.

## Representation and allocation tradeoffs (VE04)

Measured VE04 release/debug Value is 16 bytes and Copy, HeapObjectId is 12, HeapObject
is 128, and a complete ObjectSlot is 168. TupleData is 32 plus 16 bytes per member;
String control storage is 24 plus text capacity; RangeValue is 24. RootedValue
remains a 32-byte owning lease. Host descriptors remain 40/104/48-byte payloads
behind compact checked IDs. These are implementation layout observations, not
portable ABI sizes, retained-memory measurements or RSS. Scalar banks already
stored raw 64-bit payloads and are not halved by the Value change.

The unchanged allocator probe runs one 5,000-iteration public execution after
three warmups and checks 325000. Counts cover Rust alloc/alloc_zeroed/realloc and
requested bytes, not just strings or heap nodes:

| Workload | Baseline calls → candidate calls | Baseline bytes → candidate bytes | Baseline heap allocations / collections → candidate |
| --- | ---: | ---: | ---: |
| string constants/length | 480007 → 7 | 50155179 → 163 | 0 / 0 → 0 / 0 |
| string function transfer/length | 500007 → 7 | 51455175 → 159 | 0 / 0 → 0 / 0 |
| tuple construction/member read | 35007 → 5315 | 1680179 → 1175451 | 0 / 0 → 5000 / 14 |
| one-element range traversal | 1540708 → 1480704 | 107233227 → 106005195 | 15000 / 25 → 20000 / 29 |

Strings now materialize once per runtime/module version, and standard readonly
string inputs no longer decode to owned Rust Strings. Warmed loads/transfers copy
no text. The seven remaining allocations occur at the complete execution boundary.
Tuples/ranges now have GC records: this is a real added collector cost, not a hidden
zero-allocation claim. Tuple copying no longer duplicates membership, giving fewer
Rust requests/bytes; each range construction adds one record, not one per iterator
step. Range iterator/Option/native costs still dominate its allocation workload.
No new collector or general enum-unboxing work was added to explain these results.

The comparison is VE00 -> VE04 on the M1 Max/Rust 1.98.1 release setup recorded
above, using a separate counting allocator and serial execution with ordinary GC.
Baseline is `target/ve00/measure_source`, candidate `target/ve03/measure_source`
(the same runtime accepted in VE04); sources are `strings.kgr` and
`tuple-range.kgr` under `target/ve00/`. The latter repeats `(i,65)` member reads
and one-element range traversal 5,000 times, adding 65 per iteration. Counts include
public execution boundaries and exclude compilation/setup; requested bytes are not
RSS. Rebuild historical probes from their matching production revisions. The full
VE04 record at `0f6252f8` retains raw paths and intermediate comparison tables.

## Compact value baseline (VE00), 2026-10-09

Production revision: `19fe129d` (runtime unchanged from `f521b2f0`) plus the
VE00 benchmark additions. Frozen workloads and representation decisions belong to
[the execution plan](interpreter-value-execution-plan.md#implemented-representation-and-boundaries).
The baseline executable was preserved before runtime edits at
`target/ve00/baseline-executable`, SHA-256
`60aa2c07334fb25123208c8c77a4008d1bc93d2f680da6ac21560838e7ebfc81`.
The final benchmark import-style repair does not alter any workload or runtime.

Environment: Apple M1 Max, 10 logical CPUs, 32 GiB RAM, macOS 26.6.2 arm64;
Rust 1.98.1 (`48a229cea`), LLVM 22.1.8, Cargo 1.98.1, default workspace release
profile, target and Cargo parallelism. SDK source/native and mlua lua54/vendored
features; locked PUC Lua 5.4.8; interpreter execution only. Command environment:
`DEVELOPER_DIR=/Library/Developer/CommandLineTools`; no custom compiler flags,
CPU target or jobs. Initial cold release build took 69 seconds; warm driver
builds took 0.083–0.104 seconds. Builds are excluded from execution.

Reproduce with `uv run python scripts/benchmark_lua.py --interpreter-only`,
then separately with `--source-forms` and `--numeric-matrix`, using the environment
above. Each route has three warmups and eleven checked samples in each of two
fresh sequential processes; the second reverses order. Ordinary GC remains enabled.
Compilation, verification and linking are outside execution; host entry/return
remain inside. No agent-started builds/tests/profilers ran during throughput
measurement. CPU frequency, affinity and unrelated background activity were not
controlled. All 1,364 measured execution batches passed independent checksums;
none were discarded. Allocation observations use separate instrumented runs.

The final VE08 report above compares this same preserved binary with the completed
track. Original one-version tables and representation probes remain in the historical
report at `29a5df9b`; entry is a separate boundary diagnostic.

## Historical interpreter checkpoints

Intermediate tables, rejected candidates, assembly/profiler paths and successful
command logs are retained in Git history, not repeated in the current report.
Use `git show 9dfeba3c:docs/performance-baseline.md` for the complete pre-cleanup
measurement record, or the phase commits below for their original context. These
are different revisions/workloads, not additional measurements of today's tree.

| Track | Historical evidence and checkpoint |
| --- | --- |
| IP00-IP04 | Post-GO06 baseline, execution windows, compact instructions, register/call windows and native preparation; final `3d3fb624`. |
| NE01-NE05 | Typed numeric baseline, closed regions, scalar banks/kernels and final numeric acceptance; final `da2a2d5c`. |
| VE00 | Preserved executable and representation choice, `29a5df9b`; current baseline identity retained above. |
| VE04 | Value/string/GC tradeoffs, `0f6252f8`; selected allocation evidence retained above. |
| VE05 | Prepared fields and scoped collections, `263499b5`; separate Vec/Map allocation probes. |
| VE06 | Prepared calls/frame retirement, `13c151ef`; call counts and added metadata cost. |
| VE07 | Borrowed scalar segments, `0a6c34f0`; code/sample evidence and unchanged canonical instruction counts. |

The final VE08/VE09 reports retain all matched workloads, regressions, measurement
conditions and open parity gates. Historical experiments never establish current
performance or unrun CI acceptance.

## Windows arithmetic comparison, 2026-10-07

Only the unchanged arithmetic workload was measured: 50,000 iterations of
`sum += i % 97; i += 1`, with result 2,398,830. Current worktree over `ef080344`
includes the focused benchmark selector and existing uncommitted frontend edits;
raw metadata and `worktree.diff` retain that state. This is a fresh Windows
observation, not a regression comparison with the M1 Max measurements.

Environment: Intel Core i9-12900K, 16 cores/24 logical CPUs, about 64 GiB RAM,
Windows 11 10.0.26300, Rust/Cargo 1.99.0, LLVM 23.1.1,
x86_64-pc-windows-msvc, workspace release/default target/default Cargo parallelism,
opt-level=3/default CPU target. SDK source/native features and mlua 0.11.6
lua54/vendored are enabled; native preparation/execution is disabled. The initial
release build is cold and takes 128.739 seconds, excluded from execution.
Lua source is the locked `lua-src` product recorded in `Cargo.lock`.

| Route | Median us per complete loop | Sample range us |
| --- | ---: | ---: |
| Kagari interpreter | 8648.300 | 5697.700-17582.000 |
| PUC Lua 5.4 | 630.650 | 597.800-3807.700 |
| Rust ordinary release | 19.639 | 18.445-21.227 |
| Rust explicit checked additions | 22.371 | 20.916-25.814 |

Kagari/Lua pooled median ratio is **13.71**. Two fresh sequential processes
perform three warmups per route and eleven single-call timed samples per route,
with rotating engine order and reversed second-process order. All 44 timed results
pass the independent Rust reference checksum. Per-process Kagari/Lua medians are
7,491.900/623.300 and 8,766.600/655.900 us. Source compilation, artifact preparation,
runtime construction and linking are outside timing; public host entry/return and
default GC remain included. No agent build/check/profile runs concurrently with
execution. Desktop activity, core placement and frequency are uncontrolled;
large sample ranges prevent treating the reported ratio as a precise constant.

Rust is rerun afterwards in two sequential processes: 20 warmups per route,
eleven batches of 1,000 calls per route/process, non-inlined functions and runtime
input/result `black_box`, all batch checksums validated. No dependencies/features;
standalone `rustc -C opt-level=3`, default CPU target, existing executable/cache.
Its public function boundary differs from the embedding APIs; all intermediates
fit i32. Earlier standalone medians were 19.148/22.201 us on the same machine.

Reproduce the focused Kagari/Lua run:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --workload arithmetic
```

Raw CSV, hashes/environment, setup timings, summary and Rust rerun samples are in
ignored `target/lua-comparison/20261007T091754Z/`; Rust source/executable and original
methodology remain under `target/rust-loop-baseline/`. The driver's original final
printing failed after saving all samples/JSON because it still expected seven
workloads; focused printing was corrected and replayed against the retained report.
No timed batch was rerun for that correction. Release build, Rust formatting,
structure audit (921 files, no violations), focused CLI/summary checks and diff
checks pass; no workspace test suite was run.

## Scoped identity pipeline, 2026-10-03

Reproduce with `uv run python scripts/measure_definition_identities.py`. The driver
creates/reuses an isolated worktree at clean `7857fd8a`, copies the identical
`definition_pipeline.rs` probe into it, builds both versions sequentially and runs
baseline/candidate/candidate/baseline. Candidate production code is `41b29c28`.
Raw logs, allocator passes, toolchain and probe hash are under ignored
`target/identity-measurements/`. The checked workload has 32 nominal `Player`
functions, one generic identity function and an entry constructing a Player and
returning 31. Both versions verify and execute that result.

Environment matches ID01: Windows 11 Pro, i9-12900K (16 cores/24 logical CPUs),
63.7 GiB RAM, rustc 1.99.0 (`b940084d7`, LLVM 23.1.1), x86_64-pc-windows-msvc.
Both use the workspace release profile, default SDK source/native features,
default Cargo parallelism and their worktree's default target directory. No builds
or tests run concurrently with measurements. Runtime operations are single-threaded.
One warmup precedes each operation in each process; fresh engines/runtimes are
created as specified below. Each timing result pools fourteen execution samples.
Destruction is outside the timed interval. An inactive forwarding System allocator
remains installed during timing; host background activity is uncontrolled. These
are finite workload observations, not whole-language performance guarantees.

Builds are excluded; differing target-cache states do not establish a build-speed comparison.

| Operation | Owned baseline median | Scoped candidate median | Observed change |
| --- | ---: | ---: | ---: |
| Fresh engine + complete analysis | 137.940 ms | 174.065 ms | +26.2% |
| Fresh engine + artifact compilation | 292.057 ms | 266.374 ms | -8.8% |
| Clone authored program + verify/adopt | 50.244 ms | 31.763 ms | -36.8% |
| Create runtime + install foundation | 57.595 ms | 63.345 ms | +10.0% |
| Import into an existing runtime | 2.275 ms | 2.012 ms | -11.6% |

`fresh_analysis` retains both engine caches and snapshot. Artifact compilation
retains the returned unsealed authoring artifact, including portable MIR.
`verified_program` clones its input inside measurement and retains the complete
runtime verification product. `runtime_creation` retains the runtime; for
`runtime_import`, both the input verified program and runtime construction are
outside timing/counting, and the returned loaded program remains installed.

Allocation counting uses a separate warmed pass. Calls include allocations and
reallocations; gross bytes sum requested sizes, including full realloc sizes.
Retained/peak bytes are net live-allocation deltas from entry, so import may also
free pre-existing runtime buffers. They exclude allocator bookkeeping, fragmentation,
pre-existing caches and input programs; they are not RSS or exclusive metadata size.
The analysis delta covers the complete retained cache graph; the verification delta
covers its full owned program/table graph. Counts varied slightly between processes;
the table reports the first counting pass, with both raw passes preserved.

| Retained operation result | Baseline bytes | Candidate bytes | Baseline allocation calls | Candidate allocation calls |
| --- | ---: | ---: | ---: | ---: |
| Fresh engine + complete analysis | 14,624,321 | 10,364,137 | 3,288,870 | 3,601,730 |
| Fresh engine + artifact compilation | 2,728,648 | 3,182,804 | 7,471,364 | 5,595,317 |
| Clone authored program + verify/adopt | 1,827,175 | 760,720 | 1,480,900 | 704,051 |
| Create runtime + install foundation | 1,417,415 | 1,221,122 | 1,485,456 | 1,562,887 |
| Import into an existing runtime | 53,176 | 1,163,897 | 15,859 | 23,447 |

| Complete encoded artifact | Baseline | Candidate |
| --- | ---: | ---: |
| KBC including portable MIR/debug metadata | 778,657 bytes | 279,003 bytes |

The artifact is **64.2% smaller**. Retained analysis state decreases **29.1%** and
verified-program state decreases **58.4%**, while the returned unsealed authoring
artifact retains **16.6% more** memory. Verification allocation calls decrease
52.5%; artifact compilation calls decrease 25.1%. Gross requested bytes increase
for analysis (272.5 -> 322.3 MB) and artifact compilation (539.2 -> 575.1 MB).
Source checking/proof validation still project transient exact paths, so shorter
stored records do not establish lower total allocation traffic. Analysis takes
26.2% longer and runtime creation 10.0% longer in this run; those regressions remain
explicit measured costs rather than claimed speedups.

Independent runtime imports retain the original immutable version identity but
normalize nonempty module metadata separately. The probe records full module Arc
sharing as true for baseline and false for candidate. Import retains 1.16 MB versus
53 KB and requests 3.23 MB versus 0.77 MB. Do not equate version sharing with whole
normalized-module sharing, or extrapolate the observed 11.6% import-time reduction
into a general loading improvement.

The common append probe measures one append to a 10,000-definition table,
excluding setup: 0.2 us/zero allocations without a retained snapshot versus
323.2 us/ten allocations with one. This isolates retained-prefix index copying,
not whole-pipeline cost.

Finite follow-up costs are source/proof authoring projections, table index copies
on retained-prefix appends, and normalized metadata copies across independent
runtimes. No interpreter-loop optimization, global identity interner, stable identity
hash, incremental disk cache, format version bump or compatibility reader is part
of ID01-ID05.


## Preparation and runtime construction

Both comparisons use Windows 11/i9-12900K/63.7 GiB/rustc 1.99.0, workspace O1,
default target/parallelism and warm build caches. Execution is single-threaded,
builds excluded, with no concurrent measuring jobs; host background activity is
uncontrolled. These are historical paired observations, not current timings.

| Workload/change | Baseline | Candidate | Scope |
| --- | ---: | ---: | --- |
| First Runtime::new on a host thread, registration sharing | 1,124.628 ms | 481.447 ms | Baseline 1b975a36 diagnostic executable, stage-print overhead included; default runtime features |
| Next five fresh runtimes, median | 1,079.989 ms | 76.996 ms | Same thread's immutable registrations; destruction excluded |
| Complete language-contract matrix, sealed bytecode reuse | 155.36 s | 105.71 s | Clean 145b34b1 versus TO02, one serial process each; default SDK source/native, unchanged 149 cases/408 routes |

Reproduce construction with `cargo run -p kagari-runtime --example runtime_construction`.
For the matrix, build with `cargo test -p kagari-embed --lib --no-run` and run the
matching executable with the exact language-contract test filter and
`--test-threads=1 --nocapture`; an obsolete executable is not a valid baseline.

Sharing checked registrations preserves fresh heap/host/generation state and
installation checks. Sealed-input reuse reduces native preparation from four
equivalent bytecode verifications to one, while independently verifying MIR,
canonical correspondence and raw/changed inputs. Later identity migration changes
the workload representation; these figures must not be labeled current startup
performance. The matrix result is a suite observation, not script throughput.

## Interface dispatch and sorting

The source/allocation finding was measured on the Apple M1 baseline documented in
[the architecture review](architecture-review-2026-10-03.md). Subsequent interface
sharing uses that review's O1/default SDK features, prebuilt serial processes and
counting allocator. Five warmups and 21 samples include construction plus 1,000
interpreted calls; widths 1/8/32 retain the same selected method/result.

| Interface width | Baseline allocation calls | Shared descriptors/receiver cache | Candidate median |
| --- | ---: | ---: | ---: |
| 1 | 75,099 | 41,111 | 4.781 ms |
| 8 | 243,358 | 41,370 | 4.888 ms |
| 32 | 820,246 | 42,258 | 4.891 ms |

The removed cost is whole-table copying. One-time construction/cache allocation
remains. Existing-view regressions isolate 1,000 warmed calls: 16,000 allocations
for direct methods and 26,000 for closed generic inherited methods at every width,
with zero GC publications. No zero-allocation dispatch claim is made.

Reproduce with `cargo test -p kagari-vm --test library_measurements -- --ignored
--nocapture`. The bounded sorting workload uses one warmup/three reset samples,
compilation/input construction excluded, O1 and explicit roots. At 4,096 elements,
primitive concrete/interface sorting medians were 95.917/104.250 us (58/439 Rust
allocations, zero GC publications). Script comparator sorting still made 53,392
comparisons and 53,393 GC allocations, at 162.905/162.114 ms. The recorded earlier
callback baseline was 159.662/159.655 ms: no callback-loop speedup was demonstrated.
Use profiling evidence in the Lua report before selecting interpreter work.

## Reproduction boundaries

Committed probes and benchmark reports preserve workloads. Temporary executable
copies/logs belong under ignored target directories. Use the recorded commit when
reproducing historical comparisons; rebuilding the current tree alone is not a
before/after measurement. Mac O1, Windows O1 and Windows release observations are
not interchangeable. Source-free checks, roots, storage access, cancellation and
generation validation cannot be removed to improve a timing.
