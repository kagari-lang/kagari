# Collection interface ownership execution plan

Status: active execution plan for [SA18](review.md#sa18-collection-operations-should-be-constrained-by-trait-interfaces).
Execution authorized on 2026-10-12. CI01-CI02 are complete; CI03 is in progress. The [roadmap](implementation-roadmap.md#review-follow-up-order)
owns queue placement. This document owns CI01-CI06, the activation decisions,
acceptance and progress ledger. CI phase IDs here mean collection interfaces;
GitHub CI acceptance is recorded separately.

## Execution handoff (2026-10-12)

Resume from implementation checkpoint `93909724`; the progress ledger below owns
completed changes and their validation evidence. CI01-CI02 are complete and CI03
remains active. CI04 has implementation checkpoints but still needs its phase
acceptance audit; CI05 marker removal and CI06 documentation/final acceptance
remain open. Do not restart completed migrations or treat partial CI04 work as
completion of the overall plan.

The next implementation step is checked conditional operation composition for
`Vec<(old Local, new Item)>` across reload. Retain the old Local operation and the
new Item operation independently. Reuse the checked shared receiver selections
from `93909724`; whole closed-application lookup cannot supply this mixed tuple.
Keep operation selection and structural adapter proof in compiler/contract
owners, with runtime binding to the actual arguments' supplying generations.
Do not add runtime trait search or substitute layout equality for generation
identity.

The two enabled reproductions currently fail with
"interface receiver does not support object comparison":

```text
cargo test -p kagari-embed --test generic_reload shared_collection_comparison_composes -- --nocapture
```

They separately cover the retained Local operation and the new Item operation.
The latest checkpoint passed 11 selected contract cases and scoped library
Clippy; exact coverage and earlier evidence are recorded in the ledger. Continue
with affected tests and lightweight checks, leaving the exhaustive workspace and
backend matrix to CI. These two failures and the remaining phase work must be
resolved before declaring completion.

## Outcome and bounded scope

Make trait members, checked implementations and ordinary inheritance determine
collection operations. Remove `TraitDef.storage_access` and
`TraitBuilder::storage_view` after relocating every current consumer's guarantee.
An interface value retains its receiver and checked method witness; adapting an
interface does not change an object's mutable/immutable storage state.

Preserve the existing access and aliasing contract:

```text
val concrete = Vec::from([1, 2]);
val writable: MutableList<i32> = concrete;
val readable: List<i32> = writable;
writable.push(3);
readable.len(); // 3: the same underlying object.
readable[0];    // 1: an ordinary checked read.
// readable.push(4); // Rejected: no such method in List.
// readable[0] = 4;  // Rejected: no selected write operation.
// val upgrade: MutableList<i32> = readable; // Rejected.
```

`val`/`var` continue to govern rebinding. Readable interfaces remain shallow;
referenced elements and other aliases retain their own operations. Keep generic
arguments invariant and conversions one-way through declared parents. Preserve
native/script implementations, once-only evaluation and completed effects.

The accepted design uses ordinary protocol selection: stdlib explicitly implements
content PartialEq/Eq/Hash for Vec, HashSet and HashMap under their declared bounds.
Object `==`/`!=` remain available through List/Set/Map interfaces: invoke the
underlying object's applicable equality implementation, otherwise compare object
handles. Interface adaptation preserves this behavior. Explicit library functions
compare contents through collection interfaces, including across concrete
implementations. `===`/`!==` always compare object identity. Availability of object
comparison does not by itself establish Eq/Hash bounds or hash-key eligibility. These
are the target rules; implemented paths and remaining acceptance are distinguished
in the progress ledger. Current specifications remain the baseline until
implementation and documentation handoff are complete.

The task includes declaration/model/wire consumers, source analysis, executable
proofs, verification, linking, runtime view semantics and their documentation.
Audit `CollectionAccess` uses and remove its role as a second authority for
ordinary collection interfaces. Preserve concrete storage layout/tracing,
host capabilities, scoped borrows, roots, cancellation and supplying generations.

Separate work remains separate:

- NC01-NC06 owns moving Vec/HashMap/HashSet storage and existing algorithms to
  libraries, retiring concrete Map/Set type/value families and proving external
  storage. This plan owns the explicit comparison/hash implementations and three
  content comparison functions below; NC reuses them rather than reauthoring policy.
- SA21 owns Host/Native registration unification and any new borrowed input/return API.
- Variance, downcasts, a universal object superclass, new syntax and unrelated
  interface identity operations are outside this task.
- Builtin Array remains fixed-length and separate from Vec/List. No new Array-to-List
  conversion or MutableList implementation is introduced. Its identity-based
  equality/hash, ordinary non-collection Struct defaults and scalar floating-point
  semantics are unchanged by this collection-specific contract migration.

## Current-code inventory and root cause

The inventory below was inspected at `53949725`, with only the proposed review
ordering changed in the working tree. Recheck it at CI01; paths are owner locations,
not a complete list of all callers.

| Responsibility | Current owners and observed coupling | Replacement direction |
| --- | --- | --- |
| Authored and portable declarations | [TraitDef](../crates/kagari-types/src/declaration/mod.rs), [wire records](../crates/kagari-types/src/declaration/wire/declarations.rs), [builder](../crates/kagari-runtime/src/native/builder/trait_builder.rs), [stdlib collection traits](../crates/kagari-stdlib/src/catalog/collections.rs) | Ordinary declaration members/parents plus independently owned operation and installation evidence. Remove the field, builder API and all copies together at CI05. |
| Receiver matching | [shared matching](../crates/kagari-types/src/ty/matching.rs), [contract matching](../crates/kagari-contract/src/types/matching.rs), [HIR inference](../crates/kagari-hir/src/typeck/inference.rs) | Invariant implementation substitution and checked interface relationships. Current matching permits outer access weakening for a readonly marker or Index/Iterable; relocate necessary host adaptation explicitly rather than deleting its validation. |
| Common types and operand compatibility | [storage views](../crates/kagari-hir/src/aggregates/storage.rs), [body checking](../crates/kagari-hir/src/typeck/body.rs), [operators](../crates/kagari-hir/src/typeck/body/operators.rs) | Applied interface candidates from valid implementations and parent closures. Current concrete candidates are engine-owned and filtered by the readonly marker. |
| Protocol eligibility and member surfaces | [HIR implementations](../crates/kagari-hir/src/aggregates/implementations.rs), [language semantics](../crates/kagari-hir/src/language/semantics.rs), [members](../crates/kagari-hir/src/typeck/members.rs), [source-free proofs](../crates/kagari-contract/src/types/proofs/structural.rs) | Checked protocol/default evidence independent of read/write classification; HIR and executable proofs must agree. |
| Interface identity/equality/hash/format | [runtime value semantics](../crates/kagari-runtime/src/value_semantics.rs) and its callers | Preserve receiver identity for identity operators and explicit selected protocols for valid ordinary calls. Interface object equality follows the receiver; Eq/Hash bounds still require evidence. CI03 replaces the runtime marker scan with checked receiver-comparison evidence retained in the interface snapshot. |
| Implementation ownership | [HIR override checks](../crates/kagari-hir/src/aggregates/implementations.rs), [contract ownership](../crates/kagari-contract/src/types/proofs/ownership.rs), [receiver owners](../crates/kagari-types/src/declaration/ownership.rs) | Ordinary coherence/receiver ownership and explicit native/builtin registration authority. Removing the marker must not create uncontrolled downstream overrides. |
| Installation without calls | [installed trait checks](../crates/kagari-runtime/src/native/registry/dependencies.rs), [installation tests](../crates/kagari-runtime/tests/installation_access.rs) | Exact installed declaration/layout/witness requirements, including private records and modules with no native calls. An untrusted artifact cannot grant authority through a copied name or provenance flag. |
| Writable indexing and verification | [places](../crates/kagari-hir/src/typeck/body/places.rs), [compiler lowering](../crates/kagari-compiler/src/source/lower/expr.rs), [bytecode access](../crates/kagari-bytecode/src/access.rs) | Selected checked write operations and receiver evidence. Source checking and source-free verification reject the same upgrades and unauthorized writes. |
| Installed List indexing bindings | [native discovery](../crates/kagari-hir/src/native/paths.rs), [Array bridge](../crates/kagari-hir/src/builtin/array_bridge.rs) | Preserve the bounded installed List/MutableList member binding, using validated member/registration facts rather than storage_view discovery. No collection-name dispatch in generic algorithms. |
| Remaining access-tagged representations | [Ty access](../crates/kagari-types/src/ty/access.rs), [HIR types](../crates/kagari-hir/src/types.rs), [host value types](../crates/kagari-types/src/host_interface/value_type.rs), [host paths](../crates/kagari-types/src/host_interface/path.rs) | Classify script interface policy, host admission and legacy physical family support separately. The current Array type has no access parameter; Map/Set do. |

The architectural defect is the distribution of several independent guarantees
through one trait marker. No correctness failure or measured performance bottleneck
has been established. Replacing it with a collection-name check, a renamed access
flag or a second general collection capability table would retain that coupling.

## Accepted contracts and CI01 decisions before semantic edits

CI01 closes the following requirements through the execution decisions below,
with concrete examples and the exact consuming owners. Open choices are not permission to pick different
rules independently in HIR, artifact verification and runtime.

### Accepted collection equality, hashing and identity

The language applies its ordinary protocol rules to collections. Stdlib owns the
explicit implementations below; it does not delegate collection comparison policy
to engine recognition of List/Set/Map or concrete collection names. A third-party
collection follows the same registration, coherence and conditional-impl rules.
Merely implementing a collection interface imposes no object equality/hash policy.

| Concrete stdlib content implementation | PartialEq (`==` / `!=`) | Eq | Hash |
| --- | --- | --- | --- |
| Vec<T> | T: PartialEq | T: Eq | T: Eq + Hash |
| HashSet<T> | T: Eq + Hash | T: Eq + Hash | T: Eq + Hash |
| HashMap<K, V> | K: Eq + Hash, V: PartialEq | K: Eq + Hash, V: Eq | K: Eq + Hash, V: Eq + Hash |

HashSet elements and HashMap keys already require Eq + Hash for concrete storage
lookup. The table gives the bounds for the explicit content implementations, not
all protocol availability. Vec and HashMap values remain storable when those bounds
are not met. Select an applicable explicit implementation first; otherwise use the
type's eligible builtin default, including object identity where applicable. A
conditional content implementation whose bounds are not met does not by itself
forbid that default. Once explicit PartialEq applies, however, missing Eq/Hash must
not be supplied by identity defaults: Vec<f32> has content PartialEq but no Eq/Hash.
Use the same selected implementation for a concrete instantiation in direct calls,
generic code and containers, including after collection interface adaptation.
Object comparison is available through those interfaces without an explicit
PartialEq bound on the interface: invoke the receiver's applicable equality or
use object-handle comparison when none applies. This does not establish Eq/Hash
bounds, add equality members to the interface declaration or change scalar rules.
The existing [selection rule](spec/value-semantics.md#one-implementation-selection-rule)
is the baseline; retaining receiver equality through these interfaces is a target
contract of this plan, not a claim that the current implementation already does so.

| Operation or boundary | Accepted behavior |
| --- | --- |
| Vec PartialEq | Equal lengths and equal elements at corresponding indices; order matters. Compare elements left to right and stop on the first unequal element. |
| HashSet PartialEq | Equal cardinality and the same elements under the selected element Eq; insertion and traversal order do not matter. |
| HashMap PartialEq | The same keys under the selected key Eq, with equal values for each key; insertion and traversal order do not matter. |
| Concrete Hash | Vec combines element hashes in order. HashSet combines element hashes independently of order. HashMap combines each key/value pair, then entries independently of order. Equal values under the selected Eq must hash equally. |
| Plain List/MutableList, Set/MutableSet, Map/MutableMap | `==`/`!=` compare underlying objects using their applicable equality implementation, otherwise object handles. Wrapping or upcasting must neither reject comparison nor replace content equality with identity. This operator availability alone does not authorize `.hash()` or use as an Eq + Hash key. |
| Explicit protocol bounds and interfaces | Explicit protocol calls and Eq/Hash bounds retain their checked declarations and operand contracts. Interface conversion preserves the selected object comparison and supplying generation; it does not manufacture missing Eq/Hash evidence. |
| `===` / `!==` | Retain the current underlying-object identity behavior, including supported collection upcasts, without invoking element protocols. This is separate from Eq/Hash eligibility. |
| Third-party collection implementations | May use ordinary eligible defaults or their own explicit Eq/Hash implementations. List/Set/Map membership does not force content equality, a shared hash formula or conflicting ordered/unordered protocols on a type implementing several interfaces. |
| Different concrete implementations | Interface `==` remains an object comparison, not an automatic cross-implementation content comparison. Invoke only an equality implementation applicable to the actual operands; otherwise compare object handles. Never pass an incompatible receiver to a typed `Self` parameter. Use the explicit functions below when cross-implementation content comparison is intended. |

For example, these are the target outcomes; content function names are provisional
until CI01 freezes their public declarations:

```text
val a = Vec::from([1, 2]);
val b = Vec::from([1, 2]);
val x: List<i32> = a;
val y: List<i32> = b;
a == b;                       // true: Vec's explicit PartialEq.
a === b;                      // false: different objects.
a === x;                      // true: the same underlying object.
a.hash() == b.hash();          // true: Vec's content Hash agrees with Eq.
list_content_equal(x, y);      // true: explicit library content comparison.
x == y;                       // true: still Vec's content comparison.
// x.hash();                  // Rejected: plain List grants no Hash.
// HashSet<List<i32>>::new();  // Rejected without the required Eq/Hash evidence.
```

For a custom MyList with no explicit equality implementation, two separately
allocated objects containing [1, 2] compare unequal through List<i32> because
object-handle comparison applies. list_content_equal still returns true. Thus
`==` follows the object's equality, `===` always follows identity, and the helper
explicitly requests content comparison. No interface-specific content rule is added.

### Explicit library content comparison

Provide three ordinary stdlib functions, with the following provisional names
and contracts. They accept two compatible collection applications, which may have
different concrete receivers. Their implementation uses checked collection members
and selected element/key/value protocols, not either receiver's object PartialEq.
Do not require a concrete collection Eq/Hash implementation to call these functions.

| Function | Content rule | Protocol requirements |
| --- | --- | --- |
| list_content_equal | Equal lengths and corresponding elements in order; stop on the first mismatch. | List<T> operations on both operands; T: PartialEq. |
| set_content_equal | Equal cardinality and the same elements, independently of traversal order. | Set<T> operations on both operands; T: Eq; the element matching rule below. |
| map_content_equal | The same keys and equal values for each key, independently of traversal order. | Map<K, V> operations on both operands; K: Eq, V: PartialEq; the key matching rule below. |

A shared content helper can serve the concrete stdlib PartialEq implementations
and the public functions. Reuse one algorithm where its contracts match, while
retaining valid concrete fast paths. This does not turn the helper into a second
operator-selection mechanism. An explicit function returning true does not imply
that either operand's object-level `==` or Hash uses that content relation.
Cross-implementation helpers therefore impose no shared object hash requirement.
New content-hash APIs or comparison-strategy containers are outside this scope.

Set/Map helper matching is fixed by the selected element/key Eq, independently of
either receiver's lookup policy. Compare cardinalities, then match elements (Set)
or entries (Map) one to one without depending on traversal order. For Map, each
matched pair must have equal keys under K: Eq and equal values under V: PartialEq.
Swapping the two operands must not change the matching relation. No caller-selected
comparison strategy or receiver-specific admission rule is required.

For example, maps {"x": 1, "y": 2} and {"y": 2, "x": 1} compare equal. Maps
{"A": 1} and {"a": 1} compare unequal in either argument order under String's
case-sensitive Eq, even if one receiver's own lookup ignores case. The helper
compares keys under that Eq rather than delegating its meaning to get.

A general implementation can match through iteration and protocol calls; helper
bounds do not require Hash. contains/get may optimize matching only when their
uniqueness and lookup rules agree with the selected Eq and preserve one-to-one
matching. This optimization must not alter the helper's observable content rule
or impose an object equality policy on every ordinary Set/Map implementation.

Floating elements retain Kagari's existing IEEE PartialEq: NaN is unequal to NaN,
and positive and negative zero compare equal. Vec<f32>/Vec<f64> and
HashMap<String, f32> support PartialEq but not Eq/Hash. Their List<f32> and
Map<String, f32> views retain that content equality through `==`, and can also use
the corresponding helpers. Neither path grants Eq/Hash. set_content_equal is
unavailable for f32 under its Eq bound.
Neither Vec's content comparison nor list_content_equal may skip element PartialEq
merely because the operands share identity: a list containing NaN is unequal to
itself under content comparison. The same rule applies to Map value PartialEq.
Nested elements follow their own equality behavior, including retained receiver
equality for object comparisons through interfaces. Explicit generic protocol
bounds still require checked evidence; operator availability alone must not be
used to fabricate it. Content comparison does not turn ordinary object fields
into structural comparisons.

Hash-key stability follows the selected object Eq/Hash. Vec/HashSet/HashMap keys
using the stdlib content protocols must keep all equality/hash-relevant state
stable while stored, including referenced element state. Remove a key before
changing that state and reinsert afterward. Readonly aliases do not freeze data;
there is no automatic reindexing, deep copy or mutation subscription. Calling a
content helper does not change a third-party object's key semantics. Violations
remain user logic errors, never permission to weaken GC or runtime integrity.

Kotlin's [List](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/-list/),
[Set](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/-set/) and
[Map](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/-map/)
content rules inform the concrete stdlib implementations and explicit helpers.
Kagari does not adopt their universal interface equality/hash contract, List
covariance, universal Any methods or boxed float equality. Protocol selection
and invariant arguments remain ordinary language rules.

### CI01 protocol evidence and execution decisions

The concrete protocol matrix, receiver equality through interfaces and explicit
helper scope above are settled. CI01 freezes their ordinary declarations and executable
bindings before CI03. Use library-authored implementations and existing conditional
bounds. Interface object comparison needs checked receiver dispatch and operand
compatibility, not a collection-family protocol table or unsafe cross-concrete
`PartialEq::eq` calls. Preserve existing Debug behavior independently
of the removed marker and retain non-collection explicit protocol selection.

The execution decisions below resolve these implementation requirements:

- Map concrete conditional impls and eligible builtin default selection to
  types/HIR/contract owners. Check consistent selection for the same concrete
  instantiation in direct and generic calls, including identity fallback when no
  explicit PartialEq applies and rejection of identity Eq/Hash when it does.
  Verify the same evidence source-free; do not exempt Map/Set by engine-owned names.
- Define the checked representation for object equality through interfaces,
  retaining receiver comparison, operand compatibility and supplying generation.
  `List<i32>` views of two equal Vec values must compare equal; custom list objects
  without explicit equality use handles. Specify the checked dispatch for different
  concrete receivers without invoking a typed `Self` method on incompatible data.
  Keep Eq/Hash evidence separate from the availability of object `==`.
- Freeze the three helpers' names, module, parameter/bound declarations and checked
  member/callback requirements. Their two receivers may have different concrete
  types; cross-implementation `==` is not a prerequisite or an implicit rewrite.
  Implement the fixed Set/Map Eq matching rule above; lookup shortcuts must prove
  compatibility rather than choose a different relation for either operand.
- Fix the concrete stdlib hash formulas and explicit overflow rules at their library
  owner. Equality/hash agreement applies to values comparable under each selected
  protocol; content-helper equality alone creates no cross-type hash obligation.
- Specify recursive/cyclic content comparison/hash and mutation from element or
  receiver callbacks. Record traversal, invalidation/trap behavior, cancellation,
  depth limits, roots and completed effects using existing boundary mechanisms.
  No implicit snapshots, rollback or unbounded recursion are assumed.

Move marker-owned identity/Debug guarantees to their checked semantic/executable
owners, specifying authoring authority, receiver categories, parent adaptation,
GC tracing and generation lifetime. Never infer privileges from read/write access,
arbitrary boxing, method spelling or an unchecked artifact flag. Scalar/tuple/enum
boxes and unrelated interfaces must not incidentally gain identity operators.

Exit evidence covers concrete content protocols versus object identity, helper
comparisons across implementations, receiver equality through interfaces, ordinary
protocol-bound calls and retained generations, eligible default selection and
missing-bound rejection where no eligible default exists, IEEE
float cases and hash-key deduplication with concrete keys. Preserve identity/alias
coverage while replacing only assertions superseded by these explicit contracts.

### Common interfaces and coherence

Use valid applied implementations and declared ancestry, preserving invariant
arguments and associated bindings. Start with equal types and checked one-way
conversions; then compute applicable common interface candidates. Remove duplicate
applications and candidates dominated by a more specific valid candidate. Select
a unique remaining candidate; diagnose ambiguity rather than use catalog/ID order.
An explicit result annotation can supply the intended target and must be checked.

For a multi-arm match, resolve the common result across all reachable arms that
can complete normally. Keep candidate alternatives until every contributing arm
has been considered; do not diagnose an intermediate pair's ambiguity or commit
to an inferred interface that discards candidates needed by later arms. An explicit
expected result type constrains each contributing arm directly. Once the final
target is known, validate and record each arm's conversion to that target. Preserve
existing reachability, non-completion and contextual literal rules; other arm
checking still runs as required. Reordering contributing arms with unchanged
reachability and typed results must not change the chosen interface or ambiguity.

For example, let interface A and interface B each inherit unrelated X and Y,
and let interface C inherit only X. Match arms returning A, B and C have X as
the unique common interface, even though A and B alone are ambiguous. Every
permutation of these arms must select X; a match containing only A and B remains
ambiguous unless a checked result annotation selects X or Y. This is a whole-expression
join rule, not a promise to infer through unrelated nested expressions globally.

Reuse the existing ancestry implementation and search limits/cancellation. Do not
add an interning/cache scheme or erase associated outputs to force a match. Preserve
existing accepted concrete/readable joins and rejection of write recovery; enumerate
cases where more general user-defined interfaces change inference before freezing
the rule. Joins preserve object comparison behavior without manufacturing Eq/Hash
evidence or changing the underlying object identity.

Review ownership for collection impls against ordinary coherence and the existing
type-owned protocol rules. Define a consistent HIR/source-free rule for nominal
receivers, registered builtin families and interface adapters. Preserve required
owner checks without assigning every ordinary trait native installation privileges.

### Host and NC boundary

Classify every `CollectionAccess`, `read_only_view` and `can_weaken_to` consumer.
For each one record: observable contract, proposed owner, retained representation
if any, validation and removal checkpoint. Standard interface operations/upcasts
must cease consulting a separate storage-access axis.

Native host Map/Set schemas may retain an explicit access descriptor for checked
host admission while existing concrete family layouts remain. Any retained use
must validate a host/storage boundary, not grant ordinary trait methods, defaults,
identity or public parent conversions. Define the checked adaptation that prevents
a readonly host result from obtaining a writable witness. Preserve artifact and
binding rejection for mismatched host declarations.

Do not silently make this plan implement NC's nominal storage migration. If a
remaining semantic Map/Set access axis cannot be removed without that migration,
CI01 must explicitly amend the roadmap/NC boundary and completion order before
code changes; it cannot be hidden as accepted structural debt. A retained physical
or host descriptor needs its precise justification here and removal owner where
applicable. SA18 completion requires that no second script interface authority remains.

## CI01 execution decisions and migration allocation

The following decisions implement the accepted behavior without activating NC's
storage migration or SA21's registration redesign.

| Existing responsibility | Replacement owner and checkpoint |
| --- | --- |
| Installed trait identity, including private/no-call records | CI02: runtime registration catalog checks every installed identity against its exact declaration, independently of storage_access. Native callable declarations retain exact binding and dependency requirements even without an executed call. An ordinary methodless trait conveys no storage authority. Keep the legacy marker check only while CI03-CI04 consumers still depend on it; delete it in CI05. |
| Collection implementation ownership | CI02: ordinary trait coherence in HIR and contract proofs; type-owned equality/conversion and registered builtin receiver ownership remain checked separately. Merely implementing List/Set/Map grants no native authority. |
| Receiver comparison through an interface | CI03: compiler-selected comparison evidence accompanies checked interface construction; executable validation checks its receiver and operands. Runtime retains the operation/default with the existing interface snapshot, type environment and supplying module. Reuse OperationWitness/operation groups and checked call dispatch; do not search implementations from a live value. Parent conversion preserves the receiver selection. |
| Object identity and Debug | CI03: checked receiver categories and interface construction evidence replace the runtime scan for a marked trait. Do not make boxed scalar/tuple/enum values identity-bearing objects. Preserve existing bounded formatting independently of content equality. |
| Builtin Map/Set equality overrides | CI03: use registered receiver ownership and ordinary conditional implementation proofs for these constructors, just as for nominal Vec. Once explicit PartialEq applies, identity Eq/Hash defaults are unavailable. |
| Common interfaces and generic inference | CI04: types owns invariant implementation substitution and parent closure; HIR gathers valid script/native candidates and checks bounds. Select the unique most-specific common candidate over all completing arms, then record conversions. A successful join does not grant Eq/Hash bounds. |
| List indexing and source adaptation | CI04: checked Index output and declared write members replace storage-marker discovery. Replace native array_interfaces discovery and its copies in lowering/imports/declarations; retain builtin Array element context separately. No Array-to-List conversion is added. |
| Ty/TypeId Map/Set access descriptors | CI04 retains them only for the existing host/storage representation until NC removes the concrete families. Register exact readonly receiver implementations for readable host Map/Set operations; generic impl matching must not weaken access based on a trait flag or name. These descriptors never define ordinary interface inheritance. |
| Host schema/path access, native collection handles and artifact write validation | CI04 preserves host-owned readonly admission, scoped borrows and rejected writable upgrades. Any required outer host adaptation is explicitly checked at this boundary; nested arguments remain invariant. Keep host wire/fingerprint and storage-layout evidence until their NC/SA21 owners replace the representation. |
| Marker fields, mappings, wire records, builders and generated declaration caches | CI05 removes storage_access/storage_view/native_trait_access and ImplementationPattern's copy after the above consumers have moved. Regenerate only affected products; no compatibility reader or format-version bump. |

The three public functions will be exported by std::collections with the accepted
names list_content_equal, set_content_equal and map_content_equal. Their public
parameters are List<T>, Set<T> and Map<K, V> respectively, with the bounds in the
accepted table. Two independent interface receivers permit different concrete
implementations. Stdlib owns their algorithms and concrete protocol bindings;
native callable requirements/checked interface calls supply reads, iteration and
element comparison. The runtime supplies calls, storage access and lifetime checks.

List comparison advances two iterators left to right and short-circuits at the
first mismatch. Set/Map matching uses the selected Eq, with one-to-one matching
of members/entries. If a custom lookup relation permits multiple candidates under
that Eq, use a general matching algorithm rather than greedily reusing an entry.
Working match state is rooted and cancellation-polled; it is not a frozen copy of
receiver state. The initial implementation need not add lookup or hash fast paths.

Traversal uses existing iterator invalidation: builtin structural changes commit,
and subsequent reads through invalidated cursors trap. No rollback or implicit
freeze is introduced. If comparison short-circuits before another cursor read,
completed callback effects remain and there is no additional traversal solely to
force an invalidation error. Custom iterators retain their declared next behavior;
there is no invented universal mutation-version field for arbitrary script types.
Bounds/handle validation and roots remain mandatory regardless of callback behavior.
Recursive content comparisons/hashes use the existing call-depth and cancellation
boundaries; no coinductive cycle equality or same-identity success shortcut is added.

Concrete content hashing uses u64 bit patterns from selected i64 element hashes;
all arithmetic below wraps modulo 2^64, and the final bits are returned as i64.
Define mix(h, x) = (h XOR x) * 1099511628211 and seed = 14695981039346656037.
Vec folds mix over element hashes in order and finally mixes its length. HashSet
sums mix(seed, element_hash) independently of order and mixes the sum with its
cardinality. HashMap sums mix(mix(seed, key_hash), value_hash) for each entry,
then mixes that sum with its cardinality. These formulas belong to stdlib, not
runtime object hashing or collection interfaces; internal table hashing stays NC-owned.

Validation allocation: CI02 uses installation_access and the existing ordinary
trait/override proof owners; CI03 uses collection_interfaces/collection_access,
standard-protocol proof cases and the retained generic-operation reload test;
CI04 uses common-interface/type-matching tests and host readonly rejection cases.
CI05 selects declaration codec and source-free artifact cases changed by removal.
Reuse existing GC/cancellation/native-callback tests and add only missing contracts.
The final local selection remains focused; full workspace/backend coverage is CI-owned.

## Phases and implementation checkpoints

Default to buildable checkpoints. Group inseparable HIR/contract/verifier/runtime
changes within a phase; do not publish a frontend promise the executable path
cannot satisfy. While the old field is present, each responsibility has exactly
one semantic implementation; switched consumers do not fall back to the marker.

### CI01: freeze contracts and migration inventory

1. Refresh all production and test callers using the inventory searches below;
   read affected diff/history and the current NC/host boundaries.
2. Close the protocol evidence/execution, inference/coherence and host/NC
   decisions above, consistent with the concrete protocol matrix and explicit
   helper contracts. Record examples, source-free representations and owner APIs,
   including List indexing and helper calls across different implementations.
3. Map required evidence to existing tests. Add cases only for distinct missing
   contracts; use temporary reproductions where current behavior is uncertain.
4. Record the exact phase allocation for each remaining access-tagged consumer.

Exit: no unresolved design choice required by CI02-CI05, no hidden NC/SA21 expansion,
and a reviewed test/owner map. This phase does not promise a performance gain.

### CI02: independent declaration and implementation authority

1. Move exact installed declaration checks to verified Native requirements and
   installed registration identities, preserving public/private/no-call coverage.
2. Align HIR implementation ownership and contract coherence checks with CI01.
   Keep reserved roles, conversion adapters and builtin ownership checks intact.
3. Establish replacement no-call coverage for ordinary installed traits and native
   declarations, including forged/missing/private/reload rejection. A methodless
   ordinary trait gains no native privilege. Retain the legacy methodless-marker
   forgery check while marker consumers remain; CI05 removes it with the marker.

Exit: installation and ownership guarantees no longer depend on storage_access;
existing view behavior remains intact. No new provenance bit is trusted by itself.

### CI03: protocol and receiver-identity semantics through execution

1. Register explicit conditional PartialEq/Eq/Hash implementations for Vec,
   HashSet and HashMap and the three content comparison functions. Update stdlib
   algorithms/registration, HIR queries and contract proofs together. Remove
   marker-based grants; retain object equality through interfaces with checked
   receiver dispatch and eligible defaults, without fabricating Eq/Hash bounds.
   Include conversions required by helper arguments so
   CI04 is not needed to execute a CI03 call already accepted by HIR.
2. Carry checked operation/identity evidence through compiler/MIR/bytecode where
   required. Independent verifiers validate its receiver, signature and witness.
3. Update interface construction, parent adaptation and runtime comparison/hash/
   formatting to consume that evidence. Remove marker-based collection_data
   discovery for these operations; retain roots, categories and pinned versions.
4. Prove concrete stdlib Eq/Hash, helper comparisons over custom script/native
   collections, interface object comparison, object identity, float behavior and
   selected operations after reload. Adapt superseded comparison/hash tests while
   retaining distinct source-free, alias, GC and borrow coverage. No runtime source
   analysis or implementation search.

Exit: concrete content protocols and explicit helpers execute through ordinary
checked declarations; interface `==` preserves receiver equality or eligible
identity defaults. Object comparison alone grants no Eq/Hash bounds. Identity
operators remain separate, and decoded artifacts validate the same facts.

### CI04: ordinary relationships, common types and checked operations

1. Replace storage_views/shared_storage_view/matching_storage_headers with the
   CI01 interface relationship rules. Include script impls, not only engine-owned
   candidates; preserve bounds, invariant arguments and associated members.
2. Update implementation substitution, if/match joins, ordinary operator operand
   checks, helper argument conversion, generic inference and tooling consistently.
   A common collection interface retains object equality without manufacturing
   Eq/Hash evidence.
   Resolve multi-arm common interfaces before reporting ambiguity or committing
   arm conversions, following CI01's order-independent whole-expression rule.
3. Rebind the existing installed List indexing/member facts without storage_view
   discovery. Keep Array context and intrinsic writes under their current owner.
4. Make collection method/index writes depend on selected write operations. Rebuild
   matching/verifier facts from contracts; do not authorize raw writes just because
   a receiver has native storage or an interface wrapper.
5. Implement the CI01 host/access-tag classification and checked adaptation. Remove
   generic marker/access weakening shortcuts that would reintroduce a second
   collection-interface authority; preserve readonly host admission separately.

Exit: valid alias/upcast/branch/generic cases execute; ambiguous or invariant-mismatched
relationships diagnose consistently; source and source-free paths reject forged
receiver upgrades. Every retained access descriptor has a reviewed boundary owner.

### CI05: remove the obsolete model and refresh affected artifacts

1. Remove TraitDef.storage_access, TraitBuilder.storage_view, ImplementationPattern's
   copy, HIR trait-signature/native attachment maps and every now-dead caller.
2. Update declaration mapping, scoped identity visitors, wire records, registration
   rendering/shape checks, compiler envelopes, tests and examples together.
3. Invalidate affected generated declaration caches and regenerate the necessary
   tracked source-free fixtures at this coherent checkpoint. Keep meaningful
   source-free rejection coverage; add no old-format reader or API alias.
4. Audit the remaining collection_access/read_only_view/can_weaken_to and Array
   binding uses against CI01's classifications. Remove obsolete branches/helpers.

Exit: searches find no old field/API/attachment path in implementation; retained
host/layout access facts cannot recreate ordinary collection privileges. Keep ABI
and artifact identifiers unless publication or an explicit compatibility commitment
requires a bump; disposable development products are regenerated instead.

### CI06: final focused acceptance and documentation handoff

1. Complete the selected tests and a small regression set below, including source-free
   loading, GC/borrow cleanup and pinned reload. Resolve all carried build/test errors.
2. Review changed Rust imports, ownership, visibility, re-exports, effective LOC and
   mixed responsibilities; run structure, formatting, scoped Clippy and diff checks.
3. Move implemented behavior to architecture, collection/value/trait/native/artifact
   specifications and model documentation. Refresh generated navigation examples
   if declaration parents or member surfaces changed.
4. Update the NC/SA21 inventories to consume the resulting APIs, without implementing
   their migrations. Remove resolved SA18 from review and its queue entry. On actual
   completion remove this plan after lasting contracts/open work have their owners.
5. Report focused local evidence and unrun GitHub CI scope separately. Any performance
   claim needs a matched measured workload; no benchmark target is an acceptance gate
   for this architectural cleanup.

Exit: the obsolete marker is removed, all owned guarantees have verified replacements,
no required local integration failure remains, and current contracts have durable owners.

## Verification selection

Reuse existing owners; do not reproduce a workspace sweep through package commands.
CI01 records replacement names when marker-specific or old equality tests are
rewritten. Preserve collection_interfaces' distinct-object and alias `===` checks.
Retain plain-List equality checks and update their expectations to the underlying
receiver's comparison. Add explicit helper cases for content comparison independent
of receiver policy, and test missing Eq/Hash evidence separately. Existing
view_identity_survives_upcasts_branches_and_hash_storage and collection_access
cases currently use plain List equality/hash and HashSet<List<...>>: move their
hash/dedup assertions to concrete keys or valid ordinary protocol evidence while
retaining upcast/branch/alias checks. Do not preserve those obsolete grants merely
to keep old tests green, or force custom collections to adopt stdlib object Eq.

| Checkpoint | Existing evidence to select | Why |
| --- | --- | --- |
| CI02 | installation_access no-call/private/reload cases; native_builder ownership/member tests; affected contract ownership tests | Protect registration authority before removing its old trigger. |
| CI03 | adapted collection_interfaces and collection_access cases; affected concrete conditional-impl/default proofs; explicit helper execution; one relevant retained-operation reload case | Cover concrete Eq/Hash, helper content rules, interface object comparison, Eq/Hash evidence, identity, aliases and generations through HIR and executable paths. |
| CI04 | collection_interfaces and collection_access targets; focused type matching/inheritance tests; one directly affected Array indexing/mutation case | Cover custom implementations, invariant arguments, joins, write rejection, host families and forged artifact upgrades. |
| CI05 | affected public ABI wire/declaration codec tests; exact-installation rejection; source-free build; affected artifact fixture consumer | Prove removed fields and regenerated products do not bypass executable contracts. |
| CI06 | complete skipped coverage from the selected scope; one native boundary cleanup case and one interface/operation reload case if not already covered | Verify roots/borrow guards and retained witnesses at the changed boundary. Previously passed unaffected cases remain evidence. |

Concrete command examples for the current test names (run at their owning checkpoint,
not all at every phase):

```text
cargo test -p kagari-runtime --test installation_access
cargo test -p kagari-runtime --test native_builder trait_groups_bind_declared_members_and_associated_outputs
cargo test -p kagari-embed --test collection_interfaces --test collection_access --no-fail-fast
cargo test -p kagari-contract --lib types::matching::tests::
cargo test -p kagari-vm --test native_boundary hash_callbacks_can_collect_and_trap_without_losing_keys_or_lookup_guards
cargo test -p kagari-embed --test generic_reload retained_generic_default_and_override_closures_pin_types_and_operations_after_reload
cargo check -p kagari-embed --no-default-features --lib
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
git diff --check
```

Select exact affected proof/codec/Array test names at CI01 after inspecting those
owners. A filter matching zero tests is not acceptance. Existing Source/Artifact/
ArtifactJit routes must keep their assertions; ArtifactJit is not evidence that
unsupported code actually entered native execution. Exhaustive feature/backend
and workspace Clippy/test validation belongs to GitHub CI, including full artifact
and native coverage. Scoped Clippy follows the actual changed crate/target set.

Add focused cases only for missing contracts: concrete Vec/HashSet/HashMap content
Eq/Hash versus object identity; ordered sequence and unordered set/map helpers;
Map key/value association; helper comparisons across different implementations;
interface `==` retaining Vec content equality and custom-object identity defaults;
Hash/key rejection without Eq/Hash evidence; valid ordinary protocol-bound calls;
conditional implementations and eligible identity
fallback; no identity Eq/Hash mixed with explicit PartialEq; consistent concrete
instantiation selection in direct and generic calls;
concrete-key deduplication and hash agreement; IEEE NaN/signed-zero values, including
same-object content comparisons; nested elements using their selected protocols.
Include a custom collection whose object equality differs from helper content
comparison, proving that interface membership does not impose stdlib Eq/Hash.
Cover reordered entries and swapped helper arguments, including custom lookup
policies that differ from the selected Eq; preserve key/value association and
one-to-one matching instead of relying on either receiver's get/contains semantics.
Select callback mutation, cyclic/depth, cancellation and cleanup evidence after
CI01 freezes their rules. Preserve builtin Array's separate identity behavior.
Do not assert a deterministic lookup result after violating hash-key stability;
reuse valid remove-before-mutate/reinsert coverage where available.

Add relationship/authority cases only when missing: multiple incomparable common
interfaces, multi-arm joins whose later arm resolves an earlier pair's ambiguity (including
permutations of the contributing arms), explicit result annotations, and exclusion
of unreachable or non-completing arms from join candidates. Cover user-defined
parent inference, interface operations lacking valid evidence,
copied/private native declarations without calls, excluded identity categories,
or readonly host data attempting a writable witness. Reuse existing trap/callback/
GC/reload contracts rather than create a parallel smoke suite. Verify accepted
cases through execution as well as analysis; verifiers must reject forged facts
independently of source checking.

## Migration and structural policy

Inventory commands:

```text
rg -n 'storage_access|storage_view|native_trait_access' crates -g '*.rs'
rg -n 'CollectionAccess|collection_access|read_only_view|can_weaken_to|storage_views|shared_storage_view|matching_storage_headers|array_interfaces' crates -g '*.rs'
```

These searches locate consumers; they do not establish their semantic ownership.
Inspect macro-generated call sites, docs, fixtures and generated-source contracts
as well. Share semantic matching/ancestry at the existing types owner; HIR owns
source inference/diagnostics, contract owns executable evidence, runtime owns live
values/storage, and stdlib owns collection declarations/policy. Do not create a
forwarding crate or move policies into common to break a dependency cycle.

Default to buildable vertical checkpoints. Attempt focused checks at architecture
boundaries; record any unavoidable intermediate failure below with reproduction,
representative diagnostic, cause and owning phase. Group dependent repairs and
rerun failing/affected cases. No weakened assertions, stubs or dual semantic paths.
Temporary migration data must have its CI05 removal checkpoint. No routine ABI
version bump or repeated fixture refresh is required for this unpublished project.

Implementation checkpoints use `Plan: collection-interface-plan CI0N` trailers.

## Progress and error ledger

- [x] Record proposed scope, inspected consumers, phase order and acceptance owners.
- [x] Incorporate the planning review's multi-arm join rule, CI04 migration
  requirement and focused acceptance cases; implementation is tracked by the phase entries below.
- [x] Record the ordinary object-comparison design: concrete stdlib Eq/Hash,
  receiver equality through collection interfaces, three explicit content helpers, IEEE
  element behavior and hash-key stability. Align CI03 and acceptance with this
  scope; implementation is tracked by the phase entries below.
- [x] Fix Set/Map helper matching to element/key Eq and Map value PartialEq,
  independent of receiver lookup policy and argument order. Retain ordinary eligible
  identity fallback when no explicit PartialEq applies, without mixing identity
  Eq/Hash into an explicit content comparison; implementation is tracked by the phase entries below.
- [x] Correct the superseded plain-interface `==` prohibition throughout the plan:
  interface comparisons retain receiver equality or eligible handle defaults.
  Keep Eq/Hash requirements distinct; synchronize examples, CI03 and acceptance.
- [x] CI01: Freeze execution decisions, access-consumer allocation and focused
  validation. InterfaceTableRecord/MakeInterface and InterfaceValueSnapshot are the
  checked construction/retention owners for CI03 comparison evidence. Loading calls
  NativeRegistry before publication; CI02 extends that existing authority check to
  exact ordinary installed traits and no-call native declarations.
- [x] CI02: Separate installed declaration checks and ordinary implementation
  ownership from the access marker; retain legacy forgery checks until CI05.
- [ ] CI03: Implement concrete stdlib protocols/content helpers and retain identity.
- [ ] CI04: Migrate ordinary relationships, joins, operations and host admission.
- [ ] CI05: Remove obsolete fields/APIs, migrate codecs and refresh affected fixtures.
- [ ] CI06: Complete focused acceptance and durable documentation handoff.

Execution started on 2026-10-12 from 53949725. The initial working tree contained
this plan and the related roadmap/review/NC planning edits; preserve unrelated
queue ordering. CI01 inspected registration checks, HIR and contract ownership,
interface snapshots/operation groups, collection registrations and access consumers.
CI02 added distinct no-call checks for ordinary installed traits (including private
records and reload) and native callable declarations. Before the fix,
`cargo test -p kagari-runtime --test installation_access without_calls -- --nocapture`
reproduced two failures: ordinary_installed_traits_require_exact_contracts_without_calls
accepted a forged installed trait, and
native_declarations_require_exact_installation_without_calls accepted an uninstalled
native declaration. The reserved-core case passed. Runtime registration validation now checks both paths.
`cargo test -p kagari-runtime --test installation_access --test native_builder --no-fail-fast`
passed all 9 selected tests. Structure checks passed (1087 files, no violations).

CI02 also removed collection-marker ownership exceptions from HIR and source-free
proofs, preserving type-owned language protocols and requiring explicit PartialEq/Eq
prerequisites for registered builtin Hash implementations. The existing custom
collection execution fixture now declares Sequence in a child module and implements
List/MutableList in its parent, preserving Source/Artifact/ArtifactJit assertions.
Its first reruns exposed fixture errors (KG_RESOLVE_UNKNOWN_NAME for an unqualified
import, then KG_TYPE_INVALID_TRAIT_IMPL for foreign Index/Iterable implementations).
The fixture now uses self::storage::Sequence and keeps Index/Iterable in the type's
own module. `cargo test -p kagari-contract --lib equality` passed all 3 selected
proof tests, including the new builtin prerequisite case. The execution rerun and
scoped Clippy both passed. The execution case retained all three routes
(Source, Artifact, ArtifactJit); the route name alone makes no claim of native
execution. `cargo clippy -p kagari-runtime -p kagari-hir -p kagari-contract --lib -- -D warnings`
passed. No CI02 failure remains. Full workspace/backend validation is left to CI.
CI03 is in progress. The stdlib now declares conditional concrete content protocols
and the three interface helpers, with rooted checked reader/callback algorithms,
ordered List comparison, unordered one-to-one Set/Map matching and the agreed hash
formulas. HIR, lowering and executable composition now consider explicit Map/Set
protocol implementations. Equality lowering reuses the ordinary applied-method path
for native and script overrides. `cargo check -p kagari-stdlib --lib` passed.

The first focused execution test failed during standard registration with
`native template defaults or selected calls differ from their declared contracts`.
The dependency closure retained callback traits but omitted applicable implementations
when a callback receiver was a constructed type rather than a function-bound parameter.
Collecting those ordinary implementation dependencies fixed the failure without weakening
validation. `cargo test -p kagari-stdlib --test registration
canonical_foundation_contracts_survive_scoped_publication_and_installation` passed.
The next rerun exposed equality lowering treating native overrides as script functions;
using the ordinary applied-method lowering fixed that issue.
`cargo test -p kagari-embed --test collection_interfaces
concrete_content_protocols_and_interface_helpers` then passed across Source/Artifact/
ArtifactJit; this does not establish native machine-code entry. The `content` filter
then passed both concrete and nested/IEEE cases, including direct/generic Vec selection,
NaN self-comparison, signed zero, nested key lookup and float collection Hash rejection.
The custom List fixture passed with helper calls in both directions across Sequence/Vec.
The custom Map helper case passed one-to-one matching with a lookup relation distinct
from key Eq, a non-greedy augmenting path, swapped arguments and a missing full matching.
Its first run had a fixture parser error from a struct literal in an if condition;
using a local lookup key fixed it.

The legacy interface-hash test has been migrated to
`receiver_identity_survives_upcasts_branches_and_concrete_hash_storage`. It now
checks retained identity through upcasts/joins and concrete Vec content hashing
and key deduplication. Plain List hashing and HashSet<List<i32>> are rejected;
operator availability does not provide Eq/Hash. The earlier I32(0) failure is
resolved, with source/artifact routes passing at the receiver-comparison checkpoint.

Direct generic helper calls such as `list_content_equal(vec_a, vec_b)` expose an
existing first-argument coercion gap: inference discovers T but fails to install the
interface adaptation for the first argument. CI04 owns the inference/coercion repair
and direct-call acceptance; current CI03 fixtures use explicitly typed interface
variables to exercise the accepted executable path. A fixture-only `HashMap::from`
call was replaced with the existing new/insert API.
Concrete interface receiver comparison and the separation from Eq/Hash are now
implemented as described below. Shared generic construction and native result
adapters now have the additional closed-call coverage recorded below. CI03 remains
incomplete: callback boundaries, caller-scoped shared reload and legacy readonly
builtin Map/Set host descriptors still require acceptance against concrete selection. Keep NC
storage migration out of scope.

The directly affected script/enum composition regression
`cargo test -p kagari-embed --test standard_traits
custom_equality_composes_and_enum_overrides_variant_checks` passed. Scoped Clippy
(`cargo clippy -p kagari-stdlib -p kagari-runtime -p kagari-compiler -p kagari-contract
-p kagari-hir --lib -- -D warnings`), structure checks (1090 files, no violations),
formatting, local plan links and diff checks passed. Structural review found no new
re-exports, production globs, ownership widening or LOC exceptions. Library comparison
policy stays in stdlib; generic proof/dependency and native call dispatch fixes stay
with their existing owners. Callback mutation/depth/cancellation and retained-generation
acceptance remain pending with the rest of CI03; full workspace/backend checks remain CI-owned.
`cargo test -p kagari-runtime --test installation_access` passed all 6 tests after
the callback dependency change, retaining the ordinary/private/no-call installation
checks. The initial stdlib checkpoint carried the legacy-interface behavior failure resolved
above; neither that checkpoint nor the receiver-comparison checkpoint completes CI03.
Current semantic specifications remain unchanged until their implementation checkpoint.


CI03 receiver-comparison checkpoint: compiler interface demands now carry a checked
PartialEq operation for concrete object receivers, including eligible default
adapters. MIR and bytecode encode/remap/visit that evidence, validate its requirement,
receiver, type applications and executable target, and retain the target dependency.
A concrete object MakeInterface without comparison evidence is rejected. Each table
uses its own module's checked selection, rather than borrowing an equivalent adapter
from another caller's dependency closure. Interface snapshots retain the supplying
table, generation and type environment as traced metadata; parent upcasts preserve
that original binding. Runtime equality checks the actual receiver types before
calling the retained operation, uses handles for different concrete receiver types,
and invokes compatible operations with both self and other rooted. It performs no
live trait search. Identity uses checked object categories without a collection-marker
scan; ordinary interface values no longer enter the implicit hash-key fast path.

The first execution reruns exposed a missing self argument and a VM frame borrow
held across the new callback. Supplying both checked arguments and releasing the
frame borrow before synchronous reentry resolved those failures. The custom-container
case then exposed selecting a caller-local adapter for a foreign table; using the
table owner's demand resolved its source-free target validation failure. A remaining
HIR method-candidate grant allowed List.hash even after bound proofs were removed;
that candidate grant is now restricted to PartialEq/Debug. The malformed-artifact
fixture initially used a whole-context definition snapshot rather than the program's
collected identity table (InvalidPublicAbi); it now checks the original and mutations
in the same collected program scope.

`cargo test -p kagari-embed --test collection_interfaces --no-fail-fast` passed all
8 cases before the additional artifact test. The subsequent `receiver_` filter passed
both identity/upcast/key behavior and the new source-free evidence test (missing
selection, wrong receiver, wrong result, and unproved forwarding). The expanded
`user_containers` case passed explicit script comparison, eligible identity defaults,
foreign-module tables and mixed receiver types in both operand orders. Concrete
interface equality, ordinary generic boxing, tuple composition and NaN self-comparison
passed across Source/Artifact/ArtifactJit, with forced collection enabled. These route
names alone do not prove native machine-code execution.

The symbolic shared-template omission described at this checkpoint is resolved for
closed calls by the following construction-coverage checkpoint. Remaining CI03
acceptance includes retained shared methods receiving caller-scoped nominal arguments,
ordinary protocol-bound calls, callback mutation/traps/depth/cancellation, and readonly
builtin Map/Set host descriptors. Static collection
operator eligibility still uses the old marker pending its CI04/CI05 migration; the
runtime comparison path no longer does. Do not mark CI03 complete from the concrete
coverage above. The CI04 direct-helper first-argument coercion gap remains unchanged.

`cargo test -p kagari-vm --lib execution::interfaces` passed all 23 selected interface
construction/call/generation/root/trap regression cases. `cargo test -p kagari-runtime
--lib value_semantics` passed both enum/identity/IEEE contract cases. The new
`cargo test -p kagari-embed --test generic_reload
interface_comparison_retains_its_receiver_generation_in_a_new_caller` passed after
artifact round-trip and forced collection: a newly loaded caller invokes old receiver
equality for a retained interface and new equality for a fresh one; dropping all
roots releases the old runtime-value retention. This establishes comparison-specific
supplying-generation retention, not merely ordinary method retention.

Receiver-comparison checkpoint validation also passed scoped Clippy
(`cargo clippy -p kagari-compiler -p kagari-runtime -p kagari-vm -p kagari-stdlib
--lib -- -D warnings`), structure checks (1093 files, no violations or exceptions),
formatting, local plan links and `git diff --check`. Clippy first caught a redundant
clone of interface demands; removing it resolved the finding. Structural review
kept selection/validation at compiler/contract owners and checked invocation at
runtime, with no new re-exports, production globs or LOC exceptions. The selected
regressions cover the changed equality and interface boundaries; full workspace
and exhaustive feature/backend validation remain CI-owned and were not run locally.


CI03 shared-construction checkpoint: a focused reproduction made an interface's
unconstrained generic method return List<T> from Vec<T>, then compared two results.
It trapped with `interface receiver does not support object comparison`. Reusing an
exact prechecked table application fixed the case when another concrete boxing site
had already demanded that application; the isolated shared-only case still failed.
The compiler now collects closed interface-construction applications through shared
script calls, dynamic method slots, retained closure bodies and native result adapters.
It materializes the corresponding interface records and selected comparisons without
specializing shared function bodies. A common contract-owned construction graph
performs bounded binder substitution and deduplicates pending applications, with
cancellation polling and existing generic-instance diagnostics. MIR and bytecode
independently project their executable records into that graph and reject missing
concrete object comparison applications before execution. No live trait lookup is added.

The graph follows method-local binder mappings separately from receiver arguments,
including native default applications. Runtime shared construction retains the exact
record for the selected implementation and reified arguments, together with its owner
and type environment. Native return conversions use the same retained records as
script MakeInterface; no collection-name dispatch or separate content algorithm is
introduced. The added graph is executable call/construction metadata analysis, not
source type inference or a replacement for ordinary method/implementation verification.

`cargo test -p kagari-embed --test collection_interfaces --no-fail-fast` passed all
10 cases after initial shared-construction/bytecode integration. The expanded isolated
`shared_generic_boxing` case additionally passed a generic helper, construction inside
the shared body, a retained closure, NaN and signed zero. The `receiver_comparison_evidence`
case passed all four corruption variants for both a direct construction and a program
whose collection is constructed only in a shared generic method. The extra native
harness in that filtered invocation ran zero tests; it is not native-result evidence.
`cargo test -p kagari-embed --test native_provider_reset
concrete_native_results_construct_declared_interfaces_in_generic_calls` passed with
an added content comparison of two native generic factory results, including decoded
artifacts, and passed again after MIR coverage validation was enabled.
`cargo test -p kagari-embed --test default_methods shared_generic_method --no-fail-fast`
passed all 3 selected shared list/helper/nominal-construction regression cases.
The temporary compile error from importing MAX_TABLE_RECORDS from common rather than
its executable-contract owner was corrected; no selected behavior failure remains.

CI03 is still open for the remaining acceptance above; CI04's direct-helper inference
repair and CI05's marker removal remain pending. Shared comparison under mixed nominal
supplying generations must be checked before declaring the generic/reload boundary
complete. Complete feature/backend/workspace coverage remains CI-owned.

The isolated shared-boxing regression passed again after MIR coverage validation and
native slot mapping changes. The existing `instantiation` test
`instance_limits_report_revision_owned_diagnostics_without_poisoning_compilation`
passed after preserving ordinary generic-instance diagnostics for construction limits.
Scoped Clippy (`kagari-compiler`, `kagari-runtime`, `kagari-mir`, `kagari-bytecode`,
`kagari-contract`, library targets only), structure checks (1096 files, no violations
or exceptions), formatting, plan links and diff checks passed. Structural review kept
binder traversal at contract, representation extraction at MIR/bytecode, materialization
at compiler and generation retention at runtime; no new re-exports, production globs,
compatibility APIs or LOC exceptions were added. This is a buildable CI03 checkpoint,
not completion of CI03 or the full goal.

CI03 supplying-generation checkpoint: the new decoded-artifact reload case retains
an old Boxer.one<T> shared body while a new Probe supplies Item from its own
program generation. Changing Item.eq from true to false exposed a real failure:
the result remained true because interface construction picked its exact Ty
application from the old body's program. Receiver comparison preparation now
uses the reified receiver and nested type-argument origins to consider only retained
supplying programs, and requires complete nominal-generation identity before
retaining an already checked application of the same interface implementation.
It does not use layout compatibility or search trait implementations from a live
value. The retained comparison binding keeps the selected program alive through
the existing snapshot metadata edge. The old caller still uses old Item.eq and
the new caller uses new Item.eq; forced collection and final root/object release
passed in `shared_collection_comparison_retains_the_callers_nominal_generation`.

The collection_interfaces target passed all 10 cases after this fix, covering
direct and shared construction, custom implementations, content operations,
identity and decoded executable corruption checks. A new collection_callbacks
contract test exercises both concrete Vec equality and list_content_equal, plus
Vec hashing. It checks committed structural mutation followed by iterator failure,
short-circuit false without a forced extra read, element traps, recursive equality
and hashing hitting the configured depth limit, cancellation, root release and
successful reentry after cancellation under forced GC. It passed against decoded
artifacts. The initial fixture used a nonexistent Value::Usize variant and expected
the storage-layer iterator error rather than the exposed cursor error; those test
errors were corrected without changing runtime behavior. Full native/backend
coverage remains CI-owned; these callback assertions use the interpreter.

Remaining CI03 work: check newly introduced type applications after reload,
including primitive arguments without nominal origins, and compositions whose
nominal components come from different supplying generations. Exact application
reuse is not evidence that these boundaries are complete; source-free checked
construction evidence must cover them without choosing a layout-compatible but
semantically different callback. Finish readonly host descriptor admission and
ordinary protocol-bound acceptance before closing CI03. CI04 relationship/helper
inference work and CI05 marker removal remain open.

The preexisting retained-receiver reload regression also passed after the change.
Runtime library Clippy, structure checks (1097 Rust files, no violations or
exceptions), formatting, local plan links and diff checks passed. Structural
review found no new public API, re-export, import glob or duplicated semantic
implementation. Receiver application preparation remains runtime-owned and uses
existing checked construction records and type provenance. This checkpoint does
not complete CI03 or the active goal.


CI03 construction-scope checkpoint: reloading Probe to first call an old
Boxer.one<T> with f32 reproduced `interface receiver does not support object
comparison`. Primitive type arguments have no nominal layout origin, so nominal
provenance alone cannot locate the caller's newly checked List<f32> application.
Shared execution environments now retain a finite construction scope containing
the supplying checked programs. Shared calls, interface method applications,
selected native callbacks and native result adapters forward it; closures retain
it through their existing environment. Application cache keys include its program
identities, and executable metadata tracing retains its program edges. Exact
receiver identity is still checked when selecting the already verified table
application; no live trait search, source lookup or runtime specialization is added.

The allocating frame passes its environment to make_interface_applied, and native
result conversion does the same. This changes that unpublished Rust method's
signature to accept Option<&TypeEnvironment>; its script/native consumers were
updated together and the obsolete single-use make_interface_view wrapper was
removed. Environment handles retain owner/generation validation. Borrowed frame
access ends before interface allocation. No artifact/ABI identifier was bumped.

Both shared_collection_comparison reload cases passed with their shared method
now creating a closure that calls a generic helper before constructing List<T>.
They cover old/new nominal callbacks and a primitive application absent from the
old program, forced GC, and final root/object/version release. The 5
objects::application::tests passed for receiver-independent caching, version
separation, abandoned providers, bounded retention and reclamation. The selected
native_provider_reset concrete_native_results_construct_declared_interfaces_in_generic_calls
case passed, including its decoded native generic result comparisons. These checks
close the newly introduced primitive application failure; compound receivers with
nominal components from different generations still need distinct evidence.

CI03 helper-argument completion: direct list_content_equal(Vec, Vec),
set_content_equal(HashSet, HashSet) and map_content_equal(HashMap, HashMap) calls
reproduced ArgumentTypeMismatch on the first argument after successfully inferring
the element types. HIR now completes unselected expression-boundary conversions
after collecting generic substitutions, without inferring arguments again or
replacing an existing selected conversion. The collection_interfaces target passed
all 10 cases with direct concrete helper calls, mixed concrete/interface operands,
and custom Sequence/Vec arguments in both orders. Its existing explicit-interface
and independent content/identity assertions remain. The 3 selected
shared_generic_method default_methods regressions also passed. General inference
when only custom implementations supply the type, common-interface joins, and
replacement of marker-filtered candidate discovery remain CI04 work.

CI03 remains open for compound supplying-generation construction and readonly
host comparison/protocol admission. The constructor application graph and exact
record reuse must not be treated as proof of arbitrary mixed-version composition.
CI04-CI06 remain pending; the helper argument repair is the conversion requirement
already owned by CI03, not completion of ordinary interface relationship inference.

Runtime/VM/HIR library Clippy, structure (1098 Rust files, no violations or
exceptions), formatting, local plan links and diff checks passed. Review found
no new re-exports, production globs, compatibility entrypoints or LOC exceptions.
Construction provenance belongs to executable environments, while immutable type
provenance still owns nominal identity; the two are not conflated. Full workspace
and exhaustive backend validation remain CI-owned.


CI03 readonly host protocol checkpoint (also the exact receiver declaration part
of CI04): the stdlib now declares readable Map/Set/Iterable and conditional
PartialEq/Eq/Hash implementations for the existing readonly concrete host Map/Set
descriptors. These reuse library algorithms and selected callbacks, with the
same element/key/value bounds as writable descriptors. MutableMap/MutableSet and
allocating inherent operations remain attached to the writable receiver only.
The legacy concrete host descriptors remain until NC replaces that representation;
this work does not add readonly source syntax or interface capability markers.

The registered declaration renderer previously rejected readonly descriptors.
After exact source/registration correspondence, the importer now attaches readonly
access to the corresponding concrete type occurrences. Ordinary name and generic
resolution still runs before applying this descriptor. Traversal covers nested
parameter/result types, implementation receivers and outputs, trait parents and
associated constraints, enum payloads and rendered where predicates. Signature
reuse compares this metadata as well as the declaration text. Authority stays with
the registered type descriptor, not a displayed collection name.

The first declaration installation then exposed a second defect: HIR overlap
checking treated writable and readonly generic Map/Set receivers as identical.
It now distinguishes their access descriptors. Source and executable implementation
matching require exact receiver access, including nested generic arguments; neither
trait storage_access nor Iterable/Index language roles relax a match. Existing
outer host adaptation remains at its host boundary. Removing the now-unused
ImplementationPattern marker fields remains the coordinated CI05 schema cleanup.

The decoded readonly_host_hash_storage_has_exact_read_and_content_protocols
case passed with forced GC: readonly host results satisfy ordinary Eq/Hash bounds,
execute reads and concrete content equality/hash, preserve equality and identity
through Map/Set interfaces, and support explicit content helpers. Writes through
the original aliases remain visible. Direct mutation and writable interface
upgrades are rejected; roots and objects are released after execution. The
native_collection_views case and both executable types::matching::tests passed.
The matching test now checks exact descriptor admission regardless of legacy
trait markers, retaining repeated-binder, associated-family and cancellation coverage.

Focused HIR tests initially failed to build because renderer tests had implicitly
inherited a removed production CollectionAccess import; they now import it at the
test owner. The nested-result fixture initially used unsupported tuple `.0` syntax
and was corrected to indexing. The existing native-view trivia fixture also passed
independent provider identity scopes directly to an importer requiring an adopted
batch; it now uses the production adoption step before testing trivia equivalence.
These fixture repairs do not relax source or registration validation.

The broader selected collection_access run found one obsolete assertion granting
hash() to an ordinary List view. It was removed from the alias/closure/join case:
collection_interfaces already rejects List.hash() and HashSet<List<T>>, and checks
concrete Vec equality/hash agreement. All alias, identity, object-equality, branch,
closure and shallow element mutation assertions remain in the access case.

Final focused results for this checkpoint: collection_interfaces passed 10 cases;
collection_access passed 5 initially and its corrected alias/closure/join case
passed on the focused rerun (all 6 selected cases completed). The 6 native::api
HIR tests passed, including nested readonly result access and registration tamper
rejection. Both types::matching tests passed again after recursive access guards.
HIR/types/stdlib library Clippy passed. Structure checked 1099 Rust files with no
violations or exceptions; formatting, plan links and diff checks passed. Structural
review kept descriptor attachment in native import, exact matching in source and
executable type owners, and protocol policy in stdlib. No re-exports, production
globs, compatibility wrappers or version bumps were introduced.

CI03 remains open for compound receivers whose nominal components are supplied by
different generations. CI04 still owns ordinary script/native relationship and
multi-arm join discovery; CI05 removes the obsolete marker fields. This checkpoint
closes the readonly host comparison/protocol admission gap, not those phases or
the overall goal. Exhaustive workspace/backend validation remains CI-owned.


CI03 compound-generation reproduction (known failing, owning follow-up: CI03
comparison construction): `generic_reload` now also exercises an old Boxer with
`one<T> -> List<(Local, T)>`. Its retained pack body constructs Local itself while
the new Probe supplies Item as T. Reload changes Local.eq from true to false. Both
old and new probes must observe the old Local.eq when using the retained factory;
Item remains supplied by the respective caller. The original single-argument
nominal and primitive cases share the same runtime/GC harness.

`cargo test -p kagari-embed --test generic_reload shared_collection_comparison_composes -- --nocapture`
failed on the new probe with `interface receiver does not support object comparison`.
The old probe passed before that failure. This is a real CI03 execution defect,
not a fixture issue. The receiver's reified identity correctly records old Local
and new Item. Runtime receiver_comparison_binding compares that entire identity
to closed table applications in retained programs; old and new programs contain
only their own uniform-generation applications, so neither qualifies. Closed
OperationWitness::Selected targets also retain one lexical program, including
structural protocol adapters; weakening the identity check could silently select
the wrong nested callback and is not a fix.

The next CI03 checkpoint must compose checked comparison operations over the
receiver's actual supplying type arguments. Review and reuse shared callable and
operation-environment preparation; carry explicit substitution/child-operation
contracts through compiler and independent executable verification rather than
searching implementations at runtime. Keep this migration finite: replace the
exact-closed-application assumption for generic receiver construction, preserve
existing concrete/primitive/nominal and native-result paths, and remove superseded
lookup scaffolding when its responsibility has moved. The regression must prove
old Local selection and new Item selection independently, plus existing forced-GC
and root/generation-release assertions. No runtime fix or completion is claimed
by this reproduction checkpoint.

After sharing the harness, the two existing shared_collection_comparison nominal
and primitive cases passed again (the unchanged known-failing compound case was
explicitly skipped in that regression run). Formatting and diff checks passed.
The compound test remains enabled and failing pending the owning CI03 repair;
this is intentionally an incomplete test checkpoint, not passing phase acceptance.


CI04 inference checkpoint in progress: the CI03 mixed-generation failure remains
owned by comparison construction. Independent ordinary-interface inference is
proceeding while that error is carried; it uses the existing checked implementation
and member-surface owners and does not depend on compositional receiver comparison.
This sequencing does not waive any CI03 exit requirement or final acceptance gate.

Adding list_content_equal(Sequence, Sequence) to the existing custom collection
fixture reproduced CannotInferGenericArgument for T. Mixed Sequence/Vec calls had
succeeded because Vec supplied the missing inference information. The first probe
also used a struct initializer inside an if condition; it was moved to a local so
the focused failure concerns type inference alone. The two-custom-operand call and
first(source) now execute successfully with the existing Source/Artifact/JIT routes.

Callee inference now receives the caller's checked declarations and generic bounds.
For an interface parameter, it uses the ordinary member/interface query's valid
applied implementations, including script implementations, generic assumptions and
declared parents. It infers against the requested declaration, preserves invariant
arguments and explicit associated bindings, and commits a unique substitution only.
Multiple incompatible applications remain unresolved for the caller's diagnostic;
explicit result/type arguments can select a matching application. Inference does
not grant a conversion: the existing final coercion/proof check still owns that.

For a concrete return pattern with an interface result context, inference first
projects an implementation header and solves receiver arguments, then discharges
that applied implementation's bounds. This preserves generic collection constructor
context without the old readonly marker. matching_storage_headers was deleted.
Implementation/inherent header matching and propagation solver header projection
now request structural substitution only, preventing candidate enumeration from
recursively authorizing conversions while matching the same header. Ordinary
branch/common-interface discovery still uses storage_views/shared_storage_view;
its replacement and whole-expression multi-arm selection remain CI04 work.

Focused acceptance for the inference checkpoint: collection_interfaces passed all
10 cases and collection_access all 6. The custom fixture executes two concrete
Sequence operands without a native operand or interface annotation supplying T,
while retaining both argument orders for mixed implementations. The 6 HIR
inference tests passed after the final helper extraction. Their new source matrix
covers applicable and inapplicable conditional implementations, caller generic
bounds, ambiguous applications, explicit result disambiguation, inference from an
interface return context, and passing an interface through an unconstrained generic
identity function. The 3 shared_generic_method default_methods regressions passed.
These are the selected affected contracts, not full workspace acceptance.

HIR library Clippy, structure (1099 Rust files, no violations or exceptions),
formatting, plan links and diff checks passed. Structural review separates ordinary
argument/result inference from header substitution and reuses the existing checked
member and implementation queries. No new cache, public re-export, compatibility
alias or executable format change was introduced. CI03's enabled mixed-generation
comparison reproduction remains failing and was not rerun unchanged. CI04 remains
open for common-interface candidate discovery, whole-expression joins, operator
and indexing consumers; CI05-CI06 remain open. Full workspace and exhaustive
backend coverage remain CI-owned.


CI04 common-interface checkpoint: ordinary checked applications now have a shared
[relationship owner](../crates/kagari-hir/src/aggregates/relationships.rs).
Member lookup and inference reuse declared implementation discovery; branch joins
and equality/identity operand compatibility no longer use shared_storage_view,
which was deleted. Candidate enumeration checks exact receiver substitution and
conditional implementation bounds under caller assumptions. Interface/generic
inputs retain declared ancestry and associated outputs. Concrete receivers use
separately proven parent implementations and their actual associated outputs;
intrinsic operator availability is not an implicit interface conversion candidate.
The marker-based has_storage_view identity eligibility predicate remains a CI05
consumer; this checkpoint does not replace that independent semantic boundary.

[Branch inference](../crates/kagari-hir/src/typeck/body/joins.rs) now collects all
reachable, normally completing match arms with their checked environments before
intersecting interface candidates. It removes dominated parents only after the
intersection, then checks and records every final conversion. Equal concrete types
and valid outer host access weakening retain priority. Earlier arm types may still
supply contextual literal/constructor facts, but an intermediate interface join
cannot discard alternatives. If branches use the same checked relationship query.
Ambiguity reports KG_TYPE_AMBIGUOUS_COMMON_INTERFACE with the candidate names and
an explicit-result-type suggestion, instead of silently picking a catalog order.

The source matrix first reproduced A/B/C rejection when A and B each inherit X/Y
and C inherits only X. All six permutations now select X. Tests cover ambiguous
A/B pairs in both orders, explicit X/Y annotations, most-specific-parent selection,
if/match joins, unreachable/non-completing arms, contextual numeric and Option
results, mutable concrete host-family preservation, invariant arguments, associated
outputs, and conditional implementations with/without caller Eq evidence. The new
execution fixture covers inferred interface upcasts and concrete conditional generic
implementations through Source/Artifact/JIT with forced GC; branch-producing calls
are observed exactly once.

An intermediate collection_interfaces run passed 8 cases and failed 3. One failure
was a fixture struct initializer placed directly inside an if condition; moving it
to a local restored the intended generic conversion test. Two failures exposed false
iter ambiguity from mixing concrete and erased inherited iterator outputs. Candidate
discovery now keeps concrete outputs at their implementation owner. The repaired
collection_interfaces target passed all 11 cases. collection_access passed all 6;
its readonly admission and source-free forged-upgrade guarantees remain intact.
The initial HIR context matrix also used an unsupported Option.unwrap method;
its assertion now checks the inferred Option<i32> type directly.

CI03's enabled mixed-generation comparison construction regression is unchanged
and remains failing; it was not rerun without a repair. CI04 still owns remaining
indexing/relationship consumers and phase acceptance, and CI05-CI06 remain open.
This checkpoint introduces no runtime specialization, cache, public re-export,
compatibility model or executable format revision. Focused validation continues
below; full-workspace and exhaustive backend validation remain CI-owned.


The final one-way-conversion review preserves an existing operand's interface
before inventing a common target. This allows Vec<i32>/Iterable<Item = i32> arm
orders in HIR without erasing unrelated associated-output constraints. The
additional enabled inferred_iterable_join_preserves_iterator_result_adaptation
execution test exposes an existing representation boundary that remains CI04 work:
`cargo test -p kagari-embed --test collection_interfaces inferred_iterable_join -- --nocapture`
fails BytecodeLowering verification with "invalid collection access flow".
The implementation constructs Iterable with Item = i32 and concrete Iter = Iter<i32>,
while the authored parameter/result exposes only Item = i32. The separately retained
dynamic iterator view instead exposes Iter = Iterator<Item = i32>. Treating these
three contracts as identical is invalid. An attempted explicit upcast confirmed the
mismatch at MIR InterfaceContract verification; merely admitting that upcast would
also require a correctly retained runtime view/result adapter. The speculative
compiler/verifier/linker edits and temporary diagnostic output were removed.

The failing test is separate from the passing custom-interface/generic-implementation
fixture and remains enabled. Finish the canonical authored/dynamic interface contract
and its checked construction/adaptation path coherently; preserve exact associated
bindings, iterator result adaptation, source-free verification and runtime lifetime
checks. Do not fix it by relabeling registers, dropping associated outputs in general,
or weakening validators. This is a carried CI04 error, not a completed phase gate.


Final focused evidence for this checkpoint: 30 distinct selected cases passed
(collection_interfaces 11, collection_access 6, HIR joins/completion 7, HIR inference
6). The extended context matrix passed again with both concrete/Iterable arm orders.
After removing the speculative adaptation edits, collection_interfaces passed its
11 other cases with the enabled Iterable reproduction explicitly filtered out;
the isolated reproduction was run separately and still failed as recorded above.
No skipped failure is counted as passing. The existing CI03 compound-generation
failure remains independently outstanding. HIR/source library Clippy, structure
(1102 Rust files, no violations or exceptions), formatting, relative plan link
targets and diff checks passed. Structural review kept declaration proof discovery,
member policy and branch inference in their existing ownership layers; no new
re-exports, visibility bridges, unchecked conversions or production placeholders
remain. This is a coherent but incomplete CI04 checkpoint; CI acceptance is unrun.


CI04 iterator-value adaptation checkpoint: the enabled
inferred_iterable_join_preserves_iterator_result_adaptation reproduction is now
resolved. A concrete implementation still constructs its exact Iterable output
(e.g. Iter<i32>); source lowering then emits a checked UpcastInterface to the
bounded Iterator<Item = i32> value view. It no longer labels the raw construction
as the authored interface value. MIR and bytecode verification share the exact
permitted targets at the types ancestry owner, including only the existing bounded
iterator-result adaptation. Other explicit associated outputs remain invariant.
The linker retains the corresponding view of the same implementation and its
checked result adapter through the existing parent-link mechanism. Runtime parent
preparation and receiver-comparison retention need no new selection path or cache.

Executable value encoding now canonicalizes omitted iterator outputs consistently
inside signatures, locals/registers, closures, nominal/layout arguments, callable
requirements and implementation identities. Requirement roots remain constraints:
T: Iterable<Item = i32> does not acquire an Iter = Iterator<Item = i32> equality
requirement. Only its nested value arguments use canonical value encoding.
Associated projections remain separate checked normalization facts. Source
substitutions and implementation selection stay at HIR; encoding runs after those
steps. Implementation matching compares canonical nested values on both sides when
checked operation requirements are raised back from executable types, preserving
exact root constraints, access, conditional bounds and explicit outputs.

Intermediate verification exposed two boundary errors during this repair. Applying
value normalization to a default method's constraint root caused InvalidPublicAbi
for the standard Iterable implementation over Iterator; constraint encoding now
preserves the root. Canonicalizing InstancePlanner.arguments before implementation
selection caused UnsupportedExpr("interface dispatch requires linked implementation
tables") for Reader<Iterable<Item = i32>>. Keeping source substitutions separate
from executable encoding resolves that case, including generic default-method
bounds, ordinary bounded calls and closure results. Temporary verifier diagnostics
were removed; no assertion or executable validation was weakened.

The existing reproduction now executes both branch choices, a generic interface
method returning a closure, and a nested Reader<Iterable<Item = i32>> implementation
through ordinary and default generic calls. Source, artifact and ArtifactJit routes
retain forced-GC execution. A distinct source-free case rejects both a forged
Iterator item type and replacing the bounded iterator result with a scalar.
collection_interfaces passed all 13 cases and collection_access all 6; the former
carried CI04 failure is included, with no filtering or disabled case. Further
selected regression and structural evidence is recorded below. CI03's enabled
mixed-generation compound-comparison failure remains independently outstanding
and was not rerun unchanged. CI04 indexing/relationship acceptance and CI05-CI06
remain open; this checkpoint does not claim goal or phase completion.


Final selected evidence for this checkpoint: 43 distinct tests passed across
collection_interfaces (13), collection_access (6), default_methods upcast (2),
HIR joins/completion (7), HIR inference/recovery (14), and the existing runtime
rejected_parent_publication_keeps_the_cache_empty_until_valid_upcast case (1).
These target exact output adaptation, ordinary invariant matching, conditional
inference, source-free rejection and retained parent lifetime boundaries affected
by this change. The 19 collection cases remain evidence after the subsequent
warning-only budget-variable cleanup and formatting. Scoped library Clippy passed
for compiler, bytecode, MIR, HIR and types. Structure checked 1102 Rust files with
zero violations/exceptions; formatting, plan links and diff checks passed.

Structural review keeps source substitutions at HIR, value encoding at compiler
lowering, permitted adaptation at the shared types ancestry owner, and retained
view/result links at bytecode linking. No runtime trait search, executable code
specialization, new cache, public re-export, compatibility reader or format revision
was added. Full workspace and exhaustive feature/backend acceptance remain CI-owned
and unrun locally. The unrelated roadmap working-tree edit is excluded.


CI04 indexing checkpoint: assignment now uses the ordinary checked
Index result and records the applied write interface together with the exact
setter method identity in TypeTable. Source lowering consumes that pair through
ordinary applied method lowering for concrete, generic and interface receivers.
The separate InterfaceIndex path and compiler-side collection/member discovery
were deleted. Readonly element access remains a read and does not grant container
writeback; builtin Array element context and intrinsic mutation remain separate.

The existing bounded assignment spelling resolves the symbolic declaration
std::collections::MutableList.set in the retained definition table, then requires
an applicable ordinary implementation/parent and validates the substituted setter
parameters/result and absence of additional method bounds. This is only the source
syntax binding: arbitrary same-named traits/methods gain no indexing privilege,
and native storage grants no method or write authority. Index reads and recovery
use ordinary Index contracts. A malformed index retains a unique checked result
for tooling while its diagnostic still prevents code generation.

Deleted array_interfaces/native_array_interfaces discovery and all copies from
native import, lowering, imports, declarations and identity/cache visitors. The
old native-storage/access-marker discovery no longer participates in indexing.
The renderer/adoption tests retain scoped declaration/navigation checks; assertions
about the deleted private discovery table were removed. The Array bridge now owns
only intrinsic element context. TypeTable setter identities are remapped/visited
alongside their applied interface, preserving scoped identity validation.

The initial affected run passed 16 cases and failed 3 legitimate write cases with
KG_TYPE_INVALID_ASSIGNMENT_TARGET. The new signature guard incorrectly treated a
generic parameter's empty constraint-list entry as a method bound and expected
unit to use the tuple representation instead of HIR Builtin(Unit). Both guards now
match the actual declaration model; all three failing cases passed in focused
reruns. No assertion or verifier was weakened, and temporary diagnostic output was
removed. The custom-container fixture now covers generic element replacement and
an inherited writable interface, while the rejection matrix covers an unrelated
same-named MutableList/set declaration. Remaining focused evidence follows below.
CI03's mixed-generation comparison failure remains independently outstanding;
CI05 marker/protocol cleanup and CI06 handoff remain open.


Final selected evidence for the indexing checkpoint: 30 distinct tests passed.
The 19 collection_interfaces/collection_access cases completed across the initial
16 passes and the three repaired cases; no failed case is hidden by filtering.
After adding ordinary Index error recovery, both readonly rejection matrices
passed again. The 5 selected HIR cases cover partial-index recovery, receiver/index
completion, provider adoption and canonical native navigation. The 5 selected
operator_traits cases cover readonly object results, rejected replacement/immutable
writes, once-only left-to-right evaluation, actual integer arguments and checked
native Index dispatch. The fixed_arrays_preserve_aliases_and_explicit_constructors_copy_storage
case covers builtin Array separation and generic/concrete target capture before
index/RHS alias rebinding. Existing Source/Artifact/ArtifactJit assertions and GC
checks remain intact.

Removed the unreachable access-flag branch after place checking has already
restricted the receiver to Array/Tuple: TypeId::collection_access only describes
host/storage Map/Set. The remaining place diagnostic may describe that host boundary;
it does not authorize writes. HIR/compiler library Clippy, structure (1102 Rust
files, no violations or exceptions), formatting, plan links and diff checks passed.
Structural review confirmed ordinary checked read/write calls, exact setter identity
mapping, no compiler member search, no new runtime policy/cache, no re-exports and
no ABI/wire version change. The unpublished HIR helper/getter/visitor surface changes
replace their old consumers directly. Source-free validators remain unchanged and
the forged-access case passed. Full workspace/feature/backend coverage remains
CI-owned; CI03 and CI05-CI06 work is not claimed complete.

CI03 identity/comparison separation checkpoint: the mixed-generation fixture now
tests both sides of the operation provenance independently. One replacement changes
Local.eq and must still use the retained body's old Local operation; another also
changes Item.eq and must use the new caller's Item operation. The command
`cargo test -p kagari-embed --test generic_reload shared_collection_comparison_composes -- --nocapture`
ran both enabled cases; both still fail with "interface receiver does not support
object comparison". No production comparison-selection change is claimed here.

The same construction exposed a separate identity error: object_data required an
available PartialEq application before unwrapping an already validated object
interface. Consequently ===/!== failed even though they invoke no comparison
operation. The new shared_collection_identity_does_not_require_a_comparison_application
case first reproduced "identity comparison requires object references" and then
passed after removing that dependency. Publication still validates the concrete
receiver, object_receiver still restricts the eligible categories, and identity_equal
still validates heap ownership, generation and object kind. The helper is now private
to its sole consumer module. Static interface eligibility and Eq/Hash proofs are
unchanged. The fixture checks same/different objects under both operators, forced
GC and release of old-generation runtime values.

The remaining comparison repair belongs to checked operation composition. Current
Selected witnesses and generated protocol adapters are closed applications; retaining
more program roots cannot create a checked operation for an old/new compound type.
The existing shared-entry model already canonicalizes native parameters, so a wider
native parameter collector is not an established fix. Preserve exact supplying
identities and use checked shared operation arguments for structural composition;
do not reuse an old closed tuple adapter whose child calls select the old Item.
Keep compiler selection, source-free application verification and runtime binding
in their existing owners. This prerequisite does not authorize runtime trait search
or treating structural type equality as generation equality.

Focused validation for this separation passed four cases: the new mixed-generation
identity case, boxed_scalar_does_not_acquire_object_identity, the existing
identity_comparison_rejects_foreign_stale_and_disguised_handles, and
receiver_identity_survives_upcasts_branches_and_concrete_hash_storage (its existing
source/artifact/JIT routes). The two compound equality cases above remain enabled
and failing; they are not counted as passes. Runtime library Clippy, structure
(1102 Rust files, zero violations/exceptions), formatting, plan links and diff
checks passed. Structural review found no new public API, re-export, cache or
runtime selection path. Full workspace and exhaustive backend checks remain CI-owned
and unrun; CI03 and the overall goal remain incomplete.

CI03 shared receiver operation checkpoint: SharedMethod witnesses now also select
an ordinary non-generic member on a receiver that contains caller binders, when
the source catalog proves that implementation without additional assumptions.
For example, an unconditional PartialEq implementation for Holder<T> can supply
the operation to same(Holder<T>, Holder<T>) inside a shared generic body. Previously
lowering emitted Forward even though the caller had no such bound, producing
InvalidGraph. Source-free proof now checks the ordinary member signature and
eligibility in the actual parameter/assumption scope, then checks the exact selected
implementation, substitutions and bounds through the existing table contract.
The existing generic-member path and wire representation remain in use.

Executing that reproduction exposed a second error: generic == lowering emitted
primitive value equality instead of invoking the already supplied PartialEq
operation. Generic protocol lowering now emits the ordinary checked constrained
member call. Concrete and erased-interface paths retain their existing selection
rules. The shared-call fixture passes with distinct integer and string arguments;
ordinary source, decoded artifact and ArtifactJit routes preserve the explicit
Holder equality instead of object identity.

Interface construction can retain the same proven shared selection. A generic
Bag<T> with unconditional PartialEq, constructed by an old factory with an old
Local/new Item tuple parameter, now compares using the retained old Bag operation.
The enabled shared_collection_comparison_retains_a_generic_receiver_selection case
first reproduced the missing-comparison trap and now passes, including forced GC
and old-generation release checks. MIR and bytecode verification retain the
template's parameter scope separately from its instantiated contract; instantiation
removes declaration binders and must not erase the scope of symbolic arguments.
Runtime construction uses this verified selection's existing environment directly;
comparison resolves its operation requirement in that environment before lookup.
It does not search for implementations or invoke an incompatible closed target.

The two enabled Vec<(old Local, new Item)> failures remain owned by CI03: Vec's
conditional element bound and the tuple's component operations still require
checked structural composition. Unconstrained templates do not receive fabricated
Forward operations or premature identity defaults. This checkpoint supplies the
shared receiver foundation, not complete mixed-generation conditional comparison.

Selected validation passed 11 distinct cases: five default_methods cases (shared
receiver comparison, generic shared member/override, bounded helper forwarding,
method-local bound operations and forged selections); three collection_interfaces
cases (concrete content/helpers, nested/float content, source-free receiver
comparison evidence); and three generic_reload cases (the new shared receiver,
retained nominal caller and new primitive application). The receiver evidence
matrix now separately targets shared selections, including out-of-scope binders,
without relying on an unrelated malformed concrete witness for rejection.
The expanded shared-method target-removal mutation initially removed no table
because application binders differ from canonical table binders; it now asserts
that the named implementation exists and removes its tables before checking
rejection. All original rejection cases remain exercised.

After changing receiver selection, the two compound Vec cases were rerun with
the nominal/primitive generation regressions: two passed and the same two failed
with "interface receiver does not support object comparison". Those failures
remain enabled and are not counted as passing. Temporary verifier diagnostics
were removed. Scoped contract/compiler/MIR/bytecode/runtime library Clippy,
structure (1102 Rust files, zero violations/exceptions), formatting, plan links
and diff checks passed. Structural review keeps selection at source, eligibility
and scope proof at the contract owner, and binding at the existing runtime call
owner. No new witness variant, cache, public re-export, wire revision or runtime
trait search was added. Exhaustive workspace/backend coverage remains CI-owned
and unrun; CI03 and CI05-CI06 remain open.
