# Interpreter execution architecture plan (HP00-HP06)

Status: active, authorized by the user on 2026-10-10; HP00–HP02 are complete; HP03 is in progress.
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
- [x] HP01 — Runtime-linked executable identities and publication.
- [x] HP02 — Active execution ownership and transitions.
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


2026-10-10 HP01, applied-call ownership checkpoint (in progress): immutable method
applications now belong to linked module records rather than receiver snapshots or
closed-only operation cells. `execution_metadata/application_key` identifies the
method/view/adapter, exact receiver environment, supplied lexical type scopes and
operation witness identities. Nominal type provenance includes runtime owner and
pinned program generation, normalized across members of that program; neither names
nor raw addresses serve as identities. Different receiver values can reuse the same
facts without being retained by them. The existing GC application store still owns
checked slot/generation IDs and immutable signature/environment/adapter records.

`module/applications` publishes only after owner/dependency validation. Each module
retains at most 128 applications with FIFO eviction; eviction removes an optional
program edge, not active frame/host roots. The bound limits retained polymorphic keys
and provenance rather than claiming an optimal cache size. Program tracing now visits
these application edges, so old program/application/environment/operation cycles are
collectible after their last real root. Superseded `MetadataCache<ApplicationId>`
fields, `cache_method_application` and `cached_application` are removed, not kept as
a second path. Remaining receiver/parent-interface lazy cells are separate contracts.

Application dependency graphs are immutable after those cells are removed. Publication
records their checked flat program dependencies. Lookup and GC retention check that
these versions remain available: abandoning an unpublished provider expires optional
cache retention; publishing it preserves validity. Independently rooted active facts
still undergo full graph validation. This does not grant a blanket validation stamp
to mutable interface graphs or extend candidate leases. Flat checks remain in release;
no raw-pointer bypass, missing root or debug-only safety proof is introduced.

Focused ownership evidence so far: exact application reuse across different receiver
values, distinct scalar/nested nominal supplying types and versions, old handles across
reload, FIFO eviction preserving an active handle, 128-entry retention, receiver
snapshot reclamation, whole retired-cycle reclamation, and both published/abandoned
witness providers. Existing stale/foreign/publication/collector-atomicity tests remain.
Runtime application tests pass (12 selected contracts). Checkpoint measurements
and additional focused checks are recorded below.

This is partial HP01, not phase acceptance. Shared-function preparation, normalized
layout descriptors/admission, consolidation of native type preparation and remaining
publication consumers still require migration. HP02/HP03 must remove internal
host-style method roots and repeated graph validation; HP04/HP05 retain their planned
operation/enum work. The immutable descriptors introduced here must become inputs to
that common protocol, not a permanent alternate execution path.


HP01 ownership checkpoint evidence, 2026-10-10: frozen shared-generic identity
performs one method preparation/two environment allocations on its first 5,000-call
execution, then zero/zero on warmed execution. The four changing receiver/type
combinations prepare four/eight initially and zero/zero warm, at both 2,500 and 5,000
iterations. Warm shared-generic system requests fall from HP00's 315,599 to 160,050;
requested bytes fall from 26,257,537 to 13,924,513. Normal GC remains enabled (14
collections in the HP00 measured call, zero here). Metadata graph validation remains
20,002 entries per warm call, so the active-root protocol is still unfinished.
Fresh first-call net allocation is +14,682 bytes for the shared probe, including
frame/storage setup; it is not an isolated descriptor size. Post-collection tests
show 128 cached applications plus one independently rooted evicted application,
then 128 after dropping that handle, with no receiver object retained. Precise
retained-byte/repeated-retirement measurement remains part of full HP01 acceptance.
Counters/logs and environment metadata are under `target/hp01/`; the README documents
cold/warm measurement and the changed fresh-runtime probe setup.

Uninstrumented paired source-form run:
`DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/hp00/baseline-executable`.
The saved baseline has no single-form selector, so the existing frozen source-form
suite is run intact; entry/host diagnostics are not promoted into the parity gate.
Workspace release/default features, target and parallelism; same M1 Max/toolchain
as HP00; reused build cache, fresh processes; three warmups and eleven samples per
process; baseline/candidate/candidate/baseline order; normal GC/host entry included,
compilation/linking excluded. All checksums pass. No concurrent agent build/test ran.
Candidate binary SHA-256 is
`b462933df76bc8ba6dc73ce0fd8b8579e55deb87fe7c689b65f180cb79ed2d98`
(saved at `target/hp01/candidate-executable`). Raw data/metadata:
`target/lua-comparison/20261010T013246Z-forms-paired/`.

| Selected workload/control | HP00 median, us | Candidate median, us | Candidate / HP00 |
| --- | ---: | ---: | ---: |
| Shared generic identity | 22,930.334 | 17,656.000 | 0.7700 |
| Interface | 10,783.604 | 11,029.812 | 1.0228 |
| Direct scalar | 350.625 | 363.083 | 1.0355 |
| Helper | 3,232.750 | 3,229.041 | 0.9989 |
| String calls | 13,603.000 | 13,332.271 | 0.9801 |
| Byte state | 7,358.375 | 7,477.188 | 1.0161 |

Shared-generic process medians are 23,075.250/22,801.916 us for baseline and
17,667.834/17,514.416 us for candidate; the 23% reduction exceeds their observed
variation. Other source-form medians remain within 5% of baseline. This limited
checkpoint benefit is not phase/architecture/parity acceptance: shared-generic
still costs 152.76x its measured Lua median. No original-suite throughput claim
or final 16-workload acceptance is made here.

The new independent `witness_application` diagnostic uses a working `T: Ord`
comparison body, without changing the frozen fixtures or hiding the separate shared
Add lowering gap. It exposes unfinished producer ownership: 5,000 warm calls still
prepare 5,000 methods/10,000 environments (1,110,956 allocation requests, 30,002 graph
validations). `bind_operations_in` repeatedly creates equivalent operation groups;
their distinct checked IDs correctly miss the application index. Increasing the
cache limit would conceal the wrong boundary. Next HP01 work must move witness/shared
preparation into runtime-linked facts and reuse canonical admitted operation identities,
then rerun this probe alongside the changing-key cases. This is required remaining
scope, not an accepted permanent exception or an extra follow-up goal.

Checkpoint validation passes: runtime application/publication/lifecycle contracts
(12); type-provenance contracts (4); embed generic-associated-types (12) and
source-free generic reload (2); VM interface allocation/cleanup (3) and native generic
interface applications (2); strict runtime/benchmark all-target diagnostic-feature
Clippy; final benchmark Clippy after the witness probe; release diagnostic build and
all original/source-form/protocol checksums; ordinary release paired run above;
formatting; structure (993 files, no violations/exceptions); 703 local Markdown links;
`git diff --check`. Old-cell test compile errors and one probe type annotation were
resolved; no carried build/test error remains. Full workspace/CI checks are reserved
for their designated acceptance scope and were not run. HP01 stays in progress.

2026-10-10 HP01, witness/shared preparation checkpoint (in progress): applied methods,
operation witnesses and shared-call environments now use one linked-program publication
and retention protocol (`module/descriptors.rs`, replacing `module/applications.rs`).
Each index retains at most 128 entries across all lexical scopes. Publication validates
immutable dependency graphs before installing an edge; eviction and abandoned-provider
expiration only remove optional retention, preserving independent roots. Recoverable
reservation failures leave no partial descriptor or accumulating empty scope.

Witness keys include the supplying member, lexical environment identity and the complete
checked witness facts. Shared environments additionally distinguish target owner/entry
and the complete checked call contract. Borrowed lookups avoid cloning these contracts
on hits. Generational environment checks reject stale/foreign scopes before lookup;
the index does not itself root a caller merely because its ID is in a key. Immutable
operation bindings share their storage, with copy-on-write only during preparation.
`bind_operations_in` no longer resolves targets and allocates groups for identical
applications; `prepare_shared_environment` owns environment allocation/publication for
both bytecode entry and source-free host function binding. Neither index stores receivers
or script Values. Hash derives for checked contracts change no serialized schema.

Diagnostic evidence uses the HP00 machine/toolchain/profile/default parallelism above,
an incremental release build with `diagnostics`, a fresh loaded runtime per probe,
one cold call, two warmups and a measured fourth call. Compilation is excluded. The
frozen Lua workloads are unchanged. A separate `shared_application` probe adds a generic
method forwarding to a generic comparison function; all results check the independent
integer checksum. Both 2,500 and 5,000 iterations give the following preparation counts:

| Probe | Cold method / shared / witness preparations | Cold environment allocations | Warm preparations / environment allocations |
| --- | --- | --- | --- |
| Fixed unconstrained application | 1 / 0 / 0 | 2 | 0 / 0 |
| Four receiver/type combinations | 4 / 0 / 0 | 8 | 0 / 0 |
| Constrained comparison application | 1 / 0 / 1 | 2 | 0 / 0 |
| Shared comparison application | 1 / 1 / 2 | 3 | 0 / 0 |

At 5,000 warm constrained calls, allocation requests fall from the previous checkpoint's
1,110,956 to 495,466; graph validation entries fall from 30,002 to 25,002. The new shared
probe still makes 535,466 allocation requests and 30,002 graph validations. Frozen
shared-generic identity makes 155,050 requests (previously 160,050) and still 20,002
graph validations. These are protocol/allocation counts, not throughput improvements.
Internal root/frame admission and enum materialization still scale with execution and
remain HP02/HP03/HP05 work. Cold net bytes include execution stacks, collection capacity
and live Values; they do not establish isolated descriptor memory acceptance. Logs:
`target/hp01/witness-shared-diagnostics.log` and `witness-shared-build-final.log`.

The existing source-free shared-function contract now checks reuse without environment
growth, invocation after reload and complete environment reclamation when the retired
owner loses its last callable root. Its former immediate-reclamation assertion failed
because the current program now owns preparation; the test was updated to require
retired-owner reclamation rather than dropping that guarantee. The witness lifecycle
contract checks three reload/retirement cycles, bounded retention across 160 lexical
scopes, survival of independently rooted evicted operations and rejection of a stale
scope even when its key remains indexed.

Remaining HP01 scope: normalized applied layout identity/admission, consolidation of
native type preparation and remaining publication consumers, and isolated retained-byte
accounting. Internal host-style call roots remain scheduled for HP02/HP03. No phase,
architecture, CI or Lua-parity acceptance is claimed by this checkpoint.

