# Native modules and standard library declarations

Implementation transition (2026-10-02): the
[native collections reset plan](../native-provider-refactor.md) supersedes the
registration ownership of language protocols, Rust-derived declaration macros
and mandatory continuation examples below. Those examples describe the
predecessor implementation awaiting removal.
The target keeps complete language contracts in the compiler. ModuleBuilder
defines Kagari declarations and scoped implement/trait_impl blocks. Ordinary bind
checks Rust entry conversions against existing Kagari signatures; bind_with accepts
explicit NativeBinding codecs. Both produce the same prepared binding. Tooling
projects the same declarations; calls
are synchronous by default. The former NR restoration obligations are historical,
not acceptance requirements. Corresponding phase implementation updates the
detailed contracts.

List/MutableList, Map/MutableMap, Set/MutableSet and iteration contracts belong to
the compiler-owned catalog. Modules reference these complete contracts when
registering impls; they do not declare them again. ArrayList/HashMap/HashSet
are compiler-declared defaults with basic Rust runtime implementations always
available. Array literals create ArrayList. Concrete HashMap/HashSet impose Eq +
Hash bounds and promise no insertion/sorted order; Map/Set contracts impose neither.
Optional modules own other containers and algorithms, using the same native
binding/storage/GC registration mechanism rather than core enum entries.

Default HashMap/HashSet use Rust std::collections::HashMap/HashSet. indexmap is
backing only for optional standard-library LinkedHashMap/LinkedHashSet.

Native registration definitions own API declarations and Rust implementation
bindings. The standard library is an optional native package installed by Engine
by default, using the same path as application-owned APIs. `.kgr` declaration text
is generated for tooling; compiler semantics come directly from registered records.
Signatures, documentation and navigation locations must describe the same API.
Generated text does not authorize installation. Runtime owns storage and execution.
All comments, documentation and examples are written in English.

The `#[native_module]` attribute derives registered records and checked invocation
adapters from Rust functions, native array wrappers, traits and implementations.
`NativeValue` supplies type metadata and conversions, including resolved aliases;
Rust checks function bodies and trait signatures. Generic script values use rooted
checked proxies. `#[native]`, `#[native_type]`, `#[native_trait]` and `#[native_impl]`
mark the exported items. `#[native_default]` adds a trait default from its executable
Rust function template, retaining that template as a private registration. Low-level factories declare scratch roots and own resumable
state; ordinary typed functions do not manage scratch slots themselves. The
current adapter supports a bounded set of values and declarations, rather than
arbitrary Rust/opaque types. See [the native reset plan](../native-provider-refactor.md).

Registered free-function ABI records may carry ordinary named trait bounds,
including applied generic trait arguments. Direct HIR import checks these bounds;
generated declarations render matching where clauses and tooling coordinates.
Concrete native imports carry their substituted requirements, which source-free
verification proves against the executable dependency closure. Runtime linking
uses the sealed VerifiedProgram and requires exact equality of the complete
carried declaration with an installed registration, including bounds and passing
metadata. A weaker installed signature cannot authorize a stronger product.
The typed macro retains its bounded NativeValue Rust generic form. Injected selected
parameters additionally declare checked script constraints; broader Rust constraint
authoring remains queued.
Applicability bounds alone do not declare a callable dependency.

Registered traits may declare ordinary associated types with named output bounds;
native impl records supply exact bindings, and method signatures derive from those
bindings. Free-function signatures may retain qualified output projections. HIR
and offline proofs normalize outputs and reject missing, foreign or invalid bindings.
Generated tooling views include associated declaration, bound and binding spans.

Typed authoring accepts `type Item: NativeValue;` on an exported Rust trait and
`type Item = T;` on its actual exported impl. `Self::Item`, the own trait's qualified
projection, nested value forms and ordinary `Self` arguments/results produce the
same registered signatures. Rust checks the actual associated method signatures;
adapters use qualified concrete Rust impl types. NativeValue is a Rust conversion
requirement, not a script trait bound. Associated families, member-local generics
and broader script constraint authoring remain later work. General
application-native interface slots and selected-member callbacks share ordinary
checked target applications, including dynamic calls and retained dependency versions.

