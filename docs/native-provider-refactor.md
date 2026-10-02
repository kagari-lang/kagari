# Native Collections Reset Plan

Status: phases 1-3 have reached their scope checkpoints. Phase 4 owns the bounded
optional library proof, predecessor-test disposition and final workspace acceptance.
The goal follows this replacement plan, not the retired restoration sequence.

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
retains the actual associated outputs and implementation targets. A dynamic
Iterable view may hide its concrete Iter behind Iterator<Item = Item>. The
executable InterfaceTableRecord retains the original implementation signature;
its optional InterfaceViewRecord names the erased surface and exact result-boxing
implementation applications. Compilation materializes those witnesses, and
artifact verification checks the unchanged inputs, original result and wrapper
type before execution. Runtime selects prepared slots and validates the raw return
before boxing it; it neither infers an implementation nor treats a concrete cursor
as an interface value without a wrapper.

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
| NativeBinding | Prepared synchronous Rust entry and checked argument/result conversion views; ordinary bind assembles it from an existing declaration, while bind_with supplies explicit codecs |
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
ModuleBuilder::implement(receiver, closure) opens a scoped receiver implementation
group. trait_impl(applied_trait, closure) declares an individual Kagari impl within
it. bind(member_name, entry) attaches a Rust body to that trait's checked member;
bind_with accepts an explicit NativeBinding for unusual or ambiguous conversions.
Closure completion assembles the impl records; module.finish validates the complete
module before publication. These authoring closures run during registration, not
on each native call.

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

Group implementations by their receiver type, declare each applied Kagari trait
explicitly, and bind ordinary Rust functions directly. Configure the receiver's
Rust storage family once for the group instead of repeating it for each method.
Sequence and MutableSequence converters share that family; each method still checks
its declared read/write access. A readonly array cannot gain mutable access through
a group configuration.

```rust
module.implement(buffer, |implementation| {
    let t = implementation.parameter("T")?;
    implementation.receiver_codec(Codec::Sequence)?;

    implementation.trait_impl(list.apply([t.ty()]), |methods| {
        methods.bind("len", entries::len)?;
        methods.bind("get", entries::get)?;
        // Bind the remaining required List members here.
        Ok(())
    })?;

    implementation.trait_impl(mutable.apply([t.ty()]), |methods| {
        methods.bind("set", entries::set)?;
        // Bind the remaining required MutableList members here.
        Ok(())
    })?;

    implementation.trait_impl(
        language.index().apply([Type::usize()]),
        |methods| {
            methods.associated_type("Output", t.ty())?;
            methods.bind("index", entries::index)?;
            Ok(())
        },
    )?;

    Ok(())
})?;
```

Passing the Buffer type declaration creates a fresh scoped receiver template
Buffer<T>. parameter("T") retrieves its existing template parameter; it does not
declare another parameter or reuse the type declaration's binder identity. Each
trait_impl lowers to its own ImplDecl with fresh binders and explicit substitutions
from that template. Separate groups do not share binder identities. A concrete
receiver such as buffer.apply([Type::i32()]) opens a specialized group with no
unbound T. Additional impl bounds are declared explicitly, never inferred from a
Rust function's generic bounds.

Member names resolve within the applied trait during registration, then become
checked MethodId/FunctionId references. Binding creates a concrete implementation
function; it does not install a universal trait body or perform name lookup during
execution. Declaration order of trait_impl blocks does not determine parent validity:
module finalization checks their complete parent and associated-type closure.

These excerpts show selected bindings, not a complete finished Buffer module.
The full implementations also bind all required foundational members (including
is_empty and structural mutations) and provide the associated Iterable/Iterator
implementation; missing members/parents are rejected at module finalization.
Index/Iterable satisfy List's parents, and List satisfies MutableList's parent.
After completing these implementations, module.finish() produces the checked
module for engine.install(module).

Inherent methods use implementation.inherent_impl(closure), with explicit method
declarations before binding. Algorithms on the core ArrayList use functions or
library-owned extension traits; this API does not allow cross-owner inherent
mutation or duplicate core declarations.

bind follows three steps: substitute the existing Kagari declaration, assemble
conversion views supported by the Rust entry type and configured receiver codec,
then validate exact compatibility. Primitive and generic views can use the short
form only when the conversion is unambiguous. A generic argument/result view takes
its semantic type from its declared signature position, not from the erased Rust
wrapper. Rust cannot invent a trait, method, impl, bound or Kagari result type.
CallContext is injected and NativeResult describes the Rust invocation outcome;
neither adds an argument or Result wrapper to the Kagari declaration.

When a conversion needs explicit configuration, replace the short get binding
above with this alternative in the same trait scope:

```rust
methods.bind_with("get", NativeBinding::new(
    vec![Codec::Sequence, Codec::Scalar(Type::usize().abi().clone())],
    Codec::Value,
    entries::get_raw,
))?;
```

The raw entry receives CallContext and returns NativeResult<Value>; the Option<T>
result is checked against the authoritative declaration rather than a Rust codec
that invents the enum signature.

bind and bind_with produce the same checked NativeBinding and runtime entry; they
are authoring forms of one invocation mechanism. An explicit codec remains subject
to the same signature checks. Ambiguous conversions produce registration errors,
not a guessed mapping or a runtime fallback. If bind_with supplies a receiver codec,
it must agree with the configured receiver contract and access representation.
No conversion inference, builder closure or name lookup remains in the hot path.

Free functions follow the same separation:

```rust
let add = module.define_function(
    FunctionDecl::new("add")
        .parameter("left", Type::i32())
        .parameter("right", Type::i32())
        .returns(Type::i32()),
)?;
module.bind(add, entries::checked_add)?;
```

