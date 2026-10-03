# Kagari Architecture

This document defines the production architecture for Kagari.
It describes the intended system shape that implementation work must converge on.
When existing code conflicts with the specifications, the specifications are authoritative.

The [foundation API completion plan](foundation-api-completion.md) records the
always-present common APIs, collection default trait methods and built-in String
inherent methods. Generic interface methods pass checked type/constraint arguments
to shared script or native entries; static calls retain specialization. In-place
collection operations preserve completed mutations on failure without rollback-only
buffering. Foundation assembly is mandatory, and ArrayList overrides use the direct
scoped sequence-edit path described below.

The completed [native collections reset plan](native-provider-refactor.md) records
the baseline implementation and acceptance ledger. Phases 1-3 replaced the
predecessor library and native ABI; phase 4 verified the bounded optional library
and consumers.
The [MIR and crate architecture refactor](mir-architecture-refactor.md) records the
preceding foundation checkpoints. Language behavior follows the specifications.

## Language contracts and native implementations

The compiler owns language protocols, including operators, equality/hash/ordering,
indexing, iteration, callable and formatting contracts, and the complete
List/MutableList, Map/MutableMap and Set/MutableSet surfaces. ArrayList, HashMap and
HashSet are canonical defaults with always-present Rust runtime implementations.
[T] denotes List; array literals create ArrayList. Concrete hash types require
Eq + Hash and use Rust std::collections::HashMap/HashSet without an ordering
promise. Foundation behavior is independent of optional library installation.

Application and library modules use the same explicit ModuleBuilder API. Kagari
types, functions and scoped implement/trait_impl blocks define their own
signatures and generic parameters. Ordinary bind checks Rust scalar and borrowed
view converters against those declarations; bind_with supplies explicit codecs.
Rust function signatures do not define Kagari traits or infer exported contracts.
There is no native declaration macro crate or separate standard-library crate.

ModuleBuilder::finish produces a validated NativeModule. Installation checks its
binding/storage closure atomically. Runtime construction installs the foundation
and bundled collection algorithms unconditionally, including source-free execution.
Fixed foundation/collection modules reuse checked immutable registrations within
the owning host thread. Native modules share declaration catalogs, binding records
and storage descriptors; each runtime still performs atomic installation and owns
its own heap, host registry, resources and generations. Authoring computes common
module dependency closures once and extends them with exact per-binding facts.
Engine construction supplies their declarations to source analysis and installs
only explicit application modules afterward. There is no foundation opt-out.
List/MutableList declare sorting, reversal and filtering as native default methods;
ArrayList supplies compact storage overrides. The bundled collection module keeps
lazy map as an ordinary native function. String inherent signatures also belong to
the language catalog, with ordinary Rust bodies for UTF-8 byte queries, checked
slicing, Unicode trimming, literal replacement and eager splitting. Split returns
a List through the same checked concrete-result adapter. Additional
containers and algorithms remain application-installable future modules.

HIR consumes native ModuleDecl records directly for static checking, generic
bounds, ordinary trait selection and tooling. Generated .kgr files provide
signatures, documentation and declaration-to-span navigation; they are not parsed
to recover executable semantics. Portable MIR/bytecode retain native imports,
concrete signatures, declaration contracts and selected callable witnesses. ABI
verification has no syntax or HIR dependency. Loading compares those records with
the installed declarations before preparing entries and selected targets.

A native body can declare `produces(concrete_type)` while its exported signature
returns an interface. Registration checks the concrete result's trait conformance
and the Rust result codec. Compilation selects the exact interface implementation
and arguments, retaining them on the native import. Portable validation rechecks
that proof and its executable table. Invocation validates the concrete value and
boxes it through that table, preserving generic argument scopes and generation
ownership. It does not search for a trait implementation at runtime. The same
path serves application functions, native defaults and foundation methods.

Implicit language protocols materialize ordinary checked executable adapters.
Their origin, exact receiver types and signatures remain explicit portable facts;
explicit script implementations take precedence. Runtime consumes checked targets
rather than resolving traits or recognizing library algorithm names. Native and
script interface slots retain their implementation module and generation. Dynamic
associated iterator results use checked interface views and result boxing; erasure
does not change the concrete implementation's signature.