Checkpoint validation passes: runtime application/publication contracts (13), followed
by the strengthened bounded-scope/three-retirement contract; type provenance (4);
embed generic-associated-types (12) and serialized/source-free generic reload (2);
VM interface allocation/cleanup (3) and source-free shared function binding (1).
Strict all-target Clippy passes for types/contract/runtime/benchmark with diagnostics,
and final runtime/VM Clippy covers the changed lifecycle tests. Formatting, structure
(994 files, zero violations/exceptions), 703 local Markdown links and `git diff --check`
pass. Ordinary release is restored, and original interpreter-only plus frozen
source-form checksum checks pass. No carried build/test failures remain. No full
workspace suite, GitHub CI matrix or new paired throughput run was performed.
The ordinary executable is preserved as `target/hp01/witness-shared-executable`, SHA-256
`904289770aa24ee12440c19acd5b5f7f22a6508f1670bfbaddcf2e264fa89346`.

2026-10-10 HP01, linked layout identity checkpoint (in progress): `ProgramDescriptor`
now owns canonical struct/enum layout tables prepared from the verified, normalized
members before publication. Dense identities are meaningful only within that exact
program descriptor and aggregate kind. Hash buckets select candidates; complete layout
equality, including declaration, arguments and field/variant contracts, establishes
equivalence. The tables keep bytecode locations and IDs instead of copying layouts or
retaining runtime resources. Their size is bounded by the linked program's layout count.

Template applications resolve a matching linked identity on preparation, retaining it
with their existing application record. Prepared field operands use the same identities.
This replaces the VE09 same-program enum whole-layout equality shortcut: duplicate
member layouts now compare their prepared identities, without hashing or walking layout
trees during access. An identity cannot bypass variant, runtime/program or lexical
payload-scope checks. Cross-generation compatibility and applications with no matching
linked layout still use complete `TypeView` compatibility; migration of those explicit
admissions and runtime-only/scoped layout preparation remains required HP01/HP05 work.

The runtime layout ownership contract verifies duplicate member layouts, matching
generic/closed applications, distinct versions, variant mismatch and collection of
retired executable records while immutable type facts remain usable. Focused module
record tests (6), VM native enum contracts (6), embed enum payload/ABI/reload contracts
(4) and serialized native enum contracts (2) pass. Strict affected types/contract/runtime
all-target Clippy, formatting and structure (995 files, zero violations/exceptions)
pass. Initial test-only portable/runtime type inference errors were fixed; the identity
module sits directly under its program owner, avoiding unnecessary wider visibility.

The next preparation audit has a concrete remaining producer: `frame/native.rs`
still invokes `LinkedNativeFunction::apply` for every generic native entry, rebuilding
signatures/adapters/selections. Its selected calls currently create host-style pinned
roots in `native/selected.rs`. Persistent reuse must first replace those owning roots
with traced descriptor edges; storing the existing pinned functions in a program index
would create permanent retention. This is within the existing HP01/HP03 migration,
not justification for another per-call cache or a new scope expansion.

Paired ordinary-release evidence: `target/lua-comparison/20261010T020217Z-paired/`
(`results.json`, raw CSVs, build/toolchain/machine metadata), reproduced with
`DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py
--interpreter-only --baseline-executable target/hp01/witness-shared-executable`.
The unchanged original seven workloads run baseline/candidate/candidate/baseline,
with reversed workload order in the second pair, three warmups and eleven execution
samples per route/process. All checksums pass. Default release features/profiles and
Cargo parallelism are unchanged; execution is single-threaded and processes sequential.
Build time (20.126 s) is excluded. Candidate is preserved as
`target/hp01/layout-identity-executable`, SHA-256
`b707d0edc523b542a4326028642328ec1694972b243595a3a04eedb8c4189908`.

| Original workload | Previous checkpoint median (us) | Candidate median (us) | Candidate / previous |
| --- | ---: | ---: | ---: |
| Entry | 1.452 | 1.443 | 0.9944 |
| Arithmetic | 2,411.855 | 2,517.771 | 1.0439 |
| Branches | 2,908.625 | 2,927.250 | 1.0064 |
| Calls | 6,433.313 | 6,374.000 | 0.9908 |
| Fibonacci | 13,100.354 | 13,001.375 | 0.9924 |
| Arrays | 5,075.980 | 5,158.271 | 1.0162 |
| Maps | 6,106.521 | 5,919.187 | 0.9693 |

Map process medians are 5,998.291/6,146.916 us for the baseline and
5,925.000/5,916.375 us for the candidate. The observed aggregate reduction is 3.07%,
with control variation up to 4.39%; it is not evidence of a large isolated speedup.
Candidate maps remain 86.57x Lua. Map linking medians are 2,520.854/2,590.480 us
(baseline/candidate, six setup samples each), reported separately from execution.
These timings do not satisfy retained-byte accounting, which remains outstanding.
Final diff/content checks and all 703 local Markdown links pass; no carried build/test
error remains. No full workspace suite or GitHub CI run was performed. HP01 and all
later architecture/CI/Lua-parity acceptance gates remain open.

2026-10-10 HP01, generic native application checkpoint (in progress): generic native
entry now consumes a published `NativeApplication` containing its exact supplying
member/environment, applied signature, result adapter and selected call facts. The
existing linked-descriptor protocol validates new edges before publication and retains
at most 128 native applications across all scopes per member. Keys include import and
generational environment identity; every lookup still checks runtime/module availability
and the environment handle. The first preparation checks the template binder contract;
the unchanged frame-entry contract also checks its supplied environment. Completed
signatures retain their prepared type arguments instead of discarding scalar-only facts
and preparing them again through the native signature cell.

`native/selected.rs` now produces resolved executable edges rather than owning host
roots for internal native preparation. `LinkedCallable` owns their shared traversal,
also used by `StoredSelection`; explicit escaping typed handles still promote to pinned
ownership. Before invoking Rust, the native frame publishes its descriptor in the
existing execution window. Collection traces the window independently of the optional
program index, so callbacks/reentry can evict an application without invalidating an
outer call. Window release removes that active edge on ordinary/trapping cleanup.
The descriptor contains no argument/receiver Values. This replaces per-entry
`LinkedNativeFunction::apply` and its internal selected-call roots, rather than caching
those roots and creating an uncollectable program cycle. General active-frame and
call/return migration remains HP02/HP03 work.

The execution-window ownership test fills 160 distinct preparation scopes, verifies
that an evicted active descriptor and its otherwise-unrooted selected environment
survive collection, then verifies reclamation after window release and complete
environment reclamation after program retirement. Active-root accounting includes the
single execution window and no extra host root. Existing source-free shared binding
and typed selected-call GC/reload contracts preserve independently retained host calls.

Diagnostic probes add fixed and changing-receiver/type generic `Vec<T>::push` calls
outside the frozen benchmark sources. On the documented HP00 machine/toolchain/default
parallelism, incremental release with diagnostics, fresh runtime per probe, one cold
call and a measured fourth call after two warmups, both 2,500 and 5,000 iterations give:

| Native probe | Cold method / shared / native preparations | Cold environment allocations | Warm preparations / environment allocations |
| --- | --- | --- | --- |
| Fixed receiver and element type | 1 / 1 / 1 | 3 | 0 / 0 |
| Four receiver/type combinations | 4 / 4 / 4 | 12 | 0 / 0 |

Final counts are in `target/hp01/native-preparation-diagnostics-tracing.log`; compilation
is excluded. At 5,000 iterations the fixed probe still makes 705,372 warm allocation
requests and 25,002 graph validations; changing types makes 705,616 and 25,004. Their
array/iterator values and remaining execution-boundary work still scale with execution.
These counts establish preparation reuse, not a throughput speedup or isolated retained
descriptor bytes. Existing fixed/changing/witness/shared probes and all checksums pass.

Focused checks pass: execution windows (6), metadata publication/lifecycle contracts
(31), VM source-free shared binding (1), typed selected methods including generic object
results/GC/reload (3), embed generic reload (2), strict runtime/benchmark all-target
diagnostic-feature Clippy, formatting and structure (996 files, zero violations or
exceptions). The initial test-location privacy error, root-accounting assertion and
redundant-field Clippy finding were resolved. The ordinary release binary is restored;
no carried build/test failure remains. No full workspace/CI matrix or new paired timing
run is claimed. HP01 still requires scoped/runtime-only layout identity and explicit
compatibility admission, completion of the remaining preparation-owner audit, and
isolated descriptor-memory accounting; later phase and Lua-parity gates remain open.

Final ordinary original/source-form checksum checks and all 703 local Markdown links
pass, as does `git diff --check`. The restored release executable is preserved at
`target/hp01/native-application-executable`, SHA-256
`60e9def12e3d8668fef39683ab9667b2cd35e4d75164e68c89117ccfcf06563d`.

2026-10-10 HP01, shared layout-scope checkpoint (in progress): interpreter aggregate
construction, native enum preparation and public object binding now use the same
`module/layout_scope.rs` preparer. Checked argument identities include closed type
expressions and exact supplying program versions; `TypeArgument` prepares that identity
once per supplied argument. Independently created equivalent arguments and different
members share a declaration/argument scope under the program-root runtime record.
Foreign arguments/owners are rejected, and different supplying generations cannot hit
the same scope. Template/payload compatibility remains checked by the entry owners.

The existing descriptor index now supplies bounded storage independently of edge
semantics. Executable publication still wraps entries with validated dependencies and
traces them; pure layout facts contain no executable roots or GC metadata edges. Layout
scopes retain at most 128 entries per program. This also replaces the old unbounded
per-member struct/enum application maps with 128 entries per aggregate kind across all
IDs. Capacity is reserved before insertion/eviction. Pure-fact retention is optional;
failure to retain, a borrowed store or retired records leaves complete descriptors
usable without an executable admission. The three duplicate binding constructors are
removed. Existing exact-descriptor matching benefits from shared scopes; arbitrary
runtime-only cross-member layouts and cross-version comparisons retain full checks.

The layout lifecycle contract supplies equivalent nominal arguments independently,
checks sharing across members, prepares 160 distinct scopes/struct/enum applications
and observes early entries released through weak references. It then retains old
provenance across reload and collection, verifies executable records retire, checks
that old/new supplying versions stay distinct and rejects foreign scopes. This is
retention/lifetime evidence, not isolated retained-byte accounting.

New diagnostic probes, separate from the frozen parity sources, return nested
`Wrapped<Holder<Record>>` through a generic interface method. The changing probe
alternates `Record` and `OtherRecord`. Under the recorded HP00 machine/toolchain,
default Cargo parallelism and incremental release diagnostics build, with a fresh
runtime per probe, one cold call and a measured fourth call after two warmups:

| Probe (both 2,500 and 5,000 iterations) | Cold method preparations | Cold layout-scope preparations | Warm method / layout-scope preparations |
| --- | ---: | ---: | --- |
| Fixed nominal argument | 1 | 2 | 0 / 0 |
| Alternating nominal arguments | 2 | 4 | 0 / 0 |

