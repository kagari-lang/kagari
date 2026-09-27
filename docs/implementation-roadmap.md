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

Each checkpoint uses a Conventional Commit. Conversion calls are explicit; reverse
protocols are derived and cannot be implemented independently. Iteration preserves
native structural-mutation guards; custom iterators own their consistency rules.
General propagation, Clone and writable indexing remain separate. Error origins
are completed in E01–E03 below.
Result propagation now also uses From for error conversion; see the propagation
checkpoint below for the current contract.

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
- [x] `ArrayList<String>.join(separator)` and `std::array::ArrayList::join`, using checked byte-length
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

The authoritative [collection contract](spec/collection-access.md) now separates
List/MutableList, Map/MutableMap and Set/MutableSet interfaces from ArrayList,
LinkedHashMap and LinkedHashSet storage. `[T]` means read-only List; literals create
ArrayList. Constructors and collection destinations name concrete storage.

The earlier C00-C04 checkpoints established access checking, host ABI access flags,
artifact validation, shallow factories and source queries (KBC v63-v64, KHI v12).
Their public paired-native-type surface is superseded by CI01 below. Host storage
access flags remain useful independently of script interface dispatch.

- [x] CI01: declare six storage-independent interfaces and native impl witnesses;
  support generic and dynamic dispatch, inherited access, native bridge validation,
  readonly views and script-defined implementations; migrate constructors,
  collection destinations, documentation and examples; advance KBC/runtime ABI v84.

Native direct calls retain intrinsic paths. Dynamic interface values retain their
underlying object and version through ordinary GC-managed interface wrappers.
Map/Set interfaces do not require Eq/Hash or define iteration order. Initial linked
hash implementations require Eq/Hash and preserve current insertion order.
Other concrete containers, variance, deep freezing and general clone remain
separate work. Existing failure/identity contracts still apply.

CI01 validation: all 1,275 workspace tests passed (1,274 ordinary tests plus the
standard documentation test executing 337 API examples). Source/artifact/JIT
fallback coverage includes native and script collection implementations, writable
views, alias identity/hash, custom keys and access rejection. Workspace formatting,
clippy with warnings denied, and `git diff --check` passed. The public SDK and
executable collection examples use the new interface/storage split.

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
group_by is a Kagari extension returning LinkedHashMap<K, ArrayList<T>>. flat_map,
flatten, sum/product and Result/Option collection lifting are covered by J01-J06 below.

I01 validation: HIR and embedding tests passed, including executable standard API
examples, native declaration navigation, removed-export rejection, receiver access
and concrete List<String> method checks. Workspace check and git diff --check passed.

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
- [x] J06: explicit native collection Iterable/FromIterator declarations, examples and final validation.

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

J06 moves native collection and fallible wrapper implementation facts into their
SDK declarations, including generic key and destination constraints. API queries
retain concrete member locations. Associated iterator bounds preserve their
originating Item equalities, and tolerant analysis retains collection shape even
when element types are erroneous. Narrow integer aggregates check the destination
range before updating the accumulator.

Final validation: 1,217 workspace tests passed, including 38 standalone examples
and 158 executable API documentation blocks. Tests cover lazy nested state across
GC and host reentry, native guard cleanup, short-circuiting, original Err stacks,
custom destinations and numeric overflow. Formatting, workspace/all-targets clippy
with warnings denied and `git diff --check` passed. KBC/runtime ABI v76 rejects
earlier products.

## Body inference and numeric literals

- [x] T01: body-local inference variables, structural unification, occurs checks and cancellation.
- [x] T02: infer local bindings and empty containers from subsequent uses.
- [x] T03: solve calls, constructors, branches and closures independently of source order.
- [x] T04: propagate constraints through iterator chains and associated types.
- [x] T05: numeric suffixes, contextual numeric inference, checked ranges and f64 fallback.
- [x] T06: local type placeholders and explicit function/method type arguments.
- [x] T07: diagnostics, source/artifact/backend acceptance, examples and final validation.