Typed callback parameters use `NativeFn<A, R>` from the continuation owner module.
`A` is an explicit outer tuple implementing NativeArguments: `()` means no
arguments, `(T,)` means one and `(A, B)` means two. Built-in packs support zero
through eight arguments, each converted left to right under the checked function
signature without allocating a script tuple for the pack. `R` is one NativeValue;
unit and Option results retain their ordinary value semantics. NativeFn.request
creates a checked rooted callback; NativeFn.result converts the resumed value.
The common frame driver owns execution, logical steps, callback-result roots and
cleanup. This replaces the earlier usize-only callback adapter; from_fn now uses
`NativeFn<(usize,), T>` with the same generated/executable script signature.
`native_value::iterator::NativeIterator<T>` adds returned iterator state. An
actual registered constructor returning the Iter representation calls
`NativeIterator::new(call, data, dependencies, step)`. Its constructor arguments
become checked GC capture edges; selected applications and their original program
remain pinned. `step` is a function pointer returning
`NativeContinuation<Option<T>>`, so it cannot hide an untraced Rust environment.
Idle data implements the sealed NativeStateData contract: scalars, tuples and
fixed-size arrays contain counters/phases; script values belong in captures.
NativeStateCall provides checked `call()`, `data()`, `set_data()` and
`set_argument()` access during one invocation. Capture replacement must match its
constructor parameter's concrete ABI, and traversal dependencies cannot be replaced.

`NativeIterator::next()` prepares a fresh common invocation with temporary roots
and scoped traversal guards. Aliases share the same cursor. Explicit cursor writes
commit progress; a later callback trap/cancellation does not undo completed source
steps. Result conversion/allocation must precede the implementation's cursor commit
where required by its protocol. The retained item ABI checks every completion.
Access epochs reject concurrent/reentrant steps and invalidate escaped access at
completion or any exceptional exit. Dropping an older access cannot clear a newer
invocation. Direct collection dependencies retain their structural revision;
optional dependencies also support script sources whose selected next controls
its own traversal policy. Nested dependencies participate in the common traversal
guard graph, including independently held guards.

The application-owned Cursor/Source/map proof in
native_state_api.rs (retired predecessor file)
uses these interfaces with defaults disabled. It checks idle GC and cyclic
collection, non-fused source semantics, shared progress and generation-pinned
selected calls. This bounded facility does not restore the full standard Iterator
family or supply arbitrary Rust state/extra capture schemas; those remain NR work.

`native_value::selected::NativeSelected<A, R>` is an injected checked trait-member
handle. For example, a generic free/inherent native entry can declare:

```rust
#[selected(T: Echo<Output = NativeArray<i32>>::echo)]
echo: NativeSelected<(T,), NativeArray<i32>>,
```

This parameter adds the script bound `T: Echo<Output = ArrayList<i32>>` and the exact
callable requirement; it is absent from the script parameter list. The registered
trait owns the member signature. Registration rejects a mismatched argument pack
or result type before publication. The Rust receiver remains `T: NativeValue` and
does not pretend that a runtime script type implements a Rust trait. The typed
authoring supports generic receiver parameters and local or explicitly cataloged
registered traits. Projected receivers and member-local generics remain queued.
Required Rust trait signatures do not inject implementation handles.

An injected PartialEq, Hash, Debug or Display dependency may select an eligible
implicit language implementation. HIR checks the registered member signature;
compilation materializes an ordinary protocol function using existing primitive
and member-composition lowering. The portable selection records a protocol-adapter
origin and the function carries its applied contract. Offline verification checks
that no explicit implementation supersedes it and that identity, signature and
function metadata match. Explicit native/script implementations keep their actual
implementation identities. All targets use the common callback driver, roots,
budgets and generation retention. This does not add a runtime protocol interpreter.
The generated function body receives ordinary executable validation; the verifier
does not prove algorithm equivalence of arbitrary replacement bytecode.

ArrayList sorting provides a concrete implementation of both callback forms:

```rust
#[native]
pub fn sort_by(&self, compare: NativeFn<(T, T), Ordering>)
    -> NativeResult<NativeContinuation<()>> {
    sorting::stable_sort(&self.0, compare)
}

#[native]
pub fn sort(
    &self,
    #[selected(T: std::cmp::Ord::cmp)] compare: NativeSelected<(T, T), Ordering>,
) -> NativeResult<NativeContinuation<()>> {
    sorting::stable_sort(&self.0, compare)
}
```

