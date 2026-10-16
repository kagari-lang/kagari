# Runtime storage and shared type descriptors execution plan

Status: execution started on 2026-10-16; RS01-RS04 implemented; RS05 deferred; RS06 integrated latency acceptance next.
The execution document and follow-up consistency review are complete.
The [roadmap](implementation-roadmap.md#runtime-storage-redesign)
owns priority; this plan owns RS01-RS06, decisions, acceptance and the progress
ledger. Required phases remain open until their measured acceptance gates pass.
All target structures below are conceptual designs, not existing APIs.

## Outcome, priority and finite scope

Reduce repeated per-object storage and interface/type metadata while preserving
one language value model, checked host boundaries and generation-pinned execution.
Separate universal value transport from storage whose type and owner are already
known. A smaller Rust enum alone is not the objective.

Run this storage work before [interface downcast](interface-downcast-plan.md),
[NC collection migration](native-collections-plan.md) and the proposed interpreter
call-lifecycle optimization. Those tracks remain separate. Downcast must consume
the resulting concrete type/provenance facts; it is not implemented here. NC keeps
ownership of library storage and algorithms; this work changes shared physical
heap mechanisms and updates their existing consumers without moving collections
between crates or changing their APIs. Revisit the call-lifecycle baseline after
storage changes rather than layering unrelated optimizations onto this migration.

Required scope:

- Share receiver-independent interface descriptions across compatible instances.
- Replace the largest-variant-sized heap slot with a compact identity directory
  and focused physical stores for the existing object categories.
- Replace ordinary script struct fields stored as Vec<Value> with checked,
  type-directed storage and shared physical layout descriptions.
- Transfer verified scalar fields directly to/from scalar frame slots, reducing
  intermediate Value conversions and repeated checks with explicit validity proofs.
- Integrate these representations with roots, coordinated metadata/heap GC,
  host access, generic layouts, suspended frames, reload and source-free products.
- Evaluate heap-local handle compression only after the owner boundary is proven;
  RS05 permits an evidence-backed deferral rather than forcing a weaker handle.

Preserve the existing 16-byte general Value transport, scalar/managed frame banks,
compact numeric sequence buffers, nominal typing, value/object semantics and
source-language APIs. Exclude a universal 8-byte Value or NaN boxing, numeric
range/bit-pattern restrictions, a new Any type, a new GC algorithm, a moving or
concurrent collector, state migration, whole-runtime type interning, a new public
embedding facade and general native-backend expansion.

Script enums, tuples, closures and native payloads must move off the old universal
heap slot where necessary, but do not acquire unrelated payload unboxing or a new
public ABI here. Native Rust state remains external. No new concrete collection
Value variants or family-specific storage policy may be added to generic loops.

## Baseline evidence and diagnosis

Planning baseline: `c6a62b86`; the working tree was clean before this document.
The preceding read-only investigation on that revision measured the following
shallow sizes, not transitive allocation, retained memory or workload RSS:

| Representation | Bytes | Interpretation |
| --- | ---: | --- |
| Value | 16 | Tag and inline scalar or checked identity; not a complete heap object. |
| HeapObjectId | 12 | Three u32 fields: owner, slot and generation. |
| HeapObject | 128 | Universal inline object enum; every slot reserves its maximum size. |
| ObjectSlot | 168 | Includes HeapObject storage and lifecycle bookkeeping. |
| StructLayoutRef | 48 | References immutable program/layout/scope facts; their allocations are additional. |
| Ty / NominalTy | 64 / 56 | Semantic type structures; nested Vec/Box/map storage is additional. |
| InterfaceValueSnapshot | 464 | Receiver plus types, environment and binding-list headers. |
| InterfaceMethodBinding | 328 | One binding; nested argument/signature allocations are additional. |

Environment: Apple M1 Max, aarch64-apple-darwin, rustc 1.98.1
(`48a229cea`, 2026-09-01), default features, workspace dev/test profiles
(`opt-level=1`, debug information), default target and build parallelism, existing
incremental target cache. Compilation elapsed time is not an execution measurement.
Release and other-platform layouts have not been measured by this investigation.

Reproduction for the existing probes:

```text
cargo test -p kagari-runtime --lib scalar_slot_layout_does_not_inline_host_descriptors -- --nocapture
cargo run -p kagari-contract --example definition_metadata
```

The first command passed one selected test; the second printed Ty/NominalTy/binder
sizes. For the remaining shallow sizes, load the runtime test executable reported
by Cargo into LLDB without launching it and use `image lookup -t` with
`kagari_runtime::module::StructLayoutRef`,
`kagari_runtime::gc::interfaces::InterfaceValueSnapshot` and
`kagari_runtime::gc::interfaces::InterfaceMethodBinding`.
These earlier checks are evidence for the baseline, not new implementation tests.

For Point { x: i32, y: i32 }, current directory storage plus field elements is
168 + 2 * 16 = 200 bytes, excluding spare capacity, allocator overhead, external
references and shared metadata. Merely halving Value reduces that subtotal to
184 bytes. A target payload containing two i32 fields needs eight data bytes,
but still needs identity, lifetime, layout and allocation storage. No eight-byte
total-object claim or predicted speedup follows from these arithmetic examples.

| Current owner | Architectural disposition |
| --- | --- |
| [Value](../crates/kagari-runtime/src/value.rs), [heap directory](../crates/kagari-runtime/src/gc.rs), [physical object enum](../crates/kagari-runtime/src/gc/storage.rs) | Retain checked transport/identity semantics; remove inline largest-variant storage from every directory entry. |
| [Execution values](../crates/kagari-runtime/src/frame/values.rs), [numeric sequences](../crates/kagari-runtime/src/native/sequence.rs) | Scalar registers already use u64 payloads and sequences already have compact numeric buffers. Reuse rather than replay these optimizations. |
| [Interface records](../crates/kagari-runtime/src/gc/interfaces.rs), [construction](../crates/kagari-runtime/src/objects.rs), [snapshot store](../crates/kagari-runtime/src/execution_metadata/interfaces.rs) | Split receiver ownership from repeated type, method and adapter descriptions. |
| [Metadata publication](../crates/kagari-runtime/src/execution_metadata/links.rs), [method identity](../crates/kagari-runtime/src/execution_metadata/application_key.rs) | Preserve validated lazy edges and exact operation provenance. Parent snapshot caches currently retain a receiver and cannot be shared as descriptor data. |
| [Type arguments](../crates/kagari-runtime/src/frame/types/arguments.rs), [layout preparation](../crates/kagari-runtime/src/module/layouts.rs), [layout admission](../crates/kagari-runtime/src/module/layout_admission.rs) | Extend/reuse prepared facts and scopes. Avoid another nominal identity or compatibility system. |
| [Contract layouts](../crates/kagari-contract/src/layout.rs), [prepared fields](../crates/kagari-runtime/src/module/execution/fields.rs), [field execution](../crates/kagari-runtime/src/frame/fields.rs) | Derive physical storage from checked semantic layouts; keep access policy and trap order above raw storage codecs. |
| [Collection driver](../crates/kagari-runtime/src/gc/collection.rs), [metadata traversal](../crates/kagari-runtime/src/execution_metadata.rs), [roots](../crates/kagari-runtime/src/gc/roots.rs) | Preserve one coordinated reachability decision, then detach and release. New stores are not new roots. |
| [Native objects](../crates/kagari-runtime/src/native/storage.rs), [managed payload fields](../crates/kagari-runtime/src/native/payload/managed/mod.rs) | Keep native tracing/drop and checked script-edge mutation; do not reinterpret Rust payloads as script struct bytes. |

The defects are repeated storage and misplaced per-instance descriptors. Owner,
generation, nominal compatibility, roots and pinned versions are necessary
semantics; removing them is not an optimization. The existing prepared layouts,
bounded indices and selected-call evidence remain useful foundations. RS01 must
identify any superseded caches/bridges and assign removal to the replacing phase.

## Target ownership and representation

### General transport and type-directed storage

Value remains the checked, self-describing transport for general native/SDK
arguments, inspection and genuinely erased values. It stays unrooted; keeping
one in Rust does not retain a script object. Preserve complete i64/u64 ranges,
floating-point bit behavior, tags and existing ownership checks.

Known-type fields use a shared storage description rather than a tag per scalar:

```text
Point storage description: size/alignment, x at 0:i32, y at 4:i32, no GC edges
Point instance payload:   [x:4 bytes][y:4 bytes]

Node storage description: scalar offsets, managed-reference offsets, type facts
Node instance payload:   scalar data + checked references at declared locations
```

One physical storage owner implements checked read/write, encoding/decoding,
initialization and edge enumeration. Field operations retain visibility,
writeability, bounds, once-only evaluation and failure precedence. Interpreter,
reflection, host adapters and supported native paths consume this same contract;
they do not each cast arbitrary bytes or duplicate the field semantics.

Prepare size, alignment, offsets, scalar representations and precise reference
maps from verified closed layouts. Reuse existing declaration identities, generic
applications and lexical provenance; physical offsets are not new semantic IDs.
Generic field applications must be prepared before use, not inferred from stored
values. Type facts and field names stay shared. Ordinary object payloads do not
each own a fresh Ty tree, field-name table or full StructLayoutRef when a checked
reference to its shared description suffices.

The first field migration covers ordinary script structs, including scalar,
object/interface reference and applied generic fields. Stored enum/tuple/function
values can keep their current managed representation; do not expand this into
unboxing every language value. Native/script-host projections keep their existing
adapter contract. Existing physical plans and codecs are replaced at their owner,
not kept as a permanent Vec<Value> fallback for migrated struct shapes.

### Direct field-to-scalar transfers and check ownership

The existing scalar loop already uses raw payloads and prepared numeric kernels.
The remaining field path in [cursor object operations](../crates/kagari-runtime/src/frame/cursor/objects.rs)
and [field actions](../crates/kagari-runtime/src/frame/fields.rs) reads/writes Value
and passes through generic bank admission. Compact fields alone do not remove
those conversions. RS04 must connect prepared scalar field access to the existing
scalar bank through the common storage codec, without an intermediate Value for
eligible ordinary script fields. General host/reflection entrypoints adapt Value
at their boundary using the same codec and validation owners.

For a loop containing p.x += dx, the target flow is:

```text
Preparation: bind the applied layout, x's offset/representation and scalar slots
Execution:   admit receiver -> load i32 payload into scalar slot
             -> existing checked arithmetic -> store scalar payload into x
```

This is a representation change, not instruction fusion: preserve logical PCs,
once-only evaluation, overflow/trap order and the original write commit point.
Narrow integers, signed extension, f32/f64 bits and bool domains use the shared
encoding rules. Managed fields still use traced references; host projections keep
scoped borrow/access validation and do not become raw script-memory accesses.

RS01 inventories repeated checks by producer, consumer, validity interval and
invalidation boundary. RS04 removes or relocates a check only when an enforced
invariant supplies equivalent authority:

| Check or conversion | Intended owner and retained guarantee |
| --- | --- |
| Field type, access direction, physical representation and slot mapping | Checked executable/layout preparation binds these facts once per valid application; source-free validation remains independent. |
| Repeated scalar Value construction/tag dispatch and generic heap-value admission | Direct payload transfer uses verified scalar domains and initialized bounded bank/storage access; scalar data creates no managed edge. |
| Receiver layout compatibility and field offset | Bind to the admitted actual layout through the existing compatibility owner. A declared type alone cannot authorize an offset in an arbitrary compatible/versioned object. |
| Heap owner/generation/kind, candidate access, host borrowing and dynamic bounds | Keep checks at access or reuse authority only within a proven scope. Static typing alone does not establish these changing facts. |
| Overflow, division errors, cancellation, GC and observation boundaries | Preserve existing numeric kernels and execution boundaries; storage optimization supplies no exemption. |

Prepared plans retain descriptor identities and offsets, not persistent raw payload
pointers. Any borrowed access proof ends before a transition that can invalidate
it, including allocation/GC, callbacks/reentry, suspension or storage relocation.
Readonly aliases do not freeze field contents. Do not hoist field values or mutable
state checks out of a loop on that assumption. No new general unchecked API,
duplicate scalar semantics or speculative cache belongs to this work.

### Compact directory and focused stores

Use a small stable identity directory recording allocation generation, kind and
payload location, plus the lifecycle state required for that object. Store
payloads in focused arenas/slabs or size/layout-organized regions. RS01 selects
the concrete allocator/locator scheme using measured density and access costs.
Do not replace the large inline enum with one Box of that same maximum-sized enum
per object: that relocates padding and adds allocation rather than solving it.

Keep one logical heap identity namespace. Payload locations may change internally
without renumbering live objects, but this plan does not introduce moving-GC
semantics. Object kind/locator consistency and live generation are checked before
access. Public/native borrowed views must not survive a storage mutation that
could invalidate their backing allocation; retain existing borrow/reentry guards.

Revision counters, candidate-initialization ownership and moved/taken native
states are audited separately. They can use focused records or side tables only
with complete access validation and a lifetime keyed by allocation identity.
Reusing a directory slot must reset/retire every associated record; no stale
side-table entry may authorize a new object. Preserve candidate isolation,
collection cursor revision checks and existing errors.

Track live bytes and reserved capacity in both directories and payload stores.
Sparse occupancy must not retain hidden unbounded slabs or one large allocation
per dead layout. Generation exhaustion still retires the identity; reclaiming
payload capacity must not reset a retired generation or resurrect stale handles.

### Shared interface descriptions

```text
Interface value:      receiver Value + checked descriptor reference
Shared description:   concrete/applied interface facts + method/parent adapters
                      + generic operation evidence + supplying versions
```

Descriptor equality includes the complete applied type identities, associated
outputs, selected impl/table, adapters, operation/construction evidence and exact
provenance that can affect execution. It excludes the receiver object. Do not
merge by names, method count, erased signatures or coincidentally equal local IDs.
Equally spelled types from incompatible versions remain different descriptions.

Classify every existing snapshot field before migration. In particular, a cached
parent InterfaceSnapshotId includes a receiver and must stay receiver-local or
be replaced by a shared parent-description edge plus receiver-specific view.
Do not accidentally return another object's parent view. Lazy operation caches
may move only after proving they contain receiver-independent evidence.

Avoid defeating sharing with a freshly allocated environment identity for every
wrapper. Reuse existing prepared environments when their complete provenance and
witnesses match; genuinely distinct environments remain distinct. This is not
whole-runtime structural interning. Shared method records should borrow or retain
immutable checked signatures rather than clone parameter trees per receiver.

Define an owner-local live descriptor registry and its canonical lookup together.
Entries represent live applied contracts and participate in coordinated tracing;
the lookup does not become an immortal root. Existing bounded preparation indices
may accelerate access, but eviction cannot invalidate a referenced description or
silently recreate a full method table for every still-live identical wrapper.
Account for keys, registry slots and environments as well as the smaller wrapper.
No receiver or captured payload is retained solely to keep a description cached.

### Conditional heap-local handle compression

Retain the current external owner/slot/generation identity. RS05 may encode
heap-internal references as u32 slot + u32 generation, with owner supplied by the
proven heap context. This is a private storage encoding, not a new public handle
or another nominal identity system. No shortening/truncation of slot or generation
is permitted; preserve admission limits and generation-exhaustion retirement.

Import validates owner, generation, kind and admissibility before dropping the
redundant owner word. Export reconstructs a checked external identity from the
actual owning store. A safe API must not allow a bare local token from one heap
to be interpreted in another heap that happens to have the same slot/generation.
Codec access needs a bound storage/session capability; local tokens never enter
portable artifacts, detached native captures or arbitrary host-owned containers.

Trace all crossings: roots, active/suspended frames, native callbacks and reentry,
fields, interface payloads, aggregate snapshots, debugger views, failure paths and
runtime transfer between threads. Start with owned object storage; do not silently
extend compression to the frame bank or public APIs. If this boundary cannot be
proved and measured beneficial within the finite scope, retain full handles and
record the evidence-backed deferral in RS05. Required RS02-RS04 work still stands.

## Safety, executable contracts and backend boundaries

- Physical storage does not weaken nominal/access/schema/ABI or handle validation.
  Contract owns checked semantic inputs; runtime preparation owns local physical
  offsets and host-layout facts. Source-free loading independently validates all
  new persisted evidence. Never serialize Rust addresses, local IDs or unchecked
  platform offsets as portable authority.
- Allocation validates sizes/alignment and checked arithmetic before publication.
  Partial objects expose only initialized edges to GC. Define initialization and
  rollback states explicitly; no scan reads uninitialized bytes, stale padding or
  an invalid Rust bool/enum/reference representation. Encodings use valid bit/byte
  storage and checked decoding. Unsafe blocks require a local invariant and
  focused validation, not blanket Send/Sync declarations.
- Writes validate incoming values and retain necessary roots before any allocating
  preparation. Publish reference fields and tracing state coherently. Preserve
  existing mutation commit points, trap order, completed effects and candidate
  restrictions. Do not introduce transactional semantics or remove checks because
  a type descriptor was previously validated.
- Keep the existing collector/safepoints, root providers, cancellation and
  coordinated heap/program/environment reachability. Freeze live sets before
  detach; native tracing/drop remains restricted as today. Shared descriptors
  and side tables cannot create hidden roots or unload a version still in use.
- Old objects retain usable layout/operation descriptions across reload. Reusing a
  new physical descriptor cannot retarget pinned behavior. Closure/interface/native
  cycles through old module state must remain collectible. Compatible layout
  admission stays with its existing owner, not a new size-equality check.
- VM and any currently supported native path use the same storage contracts.
  Unsupported native bodies retain pre-entry interpreter selection. Do not expand
  J01-J06 or count a JIT-enabled fallback test as actual native execution.

### Bounded unsafe implementation policy

This plan does not require an all-safe implementation. Prefer safe representations
and APIs; permit small private unsafe kernels inside the RS03 storage owner and
RS04 field codec/transfer boundary when the selected layout requires raw memory,
or when a measured hot path retains avoidable checks after safe restructuring.
This does not authorize a separate rewrite of the scalar interpreter or a general
unchecked access API. Existing host/JIT unsafe boundaries keep their own contracts.

For each new unsafe operation, document allocation provenance, bounds/alignment,
initialized bytes and valid bit patterns, aliasing, lifetime and invalidation
conditions. Enforce those conditions through construction and access APIs, including
independent artifact validation; debug assertions alone are not a safety proof.
Unchecked indexing requires proof for the actual admitted storage window and every
reachable access. Removing an initialization check additionally requires definite
initialization evidence. Neither proof follows merely from static scalar typing.
Keep these obligations across error cleanup, GC, callbacks/reentry and suspension;
no invalid raw access may replace an existing script error or quarantine result.

Performance-motivated unsafe changes require a matched safe baseline, generated-code
or profile evidence of the remaining cost, and repeatable improvement on the frozen
affected workloads. If safe code produces equivalent code/performance, keep it.
For raw storage required by the chosen representation, record that necessity and
measure the whole representation change without attributing its gains to unsafe
alone. Keep one production implementation after comparison. Use focused Miri and
boundary tests for new unsafe kernels; record unsupported checks and their coverage
gaps explicitly. Passing tests supplements, but does not replace, invariant review.

## Execution phases and migration policy

Execute RS01-RS06 in order. Keep coherent checkpoints buildable and attempt
focused tests at changed boundaries. Use temporary adapters only with an exact
consumer/removal checkpoint recorded here; remove the obsolete storage path in
its owning phase. No production stubs, duplicated semantic implementations,
compatibility aliases or routine unpublished format/helper-ABI bumps. Invalidate
affected development products and batch fixture regeneration at coherent points.
Implementation commits use `Plan-Phase: RSxx`.

### RS01 — Freeze ownership, measurement and physical contracts

Inventory the owners above and classify current per-instance/shared data. Capture
the baseline workloads below and set their count, retention pattern and acceptance
thresholds before changing implementation. Record locator/slab design, field
encoding/alignment/initialization, descriptor identity/lifetime and shared metadata
placement. Map all heap kinds and stored field categories to an owner, including
rare host/future/task/error states. Define how source-free validation, supported
native consumers and GC tracing will consume the replacement.
Inventory field/bank conversions and repeated checks with their proof lifetimes;
freeze field-loop throughput gates separately from pure-arithmetic regression gates.

Acceptance: no unresolved required storage/identity/initialization contract;
reproducible baseline and numerical memory/latency gates recorded here. Separate
RS05's conditional owner-elision decision from required storage work. Set finite
follow-ups instead of widening the task when the inventory exposes unrelated debt.

### RS02 — Share interface and applied type descriptions

Split receiver records from immutable applied descriptions; migrate construction,
upcasts, method selection, lazy edges, tracing and retirement. Reuse existing
type arguments, scopes and witness machinery, merging superseded overlapping
preparation at its owner. Retain raw/closed expression provenance only where the
shared description needs it, not in every receiver record.

Acceptance: simultaneously live identical applications share method/type data;
different receivers always dispatch on their own payloads, including cached parent
views. Width/population probes show descriptor storage scaling with distinct live
applications rather than receiver count. Generic witnesses, host liveness and
pinned generations pass selected boundary cases; old cycles reclaim correctly.

### RS03 — Replace uniform object slots with focused storage

Implement the compact stable directory and chosen payload stores, migrate all
existing heap kinds and their roots/borrows/trace/drop paths, and remove the
largest-variant directory representation. Preserve current payload semantics;
script fields can remain Value-based until RS04. Do not mix in a GC algorithm or
collection-library rewrite. Audit initialization-owner, revision and moved-state
side records under reuse, reentry and candidate sessions.

Acceptance: measured total directory plus payload capacity falls for the frozen
small-object workload; savings are not merely moved to Box allocations. Sparse
allocation/drop/reuse tests meet the frozen retention gate. Foreign/stale/wrong-kind
handles, exhaustion retirement, candidate isolation and native take/drop preserve
their existing failures and cleanup behavior.

### RS04 — Store script struct fields by verified type

Prepare shared physical layouts and migrate struct allocation, read/write,
reflection, host projection and supported execution consumers to the common field
codec. Preserve complete numeric bits, managed references, generic applications
and initialization safety. Remove Vec<Value> as the backing store for migrated
structs. Retain unrelated enum/tuple payload representations explicitly.
Implement direct scalar field loads/stores into/from existing scalar slots using
prepared access facts and the common codec. Remove intermediate Value construction
and redundant generic checks on this eligible path; document each replacement
proof and its invalidation boundary. Keep generic external adapters at their owner.

Acceptance: the Point field payload is eight bytes, with total object/descriptor
cost reported separately. Mixed scalar/reference and generic structs preserve
identity, declared access and precise tracing through collection/reentry/reload.
Source-free construction/access rejects malformed layout/type evidence. Existing
scalar banks and numeric buffers retain their prior behavior without duplication.
Focused cases cover scalar bit/domain preservation, alias-visible writes, overflow
before commit, stale/foreign receivers and reentry/reload across prepared access.
Diagnostic/code-path evidence confirms eligible warmed transfers avoid intermediate
Value conversion; matched field-loop timing meets RS01's frozen throughput gate.
Smaller payload size alone does not satisfy this execution acceptance criterion.

### RS05 — Decide and, if justified, implement local reference encoding

Audit every owner boundary and compare full versus heap-local handles using the
same reference-heavy workload. Freeze private APIs that cannot detach local
tokens from their owner capability. If justified, migrate only the agreed internal
storage sites and remove their temporary conversions. Otherwise document the
specific safety, complexity or measured-cost reason for deferral.

Acceptance: either implemented with cross-runtime collision, stale reuse,
generation exhaustion, reconstruction and cleanup evidence, or explicitly deferred
with full handles retained. Do not report compression as delivered in the latter
case. Universal 8-byte Value remains outside this task in either outcome.

### RS06 — Validate integrated storage and publish lasting contracts

Run selected composition checks and matched final measurements. Review changed
responsibilities, remove migration adapters/obsolete caches and update architecture,
runtime/host contracts and lasting performance findings. Refresh DC's inventory
against the resulting storage owners before downcast implementation; update NC's
shared-storage inventory without starting NC. Move remaining gaps and CI acceptance
to roadmap/review, then remove this completed plan. Git history owns the ledger.

Acceptance: required phases meet the frozen gates, RS05 disposition is explicit,
all selected local checks pass and no carried integration error remains. Report
CI-only scope separately. Do not claim Lua parity, Java parity or universal speedup
from storage microbenchmarks; existing throughput goals remain independent.

## Measurement and validation gates

RS01 freezes a small set of workloads, reusing existing fixtures where practical:

| Workload | What must be distinguished |
| --- | --- |
| Many live two-scalar structs, then sparse release/reuse | Field bytes versus directory/slab capacity, allocation count and GC cost. |
| Mixed scalar/reference structs and a cyclic object graph | Reference density, typed tracing, construction/mutation cost and full release. |
| Repeated scalar field reads, writes and read-modify-write on one retained object | Direct transfer/check overhead with a small working set; preparation and allocation excluded from the warmed execution interval. |
| Traversal of many scalar-field objects, including a fixed applied-generic layout | Field transfer plus cache/storage density; report separately from the single-object case and from cold generic preparation. |
| Many independently constructed interfaces with fixed types and narrow/wide traits | Receiver count versus descriptor/method count; copying one wrapper is a separate control. |
| Applied generic interfaces/layouts with differing witnesses and retained versions | Legitimate distinct descriptions versus duplicates; cold/warm preparation and old-version retirement. |
| Existing scalar-bank and numeric-buffer regression workloads | No accidental reboxing, slower conversion loop or loss of compact storage. |

For field loops, use observable checksums/final fields and identical arithmetic,
iteration counts, alias behavior and logical execution settings across revisions.
Include a pure scalar loop with the same arithmetic as a regression control; do
not attribute its existing payload-kernel behavior to RS04. Cover i32 and a
representative floating-point field in performance workloads; correctness tests
own the full affected representation/domain coverage. Keep host calls outside
the warmed loop unless measuring the separate generic boundary cost.

Compare the pre-redesign baseline, buildable RS03 checkpoint and RS04 checkpoint
on the frozen workloads to separate broad storage gains from field-path gains.
Use short diagnostic runs or code/profile inspection to establish which Value
conversions and checks remain, distinguishing preparation from each field access.
Do not add permanent production counters solely for this measurement or treat
fewer Rust constructors as proof of fewer machine instructions. Ordinary timing
runs exclude diagnostic instrumentation. Report execution time per iteration,
throughput, dispersion and allocation counts alongside memory; record cold
preparation cost separately. Inspect generated code/profiles if the direct path
does not improve timing, rather than adding another speculative fast path.

Measure compilation, preparation, execution, live retained bytes, reserved
capacity, allocation requests/bytes and GC separately. Use allocation-instrumented
runs separately from ordinary timing. Keep rooted data, workload results, capacities
and collection points equivalent; include descriptor keys/side tables/root overhead
in total memory. Record process RSS only as additional evidence, not as a substitute
for storage accounting. Include after-drop/GC and runtime-teardown observations.

Use the workspace release profile for performance comparisons, default target
and parallelism; record exact revisions, toolchain, machine/OS, features, cache
state, warmups, repetitions and noise. RS01 sets numerical memory improvements
and acceptable latency/allocation/GC regressions from the baseline before repairs
or tuning. If gates fail, diagnose ownership/representation again before more
local fast paths. Do not change workload counts or silently relax a target to pass.
Keep raw logs under ignored target/ and necessary methods/results in this plan.

Reuse [test owners](testing.md), especially
[GC ownership](../crates/kagari-runtime/src/gc/tests/gc_ownership.rs),
[generation/metadata collection](../crates/kagari-runtime/src/gc/tests.rs),
[frame bit/root contracts](../crates/kagari-runtime/src/frame/values/tests.rs),
[interface identity](../crates/kagari/src/runtime/tests/language/interface_identity.rs),
[host interfaces](../crates/kagari/src/runtime/tests/embedding/host_interfaces/mod.rs)
and [generic reload](../crates/kagari/src/runtime/tests/embedding/generic_reload.rs).
Select affected semantic cases and a small adjacent regression set, including
partial initialization, malformed artifacts, suspended roots and candidate-session
access where touched. Add cases only for distinct uncovered boundaries; preserve
source-free/native/ownership evidence and existing assertions.

Each implementation checkpoint records exact filtered commands and their selection
rationale, failures/skips and follow-up owner. Run repository-wide lightweight
checks and workspace Clippy as required by [AGENTS.md](../AGENTS.md):

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Inspect imports/re-exports, effective LOC, unsafe invariants and module ownership.
Run focused Miri cases if new raw-memory operations require it. Full-workspace
tests and the exhaustive feature/backend/platform matrix remain CI-only, including
final acceptance. Do not reproduce a workspace sweep as many package commands.
Documentation-only planning uses content/link/diff checks, not builds or Clippy.

## Progress, decisions and review ledger

### RS01 execution record

Baseline runtime revision: `159c74ef`. The new
[storage probe](../benchmarks/lua-comparison/examples/runtime_storage/main.rs)
uses one fixed script, 512 retained instances (plus a one-instance memory control),
50,000 single-object/scalar iterations, three warmups and eleven timing samples.
It uses the interpreter, explicit GC and no observer. Source compilation,
preparation and linking are outside execution timing. The numeric sequence control
retains 512 i32 elements. Sparse release keeps every sixteenth Point, then drops
all roots and repeats the original population to exercise identity/capacity reuse.

Commands (run sequentially so measurement does not compete with builds):

```text
cargo run -p kagari-lua-benchmark --example runtime_storage --release
cargo run -p kagari-lua-benchmark --example runtime_storage --release --features diagnostics
cargo test -p kagari-runtime --release --features execution-diagnostics --lib diagnostics::memory:: -- --ignored --nocapture
```

The last command reuses existing native, applied-layout/cross-version admission,
method-application and witness-scope retention probes at 1, 4 and 160 applications.
It is selected allocation evidence, not a runtime test-suite sweep. Each runtime
lifecycle must balance requested allocation bytes at teardown. First-use immutable
program caches are reported separately; the measured runtime lifecycle follows an
identical warmup so shared preparation is not mistaken for a runtime leak.

Environment: Apple M1 Max, macOS 26.6.2 (25G83), arm64, rustc 1.98.1
(`48a229cea`, 2026-09-01), workspace release profile and default build parallelism.
The first build populated missing release dependencies; timings exclude compilation.
The ordinary probe uses the benchmark package's default features; only the separate
allocation probe enables diagnostics. No allocator overhead or RSS claim follows
from requested-byte accounting.

Initial ordinary-run medians (ns; repeat before final comparisons if noise warrants):

| Workload | Execution | Construction | Live GC | Release GC |
| --- | ---: | ---: | ---: | ---: |
| Scalar, 50,000 iterations | 1,563,625 | — | — | — |
| Field read / write, 50,000 iterations | 4,459,500 / 3,700,375 | — | — | — |
| i32 / f64 field RMW, 50,000 iterations | 9,433,750 / 9,397,792 | — | — | — |
| Point traversal, 512 instances | 219,084 | 509,042 | 103,667 | 20,833 |
| Pair<i32> traversal, 512 instances | 217,750 | — | — | — |
| Cyclic nodes, 512 instances | — | 1,515,375 | 215,208 | 69,208 |
| Narrow interfaces, 512 instances | — | 1,817,875 | 452,542 | 76,083 |
| Wide interfaces, 512 instances | — | 2,679,125 | 451,375 | 106,750 |
| Generic interfaces, 512 instances | — | 3,453,458 | 552,708 | 156,334 |
| Numeric buffer, 512 elements | 242,041 | 311,708 | 5,584 | 3,584 |

Requested retained bytes above the loaded, collected Runtime setup baseline:

| 512-instance workload | Live | After all roots are dropped and GC runs |
| --- | ---: | ---: |
| Point | 220,896 | 187,632 |
| Pair<i32> | 221,672 | 187,672 |
| Narrow interfaces | 834,960 | 613,056 |
| Wide interfaces (four methods) | 1,437,072 | 613,056 |
| Generic interfaces | 1,314,192 | 666,304 |
| One interface copied 512 times | 18,776 | 9,472 |
| Cyclic nodes | 581,360 | 367,456 |
| Numeric buffer | 10,256 | 7,520 |

These totals include retained directory capacity, wrapper/field allocations,
runtime preparation, roots and backing-container capacity. After-release retention
is not necessarily a leak: the baseline keeps directory/free-list capacities.
All sixteen warmed runtime teardown samples balanced to zero requested bytes.
Each warmed 50,000-iteration read/write/RMW call requests 98 allocations / 6,928
bytes including host entry/exit, with zero net retained bytes.

Frozen gates, before storage implementation:

- RS02: narrow, wide and generic interface live increments fall by at least 40%
  against the table above. Method/type descriptions scale with distinct applications;
  increasing interface width must not add a method table per receiver.
- RS03/RS04: Point and Pair live increments fall by at least 50%; empty retained
  increments fall by at least 50%. Cyclic-node live storage must not grow. Sparse
  release/reuse remains bounded, and every runtime teardown balances to zero.
- RS04: each single-object field read/write/RMW median improves at least 10%
  against baseline; Point/generic traversal does not regress more than 5%.
- Scalar and numeric-buffer execution medians may not regress more than 5%.
  Construction and GC medians may not regress more than 10% for the fixed workloads;
  numeric-buffer retained bytes may not grow more than 5%. Allocation requests may
  not grow for warmed field-loop execution. RS03 is measured again before RS04.
- Use repeat process runs when ranges overlap a gate or machine noise is visible;
  a noisy sample is inconclusive, not permission to relax thresholds. Architecture
  diagnosis owns a missed gate before further optimization. Exhaustive CI remains
  separate from these local measurements and focused semantic tests.

Selected representation contracts for implementation:

- RS02 uses an owner-local weak canonical registry of shared descriptions. Keys
  contain the exact implementation ModuleKey/table/view, complete TypeArgument
  identities and construction-program provenance used by preparation. Shared
  environments are created once per description; receiver parent-view caches stay
  in snapshots. The registry supplies no executable root. Snapshot traversal visits
  each shared description once, including its lazy operation/environment edges;
  detached snapshots release their descriptions and dead registry entries are pruned.
  The preparation audit confirms that it copies supplied type bindings and only
  the caller's construction scope: it does not inherit caller operation bindings or
  parent environments. Complete TypeArgument identities distinguish normalized
  nominal provenance; construction program order is retained. Method operations
  are subsequently derived from this shared table/environment. This is interface
  description equivalence, not general environment interning.
- RS03 uses one checked directory and dense typed stores per existing heap kind.
  A payload entry retains its directory index; swap removal updates the moved
  entry's locator while preserving external identity/generation. Revision remains
  in the directory to avoid adding a lookup to native mutation; rare candidate
  initialization ownership moves to an identity-keyed side table. All stores share
  the existing heap borrow exclusion. Capacity shrinks with hysteresis after sweep;
  identity generations are never reset by payload compaction. The wide owned enum
  may serve transient allocation/detachment, never persistent directory storage.
- RS04 uses shared closed physical descriptions and a single byte buffer per
  struct. Scalar fields use exact semantic widths (Unit consumes no payload),
  native-endian byte codecs and shared scalar domain rules. Managed fields initially
  encode a representation discriminator and the full owner/slot/generation identity;
  the descriptor enumerates only their reference offsets. This avoids reinterpreting
  Rust values or requiring unsafe aligned loads. GC decodes copied identities through
  the same codec; native Rust payload tracing keeps its existing borrowed-value API.
  RS05 separately assesses owner elision. Physical descriptions retain immutable
  layout provenance without turning plain objects into executable-version roots.
- Construction validates all values and prepares storage before publication; failed
  construction drops unpublished buffers. No partially initialized object is exposed
  to GC. Field writes validate before mutating bytes. Prepared scalar operations
  bind actual-layout-compatible offsets and bounded initialized bank access, with
  proofs ending at existing region exits. Existing semantic/host adapters share
  the codec; no second unchecked object model is introduced.

Heap-kind coverage: String, Tuple, Range, HostRoot, HostPath, Ephemeral, Native,
Enum, Struct, Interface, Closure and Cell each receive a typed store. Array/Map/Set,
future/task state and custom native payloads remain Native-owned; their existing
trace/drop policies are reused. Error traces remain with Enum payloads. No native
Rust allocation is recast as a script struct. Source-free checked layouts supply
physical descriptions; reflection and native projections use the same field codec.

Repeated-check inventory: checked bytecode preparation owns field direction,
semantic type and register bounds for the immutable program generation. Closed
layout preparation owns representation/offsets for that application. The actual
object's identity, candidate visibility and compatible layout remain checked per
access; any temporary borrowed proof ends before GC, reentry, suspension or moving
storage. Scalar domains are established by existing numeric/bank rules and the
field codec, so the direct path can omit generic Value tag and managed-edge checks.
No general arithmetic-loop check hoisting is authorized by these facts.

Existing descriptor probes passed after updating their stale DefinitionMetadata
and native builder APIs, and omitting an unused native declaration from non-native
fixtures (the runtime correctly rejected that uninstalled contract). All three
selected probes passed with teardown zero. Extra retained bytes at 160 applications
(repeated preparation / retired generation): native 1,720,072 / 0; struct 3,868,112
/ 0; enum 3,873,240 / 0; cross-version admission 26,396 / 0; method 2,743,268 /
180,224; shared application 194,312 / 26,624; witness scope 486,032 / 56,896.
Retired executable-store capacity is reported separately from live descriptors.
These are controls for provenance/retirement, not additional migration targets.

Probe repair: direct generic-parameter-to-interface conversion in
`fn erase<T: Read>(value: T) -> Read` produced KG_TYPE_RETURN_TYPE_MISMATCH. This is
the queued shared-construction feature, not RS scope. The workload now uses supported
`impl<T> Read for Pair<T>` instantiated as Pair<i32>. The corrected ordinary probe
and allocation lifecycle run passed; no production behavior was changed for it.

### RS02 execution record

Receiver records now hold data, a shared Arc description and receiver-local parent
view caches. Complete implementation/table/view/type/construction keys use a weak
runtime registry. GC traces each description once and prunes expired registry
entries after detached snapshots are disposed. Method identity and lazy executable
edges use OnceLock, preserving Runtime's Send contract without unsafe code.
Construction remains checked on every receiver; sharing does not waive mutable
host liveness, heap ownership or generation validation. Interface preparation now
lives in objects/interfaces.rs; there is one construction implementation.

For 512 distinct receivers, requested live increments are 418,404 bytes (narrow),
419,628 (four-method wide), and 419,780 (generic): reductions of 49.9%, 70.8% and
68.1% from RS01. Width adds 1,224 bytes for the shared description, rather than
602,112 bytes across all receivers. Generic environments fall from 512 to one.
All sixteen teardown samples balance to zero; non-interface storage increments
and warmed field-loop allocation counts are unchanged.

Focused validation covers 13 interface graph/publication tests, five applied-fact/
witness-retirement tests and three interpreter dispatch/allocation contracts.
New coverage establishes that sharing never shares receiver/parent-view identity,
and that old/new loaded generations retain separate descriptions. Existing host
receiver preparation, witness providers and cached native dispatch remain covered.
All three descriptor retention probes also pass, with unchanged reported retained
application controls and zero teardown. Structure, formatting, workspace all-target
Clippy, diagnostics Clippy and diff checks pass. No build error remains; exhaustive
workspace/backend tests remain CI-only.

Uncontended timing repeats confirm interface construction medians of 1,113,500 /
1,113,083 / 2,394,083 ns (narrow/wide/generic), and
live GC around 304,000 ns. These meet the construction/GC regression gates.
Numeric-buffer execution is 258,666 ns initially and 257,833 ns on repeat, versus
242,041 ns baseline (6.5-6.9% slower); the 5% overall gate is **not passed**.
Architecture review finds no changed numeric-buffer, scalar-bank or native-sequence
access path in RS02; the only changed graph work is interface descriptions and a
weak-registry prune at collection. The loop does not collect. Code-generation or
machine-state effects remain unproven, so this is not dismissed as noise. RS03 owns
the next matched storage/control measurement; RS06 must resolve the regression
against the original gate before integrated acceptance. No local arithmetic-loop
optimization or threshold relaxation is authorized by this result.

### RS03 execution record

The directory is now 24 bytes per slot on the recorded arm64 release target,
down from 168. All twelve existing heap kinds use dense typed stores. Transient
HeapObject owned/shared/exclusive forms preserve one tracing/accounting policy;
no maximum-sized enum or separately boxed maximum-sized payload remains in slots.
Single-object extraction repairs the swapped payload's locator; collection detaches
typed batches and compacts survivors. Revisions remain in the stable
directory; candidate owners use a full-identity-keyed side table. Store shrinking
uses a four-times-live/64-entry hysteresis; tombstone generations remain intact.
Allocation reserves all required storage before publishing a payload/identity.

Initial requested-byte increments (512 instances; live / empty / repeated population):
Point 114,944 / 45,840 / 123,552; Pair 115,720 / 40,760 / 123,912; narrow interface
185,476 / 101,964 / 205,956; wide interface 186,700 / 101,964 / 207,180; generic
interface 186,852 / 102,380 / 207,364; cyclic nodes 466,672 / 72,544 / 483,056;
numeric buffer 10,224 / 7,488 / 10,256. Point sparse retention is 45,184 bytes.
The 50% combined RS03/RS04 live gate awaits field storage; Point is already 48.0%
lower, and empty retention is 75.6% lower. Every warmed teardown balances to zero.
Warmed field loops retain the same 98 allocation requests per call.

Local checks so far: 14 existing GC ownership/storage cases, one new sparse
relocation/alias mutation case, candidate-slot validation, three native panic/drop
cases, two extraction/slot-reuse cases and the existing release layout probe pass.
The initial integration errors were old direct slot accesses and whole-native
replacement; they are resolved through the directory and concrete native store.
The first timing run exposed representation-boundary costs: native/struct reads
still built a general borrowed enum, and collection repacked every dead payload
into the wide transfer enum. Concrete access now goes directly from the checked
locator into its typed store. Collection detaches typed batches, takes a wholly
dead large buffer directly, and disposes each payload outside storage borrows with
panic containment. Reachability, generation retirement and root policy are unchanged.
Repeated focused GC, candidate, panic/drop and extraction checks pass after these
changes; requested retained bytes and warmed-loop allocations are unchanged.
Structure, formatting, workspace all-target Clippy and diff checks pass. No carried
build/test failure remains. Revised ordinary timing medians (ns) are:

| Workload | Execute | Construct | Live GC | Release GC |
| --- | ---: | ---: | ---: | ---: |
| Scalar | 1,637,791 | — | — | — |
| Field read / write | 4,971,500 / 3,855,167 | — | — | — |
| i32 / f64 RMW | 10,271,084 / 10,164,292 | — | — | — |
| Point | 229,792 | 541,667 | 110,041 | 24,250 |
| Pair | 233,791 | — | — | — |
| Cyclic nodes | — | 1,612,083 | 227,000 | 80,333 |
| Narrow interface | — | 1,145,667 | 308,291 | 44,042 |
| Wide interface | — | 1,141,833 | 310,584 | 44,167 |
| Generic interface | — | 2,434,042 | 313,916 | 44,958 |
| Numeric buffer | 271,667 | 323,875 | 6,000 | 3,917 |

This is the required pre-RS04 measurement, **not integrated performance acceptance**.
The first RS03 release-GC medians for Point/nodes were 37,500 / 107,792 ns; typed
batch disposal reduces them to 24,250 / 80,333 ns, but they remain 16.4% / 16.1%
above baseline, outside 10%. Numeric execution is still 12.2% above baseline;
Pair traversal is 7.4% above baseline. RS04 owns field/traversal improvements;
RS06 owns numeric and collection regression resolution, including repeated matched
runs where the small numeric GC samples show noise. Keep all original gates.
RS03 storage/lifetime acceptance is met; integrated latency acceptance remains open.

### RS04 integration record

Struct payloads now share a closed physical description keyed by registry, module
version, nominal layout, concrete arguments and nominal argument provenance.
Each instance owns one initialized byte buffer: exact-width scalar bits and a
13-byte tagged full owner/slot/generation encoding for managed fields. Safe byte
copies preserve unaligned mixed layouts and all float bit patterns. GC follows
only described managed offsets; weak description entries are pruned after disposal.
The general Value API adapts through the same field codec.

Prepared concrete and applied scalar field operations transfer raw bits directly
between that codec and existing initialized scalar banks. Receiver identity,
layout compatibility, field permission, scalar domain, slot bounds and execution
state checks remain enforced. Host references and reflection retain their existing
admission paths. No unsafe code has been added.

Initial focused runtime GC (15) and field-layout (4) cases passed, followed by the
mixed-width/domain/float-bit layout case (5 total). The allocation lifecycle probe
completed with zero retained bytes after all 16 warmed teardowns and unchanged
98 requests / 6,928 requested bytes per field-loop invocation. Timing and broader
selected boundary checks are pending. Initial VM field/reload cases failed during
loading because the shared test compiler included an uninstalled logging contract.
The fixture now declares logging explicitly for logging workloads; ordinary source
fixtures carry only installed standard declarations. Revalidation is in progress;
this is a fixture repair, not relaxed production loader validation.
The subsequent nominal-generation case also failed unchanged on afec4a3b (RS01)
and ce56ae7f (RS03): its expectation predated caller construction dependencies in
shared environments. The fixture now requires that dependency to remain live
with the closure, and requires both old and caller generations to retire after
closure/interface roots are released. Pure aggregate provenance remains covered
by the runtime layout retirement cases; production retention is unchanged.

RS04 focused acceptance: all five runtime field-layout tests pass, including all
scalar domains and f32/f64 NaN/signed-zero bits. VM checks pass for ordinary fields
(2), integer-domain/source-free and overflow-before-commit cases (2), shared generic
field provenance across reload (1), owned execution/reentry and iterator invalidation
(2), and explicit logging registration (1). Overflow preserves prior effects,
leaves the failed field unchanged, and skips later writes. The new overflow fixture
initially selected a non-exported function as a public entry; exporting that entry
resolved the fixture failure. Existing GC identity/compaction coverage remains
applicable. Structure, formatting, workspace all-target Clippy and whitespace pass.
No carried build or selected test failure remains; exhaustive CI is not run locally.

Requested-byte increments above each loaded setup (512 objects):

| Population | Live | Empty | Reused |
| --- | ---: | ---: | ---: |
| Point | 62,180 | 43,100 | 70,788 |
| Pair<i32> | 63,020 | 41,092 | 71,212 |
| Narrow interface | 132,712 | 102,296 | 153,192 |
| Wide interface | 133,936 | 102,296 | 154,416 |
| Generic interface | 134,152 | 102,712 | 154,664 |
| One copied interface | 18,608 | 8,728 | 18,672 |
| Cyclic nodes | 418,524 | 72,876 | 434,908 |
| Numeric buffer | 10,224 | 7,488 | 10,256 |

Point/Pair live increments fall 71.9% / 71.6% versus RS01; their empty increments
fall 77.0% / 78.1%. Each Point's field bytes are exactly eight; totals include
directory/store capacity and shared descriptions. Interface live storage falls
84.1% / 90.7% / 89.8%; numeric storage does not grow.

Two alternating release-process comparisons against a freshly rebuilt afec4a3b
baseline use the identical probe, flags and environment. Read/write medians improve
36.7-38.1% / 20.5-21.7%; i32/f64 RMW improves 46.2-46.6% / 43.9-44.7%; traversals
improve 27.7-29.0%. The original RS01 read/write/RMW thresholds also pass. Allocation
and required field latency acceptance are met. Integrated controls remain RS06 work:
scalar ratios vary +2.7% to +6.4%, node release +9.0% to +14.2%, and tiny numeric
release +1.1% to +12.4%. A third matched pair will distinguish drift from persistent
regression without changing the original gates. No additional safe/unsafe tuning
has been applied merely to manufacture a passing sample.

### RS06 architecture follow-up

Three matched process pairs confirm remaining medians outside the original 10%
gate: Point construction +10.6%, Point release +10.3%, node release +13.2%.
Reusing canonical layout identities removes repeated structural-key preparation
(including transient generic argument copies), but the first trial did not resolve
construction/retirement latency. A borrowed-view inlining hint showed no benefit
and was removed.

Diagnosis: RS04 retained a separately allocated byte buffer for every struct in
addition to its already staged constructor Values. This added an allocation/copy
boundary and per-object buffer disposal. Replace those boxes with one packed
struct-byte arena under the existing ObjectStorage borrow. Records own a shared
layout and private byte offset; stable directory handles remain unchanged.
Construction reserves record/byte capacity before writing admitted fields and
publishing an entry. Sweep keeps record order, packs surviving bytes with safe
copy_within, updates offsets and shrinks with the existing hysteresis; an empty
arena detaches its allocation for disposal outside the borrow. A live view cannot
survive allocation, sweep, reentry or suspension. General native payloads remain
separate typed entries. This is a storage-boundary correction within RS scope,
not a GC algorithm change. The per-object byte-buffer checkpoint is superseded.

Integration currently builds; an unused import from general extraction was
removed after restricting single-object extraction to its sole native consumer.
Focused GC (15), field-layout (5), mixed-layout relocation (1), field/domain/
overflow (2), nominal reload (1), native extraction (2) and native trace/drop panic
cases (3) pass. Mixed-size/zero-byte rows retain managed references, NaN bits and
alias-visible mutation through compaction, growth and directory reuse. Structure,
formatting and workspace Clippy pass before the final field-link cleanup.
The packed arena lowers Point/Pair live increments to 58,084 / 58,860 bytes and
removes 502 / 1,014 construction requests versus RS04; warmed teardown and loop
allocation contracts remain unchanged.

Further GC diagnosis found repeated per-store scans after directory retirement.
Retirement now supplies exact per-kind counts; extraction scans only partially
dead stores and moves an entirely dead buffer directly, including small stores.
With no dead payload and no outstanding native extraction identity, sweep skips
payload maintenance; marking, metadata cleanup and validation still run. Moving a
small dead buffer avoids the former temporary batch allocation/copy, rather than
adding a new allocation to each create/collect cycle.

Field handlers also reconstructed/cloned concrete layouts while function links
separately owned aggregate/scoped layouts. Concrete field sites now join that
existing linked-layout owner and borrow its descriptor for the closed region; scoped misses still
exit before preparation. The duplicate concrete-layout constructor and redundant
prepared fields are removed. Focused field/reload checks and final matched
latencies are being rerun; no gate has been relaxed. The first cleanup build found
two remaining concrete-site exclusion filters; both now include all field sites
in the shared linked-layout preparation. No check was disabled to resolve them.

The linked-layout checkpoint passes field/domain/overflow (4), pinned generic
reload (1), source-free nominal/reference rejection (2) and scoped Rust loan (2)
cases. Its final three process pairs pass all controls except numeric-buffer
release GC (4,667 ns versus 3,667 ns, +27.3%). The unchanged 10% gate remains open.
Other controls include scalar +1.9%, Point construction +6.7%, node release +8.1%;
field read/write/RMW gains are 24.9-47.3% on the matched rebuilt baseline.

The remaining tiny-collection diagnosis found capacity checks for all twelve
stores after retiring one object and destructor-loop calls for empty batches.
Capacity maintenance now belongs to the changed store: partial extraction shrinks
its survivors, complete extraction detaches its whole buffer, and native take
maintains its own store immediately. An empty sweep need not visit payload stores.
Empty disposal branches stay inline while nonempty destructor/panic loops remain
out of line. Full marking/identity validation and per-payload panic containment are
unchanged. Focused cleanup checks pass: GC (15), mixed-row relocation (1), native take (2),
trace/destructor panic cleanup (3), and interface dispatch/retirement allocation
composition (3). Structure, formatting, workspace all-target Clippy and diff
whitespace pass; 294 links across eight touched documents are valid. Final matched
measurements remain the only open local acceptance item.

### RS05 decision

Defer heap-local owner elision. The migrated struct codec is the only proposed
site: its fields are admitted to one heap before publication and currently retain
the complete checked identity. External Value, roots, native collection payloads,
host projections, captured Values and detached tracing edges continue to cross
owner boundaries and must not accept an ownerless token.

For the frozen 512-node reference-heavy workload, each Node contributes one
managed struct field (its edge vector). Removing its four-byte owner would save
2,048 requested bytes out of the measured 418,524-byte live increment, about 0.49%.
The vector's internal reference elements are native-owned and outside this finite
migration. Point/Pair already exceed the required 50% live/empty reduction with
full handles. This small bounded benefit does not justify another owner-capability
API and reconstruction invariant at every codec/trace boundary. Keep the 13-byte
managed encoding and full owner/slot/generation validation. No compression or
unsafe speedup is claimed; revisit only with a concrete reference-heavy workload
showing a material opportunity.

- [x] Planning: scope, priority, measured shallow-size evidence, target ownership
  and RS01-RS06 written on 2026-10-16.
- [x] Planning consistency review against accepted direction, code and adjacent plans.
- [x] RS01: baseline workloads, frozen gates and physical/ownership contracts.
- [x] RS02: shared interface descriptions; overall numeric control carried above.
- [x] RS03: compact directory and focused stores; integrated latency gates remain open.
- [x] RS04: type-directed struct fields and direct scalar-bank transfers.
- [x] RS05: owner elision deferred with measured 0.49% node-workload opportunity.
- [ ] RS06: integrated acceptance and documentation handoff.

Accepted: preserve universal Value and existing scalar banks; prioritize actual
per-object storage and shared descriptions. Conditional: local handle compression.
RS01 fixes allocator/locator details, physical encoding, descriptor equivalence,
workload counts and numerical gates above. RS05 remains a conditional decision.

Review conclusions: parent-interface snapshot caches contain receivers and cannot
be copied into shared descriptions; applied environments require complete witness
and version provenance; registry lookup must preserve sharing without becoming a
permanent GC root. Existing scalar banks and numeric buffers remain in place.
RS05 has an explicit deferral outcome, so required storage acceptance does not
depend on forcing every value or reference into eight bytes. Roadmap, documentation
index, DC and NC scheduling now agree on the predecessor and ownership boundaries.

Planning extension, 2026-10-16: direct field/scalar transfers and evidence-backed
check reuse are required RS04 scope. RS01 freezes their proof boundaries and
field-loop performance gates; single-object, traversal and scalar-control workloads
separate execution overhead from storage density and existing arithmetic behavior.

Planning clarification, 2026-10-16: bounded unsafe storage/transfer kernels are
permitted with enforced safety invariants and focused validation. Check-elision
optimizations additionally require measured benefit over a safe baseline; no new
phase or general scalar-loop optimization is added.

Planning validation: 212 local links/anchors across the five changed documents,
Markdown fences, phase/checklist consistency and plan whitespace passed;
git diff --check passed. No implementation edits or new implementation test
results are claimed. No intermediate build failure has been established.

RS01 validation: ordinary timing and diagnostics lifecycle probes passed; the
three existing ignored descriptor probes passed after the fixture repairs above.
Structural review found no new re-exports, unsafe blocks or mixed storage owners.
Structure check, formatting, workspace all-target Clippy, diagnostics runtime/probe
Clippy and diff whitespace passed. No carried build error remains. Full-workspace
and exhaustive feature/backend tests remain CI-only and have not been run locally.