This independent excerpt runs before module finalization. Its Rust body receives
the context and two i32 values and returns NativeResult<i32>. module.bind_with
accepts an explicit NativeBinding when needed. Both forms preserve the already
declared Kagari signature.

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
    values.len()
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
| From used by Result propagation | Preserve the specified `F: From<E>` error conversion for `?`, including identity/lossless scalar cases and type-owned explicit implementations; no reverse/fallible/parsing selector catalog |
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
The existing Result `?` specification does require From, so that minimal forward
contract belongs to the language; this does not restore the predecessor conversion
package or its method-name selectors.
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
  are authoritative; scoped implement/trait_impl groups reduce repeated receiver
  configuration. Ordinary bind and explicit bind_with produce equivalent prepared
  entries and reject incompatible signatures/codecs. Check fresh impl binders,
  associated substitutions and missing parent/member rejection during registration;
  Rust signatures do not declare the Kagari contract.
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
- Intermediate compilation failures are allowed throughout the migration at the
  user's explicit direction. Do not add temporary implementations, compatibility
  wrappers or restore obsolete code merely to make a checkpoint compile. Record
  representative errors and their owning phase; final integration still requires
  the intended implementation and passing checks.
- Use focused checks during implementation; do not repeat unchanged expensive
  suites or inherit the old full-library/budget matrix.
- No routine ABI/format bumps for unpublished changes. Regenerate only affected
  disposable artifacts under target/ when needed.
- Use coherent Conventional Commits. Implementation checkpoints carry
  Native-Reset-Phase: 1, 2, 3 or 4. Disclose intermediate build failures and owners
  in the commit and this ledger. Commit once per completed phase, as requested;
  do not make intermediate implementation or documentation-only commits.
- Final integration runs structure checks, formatting, workspace Clippy/tests and
  git diff --check. Additional feature/backend routes must serve this fixed proof.
  Historical passes are not current acceptance evidence.

## Checklist

- [x] Phase 1: old library implementation and tracked executable fixtures removed.
- [x] Phase 2: compiler-owned protocol/default declarations, lowering and base storage implemented; execution integration carried below.
- [x] Phase 3: efficient synchronous native calls and typed storage implemented and allocation acceptance measured.
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

2026-10-02 — Scoped implementation authoring adopted at the user's request.
implement(receiver, closure) groups trait_impl blocks with fresh per-impl binders
and a shared receiver codec. Ordinary bind checks Rust conversions against an
existing Kagari member; bind_with supplies explicit codecs for ambiguous/special
representations. Both produce the same prepared binding, without runtime name
resolution or a second signature authority. Examples and ownership descriptions
are updated; four phases remain unstarted and the previous goal remains paused.

2026-10-02 — Execution authorized in goal mode, with one commit per phase. Phase 1
is active. The user explicitly permits compilation failures throughout migration
and rejects temporary implementations to satisfy builds. The entry worktree diff
and untracked language-protocol files are preserved under target/native-reset-entry
for inspection; language-owned implicit protocol facts remain in the working tree.
Old macro authoring, provider algorithms and binary fixture consumers are cleanup
inputs, not requirements to preserve the predecessor architecture.

2026-10-02 — Phase 1 completed. Removed kagari-stdlib, kagari-native-macros, all
stdlib source products, HIR legacy package preparation/prelude/cache state, bundled
native registrations/algorithms, Rust declaration-expansion helpers and mixed
StandardTrait/intrinsic/native-adapter tables. Generic source package aliases,
portable declaration/verification machinery, low-level native registration and
independent arithmetic/GC/host infrastructure remain. Removed all 19 tracked .kbc
products; the retained feature fixture is generated under target/fixtures and its
standalone consumers read that file. Cargo dependencies/feature checks no longer
refer to removed crates. Removed the uncommitted routine ABI/KBC/MIR increments;
format identifiers and validation remain.

Test disposition: 221 tests exclusively tied to removed macro/package integration
were retired with their authoring mechanism (161 obsolete files removed in total).
Full numeric/math/string/conversion/default-template restoration is intentionally
withdrawn. Retained compiler/VM language, trait, host, mutation, GC, artifact and
source-query suites preserve independent behavior; retained ABI native-declaration
tests preserve signature/contract rejection. Phase 2 owns foundational protocol
and three-default-container coverage, phase 3 owns explicit registration/binding
coverage, and phase 4 owns sort/comparator/lazy-adapter/non-sequence/source-free
proofs. The generic feature fixture retains basic MutableList operations, GC/root
cleanup, forged binding rejection and actual Cranelift scalar/unsupported paths;
its retired full-library pipeline is not an acceptance obligation. The initial
implicit-protocol metadata/lowering changes remain uncommitted for reconciliation
with compiler-owned contracts in phase 2, including the three new protocol files.

Validation: cargo test -p kagari-common -p kagari-syntax passed (23 + 80 tests).
uv run --locked scripts/check_structure.py, cargo fmt --all -- --check and git diff
--check passed. Removal audit found no retired crate imports, legacy package cache,
bundled registration/macro or embedded .kbc consumer. cargo check -p kagari-abi
failed with 11 diagnostics: E0432 for removed standard::traits/intrinsic/native
imports in proof/verification code, plus the resulting E0277 inference diagnostic.
Phase 2 replaces these language-contract references, without restoring old tables.
The removed typed macro adapters also leave managed_iter's native_value references
for phase 3's cursor/context replacement. No temporary implementation is added.
Whole-workspace execution is intentionally unavailable at this intermediate point;
final acceptance still requires resolving retained consumers and all required checks.

2026-10-02 — Phase 2 in progress (not a completed checkpoint). Added the HIR-owned
explicit language declaration catalog and portable kagari-core::language protocol
identities, independently of optional native installation. The catalog defines
complete value/operator/collection/iteration contracts, Option/Result/Ordering/
Bound/range representations, canonical defaults and ordinary foundation binding
slots. Native type bounds now reach declaration text and HIR from the same checked
records; hash bounds belong to HashMap/HashSet, never Map/Set. Removed conversion/
aggregation method-name selectors, synthetic native interface tables and their
native_bridge ABI flag. Retained From because Result `?` requires its specified
forward error conversion, with identity/ownership checks preserved. Ordinary user
traits with library names use normal nominal proof records.

