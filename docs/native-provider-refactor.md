# Native Collections Reset Plan

Status: replacement plan defined on 2026-10-02; implementation has not started.
The previous restoration goal remains paused. This documentation update does not
resume it or start implementation.

This is the active native-library plan. It replaces NR00-NR05, the full-library
restoration sequence and inherited ST06 acceptance obligations. Historical results
are evidence about the predecessor, not a requirement to restore its public
surface. See the [roadmap](implementation-roadmap.md) for other queued work.

## Objective and boundaries

Complete four ordered phases: remove the old library implementation, establish
compiler-owned language protocols, replace the costly native invocation path,
and prove the design with one collection package. Finish cleanup before building
its replacement; do not keep both architectures while migrating algorithms.

- Collection algorithms stay in Rust. Do not rewrite them in Kagari or introduce
  a separate standard-library crate.
- Language types and protocols required by syntax, static checking or implicit
  value semantics are always available, independently of installed libraries.
  This includes complete List/MutableList, Map/MutableMap and Set/MutableSet
  contracts, not just operators and iteration.
- Library and application native functions share registration, checked signatures,
  linking and execution. Native packages implement compiler-owned protocols
  without redeclaring their contracts.
- Native registrations own exported native declarations and bindings. Generated
  `.kgr` files provide tooling navigation, completion and documentation, not
  executable compiler input or a separate signature authority. Installation
  needs no declaration binary.
- Define Kagari declarations through an explicit API. Rust functions implement
  them; Rust signatures/traits/impls do not define Kagari contracts. Retire the
  #[native_module] declaration derivation model.
- Ordinary native functions and native-to-script calls execute synchronously.
  Callbacks alone do not require continuation state machines. Persistent state
  belongs to lazy iteration or actual asynchronous suspension.
- Avoid redundant allocation, rooting, metadata copying, validation and target
  resolution in hot loops. Performance is part of acceptance.
- Preserve static typing, checked arithmetic, alias identity, bounds, GC safety,
  failure semantics and generation-pinned calls. Budget schedules and permission
  matrices are not requirements or prerequisites of this reset and must not
  dictate the new native ABI or add per-step/per-call bookkeeping to it.

Out of scope: full-library restoration, extended Map/Set algorithms, arbitrary inline
script-struct layouts, a complete async executor, JIT/LLVM feature expansion,
new syntax, LSP transport, broad Rust interoperability and a separate execution-
policy migration. Necessary consumer changes belong to their owning phase.

## Collection ownership boundary

| Layer | Owns |
| --- | --- |
| Compiler/language | Complete foundational trait declarations, associated outputs, inheritance, interface conversions and read/write semantics; canonical ArrayList/HashMap/HashSet declarations, [T] and existing collection literal typing |
| Runtime foundation | Always-present ArrayList/HashMap/HashSet basic implementations, GC-managed object identity, generic native storage registration, contiguous primitive buffers, scoped access and ordinary native invocation |
| Native library | Concrete additional collection types and Rust algorithms such as sorting, searching, deduplication, grouping and set operations |

List<T>, MutableList<T>, Map<K,V>, MutableMap<K,V>, Set<T>, MutableSet<T>,
Iterator and Iterable are always-present language contracts. They use ordinary
trait checking and implementation selection. The compiler does not select a
hash table, tree or traversal algorithm from a collection trait's name.

Foundational members describe access, mutation and traversal. Convenience or
callback algorithms do not become compiler features merely because they operate
on a foundational interface. Map/Set contracts impose neither Eq/Hash nor a
traversal order: a hash implementation can require Eq + Hash, while a tree
implementation can require Ord. Each concrete implementation owns that choice.
Iterable's associated Iter must satisfy Iterator<Item = Item>; do not fix it to
an optional library's concrete Iter<T>. Checked static/dynamic interface metadata
retains the actual associated outputs and implementation targets.

Each collection family has one canonical default implementation:

| Contracts | Default type | Required implementation bounds |
| --- | --- | --- |
| List<T> / MutableList<T> | ArrayList<T> | No blanket equality/hash/ordering bound |
| Map<K,V> / MutableMap<K,V> | HashMap<K,V> | K: Eq + Hash |
| Set<T> / MutableSet<T> | HashSet<T> | T: Eq + Hash |