All diagnostic checksums pass (`target/hp01/layout-scope-diagnostics.log`). At 5,000
iterations, fixed/changing probes still request 4,161,197/4,161,171 allocations and
both perform 20,002 metadata validations in the warm call. Operand/type-argument
resolution, identity hashing, compatibility and execution protocols remain material
per-execution work; HP02–HP05 own their migration. These counts show scope-preparation
reuse, not throughput improvement. Compilation is excluded, and cold net bytes include
unrelated execution storage. No new Lua timing or descriptor-memory acceptance is claimed.

Focused checks pass: module records/layout lifetime (7), lexical type arguments (4),
metadata publication/lifecycle (31), VM native enum contracts (6), embed enum payloads
(4), native enums (2) and generic associated types (12). Strict runtime/benchmark
all-target Clippy with diagnostics passes. The initial structure check's diagnostic
import findings were fixed (997 Rust files, zero violations/exceptions); the diagnostic
source's match-scrutinee parse error was fixed by binding the constructed value before
matching. No grammar changes were made. HP01 remains open for runtime-only layout
normalization/explicit admission, the remaining preparation-owner audit and isolated
memory accounting; HP02–HP06, GitHub CI and Lua parity are not accepted.

Final host-object checks also pass: nominal applied fields and old-version handle/cache
retention (two focused tests). Formatting, all 703 local Markdown links and final diff
checks pass. The restored ordinary release original/source-form checksum checks pass;
these cold checks are not throughput samples. The executable is preserved at
`target/hp01/layout-scope-executable`, SHA-256
`f4b8fb1ced68b8314fb724e242b8e8ec9d3de5abb4e9553867ccd1350ccfa84d`.
No carried build/test error remains; no full workspace or GitHub CI run was performed.

2026-10-10 HP01, complete applied-layout identity checkpoint (in progress): layout
applications now prepare physical shape and immutable lexical scope together. The
old writable `StructLayoutRef`/`EnumVariantRef` environment fields are removed;
interpreter, native enum and host object consumers receive complete descriptors and
read-only type bindings. The per-member application key includes the exact immutable
scope ID, so warm hits also reuse the prepared canonical identity.

The program-root record lazily owns scope preparation and complete applied-identity
normalization; other members carry only their application indexes. Normalization keys
include the entire applied layout and every scoped argument's type/provenance identity.
A scoped application reuses a linked ID only if its arguments exactly match the linked
scope, including nominal supplying versions. Runtime-only equivalent shapes normalize
across members, including equivalent scoped and unscoped preparations. The program
retains 128 normalization entries per aggregate kind in addition to its 128 scopes;
each member retains 128 complete applications per kind. No receiver values, executable
leases or hidden GC edges are introduced.

Dynamic layout IDs start beyond the immutable linked table; scope/layout counters
never recycle IDs after eviction. Checked counter exhaustion falls back to complete
compatibility without wrapping. Borrowed stores and retired records retain detached
readability. The removed unscoped-only fast-path guards are justified by the now-sealed
complete identity: scope cannot change after preparation, changed supplying versions
have different keys, and same-ID comparison still requires the exact program descriptor
(and the enum variant). Without that proof, full `TypeView` checks remain. Genuine
cross-generation compatibility admission is still outstanding HP01/HP05 work.

The existing layout identity/lifetime contracts now verify a scoped producer against
a linked concrete consumer, runtime-only nested applications through two members,
scoped/unscoped identity agreement, changed supplying generations, wrong-binder rejection,
non-reuse of IDs after 160 applications, and readability after executable retirement.
This extends the relevant contracts rather than adding parallel smoke tests.

Under the same recorded HP00 machine/toolchain/default parallelism, incremental release
diagnostics, fresh runtime per probe, one cold call and a measured fourth call after
two warmups, the unchanged nested generic probes compare with the preceding checkpoint:

| Warm probe | Previous allocation requests | Current allocation requests | Current GC objects |
| --- | ---: | ---: | ---: |
| Fixed nominal, 2,500 iterations | 2,080,627 | 1,055,627 | 7,501 |
| Alternating nominals, 2,500 iterations | 2,080,616 | 1,055,616 | 7,501 |
| Fixed nominal, 5,000 iterations | 4,161,197 | 2,111,197 | 15,001 |
| Alternating nominals, 5,000 iterations | 4,161,171 | 2,111,171 | 15,001 |

The difference is 410 allocation requests per iteration; object counts and checksums
are unchanged. Cold scope preparation remains two/four records for fixed/changing types
at either loop length, and zero when warm. At 5,000 iterations both probes still perform
20,002 metadata validations and 60,003 slow boundaries. Input resolution, remaining type
checks and execution protocols are not eliminated by layout identity. Final evidence:
`target/hp01/applied-layout-identity-diagnostics.log`; the preceding checkpoint's log
remains preserved. These are allocation counts, not throughput or isolated descriptor
retained bytes; compilation is excluded. No new Lua timing claim is made.

Focused checks pass: module identity/lifetime (7, including the final root-store ownership
change), VM native enums (6), embed enum payloads (4), source-free native enums (2),
generic reload (2), host nominal applied fields (1), strict runtime/benchmark all-target
Clippy with diagnostics, formatting, structure (998 files, zero violations/exceptions),
all 703 local Markdown links and diff checks. An intermediate unused import was removed.
No carried build/test error remains. HP01 still needs explicit compatibility admission,
completion of the preparation-owner audit and isolated descriptor-memory accounting.
No full workspace or GitHub CI matrix has run; later phases and Lua parity remain open.

The ordinary release executable is restored and preserved at
`target/hp01/applied-layout-identity-executable`, SHA-256
`1e4bf43f04bfcf2813733636b92dc05bf0d2e1ec3f861e2de475b342ff9501bc`.
Original/source-form checksum checks pass. These are cold correctness checks, not
throughput measurements; the frozen benchmark definitions remain unchanged.
The focused associated-type family/GC-object/default-method execution contract also
passes against the final implementation. Final worktree diff checks pass.

2026-10-10 HP01, layout compatibility admission checkpoint (in progress): struct and
enum descriptor matching now share `module/layout_admission.rs`. Equal complete IDs in
one program stay on the direct path. A different pair first undergoes the existing full
`TypeView` comparison; only successful, complete prepared pairs publish reusable type
compatibility evidence. Missing IDs retain the full checked path. Enum variant and runtime
owner checks still precede admission, and per-value GC generation, access, bounds and
current payload checks remain at their existing owners.

The consumer's immutable `ProgramDescriptor` owns a lazily allocated, thread-safe index
of at most 128 relations across both aggregate kinds. Keys include producer version and
both complete layout IDs; a weak producer descriptor must match the exact source Arc as
well. This is pure type evidence, not executable admission: it survives runtime-record
retirement when immutable facts remain held, but holds no Values, scopes, native links,
program leases or strong producer references. It therefore needs no GC root/edge. The
existing runtime-owned application/signature stores continue to own executable edges.
Changing versions/identities cannot reuse a proof; eviction, contention, poisoned cache
access or failed optional retention falls back to full comparison. No lock crosses that
comparison. New versions receive empty evidence, and never-recycled layout IDs prevent
meaning changes within a version.

The diagnostic `layout_comparisons` counter counts aggregate-descriptor fallback walks,
not every raw `TypeView` comparison in the runtime. Focused metadata tests with
`execution-diagnostics` establish one cold comparison for a cross-version struct pair
and one for an enum pair, then zero for 2,500 and 5,000 repeated checks of either pair.
The bounded-lifetime contract fills 160 relations, observes a fresh comparison after
eviction and subsequent reuse, checks that copied numeric IDs in a different descriptor
cannot reuse a producer's proof, transfers prepared facts across a thread, rejects a
foreign runtime and confirms that a weak producer descriptor expires after retirement.
An existing descriptor-forgery fixture is shared between the relevant contracts.

All frozen/source-form and scaling diagnostic checksums pass. Warm source-form probes
report zero aggregate-descriptor walks. Nested 5,000-iteration probes still request
2,111,197/2,111,171 allocations and perform 20,002 metadata validations, unchanged from
the previous checkpoint. Other raw type checks and execution protocols remain; zero in
this new counter does not mean all type validation disappeared. Evidence is in
`target/hp01/layout-admission-diagnostics.log`, using the documented HP00 machine and
toolchain, default Cargo parallelism, incremental release diagnostics, a fresh runtime
per probe and the measured fourth call after two additional warmups. Metadata reuse
counts above use the optimized test profile. Compilation is excluded; neither dataset
is a new throughput claim or isolated descriptor-memory measurement.

Focused checks pass: diagnostic module records/admission/lifetime (8), VM native enums
(6), embed enum payloads (4), source-free native enums (2), generic reload (2), and host
old-version binding/cache lifetime (1), plus strict runtime/benchmark all-target Clippy
with diagnostics and structure (999 files, zero violations/exceptions). The intermediate
duplicate module import was fixed. No carried build/test failure remains.

The preparation-owner audit confirms that method applications, shared environments,
witnesses and generic native applications use linked publication; layout applications
and scopes use the complete pure-fact model above; host `bind_enum_type` preparation is
an explicit owning setup boundary. `LinkedNativeFunction` still carries both an applied
signature option and a lazy closed-signature cell: consolidate that storage under its
existing owner before HP01 acceptance. Raw type-expression/operand checks lacking
prepared producer/consumer facts remain HP03–HP05 migration work. HP01 also still needs
isolated descriptor-memory accounting, including retirement; it is not accepted yet.
No full workspace or GitHub CI matrix has run, and Lua parity remains open.

Final ordinary original/source-form checksum checks, formatting, all 703 local Markdown
links and diff checks pass. The restored ordinary release executable is preserved at
`target/hp01/layout-admission-executable`, SHA-256
`a9d904035cfa44fc41ee4f7f1f694672d872d437c7bcac63518f1ab5d5abbee6`.
These cold checksum checks are not throughput samples.


2026-10-10 HP01, native signature and allocation-accounting checkpoint (in progress):
`LinkedNativeFunction` now owns one preparation cell. Closed bindings fill it lazily;
generic application preparation fills the same cell directly. Removed the separate
`scoped_signature` option/Arc and duplicate result-validation fallback. Native argument
views, object storage scopes and typed payload construction consume the same prepared
signature; closed native result construction no longer rebuilds its result TypeArgument.
The separate selected-call descriptor still owns its own signature, as required by its
callable boundary. No value/owner/generation or conversion check was removed.

