# Kagari Embedding API Specification

This document defines the Rust-facing API shape for embedding Kagari.
It is a semantic API specification, not a commitment to exact Rust type names.
The SDK package and Rust import are `kagari`, located at `crates/kagari`.
Its independent `source` and `native` features retain their existing contracts.

## Source packages

With the source feature enabled, `kagari-package::project::ProjectInputs::load`
acquires a workspace or standalone package from a filesystem/virtual provider.
`KagariEngine::compile_package` consumes one immutable `ProgramInputs` closure
through the normal checked-program pipeline. `CheckedModule::package_inputs` and
`package_fingerprint` retain the inputs actually checked; manually supplied source
snapshots have no package acquisition provenance.

`engine::packages::PackageBuildSet` reports selected roots affected by source,
selection, alias, provider or query-setting changes. Hosts compile those candidates
and explicitly publish through existing load/reload APIs; removal from the selection
does not unload anything. See [packages](packages.md) for identity, locking and
publication rules, and the [runnable host](../../crates/kagari/examples/package_workspace.rs)
for several calls into one package Program. Artifact-only SDK configurations have
no package acquisition or source-analysis dependency.

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
unchanged. Without an explicit binding, the normalized source name identifies an
unnamed root in its own `source-file:` package family. `SourceSnapshot::module`
resolves the binding to its effective source, including an overlay.

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
function bindings without executing script code. Artifact emission includes the
complete program. Loading preflights every member's bytecode and host bindings
before publication or resource accounting. Binding validation covers every member before an entry function runs.
`compile_source` supplies base text through this same database, so an active
overlay still takes precedence. Installed declarations are analysis inputs: changing an interface invalidates
results derived from the previous analysis environment.

`declarations(source_snapshot, cancel)` returns a `DeclarationSnapshot` with
per-file module names, named declarations and parse/declaration diagnostics.
`signatures(source_snapshot, cancel)` returns a `SignatureSnapshot` with checked
signature facts and signature diagnostics. Neither entry resolves body names,
collects local bindings, checks bodies or evaluates constants. These queries are
body-independent tools; their results cannot be passed to code generation.
`FileSignatures::definition_at(byte_offset)` follows declaration sites and
checked signature type names, including imported source and shared external types.
External type references select only the terminal name, including through aliases;
qualifiers and separators have no type target. The declaration's scoped identity
resolves its schema through `Declarations::external_type`. It does not
offer body-reference navigation before that body has been analyzed.
`FileSignatures::host_type_at(byte_offset)` returns the checked offline host
declaration for the terminal name of a signature type; module qualifiers and
separators do not select the Host type. Host declarations have no synthetic
source location; this query reads the interface catalog without registration.
Full `analyze` and compilation enforce the same language semantics.

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
semantic validation. Exact cached results retain their original source/signature
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
IR generation consumes this semantic fact. A user
binding with a helper's spelling is resolved as that binding; it does not acquire
the helper's behavior. An unresolved or invalid call
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
Semantic Struct/Enum/Trait TypeId values currently carry the same DefinitionPath used by
navigation. Generic TypeId equality and hashing use owner and parameter position;
the retained parameter name is diagnostic metadata. Implicit Self has its own
trait-owned type identity, distinct from an ordinary parameter named Self.
Impl ABI types and interface validation
consume type-reference facts instead of reinterpreting HIR type syntax.

LoweredModule owns its source origin. Lowering accepts SourceFile; the origin-free
AST lowering and standalone type-check entry were removed. Analysis collects
declarations before checking types. Executable type layouts and linked ABI
identities remain separate unfinished R07/R08 work.

