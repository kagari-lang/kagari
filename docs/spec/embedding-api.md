# Kagari Embedding API Specification

This document defines the Rust-facing embedding contracts and implemented SDK APIs.
Lower-level runtime and VM interfaces are identified separately from the SDK facade.
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

## Direct source preparation

`KagariEngine::compile_to_program` checks source and prepares it for execution;
`prepare` accepts an already checked module or captured package. Both return the
same `PreparedProgram` used by artifact import. One private lowering owner pairs
verified MIR with bytecode emitted from that exact MIR. The bytecode seal retains
scoped identities, exports and suspension facts. Preparation does not construct a
KBC envelope, encode/decode MIR or repeat unchanged bytecode graph verification.
Dropping the source engine does not invalidate the program; independent runtimes
still perform their own host binding and generation checks before execution.

`PreparationOptions` enables bounded MIR simplification by default. With native
support compiled in, `retain_native_input` explicitly preserves the paired MIR for
native preparation; it defaults to false even if Cargo unifies the native feature.
This does not remove required MIR verification. Explicit artifact export remains
`compile_to_artifact` or `emit_artifact` on checked input, with separate portable
MIR options. Export derives metadata from the exact seal, while editable/imported
artifacts remain untrusted and independently validated. Caller cancellation is
carried by lowering options through source checking and preparation admission.

Source options propagate cancellation through checking, lowering and bytecode
admission. `PreparedProgram::from_artifact` propagates its caller token through
resource checks, identity adoption and executable graph validation; interruption
returns `ProgramPreparationError::Cancelled` without a prepared program. Loading
still validates the receiving runtime's installed bindings before publication.

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
Full `analyze` and compilation enforce the same language semantics.

`analyze_source(source, cancel)` and `analyze_package(inputs, cancel)` retain a
`SourceAnalysis` under the engine's installed declarations. Its `snapshot()` and
`root()` are the optional-tool handoff; `check(cancel)` requires the existing
strict checked-program gate and returns `CheckedModule` without repeating semantic
processing. `compile_source`, `compile_package` and `compile_snapshot` use that
same gate. Package analysis still requires complete `ProgramInputs`; acquisition
recovery cannot introduce partial executable inputs. Retained handles keep their
old facts across later edits. Lint policy is supplied to the separate linter,
never to the SDK analysis environment.