The diagnostic allocator moved from the Lua comparison executable into the runtime's
opt-in diagnostics module. The benchmark registers it explicitly; only the diagnostic
runtime unit-test binary registers it automatically. Ordinary runtime builds do not
install an allocator. The implementation still forwards System's allocation contracts
unchanged and counts successful requests on the measured thread. Allocation and execution
counters retain separate measurement scopes. The new ignored memory probe uses checked
source and installed native declarations; it prepares metadata without executing script,
creating frames or calling native callbacks. Compilation occurs outside measurement.

Reproduce isolated native accounting with:

```text
DEVELOPER_DIR=/Library/Developer/CommandLineTools cargo test --release -p kagari-runtime --lib diagnostics::memory --features execution-diagnostics -- --ignored --nocapture --test-threads=1
```

The probe measures the whole runtime lifecycle, with setup, cold preparation, two repeated
preparation/collection passes, compatible hot reload/old-version retirement and teardown.
An identical control loads the same program and explicitly roots the same input type
environments, but skips descriptor preparation. A preliminary unmeasured lifecycle warms
process/thread state. Reported retained bytes are prepared-minus-control snapshots after
collection, including descriptor/index capacity and excluding common input/setup costs.
The new program version is unprepared. Allocator overhead, peak memory, other threads and
execution throughput are outside this probe's scope.

| Preparation case | Extra bytes after cold preparation | After two repeats | After retirement | After runtime teardown |
| --- | ---: | ---: | ---: | ---: |
| One closed zero-argument scalar signature | 280 | 280 | 0 | 0 |
| One generic native application | 1,552 | 1,552 | 0 | 0 |
| Four distinct generic native applications | 5,984 | 5,984 | 0 | 0 |
| 160 distinct generic native applications | 1,735,432 | 1,735,432 | 0 | 0 |

Generic inputs are tuples containing 1 through N i32 elements. The last row deliberately
exceeds the 128-entry bound and varies type size; it is neither a fixed per-entry byte
cost nor evidence of allocation-free warm lookup under eviction. Cold and repeated
retained sizes agree, while the over-capacity workload repeatedly prepares evicted facts.
Setup retained-byte differences are zero in every pair, and *each* complete control and
prepared lifecycle independently ends at zero net bytes. Retirement retains common
runtime/module/environment arena capacities (for example, 83,440 bytes in either side
of the 160-input case); their release occurs at runtime teardown. These observations
cover native descriptors only, not yet the full HP01 descriptor-memory acceptance.

Evidence: `target/hp01/native-memory.log`; M1 Max, 32 GiB, 10 logical CPUs,
macOS 26.6.2 arm64, rustc 1.98.1 / LLVM 22.1.8, workspace release profile,
`execution-diagnostics`, default Cargo build parallelism and one probe test thread.
Build caches were incremental; compilation is excluded. Initial probe failures were
resolved: annotate scalar-only Ty with DefinitionId, use an interface generic body to
emit a shared native template, and reload an ABI-compatible program instead of an empty
incompatible replacement. The reload ABI check was preserved. The earlier stale native
storage field consumers were also migrated to the single signature cell.

Focused validation passes: existing frame-window contracts (6), VM native function/
application/selected-call contracts (38), generic managed payload fields (1), native enum
boundaries (6), and the ignored release memory probe (1). Runtime/benchmark all-target
Clippy with diagnostics is clean; structure checks 1,001 Rust files with zero violations
or exceptions; formatting, 703 local Markdown links and diff checks pass. No build/test
failure is carried. This checkpoint adds a manual diagnostic tool, not another ordinary
regression matrix.

The diagnostic source-form/scaling run passes all 64 cold/warm checksum probes;
`target/hp01/native-signature-diagnostics.log` records the same toolchain/machine and
fourth-call warm protocol used previously. Fixed/changing native application probes
request 12/24 fewer allocations per measured call at both 2,500 and 5,000 iterations.
All other warm request counts, and all warm object/metadata-validation/environment
counts, are unchanged. This is a constant preparation reduction, not a per-iteration
speedup claim. The frozen benchmark workloads remain unchanged. HP01 remains open for
isolated method/shared/witness/layout/admission memory accounting; HP02–HP06 and full
workspace/CI/Lua-parity acceptance remain outstanding.

Final ordinary original/source-form checksum checks pass. The ordinary release binary
is restored and preserved at `target/hp01/native-signature-executable`, SHA-256
`0b819cc66b167a1ed42bf76dd69eeeeb54581e9186c22c4ddadea5ece438e512`.
These cold correctness runs are not throughput measurements.


2026-10-10 HP01, descriptor-memory and closed-link preparation checkpoint:
manual probes now cover method applications, shared environments, witness selections,
scoped struct/enum applications and cross-version layout admission, alongside native
applications. They use real checked source contracts without executing script or native
callbacks. Caller environments, interface receivers and type/layout inputs are created
and rooted identically in control/prepared runtimes before the first snapshot. Temporary
method host handles drop before collection/sampling; their freed allocations count as
requests, but do not inflate retained descriptor bytes. Admission inputs include both
versions' complete layouts before sampling, so its row measures compatibility evidence
rather than layout preparation. Each case measures cold preparation, two repeat passes,
32 repeat passes, compatible reload/retirement and complete runtime teardown.

Final release results below are incremental retained bytes over the control, after GC.
`N` is the number of distinct applications or lexical witness scopes. Admission uses
N struct pairs plus N enum pairs, sharing one 128-relation bound. Method/shared/native
input tuples contain 1 through N i32 elements; layout inputs also include a nominal
Marker to exercise provenance scopes. These sizes are workload-specific, not fixed
per-entry sizes or peak memory. Over-capacity cases evict and reprepare facts. Separate
per-index bounds remain 128 entries; arena capacity and variable-size type trees mean
that is not a 128-byte or constant-byte bound.

| Descriptor | N | Cold bytes | After 32 repeats | After retirement |
| --- | ---: | ---: | ---: | ---: |
| method | 1 | 4,984 | 4,984 | 1,280 |
| shared | 1 | 1,464 | 1,464 | 0 |
| witness | 1 | 4,140 | 4,140 | 448 |
| method | 4 | 11,100 | 11,100 | 1,568 |
| shared | 4 | 4,480 | 4,480 | 288 |
| witness | 4 | 14,064 | 14,064 | 448 |
| method | 160 | 3,513,076 | 3,578,356 | 163,840 |
| shared | 160 | 159,752 | 161,544 | 18,432 |
| witness | 160 | 469,512 | 497,928 | 57,344 |
| struct | 1 | 3,581 | 3,581 | 0 |
| enum | 1 | 3,616 | 3,616 | 0 |
| admission | 1x2 | 900 | 900 | 0 |
| struct | 4 | 11,348 | 11,348 | 0 |
| enum | 4 | 11,488 | 11,488 | 0 |
| admission | 4x2 | 1,388 | 1,388 | 0 |
| struct | 160 | 3,852,120 | 3,868,760 | 0 |
| enum | 160 | 3,856,600 | 3,873,240 | 0 |
| admission | 160x2 | 21,148 | 21,148 | 0 |
| native | 0 | 0 | 0 | 0 |
| native | 1 | 1,512 | 1,512 | 0 |
| native | 4 | 5,824 | 5,824 | 0 |
| native | 160 | 1,730,312 | 1,730,312 | 0 |

Every pair has zero setup retained-byte difference. Every control and prepared runtime
independently finishes with zero net bytes at teardown. Executable probes also assert
that their environment/application/group live counts are zero after retirement. The
remaining method/shared/witness bytes above are retained slot/free-list capacities:
central stores detach records, increment generations and reuse slots without shrinking
vectors. They are released at runtime drop. Method/shared/witness retained totals after
32 passes equal their two-pass totals. Optional layout/admission hash-index capacities
can grow during repeated eviction; randomized table placement affects when growth occurs
and request counts. The table records observations, not a claim of no future capacity
change. Entries remain bounded; pure-fact indices fully disappear at retirement. No
arena-shrinking or allocator tuning was added to hide these costs.

Reproduce using the previous `cargo test --release ... diagnostics::memory` command;
all three ignored probe functions pass. Final evidence is
`target/hp01/descriptor-memory.log`, on the same M1 Max/32 GiB/10-core macOS 26.6.2,
rustc 1.98.1/LLVM 22.1.8 host, workspace release profile, execution-diagnostics feature,
default build parallelism, incremental build caches and one test thread. Compilation
and process warmup are outside measurement. The initial method fixture looked up the
implementation table by a trait name; selecting its checked method entry fixes that
probe-only failure without changing product behavior.

The acceptance audit found one remaining mismatch with the target preparation model:
closed native signatures still initialized during their first call. `Runtime` loading
and reload now share `stage_linked_program`, which obtains exact staged provenance and
prepares every closed native signature before returning/publishing the candidate.
Generic templates remain unapplied until concrete environments exist. Failed preparation
drops the existing staged lease; abandoned-candidate reclamation remains the cleanup
owner. The cell now stores only a completed ScopedSignature, and consumer access cannot
perform preparation or cache an error. Argument conversion, result construction, typed
futures and selected operations use that read-only accessor. No callback or execution
happens during signature preparation, and value/generation/access checks remain intact.

The native N=0 row now measures a read of an already linked closed signature: zero
additional bytes. Its preparation moved into the common setup baseline; it did not become
free. Compared with the earlier native table, the smaller completed-signature cell saves
40 retained bytes per live native application (5,120 bytes for 128 retained entries).
These are allocation/layout observations, not interpreter-throughput claims.


HP01 requirement audit against the implemented owners:

| Requirement | Current implementation and evidence |
| --- | --- |
| Shared owner and exact supplying scopes | `module/descriptors.rs` owns method/shared/witness/native indices; `ApplicationKey`, `SharedScope`, generational environments and operation identities distinguish applications. Existing application identity, foreign-scope and abandoned-provider contracts pass in the recorded HP01 checkpoints. |
| Normalize same-program layouts; admit distinct versions explicitly | `layout_identity.rs`, `applied_layout_identity.rs` and sealed `LayoutScope` prepare complete identities; `layout_admission.rs` uses checked complete pairs with weak producer identity. Records tests cover equivalent members, changed scopes, copied-key forgery, eviction and cross-version retirement. |
| Transactional publication and checked lazy edges | `publish_descriptor` validates dependency graphs before installation; `DescriptorIndex` reserves before mutation. Remaining receiver/parent `MetadataCache` cells are relationship edges on traced interface metadata, not competing applied-call stores; writes remain in checked `execution_metadata/links.rs`. Existing publication tests cover partial/foreign/abandoned edges. |
| General generic preparation and repeat reuse | Method/shared/witness/native preparers use those indices; warm diagnostics record zero repeated preparations for stable applications. Changed types, supplying versions and selected operations cannot reuse a different entry. Cold closed native signatures are now prepared before candidate publication. |
| Bounded retention without hidden receiver values | Each descriptor index has a 128-entry bound across its lexical scopes. Descriptor records contain immutable facts and checked metadata IDs, not invocation Values; active frame/native edges and escaped host roots retain needed dependencies independently of eviction. |
| Reclaim cycles and measure memory | Existing active-window, old-version, abandoned-candidate and metadata-cycle contracts establish lifetime behavior. The new isolated probes cover every HP01 descriptor family, explicitly distinguish retired arena capacity from live records and finish every measured runtime at zero net bytes. |
| Remove superseded paths | Receiver-local applied-call cells, direct per-entry shared/witness/native reconstruction, duplicate native scoped-signature storage and the VE09 same-program structural comparison shortcut have been removed in the recorded commits. No compatibility aliases or second semantic implementations were added. |