Default HashMap storage uses Rust std::collections::HashMap; default HashSet uses
std::collections::HashSet. Do not use indexmap for either default. indexmap belongs
only to optional standard-library LinkedHashMap/LinkedHashSet implementations.
Checked Kagari Eq/Hash selections still govern script keys; Rust storage does not
replace the language's key semantics with automatically derived Rust contracts.

The compiler owns these nominal declarations and complete basic signatures.
Runtime implements their construction, access, mutation and traversal in Rust
through ordinary checked native bindings and storage registration. They are part
of the engine foundation and cannot disappear when optional modules are disabled.
Hash requirements belong to HashMap/HashSet, not to Map/Set interfaces. Default
hash containers promise no insertion or sorted traversal order; explicitly ordered
containers such as LinkedHashMap/LinkedHashSet or TreeMap/TreeSet are library types.

[T] denotes List<T>. Existing array literals create ArrayList<T>; selecting defaults
does not introduce new map/set literal syntax or make an interface constructor
silently choose storage. All three defaults support read-only/writable conversions.
Phase 1 removes predecessor registrations; phase 2 establishes these contracts and
basic operations, and phase 3 optimizes their storage/call boundary. Each default
has one canonical type and implementation; optional libraries do not reinstall it.

Optional modules add sort/sort_by and the lazy adapter proof through ordinary
registered functions or library-owned extension traits. Do not mutate the core
type declaration at runtime or reinstall its contracts with the algorithm module.
Disabling the module removes its algorithms; all three default containers and
language syntax remain. Additional containers and extension algorithms use the
same registration mechanism, without a separate standard-library crate.

New native objects register allocation, GC tracing, destruction and scoped access
through the generic storage interface. Adding a queue, tree or application object
must not add a named variant to Value, HeapObject or NativeTypeConstructor, nor
add compiler/verifier/VM dispatch branches. Finite primitive layouts remain valid;
a growing catalog of concrete collection types is not a physical layout model.
This manual GC-managed storage contract does not require the deferred automatic
Rust interoperability design or extended Map/Set algorithm families.

## Proposed data structures and explicit API

This is a design proposal, not an implemented interface. Naming follows modules
and language declarations: define a trait/type/function, implement it, bind its
Rust body, then install the module. The snippets show contracts and ownership;
they do not add execution phases or restore more algorithm families.

### Declaration, binding and execution models

| Model | Responsibility |
| --- | --- |
| ModuleBuilder | Builds one Kagari module with explicit declarations and implementation bindings |
| LanguageContracts | Complete compiler-defined language declarations; an immutable portable view is available to registration/tooling without HIR or library installation |
| ModuleDecl | Kagari types, traits, functions, impls, documentation and dependencies; no Rust pointers or execution state |
| Type | Primitive, nominal application, owner-scoped parameter, Self, associated projection or function type; represents Kagari types |
| TypeDecl | Nominal identity, binders, value/reference semantics and an optional storage contract |
| TraitDecl / MethodDecl | Parents, bounds, associated types and full Kagari method signatures, including receivers and method binders |
| ImplDecl | Its own binders/bounds, receiver, applied trait, associated bindings and concrete member implementations; inherent impls have no trait |
| NativeModule | Checked ModuleDecl plus local Rust binding entries and storage factories |
| NativeBinding | Synchronous Rust entry with explicit argument/result conversion views, checked against a declaration before installation |
| LinkedNativeFunction | Closed layouts, linked entry, selected callable slots and retained generations; prepared before execution |
| CallContext | Scoped view over existing rooted arguments and prepared targets; no owned copy of the API/signature graph |
| NativeStorage | Registered factory, tracing/destruction and scoped views for a native object; optional checked layout capabilities enable specialized access |
| SequenceStorage | Shared object containing a concrete contiguous primitive buffer or traced values |
| NativeCursor | Persistent cursor, traced captures and pinned callable handles; no expired context or Rust buffer borrow |

The declaration graph is conceptually:

```rust
struct ModuleDecl {
    types: Vec<TypeDecl>,
    traits: Vec<TraitDecl>,
    functions: Vec<FunctionDecl>,
    implementations: Vec<ImplDecl>,
}

struct ImplDecl {
    parameters: Vec<TypeParameter>,
    bounds: Vec<TraitConstraint>,
    receiver: Type,
    interface: Option<TraitApplication>,
    associated_types: Vec<AssociatedBinding>,
    methods: Vec<ImplementedMethod>,
}

struct ImplementedMethod {
    member: MethodId,
    function: FunctionId,
}
```

