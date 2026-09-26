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
and stacks are completed in E01–E03; Iterator/IntoIterator are completed in B08 below.

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
- [x] B08: Iterator/IntoIterator contracts, custom for loops and native collection integration.
- [x] B09: conformance, examples, resource cleanup and workspace validation.

Each checkpoint uses a Conventional Commit. Conversions are explicit; reverse
protocols are derived and cannot be implemented independently. Iteration preserves
native structural-mutation guards; custom iterators own their consistency rules.
General propagation, Clone and writable indexing remain separate. Error origins
are completed in E01–E03 below.

B08 uses artifact format 59 and runtime ABI v59. Custom iterators and native
cursors share protocol-based for lowering. Native cursors retain source guards,
GC roots and code versions; loop/session cleanup and structural revision checks
cover early exit and resumed cursors. See the authoritative iteration contract in
[the builtins specification](spec/builtins.md#iteration-protocols).

B09 validation: 1,132 workspace tests passed, including 6 conversion and 10
iteration integration cases. Source, artifact roundtrips and JIT fallback cover
single evaluation, qualified conversions, derived bounds, cross-module generic
implementations, native/custom iteration and associated output checking. Invalid
cursor instructions are rejected before execution. Cleanup tests cover nested
loops, return, trap, cancellation, budgets, rooted cursors across GC, foreign/stale
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
connected native for_each to ordinary script closure frames and cursor cleanup.
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
- [x] `[String].join(separator)` and `std::array::join`, using checked byte-length
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

The [collection access proposal](spec/collection-access.md) records the proposed
read-only/writable Array, Map and Set types, associated constructors and acceptance
cases. It is not implemented. Current standard declarations and value semantics
continue to describe the executable language. The proposed array spelling is
`[T]` for read-only `Array<T>`, with literals producing `MutableArray<T>`.

- [x] C00: document access boundaries, constructor shape and implementation plan.
- [x] C01a: carry collection access through HIR type identity/substitution, ABI
  types and bounded host/artifact encoding; migrate existing Rust call sites to
  explicitly request their current mutable access.
- [ ] C01: source-owned native access types and associated constructors; HIR
  assignability, generic invariance, branch joins and complete write-access checks.
- [ ] C02: verified IR, artifact encoding/version rejection, host declaration
  contracts and runtime/backend integration without duplicating collection storage.
- [ ] C03: migrate standard declarations, CLI/embedding examples and executable
  documentation; remove old constructors and update authoritative specifications.
- [ ] C04: navigation/completion and source/artifact/backend conformance, negative
  access tests, GC/alias/iteration coverage and final workspace validation.

C01 and C02 must land together if publishing C01 alone would erase access before
verification or leave an executable write bypass. Each coherent checkpoint uses
a Conventional Commit. No compatibility constructors or dual mutability model
are planned. General variance, frozen/persistent collections, deep immutability
and additional copy/capacity/from APIs remain separate work.

C01a introduces `CollectionAccess::{ReadOnly, Mutable}` as semantic type metadata.
The source language still produces mutable collections with the existing API;
`MutableArray`, `MutableMap`, `MutableSet` and associated constructors are not yet
exposed. This checkpoint does not claim enforcement of read-only access during
execution. Register/call/storage validation must preserve access before C01/C02
activate the public surface: current instruction registers retain representation
types such as `HeapObject`, rather than complete semantic types.

The encoded type shape changes artifact format/runtime ABI to v63 and host
interfaces to KHI v12. Old formats are rejected without compatibility decoding.
GC object storage and helper ABI v6 remain unchanged. Round-trip tests cover
both access modes, nested types, directional outer access weakening, invariant
nested arguments and host binding fingerprint changes.

C01a validation: 1,176 workspace tests passed, including 101 executable standard
API documentation examples. Workspace clippy with warnings denied, formatting
and `git diff --check` passed.
