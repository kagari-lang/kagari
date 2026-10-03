# Foundation API Completion

Status: active, 2026-10-02. The user authorized implementation of this plan,
including the bounded API inventory and approved generic-interface/mutation rules.
FA01–FA04 are accepted; FA05 final integration is in progress. This plan fills the gaps left by the completed native collections
reset and execution-policy simplification. It does not resume the historical full
standard-library restoration checklist.

## Direction

Provide one coherent language foundation. Its traits, basic types, default
containers and approved common methods are always available. Remove the engine's
choice to disable bundled foundation algorithms. Compiler-only consumers receive
the same declarations without constructing a runtime; executable consumers receive
the corresponding validated native bindings. Source-free consumers still validate
their imports and must not execute missing or mismatched bindings.

Keep application native module installation. Installing a host service remains an
explicit embedding operation, independent of the built-in foundation. This change
does not install application IO or other host services implicitly.

Use the existing trait model:

- List and MutableList declare common list methods directly on the traits.
- Required operations describe what a container must implement. Default methods
  supply shared Rust algorithms; implementations can override them where allowed.
- MutableList inherits List, including methods returning new collections.
- Method-level generics are callable through interfaces, with the same default
  and override rules for script and native implementations. This is a general
  language capability, not a collection-only exception.
- ArrayList supplies compact storage and optimized implementations where useful.
- HashMap/HashSet remain always-present defaults backed by Rust's HashMap/HashSet.
- String has language-owned inherent methods implemented by ordinary Rust natives.
  No String trait or extension-method language feature is introduced.

Do not add Deref, extension-function syntax, inherent impls on interface types,
runtime method-name lookup, or a second algorithm implementation in Kagari.
Declaring a method in the foundation does not make it a dedicated opcode.

## Current implementation and concrete gaps

The authoritative portable foundation declarations currently live in
`kagari-abi/src/language/catalog`; HIR consumes these contracts. Runtime foundation
bindings live in `kagari-runtime/src/native/foundation`. These are current owners,
not an endorsement of the ABI crate name. The queued contract/common cleanup stays
separate and is not a prerequisite.

The checked callable model already has NativeDefault applications, generic
substitution and HIR default-method metadata. Reuse and complete this path rather
than introduce another default-provider registry. Script default methods and
explicit overrides already have coverage. The public MethodDecl builder currently
lowers methods as Required; filling the declaration/binding API is distinct from
inventing default dispatch from scratch.

At plan activation, the engine's `default_modules(bool)` gated a collections module with
free functions sort, sort_by and lazy map. Inherent native registration currently
accepts only a module's own NativeObject types. Foundation String methods need an
owner-controlled binding path for built-in types; this must not authorize arbitrary
modules to modify another type's inherent surface.

SequenceEdit currently supports scalar slice edits and complete permutations of
reference slots. Its edit path clones storage and publishes only on success;
callback sorting additionally allocates a permutation. These are implementation
facts to review and replace, not failure guarantees to preserve in this plan.
The native edit surface also cannot express retain/dedup yet.
NativeCursor's convenient constructor currently accepts ArrayList only. General
selected Iterator/Iterable calls exist elsewhere and can be reused when a scoped
adapter is needed; there is no need to rebuild callback execution.

## Ownership and data flow

| Concern | Owner and intended change |
| --- | --- |
| Foundation signatures | Portable language catalog: trait methods, method bounds, String inherent signatures and stable definition identities |
| Rust registration API | Explicit method signatures and native default template bindings; no Rust-signature-derived language declarations |
| Static checking | HIR selects required/default/override implementations and validates receiver/method bounds |
| Executable contracts | Callable identities, native imports and checked generic call environments; represent shared generic bodies and constraint operations explicitly |
| Runtime algorithms | Focused list and String modules under the runtime native foundation; internal helpers shared with existing implementations |
| Concrete storage | ArrayList overrides and safe scoped mutation; no compiler branch for individual algorithms |
| Engine construction | Unconditionally assemble and validate foundation bindings on supported execution paths |
| Tooling | Generate .kgr documentation/signatures from the same declarations; no executable source catalog |

The call path remains:

```text
trait/inherent declaration
  -> checked receiver, method bounds and implementation selection
  -> selected default or override, with checked type/constraint arguments when needed
  -> ordinary verified call / interface adapter
  -> script body or scoped Rust implementation
```