Interface GC objects share immutable receiver descriptors. A rooted method handle
selects a verified ordinal or a shared bound operation without copying the method
table. Closed method signatures and receiver-operation groups are prepared lazily
and reused within that retained view; method-local type arguments and caller
witnesses remain call-specific. Type environments share immutable binder slices
and parent scopes, keeping nominal origins distinct across reloads. Indexed
receiver groups own operation descriptors; descriptors refer back through Weak
handles, and active environments retain the needed group without ownership cycles.
Inherited dispatch selects a cached parent descriptor while rooting the original
value. An escaping upcast still publishes a normal GC interface value and validates
its receiver, including host ownership/schema. Rc metadata ownership does not
replace GC tracing, call roots or executable-generation retention.

## Synchronous calls and registered storage

NativeEntry returns NativeResult<Value> synchronously. CallContext borrows the
existing argument/frame roots. Typed scalar arguments use stack packs; sequence
views expose scoped contiguous buffers. Prepared selected calls and supplied
CallableHandle arguments invoke script bodies synchronously on the existing
execution stack. Ordinary callbacks require no continuation protocol or scratch
slots. Trap, reentry and result validation use the same runtime boundary as script
calls. Host-owned state still uses its declared schemas and scoped borrow checks.

StoredCallable retains a checked closure and its defining generation. When stored
inside a GC payload, its captures are trace edges; independently retained host
callbacks use explicit RootedCallable ownership. Generation checks prevent stale
handles and reload preserves pinned callback implementations. Native payloads
retain layout ownership without unnecessarily pinning an entire execution program.

Generic NativeStorage registration attaches a Rust payload to a declared native
type. NativePayload supplies tracing, logical size and normal Rust destruction;
scoped access validates its registered Rust type and cannot span script reentry or
collection. New payload types require no concrete Value/HeapObject variants or
compiler, verifier or VM branches. NativeObject is the common nominal ABI form.

SequenceStorage selects a compact Vec of the declared scalar type even when empty;
GC-bearing elements use Vec<Value>. One shared scalar table generates the storage
and converter cases. SequenceHandle and SequenceMutHandle allow borrowed slices
without per-element dynamic conversion. Default hash storage caches checked hashes
and stable key tokens; script Eq/Hash calls run outside table borrows.

sort uses Rust's stable slice sort. Infallible primitive ordering sorts a compact
buffer directly. Script ordering and supplied comparators use a SequenceEdit
lease of the actual buffer, with no storage clone or index permutation. An explicit
root snapshot protects reference-bearing values while Rust uses sorting scratch
space; scalar buffers need no such snapshot. Receiver slots are exclusively
borrowed during the lease, which restores completed edits on every exit path.
Failure preserves original elements but may change their order; completed effects
on referenced payloads remain. A first comparator error suppresses further user
comparisons. No sorting state machine or Kagari algorithm implementation is used.

The library-owned MapIterator payload stores a NativeCursor and StoredCallable.
Aliases share progress, and each next invokes its mapper synchronously. Cursor
consumption precedes the callback; failure retains that consumption and completed
side effects. The native source read allocates no intermediate script Option.
NativePayload::iteration_sources declares wrapped iteration resources and traces
them as GC edges. A for scope follows those edges once, retaining guards until
normal exit, break, return or failure; generic execution never names MapIterator.
Recursive next on the same adapter is rejected. Only lazy state persists between
calls; genuine asynchronous suspension remains separate future work.

## Foundation Contracts

The completed foundation track is [foundation-refactor.md](foundation-refactor.md).
[Value semantics](spec/value-semantics.md), [failure semantics](spec/failure-semantics.md),
and [module activation](spec/module-activation.md) define observable behavior and
supersede conflicting historical descriptions. Unchecked work is not implemented.

## Architectural Principles

- Kagari is a statically typed, GC-backed scripting language for Rust-hosted applications.
- The source language is Rust-inspired in syntax and Kotlin-like in value ergonomics.
- Script authors do not work with Rust lifetimes, Rust borrowing, or script-level `dyn Trait`.
- Hot reload is a core runtime property, not a later patch over module loading.
- Host-owned state and Kagari-owned state remain explicit and separately controlled.
- Verified MIR is the common execution contract; bytecode is its interpreter target.
- Cranelift JIT is an optional backend layer and never the definition of language semantics.