This closes the HP01 preparation/ownership work after the checkpoint checks below.
It does not close HP02 active execution admission, HP03 unified transfers, HP04 prepared
managed operations or HP05 raw enum operand consumers. Those named consumers still need
migration; the existing zero aggregate-layout-walk counter does not mean all type checks
or metadata traversals are gone. Full workspace/CI acceptance and all 16 Lua-parity gates
remain HP06/final work, not an implication of this phase's acceptance.

Checkpoint validation passes: release memory probes (3), VM native function/application
contracts (38), diagnostic module records/layout/retirement contracts (8), source-free
native Future waiting/completion/admission/cleanup contracts (4), strict runtime/benchmark
all-target Clippy with diagnostics, structure (1,003 Rust files, zero violations or
exceptions), formatting, 703 local Markdown links and diff checks. No carried error
remains. The final source-form/scaling diagnostic run passes all 64 cold/warm checksums
(`target/hp01/linked-signature-diagnostics.log`); every warm request/object/environment/
metadata-validation/native-preparation count equals the preceding signature checkpoint.
The same fourth-call warm protocol and machine/toolchain apply. Moving closed preparation
into linking is an architectural boundary change, not a demonstrated steady-state speedup.

HP02 starts from `ExecutionStack::cursor`, `ExecutionCursor` and the VM executor driver.
The cursor already borrows frame/window/session state, but general operand reads/writes
still repeat session admission and window lookup; scalar regions already retain direct
bank slices. The next migration must give ordinary execution one admitted ownership
scope with explicit exits for callbacks, GC, observation, growth, parking and returns.
It must preserve quarantine and external checked access while replacing the repeated
internal protocol, not add a parallel unchecked accessor at each call site. Driver
native/waiting rediscovery and managed-operation boundaries remain part of HP02–HP04.

Final ordinary original/source-form checksum checks pass, and the ordinary release
binary is restored at `target/release/kagari-lua-benchmark`. Its preserved copy is
`target/hp01/linked-signature-executable`, SHA-256
`becc1d1b12ee30d1494dc1503dc36bcd42156e4a62a7d05ce5ff857c3abb7f4d`.
These are correctness checks, not throughput samples. HP01 is accepted for its stated
preparation/publication scope; HP02 is the next implementation phase. The full goal
remains active, with architectural integration, GitHub CI and Lua parity still open.


2026-10-10 HP02, admitted operand scope checkpoint (in progress):
`ExecutionStack::execute_region` now owns the complete transient frame/session/bank
borrow. Its internal cursor is private; the old public `ExecutionStack::cursor` and
externally callable cursor read/write API are removed, not retained as compatibility
wrappers. The only backend-facing operation returns a region transition after all
borrows end. Existing checked `ExecutionFrame` inspection/publication remains the
external value boundary. No supplied Value or caller closure enters the admitted region.

Admission validates the runtime pointer, active session and top frame scope, sticky
termination/quarantine, native borrow state and exact window owner/generation. An
`OperandWindow` then holds bounded managed/scalar/initialization slices and the immutable
location map. Internal field operands no longer reacquire the session or resolve the
window handle. The obsolete `ensure_cursor_allowed` layer is removed. Scalar segments
reborrow those admitted slices directly instead of recomputing window ranges on each
field/scalar handoff. Both checked external frame storage and admitted execution share
`values/operands.rs` for Value materialization, representation/numeric admission and
slot writes; this does not introduce a second storage semantic implementation.

Proof/invalidation audit for the removed checks:

| Removed repetition | Proof owner | Invalidating boundary and remaining checks |
| --- | --- | --- |
| Session/top-scope/termination checks at each internal operand | Region entry plus private synchronous scope; it cannot invoke caller code | Return before callbacks, observation, GC, parking or frame changes; next entry re-admits. Errors exit immediately. External frame access still checks current authority. |
| Window owner/generation lookup at each internal operand | Checked `borrow_operands` and exclusive bank borrow | Growth/retirement/compaction requires releasing the region. Relative slot bounds, representation checks, initialization semantics and full heap handle validation remain. |
| Native-storage-borrow check on every internal write | Entry check and no callbacks/native borrowing inside closed handlers | Native calls remain region exits. Heap storage continues checking dynamic generation, layout, access and bounds. |
| Bank-range reconstruction at scalar/object handoff | One admitted bounded slice per bank | Handoff only reborrows; logical PC, instruction count and slice accounting are unchanged. |

Cancellation, observer attachment and abandoned program leases retain their existing
logical-PC checks. The driver still polls/observes the first instruction before region
entry. Automatic collection eligibility is still recomputed across allocating/reentrant
boundaries. GC roots remain in runtime-owned windows independently of transient frame
views; no new root registry or unchecked raw pointer is introduced.

The old VM fault test used an escaped cursor to hold a window borrow across GC. That
illegal operation is now constructible only by engine code, so the contract moved to
runtime `frame/cursor/tests.rs`: hold the actual bank borrow, require an EngineFault,
then reject external reads/writes and further region execution, and verify cleanup.
An initial migration incorrectly assumed a frame borrow alone blocks collection; the
collector intentionally traces independent banks, so that probe returned success. The
corrected test exercises the actual invalid bank borrow without changing GC policy or
weakening assertions. A private child-module method visibility error and import/format
warnings were also corrected. Successful GC lifecycle cases from that same focused run
remain valid; only the relocated failing contract required correction.

HP02 remains open: the executor still rediscovers native return/entry and pending-wait
state at outer driver iterations; ordinary managed operations retain slow boundaries
until their HP03/HP04 migrations. This checkpoint does not claim that every instruction
uses the admitted operand path or that execution-wide admission is complete.

Focused checks pass: runtime frame/window/type contracts including the relocated GC
fault test (11), VM allocation contracts (2), field/nominal/reflection contracts (5),
synchronous debugger contracts (4) and async debugger drive (1). Nine existing GC
lifecycle cases also passed before relocation of the one incorrect fixture noted above.
Strict runtime/VM/benchmark all-target Clippy with diagnostics passes; structure checks
1,005 Rust files with zero violations/exceptions; final formatting, 703 local Markdown
links and diff checks pass. No build/test error is carried. No full workspace or CI run
was performed at this intermediate checkpoint.

Paired ordinary release measurements compare this worktree with the preserved final
HP01 executable. Commands, both on the documented M1 Max/32 GiB/10-core macOS 26.6.2,
rustc 1.98.1/LLVM 22.1.8 machine:

```text
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/hp01/linked-signature-executable
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/hp01/linked-signature-executable
```

Default workspace release profile/features/target and Cargo parallelism; sequential,
single-thread benchmark processes in baseline/candidate/candidate/baseline order, fresh
process/state, three warmups and 22 pooled samples per engine/workload/variant. Build
caches were incremental (20.103 s / 0.088 s builds, excluded from execution). No allocation
instrumentation or concurrent build/test workload ran during sampling. All checksums
pass; frozen definitions remain unchanged. Diagnostic-only native/entry rows retain
their existing status and do not redefine the 16-workload parity gate.

| Workload | HP01 median µs | Candidate median µs | Candidate / HP01 | Lua control ratio | Candidate / Lua |
| --- | ---: | ---: | ---: | ---: | ---: |
| forms_byte_state | 7,362.562 | 7,085.729 | 0.9624 | 1.0033 | 62.80 |
| forms_capture_cell | 10,308.375 | 9,975.416 | 0.9677 | 1.0196 | 68.71 |
| forms_concrete_generic | 3,221.146 | 3,123.188 | 0.9696 | 0.9895 | 30.46 |
| forms_direct | 348.542 | 347.000 | 0.9956 | 1.0101 | 4.49 |
| forms_field | 3,767.291 | 3,344.063 | 0.8877 | 0.9982 | 36.41 |
| forms_helper | 3,230.438 | 3,103.416 | 0.9607 | 1.0219 | 21.59 |
| forms_host_callback | 1,936.625 | 1,855.938 | 0.9583 | 1.0066 | 7.27 |
| forms_interface | 10,429.229 | 10,189.791 | 0.9770 | 1.0006 | 74.53 |
| forms_native | 1,935.021 | 1,842.167 | 0.9520 | 0.9867 | 12.94 |
| forms_shared_generic | 16,980.062 | 16,804.812 | 0.9897 | 0.9699 | 155.45 |
| forms_string_calls | 12,948.562 | 12,606.021 | 0.9735 | 1.0036 | 98.92 |
| forms_string_constants | 7,015.521 | 6,749.937 | 0.9621 | 0.9997 | 99.78 |
| arithmetic | 2,491.937 | 2,489.646 | 0.9991 | 1.0187 | 6.47 |
| arrays | 5,169.833 | 4,963.687 | 0.9601 | 1.0103 | 71.72 |
| branches | 3,019.750 | 2,942.084 | 0.9743 | 0.9716 | 3.70 |
| calls | 6,415.105 | 6,137.396 | 0.9567 | 1.0044 | 27.52 |
| entry | 1.405 | 1.379 | 0.9811 | 1.0015 | 48.20 |
| fibonacci | 13,014.687 | 12,450.896 | 0.9567 | 1.0015 | 35.95 |
| maps | 6,074.312 | 5,790.438 | 0.9533 | 0.9534 | 86.56 |

The targeted field loop falls from 3,767.291 to 3,344.063 µs (11.23% lower time), while
its Lua control ratio is 0.9982. This is evidence of benefit for that workload, not a
universal speedup. Source-form controls vary by up to about 3%; smaller improvements
need that context. The original Map row and its Lua control both fall about 4.7%,
so that row does not isolate a Kagari-specific improvement. All original/source-form results remain above Lua time; parity is
still unmet. Raw samples, machine metadata and hashes are in
`target/lua-comparison/20261010T034614Z-forms-paired/results.json` and
`target/lua-comparison/20261010T034728Z-paired/results.json`.

