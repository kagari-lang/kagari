# Library-owned native collections execution plan

Status: PG00/NC01 audit, contract closure and shared baseline locally complete,
activated at `50ab194b30b0218e9befe3176d6b5711ecf17a28`, 2026-10-26. The user authorized
the joint PG00-PG06/NC migration with one commit per PG phase. PG00/NC01 is committed
at `47be3cb7`; PG01/NC02's general protocol and external proof are locally complete
after coordinated final validation (PG01 commit `fc651b92`). PG02 physical storage
is locally complete at `4d8aa0c1`. PG03/NC03-NC04 is locally complete and committed
at `8d597b93` after final independent review, focused contracts, selected paired gates
and parent local checks. The finite removal inventory below is closed. PG04 is locally
complete at `fe56b675`, with synchronously drained marking and bounded physical sweep.
PG05/NC05 is integrated but not complete: borrowed-view preflight and callback-unwind
cleanup are fixed, and selected actual-native proof passes. Budget/cache source review,
selected tests and local quality pass. Ordinary-build runtime observations are implemented
and locally validated; the later disabled-observation overhead finding is repaired with
focused/quality checks passing. The trusted actual-root replacement completed all 112 timing
processes with semantic assertions passing: **77 ordinary gates pass / 11 fail; 19 observed
normal gates pass / 3 fail; 34 rows are unobserved, not passes**. Separately audited fresh memory
passes **55/55 with 21 zero-net teardowns**. Old 78/10 and 22-normal summaries are historical;
the contaminated selected 23/6 interpretation is withdrawn. Numerical acceptance remains open;
the [PG05 ledger](paged-incremental-gc-design.md#pg05-trusted-replacement-results-and-finite-follow-up)
owns current results and finite follow-up. This is not NC06 or joint CI acceptance. The
[roadmap](implementation-roadmap.md#library-owned-native-collections) owns sequencing;
this plan owns NC01-NC06, the selected Native contracts and the collection ledger.
General provider APIs and standard collection algorithms below are implemented;
PG04 graph/publication mechanisms have passed local acceptance under synchronous
draining; the integrated PG05 interleaved driver remains under validation. The [PG00 baseline](paged-incremental-gc-design.md#pg00-contract-closure-and-baseline)
owns shared measurements, frozen gates and actual local validation.

Storage inventory refresh, 2026-10-18: the prioritized
[runtime storage redesign](implementation-roadmap.md#runtime-storage-redesign) and
[object overhead work](implementation-roadmap.md#object-storage-overhead) are locally complete.
The predecessor inventory at `65bbf477`, rechecked at the activation revision, has:
compact directory, typed native payload store, packed script-struct arena, weak
shared interface descriptions and direct snapshot locations. Mutation revisions
belong to runtime-owned NativeObject records, not the common directory or a
library-controlled payload field. NC must reuse that owner and define checked
public edit/commit access instead of creating another revision counter.
Full owner/slot/generation handles, native tracing, extraction, revision checks and
panic-contained disposal remain the contracts NC must reuse. NC still owns
library collection storage/algorithms; the predecessor does not perform NC's
collection migration or add new collection-family policy to generic storage.

## Outcome and scope

Make Vec, HashMap and HashSet ordinary nominal native objects registered through
NativeModule. Their Rust library implementations own storage and algorithms;
declarations describe generic parameters, bounds, methods and trait implementations.
Adding an application collection must not require a new semantic type, runtime
value variant, compiler-recognized container layout or backend operation.

Strict independent-stdlib acceptance: all three standard collections, including
storage, algorithms, concrete cursors and host adapters, must be implementable in
a separate crate using the same public generic Native APIs as external providers.
Public collection-specific Runtime/GcHeap methods do not satisfy this requirement.
The standard implementation must exercise that boundary, not leave the external
fixture on a generic path while standard collections retain privileged paths.
This is an ownership/access test, not a requirement to create a new production crate.

```text
Rust collection storage and operations
    -> checked NativeModule declarations and local storage/entry bindings
    -> ordinary kgr imports, method calls and trait dispatch
```

Keep existing script APIs and observable collection semantics:

```kgr
val values = Vec::from([1, 2, 3]);
values.push(4);
val readable: List<i32> = values;

val scores: HashMap<String, i32> = HashMap::new();
scores.insert("Alice", 100);
val view: Map<String, i32> = scores;

val unique = HashSet::from([1, 2, 2, 3]);
```

Array literals still construct builtin fixed-length arrays, distinct from Vec and
List. Assignment/upcasts preserve shared identity. Read-only interfaces hide
mutators without freezing objects or introducing borrow syntax. Map/Set interfaces
impose neither hashing nor order; HashMap/HashSet require Eq + Hash on keys/elements.
Ordered implementations can require Ord instead.

The [collection content contracts](spec/collection-access.md#content-protocols-and-helpers)
own conditional Array PartialEq, stdlib PartialEq/Eq/Hash for Vec/HashSet/HashMap,
and the three content comparison functions. The implemented
[comparison rules](spec/value-semantics.md#accepted-comparison-design) have no
identity equality defaults or collection-interface receiver forwarding. NC must
reuse checked content implementations, helpers, interface Debug and ordinary
selected calls without reintroducing comparison certificates or duplicate
algorithms. Internal hash-table storage remains NC-owned.

Trait access markers and their builder/wire APIs are removed. The remaining
[host/storage descriptor inventory](spec/collection-access.md#host-storage-boundary)
belongs to the current concrete Map/Set families, typed host paths/handles and
source-free access-flow checks. NC replaces those families coherently and preserves
exact readonly admission, checked setters, roots and supplying generations. SA21
consumes the same ordinary declarations while owning Rust borrowing and registration.
Neither owner may turn an interface into a second storage/access authority.

The [paged incremental GC design](paged-incremental-gc-design.md) owns the
collector/page architecture. Accepted sequencing, 2026-10-26: opaque registration,
the general payload protocol and affected collection integration are one coordinated
replacement migration. Implement reference publication, accounting, scanning and
opaque API changes together, not as a separate post-GC API revision. NC owns
concrete collection layouts, algorithms and its detailed phases; the GC replacement
cannot reach final acceptance with container-specific collector/storage paths
deferred to NC. Activation is recorded above; PG00's shared baseline and dependent
phase entry gates still apply.
Current-call selection follows the implemented
[publication contract](spec/module-activation.md#live-installations-and-call-selection),
not historical wording that would pin every stored method body.

Scope includes collection declarations, public native storage capabilities,
retained checked calls, collection cursors, host collection adapters, executable
contracts and source-free validation. Migrate affected collection paths end to end.
Generic iteration mechanisms follow the implemented iteration contracts.

Exclude a kgr rewrite of collection algorithms, dynamic-library loading, changes
to the implemented host registration/interface contracts, further identity
redesign, new language syntax, a borrow checker, async collections, new collection
methods and production
BTreeMap/IndexMap libraries. Small external collection fixtures prove extensibility;
they do not create a new standard-library product obligation. String, Range,
Future, Task and TaskScope representations are not migration targets.

## Baseline evidence and root cause

Planning baseline: `9cd90557`, after the five Iterable commits following
`35de1818`. Iterable APIs are implemented and locally accepted; the roadmap records
the remaining CI gap, while current contracts live in the specifications.
The activation audit rechecked implementation at `50ab194b`; NC01/NC02 are locally
complete. The table records the historical coupling that PG03 removes, not current
runtime collection ownership. Reuse the completed Iterable work.

| Activation boundary / surviving owner | Historical evidence and disposition |
| --- | --- |
| [Vec declarations](../crates/kagari-stdlib/src/foundation/vector.rs), [hash declarations](../crates/kagari-stdlib/src/foundation/hash.rs) | Vec was nominal but declared Sequence storage; HashMap/HashSet used special native constructors. PG03 replaces all three with ordinary native declarations. |
| [Shared native declarations](../crates/kagari-types/src/declaration/native.rs), [HIR application](../crates/kagari-hir/src/native.rs) | Collection constructor/layout variants leaked implementation families into semantic modeling. PG03 removes these dependencies rather than renaming them. |
| [Heap directory](../crates/kagari-runtime/src/gc/directory.rs), [shared interfaces](../crates/kagari-runtime/src/gc/interfaces.rs) | The directory owns generation/locator; native records own mutation revisions. Interfaces locate metadata snapshots directly, preserving distinct checked heap/metadata identities and roots; shared descriptions do not own receivers. Packed script fields do not reinterpret Rust payload memory. No public raw arena/ownerless-token API was added. |
| [Native storage](../crates/kagari-runtime/src/native/storage.rs), [type builder](../crates/kagari-runtime/src/native/builder/type_builder.rs) | Factories, payload bindings, tracing and scoped type arguments already exist. Native revision initialization/reset/replacement/exhaustion has one runtime owner; the selected NC01 contract extends checked public mutation access at NC02. Extend these owners only for proven gaps. |
| [Managed fields](../crates/kagari-runtime/src/native/payload/managed/mod.rs), [fixed native data](../crates/kagari-runtime/src/native/payload/data.rs) | Safe edits separate fixed Rust data from checked script edges. Dynamic storage must preserve that boundary; arbitrary mutable payload access is not a solution. |
| Former runtime sequence/hashed/hash_storage modules; now [stdlib collections](../crates/kagari-stdlib/src/collections.rs) | Runtime owned concrete storage and optimizations. PG03 relocates policy while preserving generic checks. |
| [Hash operations](../crates/kagari-stdlib/src/foundation/hash/operations.rs), [codecs](../crates/kagari-runtime/src/native/binding.rs) | Methods depended on special value tags, heap operations and layout admission. PG03 replaces these with checked nominal access. |
| [Iterable declarations/defaults](../crates/kagari-stdlib/src/foundation/iteration.rs), [shared default lowerer](../crates/kagari-runtime/src/native/builder/default_lowering.rs), [operations](../crates/kagari-stdlib/src/foundation/iteration/operations.rs), [lazy cursors](../crates/kagari-stdlib/src/foundation/iteration/cursor.rs) | Uniform defaults, Iterator identity adaptation and nominal lazy adapters are implemented. Reuse these algorithms and checked iter/next calls; the old Vec-only free map and NativeCursor SDK were removed. |
| [Call context](../crates/kagari-runtime/src/native/context.rs), [stored selections](../crates/kagari-runtime/src/native/stored_selection.rs) | Payload allocation retains selected calls as metadata edges and later invokes them through the rooted receiver. PG03 reuses this mechanism for collection key calls through the reviewed generic creation boundary. |
| [Vec construction/requirements](../crates/kagari-stdlib/src/foundation/vector.rs), [hash construction/requirements](../crates/kagari-stdlib/src/foundation/hash.rs), [Vec operations](../crates/kagari-stdlib/src/foundation/vector/operations.rs), [shared construction traversal](../crates/kagari-stdlib/src/foundation/iteration/traversal.rs) | Vec/HashMap/HashSet FromIterator and collect delegation already work. Hash construction selected insert but declared Ty::Map/Set and branched on Value::Map; PG03 removes these representations without replacing consumption semantics. |
| [Library vector cursor](../crates/kagari-stdlib/src/collections/vector/cursor.rs) and hash cursors | PG03 replaces concrete runtime cursors with nominal traced payloads. Reads check structural revisions without iteration leases or consumer-scope traversal. |

At activation, map insertion passed through Value::Map and GcHeap map operations
into a runtime-owned table. That path also enforced key/value typing, allocation
admission, custom Hash/Eq calls, lookup guards and mutation commits. Vec similarly
used runtime sequence operations despite its nominal identity. These are real
responsibilities, not removable overhead.

The defect is coupling those responsibilities to concrete container families.
Runtime should own general checking/lifetime mechanisms, and libraries should
own storage and operation policy. Historical motivation is not inferred from
current code. This migration makes no unmeasured performance claim.

### Finite removal inventory

All rows are closed and locally accepted after final independent review of the
PG03 checkpoint `8d597b93` based on `4d8aa0c1`. This is not NC06/CI completion.

| Removed privilege | Current owner / preserved boundary |
| --- | --- |
| Runtime sequence payload, allocation/admission and heap sequence operations | [Stdlib Vec](../crates/kagari-stdlib/src/collections/vector.rs) owns compact buffers and ordinary opaque representation; builtin Array remains independent. |
| SequenceEdit/detached buffer and sorting reference-root snapshot | [Library algorithms](../crates/kagari-stdlib/src/collections/vector/algorithms.rs) keep main/scratch/progress in one payload using generic guards/recovery; no sort-admission prepass. |
| Runtime hashed/hash_storage/custom_keys policy | [Library hash kernel](../crates/kagari-stdlib/src/collections/hash.rs) owns dense entries, prehashed scalar index and collision links; checked selected calls stay generic. |
| Runtime Vec/Map/Set cursor variants | Nominal library cursors scan source edges and validate native revisions through the public provider boundary. |
| Concrete Map/Set Ty/Value families and collection constructors/layouts | [Ty](../crates/kagari-types/src/ty.rs), [Value](../crates/kagari-runtime/src/value.rs), native declarations, HIR/compiler and wire consumers use ordinary nominal applications, with no compatibility readers. |
| Collection binding codecs and special host constructor matching | Generic opaque admission, verified native-application witnesses and selected-slot creation remain authoritative. |
| VecIndex/VecSet/VecSetFluent and linked Vector execution | Ordinary library entries use checked borrowed payloads; builtin Array and unrelated fixed primitives remain engine-owned. |
| Runtime collection adapters/conversions and hardcoded Vec identity | `kagari_stdlib::collections::{map::ScriptMap, set::ScriptSet, vector::{ScriptVec, VecValues}}`; owned conversion preserves snapshots before child conversion. No runtime facade. |
| Runtime Serde collection conversion owner | [Stdlib Serde](../crates/kagari-stdlib/src/serde/mod.rs) owns schema/serializer/deserializer; runtime retains only RuntimeError's Serde error integration. |
| Per-heap collection descriptors and diagnostic downcasts | Generic descriptor sharing/accounting plus library diagnostic counters; no collection-specific heap cache or downcast. |

Builtin fixed Array storage/operations and justified shared buffer helpers remain;
so do language/execution state, frames, roots, scopes and generic call machinery.
Their Rust Vec/HashMap use is not a script-collection semantic dependency. String,
Range, Future, Task and TaskScope remain outside this removal inventory.

NC01 decision: keep the [index-write bridge](../crates/kagari-hir/src/typeck/body/places.rs)
as a bounded language-to-declared-interface bridge. It selects the exact installed
`std::collections::MutableList::set`, requires a unique applicable bound and the
receiver/u64/item-to-unit signature, and preserves once-only ordered place evaluation.
It grants no Vec storage authority and no assignment privilege to an unrelated `set`.
Keep fluent inherent Vec set distinct from unit-returning trait set. Remove Vec's
backend primitives, not this ordinary selected-call bridge; no trait-role redesign.

### Reuse the completed Iterable mechanisms

For `val result: Vec<i32> = values.map(f).take(2).collect();`, the existing default calls
the selected `iter` once. Its adapter payload stores the source and callback,
and allocation retains the selected `next` slot. Consumption invokes that stored
target with the original supplying generation. `collect` delegates to the
destination's FromIterator; traversal opens the existing consumer scope.

After NC, the same flow must work when values is the migrated Vec or an external
collection. Only the concrete source/destination storage, their methods/cursors
and general storage protection change. Do not recreate map/filter/take, the other
adapters, terminal algorithms, Iterator identity adaptation or destination dispatch.

At activation, the existing low-level selected-slot path is:

```rust
// During a checked native constructor, with declared callable requirements:
cx.allocate_result_payload(&[hash_slot, eq_slot], |_cx, _result| Ok(payload))?;
// During a later method, whose argument 0 roots that native receiver:
cx.call_stored_values(0, 0, &[key])?;
```

Allocation copies the chosen selected calls into object metadata slots, in the
supplied order. StoredSelection stays runtime-private; libraries do not construct
owning metadata roots inside payloads. StoredCallable remains the mechanism for
traced script closure values. These are existing capabilities, not new NC APIs.
They do not by themselves provide checked resizing, public revision/commit access or
factory-level key-call registration. PG01/NC02 now preserves `RootedValue` from
`call_stored_values`; typed creation uses `NativeType::create(cx, payload, slots)`.
The general protocol below is locally accepted in PG01/NC02; the standard
collection special paths were removed in PG03 commit `8d597b93`.

## Ownership and representation

| Layer | Target responsibility |
| --- | --- |
| types / HIR | Native definition identity and arguments; ordinary declarations, trait bounds and checked calls. No concrete Map/Set semantic type family. |
| contract / compiler / backends | Verify/lower nominal native calls, interface evidence and storage admission. No collection-name-based type inference or method rewriting. |
| runtime native infrastructure | Identity, installed payload bindings, scoped access, type/owner/generation checks, tracing, roots, allocation accounting and retained-call lifetime. |
| stdlib collections | Vec/HashMap/HashSet declarations, buffers/tables, methods, iteration policy and concrete optimizations. |
| application module | Its own storage and methods through the same public capabilities used by stdlib. |

Portable declarations contain no Rust TypeId, function pointer or physical payload
layout. Installation binds the checked nominal definition to local Rust storage
and entries. Source-free loading rejects missing/mismatched bindings without HIR.

Remove collection-only Map/Set TypeId/Ty and Value variants and constructor
branches. Retire Sequence/Map/Set layout categories as collection identities or
blanket storage authorization. A generic buffer/slot capability may remain only
with an explicit owner and validated access contract; it cannot assert that its
owner is Vec, List or Iterable. Preserve builtin Array support and justified
shared low-level buffers without keeping a second Vec implementation in runtime.

Vec/HashMap/HashSet base iterators become ordinary registered native objects;
the lazy adapters already are. NC01 inventories CollectionCursor/Iter consumers,
including builtin Array, String and Range, before removing any shared cursor tag
or layout. Migrate the concrete collection cursor paths and leave the other
consumers functional through their existing representation unless a bounded
shared-mechanism change is necessary. Residual builtin cursor support is allowed;
using it to retain runtime-owned Vec/HashMap/HashSet policy is not. Unrelated
iterator or builtin storage redesign is outside this task.

## Public API shape

### Ordinary Native callers versus storage providers

Native functions using GC objects should call checked handle methods directly:
field `get/set` and collection `get/set/push/insert/remove`. Accessing several
objects must not require nested payload-edit closures or long-lived borrow guards.
The [GC design's Native-facing operations](paged-incremental-gc-design.md#native-facing-gc-object-operations)
own the examples, root/edge distinction and incremental publication requirements.
Existing Object/Field and ScriptVec access provide the baseline ergonomics, not
an obligation to retain ScriptVec or collection methods in Runtime. Library-owned
safe functions/extensions over Opaque<library payload> are sufficient; no mandatory
VecHandle/ScriptVec wrapper or second handle category is required. This does not
claim that the proposed general storage protocol is implemented.

The registration and payload sketches below are for collection implementers, not
ceremony required of ordinary Native callers. NC01 must preserve flat handle
operations while moving concrete storage into libraries. Runtime owns reference
publication, root protection, accounting and native revision updates; callers do
not manage barriers or mutation counters. Provider access remains audited, and
this distinction does not grant arbitrary mutable access to traced Rust payloads.

### Opaque registration and public categories

Accepted API direction, 2026-10-26; implemented and locally accepted in PG01/NC02. Keep the existing public
vocabulary: `Ref<T>` is a scoped shared borrow, `RefMut<T>` is a scoped mutable
borrow, and `Opaque<T>` is a retained handle to an opaque Native object. Opaque
describes access to a Rust representation that is not exposed as script fields;
it does not mean that the collector cannot trace the representation. Current
runtime-owned payloads use GC for their lifetime, but that implementation choice
must not introduce a separate public "GC type" category or `define_gc_type` API.
Host-owned instances remain host-owned when borrowed; registering a type does not
by itself transfer an instance into runtime storage. Borrowing still follows the
existing access and lifetime contracts.

Ordinary opaque registration must associate the script declaration with its Rust
payload type once. Derive the storage descriptor from that association and the
payload's trace/accounting/access contract; do not require a second routine
`native_storage(NativeStorage::payload::<T>())` declaration. `NativeStorage` remains
a lower-level runtime descriptor, not another public object category. Reconcile
explicit declaration building with the existing `Opaque<T>` registration and
`NativePayload::registration_storage` paths, rather than adding a parallel facade.
Special factory/managed-field requirements remain explicit where needed; they
must not make ordinary opaque declarations repeat the type association.

Select `ModuleBuilder::define_opaque::<P>(name) -> NativeResult<TypeBuilder<'_>>`.
The existing type builder gains `bound` support and records those bounds rather
than finishing with an empty list. `TypeRegistration::opaque` uses the same payload
association/mapping implementation; reject duplicate/conflicting associations.
`NativePayload::registration_storage` remains the descriptor policy hook. Instances
use existing `Opaque<P>`; script generic arguments are checked runtime evidence,
not dynamically instantiated Rust generic payload types.

This naming/registration change does not grant unrestricted mutable payload access.
Complete dynamic reference-bearing edits, accounting, revisions and publication
under the existing Native owner. Ordinary Vec/HashMap/HashSet implementations must
use that public opaque path, with no collection-specific storage admission or
dispatch privilege. Efficient calls use prepared Native entries and checked payload
access; retain compact library-owned buffers and validate costs through the focused
measurements below, without promising unmeasured parity with special entrypoints.
Implement this adjustment within the GC replacement and its general payload
protocol. NC01/NC02 remain the detailed API work breakdown, not a later independent
delivery. Do not first implement a new collector around special collection storage
and defer the ordinary opaque boundary until afterward.

### End-to-end API proposal for review

NC01 selection at the activation revision: precise tracing, collecting Native
callbacks and unsafe payload integration. PG01/NC02 now implements the general
provider signatures below, replacing the whole-trace/fallible-charge API, with
coordinated local validation complete. Collection algorithms and incremental
publication described here retain their later-phase owners. The
[GC design](paged-incremental-gc-design.md#incremental-tracing-and-mutation) owns
phase scheduling, physical storage and joint graph closure. No new GC handle
category, NoGc whitelist, mandatory branding or per-element owning roots are added.

| Owner | Selected change |
| --- | --- |
| [native/storage.rs](../crates/kagari-runtime/src/native/storage.rs) | Unsafe NativePayload, one erased scanner, admission and descriptor policy. |
| [native/storage_type.rs](../crates/kagari-runtime/src/native/storage_type.rs) | Expose StorageType with private fields and scoped preparation; retain its exact type/owner comparisons. No CheckedElementType replacement. |
| native/storage/access.rs, a new ordinary submodule of the existing storage owner | StorageAccess and EditPlan are loan-scoped services over existing validation, roots, resources and revisions, not another heap or registry. |
| [native/payload/mod.rs](../crates/kagari-runtime/src/native/payload/mod.rs) | Existing NativeType creation and Opaque read/edit; safe fixed-data access remains distinct. |
| [gc/native.rs](../crates/kagari-runtime/src/gc/native.rs), [gc/leases.rs](../crates/kagari-runtime/src/gc/leases.rs), [native/views.rs](../crates/kagari-runtime/src/native/views.rs) | One operation guard with central conflict enforcement and sealed recovery. |
| [gc/roots.rs](../crates/kagari-runtime/src/gc/roots.rs), [native/value_handle.rs](../crates/kagari-runtime/src/native/value_handle.rs), [resource.rs](../crates/kagari-runtime/src/resource.rs) | Existing root leases/ScriptValue retention, publication, HeapChange and committed native charge. NativeObject remains the only revision owner. |
| [stored selections](../crates/kagari-runtime/src/native/stored_selection.rs) and [prepared calls](../crates/kagari-runtime/src/native/function_handle/selected.rs) | Stored relationship edges select current bodies; active PreparedFunction computations pin exact selections. |

#### Unsafe access and safe typed loans

Make NativePayload an unsafe trait. Every strong internal edge must be discoverable,
including nested/shared backing and executable relationships; metadata selections
stay with the containing NativeObject. Interior mutability may not bypass checked
publication, accounting or revisions. Value remains non-rooting: copying it into a
Rust local/container grants no lifetime protection. Payload references cannot
escape a loan and no collection/reentry through any Runtime alias may occur during
that loan. Unsafe integration does not waive type/owner/generation or host-scope
validation. Native entry as a whole remains collectable.

Select these signatures (text describes scoped closures, not compiling Rust):

```text
unsafe Opaque<P>::read<R>(
    &self, cx: &NativeContext, operation: Option<&NativeOperationGuard>,
    f: for<'loan> FnOnce(&'loan P, &mut StorageAccess<'loan>) -> NativeResult<R>
) -> NativeResult<R>

unsafe Opaque<P>::edit<R>(
    &self, cx: &mut NativeContext, operation: Option<&NativeOperationGuard>,
    plan: EditPlan,
    f: for<'loan> FnOnce(&'loan mut P, &mut StorageAccess<'loan>) -> NativeResult<R>
) -> NativeResult<R>

EditPlan { max_logical_units: usize, structural: bool }
```

max_logical_units bounds the payload's post-edit charge, excluding the runtime's
identity/shallow charge; structural preflights a possible revision increment.
Runtime applies the same checks to lower-level payload views. Make unrestricted
ValueHandle::with_payload and CallContext::with_payload unsafe too.

The [typed shared receiver adapters](../crates/kagari-runtime/src/native/typed/loans.rs)
and their [paired forms](../crates/kagari-runtime/src/native/typed/loans/pairs.rs)
now require fixed-data evidence for runtime-owned shared payload loans:
safe adapters may lend runtime-owned Rust data only when the installed descriptor
carries the existing NativeData proof. Check this before argument converters or
callback effects, for each opaque receiver in a pair. External host-owned Ref/RefMut
continue through the existing host borrow checks, without requiring NativeData.
Unrestricted providers use Opaque and unsafe access instead of safe shared adapters.
Do not hide an unsafe read inside a safe blanket adapter. NativeTake is not a
shared-loan proof: it permits edge-free variable/interior state. Keep safe
read_data/edit_data conveniences for NativeData; do not define overlapping safe
and unsafe inherent read/edit methods or introduce another proof whitelist.

#### Retention and publication

StorageAccess exposes the following bounded services:

```text
retain(&mut self, contract: &StorageType, value: &Value) -> NativeResult<ScriptValue>
publish(&mut self, contract: &StorageType, input: &ScriptValue) -> NativeResult<Value>
unsafe republish_existing(&mut self, value: &Value) // no allocation, no Result
revision(&self) -> u64
check_revision(&self, expected: u64) -> NativeResult<()>
commit_structure(&mut self) // infallible after structural preflight; once per edit
```

StorageType keeps scoped TypeArgument/owner evidence; expose prepare_scoped and
StorageContext::element_contract through that same owner. Preparation establishes
a closed heap-storage contract; access still validates it against this Runtime.
Typed constructors extract a parameter from NativeType::type_argument using its
supplying owner and prepare that same StorageType evidence. Do not infer storage type from a first element or a textual name.

retain constructs the existing ScriptValue directly using RootedValue, its exact
TypeArgument and supplying ProgramLease. It prepares fallible root/metadata lease
capacity and publishes those roots while the source is still protected. It may
allocate Rust bookkeeping but may not collect, invoke conversion or admit a new
graph identity under the payload loan. Existing admitted metadata is retained,
not reconstructed. Do not call decode_prepared/ScriptValue::decode here: ordinary
conversion can safepoint. Implement checked access against split storage/identity
views, not by recursively borrowing the same objects RefCell during an edit.

publish validates runtime/slot/generation, declared access and scoped type before
shading the input and returning its internal Value. The input remains rooted until
the destination is installed. republish_existing is only for already validated
internal edges moved within their checked storage contract before losing source
protection; it cannot admit a new external value or change its type. Runtime uses
the same allocation-free shading core, with identity invariant failures failing
closed/quarantining rather than resuming normal execution. All root replacement,
frame/result and metadata publication must use that core before PG interleaving.

Example pop: preflight edit, retain the last item, commit structure, remove it
infallibly, finish accounting, end the loan, then return Option<ScriptValue>. A retention
failure leaves the item in place. User decoding happens afterward. A safe library
push accepts a retained item and encapsulates validation/publication; ordinary
callers never manage these services or individual element roots. No public
prepare_removal/commit_removal pair or second root registry is introduced.

#### Unwind accounting and structural commits

Select infallible `charge(&self) -> HeapCharge`: O(1), nonallocating and nonpanicking
for every installed state, including partially completed operations. Providers
maintain checked capacity totals at allocation/transfer boundaries; they must prove
per-object arithmetic representable before retaining new backing. Variable reserve
results use actual capacity, not the requested minimum or an O(n) recount.

Reuse ResourceState::prepare_heap_change/HeapChange before mutation: execution
admission, maximum logical occupancy, retained-byte arithmetic and required revision
capacity are fallible preflight. Runtime's edit RAII finalizer samples charge and
commits actual backing on success, returned error and unwind. Successful reserve
followed by failure remains charged. No fallible post-edit trace/charge establishes
safety retroactively; the current bookkeeping after with_buffer_mut's callback is
not sufficient for this contract. Provider code must leave a scanner-valid,
charge-valid intermediate state at every panic-capable boundary. Unexpected violation
quarantines rather than sweeping or continuing with a guessed charge.

commit_structure consumes the preflighted revision transition immediately before
the infallible structural write; all potentially failing preparation precedes it.
Element replacement stays nonstructural where current cursor semantics require it.
Exclusive sequence-style algorithms reserve one invalidate-on-exit transition at
operation admission, matching existing lease behavior even on failure; their short
internal edits do not increment another counter. Cursors retain source plus expected
NativeObject revision, not an iteration-lifetime lease. Scratch capacity is attributed
to its actual owner; moving backing does not duplicate its charge. Deferred disposal
retains the backing charge until destruction, even after identity retirement.

#### Operation protection and recovery

Extend NativeOperationGuard/LeaseTable, keyed by full heap identity, with exactly
two policies: ExclusiveSlots and ReadOnlyCallbacks. ExclusiveSlots permits access
only through its unforgeable lease token; alias slot reads/writes/take and recursive
entry reject. ReadOnlyCallbacks permits reads/nested lookup but rejects mutation.
All opaque, raw-view, typed-loan, managed access and extraction routes enforce this
centrally; replace the SequencePayload.detached downcast. Identity/type conversion
is not a slot read: remove Opaque::from_checked's use of unrestricted with_native
for mere identity validation. GC may scan protected storage but never an active Rust
loan. Callback access to unrelated objects and referenced element objects remains legal.

Select unsafe Opaque::try_enter_operation(cx, kind, recovery) returning
NativeResult<Option<NativeOperationGuard>>. The guard retains the receiver through
existing roots without borrowing NativeContext across callbacks. `recovery` is an
optional `fn(&mut P, &mut StorageAccess)` registered against the checked payload
TypeId; its state resides in P, not a captured Rust work buffer. Acquisition prepares
lease/root/revision and cleanup-accounting resources before any effects. Do not hold
HeapChange's counters borrow across callbacks: each short edit commits separately;
cleanup reconciles the latest committed charge without growth, so its arithmetic
cannot require a new fallible reservation. Drop runs recovery, reconciles
charge/revision and then releases protection. Recovery is
nonallocating, nonpanicking, cannot invoke script/converters, and cannot create a
graph identity or increase the pre-admitted occupancy bound. It uses a sealed cleanup
loan which bypasses cancellation/admission only to restore invariants, not ordinary
Opaque::edit (which can reject after cancellation). Normal work still polls; no poll
or cancellation redesign is included. Restoration cost is measured, not claimed to
have a hard latency bound. Unexpected recovery failure quarantines the runtime.

For hash lookup/commit, retain candidates during protected reads, end loans for
Hash/Eq and prepare required result objects before removal. Then release the lookup
guard and enter the non-reentrant edit without any callback/conversion/GC gap.
Receiver backing reserve occurs under that edit, after callbacks and before commit;
read-only protection does not authorize mutating the table to reserve capacity. Enclosing lookup
leases still reject nested mutation. Do not retry observable callbacks. Guarding
only recursive entry, as at baseline, does not establish this contract.

#### Single scanner and continuation

Replace whole-payload trace with one provider scanner used by initial validation,
full collection and incremental marking:

```text
unsafe trait NativePayload: Any + Debug + Send {
    type ScanState: Copy + Send + 'static;
    const SCAN_KIND: NativeScanKind;
    fn begin_scan(&self) -> Self::ScanState;
    fn scan<'p>(&'p self, state: &mut Self::ScanState, budget: &mut ScanBudget,
                visit: &mut dyn FnMut(&'p Value)) -> ScanStatus;
    fn charge(&self) -> HeapCharge;
    // Existing registration_storage policy hook remains.
}
NativeScanKind = EdgeFree | Bounded { max_work: usize } | Resumable | Indivisible
ScanStatus = Pending | Complete
```

begin_scan and scan cannot allocate, mutate payloads, panic, reenter or retain
visitor references. ScanState contains only owned cursor data: no payload pointers,
roots, untraced edges or drop obligations. ScanBudget spends inspected logical
slots/edges, including holes, and setup/metadata work; Pending must make progress
for positive sufficient budget. Indivisible providers explicitly charge their full
work and provide no bounded-step claim. PG05's traced service pre-reserves up to four
bitmap-reset words per emitted edge in addition to provider work. Bounded max_work is
therefore at most **204 provider units** (204 × 5 = 1020 within normal 1024 work);
larger scans must be Resumable or explicitly Indivisible. Resumable chunks are capped
at 256 provider units but adapt to the remaining shared budget. Unused reservation
is refunded; actual provider/reset work is counted, including Indivisible overruns.
Large standard collections use Resumable.
Runtime separately traverses NativeObject's selected metadata through the same graph
work owner; providers do not embed external Function/root leases to retain internal
relationships. Stored callable values use their existing traced representation.

Use numeric positions and a finite captured extent, not borrowed iterators. Reallocation
does not invalidate indices. Every inserted or relocated edge is forward-published,
even when the parent is gray or the destination lies behind the cursor. Appends beyond
the captured extent are covered by publication; do not reset a scan after each edit.
Truncation/compaction may skip removed slots only after publishing surviving moved
edges. No iter().skip restart or payload-sized cursor construction is allowed.

NativeStorage admission drains this same scanner to validate a newly populated
payload before publication. This can be O(n) and is not sort admission: sort keeps
its main/scratch storage in the existing payload. Admission reserves all fallible
identity/continuation/root resources before any collecting boundary and roots the
new object before its first safepoint. Publication during marking cannot install
unshaded outgoing edges; the PG owner defines newborn/sweep phase handling.

#### Allocation-free insertion work

The implemented PG04 marker reserves one collector-owned pending queue at admission.
Its entries name deduplicated **128-slot original-owner blocks** or singleton static
jobs, not one entry/continuation allocation per graph identity. Discovered/scanned
bitmaps and full-generation resolution remain with the original stores. These are
identity-store blocks, not payload slabs. The insertion barrier shades/enqueues; it
never begins a scan. One owned active identity cursor retains scan progress.

Identity credits and block/singleton work credits have separate bounds. Admission
reserves the applicable work capacity before exposing an identity; checked reservation
failure leaves it unpublished. Credits remain until queued/active work is impossible,
including empty pending blocks after take and superseded static jobs. Repeated shading
does not duplicate pending block work; resuming the active task uses its existing
reservation. No second ID registry, duplicate edge log or backward regray queue is
needed. Faults prevent partial-graph sweep, rather than blackening incomplete work.
Queue capacity/slack is included in the PG owner's measured backing/peak gates;
reservation is a safety invariant, not a claimed saving.

The single aligned erased scanner workspace is reserved before payload admission.
PG04 transactionally preserves an active Copy continuation through growth; failure
leaves the old state intact. No scanner/loan holds a workspace pointer across growth.
Builtin/metadata scan state and root/frame enumeration use owned resumable cursors,
not root snapshots. Existing-target barrier enqueue, scanner start/resume and cleanup
republication require no allocation. Retaining a new external root may fail before
removal, never as recovery work. PG04 drains synchronously, with finite
256-unit resumable scanner chunks; this is distinct from the selected 1024-unit normal
driver budget. PG05 integrates interleaved service, with open acceptance below. The
[PG04 ledger](paged-incremental-gc-design.md#pg04-marker-implementation-and-open-acceptance)
owns local evidence and source applicability.

Root release satisfies the same cleanup promise: RootLease::drop uses a shared
mutex-protected pending-ID Vec rather than allocating std mpsc transport in gc/roots.rs,
reserving one notification credit per admitted root registration. Clones share that
registration and only the last drop enqueues once. Reserve the root free-slot Vec
at root-slot admission too, so draining notifications cannot allocate. Keep pending
capacity while draining (no mem::take that discards the reserve); release a credit
only after processing its notification. Last-drop enqueue locks only this transport,
never heap storage or script, and cannot grow it. Table destruction disconnects the
transport; outstanding leases own only IDs/transport state, not heap data. Preserve
full root generations, cross-thread release and live-lease checks. This is the
existing root table/lease owner with a bounded release transport, not another root
registry; do not claim std mpsc itself supports reserving send capacity. Prepare
registration/ProgramLease allocations before destructive removal; Arc deallocation
and existing program-token release require no new allocation. Arbitrary external
Rust destructors remain outside the provider recovery guarantee.

Example: after a comparator cancels, recovery copies a scratch element behind the
Vec scan cursor. Its target identity was already admitted, so shading either does
nothing or uses one of these reserved pending slots; recovery needs neither a new
root nor queue growth. New allocations made by the comparator reserved their own
credits before publication. This is the cleanup guarantee, not a best-effort reserve
at sort entry. PG owns byte layout, capacity/slack measurements and pressure gates;
this choice is justified by the invariant, not a measured memory/speed claim.

### Concrete Vec sorting API proposal

Selected tracing direction, 2026-10-26. Keep safe library operations and existing
[sorting semantics](spec/collection-access.md#foundation-algorithms), not a required
runtime-owned ScriptVec facade. Illustrative library functions (extensions are
also allowed); T/K denote checked script applications, not Rust payload instances:

```text
sort(cx, &Opaque<VecStorage>) -> NativeResult<()> // selected element T: Ord
sort_by(cx, &Opaque<VecStorage>, Function<(T, T), Ordering>) -> NativeResult<()>
sort_by_key(cx, &Opaque<VecStorage>, Function<T, K>) -> NativeResult<()> // K: Ord
```

Resolve and pin comparator/key-selector evidence once. Sort uses Ord, not Hash;
key selectors run during comparisons without a cached-key prepass. Preserve stable
worst-case O(n log n) comparisons/moves, with callback and GC work accounted for
separately. No preparatory O(n) retention snapshot or index construction is allowed.

Selected representation: main buffer, optional scratch and merge-progress state
stay in the same already-rooted VecStorage. There is no detach, separately admitted
filled work object, retention snapshot or preparatory index. Reserve empty scratch;
initialize it only when a merge needs that run. Select a library-owned fixed-split
stable mergesort with shorter-run scratch and index-based gap restoration, adapting
the reviewed Rust tiny/merge mechanisms below. Do not call Rust slice::sort_by with
collecting callbacks: its private scratch/long slice loan lack publication hooks.
This is the general collecting kernel, not a public SortWork/SortSession abstraction.
PG03 additionally selects a noncollecting path only from checked ValueCmp evidence
and actual pointer-free storage: at most 512 scalar elements per Rust 1.99.1 stable
sort leaf, each at most eight bytes, within its 4096-byte stack workspace. Larger
inputs use bounded merges and same-payload scratch/recovery. This path is compiled
only with panic=unwind; abort targets use the existing Result-returning general
algorithm. Arbitrary callbacks always use the collecting kernel. Reported zero
same-payload copies excludes Rust-internal moves and stack scratch.

The source and scratch together cover every live element. Before every callback:

1. Complete transfers under short non-collecting access; publish roots/edges and
   repair incremental scan progress before losing prior protection.
2. Expose only initialized scanner-visible ranges and end all conflicting heap loans.
3. Invoke the retained comparator/key selector with protected arguments/results;
   GC can progress during nested allocating script execution.

Keep ExclusiveSlots protection and register recovery before the first merge move.
All main slots remain initialized; temporary stale copies may remain traced until
overwritten, while merge indices identify the authoritative source/scratch contents.
The scanner has main/scratch positions and finite captured extents; it scans only
initialized scratch. Publish each copy into scratch or an already-scanned main slot
before losing old protection. Copying a run is merge work, not an entry prepass;
chunk long transfers and poll at valid states. End loans before calls and retain
both comparator arguments and the first key result while computing the second.
No payload reference, borrowed slice or raw scratch pointer spans the call.

Restore valid source/work state before releasing protection on success, error,
cancellation or unwind. Stop user callbacks after the first failure; preserve the
original element multiset, permit partial reordering and retain completed callback
effects. No user callback or fallible allocation may be needed solely to restore
ownership. Reuse existing error semantics; do not retain an adapter that depends on
unobservable Rust scratch. Cancellation remains active within long library work.

Rust kernels without collecting callouts remain reuse candidates, including compact
scalar sorting, but long indivisible work still requires latency/cancellation review.
Do not infer speed parity from O(n log n). Measure comparisons/moves, root/barrier
work, allocations, peak scratch memory and GC-step delay across scalar, cheap-script
and allocating-script comparators. Test collection at temporary transfers, partial
scans, nested callbacks, errors/unwind and restoration. The selected fixed-split
kernel is not adaptive like Rust driftsort; throughput and recovery-delay acceptance
remain with the shared PG00 baseline, not inferred from the complexity bound.

### Other standard-library operation boundaries

The [operation-family inventory](paged-incremental-gc-design.md#future-standard-library-extraction-families)
includes future collection/iterator designs. Do not rewrite the whole standard
library or assume only Hash/Eq call script. Reuse Rust storage and algorithms where
all values remain protected at collecting boundaries.

- for_each/map/filter/fold/zip/collect retain only the required current items,
  accumulator and iterator state; end loans before user callbacks/custom next.
  Allow GC during callbacks, rather than delaying collection for the entire loop.
- Hash/Eq use rooted stable candidates and lookup/callback/commit protection, without
  silently retrying observable callbacks. Existing primitive fast paths remain.
- retain/dedup and other compaction algorithms need controlled temporaries when
  callbacks observe a representation with holes. Use shared root/work-state machinery
  and preserve their completed-removal/error semantics; no full-input root prepass.
  Vec retain/dedup require linear traversal/compaction, not repeated remove/shift
  that becomes quadratic. Custom-interface defaults retain their specified selected
  remove-as-they-go behavior; do not silently replace it with atomic bulk commit.
  Count moves, callbacks, roots/barriers and actual GC work
  separately; collection costs cannot conceal an algorithmic regression.
- Future heap/tree algorithms need the same audit around comparison callouts. Rust
  Hash/Eq/Ord do not directly express context-bearing fallible script calls.
- Escaping iterators/entry-like state use traced owners and revision checks; borrowed
  Rust guards or no-GC regions cannot span arbitrary script lifetimes/suspension.

Validate these families at their existing contract owners, including collection
inside callbacks and long allocating loops. No percentage of future APIs, latency
bound or measured speedup is claimed. GC itself still needs incremental scan,
barrier and sweep protocols; collecting Native callbacks alone do not bound pauses.

### Collection registration and storage

Reuse ModuleBuilder, NativePayload and the underlying NativeStorage descriptor,
applying the selected opaque registration contract above. The following abbreviated
registration examples omit ordinary method signatures/required trait members; they
illustrate the selected owners rather than introduce additional builder APIs.

```text
let mut declaration = module.define_opaque::<VecStorage>("Vec")?;
let t = declaration.type_parameter("T")?;
let vec_type = declaration.finish()?;

module.implement(vec_type, |group| {
    let t = group.parameter("T")?.ty();
    group.associated_function("new", vec_new)?;
    group.associated_function("from", vec_from_array)?;
    group.trait_impl(core.mutable_list(t), |methods| {
        methods.bind("push", vec_push)?;
        methods.bind("set", vec_set)
        // Bind remaining members and implement parent traits separately.
    })
})?;
```

VecStorage uses checked element evidence and owns its native buffer. Keep compact
scalar storage where valid; object elements require traced values. A compact
scalar/traced enum may live inside the library; it must not become an engine
protocol. A kgr generic parameter cannot dynamically instantiate arbitrary Rust
generic code. This plan does not select a new generic per-application Rust payload
mechanism. Select storage from checked type arguments, never the first element or
a textual name.

Physical layout is not element evidence: both Vec<User> and Vec<Enemy> may use
Traced storage, but inserting Enemy into Vec<User> must still fail. Reuse the
existing checked application scope/StorageType contracts, generation and owner
checks at constructor and mutation boundaries. The selected public StorageType
contract reuses that evidence; NativeObject.scope remains the nominal application
authority. Constructors build VecStorage with prepared element evidence and create
through NativeType. No duplicate factory per constructor or unchecked payload field
replaces type admission.

```text
let mut declaration = module.define_opaque::<HashMapStorage>("HashMap")?;
let k = declaration.type_parameter("K")?;
let v = declaration.type_parameter("V")?;
declaration.bound(k.ty(), core.eq())?;
declaration.bound(k.ty(), core.hash())?;
let map_type = declaration.finish()?;
// Constructors declare checked Hash/PartialEq::eq requirements, create storage
// with checked key/value contracts, then call NativeType::create(cx, payload, slots).
// Register Map/MutableMap/Iterable implementations normally.
```

HashMapStorage retains key/value contracts and its table; the containing native
object's metadata retains selected key operations. Eq remains the semantic bound;
the callable equality member is PartialEq::eq, as in current hash construction.
HashSet shares the key-operation machinery. Use checked constructor requirements
and existing retained slots, not a separate type-level resolver or cx.retain_call.
The proof and creation path below supplies equivalent script/host evidence.

Register Iterable Item/Iter and Iterator::next normally. Do not infer traversal
from physical storage. Preserve default/override dispatch through interfaces.
Constructors explicitly declare Array/Iterable inputs and use ordinary inference.

### Bounded applications and constructor evidence

The activation audit distinguishes three existing capabilities:

- [EntryEvidence::find](../crates/kagari-runtime/src/native/function_handle/evidence.rs)
  admits installed closed script/native entries or a closed SharedCall in a
  nongeneric carrier. It checks supplying type scope; an open generic body alone
  proves no host-callable application. [Associated-function binding](../crates/kagari-runtime/src/native/methods/binding.rs)
  already handles static members, including constructors.
- [NativeImport::matches_declaration](../crates/kagari-contract/src/native_import/linked.rs)
  checks binding identity, substituted signature, exact authored requirements and
  selected operations. [SharedCall::check](../crates/kagari-contract/src/callable/shared.rs)
  checks substituted body bounds with ProofCatalog::constraints_hold and validates
  both operations and construction witnesses. [ConstructionWitness](../crates/kagari-contract/src/callable/construction/evidence.rs)
  proves method-independent conformance, including empty traits. Hash/eq slots alone
  do not prove Eq. Static-only constraints must not be forced through dynamic
  interface construction.
- [Application validation](../crates/kagari-bytecode/src/trait_bounds/applications.rs)
  at activation reduced nominal lookup to kind/arity, without proving TypeDef.bounds.
  [ProofCatalog](../crates/kagari-contract/src/types/proofs/search.rs) can check those
  bounds from executable contracts. [ProgramProofs::view](../crates/kagari-contract/src/types/proofs/resources.rs)
  restricts proof resources to the carrier's dependency closure. PG01 adds bounded
  native application checking in the existing contract owner and seals occurrences
  through bytecode verification, rather than adding a runtime trait resolver.

PG01 implements the following locally accepted proof chain;
it replaces bind_native_type's blanket nonempty-bounds rejection:

1. TypeBuilder records the actual native TypeDef parameters/bounds. HIR checks
   applications as usual and emits ordinary native imports/shared calls and their
   existing evidence. A constructor's required Hash/PartialEq calls remain declared
   requirements; declaration bounds and operation availability are distinct checks.
2. Extend the existing contract application-validation owner with a bounded-native
   application check taking the full TypeDef, nominal arguments, binder parameters,
   authored assumptions and the carrier-scoped ProofCatalog. The bytecode applications
   owner already finds the full public/private TypeDef: pass it through instead of
   deriving authority from kind/arity. Validate declaration identity, Opaque layout,
   exact arity, argument scope, associated outputs and heap admissibility. Substitute
   the declaration's bounds and call constraints_hold for every constraint, including
   methodless and static-only traits. Recursively validate bounded native applications
   in the supplied types with existing resource/cancellation limits. Never add the
   very bounds being proved to the assumptions or accept cycles as self-proof.
3. Apply that check to native import signature/application occurrences and the actual
   native signature at shared call sites, after their existing declaration/target
   checks. Closed imports use no assumptions. Open native bodies may use only their
   verified authored body bounds; their result application is a proof under that
   binder, not a new closed application. Shared calls substitute one binder layer
   and verify actual arguments/obligations against the caller's verified assumptions.
   Native result adapters validate the concrete native result as well as the exposed
   result view. Merely finding a type in an arbitrary function signature is not proof.
4. Add an owner-integrated, nonserialized NativeApplicationWitness to the existing
   [ProgramVerification](../crates/kagari-bytecode/src/program/verified.rs) evidence.
   It identifies the carrier module, native-import slot or shared-call function/offset,
   exact admitted NominalTy and its binder scope. Issue it only after the preceding
   checks and full graph validation. These are views of checked occurrences, not
   caller-authored claims or a second portable proof language. Immutable verified
   bytes own the witness; editable extraction loses it and identity remapping must
   remap/revalidate it with its owner. Existing imports/SharedCall records carry the
   necessary inputs, so no new serialized conformance flag or version bump is needed.
5. Runtime links that witness with the existing selected environment/type scopes.
   A contextual result_native_type consumes the proof for its exact native result;
   symbolic applications can be instantiated only by their already checked caller
   environment. A bounded NativeType may also be recovered from an already validated
   Opaque object. prepare_native_type must not manufacture bounded admission from an
   arbitrary TypeArgument. Keep owner/slot/generation/Rust TypeId and layout checks.
6. Host bind_native_type searches the installed verified occurrences for the exact
   closed application, matching each argument with supplying TypeArgument/TypeView
   scope, not only Ty equality. It retains the witness carrier through the existing
   program/type retention owner. Open witnesses, missing bounds, absent applications,
   unrelated dependency closures and foreign runtimes reject. No HIR, impl search,
   generic specialization or ProofCatalog query occurs at host binding/execution.
7. Host bind_associated_function(&native_type, "new") then uses existing
   AppliedReceiver/member matching, EntryEvidence and FunctionCache. A native type
   witness does not grant a callable new application: callable evidence must also
   exist. Function::prepare_call selects the computation normally. On publication,
   retained relationships select current compatible bodies while prepared calls pin
   exact owners; old type evidence cannot silently borrow a newer argument scope.

For example, a checked closed call to HashMap<Key, i32>::new proves Key's declared
Eq+Hash constraints and carries its selected hash/eq calls. Host preparation matches
that application, then invokes the same new entry. A table with matching method
names, an open HashMap<K,V> declaration, Hash+PartialEq without Eq, or an unseen
HashMap<Other,i32> application is insufficient. A closed from_iter application can
admit the type without proving that a host-callable new application was emitted.

Selected creation signature:

```text
NativeType<P>::create(&self, cx: &mut NativeContext, payload: P,
                      selected_slots: &[usize]) -> NativeResult<Opaque<P>>
```

Use `cx.result_native_type::<P>()?.create(cx, payload, &[hash_slot, eq_slot])` when a
declared constructor needs an independently owned factory. Immediate raw result creation
instead uses `cx.allocate_result_payload(&[hash_slot, eq_slot], builder)`, with exact
result occurrence/mapping/publication preflight before construction and selected-slot
resolution afterward. Both paths use the common NativeStorage allocation kernel, not
separate scanning/admission semantics. Use `&[]` when no selections are needed;
nonempty slots belong to the checked invocation. The
[immediate result contract](spec/host-interop.md#immediate-native-result-construction)
defines builder errors, completed effects, callback roots and terminal raw handoff. The object stores StoredSelection metadata edges,
not external program/root leases inside P. PG01 removes `cx.create_opaque`;
PG03 removes special Map/Set constructor resolution. Host convenience
factories invoke the checked
associated function; they cannot invent constructor selections from a bare type.

Expose selected operations through existing Function/PreparedFunction owners:
NativeContext::selected_function<A,R>(slot) and
Opaque<P>::stored_function<A,R>(cx, slot) prepare typed relationship handles using
the existing selected-call implementation. A stored relationship selects current
bodies per ordinary invocation; a sort prepares once for its active computation.
PG01 changes low-level `call_stored_values` to preserve its `RootedValue` result,
rather than extracting Value and immediately dropping the root. Other raw result
transport remains only under the audited no-safepoint handoff contract; ordinary
safe helpers return retained results. Do not create another StoredFunction registry.

Preserve Vec new/from(Array), HashMap/HashSet new, HashSet from(Array), Default and
FromIterator/collect through their existing declared requirements/traversal. There
is no new HashMap from(Array) API. Owned Rust collection conversions move through
library-owned adapters/newtypes: runtime-trait impls for std::vec::Vec cannot simply
move into stdlib under Rust coherence. Interface adapters invoke selected methods,
not payload downcasts. Preserve owned-conversion snapshots before child converters
can mutate the source; these are not sort-admission snapshots. Script map remove
prepares its Option result before removal, while host removal remains committed if
subsequent user result conversion fails.

### Selected indexed hash layout

Move HashMap/HashSet policy into the library, replacing reference-bearing std hash
entries with dense indexable entry slots. A map entry owns key/value/cached script
hash; a set entry owns key/hash. A prehashed Rust HashMap maps each complete cached
script hash to one head index. Entry-owned previous/next indices form collision
chains; there is no per-bucket Vec. One `HashMap::Entry` lookup updates the bucket.
Back-links permit dense swap relocation without rescanning all entries, while
preserving callback candidate order. HashSet shares this key/index kernel, not
a second runtime Set storage category. This is a deliberate layout adjustment from
HashMap<MapKey,Value>/HashSet<MapKey>, not a claim that relocation alone enables
resumable scans or that Rust's opaque bucket capacity is exactly measurable.

The scanner captures entry count and advances a numeric entry/field position.
Insertion reserves entry and bucket capacity before commit and publishes key/value.
Removal swaps the last dense entry into the hole, repairs that entry's bucket index
and republishes its edges even if the destination is behind the scan cursor. While
marking, count each inspected entry/edge against work; no retained std iterator or
iter().skip reconstruction is used. Capacity growth does not change logical indices.
Keep capacity accounting incrementally, with actual dense-entry Vec capacity and
documented std hash-table estimates. No per-element external roots.

Lookup uses cached hashes to choose candidates, retains each candidate before
ending a short loan and calls selected Eq outside it under ReadOnlyCallbacks.
Structural edits invalidate cursors; replacement of an existing value follows the
existing nonstructural policy. Commit updates entry and index consistently without
allocating or invoking script. Primitive hashing/equality fast paths require checked
selected-primitive evidence, not a key-type/name shortcut. Preserve collisions,
last-value-wins, deduplication and no-repeat callback/error ordering. Native cursors
retain the source/revision and positions through ordinary payloads; unspecified hash
iteration order is not made stable by this layout. Measure primitive-key, collision
and churn costs under the shared baseline; no throughput parity is claimed.

## Runtime capabilities and safety

The selected contracts above close the NC01 API decisions. The following inventory
tracks their locally accepted PG01/NC02 implementation and remaining migration
owners. Later collection and incremental acceptance is not implied by that result.

### Proven capability gaps, not replacement subsystems

- Dynamic edits/accounting: unsafe [NativePayload](../crates/kagari-runtime/src/native/storage.rs)
  now supplies infallible constant-time installed charge. Public StorageAccess and
  EditPlan preflight retention/publication, logical accounting and revisions; edit
  finalization records actual backing on success/error/unwind. General safe access
  remains [fixed NativeData](../crates/kagari-runtime/src/native/payload/data.rs).
  PG04 supplies locally accepted graph shading under synchronous draining;
  production interleaving and joint acceptance remain PG05/PG06 work.
- Operation protection: participating general access checks now enforce
  ReadOnlyCallbacks/ExclusiveSlots, and operation guards own preflighted sealed
  recovery outside payload loans. The external fixture exercises callback alias
  rejection and same-payload scratch recovery; standard sorting now uses this owner.
- Scan/admission: one `begin_scan`/`scan` protocol supports EdgeFree, Bounded,
  Resumable and Indivisible providers, with an admission-reserved aligned workspace.
  The current driver drains synchronously. PG04 now preserves active Copy state
  across workspace growth and supplies graph publication, with PG04 locally complete. PG03 removed the old sequence-edit reference snapshots. New external
  payload admission still validates every edge;
  merely moving a filled buffer into Opaque is not traversal-free admission.
- Type/selected-call construction: `StorageContext::element_contract` is public,
  [NativeType::create](../crates/kagari-runtime/src/native/payload/mod.rs) accepts
  selected slots, and `call_stored_values` retains its result root. Bounded
  [native binding](../crates/kagari-runtime/src/native/payload/binding.rs) consumes
  verified occurrences rather than rejecting all bounds or searching traits at
  runtime. PG03 removes special Map/Set constructor binding and runtime-owned
  collection conversions. Rust coherence is satisfied by library-owned
  `VecValues<T>`, not impls of runtime traits for std containers in stdlib.
- Raw access safety: NativePayload and general raw payload access are now unsafe;
  safe shared/mutable typed loans require NativeData, including paired receivers.
  NativeTake alone is not shared-loan evidence. PG02 short loans pin the actual
  inline payload rather than detach a Box; public string/tuple/enum/closure views
  still cannot escape the loan. Internal scoped validation retains actual aggregate/
  type/owner checks. Physical placement does not relax lifetime, retention or
  accounting guarantees.

Reuse roots/conversion scopes and runtime-owned NativeObject revisions throughout.
NC02's public-only fixture exercises these signatures without private imports;
its evidence and remaining acceptance are recorded below. This is not a new phase,
physical layout or permission to drop validation.

| Capability | Required contract |
| --- | --- |
| Native object handle | Check runtime ownership, generation, nominal application and installed payload mapping. Interfaces never grant arbitrary payload downcasts. |
| Dynamic construction/mutation | Validate stored types and heap ownership before publication; prepare allocation/accounting before mutation, update accounting on growth/shrink and preserve required write barriers. |
| Scoped access | References cannot escape; no storage borrow spans GC or callback/reentry. Safe APIs cannot overwrite traced slots with unchecked values. |
| GC edges/roots | Trace stored values and call dependencies; root reads/removals/candidates across allocations or callbacks; deterministically release failed-construction and temporary roots. |
| Retained calls | Unify allocation's existing selected-slot mechanism under NativeType::create and preserve RootedValue from call_stored_values. Keep signatures/generations, metadata edges versus external roots and ordinary current-body selection; no borrowed CallContext or repeated name lookup. |
| Operation guards and iterator validity | Preserve short native borrow, hashing and mutation-commit guards. Libraries own structural-edit/cursor policy through checked access to the runtime-owned native revision; writes commit and stale reads trap. Define public edit/commit capabilities without a second counter or lifetime iteration leases. Source/callback retention uses ordinary GC tracing; ordinary calls select current bodies while retained computations keep exact owners. |

Do not expose arbitrary safe &mut P for reference-bearing or variable-allocation
payloads. Reuse/extend managed storage or define a bounded audited storage-provider
contract with explicit obligations. Making private heap methods public or accepting
an empty trace does not establish safety. External implementers must satisfy the
contract without runtime-private imports, whitelists or disabled validation.

For map.insert(key, value), preserve this logical sequence:

1. Evaluate receiver/key/value once, left to right; validate the native boundary.
2. Acquire key-operation protection and call selected Hash.
3. Retain a candidate under scoped access before ending the loan.
4. Invoke selected Eq outside that borrow; reject conflicting reentry and preserve
   completed effects. Continue collision search with cancellation checks.
5. Prepare result/allocation requirements while protection keeps the candidate
   valid; transition directly from lookup protection into the non-reentrant commit
   as specified above. Release temporary roots/protection on every exit.

Preserve trap/commit ordering, including allocation before destructive removal
where required. No retries that silently repeat user callbacks. Rust Value Hash/Eq
cannot substitute for Kagari operations. Primitive fast paths require evidence of
the selected primitive implementation, not just a scalar key type. Stored key
hash/equality stability remains the user's existing obligation.

Vec retains bounds/slot-update ordering, aliases, compact buffers and sort/edit
lease guarantees. Failed in-place algorithms preserve their specified completed
effects and element-multiset guarantees; do not add rollback semantics.

Iteration preserves declared order, shared progress, structural mutation rules and
live shallow views. Structural edits commit and invalidate subsequent source reads;
short-circuit consumers do not force an extra read. Release temporary roots on
completion and failure, without introducing cursor lifetime leases.

## Consumers and optimizations

Inventory type substitution/formatting, interface coercions, executable schemas,
codecs, bytecode verification, lowering, host ScriptVec/ScriptMap/ScriptSet
adapters, source revision checks, reflection/tooling and artifact producers. Separate
concrete native handles from generic List/Map/Set handles: interface adapters
invoke selected methods rather than infer storage from the trait name.

Review prior optimizations. Retain compact buffers and validated primitive hashing;
remove repeated preparation/checks only when their guarantees have a new owner.
Exact standard-Vec fast paths may remain as library paths with checked identity
and call evidence. Opaque, GcHandle or a buffer capability alone never
authorizes bypassing custom iteration or overrides. Do not add caches or blanket
boxed storage merely to simplify migration.

NC01 captures a small reproducible baseline for scalar Vec traversal/editing,
reference-bearing Vec and primitive/script-key HashMap operations. NC06 repeats it
with toolchain, machine, profile/features, parallelism, cache state and workload
recorded. Separate preparation from execution. Investigate material regressions
through ownership/data-flow review and record measured tradeoffs. This is not a
new speed target or the Lua parity workstream; do not repeatedly run full benchmarks.

## Sequencing and phases

The [Iterable APIs](spec/builtins.md#iteration-protocols) and
[retained native cursors](spec/standard-declarations.md#sorting-and-lazy-iteration)
are implemented at this baseline; NC01/NC02 and PG02 are locally complete. PG03
and NC03/NC04 are locally complete after final independent review and focused/paired evidence;
NC05/NC06 and interleaved incremental acceptance remain outstanding.
Use the current [scoped identity APIs](architecture.md#definition-identity-ownership).
NC01 audited those APIs; reuse that inventory rather than rebuilding path/ID variants
or treating historical baseline signatures as a compatibility requirement.
Within the joint GC/opaque/collection migration, collection work begins with NC01,
using the existing retained slots, adapters, terminal consumers and FromIterator
implementations. Do not add another Iterable phase or make the trait surface
depend on this storage migration. Reuse implemented host registration/scoped
borrowing; the roadmap owns subsequent activation changes.

The [joint GC execution plan](paged-incremental-gc-design.md#execution-plan) owns
PG00-PG06 and the collector activation order. NC01 pairs with PG00, NC02 with PG01,
and NC03/NC04 execute at PG03 after the paged-storage checkpoint PG02. NC05 closes
the early external fixture under PG05's actual incremental schedules; NC06 joins
PG06 acceptance. PG04 integrates marking/publication between those handoffs.
NC02 may prove the public protocol with the synchronous collector and isolated
scanner tests, but cannot claim interleaved GC acceptance. Record collection
results only in this ledger; link PG results rather than copy their checklist.
The user authorized this phase sequence at the activation revision. PG00/NC01 and
PG01/NC02, PG02, PG03/NC03-NC04 and PG04 have passed local acceptance. PG04 is committed
at `fe56b675`; its fresh/reused evidence is owned by the PG ledger. PG05/NC05 is in
progress with integrated code, not locally complete.
Cancellation policy and cx.poll redesign remain outside the migration.

| Phase | Scope and exit condition |
| --- | --- |
| NC01: inventory and API contract | Audit at the activation revision and select the public protocol, bounded native-application proof chain, allocation-free cleanup work reservation, indexed hash layout and retained MutableList::set bridge. Locally complete with the shared PG00 baseline/design acceptance; NC02 supplies the external implementation proof. No new generic Rust payload mechanism. |
| NC02: general storage capabilities | Implement unified opaque registration and the identified generic capabilities under existing root/type/revision/call owners. Build an early separate-crate prototype before declaring the API sufficient: reference-bearing storage, dynamic edits/accounting, constructor/host calls, protected callbacks, scan admission and existing adapters with invalidation/failure cleanup. No collection-specific privileges or second guard/selected-call subsystem. |
| NC03: Vec migration | Move storage, operations, base cursor and host adapters through the independently usable public boundary; remove Sequence/Vec primitive/link/execution paths while retaining builtin Array. Preserve compact buffers, checked element evidence, defaults, indexing and pipelines. Prove stable worst-case O(n log n) sorting without retention/index prepass and linear retain/dedup, separating actual GC work. |
| NC04: HashMap/HashSet migration | Move nominal storage, table/key policy, cursors and host adapters through the same public generic boundary; remove special type/value/constructor/layout/codecs and host binding branches. Rebind FromIterator requirements. Preserve collisions, last-value-wins/dedup, protected Hash/Eq, cleanup and publication semantics; generic constructor evidence must work for script and host paths. |
| NC05: external extensibility and cursors | Complete the NC02 external prototype as a public-API-only BTreeMap<i32, T> fixture with Map/MutableMap/Iterable, reference-bearing values and revisions. Close cursor consumers; exercise adapters/terminals and retained script comparisons without core enum changes or new defaults. Recheck that standard collections use the same capabilities. No production ordered collection required. |
| NC06: final integration | Close every removal-inventory row and all script, host, conversion, source-free and optimized paths on the same public generic APIs. No runtime-owned collection facade, private-import exception or special codec/primitive may remain. Update docs/examples/tooling, batch artifacts, repeat focused measurements and final local checks; report CI separately. |

The external BTreeMap fixture can use Rust integer ordering because its keys are
fixed to i32. This does not prove arbitrary script Ord can be passed directly to
std::collections::BTreeMap: generic ordered storage needs a callback-compatible
library algorithm. The separate comparison proof validates the runtime capability
without adding that algorithm's production implementation to this task.

Default to buildable checkpoints; group declaration/execution/caller changes when
inseparable. Attempt necessary intermediate checks and record failures with command,
representative diagnostic, cause and owning phase here and in commit bodies. No
weakened assertions or production stubs. Temporary bridges need explicit removal
by NC06. Replace unpublished APIs directly; retain no compatibility aliases,
forwarding implementations or old-format readers. Keep ABI/format identifiers
unless publication or explicit compatibility requirements call for a bump.
Implementation commits use `Plan: native-collections-plan NC0N` trailers.

## Verification and acceptance

Reuse existing semantic owners, extending only distinct missing contracts:

| Coverage owner | Required evidence |
| --- | --- |
| [Interfaces](../crates/kagari/src/runtime/tests/language/collection_interfaces.rs), [access](../crates/kagari/src/runtime/tests/language/collection_access.rs), [arrays](../crates/kagari/src/runtime/tests/language/array_operations.rs) | Nominal distinction, generic/interface dispatch, invariance, shallow views, aliases, bounds, Array separation and once-only mutation targets. |
| [Iteration](../crates/kagari/src/runtime/tests/language/iteration_traits.rs), [conversions](../crates/kagari/src/runtime/tests/language/conversion_traits.rs), [library collections](../crates/kagari-vm/src/tests/library_collections.rs) | Preserve implemented adapter/terminal behavior while changing source/destination storage: generic/interface traversal, custom destination dispatch, HashMap last-value-wins, HashSet deduplication, early exit and shared progress. |
| [Lazy cursor boundaries](../crates/kagari-vm/src/tests/library_collections/library_mapping/mod.rs), [source-free pipeline fixture](../crates/kagari/tests/fixtures/feature_artifact.kgr) | Reuse retained_zip_uses_both_original_custom_sources_and_callbacks_after_reload and existing recursive-next/cycle/invalidation coverage. Prove migrated sources propagate through nested/paired adapters and source-free short-circuit consumption, without rebuilding a duplicate iterator suite. |
| [Native builder](../crates/kagari-stdlib/src/tests/runtime_boundary/native_builder.rs), [native boundary](../crates/kagari-vm/src/tests/native_boundary.rs) | Public extensions, wrong nominal/payload/type/owner rejection, dynamic edges, roots, reentry/traps/cancellation, cyclic collection cleanup and host retention. |
| [Reload](../crates/kagari/src/runtime/tests/embedding/generic_reload.rs), [artifacts](../crates/kagari/tests/artifact_features.rs), [native artifacts](../crates/kagari/src/program/tests/native_artifacts.rs) | Retained type/cursor and exact computation owners, current ordinary callback bodies, source-free linking, forged storage/call evidence rejection and backend agreement. |

Cover script key collisions and side effects, failures before commit, GC objects
returned by reads/removals, alias mutation during protected operations, cancellation
and post-error reuse. Consolidate existing cases rather than duplicate smoke suites.
Preserve source-free, host ownership and backend cases with distinct contracts.

Acceptance requires the actual standard Vec/HashMap/HashSet implementations and
host adapters to pass the independent-crate boundary, not merely a toy external
collection or pub collection-specific methods. Exercise new/from/from_iter/collect,
reads/edits, callbacks, iteration, host conversions, reload and source-free loading
through the same generic APIs. Reuse existing semantic tests for these paths; no
parallel acceptance suite or physical crate split is required. Ordinary opaque
declarations associate the Rust payload once, without
duplicate storage declarations or a new GC-specific public category. Reuse the
existing wrong-payload/type/owner and borrowed/retained lifetime contract coverage
when consolidating registration. The external fixture cannot import runtime-private
storage, add a compiler name check or receive special installation privileges. Classify remaining
collection branches by owner: library algorithms and tests are valid; concrete
storage semantics in HIR/type enums or compiler/backend dispatch are not. Document
justified generic buffer/Array helpers rather than relabeling special cases.

During implementation run affected focused tests, formatting, structural review,
`uv run --locked scripts/check_structure.py` and `git diff --check`. Review imports,
re-exports, visibility, effective LOC and responsibilities. NC06 also uses targeted
tests plus a small regression set for directly affected contracts, and full-workspace
Clippy locally (`cargo clippy --workspace --all-targets -- -D warnings`);
record the selection rationale and results. Batch repairs and rerun failed/affected
cases only, completing skipped coverage within that scope. Do not run workspace
suites locally or reconstruct them through package commands.

The following full sequence belongs to CI; structure, formatting, workspace Clippy
and diff checks also run locally. Workspace tests and exhaustive feature/backend
validation remain CI-only:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
cargo test -p kagari --no-default-features --test artifact_features
git diff --check
```

GitHub CI owns the full feature/backend matrix; local success does not establish
CI acceptance. Planning-only edits need content/link/diff checks, not builds.

## Progress and error ledger

- [x] Record direction, concrete API examples, scope and acceptance.
- [x] Rebase planning on 9cd90557 and distinguish completed IA capabilities from
  remaining collection storage/constructor/source-protection gaps.
- [x] Refresh native revision and interface storage owners after accepted OM,
  2026-10-18; NC implementation and activation remain separate.
- [x] Clarify ordinary Native GC handle operations versus collection-provider
  storage access and link the shared GC API contract, 2026-10-26.
- [x] Record the accepted opaque registration adjustment: preserve Ref/RefMut/Opaque
  categories, infer the underlying storage descriptor from one payload association,
  with NC01/NC02 detailing the API work within the GC replacement, 2026-10-26.
- [x] Couple opaque API and general payload changes with GC implementation;
  require removal of special collection collector/storage paths before joint
  acceptance, 2026-10-26.
- [x] Connect opaque registration, retained extraction and collecting callbacks
  through existing Native owners; superseded draft shapes are replaced by the
  selected provider/evidence contracts above.
- [x] Select interim unsafe opaque integration and unrestricted payload access;
  defer new NoGc/derive, split-type and lifetime-brand admission mechanisms. Retain
  safe rooted handles and audited library methods, with complete tracing, barrier,
  extraction and cleanup obligations, 2026-10-26.
- [x] Select tracing/incremental GC on 2026-10-26, superseding counted Value and
  cycle collection. Permit Value in unsafe Opaque providers, preserve safe retained
  Native handles and allow GC during Native callbacks.
- [x] Select existing roots, NativeType creation, unsafe scoped read/edit and
  retain/publication services; no with_storage or public removal transaction pair.
- [x] Select library-controlled temporaries for callback-bearing algorithms; remove
  the counted detached-buffer/Rust-sort conclusion. Preserve stable worst-case
  O(n log n), no retention prepass, receiver guards and callback/error semantics.
- [x] Record future operation families including Ord, predicates and custom iterators;
  per-item rooted iteration permits GC without whole-call exclusion. Incremental
  GC pause/progress validation remains separate from Native liveness correctness.
- [x] Record strict independent-stdlib acceptance, finite source removal inventory
  and evidenced capability gaps after architectural audit, 2026-10-26. Require the
  early NC02 external prototype and final same-path host/source-free acceptance.
- [x] Clarify Opaque<library payload> suffices, safe library functions/extensions are
  allowed, scalar layout stays library-owned and checked element evidence is reused.
  Retain MutableList::set as a checked declaration bridge, not a storage privilege;
  preserve no-prepass sort and Vec linear compaction requirements.
- [x] Link the PG00-PG06 execution order and synchronous-to-incremental handoffs;
  user activation at `50ab194b30b0218e9befe3176d6b5711ecf17a28` authorizes one commit
  per PG phase. This NC01 checkpoint edits this plan only; no production code changed.
- [x] Audit actual payload/typed-loan/root/selected-call/constructor/conversion and
  operation-protection paths. Select the contracts above, including verifier-owned
  native application witnesses, one admission-reserved pending stack, same-payload
  Vec scratch and library indexed hash entries. Sources and limitations are below.
- [x] NC01: Inventory and API contract.
- [x] NC02: General storage capabilities, locally complete with PG01.
- [x] NC03: Vec migration (locally complete with PG03).
- [x] NC04: HashMap/HashSet migration (locally complete with PG03).
- [ ] NC05: External extensibility and cursors (integrated stdlib/external immediate access;
  fresh representative root-built correctness/quality pass. Trusted PG05 replacement:
  ordinary 77 pass / 11 fail, observed normal 19 pass / 3 fail, 34 unobserved;
  fresh memory 55/55 and 21 zero teardowns. Numerical/joint acceptance remains open).
- [ ] NC06: Final integration.

NC01 is locally complete with PG00. The GC plan owns physical/pressure choices,
reproducible measurements, frozen gates and actual local validation; NC reuses its
Vec, primitive/script-key Map, reference-storage and callback baselines. PG00/NC01
was committed at `47be3cb7`; its baseline does not establish future speedup or CI
acceptance. Current NC05 progress, the PG03/NC03-NC04 handoff and historical
PG01/NC02 evidence follow.

### NC05 interleaved provider handoff (in progress)

PG04 is committed at `fe56b675`; PG05/NC05 is integrated but **not locally complete**.
The trusted actual-root replacement completed **112/112 timing processes**, all workload
semantic assertions passing: **77 ordinary pass / 11 fail; 19 observed normal pass / 3 fail;
34 unobserved rows are not passes. Fresh memory passes 55/55 with 21 zero-net teardowns.**
The [owning PG05 result table](paged-incremental-gc-design.md#pg05-trusted-replacement-results-and-finite-follow-up)
contains current failures/limits, normal same-family associations and follow-up; do not
maintain a second correction queue here. Normal failures affect mixed construction under
both policies and pressure scalar construction despite the correct 1024-work ceiling.

The selected 23/6 production interpretation is withdrawn for confirmed instrumented artifacts.
Earlier 78/10 ordinary and 22 observed-normal summaries are historical, not current acceptance.
The 36-path audit distinguishes 4 contaminated images, 6 verified baseline paths, 18
unverifiable images and 8 intentional probes; absence of markers alone is not complete
source-to-image proof. The trusted replacement uses two byte-identical actual-root clean
builds, dependency/marker/accessor checks, and separately rebuilt/audited memory diagnostics.
Fresh root-built validation passes 22 runtime, 6 VM and 4 SDK/provider cases, one compile-fail
doctest and 4 Miri cases; structure1819/Clippy/fmt/diff pass. This is focused local evidence,
not exhaustive CI or a fresh rerun of all earlier external tests.

The existing public OrderedMap fixture exercises actual automatic scanner/cursor/callback
schedules without private runtime access. Fresh SDK/provider scope includes constructor and
retained extraction, indexed scanner/adapters, callback alias/recovery, plus the source-free
algorithm/application boundary under default features. Earlier seven-fixture results remain
historical. Operation guards distinguish entry unwind from a new panic without clearing
existing poison; all-store borrowed-view preflight precedes resumed destructive service.

Implemented collection-facing contracts use the ordinary runtime owners:
- `allocate_result_payload(selected_slots, builder)` preflights mapping/authority before
  builder effects; callback adapters preserve original callback/root ordering.
- Fixed outer Option codecs move immutable nominal/provider proof to registration/link/
  application without removing child/scope/custom conversion or final-result checks.
- Unsafe immediate `NativeContext::read_payload` reuses the checked loan kernel. Stdlib and
  external cursors copy source/contract under a rooted parent, then read the child with no
  callback/collection gap and retain escaping results before loan end. Traced membership is
  a provider obligation, not a runtime-discovered projection capability. Hash paths share
  the same retained-entry body; revision/cancellation/result ordering remains intact.
- Scoped StorageType clones preserve lexical metadata without creating roots/leases; copied
  scalar retention does not change supplied RootedValue aliases or metadata.

The [host-interop contract](spec/host-interop.md#immediate-checked-payload-reads) owns safety;
[observation windows](spec/runtime.md#collector-observation-windows) own normal/assist/explicit
and indivisible reporting. Normal and explicit/assist use the same collector engine; raw
normal pauses include indivisibles. No collection-specific bypass or NoGc category is added.

Parent authorizes only 53's prepared-large consumption and sound vacant-word skip/charge
proposal, 820's common permission/preparation redundancy work, and 0a's normal-phase/observer
audit under the PG05 ledger. No gate/work-budget relaxation or backing/descriptor transfer.
Artifact provenance remains mandatory. **NC05 and NC06 stay unchecked**; joint numerical
closure and exhaustive CI/backend/platform acceptance are pending and unrun.

### NC03/NC04 implementation handoff (2026-10-27; locally complete)

The finite inventory above is closed at commit `8d597b93`: collections, cursors, host
adapters and Serde conversion policy live in stdlib; no concrete runtime type/value,
layout/constructor/codec, Vec primitive or compatibility facade remains. Script Vec
is unchanged; Rust owned conversion uses VecValues. The public OrderedMap fixture
uses the same generic protocol and passes seven selected tests, including recovery.
This does not complete its NC05 incremental schedule proof.

BorrowedPayload, fused edit preparation and call-argument publication keep checks
with the generic loan/root/type owners. General stable sort and linear compaction
keep traced main/scratch/progress in the payload and retain only active callback
arguments. The checked pointer-free scalar path uses bounded Rust stable-sort leaves
on unwind targets; callbacks never gain a noncollecting exemption. Hash storage uses
one prehashed scalar index with entry-owned collision chains for all key kinds,
checked Hash/Eq evidence, relocation publication and ordinary nominal cursors.

Opaque now retains data metadata separately from executable NativeType preparation.
The existing retention test passes 15 variants (five collections × raw root,
ScriptValue and owning adapter). Vec keeps all 40 old-module reclamation assertions
while the adapter lives, then reads/edits through a live context. Map/Set still trace
their selected Hash/Eq Program, even for intrinsic keys: zero old modules reclaim
while rooted, all reclaim after the last root drops. Do not infer Map/Set access
after old-program reclamation. The stored-key case distinguishes current ordinary
calls from an explicitly prepared old body. Final independent review approved this
scope distinction; no broader selected-call ownership redesign or stronger Map/Set
reclamation coverage is claimed.

The [PG03 handoff](paged-incremental-gc-design.md#pg03-collection-migration-evidence-and-handoff)
records exact commands, failed iterations, primary mechanisms, latest seven-pair
measurements and all 55 memory/21 teardown checks. Those selected gates pass.
Final independent review approved the current collection/provider, retention, scalar
sorting, scoped-evidence and disposal boundaries. Independent reruns passed stdlib
collections 5, ownership/page-quarantine 7 and retention 15 variants; parent structure
(1766 files, zero violations), formatting, workspace Clippy and diff checks passed.
These are repeated contract checks, not additional distinct-test totals.
NC03/NC04 are locally complete with PG03 at `8d597b93`. The
[PG04 ledger](paged-incremental-gc-design.md#pg04-marker-implementation-and-open-acceptance)
owns the locally complete checkpoint `fe56b675`: synchronously drained marker, 128-slot
owner-block queue, physical sweep, 32-total bounded disposal and module/frame
corrections. Final Map/arrays paired gates pass; six sort/construction and older
script-key/GC results retain their prior provenance. Authenticated memory reuse is
55/55 with 21 zero-net teardowns, not a fresh probe. Known blockers are fixed and
parent final local checks passed for PG04. PG05/NC05 is now in progress; PG06/NC06 and full CI remain
outstanding. No parallel collection correction queue is added.

### NC02 implementation handoff (2026-10-27; locally complete)

**Historical NC02 checkpoint with PG01 (`fc651b92`):** this section records the
early protocol/fixture proof, not current collection-migration status. It implemented
the unsafe provider/scanner/charge protocol,
`define_opaque` association, selected-slot creation, unsafe general loans, safe
NativeData access, checked retained extraction/publication, operation recovery and
verified bounded-application admission. Current semantic contracts now live in
[host interop](spec/host-interop.md#general-native-payload-protocol) and the
[native registration guide](native-registration.md#exports-documentation-and-storage).
The [PG01 ledger](paged-incremental-gc-design.md#progress-and-error-ledger) owns
runtime/root/verifier integration results and synchronous-to-incremental adaptations.

The external [native_storage_provider fixture](../crates/kagari/tests/native_storage_provider.rs)
and [ordered_provider modules](../crates/kagari/tests/ordered_provider/mod.rs) implement
OrderedMap through public Map, MutableMap and Iterable declarations, with an ordinary
opaque cursor. The implementation agent reports **six passing tests** covering
constructor/host creation and retained extraction; checked publication and committed
error/unwind edits; collecting callbacks, alias protection and allocation-free scratch
recovery; indexed continuation and existing iteration adapters; closure cycles and
retained results without per-element roots; and aggregate validation during loans.
These results are reported, not rerun by this documentation pass. They establish an
early public-provider fixture, not migration of standard Vec/HashMap/HashSet or
integrated incremental GC acceptance.

NC02 is locally complete with PG01. The parent reports successful manual fixture
regeneration (no tracked artifact diff), three actual no-default-features artifact
tests, three focused callback/reload regressions and final workspace Clippy. The
reviewer reports the additional schema NativeData diagnostic test and benchmark
diagnostics compilation passing; structure (1751 files, zero findings), formatting
and diff checks also passed. Exact test names and the repaired view SIGABRT,
operation-guard bypass, HIR/compiler bounds and diagnostic-panic history are owned
by the [PG01 handoff](paged-incremental-gc-design.md#pg01-implementation-handoff-2026-10-27-locally-complete),
not inferred from the earlier verifier results.

PG02 subsequently replaced the temporary boxed-loan storage path; its
[physical-model correction and evidence](paged-incremental-gc-design.md#pg02-physical-model-correction-and-checkpoint-evidence)
passed final local acceptance. PG03/NC03-NC04 then removed legacy collection codecs,
special type/value/wire paths, snapshots, constructor resolution and runtime-owned
host adapters, with selected paired/backing acceptance recorded in the PG03 handoff.
Current VecStorage keeps inline `Option<Work>` and same-payload main/scratch buffers.
PG04 implements active scanner continuation/publication reservations beyond idle
workspace growth; PG05/NC05 owns actual driver interleaving. Full CI and future
incremental performance/peak/pause acceptance remain unrun; historical NC02/PG02
results alone do not establish those gates.

Remaining real gates, not alternative API drafts:

- PG05/PG06: preserve PG04's locally accepted pending-capacity/slack, continuation
  workspace and bounded disposal under PG00's unchanged backing/peak gates as the
  driver opens production interleaving. Reservation remains a safety invariant,
  not a speedup claim; root-release transport is distinct from the marker queue.
- PG05/NC05: retain bounded admission, exact retention, charge/revision finalization
  and sealed recovery under actual driver schedules. PG04 covers active-workspace
  growth and graph-admission failures; recovery must continue to avoid allocation,
  ordinary execution readmission and element loss.
- PG05-PG06/NC05-NC06: extend PG03's locally accepted same-payload sorting/compaction
  and indexed hash relocation to actual interleaved partial scans. Preserve PG04's
  reviewed object/root/frame/metadata publication coverage and the closed collection
  removal inventory. Isolated scanner tests and
  synchronous callback/recovery evidence do not establish integrated incremental GC safety.

Keep these gates in the existing PG/NC ledgers; do not add parallel progress docs.

Planning refresh: inspected Iterable declarations/default requirements, adapter
bindings, retained-call APIs, destination construction and source revision validation
at 9cd90557. The old NativeCursor/mapping path is not a migration prerequisite.
Historical Iterable validation remains in Git; it does not establish NC acceptance.
Earlier test-cost measurements at 35de1818 remain historical and are
not a performance baseline for the rebased implementation.

Documentation-only validation uses content/link/diff checks. Shared baseline
measurements belong to the coordinating PG00 work, not a second NC benchmark run.
For implementation, select existing focused contract tests for payload admission,
borrow/reentry unwind, hash callbacks, checked host factories/reload, scoped generic
fields and retain/cursor failures. Extend the NC02 public-only fixture with distinct
precommit-failure, tiny-step relocation and allocation-free recovery cases. Full
workspace tests and the feature/backend/platform matrix remain CI-only.

## Reviewed sources and applicability

NC01 reviewed the following primary sources for the selected mechanisms. The
application proof chain above is grounded in Kagari's executable owners, not syntax
similarity to another language. PG owns allocator/page and measured memory decisions.

| Source/version | Mechanism used and differing assumptions |
| --- | --- |
| [gc-arena 0.7.0 context](https://docs.rs/crate/gc-arena/0.7.0/source/src/context.rs) | Forward barriers may shade the child without relying on parent color; gray deduplication and preserving unfinished work on trace panic are relevant. Its Vec-backed gray/gray-again queues grow normally and it uses branded pointers/whole-object tracing. Kagari selects admission-reserved capacity and forward-only publication to make cleanup enqueue nonallocating and avoid repeated large-parent scans. This reservation proof is an explicit Kagari adaptation, not an upstream guarantee. |
| [OpenJDK 25 GA SATB queue](https://raw.githubusercontent.com/openjdk/jdk/jdk-25-ga/src/hotspot/share/gc/shared/satbMarkQueue.hpp) | Explicit active-state, buffer completion/flush and abandoned-marking lifecycle demonstrate that barrier work needs a lifecycle owner. Thread-local concurrent SATB logging is not selected; Kagari's single driver can deduplicate identities before enqueue and reserve a finite pending capacity at admission. |
| Rust 1.99.1 [tiny stable mergesort](https://raw.githubusercontent.com/rust-lang/rust/1.99.1/library/core/src/slice/sort/stable/tiny.rs) and [merge/gap recovery](https://raw.githubusercontent.com/rust-lang/rust/1.99.1/library/core/src/slice/sort/stable/merge.rs) | Fixed-split stable mergesort and restoration of unconsumed scratch into a tracked gap supply the algorithm skeleton. Kagari must replace raw slice/pointer lifetimes with indices, scanner-visible same-payload scratch and per-transfer publication. The tiny kernel is nonadaptive; no parity with driftsort is inferred. |
| Rust 1.99.1 [stable sort driver](https://raw.githubusercontent.com/rust-lang/rust/1.99.1/library/core/src/slice/sort/stable/mod.rs) | Private MaybeUninit scratch/BufGuard does not provide move/publication hooks. Rooting only the receiver and two comparator arguments cannot make the unchanged kernel safe for collecting callbacks. |
| [V8 array sort](https://chromium.googlesource.com/v8/v8/+/refs/heads/main/third_party/v8/builtins/array-sort.tq), reviewed source blob `90e36b5cd08715c65f83bae28e96587534a406fe` | Managed work/temp arrays demonstrate collector-visible algorithm state. CompactReceiverElementsIntoWorkArray performs a full preprocessing copy, so its entry protocol is not adopted; Kagari scratch starts empty inside the existing payload. |

Rust's [nominal ADT model](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_type_ir/enum.TyKind.html)
and library [HashMap](https://doc.rust-lang.org/src/std/collections/hash/map.rs.html)/
[BTreeMap](https://doc.rust-lang.org/src/alloc/collections/btree/map.rs.html) remain
ownership references, not Kagari's runtime model. The indexed hash adaptation is
needed because ordinary borrowed std hash iterators offer no persistent numeric
scan position across mutations. Retain Rust's hash index for scalar bookkeeping;
Kagari's library owns reference slots, publication and callback-aware collision
handling. Do not infer a faster table or transfer Rust borrowing into script syntax.