## Specification Authority

The implementation must be driven by the documents under `docs/spec/`.
The current Rust code is an implementation snapshot and may contain legacy behavior that no longer matches the specifications.

Examples of implementation behavior that must not be preserved for compatibility when it conflicts with spec:

- `let` / `let mut` as source binding syntax
- script-visible `static` or `static mut` module storage
- script-visible Rust-style `dyn Trait`
- long-lived host mutable references represented as script values
- ordinary field mutation lowered through reflection helpers
- runtime string field lookup in hot field-access paths

## Workspace Shape

The repository is a Rust workspace with structural separation between language phases:

```text
crates/
  kagari-common             source identities, diagnostics, limits and shared primitives
  kagari-syntax             lexer, parser, concrete syntax tree and AST views
  kagari-hir                recoverable analysis, resolution, typing and tool queries
  kagari-abi                executable types/layouts, helper ABI and native contracts
  kagari-mir                concrete CFGs, verification, analyses, passes and portable codec
  kagari-compiler           source monomorphization, MIR/bytecode lowering and native links
  kagari-bytecode           interpreter model, validation, codec and artifact envelope
  kagari-codegen            compilation-only verified MIR interface and diagnostics
  kagari-codegen-cranelift  MIR-to-CLIF emission and executable code ownership
  kagari-runtime            values, GC, host state, authority, sessions, native calls and reload
  kagari-vm                 interpreter/frame driver, debugger and prepared native selection
  kagari-embed              host SDK, features, preparation/cache and execution orchestration
  kagari-cli                arguments, filesystem IO and presentation
```

Runtime, bytecode and VM have no production dependency on MIR, source analysis or
codegen. MIR depends on ABI/common rather than HIR. Compiler core works without its
`source` feature. Native backends depend on codegen/MIR/ABI and their backend libraries,
not on runtime, bytecode, compiler or SDK. Source-based tests may use dev-dependencies;
they do not define the production graph. ABI has no source generator or syntax
build dependency. HIR reads compiler-owned language contracts and installed
native declaration records from ABI types. Runtime declaration construction does
not consume Kagari source or call its parser/compiler. The feature audit checks
ABI's build graph as well as normal production dependencies. LLVM is deferred;
no placeholder crate exists.

## Contract and common responsibility cleanup

The approved replacement name for `kagari-abi` is **`kagari-contract`**. This is
a queued ownership cleanup covering both `kagari-abi` and `kagari-common`, not
just a crate rename. The workspace list above describes the current code; this
section records the target direction. Implementation has not started.

The current ABI crate combines portable executable records and layouts, generic
substitution and proof checking, language foundation catalogs, declaration
construction and `.kgr` rendering. Common also combines source management and
diagnostics, declaration identities, numeric semantics and host interface records.
Being used by several crates is not sufficient reason to put a feature in either
shared crate. The cleanup must assign each responsibility a clear owner without
turning `kagari-contract` into the same collection under a new name.

### Names

Use domain names within meaningful modules instead of an `Abi` prefix or suffix:

| Current name | Target name |
| --- | --- |
| `AbiType` | `Type` |
| `NominalAbiType` | `NominalType` |
| `FunctionAbi` | `FunctionDecl` |
| `TypeAbi` | `TypeDecl` |
| `TraitAbi` | `TraitDecl` |
| `ParameterAbi` | `Parameter` |
| `GenericBoundAbi` | `GenericBound` |
| `InterfaceTableAbi` | `InterfaceTable` |

Keep the existing `ModuleDecl` / `ImplDecl` vocabulary. Do not replace every `Abi`
suffix with `Contract`. Resolve collisions through meaningful module imports,
such as `use kagari_contract::types as contract;` and `contract::Type`. The runtime's
existing native `FunctionDecl` authoring model must be reviewed alongside the
portable declaration model: decide their distinct responsibilities or consolidate
them before renaming. Do not preserve duplicate models through compatibility
aliases or forwarding re-exports.

### Ownership boundaries

These are responsibility boundaries, not a requirement to create one crate per row:

