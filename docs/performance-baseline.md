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

The latest compiler-loop result is [BO13-BO16 on the i9-12900K](#bo13-bo16-scalar-loops-windows-2026-10-27).
It includes all 19 current original/forms timings and arithmetic machine-code
attribution. Earlier i7 measurements below remain revision-specific evidence.

## BO08 Kagari/Lua cost attribution, Windows 2026-10-27

This is a fresh analysis of production revision `03246d94a1a153b18746f0c245ec2c07cb015421`,
with a clean working tree during measurement and no runtime changes. It supersedes
the applicability of older attribution to this revision, not the older measurements
or their unresolved IP12/BO04 acceptance gates. The 16 matched non-entry rows take
1.47..100.84 times PUC Lua time on this machine. Ordinary calls and native/library
boundaries now explain much more of the gap than excess scalar instruction count.
These measurements do not establish a regression against another revision/machine.

### Environment and timing method

Intel Core i7-10700F, 16 logical CPUs, 67,017,248 KiB visible memory, Windows 10
10.0.19045; rustc 1.99.1 (`58cf8f9c3`, LLVM 23.1.1), Cargo 1.99.1 (`5f94df478`),
x86_64-pc-windows-msvc. Workspace release defaults (opt-level 3), default Cargo
parallelism and target directory; no environment overrides, affinity or priority
changes. Kagari source/native features are compiled in, but JIT backend preparation and
execution are disabled by `--interpreter-only`. Lua is vendored PUC 5.4.9 through
mlua 0.12.2 / lua-src 551.0.2, not LuaJIT.

The original, source-form and numeric suites ran sequentially, each with two fresh
processes, three warmups and eleven checked execution samples per engine/workload.
Workload/engine order reverses in the second process. Tables report the median of
the two process medians, normalized by batch size. Each process's full sample range
remains in raw data; the final column below is only the range of process medians,
not a confidence interval. All 1,276 timed batches and warmups passed independent
result checks. No build, test or profiling process overlapped throughput timing.
The original release rebuild took 206.154 seconds; forms/numeric reused it (0.263 /
0.293 seconds). Compilation, validation, linking and construction are excluded from
execution. Host entry/result handling and default automatic GC remain included.
Desktop activity/frequency/temperature were uncontrolled; two processes do not
establish significance for close rankings among numeric types.

Executable SHA-256: `883ea385c40e3d262b5019889ddc7ad697209ac9a585734ded4426f69ee07f9a`;
Cargo.lock: `23d256429adeecc8ad6c0501a6c2ee3e900532aecb6e4571c3b06862b0ccfca4`.
Original/forms/numeric workload SHA-256 values are respectively
`cbe3cd56a118fcca729ecdbdfaf09291fbb6f73a31d1ee19a8d80e01b644b57d`,
`38a7c148386d55a90a6c8aa2e632552d8cf632fb527db739db927b58a003889b`,
`81cc727e81bedcac2e27a99c61716fe212f85c1e6822e7c65be91d229f91fa95`.

### Execution observations

Times are microseconds per complete workload; entry is per host call from a batch
of 1,000. Original sources use matched while loops. Source forms use 5,000
iterations; `native` compares a Kagari Rust callback with a Lua script helper and
is diagnostic only. `host_callback` invokes the same checked Rust helper through
both adapters. Interface/shared-generic rows match algorithms, not type systems.

| Workload | Kagari (us) | Lua (us) | K/L | Kagari process medians (us) |
| --- | ---: | ---: | ---: | --- |
| arithmetic | 1,568.350 | 891.100 | 1.76 | 1,562.200..1,574.500 |
| arrays | 11,748.100 | 145.950 | 80.49 | 11,734.100..11,762.100 |
| branches | 2,169.750 | 1,480.500 | 1.47 | 2,114.000..2,225.500 |
| calls | 8,690.450 | 433.050 | 20.07 | 8,673.400..8,707.500 |
| entry | 2.800 | 0.061 | 46.02 | 2.779..2.822 |
| fibonacci | 18,659.850 | 469.050 | 39.78 | 18,499.000..18,820.700 |
| maps | 9,486.850 | 131.700 | 72.03 | 9,471.600..9,502.100 |

| Workload | Kagari (us) | Lua (us) | K/L | Kagari process medians (us) |
| --- | ---: | ---: | ---: | --- |
| forms_byte_state | 21,378.550 | 212.000 | 100.84 | 21,316.400..21,440.700 |
| forms_capture_cell | 9,551.550 | 218.400 | 43.73 | 9,480.200..9,622.900 |
| forms_concrete_generic | 4,252.550 | 189.700 | 22.42 | 4,204.800..4,300.300 |
| forms_direct | 210.150 | 118.350 | 1.78 | 205.600..214.700 |
| forms_field | 1,085.600 | 180.700 | 6.01 | 1,082.800..1,088.400 |
| forms_helper | 4,176.600 | 195.350 | 21.38 | 4,174.700..4,178.500 |
| forms_host_callback | 5,127.500 | 384.550 | 13.33 | 5,111.400..5,143.600 |
| forms_interface | 12,603.000 | 217.150 | 58.04 | 12,437.300..12,768.700 |
| forms_native | 5,149.050 | 196.550 | 26.20 | 5,126.200..5,171.900 |
| forms_shared_generic | 18,723.250 | 205.600 | 91.07 | 18,099.300..19,347.200 |
| forms_string_calls | 10,463.150 | 215.800 | 48.49 | 10,411.000..10,515.300 |
| forms_string_constants | 5,858.300 | 140.500 | 41.70 | 5,852.300..5,864.300 |

The numeric matrix uses 20,000 iterations. Kagari uses while and fixed-width
checked arithmetic/casts; Lua uses numeric-for and its integer/double domains.
These are bounded-input diagnostics, outside the matched parity gate. In
particular, f32 is not equivalent to Lua's double semantics, and the rows do not
justify ranking integer widths by implementation quality.

| Workload | Kagari (us) | Lua (us) | K/L | Kagari process medians (us) |
| --- | ---: | ---: | ---: | --- |
| numeric_f32 | 1,183.450 | 444.000 | 2.67 | 1,183.400..1,183.500 |
| numeric_f64 | 1,431.450 | 593.550 | 2.41 | 1,191.000..1,671.900 |
| numeric_i16 | 1,880.550 | 913.850 | 2.06 | 1,467.500..2,293.600 |
| numeric_i32 | 1,435.250 | 723.450 | 1.98 | 1,432.500..1,438.000 |
| numeric_i64 | 1,508.400 | 728.200 | 2.07 | 1,440.500..1,576.300 |
| numeric_i8 | 1,467.350 | 726.300 | 2.02 | 1,452.800..1,481.900 |
| numeric_u16 | 1,374.100 | 722.300 | 1.90 | 1,367.200..1,381.000 |
| numeric_u32 | 1,743.350 | 761.800 | 2.29 | 1,431.400..2,055.300 |
| numeric_u64 | 1,403.550 | 725.250 | 1.94 | 1,403.500..1,403.600 |
| numeric_u8 | 1,402.550 | 732.300 | 1.92 | 1,369.800..1,435.300 |

### Independent instruction and lifetime observations

After throughput, the existing Windows sampler ran the same saved executable/PDB
for every original and source-form row, one process at a time. Each has three
warmups and a ten-second ordinary execution window. GC/object deltas come from
that window; observer-based Kagari counts and Lua instruction hooks run separately,
outside the window. Object counts exclude executable metadata records, including
admitted method computations. Counts are logical VM operations, not CPU instructions
or equally costly units. Sampling timings are not substituted into the tables.

| Workload | Kagari instructions | Lua instructions | Objects / invocation | Collections / invocation | Unresolved RIP samples |
| --- | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 300,011 | 250,007 | 0.000 | 0.0000 | 99.6% |
| arrays | 56,021 | 42,012 | 1.000 | 2.0000 | 43.7% |
| branches | 390,012 | 309,968 | 0.000 | 0.0000 | 99.2% |
| calls | 120,011 | 110,007 | 0.000 | 0.0000 | 37.3% |
| entry | 2 | 2 | 0.000 | 0.0000 | 68.6% |
| fibonacci | 164,183 | 120,400 | 0.000 | 0.0000 | 27.9% |
| maps | 46,021 | 24,012 | 2001.000 | 6.0000 | 50.6% |
| byte_state | 130,014 | 80,523 | 2.000 | 1.0000 | 25.0% |
| capture_cell | 70,013 | 75,007 | 2.000 | 0.0057 | 46.5% |
| concrete_generic | 55,008 | 60,007 | 0.000 | 0.0000 | 47.6% |
| direct | 40,008 | 40,006 | 0.000 | 0.0000 | 98.4% |
| field | 60,010 | 55,008 | 1.000 | 0.0019 | 57.7% |
| helper | 50,008 | 65,007 | 0.000 | 0.0000 | 41.1% |
| host_callback | 30,008 | 45,006 | 0.000 | 0.0000 | 46.2% |
| interface | 55,010 | 65,009 | 1.000 | 4.9107 | 41.9% |
| native | 30,008 | 65,007 | 0.000 | 0.0000 | 43.0% |
| shared_generic | 60,010 | 60,009 | 1.000 | 4.9196 | 31.1% |
| string_calls | 90,008 | 77,506 | 0.000 | 0.0000 | 35.2% |
| string_constants | 70,008 | 52,506 | 0.000 | 0.0000 | 42.4% |

RIP samples use local release PDB names only when size/unwind boundaries verify the
address. Many addresses remain unresolved, especially scalar loops. Identified
call samples include route selection, installation validation, frame/window setup,
driver transitions, polling and hashing. They corroborate those paths but do not
partition total execution into CPU percentages; unresolved samples must not be
relabelled as dispatch, numeric kernels or some nearby PDB symbol. No new assembly
or hardware-counter attribution was collected. The prepared operation is 24 bytes,
canonical bytecode instruction 144 bytes, Value and return packet 16/24 bytes on
this build; canonical size is not the hot-stream footprint.

### Current mechanisms and avoidable work

**Scalar loops (arithmetic, branches, direct, numeric).** The old 550,015-operation
arithmetic stream no longer applies: arithmetic executes 300,011 operations versus
Lua's 250,007, with 50,004 remaining Moves. Branches executes 390,012 versus
309,968, with 60,004 Moves. Direct is already 40,008 versus 40,006. Unified slots,
direct destinations, constant operands and predicate branches are implemented.
Remaining work includes transport retained by conservative placement, per-instruction
PC/initialization/state checks, and indirect calls to shared numeric payload kernels
returning Result. Checked overflow, source width, trap position and cancellation
are real contracts; source code alone does not prove which checks survive LLVM or
their elapsed shares. The current token check is inline, unlike the old Windows
assembly finding. See [scalar execution](../crates/kagari-runtime/src/frame/cursor/scalars.rs)
and [payload kernels](../crates/kagari-types/src/payload.rs).

**Small script calls (calls, fibonacci, helper, concrete_generic, string_calls).**
Helper takes 19.87 times direct time, although it dispatches only 1.25 times as many
Kagari operations and fewer than Lua's helper. Calls executes 120,011 operations
versus Lua's 110,007; no script objects or GC occur. Fibonacci admits 21,892 script
frames including main. Each invocation selects the current route, admits its
publication and arguments, reserves/initializes banks, copies parameters, publishes
dependencies, leaves/reenters the driver, retires the window and publishes return
values. [Route selection](../crates/kagari-runtime/src/module/dispatch/mod.rs)
captures current code through a Mutex; [installation validation](../crates/kagari-runtime/src/module/installation.rs)
also locks current metadata, and [frame admission](../crates/kagari-runtime/src/frame/admission.rs)
validates executable metadata again. Repeated registry/liveness admission inside
one callback-free transition and initializing parameter storage before overwriting
it are concrete ownership-review candidates, not permission to remove checks.
Same-publication correspondence no longer rebuilds an identity table, and direct
scalar calls already avoid temporary argument Vecs. Ordinary nested windows retire
at the tail; independent-session compaction does not explain these rows.

**Native/library routing (arrays, byte_state, string_constants, native,
host_callback, maps).** [prepare_calls](../crates/kagari-runtime/src/module/execution/calls.rs)
rewrites native imports into the common current-selection Call operation and sets
native_calls to zero. Thus the earlier region-native descriptors for String length
and Vec index/set are not selected by these ordinary prepared functions. This
source change is present in `4d9f2afa`; it prevents a pinned old native callback
from bypassing current publication selection. It also means every such operation
now pays explicit native frame admission, entry, result publication and return
through [native frames](../crates/kagari-runtime/src/frame/native.rs). Arrays has
8,001 native invocations (construction, 2,000 push, 4,000 get, 2,000 set), plus its
call to run. Byte_state has 15,000 index/set calls and one Vec construction.
String constants has 5,000 native len calls. A length load or scalar element read
is therefore surrounded by a general invocation protocol. Merely editing the old
cursor-native handler would not optimize these selected execution paths.

**Vec-specific repeated admission (arrays, byte_state).** Storage is already
Vec<i32>/Vec<u8>, not a boxed object per scalar. Sequence argument checking,
sequence-family admission and buffer access repeatedly resolve the same handle
and storage contract. [VecSet](../crates/kagari-runtime/src/native/primitive.rs)
checks its value signature and runs replacement preflight, then
[buffer_set](../crates/kagari-runtime/src/gc/buffers.rs) repeats mutable/type/bounds
admission; multiple polls surround those stages. General SDK conversion may
reenter, so that gap requires revalidation; the closed primitive has no such gap.
Byte_state additionally executes 40,000 conversions, 25,005 Moves and 10,000
numeric operations for its explicit width/bit rules. These are additional costs,
not evidence that eight-bit arithmetic itself is 100 times slower. Arrays allocates
one script object and collects twice per workload; byte_state allocates two objects
and collects once. Per-element object allocation is not their explanation.

**Map insertion/results (maps).** Integer keys use the builtin-key branch, with
no script Hash/Eq invocation. [map_insert](../crates/kagari-runtime/src/gc/maps_sets.rs)
checks contains_key and reserves, then [HashMapStorage::insert](../crates/kagari-runtime/src/native/hash_storage.rs)
checks contains_key and reserves again before insert. The 2,000 inserts therefore
make 6,000 explicit membership/insertion searches, plus 2,000 gets (rehash work is
additional). [Option construction](../crates/kagari-stdlib/src/foundation/enumerations/value.rs)
allocates a heap enum and payload buffer for every successful get. The workload
allocates 2,001 script objects and collects six times, then separately executes
2,000 variant tests and 2,000 payload reads through
[canonical VM handlers](../crates/kagari-vm/src/executor/aggregate_ops.rs).
Repeated searches and result protocol are concrete excess work; their separate
elapsed shares are unknown. IP11 previously rejected an insertion-only prototype
after it missed its timing target, so that experiment is not a promised solution.

**Interfaces and shared_generic.** Interface has 55,010 Kagari instructions versus
65,009 Lua; shared_generic has 60,010 versus 60,009. Slowdown therefore cannot be
explained by a large opcode-count multiplier. Each fresh
[MethodInvocation](../crates/kagari-runtime/src/objects/invocation.rs) repeatedly
constructs/views current selection during application, compatibility and admission;
admit then inserts a MethodComputation in the
[application store](../crates/kagari-runtime/src/execution_metadata/applications.rs).
The 5,000 calls therefore create 5,000 admitted metadata records, independently of
whether semantic application/environment caches hit. Retirement drops the frame
edge; records are reclaimed by metadata GC, not immediately on return. Both runs
collect about 4.9 times per workload despite creating only one ordinary interface
object. Shared_generic additionally validates type arguments, looks up an application,
transports scoped generic Values and validates adapted results. A Value carrying i32
is not itself a heap box. Application-cache hits do not eliminate this invocation
work, and historical zero warm application-allocation claims do not cover it.

**Captured cell and fields.** capture_cell creates one closure and one cell per
workload, not per iteration. Its 5,000 calls add 10,000 ReadCell and 5,000 WriteCell
operations through canonical VM boundaries; those repeatedly acquire frame/slot/type
and checked heap access. Field uses the prepared scalar-field path and only one
struct allocation per workload; 10,000 field reads and 5,000 writes still admit the
current heap identity/layout, acquire storage and decode/encode field bytes. Neither
row is repeated object copying. Cell access needs shared mutation semantics; fields
must keep alias-visible writes and trap order. A prepared cell operation is a
candidate for the existing region owner, not scalar replacement across arbitrary calls.

**Strings and entry.** Both warmed string windows allocate zero script objects and
perform zero GC. Literal materialization is once per module constant pool; handle
copies do not copy the 62/68-byte texts. string_constants is primarily the length
call protocol plus managed transport; string_calls adds 5,000 identity script calls
and managed argument/result admission. Entry executes the same two logical operations
as Lua but includes SDK selection/context validation, outer session scope, VM nested
scope admission (same session, not a second independent session), frame execution,
rooted result/report creation and retirement. Its 2.800 us host cost is a distinct
embedding/API question, not the scalar engine's arithmetic throughput.

### Direct bytecode inspection, 2026-10-27

A follow-up inspection compiled the full SOURCE string from the source-form harness
with the same declared native_step interface, default compilation options and SDK
artifact verification. A temporary Rust probe linked the existing release libraries
and used PreparedDisassembly to inspect the resulting validated artifact. No runtime
source or benchmark timing changed. direct has eight logical slots (state=s0,
sum=s1, i=s2) and fifteen stored instructions. The existing execution count of
40,008 is exactly five entry operations, 5,000 iterations of eight operations,
one final predicate and two exit operations. Simplified operand spelling below
resolves constant-pool operands and abbreviates checked i32 operation names:

```text
00  load_const s0, 1
01  load_const s3, 0
02  move s1, s3
03  move s2, s3
04  jump 05
05  predicate_branch.lt s2, 5000, then=06, else=13
06  mul s4, s0, 3
07  add s5, s4, 7
08  rem s6, s5, 251
09  move s0, s6
10  checked_add s1, s1, s6
11  checked_add s2, s2, 1
12  jump 05
13  move s7, s1
14  return s7
```

The hot redundancy candidate is 08-10: the final remainder can commit directly to
state, with the following sum reading state, eliminating the 5,000 executions of
09. Multiplication/addition must keep their intermediate results separate from
state: if either traps, the assignment must not partially commit. The current
[placement rule](../crates/kagari-compiler/src/bytecode/placement.rs) only redirects
an adjacent producer/Move when the temporary's sole use is that Move. The displayed
result also supplies the sum, so this multi-use form requires a more general proof
and rewriting of later uses. Full types, origin selection, debugger availability
and successful assignment publication still constrain the transformation.

The jump at 04 targets the immediately following instruction; the return temporary
at 13 carries no additional computed value; and the zero temporary at 01-03 uses
three instructions where two direct constant-to-home writes could suffice. These
are entry/exit savings, not per-iteration costs. Together with the hot Move, an
illustrative compact form would have eleven stored instructions and 35,005 dynamic
operations, assuming those origin/debug/commit proofs are supplied. This is a
work-count proposal, not an implemented change or a measured elapsed improvement.
Scalar physical slot reuse does not itself remove the Move instruction/dispatch.

Constant operands 3, 7, 251, 1 and 5000 already avoid per-iteration LoadConst;
PredicateBranch already combines comparison and control without materializing a
Boolean slot. There is no repeated type resolution, heap operation or call in the
loop body. The arithmetic operations, loop predicate and backedge do real work;
intermediate slots alone are not proof of redundant operations. The full disposable
dump is target/lua-comparison/direct-bytecode.txt; the listing and method above
preserve the relevant evidence independently of that local file.

### Direct Lua instruction comparison, 2026-10-27

A temporary Rust probe linked the benchmark's existing mlua release library and
compiled the exact Lua direct source generated by forms.rs (the explicit while,
not numeric-for). Function::dump(true) produced the stripped 5.4 chunk. A local
reader followed the vendored Lua 5.4.9 ldump.c layout and lopcodes.h bit fields,
checked its header/sizes/native test numbers, consumed the entire stripped chunk,
and resolved all instruction words/constants. The vendored Lua execution returned
569840 and its independently attached instruction hook counted exactly 40,006,
matching the original profile. No new execution timing was measured. The artifact
and reader are disposable target/lua-comparison/direct-lua54.luac and
read-direct-lua.py; relevant decoded facts are retained here.

Lua uses four register slots (state=R0, sum=R1, i=R2, scratch=R3), two constants
K0=3/K1=251, and nineteen 32-bit instruction words. Its decoded body is:

```text
00 LOADI    R0, 1
01 LOADI    R1, 0
02 LOADI    R2, 0
03 LOADI    R3, 5000
04 LT       R2, R3, polarity=0
05 JMP      17
06 MULK     R3, R0, K0
07 MMBINK   (multiply fallback)
08 ADDI     R3, R3, 7
09 MMBINI   (add fallback)
10 MODK     R0, R3, K1
11 MMBINK   (modulo fallback)
12 ADD      R1, R1, R0
13 MMBIN    (add fallback)
14 ADDI     R2, R2, 1
15 MMBINI   (add fallback)
16 JMP      03
17 RETURN1  R1
18 RETURN0
```

Successful integer arithmetic skips its following metamethod fallback. LT consumes
or skips its following jump without a second dispatch. RETURN0 is unreachable after
the explicit return. The successful iteration therefore dispatches eight handlers:
LOADI bound, LT/conditional control, MULK, ADDI, MODK, ADD, ADDI and the backedge
JMP. Its temporary R3 also holds the bound, so LOADI 5000 executes on every check,
including the last failed check. This explains exactly 40,006 total dispatches:
three initializations, 5,000 * eight handlers, two final-check handlers and RETURN1.
Fallback instruction words still occupy code storage; they do not add five dispatches
per successful iteration.

The proposed [Kagari target](bytecode-optimization-plan.md#direct-target-instruction-shape-2026-10-27)
has eleven semantic instructions, four target scalar locations and 30,005 dynamic
operations with the entry guard retained (30,004 after proving that guard true).
Both engines can use one scratch register, write the final remainder directly to
state and update sum/i in place. Kagari's target saves a standalone bound load
through its constant comparison operand, and saves the unconditional backedge
through a conditional-backedge layout. This is two fewer dispatches per successful
iteration (six versus eight), about 25% fewer dynamic dispatches for this fixture.
The earlier placement-only target has seven per iteration / 35,005 total; current
unmodified Kagari still has eight / 40,008. These are representation/work-count
comparisons, not measured CPU instruction counts or elapsed speedups.

Storage density is a separate result. Lua's nineteen encoded words occupy 76 bytes
of instruction code, including the skipped fallbacks/dead return. Current Kagari
prepared instructions are 24 bytes each and scalar sites occupy additional tables.
If eleven target instructions retain that representation, their instruction array
alone is 264 bytes before sites/origins/layouts; the target has no finalized new
encoding. Do not claim that fewer operations automatically gives a smaller hot code
representation. Lua's wire chunk size, Kagari's serialized artifact size and their
complete runtime metadata are not being equated with these instruction arrays.

Primary confirmation: vendored PUC Lua 5.4.9
[lopcodes.h](https://www.lua.org/source/5.4/lopcodes.h.html),
[lvm.c](https://www.lua.org/source/5.4/lvm.c.html) arithmetic skip and comparison/jump
macros, and [ldump.c](https://www.lua.org/source/5.4/ldump.c.html). Kagari arithmetic
retains checked i32 commits; Lua has dynamic numeric/metamethod handling and different
integer overflow semantics. One handler in either engine is not an equal unit of
machine work. No runtime source, compiler pass or benchmark fixture was changed.

### Review order and preserved guarantees

1. Review the selected call/native owner first: carry current target and admission
   facts through one closed transition; avoid repeated selection/registry checks
   where the synchronous publication guard proves stability. Preserve candidate
   restrictions, current-call reload semantics, depth, cancellation, traces, reentry,
   native result rooting and suspended exact bodies. Never restore a stale callback
   captured only in the caller's old publication. The reusable semantic application
   and one admitted computation need distinct lifetimes; investigate frame-owned
   selection rather than mandatory GC records for every synchronous invocation.
2. Address native collection transactions within the existing joint GC/native
   collection plans, retaining one owner for checked handle access, type/mutation
   admission, accounting and fallible reserve/commit. Do not revive the rejected
   IP11 prototype or run a parallel concrete-container migration. Preserve custom
   key callbacks, SDK reentry gaps and replacement-without-growth behavior.
3. Review prepared enum/cell handlers and general enum representation separately.
   Option discrimination is required; one heap object/payload buffer plus separate
   boundary dispatches is a representation choice. A map-only sentinel shortcut
   would create incompatible semantics. A new GC alone does not remove these allocations.
4. Revisit scalar kernels/transport using new machine-code attribution after the
   common invocation boundary is understood. Keep source-width arithmetic and
   executable/inspection proofs; do not remove initialization checks or per-instruction
   observations merely because a source program typechecks.

Mature references checked for this diagnosis: Lua 5.4.9
[lvm.c](https://www.lua.org/source/5.4/lvm.c.html) and
[ldo.c](https://www.lua.org/source/5.4/ldo.c.html) keep ordinary script calls in the
interpreter with arguments already in stack slots and implement length/table
access as VM operations; [ltable.c](https://www.lua.org/source/5.4/ltable.c.html)
owns table storage. Rust 1.99.1
[Entry](https://doc.rust-lang.org/std/collections/hash_map/enum.Entry.html) carries an
occupied/vacant lookup into update. These support ownership/transport models, not
copying Lua pointers or unchecked integer semantics into Kagari. Kagari additionally
requires checked runtime handles, explicit native-frame observability, hot publication
selection and fallible allocation/commit order. Any implementation needs a frozen
same-machine candidate/baseline comparison plus focused ownership, reload, cancellation,
depth, alias/effect/trap-order and source-free validation; these analysis measurements
activate no new implementation phase and do not close existing performance gates.

Reproduce with the existing drivers, running each command sequentially:

```text
uv run python scripts/benchmark_lua.py --interpreter-only
uv run python scripts/benchmark_lua.py --source-forms
uv run python scripts/benchmark_lua.py --numeric-matrix
uv run python scripts/profile_lua_comparison.py entry arithmetic branches calls fibonacci arrays maps --executable target/release/kagari-lua-benchmark.exe --pdb target/release/kagari_lua_benchmark.pdb
uv run python scripts/profile_lua_comparison.py direct helper concrete_generic interface shared_generic capture_cell field native byte_state host_callback string_constants string_calls --source-forms --executable target/release/kagari-lua-benchmark.exe --pdb target/release/kagari_lua_benchmark.pdb
```

Raw reports were saved under target/lua-comparison at 20261027T034301Z,
20261027T034721Z-forms and 20261027T034746Z-numeric; profiles at
profile/windows-20261027T034806Z and profile/windows-20261027T035017Z. These are
disposable evidence, not required links or documentation dependencies. This report
retains the method, scope, results and source explanation. No production code or
test assertions changed; full workspace tests/feature/backend CI were not run.

## Program-owned instance scheduling, 2026-10-26

### Method and workload

Baseline `71e72b02be2b5113a0785207a28f00aaa8f4c628` retains whole-module demand
rebuild rounds; candidate `8244d7f7081f73a236e90945e1c348ba63d066cc` retains module
products and scans newly generated bodies. Both trees were clean during the paired
measurement. The [maintained harness](../crates/kagari-compiler/src/tests/source_programs/worklist.rs)
uses the same source construction, history helpers, sizes and measurement loop:
13 workloads, three repetitions each, 39 samples per revision. Tiny returns a
constant; chains of 2/4/8 imported generic identity functions forward one value;
fan-outs of 2/4/8 branches converge on one generic identity leaf. Retained variants
replace root calls with `0`, preserve imports and supply the original publication
demands. The checked context contributes 38 library modules.

Apple M1 Max, arm64, 32 GiB RAM, macOS 26.6.2 (`25G83`); rustc 1.99.1
(`58cf8f9c3`, LLVM 23.1.1). Workspace test profile (optimization level 1 plus debug
information), default source feature, default Cargo parallelism and target directory,
one test thread, default MIR options with optimization disabled. Each sample has
fresh source/checking/lowering state, with no compiler-output cache. Source checking
is timed separately; lowering excludes history preparation, encoding and Rust
compilation. Test-only event counters are enabled on both revisions.

Build the selected test executable, then run the executable reported by Cargo under
macOS `/usr/bin/time -l`; do not include Cargo/build memory in the measured process:

```text
cargo test -p kagari-compiler --lib tests::source_programs::worklist::bounded_worklist_measurements --no-run --message-format=json
```

At these revisions Cargo reported this executable; use the path from the build
output on other toolchains rather than assuming the hash is stable:

```text
/usr/bin/time -l target/debug/deps/kagari_compiler-a6a945148dfa7197 tests::source_programs::worklist::bounded_worklist_measurements --ignored --exact --nocapture --test-threads=1
```

Baseline build/run preceded candidate build/run, serially, in separate fresh
processes. OS caches were not flushed, and there was no randomized ordering,
warmup policy, thermal isolation or external-process isolation. The cold baseline
Rust build (3m46s) and cached candidate build (0.04s) are excluded from the results.
No script execution was measured.

### Paired observations

Times are median milliseconds, baseline -> candidate. Initialization and generation
counts are per measured lowering call, not history preparation. Candidate
verification/pass counts, instructions and encoded MIR sizes were unchanged.

| Workload | Retained | Checking ms | Lowering ms | Module initializations | Body generations | Verification / passes | Instructions | MIR bytes |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Tiny | No | 737.972 -> 441.807 | 708.277 -> 277.983 | 78 -> 39 | 2 -> 1 | 1 / 2 | 2 | 707124 |
| Chain 2 | No | 738.362 -> 442.492 | 1026.313 -> 279.114 | 123 -> 41 | 6 -> 3 | 1 / 3 | 8 | 715472 |
| Chain 2 | Yes | 838.734 -> 444.025 | 1126.773 -> 354.097 | 123 -> 41 | 5 -> 3 | 2 / 3 | 7 | 715085 |
| Chain 4 | No | 672.325 -> 452.301 | 1183.041 -> 280.251 | 215 -> 43 | 15 -> 5 | 1 / 5 | 14 | 723876 |
| Chain 4 | Yes | 499.006 -> 452.031 | 702.804 -> 353.736 | 129 -> 43 | 7 -> 5 | 2 / 3 | 13 | 723489 |
| Chain 8 | No | 539.428 -> 472.049 | 1575.359 -> 281.243 | 423 -> 47 | 45 -> 9 | 1 / 9 | 26 | 740684 |
| Chain 8 | Yes | 512.099 -> 471.717 | 683.674 -> 354.936 | 141 -> 47 | 11 -> 9 | 2 / 3 | 25 | 740297 |
| Fan-out 2 | No | 490.266 -> 455.354 | 587.186 -> 280.261 | 126 -> 42 | 8 -> 4 | 1 / 3 | 14 | 720627 |
| Fan-out 2 | Yes | 473.678 -> 447.656 | 659.466 -> 351.893 | 126 -> 42 | 6 -> 4 | 2 / 3 | 10 | 719321 |
| Fan-out 4 | No | 505.317 -> 479.966 | 581.730 -> 282.852 | 132 -> 44 | 12 -> 6 | 1 / 3 | 26 | 730951 |
| Fan-out 4 | Yes | 478.372 -> 457.578 | 649.049 -> 353.645 | 132 -> 44 | 8 -> 6 | 2 / 3 | 16 | 727807 |
| Fan-out 8 | No | 557.357 -> 532.938 | 583.771 -> 284.134 | 144 -> 48 | 20 -> 10 | 1 / 3 | 50 | 751599 |
| Fan-out 8 | Yes | 498.440 -> 477.927 | 648.565 -> 355.829 | 144 -> 48 | 12 -> 10 | 2 / 3 | 28 | 744779 |

Every candidate sample initializes exactly the final module count and generates
exactly the final unique body count. Required scheduling passes and the second
retained-demand verification remain; the result is not a one-pass compiler.
MIR byte lengths alone do not establish byte-for-byte equality; the separate
registration-order canonical-output test passed on the candidate.

| Whole-harness process metric | Baseline | Candidate |
| --- | ---: | ---: |
| Elapsed seconds | 83.91 | 44.82 |
| User / system CPU seconds | 71.35 / 0.13 | 44.71 / 0.09 |
| Maximum resident set size, bytes | 191954944 | 149422080 |
| Maximum resident set size, MiB | 183.062 | 142.500 |

RSS covers all 39 samples, source checking, history preparation, encoding, library
initialization and allocator retention. It is not lowering-only peak memory or
per-workload memory. This is macOS `time`'s maximum resident set size, not its
separate peak-memory-footprint statistic. No allocation-level ownership measurement
or large-project capacity result is claimed.

### Interpretation and remaining work

Counts establish elimination of module/body regeneration for these bounded shapes.
Checking times also changed substantially although that workload was identical;
this ordered pair therefore does not establish a precise speedup ratio. There is
no runtime-speed, universal complexity, startup-goal or release-profile claim.

A separate candidate-only attribution run on `8244d7f7` plus test instrumentation
and enabled scheduling assertions extracted uses exactly once per final body.
The same 39-sample harness passed. Candidate snapshot/evidence/optimization/verification
accounted for 71.827-138.042 ms of ordinary median lowering (443.787-611.444 ms),
and 166.424-274.421 ms of retained median lowering (514.743-769.502 ms). These are
ranges of workload medians, not matched baseline ratios. The timing excludes
coverage expansion and construction-closure work, which remain separately bounded
but unattributed. Large timing differences from the paired run reinforce the
noise limitation; no further optimization was added to chase a transcript target.

Five maintained scheduling cases assert once-only initialization, body generation
and scheduling extraction for chains, converging demand, recursion, late retained
bodies after bridge emission, and persistent shared-layout binders. The
[closed-table test](../crates/kagari-compiler/src/source/program/scheduler/tests.rs)
separately preserves table-only versus explicit-layout charging. Existing focused
compiler checks cover imported generics, private layout arguments, static/closure
obligations, foreign default origins, canonical output, interface budgets,
cancellation, update evidence and optimization failure isolation. Four selected
SDK checks cover shared helpers, source-free witness/layout rejection and current
operation selection after retained-demand publication. Local structure, formatting,
workspace all-target Clippy and diff checks passed. Full workspace tests and the
complete feature/backend/platform matrix remain CI-owned and unrun locally.

Two discarded inputs exposed [pre-existing shared-body admission defects](review.md#shared-body-admission-follow-ups),
reproduced at both `71e72b02` and the candidate. They were not repaired by R7.
Further frontend latency, R8 invalidation, artifact pruning and phase-specific
assembly/proof optimization remain separately scoped work.

## RL03 exact version identity and bounded retention, 2026-10-23

### Scope and before/after evidence

Baseline: `fd661985027dc2fe71b6d094f5e4ae670bd96bd1`, after RL02, initially clean.
Before changing production, a test-only probe in the existing native execution
owner prepared one referenced program, loaded source -> A -> B, and recorded
actual `Weak<BytecodeModule>` liveness. It passed with the original payload-backed
marker: the source allocation survived source drop and A drop, while A's remapped
allocation expired after A drop; the source expired only after B drop.

After: RL03 working-tree changes on that baseline. A private `Arc<VersionToken>`
replaces the original module-array marker, and `version_definitions` is removed.
`same_version` still compares root plus allocation identity. Normalization still
imports from the current immutable scope and preserves destination reference IDs,
exports, execution preparation and dependency facts. Independent equal programs
remain distinct. Native/AOT production consumers and executable leases are unchanged.

| Observation after dropping the named owners | Before: source alive | After: source alive | Intermediate A alive, before/after |
| --- | ---: | ---: | --- |
| Source preparation/context | 1 | 0 | 1 / 1 |
| A loaded handle/runtime | 1 | 0 | 0 / 0 |
| B native handle/loaded handle/runtime | 0 | 0 | 0 / 0 |

The maintained [normalization test](../crates/kagari-runtime/src/native/tests/native_execution/lifecycle.rs)
uses the existing two-instruction `Jump(1); Return(None)` native fixture and adds
one empty nominal `Record` layout. It observes every module Arc (one module in
this fixture), checks A/B-local IDs and portable paths, rejects a later ID through
A's immutable accepted prefix, and executes the installed B native fixture after
source and A release. The baseline used the same release schedule with source-live
assertions at the first two checkpoints; the current test requires source-dead.
No baseline flag or obsolete production implementation remains.

The [private token test](../crates/kagari-runtime/src/module/tests.rs) retains only
the token after old descriptor drop: original/current bytecode, descriptor and
execution-preparation Weaks expire. Before dropping that descriptor, its registry
entry is retired and executable admission/lease registration are rejected. A
same-token/different-root comparison remains unequal. Existing identity tests
retain independent-equal, foreign-runtime and intentionally shared reference-free
current-payload controls; referenced same-context normalization also shares current
records. These tests do not claim that a token grants execution authority.

### Environment and reproducible probes

Windows `x86_64-pc-windows-msvc`; Intel Core i7-10700F, 8 cores / 16 logical CPUs;
rustc 1.99.1 (`58cf8f9c3`, 2026-10-13), Cargo 1.99.1, LLVM 23.1.1. Default workspace
`target` and Cargo build parallelism. The ownership correctness tests used the
workspace test profile (dev optimization level 1, debug information). Allocation
probes used the workspace release profile and `execution-diagnostics`, one test
thread, and a single allocation observation per matrix cell. Existing build caches
were reused; the changed runtime/test targets were rebuilt. Cargo invocations were
serialized. There are **no elapsed-time, speedup, peak-memory or RSS claims**.

The maintained [private probe source](../crates/kagari-runtime/src/native/tests/native_execution/lifecycle/probes.rs)
reuses the [existing allocator counter](../crates/kagari-runtime/src/diagnostics/allocations.rs),
not a public instrumentation extension. It counts successful allocation/reallocation
requests, gross requested bytes and signed net requested bytes on the test thread.
It includes fixture holder/observer containers; it does not count allocator overhead,
other threads or executable mappings. Weak observers retain Arc allocation shells
until their own drop, even after payload destruction. Final balanced teardown is
asserted separately from payload Weak expiry and registry retirement.

```text
cargo test -p kagari-runtime --release --features execution-diagnostics --lib -- --ignored identity_domain_retention_probe type_only_provenance_retention_probe --nocapture --test-threads=1
```

This selects two ignored tests, not all diagnostics. The exact finite matrices are
3 counts (1/16/128) x 2 name policies x 2 installation lifetimes x 2 snapshot policies
= 24 domain cells, and 3 counts x 4 holder kinds = 12 provenance cells. All passed.
The existing native fixture supplies instructions, function metadata and ABI; the
local `scoped_program` helper supplies the nominal record. No source compiler or
large generated workload is required to construct these probe inputs.

### Identity-domain observations and decision

Inputs are independently verified before allocation accounting. The module identity
and instruction/layout shape stay fixed. Names are `Record000` repeatedly or the
fixed-width sequence `Record000` through `Record127`. Each cycle creates a runtime
from an empty prepared native installation, loads one input, optionally retains its
post-load table snapshot, and drops the loaded handle/runtime. Every remapped module
Weak expires at that cycle's end, even when snapshots or installation remain.

The retained arm keeps one `NativeInstallation`; the independent arm prepares and
drops a fresh installation each cycle. Table IDs are asserted equal across retained
cycles and unequal across independent cycles. This directly exercises the writer
shared by installation clones. SDK `new_session()` clones that same installation;
it is not an independent runtime identity domain. No source-session compiler-cache
measurement is claimed.

`cycles` below is net bytes after all runtimes are dropped; `snapshots` is net after
dropping the snapshot holder vector, while a retained installation is still alive.
The vector reserves 16*N bytes even in the released-snapshot control. All final net
counts after installation drop are **zero**. Request/gross totals include runtime
construction, installation preparation, linking and their teardown, but exclude
pre-verification of the retained input programs.

| N | Installation | Names | Keep snapshots | Requests | Gross bytes | Net after cycles | Net after snapshots |
| ---: | --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | Independent | Either | No | 523 | 49710 | 16 | 0 |
| 1 | Independent | Either | Yes | 523 | 49710 | 13116 | 0 |
| 1 | Retained | Either | No | 523 | 49710 | 14212 | 14196 |
| 1 | Retained | Either | Yes | 523 | 49710 | 14212 | 14196 |
| 16 | Independent | Repeated | No | 8353 | 795360 | 256 | 0 |
| 16 | Independent | Repeated | Yes | 8353 | 795360 | 209856 | 0 |
| 16 | Independent | Distinct | No | 8353 | 795360 | 256 | 0 |
| 16 | Independent | Distinct | Yes | 8353 | 795360 | 209856 | 0 |
| 16 | Retained | Repeated | No | 5893 | 394395 | 14452 | 14196 |
| 16 | Retained | Repeated | Yes | 5893 | 394395 | 14452 | 14196 |
| 16 | Retained | Distinct | No | 5910 | 399227 | 17108 | 16852 |
| 16 | Retained | Distinct | Yes | 6103 | 641910 | 219082 | 17600 |
| 128 | Independent | Repeated | No | 66817 | 6362880 | 2048 | 0 |
| 128 | Independent | Repeated | Yes | 66817 | 6362880 | 1678848 | 0 |
| 128 | Independent | Distinct | No | 66817 | 6362880 | 2048 | 0 |
| 128 | Independent | Distinct | Yes | 66817 | 6362880 | 1678848 | 0 |
| 128 | Retained | Repeated | No | 45989 | 2968043 | 16244 | 14196 |
| 128 | Retained | Repeated | Yes | 45989 | 2968043 | 16244 | 14196 |
| 128 | Retained | Distinct | No | 46123 | 3003003 | 35732 | 33684 |
| 128 | Retained | Distinct | Yes | 47770 | 6238526 | 2678714 | 38752 |

N=1 "Either" combines two identical observations, not an omitted name-policy cell.
The final loaded table contains 116 records / 41 symbols in every independent cell
and every retained/repeated cell. Retained/distinct counts are 116/41, 131/56 and
243/168 at N=1/16/128. The installation includes runtime builtin registration:
these are not counts of just the single script struct.

**Decision: retain the present installation-scoped writer for this lifetime.**
Repeated-name churn is flat after runtime/snapshot release (14196 bytes), distinct
names grow within the deliberately retained owner, and all measured allocations
balance after final owner release. There is no demonstrated process-global payload
leak. Hosts requiring unrelated identity lifetimes can prepare independent
installations; cloning sessions does not reset the writer. Retaining all distinct
prefixes is materially more costly (2678714 live requested bytes at 128), so this
result does not endorse unbounded snapshot/name retention. The
[common-owner follow-up](#common-owner-snapshot-backing-and-capacity-follow-up)
separately records private capacities, append-time COW isolation and backing-snapshot
Weak evidence. Runtime totals are not a substitute for that evidence or a measured peak.

### Type-only provenance observations and decision

One fixed `Record` layout and the two-instruction Unit function are loaded, then
replaced with independently prepared generations N times. Only the selected old
holder is retained per generation: none, `StructLayoutRef`, nominal `TypeArgument`,
or `InstalledNativeFunction`. Source metadata is outside allocation accounting;
all verified/loaded old owners other than the selected holder are released. The
final current version stays alive through both collection checkpoints.

After publication/collection, layout facts and nominal paths remain readable.
Retired layout owners fail executable admission. Native controls retain executable
leases and are actually invoked, returning Unit. Every old module Weak and every
native product-owner Weak expires after the final holder drop and collection;
all old registry entries are then absent. The native fixture controls ownership and
invocation, not OS executable-page accounting; the separate real AOT test below
covers library-owner lifetime.

`R/P` is old registered modules / live old bytecode payloads. Request/gross columns
are sampled at the first retirement checkpoint, before readability/native calls.
`holders` is net after those checks, holder drop and another collection. It includes
the current runtime, current code, store capacities and observer containers, not
just the old payload. Native invocation can warm execution storage, explaining why
its post-holder net differs from the non-native controls. Full runtime/container
teardown has zero net bytes in every cell.

| N | Holder | R/P | Requests | Gross bytes | Net after retirement | Net after holders |
| ---: | --- | --- | ---: | ---: | ---: | ---: |
| 1 | None | 0/0 | 1547 | 152079 | 22337 | 22281 |
| 1 | Layout | 0/1 | 1547 | 152079 | 25246 | 22281 |
| 1 | TypeArgument | 0/1 | 1559 | 152719 | 25702 | 22281 |
| 1 | Native | 1/1 | 1555 | 151764 | 25755 | 28641 |
| 16 | None | 0/0 | 13990 | 1172843 | 52689 | 51793 |
| 16 | Layout | 0/16 | 13990 | 1172843 | 99233 | 51793 |
| 16 | TypeArgument | 0/16 | 14182 | 1183083 | 106529 | 51793 |
| 16 | Native | 16/16 | 14173 | 1173483 | 107377 | 58513 |
| 128 | None | 0/0 | 106847 | 8817163 | 290577 | 283409 |
| 128 | Layout | 0/128 | 106847 | 8817163 | 662929 | 283409 |
| 128 | TypeArgument | 0/128 | 108383 | 8899083 | 721297 | 283409 |
| 128 | Native | 128/128 | 108271 | 8826123 | 728081 | 292817 |

Relative to None, Layout retains 2909 additional requested bytes per old generation
and TypeArgument 3365 for this small fixture: 372352 and 430720 bytes respectively
at 128. These differences include required type facts, program descriptors and
execution preparation, not exclusively instruction buffers. Payload Weak liveness,
not those byte differences or registry counters, proves the old code remains owned.

**Decision: separately scope a type/layout-provenance versus executable-payload
split.** A host keeping only nominal descriptors across reload can keep every old
program alive despite executable retirement; this is a concrete ownership mismatch
for that lifetime, demonstrated at 1/16/128 rather than inferred from pointer
identity. A future design must retain scoped IDs, immutable layout/associated-type
facts, accepted-prefix checks, old-object allocation compatibility and actual
callable/native dependency leases. This change implements no such split, no new
identity-domain writer and no cache/collector migration. The measured effect is not
a claim about production prevalence or larger-program byte savings.

### Focused validation and remaining limits

Seven selected runtime contracts passed: shared verified identity, referenced
source/A/B release, token/root/non-admission, scoped normalization/epochs, scoped
input snapshots, nominal origins without execution parents, and shared code with
private runtime instances. The two SDK tests
`rejects_equal_but_distinct_versions_before_compiling` and
`reload_reuses_candidate_code_and_keeps_old_native_entries_version_pinned` passed;
the latter asserts actual Native status, not fallback.

The exact AOT `installed_images_pin_versions_share_bindings_and_release_after_last_owner`
test first compiled but failed with `AotError("program not found")`: its default
`lld-link` was absent, and the PATH `link.exe` was Git's unrelated utility. It then
passed using the existing environment override, without SDK source changes:

```powershell
[Environment]::SetEnvironmentVariable('KAGARI_AOT_LINKER', 'C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC\14.51.36231\bin\Hostx64\x64\link.exe', 'Process')
cargo test -p kagari --features aot-build --lib installed_images_pin_versions_share_bindings_and_release_after_last_owner -- --nocapture
```

That test links/loads real images, asserts Native execution of pinned old/new
results, rejects foreign/mismatched installation, and checks library-owner Weaks.
The linker override was process-local, not persisted. The scoped IDE build request
timed out; Cargo supplied successful compilation. An early token-test fixture used
persistent static initialization, which legitimately retained an old recipe; it
was replaced by a stateless fixture to isolate token ownership, not by weakening
retention assertions. A transient test-edit syntax error was corrected before the
successful selected runs.

Local structure, formatting, workspace all-target Clippy, diagnostics-feature
runtime all-target Clippy and diff checks passed. Full-workspace tests and complete
feature/backend/platform matrices were not run and remain CI-only. The common-owner
snapshot/capacity/COW checks below also passed. R19/R14 and payload-backed version
identity are resolved; the remaining type-only retention finding stays in
[review](review.md). No type/layout-code split or broader R13 closure is claimed.

### Common-owner snapshot backing and capacity follow-up

This supplies common-owner evidence independently of runtime
`Weak<BytecodeModule>` observations. The two owner-local
[table tests](../crates/kagari-common/src/identity/table/tests.rs) passed serially
on the same RL03 working tree/environment, using release, default features/target/
build parallelism and existing caches. No production changes were needed.

```text
cargo test -p kagari-common --release --lib identity::table::tests::rl03_imported_snapshots_release_source_and_intermediate_backings -- --exact --test-threads=1
cargo test -p kagari-common --release --lib identity::table::tests::rl03_snapshot_capacity_matrix -- --exact --nocapture --test-threads=1
```

The first test checks source -> A -> B exact imports with different local indices,
omitted identities and immutable accepted prefixes. Direct `Weak<TableData>`
probes expire after source/intermediate writer and snapshot owners drop, while
B still resolves the imported path. Retained snapshot controls stay alive until
explicitly dropped. Probes are captured after final mutation or protected by strong
snapshots during append, avoiding `Arc::make_mut` Weak disassociation as false release.

The second test seeds `probe::nested::Type0`, then repeats Type0 or appends Type1
through TypeN. Each pre-operation snapshot is retained or dropped before interning.
All 12 cells passed, including final backing release. `V` lists current capacities
for modules/symbols/nodes/depths; `I` is the capacity of each symbol/node index map.
The other three map capacities are always 3. Records and symbols have equal counts.
`C/S` means COW copies / retained handles sharing current; unique old backings equal
C. Extra V/M are summed capacity-times-element-size estimates for unique old vector/
map containers alongside current, not per-handle sums.

| N | Names | Keep snapshots | Records | V | I | C/S | Extra V/M slot bytes |
| ---: | --- | --- | ---: | --- | ---: | --- | --- |
| 1 | Repeated | No | 3 | 4/4/4/8 | 3 | 0/0 | 0/0 |
| 1 | Repeated | Yes | 3 | 4/4/4/8 | 3 | 0/1 | 0/0 |
| 1 | Distinct | No | 4 | 4/4/4/8 | 7 | 0/0 | 0/0 |
| 1 | Distinct | Yes | 4 | 2/6/6/8 | 7 | 1/0 | 168/240 |
| 16 | Repeated | No | 3 | 4/4/4/8 | 3 | 0/0 | 0/0 |
| 16 | Repeated | Yes | 3 | 4/4/4/8 | 3 | 0/16 | 0/0 |
| 16 | Distinct | No | 19 | 4/32/32/32 | 28 | 0/0 | 0/0 |
| 16 | Distinct | Yes | 19 | 2/36/36/36 | 28 | 16/0 | 10310/12332 |
| 128 | Repeated | No | 3 | 4/4/4/8 | 3 | 0/0 | 0/0 |
| 128 | Repeated | Yes | 3 | 4/4/4/8 | 3 | 0/128 | 0/0 |
| 128 | Distinct | No | 131 | 4/256/256/256 | 224 | 0/0 | 0/0 |
| 128 | Distinct | Yes | 131 | 2/260/260/260 | 224 | 128/0 | 555414/559116 |

Repeated identity retention shares one backing; retaining every distinct prefix
keeps N extra containers, including 8512 old node records at N=128 versus 131 in
current. COW still shares existing module/string payload Arcs. These capacity
estimates exclude nested payloads, Arc headers, map bucket/control overhead and
probe bookkeeping; they are not allocator-request counts, RSS or append-time peak
measurements. Weak expiry establishes backing-value destruction, not OS page return
or runtime executable release. This supports bounded snapshot lifetimes, not a new
identity-domain model or an elapsed-time/performance claim.

## RL02 transition-owned parking, 2026-10-23

### Scope, baseline and ownership

Before: `b47f79edef87bc976cb9d2743748297c0f9732b7` (RL01), initially clean.
The before probe was compiled and executed with only `cfg(test)` instrumentation
and the parking fixture added, **before any production edit**. After: the RL02
working-tree changes on that revision. Allocation/layout, version identity,
scheduler policy and RL03 are unchanged. No ABI/schema identifier changes.

[`ExecutionStack::try_park_ready`](../crates/kagari-runtime/src/frame/owned.rs)
now accepts `&mut Option<ExecutionStack>` and consumes it only after a successful
runtime-owned proof. Decline retains the runnable activation; errors retain it
for admission, trace capture and cleanup. The VM attempts this at slice exhaustion
and returns Runnable only after commit, instead of querying and proving again at
slice exit. `park_waiting` uses the same ownership slot but is strict and proves
fresh post-poll state. It never overwrites readiness. Ready commit sets the parked
flag before invoking the wake route; both paths end the activation and retain the
runtime-owned frames/roots. `can_park` is now crate-private admission machinery;
the obsolete public consuming `park` entrypoint is removed.

The ownership discipline follows the already selected
[Tokio 1.48.0 task harness](https://github.com/tokio-rs/tokio/blob/tokio-1.48.0/tokio/src/runtime/task/harness.rs):
commit state before notification. This does not import Tokio scheduling or imply
that a stack borrow freezes Kagari's interior-mutable heap. The actual proof stays
with Kagari's collection guard and checked value traversal. There is no transferable
permit, cached object safety, alternate collector or persistent traversal scratch.

[`GcHeap::with_async_values`](../crates/kagari-runtime/src/gc/future.rs) owns one
visited set and temporary edge/pending buffers inside one collection-guarded
operation. Sequential frame root batches retain frame-order errors: an earlier
invalid graph still errors before a later frame can decline. Every root and emitted
Value is validated before object-identity deduplication, including differently
tagged aliases. [`collector::Visited`](../crates/kagari-runtime/src/gc/collector.rs)
is the shared marking loop used by both existing marking and async retention.
Native trace hooks remain inside the guard; panic and ignored reentry/mutation
rejection still quarantine rather than becoming a declined slice.

Future capture, queued-factory closure/argument, queued-Future, awaitable and
post-poll checks remain independent. No proof crosses submission, conversion,
script execution or host polling. Claimed capture roots and output validation are
unchanged. A later transition rechecks mutable aliases.

### Environment and frozen probe

Windows x86_64-pc-windows-msvc, Intel Core i7-10700F (8 cores / 16 logical CPUs),
rustc 1.99.1 (`58cf8f9c3`, 2026-10-13), Cargo 1.99.1, LLVM 23.1.1.
Default workspace features, target directory and build parallelism. Cargo work was
serialized; no other Cargo workload was launched during either timing run. A
process inspection before the baseline build found no running cargo/rustc process.
No CPU pinning or machine-wide idle guarantee was established.

The durable fixture is
[`frame/owned/tests.rs`](../crates/kagari-runtime/src/frame/owned/tests.rs),
`parking_transition_probe`. It builds a checked `hold` function with no parameters,
one Return instruction and A managed HeapObject slots or A scalar I32 slots. It
creates D real owned frames and installs roots directly in their managed windows
for this owner-local retention test; it does not claim these injected values are
typed script assignments. Other selected tests exercise ordinary source/decoded
execution and await admission.

Each managed graph contains eight objects: one Unit cell wrapped in seven
single-element tuples. Shared frames reuse one graph; disjoint frames retain
separate graphs; cyclic frames reuse a graph whose cell points back to the outer
tuple. Each frame repeats its graph root A times. Scalar controls assert zero
managed slots. Cases are `(D,A) = (1,1), (4,1), (4,3)` for each shape. Setup,
initial admission parking, graph construction and teardown are outside measurement.
The disjoint fixture's unused seed graph is not a retained frame root and is not
included in the traversal counts.

[`gc/retention_probe.rs`](../crates/kagari-runtime/src/gc/retention_probe.rs) is
compiled only for runtime unit tests. Its thread-local scope is filtered by heap
identity and reset on unwind. It counts proof entries, frame examinations, guarded
walk scopes, managed-root occurrences, object expansions and outgoing value-edge
visits. Distinct objects use complete generational HeapObjectIds; distinct edges
use `(source object ID, trace ordinal)`, not destination identity. The final cell's
Unit is a traced value edge in acyclic cases; the cycle replaces it with a heap
edge. Thus these fixtures each have eight outgoing **value** edges, not necessarily
eight heap-to-heap edges. Repeated work is total minus distinct. Root enumeration
is separate and is not deduplicated. These are not public or production statistics.

#### Reconstructing the before probe at `b47f79ed`

Use a separate checkout of `b47f79edef87bc976cb9d2743748297c0f9732b7`, leaving
its production parking and collector implementations unchanged. The following
recipe uses the retained source files linked above, not ignored logs or patches.
**Do not copy the entire current `frame/owned/tests.rs`: its other tests use
RL02-only APIs and will not compile against this baseline.**

1. Copy `gc/retention_probe.rs` in full to the same runtime source path. Create
   `frame/owned/tests.rs` containing **only** the six functions `module`, `graph`,
   `set_frame`, `fixture`, `transition`, and `parking_transition_probe` from the
   current file. Keep the last function's `#[test]` attribute. Do not copy any
   other test, the `TraceFailure` type or its implementation, or the waiting test
   module. Use exactly these imports for this subset:

   ```rust,ignore
   use crate::{
       Runtime,
       frame::ExecutionStack,
       gc::retention_probe::{measure, replace_cell},
       module::LoadedModule,
       session::owned::OwnedExecution,
       value::Value,
   };
   use kagari_abi::representation::ValueType;
   use kagari_bytecode::{
       instruction::BytecodeInstruction,
       module::{
           BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord,
           InstructionOrigin, RootSlotLayout,
       },
       program::{BytecodeProgram, ModuleRef},
   };
   use kagari_common::identity::metadata::DefinitionMetadata;
   use kagari_contract::ids::FunctionRef;
   use std::{hint::black_box, time::Instant};
   ```

2. Add the following test-only module declarations. The existing `mod owned;`
   in `frame.rs` stays unchanged; no other module wiring is needed.

   In `crates/kagari-runtime/src/gc.rs`:

   ```rust,ignore
   #[cfg(test)]
   pub(crate) mod retention_probe;
   ```

   In `crates/kagari-runtime/src/frame/owned.rs`:

   ```rust,ignore
   #[cfg(test)]
   mod tests;
   ```

   In both `frame/owned.rs` and `gc/future.rs`, add this module-scope import:

   ```rust,ignore
   #[cfg(test)]
   use crate::gc::retention_probe::{self, Event};
   ```

   Add only the instrumentation below to the baseline methods. Prefix **each**
   statement with `#[cfg(test)]`; do not copy the after implementation or its
   `with_async_values`/`Visited` machinery.

   | Baseline insertion point | Statement |
   | --- | --- |
   | `can_park`, before `self.validate_runtime(runtime)?` | `retention_probe::record(runtime.gc(), Event::Proof);` |
   | First statement in `can_park`'s `for frame in frames.iter()` body | `retention_probe::record(runtime.gc(), Event::Frame);` |
   | `validate_async_values` entry, before `ensure_no_native_borrow` | `retention_probe::record(self, Event::Walk);` |
   | Immediately after that Walk statement | `retention_probe::record(self, Event::Roots(values.len()));` |
   | First statement of its `collector::mark(pending, \|id, pending\| { ... })` callback | `retention_probe::record(self, Event::Object(id));` |
   | Immediately after `.trace(...)?`, before `valid_edges(&edges)` | `retention_probe::record(self, Event::Edges(id, edges.len()));` |

3. Adapt the two parking call sites in the copied fixture. At the end of `fixture`,
   replace the `try_park_ready` assertion with `stack.park(runtime).unwrap();`,
   retaining the following `owner` return. Replace the **whole** `transition`
   function, including its Option/consumption checks, with:

   ```rust,ignore
   fn transition(runtime: &Runtime, stack: ExecutionStack<'_>) {
       assert!(stack.can_park(runtime).unwrap());
       stack.park(runtime).unwrap();
   }
   ```

   Setup still parks only once, outside `measure`; the measured transition executes
   the baseline driver's successful query followed by slice-exit parking. There
   must be no `try_park_ready` or new-signature `park_waiting` reference in this
   baseline test module.

4. In `parking_transition_probe`, replace the **entire count-expectation block**
   from `let unique = ...` through the final `distinct_edges` assertion with the
   following before expectations. Keeping the after assertions would fail before
   entering the optional timing loop, even with `KAGARI_PARK_TIMING=1`.

   ```rust,ignore
   let scalar = shape == "scalar";
   let unique = if scalar {
       0
   } else if shape == "disjoint" {
       depth * 8
   } else {
       8
   };
   let expansions = if scalar { 0 } else { 2 * depth * 8 };
   assert_eq!(counts.proofs, 2);
   assert_eq!(counts.frames, 2 * depth);
   assert_eq!(counts.walks, 2 * depth);
   assert_eq!(counts.roots, if scalar { 0 } else { 2 * depth * aliases });
   assert_eq!(counts.objects, expansions);
   assert_eq!(counts.distinct_objects.len(), unique);
   assert_eq!(counts.edges, expansions);
   assert_eq!(counts.distinct_edges.len(), unique);
   ```

   These formulas cover every Before entry in the Actual work counts table below,
   including all scalar and alias controls. Each graph has eight traced value
   edges, including the acyclic cell's Unit, so baseline edge visits equal object
   expansions. Leave `fixture`'s managed-range assertion and the final zero-call-depth
   assertion unchanged. Also retain the four shapes, all three `(depth, aliases)`
   pairs, `measure` boundary, printing, warmup/sample loop and teardown unchanged.

Run the focused count command below first, then build release and run its reported
executable for timing. The normal current-tree probe retains its **after** assertions
and single atomic transition; these substitutions apply only to the separate
baseline reconstruction. No production baseline implementation is retained.

### Actual work counts

Counts used the workspace **test** profile (optimized + debuginfo), deliberately:
these are deterministic structural measurements, not debug-profile elapsed claims.
The same counts were also observed in the release timing binaries. Each row is one
successful measured transition. Objects and edges are `total/distinct`.

| Shape | D | A | Proofs before -> after | Frames before -> after | Walks before -> after | Roots before -> after | Objects before -> after | Value edges before -> after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Shared | 1 | 1 | 2 -> 1 | 2 -> 1 | 2 -> 1 | 2 -> 1 | 16/8 -> 8/8 | 16/8 -> 8/8 |
| Shared | 4 | 1 | 2 -> 1 | 8 -> 4 | 8 -> 1 | 8 -> 4 | 64/8 -> 8/8 | 64/8 -> 8/8 |
| Shared | 4 | 3 | 2 -> 1 | 8 -> 4 | 8 -> 1 | 24 -> 12 | 64/8 -> 8/8 | 64/8 -> 8/8 |
| Disjoint | 1 | 1 | 2 -> 1 | 2 -> 1 | 2 -> 1 | 2 -> 1 | 16/8 -> 8/8 | 16/8 -> 8/8 |
| Disjoint | 4 | 1 | 2 -> 1 | 8 -> 4 | 8 -> 1 | 8 -> 4 | 64/32 -> 32/32 | 64/32 -> 32/32 |
| Disjoint | 4 | 3 | 2 -> 1 | 8 -> 4 | 8 -> 1 | 24 -> 12 | 64/32 -> 32/32 | 64/32 -> 32/32 |
| Cyclic | 1 | 1 | 2 -> 1 | 2 -> 1 | 2 -> 1 | 2 -> 1 | 16/8 -> 8/8 | 16/8 -> 8/8 |
| Cyclic | 4 | 1 | 2 -> 1 | 8 -> 4 | 8 -> 1 | 8 -> 4 | 64/8 -> 8/8 | 64/8 -> 8/8 |
| Cyclic | 4 | 3 | 2 -> 1 | 8 -> 4 | 8 -> 1 | 24 -> 12 | 64/8 -> 8/8 | 64/8 -> 8/8 |
| Scalar | 1 | 1 | 2 -> 1 | 2 -> 1 | 2 -> 1 | 0 -> 0 | 0/0 -> 0/0 | 0/0 -> 0/0 |
| Scalar | 4 | 1 | 2 -> 1 | 8 -> 4 | 8 -> 1 | 0 -> 0 | 0/0 -> 0/0 | 0/0 -> 0/0 |
| Scalar | 4 | 3 | 2 -> 1 | 8 -> 4 | 8 -> 1 | 0 -> 0 | 0/0 -> 0/0 | 0/0 -> 0/0 |

For shared/cyclic D=4, repeated expansions fall from 56 to zero; for disjoint D=4,
from 32 to zero. Necessary root occurrences and disjoint-object expansion remain.
Scalar cases still enter the scoped guard but expand no objects and allocate no
visited entries. No heap-sized scratch survives the operation.

### Isolated elapsed observations

Both binaries were built using workspace **release** before sampling. Each row
uses one discarded warmup batch and five retained batches, each containing 1,000
transitions. Each activation is resumed before the timer; timing includes only the
baseline query-plus-park or the new attempt, including proof, commit, scope drop
and timer overhead. Counters are disabled during timing, although the test binary
still contains their disabled-scope checks. No host waker is registered. This is
not end-to-end `Vm::drive`, compilation, script execution, GC collection or
reclamation throughput. Those elapsed scopes were not measured.

All values below are milliseconds per 1,000 transitions. Cache state: baseline
release dependencies were compiled locally; each executable then ran in one process
with setup and warmup preceding retained samples. The after build reused unchanged
dependencies. Graph timings in the before run vary substantially; scheduling,
clock overhead, allocator/cache state and test-only hook overhead are not separated.
**No speedup ratio or general runtime speed claim is made.** The acceptance evidence
is the actual reduction in repeated proof/traversal work. The tiny scalar control
shows no measured regression in this sample, not a universal latency guarantee.

| Shape D/A | Version | Five retained samples | Median | Range |
| --- | --- | --- | ---: | --- |
| Shared 1/1 | Before | 4.2273, 8.6450, 9.1274, 7.5203, 14.4836 | 8.6450 | 4.2273-14.4836 |
| Shared 1/1 | After | 1.9297, 1.1154, 1.2102, 1.0996, 1.0925 | 1.1154 | 1.0925-1.9297 |
| Shared 4/1 | Before | 70.0007, 56.5872, 28.1886, 21.9994, 29.2790 | 29.2790 | 21.9994-70.0007 |
| Shared 4/1 | After | 1.2792, 1.2937, 1.2893, 1.4317, 1.3174 | 1.2937 | 1.2792-1.4317 |
| Shared 4/3 | Before | 29.4734, 20.4260, 21.3739, 20.8992, 16.9249 | 20.8992 | 16.9249-29.4734 |
| Shared 4/3 | After | 1.6245, 1.6238, 1.5674, 1.5656, 1.5845 | 1.5845 | 1.5656-1.6245 |
| Disjoint 1/1 | Before | 5.3638, 8.3148, 7.9726, 11.3092, 7.5422 | 7.9726 | 5.3638-11.3092 |
| Disjoint 1/1 | After | 1.2535, 1.1138, 1.1178, 1.1251, 1.1115 | 1.1178 | 1.1115-1.2535 |
| Disjoint 4/1 | Before | 15.5136, 27.7406, 31.4386, 40.6192, 38.9026 | 31.4386 | 15.5136-40.6192 |
| Disjoint 4/1 | After | 4.7334, 3.7112, 3.6949, 3.8413, 5.0454 | 3.8413 | 3.6949-5.0454 |
| Disjoint 4/3 | Before | 18.8588, 21.0377, 24.7772, 22.5867, 21.1399 | 21.1399 | 18.8588-24.7772 |
| Disjoint 4/3 | After | 5.3984, 6.4041, 4.0269, 4.2482, 4.0331 | 4.2482 | 4.0269-6.4041 |
| Cyclic 1/1 | Before | 5.1829, 7.9924, 6.4687, 9.5857, 6.9510 | 6.9510 | 5.1829-9.5857 |
| Cyclic 1/1 | After | 1.1212, 1.1230, 1.1693, 1.1991, 1.1876 | 1.1693 | 1.1212-1.1991 |
| Cyclic 4/1 | Before | 19.5601, 23.2380, 26.0052, 25.3708, 22.2098 | 23.2380 | 19.5601-26.0052 |
| Cyclic 4/1 | After | 2.4030, 1.7020, 1.3311, 1.2859, 1.4039 | 1.4039 | 1.2859-2.4030 |
| Cyclic 4/3 | Before | 18.4584, 12.8577, 15.7150, 14.0743, 14.4120 | 14.4120 | 12.8577-18.4584 |
| Cyclic 4/3 | After | 1.5783, 1.6607, 1.6290, 1.6013, 1.5977 | 1.6013 | 1.5783-1.6607 |
| Scalar 1/1 | Before | 0.3224, 0.2216, 0.2211, 0.2396, 0.3063 | 0.2396 | 0.2211-0.3224 |
| Scalar 1/1 | After | 0.1254, 0.1441, 0.1234, 0.1246, 0.1227 | 0.1246 | 0.1227-0.1441 |
| Scalar 4/1 | Before | 0.7722, 0.7825, 0.7721, 0.7652, 0.7767 | 0.7722 | 0.7652-0.7825 |
| Scalar 4/1 | After | 0.1682, 0.1695, 0.1836, 0.1696, 0.1695 | 0.1695 | 0.1682-0.1836 |
| Scalar 4/3 | Before | 0.9094, 0.9046, 0.8802, 0.8924, 0.9078 | 0.9046 | 0.8802-0.9094 |
| Scalar 4/3 | After | 0.1734, 0.1689, 0.1688, 0.1695, 0.1695 | 0.1695 | 0.1688-0.1734 |

Reproduction commands (Git Bash; use the executable reported by the no-run build):

```text
cargo test -p kagari-runtime --lib frame::owned::tests::parking_transition_probe -- --exact --nocapture --test-threads=1
cargo test -p kagari-runtime --release --lib frame::owned::tests::parking_transition_probe --no-run
KAGARI_PARK_TIMING=1 target/release/deps/kagari_runtime-311c7eab2e0fa4bc.exe frame::owned::tests::parking_transition_probe --exact --nocapture --test-threads=1
```

### Focused correctness and limits

The selected runtime run passed 12 tests: four `frame::owned::tests` cases,
`waiting_commit_rechecks_graph_after_poll_mutation`, `async_retention_graph_contract`,
`pending_commit_preserves_completion_and_cancellation_notifications`,
`initializer_frame_drop_poison_survives_termination_and_releases_recipe_edges`,
the two `execution_state` collection panic/reentry tests,
`collection_callbacks_cannot_replace_roots_even_if_they_ignore_rejection`, and
`owned_retirement_drains_all_parked_roots_after_payload_drop_fault`.

The selected VM run passed these six exact tests: `tests::owned_drive::async_owned_drive_contract`,
`tests::native_wait::async_wait_live_storage_contract`,
`tests::native_wait::async_native_completion_contract`,
`tests::native_wait::async_native_completion_contract_retirement_and_admission`,
`tests::native_wait::async_native_completion_contract_cleanup_quarantine`, and
`executor::driver::tests::driver_admits_resource_errors_before_frames_unwind`.
This selection covers the changed transition plus directly affected cancellation,
root, trace and cleanup contracts; it is not a whole-crate or workspace sweep.

The new deterministic notification test uses the existing native completion
fixture, publishing completion/cancellation after a real Pending poll and before
waiting commit, plus a no-notification control. A separate native-poll fixture
injects a hidden invalid edge during polling and verifies a fresh commit error,
retained diagnostic frames and cancellation cleanup. Graph injection is test-only;
ordinary mutation admission is not weakened. Other cases verify retry after
decline, error retention, per-frame error order, alias mutation between transitions,
trace-hook panic/quarantine and differently tagged roots after deduplication.

The scoped IDE build request timed out and was not repeated. Focused Cargo builds
and tests succeeded. Initial structure/format checks identified one qualified
import and one formatting issue, both corrected. Final structure (1,509 Rust files,
zero violations), formatting, workspace/all-target Clippy and diff checks passed.
Tracked files retain CRLF working-tree line endings. Full workspace tests and
feature/backend/platform matrices remain CI-only;
no complete acceptance or BO/IP performance claim follows from these local results.

## RL01 scalar storage-class allocation, 2026-10-23

### Scope, environment and interpretation

Before: `289bee0b82325d85449db30bbd0bb41f9f964dd6`, clean tree before adding
only the temporary test probe. After: RL01 changes to
[`allocation.rs`](../crates/kagari-runtime/src/module/execution/allocation.rs) and
[`layout.rs`](../crates/kagari-runtime/src/module/execution/layout.rs) on that base.
The frozen probe ran successfully before changing production code, then ran with
unchanged workloads after implementation. No parking, version-token, solver,
managed coalescing or BO/IP tuning is included.

Windows 10 Pro 10.0.19045, Intel Core i7-10700F (8 cores / 16 logical CPUs),
67,017,248 KiB visible memory; native x86_64-pc-windows-msvc, rustc 1.99.1
(`58cf8f9c3`, 2026-10-13), Cargo 1.99.1, LLVM 23.1.1. Workspace **test** profile
(optimized + debuginfo), default features, target directory and Cargo parallelism;
test harness serialized with `--test-threads=1`. These are deterministic work and
bank-capacity counts, **not elapsed comparisons**. No warmup/sample timing series,
release-speed claim or compilation/execution throughput measurement was made.
Build caches were reused; after an interrupted post-change build and subsequent
MSVC LNK2019/LNK1120 anonymous LLVM-symbol errors, `cargo clean -p kagari-runtime`
was followed by a successful rebuild/probe. An unrelated Beacon Cargo build was
observed on the machine; its processes were not changed. This does not alter the
logical counts and is another reason not to interpret build durations as timings.

The mechanism follows the storage/interference-class separation selected in RL01
(Cooper and Torczon, *Engineering a Compiler*, second edition, chapter 13): these
are VM backing slots, not CPU registers or spilling. Layout's existing
`scalar_type` remains the single physical classification, including Unit and Char.
A managed capture cell with a scalar content type still occupies managed storage.
`SlotColor::Managed` consumes no scalar color; dense scalar colors directly index
the scalar bank, removing layout's obsolete color-to-bank `BTreeMap`. All logical
ranges and the existing CFG/debug solver remain intact.

### Frozen workloads and measurements

Each function has six logical slots, no parameters, and nine instructions:
`load 0; load 1; load 2; move 1 <- 1; load 3; load 4; move 3 <- 3; load 5; return`.
`load` is `LoadConst` with constant ordinal zero; these are owner-local allocation
fixtures, not executed or presented as independently verified programs. Mixed
representations are `[I32, HeapObject, I32, HeapObject, I32, HeapObject]`.
Scalar-only and managed-only replace all six representations with I32 and
HeapObject respectively. Debug uses mixed storage plus slot 0 availability
`[1, 6)`; fallback uses mixed storage and initial work `4_000_000`.

Common closed allocation ranges before and after are
`[(0,0), (1,3), (2,2), (4,6), (5,5), (7,7)]`. Debug extends the first to `(0,5)`;
fallback has no reuse ranges. Thus slot 0's debug use and slot 4's definition at
PC 5 still interfere. All managed locations remain distinct.

| Control | Scalar slots before -> after | Managed slots before -> after | Preparation work delta before -> after |
| --- | ---: | ---: | ---: |
| Mixed | 2 -> 1 | 3 -> 3 | 34 -> 32 |
| All scalar | 2 -> 2 | 0 -> 0 | 34 -> 40 |
| All managed | 0 -> 0 | 6 -> 6 | 34 -> 27 |
| Debug extended | 3 -> 2 | 3 -> 3 | 55 -> 50 |
| Budget fallback | 3 -> 3 | 3 -> 3 | 1 -> 7 |

Work is the allocator's charged-unit counter, not CPU instructions or a complete
count of allocator traffic. Six new classification units are charged per normal
control; excluding managed intervals reduces active/free processing in mixed and
managed controls. The all-scalar control therefore has a real six-unit preparation
increase, not a claimed speedup. Fallback also charges the mandatory linear output
classification even after optimization exhaustion; the counter saturates safely.
The explicit color enum carries a discriminant in addition to the old usize
color; this is temporary preparation storage, not retained frame memory. Its
full `size_of::<SlotColor>()` per logical slot is included in the bounded scratch
admission calculation; peak preparation allocator traffic was not measured. No heap-wide scratch or persistent cache was introduced.

For each condition, start with a fresh `ExecutionValues`, allocate either one or
eight simultaneous windows using the prepared layout and an empty loaded module,
then release in reverse order. No arguments or values are populated. "Requested"
below means the logical backing requirement at peak bank length, not cumulative
allocation requests: scalar bytes = `payloads.len()*8 + initialized.len()`, managed
bytes = `values.len()*16`. Retained bytes use the corresponding `Vec::capacity()`;
capacities were checked unchanged after release, with all three lengths zero.
Measured element widths are u64=8, bool=1, Value=16 bytes. Tuples list
`(payload capacity, initialization capacity, managed capacity)` in elements.

| Control | Windows | Bank capacities before -> after | Scalar requested / retained bytes before -> after | Managed requested / retained bytes before -> after |
| --- | ---: | --- | --- | --- |
| Mixed | 1 | (4,8,4) -> (4,8,4) | 18 / 40 -> 9 / 40 | 48 / 64 -> 48 / 64 |
| Mixed | 8 | (16,16,32) -> (8,8,32) | 144 / 144 -> 72 / 72 | 384 / 512 -> 384 / 512 |
| All scalar | 1 | (4,8,0) -> (4,8,0) | 18 / 40 -> 18 / 40 | 0 / 0 -> 0 / 0 |
| All scalar | 8 | (16,16,0) -> (16,16,0) | 144 / 144 -> 144 / 144 | 0 / 0 -> 0 / 0 |
| All managed | 1 | (0,0,6) -> (0,0,6) | 0 / 0 -> 0 / 0 | 96 / 96 -> 96 / 96 |
| All managed | 8 | (0,0,48) -> (0,0,48) | 0 / 0 -> 0 / 0 | 768 / 768 -> 768 / 768 |
| Debug extended | 1 | (4,8,4) -> (4,8,4) | 27 / 40 -> 18 / 40 | 48 / 64 -> 48 / 64 |
| Debug extended | 8 | (32,32,32) -> (16,16,32) | 216 / 288 -> 144 / 144 | 384 / 512 -> 384 / 512 |
| Budget fallback | 1 | (4,8,4) -> (4,8,4) | 27 / 40 -> 27 / 40 | 48 / 64 -> 48 / 64 |
| Budget fallback | 8 | (32,32,32) -> (32,32,32) | 216 / 288 -> 216 / 288 | 384 / 512 -> 384 / 512 |

One-window minimum capacities mask the scalar slot reduction; eight-window banks
show a retained reduction. Managed retention is unchanged. This excludes window
records, layout/guard metadata, module/code ownership, referenced heap objects,
allocator overhead and RSS. Total frame/process memory, execution speed and all
function shapes are not established by these numbers. Existing public-frame and
scalar-window/root tests provide behavioral checks separately from these counts.
Full feature/backend/platform tests remain CI scope.

### Reproduction source recipe

The temporary probe was removed from production test sources after measurement.
To replay, add the following ignored test inside `layout.rs::tests`, using that
file's `storage_control`, `storage_module` and module-scope test imports. On the
before revision, copy those two fixture helpers and their test imports only from
the RL01 revision; do not copy the production changes or post-change assertions.
The helpers preserve the workload above, including the exclusive debug-range end.
This recipe uses both revisions' existing APIs and does not depend on ignored logs.

```rust
#[test]
#[ignore = "manual RL01 bank-capacity and preparation-work probe; no timing"]
fn rl01_storage_probe() {
    let mut runtime = Runtime::default();
    let module = storage_module(&mut runtime);
    let mixed = || vec![
        ValueType::I32, ValueType::HeapObject, ValueType::I32,
        ValueType::HeapObject, ValueType::I32, ValueType::HeapObject,
    ];
    for (name, types, debug, initial_work) in [
        ("mixed", mixed(), false, 0),
        ("scalar", vec![ValueType::I32; 6], false, 0),
        ("managed", vec![ValueType::HeapObject; 6], false, 0),
        ("debug", mixed(), true, 0),
        ("fallback", mixed(), false, 4_000_000),
    ] {
        let function = storage_control(types, debug);
        let mut work = initial_work;
        let layout = Arc::new(FrameLayout::prepare(&function, &mut work));
        let mut allocation_work = initial_work;
        let allocation = SlotAllocation::prepare(&function, &mut allocation_work);
        println!("RL01 {name}: scalar={} managed={} work_delta={} ranges={:?}",
            layout.scalar_count, layout.managed_count,
            work - initial_work, allocation.ranges);
        for depth in [1, 8] {
            let mut values = ExecutionValues::default();
            let mut frames = Vec::new();
            for _ in 0..depth {
                frames.push(values.allocate(
                    6, &[], &FrameArguments::plain(&[]),
                    FrameMetadata {
                        program: module.clone(), environment: None, invocation: None,
                    },
                    Some(layout.clone()),
                ).unwrap());
            }
            let requested = (
                values.payloads.len() * size_of::<u64>() + values.initialized.len(),
                values.values.len() * size_of::<Value>(),
            );
            let capacities = (
                values.payloads.capacity(), values.initialized.capacity(),
                values.values.capacity(),
            );
            let retained = (
                capacities.0 * size_of::<u64>() + capacities.1,
                capacities.2 * size_of::<Value>(),
            );
            for frame in frames.into_iter().rev() {
                values.release(frame).unwrap();
            }
            assert!(values.payloads.is_empty() && values.initialized.is_empty()
                && values.values.is_empty());
            assert_eq!(capacities, (
                values.payloads.capacity(), values.initialized.capacity(),
                values.values.capacity(),
            ));
            println!("RL01 {name} depth={depth}: requested={requested:?} \
                capacity={capacities:?} retained_after_release={retained:?} \
                value_size={}", size_of::<Value>());
        }
    }
}
```

Run the same command before and after, serially:

```text
cargo test -p kagari-runtime --lib rl01_storage_probe -- --ignored --exact module::execution::layout::tests::rl01_storage_probe --nocapture --test-threads=1
```

Only the probe executes (one passing test per successful invocation). Permanent
coverage is at `storage_classes_determine_actual_bank_capacity_without_losing_ranges`
and `unused_unit_char_and_managed_capture_slots_keep_their_physical_class` in the
same layout test owner. The first checks actual execution-bank lengths, managed
separation, mixed/all-scalar/all-managed controls, closed debug endpoints and
fallback. The second covers empty/unused slots, Unit, Char, initialized parameter
ranges and managed capture-cell physical storage despite scalar content semantics.

## Bounded analysis and MIR compaction, 2026-10-21

R16-R18 local algorithm acceptance is complete. Current
[ownership contracts](architecture.md#bounded-flow-debug-availability-and-mir-deletion)
replace repeated debug-range filtering, coupled full-graph flow sweeps and repeated
MIR deletion shifts. These results do not resolve CS's 500 ms goal or the outstanding
BO04/IP12 lifecycle gates. Full feature/backend/platform coverage remains CI-owned.

### Environment and method

Native x86_64-pc-windows-msvc on Windows 11 Professional 10.0.26300, Intel i9-12900K
(16 cores / 24 logical CPUs), about 64 GiB RAM; rustc 1.99.1
(`58cf8f9c3`, 2026-10-13), Cargo 1.99.1, LLVM 23.1.1. Workspace release profile,
default features/target/Cargo parallelism, no custom flags, affinity or frequency
control. Builds completed before timing. Caches were warm; these are not cold
process, browser or deployed network measurements.

R17/R16 use `ea5c1386`; R18 uses the same production compaction implementation in
the AE03 working tree, SHA-256
`7738b27a46dac79b19361d210dc45cf49062b8e7a22f1cb49c64f8a40da3234f`.
Old algorithms are source-inspected test-only baselines compiled alongside the new
algorithms, not an old-checkout end-to-end benchmark. The
[tracked record](measurements/analysis-efficiency-2026-10-21.json) contains every
sample/warmup, environment, methods and probe sources; ignored local logs are not
required. Temporary probes were removed after measurement.

R17/R16 use one warmup and seven measured batches per condition, alternating algorithm
order. Batch counts: ranges 1/16/256/4096 use 16384/2048/64/2 operations; blocks
1/16/128/1024 use 4096/256/8/1. Construction/cloning of fixtures, reachability, resets
and assertions are outside timing; algorithm-owned scratch remains inside. R17
includes index creation, 2D reverse-PC queries and destruction, with disjoint
length-one ranges at even PCs and maximum overlap one. Outputs are checked exactly.
R16 uses reachable chains with one bitset word and a terminal parameter use, in
forward/permuted layouts without debug ranges; final facts and actual visits match.
Its timed old solver includes original work/cancellation charges, not the uncharged
test oracle. R17 uses counting callbacks, not the entire verifier's cancellation
plumbing. Larger overlapping ranges, dense joins, wider states and cancellation
latency are not timed here.

R18's replay retains one explicit warmup and five samples per size/algorithm, old
then new, one affected block with half dead prefix and half retained suffix. Timers
include old sorting/removal or production compaction, charges and tail cleanup;
fixture clones/deletion-vector creation/assertions are outside timing. Earlier
samples are retained separately and are not pooled with the replay. The smallest
measurements are resolution-sensitive; all ranges/outliers are retained.

### Bounded operation results

Times are microseconds per operation, median [minimum, maximum].

| R17 ranges | Scan | Index construction + queries |
| ---: | ---: | ---: |
| 1 (control) | 0.0035 [0.0029, 0.0037] | 0.0458 [0.0454, 0.0492] |
| 16 | 0.321 [0.320, 0.347] | 0.731 [0.726, 0.746] |
| 256 | 48.972 [47.781, 50.736] | 20.595 [18.922, 33.389] |
| 4096 | 10921.600 [9759.050, 16135.200] | 689.150 [662.450, 1023.950] |

For 16/256/4096 ranges, old predicate inspections are 512/131072/33554432;
new build charges are 177/5700/139887 and query charges 181/5893/143365, including
node visits and emitted records. Scratch is 384/6144/98304 bytes. Indexing improves
the larger sparse cases but **regresses the tiny controls**, including 16 ranges:
construction/allocation dominates there. No small-input specialization is introduced
as another unplanned tuning round. Actual caller benefit depends on metadata size
and query frequency; do not infer a whole-compiler speedup.

| R16 blocks / layout | Original coupled solver | Directional worklists |
| --- | ---: | ---: |
| 1 (control) | 0.141 [0.131, 0.216] | 0.282 [0.270, 0.444] |
| 16 / forward | 18.267 [17.867, 20.237] | 1.483 [1.424, 1.553] |
| 16 / permuted | 17.428 [16.468, 28.646] | 1.593 [1.408, 2.781] |
| 128 / forward | 1078.037 [976.962, 1168.500] | 10.675 [9.700, 22.163] |
| 128 / permuted | 1023.763 [977.925, 1061.888] | 10.325 [9.863, 11.650] |
| 1024 / forward | 69353.900 [67498.300, 74595.100] | 85.200 [77.600, 183.600] |
| 1024 / permuted | 66044.100 [64428.500, 69121.900] | 80.000 [74.100, 90.200] |

At 1024 blocks, old forward/permuted combined visits are 1049600/1048576 versus
2048 new directional visits. Charged work is 7345150/7337984 versus 21499, excluding
fixture reachability. The one-block control regresses due to explicit ordering/queue
setup. Chains demonstrate removed scheduling work, not a universal linear solver
bound. Correctness additionally compares all 512 directed three-node edge sets,
including unreachable predecessors and no-exit cycles; those are not timing probes.

| R18 block entries | Repeated removal | Stable compaction |
| ---: | ---: | ---: |
| 16 (control) | 0.2 [0.2, 0.3] | 0.2 [0.1, 0.2] |
| 256 | 17.2 [17.2, 17.3] | 2.0 [1.9, 2.0] |
| 4096 | 10437.9 [10128.4, 10530.5] | 33.6 [31.8, 36.9] |

For these sizes, old/new logical column relocations are 192/48, 49152/768 and
12582912/12288. These are source-derived element counts, not hardware counters.
New charges 96/1536/24576 exceed old 8/128/2048 because previously uncharged retained
visits/moves/cleanup are now counted. The finder already groups descending decisions;
consuming those groups needs O(D + N) work and O(1) added scratch, with N entries in
affected blocks. No eligibility or pass-order change is included.

### Current SDK paths and correctness limits

Current-only native release SDK measurements use one warmup and five samples per
route, with fresh engines, separately timed setup and a fixed source containing
debug locals, branches and dead pure temporaries. Each product has 2 functions,
10 debug ranges and 2 branch instructions. All 18 executions (direct, staged-source
attribution and artifact routes, including warmups) return exact `Value::I32(42)`.
No JIT/AOT is requested. Portable MIR is exported by default artifact options and
validated with native-enabled preparation; direct preparation does not retain it.

| SDK phase | Milliseconds, median [min, max] |
| --- | ---: |
| Direct `compile_to_program` | 572.886 [566.497, 584.976] |
| Staged source checking | 127.440 [124.257, 130.590] |
| Staged lowering/preparation | 454.498 [439.459, 464.156] |
| `compile_to_artifact` | 593.022 [590.606, 756.243] |
| Artifact serialization | 5.186 [4.865, 5.363] |
| Artifact decode | 4.076 [4.007, 5.238] |
| Artifact preparation | 209.492 [201.751, 210.075] |
| Direct runtime linking / first execution | 5.194 [4.980, 5.422] / 0.0184 [0.0157, 0.0199] |
| Artifact runtime linking / first execution | 4.869 [4.827, 5.859] / 0.0173 [0.0160, 0.0189] |

Engine setup medians are 427.452/425.178/425.347 ms for direct/staged/artifact, outside
these source timers; runtime construction is separate in raw samples. Build time
55.13 s is excluded. Three earlier attempts detected competing Cargo activity and
were rejected before accepted sampling; the accepted 20.13 s run found no competing
build/test across 182 monitor polls and stable Rust/Cargo input hashes. Do not pool
attribution medians or compare these numbers to the historical Mac CS timings.
These verify current paths, not matched end-to-end before/after improvement.

Selected local acceptance passed availability (4), flow solver (5), async verifier
(3), allocation (4), lexical debug ranges (1), malformed debug metadata (1), ordinary
entry/join initialization (1), source-free predicate branches (1), MIR compaction
(5), compiler passes (13), debugger optimized/unoptimized plus artifact roundtrip
(1), and independent source-disabled artifact execution (1). The last test initially
failed because its ignored generated fixture was absent; running
`cargo run -p kagari --example regenerate_feature_artifact` and rerunning only that
test passed. Structure, formatting, workspace all-target Clippy with warnings denied
and diff checks passed. No in-scope carried error remains. Full workspace suites,
backend/platform matrices and browser acceptance were not run; these local results
do not establish Lua parity, old lifecycle gates or deployed latency.

## Inline module AST reuse, 2026-10-21

Baseline `e7135c16` and candidate `3400f651` compare complete HIR declarations
queries before/after SA1. The [HIR ownership contract](architecture/hir.md#query-work-reuse-and-publication)
now shares physical text/line indexes and one parse across logical module views,
while retaining independent lowering arenas and source identities. Source acquisition,
signature/body checking, MIR/codegen and execution are outside this measurement.
Graph construction and declaration resolution remain inside it; this is not a
parser-only timing and does not isolate SA9's earlier graph improvements.

### Workloads and method

Use SourceDatabase with a single `inline.kgr` input and AnalysisDatabase without
native registrations. The source fixtures are:

- Control: `// 中文😀\r\nfn entry() {}` (28 UTF-8 bytes, one logical module).
- Siblings: concatenate `mod m{i} { pub fn f{i}() {} }\r\n` for i=0..31
  (908 bytes, 33 logical modules including the physical root).
- Nested: concatenate `mod m{i} { ` for i=0..7, append `pub fn leaf() {}`,
  then eight ` }` suffixes (104 bytes, nine logical modules).

Here `\r\n` denotes actual CRLF. All declarations queries assert the module count
and empty diagnostics. Fresh work constructs a new AnalysisDatabase and publishes
its declarations. Unchanged work reuses that database/input while retaining the
first snapshot. Edit work creates and retains a fresh old snapshot outside timing,
replaces each `() {}` with `() { val changed = 1; }` in an overlay outside timing,
and measures publication of the edited declarations. Every measured result is kept
alive until after timing/counting; old snapshots remain retained. Input creation,
compilation and result destruction are excluded.

Build the same temporary integration probe with
`cargo test -p kagari-hir --test sa1_probe --no-run --message-format=json`, preserve
each revision's executable, then run it with
`--ignored --nocapture --test-threads=1`. Per workload/process: one warmup, seven
elapsed-time samples and a separate allocation pass. Edit samples rebuild their
old setup outside each measurement. A forwarding System allocator with thread-local
counters records successful alloc/realloc requests, gross requested bytes and
requested bytes allocated minus freed during the operation. Net bytes exclude
pre-existing retained input/old snapshots; they are not RSS or total resident size.
A small negative unchanged-query net value reflects release of previously allocated
cache storage. Timing runs disable allocation counters.

Environment: Windows x86_64 MSVC, Intel i7-10700F (8 cores/16 logical processors),
rustc 1.99.1 (58cf8f9c3, 2026-10-13), LLVM 23.1.1. Workspace test profile
opt-level 1 with debug info, default HIR features/dev dependencies, default target
directory and default build parallelism. Compilation/cache rebuild time is excluded.
Workload caches are warmed as above; process startup, system load, frequency and
affinity are uncontrolled. No Cargo/rustc process was observed before/after each
final pair; this does not exclude other background load.

Initial three paired processes showed broad timing variance, including a 22%
unchanged-nested median increase. Before collecting final data, the follow-up was
fixed at ten pairs with alternating baseline/candidate execution order. The tables
use all 70 timing samples and ten allocation samples per revision/workload in that
follow-up; no timed outliers were removed. The earlier exploration is excluded.
Temporary probes/logs are disposable; the method, fixture recipe and results here
are the retained evidence.

### Complete declaration query observations

Times are microseconds: median followed by the complete observed sample range.
Wide ranges and these small fixtures preclude a general compiler speed claim.

| Workload | Baseline median [min, max] | Candidate median [min, max] |
| --- | ---: | ---: |
| control/fresh | 38.10 [31.1, 155.3] | 38.85 [31.4, 99.5] |
| control/unchanged | 13.55 [10.7, 114.6] | 13.40 [10.7, 49.5] |
| control/retained_edit | 46.85 [37.6, 77.4] | 46.15 [38.5, 145.8] |
| siblings/fresh | 2,042.00 [1,790.3, 9,242.9] | 1,300.10 [1,079.2, 6,281.7] |
| siblings/unchanged | 5,050.50 [4,237.3, 63,213.5] | 4,721.25 [3,902.9, 20,914.7] |
| siblings/retained_edit | 2,072.65 [1,751.8, 8,805.3] | 1,266.60 [1,048.0, 14,274.1] |
| nested/fresh | 404.75 [332.5, 1,029.8] | 336.45 [225.4, 1,483.6] |
| nested/unchanged | 423.55 [379.8, 2,335.0] | 463.00 [383.0, 8,552.7] |
| nested/retained_edit | 320.50 [265.2, 2,073.3] | 205.50 [160.5, 5,300.6] |

Allocation columns show the complete observed range, baseline to candidate.

| Workload | Requests | Gross requested bytes | Net requested bytes |
| --- | ---: | ---: | ---: |
| control/fresh | 180 → 184 | 35,267 → 30,707 | 17,191 → 17,287 |
| control/unchanged | 56 → 59 | 21,087 → 16,487 | 0 → 0 |
| control/retained_edit | 234 → 238 | 39,499 → 34,939 | 18,232 → 18,328 |
| siblings/fresh | 9,523–9,524 → 6,528–6,529 | 1,775,252–1,775,620 → 1,528,476–1,528,844 | 1,036,359–1,036,727 → 905,367–905,735 |
| siblings/unchanged | 21,657 → 21,509 | 9,445,638 → 9,337,078 | -368–0 → -368–0 |
| siblings/retained_edit | 10,194–10,195 → 8,223–8,224 | 1,530,417–1,530,785 → 1,205,753–1,206,121 | 779,886–780,254 → 594,686–595,054 |
| nested/fresh | 2,280 → 1,288 | 298,002 → 235,722 | 156,037 → 136,357 |
| nested/unchanged | 2,139 → 2,176 | 832,272 → 829,208 | 0 → 0 |
| nested/retained_edit | 1,818 → 1,097 | 279,648 → 205,632 | 135,356 → 111,052 |

Fresh sibling requests fall about 31% and fresh nested requests about 44%; gross
requested bytes fall about 14% and 21%, respectively. Fresh retained net bytes fall
about 13% in both fixtures. Retained-edit requests/bytes also fall; those net values
count only incremental allocation during the edited query, not the complete pair
of snapshots. The ordinary-file control adds four allocation requests and 96 net
bytes while reducing gross requested bytes; its timing medians are approximately
unchanged relative to the observed variance.

The mechanism is verified by shared parse/content ownership tests and the removed
masking/parser path: fresh user-source preparation performs one physical parse
instead of 1/33/9, with no per-child full source or line-index construction.
Unchanged queries perform no new parse in either version. Candidate snapshots
retain exact physical parse ownership, so no old view resolves against newer text.

The unchanged nested-query median increases from 423.55 to 463.00 microseconds
(about 9%), with 37 extra requests (about 1.7%) and slightly fewer gross bytes.
Inspection shows the unchanged query still rebuilds logical descriptors and the
module/declaration graph: parse sharing does not skip those owners. New logical
locators/diagnostic projections have bookkeeping cost; the measurements do not
isolate how much of the timing difference is bookkeeping versus background noise.
The initial larger increase did not persist at the same magnitude in the fixed
follow-up. No cache redesign or weakened namespace checks were added to hide this
limitation. Finer invalidation remains R8, and no uniform speedup is claimed.

Correctness acceptance uses focused source mappings, parser recovery/budgets,
retained views, signature/body remapping, declaration cache/cancellation, docs/lint,
portable inline origins, source-free exports and SDK inline execution/error cases.
The decoded-artifact nested execution returns 42. Required local structure,
formatting, workspace all-target Clippy and diff checks pass. Complete workspace,
feature/backend/platform acceptance remains CI-owned and unrun for this change.

## Context-owned module graph handles, 2026-10-21

Baseline `79ed7606` and implementation `8f029cd7` compare the ordinary HIR import
builder before/after SA9. Source units, namespace owners, dependency edges and
compiler source indexes now carry checked module-root handles. The
[identity architecture](architecture.md#definition-identity-ownership) owns the
contract; these measurements cover graph construction only, not end-to-end
compilation, checked-program construction, MIR lowering, encoding or execution.

### Environment and method

Windows x86_64-pc-windows-msvc, Intel Core i7-10700F (8 cores/16 logical CPUs),
rustc 1.99.1 (`58cf8f9c3`, 2026-10-13), LLVM 23.1.1. Workspace test profile
(opt-level 1, debuginfo), default HIR features/dev dependencies, default Cargo
target and parallelism. Compilation used mixed caches and is excluded: the
baseline harness build took 130 seconds, candidate harness build 53.53 seconds.
These compile times are not a performance comparison.

A temporary ignored library test calls production `ModuleGraph::build`; there
is no benchmark-specific preparation path or disabled validation. Reproduce with
`cargo test -p kagari-hir --lib imports::sa9_probe::measure_module_graph --
--exact --ignored --nocapture` after recreating the temporary harness described
below. The harness is removed after measurement; the workload and counting rules
here do not depend on retained local logs or executables.

Bind 32 SourceDatabase modules in package `game`, with paths
`combat::damage::mNN` for NN 00 through 31 and source URIs `mem://mNN`. Module i
contains `pub fn fi() {}` and two public named imports: the next module's function
(wrapping after 31) as `next`, and m00's f0 as `shared`. For example, module 00 is:

```text
pub use game::combat::damage::m01::f1 as next; pub use game::combat::damage::m00::f0 as shared; pub fn f0() {}
```

Capture the sources and call `lower_module` on each. Measure source acquisition,
parsing and lowering separately. Build the graph over the same lowerings; assert
32 nodes, 93 deduplicated edges (including the existing package hub), and no import
diagnostics. The candidate fresh operation creates its DefinitionContext, registers
all paths/prefixes through the normal builder, freezes/publishes the table and
returns both context and graph. Its rebuild uses that same context while retaining
the first graph. The baseline rebuild retains its first graph as well. Candidate
registration and publication are inside the measured operation, not free setup.

Use a forwarding System global allocator with thread-local counters, counting
successful alloc/realloc requests, gross requested bytes, and net allocated bytes
still live on return (subtract deallocations and old realloc sizes). Counting runs
separately from timing. For each stage/process, warm once, take seven elapsed
samples with destruction outside timing, then take one allocation sample. Both
executables use the same allocator instrumentation, including its disabled-counter
check during timing. Alternate three baseline/candidate processes: 21 elapsed
samples and three allocation samples per stage. No timed sample is removed.

An exploratory pass overlapped editor-triggered Cargo checking and is excluded
from the comparison below. The final three pairs ran after it finished; checks
before/after each pair found no Cargo/rustc processes. Other host activity, CPU
frequency and affinity were not controlled. Source-stage noise and graph timing
outliers are retained in the ranges. No RSS, allocator-overhead, production-load
or statistically established general compiler-speed claim is made.

### Matched results

Times are microseconds. Requested/net bytes exclude allocator overhead; net bytes
are incremental live allocations, not an exclusive heap attribution or RSS.
Ranges include variation across all three allocation passes.

| Stage | Median time (21 samples) | Time range | Requests | Requested bytes | Net live bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Baseline source/parse/lower | 1403.2 | 1354.4-1648.6 | 13364 | 711692 | 187224 |
| Candidate source/parse/lower | 1368.0 | 1337.3-2505.0 | 13364 | 711692 | 187224 |
| Baseline fresh graph | 6694.4 | 6482.2-11351.6 | 70243-70244 | 3661132-3661940 | 734800-735608 |
| Candidate fresh graph + context | 2888.6 | 2830.0-3997.1 | 12310 | 1949069 | 427927 |
| Baseline rebuild, first graph retained | 6619.5 | 6480.9-7362.7 | 70243 | 3661132 | 734800 |
| Candidate rebuild, first graph/context retained | 2873.0 | 2757.7-3171.7 | 11869-11870 | 1923659-1924027 | 413469-413837 |

Per-process fresh-graph medians are 6719.2/6694.4/6648.2 for baseline and
2862.7/2933.6/2903.3 for candidate; rebuild medians are
6520.8/6699.1/6619.5 and 2798.6/2924.5/2867.1 respectively. Source-stage medians
are 1366.7/1374.9/1408.5 and 1349.0/1355.1/2377.1, illustrating unrelated timing
noise despite identical source-stage allocations.

For this graph workload, the fresh operation reduces median allocation requests
by 82.48%, requested bytes by 46.77%, net live bytes by 41.83%, and elapsed median
by 56.85%. Rebuild reductions are 83.10%, 47.45%, 43.68% and 56.60% respectively.
Keeping both graphs alive retains about 0.84 MB of requested allocations including
the candidate context, versus about 1.47 MB for the baseline pair. This is not a
process-memory comparison. The representation removes repeated portable-path
copies and content-key operations; the measurements do not isolate each mechanism.
There is no basis here to extrapolate the timing ratios to full compilation.

### Table growth with an old prefix retained

The candidate graph registers 35 roots and 35 symbols: package root, `combat`,
`combat::damage` and 32 leaf modules. As a separate candidate-only boundary probe,
prepare those roots and retain the frozen table outside timing/counting, then
intern `game::combat::damage::late` and publish a new table inside the operation.
Repeat from the same initial state for seven timing samples and one allocation
sample per process. Verify the old table has 35 roots and rejects the new handle;
the new table has 36 roots. Input path construction and setup are excluded here.

Across three processes this append/publish costs 26 requests, 9933 requested bytes
and 8382 net live bytes, with a 3.7 microsecond median (3.4-9.2 range). It includes
the copy-on-write table/index growth caused by keeping the old prefix alive.
It has no baseline counterpart and is not a speedup claim. Larger retained
semantic tables can cost more; this bounded graph-only result does not resolve
identity-domain reclamation (R13) or broader compilation preparation costs.

## Direct source preparation, 2026-10-21

The compilation/startup work is measured separately from the historical Windows
investigation below. Baseline `77ef42e8` and implementation `48f3ba40` use matching
release settings on this Mac. CS02-CS05 replace source artifact round trips, retain
verification evidence, remove wire allocation from type-limit predicates, and share
graph proof resources. The CS06 bounded repair shares admission for identical
closures and prepares the lowering native-default catalog once per graph. On
2026-10-21 the user accepted closure with the 500 ms compile/load target unmet;
the [roadmap](implementation-roadmap.md#fast-compilation-and-eager-startup) owns
that deferred follow-up. The measurements and original thresholds are unchanged.

### Environment and method

macOS 26.6.2 (25G83), MacBookPro18,2, Apple M1 Max, 10 logical CPUs, 32 GiB RAM;
Rust/Cargo 1.98.1, rustc `48a229ceaefd4985c50990b14116b6d856af0985`, LLVM 22.1.8;
Chrome 155.0.8059.82; Node 22.20.0, npm 10.9.3; wasm-bindgen crate/CLI 0.2.129.
Standard Cargo release profile (opt-level 3), default target directories and build
parallelism, no wasm-opt or custom profile. All builds finished before timing;
no other task builds or benchmarks ran during sampling. Host/browser background
activity, CPU frequency and V8 code caching were not controlled.

The baseline native workspace was reconstructed with `git archive 77ef42e8` under
ignored `target/`. The baseline release WASM was built before source changes and
retained with its matching bindgen wrapper. Current exact source/artifact products
were rebuilt by `node site/scripts/wasm-products.mjs`. Native probes use default
SDK source+native features with interpreter execution and no JIT compilation;
source WASM uses the web adapter's default source feature, without native. These
feature sets differ across native and WASM; percentages must not be extrapolated.

Browser measurements serve immutable local products with the maintained
`startProductServer`/`launchBrowser` helpers. For each minimal and generic-trait
workload, five samples alternate baseline/current, each in a fresh browser context
in one Chrome process. After WASM initialization, each context constructs a first
session, compiles, executes twice, resets and frees it; constructs a second session
and repeats; then recompiles/executes on that retained second session. The external
artifact control uses a new session and the same fixture in both versions. Import/
initialization, construction, compile/load, first/repeat execution, reset/free and
external admission are separate observations. No sample is discarded as warmup.
Fresh construction+compile is computed per sample before taking its median. It
excludes WASM import/network initialization, as the plan's goal specifies.

The minimal source is `fn main()->i32{42}`. The generic-trait fixture defines
`Echo<T>`, `Pair { number: i32 }`, `impl Echo<i32> for Pair`, and
`read<U: Echo<i32>>(value: U) -> i32 { value.get() }`; main reads a Pair containing
42. It exercises generic trait dispatch, not a broad stress test. Both browser
executions assert 42. The external fixture asserts u64 max and `artifact hello 😀`.

Native timing uses five observations per workload in one process per revision;
baseline ran first, then current, with no discarded warmup. Each observation creates
an engine, checks/prepares source, constructs/loads a runtime, executes, repeats,
explicitly exports/encodes outside the measured regions, separately decodes/prepares
and loads that artifact, then checks/prepares again on the retained engine. Source
and external results are checked. The old preparation uses `emit_bytecode` followed
by `PreparedProgram::from_artifact`; current uses `engine.prepare`. This is the
intended public operation replacement, including the old default native-input
serialization costs. No native-backend timing claim is made.

The growth inputs add 1/8/32 leaf modules, each calling one shared function returning
42; root sums their results. On the 8-leaf graph, body edit returns 43, then public
signature edit adds an i32 parameter and updates callers with zero. Both edits
assert 344. The helper rebinds all module URIs on each preparation, including retained
and edit samples. Thus these measurements describe this workload's costs, not cache
hit rates, minimal invalidation or incremental dependency reuse. Record counts
include standard-library records. Table-bound constraints are counted, not dynamic
solver invocations; maximum type nodes covers function semantic parameters/results/
slots, not every carried contract or a worst-case type-limit workload.

A separate allocator-counting executable measures requested live Rust heap bytes
using the existing architecture-baseline `System` wrapper. It drops the checking
engine and checked module before measuring the prepared program, then binds to a
separate engine/runtime, and finally drops all owners. This is not allocation
traffic, peak memory, allocator overhead or RSS. The first observation includes
process-wide initialization; it is retained and labelled. Browser linear-memory
page counts are committed high-water storage and cannot establish live heap or
cleanup by themselves. Reset/failure/cancellation behavior is checked by the exact
browser and Worker suites, not inferred from page counts.

### Results and acceptance

All values below are milliseconds, median [minimum, maximum], with five samples
per condition. [Complete observations, product hashes and probe sources](measurements/compilation-startup-2026-10-21.json)
retain every timing/memory sample, including first-process initialization and the
separate excluded profile. Zero browser execution samples reflect timer resolution,
not a zero-cost execution claim.

| Browser workload / revision | First compile/load | Fresh construction + compile/load | Retained compile/load | External artifact admission |
| --- | ---: | ---: | ---: | ---: |
| minimal / baseline | 2748.8 [2731.5, 2799.9] | 3554.6 [3538.7, 3623.6] | 2574.4 [2556.5, 2606.5] | 541.0 [539.2, 554.3] |
| minimal / current | 708.9 [702.8, 808.4] | 1458.2 [1448.3, 1646.4] | 587.0 [585.7, 688.7] | 142.4 [141.8, 159.4] |
| trait / baseline | 2743.1 [2733.8, 2755.2] | 3543.1 [3536.7, 3572.0] | 2569.4 [2566.6, 2580.2] | 541.9 [540.4, 546.4] |
| trait / current | 717.8 [713.9, 734.1] | 1461.3 [1459.9, 1478.2] | 600.9 [595.3, 611.7] | 142.6 [141.6, 148.3] |

The minimal fresh-session median meets the 1500 ms goal, but its 1646.4 ms maximum
exceeds it; this is a median target, not a tail guarantee. First compile/load still
misses the 500 ms goal, including on the retained session. The user accepted this
remaining gap when closing the implementation; closure is not a claim that the
latency goal passed. These are matched Mac results, not acceptance on the historical
Windows machine or measurements of deployed network delivery.

For the minimal workload, first execution is 2.6 [2.5, 2.7] ms baseline versus
3.8 [3.4, 4.1] ms current; generic-trait first execution is 4.3 [4.2, 4.7] versus
4.6 [4.4, 4.6] ms. Repeat executions are 0.0-0.1 ms at browser timer resolution.
The first-call difference is reported rather than hidden inside compile savings;
there is no deferred graph verifier in the execution path. Current minimal reset
is 1.3 [1.2, 1.4] ms and free is 7.7 [7.5, 8.1] ms. WASM import/initialization is
50.1 [48.3, 54.9] ms current versus 59.6 [50.4, 61.5] ms baseline on loopback;
this is not a CDN/download claim. Second fresh-session construction and compile
observations are retained in the linked data, separately from retained-session reuse.

| Native workload | Baseline source check/prepare | Current source check/prepare | Baseline external decode/prepare | Current external decode/prepare |
| --- | ---: | ---: | ---: | ---: |
| minimal | 4152.2 [4141.7, 4165.1] | 576.9 [570.6, 624.9] | 1536.5 [1526.2, 1553.4] | 181.0 [178.8, 185.9] |
| trait | 4160.5 [4147.0, 4265.1] | 575.7 [574.4, 581.8] | 1532.6 [1528.5, 1554.1] | 180.2 [179.6, 183.8] |
| growth1 | 4292.0 [4277.6, 4297.1] | 584.0 [582.4, 592.7] | 1585.3 [1579.6, 1602.0] | 180.1 [179.9, 180.5] |
| growth8 | 4847.5 [4842.7, 4857.8] | 689.9 [680.1, 739.9] | 1784.3 [1775.7, 1793.9] | 182.8 [181.7, 184.3] |
| growth32 | 6864.0 [6828.9, 6923.8] | 1128.1 [1125.8, 1129.5] | 2446.5 [2437.2, 2455.7] | 192.3 [189.3, 197.2] |

| Native minimal phase | Baseline | Current |
| --- | ---: | ---: |
| Engine construction | 434.380 [432.437, 672.033] | 428.379 [420.423, 561.675] |
| Runtime construction/load | 5.615 [5.547, 5.993] | 4.349 [4.234, 4.686] |
| First execution | 0.016 [0.015, 0.107] | 0.016 [0.015, 0.112] |
| Repeat execution | 0.002 [0.002, 0.002] | 0.002 [0.002, 0.002] |
| External runtime construction/load | 5.506 [5.456, 5.538] | 4.242 [4.099, 4.317] |
| Retained engine check/prepare | 4158.833 [4148.038, 4262.450] | 574.334 [572.387, 575.456] |

Growth's remaining source preparation increases from 584.0 ms (1 leaf) to 1128.1 ms
(32 leaves), while external preparation increases from 180.1 to 192.3 ms. This
supports reduced shared verifier setup on these inputs; it does not establish
linear source compilation or a bound for generic fan-out or dense dependency graphs.
Body-only edit check/prepare is 4870.9 [4855.9, 4906.8] ms baseline versus
697.5 [697.3, 707.7] ms current. Public-signature edit is 4858.4 [4847.2, 4876.9]
versus 704.4 [703.1, 704.8] ms. All bindings are refreshed as described above;
these are not R8 incremental-reuse results.

| Workload | Modules | Functions | Instructions | Declarations | Table-bound constraints | Physical type records | Max measured semantic type nodes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| minimal | 37 | 1 | 2 | 618 | 89 | 47 | 1 |
| trait | 37 | 3 | 10 | 621 | 89 | 48 | 1 |
| growth1 | 39 | 3 | 6 | 620 | 89 | 49 | 1 |
| growth8 | 46 | 10 | 34 | 627 | 89 | 56 | 1 |
| growth32 | 70 | 34 | 130 | 651 | 89 | 80 | 1 |

### Retained memory and remaining diagnosis

After source analysis/engine disposal, minimal prepared-program live requested heap
is 5,791,387 bytes baseline versus 2,382,713 current for samples 1-4. The first
minimal sample is 18,700,868 versus 15,292,194 bytes, including 12,909,481 bytes of
one-time global initialization in each executable. Generic-trait prepared retention
is 5,819,191 versus 2,394,746 bytes across all five samples. Old default artifact
preparation retains portable native input; current interpreter preparation does not
request MIR retention. These are whole-route comparisons, not a claim that the seal
representation alone saves those bytes.

All samples after the first return to zero net requested live heap after dropping
program, independent runtime and engines; the first retains that same global
initialization. Current minimal runtime+program median is 49,520,279 bytes versus
52,917,937 baseline (all five observations, including the first, are retained).
Browser minimal linear memory ends at 71,041,024 bytes baseline versus 70,975,488
current; generic-trait ends at 71,237,632 versus 71,106,560. These high-water page
counts do not shrink on reset/free and must not be interpreted as leaked live objects.

The plan's one bounded reassessment/repair batch is complete. A separate retained
minimal compile with CDP profiling took 597.3 ms, after one explicit warmup compile.
Requested sample interval was 1000 us; actual intervals vary. Disjoint ancestry
weights are approximately 179.2 ms source analysis, 195.6 ms MIR lowering excluding
verification, 36.1 ms MIR verification, 36.0 ms bytecode lowering excluding
verification, and 62.7 ms bytecode graph verification. Remaining samples include
teardown and profiler/JS overhead. These are sampled weights, not exact phase timers
or a proven attainable floor. The initial excluded profile's linked-catalog
validation was about 253 ms inclusive; the final profile is about 16.5 ms inclusive.
Do not add inclusive costs to the disjoint phase totals.

Source inspection connects the remaining profile to two ownership boundaries:
`AnalysisDatabase::prepare_signatures` constructs imported type bindings for each
file before deciding whether the old signature result can be reused; MIR lowering
builds callable/interface ABI facts and resolves native-default applications through
HIR aggregate normalization. Inclusive samples identify about 81.3 ms in type
bindings, 67.7 ms in namespace reachability, 94.7 ms in interface instance recording
and 72.2 ms in ABI collection, with substantial overlap. This justifies a focused
review of snapshot-owned signature inputs and checked lowering facts, not an
unmeasured cache or a blanket removal of verification. It does not prove an R7
round-count cause, activate R7/R8/R16, or authorize permanent browser engine reuse.
The user chose closure with the remaining goal unmet after the bounded repair.
No additional optimization phase was activated; future work requires a separately
authorized scope. The 500 ms threshold is retained as an unresolved target.

### Correctness and product boundaries

Selected local seal/provenance, shape limits, dependency visibility, associated
outputs, host binding, cancellation, reload, package, debugger and shared source/
artifact/JIT contracts passed at their implementation checkpoints. The final repair
reran affected verification/host contracts and representative runtime boundaries;
separate Cargo invocations passed source-free portable and Cranelift artifact
execution. The exact final browser source/artifact products passed all maintained
adapter boundary cases. The selected real Worker `cooperative-loop-reset-effects`
case and its Node protocol suite passed. Product hashes were checked unchanged
through each browser run. Full workspace tests and feature/backend/platform matrices
remain CI-owned and were not run locally; the complete Playground UI matrix was
not rerun for this internal adapter/recipe change.

Source WASM is 18,986,115 bytes baseline and 17,832,439 current; the final artifact-only
WASM is 10,589,141 bytes. SHA-256 values, wrapper sizes and the 929,790-byte external
fixture identity are retained in the measurement data. These local products were
not pushed or deployed. Structure checks (1400 Rust files), formatting, workspace
all-target Clippy and diff checks passed for implementation `48f3ba40`.

## Browser compilation latency, 2026-10-20

**Historical diagnosis at the recorded revision, not a current latency gate.** The user reported
long Run waits at `https://kagari-lang.github.io/kagari/playground/`. Separate remote
delivery, session preparation, compilation and execution; the interpreter's steady
execution benchmarks do not explain this path. Subsequent
[source handoff and eager-verification changes](#direct-source-preparation-2026-10-21)
have their own matched measurements above; the
[roadmap](implementation-roadmap.md#browser-wasm-execution) retains browser delivery
follow-ups. The later 500/1500 ms goals are not results of this historical measurement.

### Live delivery versus repeated execution

Read-only Chrome measurements on the machine below observed the default example's
first successful Run at 36.30 s and immediate repeat at 4.14 s; the minimal program
`fn main()->i32{42}` took 25.24 s then 4.44 s. Both repeats made **zero requests**.
WASM fetches alone took 27.62 s and 15.63 s respectively. A separate minimal adapter
sample measured 41.27 s download/initialization, 1.085 s construction, 3.840 s
compile/load, 3.6 ms execution and 1.9 ms reset. Streaming compilation may overlap
transfer, so fetch duration is not a pure network-versus-V8 decomposition.

The later live WASM response was **23,762,457 decoded bytes / 4,546,825 gzip bytes**,
HTTP/2 200, `Content-Type: application/wasm`, `Content-Encoding: gzip`,
`Cache-Control: max-age=600`, `Vary: Accept-Encoding`. Missing compression is not the
cause. Several WASM and ordinary page-asset requests failed with
`net::ERR_CONNECTION_CLOSED`; this does not identify which network/CDN/intermediary
caused the failure. These observations are from this machine's route, not the
user's connection.

The served representation changed during the 13:13-13:19 UTC investigation
(earlier WASM: 23,758,722 decoded bytes). The public `.checked-build.json` returned
404 and no complete live binary hash was obtained. Thus neither live commit nor
profile was established, and these individual observations are **not a controlled
build comparison**. At that revision, local checked-in product orchestration used dev; that alone
is not proof of the deployed profile. No publication or remote setting changed.

### Matched local dev/release comparison

Revision **98f5c39d9d29449b3856bb155f677c48d37aee99**, source/default features,
`wasm32-unknown-unknown`, default target and build parallelism. Rust 1.99.1
(58cf8f9c3), LLVM 23.1.1, Cargo 1.99.1, wasm-bindgen crate/CLI 0.2.129. Workspace dev
uses opt-level 1; standard release uses opt-level 3 with debug assertions disabled.
No custom profile, wasm-opt, CPU affinity or deliberate throttling. Target build
outputs were initially absent; Cargo/registry caches were not purged. Both builds
finished before measurement; no concurrent build/probe was running.

Environment: Windows x64 10.0.26300, Core i9-12900K / 24 logical CPUs / about 64 GiB
RAM; headless Chrome 155.0.8059.82, Playwright 1.64.1, Node 22.21.1, 1280x900 viewport.
Local immutable loopback serving with HTTP cache disabled/no-store removed remote
transfer delays. Three sequential fresh-context samples per profile in one browser
process, no discarded warmups. OS, allocator, V8 tiering and WASM code caches were
uncontrolled; this is not a statistically established cold-cache benchmark.

Each context initialized the module, constructed session A, compiled/executed/reset
and freed it, then constructed B and compiled/executed/reset twice on B before
freeing it. Source: `fn main()->i32{42}`; all 18 baseline executions returned exact
`i32 "42"`. Medians in milliseconds:

| Boundary | Dev | Release |
| --- | ---: | ---: |
| Import + WASM initialization | 83.5 | 77.9 |
| First session construction | 1053.5 | 821.9 |
| First compile/load | 3823.9 | 2817.9 |
| First construction + compile/load, median of per-sample sums | 4877.4 | 3646.5 |
| First start + drive | 3.6 | 2.9 |
| Second fresh session construction | 611.6 | 460.3 |
| Second session first compile/load | 3643.9 | 2639.2 |
| Recompile on retained second session after reset | 3565.7 | 2631.5 |

Release reduced the measured fresh construction/compile sum by **25.2%**, but did
not remove multi-second compilation. Retaining the engine avoids construction,
not the remaining approximately 2.63 s compilation. Faster second *fresh* construction
also reflects warm-instance effects; it is not evidence of engine reuse.
Initialization uses import + adapter init; compile measures the synchronous adapter
call (analysis, lowering, artifact verification/preparation and runtime loading),
excluding response JSON decoding. Execution includes start/drive and JSON decoding,
not Worker scheduling/UI. First reset/free medians were 1.6/8.1 ms dev, 1.4/7.3 ms
release. These are adapter timings, not end-to-end Run latency.

| JS + WASM product | Raw bytes | gzip level 9 | Brotli quality 5 |
| --- | ---: | ---: | ---: |
| Dev | 24,098,645 | 4,373,978 | 3,090,904 |
| Release | 18,872,247 | 4,058,774 | 2,937,124 |

Each file was compressed separately offline; these are not HTTP transfer sizes.
Raw WASM SHA-256: dev `ca115d7f9e643de7c91ea09974250d7b78fb836892a26b161e08271b4418a7b2`,
release `88a642a0f8c386df1c825c57d97d90aefba582fda8a95eb0ee8ebea79e78257b`.
Both JS files hash to `886d761d598aaff0a18ddf7d05775e610c2739e1fc0b6de6e5471e59972d2be0`.

Reproduce products with `cargo build -p kagari-web --target wasm32-unknown-unknown
--locked`, adding `--release` for the other profile; run matching wasm-bindgen
`--target web` on each profile's `kagari_web.wasm`, keeping outputs separate under
ignored target. Use the phase timing technique in
[the maintained browser harness](../site/scripts/playground-browser.mjs), with the
minimal source above and separate fresh/retained sessions as specified here.

### Sampled compile hotspot and architectural cause

One additional release compile, excluded from medians, took 2676.5 ms. CDP CPU
profiling requested 1000 microsecond intervals; actual intervals varied. Of 1725
samples, 1719 were under `BrowserSession::compile`. Weight samples by `timeDeltas`
and classify exact ancestor boundary frames, prioritizing verifier frames over
their lowering callers; do not match arbitrary substrings in Rust generic names.
The disjoint approximate sampled attribution was:

| Boundary | Approximate ms | Share |
| --- | ---: | ---: |
| Source analysis | 200 | 7.5% |
| MIR lowering excluding verification | 237 | 8.9% |
| MIR verification | 340 | 12.7% |
| Bytecode lowering excluding verification | 298 | 11.1% |
| Bytecode verification | 1506 | 56.3% |
| Other | 95 | 3.6% |

Verification accounts for about **69%** of this trace. `verify_trait_bounds`
accumulates about 1475 ms inclusive across three call paths: compiler bytecode
emission (477 ms), `VerificationMetadata::from_program` (492 ms), and
`KbcArtifact::validate_execution` during preparation (505 ms). This establishes
repeated work at distinct boundaries, not permission to remove safety checks.

Largest self-time entries: `Ty::within_wire_limits` about 613 ms, allocator malloc
301 ms, free 205 ms. [The limit predicate](../crates/kagari-types/src/ty/wire.rs)
constructs temporary wire-node vectors merely to return `wire_nodes().is_ok()`;
[type matching](../crates/kagari-types/src/ty/matching.rs) invokes it on candidate
and requested receiver types. Sampled stacks connect this to trait proof and
normalization during verification. Inclusive function, allocator and phase totals
overlap; do not add them or infer allocation/call counts from sampling.

Source inspection explains the fixed work even for returning 42:

- [Worker](../site/src/playground/worker.js) retains the WASM instance but creates
  and frees a Rust BrowserSession on each Run. Its engine/native analysis baseline
  does not survive; stdlib's static authored bundle does survive.
- [Implicit dependencies](../crates/kagari-hir/src/imports/resolve.rs) add installed
  native APIs to nonnative modules. Checked closure/lowering therefore carry stdlib
  contracts even for an import-free program; this does not recompile Rust callbacks.
- [The adapter](../crates/kagari-web/src/lib.rs) forks a fresh source session, emits
  an artifact, prepares it and loads a fresh runtime. Existing native syntax caches
  can be reused within that fork; claiming all stdlib bodies are re-typechecked
  would overstate the evidence.

The measured priority is allocation-heavy validation/proof preparation and its
ownership across compile/artifact/load boundaries, not interpreter dispatch.
Artifact reachability and engine baseline lifetime also need explicit semantics;
reuse must not retain unlimited user identities or weaken source-free validation,
GC roots, cancellation, host contracts, reflection or reload generations. No core
implementation was changed, no optimization speedup is claimed beyond the measured
profile comparison, and full CI/platform coverage was not run.

## Root release and occupied discovery work counts, 2026-10-20

**Current R12 implementation evidence, not a timing baseline.** Root registration
drains final-lease IDs from MPSC instead of pruning the historical slot table.
Dead-object discovery walks occupied concrete stores and a narrow interface-wrapper
index; it does not scan directory tombstones. The lasting ownership and safety
contracts are in [the runtime model](spec/runtime.md#gc-heap).

Scope: default-feature `kagari-runtime` library tests with `cfg(test)` counters in
the actual [release-drain loop](../crates/kagari-runtime/src/gc/roots.rs)
(`release_visits`) and
[occupied-directory iterator](../crates/kagari-runtime/src/gc/directory.rs)
(`discovery_visits`). These count processed release records and yielded occupied
object identities, not inferred table lengths, elapsed time or total GC work.
Collections are invoked explicitly; discovery counters are reset before each
collection. The input sizes and ownership transitions below determine the counts;
CPU affinity, cache warmth and clock resolution are not measurement inputs.

| Workload / retained history | Counted work |
| --- | --- |
| Register and retain 64 / 4,096 distinct roots | 0 release records processed at both peaks |
| Drop all but four roots, then register once to drain releases | 60 / 4,092 records processed respectively; historical slot capacity remains |
| Then perform 32 registration/drop reuse iterations | 32 release records processed at either peak; no historical-slot scan |
| Object directory history 256 / 16,384, with eight occupied objects after the first collection | 8 discovery visits per collection at either history, including the collection reclaiming those eight; 0 on the following empty collection |
| Interface directory peaks 65 / 4,097, then two retained wrappers | Discovery visits 2 -> 2 -> 1 -> 0 -> 1 at both peaks: retain two, retire one, retire the last, metadata-only snapshot, then create/retire one replacement |
| Native take with the old identity still rooted, then reuse its slot at a new generation | 2 visits for the actual occupied native/cell objects, 2 when reclaiming them, then 0; the inert old identity is not a third occupied object |

The object workload creates 64 / 4,096 groups of String, cell, struct and tuple;
the first collection visits all 256 / 16,384 occupied objects, including garbage,
before retaining two four-object groups. The interface workload's first collection
likewise visits all 65 / 4,097 wrappers before retaining two. Metadata-only snapshot
retention neither keeps a wrapper occupied nor prevents its directory slot reuse.
The native-take case checks the replacement generation, old-identity invalidity and
inert-identity cleanup as well as work counts.

Reproducibility owners and exact library-test paths:

- [roots/tests.rs](../crates/kagari-runtime/src/gc/roots/tests.rs):
  `gc::roots::tests::registration_work_follows_releases_not_root_high_water`.
- [gc/tests.rs](../crates/kagari-runtime/src/gc/tests.rs):
  `gc::tests::occupied_discovery_follows_live_payloads_not_directory_peak`,
  `gc::tests::interface_occupancy_tracks_wrappers_not_snapshot_liveness_or_history`,
  `gc::tests::occupied_discovery_excludes_taken_native_identity_after_slot_reuse`.

Run an individual case with `cargo test -p kagari-runtime --lib <test-path> -- --exact`
using the workspace test profile, default target and build parallelism. The assertions
above are algorithm evidence, not a success-command log or a fresh timing run.
Root snapshots still scan historical root slots; metadata discovery, marking and
changed-store compaction retain separate costs. Discovery counts occupied objects,
including not-yet-collected garbage, **not only live objects**. MPSC sending may
allocate. Neither allocation-free release nor total GC O(live), wall-time improvement
or closure of BO04/IP12 gates follows from these counters.

## Bytecode slot and operand evaluation, Windows 2026-10-19

**Implementation is present, but BO04 performance acceptance is blocked.** The
[independent bytecode plan](bytecode-optimization-plan.md) owns the bounded decision
and outstanding gate. This result neither completes historical IP12 nor establishes
Lua parity. Current slot/origin/operand behavior is specified in
[bytecode](spec/bytecode.md) and [architecture](architecture.md#compilation-pipeline).

**Historical images versus current status:** these BO results and the earlier IP
measurements used the then-current scalar-constant hoisting behavior. R15 now keeps
scalar constants block-local, with no cross-block hoisting. Historical hoisted
images are not validation of that implementation; the focused debugger repair and
R11/R12 runtime changes establish no new performance result. BO04/IP12 failed gates
remain recorded below, not converted to passes or remeasured by this update.

### Images, environment and procedure

Baseline production revision: `06172d5e1aee6dca0b8f88fe46fc9726d8992b85`;
ordinary executable SHA-256
`ba2c5e44beba5fee14800275483585c7d3a862f02c48503433f08ed042afc740`,
27,663,872 bytes. BO02 is `6257833c`, BO03 is `9e739cd7`; the final image additionally
contains the bounded BO04 completion-publication ownership correction. Its SHA-256
is `7992458ac9914ffc0869daf69f442727b3d86989077ae9edcd15644300e3b9b8`,
27,816,448 bytes, 1.00552x baseline (limit 1.02). Cargo.lock SHA-256 is
`6fe327a7801e48778ae27a74764b556250f9c9007a0528e4fa3d38d424099009`.

Windows 11 10.0.26300, Intel Core i9-12900K, 24 logical CPUs, 66,843,692 KiB visible
memory; rustc 1.99.1 (`58cf8f9c3`, LLVM 23.1.1), Cargo 1.99.1 (`5f94df478`),
x86_64-pc-windows-msvc. Workspace release profile, default target and build
parallelism, Kagari source/native and vendored Lua 5.4.9 via mlua lua54/vendored;
no build flags/job overrides. Existing build cache was reused, with fresh process,
engine and runtime state for each run; execution follows three warmups. Compilation,
verification, preparation and linking are separate diagnostics, not execution time.
Builds/tests/diagnostics did not overlap timing.

Use the unchanged [benchmark procedure](../benchmarks/lua-comparison/README.md#beforeafter-workflow):
seven alternating process pairs, eleven checked samples per process, sole estimator
median(candidate process median / baseline process median). Preserve ranges, not a
second estimator or favorable sample selection. Workload SHA-256 values remained:
original `ce648a8dc75951a9f148ff3ce1fd0497c44b93e21ffb2b7324fc63f9e1eaaaee`,
forms `63da18bd97ed4e11e9cdb6a3fb8d77c7903031559020bc452dc1d90018f8808f`,
controls `8570e946697b691a2989188686033c8be32af26f4c2e7a35a017af464d227492`.

Initial unrestricted BO03 runs failed 16/43 execution/lifecycle rows, with pair
ranges as wide as approximately 0.35..3.08. One predeclared scheduling diagnosis
pinned the unchanged image pair to logical CPU 2, affinity mask 0x4: Windows
GetLogicalProcessorInformationEx reports this as class-1 P-core/SMT (core mask 0xC),
versus class-0 E-cores. Both engines/processes inherit the same affinity. Forty of
43 rows then passed; forms_native 1.05204 and single-cancel admission/retirement
1.09091/1.10 still failed. Unrestricted failures were not converted into passes.

Source diagnosis found an added post-region `current_mut()` solely to record
completion, repeating validated frame admission immediately after native result
publication. The one bounded repair assigns completion to the existing closed-region
or result-publication owner, within its current frame borrow. It retains post-callback
admission, value/window checks, exact executing PC, cancellation and slice rules.
There was no collection optimization, poll reduction, inline-hint sequence or new
fast interpreter. The final comparison below reruns affected suites after that code
change, using the same CPU, image baseline, sample count and thresholds.

To reproduce, freeze the ordinary baseline at the specified revision using the
three driver modes, then run each mode with `--baseline-executable` on the candidate:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --freeze-baseline target/bytecode-optimization/before
uv run python scripts/benchmark_lua.py --source-forms --interpreter-only --freeze-baseline target/bytecode-optimization/before
uv run python scripts/benchmark_lua.py --execution-controls --freeze-baseline target/bytecode-optimization/before
```

For the fixed-CPU experiment, set parent-process affinity with Windows
`SetProcessAffinityMask(GetCurrentProcess(), 0x4)` before launching each driver;
children inherit it. The ordinary driver does not set affinity itself. Check the
original paired report with `--target arithmetic=0.90 --target branches=0.95` and
the forms/controls reports with the unchanged default 1.05 limit. Do not replace
these thresholds or hide unrestricted results.

### Final timing and remaining failures

All checksums pass; 39/43 timing gates pass, **four controls fail**. Original seven
and forms twelve all pass. Selected ratios and complete failure list:

| Row | Paired median ratio | Pair min..max | Gate |
| --- | ---: | --- | --- |
| arithmetic | 0.691811 | 0.676915..0.713096 | <=0.90, pass |
| branches | 0.719951 | 0.504493..0.822588 | <=0.95, pass |
| maps | 1.005201 | 0.946474..1.043956 | <=1.05, pass |
| fibonacci | 1.008655 | 0.931481..1.061990 | <=1.05, pass |
| forms_direct | 0.668096 | 0.617169..0.699187 | <=1.05, pass |
| forms_native | 0.961690 | 0.917887..0.988957 | <=1.05, pass |
| forms_capture_cell | 1.042731 | 1.027786..1.114443 | <=1.05, pass |
| admit / roots_1_tail_complete | 1.076923 | 0.636364..1.083333 | <=1.05, **fail** |
| collect / roots_256_forward_cancel | 1.068182 | 0.763636..1.361111 | <=1.05, **fail** |
| collect / roots_256_random_cancel | 1.067797 | 0.657143..1.238095 | <=1.05, **fail** |
| retire / roots_1_tail_cancel | 1.100000 | 0.625000..1.111111 | <=1.05, **fail** |

The two single-root deltas are approximately 0.1 microseconds, but that does not
make a failed gate pass. Hybrid scheduling and quantization remain uncertainty;
no exact cost partition establishes that all residual failures are noise. Do not
continue favorable reruns or add a collection tuning queue. Further disposition
requires an explicit bounded scope decision under the active plan.

Diagnostic total-setup paired medians are 1.015981 original and 1.010372 forms
(limit 1.10); ranges 0.958320..1.059369 and 0.988551..1.025129. Totals sum Kagari
engine/runtime initialization, source-to-artifact, artifact preparation and linking
per process where supplied. They are matched setup diagnostics, not controlled cold
startup measurements. Lua comparison remains diagnostic: final Kagari/Lua
arithmetic is 2.18x and branches 1.64x. No parity claim follows.

### Complete affected retained bytes and allocation boundaries

Separate release diagnostics compared the BO01 source archive and BO03 source using
the same lockfile/toolchain/features and eight actual fixture/population states.
The archive used its own default target directory, never current compiled outputs.
The diagnostic feature was `kagari-runtime/execution-diagnostics`; ordinary timing
images were restored afterward. The later completion-owner repair changes neither
fields/types nor capacity policy, so these retained-layout results still apply.

The existing [counting allocator](../crates/kagari-runtime/src/diagnostics/allocations.rs)
measured net live allocation bytes and actual deallocation, not cumulative requests.
Temporary private-owner instrumentation consumed uniquely owned snapshots and
measured disjoint drop deltas; it was removed after measurement. To repeat the method:
compile outside accounting using identical absolute fixture paths, perform a balanced
PreparedProgram/runtime lifecycle, warm or park actual windows, retire roots in
seeded out-of-order sequence, then uniquely consume and drop each owned component.
Measure both actual canonical copies (source version and normalized user module),
one shared prepared product, and retained banks/frame/session capacities. Credit an
allocation only on final-owner drop; assert unique ownership rather than approximate
Vec capacity, BTreeMap nodes or shared Arc children. Complete independent teardown
must reach zero without subtracting process-lazy initialization from the result.

The affected total includes canonical instructions/nested buffers, full semantic types,
binding/debug and mandatory origins/actual-origin choices, slot/parameter/root/CFG
maps, function/module-owned allocation, constants and other affected tables, hot
prepared instructions, 48-byte scalar sites, layouts/reuse guards/call/field/index/
await tables, root execution links/constants, scalar/managed bank capacities,
initialization/window/free-link/allocation-order capacities and actual frame/session
backing storage. Distinct normalization copies count twice, shared prepared storage
once. Unchanged stdlib identity/declaration residue and the unchanged 2,892-byte
program-layout cache are reported separately, **not denominator padding**. SDK
retirement bytes are also identical per fixture; all six portable MIR payloads are
byte-identical and not used to dilute representation growth.

| Selected state | BO01 affected bytes | BO03 affected bytes | Ratio |
| --- | ---: | ---: | ---: |
| arithmetic | 38,346 | 32,708 | 0.852970 |
| branches | 56,061 | 49,743 | 0.887301 |
| forms_direct | 35,137 | 35,302 | 1.004696 |
| forms_helper | 40,346 | 35,868 | 0.889010 |
| forms_capture_cell | 44,282 | 42,460 | 0.958855 |
| owned, 1 complete | 30,108 | 27,226 | 0.904278 |
| owned, 32 complete | 89,876 | 90,666 | 1.008790 |
| owned, 256 cancel | 543,252 | 570,922 | 1.050934 |

All selected rows satisfy 1.10; even removing session-record storage from the
denominator gives maximum 1.068820. Frame growth 240->272 bytes accounts for much
of the large-population increase; hot instructions remain 24 bytes and scalar
sites add 48 bytes each. Arithmetic's canonical instructions shrink 7,892->5,336
bytes, semantic maps 9,888->4,848, while new origins cost 1,824; prepared hot code
528->408 plus 48 site bytes. Its physical payload bank shrinks 88->72 bytes. These
figures explain the whole-representation result without equating hot-enum size
with retained memory. This is not RSS, allocator overhead or an OOM guarantee.

All sixteen measured before/after lifecycles end at zero net bytes. Existing focused
allocation contracts pass on both images: warmed native boundaries and script scalar
argument/return windows have zero new allocations. Whole SDK calls retain existing
allocations: eight warmed arithmetic calls request 56 allocations/1,472 bytes on
both versions, not zero. Await-heavy/generic/aggregate-heavy retained workloads and
exhaustive feature/backend/platform coverage were not measured here. Focused local
correctness, workspace Clippy and lightweight checks pass; CI remains unrun and
BO04 stays open because the four timing gates above are unresolved.

## Object storage overhead, 2026-10-18

OM01-OM05 completed local acceptance. Native mutation revisions now belong to
runtime-owned native records; the common directory entry is 12 bytes rather than
24 on this target. Interface heap identities locate metadata snapshots directly,
removing the intermediate 40-byte wrapper record. Full checked identities, 16-byte
Value transport, packed struct fields, shared descriptions and the collector are
preserved. Per-layout struct groups were explicitly deferred after a sparse-cost
projection failed the declared limit. Current contracts live in
[architecture](architecture.md#runtime-model) and the [runtime specification](spec/runtime.md);
Git history owns the completed execution ledger.

### Environment and matched procedure

Apple M1 Max, 32 GiB, 10 logical CPUs, arm64 macOS 26.6.2; rustc/Cargo 1.98.1,
LLVM 22.1.8. Workspace release profile (opt-level 3), default target/build
parallelism, warm Cargo cache, SDK source/native features and vendored PUC Lua
5.4.9 through mlua 0.12.2. Execution is single-threaded and interpreter-only.
No build, test, sampler, allocation instrumentation or other measurement overlapped
ordinary timing. Desktop scheduling/power were uncontrolled.

The frozen OM01 runtime is `51ca855d` plus the probe-only changes committed at
`b5ab2bb9`. Native revision migration is `1788d94b`; final runtime is `65bbf477`.
The additional interface-call control was frozen on accepted OM02 before OM04
production edits. All OM comparisons use the same lockfile/toolchain/workload.
Inherited RS comparison uses `afec4a3b` with only equivalent raw-sample output added:
its original script is byte-identical and shared external dependency versions
match; its lock predates unrelated, unused LSP dependency additions.

Each comparison uses seven fixed alternating baseline/candidate process pairs,
three warmups and eleven checked samples per row. The sole threshold estimator
is median(C_i / B_i), where each term is one process's sample median. Preserve raw
samples and paired ranges; ranges are not confidence intervals. No second estimator,
calibration, report extension, discarded pair or unchanged favorable retry is used.
Compilation, verification, preparation and linking are outside execution timers;
setup and Lua are diagnostics. This is regression acceptance and measured storage
reduction, not a general execution speedup or Lua-parity claim.

The [storage probe](../benchmarks/lua-comparison/examples/runtime_storage/main.rs)
keeps the original 26 rows: eight execution cases and construction/live-GC/release-GC
for six 512-object populations. Scalar/field loops use 50,000 iterations; traversal
uses 512 elements. Three separately compiled 2,000-element controls cover sequence
growth, iteration and callback-driven retain/reverse; their preparation follows
the original rows. Build and save each ordinary image before timing:

```text
cargo build --locked --release -p kagari-lua-benchmark --example runtime_storage
```

Run the saved before/after images sequentially for the seven alternating pairs,
checking identical counts/results and all eleven samples, then compute each row's
paired ratio. Build separately with `--features diagnostics` for counting-allocator
and backing-store attribution; never overwrite the saved ordinary images with
diagnostic builds. The existing forms driver/checker reproduces the call control:

```text
uv run python scripts/benchmark_lua.py --source-forms --interpreter-only --freeze-baseline target/om-before
uv run python scripts/benchmark_lua.py --source-forms --interpreter-only --baseline-executable target/om-before
uv run python scripts/check_benchmark.py <paired-results.json>
```

Freeze on the baseline checkout, then compare on the candidate checkout. Methods
and results here do not depend on ignored local reports or orchestration scripts.

### Total requested-byte storage

The separate diagnostic probe measures eight populations at one and 512 objects,
each through setup, construction, live GC, sparse/released roots, empty GC and
three equal create/release/reuse cycles. Values below are net requested-byte
increments above each loaded runtime's setup; they include capacity, roots and
metadata, and are neither per-object sizes nor RSS.

| Population (512) | OM01 live | Final live | OM01 empty | Final empty | OM01 reused | Final reused |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Point | 57,036 | 44,780 | 39,460 | 27,172 | 65,644 | 53,388 |
| Pair<i32> | 57,812 | 45,556 | 39,500 | 27,212 | 66,004 | 53,748 |
| Narrow interfaces | 127,584 | 82,560 | 100,704 | 76,128 | 148,064 | 103,040 |
| Wide interfaces | 128,808 | 83,784 | 100,704 | 76,128 | 149,288 | 104,264 |
| Generic interfaces | 128,960 | 83,936 | 101,120 | 76,544 | 149,472 | 104,448 |
| Repeated interface copy | 17,544 | 17,368 | 6,848 | 6,800 | 17,608 | 17,432 |
| Cyclic nodes | 413,380 | 396,996 | 71,828 | 47,252 | 429,764 | 413,380 |
| Numeric buffer | 9,176 | 9,160 | 5,896 | 5,848 | 9,208 | 9,192 |

OM02 lowers Point/Pair live increments by 21.49%/21.20% and empty increments by
31.14%/31.11%, clearing the 10% targets. Dense native entries grow from 136 to
144 bytes. With 1,024 reserved directory slots and four reserved native entries,
12,288 saved directory bytes minus 32 extra native bytes explain the 12,256-byte
live reduction. Struct records remain 24 bytes and Point fields remain eight bytes.

OM01 Point live backing attribution is 49,816 bytes: directory 24,576, struct
records 12,288, field bytes 4,096, native entries 544, native payload box 120 and
sequence capacity 8,192. The remaining 7,220 bytes include roots/frame capacity,
shared layout/type/codec and lookup metadata not individually attributed by the
shallow store view. Pair's remainder is 7,996. Interface/node metadata and native
children also require their own owners; an unattributed remainder is not available
payload savings. Diagnostic accounting allocates no result buffer or permanent
counters and reports actual interface snapshot-slot backing (48-byte slots).

OM04 further lowers narrow/wide/generic live increments from OM02's
103,040/104,264/104,416 by exactly 20,480 bytes each (512 x 40):
19.88%/19.64%/19.61%, above the 15% targets. Snapshot roots and checked heap aliases
remain independent. Empty interface wrapper capacity had already been released
under OM02, so OM04 does not claim another empty-capacity saving.

Both final 96-row comparisons pass: all setup/construction/live/released/empty/reused
increments meet the OM01 common +5% budget and Point/Pair targets, and the three
interface targets meet OM02's additional 15% reduction. Loaded setup moves from
5,251,635 to 5,251,827 net bytes (+192, within 5%). All sixteen runtime teardowns
are zero, all three reuse cycles plateau, and warmed field loops retain
98 allocation requests/6,928 requested bytes/zero net bytes.

Construction allocation totals below subtract setup; these are requested bytes
over the operation, distinct from retained bytes above.

| Population (512) | Requests, before → after | Requested bytes, before → after |
| --- | ---: | ---: |
| Point | 654 → 654 | 143,208 → 118,712 |
| Pair<i32> | 682 → 682 | 144,608 → 120,112 |
| Narrow interfaces | 3,761 → 3,753 | 520,745 → 430,873 |
| Wide interfaces | 3,794 → 3,786 | 525,377 → 435,505 |
| Generic interfaces | 31,435 → 31,427 | 2,743,915 → 2,654,043 |
| Repeated interface copy | 1,167 → 1,166 | 41,077 → 40,901 |
| Cyclic nodes | 7,324 → 7,324 | 972,743 → 939,991 |
| Numeric buffer | 102 → 102 | 15,240 → 15,224 |

Live/empty GC allocation requests and requested bytes are unchanged in these
512-object cases. Release GC requests are unchanged; Point requests eight extra
bytes, all four interface populations request 48 fewer bytes each, and the other
cases are unchanged. Retained storage wins do not conceal repeated preparation.

### Timing gates and rejected candidate

All 29 final OM01 gates pass at <=1.05. The following maxima cover every row in
each group; ranges are the seven individual paired ratios.

| Rows | Maximum median ratio | Workload | Paired range |
| --- | ---: | --- | --- |
| Eleven execution/control rows | 1.009081 | read_fields | 0.973097–1.025891 |
| Six construction rows | 1.018304 | nodes | 0.997398–1.048227 |
| Six live-GC rows | 1.016003 | points | 0.997858–1.038660 |
| Six release-GC rows | 0.990580 | interfaces | 0.965156–1.004211 |

Growth/iterator/callback ratios are 1.005592/0.997324/1.001721. All 26 inherited
pre-RS gates also pass without compounding OM allowances: fields/float/read/write
ratios are 0.597290/0.615716/0.615961/0.793797 (limit 0.90); other execution maxima
remain <=1.05. Node release GC is the maximum lifecycle ratio, 1.049002
(1.017606–1.075983), within the original 1.10 limit.

All twelve forms rows pass <=1.05 against accepted OM02. Maximum is direct
recurrence 1.014041 (0.980426–1.027427); interface is 0.999405
(0.980566–1.002674), shared generic 0.996589 (0.985080–1.001047), and the remaining
rows range from 0.989149 to 1.003984. This covers byte state, captures, concrete
generics, fields, helpers, host callbacks, native calls and strings as well.

The first OM04 prototype at `90342034` passed 28/29 OM01 rows but failed growth:
1.062032 (1.042037–1.069698), above 1.05. Its pre-RS/forms comparisons did not run.
Diagnosis found that copying interface method count into every transient object
view widened shared/exclusive views to 40 bytes. `65bbf477` moves count lookup to
accounting boundaries and restores those views to 32 bytes while retaining full
metadata IDs and the same unit formula. The complete new comparisons above follow
that production change; the failed result is retained, not reclassified. Layout
size alone does not isolate the cause of the full timing delta. No inline hints,
name-based specialization, cache or alternate semantic implementation was added.

### Layout grouping decision and reference fit

OM03 measured one/64 exact eight-byte scalar layouts with one/four rooted rows.
An optimistic group model uses a 56-byte group (description Arc, byte Vec and
reverse-owner Vec), eight-byte global row and four-byte per-row directory backref,
excluding lookup/slack/control allocation costs in its favor.

| Layouts / rows each | Current live bytes | Current empty bytes | Projected byte change | Projected live ratio |
| --- | ---: | ---: | ---: | ---: |
| 1 / 1 | 924 | 700 | -4 | 0.995671 |
| 1 / 4 | 1,044 | 700 | +8 | 1.007663 |
| 64 / 1 | 29,064 | 16,264 | +2,816 | 1.096890 |
| 64 / 4 | 55,944 | 33,928 | +512 | 1.009152 |

All four diagnostic teardowns were zero. The sparse 64-layout case exceeds +5%
even before omitted costs. A projected 6,088-byte homogeneous Point saving cannot
override that gate. Keep the single packed arena; do not add a second sparse
fallback. This rejects the evaluated model, not every possible grouping design.
The temporary probe was removed; no grouping implementation or speedup is claimed.

Reference review used OpenJDK `jdk-25-ga`
[AbstractList](https://github.com/openjdk/jdk/blob/jdk-25-ga/src/java.base/share/classes/java/util/AbstractList.java),
[markWord](https://github.com/openjdk/jdk/blob/jdk-25-ga/src/hotspot/share/oops/markWord.hpp)
and [InstanceKlass](https://github.com/openjdk/jdk/blob/jdk-25-ga/src/hotspot/share/oops/instanceKlass.hpp),
Go `go1.25.2` [mheap](https://github.com/golang/go/blob/go1.25.2/src/runtime/mheap.go),
and Rust's [trait objects](https://doc.rust-lang.org/reference/types/trait-object.html)
and [dyn compatibility](https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility).
Adopt the split between collection mutation state, identity and shared type/dispatch
metadata. Java's optional int fail-fast counter/header bits, Go's concurrent span
allocator and Rust's exclusion of dispatchable type-generic methods do not replace
Kagari's u64 exhaustion, checked nominal generations, GC roots, scalar/generic
interface receivers or pinned reload. No new collector follows from those sources.

### Local correctness and remaining scope

The final 28 selected contracts cover fourteen interface metadata/alias boundaries,
revision relocation/replacement/exhaustion, two lease/error/unwind cases, three
bulk edits, sparse relocation, layout budget, iterator GC/failure, concrete identity,
ordered traps/root cleanup, host GC/reentry/reload, retained generic reload and
native-enabled source-free payloads. Earlier OM02 native data/managed-field/extraction
checks remain evidence for unaffected paths. Initial disposable-fixture and
intermediate unit-signature build errors were resolved before acceptance.

Workspace all-target Clippy, diagnostics-example Clippy, formatting, structure
(1,307 Rust files, no findings or exceptions) and diff checks pass. OM05 reuses the
final unchanged runtime's composition and measurement evidence; documentation edits
need content/link/diff checks. Full workspace/feature/backend/platform acceptance
remains pending CI. DC/NC inventories are refreshed without activating either;
Lua parity and the separate interpreter lifecycle acceptance remain unresolved.
Handoff checks cover balanced fences and 280 local links/anchors across six changed
Markdown documents; no findings or deleted-plan references remain.

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

At this historical baseline, copy propagation reused and hoisted scalar constants,
so those constants were not loaded every loop iteration. Current
[copy propagation](../crates/kagari-mir/src/passes/copies.rs) instead keeps scalar
constant reuse block-local; this old instruction stream is not current validation.
At the measured revision, loop-header forwarding declined an unprocessed backedge,
and local stores retained debugger availability.
[Physical preparation](../crates/kagari-runtime/src/module/execution.rs) turned
local reads/writes into moves while
[register allocation](../crates/kagari-runtime/src/module/execution/allocation.rs)
reused temporary slots but kept named locals separate. Thus better slot allocation
alone did not eliminate that baseline's local traffic or comparison-result/branch pair.

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
[builtin-key branch](../crates/kagari-stdlib/src/foundation/hash/operations.rs); it does not call
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


## General loop placement follow-up (Windows, 2026-10-27)

The user authorized BO05-BO08 after the direct bytecode review. Compiler placement
now initializes scalar homes directly, selects stable multi-use destinations,
forwards scalar tail returns with their visible bindings, and removes final
fallthrough jumps. A bounded MIR pass retains entry guards and clones closed
scalar conditions on dominated Jump backedges with fresh temporaries and original
spans/scopes. Runtime allocation reuses dying unnamed scalar inputs only where
handlers read inputs before computation and commit after success. Named/parameter
homes and managed storage retain their existing boundaries. The
[architecture](architecture.md) and
[public frame contract](spec/execution.md#public-frame-access) own these mechanisms;
the [active plan](bytecode-optimization-plan.md#authorized-general-follow-up-2026-10-27)
owns the still-open integrated acceptance.

This is a general, conservative selection, with no benchmark-name recognition,
numeric-for opcode, numeric-kernel ABI change, collector change or call-boundary
rewrite. Header snapshots consumed outside a closed condition and suspending
functions are declined. Not every while loop acquires the direct shape, and no
speedup is inferred for unchanged cases. The first-entry guard remains even when
a separate future proof could establish one iteration.

### Measurement contract and validation

Baseline production is 03246d94, frozen at documentation HEAD 22854ff9 before edits;
candidate production is f6aa9cbc. Ordinary binary SHA-256 values are
883ea385c40e3d262b5019889ddc7ad697209ac9a585734ded4426f69ee07f9a and
01791dd819e30149109b0ac294bee2090c94bce0a6fd8fb18224c9a76d2c04e8.
The machine is an i7-10700F, 16 logical CPUs, 64 GiB RAM, Windows 10 Pro
10.0.19045, x86_64 MSVC. Toolchain: rustc 1.99.1 (58cf8f9c3), LLVM 23.1.1,
Cargo 1.99.1. Workspace release defaults, opt-level 3, source/native and
mlua lua54/vendored features, default Cargo parallelism, sequential single-thread
benchmark processes and unchanged workloads/estimators were used. No build,
test, profiler or diagnostic allocator overlapped a throughput window. Build and
source/artifact/runtime/link preparation were excluded from execution timings.

Freeze all three ordinary suites before changing production. On the candidate run:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/lua-comparison/general-before
uv run python scripts/benchmark_lua.py --interpreter-only --source-forms --baseline-executable target/lua-comparison/general-before
uv run python scripts/benchmark_lua.py --interpreter-only --execution-controls --baseline-executable target/lua-comparison/general-before
```

Each suite used seven alternating baseline/candidate pairs, three warmups per
route and 11 samples per process. Fresh runtime/process state is separate from
warmed execution. All checksums and image/contract validation passed. Gates use
the median of seven within-pair candidate/baseline process-median ratios, not a
ratio of the displayed seven-process medians. UTC starts: original 08:14:02,
forms 08:18:54, controls 08:20:10. Both ordinary check routes also passed before
throughput measurement; their cold single-sample numbers are not speed evidence.

Focused local parser, compiler lowering/pass/canonical/invalid-artifact, runtime
frame/allocation/layout/origin, VM numeric/trap/loop/slice/cancellation checks pass.
The loop fixture exposed a pre-existing frontend error: a block condition was
also selected as the body. WhileStmt now selects the correct body position,
including binding conditions; optimized/unoptimized and decoded execution pass.
The structure check (1,732 files, zero exceptions), format check, full workspace
all-target Clippy with denied warnings and diff check pass. Full workspace tests
and the complete backend/feature/platform matrix remain CI-only and unrun here.

### Execution results

All 7 original and 12 forms execution gates pass, including the predeclared direct
limit 0.90 and other limits 1.05. This does not mean every case improved. In
particular, helper and concrete_generic slowed about 4.17% and 3.97% by the paired
estimator; arithmetic is essentially unchanged. Those results are retained.

| Workload | Baseline median (us) | Candidate median (us) | Paired candidate/baseline | Candidate VM/Lua |
| --- | ---: | ---: | ---: | ---: |
| arithmetic | 1580.700 | 1583.500 | 1.00316 | 1.77 |
| arrays | 12325.800 | 11398.400 | 0.92758 | 79.10 |
| branches | 2140.400 | 2114.000 | 0.98152 | 1.45 |
| calls | 8630.400 | 8663.300 | 1.01645 | 19.10 |
| entry | 2.850 | 2.743 | 0.97375 | 53.37 |
| fibonacci | 19007.400 | 18707.100 | 0.97884 | 39.68 |
| maps | 9507.200 | 9293.600 | 0.98153 | 69.93 |
| forms_byte_state | 21081.100 | 21412.100 | 1.01106 | 100.06 |
| forms_capture_cell | 9567.400 | 9740.200 | 1.01613 | 44.68 |
| forms_concrete_generic | 4298.200 | 4472.300 | 1.03967 | 23.34 |
| forms_direct | 206.500 | 179.700 | 0.87306 | 1.51 |
| forms_field | 1082.600 | 1102.200 | 0.99427 | 6.10 |
| forms_helper | 4206.600 | 4362.300 | 1.04174 | 22.08 |
| forms_host_callback | 5180.800 | 5184.100 | 1.00949 | 10.61 |
| forms_interface | 12559.400 | 12489.500 | 0.99376 | 57.85 |
| forms_native | 5139.800 | 5236.300 | 1.01815 | 26.72 |
| forms_shared_generic | 18428.700 | 18425.100 | 0.99962 | 89.36 |
| forms_string_calls | 10389.600 | 10802.700 | 1.02534 | 49.37 |
| forms_string_constants | 5869.300 | 5951.900 | 1.01407 | 41.77 |

Direct's paired ratio is 0.873062, range 0.853751..0.894915: about 12.7% less time.
Its displayed medians are 206.5 -> 179.7 us; the median-of-ratios estimator differs
from their quotient. Candidate Lua is 119.4 us, making VM/Lua about 1.51. Lua's
paired direct ratio is 1.00758. Cross-engine ratios for the host_callback row need
care: Lua's own paired ratio was 1.28560 while Kagari's was 1.00949, so that row
does not establish an improvement in Kagari's host-call implementation.

### Direct executable and retained storage

The real full forms module produces the following simplified canonical code,
with checked arithmetic and success-only destination commits:

```text
0  const state, 1
1  const sum, 0
2  const i, 0
3  branch (i < 5000), body, exit
body:
4  mul scratch_a, state, 3
5  add scratch_b, scratch_a, 7
6  rem state, scratch_b, 251
7  checked_add sum, sum, state
8  checked_add i, i, 1
9  branch (i < 5000), body, exit
exit:
10 return sum
```

The two logical scratch values share one physical location. Static instructions
15 -> 11; logical slots 8 -> 5; physical scalar locations 5 -> 4. The independent
observer pass counts 40,008 -> 30,005 actual instructions, with 6 loop operations.
Lua's unchanged hook counts 40,006 instructions, with the previously established
8-operation successful integer loop. These are dispatch counts, not equal-cost
operations or evidence of Lua code-density parity. Prepared Kagari instructions
remain 24 bytes and scalar sites 48 bytes; no byte-packing migration was made.

A temporary private-owner probe loaded both complete KBCs, adopted/verified them,
and compared the baseline/current allocator and layout. A separate public probe
cloned canonical functions exactly as VerifiedProgram::from_bytecode does and
measured vector capacities times element size. Temporary source was removed.
The intermediate decoder reserves 16 canonical instruction elements in both
cases; that staging capacity is not the runtime's cloned retained code payload.
All changed instruction/site/layout/origin/debug array payloads are included below:

| Direct retained payload | Before (bytes) | After (bytes) |
| --- | ---: | ---: |
| Canonical instructions, 144 bytes each | 2160 | 1584 |
| Prepared instructions | 360 | 264 |
| Prepared scalar sites, 5 -> 6 | 240 | 288 |
| Logical location table | 64 | 40 |
| Reuse guards, 4 -> 2 records | 160 | 80 |
| Scalar payload and initialization banks | 45 | 36 |
| Mandatory origins, 28 -> 31 | 1120 | 1240 |
| Actual-origin selections, 7 -> 10 | 56 | 80 |
| Slot types plus debug frame slot types | 16 | 10 |
| Debug spans, lines and points | 2496 | 2760 |
| Debug local range records | 216 | 216 |
| Control targets | 12 | 8 |
| Selected payload sum | 6945 | 6606 |

This is a selected retained-array/bank comparison, not whole-program allocator or
RSS accounting; unchanged headers, declarations, shared source metadata and other
heap allocations are outside the sum. Hot prepared instruction/site storage is
600 -> 552 bytes, while additional origins/debug columns offset part of the
transport savings. The entire forms KBC, including all modules and portable MIR,
is 1,738,824 -> 1,743,458 bytes (+0.267%). Fewer executed operations do not imply
a smaller complete artifact or a proportional memory reduction.

### Remaining execution costs

Fresh ordinary-image Windows samples used the existing sampler, copied matching
PDBs, selected the main thread, three warmups and independent 10-second windows.
Instruction/GC counting ran afterward and was excluded from throughput. Original
arrays/maps began 08:25:50 UTC; forms direct/helper/interface/native/byte_state/
string_constants began 08:26:19. PDB attribution is incomplete: unresolved samples
range from 25.0% to 53.5% for boundary-heavy cases and 98.4% for direct. Only range-
verified names are treated as sampled evidence. No precise direct kernel/dispatch
percentage, inclusive call-chain total or attribution of unresolved preceding
symbols is claimed.

| Workload | Kagari instructions | Lua instructions | Kagari Calls | GC collections/entry | Heap objects/entry |
| --- | ---: | ---: | ---: | ---: | ---: |
| direct | 30005 | 40006 | 0 | 0 | 0 |
| helper | 45005 | 65007 | 5000 | 0 | 0 |
| interface | 50008 | 65009 | 5000 | 4.910 | 1 |
| native | 25005 | 65007 | 5000 | 0 | 0 |
| byte_state | 130011 | 80523 | 15001 | 1 | 2 |
| string_constants | 67505 | 52506 | 5000 | 0 | 0 |
| arrays | 56016 | 42012 | 8002 | 2 | 1 |
| maps | 46017 | 24012 | 4002 | 6 | 2001 |

Native body work and metadata admissions have no script-bytecode event; the
counts do not measure the operations inside a native callback.

The source paths and these measurements identify the following review order:

1. **General call admission/return.** Helper executes fewer bytecodes than Lua,
   yet takes about 22 times as long; direct and helper run the same 5,000-step
   recurrence. Range-verified samples include select_call_route, installation
   validation/current capture, DefaultHasher/HostFrameId hashing, termination
   checks, type comparison, bank allocation and memmove. Source
   [call selection](../crates/kagari-runtime/src/module/dispatch/mod.rs) captures
   the current installation and validates it; subsequent admission/metadata and
   host paths perform more checks/lookups. Review which facts can be owned across
   an exact internal transition while preserving publication selection, reentry,
   cleanup and host borrow validation. This is a lifetime/ownership review, not
   evidence that all generation, argument or cancellation checks are redundant.
   Calls/Fibonacci and concrete/shared/captured variants inherit this boundary.
2. **Interface invocation allocation.**
   [MethodInvocation::admit](../crates/kagari-runtime/src/objects/invocation.rs)
   allocates a MethodComputation for each newly live invocation to pin its selected
   body/environment. Reusable semantic application caches do not remove this
   allocation. The interface profile observes about 4.91 collections per 5,000
   method calls even though ordinary heap-object allocations are only the one
   interface object. Application/computation allocation is a separate metadata
   store and is not counted by heap_object_allocations. Invocation ownership and
   exact-method pinning require review before considering a cache or alternate
   allocation owner; shared generic witness/environment work is additional.
3. **Native boundaries for Vec/String.**
   [prepare_calls](../crates/kagari-runtime/src/module/execution/calls.rs) replaces
   all native-import sites with common prepared Calls and sets native_calls to
   zero. Existing in-region Vec/String primitive handlers are consequently not
   selected for these sites. Index/len/get/set operations still enter generic
   selection, native frame, argument/borrow and return handling. The fresh array,
   byte-state and string profiles resolve those boundary/check/allocation paths.
   Warm string_constants has zero heap-object allocations and collections, so
   repeated string allocation is not its observed bottleneck. Native and host
   callback bodies also retain their checked Value adapters.
4. **Container-specific work after admission.** Map insertion performs an outer
   contains_key, another in HashMapStorage::insert and the final insert. This is
   repeated probing visible in
   [maps_sets](../crates/kagari-runtime/src/gc/maps_sets.rs) and
   [hash storage](../crates/kagari-runtime/src/native/hash_storage.rs). Map gets
   construct Option records; the fixture produces 2,001 heap objects and 6
   collections per complete workload. Option representation, checked collection
   mutation and host boundaries are current semantic costs; they are separate
   from the clearly repeated probe. Scalar field/cell cases retain object identity,
   generation/type checks and shared mutation semantics. Local transport removal
   does not erase those requirements.

Direct has no observed GC or heap allocation. Checked arithmetic, indirect numeric
kernels, prepared-site operand selection, initialized/bounded bank access and
final-executable polling are visible source-level remaining costs. The unresolved
profile prevents assigning precise shares among them. More bytecode removal or
unsafe check deletion is not justified by the instruction count alone. Helper
still has an explicit call-result commit Move per iteration; that conservative
call/observation boundary is outside scalar destination selection and cannot
explain the much larger whole-call cost by itself.

### Lifecycle timing remains open

Unrestricted controls pass 22/24 gates. The failures are:

| Phase/workload | Paired ratio | Pair range | Limit |
| --- | ---: | --- | ---: |
| collect/roots_256_forward_complete | 1.101449 | 0.324786..2.033333 | 1.05 |
| collect/roots_256_random_cancel | 1.078947 | 0.490683..1.586667 | 1.05 |

One bounded diagnostic was declared before running it: the PowerShell parent and
children used affinity mask 0x4 (logical CPU 2), unchanged binaries/workloads,
seven pairs and the same limits. At 08:23:08 UTC this also passed 22/24, but failed
different rows: forward_cancel collection 1.271429 (1.029412..2.25) and
random_complete collection 1.273973 (0.739496..2.197183). The two original failed
rows were 0.944444 and 0.985915 in that diagnostic. Pinning did not stabilize the
collection rows or establish the cause; no more samples, threshold relaxation,
GC tuning or passing-result substitution was performed. Both default failures
remain unaccepted. BO08 integrated acceptance and full CI remain open; BO05-BO07
implementation/correctness and the 19 execution gates are reported separately.
The older BO04/IP12 failures keep their own owners and are not closed by this run.

## Direct scalar execution attribution (Windows, 2026-10-27)

The user requested a deeper direct-only diagnosis after BO05-BO07. This is analysis
of the unchanged production candidate f6aa9cbc, documented at HEAD 179f0dbe; it
does not activate a runtime implementation phase or close BO08/IP12/BO04 gates.
The preceding paired result remains 179.7 us versus Lua 119.4 us (1.51x).
No new throughput comparison or speculative speedup is reported here.

### Measurement and address validation

Reuse the preceding report's i7-10700F / Windows 10 19045 / Rust 1.99.1,
LLVM 23.1.1, x64 MSVC, default workspace release profile and source/native plus
vendored Lua 5.4.9 configuration. There was no rebuild, feature/profile change,
affinity override or concurrent build/test. A fresh process with three warmups and
a ten-second main-thread sample window reproduced the earlier direct profile.
The executable SHA-256 remains
`01791dd819e30149109b0ac294bee2090c94bce0a6fd8fb18224c9a76d2c04e8`;
PDB SHA-256 is `5bb577186aa927be5ee73c8d858b3087836cab8d82ad54447f7faef7708a3d72`.
Both windows report zero script heap allocations and zero collections; independent
observer counting remains 30,005 Kagari / 40,006 Lua instructions.

Public PDB lookup alone leaves approximately 98% unresolved. Resolve these addresses
by comparing the same-build release COFF function sections with the exact sampled
PE image, masking only COFF relocation bytes. Find a unique full match of all
remaining bytes, translate the file offset through the PE section table to RVA,
then subtract the sampled module base from each RIP. Do not use preceding PDB
names as evidence. The unbounded loop matches 2,005 of 2,169 bytes; the other 164
are relocation fields. Its interval also agrees with the sampled unwind range.
The matching typed kernels are complete leaf-function sections:

| Function | PE RVA interval, half-open | Nonrelocation / total bytes |
| --- | --- | ---: |
| Unbounded `ExecutionCursor::execute_slice` | `0x00ffbfe0..0x00ffc859` | 2005 / 2169 |
| `integer_value::<0,32,true>` (add) | `0x017d0200..0x017d025b` | 83 / 91 |
| `integer_value::<2,32,true>` (multiply) | `0x017d06e0..0x017d073c` | 84 / 92 |
| `integer_value::<4,32,true>` (remainder) | `0x017d0d70..0x017d0e03` | 135 / 147 |
| `integer_binary::<7,32,true>` (less-than) | `0x017d19b0..0x017d19f8` | 68 / 72 |

Reproduction uses the existing sampler and MSVC 14.51.36260.0 dumpbin `/symbols`
and `/disasm` on the ordinary release `kagari_runtime` and `kagari_types` rlibs:

```text
uv run python scripts/profile_lua_comparison.py direct --source-forms --executable target/release/kagari-lua-benchmark.exe --pdb target/release/kagari_lua_benchmark.pdb
dumpbin /symbols target/release/deps/libkagari_runtime-<current-hash>.rlib
dumpbin /disasm target/release/deps/libkagari_runtime-<current-hash>.rlib
dumpbin /disasm target/release/deps/libkagari_types-<current-hash>.rlib
```

The first and repeated windows contain 3,904 / 3,921 successful RIP samples:

| Sampled function | First samples (%) | Repeat samples (%) |
| --- | ---: | ---: |
| Scalar interpreter loop | 2758 (70.65) | 2744 (69.98) |
| i32 remainder kernel | 584 (14.96) | 598 (15.25) |
| i32 add kernel, three sites per iteration | 275 (7.04) | 276 (7.04) |
| i32 multiply kernel | 127 (3.25) | 115 (2.93) |
| i32 less-than kernel | 70 (1.79) | 67 (1.71) |
| Everything else, including remaining unresolved addresses | 90 (2.31) | 121 (3.09) |

Within the interpreter, the following disjoint code ranges account for these
fractions of *all* samples. Offsets are relative to the unbounded function start:

| Instruction range / work | First (%) | Repeat (%) |
| --- | ---: | ---: |
| `0x1ad..0x222`: site lookup, operand shape, checked constant-slot input and argument arrangement | 22.98 | 23.00 |
| `0xb8..0xf4`: instruction bounds, PC publication, fetch and jump-table dispatch | 13.70 | 13.57 |
| `0x63..0xb8`: executing reset, cancellation, observer, collection and abandonment polling | 13.22 | 13.75 |
| `0x371..0x38c`: site kernel call and Result unpack | 5.35 | 4.54 |
| `0x38c..0x3d6`: site destination selection and conditional branch | 5.35 | 4.79 |

The remaining loop samples cover two-slot arithmetic, result reloads, destination
bounds, payload/initialization writes and branch-PC writes. These are sampled
locations, not exclusive causal cycle accounting, confidence intervals or removable
time. A load/branch sample does not identify its cache-miss or prediction cause;
call/return effects can appear at adjacent instructions. No hardware counters or
ablation timings were collected. In particular, 70% in the loop is not 70% opcode
dispatch, and a range's percentage is not a promised optimization gain.

### Confirmed work and its ownership

For `i += 1`, bytecode placement already emits one constant-operand operation.
Physical preparation still represents it as `OperandSite { index }`. The
[scalar loop](../crates/kagari-runtime/src/frame/cursor/scalars.rs) checks that
index, loads a 48-byte site's operand variant, checks the scalar bank and
initialization state, selects constant-left/right, calls a function pointer,
unpacks a stack-returned Result, selects Store versus Branch, and finally checks
and writes the destination. The successful arithmetic itself is a 32-bit add plus
overflow branch. This second generic execution layer is a concrete cost even
though operation/type selection already happened during preparation.

The complete direct invocation performs 25,001 site lookups, 30,001 indirect
numeric-kernel calls, 35,002 scalar payload reads and 25,003 payload writes.
Each current input read checks two slice bounds and an initialization flag; each
output write checks two bounds and writes that flag again. The generated machine
code retains these checks. Each of the 30,001 kernels validates both u64 carriers
against the i32 domain, including prepared constants: 60,002 carrier checks.
These are dynamic operation counts derived from the verified stream and inspected
handlers, not time shares.

The [kernel ABI](../crates/kagari-types/src/payload.rs),
`fn(u64, u64) -> Result<u64, &'static str>`, also survives unchanged in machine
code. The caller passes a stack result address; successful kernels store the
payload and success status there, and the caller reloads both. Caller-saved bank
and polling registers are restored after the call. Even the less-than predicate
uses this path, then checks the returned payload is boolean and selects a target.
Typed input selection therefore avoids dynamic type dispatch but does not avoid
carrier validation, the indirect call or its result transport.

The remainder kernel uses ordinary 32-bit `idiv`, not an i128 division helper.
It also tests zero divisor and signed MIN/-1 overflow. The prepared divisor 251
is passed as an ordinary runtime argument, so this kernel cannot exploit it as a
compile-time constant. Remainder is a real cost, but its whole kernel accounts for
about 15%, not the majority. The i128 source implementation has already optimized
to native 32-bit arithmetic on these paths; replacing it merely because its source
mentions i128 would misdiagnose the generated code.

The cancellation token check is now an inlined acquire load, contrary to the older
out-of-line cancellation observation in historical reports. The hot loop still
checks cancellation, observation, invariant collection-due state, automatic-GC
policy and asynchronous module abandonment on each final logical instruction.
It resets and republishes executing-PC state and advances the memory-resident PC.
No optional slice countdown survives in this unbounded instantiation. Zero actual
collections does not mean zero polling cost.

The mature comparison is the locally vendored PUC Lua **5.4.9** `lvm.c`:
`op_arithI`, `op_arith_aux`, `op_arithK`, `vmfetch`, and `dojump`. Integer add/mul
fast paths perform arithmetic inside their handlers after tag checks, without
Kagari's per-arithmetic function-pointer/Result boundary. Fetch uses a local PC
and hook/trap state; selected boundaries save state or refresh it. This explains
how Lua can execute more bytecodes yet less machinery per arithmetic operation.
Lua integers wrap and its hooks/GC/publication rules differ, so copying its unchecked
arithmetic or polling policy is not a valid Kagari optimization. The inspected
vendored source agrees with the relevant mechanisms in the official
[Lua 5.4.9 interpreter source](https://www.lua.org/source/5.4/lvm.c.html).

### Recommended finite next review

Prioritize prepared scalar execution shape and numeric-call transport. The semantic
owner stays `kagari-types`; reusable typed checked operations should remain the
single arithmetic implementation. `ExecutionModule::prepare` can establish the
operation, width, operand shape and destination kind once; review a bounded set of
closed handlers that can inline those shared semantics rather than rediscover the
shape and invoke an opaque pointer. For example, a prepared i32 add-constant and
an i32 compare-constant branch can carry their actual operands directly. Measure
handler/code-size growth as well as time; do not create direct-specific opcodes,
unbounded specialization or a duplicate unchecked arithmetic implementation.

Repeated carrier/bounds validation is a proof-ownership issue. Prepared constants,
validated frame inputs and successful typed results establish facts currently lost
at the generic u64/slice boundary. Review carrying those facts into the privately
borrowed, callback-free region. Public frame mutation, arbitrary debugger jumps,
resume and uninitialized states still require admission checks; their guarantees
cannot be deleted just because normal direct execution never exercises them.
Keep overflow, divisor checks where not proven, read-before-write failure commits,
trap origins and same-PC reuse semantics. Only after this boundary is explicit
should internal redundant checks be considered for elimination.

Review PC publication and invariant poll inputs second. Cancellation and asynchronous
abandonment remain genuinely dynamic; preserving the existing observation/trap
contract is mandatory. Do not obtain a benchmark win by disabling checks or moving
them to backedges without an explicit semantic decision. Any later implementation
needs focused checked-trap, source-free validation, scalar reuse, debug mutation /
side-entry, cancellation and cleanup coverage, followed by paired direct timing and
a small scalar regression set. This analysis makes no production code changes;
earlier failed gates and pending CI remain unchanged.


## Compact prepared scalar execution, Windows 2026-10-27

BO09-BO12 replace the second scalar interpretation layer with a private catalog
of **231 scalar + 7 base prepared variants**, each 24 bytes. This is an internal
execution enum, not the complete language opcode count: managed and cold boundary
operations retain their own representations. Canonical bytecode and its verification
contract are unchanged. Checked source-width integer semantics, casts/IEEE behavior,
read-before-write commits, manual entry and logical polling remain authoritative.
The catalog uses direct RR/RK arithmetic handlers, with conditions, count domains,
conversion domains and uncommon KR/KK shapes represented as data where appropriate.
The [architecture](architecture.md#numeric-scalar-library-ownership) and
[active plan](bytecode-optimization-plan.md#prepared-scalar-execution-follow-up-bo09-bo12)
own the implemented boundaries and checkpoint ledger.

### Method and measured images

Rust 1.99.1 (58cf8f9c3), LLVM 23.1.1, x86_64-pc-windows-msvc; Windows 10 build
19045, i7-10700F, 64 GiB RAM. Workspace release defaults (opt-level 3), default
Cargo parallelism/target directory, kagari source/native and mlua lua54/vendored
features; Lua is 5.4.9. Fresh sequential benchmark processes, one benchmark thread,
three warmups and eleven samples per process, seven alternating baseline/candidate
pairs. No concurrent build, test or profiler ran during timings. Builds reused the
existing cache; execution excludes compilation, artifact preparation and linking.
No workload, affinity, checking or GC-policy change was used to improve timings.

The baseline frozen by BO09 has executable SHA-256
`01791dd819e30149109b0ac294bee2090c94bce0a6fd8fb18224c9a76d2c04e8`.
The final BO12 image, based on BO11 commit `95e252b1` plus the native arithmetic
adapter repair, is
`207901bd948eb8fdf350f809d2e72bfa9a67efcc917a4418cc4c5abe4d73059b`.
A later Rustdoc-only restoration does not change the measured implementation.
Its cache-reusing release build took 106.9 seconds; this is not a clean-build
comparison. Forms setup process medians, baseline -> candidate: source-to-artifact
1,990.257 -> 1,974.511 ms; artifact preparation 475.129 -> 454.137 ms; program linking
43.698 -> 43.231 ms. These setup observations are separate from execution claims.

Reproduction uses `uv run python scripts/benchmark_lua.py --interpreter-only
--baseline-executable target/lua-comparison/scalar-before`, with `--source-forms`
for forms and `--execution-controls` for lifecycle controls. Numeric diagnostics
run each frozen/current executable with `--interpreter-only --numeric-matrix`,
alternating order for seven pairs and reducing the eleven execution samples to
a process median. Their Lua workloads are not matched parity evidence. Ratios
below are the median of seven within-pair candidate/baseline process-median ratios;
they need not equal the ratio of separately aggregated process medians.

### Execution results and retained failed evidence

Final forms and original runs began at 11:53:17 and 11:56:34 UTC. Every checksum
passes; all predeclared median-ratio gates pass (direct <=0.90, others <=1.05).
Every pair is retained, including outliers. Calls is close to the limit and noisy;
this does not establish a stable improvement for all nonscalar workloads.

| Workload | Paired ratio | Pair minimum..maximum |
| --- | ---: | --- |
| arithmetic | 0.64988 | 0.62685..0.68483 |
| arrays | 0.94832 | 0.65943..1.00802 |
| branches | 0.67414 | 0.65551..0.72793 |
| calls | 1.04618 | 0.93758..1.72638 |
| entry | 1.00977 | 0.99495..1.34740 |
| fibonacci | 0.97509 | 0.93967..1.07953 |
| maps | 0.99288 | 0.86851..1.32905 |
| byte_state | 1.00613 | 0.97615..1.02104 |
| capture_cell | 0.94950 | 0.93191..1.05328 |
| concrete_generic | 0.94628 | 0.92848..0.99598 |
| direct | 0.59821 | 0.56963..0.61734 |
| field | 0.97474 | 0.95298..1.06145 |
| helper | 0.94655 | 0.92408..1.10940 |
| host_callback | 0.99232 | 0.98878..1.01582 |
| interface | 1.02116 | 1.00830..1.04319 |
| native | 0.99323 | 0.97059..0.99897 |
| shared_generic | 1.01985 | 0.96386..1.05698 |
| string_calls | 0.99848 | 0.82936..1.11095 |
| string_constants | 1.02042 | 1.00453..1.03973 |

Direct process medians are **180.2 us -> 108.0 us**, versus **119.4 us Lua** in
the final candidate processes. The paired ratio 0.59821 is a 40.2% reduction against
the Kagari baseline. This particular scalar workload now runs about 9.5% below
Lua's elapsed time; it is not a general language/runtime performance claim.

The final numeric run began at 12:01:36 UTC. Ratios: f32 0.73657, f64 0.76686,
i8 0.81866, i16 0.82504, i32 0.80076, i64 0.81934, u8 0.83151, u16 0.83135,
u32 0.83518, u64 0.81728. All ten pass; the f64 maximum pair ratio 1.32126 remains
in the evidence. All 24 owned-control rows passed at 11:46:34 UTC on the earlier
BO11 image, worst admit/roots_1_tail_cancel exactly 1.05. That evidence is retained:
these fixtures execute typed RR/RK scalar paths, not the subsequently repaired
Value adapter/shared KR/KK arithmetic. They were not rerun as a fresh all-green sweep.
These are explicitly two-image results, not 53 rows remeasured on the final image.

The first integrated image
`f88ec00086c0e49e816503226c4e602136bd200c2a9085ee5c104decd58930f0`
FAILED host_callback 1.0725 (1.0596..1.0968) and native 1.0886 (1.0655..1.0936).
The bounded investigation found ordinary `numeric::binary` reconstructing prepared
operation metadata despite already knowing a full-width Value carrier, followed
by shared domain decoding. Both forms call the same host_step using this adapter.
The repair admits the carrier once and invokes the existing typed arithmetic owner;
shared integer operations also compute their domain layout once. No second numeric
semantics implementation or extra opcode was added. The final affected-suite rerun
passes host_callback 0.99232 and native 0.99323. Lua host_callback invokes the same
Rust helper and also benefits; its changed time must not be attributed to Lua VM
changes. The failed image/results remain recorded in the plan. Earlier BO04/BO08/
IP12 lifecycle/collection failures are independent and remain unresolved.

### Representation and remaining direct costs

Direct remains 11 static / 30,005 dynamic Kagari instructions, versus 40,006 Lua
instructions, with five logical locations and four physical scalar slots. The
following retained payload accounting derives from the unchanged canonical/layout/
debug arrays and removal of the six 48-byte scalar sites:

| Direct retained payload | BO09 bytes | BO12 bytes |
| --- | ---: | ---: |
| Canonical instructions | 1584 | 1584 |
| Prepared instructions | 264 | 264 |
| Scalar sites | 288 | 0 |
| Logical locations / reuse guards | 40 / 80 | 40 / 80 |
| Scalar payload and initialization banks | 36 | 36 |
| Origins / selections | 1240 / 80 | 1240 / 80 |
| Slot and debug slot types | 10 | 10 |
| Debug spans, lines and points / local ranges | 2760 / 216 | 2760 / 216 |
| Control targets | 8 | 8 |
| Selected retained payload sum | 6606 | 6318 |

This is selected array/bank payload, excluding unchanged headers/shared metadata
and allocator overhead, not RSS. Hot prepared instructions/sites decrease from
552 to 264 bytes. PE SizeOfCode increases from 26,165,760 to 26,203,136 bytes
(+37,376 bytes, 36.5 KiB, 0.143%); whole executable bytes are 32,863,232 -> 32,900,096.
The unbounded scalar function grows from 2,169 to 25,615 bytes, and the bounded
function is 28,025 bytes. Direct handlers exchange interpreter code size for less
per-operation interpretation; the measurements do not prove 238 universally optimal.

A separate ten-second main-thread direct profile at 12:02:37 UTC uses the final
image and matching runtime archive, three warmups and 3,866 samples. PDB names alone
leave most samples unresolved. Exact COFF-to-PE matching masks only relocation
bytes and verifies 25,227 of the unbounded function's 25,615 bytes; it locates 3,719
samples (96.2%) in that function. The remaining 147 (3.8%) are other/unmapped code.
Instruction/GC observation runs separately: 30,005 instructions, zero collections,
zero script heap-object allocations and zero live-object delta.

| Final unbounded function offsets | Sample location | Samples / all samples |
| --- | --- | ---: |
| 0x72..0xc4 | Boundary control and cancellation/observer/abandonment polling | 945 / 24.44% |
| 0xc4..0xf5 | Checked fetch, local PC advance and dispatch | 702 / 18.16% |
| 0x5d30..0x5d5d | Checked result writeback, initialization and loop backedge | 529 / 13.68% |
| 0x2c86..0x2ce7 | i32 immediate remainder, operand/domain/divisor checks | 577 / 14.92% |
| 0x4d5e..0x4da5 | i32 immediate add, operand/domain/overflow checks | 306 / 7.92% |
| Other offsets in scalar loop | Other arithmetic, comparison and paths | 660 / 17.07% |
| Other/unmapped | Outside matched scalar loop | 147 / 3.80% |

These are sampled locations, not causal cycle percentages or removable-time
predictions. In particular 403 samples occur just after idiv; sampling skid and
dependencies prevent assigning them all to that single instruction. Remainder is
shared workload work, not an explanation by itself for a Kagari/Lua difference.
The prepared constant is runtime data, so `% 251` still uses native i32 idiv.

Assembly confirms direct arithmetic is inline, with no numeric function-pointer
calls, site lookup or stack-returned kernel Result. Successful add retains a real
overflow branch. PCs stay local and publish at the common segment exit. Operand
bank bounds/initialization and source-domain checks remain; writes still update
initialization. A comparison's condition is data, so condition selection remains
inside its typed handler. These are the next concrete proof/dispatch costs to
review if needed. Eliminating bank/domain checks would require a private validity
contract covering all writers, debugger/manual side entry and resume; removing
polls would change semantics. Neither is authorized as an automatic extra phase.

Focused numeric, frame, source-free, slicing, trap/observation, cancellation and
reentry contracts pass, as do structure, formatting, diff and workspace all-target
Clippy checks. The BO12 repair specifically passes six runtime numeric boundary,
three payload and eleven VM numeric tests. Full workspace/backend/feature/platform
correctness remains CI-only and unrun; local evidence does not close that acceptance.

## BO13-BO16 scalar loops, Windows 2026-10-27

Production `41d613a9` reaches the arithmetic target: four scalar slots, eight static
run instructions and four instructions per iteration. Independent counting confirms
300,008 -> 200,007 instructions for n=50,000, including unchanged main/run calls.
All 19 original/forms execution gates pass against the freshly frozen `36c4419c`
image. Arithmetic's median paired ratio is 0.77414 (22.6% lower elapsed time),
against the predeclared <=0.90 target; every other row is below its 1.05 limit.
This completes BO13-BO16 locally, not full CI or earlier BO04/BO08/IP12 gates.

### Environment and matched measurement

Windows 11 Pro 10.0.26300, Intel Core i9-12900K, 24 logical CPUs, 66,843,692 KiB
visible memory. Rust 1.99.1 (`58cf8f9c3`, LLVM 23.1.1), Cargo 1.99.1 (`5f94df478`),
x86_64-pc-windows-msvc; workspace release opt-level 3, default target and Cargo
parallelism. Kagari source/native and vendored PUC Lua 5.4.9 are compiled in;
execution is interpreter-only. No environment, affinity or priority overrides.
Use fresh process/runtime state, three warmups, eleven execution samples per
process and seven alternating baseline/candidate pairs per suite. Compilation,
preparation and linking are excluded from execution. Builds, tests, counting and
profiling do not overlap throughput measurements. Baseline freezes began at
14:03:28 and 14:05:42 UTC; paired original/forms runs at 14:17:02 and 14:20:37 UTC.

Baseline executable SHA-256:
`de8d76c7c7754cab3c2a1ed990d5d14d8f832640dc79880f9c21f826993ed154`.
Candidate executable SHA-256:
`d7c2457b345ba1ead750d3d51b72da2618032efaca5a982a552facefdb180697`.
Both paired suites use that same candidate image with a clean tracked working tree.
The existing [driver](../scripts/benchmark_lua.py) and
[gate checker](../scripts/check_benchmark.py) validate unchanged inputs, samples,
hashes and every checksum. Reproduction commands and the frozen-image procedure are
in the [active plan](bytecode-optimization-plan.md#measurement-contract-and-acceptance).

Times below are medians of seven process medians, in microseconds per complete
workload. Paired ratios are medians of seven within-pair ratios, so they need not
equal the quotient of the displayed medians. All pairs, including the broad
arithmetic/branches/field minima, remain in the evidence; no retry or outlier
deletion was used. Small changes on unaffected paths are not proven speedups.

| Workload | Before us | After us | Lua us | After/Lua | Paired after/before | Pair minimum..maximum |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| arithmetic | 644.700 | 503.700 | 366.500 | 1.37 | 0.77414 | 0.62756..0.79492 |
| arrays | 7646.400 | 7526.600 | 84.300 | 89.28 | 0.97594 | 0.94961..1.00737 |
| branches | 1006.600 | 869.700 | 670.800 | 1.30 | 0.87085 | 0.64380..0.91947 |
| calls | 5723.100 | 5496.800 | 212.700 | 25.84 | 0.95938 | 0.93771..0.99669 |
| entry | 2.007 | 1.953 | 0.037 | 52.35 | 0.96385 | 0.88853..0.98826 |
| fibonacci | 12741.400 | 12326.800 | 303.300 | 40.64 | 0.96241 | 0.94027..1.01224 |
| maps | 5841.100 | 5835.200 | 83.900 | 69.55 | 0.98372 | 0.95176..1.00883 |
| forms_byte_state | 14612.700 | 14225.700 | 135.800 | 104.75 | 0.97352 | 0.94658..1.00853 |
| forms_capture_cell | 6295.700 | 6190.200 | 130.800 | 47.33 | 0.98231 | 0.95324..1.01143 |
| forms_concrete_generic | 2823.000 | 2757.100 | 112.600 | 24.49 | 0.96455 | 0.94363..1.01708 |
| forms_direct | 68.700 | 68.700 | 55.200 | 1.24 | 0.99858 | 0.93452..1.02537 |
| forms_field | 706.200 | 701.900 | 102.000 | 6.88 | 0.99886 | 0.80180..1.02294 |
| forms_helper | 2769.300 | 2739.700 | 114.500 | 23.93 | 0.98924 | 0.88234..1.03230 |
| forms_host_callback | 3484.900 | 3409.200 | 224.200 | 15.21 | 0.98011 | 0.92644..1.00781 |
| forms_interface | 9140.900 | 8707.200 | 122.700 | 70.96 | 0.95811 | 0.89056..0.96163 |
| forms_native | 3445.600 | 3370.000 | 114.800 | 29.36 | 0.97084 | 0.96100..1.00564 |
| forms_shared_generic | 13222.700 | 12677.900 | 118.400 | 107.08 | 0.95674 | 0.93835..0.99480 |
| forms_string_calls | 7269.300 | 7051.100 | 137.600 | 51.24 | 0.96651 | 0.93919..0.99405 |
| forms_string_constants | 4087.100 | 4002.400 | 80.400 | 49.78 | 0.97649 | 0.89710..1.00409 |

Entry's underlying medians are 1.9526 us versus 0.0373 us; rounding explains the
displayed ratio. Original workload sizes/checksums and the 5,000-iteration forms
are unchanged. Kagari checked i32 arithmetic and Lua integer arithmetic have
different overflow contracts; these bounded workloads match results. Byte-state
compares typed u8 storage/casts with Lua's integer/table representation.
`forms_native` compares a Rust callback with a Lua script helper and is diagnostic,
not equivalent native-boundary performance. `forms_host_callback` invokes the same
checked Rust helper from both engines. No numeric/backend matrix was added here.
Earlier i7 timings cannot establish gains or regressions on this i9 machine.

Candidate build wall time is 18.964 seconds; forms reuses it in 0.389 seconds.
Arithmetic setup process medians, before -> after: source-to-artifact 801.678 ->
809.058 ms; artifact preparation 266.779 -> 271.042 ms; program link 25.815 ->
25.307 ms; runtime initialization 20.1 -> 19.5 us. These are separate setup
diagnostics, not costs hidden in the 503.7 us execution time.

### Instruction shape, generality and storage

The unchanged arithmetic body is `sum = sum + i % 97; i = i + 1`. Rotation now
runs before copy propagation, preserving its closed-condition eligibility. Numeric
constant selection removes a shared literal temporary only when the next retained,
same-origin home initializer anchors its original observation. Runtime preparation
and instruction handlers are unchanged. Equivalent final bytecode is:

```text
load i, 0
load sum, 0
branch 0 < n -> body, else -> done
body:
  rem t, i, 97
  add sum, sum, t
  add i, i, 1
  branch i < n -> body, else -> done
done:
  return sum
```

The entry guard preserves negative/zero-trip behavior. Every successful result
commits separately; overflow, cancellation, source observations and sliced resume
keep their contracts. Final instruction observation reports BinaryOperands 100,000,
PredicateBranch 50,001, Binary 50,000, LoadConst 3, Return 2 and Call 1. Both logical
and physical run slots fall from five to four. Lua's independent hook reports
250,007 instructions: fewer Kagari dispatches now coexist with higher elapsed time.
One bytecode in either VM is not an equal unit of machine work.

Unchanged branch/call/array sources also benefit: static run instructions/slots
change 19/11 -> 18/10, 11/7 -> 10/6 and 35/23 -> 32/20 respectively. All three
array loops use conditional backedges. The pre-call sum and managed object snapshots
remain. These results demonstrate general compiler rules, not a benchmark match;
they do not remove the much larger collection/call-boundary costs in the table.

Run's canonical instruction payload falls 1,296 -> 1,152 bytes (144 bytes each);
prepared instructions 216 -> 192 bytes (24 bytes each); physical scalar payload plus
initialization bytes 45 -> 36. These are selected payload arrays, excluding metadata,
headers and allocator overhead, not RSS. Whole executable bytes change 32,896,000 ->
32,904,192; PE SizeOfCode 26,198,528 -> 26,204,672 (+6 KiB). The runtime scalar-loop
function remains 25,615 bytes; this compiler change does not shrink each handler.

### Remaining arithmetic costs

A separate ten-second profile began at 14:21:44 UTC using the exact candidate image
and PDB SHA-256 `de19f3024a7083ebc21029b4c9c5e48bf5c5ef97a89aaff601c5a55e43aa54ef`.
The [sampler](../scripts/profile_lua_comparison.py) waits for warmup completion,
briefly suspends the main thread at jittered 1.5..2.5 ms intervals and records RIP;
it does not collect inclusive stacks. There are 4,219 samples and zero capture
errors. Counting occurs after the sampling window. The window records zero GC
collections, zero script heap-object allocations and zero live-object delta.

PDB names leave 99.41% unresolved. Match the same-build runtime rlib's COFF function
section against the exact PE, masking only relocation bytes, then map samples by
module-relative address. The unique match verifies 25,227 of 25,615 bytes at RVA
`0x010a9310..0x010af71f`; it agrees with unwind function bounds and attributes 4,177
samples (99.00%) to unbounded `ExecutionCursor::execute_slice`. The bounded
28,025-byte function at RVA 0x010a2590 has zero samples. Reproduce symbol/assembly
inspection with `llvm-nm` / `llvm-objdump` on the matching release runtime archive;
the sampler command is:

```text
uv run python scripts/profile_lua_comparison.py arithmetic --executable target/release/kagari-lua-benchmark.exe --pdb target/release/kagari_lua_benchmark.pdb
```

Offsets below are relative to the matched unbounded function, half-open. The
comparison path is split by LLVM's shared tails; retain these exact intervals when
reclassifying the saved sample addresses.

| Machine-code offsets | Sample location | Samples / all |
| --- | --- | ---: |
| 0x72..0xc4 | Boundary control and cancellation/observer/abandonment polling | 697 / 16.52% |
| 0xc4..0xf5 | Checked fetch, PC advance and opcode dispatch | 663 / 15.71% |
| 0x5d30..0x5d5d | Checked result writeback, initialization and backedge | 409 / 9.69% |
| 0x2c86..0x2ce7 | i32 immediate remainder, operand/domain/divisor checks | 1416 / 33.56% |
| 0x8ac..0x924; 0x15cd..0x15dd | i32 register add, operand/domain/overflow checks | 365 / 8.65% |
| 0x4d5e..0x4da5 | i32 immediate add, operand/domain/overflow checks | 174 / 4.12% |
| 0x4452..0x44d9; 0x59b2..0x59ba; 0x5a0a..0x5a1b; 0x5c48..0x5c61 | i32 predicate operands, condition dispatch and checked target | 453 / 10.74% |
| Outside the matched function | Other/unmapped | 42 / 1.00% |

Polling/fetch/writeback account for 41.93% of sampled locations. The remainder
handler still loads 97 from prepared runtime data and executes `idiv r8d`; it is
not native compile-time constant division. Of all samples, 1,053 occur at 0x2cdf,
immediately after idiv. Sampling skid and data dependencies prevent interpreting
these as precise division cycles or removable-time percentages. Remainder is also
work Lua must perform, so this profile alone does not attribute the entire Lua gap.

The source/assembly evidence now places remaining arithmetic cost in executing the
required operations, rather than excess counter transport. The scalar loop retains
bank bounds/initialization checks, i32 payload-domain validation, checked overflow
and target checks. Comparison also selects its condition inside the typed handler.
GC, script allocations and repeated function calls are not hot-loop causes here.
Review prepared execution validity and dispatch ownership before any further runtime
optimization: a private proof would have to cover every writer, public frame access,
debug/manual side entry and resume. Preserve overflow commits and per-instruction
polling. Constant-divisor specialization needs a separate general design and measured
case; no new opcode, unchecked path or reduced cancellation frequency follows from
these samples automatically. These questions remain in the existing plan ledger.

The selected compiler/VM contracts and required structure/fmt/workspace Clippy/diff
checks pass; details are in the BO14-BO15 checkpoint. Only documentation changed
after that implementation checkpoint. Exhaustive tests remain CI-only and unrun.