An impl method obtains its Kagari signature by substituting Self, trait/impl
parameters and associated bindings into the actual MethodDecl. Its Rust binding
does not declare a second language signature. Free functions and inherent methods
supply explicit FunctionDecl records.

TypeRef, TraitRef, MethodRef and parameter refs retain their owning catalog and
declaration identity. Impl parameters have their own binder: two parameters named
T are not interchangeable. Name resolution happens during authoring, followed by
checked ID references. A library defines Kagari traits without declaring matching
Rust traits; Rust receiver syntax and inheritance do not define Kagari semantics.

The define_ operations add declarations; method(name) resolves an existing member.
ModuleBuilder::implement starts an impl builder with its own parameter scope;
receiver and implements then set its header. Binding attaches a body to a checked
member, while finish validates and publishes the completed declaration/module.

The execution view is conceptually separate:

```rust
struct LinkedNativeFunction {
    entry: LinkedNativeEntry,
    signature: SignatureRef,
    argument_layouts: Box<[ValueLayout]>,
    result_layout: ValueLayout,
    selected: Box<[LinkedCallable]>,
    owner: RetainedModule,
}

struct LinkedCallable {
    target: ResolvedTarget,
    signature: SignatureRef,
    owner: RetainedModule,
}

struct CallContext<'call> {
    function: &'call LinkedNativeFunction,
    arguments: ArgumentView<'call>,
    runtime: RuntimeAccess<'call>,
}
```

Owned tables allocate during preparation, not each call. ResolvedTarget identifies
a linked script/native slot or checked intrinsic operation, not a method name.
ArgumentView addresses stable rooted slots during reentry; it cannot borrow a
relocatable VM stack buffer across callback frame growth. Returning a result
transfers it into the caller's rooted destination before the native scope ends.

### Explicit Kagari declarations

List/MutableList now come from LanguageContracts. The compiler constructs their
complete declarations through the same explicit declaration model; a library
references these contracts and defines only its own additional types/traits.

```rust
let mut module = ModuleBuilder::new("example::collections", &language);
let list = language.list();
let mutable = language.mutable_list();
let len = list.method("len")?;
let get = list.method("get")?;
let set = mutable.method("set")?;

let mut buffer = module.define_type("Buffer");
let t = buffer.type_parameter("T");
buffer.reference_semantics();
buffer.sequence_storage(t.ty());
let buffer = buffer.finish()?;
```

Buffer<T> is an application/library-owned type implementing existing language
contracts. It is not another definition of the language's default ArrayList<T>.
Custom library traits can still be declared explicitly:

```rust
let mut sized = module.define_trait("SizedView");
sized.define_method(MethodDecl::instance("size").returns(Type::usize()))?;
let sized = sized.finish()?;
```

No corresponding Rust trait or struct hierarchy is required. Kagari read/write
behavior follows its language contracts, not a Rust &mut self receiver.

The sequence storage capability includes an element layout contract and its
runtime factory. It can serve arbitrary native type identities; the compiler
never recognizes an ArrayList name to select an algorithm. Compiled access uses
the checked runtime buffer contract, not the memory layout of Rust's Vec struct.

### Bind concrete implementations

Impl builders explicitly define Kagari impls. Binding a member creates its actual
concrete implementation function; it does not install a universal trait body.

```rust
let mut implementation = module.implement();
let t = implementation.type_parameter("T");
implementation.receiver(buffer.apply([t.ty()]));
implementation.implements(list.apply([t.ty()]));
implementation.bind(len, NativeBinding::new(
    Args::receiver(Codec::sequence(t.ty())),
    Codec::usize(),
    entries::len,
))?;
implementation.bind(get, NativeBinding::new(
    Args::receiver(Codec::sequence(t.ty())).arg(Codec::usize()),
    Codec::option(Codec::value(t.ty())),
    entries::get,
))?;
implementation.finish()?;

let mut implementation = module.implement();
let t = implementation.type_parameter("T");
implementation.receiver(buffer.apply([t.ty()]));
implementation.implements(mutable.apply([t.ty()]));
implementation.bind(set, NativeBinding::new(
    Args::receiver(Codec::sequence(t.ty()))
        .arg(Codec::usize()).arg(Codec::value(t.ty())),
    Codec::unit(),
    entries::set,
))?;
implementation.finish()?;

let mut implementation = module.implement();
let t = implementation.type_parameter("T");
implementation.receiver(buffer.apply([t.ty()]));
implementation.implements(language.index().apply([Type::usize()]));
implementation.associated_type("Output", t.ty());
implementation.bind(language.index().method("index")?, NativeBinding::new(
    Args::receiver(Codec::sequence(t.ty())).arg(Codec::usize()),
    Codec::value(t.ty()),
    entries::index,
))?;
implementation.finish()?;

let module = module.finish()?;
engine.install(module)?;
```