Semantic optional consumers use [HIR-owned eligibility and usage facts](../architecture/hir.md#optional-source-tool-facts).
The linter can report an unused binding in a healthy function alongside a required
error in another function; neither partial facts nor diagnostic truncation can
make the whole program check successfully. Source diagnostics use
`kagari_source::presentation::DiagnosticRecord`, independent of SDK/runtime types.

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

`source_function_at` likewise limits a dotted callee to
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

`TypeTable::constraint` returns a resolved nominal trait application, with the
same representation for standard-library and user traits. Declaration identity,
ordered type arguments and associated-output bindings are retained. Bounds are
resolved once in their declaring context; inherited copies share their reference
and diagnostic. Invalid `where` targets report `KG_TYPE_INVALID_BOUND_TARGET`;
[static bounds](traits.md#static-trait-bounds) define supported targets. Impl bounds apply
to methods, while a method's own bounds do not affect sibling methods. Shadowing
an inherited parameter gives the method parameter its own identity and constraints;
the implicit receiver can still contain and use the outer parameter's identity.
Type checking and bound ABI metadata consume these facts.
Static trait constraints do not require interface-value permission.
Unknown bounds retain precise reference diagnostics and do not remove nearby
navigation targets. Applied trait bounds lower through checked specialization or
shared callable requirements, preserving their arguments, associated outputs and
supplying generations. Loading verifies portable proofs and executable witnesses
without source analysis.

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
`ExecutableModule::enum_variant` returns a verified `EnumVariantRef` retaining its
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

Runtime
  owns loaded modules, heaps, host registries, security state, and execution policy

DebugController
  owns debugger sessions, breakpoint registries, pause state, and debugger events

HostRegistry
  owns registered host types, functions, interfaces, paths, and metadata

ModuleLoader
  resolves source modules, bytecode artifacts, package roots, and imports

Program / Module
  identify a live installation and one of its logical modules

ExecutableModule
  owns one exact executable version for admitted execution or advanced use

ExecutionContext
  carries cancellation, fixed logical time,
  a random seed, and tracing hooks
```

The actual Rust API may split these objects across crates, but the same ownership boundaries must be preserved.

## Installed programs and public paths

`PreparedProgram` is a verified preparation product. `Runtime::load_program`
returns a `Program` identifying one installation; loading the same product again
creates independent state and authority, even with the same display label.
`Program::root()` constructs its root `Module`. `Program::module(path)` starts at
that root; `Module::module(path)` starts at the supplied logical namespace.
Navigation reads the installation's latest published contents, not an immutable
public snapshot. Aliases retain canonical namespace identity; a namespace need not
have a physical executable slot. Retained handles need no reacquisition after reload.

The SDK surface is:

```text
Runtime::load_program(&mut self, &PreparedProgram, LoadOptions) -> LoadResult<Program>
Runtime::reload_program(&self, &Program, &PreparedProgram, ReloadOptions) -> ReloadResult<()>
Program::root(&self) -> Module
Program::module(&self, path: &str) -> Result<Module, RuntimeError>
Module::module(&self, path: &str) -> Result<Module, RuntimeError>
Runtime::object_type(&self, &Module, path: &str) -> RunResult<ObjectType>
```

`function` and `async_function` also bind relative to a supplied `Module`; neither
falls back to the Program root. Whole-program `execute`, `execute_prepared` and
native entry preparation instead take `Program` and explicitly select its current
root. `ProgramEntry::Main` retains its separate private-root-entry capability.
There is no implicit Program-to-Module conversion or legacy `LoadedModule` alias.
`LoadOptions.program_name` and `ReloadOptions.program_name` are diagnostic labels,
not compiled or installation identities; reload changes the label only on success.
Source `update` and prepared reload target the same installation and return unit
success, not replacement handles. See [source updates](#source-updates).

Paths contain 1..64 nonempty ASCII identifier segments separated by `::`. The
whole syntax is validated before graph traversal. Empty paths/components,
leading/trailing separators, `crate`/`self`/`super`, filesystem paths and generic
expressions are not accepted. `root()` is the explicit zero-segment operation.
One bounded checked public-graph traversal serves module, function and type lookup;
it follows aliases/cycles without flattening them or searching implicit packages.
Type/module names share a namespace; a same-spelling function does not shadow them.

A public leaf does not grant access to its private containing module. For example,
`pub use internal::User as Player` permits binding `Player`, not navigating
`internal` or accessing private fields. Binding does not execute script, initialize
statics, invoke host handlers or compile a missing specialization. Function binding
resolves the path once to installation + canonical declaration + checked application/
signature. [Call selection](module-activation.md#live-installations-and-call-selection)
owns invocation and retained-computation semantics; changing an alias affects fresh
lookup, not the declaration of a retained binding.

For a complete runnable example, see
[host_objects](../../crates/kagari/examples/host_objects.rs): it navigates a
namespace-only facade, binds relative functions/types, retains an object and reuses
its handles after compatible publication. Simplified pseudocode:

```text
program = runtime.load_program(prepared, options)
users = program.module("api::users")
score = runtime.function(users, "score")
player_type = runtime.object_type(users, "Avatar")
runtime.reload_program(program, replacement, options)
runtime.function(users, "selected")  // fresh lookup in the latest namespace
runtime.call(score, ())              // original declaration's current implementation
```

### Public object types and binding errors

SDK `object_type` accepts no explicit type arguments. It binds public nongeneric
script structs, renamed exports and supported closed transparent aliases to script
structs, including closed generic applications with verified layouts. Core
`bind_type(&Module, path, &[TypeArgument])` also supports generic struct declarations
with exact explicit arity and existing checked layout/template evidence. The
advanced `bind_type_declaration` route remains installation-scoped and does not
expose private namespaces. No route parses strings such as `Cell<i32>` or asks the
runtime to synthesize missing applications.

Closed aliases accept no additional host arguments. Open aliases, including
`type Erased<T> = User`, are unsupported for host application even with arguments.
Enum, trait, scalar, native/opaque and other non-script-struct targets never become
`ObjectType` handles. [Public type exports](artifacts.md#public-type-exports) owns
the source-free schema, declaration/binder proof and compiler-owned layout demand;
valid unsupported exports still compile. Supported export-only closed layouts need
no constructor/body use to become bindable.

Binding first admits the runtime/installation and borrow scope, then resolves
syntax, public visibility/kind, argument contract and executable evidence. Private
and absent names are indistinguishable. Failures have no script effects:

| Condition | Core error kind / diagnostic |
| --- | --- |
| Malformed or overlong path | `TypedPathValidation`: `invalid public path` |
| Missing/private name | `TypedPathValidation`: `public path not found` |
| Public name of another requested kind | `TypedPathValidation`: `public path has wrong target kind` |
| Non-object type | `TypedPathValidation`: `type is not a script object` |
| Open alias | `TypedPathValidation`: `open generic alias is not supported for host binding` |
| Wrong arity or arguments on a closed alias | `TypedPathValidation`: `type argument count mismatch` |
| Generic declaration without executable application | `ModuleValidation`: `missing executable function application` or `missing executable struct application` |
| Foreign runtime, unknown installation or closed handle | `StaleHandle`, never same-name fallback |
| Poisoned metadata cell or impossible verified target | `EngineFault`, never missing-path fallback |

SDK mapping preserves `TypedPathValidation`, maps `StaleHandle` to
`StaleModuleOrHostRoot` and module verification failures to `BytecodeVerification`.
Artifact forgery rejects during verification through the existing contract/export
validation family. Publication admission/errors remain owned by
[module activation](module-activation.md#publication-boundary).

## Compile and Load Flow

The host-visible pipeline is:

```text
compile source -> checked module -> bytecode artifact -> load module epoch -> execute entry
```

The embedding API must expose operations equivalent to:

```text
compile_source(source, compile_options) -> CompileResult<CheckedModule>
emit_artifact(checked_module, artifact_options) -> CompileResult<DefinitionMetadata<BytecodeArtifact>>
compile_to_artifact(source, artifact_options) -> CompileResult<DefinitionMetadata<BytecodeArtifact>>
prepare_artifact(artifact, compatibility, cancellation) -> PreparedProgram
load_program(prepared_program, load_options) -> LoadResult<Program>
execute(program, entry, args, execution_context) -> RunResult<Value>
reload_program(program, prepared_candidate, reload_options) -> ReloadResult<()>
update(program, update_batch) -> ReloadResult<()>
```

Convenience functions may combine these operations for CLI use, but the underlying phases remain separate.

The current Rust facade uses `KagariEngine` for compile and artifact emission and
`Runtime` for load, execute, reload and optional backend execution. Native
declarations and bindings are installed through `KagariEngineBuilder` before engine
construction; live object roots and scoped host access belong to the runtime.
`Runtime::reload_program` stages and validates the candidate before publishing,
following [module-activation.md](module-activation.md). Validation errors retain their
reload codes.
Reload preserves the existing module identities, public ABI and typed-path contracts.
[Public namespace continuity](module-activation.md#public-namespace-continuity)
defines compatible paths, canonical namespace identities, alias retargeting and the
separate root-entry contract. Export mismatches report
`KG_RELOAD_PUBLIC_EXPORT_MISMATCH` after the existing module checks. Compatible
additions still require frozen contracts, retained demands and local static-initializer
comparison, not merely ABI signature equality.

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

`Runtime::execute` runs through the interpreter.
`PreparedProgram::from_artifact` consumes `DefinitionMetadata<BytecodeArtifact>`,
checks resource limits against the retained input table, explicitly imports into
its preparation context, and validates reusable code before runtime linking.
Identity import never creates executable verification evidence.
With `native`, it also verifies optional portable MIR and canonical bytecode
correspondence. `Runtime::prepare_native` uses a trusted, host-supplied
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

At the lower-level API, Vm starts a session from core runtime defaults automatically.
Hosts can select explicit ExecutionOptions with
`kagari_runtime::Runtime::begin_execution` and keep the returned ExecutionSession
alive while driving the VM. Nested scopes inherit
the installation authority, cancellation and call-depth limit. Exact admitted
bodies are retained separately from current ordinary-call selection.
Dropping the final scope releases the session; dropping an outer handle early does
not reset a still-active nested scope. ExecutionSession::counters reports root-call
lifecycle peaks and elapsed time; Runtime counters track live occupancy and
runtime peaks. Synchronous script
reentry is available through HostCallContext and kagari_vm::reenter, using
an exact `ExecutableModule` and checked `FunctionRef` for the admitted entry. Its returned
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
session's cancellation and exact admitted entry. The explicit named-entry policy matches
`execute`.

### Scoped external host entry

Runtime::with_host_scope reserves the mutable runtime and gives its callback
an invariant native HostScope. The callback returns RunResult, so native lending
errors use the existing SDK runtime error mapping without string conversion. No
scope brand, borrowed result or host reference can escape.

Inside that scope, Runtime::execute_scoped(scope, loaded, entry, values,
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
`|cx: &mut NativeContext<'_>, (text,): (String,)| -> NativeResult<VecValues<String>>`.
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
the stdlib `VecValues<T>` owned adapter, ordinary installed Option/Result and tuples
of arity 0-12. The outer tuple is
the argument list: `()` means no arguments and `((a, b),)` means one tuple argument.
Owned conversion copies data, bounds recursion/work and rejects unrepresentable
cycles; identity-preserving adapters retain the original object instead. Input
and output capabilities are independent, and no borrowed input reference escapes.
NativeResult is the runtime-failure channel; a nested business Result remains data.
Conversion does not implicitly install nominal providers or ignore type origins.
Rust `VecValues<T>` and `ScriptVec<T>` refer to the installed nominal Vec declaration and
reject builtin arrays, including empty ones. Portable `Ty::Array` carries only its
element schema; nominal collection types carry their explicit declaration identity
and type arguments. Runtime matching preserves these families and checks
empty-container element contracts as well as each stored value. CallContext exposes
`allocate_array` for fixed arrays; Vec construction uses the library's ordinary
nominal payload registration, not an engine `allocate_vec` path.

Import library-owned adapters from their owners:

```rust
use kagari_stdlib::collections::{
    map::ScriptMap,
    set::ScriptSet,
    vector::{ScriptVec, VecValues},
};
```

`VecValues(vec![1, 2])` wraps an owned Rust Vec for conversion; decoding snapshots
immediate children before invoking child conversions. Script `Vec<T>` spelling,
shared-object semantics and ScriptVec identity-preserving access are unchanged.
There is no runtime collection compatibility facade.

`ModuleBuilder::register_type::<T>()` stages a finite schema graph transactionally.
`kagari_derive::KagariValue` supplies ordinary named structs and unit/tuple enums,
using the existing rooted object/variant converters. It supports nongeneric owned
fields with registered conversions; tuples/packed structs, unions and record enum
variants require an explicitly authored adapter. Registering a data schema adds
no Rust methods or Kagari trait impls. Opaque fields retain object identity instead
of copying their Rust payload; see the [DTO/Opaque consumer](../../crates/kagari/examples/value_interop.rs).

The optional stdlib `serde` feature exposes `kagari_stdlib::serde::{Serde,
KagariSchema}`; the SDK `serde` feature forwards to it. Runtime retains only its
RuntimeError integration with Serde errors, not the schema/conversion facade.
Use `kagari_derive::KagariSchema` for a static serialization schema,
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
checks Rust Vec capacity growth; wrap that owned result in VecValues for script
conversion. Individual blocking iterator steps are not preempted.
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
`Interface` and `Function<A, R>` retain the values needed by the host. Clones
preserve identity and share retention; dropping the last handle permits collection.
`ScriptValue` forwards an exact declared generic value without erasing its type.
Ordinary field/collection reads and callback results need no manual root operation.
Opaque and collection handles retain data metadata without implicitly preparing an
executable NativeType. Use `opaque.prepare_native_type(cx)` when executable access
is actually needed; its lexical `owner()` need not remain executable. See
[data versus executable retention](host-interop.md#retained-native-data-and-executable-preparation)
for the current stored-selection retention distinction.

Backend VM execution (`execute`, `execute_typed`, `start`, `execute_prepared`),
SDK `execute`/`execute_prepared` and SDK `prepare_native` accept `ProgramEntry` from
`kagari_runtime::module::entry`. A string converts to `ProgramEntry::Export` and
selects only a checked public path. Use `ProgramEntry::Main` to explicitly invoke
the root's separately verified zero-argument script entry, including a private
`main`; the CLI does this for both interpreter and prepared native execution.
A string `"main"` has no special visibility privilege. Re-exported paths use the
target module for conversion, native compilation/cache keys and installed-code
matching. Missing entries fail before script execution, and implementation display
names do not participate in selection. Ordinary typed paths can accept arguments;
the explicit root entry remains zero-argument.

Core `kagari_runtime::Runtime::bind_function`, exposed by SDK `Runtime::function`,
resolves verified public paths relative to the supplied `Module`, including
re-exports into dependencies, and returns a typed `Function`. Async binding uses
the same [public-path contract](#installed-programs-and-public-paths), with no root
fallback. Private declarations and private `main` are absent from ordinary lookup.
Public generic declarations are discoverable for source updates without executable
applications; declaration discovery alone grants no typed call authority. Binding
requires an existing executable target and never specializes a template at runtime.
A concrete public script wrapper can supply an ordinary typed entry.
Explicit public declaration/application binding remains available for installed
native entries and other existing checked executable evidence. Both binding routes
validate the complete Rust signature. Named bindings retain a checked live route,
not an obsolete executable lease; invocation selects and admits the exact current target.

Prepare `Field<T>` and `Method<A, R>` once from the installed applied type, then
reuse them. Native and script inherent members share declaration/name binding;
method tuples exclude the receiver. Associated functions return `Function`.
Generic binding consumes installed executable evidence and retains each supplied
type's lexical scope; it does not infer new implementations at runtime. Interface
and selected calls retain their checked implementation identity and evidence,
including inherited, default and static members, while ordinary calls use its current
body. Handles reject foreign runtimes and incompatible applications or layouts.
Created closures/Futures and admitted frames/queues retain exact bodies; their ordinary
inner calls are current. Retaining a named function value does not pin old code.

`Runtime::with_context(owner, execution, |cx| ...)` opens SDK host access
with cancellation, execution phase and backend policy. Field reads/writes and
method/closure calls share that scope; native callbacks receive it automatically.
No metadata or payload borrow spans script reentry. See the executable
[host_objects example](../../crates/kagari/examples/host_objects.rs).

Safe `Opaque<T: NativeData>::read_data`/`edit_data` require installed fixed,
reference-free data capability; `native_data!` verifies every declared field.
ManagedStorage provides safe field tokens, complete builders and checked writes;
its `edit_data` lends only the fixed data portion. General `Opaque<T>::read`/`edit`
are unsafe provider loans with `StorageAccess`, retained extraction and preflighted
`EditPlan` accounting/revisions. They are not a safe arbitrary mutable reference
API. No payload reference may escape a loan or span callbacks/collection. See the
[provider protocol](host-interop.md#general-native-payload-protocol).

Ordinary registration uses `define_opaque::<T>(name)?`. Create instances through
`NativeType<T>::create(cx, payload, selected_slots)`; typed constructors obtain the
descriptor with `cx.result_native_type::<T>()?`. Use `&[]` without stored selections;
nonempty slots require current checked native-call evidence. Bounded host binding
uses [verified application occurrences](host-interop.md#native-application-admission),
not a runtime trait resolver.

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
host callbacks use `Function`. CallContext, borrowed native views and traced
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

The host supplies a validated `PreparedProgram` candidate for an existing live
`Program`, not a label or raw module id. `reload_program` returns `ReloadResult<()>`;
the same live handles observe the new publication. The SDK retains the candidate's
exact verified identity during staging and publication.
The runtime validates:

- public ABI fingerprints
- type identities and layouts
- interface tables
- host registrations
- typed path descriptors
- bytecode verifier metadata
- JIT artifact dependencies, if JIT is enabled

Publication requires a serialized host-top-level boundary with no active synchronous
script, driver, callback or reentry scope; parked executions may remain. The
[activation contract](module-activation.md) also requires frozen declarations,
retained executable demands and unchanged direct static initializers with preserved
checked bindings. Hash/Eq bodies and ordinary initializer helpers may change.
If validation succeeds, code, dispatch and source association advance atomically.
If it fails, the existing publication and static state remain unchanged. See
[compatible declaration changes](module-activation.md#compatible-declaration-changes)
for allowed additions and frozen contracts. This API has no state snapshot, migration
or Actor state-replacement mode; general Serde conversion remains independent.

## Source updates

With the SDK `source` feature, `kagari::update::Update` builds a detached batch;
`Runtime::update(&Program, Update) -> ReloadResult<()>` checks and publishes it.
The builder does not compile or publish. Its operations are:

- `replace_function(target, definition)`: one complete authored definition, not a
  body fragment. `target` is a checked public path or an opaque `SourceDeclaration`.
  The replacement stays in its original owner/import context and cannot rename the
  target or change a frozen callable contract. A locator can address private
  functions, methods and named locals without granting permission to call them.
- `replace_file(name, text)`: replace a captured physical document, preserving its
  module mapping. Names normalize against the captured source root without IO.
- `add_file(module, name, text)`: add a document with an explicit `ModuleIdentity`
  in an already captured package, allowing a new module in the candidate closure.
- `with_cancellation(token)`: choose a submission cancellation token.

For an existing `program` with a replaceable public `answer` declaration:

```rust,ignore
use kagari::update::Update;

let mut update = Update::new();
update.replace_function("answer", "pub fn answer() -> i32 { 42 }");
runtime.update(&program, update)?;
```

This illustrates submission; admission still checks frozen declarations, local
static initializers, executable validity and retained demands. The standard
foundation does not impose a global `Unknown` body freeze. Hash/Eq bodies and
ordinary helpers called by static initializers are eligible for normal body updates;
their callers' user-level semantic obligations remain the application's responsibility.
Retained compiler-generated tuple protocol bridges currently have a source-demand
preparation limitation; see the [review finding](../review.md#generated-tuple-bridges-in-source-updates).
A compatible body change may still fail preparation there without publishing anything.

One batch is atomic and limited to 1,024 edits and 64 MiB of replacement UTF-8.
Empty batches, duplicate targets (including aliases), overlapping edits, invalid
mappings and incomplete definitions reject. Submission rechecks one complete detached
candidate with retained demands; it performs no filesystem discovery, dependency
acquisition, deletion, initializer execution or host callback. Unchanged captured
inputs retain their identities, and engine/editor source sessions are not mutated.
The complete resulting physical documents and aggregate retained text are also
preflighted against the 64 MiB source limit before allocation/replay, counting
unchanged inputs, replacements and additions. Checked UTF-8 ranges/overlap and
arithmetic are independent of edit order; copying observes cancellation. A fresh
submission token is used unless explicitly supplied, not a stale preparation token.

A small edit can still require complete candidate preparation; the API makes no
constant-time patch or minimal-rebuild promise. Source batches compile in process
for interpreter/optional JIT execution, not AOT patch delivery. Native preparation
remains explicit, with exact-product validation and pre-entry fallback under the
[JIT contract](jit.md); no hidden backend callback is retained in Runtime.

Source preparation retains an immutable compilation capsule: physical bytes and
origins, module/package mappings and declaration environment, selected root,
acquisition provenance, compiler identity and preparation settings. The SDK associates
it with the **exact publication token**, before core commit, and rolls back the
association on failure. A later low-level reload without its own capsule makes source
update fail with `MissingCompilationContext`; it never reuses an older SDK baseline.
An artifact's retained source package supplies diagnostics, not this compilation
capsule, even when all visible source text is present.

## Exact source inspection

Runtime inspection works without source analysis. The SDK forwards to the core
`kagari_runtime::source` owner:

| API | Selection and retention |
| --- | --- |
| `source_snapshot(&Program)` | Captures one current `ProgramSourceSnapshot`, including its publication and optional verified source package |
| `snapshot.declarations()` / `snapshot.function(&declaration)` | Opaque authored locators and definitions from that snapshot; stable locators can resolve in a later publication of the same installation |
| `function_source(&FunctionSourceTarget)` | Selects a live route now, or reads a copied exact observation without selecting newer code |
| `value_source_target(&RootedValue)` | Captures the observed callable/Future body without polling or claiming it |
| `frame_source_target(&ExecutionFrame)` | Captures the exact body from the active session's safe frame view; copied public fields are not frame authority |

Callable/member `source_target` APIs derive checked current routes from ordinary
named handles, including bound interface implementations; an exact closure handle
instead supplies its retained observation. These APIs do not repeat public-name
or trait implementation lookup. An unbound interface member has a declaration but no
observed implementation body and reports `NoObservedBody`. A value observation is an
exact snapshot of what was observed, not a new live binding.

`SourceQuery::Available` provides the complete authored definition, display URI,
UTF-8 byte range, fingerprint, exact `publication`, `last_source_change`, and known
executable/application provenance. Publication identity and last text change are
separate: unchanged text can be present in a newer executable publication. Generated
closure/resume/adaptor bodies associate with their authored owner, not fabricated
standalone source. Native/intrinsic origins never masquerade as script text;
`SourceQuery::Unavailable` preserves known identity/provenance and a reason such as
`NativeOrigin`, `MissingPackage` or `NoObservedBody`.

Snapshots and exact observation tokens retain immutable diagnostic metadata only:
no executable lease, GC root, private-call authority or filesystem text fallback.
Hosts can retain old source after the old computation/code is gone. Runtime queries
still validate runtime identity/liveness. Equal display URIs in distinct logical
modules do not collapse module/declaration identities. Source retention is independent
of optional local/debug metadata and retained MIR; see
[artifact source packages](artifacts.md#retained-source-packages). Current-source
inspection captures selection at query time, not proof that a later call ran that
body. Multi-frame tasks use their available safe frame observations rather than a
fabricated single function; a consumed/composed Future may have no observed body.
Runtime keeps no automatic unbounded source history. Hosts retaining historical
snapshots own their count/byte budget, independently of executable lifetime.

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

With `native`, `Runtime::prepared_program(&Program) -> RunResult<PreparedProgram>`
creates a fresh preparation-side cache over the installation's current verified
bytecode and optional MIR. After a source update, use it with explicit native
preparation; Runtime retains neither that cache wrapper nor a codegen backend.
Native products must match the selected body/version/configuration, never just a
stable callable identity. Native preparation failure leaves code, statics and source
unchanged; it does not undo an already successful source publication.

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
Hosts may run multiple isolates on different Rust threads. The core runtime, `Vm`
and SDK `Runtime` are Send and not Sync. A host can move a quiescent runtime, or own
it across message-receive awaits in a Send task; synchronous calls and scoped
borrows finish before transfer. Native payloads/observers require Send, while
shared registrations require Send + Sync. Thread-affine services stay outside the
runtime and use host message/ID boundaries.

PreparedProgram remains preparation-side Rc ownership with a RefCell native cache;
it is neither Send nor Sync and is not retained by Runtime. Load/install code
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

## Typed async calls, scheduling and native waits

`kagari::runtime::Runtime` is the SDK execution facade, constructed by
`KagariEngine::runtime(context: ExecutionContext)`. `default_context()` exposes its
chosen default; per-operation overrides do not replace it. Bind once with
`function::<A, R>(&module, name)` or `async_function::<A, R>(&module, name)`.
The latter returns `kagari_runtime::native::function_handle::asynchronous::AsyncFunction<A, R>`:
a checked callable and the retained lexical output scope of exactly one `Future<R>`
layer. Named bindings select current implementations at invocation admission;
created Futures and admitted queues retain the selected factory/body and output scope.
Callable binding takes `&self` and requires
`A: KagariArguments + 'static, R: KagariType + 'static`; binding checks type mappings,
whereas invocation requires value conversions. All binding operations return `RunResult`.

- `call(&mut self, &Function<A, R>, A) -> RunResult<R>` uses the default context.
- `call_async(&mut self, &AsyncFunction<A, R>, A) -> RunResult<Call<'_, R>>`
  admits a cold invocation and returns an exclusive borrowing, driving Future.
- `submit_async(&mut self, &AsyncFunction<A, R>, A) -> RunResult<PendingCall<R>>`
  admits the same invocation but returns a nonborrowing, result-only Future.
- Each call has a `_with_context` variant with a final `&ExecutionContext` parameter.

Synchronous invocation requires `A: IntoKagariArguments, R: FromKagari`.

`Call` and `PendingCall` live in `kagari::runtime::call` and yield `RunResult<R>`.
Async outputs require `FromKagari + Send + 'static`, not Sync. Arguments require
`IntoKagariArguments`, without a new Send bound: conversion/rooting happens at
admission, under the invocation's original context, without executing its factory.
Completion conversion uses the retained output type scope in that same session;
publication follows local cleanup. A completed result survives runtime teardown,
but retained objects still require their live owning runtime for access.

Awaiting `Call` drives the common bounded scheduler without a second pump. The Call
holds an exclusive runtime borrow: `.await` consumes it, but a manually polled Call
still holds that borrow after Ready until released. Dropping an unfinished
Call synchronously retires that invocation and structured descendants, without a
subsequent poll or a remote IO acknowledgement. Dropping/cancelling `PendingCall`
requests cancellation; `PendingCall::cancel(&self)` and `is_finished(&self)` expose
control/observation without driving. The application must keep driving Runtime for
cleanup, or close/drop it. Runtime teardown resolves unfinished tickets. Cancellation
does not propagate back into a shared host context token. Cleanup faults quarantine the runtime.

`Runtime::poll(&mut self, &mut Context<'_>) -> Poll<RunResult<()>>` drives one shared
bounded turn across invocations, structured branches and managed scope Tasks.
An open idle runtime returns Pending; `close() -> RunResult<()>` is idempotent and
synchronously stops admission and cleans up without executing script bodies. A clean
closed runtime polls Ready(Ok(())). A fatal error drains remaining work before
terminal failure. Register a reliable coalescing host Waker and
poll on notification, not a per-Task loop. A temporary Call route preserves and then
restores the external driver's route on completion/drop, notifying remaining work.
Host adapters must explicitly handle disconnection; Waker cannot report dropped
messages. Wakes never execute script, including completion from another thread.

`create_task_scope(&mut self, &module)` uses the default context; its
`create_task_scope_with_context(&mut self, &module, &context)` variant takes an explicit
context. Both return `RunResult<TaskScopeOwner>`. Pass `scope.handle()` through a
typed call as `kagari_runtime::task::value::TaskScope`. Script `scope.spawn(factory)`
admits without executing the factory. Shared scheduling automatically retires terminal
admission slots; retained script Task handles still cache their result for multiple
awaits. `take_task_event(&self) -> Option<TaskEvent>` returns bounded diagnostics
from `kagari_runtime::scheduler::tasks`: `Failed(Box<TaskFailureEvent>)` holds task,
scope, origin and failure; `Suppressed { count: u64 }` follows buffered failures on
overflow. Script-observed failures are included; successful outputs (including business
Err) are not. Ignoring events does not hold admission capacity.
`TaskScopeOwner::close`/drop requests cancellation; Runtime polling or close acknowledges
cleanup. Keeping a capability or dropping a script Task does not change owner lifetime.

For retained objects and interfaces, binding uses `&self`; access/invocation uses
`&mut self`:

| Binding | Access or invocation |
| --- | --- |
| `object_type(&module, name)` | Resolve a public nongeneric `ObjectType`; retained Objects also supply applied generic descriptors |
| `field::<T>(&ObjectType, name)` | `get(&object, &field)` / `set(&object, &field, value)` |
| `method::<A, R>(&impl InherentType, name)` | `call_method(&object, &method, arguments)` |
| `associated_function::<A, R>(&impl InherentType, name)` | `call(&function, arguments)` |
| `async_method::<A, R>(&impl InherentType, name)` | `call_async_method` / `submit_async_method` with object, method and arguments |
| `async_associated_function::<A, R>(&impl InherentType, name)` | `call_async` / `submit_async` with function and arguments |
| `interface_method::<A, R>(&interface, name)` | `call_interface_method(&interface, &method, arguments)` |
| `async_interface_method::<A, R>(&interface, name)` | `call_async_interface_method` / `submit_async_interface_method` with interface, method and arguments |

Callable/member binding uses the same A/R bounds as function binding; field binding
requires `T: KagariType + 'static`. Field get requires `FromKagari`, set requires
`IntoKagari`. Method invocation uses the same synchronous/async conversion bounds
as free calls. Interface binding requires an existing script-provided `Interface`,
not an arbitrary Object. Every access/invocation operation above has a
`_with_context` variant with final `&ExecutionContext`, preserving cancellation,
deterministic inputs, recording, observers and backend-policy checks. There is no
arbitrary Object-to-trait cast or implicit specialization. Concrete
member instantiations must already have checked executable evidence in the program.
The [host_objects example](../../crates/kagari/examples/host_objects.rs) executes
binding, object round trips, direct await, submission and exact admitted computation
retention across reload.

Typed interpreter calls/accessors, async admission and scope creation require
`JitPolicy::Disabled`; native execution retains its explicit preparation and
`execute_prepared` path. Native preparation rejects resume bodies before compilation. Synchronous execution/reentry never drives a suspendable
body, even when its operation could complete immediately. Native producers registered
using `Async<A>` remain cold until await, receive owned inputs/completion endpoints,
and convert completion data on the driver. Native calls remain cooperative and may
overrun a slice.

For backend tooling, `Runtime::backend()` exposes `kagari_vm::vm::Vm`;
`ExecutionContext::interpreter_options()` validates interpreter policy and preserves
inputs. Low-level `Vm::{start, start_future, drive, drive_task}` and core runtime
manual scopes, reports and retirement primitives remain at their backend owners.
They are not SDK aliases or the normal async facade. A low-level OwnedExecution drop
requests retirement rather than promising Call's synchronous cleanup contract.

The [async_tasks example](../../crates/kagari/examples/async_tasks/main.rs) uses typed
scope admission and a coalesced host event loop with two native providers. Its decoded
artifact also executes without SDK source/native features.
