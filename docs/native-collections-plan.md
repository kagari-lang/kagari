# Library-owned native collections execution plan

Status: agreed direction recorded; implementation has not started. The
[roadmap](implementation-roadmap.md#library-owned-native-collections) owns queue
placement. This plan owns NC01-NC06, the bounded API design, migration acceptance
and progress ledger. API sketches are proposals, not currently callable APIs.

## Outcome and scope

Make Vec, HashMap and HashSet ordinary nominal native objects registered through
NativeModule. Their Rust library implementations own storage and algorithms;
declarations describe generic parameters, bounds, methods and trait implementations.
Adding an application collection must not require a new semantic type, runtime
value variant, compiler-recognized container layout or backend operation.

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
NC01-NC06 have not started.
Recheck implementation at NC01 without reopening the completed Iterable API work.

| Current owner | Evidence and disposition |
| --- | --- |
| [Stdlib declarations](../crates/kagari-stdlib/src/catalog/defaults.rs) | Vec is nominal but declares Sequence storage; HashMap/HashSet still use special native constructors. Move all three to ordinary native declarations. |
| [Shared native declarations](../crates/kagari-types/src/declaration/native.rs), [HIR application](../crates/kagari-hir/src/native.rs) | Collection constructor/layout variants leak implementation families into semantic modeling. Remove these dependencies coherently rather than rename them. |
| [Native storage](../crates/kagari-runtime/src/native/storage.rs), [type builder](../crates/kagari-runtime/src/native/builder/type_builder.rs) | Factories, payload bindings, tracing and scoped type arguments already exist. Extend these owners only for proven gaps. |
| [Managed fields](../crates/kagari-runtime/src/native/payload/managed/mod.rs), [fixed native data](../crates/kagari-runtime/src/native/payload/data.rs) | Safe edits separate fixed Rust data from checked script edges. Dynamic storage must preserve that boundary; arbitrary mutable payload access is not a solution. |
| [Sequences](../crates/kagari-runtime/src/native/sequence.rs), [hash payloads](../crates/kagari-runtime/src/native/hashed.rs), [tables](../crates/kagari-runtime/src/native/hash_storage.rs) | Runtime owns concrete storage and optimizations. Relocate collection policy while preserving useful storage foundations and checks. |
| [Hash bindings](../crates/kagari-stdlib/src/bindings/hash.rs), [codecs](../crates/kagari-runtime/src/native/binding.rs) | Methods depend on special value tags, heap operations and layout admission. Replace these with checked nominal access. |
| [Iterable declarations](../crates/kagari-stdlib/src/catalog/iterable.rs), [default requirements](../crates/kagari-stdlib/src/catalog/iterable_defaults.rs), [bindings](../crates/kagari-stdlib/src/bindings/iterable.rs) | Uniform defaults, Iterator identity adaptation and nominal lazy adapters are implemented. Reuse these algorithms and checked iter/next calls; the old Vec-only free map and NativeCursor SDK were removed. |
| [Call context](../crates/kagari-runtime/src/native/context.rs), [stored selections](../crates/kagari-runtime/src/native/stored_selection.rs) | Payload allocation retains selected calls as metadata edges and later invokes them through the rooted receiver. Reuse this mechanism for collection key calls; public constructor/factory ergonomics still need NC01 review. |
| [Construction requirements](../crates/kagari-stdlib/src/catalog/construction_defaults.rs), [construction bindings](../crates/kagari-stdlib/src/bindings/construction.rs) | Vec/HashMap/HashSet FromIterator and collect delegation already work. Hash construction selects insert, but still declares Ty::Map/Set and branches on Value::Map; migrate representation without replacing consumption semantics. |
| [Base cursors](../crates/kagari-runtime/src/gc/iter.rs) | Cursors check structural revisions on reads; wrappers retain ordinary traced edges, without iteration leases or consumer-scope traversal. Revision ownership for general library storage remains a native storage boundary. |

Today map insertion passes through Value::Map and GcHeap map operations into a
runtime-owned table. That path also enforces key/value typing, allocation
admission, custom Hash/Eq calls, lookup guards and mutation commits. Vec similarly
uses runtime sequence operations despite its nominal identity. These are real
responsibilities, not removable overhead.

The defect is coupling those responsibilities to concrete container families.
Runtime should own general checking/lifetime mechanisms, and libraries should
own storage and operation policy. Historical motivation is not inferred from
current code. This migration makes no unmeasured performance claim.

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

The existing public retention path is:

```rust
// During a checked native constructor, with declared callable requirements:
cx.allocate_result_payload(payload, &[hash_slot, eq_slot])?;
// During a later method, whose argument 0 roots that native receiver:
cx.call_stored_values(0, 0, &[key])?;
```

Allocation copies the chosen selected calls into object metadata slots, in the
supplied order. StoredSelection stays runtime-private; libraries do not construct
owning metadata roots inside payloads. StoredCallable remains the mechanism for
traced script closure values. These are existing capabilities, not new NC APIs.
They do not by themselves provide checked resizing, library-owned cursor revisions or a
public factory-level key-call registration contract.

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

Reuse ModuleBuilder, NativeStorage and NativePayload rather than adding a second
registration facade. These sketches show responsibilities; settle exact names,
signatures and binding ergonomics at NC01. Omitted method signatures and required
trait members must be supplied in implementation.

```rust
let mut declaration = module.define_type("Vec");
let t = declaration.type_parameter("T")?;
declaration.native_storage(NativeStorage::new(move |cx| {
    VecStorage::new(cx.type_argument(&t)?)
}))?;
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

VecStorage retains its checked element contract and native buffer. Keep compact
scalar storage where valid; object elements require traced values. A kgr generic
parameter cannot dynamically instantiate arbitrary Rust generic code. Select
storage from checked type arguments, never the first element or a textual name.

```rust
let mut declaration = module.define_type("HashMap");
let k = declaration.type_parameter("K")?;
let v = declaration.type_parameter("V")?;
declaration.bound(k.ty(), core.eq())?;
declaration.bound(k.ty(), core.hash())?;
declaration.native_storage(NativeStorage::payload::<HashMapStorage>())?;
let map_type = declaration.finish()?;
// Constructors declare checked Hash/PartialEq::eq requirements, create storage
// with checked key/value contracts, then use allocate_result_payload with those
// selected slots. Register Map/MutableMap/Iterable implementations normally.
```

HashMapStorage retains key/value contracts and its table; the containing native
object's metadata retains selected key operations. Eq remains the semantic bound;
the callable equality member is PartialEq::eq, as in current hash construction.
HashSet shares the key-operation machinery. Prefer checked constructor requirements
and existing retained slots rather than inventing a separate type-level resolver
or a cx.retain_call API. NC01 settles public builder support, slot ownership and
how new/from/from_iter and host conversion supply equivalent evidence. If a
factory path is required, record its precise gap before extending StorageContext.

Register Iterable Item/Iter and Iterator::next normally. Do not infer traversal
from physical storage. Preserve default/override dispatch through interfaces.
Constructors explicitly declare Array/Iterable inputs and use ordinary inference.

## Runtime capabilities and safety

NC01 records exact APIs and the safe/trusted boundary for each row; NC02 implements
only gaps required by these collections and the external proof.

| Capability | Required contract |
| --- | --- |
| Native object handle | Check runtime ownership, generation, nominal application and installed payload mapping. Interfaces never grant arbitrary payload downcasts. |
| Dynamic construction/mutation | Validate stored types and heap ownership before publication; prepare allocation/accounting before mutation, update accounting on growth/shrink and preserve required write barriers. |
| Scoped access | References cannot escape; no storage borrow spans GC or callback/reentry. Safe APIs cannot overwrite traced slots with unchecked values. |
| GC edges/roots | Trace stored values and call dependencies; root reads/removals/candidates across allocations or callbacks; deterministically release failed-construction and temporary roots. |
| Retained calls | Reuse allocate_result_payload selected slots and call_stored_values. Preserve signatures/generations and metadata edges versus external roots. Complete only constructor/host-path gaps; no borrowed CallContext or repeated name lookup. |
| Operation guards and iterator validity | Preserve short native borrow, hashing and mutation-commit guards. Libraries own cursor revision checks: structural writes commit and stale reads trap. Generalize revision ownership beyond builtin storage without reintroducing lifetime iteration leases. Source/callback retention uses ordinary GC tracing and pinned calls. |

Do not expose arbitrary safe &mut P for reference-bearing or variable-allocation
payloads. Reuse/extend managed storage or define a bounded audited storage-provider
contract with explicit obligations. Making private heap methods public or accepting
an empty trace does not establish safety. External implementers must satisfy the
contract without runtime-private imports, whitelists or disabled validation.

For map.insert(key, value), preserve this logical sequence:

1. Evaluate receiver/key/value once, left to right; validate the native boundary.
2. Acquire key-operation protection and call selected Hash.
3. Read/root a candidate under scoped access, then release the storage borrow.
4. Invoke selected Eq outside that borrow; reject conflicting reentry and preserve
   completed effects. Continue collision search with cancellation checks.
5. Prepare result/allocation requirements and commit while protection keeps the
   candidate valid. Release temporary roots/protection on every exit.

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
are implemented at this baseline; NC01-NC06 remain queued and unstarted.
Use the current [scoped identity APIs](architecture.md#definition-identity-ownership).
Refresh NC01 against those APIs instead of rebuilding path/ID variants or treating
the historical baseline signatures as a compatibility requirement.
Implementation begins with NC01,
using the existing retained slots, adapters, terminal consumers and FromIterator
implementations. Do not add another Iterable phase or make the trait surface
depend on this storage migration. Reuse implemented host registration/scoped
borrowing; the roadmap owns subsequent activation changes.

| Phase | Scope and exit condition |
| --- | --- |
| NC01: inventory and API contract | Trace dynamic edits, source revisions, factories, key calls, host conversions and artifacts against 9cd90557. Record exact reuse of retained slots/revision checks, constructor evidence, builtin cursor disposition, optimization review and baseline commands. Establish the external fixture and only the missing public capabilities. |
| NC02: general storage capabilities | Complete checked dynamic edits, allocation/accounting and nominal revision ownership. Extend retention only for proven constructor/host gaps. Demonstrate public-API-only reference-bearing storage participating in existing nested/paired adapters with invalidation and callback failure cleanup. No second guard or selected-call subsystem. |
| NC03: Vec migration | Move concrete storage/operation policy and its base cursor to the library; replace Sequence admission and consumers while retaining builtin Array support. Preserve defaults, indexing, sorting, existing FromIterator/pipelines, host adapters and compact buffers. Remove the obsolete parallel Vec path. |
| NC04: HashMap/HashSet migration | Register both as nominal objects; move table/key policy and base cursors to the library, replace special type/value/layout branches and migrate host/contract/backend consumers. Rebind existing FromIterator insert/iter/next requirements to nominal types. Preserve collisions, last-value-wins/dedup rules, custom Hash/Eq, cleanup and pinned calls. |
| NC05: external extensibility and cursors | Close remaining collection cursor consumers and prove a public-API-only BTreeMap<i32, T> fixture implements Map/MutableMap/Iterable with reference-bearing values and source revision checks. Exercise existing adapters and collection terminals without core enum changes or new default algorithms. Separately prove retained script comparison calls, not only Hash/Eq. No production ordered collection required. |
| NC06: final integration | Remove obsolete paths/bridges, update implemented docs/examples/tooling, refresh affected artifacts once, complete host/source-free acceptance, repeat focused measurements and run final local checks. Report CI separately. |

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
| [Interfaces](../crates/kagari/tests/collection_interfaces.rs), [access](../crates/kagari/tests/collection_access.rs), [arrays](../crates/kagari/tests/array_operations.rs) | Nominal distinction, generic/interface dispatch, invariance, shallow views, aliases, bounds, Array separation and once-only mutation targets. |
| [Iteration](../crates/kagari/tests/iteration_traits.rs), [conversions](../crates/kagari/tests/conversion_traits.rs), [library collections](../crates/kagari-vm/tests/library_collections.rs) | Preserve implemented adapter/terminal behavior while changing source/destination storage: generic/interface traversal, custom destination dispatch, HashMap last-value-wins, HashSet deduplication, early exit and shared progress. |
| [Lazy cursor boundaries](../crates/kagari-vm/tests/library_mapping/mod.rs), [source-free pipeline fixture](../crates/kagari/tests/fixtures/feature_artifact.kgr) | Reuse retained_zip_uses_both_original_custom_sources_and_callbacks_after_reload and existing recursive-next/cycle/invalidation coverage. Prove migrated sources propagate through nested/paired adapters and source-free short-circuit consumption, without rebuilding a duplicate iterator suite. |
| [Native builder](../crates/kagari-runtime/tests/native_builder.rs), [native boundary](../crates/kagari-vm/tests/native_boundary.rs) | Public extensions, wrong nominal/payload/type/owner rejection, dynamic edges, roots, reentry/traps/cancellation, cyclic collection cleanup and host retention. |
| [Reload](../crates/kagari/tests/generic_reload.rs), [artifacts](../crates/kagari/tests/artifact_features.rs), [native artifacts](../crates/kagari/tests/native_artifacts.rs) | Pinned types/callbacks/cursors, source-free linking, forged storage/call evidence rejection and backend agreement. |

Cover script key collisions and side effects, failures before commit, GC objects
returned by reads/removals, alias mutation during protected operations, cancellation
and post-error reuse. Consolidate existing cases rather than duplicate smoke suites.
Preserve source-free, host ownership and backend cases with distinct contracts.

Acceptance requires all three standard collections to use the public extension
path. The external fixture cannot import runtime-private storage, add a compiler
name check or receive special installation privileges. Classify remaining
collection branches by owner: library algorithms and tests are valid; concrete
storage semantics in HIR/type enums or compiler/backend dispatch are not. Document
justified generic buffer/Array helpers rather than relabeling special cases.

During implementation run affected focused tests, formatting, structural review,
`uv run --locked scripts/check_structure.py` and `git diff --check`. Review imports,
re-exports, visibility, effective LOC and responsibilities. NC06 also uses targeted
tests and scoped Clippy plus a small regression set for directly affected contracts;
record the selection rationale and results. Batch repairs and rerun failed/affected
cases only, completing skipped coverage within that scope. Do not run workspace
suites locally or reconstruct them through package commands.

The following full validation belongs to CI, not local NC06 acceptance:

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
- [ ] NC01: Inventory and API contract.
- [ ] NC02: General storage capabilities.
- [ ] NC03: Vec migration.
- [ ] NC04: HashMap/HashSet migration.
- [ ] NC05: External extensibility and cursors.
- [ ] NC06: Final integration.

No implementation or performance claims are made at this checkpoint. No intermediate
failure is carried. Exact API names and the NC01 decisions remain implementation
design work, not acceptance waivers. Update this ledger rather than create a
parallel progress/review queue.

Planning refresh: inspected Iterable declarations/default requirements, adapter
bindings, retained-call APIs, destination construction and source revision validation
at 9cd90557. The old NativeCursor/mapping path is not a migration prerequisite.
Historical Iterable validation remains in Git; it does not establish NC acceptance.
Earlier test-cost measurements at 35de1818 remain historical and are
not a performance baseline for the rebased implementation.

Documentation-only validation uses content/link/diff checks; implementation
measurements begin at NC01.

Design reference: Rust models ordinary library structs through
[TyKind::Adt](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_type_ir/enum.TyKind.html);
[HashMap](https://doc.rust-lang.org/src/std/collections/hash/map.rs.html) and
[BTreeMap](https://doc.rust-lang.org/src/alloc/collections/btree/map.rs.html) are
library implementations. Kagari adopts that ownership boundary with precompiled
native entries, traced identity and checked callbacks; it does not import Rust's
script-visible borrowing model or promise identical storage/call costs.