Defaults invoke the selected operations of their actual receiver. They must work
for a custom implementation and must not silently construct or downcast it to
ArrayList. Native overrides, script overrides and inherited defaults follow one
selection rule; no backend repeats trait resolution.

The explicit binding API should support these operations (API roles, not final
Rust method spellings): declare a method and its local parameters/bounds; mark it
required or bind a native default template; declare the template's receiver
operation/callback requirements; bind a concrete override without repeating the
trait signature. Finish/installation validates every template application and
required binding. Reuse CallableImplementation::NativeDefault and existing
callable requirements; do not add an unrelated `api*` object hierarchy.

## Accepted bounded API surface

This list is the accepted delivery scope, not an instruction to recover every retired API.
Names use the existing snake_case style. Existing foundation operations remain.
Changing this list is a scope decision recorded here, not a growing implementation
checklist. The rows below are required work.

### List and MutableList

| Trait | Proposed methods | Contract |
| --- | --- | --- |
| List<T> | sorted, sorted_by, sorted_by_key | Return a sorted List<T>, with ArrayList backing; original slots are unchanged |
| List<T> | reversed | Return a list with reversed order |
| List<T> | distinct | Return first occurrences in input order; require T: Eq and use equality without adding a Hash requirement in this first delivery |
| MutableList<T> | sort, sort_by, sort_by_key | Reorder the existing object; natural ordering requires T: Ord |
| MutableList<T> | reverse | Reverse the existing object |
| MutableList<T> | retain | Keep elements accepted by the predicate, preserving order |
| MutableList<T> | dedup | Remove consecutive equal elements, preserving the first of each run; require T: Eq |

Comparator methods accept `fn(T, T) -> Ordering`. Key methods have a method-local
K: Ord parameter and accept `fn(T) -> K`. Natural ordering/equality requirements
belong to the relevant methods, not to List<T> itself. Lists of non-Ord or non-Eq
elements must remain valid. distinct's initial equality-only algorithm may be
quadratic; do not claim hash-set performance or hide a new bound.

Sorting is stable. Key-based sorting evaluates selectors as comparisons require;
do not silently cache every key in a prepass. A separately named cached-key API is
outside this delivery. Comparator/key failures stop subsequent user callbacks;
the underlying Rust algorithm may finish internal cleanup without invoking them.
In-place operations do not promise rollback on error or cancellation. Preserve
completed mutations and callback effects under the failure policy below.
New-result operations copy collection structure, not referenced element objects.
Do not promise a particular comparator count or Rust sorting implementation.

The current lazy map behavior and its source-free, GC, reload and iteration-guard
coverage must survive removal of the installation switch. Keep it as an
always-provided ordinary function for this delivery; moving the whole adapter
family onto Iterator is separate work. It is not a compatibility alias for any of
the new List methods. Old sort/sort_by free entrypoints are replaced by trait
methods and their consumers migrated directly.

Map/MutableMap/Set/MutableSet retain their existing required operations. Additional
set algebra, grouping, map transformation, windows/chunks, iterator terminals and
new containers are not part of this bounded delivery.

### String

String signatures belong to the built-in type's declaration owner. Runtime bodies
belong in a focused String foundation module, using Rust string operations where
they match Kagari semantics. Existing literal, interpolation and protocol behavior
is preserved and is not reimplemented as a library.

| Proposed methods | Contract |
| --- | --- |
| len, is_empty | UTF-8 byte length and emptiness |
| contains, starts_with, ends_with | Literal String pattern matching |
| find | First matching byte offset, or Option::None |
| slice(start, end) | Half-open byte range; check order, bounds and UTF-8 boundaries |
| trim, trim_start, trim_end | Return a String, trimming Unicode whitespace |
| replace(from, to) | Literal replacement of all non-overlapping matches |
| split(separator) | Eager List<String> result for this delivery; literal separator |

String remains a value type with immutable contents. Results may share immutable
storage if safe, but never introduce mutable aliases. Empty-pattern behavior must
be documented and tested: find returns zero; replace inserts at Unicode scalar
boundaries including both ends; split keeps leading/trailing empty fields and,
for an empty separator, splits at Unicode scalar boundaries with endpoint empties.
An empty input split by a nonempty separator yields one empty string.