Default GC storage now uses std::collections::HashMap/HashSet. A cached-hash token
index enumerates collision candidates without invoking script callbacks under a
Rust table borrow. Structural mutation/lookup guards, generation checks and GC
edges remain. Hash iteration retains a key snapshot once and performs direct
lookups on next; it does not promise insertion order or perform an O(n) scan on
every next. The key snapshot's construction/tracing cost is explicit. Runtime
integration is not yet verified while the old managed-iterator adapter still
imports retired native_value state types (phase 3 owns that replacement).

Current focused evidence: cargo test -p kagari-abi passes 39 unit and 11 native-
default tests. Two predecessor numeric-dispatch tests were retired with the removed
RuntimePrimitive library variants; the independent numeric shape test remains.
The new language_contracts integration target passes its portable catalog/tooling
and library-disabled declaration cases. Its two behavior cases currently fail:
dynamic List/Map/Set views inherit Iterable with an unconstrained actual Iter,
where the existing interface checker requires all associated outputs to be fixed.
This is a real phase 2 boundary to implement, not a reason to restore a fixed
Iter<T> or skip interface validation. The custom iterator test also needs the
existing parser's parenthesized struct-construction syntax in its for expression.
The failures are retained. ABI/HIR production checks pass; compiler import cleanup
is being checked. Structure inspection passes 793 Rust files with no exceptions;
no phase 2 commit or completion is claimed.


2026-10-02 — Phase 2 scope checkpoint. Completed the compiler-owned contract
catalog and default container implementation records independently of optional
native installation. RangeBounds and the three start-bearing integral range forms
have ordinary implementation records. Iterable's actual Iter remains concrete in
implementation proofs; dynamic collection views expose Iterator<Item = Item>
from its checked associated bound. Portable dynamic surfaces are distinct from raw
implementation ancestry. Parent selection checks every specified associated output
while permitting an implementation to supply the remaining required outputs.
Removed an unsafe predecessor lowering fallback: Iterable returns its receiver
only when the Iterator identity rule is actually proved, rather than whenever
implementation lookup fails. Retained the user's pending scalar protocol adapter
metadata/lowering work, reconciled against the language kernel identities.

Focused validation: cargo test -p kagari-abi passes 39 unit + 11 integration tests;
cargo test -p kagari-hir --test language_contracts passes 6 cases (portable records,
generated source, no optional installation, defaults and user iteration, readonly/
hash rejection, and invalid associated iterator/item rejection). cargo test -p
kagari-compiler --test language_foundation passes all 3 cases, including actual
MIR/bytecode verification for scalar programs, default list native calls and dynamic
List iteration. Compiler production checking, formatting, structure inspection
(795 Rust files, zero violations/exceptions), and git diff --check pass.

Build status is separate from this phase's scope checkpoint, under the user's
explicit permission for compilation failures throughout the migration. Runtime
still cannot build because gc/managed_iter.rs imports retired native_value state
adapters. Phase 3 must replace that storage/invocation mechanism, install the
foundation binding slots through the intended synchronous interface and prepare
checked interface result conversions when a dynamic view hides a concrete iterator.
No runtime execution, custom hash collision behavior or GC pass is claimed here;
phase 2's execution exit assertions remain final integration obligations after
that real implementation is connected. There are no placeholder callbacks or
restored old adapters. cargo test -p kagari-hir --lib --no-run reports 33 obsolete
consumer errors (removed EngineNativeBinding/NativeDefaultMethod/library variants,
removed stdlib cache, and optional FromIterator/Sum/Product kernel identities).
Phase 3 owns retained native/tooling consumer migration; phase 4 owns the explicit
bounded-test disposition and final workspace integration. Do not restore the full
library or weaken retained assertions to satisfy these predecessor consumers.

2026-10-02 — Phase 3 in progress (not a checkpoint). Moved the authoritative
portable language catalog to kagari-abi::language::catalog so native registration
can consume it without depending on HIR. Renamed the portable record owner to
ModuleDecl/ImplDecl and kept NativeModule for a checked installable module. Removed
the obsolete managed-iterator invocation adapter. Ordinary VM native calls now
borrow existing caller root slots and invoke a synchronous Rust entry. Selected
callables are resolved to generation-pinned module/function slots during linking;
script callbacks enter the existing scoped execution stack and return directly.
No NativeAction/advance/receive handshake, argument root set or per-call boxed
invocation state remains in this path. Added the first explicit ModuleBuilder,
trait/method declaration and binding scopes; scalar converters preserve semantic
usize/u64 distinctions. Generic storage, scoped generic receiver templates,
foundation bindings and retained consumers are still being implemented. A VM
production check passes before the builder addition; no execution or performance
acceptance is claimed. Whole-workspace compilation remains intentionally pending,
with no temporary adapters added to restore predecessor consumers.

Phase 3 focused progress: explicit scalar registration has six passing behavioral
registration tests, and actual VM scalar calls plus raw-result rejection/cleanup
pass. Engine production consumers now install NativeModule records, with mandatory
foundation bindings installed by Runtime independently of optional modules. Added
selected Hash/PartialEq requirements to foundation lookup methods. During actual
hash execution integration, found the phase 2 catalog accidentally declared
Hash::hash as u64 while the existing specification and compiler adapter return i64.
Corrected the catalog to the specified i64; no conversion shim or version bump.
Selected script callback execution from a normal Rust loop passes. Default storage,
collision behavior, dynamic associated-result views and new compact buffers remain
under validation/implementation; no phase 3 completion or commit is claimed.


Phase 3 storage progress: seven VM boundary cases pass, including mandatory default
container operations without optional modules, custom script-key collisions,
selected script callbacks returning to a Rust loop, raw result rejection/cleanup,
a nominal Rust payload tracing a GC child and dropping after its final root is
released, and declared contiguous i32 storage (including an empty buffer and
rejected u32 views). Seven registration cases pass, including two generic impls
whose parameters retain distinct owners after substitution. Portable ABI checks
pass 40 unit + 11 integration tests, including rejected missing native storage
owners, wrong arity and associated outputs on native objects. These are focused
progress results, not final acceptance or performance measurements.