These excerpts show selected bindings, not a complete finished Buffer module.
The full implementations also bind all required foundational members (including
is_empty and structural mutations) and provide the associated Iterable/Iterator
implementation; missing members/parents are rejected at module finalization.
Index/Iterable satisfy List's parents, and List satisfies MutableList's parent.
Inherent methods on Buffer use the same impl builder without implements.
Algorithms on the core ArrayList use functions or library-owned extension traits;
they do not require cross-owner inherent mutation or duplicate core declarations.

Free functions follow the same separation:

```rust
let add = module.define_function(
    FunctionDecl::new("add")
        .parameter("left", Type::i32())
        .parameter("right", Type::i32())
        .returns(Type::i32()),
)?;
module.bind(add, NativeBinding::new(
    Args::empty().arg(Codec::i32()).arg(Codec::i32()),
    Codec::i32(),
    entries::checked_add,
))?;
```

This independent excerpt runs before module finalization. Its Rust body receives
the context and two i32 values and returns NativeResult<i32>. Binding codecs do
not create or overwrite the declared function's Kagari signature.

Codecs describe the bridge into Rust, not the authoritative Kagari signature.
Sharing a Rust integer representation does not make Kagari usize and u64 the
same type. Check exact semantic type/layout compatibility, receiver shape, arity,
results and selected dependencies. Rust checks that the function accepts codec
outputs; no macro reads the function signature to invent a language declaration.

For example, the len body is an ordinary Rust function:

```rust
fn len(_cx: &mut CallContext<'_>, values: SequenceHandle<'_>)
    -> NativeResult<usize>
{
    Ok(values.len())
}
```

The context is injected and absent from the Kagari signature. Binding/conversion
views are prepared once. Scalars use stack values; generic views borrow already-
rooted slots. Escaping values require explicit owned handles. Ordinary native
implementations neither assign scratch slots nor allocate invocation state.
Closure environments can be allocated at registration, not per invocation.

Generic views must be backed by a rooted slot or protected storage access. A value
read from mutable storage that must survive mutation/reentry needs call-scoped
retention or rooted working storage before callbacks. The zero per-element root
requirement applies to primitive buffers, not to unprotected GC references.

Finalization rejects duplicate identities, unknown/missing members, incorrect
associated bindings, unsatisfied parents/bounds, stronger impl requirements,
missing entries and incompatible codecs. It cannot prove an arbitrary Rust body
obeys behavioral laws or computes its promised result. Required boundary/result
checks and behavioral tests remain; metadata validation is not body verification.

### Selected calls, storage and lazy state

An explicit sort declaration adds T: Ord and a callable dependency on Ord::cmp.
An explicit sort_by declaration instead declares a function-typed argument.
Neither is guessed from a Rust generic bound or annotation.

```rust
sort.bound(t.ty(), language.ord().apply([]))?;
let compare = sort.requires(
    CallableRequirement::method(t.ty(), language.ord().method("cmp")?),
)?;

// Inside its registered synchronous closure:
let target = cx.selected(compare)?;
let ordering = cx.call(target, (left, right))?;
```

This is an excerpt: sort is the explicitly declared method, t its in-scope
parameter, and compare a dependency key that its registration closure may retain.
Execution uses prepared slots and stack argument packs/scoped views. There is no
request/receive handshake, target scan or signature cloning. A retained lazy
callback instead uses an owned traced/pinned handle; it cannot retain CallContext.

```rust
enum SequenceStorage {
    I32(Vec<i32>),
    F64(Vec<f64>),
    Traced(Vec<Value>),
    // Other supported primitive layouts.
}
```

