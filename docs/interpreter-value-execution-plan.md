# Compact values and interpreter execution (VE00-VE08)

Status: active, authorized on 2026-10-09. The user requested goal execution of
VE00-VE08 in order, following the debug-only diagnostic refinement. VE00-VE04 are
complete; VE05 prepared object/collection operations are next. Lua parity and
full-plan integration/CI acceptance remain open.

The [roadmap](implementation-roadmap.md#interpreter-performance-follow-up) owns
activation and queue placement. This plan owns phase order, implementation scope,
acceptance, decisions and the progress ledger for VE00-VE08. Update this ledger
rather than creating another migration checklist. Existing IP/NE implementation
completion and their still-open Lua parity acceptance remain separate facts.

## Outcome and finite scope

Separate checked host retention from compact execution values. Make proven facts
available directly to the interpreter, with roots and controlled execution
boundaries preserving lifetime and safety. Replace repeated reconstruction of
ownership, type and layout facts with prepared operations where their validity
can be established. Retain checks for facts that can change during execution.

Deliver two milestones, in order:

1. **VE00-VE04: values and references.** A compact, copyable internal value;
   explicit internal-reference/host-handle boundaries; shared immutable strings;
   indexed runtime constants; complete GC, host and asynchronous integration.
2. **VE05-VE08: interpreter execution.** Prepared concrete object/collection
   operations and script calls, a lower-overhead execution loop, and measured
   correctness/performance acceptance against the unchanged reference workloads.

The first milestone targets `size_of::<InternalValue>() <= 16` in release builds
on the supported 64-bit targets and Rust `Copy` without per-copy allocation or
reference counting. Debug-only diagnostic fields may enlarge the debug layout;
Copy remains required in both configurations.
`InternalValue` is a responsibility name, not a mandatory new public type. Retain
the existing untagged scalar banks; do not route scalar arithmetic through a
general tagged representation to simplify the migration. A small value alone is
not performance acceptance, and no overall speedup is assumed from its size.

The final performance gate is interpreter/Lua median execution time <= 1.0 for
each genuinely matched nontrivial workload, with repeated same-machine release
measurements and uncertainty analysis near parity. A geometric mean cannot hide
a failing workload. Host entry and setup are reported separately. Supported
semantics, execution path and checksum checks cannot be weakened to meet a ratio.

Exclude JIT expansion, new numeric or collection APIs, a new embedding facade,
moving/concurrent/generational GC, global dynamic-string interning, broad enum
unboxing, general escape analysis, a second implementation of library semantics,
and unrelated frontend repairs. Keep the current stop-the-world tracing model.
Address-stable storage is a representation change within that model, not a new
collector algorithm. New physical aggregate storage needed to compact Value is
included; a general unboxed aggregate ABI is not.

Use existing crates. Replace obsolete internal APIs directly, without compatibility
aliases, forwarding crates or a permanent second interpreter. Keep unpublished
format/ABI identifiers; refresh only affected disposable artifacts at a coherent
integration gate. Do not serialize process pointers or runtime-local handles.

## Baseline and implementation owners

Planning inspected a clean worktree at `f521b2f0`. No benchmark or Rust test was run
for this documentation checkpoint. The October 6 M1 Max NE05 report and October 7
Windows arithmetic report in [performance measurements](performance-baseline.md)
are historical evidence, not a measurement of the current post-async baseline.

Confirmed starting facts:

- `Value` derives Clone and contains owned String, Vec<Value>, RangeValue,
  three-word HeapObjectId, and Arc-backed host descriptors/borrow tokens. Existing
  measurements report a 32-byte Value; VE00 must measure the chosen baseline.
- HeapObjectId carries owner, slot and generation. GC storage is a growable
  `Vec<ObjectSlot>`; slot reuse increments the generation. An ID itself is not a
  root. Taking an element address from this Vec does not create stable storage.
- `RootedValue`/`RootSet`, execution windows and metadata reachability already
  provide retention. Runtime is Send and not Sync; exclusive transfer between
  workers must remain valid outside active scopes.
- The compiler collects a deduplicated module constant table, but LoadConst also
  contains a full ConstantOperand. String constants become `Value::Str(v.clone())`
  when interpreted. String constants currently leave the scalar execution region.
- Scalar frame banks, prepared numeric kernels and reusable argument windows are
  implemented. Numeric sequences already use concrete Vec element storage. Do
  not propose those capabilities as new work or attribute all collection cost to
  the width of Value.
- Historical sampling identifies call/session/frame overhead, module retention,
  type/conversion work and allocation in collection paths. Scalar workloads still
  spend substantial time in dispatch and execution bookkeeping without script GC.

| Owner | Responsibilities in this plan |
| --- | --- |
| [Runtime values](../crates/kagari-runtime/src/value.rs), [GC storage](../crates/kagari-runtime/src/gc.rs), [tracing](../crates/kagari-runtime/src/gc/storage.rs) | Physical values, internal object references, storage stability, traversal and controlled mutation. |
| [Roots](../crates/kagari-runtime/src/gc/roots.rs), [frames](../crates/kagari-runtime/src/frame.rs), [frame values](../crates/kagari-runtime/src/frame/values.rs) | Host retention, temporary roots, active/suspended slots, transfers and cleanup. |
| [Host descriptors](../crates/kagari-runtime/src/host.rs), [native conversion](../crates/kagari-runtime/src/native/conversion), [metadata](../crates/kagari-runtime/src/execution_metadata) | Checked host entry, scoped borrows, descriptor ownership, native views and version retention. |
| [Prepared execution](../crates/kagari-runtime/src/module/execution.rs), [cursor](../crates/kagari-runtime/src/frame/cursor.rs), [VM](../crates/kagari-vm/src/executor) | Concrete runtime binding, instruction execution, calls, boundaries and diagnostics. |
| [Bytecode](../crates/kagari-bytecode/src/instruction.rs), [compiler emission](../crates/kagari-compiler/src/bytecode.rs), contract/MIR/ABI owners | Checked portable facts, constant indices, physical operation contracts and source-free verification. |
| [SDK](../crates/kagari-embed/src), stdlib and current native consumers | Retained public values, explicit owned conversion, migrated callers and backend boundaries. |
| [Benchmark driver](../scripts/benchmark_lua.py), [comparison package](../benchmarks/lua-comparison/README.md) | Reproducible baseline/candidate measurements, checksums and cost attribution. |

Read [AGENTS.md](../AGENTS.md), [project goals](project_goal.md),
[architecture](architecture.md), [value semantics](spec/value-semantics.md),
[execution](spec/execution.md), [access/control](spec/security.md),
[host interop](spec/host-interop.md), [typed path mutation](spec/typed-path-mutation.md),
[debugging](spec/debugger.md), [artifacts](spec/artifacts.md) and
[runtime ownership](runtime-ownership-and-host-api-design.md) before implementation.
Reuse that context until a document or design decision changes. Preserve async
contracts and their pending CI status; this plan does not reopen AX semantics.

## Architecture contracts

### Internal references and host retention

Separate the following responsibilities even if an existing type name is reused:

| Representation | Required contract |
| --- | --- |
| Internal execution value/reference | Compact data used only in admitted execution/storage contexts. Copies do not establish roots. No arbitrary safe public construction or unchecked dereference of externally supplied bits. |
| Host-retained handle | Runtime identity plus checked root identity and retention. Cross-runtime, stale and released handles remain rejectable before conversion to an internal reference. |
| Scoped host reference/path | Retains schema, access, borrow epoch and no-escape rules. It never becomes an unrestricted Rust reference or an ordinary durable heap payload. |
| Prepared executable facts | Immutable, verified type/layout/call facts. Runtime-local references live in a distinct runtime binding, not in shared portable products. |

Prefer address-stable objects and direct internal references for the initial
design evaluation. Compare against one compact-index alternative in VE00, then
select one production representation. Do not retain a runtime-configurable pair.
Direct pointers are not a preapproved speed claim or permission to delete checks.

For each removed repeated check, record the checked constructor/entry, owning
scope, invalidation events and enforcement that make the check redundant. Keep
runtime ownership, generations and bounds at every boundary where a foreign,
stale or unproven value can enter. Internal instruction/layout bounds must be
established by the executable verifier and prepared layout or checked on access;
do not replace validation with an undocumented assumption.

An address-stable design must define allocation, reclamation and reuse; live
address stability; alignment and pointer provenance; object kinds and layouts;
drop behavior; root publication; and exclusive thread transfer. No Rust reference
to an object payload or resizable buffer may cross an operation that can invalidate
it. Localize any unsafe implementation behind reviewed storage/access operations.
An unsafe Send implementation needs an ownership argument, not a blanket assertion.

Keep existing invalid-input rejection and invariant-failure quarantine. Raw bits,
portable IDs and foreign handles cannot be accepted as internal pointers. Preserve
meaningful fault-injection coverage at the new contract owner; do not turn an
existing checked boundary failure into undefined behavior to shorten the path.

### Debug-only invariant diagnostics

The user explicitly permits extra debug-only fields and checks to support the
reference migration. Use `#[cfg(debug_assertions)]` for diagnostic fields or side
metadata, such as expected heap owner, allocation generation, object kind/layout
or execution-scope identity. Use debug assertions to cross-check facts already
guaranteed by validated construction, roots and the admitted execution context.
Gate on debug assertions explicitly rather than assuming an optimization level
or profile name determines instrumentation.

These diagnostics must not supply a missing release invariant. Foreign/stale host
handle rejection, artifact validation, dynamic bounds, borrow validity and required
failure/quarantine behavior remain enforced in release. Root publication, cleanup,
state updates and other required work must never occur only inside a debug assertion.
Debug checks must establish pointer validity before dereferencing; reading freed
memory to inspect its supposed generation is not a valid diagnostic.

Diagnostic metadata must not add strong roots, change collection eligibility or
retain old code/resources in a way that hides missing production retention. Keep
it separate from portable formats and native ABI layouts. Debug-only fields in a
Copy value must themselves be Copy. Measure the release size/performance contract
separately from diagnostic overhead, and exercise the affected correctness paths
with debug assertions both enabled and disabled.

### Roots, execution regions and publication

The complete object and executable-metadata graph remains traced. Roots include
active/suspended frames, native temporaries, host leases, installed/staged state,
debugger retention and reachable Future/Task results. A Rust local containing a
Copy value is not implicitly a GC root.

| Transition | Required work |
| --- | --- |
| Enter from host | Validate runtime/root identity and parameter contracts; retain arguments and code before admitting internal references. |
| Execute a closed operation | Use prepared facts and the current frame window. It must not invoke an unclassified callback, collect, suspend or invalidate outstanding data views. |
| Allocate or invoke a callback | Publish live values and the logical PC; protect temporaries; release incompatible borrows before any possible collection/reentry. Unknown native effects use the full boundary. |
| Return or replace a value | Protect outgoing/new references before removing the old frame/root; preserve commit and failure order. |
| Suspend/resume | Store roots and exact code/environment dependencies in owned execution; hold no transient raw views or scoped host borrows while parked; re-admit on resume. |
| Trap/cancel/depth failure | Release frames, temporary roots, borrows and code retention through the existing cleanup contract. |

Keep the current safepoint and cancellation/observation behavior through VE04.
Changing representation does not authorize coarser cancellation polling, altered
instruction slices or debugger locations. VE07 may optimize bookkeeping only
after mapping every logical boundary and preserving observable behavior. Any
proposed semantic change must be explicit and separately agreed, not hidden in
instruction fusion. No blanket ban on GC during native callbacks is acceptable.

Current stop-the-world tracing does not require a new generational write barrier.
Keep controlled mutation/publication APIs so initialization, roots, metadata
edges and object writes are complete. Preserve candidate-initialization isolation,
host dirty records, once-only evaluation, trap order and completed side effects.

### Compact value coverage

VE00 must inventory every Value variant and every public/native/ABI consumer.
Removing one large variant is insufficient while another determines enum size.

| Current contents | Required migration decision |
| --- | --- |
| Unit, booleans, integers, floats | Preserve full domains, checked arithmetic and float-bit contracts; retain untagged scalar execution. No lossy NaN boxing. |
| String | Immutable shared script storage, accessed by compact reference; explicit owned Rust conversion may copy. |
| Tuple | Shared immutable aggregate storage preserving each member's semantics and traversal. Aliased mutable members remain shared; do not add tuple identity operators. |
| Range | Preserve both full-width endpoints and exact range/item semantics. Choose compact indirect storage or a prepared typed representation; never allocate one range object per iterator step. |
| Struct/array/map/set/enum/native/interface/closure/cell | Compact reference with the correct storage/tracing owner. Retain object identity where specified, metadata edges and generation-pinned behavior. |
| Host root/path/ephemeral values | Runtime- or scope-owned descriptors referenced compactly if still in Value. Trace dynamic path arguments and enforce no-escape; copying a borrow token does not extend its validity. |

Do not make all Rust metadata GC-managed merely to remove Arc from Value. Own it
at its existing module, execution or descriptor boundary, with explicit graph
edges and bounded reclamation. Do not create permanent descriptor tables or new
strong retention cycles. Host-owned resources remain outside the script heap.

If a value representation permits temporarily non-storable members, its storage
must retain the same prohibition on heap publication, closure capture and await
escape. Moving tuples or descriptors out of line must not conceal ephemeral data.
Diagnosing expired host state must not depend on trusting a freed pointer.

Public handles remain retained rather than Copy. Moving a shared host value into
another runtime is validated, even if scalar copies are runtime-independent.
Adapt native ABI conversion explicitly; an internal Value layout is not an
implicit change to every ABI slot or serialized constant.

### Strings and constants

Keep portable constant data and runtime materialization distinct:

```text
verified constant index + immutable text
    -> runtime/program-version constant binding
    -> GC string reference
    -> Copy into execution slots and traced objects
```

Use checked constant indices in the executable path, not a repeated owned text
operand or a text-keyed lookup on every load. Decide in VE00 whether canonical
LoadConst itself becomes indexed or indexing is owned by prepared execution;
there must be one authoritative portable constant identity and bounded verification
of every use. Retain float-bit identity and source-free round trips. Shared
VerifiedProgram/preparation data must not embed another runtime's object handles.

Materialize each distinct module string constant once per live runtime/version
binding, eagerly or on a correctly rooted first use. Warmed repeated loads perform
no character copy, allocation or reference-count update. Module-local deduplication
is required; cross-module interning and deduplication of dynamic strings are not.

The constant binding retains its strings while the owning program version needs
them. Escaped strings survive through ordinary roots independently of that binding.
Either strings own their bytes or retained backing storage has an explicit lifetime;
an unretained slice into unloaded bytecode is invalid. Old pools become reclaimable
after code and object reachability end, without pinning all previous reloads.

Native read-only operations use scoped access to string contents. If they can
callback, allocate or suspend, root the string and end/reacquire the view at the
appropriate boundary. Owned `FromKagari for String` remains a deliberate copy;
do not silently change host ownership semantics. Account for owned conversions
separately rather than claiming every string operation becomes allocation-free.

### Prepared object operations, calls and dispatch

Concrete script field access should consume a prepared receiver layout, field
location and representation. Array operations should use the prepared storage
contract while retaining dynamic index and mutation checks. A read-only view does
not prove that aliases cannot mutate its referent. Hash/Eq callbacks may reenter;
preserve revision/lease checks and never carry an invalid container view across
them. Host-backed fields continue through checked host path adapters.

Library registration and checked contracts remain authoritative. A concrete fast
path must select a declared implementation and share its semantic operations,
not recognize a spelling and bypass an override. Dynamic trait/generic paths use
checked receiver-specific method/layout records; their remaining dispatch cost
is measured separately. Executable backends never re-resolve source or infer types.

Prepared script call records own the exact target/dependency version, frame layout,
parameter transfer and return destination. Known calls need no repeated symbolic
module/type resolution. Continue to check call depth, cancellation, dynamic borrow
state and new host inputs at their actual boundaries. Calls and returns must not
create a root gap, including generic/interface result adaptation and async factories.

The execution region keeps current code/frame/window access admitted until a real
boundary. Reduce repeated shared-state lookup/RefCell borrowing without holding an
exclusive Rust borrow across synchronous reentry. Runtime being Send does not make
concurrent heap execution valid. Close and reconstruct region state around callbacks.

VE07 evaluates redundant instruction removal, a bounded set of measured fusions,
and numeric handler dispatch. Preserve logical origins, left-to-right evaluation,
checked numeric traps, debugger stepping and instruction-slice accounting. Do not
remove the callee or change the source benchmark merely to improve its timing.

## Ordered execution phases

Do not start VE05 before the VE04 value/reference gate is accepted. Completing
VE04 is a valid finite implementation milestone, not acceptance of Lua parity.

### VE00: Baseline and representation decision

- Refresh the current release baseline and preserve its executable/hash before
  edits. Inventory sizes, references, roots, ABI consumers and string allocations.
- Freeze the matched workload set and semantic limits. Extend existing measurement
  owners only for missing strings, objects, closures/traits and host boundaries;
  a previously unsupported frontend case remains explicitly excluded and tracked.
  Freeze those benchmark additions before preserving the comparison executable so
  baseline and candidate run the same workloads against their production engines.
- Compare address-stable direct references with one compact-index candidate using
  bounded allocation/access probes. Include GC/root and host-boundary costs;
  do not infer whole-program speed from dereference microbenchmarks.
- Record the selected representation, allocator, root protocol, descriptor lifetime,
  constant-index owner and native conversion strategy in this plan. Explain any
  unsafe scope and Send proof. Reject designs that require an unvalidated input or
  silently discard a specified safety check.

Acceptance: one implementable design, a complete variant/consumer map, reproducible
baseline and fixed acceptance workloads. No permanent competing implementations.

### VE01: Storage and reference ownership

- Establish selected object storage and checked internal-reference constructors.
- Integrate existing host roots, execution roots and temporary publication before
  exposing direct access. Keep external identities and rejection behavior intact.
- Cover stable allocation, reclamation/reuse, metadata traversal and runtime
  transfer; audit all callbacks/destructors and native payload address constraints.

Acceptance: existing GC ownership/root/reentry contracts pass with the new storage.
No unchecked public reference API, root gap or invalid pointer survives a boundary.

### VE02: Compact internal values and consumers

- Replace owned inline strings/tuples and oversized value payloads with the selected
  representation; migrate all variant users and host descriptor ownership.
- Preserve scalar banks, aggregate semantics, map-key preparation, formatting,
  reflection, debugging and native/ABI conversion. Remove obsolete internal forms.
- Extend existing allocation/layout coverage to prove the release <=16-byte and
  both-configuration Copy contracts, and identify object/header/backing-store costs
  separately from debug-only instrumentation.

Acceptance: affected producers and consumers compile together, value semantics
and no-escape checks pass, and there is no hidden per-copy ownership operation.
Record allocation changes for tuple/range-heavy code, not just Value size.

### VE03: Runtime constants and string access

- Implement the checked indexed load path and runtime-local materialization.
- Root constant bindings, handle escaped strings and release unused old versions.
- Migrate internal string operations to scoped shared access and retain explicit
  owned host conversion. Preserve artifact validation and multi-runtime sharing.

Acceptance: warmed constant loads and internal string copies allocate/copy no text;
content equality/hash, Unicode boundaries, cross-runtime rejection and source-free
execution remain correct. Dynamic string interning is not needed for completion.

### VE04: Value/reference integration gate

- Exercise callback-triggered GC, trap/cancel/depth cleanup, suspended Future/Task
  roots, output conversion, old-version escape/release and exclusive runtime transfer.
- Complete affected SDK, stdlib, reflection/debug and currently supported backend
  adapters. Update architecture/specification descriptions to the implemented model.
- Measure representation size, allocation counts, access/string workloads and the
  original interpreter workloads against VE00. Explain any material regression;
  do not accept a representation that merely transfers cost to repeated boxing.

Acceptance: first milestone requirements pass, all carried errors are resolved,
and measured results/limitations are recorded. Full Lua parity can remain open.
This is an intermediate gate of the full plan, not a reason to run workspace
suites repeatedly; use the focused policy below.

### VE05: Prepared concrete object and collection operations

- Prepare version-correct script field access and concrete sequence/map operations
  from checked contracts. Reuse semantic storage operations and declared providers.
- Remove repeated type/layout reconstruction where facts are sealed. Preserve
  dynamic bounds, alias/iteration restrictions, callback effects and commit order.
- Measure concrete and interface/generic paths separately. Track heap-backed enum
  result cost if still dominant; do not silently activate general enum unboxing.

Acceptance: field/collection contracts pass, known paths avoid repeated semantic
lookup, and end-to-end measurements establish actual benefit without hiding
callback, conversion or GC costs.

### VE06: Prepared script calls and frame transfers

- Bind concrete targets and parameter/return transfers ahead of execution.
- Reuse frame arenas; reduce repeated session/module lookups and ownership work.
- Integrate dynamic interface/closure/native boundaries and async frame creation
  without erasing their effects or generation pinning.

Acceptance: direct, recursive, concrete generic, closure and interface calls keep
semantics; warmed scalar/window allocation guarantees remain valid; measured
helper-versus-direct cost and instruction counts are reported independently.

### VE07: Execution-region and instruction costs

- Profile the post-VE06 scalar and mixed routes before selecting a small concrete
  set of instruction/dispatch changes. Inspect generated code where attribution
  is unclear; do not assume a handler organization is automatically faster.
- Remove redundant transfers and select bounded fusions/handler changes based on
  the profile. Reuse admitted frame/code access until an actual boundary.
- Preserve cancellation, observer events, sticky termination, suspended state,
  logical PCs, instruction slices and trap order through changed instruction shapes.

Acceptance: numeric/source-form semantics and debugging boundaries pass, instruction
counts and per-workload timings explain the change, and unchanged regressions are
not dismissed as a smaller Value or a different instruction-count convention.

### VE08: Final integration and performance acceptance

- Resolve all carried failures; update current architecture/specifications and
  remove temporary prototypes. Run the batched final local checks and report CI
  independently. Do not claim unrun backend/feature matrices passed.
- Run paired preserved-baseline/candidate/Lua measurements for the frozen set;
  publish results in performance-baseline.md with links from this ledger.
- Evaluate correctness, representation, local integration, CI and Lua parity as
  separate statuses. A passing migration cannot be marked full goal acceptance
  while a required matched workload still exceeds Lua.

If parity fails, record the failing workloads, evidence and one bounded proposed
follow-up in this ledger. Leave performance acceptance open. Do not keep enlarging
VE08, lower the target, switch to JIT, or disguise unsupported semantics as a pass.

## Validation and measurement policy

Use existing contract owners before adding tests. Reuse GC ownership/roots,
native conversion and callbacks, capture cells, numeric conformance, source-free
artifacts, generic reload, runtime transfer and async lifecycle fixtures. Add only
a missing semantic/core-boundary case or a meaningful layout/allocation contract;
do not retain temporary microbenchmark smoke tests as extra regression suites.

Select a small affected subset per phase, not the following entire list every time:

```text
cargo test -p kagari-runtime --test gc_ownership
cargo test -p kagari-runtime --test native_conversion
cargo test -p kagari-vm --test native_allocations
cargo test -p kagari-embed --test generic_reload
cargo test -p kagari-embed --test runtime_transfer
cargo test -p kagari-embed --test native_artifacts
cargo test -p kagari-embed --test async_execution <affected-test-filter>
```

Locate the current host path, reentry, root and debugger test names when selecting
the affected subset. Run relevant safety instrumentation on any newly unsafe
storage/access owner where the toolchain supports it; record unavailable coverage
and its compensating review rather than inventing a passing result.
When changing debug-only representation or access diagnostics, run the small
affected contract subset in the normal test profile and with `--release`, verifying
the debug-assertion configuration. This does not require two full workspace runs;
tests must cover release behavior rather than depend on diagnostic-only rejection.

Every implementation checkpoint builds its affected consumers and passes its
focused contracts. Do not carry a deliberately broken public/internal API into
the next phase. Record any intermediate failure with command, diagnostic, cause
and owning phase; resolve it before the checkpoint. No stubs or weakened assertions.

Review changed handwritten Rust ownership, visibility, imports, qualified paths,
macro bodies, function scope and effective LOC. Run structure checking, formatting
and `git diff --check` at implementation checkpoints; structure exceptions require
the existing narrow evidence policy. Documentation-only checkpoints require local
link/content and diff checks, not a Rust rebuild or a performance run.

At full-plan final acceptance, batch the repository's permitted workspace checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Complete feature/backend matrices remain GitHub CI work. If final validation finds
a failure, fix with focused tests and repeat a full run only when needed to establish
acceptance. Do not repeat unchanged successful suites or split them across commands
to evade the phase-local policy.

Measurement commands already available include:

```text
uv run python scripts/benchmark_lua.py --interpreter-only --workload arithmetic
uv run python scripts/benchmark_lua.py --interpreter-only --baseline-executable <preserved-baseline-executable>
uv run python scripts/benchmark_lua.py --interpreter-only --numeric-matrix
uv run python scripts/benchmark_lua.py --interpreter-only --source-forms
```

Use the workspace release profile, default target directory and Cargo parallelism.
Record commit/worktree diff, binary/source hashes, toolchain, machine, features,
cache state, inputs, warmups, sample distribution and process order. Do not run
builds/tests/profilers concurrently with throughput timing. Separate compile,
verification/preparation, linking, host entry and execution; keep ordinary GC
enabled and matched script work inside the timed route. Verify every result.
Existing end-to-end execution comparisons continue to include public host entry
and return. Isolated kernel timings are diagnostic and cannot replace those
comparisons or establish parity by subtracting boundary costs from one engine.

Use separate runs for allocation counts, GC counts, instruction counts and sampling.
Record payload/header/metadata footprint as well as Value width; fewer objects or
bytes do not imply equal RSS. Preserve original matched sources and algorithms;
new workload variants must have independent expected results and declared semantic
limits. Check checked-integer/float domains separately from Lua comparisons. Do not
claim a matched native/host comparison where the two boundaries are different.

Raw logs and prototypes belong under ignored `target/`. Durable environment,
reproduction commands, conclusions and carried failures belong in this ledger and
the existing performance report. Record the exact selected filters and results.

Commit coherent checkpoints with Conventional Commits and an
`Interpreter-Phase: VE00` through `Interpreter-Phase: VE08` trailer for implementation
work. Mark breaking public API changes with `!` and explain them. Planning-only
commits do not claim phase completion. Do not amend unrelated user commits.

## Decisions, checklist and ledger

### Decisions at the planning checkpoint

- Correctness, host safety, roots, generation checks and source-free validation
  remain requirements. Performance changes their placement only with an explicit
  validity argument; the plan does not authorize deletion of these contracts.
- The internal/host-reference split is the target boundary. Direct references in
  stable storage are the preferred candidate; VE00 still owns the measured choice.
- Release <=16-byte Copy internal values and shared runtime string constants are
  the first milestone. Debug-only invariant fields may enlarge the debug layout
  without changing retention or release guarantees. NaN boxing, an 8-byte universal
  value and global string interning are not acceptance requirements.
- Keep existing scalar banks, typed collection storage, root leases and code-version
  graph. Change their boundaries where needed rather than duplicating them.
- Safepoint behavior is unchanged during the first milestone. General enum
  unboxing and a new collector remain deferred even if final profiles motivate them.

### Phase checklist

- [x] VE00: Current baseline, consumer inventory and selected representation.
- [x] VE01: Storage, internal reference ownership and roots.
- [x] VE02: Compact Copy values and complete consumer migration.
- [x] VE03: Runtime string constants and shared string access.
- [x] VE04: Value/reference integration and measurement gate.
- [ ] VE05: Prepared object and collection operations.
- [ ] VE06: Prepared script calls and frame transfers.
- [ ] VE07: Execution-region and instruction costs.
- [ ] VE08: Final integration and paired performance evaluation.
- [ ] Final local correctness/integration acceptance.
- [ ] Complete GitHub CI feature/backend acceptance.
- [ ] Lua parity for every frozen genuinely matched nontrivial workload.

### 2026-10-09: Planning checkpoint

Inspected values, GC storage/roots/collection, host descriptors, native string
conversion, constants, prepared execution, field/index access, call transfer,
async roots and existing measurements at `f521b2f0`. Added this finite phase plan
and connected it from the roadmap/documentation index. Implementation is not
activated and no VE phase is marked complete. No runtime code, test expectation,
artifact, format identifier or benchmark result changed.

Planning validation: all 81 local links/anchors in this plan and the documentation
index resolve; VE00-VE08 headings/checklist and the roadmap link are consistent;
no implementation checkbox is marked complete. `git diff --check` passes.
No new carried build/test failure was discovered because no build/test was run.
Existing roadmap gaps, including shared generic Add lowering and pending prior
track CI, remain separate; VE00 must reconfirm their status before selection of
the final workload set. No present-worktree performance or parity claim is made.

### 2026-10-09: Debug-only diagnostic policy

The user authorized additional debug-only fields to help establish correctness.
Scoped the size target to release builds, retained Copy in both configurations,
and added owner/generation/layout diagnostics with focused debug/release validation.
Diagnostics cannot provide required production checks or hide root/lifetime errors.
This is a plan refinement; implementation remains unstarted. Documentation content
and local link checks plus `git diff --check` pass; no Rust test or benchmark ran.

### 2026-10-09: Activation and VE00 baseline preparation

Goal execution of VE00-VE08 is authorized. Starting revision: `19fe129d`, clean
worktree. Establishing the current baseline and freezing the missing string and
matched host-callback workloads before preserving its executable. Original seven
workloads and existing source-form bodies remain unchanged. No performance result
or reference-representation decision is recorded until measurement completes.

### 2026-10-09: VE00 acceptance

The [current baseline](performance-baseline.md#compact-value-baseline-ve00-2026-10-09)
records machine/toolchain/profile, complete median/range tables, allocation counts,
probe limits and reproduction commands. Production is unchanged at this checkpoint.
Preserved executable: `target/ve00/baseline-executable`, SHA-256
`60aa2c07334fb25123208c8c77a4008d1bc93d2f680da6ac21560838e7ebfc81`.
Use this binary for later paired comparisons; do not overwrite it with VE01 builds.

**Frozen acceptance workloads.** Keep the original six nontrivial workloads and
the source-form direct/helper/concrete_generic/interface/shared_generic/capture_cell/
field/byte_state/string_constants/string_calls routes. The host_callback route
separately measures the same Rust arithmetic function through both host adapters.
Entry is reported separately. The existing native row is diagnostic only because
its boundaries differ. Numeric-matrix rows compare only their bounded exact inputs;
full-domain checked numeric semantics retain their independent contract tests.
Shared generic Add default-body compilation was reproduced with a trait method
`forward<T: Add<T>>(self, a: T, b: T) -> T::Output { a + b }` invoked through an
interface. MIR lowering still rejects `MissingBinding("checked callable requirement")`.
That frontend gap stays excluded and deferred; the supported identity method does
not substitute for its correctness. No workload was dropped because it is slow.

**Selected representation: checked compact indices.** Use a private-field
HeapObjectId of three u32 words: runtime owner, slot, allocation generation. Keep
contiguous Vec<ObjectSlot> storage and its existing RefCell access discipline.
The release layout probe verifies that Rust's default enum layout fits this
12-byte payload and full-width u64/f64 scalars in a 16-byte enum. Assert the actual
production size; never expose that compiler-chosen layout as a wire/native ABI.
The index constructor stays inside the heap. Copying it grants no retention or
dereference capability. Public host retention remains the non-Copy RootedValue
lease, with owner/generation/lease checks. Unrooted raw values can safely become
stale and are rejected by checked access; no dangling Rust pointers are created.

The direct-reference candidate used address-stable per-object boxes, checked
external identities and separate admitted references. It won the admitted-read
probe but lost allocation/drop and checked host admission; it also requires an
additional reference/scope admission layer across current raw-value consumers.
The compact-index alternative meets size/Copy goals without that unsafe lifetime
surface or per-object allocation. This bounded comparison does not rule out a
future stable slab design and does not predict end-to-end performance. There will
be one production representation, and VE04 remains its regression gate.

Counter policy is part of correctness: owner exhaustion fails explicitly before
reusing an identity; reject a new slot beyond u32 capacity before storage/accounting
mutation; retire a slot when its u32 generation cannot increment. Never truncate
an existing u64 identity or wrap a generation. Metadata/root identities can retain
their current widths; widening the compact owner is exact. These limits replace
unreachable practical capacities, not ownership/generation validation. VE01 covers
reuse/exhaustion, foreign values, roots and exclusive runtime transfer.

No new unsafe code or unsafe Send impl is needed for this design. Vec relocation
is harmless to IDs. Scoped payload access borrows the owning storage; conflicting
mutation/collection must reject before changing data, and no borrow survives a
callback, reentry or suspension. Runtime remains Send and not Sync through existing
field ownership; retained host leases remain independently Send/Sync. Native
payload addresses gain no new stability promise. Collection tracing/destructor
quarantine stays unchanged.

**Complete value migration map.** Keep Value as the compact transport name; use
heap-aware accessors instead of pretending an ID provides a Rust reference.

| Current variants | Selected storage/access | Semantic and lifetime requirements |
| --- | --- | --- |
| Unit, Bool, I32, I64, U64, F32, F64 | Inline tag/scalar; existing untagged frame banks | Preserve numeric domains/float bits; no pointer interpretation. |
| Str | Immutable heap text, compact ID; scoped read | Content equality/hash; explicit owned host String conversion copies. Copies in frames/objects do not copy text. |
| Tuple | Immutable heap member buffer, compact ID | Trace every member; recursive storage/borrow checks consult the heap, including temporary non-storable tuples. Preserve value equality and shared mutable children. |
| Range | Immutable heap RangeValue, compact ID | Full-width endpoints/type/kind; iterator stores progress separately, with no per-step range boxing. |
| Array, Map, Set, GcHandle | Existing native storage with compact IDs | Keep concrete Vec storage, leases, revisions, selected operations and native tracing/drop contracts. |
| Enum, Struct | Existing payload/layout records with compact IDs | Preserve structural enum equality, struct identity, checked kinds/layouts and candidate isolation. |
| Interface | Compact ID to existing interface object/snapshot graph | Trace data and method/environment metadata; retain runtime/code generations. |
| Closure, Cell | Existing snapshots/cells with compact IDs | Capture roots, shared mutation and generation-pinned call targets. |
| HostRoot, HostPathView | Traced descriptor records containing existing checked handles | Host resources stay outside the heap. Trace dynamic path args; descriptor records are reclaimed through ordinary roots, not permanent tables. Preserve no-escape restrictions. |
| Ephemeral (HostRef, HostMut), RuntimeEphemeral | Host borrows use compact IDs to scoped-token records; runtime-only IDs use a separate inline tag | Heap record allocation does not make tokens storable. Keep owner/frame/epoch checks and distinct admission domains; no nested oversized enum in Value. |

Only transport descriptors become traced records; registry, module, type metadata
and actual Rust resources retain their existing owners. Existing Arc-backed
descriptor contents may remain inside a record if needed by their API. Collection
does not release a borrowed Rust resource merely because its token record dies;
the host scope remains the validity authority. No descriptor creates an internal
root or cycle of strong metadata ownership. Heap-aware storable/borrow predicates
must inspect out-of-line aggregates and reject invalid IDs in release.

**Consumer and boundary map.** The inventory found 375 source lines in 87 files
matching large variants (including frontend namesakes, which are not runtime Value).
Migrate by contract owner rather than mechanically replacing frontend constants.

| Consumer | Required integration |
| --- | --- |
| gc/storage, collection, roots; native payload trace callbacks | Trace new text/aggregate/descriptor records; retain graph atomicity, root leases, slot retirement and quarantine. |
| frame/values, cursor, arguments, transfer, session and VM executor | Copy slots/windows; publish live values around allocation/calls; keep scalar banks, origins and cancellation. |
| module/state, execution_metadata and runtime object constructors | Version-owned constant edges, installed/staged roots, closure/interface metadata and checked payload construction. |
| native/conversion scalars/tuples/composites/context, native/objects and typed adapters | Allocate through owning conversion context, root child values during conversion, explicit owned decoding; use scoped text access for readonly operations. |
| host registry/scope/borrows, dynamic paths and typed mutation | Resolve descriptor IDs through runtime; keep schemas, epochs, once-only arguments, mutation commit and non-escape rules. |
| value_check, MapKey, value_semantics, builtin standard, numeric | Keep type checking and content equality/hash/formatting distinct from Rust ID equality; numeric scalars stay allocation-free. |
| stdlib bindings/construction, string iterator and range operations | Read shared text without owned decoding; preserve Unicode/bounds and range progress. |
| reflection, error_trace, debugger and SDK language_contract/handles | Materialize owned display data only when requested; preserve retained roots and declared member access. |
| Future/Task payloads and async conversion/execution | Trace outputs/errors/suspended windows, root publication before handoff, cleanup and no scoped-borrow escape. |
| runtime/jit_abi and Cranelift/ABI consumers | Current JitValue decodes only Unit/Bool/I32; keep explicit scalar ABI. Internal Value layout is not serialized or transmuted into ABI slots. |
| CLI, examples and existing unit/integration fixtures | Use owning allocation/access APIs and semantic comparisons; preserve foreign/stale fault injection. |

**Constants and native conversion.** Canonical bytecode LoadConst will use checked
indices into the module constant pool, making that pool the portable identity owner.
Compiler emission and source-free verification must agree on bounds and float-bit
identity. Shared prepared programs store portable indices only. Runtime module state
owns lazily or eagerly materialized immutable constants and exposes their edges to
the existing program graph. The binding is retained by its exact program version;
escaped values remain rooted independently, and dead versions release constants.
No text-keyed lookup occurs on each execution. VE03 owns this change and the focused
artifact refresh. Explicit host String conversion remains owned; internal readonly
string operations use checked scoped heap access and release it before callbacks.

**Measured starting costs.** Value 32 bytes; HeapObjectId/RangeValue 24 each;
EphemeralValue 16; RootedValue 32; descriptor payloads 40/104/48 bytes. A warmed
5,000-iteration string constant execution requests 480,007 Rust allocations/reallocs
and 50,155,179 bytes; adding an identity call gives 500,007 and 51,455,175 bytes.
These include conversion/type/root bookkeeping, not just text. Both have zero
script heap allocations/collections. VE02/VE03 must distinguish moved allocations
from eliminated copies; small Value alone cannot explain or close the current gap.

Validation: all original/source-form/numeric baseline checksums pass (1,364 batches);
the paired source-form check exercises the expanded driver with both binary roles.
The structure checker initially found one qualified import in the new benchmark;
it was repaired with an explicit LuaError import. The final release build,
formatting, structure (981 files, zero violations), Python syntax, 79 local
document link targets and diff checks pass. All 88 probe checksums also pass.
No production Rust contract changed, and no workspace tests or CI matrix ran.
The reproduced shared-Add frontend error is pre-existing and outside VE scope.

### 2026-10-09: VE01 compact checked storage identities

HeapObjectId is now 12 bytes with private u32 owner/slot/generation fields. The
global owner counter fails on exhaustion; allocation checks the slot capacity
before insertion/accounting; collection retires exhausted generations. Root and
metadata identities retain their existing widths, leases and graph protocols.
All storage accesses still validate owner, bounds, generation and object kind at
their existing contract owners. No check was removed, no raw reference API or
unsafe code was added, and no payload address stability is assumed. The Rust
`generation()` result changes from u64 to u32; this is an unpublished API change,
with no native/wire-format change or version bump.

Updated storage consumers to use the checked ID's usize index accessor. Extended
the existing layout contract for the 12-byte ID and added a missing exhaustion
boundary case: the final valid generation is usable while rooted, is reclaimed,
then its slot is permanently retired without making old values valid again.
Value itself remains 32 bytes until VE02 removes its other large payloads.

Focused validation passes with the CommandLineTools environment recorded in VE00:

- `cargo test -p kagari-runtime --test gc_ownership`: 6 pass, also with `--release`.
- `cargo test -p kagari-runtime --lib gc::`: 22 pass, also with `--release`.
- `cargo test -p kagari-runtime --lib scalar_slot_layout_does_not_inline_host_descriptors`:
  1 pass, also with `--release`.
- `cargo test -p kagari-embed --test runtime_transfer`: 1 pass; also builds the
  current source/native SDK consumer graph.
- `cargo test -p kagari-vm --test native_boundary native_boundary_callbacks::stored_callbacks_trace_captures_through_an_ordinary_native_payload`:
  1 pass, including native-triggered collection, traced captures and script reentry.
- Formatting, structure (981 files, zero violations), reviewed imports/visibility/
  ownership and `git diff --check` pass. No workspace suite or CI matrix ran.

The first debug ownership build found E0277 in gc/native/managed.rs: two remaining
u32 slot indexes had not migrated to `index()`. VE01 repaired both before the
successful checks above. No error is carried into VE02. Logs are under
`target/ve01/`; production changes remain confined to reference representation.

The preserved pre-VE01 allocation-probe binary additionally measured 5,000 tuple
constructions/member reads and 5,000 one-element range traversals, both checksum
325,000. Tuple baseline: 35,007 Rust allocation/realloc calls, 1,680,179 requested
bytes, zero GC objects/collections. Range baseline: 1,540,708 calls, 107,233,227 bytes,
15,000 GC objects and 25 collections (existing cursors/Option results included).
These are diagnostic allocation baselines for VE02, not new parity workloads.
Sources and outputs: `target/ve00/tuple-range.kgr`, `tuple-range-allocations.log`;
the probe's fixed entry aliases are string_constants=tuple and string_calls=range.
Use the same source with the candidate allocator probe at VE04. Temporary drafts
using unsupported Rust-style tuple access/destructuring were corrected to Kagari's
`pair[1]` before measurement; they were never benchmark or production changes.

### 2026-10-09: VE02 consumer migration in progress

Value now derives Copy and stores compact IDs for strings, tuples, ranges and host
descriptors. New immutable payload accessors require the owning heap. Tuple storage
caches no-escape properties computed from validated immutable members; these facts
do not retain roots and never permit publishing scoped members. Host borrow token
records store the Copy token directly rather than another Arc allocation. Path
descriptor tracing includes dynamic arguments from the complete base-view chain.

Native conversion now protects strings as GC values. Iterator preview keeps owned
construction data until all storage views are released, then materializes and roots
the item before the result callback. Canonical constants still allocate strings on
load at this intermediate phase; VE03 owns indexed runtime materialization.

The runtime library check passes. Consumer and fixture migration is still in
progress, so VE02 has not passed its build or correctness gate. `cargo check -p
kagari-runtime --tests` currently exposes old owned-value constructors, host path
constructor arguments and heap-free borrow predicates in fixtures; VE02 owns all
of those errors. Logs are under `target/ve02/`. No intermediate API checkpoint will
be committed with these errors, and no production performance claim is made.


### 2026-10-10: VE02 compact values and consumer integration

All Value payloads now satisfy Copy. Strings, immutable tuples, ranges and scoped
host descriptors use checked 12-byte object IDs. Runtime-only ephemerals keep a
separate inline ID tag, preserving their rejection by the HostHandle/Generic
representation checks; they cannot be mistaken for frame-scoped host borrows.
Rust PartialEq is transport identity, while language equality/hash/order continue
through content-aware runtime operations. Tuple updates allocate immutable
membership and share unchanged member values. Scalar banks and typed collection
buffers remain in place. No unsafe code or unchecked reference API was introduced.

All affected runtime, stdlib, VM, embedding, CLI and benchmark targets compile.
Constructors, native conversion, reflection, diagnostic snapshots, host callback
fixtures and examples now use the owning heap. Diagnostic host logs copy text
explicitly instead of retaining an unrooted raw Value. Redundant Value clones were
removed across consumers. Public RootedValue remains a non-Copy root lease.

The existing layout contract moved to the immutable-storage test owner so it also
reports backing costs. Both debug and release builds report Value=16 bytes,
HeapObjectId=12, HeapObject=128, ObjectSlot=168, TupleData=32, String control=24,
RangeValue=24, EphemeralValue descriptor=56, RootedValue=32, HostRootHandle=40,
HostPathViewHandle=104 and FrameHostBorrowToken=48. Tuple backing uses 16 bytes per
member; string bytes and path Arc storage are additional allocations. Heap units
are logical accounting, not these physical byte sizes. Copy is checked in both
configurations; the <=16-byte budget applies to release and permits future
correctness-only debug instrumentation.

Allocation diagnostics reuse the preserved VE00 source/allocator probe, with the
same M1 Max/macOS/Rust environment and ordinary release profile, default Cargo
parallelism and target directory. Dependencies were warm; the latest runtime
layout test build took 44.79 seconds and the following embedding library build
4.00 seconds. Measurement ran after builds/tests finished, in fresh processes,
with three execution warmups, one counted execution per entry, 5,000 iterations
and checksum 325,000. Compilation/loading are outside the allocation window.

| Workload | VE00 allocation/realloc calls | VE02 calls | VE00 requested bytes | VE02 requested bytes | VE00 -> VE02 GC objects / collections |
| --- | ---: | ---: | ---: | ---: | --- |
| Tuple construction/member read | 35,007 | 5,315 | 1,680,179 | 1,175,451 | 0 / 0 -> 5,000 / 14 |
| One-element range traversal | 1,540,708 | 1,480,704 | 107,233,227 | 106,005,195 | 15,000 / 25 -> 20,000 / 29 |
| Repeated string constant | 480,007 | 471,433 | 50,155,179 | 49,845,351 | 0 / 0 -> 5,000 / 357 |
| String through script calls | 500,007 | 471,451 | 51,455,175 | 49,860,487 | 0 / 0 -> 5,000 / 358 |

These are allocation counts, not execution-speed claims. Tuple copying no longer
allocates member vectors, but constructing each tuple/range adds a GC object.
String constants still allocate on each load and now contribute to GC pressure;
VE03 must remove this repeated materialization before VE04 accepts performance.
The 168-byte general object slot remains a material physical storage cost to
include in VE04 measurements. The collector and general enum boxing remain out
of scope for this phase.

Reproduction: build `kagari-embed --release --lib`, compile the unchanged
`target/ve00/measure_source.rs` against the resulting release rlibs into a separate
`target/ve02/measure_source` binary (`target/ve02/build_probe.py` and
`probe-build.log` record the exact rustc command), then run it with
`target/ve00/tuple-range.kgr` and `target/ve00/strings.kgr`. Results and layout logs
are under `target/ve02/`; the baseline executables were not overwritten.

Focused validation (DEVELOPER_DIR=/Library/Developer/CommandLineTools):

- Runtime host scopes: 8 pass; substrate/storage boundaries: 5 pass.
- Runtime GC unit contracts: 22 pass. GC ownership: 6 pass, then the extended
  existing string/tuple root/reclamation case passes individually. Borrow/no-escape:
  5 pass; nominal host boundaries: 8 pass; native conversion: 10 pass.
- Typed path contracts: 20 pass after adjusting descriptor accounting and reusing
  preallocated inputs when testing quarantined runtime rejection. Collection during
  host write preparation retains the descriptor plus both arrays; dirty-ledger
  cleanup still reclaims the arrays.
- Value category/key contracts: 3 pass; layout/Copy contract: 1 pass in each debug
  and release. Async retention graph: 1 pass. String joining: 1 pass. Reflection:
  1 pass. Bounded trace snapshots: 2 pass.
- VM GC/native payload contracts: 15 pass. Collection/storage contracts: 12 pass.
  Exact tracing/accounting assertions now include tuple nodes and string edges;
  invalid mutation assertions still measure counters after input construction.
- Embedding host interfaces: 8 pass. String methods/Unicode/slicing: 5 pass.
  VM session/reentry contracts: 8 initially pass; the remaining reentry trap case
  passes after asserting the still-active outer string argument survives and is
  reclaimed after return. No root-cleanup assertion was removed.
- Affected runtime, stdlib, VM, embedding, CLI and Lua benchmark all-target Clippy
  passes with `-D warnings`. Formatting, structure (982 files, zero violations),
  imports/visibility/module ownership review and `git diff --check` pass. No full-workspace test suite or GitHub CI matrix ran.

The initial constructor/type errors and stale inline-value allocation assumptions
are resolved. The implementation review also caught and corrected runtime-only
Ephemeral accepting HostHandle after compaction; the existing category test now
checks that distinction. No production semantic fallback or disabled validation
was used. VE03 owns indexed constant bindings; VE04 still owns the combined
reference/string performance and lifecycle gate.

### VE03 completed: indexed constants and scoped string inputs

Canonical `LoadConst` now carries a checked `ConstantId`; lowering deduplicates
portable constants by exact representation (including float bits). Verification
rejects out-of-range indices and duplicate pool entries. Prepared scalar execution
still embeds its scalar bits; runtime string bindings live only in the owning
module record and are traced with that version. Shared verified code remains
runtime-independent. The first string load materializes the bytes; subsequent
loads copy the cached Value. Escaped strings retain their own heap storage without
retaining obsolete module metadata.

Standard string methods now read scoped heap views. Owned host String conversion
remains explicit, and owned results use the existing typed conversion/rooting and
resource-limit path after dropping input views. Dynamic interning is deferred.
Production and migrated test targets compile together. The bytecode representation
changes directly, without a compatibility reader or unpublished ABI/schema bump;
VE04 refreshes affected disposable source-free products at its integration gate.

Focused validation (DEVELOPER_DIR=/Library/Developer/CommandLineTools):

- Extended runtime shared-code/isolation contract passes: repeated cached reads,
  foreign runtime and invalid-index rejection without allocation, version-rooted
  constants, old-version reclamation, independently rooted escaped string survival
  and eventual reclamation, and unaffected second-runtime state.
- Compiler constant contracts: 6 pass, including NaN payload/signed-zero identity,
  deduplication, semantic integer ranges and source lowering. Existing malformed
  bytecode contract passes with out-of-range and duplicate-pool cases, including
  serialized loader rejection. Repeated string lowering shares one pool entry.
- Embedding string contracts: 5 pass through serialized artifacts and aggressive
  collection, including content equality, Unicode slices, split/list output,
  immutable aliases and traps. The initial cleanup assertion expected zero live
  objects while a published version still owned its constants. The helper now
  publishes an unused replacement version before asserting exact zero, preserving
  detection of leaked execution roots rather than ignoring retained objects.
- Native owned conversion: 10 pass. VM execution/frame/debugger/interface contracts:
  44 pass, including pinned generations and cleanup. Extended existing key contract
  proves independently allocated equal UTF-8 strings have equal keys and hashes.
- Affected runtime, compiler, stdlib, VM, embedding, CLI and Lua benchmark all-target
  Clippy passes with `-D warnings`. Formatting, structure (983 files, zero violations),
  imports/visibility/ownership review and `git diff --check` pass. No new unsafe
  storage was introduced. No full-workspace test or GitHub CI matrix ran.

The unchanged VE00 allocator probe, linked separately as
`target/ve03/measure_source`, gives the following after three warmups, for each
5,000-iteration execution (checksum 325000):

| Workload | VE00 allocation/reallocation calls | VE03 calls | VE00 requested bytes | VE03 bytes | VE03 heap allocations / GC collections |
| --- | ---: | ---: | ---: | ---: | ---: |
| string constants and length | 480007 | 7 | 50155179 | 163 | 0 / 0 |
| string function transfer and length | 500007 | 7 | 51455175 | 159 | 0 / 0 |

This removes both VE02's repeated constant materialization and owned standard
string input conversion. Value transfer and warmed constant access copy no text;
the remaining seven allocations belong to the complete execution boundary, not
the loop's string operations. These are allocation diagnostics, not speed or RSS
claims. Environment/profile/features match VE00; Cargo uses default parallelism
and target directory. Release library build took 27.22 s, separate from execution.
Reproduce with `cargo build --release -p kagari-embed`, the rustc command recorded
by `target/ve03/build_probe.py`/`probe-build.log`, and
`target/ve03/measure_source target/ve00/strings.kgr`. Raw validation/allocation logs
are under `target/ve03/`; preserved baseline binaries remain untouched. VE03
acceptance passes. VE04 owns the combined lifecycle and paired throughput gate.

### VE04 completed: integration and performance gate

VE03 production checkpoint is `511e40a2`. Focused lifecycle, asynchronous retention/
output, callback GC, cleanup, source-free and supported backend contracts pass.
Disposable artifacts under `target/fixtures` were regenerated once for the indexed
format. Architecture/runtime/bytecode descriptions now match compact checked IDs,
Copy transport, immutable tuples/strings and version-owned constant caches.

Initial focused integration exposed three exact-count assumptions: the GC key
fixture expected clearing containers to free their constant string, and reentry/
cancellation cleanup expected a published module to have no retained constants.
The cache correctly retains those strings. Updated existing contracts assert the
remaining constant, replace the published version without executing its new cache,
and then require exact-zero collection. No frame/root cleanup check is removed.
Reproduction: `cargo test -p kagari-vm --test native_boundary native_boundary_gc`
(key fixture: reclaimed 3, expected 4), and
`cargo test -p kagari-vm --lib tests::sessions` (two cleanup cases: live 1, expected 0).
All three cases pass their focused reruns. The standalone source-free native
consumer had the same exact-zero assumption (11 published strings). It now releases
that version before exact-zero collection and also passes its focused rerun.

Validation (CommandLineTools environment, normal workspace profiles/default target
and parallelism; complete feature/backend matrices remain CI-owned):

- Embedding generic reload: 2 pass; native artifacts: 2 pass; exclusive runtime
  transfer: 1 pass. Existing asynchronous lifecycle/reload and output-publication
  contracts each pass, covering suspension, cancellation, fault and successful output.
- GC/native payload contracts: 14 pass initially plus the repaired key-root case.
  Session/reentry contracts: 7 pass initially plus the repaired trap/cancel cases.
  Stored native callbacks trace captures through callback-triggered collection.
- Warmed native scalar/bulk and script-window allocation contracts: 2 pass.
  Supported backend adapters/fallbacks: 10 pass. Real Cranelift preparation,
  retained entry ownership/reload and safepoint contracts: 3 pass.
- `scripts/check_features.py --async-only`: 13 production dependency boundaries
  and the standalone source-free async contract pass; forbidden frontend/compiler
  dependencies remain absent. Source-free native enum and forged-import contracts
  pass; repaired native binding cleanup passes independently. This is focused
  standalone validation, not a complete feature/backend matrix.
- Affected VM/embedding all-target Clippy with `-D warnings`, formatting, structure
  (983 files, zero violations), imports/ownership review and `git diff --check`
  pass. No full workspace suite or GitHub CI matrix ran. Logs: `target/ve04/`.

[Paired results](performance-baseline.md#compact-values-and-shared-constants-ve04-2026-10-10)
record all 2,728 checked baseline/candidate/Lua batches across the frozen original,
source-form and numeric sets. Same machine/toolchain/release features as VE00,
three warmups/eleven samples in two fresh processes per binary; build/test activity
was stopped for timing. Candidate strings take 29.7%/41.9% of baseline time;
arrays take 90.7%, calls 97.0%, recursion 96.2%, maps 95.9%. Branch/entry and numeric
routes show small 0.1–2.0% slowdowns; no scalar speedup or statistical-significance
claim is made. The unchanged closed scalar route has not received the later
dispatch/call optimizations. Lua host-callback comparisons include the common Rust
numeric function affected by compact Value on both adapters, as the report explains.

Value remains 16 bytes and Copy in both configurations; IDs are 12 bytes, with the
VE02 debug/release correctness evidence reused. Warmed string loops allocate zero
GC objects and make seven Rust allocation requests per full execution. Tuple/range
allocation probes reproduce VE02 counts exactly: tuples reduce Rust requests from
35007 to 5315 but add 5000 GC nodes/14 collections; ranges reduce 1540708 requests
to 1480704 while increasing GC nodes from 15000 to 20000 and collections 25 to 29.
Requested bytes fall in both cases. The added records are accounted for explicitly;
there is no per-copy payload duplication or per-step range boxing. No allocator,
collector or enum-unboxing scope was added. This accepts the representation tradeoff
with measured string/mixed-route benefits, not a size-only argument.

All carried VE04 failures are resolved. The value/reference milestone is locally
accepted, permitting VE05. Full-plan local integration, CI and Lua parity remain
open; current matched VM/Lua ratios are still well above 1.0. VE05–VE07 own the
already-authorized prepared collection/object/call and execution-loop work.