Added AbiType::NativeObject and portable NativeStorageLayout capabilities. Native
objects retain their nominal owner and generic arguments through HIR, checked
lowering, bounded serialization, matching/substitution and executable validation.
Registration carries exact referenced type contracts, pairs declarations with
local storage entries and installs them atomically. Additional native types use
one generic traced Rust payload object; no concrete Buffer/Counter heap variant
was added. NativeStorage::new registers opaque payload factories; sequence_storage
selects a sealed shared scalar-buffer implementation, so arbitrary opaque Rust
payloads cannot merely claim a contiguous sequence capability. Scoped scalar
slices forbid allocation/reentry while borrowed and use no per-element Value
conversion. Function bound records are merged and canonically ordered once during
authoring. Generic receiver groups rebind each emitted impl, including associated
outputs, to its actual independent owner. Inherent methods are restricted to the
nominal type's owning module.

Remaining phase 3 work stays within the original exit criteria: complete prepared
binding/callback and dynamic associated-result paths, migrate retained consumers,
and measure the prepared typed call/bulk paths. Default container/cursor payloads
are now integrated as recorded below. No phase 3 checkpoint/commit is made yet.

Array construction migration is now underway. MIR and bytecode MakeArray and
RepeatArray carry an explicit checked element type, including empty literals and
string interpolation arrays. The source compiler specializes that type from HIR;
portable application checks validate its referenced contracts, and bytecode access
facts use the declared type instead of inferring it from the first value. Physical
instruction validation checks operand representations and concrete/bounded types.
The first check exposed two E0027 VM operand errors while the old allocations were
still present. Those errors are now resolved through the actual typed allocator
and compact payload migration described below; no ignored operand or untyped
allocator bridge was added.


The generic inherent Buffer::new case initially exposed missing NativeObject
handling in the existing callee inference traversal (CannotInferGenericArgument
for T0). Adding it to the shared nominal traversal fixes contextual inference;
the actual VM case now passes with Buffer<i32>::new, contiguous scalar access,
identity equality and native objects as default hash-map keys. Native object
identity/hash/debug paths share their existing language semantics; untyped cursor
handles remain ineligible for general equality/hash. No constructor-specific
inference branch or signature alias was introduced.


2026-10-02 — Phase 3 array storage progress (not a checkpoint). Removed
HeapObject::Array(Vec<Value>). ArrayList uses the same NativeObject/NativePayload
allocation, tracing and destruction mechanism as a registered native object, with
SequencePayload selecting its contiguous layout from the declared element type.
MIR/bytecode MakeArray and RepeatArray pass that checked type through VM to Runtime.
Runtime array allocation now requires its verified LoadedModule and explicit element
type; the former untyped GcHeap/public Runtime allocation calls are removed directly.
StorageContext and CallContext expose typed allocation for native factories/calls.
Shared payload access accepts default arrays as well as registered sequence objects.

StorageType records the closed element ABI and recursively referenced struct/enum
layouts during allocation. ABI matching checks these contracts independently of
array length, including empty arrays, without copying or validating every stored
element on each native call. Writes still validate the incoming value against the
retained contract. Primitive contracts skip the nominal traversal. Bulk repeat,
fill, copy, extend and range removal operate on the selected typed buffers rather
than constructing intermediate Vec<Value> storage for scalar elements. Prepared
copies allocate fallibly before committing. Scoped mutable slices reject guarded
structural mutation and allocation/reentry; existing bounds, revision, roots,
candidate isolation and cleanup checks remain. An unused work-charge scaffold and
unneeded Clone derivations were removed instead of silencing dead-code warnings.

Validation: cargo check -p kagari-embed passes without warnings. The VM
native_boundary target passes ten cases, including all fourteen scalar layouts for
empty/literal/repeated arrays (42 checked calls), wrong scalar views, alias-visible
bulk writes, rejected wrong-type fill/copy, overlapping copy, retain/range removal,
self-extension, structural iteration guards and a struct child retained through GC.
The scalar-layout test initially exceeded the existing expression inference depth
because it formed one 42-call && chain; it now uses bounded sequential conditions
without changing compiler limits or dropping calls. Structure validation checks
816 Rust files with zero violations/exceptions; fmt and diff checks pass. Logs are
under target/native-reset-entry/phase3/typed-array-*. No performance claim or phase
3 commit is made on the basis of these focused results.

Carried consumer migration: cargo test -p kagari-runtime --test gc_ownership
--no-run fails in retained tests using the removed untyped Runtime::alloc_array
API (E0061, missing LoadedModule and element type). Phase 3 owns updating those
consumers to real typed contracts and generation contexts; existing HIR/native
predecessor-consumer failures remain carried as previously recorded. Do not add an
untyped overload, infer an element type from the first value, or weaken behavioral
assertions merely to make those consumers compile. Remaining phase 3 scope is the
prepared binding/callback/interface-return paths, consumer migration and
measurements already listed in the exit criteria.


2026-10-02 — Phase 3 hash/cursor storage progress (not a checkpoint). Removed
HeapObject::Map, Set and Iter. All three default containers and the collection
cursor now use generic NativeObject allocation/trace/drop, with sealed payload
codecs. The unordered table implementation moved to native/hash_storage.rs and
still uses std::collections::HashMap/HashSet with cached custom hashes/collision
tokens; no insertion-ordered table was introduced. Map/set metadata carries closed
key/value contracts, so even empty containers reject wrong types before mutation
or accounting. ABI matching checks the retained contracts independently of entry
count. Insertion and prepared retain preserve these contracts and selected key mode.

Hash constructors now declare the same Hash/PartialEq callable requirements as
lookup/mutation methods. Storage factories derive builtin/custom key mode from
those actual prepared targets once; emptying a table cannot change its selected
protocol. Removed the old untyped Runtime/GcHeap map/set allocation entrypoints.
A native author constructs these containers through the checked result factory,
which has the required closed signature and selected callables, rather than
creating a table and guessing its protocol from its first inserted key.