These are the accepted finite semantics. No regex, locale-sensitive
ordering, grapheme indexing, normalization, encoding library or lazy String cursor
is required. Numeric parsing remains owned by existing FromStr contracts.

## Approved generic interface calls and mutation policy

### Method-level generics are supported through interface values

The user approved full interface calls to method-level generic methods. A method
must not become unavailable merely because its receiver is held as List<User>
rather than ArrayList<User>. Required methods, inherited defaults and explicit
script/native overrides all participate. Native-only support, collection-specific
special cases and a static-only fallback are not acceptance outcomes.

The current specification and executable interface tables reject these methods.
Replace that restriction as part of FA01. This does not remove unrelated interface
restrictions, such as unsupported Self positions or unbound associated outputs.
Method constraints such as T: Ord are checked for each call, not imposed on the
whole List<T> trait. A List of non-Ord elements remains a valid interface value.

For example, the same List<User> value can receive sorted_by_key calls with K=i32
and K=String. The call site determines K and proves K: Ord; the receiver's interface
table selects the actual implementation. These are independent parts of dispatch.

#### Calling convention and checked data

Use a shared generic entry with explicit type and constraint arguments for dynamic
interface calls. Preserve existing specialization for statically resolved calls;
do not route all generic code through the shared path.

The following are data roles, not final Rust type names or another public builder
hierarchy. Extend/reuse current identities, substitutions and witness records:

| Record | Required information |
| --- | --- |
| Generic method declaration | Trait/method identity, separate trait and method binders, parameter/result expressions, bounds and default mapping |
| Interface method entry | Selected default/override identity, shared entry and receiver adaptation, pinned implementation owner |
| Generic call environment | Method type arguments and validated witnesses for required trait operations, including inherited requirements |
| Checked call application | Interface/method identity, substituted argument/result contract and correspondence to the call environment |
| Shared script body | Explicit generic parameter representations and constraint-operation calls, independently verifiable without source analysis |
| Native result construction | Declared concrete body result plus compiler-selected interface implementation/arguments; the exported signature remains the interface |

The callback is an ordinary typed argument, not a second dispatch registry. A
sorted_by_key<i32> call supplies its key callback and checked i32: Ord operations
to the receiver-selected entry. A generic caller may forward its own type and
constraint arguments instead of constructing concrete ones. Nested generic calls
must substitute/forward the environment without resolving trait implementations
again at runtime.

Static checking proves calls and implementation compatibility. Artifact validation
checks binder scope, signatures, witness requirements and shared-body operations;
linking verifies executable identities and pins their owners. Runtime dispatch uses
those checked entries. Do not use Unknown recovery types, unchecked Any casts,
source-dependent loading, runtime type inference or call-time code specialization.
Ordinary runtime value and ownership checks remain in place.

#### Native and script implementations

Native shared entries use the existing Value/conversion model and prepared callable
operations. Prepare reusable type/operation information outside algorithm loops;
do not resolve method names, reconstruct dictionaries or allocate wrappers for
every comparison. Keep compact primitive storage and concrete optimized paths.

Script defaults and script overrides need checked shared generic bodies as well.
Compile generic values with a known runtime representation and lower operations
on generic parameters to their supplied constraint witnesses. Preserve type
arguments when creating nested generic values, calling other generic methods or
returning results. Retained closures must retain any required environment and
generation ownership. GC visibility and cleanup remain explicit.

Reuse ordinary checked calls for operations selected by witnesses. This adds a
general compiler/executable capability beyond native registration; it is explicitly
part of FA01, not hidden inside the sorting implementation. A dynamic call must
honor the same override as a static call and must not bypass an override by always
choosing the trait default.

#### Required behavioral proof

Use a small user-defined trait outside the collections catalog, alongside List,
to prove this is a general capability. Through one interface value, invoke one
generic method with both scalar and GC-bearing type arguments. Cover a native
default, a script default, script/native overrides, trait inheritance, bounded
generic forwarding, a nested generic result and a closure retaining a generic
value/environment. Reject unsatisfied bounds and forged/mismatched witnesses.
Verify source-free execution and generation-pinned calls after reload. Source
checking, completion and navigation must expose the generic method normally.

### Approved: ordinary in-place mutation, without transactions