These methods are defined in the actual Rust array registration. Its catalog
contains the owning ops and cmp declarations. The injected parameter adds the
checked `T: Ord` bound and comparator application; the script call is simply
`values.sort()`. `sort_by` requires no Ord bound. Compilation, portable verification
and runtime linking consume those records without recognizing a sorting name.

`NativeArray::prepare_reorder` returns owned `NativeReorder<T>` preparation.
Its original reads and working-buffer writes validate the item representation.
Buffers are GC objects with bounded live storage, and dropping preparation releases
roots and guards. Algorithms advance bounded work through the common continuation
driver; they do not assign scratch slots. Commit consumes preparation and checks
structural guards, budget and allocation before one replacement. Comparator traps,
cancellation and pre-commit exhaustion preserve original slots; completed payload
effects and successful commits survive later failures. A retained target keeps its
original generation while each conversion scope releases handoff temporaries.

Owned trait defaults are attached to executable Rust free functions. The Rust
trait declares only its required members; no duplicate Rust method signature or
placeholder default body is needed. For a trait `Source<P>` with associated
`Output`, a real generic template can declare:

```rust
#[native_default(T: Source<P, Output = U>::echo, final)]
fn echo_template<U: NativeValue, T: NativeValue, P: NativeValue>(
    #[context] call: &NativeCall,
    value: T,
    by: P,
    #[selected(T: Source<P, Output = U>::read)] read: NativeSelected<(T, P), U>,
) -> NativeContinuation<U> {
    invoke(call, value, by, read)
}
```

The ordinary `invoke` continuation helper is implemented in the
compiling fixture (retired predecessor file).
The annotation explicitly maps T to Self, P to the trait parameter and U to
Self::Output, regardless of template generic order. The script member's parameters
and return type come from the real Rust signature; injected context and selected
handles are excluded. A default takes its receiver as its first script parameter.
The current mapping requires distinct template parameters for these roles and
rejects unmapped parameters, unknown associated types or additional unproved
obligations. Method-local generics and associated families remain queued.

The macro declares all owned default members before resolving template selections,
so defaults can depend on one another independently of Rust item order. Templates
remain private registered declarations and private generated `.kgr` functions;
only their trait members are public. Required Rust trait implementations bind only
required members. `final` rejects script/native overrides; omitting it allows an
explicit script body to replace the default. Generated member locations, docs and
completion use the same registered signatures and real Rust documentation.
Complete foreign parent/default/template dependency closure remains later NR02
work; the current authoring extends owned traits.

External authoring requests a declaration catalog explicitly:

```rust
#[native_module("game::consumer", catalog)]
```

This generates `native_api(&NativeCatalog)` instead of the zero-argument entry.
`NativeApi::catalog()` supplies immutable owned trait contracts and retained
authoring dependencies; `NativeCatalog::from_apis(&[&provider])` combines views.
Identical views may be shared, while different contracts for the same identity
are rejected. Selected annotations use the complete script declaration path,
such as `#[selected(T: game::provider::Echo<Output = i32>::echo)]`.
The consumer retains the exact selected contract and its declared parent closure.
A missing parent is an error. Catalogs do not install handlers: combine the actual
provider and consumer APIs, or install the provider before the consumer.
Composition and installation reject missing or changed contracts and validate
foreign native implementation signatures. Failed installation publishes neither
partial handlers nor partial trait declarations.

An actual Rust trait implementation can map its Rust path to an external script
identity explicitly:

```rust
#[native_impl(contract = "game::provider::Echo")]
impl provider::Echo for bool {
    // Implement the actual Rust trait methods and associated types here.
}
```

The mapping preserves the actual Rust trait path and arguments for Rust checks and
generated invocation adapters. Registered methods derive from the authoritative
trait contract; their types must also match the actual Rust method descriptors.
Parameter names in an implementation do not redefine the trait signature.
An incorrect catalog cannot authorize an incompatible Rust implementation.
Direct HIR import establishes ordinary dependencies for foreign record references;
it does not parse generated text or guess script identities from Rust aliases.
Portable loading additionally requires retained contract snapshots to match the
registered package, even when the product is otherwise well formed and sealed.
Generic applicability and parent witnesses retain ordinary HIR and portable
proofs; catalog signature validation does not replace those checks.