Custom lookup no longer builds Vec<Value>/tuple snapshots of collision candidates
or allocates a redundant RootSet. Each candidate is read by cached token outside
script execution while a scoped lookup guard borrows the native frame's existing
roots and prevents table mutation. Script callbacks run after the table borrow is
released. ExecutionFrame's obsolete key-lookup guard storage/entrypoints were
removed; the synchronous native context owns the actual guard lifetime.

Cursors trace their source and snapshot keys through NativePayload. Their source
iteration lease blocks structural mutation without registering another source
root, preventing an unreachable cursor from unnecessarily retaining its source
through a GC pass. Item layouts are checked from the retained closed contract;
string cursors are built with their final checked item type before publication.
Next prepares Option allocation before committing position, and session/trap/early
exit paths still release iteration leases. Ordinary calls have no cursor state.

NativeStorage::payload<S>() registers explicitly supplied state without a dummy
factory. CallContext::allocate_result_payload checks the installed Rust payload
type, traced values and generation retention before allocation. Missing default
factories produce a real allocation error; they are not replaced with temporary
success. Existing factory-based registration remains exercised independently.

Validation: production cargo check -p kagari-embed passes without warnings. VM
native_boundary now passes fourteen cases, adding wrong-type empty map/set writes,
fixed key mode after clear, GC during Hash/PartialEq callbacks, callback trap/root
cleanup, cursor source retention/shared position across calls and single-pass
reclamation, and an explicitly supplied application-owned opaque payload tracing
its input array. Structure validation checks 817 Rust files with zero violations
or exceptions; fmt and diff checks pass. Logs are under
target/native-reset-entry/phase3/hash-*, cursor-payload-* and hash-cursor-*.
The earlier cursor E0599 errors against removed HeapObject::Iter were resolved by
this migration, not by restoring a dedicated variant. These focused results are
not final acceptance or a performance claim. Phase 3 still owns ordinary typed
view/callback completion, dynamic associated returns, consumer migration and
measurements; phase 4 owns the bounded optional algorithm/tooling/source-free proof.


2026-10-02 — Phase 3 typed sequence binding progress (not a checkpoint).
Ordinary bind now accepts SequenceHandle and SequenceMutHandle parameters,
including one view combined with up to two scalar arguments. Scalar conversion
and function argument adaptation have separate owning modules. These adapters
borrow the caller's existing ArgumentView and allocate neither another RootSet
nor a per-element Value buffer. Scoped with_slice/with_slice_mut exposes the
sealed contiguous primitive layout; len and access report generation/borrow
errors rather than assuming an infallible heap lookup. Handles cannot escape the
call, and slices cannot escape their access closure.

Codec preparation now checks portable native storage capabilities against the
retained declaration catalog, so a nominal Buffer<T> with sequence storage uses
the same ordinary binding as ArrayList<T>. Opaque payloads cannot masquerade as
sequence storage. MutableSequence requires mutable physical array access or an
explicit nominal sequence storage contract. Capability lookup happens during
binding/installation/linking, not inside the Rust scalar loop. The existing
Buffer<i32> execution proof now uses ordinary bind rather than a raw receiver
conversion for its mutation/sum function.

Validation: cargo check -p kagari-runtime passes. VM native_boundary passes all
15 cases, including an ordinary named Rust function receiving a rooted sequence,
GC between accesses, rejection of GC while a slice is borrowed, scalar/view/scalar
conversion, shared alias writes, iteration mutation rejection and healthy calls
after the trap. Runtime native_builder passes all 8 cases, including registration
rejection of a sequence converter for opaque storage. Structure checks 819 Rust
files with no violations or exceptions; fmt/diff checks pass. Logs are under
target/native-reset-entry/phase3/typed-views-*. Other generic views, captured
function arguments, dynamic associated-result adapters, retained consumer ports
and measurements remain owned by phase 3; this is not a phase-completion claim.


2026-10-02 — Phase 3 generic/function binding progress (not a checkpoint).
Ordinary bind now accepts ValueHandle and CallableHandle in addition to scalar
and sequence parameters. It also supports a sequence plus a function argument.
ValueHandle borrows the existing argument slot and closed ABI type; creating the
view checks the slot without cloning its payload. Scalar reads distinguish exact
semantic types. Explicit owning roots are available for retention. NativeOutput
is sealed: scalar Rust returns keep exact codec checks and their established
encoding guarantee, while Value returns retain complete ABI validation. A generic
identity<T> can therefore use an ordinary Rust function without declaring T in
Rust or bypassing the checked return boundary.

Callable preparation uses the closure's full semantic parameter/result slots,
not just physical integer widths. Immutable closure snapshots use Rc, and captured
plus explicit arguments feed ExecutionFrame roots through borrowed FrameArguments.
There is no temporary combined argument vector or capture-vector copy on callback
entry. Native authors write synchronous Rust loops and propagate NativeResult;
no continuation or program counter was introduced. StoredCallable shares its
prepared metadata and implements NativePayload tracing without embedding a global
host root. Explicit RootedCallable retention shares an owning root; stored payload
callbacks can acquire one working root after releasing their payload borrow.

The retained-callback reload test exposed a real ModuleValidation failure:
"nested execution must use the pinned dependency program". The original session
entry admitted only the newest root graph, even though the old callback's closure
and captures retained their verified program. Runtime now accepts an opaque
PreparedClosure for that entry, validates its heap ownership/generation and
candidate eligibility, and enters its pinned program in the existing session.
Ordinary reentry still requires the root or current caller's dependency graph.
Current counters, cleanup and candidate isolation remain shared. Old callbacks
execute old code after publication; replacing/releasing their explicit roots
releases old program retention. No permission or budgeting framework was added.

Sequence receiver groups now match storage family while allowing individual
read/write converter choices; final signature checks retain access validation.
This fixes a concrete mismatch between the proposed single group configuration
and len/set style methods. The proposal's codec and low-level binding examples
were updated to the actual declaration-inherited contracts. Callback tests were
split into native_boundary_callbacks/mod.rs at the 1200 effective-LOC threshold,
without a size exception or weakening coverage.

