# Kagari Embedding API Specification

This document defines the Rust-facing API shape for embedding Kagari.
It is a semantic API specification, not a commitment to exact Rust type names.

## Source snapshots

`FileAnalysis::signatures_reused()` and `FileSignatures::reused()` report whether
their signature query reused earlier checked facts. Unchanged queries share the
original result and its statistic. Body edits can reuse signatures even when body
or signature diagnostics exist; positions and type-reference IDs belong to the
new source revision. The `source_queries` example exercises this behavior.

An engine owns one source database and caches for declaration, signature and full
analysis queries. `load_source` reads
disk text, `set_source` supplies host text or an editor overlay, and
`close_overlay` exposes the latest base text again. Relative file paths resolve
against the database's captured absolute root. File URIs and local paths share
identity; virtual source URIs retain their scheme.

Use `engine.bind_module(source_name, ModuleIdentity { package, path })` to bind a
logical module before compilation. A second source cannot claim the same module.
Rebinding creates a new revision and invalidates analysis, even when text is
unchanged. Without an explicit binding, the normalized source name identifies a
single-file module in the `source` package. `SourceSnapshot::module` resolves the
binding to its effective source, including an overlay.

HIR analysis accepts source input so facts retain file/revision/module ownership.
`CheckedModule::module_identity()` reports that identity; neither compile nor
artifact options can replace it. Bytecode, artifact header and loader identity
must agree. Runtime load names remain labels for the current runtime module store.

`source_snapshot` captures immutable inputs. `analyze` returns partial semantic
facts and diagnostics for tools; `compile_snapshot` selects a file and requires
checked facts for its entire reachable dependency closure before code generation.
`CheckedModule::program()` exposes that immutable CheckedProgram and its root;
the former single-root `analyzed()` accessor is removed. Dependency-body errors
retain dependency-owned locations, even for unused imports. Both snapshot entry
points accept a cancellation token. Program IR verifies declaration-to-module/
function bindings without executing any initializer. Artifact emission includes the
complete program and dependency initializers. Loading preflights every member's
bytecode and host bindings before publication or resource accounting. Execution-context
policy checks cover every member before any initializer runs.
`compile_source` supplies base text through this same database, so an active
overlay still takes precedence. Language profiles are analysis inputs: changing
permissions cannot reuse a result accepted under another profile.

`declarations(source_snapshot, cancel)` returns a `DeclarationSnapshot` with
per-file module names, named declarations and parse/declaration diagnostics.
`signatures(source_snapshot, cancel)` returns a `SignatureSnapshot` with checked
signature facts and signature diagnostics. Neither entry resolves body names,
collects local bindings, checks bodies or evaluates constants. These queries are
profile-independent tools; their results cannot be passed to code generation.
Full `analyze` and compilation still enforce the requested language profile.

Signature queries build the shared aggregate catalog after declaration signatures
are available, then validate applied struct/enum bounds in parameters, returns,
fields and payloads. Imported templates and facades use the same contracts as body
checking. Dependency changes recompute these diagnostics; body edits rebase their
locations, and unchanged results remain shared. Full analysis consumes the checked
signature result instead of rechecking its applications in every function query.

`AnalysisSnapshot::declaration_snapshot()` and `signature_snapshot()` expose the
immutable query results consumed by full analysis. These share unchanged file
results with standalone queries. Old snapshots retain their source locations;
local bindings created by full analysis never appear in declaration-only results.
Each successful query publishes its own cache, while cancelled queries and older
source revisions cannot overwrite newer entries. Standalone function queries
are available through `body(source, definition,
cancel)`. This returns an immutable `FunctionAnalysis`, or `None` if the declaration
has no body in that snapshot. Its type and receiver queries are restricted to the
selected function, and its scope/declaration facts exclude neighboring local
bindings. Raw local IDs belong to that result's lowering; retained binding handles
cannot be resolved in a different analysis.