NativeSelected.request converts its full argument pack and enters the common
callback driver. NativeSelected.result decodes the resumed value under the checked
selected result type. The handle retains the application and original dependency
generation instead of looking up a slot in each new context. Neither the handle
nor a script closure becomes an unrestricted Rust reference. NativeResult may wrap
any NativeReturn, including a fallible NativeContinuation; its scratch requirement
and cleanup semantics come from the wrapped return type.

Ordinary Rust tuples of one through eight NativeValue members describe script
tuple values, including nested and single-element tuples; unit remains `()`.
Tuple conversion validates the complete applied shape and retains all heap fields
across subsequent conversions. This value tuple is independent of NativeArguments'
outer callback argument pack. Rust `std::cmp::Ordering` converts to the existing
script Ordering enum through checked tags, without changing comparison policy.

The optional registered `std::cmp` package owns Ordering and the PartialEq, Eq,
PartialOrd and Ord protocols. Its actual Rust implementations cover all signed
and unsigned integer widths, bool, UTF-8 String, Ordering and unit. f32/f64
implement PartialEq and PartialOrd, preserving NaN incomparability; they do not
gain Ord. Script types implement the same registered protocols. Selected Ord
calls use the shared checked native callback path and pin the selected generation.
Self-parameter protocols retain their static applicability and cannot be boxed
as dynamic interface values. Narrow unsigned values use the ABI's I64 runtime
representation with checked range conversion; u64/usize use U64.

The optional `std::hash` and `std::fmt` packages own their Hash, Debug and Display
declarations. Actual native Hash implementations cover all ten integer widths,
bool, String, unit and Ordering. Debug covers the same values plus f32/f64.
Display covers all integer widths, bool, String, unit and f32/f64; Ordering
retains no implicit Display.
Shared checked helpers preserve the runtime key hash and diagnostic/plain
rendering rules, including UTF-8, string escapes, signed zero and the existing
one-MiB formatting bound. Native work charges scalar input and output bytes.
These protocols retain their static-only interface applicability. Application
protocols use the ordinary checked native slots and may define dynamic interfaces.
Implicit identity/member semantics remain language primitives; carrying their
callable facts into selected native callbacks remains an open migration item.

Installed native modules may declare a package alias independently of canonical
identity. Conflicting aliases or an alias shadowing another installed canonical
package reject composition. HIR resolves registered representation references
from actual owning declarations, including application names, with ordinary
checked imports. Minimal optional String and Option type providers allow this
path without installing the default library. Their methods and default
namespace/prelude exports remain pending restoration; no missing provider is
replaced with a synthesized type declaration.

`native_value::result::NativeResultValue<T, E>` represents a rooted script Result.
Reading and returning it preserve the original enum value and Err error trace.
Its payload method decodes the selected checked branch; from_result constructs a
fresh Result under the call's declared result type, with fresh Err origin tracking.
`NativeResult<T>` continues to represent native execution success/failure. Script
Err values remain ordinary values and do not become runtime traps. Arbitrary Rust
Result conversion and opaque Rust value representations are not implied.

The NR implementation registers ArrayList new/len/push/from_fn, List
len/get and MutableList.set, plus all eleven specified math helpers. Trait implementation
signatures derive from the registered trait declaration rather than being authored
a second time. Closed numeric adapters derive OrderedNumber/SignedNumber bounds
from actual Rust signatures. They remain sealed engine predicates, independent
of user ordering implementations. Math inputs/results must be finite; sqrt rejects
negative inputs, abs checks the applied signed integer width, and clamp validates
its bounds before comparing the value. Equal min/max operands preserve the left
operand, including the sign of floating-point zero.
Remaining source package declarations and namespace/prelude metadata use their legacy route during
restoration; the rules below describe that route where not superseded here.
Other predecessor library APIs are withdrawn by the reset. Retained language
semantics and boundary tests remain required; complete restoration is out of scope. See the
[active plan](../native-provider-refactor.md).

## Declaration mode