Validation: the expanded VM native_boundary suite passes all 20 cases, including
captured mutable state, GC between callbacks, callback trap/healthy cleanup,
application-owned traced callback payloads, host-retained callbacks across reload,
generic scalar/struct/array identity and shared mutation, and sequence/function
parameters in one ordinary binding. After eliminating the unnecessary payload
copy at view construction, all 5 callback/generic cases pass again. Runtime
native_builder passes 9 cases including mixed receiver views. Focused production
clippy for runtime/vm libraries passes with -D warnings; three findings were
resolved through a collapsed native-target guard, borrowed hash arguments and
NativeStorage::prepare_payload naming. Structure checks 823 Rust files with zero
violations/exceptions; fmt/diff checks pass. Logs are under
target/native-reset-entry/phase3/*callback*, *generic* and receiver-view-*.

Phase 3 is still uncommitted. Remaining acceptance includes dynamic associated
result/interface adaptation, retained consumer migration, relevant cross-boundary
validation and the prescribed warmed allocation/performance evidence. Phase 4's
optional algorithm, lazy adapter, tooling/source-free proof and final workspace
matrix remain required; the focused native suite does not substitute for them.


2026-10-02 — Phase 3 dynamic interface results (not a checkpoint).
The List<T> for-loop reproduction previously failed with ScriptTrap "invalid
interface upcast": its Iterable view hid Iter behind Iterator<Item = T>, while
only the concrete implementation table existed. Closed tables now carry an
optional InterfaceViewRecord with explicit result-boxing applications. The source
planner materializes the concrete iterator witness, and bytecode lowering selects
its exact existing table. Linked validation checks the allowed language view,
unchanged inputs, exact raw result and wrapper trait, rejects missing/duplicate/
forged adapters, and retains the original native/script signature checks.
Artifact nesting/count/identity checks include the new records; no format or ABI
identifier was incremented.

Interface creation resolves adapter applications into retained module/table
slots. Method returns validate against the original implementation signature,
root the raw value during wrapper creation, and return the boxed iterator.
Inherited views use the already checked dynamic ancestry. Concrete static calls
remain direct. GC interface metadata moved into gc/interfaces.rs at the file
size boundary, with no re-export or exception.

The Map/Set dynamic-view regression exposed a separate source-lowering error:
layout collection read a generated protocol adapter's source-context signature,
which could contain unresolved Self. It now consumes the actual closed MIR
parameter/result semantics, also covering generated closures and foreign source
contexts correctly. No dummy body, skipped validation or compatibility path was
introduced. The layouts example now explicitly constructs a dynamic generic Number value
and selects its actual i32 interface instance; a static generic call alone does
not require a boxed table, and the generic empty template is not executable.

Validation: the VM native_boundary suite passes 25 tests, including dynamic
List/MutableList, Map/MutableMap and Set/MutableSet iteration, script-defined
iterator results, GC during iteration, mutation traps and subsequent cleanup.
Artifact round-trip execution succeeds; five adapter corruptions are rejected.
Focused production clippy for compiler/bytecode/runtime/vm passes; compiler clippy
was rerun successfully after the layout-collection fix. The layouts example also
runs successfully with explicit concrete and generic dynamic values. Structure checks
828 Rust files with no violations/exceptions; fmt and diff checks pass. Logs are
under target/native-reset-entry/phase3/interface-* and dynamic-*. The phase remains
uncommitted: retained consumer migration, cross-boundary checks and warmed
allocation/performance measurements still belong to phase 3, followed by the
bounded optional library/tooling proof and final integration in phase 4.

2026-10-02 — Phase 3 retained HIR consumers (not a checkpoint).
Migrated the predecessor declaration/cache/native-default tests to the installed
ModuleDecl model, language-owned HashMap/HashSet and explicit application-owned
native fixtures. The fixtures retain generic scalar contracts, native defaults
and override policy, declaration/navigation identity, source generation and
registered receiver predicates. They are analysis inputs with no runtime handler
or compatibility implementation. Ordinary application function signatures replace
removed library math/unwrap/collection algorithms in generic recovery tests;
nested Result/Option callback inference remains covered without restoring
map/collect algorithms. Completion tests use explicit native extensions for fixed
receiver predicates and ordinary source extensions for Ord constraints. Negative
cases now use real declared constructors/functions instead of succeeding because
an obsolete library symbol is absent. Incomplete-member tests reject unrelated
semantic errors as well as checking their candidate lists.

Two production corrections came from this migration. Generated foreign language
bounds now use the legal source alias core::language rather than the canonical
kagari-core package identity (which contains an unparseable hyphen). Canonical
identities and executable authority remain unchanged. Iterator/Iterable contracts
carry their generated documentation. RangeBounds no longer falls through the
intrinsic value-protocol check for arbitrary arrays; range methods retain their
actual declared implementations and a negative completion regression covers the
array case. Readonly collection completion inputs now contain whitespace between
closing generic brackets and assignment, avoiding accidental >= token recovery.

The initial retained HIR build failed with 33 removed-model references; after
porting those callers, 329/399 tests passed. The migrated suite reached 400/400
unit tests, and strengthening incomplete-source diagnostics exposed two malformed
test fixtures, now corrected. Hash container declaration assertions enumerate Eq,
Hash and inherited PartialEq callback obligations exactly. All 14 native/collection
query tests pass with those stronger diagnostics; HIR all-target Clippy passes
with -D warnings. Final HIR package test results are recorded below. Logs and
intermediate diagnostics are under target/native-reset-entry/phase3/hir-*.

Final validation for this unit: cargo test -p kagari-hir passes all 400 unit tests
and 6 language-contract integration tests (no doctests). cargo clippy -p kagari-hir
--all-targets -- -D warnings, cargo fmt --all -- --check and git diff --check pass.
Structure checking covers 830 Rust files with zero violations or exceptions.
No HIR build/test error is carried from this unit. Phase 3 remains uncommitted:
runtime/VM/embedding retained consumers and warmed allocation/performance evidence
are still pending; phase 4 and its final workspace matrix remain required. No
compatibility path, dummy runtime handler, version bump or new phase was added.

2026-10-02 — Phase 3 runtime and embedding consumer migration (not a checkpoint).
Replaced the embedding fixture's Fill advance/receive state machine and scratch
slot with an explicitly declared generic native from_fn function, a normal Rust
loop and synchronous CallableHandle calls. The embedding owns its module and
installs it through EngineBuilder; source analysis and a fresh source-free loader
consume that same registration. The fixture explicitly roots its partial array
and each callback's heap result before forced collection. Existing zero-length,
captured mutation, nested callback, callback-trap and every-budget-boundary cleanup
checks remain. The forged-entry regression now targets the actual existing
$foundation_list_new entry for its wrong-function case, rather than a removed
name that would merely duplicate the missing-entry case.

Ported execution frames/sessions, host scopes/borrows, typed path views, substrate
and nested layout tests to explicit allocation owners and element contracts.
Candidate publication still tests late insertion of an old object: its container
now has the matching array element type, so rejection exercises generation
validation instead of an unrelated scalar layout error. Nested struct field tests
allocate correctly typed but incompatible candidate arrays, including an empty
array, and retain allocation/commit counter assertions. The ArrayList/HashMap/
HashSet candidate-isolation test moved from runtime-only construction shims to a
VM test using the real registered language constructors; all read, write, cleanup
and unchanged-state assertions remain. The offline host example uses the checked
registry identity returned at registration. No obsolete public allocation API or
forwarding compatibility layer was restored.

Validation so far: embedding native_provider_reset passes 10 tests; the seven
migrated runtime integration targets pass 66 tests; the relocated VM isolation
test passes. Focused runtime Clippy, including those targets and offline_host,
passes with -D warnings. Structure checking covers 831 files with no violations or
exceptions. Final focused checks and remaining compile diagnostics follow below.

Remaining compile inventory: cargo check -p kagari-runtime --tests --keep-going
reports 93 lib-test errors plus 21 in gc_ownership, 9 in offline_composite and 26
in mutation_resources. Representative errors are E0061 for untyped alloc_array,
E0599 for removed alloc_map/alloc_set and library RuntimePrimitive variants, and
E0624 for direct heap allocation. The lib-test owners include builtin/standard
(old join/string/math/Option/Result algorithms and collection intrinsic dispatch),
gc tests/array_bulk_tests, reflection, value semantics and runtime tests. The
next consumer migration must retain GC/cycle/ownership, typed payload, atomic
mutation and source-free validation coverage; predecessor-only algorithm cases
remain outside the bounded library restoration. These failures are owned by
phase 3 and were not hidden by cfg gates, disabled assertions or API shims.
Embedding native-provider Clippy also passes after deriving the now-single-field
EngineConfig default. The phase remains uncommitted; allocation/performance
acceptance, other retained consumers and phase 4 are still pending.
The corrected existing-entry mismatch regression passes separately. The offline
host example runs successfully (208-byte interface, immutable result 42). Final
fmt/diff checks pass; structure remains at 831 files with zero findings. Logs:
runtime-migrated-tests-2.log, vm-candidate-containers.log,
embed-native-consumers.log, embed-entry-mismatch.log, runtime-migrated-clippy.log,
embed-native-clippy-final.log, runtime-remaining-consumers.log and
offline-host-example.log under target/native-reset-entry/phase3/.

2026-10-02 — Phase 3 GC, native mutation and runtime consumer migration
(not a checkpoint). The remaining runtime unit/integration allocation callers
and examples now use explicit owners and element contracts. Shared nominal test
fixtures provide a checked empty allocation owner; production APIs did not gain
untyped allocation shims. The earlier runtime compile inventory (93 lib-test,
21 gc_ownership, 9 offline_composite and 26 mutation_resources diagnostics) is
resolved. The complete runtime test command passes 181 tests across 17 suites,
including its compile-fail doctest, and runtime all-target Clippy passes.

Five GC ownership cases remain in runtime; five container/recursive-graph cases
moved to native_boundary_gc and use real compiled constructors. Illegal
heterogeneous/self-containing arrays were replaced with typed recursive nominal
graphs. The deep chain still has 10,000 links (20,001 struct/Option objects), all
of which survive while rooted and are reclaimed after release. Tuple/enum/cycle
tracing, foreign/stale/tag-disguised handles, bounded formatting and structural
keys retaining identity objects remain checked. Structural-key lookups execute
the selected script Hash/Eq methods; low-level heap APIs are not used to bypass
those callbacks. The rooted_values example now declares its repeatable graph as
one array of 10,000 scalar arrays (10,001 objects / 30,001 logical units), rather
than an untyped recursive array. Its five collect/release rounds and the
collection_iteration example run successfully; no timing comparison is claimed.

The three offline_composite host tests moved to native_boundary_host so maps
and sets are constructed through real registrations. Nested argument/result
contracts, rejection before host callbacks, callback-time roots, foreign/stale
handles and frame-borrow escape checks remain. Native hash payload tests also
reject host roots and frame borrows before mutation/accounting. GC storage tests
moved to native_boundary_storage where required: stable identity/kind, traced
root ordering and cycles, removal absence versus stale/iteration errors,
replacement/duplicate semantics, resource counters, and native Option results.
Map/Set content comparisons deliberately do not impose insertion order on Rust
HashMap/HashSet. Category/reflection assertions previously requiring removed
constructors now use those actual native objects.

Predecessor-only test disposition: retired builtin_standard_string_helpers_
validate_utf8_boundaries, builtin_standard_option_result_helpers_use_standard_
enum_values and builtin_standard_math_and_debug_helpers_are_deterministic with
their removed optional entrypoints. The old LinkedHashMap ordering/MapKeysStorage
expectations are also outside the default HashMap contract. Their removal does
not restore a hidden provider or broaden the representative phase-4 library.
Array/map Option and mutation checks, iteration alias protection, duplicate
accounting, numeric result representations/error categories and the language
StringPartsJoin validation remain exercised on their current owning paths.
The old bulk map/set constructor duplicate test now uses duplicate native
insertions; the removed bulk constructor API is not emulated.

Migration exposed and fixed two concrete native mutation regressions:
- ArrayList.pop/remove and HashMap.remove mutated before allocating the returned
  Option. native-remove-repro.log records an allocation-limit trap after the
  array had incorrectly become empty. Results are now allocated before commit;
  failure preserves contents and counters. Custom-key removal reuses its one
  selected lookup, with a counter test proving one hash call for insertion and
  one for removal, even when result allocation fails. Existing successful-removal
  occupancy, peak allocation and non-refunded allocation accounting are retained.
- HashSet.remove with a custom key skipped the structural mutation guard when
  lookup found no entry. native-set-remove-repro-2.log records the unexpected
  successful execution during iteration. The guard now runs before lookup, and
  the regression checks the trap and complete root/object cleanup.

Validation: cargo test -p kagari-runtime passes all 181 tests; cargo test -p
kagari-vm --test native_boundary passes all 51 tests. cargo clippy -p
kagari-runtime --all-targets -- -D warnings and cargo clippy -p kagari-vm --test
native_boundary -- -D warnings pass. Structure checking covers 834 Rust files
with zero findings or exceptions; fmt/diff checks pass. Relevant logs under
target/native-reset-entry/phase3/: runtime-tests-all-2.log,
runtime-all-targets-clippy-2.log, native-boundary-consumers-final.log,
native-boundary-consumers-clippy-final.log, gc-consumer-structure-last.log,
collection-iteration-example.log, rooted-values-example.log and the repro logs
above. Intermediate test construction/Clippy errors were repaired, including
explicit path access for the host-root fixture.

Remaining integration inventory: cargo check -p kagari-vm --all-targets
--keep-going reports 187 lib-test errors and one warning. The principal owners
are native_required_methods/contracts, source_programs, native_lazy_iterators/
contracts, helpers/host_paths, native_keys and native_retention. Diagnostics
reference removed NativeCall/registration/continuation APIs, NativeImport
binding_version/witnesses and interface native_bridge fields, old allocation
signatures, and removed optional RuntimePrimitive variants. Phase 3 owns the
retained VM/embedding consumer migration and the bounded disposition of
predecessor-only cases; optional library coverage must follow the existing
phase-4 scope, not restore every removed algorithm. The exact diagnostics are
in vm-remaining-consumers.log. Native allocation/performance acceptance and
phase 4 remain pending. No implementation checkpoint is claimed or committed.


2026-10-02 — Phase 3 final integration unit. Migrated VM host import, JIT binding,
reentry, reflection, session and GC fixtures to the current native/typed-storage
contracts. Five source-free artifact tests now independently verify direct imports,
closed nested layouts, selected callable corruption and payload contract rejection
in native_boundary_artifacts. Optional predecessor algorithms were not restored.

The preserved module-state GC test exposed a real retention cycle: a native data
object pinned its entire executable program, so old module state kept itself alive
through an array. NativeObject now retains immutable LoadedModule metadata without
retaining module instances. Stored executable callbacks retain their actual code
as before. The original collection assertion passes; an additional rooted-data
case checks obsolete-instance reclamation, subsequent nominal element validation,
and eventual data reclamation. Callback capture/reload tests remain passing.

Added native_allocations using the real prepared ExecutionStack::invoke_native
boundary and public registration/loading APIs. A thread-local System allocator
counter has a positive allocation/reallocation/free control. After 128 warmups,
100,000 scalar calls and 1,000 contiguous i32 sum calls at each length 0, 16 and
16,384 record zero allocations, reallocations, frees and requested bytes. Scalar
elapsed time was 11,559,917 ns total; bulk totals were 111,417, 138,625 and 7,040,916
ns respectively. These are allocation-boundary measurements, not speedup claims.
Compilation, installation, linking, frame creation and input construction are
outside the measured region; results and root/object counts are checked.
Environment: rustc 1.98.1 (48a229cea, 2026-09-01), LLVM 22.1.8,
aarch64-apple-darwin, MacBookPro18,2, 32 GiB RAM, 10 logical CPUs, test profile
inheriting dev opt-level=1, default features/target/parallelism, incremental build
cache and warmed execution. Build time is reported separately in the Cargo log.
Remaining costs are ordinary execution scopes/frames for script callbacks,
allocating actual objects/buffers, tracing reference-bearing payloads, and retained
roots when values escape. No per-element allocation/root/signature scan is needed
for the primitive loop. Compiled callback dispatch is not available through the
current ScriptInvoker; no JIT execution or JIT speedup is claimed or added here.

The latest VM all-target inventory reports 131 lib-test compilation errors against
removed EngineNativeBinding/NativeCall/default-method catalogs and optional
RuntimePrimitive variants. Final phase 4 owns explicit predecessor-test disposition
and integration; this is an intentionally broken intermediate test target, not a
reason to add adapters or restore the old library. Focused logs are under
target/native-reset-entry/phase3/: native-module-state-repro.log (real failure),
native-lifetime-allocation-final.log (58 boundary cases and allocation case pass),
phase3-vm-focused-clippy.log, phase3-structure-final.log (837 files, zero exceptions),
and phase3-clippy-final.log (the carried legacy VM lib-test failures).


Phase 3 checkpoint: the explicit registration, synchronous callback, generic GC
storage, compact scalar representation and warmed allocation exit criteria are
accepted. cargo test -p kagari-runtime passes 181 tests across 17 suites, including
doc tests; cargo test -p kagari-vm --test native_boundary --test native_allocations
passes 58 behavioral cases and the measured allocation case. The independent
embedding native_provider_reset target passes 10 cases. Runtime all-target Clippy,
VM boundary/allocation-target Clippy, format, structure and diff checks pass.
The 131 legacy VM lib-test errors above remain explicitly carried to phase 4;
whole-workspace acceptance has not been claimed. Final logs additionally include
runtime-metadata-lifetime.log, phase3-runtime-clippy.log and phase3-embed-final.log.
No extra phase, compatibility layer, ABI bump or tracked binary was introduced.