Function queries resolve and check module constants as prerequisites, then only
the selected function body. Their diagnostics cover those constants and that body;
parse/declaration/signature diagnostics remain on the retained signature snapshot.
They do not produce checked modules and cannot bypass full compilation or language
profile validation. Exact cached results retain their original source/signature
snapshot. Unchanged user and impl bodies can reuse remapped facts after other body
edits, while local binding identities are recreated for the new analysis. Header
and dependency changes invalidate reuse. Deleted declarations and stale queries
cannot repopulate a newer cache. Full analysis batches bodies through the same
resolver and type checker; it continues to validate the complete dependency closure.

Embedding diagnostic ranges contain file identity, document revision and byte
range. Hosts must reject stale ranges before applying editor actions. UTF-8 and
UTF-16 editor coordinates are checked conversions through the source line index.

`FileAnalysis::definition_at(byte_offset)` follows resolved expression or assignment
names to a declaration with a `FileSpan`. `visible_bindings(byte_offset)` returns
typed declarations from the resolver's lexical scope facts, including match arm
bindings and declaration-order shadowing. Neither query executes script or host code.
Unresolved names have no navigation target; other functions remain queryable.
Resolved trait method calls also navigate to their checked method declaration,
including when argument checking reports an error. Field reads and assignment
targets navigate through the checked field identity, even when the assignment is
rejected as read-only or the field's type annotation is invalid. Field declarations
have module/struct-owned identities; their names survive slot reordering while
their source locations remain revision-specific. Type references in signatures,
field/const/local annotations and impl headers also provide type and definition
queries. If a tuple or generic argument is unknown, later arguments still have
their own facts and navigation targets. Builtin types have types but no source
declaration target. User trait bounds and `where` targets also navigate through
checked facts. Import paths still need dedicated navigation.

`TypeTable::call_resolution` owns each recognized call's target, optional receiver,
and inferred type arguments in declaration parameter order. Arguments can refer
to enclosing generic parameters while a template is being analyzed.
IR generation and reflection permission checks consume this semantic fact. A user
binding with a helper's spelling is resolved as that binding; it does not acquire
the helper's behavior or permission requirements. An unresolved or invalid call
still prevents code generation. Trait-call analysis does not yet imply executable
interface dispatch, which requires the planned linked implementation tables.

`TypeTable::field_type`, `expr_field`, `place_field` and `struct_init` provide the
checked field facts. A HIR field slot is qualified by its declaring struct within
the analysis; tools use the corresponding `Declarations::field` identity when
retaining a target across revisions. Initializer fields retain source order and
unknown fields remain explicit holes in an erroneous analysis. Field ABI generation
uses checked types. Executable field layout/linking remains a separate boundary;
the current bytecode field table still carries owner and field names.

`TypeTable::type_ref` returns a checked type plus an optional declaration target.
Generic parameter declarations are identified by owner and parameter position;
renaming a parameter preserves this identity. Inherited parameters keep their
trait/impl owner, and method parameters have their method owner. An implicit impl
receiver uses the impl header's context even when a method shadows a generic name.
Semantic Struct/Enum/Trait TypeId values now carry the same DefinitionId used by
navigation. Generic TypeId equality and hashing use owner and parameter position;
the retained parameter name is diagnostic metadata. Implicit Self has its own
trait-owned type identity, distinct from an ordinary parameter named Self.
Impl ABI types and interface permission checks
consume type-reference facts instead of reinterpreting HIR type syntax.

LoweredModule owns its source origin. Lowering accepts SourceFile; the origin-free
AST lowering and standalone type-check entry were removed. Analysis collects
declarations before checking types. Executable type layouts and linked ABI
identities remain separate unfinished R07/R08 work.