The user explicitly chose ordinary collection failure behavior rather than
transactional mutation. In-place sort, reverse, retain and dedup do not promise
rollback on callback failure, a custom receiver operation failure, or cooperative
cancellation. Errors propagate; completed mutations and external callback effects
remain. Do not require a bulk atomic-replacement method from MutableList, and do
not add rollback buffers, undo logs or a universal prepare/commit protocol.

ArrayList sorting should use its storage directly where safe. Review the existing
full storage clone, permutation allocation and publish-on-success path; remove
costs whose only purpose is restoring original slots on failure. Update the
existing tests and documentation that require unchanged slots on sorting failure
to the new approved contract. Do not preserve the old guarantee through a hidden
copy simply to retain those assertions.

This is not a zero-allocation requirement. Stable Rust sorting can need scratch
space; non-contiguous containers can benefit from sorting a temporary array and
writing it back; GC-safe callbacks may require rooted temporary storage. Keep
buffers with an actual algorithm, storage or safety purpose and document that
purpose. Do not weaken rooting/borrow checks to obtain direct mutable access.

For built-in ArrayList sorting, preserve the original elements even if their order
is partially changed, following Rust slice sort's failure guarantee. This does
not undo mutations to referenced objects. Length-changing operations may retain
completed removals. A generic algorithm using custom set/remove operations may
leave partial writes if a later operation fails; do not promise element-multiset
preservation for arbitrary custom storage without a supporting contract.

New-result operations such as sorted do not write the source collection as part
of their algorithm, but do not roll back callback side effects. All paths preserve
valid storage, type and GC invariants, structural iteration checks and scoped
borrow cleanup. Reject prohibited mutations before performing them. Poll
cancellation only at safe internal boundaries; no guarantee of full-operation
rollback is implied by safe cleanup. These rules apply to this collection work,
not to unrelated host-path or reload publication contracts.

## Execution phases

Implementation is authorized. One coherent commit per phase, with
`Roadmap-Step: FA01` through FA05.
Use Conventional Commits and mark breaking public API changes. Intermediate build
failures are allowed and recorded with command, cause and owning follow-up phase;
do not add temporary compatibility entrypoints or dummy implementations.

### FA01 Generic interface calls and foundation contracts

- Freeze the accepted API rows; update the collection, trait, String and failure
  specifications to the approved generic-interface and nontransactional policies.
- Extend existing declaration/binding support for native defaults, local bounds
  and owner-controlled built-in inherent methods. Keep overrides explicit.
- Implement checked generic interface entries, type/constraint environment passing
  and shared script bodies, retaining specialization for static calls. Extend
  executable validation and backend consumption of these facts together.
- Complete the generic-call behavioral proof above, including actual script
  overrides and generic forwarding, before relying on it for collection APIs.

Acceptance: method-level generics work through interface values for native and
script implementations; default/override selection agrees with static calls.
Missing/mismatched templates, invalid bounds and unchecked witnesses are rejected.
Do not add the entire algorithm set to prove the mechanism. This phase includes
the required general compiler support; native-only success is incomplete.

### FA02 Always-present foundation assembly

- Remove default_modules and the duplicate optional-foundation setup paths.
- Assemble the foundation for normal engine construction and supported source-free
  execution; preserve source-only declaration consumers and application install.
- Route current sort/map implementations toward their final owners without
  inventing an alternative public installation switch.
- Update affected consumers and installation documentation; record any algorithm
  migration build errors as FA03-owned rather than retaining obsolete aliases.

Acceptance: foundation bindings cannot be disabled through the engine API; custom
native modules still install and conflicting identities are rejected. Malformed
or incomplete external executable contracts still fail validation.

### FA03 List algorithms and scoped mutation

- Implement the accepted List/MutableList methods as native defaults, with
  ArrayList overrides for concrete storage optimizations.
- Replace transactional sorting edits with safe in-place mutation and add the
  length-changing operations needed for retain/dedup. Preserve roots for all
  reference-bearing values, including temporary algorithm storage across callbacks.
- Remove rollback-only storage copies/permutations and unused transaction helpers;
  keep necessary sorting scratch space, receiver guards and cleanup paths.
- Reuse a scoped selected-operation adapter for generic receivers, preparing call
  targets outside loops. Avoid indexed traversal that silently makes a linked
  representation quadratic when a linear traversal is available.