Choose storage from the declared element type, including empty collections.
Primitive operations use scoped slices; reference-bearing fallback values are
traced through the collection object. Selected intrinsic comparison can work
straight on a primitive buffer. Callback sorting releases buffer borrows before
reentry and uses rooted working storage. A fallible comparator propagates its
first error, never a fake ordering supplied to make a Rust sorting API succeed.

NativeStorage holds runtime factories and trace/drop/access entries. TypeDecl
carries the corresponding portable identity/layout contract, never Rust pointers.
A native type attaches its implementation explicitly:

```rust
object.native_storage(storage_registration);
```

The registration is checked against the declared type and its representation.
Objects remain GC-owned; contained script references are visited as GC edges.
Borrowed views cannot survive unsafe mutation or reentry. Generic opaque access
works even when no sequence capability exists; optimized sequence access is an
optional registered capability, not a mandatory representation for every object.

NativeCursor retains position/state, traced captures and pinned callable handles.
A map adapter retains its source and mapper, not a program counter for synchronous
callback returns. Captures are GC edges, not independently permanent roots. Normal
next returns synchronously; actual future async suspension uses a separate entry
and state interface. No async executor is introduced by this proposal.

### Data flow and tooling

```text
explicit Kagari declaration builders + explicit Rust bindings
    -> checked NativeModule
       -> declarations -> HIR/static checking -> portable executable contracts
       -> declarations -> generated .kgr + declaration-to-span navigation map
       -> binding entries/storage factories -> engine installation
portable contracts + installed entries
    -> prepared entry, layouts and callable slots
    -> scoped context -> direct Rust result or synchronous script call
```

Tooling projects the same declarations consumed by compilation, without parsing
generated text back to recover signatures. Compiler-owned declarations use the same projection
machinery. Artifacts retain complete validation/linking contracts, never Rust pointers
or a requirement to regenerate source at execution time.

## Phase 1 — Remove the previous library implementation

Task: establish a clean boundary without executable dependencies on the old
standard-library package or its restored native algorithms.

- Remove `kagari-stdlib`, its workspace/dependency/build integration, legacy source
  preparation and library declaration catalogs. Remove the retired handwritten
  and generated `stdlib/*.kgr` products.
- Remove kagari-native-macros, #[native_module] exports and Rust-to-Kagari
  declaration derivation helpers with obsolete consumers. Preserve useful behavior
  cases without retaining their macro authoring mechanism.
- Remove the restored array/sorting, math/numeric, string, option/result, debug,
  cmp/hash/fmt/ops library registrations and business algorithms, their default
  installation wiring and per-standard-method selectors across all consumers.
- Remove the mixed `StandardTrait` catalog and associated library-owned type,
  method and blanket-implementation tables. Renaming that catalog is not cleanup.
- Retain independently justified language/runtime primitives: value layouts,
  GC storage, ordinary trait machinery, checked arithmetic, enum operations and
  generic host/native infrastructure. Move language-owned behavior to its actual
  owner rather than hiding a dependency on library registration. Reassess retained
  native infrastructure in phase 3.
- Retire examples, feature routes and tests tied solely to removed APIs or the old
  invocation model. Preserve meaningful language and boundary cases in focused
  tests and record intentionally withdrawn library APIs. Do not weaken assertions
  to hide failures in retained behavior.
- Remove tracked `.kbc` products and embedded-binary consumers. Generate required
  fixtures under ignored `target/` from reviewed source/API fixtures. Preserve
  independent source-free loading and rejection coverage without Git binaries.
- Reconcile the existing uncommitted implicit-protocol migration explicitly:
  remove library coupling and retain only justified language facts. Do not reset
  the whole worktree or discard unrelated changes.

Exit: no retired library crate, source loader, registration, algorithm or tracked
executable fixture remains on an execution path. Record the removal audit and test
disposition here. Independently retained units build and pass focused checks.
Any compiler gap caused by removing old declarations is recorded once with its
reproduction and phase 2 owner; do not reinstall the old package to bridge it.

## Phase 2 — Implement compiler-owned language protocols

Task: provide complete contracts and implementations without any installed native
library. Audit existing syntax/specifications first; do not copy all 38 old traits.