The ordinary candidate is preserved at `target/hp02/admitted-region-executable`, SHA-256
`b990bf7bca4448a83ed4792c9393caf118327510c734380be97816a2adb764f0`;
`target/release/kagari-lua-benchmark` remains the ordinary build. No diagnostic rebuild
needs restoration. Next HP02 work replaces repeated `poll_await`/native-state discovery
in the executor with explicit transitions at entry, calls, returns and await/resume;
it must retain original polling, observation, slice and cleanup order.

2026-10-10 HP02, explicit driver transitions and phase acceptance:
`ExecutionStack::next_action` classifies authoritative session queues and frame state
only on activation/resume and after call, return or await. VM dispatch explicitly
reports ordinary progress, call, await or return. Ordinary instructions and inline
synchronous native calls keep their script action; native-frame completion supplies
the return action directly. No persistent shadow state/cache was added. Scalar and
managed returns converge on the existing return operation. The old public frame
`native_return`/`has_pending_native_entry` probes and VM `LoopExit` forwarding layer
are removed; `start_native_entry` now returns its next action.

The driver owns control flow, while the runtime owns frames, pending waits and entry
states. Its local action retains no frame/window borrow. All frame-changing VM dispatch
arms report a transition; synchronous callback/reentry paths unwind their nested scope
before returning. The removed ordinary native probe's runtime/window-owner check is
covered by region admission and checked external frame accesses. Native return still
validates stack authority and the exact result window. The previous checkpoint's
session/window invalidation audit continues to apply.

| Boundary | Preserved contract |
| --- | --- |
| Ordinary instruction/region continuation | Original cancellation and debugger points; only redundant no-wait/native-state probes disappear. Region entry admits session, scope and window again after releasing transient borrows. |
| Observation, GC and synchronous reentry | No operand view survives; existing active frame banks and program/environment edges remain traced. Reentry must restore its caller before returning. |
| Await/factory/resume | Queued factories/futures and pending waits are classified from runtime state. Real waits are polled before the slice check; parked activations classify again on resume. |
| Native completion and slicing | Result publication precedes possible parking and remains in the traced native frame. Resume consumes the completed return without repeating the callback. |
| Failure/cancellation | Existing trap reporting, trace capture, stack retirement and owned-session cleanup remain; invariant failures still quarantine. |

Focused validation passes: 38 native function/handle contracts; VM owned-drive (2),
native-wait (4), debugger (6) and reentry-debug (1); embed async execution (14) and
source-free async execution (1). The existing boxed-native contract now also drives a
native String result with one-instruction slices and GC between every activation:
it must park after native completion, return intact bytes and invoke the callback once.
That augmented contract passes separately. Its first fixture attempted unsupported
source-level native function boxing (`InvalidValueTarget: text`); it was corrected to
use the existing checked host binding/conversion boundary, without changing language
semantics. An initial visibility warning was corrected before strict Clippy.

Runtime/VM/benchmark all-target Clippy with diagnostics passes; the subsequently changed
native-boundary target also passes strict Clippy. Structure checks 1,006 Rust files with
zero violations/exceptions; formatting and diff checks pass. No build/test error is
carried. No full-workspace or GitHub CI run was performed at this intermediate phase.

Diagnostic release (`--features diagnostics`, `--diagnostics --source-forms`) passes all
64 cold/warm rows. New opt-in counters count actual driver admissions and wait polling.
Direct, field, byte-state, string-constant and inline native/host loops each admit once;
5,000 helper/concrete-generic/interface/shared/closure calls admit 10,001 times. All
synchronous rows report zero await polls. N/2N application rows scale with actual frame
transitions, not ordinary instruction count. Allocation counts remain unchanged:
5,000 interface/shared calls still request 65,040/155,050 allocations. Raw counts are in
`target/hp02/driver-diagnostics.log`; they are not throughput measurements.

HP02 acceptance is complete for runnable ownership and driver transitions. The checked
external frame API remains the host/native/debug boundary; managed slow operations
still await the explicit HP04 migration. Internal owning method handles and call-kind
packing/retirement are HP03 work, not additional HP02 caches. The full goal, HP03–HP06,
complete CI and unchanged Lua-parity gate remain open.

Paired ordinary release measurements compare against the preceding HP02 admitted-region
executable (`8a1aff02`), with unchanged workload bodies/checksums:

```text
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --source-forms --baseline-executable target/hp02/admitted-region-executable
DEVELOPER_DIR=/Library/Developer/CommandLineTools uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable target/hp02/admitted-region-executable
```

Same M1 Max/32 GiB/10-core macOS 26.6.2 arm64 machine, rustc 1.98.1/LLVM 22.1.8,
workspace release/default features/target/parallelism. Benchmark processes run singly
in baseline/candidate/candidate/baseline order, with fresh state, three warmups and 22
pooled samples per engine/workload/variant. Incremental build wall times are 20.416 s
and 0.090 s, excluded from execution. No allocation counters or concurrent build/test
work ran during sampling. All checksums pass.

| Workload | Prior HP02 median µs | Candidate median µs | Candidate / prior | Lua control ratio | Candidate / Lua |
| --- | ---: | ---: | ---: | ---: | ---: |
| forms_byte_state | 7,012.438 | 5,395.751 | 0.7695 | 0.9946 | 48.16 |
| forms_capture_cell | 9,887.458 | 7,852.396 | 0.7942 | 0.9764 | 56.23 |
| forms_concrete_generic | 3,062.312 | 2,635.771 | 0.8607 | 1.0088 | 25.13 |
| forms_direct | 338.521 | 333.125 | 0.9841 | 0.9995 | 4.43 |
| forms_field | 3,323.625 | 2,710.854 | 0.8156 | 1.0062 | 29.60 |
| forms_helper | 3,074.438 | 2,611.688 | 0.8495 | 1.0174 | 18.35 |
| forms_host_callback | 1,838.938 | 1,521.708 | 0.8275 | 1.0156 | 6.12 |
| forms_interface | 10,052.396 | 9,363.896 | 0.9315 | 0.9983 | 69.61 |
| forms_native | 1,837.312 | 1,518.229 | 0.8263 | 1.0012 | 10.79 |
| forms_shared_generic | 16,627.416 | 15,711.854 | 0.9449 | 1.0125 | 145.23 |
| forms_string_calls | 12,481.541 | 9,929.146 | 0.7955 | 0.9886 | 79.95 |
| forms_string_constants | 6,702.438 | 5,179.333 | 0.7728 | 0.9981 | 77.69 |
| arithmetic | 2,519.876 | 2,449.146 | 0.9719 | 1.0179 | 6.37 |
| arrays | 4,989.438 | 3,925.229 | 0.7867 | 0.9759 | 57.43 |
| branches | 2,934.000 | 2,878.896 | 0.9812 | 0.9928 | 3.63 |
| calls | 6,172.667 | 5,224.229 | 0.8463 | 0.9951 | 23.44 |
| entry | 1.376 | 1.334 | 0.9696 | 0.9971 | 46.81 |
| fibonacci | 12,461.729 | 10,545.959 | 0.8463 | 0.9994 | 30.40 |
| maps | 5,799.312 | 5,136.501 | 0.8857 | 1.0266 | 73.42 |

The managed-boundary workloads show larger reductions: field 18.44%, string constants
22.72%, byte state 23.05% and inline native calls 17.37%; their Lua controls change by
less than 1%. Original arrays fall 21.33%, with a 2.41% lower Lua control. Interface and
shared-generic loops improve only 6.85%/5.51% and retain their allocation traffic; HP03
must replace their call ownership/packing protocol. Small scalar changes remain subject
to measurement noise. Every reported workload still takes longer than Lua; diagnostic
entry/native rows do not redefine the frozen parity gate.

Raw results and metadata: `target/lua-comparison/20261010T040319Z-forms-paired/results.json`
and `target/lua-comparison/20261010T040421Z-paired/results.json`. The ordinary candidate
is preserved at `target/hp02/driver-transition-executable`, SHA-256
`11b48cda4efe11a81ba93863b476e78a2b4ffb35277b9b0a98be56e6cebcf873`.
The default release executable is also ordinary. Link checks cover all 76 local links
in the three changed architecture/roadmap/plan documents. Next is HP03: unify prepared
call ownership and physical argument/result transfers, starting from the retained
interface/shared packing and owning method-handle paths.

2026-10-10 HP03, common argument-source protocol (in progress):
`PreparedCall` replaces concrete-only `PreparedScriptCall` and covers statically
selected script plus shared script/native targets. Call sites retain sealed physical
argument sources and return destinations; the selected callee's layout owns parameter
placement. The duplicate callee-layout reference and source/target-pair table are gone,
including the temporary cross-module layout matrix used while preparing calls.

`FrameArguments` now iterates borrowed values/captures or checked window locations.
One admission/publication loop consumes these sources, validating every argument before
bank growth and frame publication. Complete scalar payloads still transfer directly
between scalar banks; managed destinations materialize Values through the existing bank
access rules. Window owner/generation, initialization, bounds, numeric domains, heap and
candidate ownership, arity, call depth and cleanup remain checked. Existing relocation,
reordering, repeated-source, stale-window and transactional rejection tests now use the
callee-owned destination layout, preserving their behavioral assertions.

VM direct/shared dispatch enters `push_prepared_call` without packing arguments. The old
public `push_shared_call` slice API is removed. Shared targets still resolve through the
caller's pinned program and HP01 environment descriptor, validate caller-scoped semantic
arguments and validate the adapted result before publishing it into the caller's physical
destination. Concrete calls do not decode a shared contract. Host/native selected calls
and closures consume the same storage protocol; their selection/validation and existing
external argument API remain. No second call cache or unchecked public entry was added.

Focused checks pass: runtime frame/window/type contracts (11), VM shared/generation
contracts (10), native allocation contracts (2), generic native application contracts
(4), source-free shared application retention (1) and embed generic reload (2). Strict
runtime/VM/benchmark all-target Clippy with diagnostics passes; structure checks 1,006
Rust files with zero violations/exceptions. No carried build/test error or new test
matrix is introduced. Full workspace and GitHub CI remain unrun at this checkpoint.

All 64 diagnostic cold/warm rows pass. Shared script forwarding and fixed/alternating
native application probes each lose exactly one 64-byte allocation request per shared
call: 2,500 calls remove 2,500 requests/160,000 requested bytes; 5,000 remove 5,000/320,000.
For example, `shared_application_5000` changes 535,466 to 530,466 requests and
`native_application_5000` changes 705,360 to 700,360. Driver transitions, metadata
validation counts and heap object counts remain unchanged. These are allocation counts,
not throughput claims; raw evidence is `target/hp03/argument-source-diagnostics.log`, compared
with `target/hp02/driver-diagnostics.log`.