- Migrate sort consumers; retain current lazy map behavior and its tests.

Acceptance: concrete, generic and interface calls, including sorted_by_key, choose the same
implementation; a custom container reuses defaults without copying algorithm
code. Cover scalar and GC-bearing storage, bounds, stable ordering, key callback
semantics, alias writes, iteration guards, callback failure and cancellation.
Verify original-element preservation for built-in sorting and valid partial
progress for other operations/custom receivers; do not assert full rollback.

### FA04 String foundation methods

- Add the accepted inherent signatures and ordinary native bindings without a
  String trait, per-method compiler special cases or declaration duplication.
- Implement the finite String surface and generate its tooling documentation.
- Validate empty inputs/patterns, multi-byte UTF-8 boundaries, invalid slicing,
  unchanged inputs and rooted result construction.

Acceptance: normal engine construction provides every accepted String method;
source checking, navigation and artifact-only execution share its declaration.

### FA05 Integration and acceptance

- Update architecture and user examples to the delivered always-present model.
- Resolve carried failures and remove superseded public entrypoints/dead setup.
- Validate script/native generic methods, defaults and overrides through source-free loading, supported
  interpreter/JIT paths, reload pinning and external native registration.
- Run final workspace checks once; record results and remaining scope explicitly.

## Verification and performance boundaries

