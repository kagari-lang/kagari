# Kagari Architecture

This document defines the production architecture for Kagari.
It describes the intended system shape that implementation work must converge on.
When existing code conflicts with the specifications, the specifications are authoritative.

The [MIR and crate architecture refactor](mir-architecture-refactor.md) records
implementation checkpoints and final acceptance. Language/runtime behavior follows
the semantic specifications.

The active [standard-library and HIR migration](stdlib-hir-refactor.md) separates
the standard source package from ABI and establishes a checked HIR handoff for
native and script implementations. ST00 is complete and ST01 is in progress:
`kagari-stdlib` owns bundled source text and structural preparation, and HIR source
queries read its manifest. Semantic import and removal of ABI's generated
declaration catalogs remain pending; the complete target handoff is not yet active.

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
  kagari-stdlib             installed source manifest, parsing and structural declaration index
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
they do not define the production graph. ABI build tooling parses `stdlib/*.kgr`
with syntax to generate declaration descriptors; that build-only dependency is not
a runtime/source-analysis service and is being removed by ST01. HIR now depends on
stdlib for source ownership; stdlib depends only on syntax/common and error support.
ABI no longer emits the full source-text table. LLVM is deferred; no placeholder
crate exists.

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

The builtin layer owns:

- primitive numeric, boolean, string, unit, tuple, array, map, set, `Option`, and `Result` types
- standard modules such as `std::debug`, `std::math`, `std::array`, `std::map`, `std::set`, `std::string`, `std::option`, `std::result`, and `std::iter`
- iterable protocol support used by `for`
- builtin metadata for type checking, bytecode, reflection profiles, reload validation, and JIT lowering

The standard library is not a historical compatibility layer and is not implemented as a second copy of core containers in Kagari source.
Core containers and string operations are runtime-native builtins with stable intrinsic identifiers.
`stdlib/*.kgr` is the authoritative declaration surface. HIR processes these declarations; ABI owns shared executable identities/contracts and runtime owns implementations. Source declarations do not own storage, GC, resource accounting or host state.

Ordered map and set behavior is deterministic.
The runtime implementation uses insertion-ordered `indexmap` backing for script-visible `Map<K, V>` and `Set<T>` behavior.
Hash-key eligibility and custom equality/hash protocols follow the checked contracts in [builtins](spec/builtins.md). Runtime callbacks execute on the same explicit frame stack with ordinary resource and reentry rules.

Standard library calls flow through one structural path:

```text
source call or method
  -> typed standard module/function/method metadata
  -> stable ABI intrinsic identity carried by MIR and bytecode
  -> bytecode verifier signature checks
  -> VM dispatch to runtime standard helper
```

This keeps ordinary standard library execution out of script-visible reflection and host string dispatch.
Reflection metadata may describe standard values for tooling when the active profile allows it, but reflection is not the implementation mechanism for arrays, maps, sets, strings, or standard helpers.
Reload validation and JIT fallback use the same intrinsic ids and metadata as the interpreter.

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
Runtime checks remain required for host state, capabilities, dynamic indexes, and hot reload epochs.

## MIR, ABI and Bytecode

MIR is a concrete typed, non-SSA control-flow representation. Source generics and
trait obligations are resolved by compiler source lowering into reachable executable
instances; MIR contains no HIR type arena or generic binder. Verification seals
function/program links and bounded analyses: initialization, liveness, effects,
logical roots, safepoints, source/debug origins and logical budget points. Public
passes consume verified input, make bounded changes and reverify the result.

The ABI crate owns nominal executable types, signatures, layouts, standard intrinsic
contracts, helper/native calling representations and version constants. Compiler
core lowers verified MIR into the register/local bytecode contract. Bytecode validates
its own instructions, metadata, dependency graph and canonical artifact envelope.
Native-enabled preparation also proves correspondence of optional portable MIR to
that same bytecode program; executable contracts never depend on source analysis.

Ordinary aggregate access uses checked nominal field slots. Host-backed typed path
access is a separate operation with declared capabilities and scoped borrow rules.
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
- security context and capability state
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
- runtime capability and resource policy
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
script execution and does not consume the script's logical instruction budget.

## Interpreter

The interpreter is the semantic execution foundation.
It executes verified bytecode against the runtime model.

The interpreter must:

- preserve bytecode-visible control flow and value behavior
- enforce traps and runtime errors consistently
- call runtime helpers at allocation, host, reflection, security, and path boundaries
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
- profile-gated reflection metadata
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

Security is layered:

- language profile
- runtime capabilities
- host API exposure
- resource policy

Reflection is metadata-driven and profile-gated.
Ordinary game-state mutation must not go through reflection.
Privileged reflective writes, when provided by an embedding, are separate from typed path mutation and must be explicitly gated.

The CLI currently maps profiles to runtime policy as follows:

- `restricted`: default-deny capabilities, no host calls, debugger, module loading, reflection, path mutation, or JIT.
- `dev`: host calls for `host.log`; JIT only when `--jit` is requested and the binary has the `jit` feature.
- `tooling`: host calls, reflection, path mutation, module loading, debugger capabilities, and optional JIT.

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
- security profiles and reflection gates are enforced at runtime boundaries
- baseline Cranelift JIT can be enabled without changing language behavior
- documentation matches implementation and gives Codex agents an executable roadmap