| Responsibility | Target ownership and constraints |
| --- | --- |
| Portable declarations, executable types, layouts, native imports, helper calling conventions and encoded contract validation | `kagari-contract`; available to compiler, artifact validation, runtime and backends without source analysis. |
| Foundation trait identities, canonical declarations and intrinsic language rules | An explicit language foundation owner, shared where source-free validation needs these facts; compiler-owned language behavior must not become an optional library. Separate catalog construction from generic record handling. |
| Substitution, matching and implementation proof checking | A coherent source-independent verification owner. Preserve artifact validation; moving these checks into HIR is not an acceptable way to shrink the shared layer. Separate generic proof machinery from closed language rules. |
| Native declaration authoring, Rust handlers and storage bindings | Native registration ownership; distinguish portable descriptions from runtime implementation state. |
| `.kgr` rendering, documentation and navigation spans | Tooling ownership consuming structured declarations; executable consumers must not need source generation or parsing. |

Audit common at the same time, using its actual consumers rather than moving it
wholesale into contract:

| Current common area | Required review |
| --- | --- |
| `source`, `source_database`, `line_index`, `diagnostic`, `literal` | Group source and tooling responsibilities; separate literal parsing from numeric execution semantics. |
| `identity`, `span` | Separate portable package/module/definition identity from source file revisions and locations where appropriate. Preserve debug metadata consumers; these modules are not uniformly source-only. |
| `arithmetic`, `integer`, `numeric`, `range`, `collection` | Give shared language semantics a deliberate owner; preserve one implementation of compile-time/runtime numeric behavior. Distinguish semantic tags from execution algorithms. |
| `host_interface` | Locate portable host schema and effect records with their contract owner; keep execution state and host state with runtime/embedding. The capability module was removed by the execution-policy simplification. |
| `cancellation`, decode-limit helpers | Retain or relocate small shared mechanisms according to concrete dependency needs; preserve cancellation and bounded decoding. |

Whether a small `common` crate remains, is renamed or disappears follows this
consumer/ownership audit. No replacement utility crate or final crate count is
approved merely by this naming decision. In particular, ABI currently depends on
common: moving declarations must account for that graph rather than introduce a
cycle, a forwarding crate or a second public path to the same model.

### Scope and completion boundary

The eventual cleanup delivers an explicit module/dependency map, the crate and
type renames, and migration of affected imports, consumers, documentation and
checks. Keep existing behavior, source-free validation and backend dependency
constraints. Replace unpublished internal interfaces directly; no routine version
bump, old-format reader or compatibility layer is required. This work does not
add traits, containers, library algorithms or new execution-policy obligations.

A future external C embedding API belongs in a proposed **`kagari-ffi`** adapter
over `kagari-embed`: C exports, opaque handles, buffer ownership, error/panic
translation and callback adapters into the existing native registration path.
Internal JIT/runtime calling conventions remain executable contracts. Creating
the FFI crate or committing to a stable external ABI is outside this cleanup;
do not add a placeholder crate now.