The `shared_generic`/`fixed_application` workload names describe generic interface method
calls, not the bytecode Shared target migrated here. Their 155,050 requests at 5,000 calls
remain unchanged. HP03 is not accepted: interface selection/owning method roots, closure
packing, remaining scoped type preparation and graph validation, and unified return
retirement still require migration. The next ownership change must separate host-retained
method leases from active call descriptors while reusing this argument-source protocol.

The first parameter-source implementation kept independent value/window fields and
chained their iterators. Two paired ordinary source-form runs showed repeatable
regressions, including helper 9.16% and concrete generic 11.41% in the first run, plus
string constants 9.18% despite no shared call in that loop. Results are retained in
`target/lua-comparison/20261010T041659Z-forms-paired/results.json` and
`target/lua-comparison/20261010T041833Z-forms-paired/results.json`; the initial executable
is `target/hp03/argument-initial-executable`, SHA-256
`2a891fbf922b56a0740e4c9ebe0b3cd76719671aaa699676371026326a4a2bc2`.

Independent macOS sampling reused `scripts/profile_lua_macos.py::sample` for helper and
string_constants against both preserved binaries (5 s, 1 ms, within each warmed 10 s
execution window). Helper's collapsed top samples for the two argument admission
instantiations rise from 28/14 to 70/53; region-exit samples rise 125 to 396. String
constants' region-exit samples rise 159 to 375. These are incomplete optimized-stack
attribution, not isolated CPU costs. Instruction counts remain 105,012/100,012 and
Value/instruction/prepared-operation/return-packet sizes remain 16/136/24/24 bytes.
Arm64 disassembly shows the unchanged region source producing a 1,356-byte body versus
1,096, with a 368-byte stack frame versus 304 and additional aggregate moves. This
supports sensitivity of the remaining region handoff to code generation; it does not
prove that all regression originates in the argument protocol. Samples, hashes and
disassembly are in `target/hp03/argument-profiles/`.

The revised model makes explicit values and frame locations mutually exclusive under
one enum and uses one indexed source iterator, eliminating invalid combinations and
stacked iterator branches. Publication accesses its newly created destination window
under exclusive storage ownership; it does not re-admit that same window per argument.
All caller-window and destination bounds/domain checks remain. Storage contracts (6),
native allocation contracts (2), generic native applications (4), strict Clippy,
structure and formatting pass again after this change. All 64 diagnostic rows pass;
warm request/byte/object counts and driver/metadata counts match the first candidate.
No alignment directives, compiler-profile overrides or reduced validation were added.

That simplification alone did not resolve the timing regression: helper/concrete-generic
remain 1.0967/1.1153 times HP02, and string constants 1.1230, in
`target/lua-comparison/20261010T042435Z-forms-paired/results.json`. Original calls/fibonacci
are 1.1093/1.0928 in `target/lua-comparison/20261010T042528Z-paired/results.json`. This
intermediate ordinary executable is `target/hp03/argument-source-executable`, SHA-256
`b0a1bef42c9e1f79f45262d5320514330d58aa66a85be757d3ebd5c3261007bb`.

The regression audit therefore revisits the retained VE07/HP02 scalar/object handoff:
it accepted every `ExecutionInstruction` although only two field operations were legal.
`PreparedFieldOperation` now owns that exact read/write description and is reused by
both sealed instructions and the handoff. This removes the broad instruction payload
and unreachable non-field branch without a second operation implementation, another
decode, changed polling or new managed-operation support. It is a repair of the existing
boundary exposed by this migration; the remaining HP04 operation migration stays in its
planned phase. Field/nominal/reflection checks (6) pass after the repair.

The precise field payload alone still leaves helper/concrete generic at 1.0837/1.0945
times HP02, string constants at 1.0609, and original calls/fibonacci at 1.1040/1.0912.
Evidence is `target/lua-comparison/20261010T043108Z-forms-paired/results.json` and
`target/lua-comparison/20261010T043207Z-paired/results.json`; the ordinary executable
is `target/hp03/field-handoff-executable`, SHA-256
`a694400f7097e0727b969e9ae132b844bbcc3700a5ac35ce733164e75b569367`.
Region disassembly also shows whole nested-exit forwarding on payload-free exits.
The internal `ScalarExit` now names boundary/slice/safepoint/return/field directly;
the region constructs each exit explicitly, forwarding a payload only where needed.
Field/reflection contracts (6), owned-drive slicing/reentry contracts (2), strict
diagnostic Clippy and structure checks pass after this change. All 64 diagnostic
rows exactly match the preceding candidate, including allocation and execution
counters (`target/hp03/flat-transition-diagnostics.log`).

The flattened-transition source-form comparison is
`target/lua-comparison/20261010T043747Z-forms-paired/results.json`. Candidate/HP02
ratios are direct 0.9300, helper 1.0209, concrete generic 1.0380, string constants
1.0006, string calls 1.0224, field 1.0238, interface 1.0054, shared generic 0.9908,
capture 1.0204, native 1.0128, byte state 0.9912 and host callback 0.9966. Lua control
ratios range 0.9712–1.0091; small changes must not be overinterpreted. All checksums
pass. This narrows the initial regression but does not establish HP03 acceptance.
The ordinary executable is `target/hp03/flat-transition-executable`, SHA-256
`2a53c4767a28dbda05c0d04e7afd0403bd49abc35d1d37318d502b3049a38f8c`.
The same M1 Max/macOS 26.6.2/rustc 1.98.1 setup, default release profile and Cargo
parallelism are used. Processes run sequentially baseline/candidate/candidate/baseline,
with three warmups and 22 pooled samples per variant; incremental build time (21.328 s)
is excluded from execution timing. Diagnostic features are disabled for throughput.
The kernel body/stack are 1,372/384 bytes; the improvement does not imply smaller
overall generated code or prove a single source of all remaining regression.

The next HP03 checkpoint owns the remaining call transition, rather than further
incidental code-layout tuning. Separate the selected invocation descriptor from the
host's `RootedInterfaceMethod` lease. Publish selection/application/environment edges
with the callee's traced window before releasing any host lease, and use the existing
method view/application and result-adapter semantics for both entries. Internal calls
must not rely on the receiver register remaining live after publication. Extend the
common argument sources for the receiver/capture inputs, then retire packing and
redundant graph admission under that ownership contract. Existing host handle checks
and generation validation remain boundary obligations. Remaining call regressions
belong to HP03 integration and managed-boundary costs to HP04; neither is accepted
or hidden by this partial checkpoint.

The final original-seven paired run also passes all checksums:
`target/lua-comparison/20261010T043850Z-paired/results.json`, same candidate hash,
0.089 s incremental build excluded. Candidate/HP02 is arithmetic 0.9564, branches
0.9557, calls 1.0379, fibonacci 1.0090, entry 1.0100, arrays 0.9987 and maps 0.9923;
Lua control ratios are 0.9911–1.0202. Calls still regress 3.79%, so the new common
protocol has not earned performance acceptance. VM/Lua remains 6.04/3.45/23.68/30.39/
47.09/57.85/74.63 respectively; Lua parity is open. The partial checkpoint carries
no build/test error. Formatting, all 76 local links in the three changed documents
and `git diff --check` pass; no full-workspace or GitHub CI run was performed.

2026-10-10 HP03, active interface invocation ownership (in progress):
`MethodInvocation` now separates checked selection/application identities from the
host-retained `RootedInterfaceMethod`. Internal interface/constraint resolution uses
the existing immutable views and application cache without constructing a host root,
copying bound receiver type descriptions or refreshing a root-container graph. Host
handles retain their checked lease protocol and converge on the same application,
argument validation and result-adapter implementation. The application owns its entry
environment; the wrapper no longer duplicates that environment reference.

Prepared InterfaceMethod call sites now share direct/shared physical argument sources
and return destinations. The selected receiver is a borrowed prefix and the other
operands come from the caller window, eliminating both VM argument vectors. A cohesive
`FrameMetadata` record publishes the callee program/environment and invocation edges
with parameter roots. The window retains selection/application dependencies across GC,
reentry and cache eviction independently of external handles or caller register lifetime.
Host entry keeps its lease until this publication completes. No unchecked public
invocation entry, new cache or second signature/adapter implementation was added.

The obsolete public `Runtime::resolve_interface_call` and
`ExecutionFrame::interface_method` interfaces are removed; VM dispatch uses the sealed
`push_prepared_call` entry. `push_callable` no longer accepts a method handle; host method
entry uses `push_interface_method`. The unused root-snapshot helper and internal
operation-to-host-handle construction are deleted. Existing storage fixtures now supply
the frame dependency record, preserving their behavioral assertions. The application
cycle fixture uses explicit metadata leases for retained operation identities; public
host-handle foreign-runtime checks remain in the interface contract tests.

Initial compilation exposed the argument module's previous frame-only visibility and
tests constructing the removed operation wrapper/accessing moved fields. These were
resolved by exposing the argument protocol within the crate to its method-validator
consumer and migrating the fixtures to the new ownership model. No intermediate build
error is carried. Runtime frame contracts (11), method-application contracts (5), VM
interface contracts (23), native-boundary interface contracts (12), host interface
allocation contracts (3), and strict affected-target diagnostics Clippy pass. The existing
pinned-method frame test now releases its external receiver root and collects before
descendant execution/return. The existing application-eviction test now proves the
active window keeps an evicted application alive after the last host lease is dropped,
then releases it after return; both strengthened checks pass. Structure checks cover
1,007 Rust files with zero violations/exceptions.

This does not accept HP03: scoped type/key preparation, frame-entry environment graph
validation, closure packing and unified return retirement remain. Performance evidence
for this ownership change follows separately; prior allocation savings or passing
correctness checks are not throughput acceptance. Full workspace and GitHub CI remain
unrun at this intermediate checkpoint.

All 64 cold/warm diagnostics pass (`target/hp03/invocation-diagnostics.log`), compared
with the preceding common-argument checkpoint's
`target/hp03/flat-transition-diagnostics.log`. Fixed and alternating receiver probes
both remove exactly 10 allocation requests/720 requested bytes per ordinary interface
call: at 2,500 calls fixed requests fall 32,540 to 7,540; at 5,000 they fall 65,040 to
15,040. Fixed generic applications remove 16 requests/1,524 requested bytes per call:
77,550 to 37,550 and 155,050 to 75,050 respectively. Alternating applications remove
the same per-call amounts. Ordinary interface metadata validations fall N+1 to 1
(alternating: N+2 to 2); fixed generic application validations fall 4N+2 to N+2.
Method/environment/application preparation counts, logical boundary/driver counts
and heap object counts are unchanged. These reductions remove host root admission
and packing, not value allocation required by script semantics. Generic frame-entry
graph validation still accounts for an N-scaled remainder; not all preparation traffic
is retired. Nested witness/shared/native and scoped-layout probes also improve, but
retain substantial scoped preparation/validation costs. Full warm deltas are recorded
in `target/hp03/invocation-diagnostic-delta.json`; these are allocation/validation
measurements, not execution-time results.