Private generic functions are templates. Calls infer their parameters structurally
from argument types and check declared bounds. Missing arguments produce
`KG_TYPE_CANNOT_INFER_GENERIC_ARGUMENT`; public generic functions are rejected with
`KG_TYPE_PUBLIC_GENERIC_FUNCTION`. Expose concrete wrappers as public entry points.
IR compilation starts from module initialization and the currently callable
non-generic functions, enqueues called instances, and deduplicates by declaration
identity plus concrete arguments. IR InstanceId is separate from HIR FunctionId.
Signatures, locals, temporaries and direct calls use the selected instance.
Static trait calls on a concrete type use checked implementation targets; generic
impl specialization, applied traits and dynamic interface tables remain pending.

`lower_to_ir(checked, options)` returns an immutable `VerifiedIrModule` after
checking IR structure, operations and definite initialization. Bytecode generation
requires this handle; editing its `into_unverified()` result requires `verify_ir`
again. The verification boundary and its remaining linking limits are defined in
[bytecode.md](bytecode.md#verified-ir-boundary).

Host function registration takes `HostFunction::new(HostFunctionDeclaration,
callback)`. The declaration can be read, encoded and compared offline without
creating a runtime; see [host-interop.md](host-interop.md#function-registration).
`runtime.host().link_interface(...)` checks it against installed bindings without
executing callbacks. General host imports and mandatory artifact host-interface
linking remain R06/R08 work.

`FileAnalysis::host_field_at(offset)` returns the checked host field declaration
only when the byte offset lies on the field name. Offsets on the receiver or `.`
return no host field, including for a rejected write path. The query uses offline
interface declarations and does not invoke a host callback.

`FileAnalysis::definition_at(offset)` applies the same name-only rule to checked
source field reads, writes and method calls. A receiver position resolves its own
binding; the `.` has no declaration target.

`host_function_at` and `source_function_at` likewise limit a dotted callee to
its function name. A nested receiver call keeps its own declaration target.
Qualified path expressions also navigate only on their final name: in
`api::run()`, `run` can resolve to the imported function, while `api` and `::`
do not resolve to that function. This applies to cross-file definition queries
and enum-variant references. Reference-name ranges are captured from CST nodes
during HIR lowering for paths and field reads; write-place member ranges are
captured there as well. Queries do not infer them by scanning source text or
subtracting a name length from the whole expression range. Trailing trivia and
non-ASCII comments therefore cannot move a target.

`lower_to_ir(checked, options)` takes `IrLoweringOptions`; embedding exposes the
same controls through `ArtifactOptions::lowering`. Defaults allow 1024 generic
instances, 8192 nodes per type expansion, depth 64 and 1,000,000 generated
instructions including terminators. Type expansion checks limits while copying,
so recursive growth fails before constructing an unbounded replacement. The
options also carry a cancellation token. Limit failures produce
`KG_COMPILE_LIMIT_EXCEEDED` with the originating source revision, and cancellation
returns `EmbeddingError::Cancelled`. An unresolved type at code generation gives
`KG_COMPILE_UNRESOLVED_TYPE`. These failures leave checked analysis reusable.
Parser depth, const-evaluation and full semantic diagnostic-output limits are
available through `KagariEngine` setters; remaining resource audits are R15 work.

`TypeTable::constraint` distinguishes standard constraint identities from user
trait identities. Bounds are resolved once in their declaring context; inherited
copies share their reference and diagnostic. A `where` target must resolve to a
generic parameter (`KG_TYPE_INVALID_BOUND_TARGET` otherwise). Impl bounds apply
to methods, while a method's own bounds do not affect sibling methods. Shadowing
an inherited parameter gives the method parameter its own identity and constraints;
the implicit receiver can still contain and use the outer parameter's identity.
Type checking and bound ABI metadata consume these facts.
Static trait constraints do not require interface-value permission.
Unknown bounds retain precise reference diagnostics and do not remove nearby
navigation targets. Applied trait constraints currently reject code generation;
their arguments are retained for analysis pending R07 concrete instantiation.

Declaration identity consists of the logical module plus typed owner/name path
segments. A same-kind, same-name occurrence distinguishes duplicate declarations;
unnamed impl owners use source-order occurrences. This is declaration identity,
not an edit-tracking guarantee for renames or reordered duplicate/unnamed items.
Parameters and locals additionally carry their owning body and an analysis instance
identity. They must not be retained as bare arena indices across analyses.

Raw HIR expression, block, statement, place, pattern, local, parameter and type
reference IDs also carry an opaque `HirArenaId`. Their bare-index constructors are
not public. Obtain them from the owning lowering; `index()` alone is not identity.
An unchanged shared lowering retains its arena, while a newly constructed lowering
has a distinct arena even when its source text/revision is identical. Semantic
table lookups do not resolve foreign IDs. Low-level node and source-map access
treats a foreign ID as a compiler invariant violation before indexing. Local IDs
also carry `HirOwner`, distinguishing a function/constant `BodyOwner` from shared
declaration type references. Lowering assigns that owner, including for missing
and synthetic nodes; it is not inferred from source spans. Node/source-map access
checks the stored owner, and name resolution rejects cross-body edges and bindings.
The module initializer is an ordinary function owner even though its source span
can cover other declarations. An impl receiver can reference a declaration-owned
type while its parameter and expressions belong to the method's function body.
Cache reuse explicitly rebases local IDs; arena IDs are not declaration identities
and do not enter executable artifacts. Analysis-scoped binding handles still carry
their own body/analysis identity. HIR storage is shared at module granularity, while
local IDs explicitly name the body that owns each node. `BodyOwner` is defined in
`kagari_hir::hir` and is shared by lowering and semantic scopes.

`FieldId` and `VariantId` contain their lowering arena, owning struct/enum and
declaration-order slot, exposed through `arena()`, `owner()` and `slot()`. They
cannot be constructed from public integers. Member lookup rejects foreign IDs;
signature reuse remaps field type keys into the current arena. A variant's nominal
declaration path contains its enum owner and a `Variant` name/occurrence segment,
so reordering uniquely named variants changes slots without changing declaration
identity. Source maps retain exact member-name ranges. `FileDeclarations::member_at`
and `Declarations::member_at` query those declaration sites without analyzing bodies;
`FileAnalysis::definition_at` returns the same declaration there.
Duplicate variant names retain separate identities and report
`KG_RESOLVE_DUPLICATE_VARIANT` at the repeated name; erroneous declarations remain
queryable, but cannot pass the checked-codegen boundary.

Enum variants retain an ordered list of declaration-owned payload type references.
Signature checking visits every payload annotation, preserving Error facts for
unknown/missing types and usable facts for later members. `FileSignatures::type_at`
queries those annotations before body analysis. Full analysis retains their type
declaration targets for navigation. The shared aggregate catalog includes nominal
enum/variant signatures and payload types for reachable source dependencies;
payload contract changes invalidate dependent body reuse. Signature reuse after
body edits remaps payload references with the other declaration type references.
Public enum ABI metadata consumes these checked types.

For qualified enum constructors, name resolution retains the resolved owner and
member name; imported owners refer to the same checked import/type bindings used
by signatures. `TypeTable::enum_constructor` records nominal enum/variant identities
for both the callee and construction expression. Unit variants can be referenced
as `Event::Empty` or called without arguments. Payload variants check argument
count and types; argument errors retain the target and enum result type. Unknown
variants retain their known enum owner and report a diagnostic. Arguments are
still checked when the member is absent. Navigation on the qualified callee uses
the retained variant declaration, including across source facades; unrelated
unresolved arguments do not navigate to the enclosing constructor. Body reuse
remaps expression keys while preserving nominal targets. These facts are available
in independent function queries as well as full analysis.

IR now consumes those facts to emit enum construction with nominal layout operands;
linking encodes module-local enum and variant slots. Unit and concrete payload
variants execute in the interpreter and existing JIT fallback. Generic enum
instantiation remains R07 work. `LoadedModule::enum_variant` returns a verified
`EnumVariantRef` that retains its executable generation. Host allocation uses
`Runtime::alloc_enum(EnumTag, fields)`; arbitrary enum/variant string allocation
has been removed. Standard Option/Result use dedicated tags, so a declared enum
with the same display name cannot enter their built-in dispatch.

`AnalysisSnapshot::declaration(id)` finds a named declaration at that snapshot's
revision, but rejects a local binding from a different analysis. An unchanged cached
analysis retains its local identities; text or profile changes create new ones.
The original snapshot remains usable after edits. See the runnable
`crates/kagari-embed/examples/source_queries.rs` example.

## Design Goals

- expose a small, stable host API for compiling, loading, running, and reloading scripts
- keep host state ownership explicit
- keep host registrations typed and capability-aware
- make hot reload and module epochs visible to the embedding layer
- keep interpreter and JIT selection behind runtime policy
- return structured diagnostics and runtime errors

## Core Host Objects

The embedding API is organized around these concepts:

```text
KagariEngine
  owns compiler services, runtime configuration, and shared registries

KagariRuntime
  owns loaded modules, heaps, host registries, security state, and execution policy

DebugController
  owns debugger sessions, breakpoint registries, pause state, and debugger events

HostRegistry
  owns registered host types, functions, interfaces, paths, and metadata

ModuleLoader
  resolves source modules, bytecode artifacts, package roots, and imports

LoadedModule
  identifies a successfully loaded module epoch

ExecutionContext
  carries capabilities, resource limits, host access policy, fixed logical time,
  a random seed, and tracing hooks
```

The actual Rust API may split these objects across crates, but the same ownership boundaries must be preserved.

## Compile and Load Flow

The host-visible pipeline is:

```text
compile source -> checked module -> bytecode artifact -> load module epoch -> execute entry
```

The embedding API must expose operations equivalent to:

```text
compile_source(source, compile_options) -> CompileResult<CheckedModule>
emit_bytecode(checked_module, artifact_options) -> CompileResult<BytecodeArtifact>
compile_to_artifact(source, compile_options, artifact_options) -> CompileResult<BytecodeArtifact>
load_program(artifact, load_options) -> LoadResult<LoadedModule>
execute(module, entry, args, execution_context) -> RunResult<Value>
reload_program(previous, artifact, reload_options) -> ReloadResult<LoadedModule>
```

Convenience functions may combine these operations for CLI use, but the underlying phases remain separate.

The current Rust facade uses `KagariEngine` for compile and artifact emission and `KagariRuntime` for load, execute, reload, host registration, and optional backend execution.
`KagariRuntime::reload_program` stages and initializes the candidate before publishing,
following [module-activation.md](module-activation.md). Validation errors retain their
reload codes; initializer failures retain their normal execution error classification.
At the runtime layer, `stage_reload_program` / `stage_reload_artifact` return an owned
`StagedReload`. A driver enters `begin_candidate_initialization`, initializes the
candidate's modules, exits that session, then calls `publish_staged_reload`.
Publication rejects uninitialized or failed members. Dropping the candidate discards
its instances and module quota. VM reload performs these steps automatically.
Candidate sessions inherit permissions and cancellation, apply host-effect restrictions,
and restore a suspended ordinary root when they end. Drivers must finish every nested
candidate execution scope before dropping the candidate session.
The candidate session borrows its staged owner. Ordinary execution rejects staged
module handles, and publication requires candidate execution to have ended.

`KagariRuntime::execute` runs through the interpreter.
`KagariRuntime::execute_with_backend` uses a host-supplied `CodegenBackend` after validating JIT capability and artifact policy.

Each call applies that call's ExecutionContext resources, capabilities and host
policy to an owned execution session, without replacing runtime defaults. Module
initialization, the entry and interpreter/JIT fallback share this session. Its
fixed time and random seed are passed to host callbacks; nested execution shares
the root's random stream. Host callbacks must supply any other external results.
The `CancellationToken` is cooperative and shared by context clones; once cancelled,
use a fresh token for a new root call. Execution cancellation reports
`KG_RUNTIME_CANCELLED`, separate from analysis cancellation and script traps.
Completed host effects survive cancellation.

With `ExecutionContext::tracing_enabled`, a successful `ExecutionReport` contains
the root code fingerprint, deterministic inputs and ordered host-call trace.
`ExecutionSession::trace` also permits inspection while a lower-level session is
active, including after a host error. Trace capture is bounded and marks omitted
data; it does not automatically replay host results.

At the lower-level API, Vm starts a session from Runtime defaults automatically.
Hosts can select explicit ExecutionOptions with Runtime::begin_execution and keep
the returned ExecutionSession alive while driving the VM. Nested scopes inherit
the pinned dependency program, permissions, cancellation and remaining budget.
Dropping the final scope releases the session; dropping an outer handle early does
not reset a still-active nested scope. ExecutionSession::counters reports root-call
usage and peaks; Runtime resource counters remain cumulative. Synchronous script
reentry is available through HostCallContext and kagari_vm::reenter, using an
initialized LoadedModule and FunctionRef from the pinned root program. Its returned
RootedValue remains alive across collection; ordinary raw Value copies do not.
See [host-interop.md](host-interop.md) for scope and error rules. The `host_reentry`
example demonstrates this boundary: `cargo run -p kagari-embed --example host_reentry`.
It also installs an ExecutionObserver on an explicit root scope to observe both
the suspended outer frame and the nested frame in the session-owned stack.
The example keeps scratch values with HostCallContext::retain_temporaries and checks
that ExecutionSession::host_scope_count returns to zero before ending the root.
The `scoped_execution` embedding example demonstrates independent budgets and
cancellation: `cargo run -p kagari-embed --example scoped_execution`.

Execution reports contain raw Value results. Retain a heap result with
`runtime.runtime().root_value(report.return_value)` before a subsequent execution
or explicit collection. Keep the returned RootedValue in host state; cloning Value
does not extend lifetime. RootedValue clones share retention and release it on last
drop. Runtime callbacks are local to one thread and may capture this rooted state.
The rooted_values runtime example demonstrates retention and collection pause reporting.

## Host Registry API

The host registry supports explicit registration of:

- host types
- host functions
- host constructors or factories, when allowed
- host-backed roots
- typed path descriptors
- trait/interface implementations for host values
- reflection metadata exposed to tooling or privileged profiles
- capability requirements

Registration must produce stable metadata identities used by type checking, bytecode validation, typed path mutation, hot reload, and optional JIT compilation.

## Type and Function Registration

Host function registration records:

- script-visible name
- parameter types
- return type
- passing style for each parameter
- capability requirements
- resource cost hints, if provided
- whether the call may allocate, trap, call host services, mutate host state, or suspend

Host type registration records:

- stable type identity
- script-visible name
- ownership model
- exposed fields and methods
- typed path access policy
- reflection exposure policy
- reload and layout fingerprint

Open Rust generics are not registered directly.
The host registers concrete instantiations or a separate factory model defined by host interop policy.

## Execution Context

Every host-initiated execution uses an execution context.

The context carries:

- language profile
- runtime capabilities
- resource limits
- host API exposure policy
- reflection policy
- JIT enablement policy
- tracing or audit hooks
- panic and engine-bug reporting policy

The execution context is not script-visible as an ordinary value.
It is an embedding boundary object used by runtime checks and host calls.

## Error and Diagnostic Model

The embedding API returns structured results.

Compile-time failures return diagnostics with:

- severity
- diagnostic code
- source span
- message
- optional notes and labels

Runtime failures return classified errors:

- script trap
- type or bytecode verification failure
- capability denial
- resource limit exceeded
- host call failure
- typed path validation failure
- stale module or host root
- reload validation failure
- engine invariant violation

Current public error values expose stable code strings through `EmbeddingError::code()`.
Frontend diagnostics, bytecode verification failures, artifact validation failures, runtime errors, and reload validation failures all map to `KG_*`-style codes.

Engine invariant violations may panic in debug builds, but production APIs still expose a controlled error boundary where practical.

## Hot Reload API

Reload is explicit.

The host supplies a candidate bytecode artifact or checked module for an existing module id.
The runtime validates:

- public ABI fingerprints
- type identities and layouts
- interface tables
- host registrations
- typed path descriptors
- bytecode verifier metadata
- JIT artifact dependencies, if JIT is enabled

If validation succeeds, the new epoch is published.
If validation fails, the existing active epoch remains active.

## Artifact API

The embedding API supports source execution and precompiled bytecode artifacts.

Artifact loading must:

- validate artifact header and format version
- validate target ABI and runtime-helper ABI versions
- validate module id and dependency fingerprints
- reject artifacts produced for incompatible language or runtime versions
- preserve debug metadata when available

The artifact format is specified in [artifacts.md](artifacts.md).
The current `BytecodeArtifact` alias exposes `to_bytes()` and `from_bytes()` helpers for the implemented Rust `.kbc` serialization.

## JIT Control

The host controls JIT policy through runtime or execution options.

Allowed policies include:

- disabled
- enabled for eligible functions
- compile on load
- compile on first call
- compile after threshold

JIT policy must not change script-visible behavior.
When JIT is disabled or unavailable, the interpreter remains the execution path.

## Debugger API

The embedding API exposes debugger control through host-created debug sessions.

Debugger operations include:

- attach session
- detach session
- set and clear breakpoints
- pause and continue
- step into, step over, and step out
- inspect call stack
- inspect frame variables
- inspect values according to host policy
- evaluate read-only watch expressions
- receive debugger events

The debugger API is not script-visible.
Debugger attachment and inspection require runtime capabilities and host policy.

Debug sessions use the model defined in [debugger.md](debugger.md).
The VM adapter boundary is documented in [debugger-adapter.md](../debugger-adapter.md) and exposes request, response, event, and event-sink types for IDE or DAP integrations.

## Threading and Isolates

A single Kagari runtime isolate is single-threaded from the script perspective.
Hosts may run multiple isolates on different Rust threads.

Host APIs must not expose one mutable script heap concurrently to multiple script threads.
Cross-isolate value sharing requires explicit serialization, host handles, or embedding-defined transfer rules.

## Acceptance Criteria

The embedding API is complete when:

- host applications can compile, load, execute, and reload modules through stable entry points
- host registrations produce metadata used by checking, bytecode, runtime, reload, and JIT
- execution contexts enforce capabilities and resource limits
- errors and diagnostics are structured
- bytecode artifacts and source modules can be loaded through the same module identity model
- JIT can be enabled or disabled without changing semantics

Assignment recovery analyzes index expressions even when an earlier receiver or
projection cannot be resolved. For example, `missing[make().value] = 1` retains the
index member's type and declaration target. Independent errors in that index still
produce diagnostics (once); the unresolved assignment continues to reject codegen.
Nested indexes use the same rule, and correct neighboring functions remain queryable.
The source_queries example demonstrates navigation through such a broken target.

`type_at` also queries HIR assignment positions and chooses the narrowest available
range alongside expression and annotation facts. A rejected write to a parameter,
`val` binding/field or an element of a read-only tuple retains its known type.
That type still supplies contextual arguments for the RHS (for example an omitted
generic constructor argument). Write rejection remains a diagnostic and prevents
code generation; recovery never grants mutation permission.

`member_receiver_type` queries both field-read expressions and field-write places,
selecting the narrowest matching HIR range. Nested writes, unknown field names and
incomplete trailing-dot targets retain a known receiver where analysis recovered
one. Full-file and per-function queries share this implementation. Repeating an
unchanged query uses its cached snapshot; source movement rebases query positions
without changing the results retained by an earlier snapshot.