`parse_declarations` is an explicit, cancellable parser entry with ordinary parser
limits. It permits top-level `fn ...;` signatures and opaque `pub type Name<T>;` declarations. Ordinary source parsing still
requires a body. Parsing an interface does not grant code-generation authority.
Standard library sources are installed by the engine, not discovered from user
imports or recognized by a user-controlled file extension.

`stdlib/std.kgr` declares the public namespace through ordinary re-exports.
`stdlib/prelude.kgr` declares the types and protocols available without an explicit
import. HIR installs the `std` package alias and those prelude bindings only from
the prepared engine package. A local declaration, explicit import or glob binding
takes precedence over an implicit prelude binding; an ambiguous explicit import
does not fall back to the prelude. Standard modules, functions, types and traits
are ordinary source import targets. Option and Result re-export their variants,
and the prelude imports those source declarations. Constructor and pattern facts
retain the variant identity, checked payload types and the owning enum's native
representation hook. Checked callable applications carry the same identities and
native/script implementation selections into portable execution contracts.

Functions and methods returning unit omit the return annotation, for example
`fn clear(self);`. Callback function types still spell out `-> ()`, as in
`fn for_each(self, callback: fn(Self::Item) -> ());`.
Non-returning operations use `-> !`, including `std::debug::panic`. Rust native
functions use NativeResult<NativeNever> for the actual uninhabited result; no Unit
value or synthetic script body substitutes for Never. These signatures participate
in ordinary static coercion, portable import validation and registered invocation.
NativeNever differs from the declared conversion-protocol Infallible enum. Checked
signatures and native execution contracts retain Never rather than Unit.

Outer `///` comments belong to the immediately following declaration. They retain
Markdown including fenced Kagari examples. The CST remains lossless. `#[native(binding)]` supplies an opaque module-qualified binding ID. The old intrinsic,
numeric, radix, protocol and default marker families are removed. User annotations
cannot install handlers or acquire provider authority. Instance methods are declared with
`self` inside an inherent or native trait `impl`; there is no method-alias attribute.

The implementation sequence and acceptance status are tracked in
[the implementation roadmap](../implementation-roadmap.md).


## Public functions and method views

