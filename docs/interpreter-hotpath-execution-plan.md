# Interpreter execution architecture plan (HP00-HP06)

Status: active, authorized by the user on 2026-10-10; HP00 is complete; HP01 is next.
The [roadmap](implementation-roadmap.md#interpreter-performance-follow-up) records
activation; this document owns the finite phase order and progress ledger.

## Objective and evidence

Make repeated interpreter operations reuse checked execution facts, with short
dynamic checks at the boundaries where those facts can change. Preserve correctness
guarantees while replacing redundant validation and preparation paths. The completed
[VE00-VE09 record](interpreter-value-execution-plan.md) remains historical acceptance;
their runtime mechanisms are explicitly reviewed below and replaced where they
conflict with the target architecture. Historical acceptance does not exempt a
design from replacement.

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

## Architecture decision and scope

The user requires architecture-first optimization, including correction of earlier
optimizations that merely bypass an unsuitable boundary. This revision supersedes
the unstarted hotspot-by-hotspot phase sequence from `72715594`. Adding call-site
caches or individual fast-path branches is not the primary implementation strategy.

Current structural findings, verified against production code:

- Prepared calls cover Function/ModuleFunction, while interface/shared/closure paths
  still assemble arguments, environments and adapters through different entry paths.
- Internal interface dispatch constructs RootedInterfaceMethod and refreshes roots;
  root publication and frame entry can traverse executable metadata again.
- Scalar execution borrows code/banks, but managed copies/constants become Boundary
  operations and return to the canonical instruction dispatcher. A frequent ordinary
  operation consequently repeats session/frame admission and driver state handling.
- Enum layout identity is fragmented across module slots/applications. VE09 improves
  comparison inside that model; it does not remove repeated structural comparison.
- Selected stdlib collection bindings bypass owning SDK conversion locally, while
  other paths retain it. The distinction needs one explicit ownership/effect contract.

These findings justify changing execution ownership and prepared representations.
They do not justify deleting necessary dynamic checks, assuming every cache is bad,
or rewriting unrelated compiler/tooling subsystems.

### Target ownership boundaries

The names below describe responsibilities, not mandatory new structs or crates.
Implement them through existing owning modules and replace obsolete internal APIs.

| Layer | Owns | Must not do in ordinary repeated execution |
| --- | --- | --- |
| Verified portable program | Types, physical layouts/transfers, declared access and logical PC origins | Hold runtime heap IDs or infer source types in the backend |
| Runtime-linked executable descriptors | Applied call/layout identities, resolved signatures/adapters, exact version dependencies and prepared operation operands | Rebuild type trees/signatures or compare complete layout graphs for every call/read |
| Active execution scope | Admitted session, frame/window views, current PC and active dependency roots | Re-enter the host-handle protocol for each internal value or operand |
| Execution driver | Entry, call/return transitions, GC/observation, waiting/resume and failure cleanup | Rediscover waiting/native state and revalidate unchanged scope for every ordinary operation |
| Host/foreign boundary | Owning handles, runtime/generation/access validation, scoped conversion and reentry | Expose an unchecked entry or let borrowed internal values escape |

**Executable descriptors:** separate immutable call/type facts from per-invocation
receiver/argument values. Statically closed facts are prepared at linking; genuinely
runtime-dependent applications are admitted when first encountered and subsequently
referenced through exact checked identities. Unify ownership and publication of those
facts before choosing memoization policy. Keys include supplying program/type scopes,
member/application and operation witnesses; printed types or raw addresses are not
identities. Runtime stores remain traced and reclaim cycles with old versions.

**Active roots:** extend the existing frame-window and metadata tracing model so
an active call retains dependencies directly. Host-retained RootedInterfaceMethod
continues to own a host root; internal dispatch uses the active frame's ownership.
Validate dependency graphs at publication/admission and validate each new lazy edge
before installation. Retain owner/generation checks on dynamic access. A past graph
validation is reusable only while its complete dependency lifetime remains proven.
No new collector or independent permanent-root registry is introduced.

**Prepared operations:** use one prepared execution model with explicit operation
and transition kinds. Common managed/scalar/field/array operations consume physical
operands directly. Canonical bytecode remains the portable verification/debug origin,
not a second interpreter repeatedly decoding already-prepared hot operations. Keep
specialized scalar handlers where measurements justify them; sharing a model does
not require putting all handlers into one enormous match or losing scalar speed.

**Call protocol:** direct, shared, interface and closure calls resolve differently,
but converge on the same physical argument/result transfer and frame lifetime model.
Native calls retain their host boundary where required. Selected closed native
primitives use a verified effect/access contract and the same semantic operation
implementation, rather than per-benchmark shortcuts or method-name recognition.

### Release invariants and review rules

For each check moved or removed, record the fact it protects, its proof owner,
invalidating events and remaining release enforcement. Follow the contracts for
[execution](spec/execution.md), [failure](spec/failure-semantics.md),
[runtime](spec/runtime.md), [traits](spec/traits.md) and
[collections](spec/collection-access.md).

- Foreign/stale values, dynamic receiver choice, runtime ownership, generations,
  current bounds/access, alias mutation and checked arithmetic remain validated.
- Before GC, callbacks, observation, frame/window growth or parking, publish live
  roots/PC and release conflicting borrows. Re-admit after such transitions.
- Protect return values and new root edges before retiring old ones. Preserve
  left-to-right evaluation, mutation commit, trap order and completed effects.
- Preserve logical debugger positions, slicing, current cancellation observation
  boundaries, call depth and cleanup through reentry, traps and suspension.
- Prefer scoped Rust borrows and checked identities; no raw-pointer bypass is planned.
  Debug-only fields may cross-check proofs but cannot supply missing release safety,
  add roots or perform required effects. Follow the [diagnostic policy](interpreter-value-execution-plan.md#debug-only-invariant-diagnostics).

A cache is acceptable only after its semantic owner, exact identity, invalidation,
traced lifetime, memory bound and miss behavior are explicit. It must reuse a stable
result, not conceal repeated reconstruction of that result. No default single-entry
call-site cache is prescribed by this plan. Any retained local specialization must
have a contract, a measured benefit and a documented reason the common path cannot
serve it; slower cases may not become a second semantic implementation.

The finite implementation scope includes the layers above and migration of their
existing consumers. Public async/language semantics, JIT, compiler inference and
unrelated tooling are unchanged. Value/collector/enum representation is reviewed,
not assumed perfect: keep 16-byte Copy values and ordinary traced enums as the
starting baseline. If profiling establishes representation as a remaining limiting
cost, specify the replacement and its bounded ABI/root migration here before coding
it. Do not claim architectural completion by permanently excluding that finding or
launch a silent general escape-analysis/collector rewrite. The shared generic Add
lowering failure remains visible; the frozen identity benchmark cannot close it.

## Mandatory review of earlier optimizations

This is an initial disposition, not evidence that the replacement is implemented.
HP00 checks the earlier IP/NE tracks as well as VE00-VE09 on every affected path;
HP06 must close each row with retained/replaced/deleted code and evidence.

| Existing mechanism | Initial disposition | Owning phase and retirement criterion |
| --- | --- | --- |
| IP execution windows, NE scalar banks/kernels, VE01-VE04 compact handles/Value | Retain foundations; audit their admission interfaces | HP02/HP04: one active ownership protocol; no duplicated per-operand admission |
| VE03 runtime-local constant pool and borrowed strings | Retain storage semantics; replace repeated execution-time module resolution | HP01/HP04: linked constant operands preserve lazy materialization and version lifetime |
| Native type preparation and closed-method application caches | Merge into runtime-linked descriptor ownership | HP01/HP03: no competing application/signature caches on the migrated call path |
| VE05 prepared fields and scoped Vec/Map helpers | Generalize ownership/effect contracts; remove operation-specific bypasses made redundant | HP04: script/SDK adapters share storage semantics and explicit borrow boundaries |
| VE06 concrete-call transfers and scalar-return specialization | Generalize transfers/frame lifetime across call kinds | HP03: interface/shared/closure paths no longer maintain separate packing/retirement protocols |
| VE07 scalar-region/ordinary-boundary split | Retain efficient scalar kernels; replace the managed-operation architectural split | HP02/HP04: common prepared operations do not bounce to a second decode/admission path |
| VE09 same-program enum structural-equality shortcut | Transitional optimization to replace, not extend | HP01/HP05: prepared layout identity/admission replaces repeated full-layout comparison; remove superseded shortcut |
| Repeated metadata graph validation at root refresh/frame entry | Move proof to publication and explicit boundary admission | HP01/HP02/HP03: active dependencies are traced without rebuilding host-style metadata roots |

Do not remove a useful optimization merely because it is specialized. Conversely,
its previous benchmark win or completed phase status does not justify retaining
obsolete duplicate machinery after migration. Any additional workaround found on
these paths receives a concrete disposition and phase owner in this same table.

### HP00 migration map and invariant ownership

The audit follows an ordinary interface call through
`executor/dispatch.rs::dispatch_call` in the VM and runtime `objects/application`,
`execution_metadata/links`, `frame/shared`, `frame/calls` and `frame/returns`.
For example, `receiver.forward<i32>(value)` currently selects a host-rooted method,
reconstructs binder/entry environments, resolves its signature, publishes metadata
roots and then validates frame entry. The target prepares immutable facts for the
exact application once; each invocation supplies its receiver/value through the
common frame transfer protocol. A different receiver implementation or supplied
nominal version selects a different descriptor. It does not mutate an earlier one.

| Protected fact | Target proof owner and invalidation | Remaining enforcement / migration |
| --- | --- | --- |
| Type/layout meaning and exact supplying versions | Runtime-linked descriptor owns normalized type/layout identity and pinned provenance; new publication/reload creates distinct identities | HP01/HP05: preserve foreign scope and genuine cross-version compatibility admission; replace repeated module-slot/layout equality, including VE09 shortcut |
| Method signature, entry environment and operation witnesses | Immutable applied descriptor includes selection, supplied type scopes and operation identities; different keys require preparation | HP01/HP03: replace closed-only `MethodSelection` application cells and repeated `prepare_method_application` / `prepare_shared_environment`; validation failure cannot publish partial facts |
| Executable dependency validity and lifetime | Checked publication of every initial/lazy edge, with descriptors reachable from existing metadata tracing | HP01: consolidate `MetadataCache` publication; preserve owner/generation lookups, abandoned-lease checks and collection/retirement; no untraced global root or permanent cache |
| Active operand ownership | Runnable scope borrows admitted session/frame/window; observation, GC, growth, reentry and parking end that scope | HP02: replace repeated internal `ExecutionStack::current/current_mut` admission; keep checked external access, stale-session rejection and cancellation/debugger cadence |
| Live arguments, captures and return values | Common call transition owns transfers and roots before retiring caller/callee state | HP03: generalize `PreparedScriptCall` and existing window transfer; remove internal `RootedInterfaceMethod` construction, temporary argument packing and separate shared/interface retirement; host-retained method handles remain |
| Managed constants, fields and primitive collection operations | Linked prepared operands plus explicit effect/access contract; current aliases/bounds are dynamic | HP04: replace `ExecutionInstruction::Boundary` fallback for migrated common operations and repeated canonical decode; retain lazy string storage, scalar kernels, checked arithmetic, trap order and storage borrow checks |
| Enum nominal identity, tag and payload | Prepared producer/consumer layout admission; rooted bounded payload borrow for each read | HP05: replace owned snapshots/pattern reconstruction and VE09 structural shortcut; preserve mismatch/failure order and ordinary boxed Option until representation evidence warrants a scoped decision |

`SessionStore` is already an indexed slot/generation store; the older profile's
HashMap attribution does not describe this baseline. IP/NE window banks, 16-byte
Copy values and VE03 traced string storage remain useful foundations. The defective
boundary is repeated admission/preparation, not the existence of owner/generation
checks. Native `TypeArgument` derived parameter/variant preparation, selected
collection borrowing helpers and direct scalar returns must be reviewed with their
new owner, rather than retained as competing semantic paths. Every row in the
retrospective table remains open until its owning phase supplies code evidence.

## Phase order and acceptance

Implementation is authorized. Execute the bounded sequence without asking again
for its routine work. Each checkpoint replaces a coherent responsibility
and must build/pass selected contracts. Temporary migration bridges need a named
removal phase and cannot survive HP06; obsolete internal compatibility APIs are not
required. A performance miss triggers ownership/representation review before another
local tweak. Record rejected candidates and unmet gates; do not silently expand scope.

### HP00 — Baseline, architecture audit and replacement map

Preserve a checked release baseline/hash at `target/hp00/baseline-executable` with
reproduction metadata. Reuse the frozen benchmark bodies; make counting/sampling
reproducible through existing tooling rather than only untracked probe sources.
Measure preparation counts, graph walks, transitions, allocation and retained memory;
include changing receivers/type arguments and separate original arrays/maps profiles.

Trace each operation through preparation, admission, execution and retirement. Finalize
the invariant map, descriptor/root ownership and keep/merge/replace decisions above.
Determine which current APIs will disappear; a list of proposed caches is insufficient.
Acceptance is a finite migration map grounded in code/profiles, with recoverable
baseline and unchanged checksums. No full correctness rerun or speedup claim is needed.

### HP01 — Runtime-linked executable identities and publication

Owners: runtime module execution/layouts, execution_metadata, scoped type/application
preparation and their checked contract inputs.

Establish a shared owner for applied call/layout descriptors and their exact supplying
scopes. Normalize duplicate same-program layout references during preparation; handle
cross-generation structural compatibility at explicit admission, retaining full checks
where no proof exists. Publish completed descriptors and lazy edges transactionally.
Generalize application preparation beyond closed nongeneric methods through this model.
Memoization, if required, is an implementation of this ownership model, with bounded
retention and no receiver values hidden in persistent code metadata.

Acceptance: repeated identical applications reuse prepared facts without rebuilding
type environments/signatures; changed types/witnesses/versions cannot alias. Collection
and retirement reclaim descriptor cycles. Record cold preparation/descriptor memory,
and name the old preparation/cache paths that were replaced.

### HP02 — Active execution ownership and explicit transitions

Owners: runtime session/frame storage, frame values/cursor, roots and VM driver.

Separate persistent/parked execution state from an admitted runnable scope. Reuse
session/frame/window access within that scope. Model call, return, native entry,
await/resume, observation, collection and failure as explicit transitions; ordinary
progress need not rediscover all states. Use existing managed frame banks and traced
program/environment edges as active roots, independent of host root leases.

Acceptance: repeated operand access does not reacquire/validate the active session
and frame through the public checked path. GC/observation/reentry/growth invalidate
transient views correctly; async and synchronous execution share semantics and exact
cleanup. Required polling/debugger boundaries remain unchanged. Remove redundant
admission layers, rather than adding an unchecked method beside each checked one.

### HP03 — One prepared call and return protocol

Owners: runtime frame calls/shared/arguments/returns, objects application/method/calls,
native entry adapters and VM call dispatch.

Migrate direct, interface, shared and closure script calls to HP01 descriptors and
HP02 frame ownership, with physical argument/result transfers. Method selection and
captures remain call-kind-specific inputs; parameter copying, receiver placement,
root publication, depth accounting and retirement have one implementation owner.
Internal calls do not create temporary owning RootedInterfaceMethod handles merely
to keep already-active dependencies alive. Host entry/escaping handles keep their
checked ownership protocol and converge after admission.

Acceptance: warm interface/shared loops stop per-invocation application preparation,
argument/root-container allocation and unchanged graph traversal. N versus 2N probes
separate unavoidable values from preparation traffic. Monomorphic and alternating
receiver/type cases, scoped generics, captures, result adapters, reentry and reload
pass. Concrete-call and scalar-return optimizations are retained through shared
contracts or removed as redundant, not copied into additional call-specific paths.

### HP04 — Complete the common prepared operation model

Owners: prepared runtime execution/cursor, VM dispatch and stdlib/native storage adapters.

Migrate managed copies, constants, field access, primitive Vec index reads/writes and
String byte length into prepared operations with physical operands and explicit
transition requirements. Lazy string creation remains an allocation transition;
warm constant loads use their linked owner. Keep efficient scalar handlers within
this model and share semantic kernels/storage operations with SDK/native adapters.
Recognize native primitives through exact verified binding/effect/access contracts,
not names or host-provided purity assertions. Callback/custom implementations retain
real transitions; a non-scalar value alone is not a reason to use full external entry.

Acceptance: these common operations avoid canonical re-decoding and repeated scope
admission. String loops allocate no new string objects; arrays retain current bounds,
access and alias behavior. Compare strings, original arrays, byte-state and scalar
controls. Delete migrated duplicate handlers and obsolete per-binding shortcuts;
measure execution metadata size and cold setup as well as throughput.

### HP05 — Unified layout admission and enum access

Owners: runtime applied layouts/enum storage/native result publication and VM patterns.

Use HP01 layout identities for producer and consumer descriptors, including equivalent
layouts referenced through different module slots. Perform genuine cross-scope/version
compatibility at admission and retain the resulting checked proof where valid. Tag
and payload reads use rooted, bounded storage access rather than owned snapshots.
Remove the superseded VE09 comparison shortcut and duplicate pattern preparation.

Acceptance: the frozen Map path no longer repeats full layout comparisons or allocates
pattern/snapshot descriptors per lookup. Keep all nominal/payload/foreign-generation
checks, source-free native results and failure order. Count ordinary Option allocation
separately. If it remains the limiting cost, document a representation decision and
bounded migration requirement instead of declaring a snapshot optimization sufficient.

### HP06 — Retire old paths and evaluate the architecture

Close every retrospective row with concrete code evidence; delete obsolete adapters,
caches, duplicate validation and temporary migration bridges. Durable opt-in diagnostic
tooling may remain; it must be absent from ordinary builds. Review module ownership,
LOC and public surfaces. Confirm external entry and real dynamic slow paths remain
checked. Architectural acceptance requires the new ownership model to serve changing
receivers, types and versions, not only frozen monomorphic fixtures.

Resolve integration failures, run the final local checks once the whole scope is ready,
and exercise changed admission/lifetime contracts with debug assertions on and off.
Measure all 16 unchanged Lua workloads and allocation/transition/memory diagnostics
against HP00. Update current architecture only for implemented changes. Separate local
correctness, architecture migration, measured benefit, full CI and Lua parity outcomes.
If parity fails, identify whether remaining cost is representation, protocol, operation
count or kernel work before proposing more optimization. Record a finite follow-up;
never claim the architecture sound solely because this checklist is finished.

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
`DEVELOPER_DIR=/Library/Developer/CommandLineTools`. The [benchmark diagnostic instructions](../benchmarks/lua-comparison/README.md#execution-architecture-diagnostics-hp00)
record counting/sampling commands; these runs are separate from throughput. Report cold preparation and retained cache memory separately,
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
| HP01/HP03 | runtime `objects/application/tests.rs`, `execution_metadata/*/tests.rs`; embed `generic_reload`, `generic_associated_types`; VM interface allocation and native-boundary interfaces/functions/GC contracts |
| HP02/HP04 | runtime session/frame values; VM execution frames/debugger/host-effects; embed string methods, collection access, async lifecycle/debugger; source-free artifact and native reentry/control contracts |
| HP05 | embed enum payloads/native enums; VM native-boundary enums, hash handles and GC payload failures |

At each implementation checkpoint run selected tests, affected Clippy, formatting,
`uv run --locked scripts/check_structure.py` and `git diff --check`. Keep tests focused;
do not split the whole suite across commands to run it at every phase. HP06 final
local acceptance runs `cargo test --workspace`, `cargo clippy --workspace --all-targets
-- -D warnings`, `cargo fmt --all -- --check`, structure and diff checks. Report actual
commands/results and GitHub CI status separately. Documentation-only checkpoints need
only content/link/diff checks. Use `Phase: HPxx` in implementation commit trailers.

## Progress ledger

- [x] Evidence-based plan recorded and revised to architecture-first scope.
- [x] HP00 — Architecture audit, baseline and replacement map.
- [ ] HP01 — Runtime-linked executable identities and publication.
- [ ] HP02 — Active execution ownership and transitions.
- [ ] HP03 — Unified call/return protocol.
- [ ] HP04 — Common prepared operation model.
- [ ] HP05 — Unified layout admission and enum access.
- [ ] HP06 — Old-path retirement and architecture evaluation.
- [ ] Complete GitHub CI acceptance.
- [ ] All 16 matched workloads reach Lua parity.

2026-10-10: original plan at `72715594` followed diagnosis `e2f93e1b`. The user then
required architecture review before local optimization and correction of earlier
workarounds. Phase responsibilities were rewritten before implementation; the
retrospective table includes IP/NE and VE mechanisms. No runtime migration, new
performance result or new build/test failure is claimed by this documentation change.

2026-10-10 HP00 in progress: baseline revision `f97b4095`, SHA-256
`a10ea34693c9a113b97cf2f1e2af1906c07a2b175e1bfcd209bb055c01cecc22`,
preserved at `target/hp00/baseline-executable`; metadata/checksums are alongside it.
Both `--check --interpreter-only` and `--check --source-forms` pass. Environment:
Apple M1 Max, 32 GiB RAM, 10 logical CPUs, macOS 26.6.2 arm64; rustc 1.98.1
(`48a229cea`, LLVM 22.1.8), Cargo 1.98.1; workspace release, warm build cache,
default target/parallelism, source/native SDK, vendored PUC Lua 5.4.8. Build prefix:
`DEVELOPER_DIR=/Library/Developer/CommandLineTools`. The preserved executable has
no counters. New diagnostic tooling uses explicit opt-in features and rejects
throughput mode; the frozen fixture bodies/checksums are unchanged.

Fresh warm diagnostic counts (one complete script execution after three warmups):

| Workload | System allocation requests | Method preparations | Environment allocations | Metadata validation graph entries | Ordinary dispatch boundaries |
| --- | ---: | ---: | ---: | ---: | ---: |
| Original arrays, 2,000 elements | 12,059 | 0 | 0 | 0 | 16,003 |
| Original maps, 1,000 keys | 30,207 | 0 | 0 | 0 | 12,003 |
| Interface, 5,000 calls | 70,040 | 0 | 0 | 5,001 | 10,002 |
| Shared generic identity, 5,000 calls | 315,599 | 5,000 | 10,000 | 20,002 | 20,002 |
| String constants, 5,000 iterations | 7 | 0 | 0 | 0 | 25,000 |
| String calls, 5,000 iterations | 7 | 0 | 0 | 0 | 45,000 |
| Byte state, 5,000 iterations | 36 | 0 | 0 | 0 | 25,002 |

The generic case requests 26,257,537 bytes, with a +176,240 net byte delta and
+484/+242 live environment/application records after the counted execution;
14 collections occur within it. These are current live deltas, not peak memory
or permanent leaks. Original maps allocate 2,001 heap objects with five collections;
arrays allocate one with two collections. Strings allocate zero new heap objects.
Each boundary count is a prepared-region exit, not an opcode or host crossing;
ordinary direct calls also exit the region. Graph counts exclude cheap Program-only
validation and collector traversals. Zero graph entries therefore do not mean no
validation or GC work.

The additional changing-receiver/type matrix passes independent checksums. Increasing
iterations from 2,500 to 5,000 changes fixed generic method preparations from 2,500 to
5,000 and environments from 5,000 to 10,000. Alternating i32/i64 receivers and i32/i64
type arguments has the same linear preparation counts (four combinations). Interface
graph validation entries grow from 2,501 to 5,001 for a fixed receiver and 2,502 to
5,002 for alternating receivers. These measurements support replacing repeated
application preparation and active-root admission; they do not establish a speedup.
The SDK's ordinary entry argument path is currently unsupported, so probe wrappers
pass iteration counts through normal script calls rather than changing that API.
Raw counters: `target/hp00/{original,forms}-diagnostics.log`; reproduction commands
are in the benchmark README. Sampling and checkpoint validation are recorded below.

Original-workload sampling completed separately in
`target/lua-comparison/20261010T011111Z-macos-profile/`: arrays has 3,991 main-thread
samples, maps 4,026. Leading collapsed top-of-stack counts include
`ResourceState::termination` (501/373), `ExecutionStack::validate_top` (470/309),
`ScalarCursor::execute` (338/265) and `ensure_execution_allowed` (229/147).
`poll_await`, `current/current_mut` and ordinary dispatch are also prominent.
Array stacks reach scoped `CallContext::array_argument`/push and checked storage;
Map stacks additionally reach `enum_snapshot`, layout application/comparison and
allocation. This independently supports HP02/HP04 for original arrays, and both
the common execution protocol and HP05 enum work for maps. Sampling does not
separate all inline costs or prove the exact origin of every allocation.
Observer counts are 72,030/63,030 Kagari instructions versus 42,012/24,012 Lua
instructions per arrays/maps execution, collected outside the sampling window.
Different instruction semantics prevent comparing those as equal units of work.

Source-form sampling also passes for shared_generic, interface, byte_state,
string_calls, capture_cell and field in
`target/lua-comparison/20261010T011203Z-macos-profile/`. Shared generic stacks show
allocator/free prominently; the other paths prominently retain frame/state/driver
work. Capture-cell now has fresh sampling evidence, rather than allocation counts
alone. Field storage access is also visible; retaining the prepared field/scalar
kernel foundation is justified while HP02/HP04 replace surrounding admission.
These source-form profiles use six Kagari warmups (three paired plus three profile
warmups); the driver metadata was corrected to distinguish these from the three
original-workload warmups. No observer/counting allocator runs during sampling.


HP00 acceptance: baseline/hash, reusable diagnostics, original/form sampling,
changing-key scaling probes and the finite migration/invariant map are complete.
Validation passed: release builds with and without `diagnostics`; both diagnostic
suites and independent checksums; instrumented/ordinary CLI mode guards; existing
`cargo test --locked -p kagari-lua-benchmark -- --test-threads=1`; strict Clippy for
runtime/benchmark all targets with diagnostic features, and benchmark all targets
with default features; formatting; structure (991 Rust files, zero violations or
exceptions); Python syntax; 702 local Markdown links; `git diff --check`. No full
workspace suite or GitHub CI ran. A constant-assert Clippy failure was fixed with
configuration-specific CLI branches; no build/test error is carried. HP00 changes
measurement support and documents architecture decisions, with no throughput claim.
Cold descriptor preparation, retained memory after reload/retirement and new publication
contracts are HP01 acceptance work; net byte deltas above cannot replace them.