| Family | Language-owned responsibility |
| --- | --- |
| PartialEq / Eq / Hash | Complete signatures, eligibility, scalar behavior, object identity, tuple/enum composition and explicit override precedence |
| PartialOrd / Ord | Signatures, Ordering, scalar implementations and floating-point partial ordering |
| Arithmetic, bitwise and unary operator traits | Existing signatures, associated outputs, checked primitives and ordinary user implementation selection |
| List / MutableList | Complete access/mutation signatures, Index/Iterable parents and read-only/writable interface semantics; [T] names List<T> |
| Map / MutableMap / Set / MutableSet | Complete lookup/mutation/traversal contracts and normal interface conversions; no universal Eq/Hash/order requirement |
| Default ArrayList / HashMap / HashSet | Canonical declarations and checked runtime bindings for construction, basic access/mutation and traversal; concrete hash bounds, not compiler-owned algorithms |
| Index and existing writable indexing rules | Index/output contracts, read/write lowering and once-only evaluation; no new assignment syntax |
| Iterator / Iterable | Associated item/iterator contracts and ordinary selected implementations used by for loops |
| Fn | Function/closure contracts, argument tuples, associated outputs and normal callable implementations |
| RangeBounds and range forms | Contracts required by existing range/index syntax and bounds checks |
| Debug / Display | Existing implicit formatting contracts and implementations; public printing functions remain library/host APIs |

Primitive types, Option, Result, Ordering and syntax-required range forms also
belong to the language. Their declarations stay available with libraries disabled.
Option/Result convenience methods and collection algorithms remain library work.