For remaining legacy modules, `kagari-stdlib` reads the installed `.kgr` manifest with the declaration parser
and retains the exact text, syntax trees, annotations, documentation and declaration
coordinates. HIR imports that package through ordinary declaration collection and
checks signatures and bounds through ordinary HIR. A native declaration with a
script body is an error; an unknown binding ID fails executable linking. Ordinary script bodies are
retained for checking and execution. Public declarations must be documented;
documentation examples remain part of acceptance. The ABI does not parse sources
or generate source descriptors. Consumer migration and validation status are
tracked in the [active plan](../stdlib-hir-refactor.md#progress-ledger).

Generic inherent blocks own their receiver parameters, for example
`impl<T> ArrayList<T> { pub fn get(self, index: usize) -> Option<T>; }`.
Associated constructors omit `self`. Method-specific generic parameters extend
the impl parameters; `Self` resolves to the impl target. Concrete targets such as
`impl ArrayList<String>` restrict methods to that receiver shape. Metadata and source
identities are derived by HIR from these declarations, including read-only versus
mutable receiver access. Method syntax and qualified calls such as
`ArrayList::get(values, index)` share the same checked signature. Old module-level
method functions are removed. Genuine free functions remain at module scope.
Native `Iterable`, `OrderedNumber` and `SignedNumber` constraints retain their
existing restricted meanings. They do not grant arbitrary Iterator or operator
implementations access to native helpers.

Public documentation follows the [rustdoc writing guidance](https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html):
a concise summary, behavior and boundary details, applicable Panics sections and
executable Examples. Kagari's Panics sections describe script traps, not Rust
unwinding. Examples run through source compilation and encoded artifact loading.
Numeric literals use context before falling back to i32 or f64; suffixes such as
`1usize` and `1.0f32` select an explicit primitive type. These rules do not introduce
implicit numeric conversions. See the [literal rules](syntax.md#literals).

## Native types and standard protocols

The same files declare Option, Result, Ordering, Array, Map, Set, String and Iter.
Native declarations bind existing engine representations; they do not define empty
script structs. Enum variant order and payload counts are checked against the
runtime discriminant contract. Primitive scalar representations remain engine-owned.

Remaining legacy standard traits derive their public contracts from these sources, including
supertraits, generic parameters, methods, associated types and associated bounds.
Trait solving and native implementations remain engine code. Declaration identities
and member locations refer to the bundled source text, not placeholder spans.
Method-local generic parameters and enclosing impl binders are retained in ABI
signature checks. Readonly interfaces expose their declared methods through
ordinary bridges; native bindings carry no receiver Read/Write flags. The registered List retains its Index supertrait; ArrayList supplies both len and
get through registered native entries. MutableList extends List and adds set.
Portable native default applications name an ordinary registered template and
explicit generic arguments. Their checked signatures and bounds replace
method-specific traversal and conversion catalogs. Registered records enter HIR
and produce ordinary native calls and interface slots. Typed Rust authoring adds
owned default members from real function templates. Complete external default-template
dependencies remain NR02 work.

Installed native defaults that forbid replacement explicitly carry
`#[method_policy(Final)]` in their declaration. Unannotated defaults remain
overridable. HIR records this policy independently of the native binding, and
portable callable declarations preserve it for interface validation. Installation
rejects a final required method; user-written policy attributes do not acquire
installed declaration authority.

`iter.kgr` retains required iterator/aggregation declarations. The old Iter impl
and native adapter defaults are removed. Restoring them requires provider-owned
persistent traced state and checked stepping entries under NR03/NR04. Identity
Iterable remains a language protocol rule, not native binding authority.

Documentation examples for functions, types and traits execute both directly and
after artifact serialization. Nested members document their role within the enclosing
protocol; protocol examples demonstrate the complete use rather than duplicating
the same example on every associated type.

## Checked callable execution

Required declarations have no body or executable target. Script declarations retain
ordinary checked bodies; native declarations have opaque binding IDs and no synthetic
script body. Engine functions, native impl methods and native defaults carry concrete
callable identities, checked substitutions, full parameter/result types, bounds and
selected protocol applications. Compiler lowering encodes these facts without
looking up declaration syntax or expanding public standard algorithms.

`NativeDefault` is not an executable implementation kind. Its declaration names a
native template and explicitly maps Self, trait arguments, ordinary associated
outputs and method arguments to template arguments. Portable validation checks
template existence, parameter mutability, exact applied parameter/result types and
the template's bounds under the trait contract, including parent obligations.
Concrete selection normalizes the mapping and resolves a normal Native instance.
A final default implementation must retain that canonical application; substituting
an unrelated native binding or script body is invalid. Dynamic slots authenticate
the same applied signature and target against the complete dependency closure,
even when the template belongs to another module or multiple members share it.
Unresolved default applications cannot enter selected callbacks or runtime linking.
The current acceptance covers portable proof, raw registration and source-to-encoded
execution for direct, generic, dynamic and selected calls, including native and
script receivers. Source implementations may omit inherited defaults; final
methods reject overrides. Native template callback targets are materialized in
their actual owning module and retain script generations across reload. Typed Rust
owned defaults share these contracts. Complete external default/template catalogs,
projected requirements and associated families remain open.

Portable verification validates each native application against its carried source
declaration. Runtime linking resolves its ID and checks the applied signature against
the trusted installed declaration before pinning the implementation. Source compilation
exports those declarations; no independently authored standard contract table exists. Host bindings additionally retain
their offline host declaration and authority contract. A user declaration with a
native-looking attribute cannot claim installed engine provenance.

Runtime continuations own callback-heavy standard algorithms, buffered construction
and all lazy adapter steps. They use the caller's frames, roots and resource scope;
selected script implementations and explicit overrides use ordinary linked calls.
Readonly interfaces hide mutation methods; ordinary checked bridges retain the
concrete implementation signature. Native imports cannot independently weaken or
upgrade receiver access. The final ownership map is in the
[integration plan](../stdlib-hir-refactor.md#final-implementation-ownership-audit).

## Tool queries

`AnalysisSnapshot::source` reads ordinary analyzed files and the installed standard
package retained by that snapshot. `definition_at` and `declaration` identify source
declarations by their real identifier ranges. `AnalysisSnapshot::documentation_at`
provides the resolved declaration, written syntax and Markdown for both user and
standard source. `DeclarationSnapshot::documentation` accepts a declaration identity
without checking bodies. Neither query falls back to a process-global source catalog;
old snapshots retain their original source revision and documentation.

`DeclarationSnapshot::files` enumerates user files, inline modules and the installed
package within the same snapshot. Declaration inventories and executable Markdown
examples use these files and declaration IDs, rather than an ABI source table.
Trait and impl associated types participate in the declaration index with separate
owner identities, exact name ranges and their own written documentation.

`FileAnalysis::call_signature_at` projects the selected local/imported function,
trait method or offline host declaration into a declaration ID, named parameter
types and result type. It applies checked call substitutions; it does not read a
standard signature catalog or replace declared types with the types of erroneous
arguments. Method syntax omits the receiver parameter, and missing arguments do
not remove declared parameters. The query does not register or execute callbacks.

`FileAnalysis::method_completions` returns declaration IDs and names for source
methods, including incomplete member expressions. Inherent receiver matching and
trait interface selection share the HIR call-checking path. Known receiver and
method bounds filter candidates; unsupplied method type arguments remain open.
Explicit implementations retain their declaration IDs, while inherited defaults
refer to the trait declaration. The query does not load a separate standard method
table. The [integration ledger](../stdlib-hir-refactor.md#progress-ledger) records validation
of the shared semantic and executable boundaries.

Standard trait methods and associated types have ordinary declaration identities.
Installed collection and iteration implementations are selected from checked HIR
impl patterns, bounds and associated types. Native selection additionally requires
installed-package provenance; a user implementation of the same trait keeps its
ordinary dispatch. Readonly native storage may satisfy read/iteration capabilities,
but it cannot satisfy mutable capability implementations.
Each snapshot owns immutable declaration facts. User declarations take precedence over
unqualified native names; navigation follows resolution, not a text-name heuristic.
Native method candidates are one input to completion; lexical trait completion and
the LSP transport remain separate tool work.

The analyzed `AggregateCatalog` exposes checked implementations and selects native
applications for a receiver, with source identities distinct from the trait's members.
Completion on `Iter<T>` includes the declared `next` implementation and inherited
`Iterator` methods. Ordinary trait-call navigation still identifies the protocol
member; the implementation catalog provides the concrete implementation location.

Native method declarations retain where predicates and qualified associated-type
projections. For example, Iterator::min requires Self::Item: Ord; this obligation
is checked at call sites and retained in portable method contracts.


Collection SDK files explicitly declare Iterable and FromIterator implementations
for ArrayList, LinkedHashMap and LinkedHashSet; String declares Iterable.
The same files declare List/MutableList, Map/MutableMap and Set/MutableSet
traits and native impl witnesses. Their inherited members and source locations
are available to semantic queries, including read-only member completion.
Result and Option declare their conditional FromIterator implementations. Generic
arguments, key constraints, destination bounds, associated outputs and member
locations come from those declarations. HIR validates the supported native
signatures and installed bindings. Static constructors do not appear as instance
completion candidates. Iterable inheritance through a generic associated Iter
retains the originating Item equality constraints.

Option and Result FromIterator providers execute through rooted native traversal.
They stop at the first None or Err, close guarded native iterators and skip the
inner destination constructor on failure. Result lifting preserves the original
error object and its origin. On success, including empty input, they construct the
selected inner destination once from the prepared ArrayList and then wrap it.
Nested lifting repeats this contract with bounded, checked destination applications.
Script constructors and source methods run on the caller's ordinary execution
frames; native Array/Map/Set constructors share their existing traversal and key
lookup implementations. Portable linking verifies source item equalities, concrete
constructor method arguments, implementation and method bounds, private dependency
closure and any key witnesses before loading. Each original logical operation keeps
its instruction charge and completed side effects across cancellation and traps.

## String query semantics

String search and slicing use UTF-8 byte offsets. `find`/`rfind` return the first/last
substring offset or `None`; empty patterns match zero/the byte length respectively.
`strip_prefix` and `strip_suffix` remove exactly one match, returning `Some` even
for an empty pattern. Trimming uses Unicode White_Space; start/end variants retain
whitespace on the opposite end. No operation normalizes Unicode or mutates its
receiver. Returned strings own their contents; optional results are ordinary
GC-rooted Option values.

String splitting returns lazy, fused `Iter<String>` values. `split` retains empty
fields and matches non-overlapping string separators. The empty separator yields
both empty edge fields and one field per Unicode scalar. `splitn(n, separator)`
yields at most n fields, with the unsplit remainder in the last field; zero yields
none. `split_once`/`rsplit_once` exclude the first/last matching separator and return
None if absent. `split_whitespace` coalesces Unicode whitespace without empty
fields. `lines` recognizes LF and CRLF, retains lone CR and omits an extra field
after a final terminator. Empty input has no lines. Traversals retain their source
and version; each step prepares its result before advancing the shared cursor.

String replacement matches non-overlapping patterns from left to right; the empty
pattern matches each scalar boundary, including both ends. `replacen` limits the
number of replacements. `repeat` checks the output size before allocation; empty
input stays empty for any count. ASCII casing only changes ASCII letters. Unicode
casing uses context-sensitive, locale-independent Unicode mappings and may change
length. `bytes` yields u8 values; `char_indices` yields (byte offset, one-scalar
String). `is_char_boundary` accepts the start/end and rejects out-of-range offsets.

The restored direct helpers are registered from actual Rust methods; tooling views
derive from these checked declarations. NativeTextBuffer prepays output-byte work,
uses a fallible reservation and prevents unchecked capacity growth. Input scans
charge length-dependent work before running; append loops also check cancellation
and deadlines. These checks preserve the existing inline String representation
and allocation-unit accounting, rather than charging Rust string bytes as GC
object allocations. Unicode lowercase uses Rust's context-sensitive algorithm,
with input work charged before mapping and output work charged afterward. Lazy
traversal and method-generic parse remain later restoration steps in the active plan.

## Option and Result combinations

`unwrap_or_else`, `or_else`, and `map_or_else` invoke only the selected callback,
once. `map_or` evaluates its fallback eagerly like ordinary arguments. Predicates
run only on the requested variant. Option filter retains the original shared
payload when accepted; zip combines two present payloads without deep copying.
`flatten` removes one layer; Result flatten requires the same error type on both
layers. `transpose` exchanges Option and Result. Forwarded Err values preserve
the original trace through flatten and transpose; errors returned by recovery
callbacks retain their own origins. `ok`/`err` discard the opposite variant and
project the payload into Option (which has no error-trace metadata). All callbacks
execute on ordinary VM frames with session budgets, roots and trap cleanup.

## String parsing

`FromStr` has associated type `Err` and static `from_str(String)` returning
`Result<Self, Self::Err>`. String.parse selects this protocol from the expected
type or explicit type argument. Numeric and boolean implementations are declared
in the bundled standard package; user types provide ordinary implementations with their own errors.
All built-in parsers consume the complete input without trimming.

The public parse call carries its checked FromStr witness, exact associated error
type and selected concrete method into the native runtime entry. Numeric providers
reuse the Rust parser; user methods execute on ordinary generation-pinned frames.
Generic associated errors resolve from the carried dependency closure. The entry
returns the original Result, preserving its error origin and logical charge schedule.

Integers use decimal unless from_str_radix specifies 2..=36; plus is accepted,
minus only for signed targets. Prefixes, underscores and whitespace are rejected.
Overflow returns ParseError::OutOfRange. Invalid radix returns InvalidRadix
(instead of Rust's radix panic); empty input returns Empty and invalid digits or
signs return InvalidDigit. Bool accepts exactly true/false. Floating parsing uses
decimal/exponent syntax and case-insensitive NaN/inf/infinity with optional sign;
overflow produces infinity. Invalid boolean/float syntax returns InvalidSyntax.
These are business Result errors carrying their creation trace, not VM traps.

## Equality assertions

`std::debug::assert_eq<T: PartialEq>` evaluates both arguments and its message once,
in order, before comparing them. Its checked native entry delegates to the selected
primitive, script, composed or declared collection-interface equality. Script and
composed callbacks use ordinary execution frames, roots and session limits. The
final assertion uses the existing Rust assertion helper on the original second
charge; failed assertions and callback traps retain their caller and script origins.