Inference is bounded by a function body; declaration signatures remain explicit.
Inference variables are distinct from unknown/error recovery facts. Evaluation
order and collection write permissions remain unchanged. Numeric suffixes cover
existing primitive types only; out-of-range literals are compile errors.

The executable [type inference example](../examples/syntax/type-inference.kgr)
combines later-use inference, phantom generic parameters, local `_` holes,
explicit function/method type arguments and checked numeric literals. This remains
body-local inference, with explicit declaration signatures and bounded solving;
it does not introduce whole-program inference or implicit numeric conversions.

Final validation: 1,232 workspace tests passed, including 39 standalone examples
and 158 executable API documentation blocks. Acceptance covers source compilation,
encoded artifacts and JIT-enabled execution with interpreter fallback. Negative
tests cover explicit type conflicts, unresolved holes, literal overflow and
malformed integer constants; tolerant analysis retains independently known types.
Formatting, workspace/all-targets clippy with warnings denied, and
`git diff --check` passed. KBC/runtime ABI v77 rejects earlier products and carries
u64/usize values without signed truncation.

## Result propagation through From

- [x] Select the canonical `F: From<E>` contract when `Result<T, E>?` returns through `Result<U, F>`.
- [x] Lower conversion to the ordinary static call path only on Err; retain original failure metadata.
- [x] Preserve identity conversion, Option behavior, generic bounds, closure inference and source-module linking.
- [x] Document direct conversion, trap cleanup and the absence of conversion-chain searches or TryFrom fallback.

`Try` and `FromResidual` remain deferred. Revisit their public protocol after Rust
stabilizes it, using the stabilized signatures and semantics as the reference.
This checkpoint does not add a custom propagation protocol, generic try_fold,
exception syntax or an Error/cause trait. The executable
[error conversion example](../examples/syntax/error-conversion.kgr) demonstrates
the supported Result behavior.

Validation: 1,236 workspace tests passed, including 40 standalone examples and
159 executable API documentation blocks. Coverage includes source/artifact/JIT
fallback, cross-module generic conversion, once-only evaluation, preserved error
origins under GC, missing bounds, rejected TryFrom/chained conversions, and trap
cleanup. Formatting, workspace/all-targets clippy with warnings denied, and
`git diff --check` passed. This uses the existing v77 execution instructions and
does not change the artifact layout.

## Numeric operations for hardware models

- [x] N01: fixed-width bitwise operations, shifts, compound assignment and const evaluation.
- [x] N02: BitAnd/BitOr/BitXor/Shl/Shr and integer Not static dispatch.
- [x] N03: wrapping, checked, overflowing and saturating integer methods.
- [x] N04: numeric casts, built-in From/TryFrom conversions and typed conversion errors.
- [x] N05: signed offset wrapping and bit rotations.
- [x] N06: 6502 examples, boundary tests, artifacts and final validation.

Ordinary integer arithmetic traps on overflow in every build mode. Explicit
numeric operations follow Rust rules, with 64-bit isize/usize in Kagari. JIT
optimization, compact buffers and fixed-length arrays are separate work.

The executable [6502 numeric example](../examples/6502-numeric.kgr) covers
address assembly, stack and zero-page wrapping, signed branch offsets, RAM
mirroring and ADC/SBC carry/overflow flags. It is a numeric acceptance sample,
not a cycle-accurate CPU implementation.

Final validation: 1,253 workspace tests passed, including 43 standalone examples
and 331 executable API documentation blocks. Numeric acceptance covers source,
encoded artifacts and JIT-enabled interpreter fallback with frequent GC. Tests
compare integer policies and floating-point cast boundaries against native Rust,
reject malformed numeric instruction contracts, preserve compound-assignment
effects on failure, and check direct native argument validation. Formatting,
workspace/all-targets clippy with warnings denied, and `git diff --check` passed.
KBC/runtime ABI v80 rejects earlier products.

## General array operations and ranges