The foundational collection surface follows the access/mutation/traversal members
in [collection access](spec/collection-access.md#shared-interface-surface), with
Iterable constrained by its actual associated iterator instead of a fixed Iter<T>.
Callback conveniences such as get_or_insert_with/update, sorting and grouping are
library algorithms. FromIterator, Sum/Product and conversion/parsing helpers remain
library declarations where no language syntax or implicit semantics requires them.
Moving basic contracts into the compiler does not restore all predecessor methods.

- HIR receives complete compiler-owned declarations, bounds and associated outputs.
  Native and script impls use ordinary trait checking and nominal identities;
  user traits named Eq or Iterator do not acquire language hooks.
- Construct Kagari contracts explicitly and publish their portable catalog view;
  do not reconstruct them from Rust traits or optional library macros.
- Specialization selects implementations once. Portable contracts retain checked
  signatures, layouts and concrete targets. Offline verification/runtime loading
  do not depend on HIR or source analysis, and generic ABI records do not recreate
  a complete library catalog.
- Make the same compiler-owned declarations available to tooling without a second
  signature authority or an optional native provider.
- Provide the minimal Rust ArrayList/HashMap/HashSet implementations in runtime,
  bound through the ordinary native mechanism. Hash lookup selects compiler-owned
  Eq/Hash implementations; it does not retain predecessor library selectors.
  Validate basic construction/mutation, views, iteration, collision handling,
  custom Eq/Hash keys, GC edges and callback failures without optional modules.
  Sorting, set algebra, grouping and callback conveniences remain library work.

Exit: with libraries disabled, primitive operators, implicit value behavior,
user-defined operator/index impls, closures, Option/Result propagation and for
loops over user-defined iterators type check and execute. Array literals, [T],
MutableList views and basic ArrayList/HashMap/HashSet construction, access,
mutation and traversal also work without optional modules. Concrete hash bounds
are enforced; other Map/Set implementations need no installed hash/tree provider.
Invalid signatures, bounds and associated outputs are rejected. Existing language semantics are
covered and phase 1 language-contract build gaps are resolved.

## Phase 3 — Optimize native calls and collection storage

Task: put a small prepared boundary around ordinary Rust functions, including
synchronous calls back into script code.

- Implement the explicit declaration/impl/binding API above. Kagari signatures
  are authoritative; checked codecs adapt Rust implementations.
- Direct calls return results without allocating Box<Completed> or entering an
  advance/receive handshake. Separate them from genuinely suspended invocations.
- Use scoped contexts over already-rooted arguments. Do not rebuild argument
  vectors/root sets or clone complete signatures/selected-application lists merely
  to read arguments. Escaping values use explicit owning handles.
- Resolve generations and concrete callable slots at linking/preparation. Repeated
  Eq/Hash/Ord/Fn calls do not scan modules, function lists or native imports, infer
  types again or clone complete contract records.
- Provide synchronous typed callback invocation returning a result. Native authors
  write normal Rust loops and error propagation, not program counters for callbacks.
- Keep necessary handle/value checks and GC tracing at the correct boundary.
  Verified contracts eliminate redundant checks; unchecked casts and escaping
  unrestricted Rust references are not acceptable substitutes.
- Interpreter and future compiled callers share prepared contracts. Use matching
  generation-pinned compiled callback targets when actually available, without
  expanding the JIT backend as part of this phase.
- Implement generic NativeStorage registration with checked factories, GC tracing,
  destruction and scoped access. Registered nominal types/codecs must not require
  new named variants or per-type dispatch cases in generic layers. Retain checked
  primitive/sequence layout capabilities for efficient interpreter/compiled access.
  The three default containers use this same mechanism; additional containers
  require only module declarations, impls and Rust storage bindings.
- Choose storage from the declared element type at construction, including empty
  collections. ArrayList<i32> uses Vec<i32>; supported primitive types have compact
  contiguous buffers. Generic/reference-bearing values may use traced Vec<Value>.
  Explicitly define the specialization set and fallback; script types do not
  dynamically become Rust generic instantiations.
- Root the shared collection object rather than each primitive element. Scoped
  typed bulk access checks layout once; primitive inner loops do not construct
  GenericValue proxies, register roots or repeat element type checks.
- Use direct primitive comparison only when that exact implementation was selected;
  custom implementations use prepared callbacks. Do not hold a Rust buffer borrow
  across script reentry that can access/resize it. Callback algorithms use rooted
  working storage and preserve specified failure/commit semantics.
- Lazy iterators retain their cursor/captures and GC edges between next calls.
  Each next can execute synchronously. Continuations are reserved for actual
  suspension; a lazy iterator does not force every internal operation into a
  state machine. Full async implementation remains deferred.

Exit: application scalar/generic/callback natives work without a library. Reentry,
errors, GC and reload release roots and retain correct generations. A warmed
ordinary scalar adapter adds no heap allocation. An i32 bulk loop adds no
per-element allocation, rooting, metadata copying or callable lookup. Measure
these properties and record remaining unavoidable costs before acceptance.

## Phase 4 — Verify one representative library

Task: implement one optional algorithm module over the language's default
ArrayList<T>. The bounded proof covers its foundational new/len/push/get/set/iter
behavior, optional sort/sort_by, and one lazy map adapter with next. Core list,
Index, Iterable and Iterator contracts/basic operations are already provided by
phases 2-3; do not redeclare or reinstall them here. Use ordinary functions or
library-owned extension traits for algorithms. Reuse Rust implementations; do not
restart full-library restoration or write a parallel Kagari algorithm.

- Prove compact i32 buffers and a traced GC-reference fallback, preserving shared
  identity and mutation behavior.
- Exercise built-in ordering, user-defined script Ord and a supplied comparator
  through synchronous invocation. Preserve sort stability and failure/commit rules.
  A comparator error stops further comparisons and leaves original element order
  unchanged while preserving completed effects on referenced payloads.
- Verify lazy consumption, retained captures, shared cursor behavior and cleanup
  on early exit/failure.
- Install the algorithm module by default through the ordinary engine mechanism.
  Disabling it removes algorithms while basic ArrayList/HashMap/HashSet operations,
  array literals and all foundational collection protocols remain available.
- The one application-owned consumer includes a small non-sequence native object
  retaining a script value, using generic storage/trace/drop registration and the
  same binding/callback interface. It must require no new Value/HeapObject/type-
  constructor variants or compiler/verifier/VM branches. This tests the extension
  boundary without adding a second standard collection algorithm family.
- Generate .kgr views and check signatures/docs/navigation through existing tooling
  queries. Generate one target/ artifact and run an independent source-free
  consumer, including mismatched-contract rejection.

Measure small/large inputs, warmed calls, allocations and callback/lookup counts.
Compare direct Rust buffers, native collections and script algorithms over
equivalent representations and semantics. Separate compilation from execution;
record toolchain, machine, profile, features, parallelism and cache state. Unsupported
JIT cases must be labeled unsupported, not timed fallback reported as JIT. Backend
expansion and a fixed speedup claim are not required by this bounded proof.

Exit: the fixed surface passes behavioral, boundary and measured performance
checks without special library dispatch or algorithm changes in generic layers.
No build/test failure remains in retained workspace consumers.

## Execution and verification policy

- Finish phases in order. No compatibility wrappers, old artifact readers or
  parallel semantic implementations. A language-declaration gap after cleanup
  belongs to phase 2, not a temporary restoration exception.
- Keep the four checklist items fixed. Record discoveries briefly with their phase
  owner; substantial scope growth requires user direction.
- Use focused checks during implementation; do not repeat unchanged expensive
  suites or inherit the old full-library/budget matrix.
- No routine ABI/format bumps for unpublished changes. Regenerate only affected
  disposable artifacts under target/ when needed.
- Use coherent Conventional Commits. Implementation checkpoints carry
  Native-Reset-Phase: 1, 2, 3 or 4. Disclose intermediate build failures and owners
  in the commit and this ledger.
- Final integration runs structure checks, formatting, workspace Clippy/tests and
  git diff --check. Additional feature/backend routes must serve this fixed proof.
  Historical passes are not current acceptance evidence.

## Checklist

- [ ] Phase 1: old library implementation and tracked executable fixtures removed.
- [ ] Phase 2: compiler-owned protocols and three default containers implemented independently.
- [ ] Phase 3: efficient synchronous native calls and typed storage implemented.
- [ ] Phase 4: ArrayList algorithm module, storage extension and measured proof accepted.

## Progress ledger

2026-10-02 — Planning reset only. The user replaced full-library restoration with
these four phases. No implementation phase has started or completed. The old goal
remains paused; budget/permission work is not a prerequisite.

Entry state: HEAD includes 158558ee (early development policy) and earlier native
restoration checkpoints. Uncommitted work includes implicit PartialEq/Hash/Debug/
Display metadata, compiler lowering, corruption tests, documentation and rebuilt
.kbc products. Phase 1 reconciles these changes without discarding unrelated work.

Previous verification is partial: focused scalar checks passed before the final
corruption case; structure/format checks passed; the route matrix was interrupted
at the user's pause. Old-model failures included fourteen ABI/seventeen HIR lib-
test diagnostics, three legacy source-package failures and an unavailable full-
feature generator. These are not new acceptance claims. Phase 1 owns retirement
or explicit disposition of obsolete targets; phase 2 owns retained language gaps;
final acceptance permits no carried failure in retained consumers. Historical
records remain in Git. The pre-reset worktree documents were copied to ignored
target/native-plan-reset/ for review, not as a second execution/progress ledger.

2026-10-02 — Explicit API proposal added at the user's request. Kagari declaration
ownership is separate from Rust implementation bindings; macro derivation is
rejected. The proposal covers declaration graphs, impl substitution, codecs,
prepared calls, contiguous primitive storage and traced lazy state. The same four
phases remain unstarted; no Rust implementation or resumed goal is claimed.

2026-10-02 — Module naming adopted at the user's request. The proposal uses
ModuleBuilder/ModuleDecl, NativeModule, NativeBinding, LinkedNativeFunction and
CallContext. Authoring uses define_trait, define_type, define_method,
define_function, implement and bind. Existing implementation names remain cleanup
input; no compatibility aliases or Rust implementation changes are introduced.

2026-10-02 — Foundational collections moved into the language at the user's request.
Complete List/MutableList, Map/MutableMap, Set/MutableSet and iteration contracts
are compiler-owned. Array literals use the always-present canonical ArrayList;
optional modules add algorithms and additional types. NativeStorage registration
is the object/GC extension boundary. The existing external-consumer proof now
checks a non-sequence object; extended Map/Set algorithms remain out of scope.
Four phases remain unstarted, with no resumed goal or Rust implementation claimed.

2026-10-02 — Symmetric defaults adopted at the user's request. ArrayList, HashMap
and HashSet have compiler-owned declarations and always-present Rust runtime
implementations for basic operations. Eq/Hash bounds belong to the concrete hash
types; default storage is std::collections::HashMap/HashSet, with no insertion/sorted
order promise. indexmap belongs to optional LinkedHashMap/LinkedHashSet. Native
modules provide other containers and extension algorithms. Phase 2 owns the three baseline
implementations, phase 3 their generic native boundary, and phase 4 retains the
ArrayList algorithm proof. Four phases remain unstarted; no new syntax, separate
stdlib crate, resumed goal or implementation completion is claimed.