Paired ordinary source forms against the preceding HP03 checkpoint are in
`target/lua-comparison/20261010T045016Z-forms-paired/results.json`. Baseline is
`target/hp03/flat-transition-executable` (hash above); candidate is preserved as
`target/hp03/invocation-executable`, SHA-256
`53e625737b56453eabfdbc3cc3f163ad05d4b4353cf718b1bb6311c875d22517`.
All checksums pass. Candidate/baseline execution ratios are interface 0.5868,
shared generic 0.6660, helper 0.9528, concrete generic 0.9543, direct 1.1559,
field 1.0208, capture 1.0117, native 1.0393, byte state 1.0151, host callback
1.0192, string constants 1.0174 and string calls 1.0070. Lua control ratios range
0.9801–1.0300. Interface and shared-generic process medians consistently improve
in both orders (approximately 9.36 to 5.48 ms and 15.49 to 10.29 ms); their VM/Lua
ratios remain 40.60 and 95.11. This is material measured benefit, not Lua parity.

The direct scalar loop is a regression requiring continued integration review:
baseline process medians are 314.333/315.125 us, while candidates differ at
378.333/330.708 us; Lua stays about 75.2 us. The pooled 15.59% regression must not
be discarded, nor interpreted as one stable per-instruction overhead without more
evidence. Other regressions are also retained. The same M1 Max/macOS 26.6.2/rustc
1.98.1 environment, default release profile/Cargo parallelism, fresh sequential
baseline/candidate/candidate/baseline processes, three warmups and 22 pooled samples
are used. Diagnostic features are disabled, normal GC remains included, and the
20.104 s incremental build is excluded from execution measurements. No benchmark,
compiler flag or alignment setting was changed.

The original-seven comparison also passes all checksums:
`target/lua-comparison/20261010T045123Z-paired/results.json`, same baseline/candidate
hashes and environment, 0.082 s incremental build excluded. Candidate/baseline ratios
are entry 0.9817, arithmetic 1.0760, branches 1.0496, calls 0.9754, fibonacci 0.9761,
arrays 1.0170 and maps 1.0071; Lua controls range 0.9819–1.0002. Arithmetic/branch
regressions reinforce that this checkpoint cannot claim whole-interpreter acceptance.

Independent direct-loop sampling reuses `scripts/profile_lua_macos.py::sample` with
the two preserved ordinary executables, sequentially, with no concurrent build/test
or throughput run. Each samples 5 s at 1 ms within a warmed 10 s execution window,
then counts instructions afterwards. Both execute 75,015 Kagari instructions (Lua:
40,006), allocate zero heap objects and perform zero collections; layout sizes remain
Value/instruction/prepared operation/return packet = 16/136/24/24 bytes. Collapsed
top samples remain concentrated in the scalar loop (3,086/3,057), cancellation
(299/292) and numeric kernels, without a new interface/root path in this workload.
Arm64 disassembly shows scalar loop body/stack sizes changing from 1,840/352 to
1,836/368 bytes with different register allocation. This establishes code-generation
sensitivity around the retained scalar region, not an isolated cause or justification
for padding/alignment patches. Evidence and binary hashes are in
`target/hp03/invocation-profiles/`. HP04's prepared-region integration and HP06's
retrospective review must keep this regression visible. Required polling, including
abandoned-program detection, remains intact; no check was weakened to recover time.

Formatting, all 76 local document links and `git diff --check` pass after the final
edits. Next HP03 work is scoped call preparation/environment admission and the remaining
closure/return transitions; no new performance subproject is authorized by these results.

2026-10-10 HP03, closure admission and common frame retirement (in progress):
Sealed ClosureRegister sites now own the same physical argument sources/return
destination as direct/shared/interface sites. The runtime borrows the selected closure
once, checks the caller/callee physical signature, and feeds captures plus caller-window
operands to the existing transactional frame admission. Host closure calls use the same
closure policy with borrowed external arguments. Signature comparison consumes borrowed
parameter iterators instead of allocating physical-signature vectors; scoped semantic
substitution and nominal argument checks remain where required. A single checked prefix
operation replaces separate capture/value and capture/window constructors.

Closure policy moved from the frame facade into `frame/closures.rs`. The VM no longer
packs closure parameters, resolves the closure twice or implements a separate signature
check. Public `ExecutionFrame::closure_signature` and
`ClosureValueSnapshot::physical_signature` are removed. A rejected closure call signature
is now a runtime ScriptTrap with reason `closure call contract`, rather than the VM's
separate TypeMismatch variant; rejection remains before callee execution. This is an
unpublished Rust API/error-routing change, not relaxed signature validation.

Frame returns now share one retirement/depth-release/publication sequence. An allocating
interface adapter or shared result check completes while callee roots remain live and
outside exclusive stack/bank borrows; authority is re-admitted afterwards. Unadapted
scalar packets still publish raw bits, while general Values retain heap owner/generation,
register bounds and representation checks. The common publication path does not reacquire
the public current-frame wrapper per write. Scope-root/factory conversion happens after
releasing the stack borrow. Retirement faults cannot report a successful return while
the runtime is quarantined. The separate `finish_scalar_return` retirement path and its
exit enum, plus the now-unused `FrameSlots::set_location` wrapper, are deleted. Ordinary
native instructions still use their checked borrowed-argument entry adapter; framed
script/interface/shared/closure and boxed-native returns converge here.

Focused checks pass: VM closure contracts (9), GC/frame-window lifecycle contracts (9),
native function/closure/interface-handle boundary contracts (38, including source-free
shared applications and reentry/cancellation/depth cleanup), owned-drive contracts (2),
runtime method-application/active-window eviction contracts (5), and strict affected-target
diagnostics Clippy. Structure checks cover 1,008 Rust files with zero violations/exceptions.
The only intermediate warning was the superseded slot wrapper, which was removed.
No build/test error is carried; no full-workspace or GitHub CI run was performed.

The frozen `capture_cell` comparison has no explicit parameters, so it cannot establish
nonempty closure-argument allocation scaling. Two diagnostic-only fixtures now exercise
captured managed objects plus a scalar parameter, with fixed versus alternating closures
at one callsite. They extend the existing N/2N protocol probes without changing any
matched Lua workload or growing the semantic test matrix. All 72 cold/warm rows pass in
`target/hp03/closure-return-diagnostics.log`; every old warm request/byte/object,
metadata-validation, driver-admission and slow-boundary count exactly matches the prior
64-row invocation checkpoint (`target/hp03/closure-return-diagnostic-delta.json`). At both
2,500 and 5,000 calls, fixed closures use 12 warm allocation requests/553 requested bytes,
two heap objects and one metadata validation; alternating closures use 17/924, four
objects and two validations. Driver admissions grow 5,003 to 10,003, confirming the calls
still occur. Thus these concrete closure paths have no invocation-scaled allocation or
graph admission. There is no earlier binary measurement of the newly added fixtures;
this is N/2N evidence, not a fabricated before/after delta.

HP03 remains open for scoped type/application preparation and repeated environment graph
admission. Closure packing and separate frame-return retirement are now removed; the
remaining preparation work must preserve the same generation/GC/source-free boundaries.

Paired ordinary source forms against the invocation checkpoint are retained in
`target/lua-comparison/20261010T050644Z-forms-paired/results.json`. Baseline is
`target/hp03/invocation-executable` (hash above); candidate is
`target/hp03/closure-return-executable`, SHA-256
`5f819a4ebf4e3c13324451cc6e158634d16520fbe4848cf0555a6deef5bf5cdd`.
All checksums pass. Candidate/baseline ratios are direct 0.9850, helper 1.0345,
concrete generic 1.0596, interface 1.0272, shared generic 0.9993, capture cell 1.0014,
field 0.9999, native 1.0015, byte state 1.0029, host callback 1.0196, string constants
1.0116 and string calls 0.9930. Lua control ratios range 0.9972–1.0323. The original
capture-cell workload allocates no nonempty parameter vector, so its flat timing and
allocation counts must not be presented as evidence of savings from removing one.
Helper/concrete-generic regressions remain integration debt; the common return model
has not earned a general speedup claim.

The environment remains M1 Max, macOS 26.6.2, rustc 1.98.1, default workspace release
profile and Cargo parallelism; benchmark processes are single-threaded and sequential
baseline/candidate/candidate/baseline. Three warmups and 22 pooled samples per variant
exclude the 20.205 s incremental build. Diagnostic features are disabled for throughput,
normal GC is included, and matched workloads/compiler flags are unchanged. Only the
separate diagnostic source gained the documented closure scaling probes.

The original-seven comparison is retained in
`target/lua-comparison/20261010T050801Z-paired/results.json`; all checksums pass,
with the same executable hashes/environment and a 0.088 s incremental build excluded.
Candidate/baseline ratios are entry 1.0227, arithmetic 1.0179, branches 1.0140,
calls 1.0708, fibonacci 1.0233, arrays 1.0334 and maps 1.0089. Lua controls range
0.9947–1.0143. Calls remain 24.67 times Lua, and the 7.08% call-heavy regression
prevents performance acceptance of this transition.

Independent sequential calls sampling uses the preserved ordinary binaries and
`scripts/profile_lua_macos.py::sample`: 5 s at 1 ms within a warmed 10 s window,
with instruction counting afterwards and no concurrent build/test/throughput work.
Both execute 210,015 Kagari instructions versus 110,007 Lua instructions, with zero
heap object allocations and collections. Collapsed top samples for `finish_return`
fall 107 to 56, while `ExecutionCursor::execute_region` rises 120 to 274 and the
scalar loop rises 740 to 798. Sampling is observational and does not isolate a
single cause; the whole profiled window also completes fewer calls (1,869 to 1,742).
Return disassembly has fewer lines but increases its stack reservation by 64 bytes.
These facts do not justify attributing the regression solely to return checks or
adding compiler/alignment patches. Raw samples, execution logs, disassembly and
hashes are in `target/hp03/closure-return-profiles/`. HP03 integration and HP04/HP06
execution-region review retain responsibility for this regression alongside earlier
scalar regressions; no required checks were removed to recover time.

Final formatting, structure review, all 84 local document links and diff checks
pass. This checkpoint completes closure packing removal and common frame retirement,
not HP03 or the overall goal. Scoped type/application preparation and environment
graph admission remain the next architectural work; full-workspace and CI acceptance
remain deferred to their designated checkpoints.
