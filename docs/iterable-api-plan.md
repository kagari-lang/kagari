# Unified Iterable API execution plan

Status: IA01–IA04 are complete; IA05 integration and final acceptance are active. The
[roadmap](implementation-roadmap.md#unified-iterable-api) owns queue placement.
This file owns the finite API inventory, phase order, acceptance and progress
ledger. Examples below describe the target API, not currently implemented calls.

## Outcome and scope

Provide ordinary method syntax for a uniform set of lazy transformations and
terminal operations on every checked `Iterable`. Put their default methods on
`Iterable`; keep `Iterator` responsible for `Item` and `next()`. Reuse the existing
identity adaptation through which an Iterator is Iterable with `Iter = Self`.
Do not duplicate the API on both traits or implement it separately per container.

```kagari
val result: Vec<i32> = [1, 2, 3, 4]
    .filter(|x| x > 1)
    .map(|x| x * 10)
    .take(2)
    .collect(); // Contains 20, 30.

val total: i32 = values.filter(|x| x > 0).sum();
val found: Option<i32> = values.find(|x| x > 10);
val accumulated = values.fold(0, |acc, x| acc + x);
```

Transformations return lazy library iterators. Terminal operations traverse the
pipeline; `collect` explicitly constructs the requested destination. Preserve
Kagari's value/shared-identity model, checked arithmetic, synchronous callbacks,
cooperative cancellation, scoped iteration protection and pinned execution
generations. No borrow or lifetime syntax is added.

This work follows the fixed-array separation and review fixes at `acdcbd32`.
It does not reopen SA18/SA19/SA21, change Array/List conversions, introduce a
general extension-method language feature, or migrate HashMap/HashSet storage.
Exclude asynchronous iteration, parallel traversal, stream fusion, size-hint
optimization, replayable query objects, `flat_map`/`flatten`, `try_*`, grouping,
windows/chunks and set algebra. These are not acceptance requirements. There is
no performance target or claim that adapter allocation/dispatch is free.

## Existing implementation and ownership

| Owner | Current behavior and required work |
| --- | --- |
| [Protocol declarations](../crates/kagari-stdlib/src/catalog/roles.rs) | Iterable declares Item, Iter and iter; Iterator declares Item and next. Add the default method surface without making custom implementers repeat it. |
| [List defaults](../crates/kagari-stdlib/src/catalog/list_methods.rs), [native default contract](native-default-bodies-plan.md) | Reuse checked native default recipes and generated forwarding bodies. Defaults, overrides, source checking and source-free proofs must agree. |
| [HIR iteration semantics](../crates/kagari-hir/src/language/semantics.rs), [method checking](../crates/kagari-hir/src/typeck/body/methods.rs), [portable adapters](../crates/kagari-types/src/language/adapter.rs) | Iterator-to-Iterable proof/identity adaptation already exists. This alone does not establish lookup, generic defaults or interface dispatch for new methods; complete those paths coherently. |
| [Mapping replacement](../crates/kagari-stdlib/src/bindings/iterable.rs) | The free function accepts only Vec and stores a NativeCursor created through sequence_cursor. Replace this source-specific path with the general iterable method and checked retained iterator state. |
| [Construction declarations](../crates/kagari-stdlib/src/catalog/construction.rs), [selected iteration requirements](../crates/kagari-stdlib/src/catalog/construction_defaults.rs), [implementations](../crates/kagari-stdlib/src/bindings/construction.rs) | FromIterator, Sum and Product already consume generic Iterable inputs. Delegate to these contracts rather than implement competing aggregation/construction algorithms. |
| [Native call context](../crates/kagari-runtime/src/native/context.rs), [stored callbacks](../crates/kagari-runtime/src/native/callable.rs), [stored selections](../crates/kagari-runtime/src/native/stored_selection.rs) | Selected calls are scoped to an invocation; StoredCallable currently retains a traced closure. Audit how to retain a checked source next operation and its supplying generation between calls. Keep policy in stdlib and any necessary retained-call capability in runtime. |
| [Collection semantics](spec/collection-access.md), [iteration semantics](spec/builtins.md#iteration-protocols) | Preserve source order, aliases, structural mutation guards and existing collection ownership. Update implemented specifications only as phases land. |

Declare adapter types in the iteration library owner, `core::iter`, with normal
checked provider dependencies and the existing standard namespace exports.
Implementation code belongs to focused stdlib iteration modules. Collection
constructors stay with their collection owner. HIR, runtime value enums and
backends must not acquire a special case for each adapter type.

## Trait surface and typing

The following is signature notation: `T` means `Self::Item`; every listed method
has a `self` receiver and a default implementation. Callback spellings use the
existing function-value model and ordinary closure coercion. Angle-bracket
applications describe types, not new syntax.

| Iterable method | Result and constraints | Phase |
| --- | --- | --- |
| `map<U>(mapper: fn(T) -> U)` | `MapIterator<T, U>`, Item = U | IA02 |
| `filter(predicate: fn(T) -> bool)` | `FilterIterator<T>`, Item = T | IA02 |
| `take(count: usize)` | `TakeIterator<T>`, Item = T | IA02 |
| `collect<C>()` | C, where C: FromIterator<T> | IA03 |
| `fold<A>(initial: A, combine: fn(A, T) -> A)` | A | IA03 |
| `sum<S>()` | S, where S: Sum<T> | IA03 |
| `product<P>()` | P, where P: Product<T> | IA03 |
| `filter_map<U>(mapper: fn(T) -> Option<U>)` | `FilterMapIterator<T, U>`, Item = U | IA04 |
| `skip(count: usize)` | `SkipIterator<T>`, Item = T | IA04 |
| `enumerate()` | `EnumerateIterator<T>`, Item = (usize, T) | IA04 |
| `zip<I: Iterable>(other: I)` | `ZipIterator<T, I::Item>`, Item = (T, I::Item) | IA04 |
| `chain<I: Iterable<Item = T>>(other: I)` | `ChainIterator<T>`, Item = T | IA04 |
| `count()` | usize | IA04 |
| `find(predicate: fn(T) -> bool)` | Option<T> | IA04 |
| `any(predicate: fn(T) -> bool)` / `all(predicate: fn(T) -> bool)` | bool | IA04 |
| `for_each(action: fn(T) -> ())` | () | IA04 |

Adapter types expose element type parameters, not a hidden concrete receiver's
Self type. Their payload may erase the source representation only while retaining
its checked item type, invocation evidence and generation. This keeps default
methods usable through existing collection and iteration interfaces. IA01 must
verify this declaration shape against interface compatibility and source-free
validation; a representation adjustment must be recorded here before IA02.

Method result inference uses ordinary context, for example
`val output: Vec<i32> = source.collect()` and `val n: i32 = source.sum()`.
Ambiguous results require an annotation or existing explicit generic arguments;
do not silently choose Vec or i32. Hash bounds belong to HashSet/HashMap
construction, not Iterable or map. Map's Item remains `(K, V)`; Set's remains T.
Callbacks receive ordinary Kagari values, so object elements retain identity.

Native and script types, generic bounds, inherited List/Set/Map interfaces and
erased Iterator values must all see the same declared methods. Preserve ordinary
method precedence and explicit trait overrides. Do not rewrite a token named
`map` into a special compiler operation or bypass overrides through interfaces.
Do not turn Iterator and Iterable into mutually recursive supertraits.

## Evaluation, state and examples

For the default implementations on a container, these calls have equivalent
traversal behavior:

```kagari
values.map(f)
values.iter().map(f)
```

The former obtains one iterator by calling values.iter(); the latter obtains it
explicitly, then uses identity Iterable on that cursor. Iterator adaptation
returns the same cursor without resetting or copying its position. Ordinary
user overrides/inherent method precedence still apply; these examples do not
justify forcibly bypassing an override.

```kagari
val values = [10, 20, 30];
val cursor = values.iter();
cursor.next();                         // Consumes 10.
val remaining = cursor.map(|x| x * 2); // Produces 40, 60 when consumed.
val all = values.map(|x| x * 2);       // New array cursor: 20, 40, 60.
val alias = remaining;                // Shared adapter/source progress.
```

Apply these rules to the entire inventory:

- Evaluate the receiver and explicit arguments left to right, once. Adapter
  construction then obtains source iterators in source order, once, and retains
  them. A custom iter() may have effects at construction; no next or element
  callback runs until consumption. No eager snapshot or element copying occurs.
- A successful source next consumes that item before a mapper/predicate runs.
  Callbacks run synchronously, once for each item they inspect. On callback
  failure, completed source advancement and earlier side effects remain.
- Filter/filter_map may pull multiple source items for one output; poll
  cancellation while skipping rejected items, even if no output is produced.
  Take(0) does not pull an item or call an element callback. Take never requests
  another item after its output allowance is exhausted; skip discards only its
  initial requested items. Enumerator positions and count use checked usize
  arithmetic, without wrapping or numeric widening of elements.
- Zip pulls left then right and yields pairs while both succeed. If the right
  side returns None, a successfully pulled left item has already been consumed;
  no rewind is promised. Chain switches permanently to its second cursor when
  the first returns None. Do not prefetch an extra item after a terminal operation
  has obtained its answer.
- Terminal traversal stops on the first None. Preserve the existing Iterator
  protocol's exhaustion/resumption rules; do not require every custom cursor to
  be globally fused. Map/filter/filter_map do not add an implicit permanent
  exhaustion latch. Adapter-owned state, such as an exhausted take allowance or
  chain's completed first input, remains in effect across aliases and calls.
- Find returns the first matching item or None. Any stops at the first true;
  all stops at the first false. Their empty results are false and true,
  respectively. Fold returns its initial value on empty input; count returns 0;
  for_each returns unit. Sum/Product retain their destination-defined empty
  identities and the existing scalar overflow behavior.
- Transformations preserve the source's traversal order. They do not impose
  order on HashSet/HashMap or perform implicit deduplication. A stored pipeline
  is a cursor, not a replayable recipe or a snapshot; aliases and later source
  mutations follow existing iteration rules.

List sorted/reversed/distinct and MutableList sort/reverse/retain/dedup remain at
their existing owners. Iterator count traverses; collection len reads length.
This task does not duplicate or silently change those APIs.

## Construction and aggregation

The Iterable methods select existing destination contracts with checked type
arguments: collect calls C::from_iter, sum calls S::sum, product calls P::product.
Delegation must not acquire/discard an extra source iterator. Preserve custom
destination implementations and their ordinary checked dispatch.

IA03 first completes Vec collection. IA04 adds ordinary implementations of
FromIterator<T> for HashSet<T> and FromIterator<(K, V)> for HashMap<K, V>:

```kagari
val numbers: Vec<i32> = source.map(|x| x * 2).collect();
val unique: HashSet<i32> = source.map(|x| x * 2).collect();
val indexed: HashMap<usize, i32> = source.enumerate().collect();
```

Vec preserves order and duplicates; HashSet requires T: Eq + Hash and discards
equal duplicates; HashMap requires K: Eq + Hash and keeps the last input value
for an equal key. Use existing checked insertion/hash/equality mechanisms,
including custom callbacks and cancellation. Collections have independent
storage; referenced elements retain shared identity. Failure returns no partial
collection, releases temporary roots/guards, and preserves completed callbacks.
No implicit Array/List conversion, default interface allocator or builtin array
collection constructor is introduced by collect.

## Retained state, safety and execution boundaries

Adapter state must retain the source cursor(s), checked next invocation(s),
callbacks, and any counters/phase flags. Runtime owns invocation lifetime and
generation validation; stdlib owns adapter policy. Do not store borrowed
CallContext, borrowed selected-call entries, Rust references into heap payloads,
or unrooted values across allocation/reentry. Reuse existing retained-call
capabilities; add a bounded owned descriptor only if the inventory proves a gap.
Do not perform name lookup or type inference on each next call.

Trace source and closure edges, root yielded items/intermediate accumulators
before allocation or callback reentry, and retain the original source/callback
generations across reload. Preserve rejection of recursive advancement of the
same active adapter, while permitting operations on unrelated iterators. Release
payload borrows before invoking script/native callbacks.

Both for-loops and terminal methods must use scoped iteration protection for
all retained source dependencies, including nested adapters, zip/chain and erased
interfaces. Unwind the same protection on exhaustion, short circuit, break,
traps, cancellation and depth exhaustion. Do not hold an iteration guard for an
adapter's entire GC lifetime or depend on finalization to release it. Ending a
consumer scope must not reset shared cursor progress or arbitrarily invalidate
an alias. Native terminal traversal must share the existing scope mechanism,
not implement a second independent guard policy.

Direct iter/next calls retain the existing root-session guard behavior: release
on None or session exit, with cleanup independent of GC. A retained cursor can
resume in another call, but structural changes to its source invalidate that
resume under the existing rules. Custom iterators without declared resource
edges retain responsibility for their own consistency; adapters must propagate
declared dependencies rather than invent storage access for arbitrary objects.

Preserve the reviewed standard-Vec fast paths only under their validated
semantic preconditions. NativeObject, GcHandle or sequence layout alone does
not authorize bypassing selected iter/next callbacks. Do not add caches,
adapter fusion or specialization merely on an assumed performance benefit.

## Migration and phases

Replace std::collections::map and its Vec-only implementation directly when the
new method path lands. Migrate examples, tests, generated source expectations,
tooling navigation and artifact producers in the same coherent checkpoint. Keep
no forwarding free function or second mapping implementation for unpublished
compatibility. Follow existing provider export policy for the canonical adapter
types; do not preserve the former collections-owned identity as an alias.

| Phase | Scope and exit condition |
| --- | --- |
| IA01: inventory and contract proof | Inspect method lookup, reserved protocol validation, identity adapters, generic defaults, interface tables and source-free proofs. Record the accepted adapter declaration/retained-call representation and concrete owner gaps here. Establish focused reproductions/proofs for container, generic and erased-cursor method calls. No unimplemented methods exposed as working APIs. |
| IA02: lazy transformation foundation | Implement Iterable defaults and complete identity adaptation for map/filter/take on all source families. Replace Vec-only mapping and migrate callers. Validate generic/associated item typing, laziness, once-only callbacks, retained state, scope cleanup and source-free method evidence. |
| IA03: terminal construction and reduction | Implement collect/fold/sum/product using the existing destination contracts, with Vec collection. Complete short-lived traversal scopes, accumulator rooting and custom destination dispatch. Accept a filter-map-take-collect/sum pipeline end to end. |
| IA04: finite API completion | Add the remaining table rows and HashSet/HashMap FromIterator implementations. Validate short circuit, left-to-right multi-source advancement, overflow and custom key callbacks. Do not expand the inventory opportunistically. |
| IA05: integration and acceptance | Complete documentation/tooling/examples, remove superseded mapping paths, refresh affected development artifacts once, and run final local acceptance. Report CI/backend acceptance separately. |

Default to buildable checkpoints; group declaration, implementation and caller
migration when they form one inseparable change. If an intermediate boundary
does not build, attempt the relevant check and record its command, representative
diagnostic, cause and owning follow-up phase here and in the commit body. No
production stubs, weakened assertions or disabled contract checks. Keep ABI/format
identifiers; unpublished changes do not require routine version bumps.

Implementation commits use `Plan: iterable-api-plan IA0N` trailers for their
owning phases. Update this ledger at each checkpoint; do not create a parallel
progress document or a new review queue for the same work.

## Verification

Reuse the existing semantic owners rather than add one smoke suite per adapter:

| Coverage owner | Distinct contracts |
| --- | --- |
| [SDK iteration](../crates/kagari-embed/tests/iteration_traits.rs) | Array/Vec/Set/Map, script and native custom Iterable, Iterator identity, inherited interfaces, generic and associated item types, default/override dispatch and inference diagnostics. |
| [SDK conversions](../crates/kagari-embed/tests/conversion_traits.rs) | Destination dispatch, empty results, short circuit, concrete/hash collection construction, overflow, allocated elements/accumulators and root cleanup. |
| [VM collection/mapping tests](../crates/kagari-vm/tests/library_collections.rs), [mapping cases](../crates/kagari-vm/tests/library_mapping/mod.rs) | Shared progress, callback order, recursive next rejection, nested/multi-source guards, cancellation, callback traps and pinned captures. Migrate existing coverage instead of retaining a duplicate old-API suite. |
| [Source-free artifacts](../crates/kagari-embed/tests/artifact_features.rs), [native artifacts](../crates/kagari-embed/tests/native_artifacts.rs) | Portable defaults/selected-call evidence, source-free linking, rejected forged signatures or dependencies, and retained pre-reload cursor/callback execution. |

Use focused selectors during each phase; test public behavior, not payload
layout. Include custom iteration that differs from physical sequence order so a
storage shortcut cannot accidentally pass. Verify take(0), short-circuit callback
counts and mutation after consumer exit. Keep source, serialized artifact and
JIT-enabled/fallback route coverage where it establishes a distinct boundary;
JIT-enabled execution is not proof every operation ran natively.

At implementation checkpoints review imports, ownership, visibility, effective
LOC and re-exports, then run `uv run --locked scripts/check_structure.py` and
`git diff --check`. Run formatting and affected contract tests. Batch full
workspace checks at IA05, not after every phase:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p kagari-embed --no-default-features --test artifact_features
git diff --check
```

GitHub CI owns complete feature/backend acceptance. Record actual local results
separately. Documentation-only planning needs content/link/diff checks, not builds.

## Progress and error ledger

- [x] Record agreed ownership, API inventory, examples and phase acceptance.
- [x] IA01: Inventory and contract proof.
- [x] IA02: Lazy transformation foundation and mapping migration.
- [x] IA03: Terminal construction and reduction.
- [x] IA04: Remaining APIs and hash collection construction.
- [ ] IA05: Integration and final acceptance.

Planning baseline: `acdcbd32`. No implementation changes or build claims are made
by this document. Existing Iterator identity support is evidence to reuse, not
proof that new generic default methods already work. Current implementation
evidence and intermediate findings are recorded below.

Planning validation: the four changed documentation files passed local file-link
checks (152 targets), code-fence and phase-ledger checks, whitespace review and
`git diff --check`. No build or test run is required for this documentation-only
checkpoint.

Design references: [Rust Iterator](https://doc.rust-lang.org/std/iter/trait.Iterator.html)
illustrates lazy adapters and terminal collection;
[Kotlin sequences](https://kotlinlang.org/docs/sequences.html) distinguish lazy
pipelines from eager collection operations. Kagari's accepted surface puts these
methods on Iterable and keeps shared GC cursor identity rather than importing
Rust's borrowing model or introducing a second eager/lazy method family.


### IA01 inventory and accepted representation

Starting checkout: `35de1818`, clean working tree. No unrelated changes existed.
IA01 adds no public methods without implementations.

- Adapter declarations remain element-only nominal native types owned by
  `core::iter`. Their result types contain Item/method parameters, never the
  concrete receiver Self. Container calls retain the actual result of one checked
  `iter` call; Iterator identity adaptation retains the same cursor value.
- Retain checked `next` selections through the existing `NativeObject.selected`
  graph and `StoredSelection`, alongside the cursor value in traced payload state.
  `allocate_result_payload` currently leaves selections empty; IA02 must provide
  bounded slot retention and invocation from the rooted native object. This avoids
  forcing concrete cursors into Iterator interface objects, storing borrowed
  CallContext/selected references, name lookup on next, or payload-owned host roots
  that could leak code/data cycles. Dynamic interface sources retain their existing
  selected interface call; concrete sources retain their concrete selected target.
- `NativePayload::iteration_sources` already recursively propagates cursor/source
  dependencies through native wrappers and interface views. For-loop and native
  terminal consumption must both use `begin_collection_iteration`; scoped guards
  end without resetting shared progress. Native aggregation currently relies on
  session cleanup for generic cursors; IA03 must add consumer-scoped cleanup.
- HIR receiver-method inventory already derives identity Iterable for concrete,
  generic and erased Iterator receivers. Method selection/lowering still needs
  support for Iterable native defaults when there is no explicit Iterable impl.
  Source-free callable proof must validate the same default recipe and Iterator
  obligation, rather than treating every Iterable method as the closed iter adapter.
- Reserved-role shape validation currently demands no defaults and no method
  bounds. IA02 must compare registered default recipes and normalized bounds while
  preserving exact member/binder/parent checks. Ordinary native-default lowering,
  method-local binders and interface dispatch remain the shared foundation.
- The old collections-owned free map retains a sequence-only NativeCursor.
  Replace that implementation and identity directly in IA02. Its private
  intermediate-Option allocation assertion does not establish general Iterable
  semantics; migrate that case to checked custom traversal and behavioral/root
  evidence. Retain the separately reviewed standard-Vec aggregation fast paths;
  sequence layout alone never authorizes skipping selected iter/next callbacks.

Focused evidence passed:

- `cargo test -p kagari-embed --test default_methods generic_interface_default`:
  four generic/interface/default cases, including retained closure environments
  and bounded forwarding.
- `cargo test -p kagari-embed --test list_algorithms generic_custom_receiver_uses_its_associated_iterator_in_default_calls`:
  the existing native default receives the selected associated iterator.
- `cargo test -p kagari-embed --test iteration_traits generic_iterator_bound_also_supplies_for_and_nested_associated_outputs`:
  generic Iterator identity Iterable and associated output use.
- `cargo test -p kagari-contract --test native_defaults hidden_associated_arguments_are_rechecked_against_linked_contracts`:
  source-free proof rejects invalid hidden iterator arguments.

Prototype findings carried to IA02: an element-only script Deferred<T,U> returned
by a generic default on `Preview<T>: Iterable<Item=T>` passes source typing for
concrete, generic and interface calls, but lowering its Iterator implementation
fails `InvalidEnumLayout`. The initial probe also used an invalid inherited member
binding `Preview<Item=i32>`; using the parent's checked Item constraint removed
those diagnostics. Reproduction source/log are under `target/iterable-api/`;
`cargo run -p kagari-cli -- emit target/iterable-api/ia01-prototype.kgr` reproduces
lowering. The temporary test was removed after inventory; actual IA02 adapter
coverage must resolve or isolate this owner gap rather than claim the prototype
passed. No failing test/stub or production API is committed by IA01.

IA01 format/structure/diff and documentation-link checks are recorded with this
checkpoint. Full workspace validation remains reserved for IA05; CI is unrun.


### IA02 lazy transformation foundation

- Added element-only MapIterator/FilterIterator/TakeIterator declarations and
  Iterable native defaults under core::iter. Removed the collections free map
  registration and migrated its consumers/examples/source-free fixture source.
- Native payload allocation retains explicitly requested checked selections as
  metadata graph edges. Stored invocation borrows the containing call's rooted
  argument; it validates source arguments/results against the original target
  generation without a host-owned root lease or per-next member lookup.
- Reserved role validation compares installed default recipes and method bounds.
  Implicit Iterable default selection uses the ordinary native-template proof;
  identity iter remains the closed protocol adapter.
- Erased Iterable result views now align concrete/view method binders before
  comparing signatures. This fixes rejection of generic map<U> while preserving
  exact parameter/result checks. Iterator still has only next.
- The default Iter projection uses the owning Iterable contract rather than an
  extra embedded Item application. This preserves the declared associated-output
  assumption used by source-free default validation.
- Lazy construction, shared progress, non-fused custom sources, take(0), callback
  effects, recursion rejection, guard cleanup and retained generations have
  focused coverage at the existing iteration/mapping contract owners.

Focused checks so far: iteration_traits (14 passed), mapping cases (8 passed),
custom-next plus callback reload (1 passed), lazy construction (1 passed), expected
collection inference (1 passed), and terminating source evaluation (1 passed).
The callable suite initially passed 10/11: native callback composition exposed
an overly broad registration filter that removed existing Vec algorithm overrides.
Restricting that filter to Iterable preserves those overrides; the focused
callback case passed. Generated map navigation/signatures also passed after
updating the expected receiver from Vec to the generic Iterable surface.

The IA01 Deferred prototype is resolved: layout collection omitted nested
function parameter/result types, leaving the generic Deferred read field without
its Option template. Traversing function types preserves strict layout validation
and the original prototype now passes all three execution routes. Its lasting
fixture lives with generic interface default coverage. The simpler direct
Deferred construction also passed during isolation. The obsolete sequence-only
NativeCursor SDK is removed with its sole mapping consumer; payload allocation
callers now explicitly supply retained selected slots. No compatibility alias or
ABI identifier bump is introduced.
Full workspace checks and CI remain reserved for IA05.


IA02 checkpoint validation also passed generic interface defaults (5 cases),
portable native default proofs (13 cases), array/range/container/custom/generic/
interface chaining including explicit custom map overrides, and final lazy-source
checks after obsolete cursor removal. Structural review found no production glob,
re-export, deep parent traversal or borrowed callback/payload state across reentry.
`uv run --locked scripts/check_structure.py`, `cargo fmt --all -- --check`,
changed-document link checks and `git diff --check` passed. No intermediate error
remains. Full workspace/local feature acceptance and CI are still unrun.


### IA03 terminal construction and reduction

- Iterable declares collect<C: FromIterator<Item>>, sum<S: Sum<Item>>,
  product<P: Product<Item>> and fold<A>. Destination defaults retain one checked
  constructor/reducer call with the original source type; they neither call iter
  themselves nor prepare a second unused cursor. Existing implementations remain
  the semantic owner of construction, custom destination behavior and arithmetic.
- Fold obtains its cursor once, roots the cursor/item/accumulator and updates the
  accumulator root before another callback or allocation. It stops at the first
  None and preserves empty initial values and completed callback effects.
- Native generic FromIterator/Sum/Product traversal now holds the shared
  begin_collection_iteration consumer scope. Standard Vec direct traversal and
  scalar reduction keep their validated concrete ownership and receive equivalent
  short-lived structural protection. Scope drop preserves cursor progress and
  releases protections on every exit.
- Existing width/overflow coverage now invokes the terminal methods across array,
  Vec and mapped sources. A custom destination stops after one item; a counted
  Iterable confirms exactly one iter conversion. GC-stressed erased-cursor fold
  uses allocated items and fresh allocated accumulators, and the original lazy
  pipeline case now terminates through sum.

Focused evidence: terminal delegation/allocated fold passed all three execution
routes; the existing native construction subset passed 4 cases; declared-width
traps/root cleanup and invalid destination bounds passed; the generic/custom/interface filter/map/take/sum pipeline passed. Initial test-only parse/import mistakes and
an identity-based Vec equality assertion were corrected to explicit slot checks.
No production stub or weakened semantic assertion was introduced. Full workspace
acceptance and CI remain reserved for IA05.

IA03 structure, formatting, changed-document links and diff checks passed.
No carried implementation or integration error remains at this checkpoint.


### IA04 finite API and paired cursor handoff

- Added filter_map, skip, enumerate, zip, chain, count, find, any, all and
  for_each as Iterable defaults. The eight opaque adapter declarations contain
  only item parameters. Construction obtains left/right cursors once in source
  order and retains their checked next selections. No callback or next is run
  during construction. Zip advances left before right; chain switches permanently
  on the first left None. Other adapters preserve resumable-source behavior.
- Count and enumerate use checked usize addition. Short-circuit terminals root
  each item and share the same consumer guard scope as fold. Nested pairs and
  erased cursors recursively protect both sources; scope exit preserves progress.
- Added ordinary HashSet/HashMap FromIterator implementations through existing
  checked Hash, PartialEq and insert operations. HashMap consumes pair items and
  last values replace earlier values. Custom keys, collisions, allocated values,
  empty destinations and root cleanup are covered at the conversion owner.
- Paired generic methods exposed receiver-insensitive associated output expansion:
  left Self::Item was incorrectly used for right I::Item. HIR and portable
  substitution now explicitly identify the supplying receiver. Bounds/proofs
  preserve the same receiver ownership and reject unrelated or cyclic outputs.
- Shared callers now carry immediate associated-type method contracts, including
  Iterable::Iter's Iterator::next. This supplies the existing cursor operation
  rather than rediscovering it during native execution. Generic and ordinary
  callable preparation normalize boxed iteration outputs through one path.
  Checked selected method tables may supply additional associated facts; runtime
  lookup retains the selected member/table, validates required outputs and keeps
  the original operation group. It does not search another implementation or
  widen member access.
- Boxed Iterator values have a standard identity Iterable table whose iter
  returns the same box and whose other entries reuse the existing defaults. This
  supports generic method witnesses for an already-erased representation;
  concrete sources keep their closed identity adapters and unboxed cursor types.
  Only installed native identity Iterable for Iterator is admitted by HIR; script
  trait-target implementations remain invalid. Receiver ownership remains core::iter.
- Interface recording also retains checked parent tables needed by hash insert
  operations. Retention remains runtime metadata edges rather than host root cycles.
  No extra pipeline methods, storage migration or ABI identifier bump was added.

The initially failing paired generic/interface calls were resolved through the
above contracts: two-receiver projection errors, absent boxed identity method
witnesses, missing associated cursor operations and overly exact associated-output
matching. Temporary diagnostics were removed and validation remains enabled.
Existing callback-failure and reload tests now use zip: a right callback failure
commits both advances; retained left/right calls and captures remain pinned after
reload. Existing loop protection now covers nested paired wrappers. Cancellation
coverage adds filter_map/count; its assertion preserves original source order
instead of applying the sort test's bag comparison.

Focused checkpoint evidence: SDK iteration (16), conversions (18), VM mapping (8),
portable native defaults (13), generic interface defaults (5) and associated-output
substitution (3) passed. Cancellation callback cleanup (1), checkpoint formatting,
structure (1,030 Rust files; zero violations/exceptions) and diff checks passed.
No carried implementation or integration failure remains at the checkpoint. Extreme
usize count/enumeration overflow is checked at the shared arithmetic operation;
no infeasible 2^64-pull execution is claimed. Full workspace and source-free final
acceptance remain IA05 work; CI is unrun.