The [roadmap](implementation-roadmap.md#contract-and-common-responsibility-cleanup-queued)
tracks this queued work separately from the completed native collection reset.

## Compilation Pipeline

```text
source database / snapshots
  -> syntax and recoverable HIR analysis
  -> checked program
  -> compiler source lowering and bounded reachable monomorphization
  -> verified concrete MIR with sealed analyses
       -> compiler core bytecode emission -> verified bytecode
       -> codegen + explicit helper links -> owned native product

.kbc envelope (bytecode + optional portable MIR)
  -> SDK PreparedProgram (validation and, with native, canonical MIR correspondence)
  -> runtime-specific linking and immutable module versions
  -> interpreter, or explicit native preparation/install/execute
```

HIR owns source meaning and incomplete-source queries. MIR owns concrete typed
operations, explicit CFG/effects/origins and checked execution facts. Compiler owns
lowering, not script execution. Backends consume the verified handoff without
recovering semantics from bytecode or re-resolving syntax. Runtime owns shared
execution services; the VM supplies the interpreter frame driver.

A `VerifiedProgram` shares immutable bytecode and validation across runtime instances.
Host bindings, heap state, authority and installed native handles remain local to
each runtime. Verification seals are not serialized: decoded inputs are bounded,
validated and sealed again. See [artifacts](spec/artifacts.md) and
[module loading](spec/module-loading.md).

## Source Language Layer

The [syntax architecture](architecture/syntax.md) defines the crate's input and
output contracts, parsing flow, ownership model and current limitations.

The syntax layer implements the grammar in `docs/spec/syntax.md` and `docs/kagari.ebnf`.

Core source-language facts:

- local bindings use `val` and `var`
- fields use `val field: T` and `var field: T`
- function parameters are ordinary non-rebindable bindings
- method receivers use `self`
- unit is written as `()`
- `const` is a compile-time value item
- script-visible `static` module storage is not part of the language surface
- trait names are interface value types directly; there is no script-level `dyn Trait`

The syntax layer must not encode semantic shortcuts that only exist because of the current implementation.

## Builtins and Standard Modules

Kagari has a typed standard surface defined in `docs/spec/builtins.md`.

The language foundation owns primitive/value types, Option/Result, syntax-required
range forms and the complete operator, collection, iteration and callable traits.
It owns [T] typing, existing collection literal semantics and the canonical
ArrayList/HashMap/HashSet declarations. Native/script impls use
ordinary checked trait records; the compiler does not select storage algorithms
from interface names.

Runtime supplies basic construction, access, mutation and traversal for all
three default containers through ordinary checked native bindings. Default hash
storage uses Rust std::collections::HashMap/HashSet, never indexmap. It owns
GC-managed object identity, generic registered native storage and scoped access.
Contiguous primitive buffers are one layout capability. Other
native storage supplies checked factories, tracing/destruction and access entries
without adding a concrete type to the compiler/ABI/VM's global catalog.

The foundation includes all 38 predecessor traits, including Into, TryFrom,
TryInto, FromStr, FromIterator, Sum and Product. Core owns their declarations and
type contracts;
ordinary trait checking and native binding handle their implementations. Merely
being default-available does not require special compiler dispatch. Try and
FromResidual are outside this correction; no new propagation protocol is added.

Optional native modules own algorithms and additional collection implementations
such as LinkedList, TreeMap/TreeSet and LinkedHashMap/LinkedHashSet. Disabling
these modules retains the three defaults and language contracts.
Native registrations explicitly declare their own types, functions and impls;
compiler-owned contracts are referenced rather than redeclared. Generated tooling
views project the same checked declarations. The [active plan](native-provider-refactor.md)
owns migration order and the bounded algorithm/extension proof. There is no second
copy of collection algorithms in Kagari source.

Foundational Map/Set traits do not prescribe traversal order. `indexmap` belongs
to future optional LinkedHashMap/LinkedHashSet implementations; it is not backing
for the default HashMap/HashSet.
Hash-key eligibility and custom equality/hash protocols follow the checked contracts in [builtins](spec/builtins.md). Runtime callbacks execute on the same explicit frame stack with ordinary resource and reentry rules.

Standard library calls flow through one structural path:

```text
source call or method
  -> ordinary checked HIR declaration and selected implementation
  -> provider-qualified native import or ordinary script callable
  -> portable declaration/signature/layout/bound/witness validation
  -> shared runtime frame/session driver and selected Rust implementation
```

This keeps ordinary standard library execution out of script-visible reflection and host string dispatch.
Reflection metadata may describe standard values for tooling through declared metadata, but reflection is not the implementation mechanism for arrays, maps, sets, strings, or standard helpers.
Reload validation and JIT preparation use the same verified callable contracts as the interpreter. Unsupported native compilation falls back before entry.

Host-sensitive APIs such as file system, networking, timers, persistence, service registries, and logging sinks are host APIs.
They are not exposed as unrestricted core standard modules.

## HIR, Resolution, and Type System

HIR is the first semantic representation.

Normal-completion analysis is independent of produced value types. Its evaluator
uses an explicit work stack over expressions, blocks, statements and places;
lazy child traversal stops after termination and after an irrefutable match arm.
Loops consume their own break exits. Each work step checks cancellation, which
returns cancellation rather than a fabricated completion fact. Completed node
facts are memoized within one traversal using the full owned HIR identity;
shared subtrees are evaluated once, and cached break exits are consumed only at
the enclosing loop boundary. The cache never crosses query or snapshot boundaries.
This removes
native-stack recursion from this analysis; it does not imply that all frontend
traversals or resource limits are complete.
It should erase parser trivia and expose stable semantic nodes for later passes.

HIR and semantic analysis own:

- module item collection and visibility
- local, parameter, field, function, const, trait, impl, and module namespaces
- `val` / `var` writeability rules
- field writeability and assignment validation
- function signatures and `()` return behavior
- generic parameter and trait-bound checking
- interface value compatibility
- concrete type identity for `is<T>` and `downcast<T>`
- compile-time metadata and generated registration data

Type checking must reject invalid programs before MIR lowering whenever the violation is statically knowable.
Runtime checks remain required for host state, declared access, dynamic indexes, and hot reload epochs.

## MIR, ABI and Bytecode

MIR is a concrete typed, non-SSA control-flow representation. Source generics and
trait obligations are resolved by compiler source lowering into reachable executable
instances; MIR contains no HIR type arena or generic binder. Verification seals
function/program links and bounded analyses: initialization, liveness, effects,
logical roots, safepoints, source/debug origins and cooperative cancellation points. Public
passes consume verified input, make bounded changes and reverify the result.

The ABI crate owns nominal executable types, signatures, layouts, provider
contracts, language primitives, helper/native representations and version constants.
Structured native declaration records have no source/handler dependency. HIR carries
installed declaration facts; MIR carries concrete native imports and bytecode
deduplicates them. Runtime links against trusted registrations and drives erased
state with explicit roots and checked callbacks, without standard-method selection. Compiler
core lowers verified MIR into the register/local bytecode contract. Bytecode validates
its own instructions, metadata, dependency graph and canonical artifact envelope.
Native-enabled preparation also proves correspondence of optional portable MIR to
that same bytecode program; executable contracts never depend on source analysis.

Ordinary aggregate access uses checked nominal field slots. Host-backed typed path
access is a separate operation with installed contracts and scoped borrow rules.
Reflection remains explicit rather than implementing ordinary field mutation.

## Runtime Model

The runtime owns execution state and services shared by the interpreter and JIT.

Core runtime subsystems:

- value representation
- GC heap for Kagari-owned values
- explicit roots for host-retained Kagari values
- module store with epochs
- shared immutable verified programs and runtime-local interpreter cache records
- installed native handles retaining code owners and exact dependency versions
- type and interface metadata registry
- host registry
- root cancellation and execution phase
- resource accounting
- hot reload coordinator

The GC manages Kagari script data.
It does not own Rust host objects, Rust references, or the Rust object graph.

## Host Interop and Typed Path Mutation

Host interop exposes Rust functionality through explicit registration.

The host boundary has two separate mechanisms:

- frame-scoped host borrow tokens for temporary host calls using `&T` or `&mut T`
- typed path mutation for ergonomic field/index access to host-owned domain state

Typed path mutation represents a checked path rooted at a host object.
It carries typed metadata, dynamic index operands, access policy, dirty tracking hooks, and reload validation data.
It must not store Rust `&mut` references in script values.

## Embedding API

The Rust embedding surface is defined in `docs/spec/embedding-api.md`.

The embedding API owns:

- compile, load, execute, and reload entry points
- host registry setup
- module loader configuration
- execution context construction
- runtime call-depth limits and cooperative cancellation
- structured diagnostics and runtime errors
- interpreter/JIT execution policy

Embedding APIs expose stable Kagari concepts rather than parser or backend internals.
Convenience CLI behavior must remain a thin layer over the same embedding pipeline.

The SDK exposes `KagariEngine`, `KagariRuntime`, `PreparedProgram`, load/reload
options and `ExecutionContext`. The default `source,native` feature set enables source
compilation and native preparation. No features gives artifact-only interpretation;
`source` adds HIR/syntax/compiler source, and `native` adds frontend-free MIR/compiler
core/codegen. The host supplies any concrete backend. Source-only builds can emit
portable MIR for native-only consumers.

Hosts compile or decode an artifact, construct a reusable `PreparedProgram`, then
load it into each runtime. `execute` interprets; `prepare_native` uses a trusted
compilation-only backend and a bounded configuration/version cache, then installs
runtime-specific handles. `execute_prepared` executes that decision. Reload consumes
a prepared candidate and preserves its verified identity. Preparation precedes
script execution and has its own structural work limits.

## Interpreter

The interpreter is the semantic execution foundation.
It executes verified bytecode against the runtime model.

The interpreter must:

- preserve bytecode-visible control flow and value behavior
- enforce traps and runtime errors consistently
- call runtime helpers at allocation, host, reflection and path boundaries
- maintain correct stack/root metadata for GC
- respect pinned module versions across hot reload
- reject unsupported or unverified bytecode instead of guessing behavior

## Debugger and Tooling

Debugger support is defined in `docs/spec/debugger.md`.

The debugger is a host/tooling capability, not an ordinary script API.
It is built on:

- bytecode source maps
- safe debug points
- runtime frame inspection
- value inspection
- declared reflection metadata
- host exposure policy
- typed path read policy
- module epoch identity

The baseline debugger is interpreter-first and supports source breakpoints, conditional breakpoints, hit counts, stepping, call stacks, variable inspection, watch expressions, trap breakpoints, and hot-reload-aware breakpoint remapping.
The current native subset has no observer callbacks; attached execution observers force pre-entry interpreter fallback. Metadata flags alone cannot provide debugger semantics.

The implemented VM exposes a debugger adapter boundary through `DebugProtocolAdapter`, `DebugAdapterRequest`, `DebugAdapterResponse`, `DebugAdapterEvent`, and `DebugAdapterEventSink`.
IDE and DAP integrations should translate their transport messages at that boundary instead of coupling directly to VM internals.

## Baseline Cranelift JIT

The baseline JIT is optional and function-level.
It compiles verified MIR and explicit ABI/link descriptions through Cranelift.

The JIT must:

- preserve interpreter semantics
- share runtime helper ABI boundaries
- emit safepoint and stack-map metadata
- respect host interop and typed path mutation checks
- bind installed code to exact immutable versions and reject mismatched entries
- avoid mandatory deoptimization, tracing behavior, and optimizing-tier complexity

`docs/spec/jit.md` defines the JIT contract.
`kagari-codegen-cranelift` implements the unsafe compilation-only `CodegenBackend`
contract. Runtime owns installation/invocation; VM only consumes prepared entries.
The supported subset is zero-argument, straight-line Unit/Bool/i32 constants, moves,
checked arithmetic, supported comparisons and return. Locals, control flow, calls,
GC-bearing values and other numeric operations use pre-entry fallback. Compiler
errors and failures after native entry never silently restart the interpreter.
Each product owns its pages independently of backend lifetime and later compilations.

## Hot Reload

Hot reload is built around module epochs and validated publication.

Reload must:

- compile and validate a new module before publishing it
- preserve the current active module when validation fails
- compare public ABI fingerprints, type metadata, interface tables, and typed path descriptors
- keep old code and metadata reachable while old values or calls need them
- ensure new calls use the latest successfully published epoch
- avoid implicit migration of script-visible module storage

Script-visible durable module storage is deferred until the reload model defines explicit versioning and migration rules.

## Security and Reflection

Kagari primarily embeds trusted scripts. Installation determines available native
and host APIs. Member visibility, readonly views and host adapters enforce declared
language/interface contracts. Reflection uses these metadata and access contracts.
There is no language-profile or generic runtime permission matrix.

Execution retains cooperative cancellation and a runtime call-depth limit, without
instruction charging or generic CPU/memory quotas. Hosts own service admission and
deadlines. GC, borrow, generation, artifact and candidate-publication validation
remain mandatory. See [execution control](spec/security.md).

## Production Readiness Definition

Kagari is production-ready when:

- syntax, semantics, runtime, and bytecode match the specifications
- module loading, artifact validation, and embedding APIs are stable
- the complete standard library surface is implemented and tested
- incompatible legacy language forms have been removed
- conformance tests cover accepted and rejected source programs
- interpreter behavior is deterministic and verified through integration tests
- host interop enforces no-escape and aliasing rules
- typed path mutation is validated, efficient, and reload-aware
- debugger sessions support IDEA-like source debugging through safe runtime hooks
- hot reload cannot corrupt the active runtime on failure
- installed interfaces and declared member contracts are enforced at runtime boundaries
- baseline Cranelift JIT can be enabled without changing language behavior
- documentation matches implementation and gives Codex agents an executable roadmap
