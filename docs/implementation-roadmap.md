# Kagari Implementation Roadmap

## Standard library and HIR integration (active)

The [standard-library integration plan](stdlib-hir-refactor.md) defines the next
scoped migration: introduce `kagari-stdlib`, import its parsed declarations into
HIR, carry checked callable metadata into executable contracts, and dispatch
native/script implementations without downstream source-catalog interpretation.
Existing Rust implementations remain Rust. ST00–ST06 own sequencing, acceptance
and the progress ledger. The shared Native callable model distinguishes Engine
and Host providers while retaining host authority and borrow checks. Generated
host declaration documents and LSP transport remain later integrations.
ST00 baseline and inventory are complete; ST01 implementation scope is complete
and ST02 is in progress. The `kagari-stdlib` source package, installed HIR import,
ordinary standard namespaces and snapshot tool queries replace the ABI generator
and source descriptors. Shared callable/provider metadata and executable consumers
remain under migration. Source and offline host calls now share HIR signature
checking and queries, with explicit Engine/Host bindings and retained host contracts.
Optional host declaration/Rust origins are retained by analysis snapshots.
Remaining engine metadata and executable provider contracts remain pending.
Checked native type declarations now carry explicit portable representation
constructors. The coupled callable migration carries source-derived trait
contracts and per-function Required/Script/Native implementations instead of
global standard trait templates or a separate default-slot catalog. Linked trait
application checks use carried declarations across both MIR and bytecode. Runtime ABI v105, KBC v106 and KMIR v4 accompany the current wire changes;
the SDK artifact fixture requires regeneration after compilation is restored.
Integration is still broken by the carried errors recorded
in the active plan; ST01 scope completion does not claim HIR or workspace acceptance.
The thirteen-crate description below is the pre-migration
architecture. This migration precedes
further native backend expansion and does not reopen completed phase ledgers.

## Native provider and contract unification (queued)

The [native provider refactor plan](native-provider-refactor.md) follows completion
of ST06 and its final acceptance; it is not part of the active ST00-ST06 work.
NR00-NR05 replace remaining per-standard-method infrastructure policy with common
native contracts, provider registration and a shared callback/resumption lifecycle
usable by both built-in and host implementations. Adding a native function using
existing capabilities must require only its declaration, implementation, provider
registration and tests, without changes to generic HIR/compiler/verifier/VM logic.
Provider authority, storage primitives and observable execution semantics remain
explicit. NR00 re-audits the completed ST06 result before implementation begins.

## Permissions and execution protection refactor queued

The [execution policy refactor plan](execution-policy-refactor.md) follows NR05
acceptance and precedes async implementation. EP00-EP05 replace per-execution
permission matrices with installed API access and exact logical charging with
coarse runaway protection. Runtime heap/depth limits, root work/cancellation and
host-owned scheduling/service limits remain distinct. Field writeability follows
declarations and exposed adapters, not extra permission flags. This is queued
planning only; existing ST/NR semantics remain in force until its implementing phases.

## Rust value and opaque interoperability queued

The [Rust interoperability plan](rust-interop-design.md) defines ordinary typed
value conversion, an optional schema-backed Serde adapter and retained opaque
objects. Module registration owns type names; parameters, results and properties
share recursive binding rules. Opaque wrappers are cloneable without cloning their
payloads, and modifying methods use host-managed interior mutation. Rust borrow
exposure and automatic exclusive receivers remain separate future work.
RI00-RI05 are queued after native provider unification, with proposed execution
after execution-policy simplification. This synchronous binding work does not
depend on async or change its prerequisites; activation and exact scheduling remain
pending. No current ST/NR scope is expanded.

## Host API unification queued

The [host API refactor plan](host-api-refactor.md) unifies preparation, loading,
calls and reload around Engine, Program, Runtime and a stable Script installation.
Normal calls infer Rust types and use an explicit outer tuple as the argument list;
each tuple element is one script value, with no implicit spreading of tuple returns.
HA00-HA05 follow native provider, execution-policy and Rust interop acceptance.
Latest-version function entries with root-pinned execution are a proposal to confirm
at activation. The synchronous facade does not require async implementation or
change the existing async prerequisites; later integration shares its type and
version contracts. This is planning only and does not expand current migrations.

The package and update proposals below refine HA's previously open identity and
reload-compatibility gates. Freeze their contracts before HA's affected phases;
UP owns lower-level update validation/cutover and HA owns the public facade. This
is not a requirement that both whole tracks finish before either can begin.

## Package and dependency design queued