- [x] A01: dynamic repeat arrays with once-only evaluation; see the safety revision below.
- [x] A02: atomic fill and equal-length copy_from_slice on writable arrays.
- [x] A03: independent range values, lazy integer iteration and range declarations.
- [x] A04: copy_within with validated ranges and overlap-safe shallow copying.
- [x] A05: examples, API documentation, artifacts and workspace validation.

These are general language and standard-library capabilities. Fixed-size array
types, compact numeric storage, host buffer exchange and JIT optimization remain
separate work. Range expressions stop materializing arrays; callers that need
storage collect the range explicitly. Array copies retain referenced object identity.

Validation: 1,265 workspace tests passed, including 44 standalone examples and
343 executable standard-library documentation blocks. Range and bulk-copy
acceptance covers source, encoded artifacts and JIT-enabled interpreter fallback
with frequent GC. Tests cover lazy wide ranges, inclusive integer maxima, signed
endpoints, generic RangeBounds inference, custom bound side effects, overlapping
copies, invalid bounds, resource failures, stale artifact rejection and API
navigation. Formatting, workspace/all-targets clippy with warnings denied and
`git diff --check` passed. KBC/runtime ABI v82 rejects earlier products.

Checkpoints: `2e1b50a` implements A01/A02; `719cd5a` implements A03/A04 and
synchronizes grammar, specifications and API examples. This acceptance record
closes A05. Range endpoints currently use builtin integers; borrowed slice views,
range indexing and range comparison/hash protocols are outside this phase.


## Safe array initialization

- [x] Restrict repeated elements to types without shared mutable identities, including recursive Tuple/enum checks and artifact verification.
- [x] Add source-declared ArrayList::from_fn with ordered per-index callbacks and ordinary execution cleanup.
- [x] Replace the shared-object repetition example with independent initialization and explicit sharing examples.
- [x] Complete focused and workspace validation; publish KBC/runtime ABI v83.

This supersedes A01's original allowance for repeating object references. Ordinary
shallow copying and fill retain their established identity semantics.

Validation: 1,269 workspace tests passed, including 44 standalone examples and
344 executable API documentation blocks. New coverage rejects nested and empty
shared-object repetitions, verifies value aggregates, generic initializers,
argument/callback order, zero calls, explicit sharing, callback failure side
effects, cancellation, budget exhaustion and forged repetition bytecode. Source,
artifact and JIT-enabled fallback paths preserve behavior with frequent GC.
Formatting, workspace/all-targets clippy with warnings denied and diff checks passed.

## Read-only map snapshots

- [x] Return List from LinkedHashMap keys/values/entries while preserving ordered,
  independent shallow snapshots; writable copies require ArrayList::from.
- [x] Normalize native snapshot allocation and List interface construction; encode
  KBC/runtime ABI v85 and reject unlowered snapshot bindings before execution.
- [x] Cover inferred readonly results, shallow aliases, writable copies, empty and
  generic snapshots, qualified calls and source/artifact/JIT fallback execution.

Validation: 1,278 workspace tests passed, including executable standard-library
documentation and standalone examples. Formatting, workspace/all-targets clippy
with warnings denied, and diff checks passed. Full tests used an isolated target
directory after a Windows linker file-access failure in the existing build output.

## String joining through collection interfaces

- [x] Declare string-constrained List and Iterator join operations in the bundled
  standard library; preserve the concrete ArrayList string fast path.
- [x] Lower generic joining through ordinary iteration, buffering each string once
  before final allocation. Keep native-default interface slots optional and retain
  declared method ordinals; publish KBC/runtime ABI v86 without compatibility.
- [x] Cover readonly and mutable views, key snapshots, generic and custom sources,
  partial progress, first-None termination, Unicode, empty inputs, type rejection,
  frequent GC and source/artifact/JIT fallback behavior.

Validation: 1,282 workspace tests passed across the final 1,281-test run and the
previously passing executable standard-library documentation test. The final run
skipped only that unchanged documentation test after fixing completion filtering
and the native-witness assertion. All 12 standard API query tests, formatting,
workspace/all-targets clippy with warnings denied and diff checks passed.