Script generic functions are templates, including public source-package functions. Calls infer their parameters structurally
from argument types and check declared bounds. Missing arguments produce
`KG_TYPE_CANNOT_INFER_GENERIC_ARGUMENT`. Public source visibility allows dependent
packages to specialize a template; it does not create a host-callable generic entry.
Expose non-generic script wrappers for host entry points. Public inherent methods and static associated functions may retain
generic templates, including across source modules. Host member binding infers
impl arguments from the applied receiver, takes method arguments explicitly, and
requires a checked concrete instance or closed shared-call witness in the installed
product. Binding does not compile new instances or search for new trait proofs.
Compiler source lowering starts from the currently callable
non-generic functions, enqueues called instances, and deduplicates by declaration
identity plus concrete arguments. MIR InstanceId is separate from HIR FunctionId.
Signatures, locals, temporaries and direct calls use the selected instance.
Static trait calls on a concrete type use checked implementation targets;
reachable generic impl methods specialize by receiver arguments. Local applied
trait impl headers and bounds retain their arguments in checked ABI tables.
Concrete interface tables execute through the runtime/VM frame driver.

`lower_to_mir(checked, options)` returns an immutable `VerifiedMirModule` after
checking MIR structure, operations, definite initialization and sealed point analyses. Bytecode generation
requires this handle; editing its `into_unverified()` result requires `verify_mir`
again. The verification and linked-program boundaries are defined in
[bytecode.md](bytecode.md#verified-mir-boundary).

Host registration uses explicit ModuleDecl facts and ModuleBuilder declarations.
`bind` checks one callback against its authored function or method; installing the
finished NativeModule publishes the checked declaration/binding set atomically.
Cold declarations can be read, encoded, adopted and compared without creating a
runtime or executing a callback. Source-free loading verifies the required native
contracts and installed schemas before script execution. See
[the shared registration contract](host-interop.md#type-registration) and the
[private source-trait example](../../crates/kagari/examples/host_interfaces.rs).

`FileAnalysis::field_at(offset)` returns the checked field identity for source and
shared external fields in reads and assignment paths. Only the field name selects
that identity; the receiver and `.` return `None`, including for a rejected write
path. Resolve the identity through the snapshot's retained definition table and
the owning schema. An external field need not have a source declaration location.
This query reads checked offline facts and does not invoke a callback.

`FileAnalysis::definition_at(offset)` applies the same name-only rule to checked
source field reads, writes and method calls. A receiver position resolves its own
binding; the `.` has no declaration target.

`host_function_at` and `source_function_at` likewise limit a dotted callee to
its function name. A nested receiver call keeps its own declaration target.
Qualified path expressions also navigate only on their final name: in
`api::run()`, `run` can resolve to the imported function, while `api` and `::`
do not resolve to that function. This applies to cross-file definition queries
and enum-variant references. For `Event::Ready`, `Event` resolves to the enum
declaration and `Ready` to the variant; `::` has no target. Reference-name
ranges are captured from CST nodes
during HIR lowering for paths and field reads; write-place member ranges are
captured there as well. Queries do not infer them by scanning source text or
subtracting a name length from the whole expression range. Trailing trivia and
non-ASCII comments therefore cannot move a target.

Struct initializer type names and field labels use the same source-map rule.
The semantic table records each checked field identity in source order; a
known label remains navigable when another initializer field or value is
invalid. Unknown labels have no declaration target, and code generation still
rejects the invalid initializer.

Type references retain their full span for `type_at`, while declaration and
offline host-type queries select only the terminal CST name. For `lib::Box<T>`,
`Box` has the checked type target; `lib`, `::`, `<` and `>` do not inherit it.
Nested type arguments keep their own targets. This also applies to trait bounds,
where targets and explicit constructor type arguments.

Named declaration locations point to the identifier token. The HIR source map
separately retains the complete item range for diagnostics and code lowering.
The same token-boundary rule applies to local bindings and named members, so
leading or trailing trivia does not become part of a declaration target.
`FileAnalysis::definition_at` also resolves a declaration's own identifier,
including named items, parameters, generic parameters and local bindings.
Only explicitly recorded declaration sites participate; a synthetic module
initializer or incomplete name cannot turn its wider source span into a target.
`FileDeclarations::member_at` applies this rule to fields and enum variants
before body analysis; missing member names retain recovery facts but have no
declaration-site target.

`lower_to_mir(checked, options)` takes `MirLoweringOptions`; embedding exposes the
same controls through `ArtifactOptions::lowering`. Defaults allow 1024 generic
instances, 8192 nodes per type expansion, depth 64 and 1,000,000 generated
instructions including terminators. Type expansion checks limits while copying,
so recursive growth fails before constructing an unbounded replacement. The
options also carry a cancellation token. Limit failures produce
`KG_COMPILE_LIMIT_EXCEEDED` with the originating source revision, and cancellation
returns `EmbeddingError::Cancelled`. An unresolved type at code generation gives
`KG_COMPILE_UNRESOLVED_TYPE`. These failures leave checked analysis reusable.
Parser depth, const-evaluation and full semantic diagnostic-output limits are
available through `KagariEngine` setters. These are logical compile-time budgets;
whole-process memory and wall-clock quotas are outside this checkpoint.

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
An impl receiver can reference a declaration-owned
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
as `Event::Empty`; calling a unit variant with parentheses is rejected. Payload variants check argument
count and types; argument errors retain the target and enum result type. Unknown
variants retain their known enum owner and report a diagnostic. Arguments are
still checked when the member is absent. Navigation on the qualified callee uses
the retained variant declaration, including across source facades; unrelated
unresolved arguments do not navigate to the enclosing constructor. Body reuse
remaps expression keys while preserving nominal targets. These facts are available
in independent function queries as well as full analysis.

IR consumes these facts to emit generic enum operations with nominal layouts;
linking encodes module-local enum and variant slots. Source, standard and registered
native enums share checked generic instantiation, ordered tuple fields and pinned
layouts. Unsupported enum operations use the existing JIT fallback before entry.
`LoadedModule::enum_variant` returns a verified `EnumVariantRef` retaining its
generation. Registered native bodies use scoped type arguments and
`CallContext::allocate_enum`; host construction uses
`Runtime::make_enum_member(owner, applied_type, member, fields)`. Both validate
identity, installed generation, substituted payload types and live nested handles.
A same-named user enum has its own nominal identity and cannot become a core type.

`TypeTable::propagation` records checked Try.branch and FromResidual.from_residual
calls, signatures, the enclosing return type and actual ControlFlow member IDs.
Lowering consumes those facts and evaluates the operand once. Generic bounded
projections are normalized after substitution; cached body queries preserve the
nominal method/member identities while remapping expression keys. Standard
Option/Result/ControlFlow policy is implemented by registered library bodies.

`AnalysisSnapshot::declaration(id)` finds a named declaration at that snapshot's
revision, but rejects a local binding from a different analysis. An unchanged cached
analysis retains its local identities; source text or analysis-environment changes create new ones.
The original snapshot remains usable after edits. See the runnable
`crates/kagari/examples/source_queries.rs` example.

## Design Goals

- expose a small, stable host API for compiling, loading, running, and reloading scripts
- keep host state ownership explicit
- keep host registrations typed with explicit effects
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
  carries cancellation, fixed logical time,
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
emit_bytecode(checked_module, artifact_options) -> CompileResult<DefinitionMetadata<BytecodeArtifact>>
compile_to_artifact(source, artifact_options) -> CompileResult<DefinitionMetadata<BytecodeArtifact>>
prepare_artifact(artifact, compatibility, cancellation) -> PreparedProgram
load_program(prepared_program, load_options) -> LoadResult<LoadedModule>
execute(module, entry, args, execution_context) -> RunResult<Value>
reload_program(previous, prepared_candidate, reload_options) -> ReloadResult<LoadedModule>
```

Convenience functions may combine these operations for CLI use, but the underlying phases remain separate.

The current Rust facade uses `KagariEngine` for compile and artifact emission and `KagariRuntime` for load, execute, reload, host registration, and optional backend execution.
`KagariRuntime::reload_program` stages and validates the candidate before publishing,
following [module-activation.md](module-activation.md). Validation errors retain their
reload codes.
Reload preserves the existing module identities, public ABI and typed-path contracts.
It also requires equivalent public namespace/function paths and an unchanged optional
root-entry signature. Public aliases may select different canonical functions when
parameter and result types match; parameter names and private implementation targets
are not public-path identities. Nominal type identities still must match. Cyclic alias
graphs are compared as finite graphs, so equivalent cycles may have different internal
shapes. Removing/renaming a public path, changing its signature or changing the separate
`main` entry contract fails with `KG_RELOAD_PUBLIC_EXPORT_MISMATCH` after the existing
module checks. Previously bound calls retain their supplying generation.

At the runtime layer, `stage_reload_verified_program` (used by the SDK),
`stage_reload_program` and `stage_reload_artifact` return an owned
`StagedReload`. A driver may enter `begin_candidate_initialization` to explicitly
evaluate candidate functions, then calls `publish_staged_reload` after that session.
Dropping the candidate discards its instances and module retention. Ordinary VM reload
does not execute candidate functions.
Candidate sessions inherit cancellation, apply host-effect restrictions,
and restore a suspended ordinary root when they end. Drivers must finish every nested
candidate execution scope before dropping the candidate session.
The candidate session borrows its staged owner. Ordinary execution rejects staged
module handles, and publication requires candidate execution to have ended.

`KagariRuntime::execute` runs through the interpreter.
`PreparedProgram::from_artifact` consumes `DefinitionMetadata<BytecodeArtifact>`,
checks resource limits against the retained input table, explicitly imports into
its preparation context, and validates reusable code before runtime linking.
Identity import never creates executable verification evidence.
With `native`, it also verifies optional portable MIR and canonical bytecode
correspondence. `KagariRuntime::prepare_native` uses a trusted, host-supplied
`CodegenBackend`, caches products by complete configuration/version identity and
installs a handle for that runtime. `execute_prepared` consumes that decision without
compiling. Only pre-entry unsupported/policy decisions fall back; compiler errors
and failures after entering native code remain errors. No codegen/MIR dependency
is added to runtime or VM. See [artifacts](artifacts.md#sdk-feature-boundary) for
source-only, artifact-only and frontend-free native configurations.

Each call applies that call's ExecutionContext cancellation and execution
policy to an owned execution session, without replacing runtime defaults.
The entry and interpreter/JIT fallback share this session. Its
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
the pinned dependency program, cancellation and call-depth limit.
Dropping the final scope releases the session; dropping an outer handle early does
not reset a still-active nested scope. ExecutionSession::counters reports root-call
lifecycle peaks and elapsed time; Runtime counters track live occupancy and
runtime peaks. Synchronous script
reentry is available through HostCallContext and kagari_vm::reenter, using a
LoadedModule and FunctionRef from the pinned root program. Its returned
RootedValue remains alive across collection; ordinary raw Value copies do not.
See [host-interop.md](host-interop.md) for scope and error rules. The `host_reentry`
example demonstrates this boundary: `cargo run -p kagari --example host_reentry`.
It also installs an ExecutionObserver on an explicit root scope to observe both
the suspended outer frame and the nested frame in the session-owned stack.
The example keeps scratch values with HostCallContext::retain_temporaries and checks
that ExecutionSession::host_scope_count returns to zero before ending the root.
The `scoped_execution` embedding example demonstrates independent execution sessions and
cancellation: `cargo run -p kagari --example scoped_execution`.

Execution reports contain an owning RootedValue result. Moving `report.return_value`
into host state or cloning it preserves retention across subsequent execution and
collection. Inspect it through `value(runtime.runtime().gc())`, which validates the
owning heap and root generation; a raw Value copied from that inspection does not
extend lifetime. RootedValue clones share retention and release it on last drop.
Shared callbacks require Send + Sync; a captured owning root retains its
script object until the registration releases it.
The rooted_values runtime example demonstrates retention and collection pause reporting.

`execute_typed` accepts an outer Rust argument tuple and returns owned Rust data or
an owning handle. It uses the installed entry's closed semantic signature; missing
generic execution evidence is rejected without source analysis. Argument and return
type mismatches fail before entry, while a data-dependent return conversion failure
preserves completed script effects. Conversion and execution share the active
session's cancellation and pinned program. The explicit named-entry policy matches
`execute`.

### Scoped external host entry

KagariRuntime::with_host_scope reserves the mutable runtime and gives its callback
an invariant native HostScope. The callback returns RunResult, so native lending
errors use the existing SDK runtime error mapping without string conversion. No
scope brand, borrowed result or host reference can escape.

Inside that scope, KagariRuntime::execute_scoped(scope, loaded, entry, values,
execution) delegates to the VM's existing checked scoped entry. ExecutionContext is
required: cancellation, deterministic inputs, host-call recording and the observer
apply to this root session. JitPolicy::Disabled explicitly selects the interpreter;
other policies reject before callback effects. Scoped native reference bodies have
no implied native backend implementation. The same API is available without source
or native features and reads only the installed source-free checked product.

A returned raw Value is rooted until scope exit. Decode owned DTO/scalar data or
retain an owning handle within the scope when a heap result must outlive it. Leaving
the scope revokes every external view on success, failure and supported Rust unwind.
The executable [scoped host example](../../crates/kagari/examples/scoped_host.rs)
uses the SDK facade for the same explicitly registered Opaque/external method body.

## Typed native registration

`ModuleBuilder::define_function` authors a `FunctionDecl` with explicit parameter
names, semantic types, return type and Markdown documentation. `bind` attaches a
raw/primitive binding, a flat call-context entry or an owned typed callback such as
`|cx: &mut NativeContext<'_>, (text,): (String,)| -> NativeResult<Vec<String>>`.
Instance callbacks may pass an owning receiver separately from the argument tuple.
Rust and Kagari traits remain independent; callback types do not register impls.
All adapters retain the authored signature and use checked installation and
per-application conversions. A concrete interface result is configured explicitly
with `FunctionBuilder::produces` so the compiler supplies the selected adapter.

Existing cold Future producers use the same `bind` with the `Async<A>` inference
marker and an explicit `Future<R>` declaration. Their callback accepts owned
arguments plus `Completion<R>`; argument conversion and submission still occur on
first await. The callback's tuple and completion types must be annotated. Borrowed
host loans cannot be retained in a producer.

The common KagariType/IntoKagari/FromKagari adapters support scalars, owned String,
Vec, ordinary installed Option/Result and tuples of arity 0-12. The outer tuple is
the argument list: `()` means no arguments and `((a, b),)` means one tuple argument.
Owned conversion copies data, bounds recursion/work and rejects unrepresentable
cycles; identity-preserving adapters retain the original object instead. Input
and output capabilities are independent, and no borrowed input reference escapes.
NativeResult is the runtime-failure channel; a nested business Result remains data.
Conversion does not implicitly install nominal providers or ignore type origins.
Rust Vec<T> and ScriptVec<T> refer to the installed nominal Vec declaration and
reject builtin arrays, including empty ones. Portable HostValueType::Array carries
only its element schema; Vec and List schemas carry their explicit declaration
identity plus element schema. Runtime matching preserves these families and checks
empty-container element contracts as well as each stored value. CallContext exposes
allocate_array and allocate_vec explicitly; native storage factories allocate
fixed arrays with allocate_array.

`ModuleBuilder::register_type::<T>()` stages a finite schema graph transactionally.
`kagari_derive::KagariValue` supplies ordinary named structs and unit/tuple enums,
using the existing rooted object/variant converters. It supports nongeneric owned
fields with registered conversions; tuples/packed structs, unions and record enum
variants require an explicitly authored adapter. Registering a data schema adds
no Rust methods or Kagari trait impls. Opaque fields retain object identity instead
of copying their Rust payload; see the [DTO/Opaque consumer](../../crates/kagari/examples/value_interop.rs).

The optional runtime/SDK `serde` feature exposes `native::serde::Serde<T>` and
`KagariSchema`. Use `kagari_derive::KagariSchema` for a static serialization schema,
then register `Serde<T>` explicitly. The wrapper is absent from script signatures;
outbound requires `Serialize`, inbound requires `DeserializeOwned`, independently.
The adapter invokes Serde directly against checked declarations, with normal
conversion roots, complete immediate-child snapshots, cycle/depth/work/string
limits and cancellation. It does not build JSON, byte buffers or a dynamic value tree.
String formatting through `collect_str` is charged before buffer growth.

Automatic Serde schemas support the same primitives, owned String, Vec, tuples,
Option/Result, named structs and unit/newtype/tuple enum variants. Symmetric short
`rename`/`rename_all`, unconditional `skip` and `default` are supported. Names must
remain script identifiers. Skipped fields have no script member; ordinary script
structs still contain every declared field, so `default` does not admit incomplete
GC objects. Flattening, conditional omission, asymmetric names, changed tagging,
custom serializer attributes, borrowed/remote schemas and generic Opaque-ID encoding
are rejected. A manual schema must match its actual serializer and passes the same
value validation; no schema grants a Rust reference to GC storage or a field projector.
Run the [Serde consumer](../../crates/kagari/examples/serde_interop.rs) with
`cargo run -p kagari --example serde_interop --no-default-features --features source,serde`.

An explicitly declared interface result can differ from the callback's concrete
Rust result. Compilation selects its existing result adapter before execution;
invocation validates the concrete value and applies that adapter without trait
lookup. NativeContext::collect polls cancellation around each iterator step and
checks Vec capacity growth. Individual blocking iterator steps are not preempted.
See the [typed_native example](../../crates/kagari/examples/typed_native.rs)
for registration and a host entry with owned arguments and results.

`RustType<T>` is the runtime-bound target descriptor shared by external scope
lending and NativeType's storage application. Bind ordinary registered structs with
`runtime.bind_rust_type::<T>(&owner)`; use `native_type.rust_type()` for a native
payload. A HostScope reserves the original Rust value, uses checked loan-capable
methods and releases every address on exit. GC struct values remain owned DTO data
and cannot become Rust references. See the [scoped consumer](../../crates/kagari/examples/scoped_host.rs)
and the [scope contract](host-interop.md).

## Retained objects and prepared calls

`Object`, `Opaque<T>`, `ScriptVec<T>`, `ScriptMap<K, V>`, `ScriptSet<T>`,
`Interface` and `PinnedFunction<A, R>` retain the values needed by the host. Clones
preserve identity and share retention; dropping the last handle permits collection.
`ScriptValue` forwards an exact declared generic value without erasing its type.
Ordinary field/collection reads and callback results need no manual root operation.

VM and SDK direct execution (`execute`, `execute_typed`, `start`,
`execute_prepared`) and SDK `prepare_native` accept `ProgramEntry` from
`kagari_runtime::module::entry`. A string converts to `ProgramEntry::Export` and
selects only a checked public path. Use `ProgramEntry::Main` to explicitly invoke
the root's separately verified zero-argument script entry, including a private
`main`; the CLI does this for both interpreter and prepared native execution.
A string `"main"` has no special visibility privilege. Re-exported paths use the
target module for conversion, native compilation/cache keys and installed-code
matching. Missing entries fail before script execution, and implementation display
names do not participate in selection. Ordinary typed paths can accept arguments;
the explicit root entry remains zero-argument.

`Runtime::bind_function` resolves verified public paths from the Program root,
including re-exports into dependencies, and returns a typed `PinnedFunction`.
Passing a child module handle does not change that lookup root. Private declarations,
private `main` and public generic templates are absent from ordinary host lookup;
use a concrete public script wrapper for a generic source application. Explicit
public declaration/application binding remains available for installed native entries
and other existing checked executable evidence. Both routes validate the complete
Rust signature and retain the supplying Program generation before execution.

Prepare `Field<T>` and `Method<A, R>` once from the installed applied type, then
reuse them. Native and script inherent members share declaration/name binding;
method tuples exclude the receiver. Associated functions return PinnedFunction.
Generic binding consumes installed executable evidence and retains each supplied
type's lexical scope; it does not infer new implementations at runtime. Interface
and selected calls retain their checked implementations, including inherited,
default and static members. Handles reject foreign runtimes and incompatible
applications or generations; old handles keep their selected version after reload.

`KagariRuntime::with_context(owner, execution, |cx| ...)` opens SDK host access
with cancellation, execution phase and backend policy. Field reads/writes and
method/closure calls share that scope; native callbacks receive it automatically.
No metadata or payload borrow spans script reentry. See the executable
[host_objects example](../../crates/kagari/examples/host_objects.rs).

`Opaque<T>::edit` is available only for storage registered with fixed,
reference-free NativeData. `native_data!` verifies every declared field. Traced
payloads use ManagedStorage field tokens, complete builders and checked per-field
writes; `edit_data` lends only the fixed data portion. Registered traversal handles
new fields without changing the collector. Mutable references cannot escape a
payload callback, and conflicting reentry fails before execution.

`Opaque<T>::take` requires the explicit unsafe `NativeTake` edge-free proof and
storage registered with `NativeStorage::takeable` (or `data`). It moves the original
payload without Clone/Copy requirements. A failed take changes nothing; a successful
take invalidates all old host/script aliases, parked values and method receivers.
Reinsertion has a new generation, including when the same physical slot is reused.
The collector retains only an inert, charged invalidation identity while old aliases
remain; it cannot be accessed or published as a live object. Managed script-edge
payloads do not receive extraction capability from an empty trace function.

`ScriptVec::retain` and `dedup_by` run typed callbacks under the existing exclusive
buffer lease. Aliases cannot mutate or read detached slots. GC may run during the
callback; errors/unwind restore storage with completed removals preserved. A read
of unavailable storage is an error, not an empty result. Per-field conversions
finish before their writes; removal handles retain detached results immediately.
Standard-library pop/remove additionally preserve their specified preparation of
Option results before committing the removal.

Preauthored native catalogs can use `NativeBinding::declared` and
`SelectedCall::from_declaration`; ordinary registration uses `bind` and
FunctionBuilder::requires. Both consume the same validated declaration, scope and
selected-operation machinery. RootedCallable is removed; independently retained
host callbacks use PinnedFunction. CallContext, borrowed native views and traced
StoredCallable remain advanced adapters for concrete storage/iterator operations,
with explicit rooting obligations. They are not a second ownership model.

## Host Registry API

The host registry supports explicit registration of:

- host types
- host functions
- host constructors or factories, when allowed
- host-backed roots
- typed path descriptors
- trait/interface implementations for host values
- declared reflection metadata exposed to tooling

Registration must produce stable metadata identities used by type checking, bytecode validation, typed path mutation, hot reload, and optional JIT compilation.

## Type and Function Registration

Host function registration records:

- script-visible name
- parameter types
- return type
- passing style for each parameter
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

- root cancellation
- deterministic inputs and optional host-call recording
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
- execution-phase violation
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

The host supplies a validated `PreparedProgram` candidate for an existing module id.
The SDK retains that exact verified identity during staging and publication.
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
The current `BytecodeArtifact` alias names the unverified KBC record. Encode scoped
metadata with `artifact.records().to_bytes(artifact.definitions())`;
`BytecodeArtifact::from_bytes(bytes)` returns records with their retained identity
table as `DefinitionMetadata<BytecodeArtifact>`. Loading and preparation still
validate compatibility and executable semantics.

## JIT Control

The host controls JIT policy through runtime or execution options.

Allowed policies include:

- disabled
- enabled for eligible functions
- compile on load
- compile on first call
- compile after threshold

JIT policy must not change script-visible behavior.
When JIT is disabled or valid input is unsupported, the interpreter remains the
execution path. The current SDK requires explicit `prepare_native` and
`execute_prepared`; compile-on-load, first-call and threshold scheduling are future
policy choices, not implemented automatic schedulers. Ordinary `execute` requires
`JitPolicy::Disabled`.

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
The host may attach a debugger; inspection validates handles and declared member access.

Debug sessions use the model defined in [debugger.md](debugger.md).
The VM [debugger adapter boundary](debugger.md#adapter-boundary) exposes request, response, event, and event-sink types for IDE or DAP integrations.

## Threading and Isolates

A single Kagari runtime isolate is single-threaded from the script perspective.
Hosts may run multiple isolates on different Rust threads. Runtime, Vm and
KagariRuntime are Send and not Sync. A host can move a quiescent runtime, or own
it across message-receive awaits in a Send task; synchronous calls and scoped
borrows finish before transfer. Native payloads/observers require Send, while
shared registrations require Send + Sync. Thread-affine services stay outside the
runtime and use host message/ID boundaries.

PreparedProgram remains preparation-side Rc ownership with a RefCell native cache;
it is neither Send nor Sync and is not retained by KagariRuntime. Load/install code
before transfer, or prepare independently on the receiving worker. Installed native
products retain transferable finalized owners. This does not advertise concurrent
shared native preparation or a portable shared Program API.

Host APIs must not expose one mutable script heap concurrently to multiple script threads.
Cross-isolate value sharing requires explicit serialization, host handles, or embedding-defined transfer rules.

## Acceptance Criteria

The embedding API is complete when:

- host applications can compile, load, execute, and reload modules through stable entry points
- host registrations produce metadata used by checking, bytecode, runtime, reload, and JIT
- execution contexts preserve cancellation and runtime call-depth limits
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

## Owned execution and native waits

`KagariRuntime::{start, drive, drain_retired_executions}` exposes runtime-owned
execution with or without SDK source/native features. Start validates a pinned
entry, retains owned arguments and queues it without executing script. It returns
`kagari_runtime::session::owned::OwnedExecution`, with no Runtime borrow in the
handle. Drive takes a `NonZeroUsize` instruction slice and returns
`kagari::runtime::owned::DriveResult`:

- `Runnable`: a safe slice exit; the host should schedule another turn.
- `Waiting`: an incomplete operation; the host should wait for readiness.
- `Complete(RunResult<RootedValue>)`: a terminal value or script/cancellation error,
  after execution cleanup. Driver API errors are the outer `RunResult`.

`OwnedExecution::set_waker` registers a host scheduling notification and rechecks
durable readiness. `is_ready` observes that coalesced state; a wake never enters the
runtime. Typed `bind` producers using `Async<A>` return cold
`core::future::Future<T>` values, receive owned input/completion endpoints on first
await, and convert results on the driver. Completion can arrive before registration,
during submission or from another thread without executing a continuation there.

Owner cancellation affects only that execution. The context's cancellation token
can cancel all executions started with that context, including while waiting;
owner cancellation does not propagate back into this shared token. Drop requests
retirement. Explicit drain, another start/drive or runtime destruction releases
abandoned frames, leases and pending operations. Cancellation of a retained owner
completes through its next drive; requesting cancellation alone is not cleanup.

The owned driver uses the interpreter and requires `JitPolicy::Disabled`, like
ordinary baseline execution. `prepare_native` returns Unsupported for resume
bodies before calling the backend, directing the host to this owned driver.
Synchronous execute/reentry cannot drive a suspendable body, even if its operation
would complete immediately. Native calls remain cooperative and may overrun a slice.

Source `async`/`.await` is implemented. `start_future` queues one cold Future layer
for owned driving; ordinary calls return the Future without running its body.
`create_task_scope` installs a host-owned scheduling capability. Script
`scope.spawn(factory)` and `Runtime::spawn_task` admit an independently driven Task
without calling its factory. `drive_task` uses bounded interpreter activations;
`ready_tasks` provides the scope's durable ready set. A dispatcher receives
identity-only notices and must arrange exclusive host driving. It can use a mailbox,
an event loop or a simple coalesced wake; it is not required to be an Actor.

`take_task_report` transfers a terminal result/failure and retires its admission
slot. Retained script Task handles still cache their result for multiple awaits.
Reports preserve task/scope and original failure identity, including bounded detached
spawn/await sites. `TaskScopeOwner::close` requests cancellation; the host calls
`drain_cancelled_tasks` until cleanup is acknowledged by `is_closed`. No remote IO
reply is required. Dropping a Task handle does not cancel the scope-owned job.

The [async_tasks example](../../crates/kagari/examples/async_tasks/main.rs)
demonstrates this public API with two native providers, ordinary for-loop awaits,
terminal reporting and scope shutdown. Its decoded artifact also executes without
SDK source/native features. The [roadmap](../implementation-roadmap.md#verification-and-open-ci-acceptance)
records the separate full GitHub CI gap.