The [package design](package-design.md) proposes Cargo-style dependency declarations,
stable logical identities, exact resolved graphs and source-to-module mapping.
PK00-PK04 distinguish packages from executable Programs and runtime installations.
The first-delivery source kinds, single-selection policy and manifest defaults are
review choices. Freeze package identity before HA's package input and UP's compatibility
implementation. Activation follows the active ST/NR work; exact placement alongside
EP/RI/HA remains to be agreed without adding scope to those active predecessors.

## Compatible hot reload and state replacement queued

The [update model](update-model-design.md) separates compatible code publication
from explicit player-state export, fresh-environment restoration and host cutover.
Compatible updates preserve existing contracts and may add new concrete types with
trait implementations; adding trait impls to old types is excluded. State replacement
permits code restructuring but carries data, not old tasks or runtime object identities.
UP00-UP05 own compatibility and cutover; RI supplies conversion/root foundations,
PK supplies identity and dependency facts, and HA supplies embedding entrypoints.
Synchronous acceptance does not require async. Async integration must later cover
quiescence, cancellation and late completions using the same update boundaries.

## Async script and native execution design

The [async execution proposal](async-execution-design.md) describes typed native
operations awaited by async scripts, with host-driven single-threaded execution.
It is a design-only follow-up after native provider unification and execution-policy
simplification, not an active implementation phase. Task semantics, owned execution
lifetimes and completion
contracts require review before activation. Ordinary callback-bearing natives
remain distinct from external async waits; no async work is added to ST or NR
acceptance, and multi-threaded script execution remains out of scope.

The companion [host task scope design](host-task-scope-design.md) covers synchronous
handlers launching scope-owned async work, generic Native registration and
Actor-mailbox dispatch through a bounded drive API. It refines the async proposal's
task lifetime and scheduling contract without adding Actor or Tokio policy to core
execution or creating another active implementation track.

## Never type (complete)

Scope: introduce the uninhabited type `!` in source, checked types, executable
contracts and artifacts. A diverging expression coerces at an expression boundary
to any expected type; this does not make generic containers covariant. Calls whose
declared result is `!` have no normal continuation. Return/break/continue remain
statements. Loops retain ordinary execution budgets. `Infallible` remains a distinct
empty enum; no compatibility alias or runtime Never value is introduced.

- [x] N01: syntax, canonical types, declaration metadata and type inference.
- [x] N02: lowering, executable validation, runtime boundaries and artifact versions.
- [x] N03: source/artifact/native-fallback tests, negative contracts and final checks.

Acceptance: `panic -> !`, user and generic diverging calls, divergent branches and
loops, closures, `Result<T, !>`, empty matches, invariant generic arguments,
rejected normal returns, preserved effects/traps/budgets and rejected invalid
executable values. Run structure, format, workspace Clippy/tests and diff checks.
Checkpoint trailers: `Roadmap-Step: N01`, `N02`, `N03` as applicable.

Ledger: started from clean commit 71fdea8e. Existing control-flow completion is
syntax-based and has no call result facts; Never must extend that analysis rather
than authorize code generation through Unknown recovery types. No existing debt
or carried build errors at entry.

N01/N02: added Never as an explicit semantic/physical type and declaration result;
completion consumes checked type facts and keeps address evaluation separate from
the stored type. Coercion stays at expression boundaries. Diverging closure results
use a deferred Never fallback after other constraints; non-completing return
operands contribute no returning value. Lowering terminates Never evaluations and
callable and native collection interface adapters; both executable verifiers
reject normal Never returns.
KBC v104, runtime ABI v103 and KMIR v2 reject old products. Regenerated the existing
SDK feature fixture using its documented source/SDK recipe.

N03: six SDK integration tests cover source/artifact/JIT-fallback execution,
generic and closure inference, trait/callable/native collection interfaces,
uninhabited containers, short-circuiting, normal-return and invariant-container
rejections, effect order, trap cleanup and loop budgets. Two compiler tests cover
Never termination and forged Never-typed returns in MIR and bytecode. The existing
terminating-assignment regression and regenerated feature fixture tests pass.

Final acceptance: `cargo test --workspace` passes 1,442 tests, including standard
API documentation examples and doctests; `cargo clippy --workspace --all-targets
-- -D warnings`, `cargo fmt --all -- --check`, `uv run --locked
scripts/check_structure.py` and `git diff --check` pass. Structure review covers
516 Rust files with no violations or exceptions. Cargo checks ran serially after
an earlier overlapping rebuild caused a transient SDK doctest rlib lookup error;
the final complete run resolves it. No carried build or test errors remain.
Logs are under ignored `target/never-workspace-final.log`, `target/never-clippy.log`
and `target/never-structure-final.log`. Native backend coverage remains unchanged;
unsupported operations use the existing verified fallback.

## Completed architecture track