Use focused behavioral tests during phases. Reuse default_methods, collection,
iteration, native-provider and artifact consumers instead of duplicating them.
Each implementation checkpoint runs the structure checker and git diff --check;
format/check affected targets as appropriate. Final integration runs:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv run python scripts/check_features.py
git diff --check
```

Run the repository's existing supported JIT integration tests for affected calls.
For one primitive ArrayList sort and one callback/custom-container case, reuse
the existing measurement harness to check whether interface/default dispatch adds
per-element overhead to the concrete fast path. Record toolchain, machine,
profile, features, cache state and workload; separate compile and execution time.
No invented speedup target or requirement to benchmark every method.

Preserve cooperative cancellation, GC/root safety, host borrow checks, readonly
views, checked native signatures, generation pinning and structural iteration
protection. No quota/permission redesign, routine ABI version bump, committed
binary regeneration or compatibility reader belongs here.

## Progress ledger

- [x] FA01 Generic interface calls and foundation contracts.
- [x] FA02 Always-present foundation assembly.
- [x] FA03 List algorithms and scoped mutation.
- [x] FA04 String foundation methods.
- [ ] FA05 Integration and acceptance.

### FA01 execution record

2026-10-02: The user approved the bounded inventory, full script/native generic
interface calls, nontransactional collection mutation, and one commit per phase.
The plan, roadmap and architecture edits present at implementation entry belong
to this checkpoint. Intermediate failures were permitted without compatibility
adapters or temporary production stubs.

2026-10-03: FA01 is accepted with the combined validation evidence below.
The phase checkpoint uses `Roadmap-Step: FA01`. FA02–FA05 have not started.

Delivered mechanism:

- HIR checks interface method-local applications and per-call bounds. Portable
  shared bodies and interface entries keep declaration-owned binders, semantic
  signatures, actual type applications and selected default/override mappings.
  Statically resolved concrete calls retain specialization.
- Generic environments carry selected operations, forwarded requirements and
  generic member families. Artifact validation checks scopes, applications,
  signatures, bounds and the selected implementation tables without source.
  Runtime applies already-selected entries; it does not search for traits or
  compile a new specialization at execution time.
- Ordinary generic helper calls and cross-module defaults use the same shared
  entry machinery. Closures retain their environments. Nested nominal layouts,
  arrays, native sequences, iterators and inherited interface views preserve
  caller and receiver type scopes through allocation, calls and reload.
- Pure type metadata does not retain executable module state. Type-only scopes
  discard operation selections; closures, interfaces and selected callable
  witnesses retain the executable generations they actually need. Reload tests
  distinguish metadata retention from live code retention and check reclamation.
- Native shared defaults and overrides use checked template applications. The
  explicit trait builder supports method-local binders, bounds, receiver operation
  requirements and defaults. Required receiver operations come from the selected
  table and its parent mappings. Callback signatures/environments are prepared
  before the Rust body; concrete native calls retain their direct path.
- Built-in inherent methods are restricted to their declaration owner in both
  authoring and portable validation. External modules cannot extend String by
  bypassing the builder. Collection/failure specifications record the approved
  in-place policy; algorithm/storage migration is still owned by FA03.

Behavioral evidence covers scalar and GC-bearing method arguments; native/script
required methods, defaults and overrides; trait inheritance; receiver/method
bounds; generic helper/member forwarding; nested values; retained closures;
source-free loading; forged applications, tables and operation witnesses; and
reload generation ownership. JIT-enabled tests may execute shared bodies through
interpreter fallback; this does not establish shared-body native code generation
or performance.

Resolved integration failures:

- Fn receivers must pass through the existing tuple argument adapter before
  generic receiver dispatch.
- Mutable captures are internally Cell handles: closure capture checks validate
  the stored semantic value without changing ordinary argument checks.
- ArrayList::from_iter operation selection must normalize a concrete associated
  iterator projection before classifying it as a forwarded requirement.
- Imported GAT defaults calling an implementation with symbolic method arguments
  must use SharedCall, not the concrete-call path. Request planning appends only
  missing method parameters. All 12 GAT tests pass, including declaration-owned
  binders and rejection of dynamic GAT interfaces.
- Native defaults calling required receiver operations need the selected receiver
  dictionary. Native callbacks into shared script members retain scoped argument
  and result contracts and the selected executable generation.

The complete workspace sweep used:

```text
cargo test --workspace --no-fail-fast -- --skip runtime::language_contract::language_contract_routes_preserve_values_diagnostics_and_effects
```

The skipped cross-route test passed in the initial workspace sweep (163.74 s),
which subsequently stopped at the now-fixed artifact fixture. That success is
reused, not counted as a newly executed test. The continuation finished and found
three failing targets, all subsequently resolved and checked:

- standard_traits: a static-only language protocol parent (Named: Eq) incorrectly
  required a boxed parent table. Such protocols use static proofs/operation
  witnesses; dynamic parent links now follow the existing protocol eligibility.
  All 20 standard_traits tests, 32 default_methods tests and eight trait_inheritance
  tests pass together after the correction.
- HIR: two old fixtures added foreign inherent methods to container/interface
  types, conflicting with the approved ownership boundary. The fixture now uses
  a native trait. Associated-item filtering remains checked on arrays/iterators;
  erased interfaces expose their declared/inherited surface, not foreign
  extensions. Generated offline declarations pass. HIR's 400 tests are accounted
  for by the 399-pass rerun and the corrected completion case's focused pass.
- embed doctests: concurrent targeted recompilation invalidated the compiler
  dependency while the original sweep reached rustdoc (`E0463`). The isolated
  `cargo test -p kagari-embed --doc` rerun passes after compilation finishes.

All other workspace targets passed in the continuation, including 71 native
boundary tests, source-free artifacts, runtime/VM units and reload/GC consumers.
All 12 GAT tests and nine artifact_features tests also passed after their earlier
fixes. This is combined sweep/rerun evidence, not a claim that the original sweep
exited successfully. No known failure remains.

Formatting, diff checking and structure checking pass (634 Rust files, no
violations or exceptions). Workspace all-target Clippy passes with `-D warnings` after the
final parent-link/fixture edits. Full feature matrices and
performance evidence remain FA05 work. Temporary diagnostic output has been
removed. No ABI/version bump or tracked binary artifact was introduced.

### FA02 execution record

2026-10-03: FA02 accepted. Removed the engine foundation opt-out. Runtime::new now owns mandatory
foundation and bundled algorithm installation for embedding and raw/source-free
VM use. Engine constructors share that runtime installation path, expose the
bundled declaration to analysis/tooling, and install only explicit application
modules afterward. Existing VM/measurement consumers no longer install the same
foundation twice. Application conflict/signature validation remains unchanged.

The existing sort/sort_by implementations remain callable at this checkpoint;
FA03 owns their replacement by trait methods and mutation implementation. Lazy map
remains an always-provided ordinary function. No compatibility alias, alternative
installation switch or temporary algorithm was introduced.

Validation passes: native_provider_reset (13), conversion_traits (15), VM collection
algorithms (15), native boundaries (71), warmed native allocation checks (1), and
runtime units (49). These include default and explicit-empty engine construction,
raw VM loading without manual foundation installation, application installation,
and conflicting foundation binding rejection. `cargo check -p kagari-embed
--no-default-features` passes. Embed/runtime/VM all-target Clippy with `-D warnings`,
formatting, structure (634 files, no exceptions) and diff checks pass. No carried
build/test error remains. The checkpoint uses `Roadmap-Step: FA02`.

### FA03 execution record

2026-10-03: Started with the existing storage/sorting path. SequenceEdit now leases
and moves the actual compact/traced buffer out of the payload while callbacks run,
then restores the edited buffer on success, error or unwind. It does not clone
scalar storage or build a permutation. Reference-bearing storage keeps an explicit
root snapshot because Rust sorting can temporarily move values into scratch space.
This snapshot is solely for GC visibility; it is never used for rollback. The
payload retains its charged occupancy until lease restoration, then releases any
completed removals. Receiver slot access is exclusive for the lease; unrelated
callbacks, GC and referenced-object mutation remain possible. Iteration and alias
mutation checks run before editing. Primitive natural ordering retains its direct
slice path.

Added fallible stable sort, retain, dedup and reverse operations to the existing
scoped edit surface. First callback failure stops further callbacks, while Rust
completes safe internal cleanup. Removed the old index-permutation implementation.
The old free sort entrypoints currently consume this new storage path and will be
removed when trait method consumers are migrated later in FA03; no alias was added.
Tests now require element preservation rather than rollback after comparator error.

Focused evidence: the direct scalar buffer identity/error test and removal/unwind
accounting test pass; the 15 existing collection/lazy-map tests pass, including a
comparator that forces GC and fails after observable object mutations. An initial
traced-sort failure came from registering roots inside a native payload borrow;
root registration now happens after that borrow ends. No validation was relaxed.
Structure passes for 634 files without exceptions; runtime/VM all-target Clippy
with `-D warnings`, formatting and diff checks pass for this unit. FA03 has no phase commit yet.

Remaining work stays within the accepted FA03 surface: declare and implement the
List/MutableList native defaults and ArrayList overrides, reuse selected receiver
operations for custom containers, and construct the declared List result through
a checked concrete-to-interface adapter. A raw ArrayList value must not bypass the
List result contract, and runtime trait search is not an acceptable replacement.
Then migrate the free-function consumers and verify the bounded algorithm matrix.

2026-10-03: Completed the checked native concrete-result conversion needed by
List's new-result methods. `FunctionBuilder::produces` records the Rust body result
separately from the exported interface return; trait defaults lower the same fact
onto their private native templates. Registration checks the result codec and
trait conformance. Compiler selection records the implementation and arguments on
NativeImport, demands its parent tables, and includes native shared binders in
layout collection. Portable validation checks the substituted receiver, interface,
bounds, table presence and executable interface surface. Runtime links the exact
table once, validates the raw result, roots it during boxing, and retains scoped
generic arguments. No runtime trait search or collection-specific compiler rule
was added.

Evidence: four new result-boundary tests pass, covering artifact round trips,
ordinary generic functions and interface generic defaults, nested arrays and
script objects under forced GC, multiple type applications through one interface,
invalid Rust values, invalid registration, and five artifact corruptions (including
a missing executable table). The provider regression run passed 16 cases; its
remaining new case initially had a malformed script field declaration. After
correcting the fixture, that case passes separately, accounting for all 17 cases.
All 32 existing default-method tests pass. Clippy for ABI, bytecode, compiler,
runtime and embed with all targets and `-D warnings` passes. Structure checks pass
for 638 files with no exceptions; formatting and diff checks pass. Initial missing
parent-table and shared-layout-scope failures were fixed through the existing
interface-demand/type-scope paths. No known build/test failure is carried by this
unit. FA03 remains open and uncommitted until its complete method inventory,
custom-container defaults and migrated consumers pass.


2026-10-03: FA03 accepted. The full eleven-method List/MutableList inventory is
implemented as ordinary native defaults, with ArrayList overrides that edit its
actual compact/traced storage. Custom defaults select iter/next and set/remove
once, traverse through the iterator, and reuse the same Rust algorithms. A hidden
template type argument carries the selected associated iterator. Runtime obtains
its operations from the preselected result table, without resolving a trait.
New-result methods use the checked native result adapter described above.

Interface tables now retain shared native entries for methods with additional
call-time bounds. Constructing List<Item> does not require Item: Ord; calling
sorted still does. Inherited default ABI methods subtract only enclosing trait
bounds, preserving method-specific constraints. Native default slots consistently
map all template arguments into a shared entry, while static calls retain concrete
specialization. MIR checks substituted shared signatures and obligations; linked
ABI validation resolves associated default arguments before checking equality.
Local validation defers only unresolved dependency projections, with positive and
forged-output tests proving that linked validation rejects a wrong hidden argument.

Removed the old std::collections sort/sort_by entrypoints and their Rust module.
Migrated VM, embedding, feature-fixture, measurement and syntax consumers directly
to trait methods; native module import/navigation tests now exercise lazy map.
Updated current architecture, collection/native specifications, README and examples.
Language-owned method docs/navigation are tested without any application module.
No format version bump, compatibility alias or binary fixture was introduced.

Evidence: eight list tests cover concrete/interface/shared calls, distinct i32,
String and newly allocated script key types, stable object order under forced GC,
comparison-time key evaluation, method-bound rejection, generic custom containers
with script-owned iterators, and cancellation during sort/retain. All four original
list cases pass; the expanded seven-case run and final generic-container case pass.
The VM's fifteen existing sorting/lazy-map tests pass after migration, and three
new failure tests prove retained removals, partial custom set/remove progress,
iteration guards and root/lease cleanup. ABI default proof tests (13), language
contracts/navigation (7), default methods (32), collection interfaces (5), callable
traits (11), instantiation (50), source modules (29) and syntax examples (9, including
45 source/artifact examples) pass. The native-provider regression accounts for all
17 cases: its missing-table corruption test was corrected to select the intended
singleton import after foundation methods added other result adapters; that focused
rerun passes. The source-module alias fixture initially used the nonexistent
Option.unwrap method; the corrected loop-based fixture passes its focused rerun.

Affected ABI/bytecode/MIR/HIR/compiler/runtime/VM/embed all-target Clippy with
-D warnings passes; the added list test target also passes Clippy. Structure
checks pass for 643 Rust files with no exceptions; formatting and diff checks pass.
No carried build/test error or structural debt remains. Final workspace, feature,
JIT and measurement acceptance stays in FA05. This checkpoint uses
Roadmap-Step: FA03. FA04 owns only the accepted String method inventory.


### FA04 execution record

2026-10-03: FA04 accepted. The language catalog now owns all twelve accepted String
inherent methods and their documentation. A focused Rust foundation module binds
those declarations without a String trait or compiler/backend method special cases.
Queries use UTF-8 byte offsets; slicing checks range order, bounds and scalar
boundaries. Trimming uses Unicode whitespace. Literal replace and eager split
follow the accepted empty-pattern and endpoint-empty semantics. Split's ArrayList
body result becomes List<String> through FA03's checked result adapter.

A private scoped argument read avoids copying complete input strings inside the
method bodies. Borrowed slot references cannot escape; methods release these
borrows before GC allocation. String outputs own immutable bytes. Split polls
cooperative cancellation while collecting fields and uses checked typed sequence
allocation; no persistent state machine, quota or permission layer was added.

Five embedding tests pass, covering every method, multibyte and combining scalars,
empty strings/patterns, nonoverlapping replacements, immutable inputs, six invalid
slice ranges (including usize::MAX), static signature errors, forced-GC List result
construction, and encoded artifact execution. Language catalog tests (7 existing)
and the new String navigation/completion test pass without application modules.
The existing raw-declaration/builder ownership rejection test also passes. An
initial error assertion used the runtime's IndexOutOfBounds code instead of the
embedding layer's ScriptTrap category; the corrected test checks both that category
and the specific slice failure message. No production behavior was changed to
satisfy that assertion.

ABI/HIR/runtime/embed all-target Clippy with -D warnings passes. Structure checks
pass for 646 Rust files with no exceptions; formatting and diff checks pass.
Current specifications and the portable feature source cover the delivered String
surface. Disposable feature bytes will be regenerated once during FA05, together
with the final four-route check. No carried build/test failure remains. The phase
checkpoint uses Roadmap-Step: FA04.
