# Kagari Implementation Roadmap

[Foundation refactor](foundation-refactor.md) is the sole active R01–R18 execution plan. Its three semantic contracts and checkpoint status define the behavior to implement and verify. Each completed checkpoint requires a Conventional Commit with a `Roadmap-Step: Rxx` trailer. The [performance baseline](performance-baseline.md) records R18 measurements.

The former M1–M11 milestone queue is historical and has been removed from this document. Git history retains its original scope and commits; those milestones do not prescribe current APIs, compatibility branches, artifact formats, or acceptance criteria.

After the foundation track, plan separate work for complete LSP/editor integration, a full incremental dependency database, async and cross-thread execution policy, incremental or generational GC, complete event replay and persistent state migration, advanced JIT optimization, and further standard-library coverage. These tracks reuse the foundation contracts without reopening their semantics.

Completed language extension: ordinary associated types, equality bindings,
projection bounds and qualified projections, preserving the existing static and
dynamic interface model. The [trait contract](spec/traits.md#ordinary-associated-types),
[runnable example](../examples/syntax/associated-types.kgr) and associated-type
integration tests define this checkpoint. Subsequent trait extensions are
recorded below; advanced coherence/solver features remain future work.

Completed language extension: generic implementation interface tables, keyed by
impl declaration and concrete arguments, with shared instantiation limits and
cross-module method reachability. See the [trait contract](spec/traits.md#generic-implementation-interface-instances)
and [runnable example](../examples/syntax/generic-interfaces.kgr). Artifact format
44 and runtime ABI v44 reject prior formats without migration.

Completed language extension: host associated-output declarations and dynamic
interfaces, using offline contracts and verified IR forwarding functions through
the existing host boundary. See the [trait contract](spec/traits.md#host-associated-outputs-and-interfaces)
and [embedding example](../crates/kagari-embed/examples/host_interfaces.rs).
Integration tests cover source/artifact execution, JIT-enabled execution,
cross-module generic inputs, GC, reentry, traps and retained reload versions.
Artifact format 45, runtime ABI v45 and KHI v10 reject previous products.

Completed language extension: bounded, acyclic trait inheritance, transitive
static bounds, inherited associated projections and dynamic interface upcasting.
Diamond paths deduplicate the same applied declaration; unrelated same-named
methods are ambiguous. Source, artifact and JIT fallback tests cover cross-module
generic parents, invalid graphs, GC and retained reload versions. See the
[trait contract](spec/traits.md#trait-inheritance-and-upcasting) and
[runnable example](../examples/syntax/trait-inheritance.kgr). Artifact format 46
and runtime ABI v46 reject previous products; KHI remains v10.

Completed language extension: checked default method bodies, explicit override
precedence and fallback for script impls, including generic impls, method-local
generics, associated outputs, closures and imported private helpers. Defaults
are specialized in the impl's module while retaining the trait's checked name
resolution and original debug source. See the [trait contract](spec/traits.md#default-methods)
and [runnable example](../examples/syntax/default-methods.kgr). Artifact format 47
and runtime ABI v47 reject previous products; KHI remains v10. Host mappings
continue to explicitly provide every declared method.

Completed language extension: scalar associated constants, required definitions,
defaults and overrides, inherited access and qualified static paths. Source and
artifact validation reject constant-bearing dynamic interfaces, including
subtraits. Artifact format 48 and runtime ABI v48 reject previous products;
KHI remains v10. Native host tables cannot supply constants; use script impls.

Completed language extension: type-parameterized generic associated types,
declaration-owned constructor binders, input and output bounds, inherited
projections, generic impls and defaults. Source, encoded artifacts and JIT-enabled
execution cover static specialization, imported contracts and malformed unused
metadata. See the [trait contract](spec/traits.md#generic-associated-types) and
[runnable example](../examples/syntax/generic-associated-types.kgr). Artifact
format 49 and runtime ABI v49 reject previous products; KHI remains v10.

Traits declaring associated constants or GAT, including every descendant,
are static-only. Native host tables cannot supply those members; script impls
on host types can. Lifetimes, script-level `dyn`, higher-kinded type parameters,
associated type defaults, specialization, negative impls, auto traits and
advanced coherence/solver behavior are outside this sequence.

Completed language extension: built-in Option/Result constructors, namespace and
alias imports, nested and alternative patterns, postfix `?`, explicit Option to
Result conversion, and checked script callbacks for map/map_err/and_then.
Source, encoded artifacts and JIT fallback share ordinary frame control flow,
including GC and iteration cleanup. See [builtins](spec/builtins.md#option-and-result)
and [result-option.kgr](../examples/syntax/result-option.kgr). Artifact format 50
and runtime ABI v50 reject previous products; KHI remains v10. General built-in
propagation traits remain deferred; error origins/stacks are completed in E01–E03 below.

Completed standard-protocol checkpoint: declaration-owned PartialEq/Eq/Hash and
Debug/Display, ordinary generic and associated bounds, sealed intrinsic equality
and hashing, explicit nominal formatting impls, structural and identity keys,
and GC tracing of keys. See [contracts](spec/builtins.md) and
[standard-traits.kgr](../examples/syntax/standard-traits.kgr). Artifact format 51,
runtime ABI v51 and KHI v11 reject old products. Protocol interface boxing
and generalized propagation remain separate later checkpoints. Error origins
and stacks are completed in E01–E03; Iterator/Iterable are completed in B08 below.

Completed equality checkpoint: object identity operators `===`/`!==`, checked
from syntax through IR and VM, with artifact format 52 and runtime ABI v52.
Source/artifact/JIT-fallback tests cover aliases and separate allocations; runtime
tests reject foreign, stale and mistagged handles.

Completed custom equality and hashing checkpoint: explicit Struct and enum
PartialEq/Eq/Hash implementations, generic bounds, recursive composite helpers,
and guarded Map/Set callbacks. Native defaults retain their fast path. Type-owned
implementations keep dependency and caller semantics consistent. Tests cover
cross-variant enum equality, nested keys, collisions, GC, callback traps, reentry,
budget cleanup, portable metadata and source/artifact/JIT fallback execution.
See the [contract](spec/value-semantics.md#equality-and-hashing) and
[example](../examples/syntax/standard-traits.kgr). Artifact format 53 and runtime
ABI v53 reject previous products; KHI remains v11. Key stability and equivalence
laws are user obligations; there is no automatic freezing or reindexing.


## Standard operator protocols

The B01–B06 extension sequence is complete. Each checkpoint updates contracts,
examples and relevant tests and is committed separately using Conventional Commits.
Clone, writable indexing and compound-assignment overrides are separate designs.

- [x] B01: declaration-owned standard protocol inputs, associated outputs and parent metadata shared with portable contracts.
- [x] B02: Ordering, PartialOrd and Ord, including checked comparison dispatch.
- [x] B03: Add/Sub/Mul/Div/Rem with an explicit RHS type and associated Output.
- [x] B04: Neg/Not with associated Output.
- [x] B05: read-only Index; returned objects retain shared reference semantics.
- [x] B06: source/artifact/backend conformance, examples, documentation and workspace checks.

Builtin operations retain direct instructions. Custom implementations use ordinary
static linked calls. Operands evaluate once from left to right. These protocols
remain static-only in this sequence. Short-circuit and identity operators cannot
be overridden. Existing assignment and mutation guarantees are unchanged.

The sequence uses artifact format 57 and runtime ABI v57; earlier products are
rejected without migration. KHI remains v11. The operator conformance suite checks
source, encoded artifacts and JIT-enabled fallback with collection at allocation
safepoints, cross-module generic implementations, single evaluation and target
identity, malformed contracts, unordered floats and preserved builtin fast paths.

B06 validation: 1,116 workspace tests passed (including 17 operator integration
cases); `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --
-D warnings` and `git diff --check` passed. Integration cases also cover applied
RHS overload selection and resource cleanup after operator traps/budget exhaustion.

## Conversion and iteration protocols

- [x] B07: explicit From/Into and TryFrom/TryInto, static conversion calls, derived reverse bounds and associated errors.
- [x] B08: Iterator/Iterable contracts, custom for loops and native collection integration.
- [x] B09: conformance, examples, resource cleanup and workspace validation.

Each checkpoint uses a Conventional Commit. Conversions are explicit; reverse
protocols are derived and cannot be implemented independently. Iteration preserves
native structural-mutation guards; custom iterators own their consistency rules.
General propagation, Clone and writable indexing remain separate. Error origins
are completed in E01–E03 below.

B08 uses artifact format 59 and runtime ABI v59. Custom iterators and native
iterators share protocol-based for lowering. Native iterators retain source guards,
GC roots and code versions; loop/session cleanup and structural revision checks
cover early exit and resumed iterators. See the authoritative iteration contract in
[the builtins specification](spec/builtins.md#iteration-protocols).

B09 validation: 1,132 workspace tests passed, including 6 conversion and 10
iteration integration cases. Source, artifact roundtrips and JIT fallback cover
single evaluation, qualified conversions, derived bounds, cross-module generic
implementations, native/custom iteration and associated output checking. Invalid
iterator instructions are rejected before execution. Cleanup tests cover nested
loops, return, trap, cancellation, budgets, rooted iterators across GC, foreign/stale
handles and structure changes between calls. Formatting, workspace clippy with
warnings denied, and `git diff --check` passed. The former generic Iterator<T>
proposal now references the implemented associated-Item contract.


## Error origins and diagnostic stacks

- [x] E01: bounded source-aware stack snapshots for runtime failures, including native JIT program points and portable source locations.
- [x] E02: Result Err origin metadata, preservation through propagation/combinators, host reports and CLI rendering.
- [x] E03: source/artifact/backend, lifecycle and diagnostic conformance; documentation and workspace checks.

Errors remain Result values. New Err captures its origin; propagation preserves it.
Reconstruction creates a new origin. None does not carry an error stack. Error
traits, cause chains and generalized propagation remain subsequent features.

E02 uses artifact format/runtime ABI 61 and helper ABI 6. The authoritative
[error-reporting contract](spec/error-reporting.md) covers origins, metadata
propagation, host inspection and CLI presentation.

E03 validation: 1,148 workspace tests passed, including 13 error-reporting
integration cases. Coverage includes imported frames, UTF-8/CRLF locations,
minimal artifacts, source edits after compilation, native overflow positions,
propagation and reconstruction, equality/hash independence, bounded recursion,
GC/reload, host reentry and cancellation/budget cleanup. Malformed mapped-error
contracts and registers are rejected before execution. CLI source/artifact
reports agree; the optional JIT CLI test also passed. Formatting, workspace
clippy with warnings denied and `git diff --check` passed.


## Standard library declaration sources

Source comments and API documentation are written in English.

- [x] S01: declaration parsing, documentation and source-location foundation.
- [x] S02: source-owned standard function signatures and method bindings.
- [x] S03: standard enum/type declarations and the 21 standard trait contracts.
- [x] S04: shared semantic queries for navigation, documentation and signatures.
- [x] S05: executable documentation, binding validation, removal of duplicate definitions and workspace acceptance.

This sequence migrates all 71 existing public standard functions and 54 method
views without expanding their runtime semantics. Host API declaration generation
and the LSP transport remain subsequent checkpoints. Native layout, GC, mutation,
resource and bytecode-validation contracts remain engine responsibilities.

S02 migrated 71 function signatures and 54 method views into English declaration
sources with executable examples. It removed duplicated intrinsic call typing and
connected native for_each to ordinary script closure frames and iterator cleanup.
Validation includes 327 HIR tests, 146 IR tests and source/artifact documentation
execution. Rustdoc-style API documentation is required for subsequent declarations.

S03 replaced handwritten standard enum, type-constructor and 21 trait signature
builders with source-derived metadata. S04 connected native function and method
navigation, type/variant definitions, trait members, Markdown, instantiated native
signatures and incomplete native-member candidates to the bundled source catalog.

S05 provides 100 executable English examples across 71 functions, eight native
types/enums and 21 traits. Documentation follows Rust's summary, behavior,
applicable Panics and Examples structure while describing Kagari semantics.
Native enum discriminants and payload types are validated as runtime ABI bindings;
public signatures must instantiate without unknown/error types. The standard API
index is [stdlib/README.md](../stdlib/README.md). Full LSP transport, lexical trait
completion, scalar reference pages and host declaration generation remain future
work. This batch does not implement those separate features.

Acceptance: 1,159 workspace tests passed. All 100 documentation examples passed
source and artifact execution; formatting, workspace clippy with warnings denied
and `git diff --check` passed.

## Rust-style outer attributes

The attribute spelling is `#[path(...)]`, including argument-free `#[meta]`.
The legacy `@...` spelling is removed without a compatibility parser. Attribute
arguments, semantic validation and native bindings retain their existing behavior.
Inner `#![...]` attributes and a macro system are not introduced. The grammar,
standard declarations, examples and tests use the same outer-attribute syntax.

Validation: 1,161 workspace tests passed, including the 100 standard API
examples. Formatting, workspace clippy with warnings denied and `git diff --check`
passed. Recovery tests cover malformed delimiters, rejected legacy/inner attributes,
lossless nested metadata and following declaration recovery.

## String interpolation and joining

- [x] Lossless, bounded `f"..."` parsing with expression holes, escaped braces,
  ordinary text escapes and nested interpolation.
- [x] Canonical Display/Debug selection, left-to-right single evaluation and
  ordinary propagation/trap cleanup through existing call frames.
- [x] `[String].join(separator)` and `std::array::Array::join`, using checked byte-length
  accumulation and one result-buffer reservation; concat remains available.
- [x] English API docs, runnable example and EBNF coverage inventories.

Artifact format and runtime ABI are v62. The helper ABI remains v6. Old artifacts
are rejected; no compatibility decoder is introduced. Width/precision formatting,
String `+`, StringBuilder and compile-time interpolation remain separate work.

Acceptance: 1,173 workspace tests passed, including source/artifact/JIT-path
interpolation tests, 101 standard API documentation examples, Unicode/source-query
rebasing, parser limits and native argument validation. Formatting, workspace
clippy with warnings denied and `git diff --check` passed.

## Collection access and construction

The [collection access contract](spec/collection-access.md) defines the implemented
read-only/writable Array, Map and Set types and paired associated constructors.
`[T]` means read-only `Array<T>`; literals infer `MutableArray<T>`. The implementation
keeps shared storage and enforces access in HIR and linked bytecode contracts.

- [x] C00: document access boundaries, constructor shape and implementation plan.
- [x] C01a: carry collection access through HIR type identity/substitution, ABI
  types and bounded host/artifact encoding; migrate existing Rust call sites to
  explicitly request their current mutable access.
- [x] C01: source-owned native access types and associated constructors; HIR
  assignability, generic invariance, branch joins and complete write-access checks.
- [x] C02: verified IR, artifact encoding/version rejection, host declaration
  contracts and runtime/backend integration without duplicating collection storage.
- [x] C03: migrate standard declarations, CLI/embedding examples and executable
  documentation; implement paired populated `Array`/`Map`/`Set` and `Mutable*`
  factories with fresh shallow storage, remove old constructors and update
  authoritative specifications.
- [x] C04: navigation/completion and source/artifact/backend conformance, negative
  access tests, GC/alias/iteration coverage and final workspace validation.

C01-C03 land together because the public API, typed execution contracts and
standard declaration/example migration must agree. No compatibility constructors
or dual mutability model remain. `Type::from(array)` creates fresh shallow storage;
map entries use `(K, V)` tuples and both access variants use checked insertion.
General variance, frozen/persistent collections, deep immutability, general
copy/clone protocols, capacity APIs and variadic calls remain separate work.
Arbitrary iterator construction is tracked in the iteration checkpoints below.

C01a introduced `CollectionAccess::{ReadOnly, Mutable}` in semantic types, KBC/runtime
ABI v63 and host interface KHI v12. C01-C03 activate the public access types and
advance KBC/runtime ABI to v64. Semantic parameter/local/register/result contracts
are encoded and fingerprinted; loading validates calls, stores and writes before
execution. Physical frame slots and shared GC storage remain unchanged. Access
changes participate in host/interface/reload contracts. Earlier artifacts are
rejected, without migration. Helper ABI remains v6.

Constructors and methods navigate to source-owned declarations; completion omits
mutators on read-only receivers. Result/Option propagation retains error provenance
while explicitly recording the resulting enum contract. Paired factories use
normal script frames for custom key protocols, preserve argument order and release
input/lookup guards on failure. Array/Map/Set factory results never reuse input slots.

C01a validation: 1,176 workspace tests passed, including 101 executable standard
API documentation examples. Workspace clippy with warnings denied, formatting
and `git diff --check` passed.

C01-C04 validation: 1,188 workspace tests passed, including 114 executable
standard-library documentation blocks. Collection integration tests exercise
source/serialized-artifact/JIT-enabled fallback, live views and shallow snapshots,
custom Eq/Hash, input iteration guards, GC during host reentry, host binding access
mismatches, malformed executable access contracts and negative writes through
methods, free functions, indexing, reflection, generics, closures and branch joins.
Constructor navigation and read-only member completion have query coverage.
`cargo fmt --all -- --check`, workspace/all-targets clippy with warnings denied,
`cargo test --workspace --no-fail-fast` and `git diff --check` passed.

## Standard methods and lazy collection pipelines

This sequence replaces the previous method aliases and native-only iteration helpers.
Each checkpoint includes specifications, examples, relevant tests and a Conventional
Commit. No compatibility aliases are retained.

- [x] I01: source-owned inherent methods and associated functions in generic impl
  blocks; remove method attributes and migrate native API calls and queries.
- [x] I02: unify Iterable/Iterator and iter, including derived iterator identity,
  for-loop conversion, associated outputs and user-defined protocols.
- [x] I03: FromIterator and target-directed collect for all six collection types
  and user-defined collections; fresh shallow construction and checked key insertion.
- [x] I04: lazy map, filter, filter_map, take, skip, enumerate, zip and chain.
- [x] I05: find, any, all, count, fold, for_each, partition and whole-input group_by.
- [x] I06: shared iterator progress, short-circuit continuation, guard lifetimes,
  callback failures, budgets and GC retention across adapter chains.
- [x] I07: executable English API documentation, examples, source/artifact/backend
  conformance and final workspace formatting, clippy and test validation.

Kagari uses iter without ownership transfer; callbacks receive ordinary values.
group_by is a Kagari extension returning MutableMap<K, MutableArray<T>>. flat_map,
flatten, sum/product and Result/Option collection lifting remain follow-up work.

I01 validation: HIR and embedding tests passed, including executable standard API
examples, native declaration navigation, removed-export rejection, receiver access
and concrete Array<String> method checks. Workspace check and git diff --check passed.

I02 replaces IntoIterator/into_iter with the canonical Iterable/iter protocol and
associated Iter type. Iterable is no longer resolved as a sealed source constraint.
Collection calls create independent progress; iterator calls preserve aliases and
position. Existing native-helper predicates are internal and will disappear with
the old helpers in I05. KBC/runtime ABI v65 rejects earlier artifacts.
Iteration, conversion, declaration examples and embedding tests passed; the HIR
recovery assertion now recognizes ordinary trait-bound diagnostics. git diff --check
passed.

I03 adds generic standard method contracts, FromIterator<T> and Iterator::collect<C>.
All six collection targets share construction with existing from(array) factories.
User destinations use statically specialized ordinary frames. Method bounds substitute
Self and associated outputs before checking. Native defaults appear in portable
implementation contracts without fictitious script bodies. KBC/runtime ABI v66 rejects
earlier artifacts. HIR, IR and embedding tests passed, including invalid bounds,
fresh destination access, user destinations, duplicate policies and API documentation.

I04 adds opaque Iter adapters driven by ordinary VM closure frames. Construction
does not invoke callbacks; aliases share progress. Generic callbacks and secondary
Iterable inputs retain associated item types. HIR, IR and embedding tests passed,
including all eight adapters through source, serialized artifacts and JIT fallback
with collection at every allocation. KBC/runtime ABI v67 rejects older encodings.

I05 adds terminal defaults, target-directed partition and whole-input group_by.
Old native-only iteration functions, callback implementations and operation
predicates were deleted; source examples use iterator methods. KBC/runtime ABI
v68 rejects older intrinsic encodings. HIR, IR, runtime, VM and embedding tests
passed after updating the renamed native diagnostic expectation. Tests include
short-circuit continuation, empty inputs, custom collection targets, colliding
custom keys, negative callback/bound checks and inference-error preservation.

I06 replaces native iterator snapshots with on-demand indexed reads and UTF-8 scalar
progress. Source shape is checked at construction and yielded payloads are checked
before progress commits. Guard acquisition/cleanup use explicit work lists, share
duplicate dependencies and retain roots across ordinary callback frames. Runtime,
VM and embedding tests passed, with additional acceptance for 1,500 adapter layers,
constant iterator allocation size, live slot replacement, trap/budget cleanup, and
rooted pipelines resumed through host reentry after GC between root sessions.
KBC/runtime ABI v69 rejects artifacts with earlier iteration semantics.

I07 completes English method documentation and adds collection-pipelines.kgr.
The bundled API now contains 127 executable documentation blocks. Native qualified
from_iter paths and trait-default navigation/signature queries have acceptance
coverage. Generated adapter steps retain their originating call-site identity and
debug location; removed native-only iterable facts no longer form a parallel
semantic model. Negative tests reject malformed closure iterator contracts and
invalid usize state, and explicit user overrides retain ordinary static dispatch.

Final validation: 1,206 workspace tests passed, including source/serialized-artifact
execution, JIT-enabled fallback, all standalone examples and API documentation.
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace --no-fail-fast` and `git diff --check` passed.

The native iterator type is now Iter<T>, replacing Cursor<T> without an alias.
Its declaration, type identities, ABI nodes, IR/bytecode instructions, runtime
operations and examples use the same name. Iterator and Iterable::Iter keep their
protocol meanings; an implementation can select `type Iter = Iter<i32>` and return
Self::Iter. Artifact format/runtime ABI v70 reject earlier products.

Rename validation: 1,207 workspace tests passed, including independent resolution
of the concrete Iter type and Iterable::Iter, removed-name rejection, checked
signature display, source/artifact/JIT iteration and executable API examples.
Formatting, workspace/all-targets clippy with warnings denied and git diff --check
also passed.

The native iterator now has an explicit `impl<T> Iterator for Iter<T>` declaration
in `stdlib/iter.kgr`. The build validates its native `next` binding and generates
implementation metadata, associated type mappings and member source identities.
Type checking reads the declared `Item` mapping; tooling can discover the concrete
implementation and complete its methods alongside inherited Iterator defaults.
The runtime stepping contract and artifact ABI are unchanged.

Declaration validation: 1,208 workspace tests passed, including implementation
member locations, incomplete-member completion, source/artifact/JIT iterator
execution and all executable standard-library documentation. Formatting,
workspace/all-targets clippy with warnings denied and `git diff --check` passed.

## Iterator extensions

- [x] J01: conditional adapters, inspection/fusion, lookup, reduction and comparator extrema.
- [x] J02: declaration-level where bounds, ordered extrema and key-based extrema.
- [x] J03: lazy flat_map/flatten with dynamically retained inner iterators.
- [x] J04: Sum/Product protocols and target-directed numeric/user-defined aggregation.
- [x] J05: Result/Option FromIterator lifting with short-circuiting and preserved error origins.
- [ ] J06: explicit native collection Iterable/FromIterator declarations, examples and final validation.

Fallible collection first buffers successful items, then invokes the destination's
FromIterator only after the source ends successfully. A failure returns immediately
without invoking destination construction; completed source effects remain.
Generic Try/FromResidual, try_fold/try_for_each, peekable, double-ended iteration
and exact-length protocols remain separate follow-up work.

J01 validation: HIR tests, lazy iterator source/artifact/JIT tests and all bundled
API documentation examples passed. KBC/runtime ABI v71 rejects earlier contracts.

J02 adds source-owned method where bounds and qualified associated projections.
Ordered extrema and exactly-once key evaluation pass source/artifact/JIT tests;
invalid Ord bounds are rejected. All executable API examples passed.

J03 validates lazy one-level flattening, custom Iterable outputs, empty inner
iterables, shared progress and dynamic guard cleanup with GC at every allocation.
Canonical native defaults no longer duplicate conditional method signatures in
every implementation ABI. IR verification tests and iterator route tests passed.

J04 provides target-directed Sum/Product with numeric identity values and ordinary
user implementations. Tests cover generic callers, custom targets, empty inputs,
float/double identities and usize aggregation across source/artifact/JIT routes.
IR and bytecode now represent native f64 constants for the empty product identity.

J05 validates Result/Option lifting into native and user destinations, nested
wrappers, generic destination bounds, empty inputs and unconsumed source tails.
Original Err stack metadata survives collection and GC. Iterator, error-trace
and executable documentation tests passed across source/artifact/JIT routes.