The [MIR and crate architecture refactor](mir-architecture-refactor.md) is complete
through A00–A05. It records the mandatory clean A00 baseline, the thirteen-crate
migration and final acceptance. The [architecture](architecture.md) and current
specifications describe the resulting implementation; the plan ledger preserves
phase decisions and validation evidence.

Final acceptance passes: 1,434 workspace tests, workspace Clippy/format checks,
514 Rust files with no structure violations or exceptions, eight production
dependency audits, four isolated SDK feature consumers, and CLI native-feature
tests. The [performance baseline](performance-baseline.md#mir-architecture-baseline-2026-09-28)
records O1 measurements and their limits. There are no carried architecture build
or test errors. Broader Cranelift coverage remains the next separately scoped native
track; LLVM is deferred until after that work.

[Foundation refactor](foundation-refactor.md) records the completed R01–R18 track.
Its semantic contracts, as amended by later specifications, continue to govern
behavior. Its `Roadmap-Step: Rxx` trailers and the
[performance baseline](performance-baseline.md) are historical evidence, not an
additional active migration queue.

The former M1–M11 milestone queue is historical and has been removed from this document. Git history retains its original scope and commits; those milestones do not prescribe current APIs, compatibility branches, artifact formats, or acceptance criteria.

After the architecture track, native backend expansion prioritizes Cranelift JIT;
LLVM is a late independent track. Complete LSP/editor integration, a full
incremental dependency database, async and cross-thread execution policy,
incremental or generational GC, complete event replay, persistent state migration
and further standard-library coverage remain separately scoped work. These tracks
reuse the existing semantic contracts.

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

Declaration style follow-up (2026-09-28): 35 standard-library function and method
declarations now omit redundant unit return annotations. Omitted returns already
resolve to unit; callback function types retain their required explicit returns.
The declaration specification records this convention. All three tests in
`cargo test -p kagari-embed --test standard_declarations` pass, including the
source/artifact documentation examples. Structure, formatting and diff checks pass.

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
- [x] A02: atomic fill and equal-length copy_from on writable arrays.
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

## Standard library completion

This sequence extends the source-declared API without borrowed views, ownership
transfer APIs or compatibility aliases. Each checkpoint updates declarations,
runtime/lowering contracts, documentation and executable acceptance cases.

- [x] S01: Unicode trimming, substring search and prefix/suffix stripping.
- [x] S02: Lazy string splitting, bounded splitting, lines and whitespace.
- [x] S03: Replacement, repetition, case conversion and byte/boundary iteration.
- [x] S04: Lazy Option/Result combinators, flattening and transposition.
- [x] S05: FromStr, typed parsing and integer radix parsing.
- [x] C01: Map snapshot interface methods and copy_from naming.
- [x] C02: List endpoint, membership, prefix/suffix and binary search queries.
- [x] C03: List reordering, truncation, prepared extension and swap removal.
- [x] C04: Concrete collection capacity construction and reservation.
- [x] C05: Set relationships and symmetric difference over readonly interfaces.
- [x] C06: Guarded Map get_or_insert_with and update operations.
- [x] C07: Prepared retain, stable sorting and adjacent deduplication.
- [x] C08: Lazy snapshot windows/chunks and immediate range removal.

Callback mutations prepare changes before committing; callback failure preserves
the target's slots/order, while previously completed object side effects remain.
Callbacks cannot modify the target container through aliases. Lazy windows/chunks
produce independent readonly shallow snapshots at yield time. Strings use UTF-8
byte offsets and Unicode scalar iteration, without implicit normalization.

Compact buffers, live sublist views, double-ended iteration, StringBuilder and
generalized Try/FromResidual remain separate follow-up work.

S01 validation: source, serialized-artifact and JIT-fallback acceptance passed
with collection threshold one; definition navigation resolves the documented SDK
member. KBC/runtime ABI v87 rejects previous formats.

S02 validation: lazy split/line acceptance passed on source, encoded artifacts and
JIT fallback with frequent collection. Constructor-forgery checks reject invalid
argument shapes, modes and missing operands. Targeted clippy and diff checks pass;
KBC/runtime ABI is v88.

S03 validation: Unicode expansions/contextual casing, empty and bounded
replacement, repetition, byte and scalar-index iteration pass across all three
execution paths with frequent GC. Artifact/runtime ABI is v89.

S04 validation: 3 combination tests and 16 error-trace tests pass, including
generic payloads, object aliases, lazy branch selection, callback trap cleanup
and original error preservation through nested flatten/transpose. KBC/runtime ABI
is v90; targeted clippy and diff checks pass.

S05 validation: native and custom FromStr, explicit/contextual/generic inference,
all integer widths at their limits, overflow, radix, signs and floating/boolean
syntax pass with source/artifact/JIT fallback. Workspace clippy passes. SDK
implementations and ParseError are source-declared. KBC/runtime ABI is v91.

C01 validation: 24 collection/array tests pass, including native and custom
readonly Map snapshots with non-hashable keys, object aliases, generic calls and
rejection of writes and the removed copy name. Native protocol identities no
longer recursively initialize the contract catalog when one protocol returns
another protocol. KBC/runtime ABI is v92.

C02 validation: native/read-only/custom List queries, generic PartialEq, custom
ordering, insertion positions, empty inputs and Ord rejection pass across source,
artifact and JIT fallback with frequent GC. Artifact/runtime ABI is v93.

C03 validation: 25 array/collection tests pass, including interface reordering,
truncation, prepared self-extension and unordered removal. Failure tests preserve
slots after invalid swap or mutation during iteration. Extend's readonly List
input is snapshotted before the storage commit. KBC/runtime ABI is v94.

C04 validation: 12 capacity/array tests pass, covering all concrete storage
classes, reservation during iteration, order preservation and overflow failure
without modification. Workspace clippy passes. Live heap units still measure
stored values rather than allocator capacity; preparation uses allocation limits.
KBC/runtime ABI is v95.

C05 validation: readonly/native/custom Set operands, generic unbounded relations,
empty/self operations and insertion ordering pass through source, serialized
artifacts and JIT fallback with collection threshold one. Existing collection
interface and standard trait tests pass. KBC/runtime ABI is v96.

C06 validation: lazy Map insertion and updates work through concrete storage and
MutableMap, including custom colliding keys, scalar results and shared objects.
Callback traps and alias writes preserve entries, keep completed external effects,
and release frame guards and roots. Source/artifact/JIT fallback tests pass.

C07 design: native ArrayList sorting uses stable bottom-up merging in explicit
frames, with once-per-element key extraction. Native retain records decisions
before replacing storage, preserving stored hash tokens. These operations belong
to concrete storage: a general user MutableList/Map/Set cannot promise an atomic
bulk replacement using only its individual write methods.

C07 validation: stable order, once-only key callbacks, merge-run boundaries,
empty inputs, live readonly aliases, custom map/set keys and adjacent dedup pass
through source/artifact/JIT fallback. Callback traps and alias mutation preserve
original array slots. Existing array and Map update tests also pass.

C08 validation: lazy snapshot timing, independent slots/shared objects, custom
List indexed traversal, zero/oversized windows, short chunks, fused exhaustion and
early-close guard release pass through source/artifact/JIT fallback. Range removal
covers readonly results, live aliases, empty/full/prefix/inclusive ranges and
failure without target modification. This checkpoint used KBC/runtime ABI v99.
The final iterator-resumption audit advances the format and runtime ABI to v100.

Final completion audit: all S01-S05 and C01-C08 checkpoints are implemented and
committed independently. Window/chunk resumption validates source revisions and
restores guards after early closure. Native API query tests now recognize FromStr
implementations and Set algebra's interface-owned declarations. Current KBC and
runtime ABI versions were v100 at that checkpoint; subsequent entries record
further version changes. Old artifacts are rejected without compatibility.

Validation covers 1,305 passing tests, including Rust doctests and the executable
SDK documentation test. The initial full workspace run passed every SDK example
(source and encoded artifact execution) before exposing two outdated HIR metadata
assertions. After updating only those assertions, the final workspace run passed
1,304 tests and skipped that one already-passing, unchanged documentation test.
Formatting, workspace/all-targets clippy with warnings denied, and git diff checks
also passed. Logs are under target/stdlib-completion-workspace.log,
target/stdlib-completion-final.log and target/stdlib-completion-clippy.log.

### Unified callable protocol

- [x] Source-owned `Fn<Args, Output = R>`, callable-bound shorthand, contextual
  closure inference, and user callable objects.
- [x] Interoperability with `fn(...) -> R` callback parameters through ordinary
  GC-managed closure adapters; source/artifact and interpreter/JIT-fallback tests.
- Deliberately excluded: ownership-based `FnMut`/`FnOnce` distinctions and new
  optimized calling conventions.

Artifacts and runtime ABI advance to v101. Function-typed values automatically
implement `Fn`; user objects are adapted to the existing closure representation.
Validation passes all 1,315 workspace tests with no ignored or filtered tests,
including executable SDK documentation, source/artifact execution, JIT fallback,
GC capture lifetime, trap cleanup, evaluation order, inferred associated outputs,
and analysis-cache reuse. Formatting, workspace/all-targets clippy with warnings
denied, and git diff checks also pass. Logs: target/fn-workspace-final.log and
target/fn-clippy-final.log.
