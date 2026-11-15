# HIR Model Review Notes

**Status:** HIR work is narrowed to responsibility/identity cleanup on current owned ID tables;
the arena/reference migration is withdrawn. HR04–HR08 compiler/tool separation is authorized.
Native registration redesign and cache-retention changes remain deferred.
**Baseline:** `main`, `43d55706a` (Runtime refactor merged in PR #61).
**Working branch:** `design/hir-native-model`.
[Roadmap](implementation-roadmap.md#hir-storage-and-attribute-refactor) owns activation;
[current HIR](architecture/hir.md) and [Runtime](architecture/runtime.md) describe implemented behavior.

## Selected storage and attribute scope

[HIR responsibility and identity cleanup](hir-storage-design.md) preserves current category
storage and ID links. Parameters now address Function.params and Module owns typed attribute
attachments, without an arena, borrowed graph or universal owner table. The compiler/tool
boundary audit below guides the active HR04–HR08 migration; Native proposals remain deferred.

## Compiler and tooling boundary

**Inventory at c96148ea4; HR04–HR08 migration authorized.** Keep compilation facts and semantic rules
in kagari-hir; derive editor products outside it. This supersedes the earlier new tool-crate
sketch: prefer the existing kagari-analyzer package, with a protocol-neutral library surface
for protocol-neutral editor queries. Its binary continues to handle LSP lifecycle/document sync;
compiler, SDK and HIR production targets must not depend on it.

```text
syntax + source + installed declarations
                ↓
kagari-hir: structural HIR + resolution/type facts + diagnostics + checked admission
   ├─ kagari-compiler: CheckedCompilation → MIR / bytecode
   ├─ kagari-lint: optional rules, usage summaries and attribute policy scopes
   └─ kagari-analyzer: IDE queries + demand-built indexes
                        └─ LSP messages, client versions and request lifecycle
```

### Retain in HIR

| Current owner | Compilation/shared responsibility |
| --- | --- |
| hir/, lower/ | Owned declarations/bodies/type syntax, typed IDs, attributes and structural origins |
| imports/, resolver/, declarations | Imports, namespaces, visibility, binding/definition identities and captures |
| types/, typeck/, aggregates/, semantic/ | Inference/constraints, resolved types, fields/calls/conversions, constants, layouts/contracts and positive checking evidence |
| analysis/ | Shared staged preparation/results, exact input validity, cancellation, signature/body checking and fact remapping; not LSP request state |
| program/, compile_input/ | Fixed checked dependency closure and compiler admission of matching nodes/facts/environments |
| source_map | Diagnostic/debug/reuse/admission origins; split navigation-derived fields, not the whole map |
| source_inspection, update_source | Authored declaration extents, source-package capture and checked static-initializer reload evidence |

Recovery and diagnostics remain core: bad source cannot confer compilation authority, but
useful semantic facts may survive errors. AnalysisSnapshot/SourceSnapshot and cancellation
are not LSP-only concepts. typeck/completion means control-flow completion, not code completion.

### Move editor projections

Paths below are under crates/kagari-hir/src; destinations are proposals, not existing APIs.

| Current symbols | Proposed destination | Consumer evidence / seam |
| --- | --- | --- |
| FileAnalysis type_at, definition_at, field_at, source_function_at, member_receiver_type, visible_bindings; signature/body offset adapters | analyzer IDE queries | SDK examples/tests consume them; no compiler/lint production calls found. Keep node-keyed facts and identity lookup in HIR. |
| AnalysisSnapshot analysis_at/definition_at and queries/namespaces | analyzer navigation | Physical offset → logical inline module → semantic target; preserve Type/Value namespaces, shadowing and import origins. |
| queries/calls | analyzer signature help | Position selection and presented parameter names differ from compiler AppliedCallSignature, which stays core. |
| queries/documentation | analyzer documentation | Markdown/written-signature composition; SDK/stdlib tests consume it. Registered docs and source origins stay with their owners. |
| queries/methods | split semantic eligibility from analyzer completion | Completion ordering/deduplication/context selection moves; receiver substitution, trait/bound and visibility rules remain shared semantic operations. |
| FileAnalysis.type_hits + analysis/type_targets::type_hits | demand-built analyzer NavigationIndex | Eagerly built in analysis.rs for every new full-file result; only namespace/navigation queries read it. Move construction, not just accessors. |

Neither hover composition nor a complete references/rename index is implemented in the
current analyzer. Do not describe future editor features as existing coverage. SDK/compiler
must not acquire an LSP dependency; move editor examples/tests or use explicit adapters.

### Split mixed storage before moving it

| Current storage | Retain core | Derive outside HIR |
| --- | --- | --- |
| ResolvedNames scopes/binding_sites | Canonical binding identity/name/rebinding policy, captures and resolver work state needed by checking/emission | Persistent byte-offset visibility/shadow history for a ScopeIndex |
| Declarations targets/identities/location/site_ranges | Definition/generic identities and authoritative source origins | DeclarationSiteIndex and cursor selection; preserve analysis coordinates versus copied/native physical locations |
| ResolvedNames lookup_hits/path_hits; ModuleImportFacts.path_hits | Selected resolution, imports, dependency/support facts and diagnostics | Prefix navigation hits, built from settled semantic facts |
| SourceMap type/expr/import prefix paths | Origins required for diagnostics, debug, admission and reload | Joined-prefix expansion and position indexes; review individual fields before removal |
| TypeTable.type_refs / ResolvedTypeRef.target | **Keep resolved declaration targets.** Alias expansion, generic/Self constraints and reload evidence use them. | Position-to-TypeRef/target lookup, not the semantic relation itself |

Import-prefix collection currently also records solver observations and participates in
checkpoint equality. Separate navigation after import settlement without weakening watcher/
convergence/cancellation behavior. This is not a safe field deletion. Likewise, scope history
currently backs compiler binding lookup: first establish an identity-keyed binding directory,
then extract editor visibility history. Do not move either complete subsystem blindly.

### Move lint projections to lint, not LSP

- queries/usage::binding_usage and BindingUsage feed kagari-lint rules.rs in production.
  Lint owns read/write counts and unused/unnecessary-var projections; HIR retains selected
  bindings, captures, writeability and positive body/static eligibility.
- queries/attributes::ScopedAttribute/scoped_attributes feed kagari-lint policy.rs.
  Lint owns lexical suppression/inheritance projection; Module.attributes and typed targets,
  source ranges, language-role checks and Native attribute admission remain core.

### Ownership sketch

```text
HIR semantic result                    IDE view (outside HIR)
├─ matching source/HIR and definition scope  ├─ reference to / owner of that result
├─ resolution/type/call/constraint facts     ├─ NavigationIndex / DeclarationSiteIndex
├─ diagnostics and checking evidence        ├─ ScopeIndex
└─ checked compiler projection              └─ completion/hover/docs/reference products
```

Raw facts are queried by node/declaration ID; editor services select a node from a cursor.
For value.method(), HIR determines receiver type and eligible/selected methods. The analyzer
selects the cursor context and presents candidates. LSP converts results to protocol objects.
Start with demand-built/query-local indexes; cached indexes must retain exact semantic owners
and validate provider/context/lowering inputs, not just file revision. No new VFS/line index:
kagari-source already owns text, revisions, provenance and position conversion. Client document
versions, request IDs, scheduling and stale-response policy belong to the analyzer session.

### Cache policy is separate

Keep owned parse preparation, distinct declaration/signature records and FunctionFacts versus
query context. An unchanged body can share facts while a new invalid dependency changes its
eligibility; returning an old whole query would be wrong. This remains true without arenas.

Weak-only graph retention is current policy, **not a required semantic boundary**. It can force
re-lowering/checking after callers drop results, including Native modules. Revisit bounded
strong module/result caching independently: define retained units, invalidation/eviction and
clone behavior first. Do not move semantic validity/remapping into LSP or blindly revert the
record/context split. No cache-policy change or measured speedup is claimed by this review.

### Naming candidates

| Current | Candidate / action | Reason |
| --- | --- | --- |
| HirArenaId | LoweringId | Identifies a lowering, not a memory allocator; changing/removing checks is separate. |
| TypeTarget / target's navigation-only comment | ResolvedTypeTarget / semantic declaration-target wording | The relation participates in checking and reload, not just navigation. |
| PreparedAnalysis | PreparedSignatures | Internal bundle is specifically the signature stage. |
| FileAnalysis.type_hits | NavigationIndex.type_path_hits outside HIR | Names owner and purpose; remove eager core preparation. |
| BindingInfo / MethodCompletion | VisibleBinding / CompletionCandidate in analyzer | Editor products, distinct from core binding/member facts. |
| update_source | source_evidence, if renamed | Extracts authored/reload evidence; does not edit or publish source. |

Do not mechanically rename AnalysisDatabase/AnalysisSnapshot as LSP concepts. FileAnalysis
actually represents logical modules, while AnalyzedModule already names its payload; settle
that wrapper/payload boundary before considering ModuleAnalysis. BodySignatures normalization
naming is optional, not necessary for the editor extraction. No broad rename is activated.

### Implementation plan and preservation

The existing [HIR plan](hir-storage-design.md#compilertool-implementation-plan) now owns
active HR04–HR08: independent lint extraction, one coherent IDE API cutover, binding/scope
separation, path/origin cleanup, then focused naming and final integration. The IDE query
closure moves together because documentation/navigation/completion call shared offset helpers.
Existing mixed metadata is a bounded intermediate input, removed by HR06/HR07; it is not a
compatibility API or completed separation.

Move editor assertions to their new owner; preserve mixed tests' semantic halves. Retain
incomplete-source, copied/native origins, UTF-8/CRLF, inline routing, dual namespaces, equal-glob
origins, cancellation, old snapshots, capture/eligibility, exact Native admission and source-free
compiler/reload contracts. Actual validation is recorded in the active plan.
HR04 lint extraction is implemented; cache-policy changes remain outside this migration.

Mechanism reference: [rust-analyzer architecture at Rust 5f4effd57950](https://github.com/rust-lang/rust/blob/5f4effd57950/src/tools/rust-analyzer/docs/book/src/contributing/architecture.md)
separates the resolved hir facade, IDE offset/label services and LSP transport. Adopt that
responsibility distinction, not its ECS/Salsa/query framework or crate count. Unlike that
IDE-only semantic frontend, Kagari must also preserve its checked compiler/reload handoff.

## Deferred LoweredModule boundary

LoweredModule should describe structural HIR and origins, not library configuration or
checked contracts. Its physical snapshot shape follows the storage design. Optional source
text/direct Rust authoring and the Native field redistribution below are not part of that cut.

Module owns ordinary declarations in its category tables. Attribute targets use declaration
identity without requiring a universal owner table. Each function owns its implementation
descriptor; each nominal type owns its representation descriptor.
HIR descriptors contain unresolved type references, not the checked Ty records below.
Source and Rust authoring use the same declaration identities and resolution pipeline.

| Current root data | Target owner |
| --- | --- |
| `source`, `module` | Existing owned LoweredModule |
| `source_map` | SourceMap keyed by structural identities, not their allocator |
| `module.attributes` | Module-owned typed target identities and attached attribute lists |
| `native_functions`, `method_policies`, `native_types` | Function/member/type declarations; no parallel Native tables |
| Dependencies, aliases, prelude, import namespaces | Common library/module input and resolution environment |
| Registered declarations/traits, definition scope, foreign trait requirements | Common declaration/contract context and its checked library view |
| Enum failure markers, foundation roles, conversion adapters | Explicit declaration facts and language/conversion checking owners |
| Registration flag, allowed Native attribute byte ranges | Input admission/origin and normal attribute validation, not root-level authority flags |

Rust callbacks, payload descriptors and prepared call links remain outside HIR.
Executable demands, including externally constructible generic types, derive from admitted
contracts and uses rather than an input-provenance boolean. No source-authoring spelling
alone grants Host authority.

## Native model

One language declaration/checking model for source and optional Rust builders.
**Replace old APIs/layouts directly; no compatibility readers, entry-ID bridges or
Native-only declaration tables.** Preserve semantics and safety, not old structures.

### What the branch changes

| Implemented boundary | Consequence for this design |
| --- | --- |
| One exclusive Runtime; immutable Program preparation is detached | Provider metadata/preparation must not own another Runtime or require HIR during installation. |
| `NativeCall` borrows `&mut Runtime`; arguments/results stay source-protected | A callback stages a result, not an escaping unrooted Value. |
| `kagari-gc` owns physical object storage and collection | Native type binding is not an object pool or another tracing system. |
| Collection-specific layouts/codecs removed | Vec/Map/Set are ordinary library payloads, not compiler-recognized representations. |

### 1. Common declarations

Schematic semantic records, not compiling Rust or final APIs. Ordinary fields are omitted;
unresolved HIR need not use checked types. Descriptor layouts/crate placement remain open.

```text
struct Function {
    id: DefinitionId,
    // Name, visibility, generics, parameters, result and bounds.
    implementation: FunctionImplementation,
}

enum FunctionImplementation {
    Required,
    Script(BodyRef),
    Native(NativeFunctionContract),
}

struct NativeFunctionContract {
    receiver_access: Option<ReferenceAccess>,
    host_call: Option<HostCallContract>,
    callable_requirements: Vec<NativeCallableRequirement>,
    concrete_result: Option<Ty>,
}

struct NativeCallableRequirement {
    receiver: Ty,
    interface: NominalTy,
    member: DefinitionId,
    arguments: Vec<Ty>,
}

struct TypeDeclaration {
    id: DefinitionId,
    // Name, generics, bounds, fields and variants.
    representation: TypeRepresentation,
}

enum TypeRepresentation {
    ScriptManaged,
    NativePayload,
    ExternalHost(HostTypeContract),
}
```

- Function declaration ID is the binding identity. Trait impls, default/override selection
  and constructors remain ordinary language declarations; a Provider cannot invent them.
- `receiver_access` is the required scoped payload loan for member access. Explicit reference
  parameters remain in their types. `host_call` describes external argument passing/effects,
  not the Rust callback ABI. Validate supported combinations; the old mutually exclusive
  Value/ReceiverLoan/Host enum incorrectly mixed these responsibilities.
- Requirements name operations used by Rust algorithms; bounds prove capability. Compilation
  selects witnesses. `concrete_result` needs proof of conversion to the public result.
- `NativePayload` describes nominal type representation, not Rust layout/scanning. Vec is
  an ordinary generic payload; Host objects remain external. This is not a classification
  of every language type. String/builtin policy is separate; no catch-all Intrinsic branch.

### 2. Library contracts and bodies

```text
struct CheckedLibrary {
    contracts: Arc<LibraryContracts>,
    bodies: CheckedBodies,
}

struct LibraryContracts {
    definitions: DefinitionContext,
    declarations: CheckedDeclarations,
    host_paths: HostPathContracts,
}
```

Declarations retain signatures, implementation descriptors and body identities, not owning
HIR/code bodies. Reserve identities before full checking; representation/access inputs must
exist before checking uses them. Imports/exports, dependencies, docs and prelude policy are
common module metadata; language roles/conversion derivation remain semantic policy.

Generic arguments and selected operations belong to checked calls, not implementation kinds:

```text
trait default body → helper<Self, T, Self::Iter>(self)
helper implementation → Native(contract)
```

No NativeDefault recipe. Ordinary checking proves substitutions, projections, constraints and
results. Forwarding elimination must preserve effects and reload/provider selection.

### 3. Rust implementations

```text
struct NativeProvider {
    contracts: Arc<LibraryContracts>,
    functions: Map<DefinitionId, NativeFunctionBinding>,
    payloads: Map<DefinitionId, RustPayloadBinding>,
    paths: Map<HostPathId, HostPathBinding>,
}

struct RustPayloadBinding {
    rust_type: RustTypeId,
    provider: GcProviderBinding,
    access: PayloadAccessAdapter,
}
```

Keys name exact functions, nominal payload types or declared paths in the retained contract
context. HostPathId is context-qualified; its layout is undecided. Bindings contain actual
callbacks/codecs/access adapters. Provider contracts retain neither HIR nor whole Programs.
Captured mutable runtime state stays Runtime-local; only suitable immutable bindings share.

`GcProviderBinding` connects allocation to concrete GC descriptors: physical layout, scan,
accounting and drop follow the GC provider protocol. It is not a second copy of those hooks.
Rust payload implementations own their data/edges; Runtime owns heap driving, loan admission,
mutation publication and revision tracking. Constructors are ordinary Native functions.

### 4. Linked calls and active calls

```text
struct LinkedNativeCall {
    binding: NativeFunctionBinding,
    signature: ScopedSignature,
    mapping: PreparedMapping,
    operations: SelectedOperations,
    result_adapter: Option<PreparedResultAdapter>,
}

struct NativeCall<'call> {
    runtime: &'call mut Runtime,
    owner: &'call ExecutableModule,
    target: &'call LinkedNativeCall,
    arguments: ProtectedArguments<'call>,
    result: PendingResult<'call>,
    scratch: NativeScratch<'call>,
}
```

Linked calls belong to recipient publication/application records, not shared declarations.
`NativeCall` is a temporary exclusive activation, not a stored callback object. It borrows
protected operand locations; results publish to the caller before Native scratch retires.
Actual body/application dependencies require managed source edges; Arc backing alone is not
a root. Physical loans end before GC/reentry; all exits clean up the admitted suffix.
Escaping values use explicit Runtime roots; borrowed views cannot escape the call.

For `Vec<T>::len`, the declaration supplies its signature/loan contract, Provider supplies
the Rust adapter/payload binding, and the linked call supplies applied types/conversions.
Invocation reads the protected receiver and stages a scalar result; it does not analyze Vec.

### Ownership summary

| Data | Owner |
| --- | --- |
| Signatures, generics, traits/impls, representation/access contracts | Common declaration model; analyzed view in LibraryContracts |
| Script/default bodies and checked call facts | CheckedLibrary / compilation |
| External imports and independent type/operation/result proofs | Immutable compiled Program |
| Rust adapters and language-type → payload bindings | NativeProvider |
| Concrete object storage, scanning and reclamation | Runtime-owned kagari-gc heap using concrete providers |
| Current selections, linked Native calls, actual authority | Runtime-local publication/application records |
| Arguments, pending results, scratch and body dependencies | Active/parked execution sources; NativeCall borrows its active suffix |

External imports are derived executable contracts, not a second call-resolution model.
Their layout follows source-free verification needs, not old NativeImport compatibility.
Ordinary named calls use current links; closures/Futures/admitted frames retain selected
bodies/providers. Publication refreshes old bodies' outgoing calls without forwarding chains.

### Preparation and outstanding work

| Stage | Work |
| --- | --- |
| Stdlib build | Analyze fixed declarations/bodies; emit checked templates/indexes; compile Rust adapters. |
| Script compile | Check demanded calls, generic/default applications and operation/result proofs. |
| Detached preparation | Prepare immutable products/adapters independent of recipient state. |
| Runtime install/publication | Admit recipient identities/providers; prepare closed local mappings and final call links atomically. |
| Invocation | Execute prepared adapters; check actual values, loans, generations and cancellation; protect/publish results. |

Do not analyze/prepare unrelated library bodies at startup. Include transitive callbacks,
interface slots, layouts and retained computations in demanded preparation; reject missing
bindings before entering the admitted computation. No runtime inference or trait search.

**Current gap:** this branch still prepares some generic Native applications during execution
(`native/application.rs`). Moving repeated Rust mapping work out of that path while admitting
runtime-supplied environments needs a concrete design/measurement; it is not already solved.
Stdlib prebuild representation and total cold-start costs also remain open.

## Acceptance and scope

- Preserve scoped identity, bounds/borrow/handle checks, roots, completed effects,
  cancellation/trap cleanup, atomic publication and reload selections; invalidate changed caches.
- First slice: a Native function using a Kagari enum, ordinary opaque payload methods and
  a trait impl; include generic defaults, GC/reentry, source-free loading and old snapshots.
- Library collections remain at [NC](native-collections-plan.md#runtime-and-gc-handoff).
  Generic bounded payload construction, dynamic storage/edits and collection restoration are
  unresolved there; this proposal does not mark them implemented or restore paused tests.
- Measure build, compile, prepare, install and first/repeated calls separately, including
  cold processes, repeated runtimes, unrelated-library growth, environment/cache/memory.
- No performance benefit is proven. Runtime GC redesign, JIT/AOT expansion, AST replacement
  and a general query framework are outside this HIR design.

## References

- Branch mechanisms: [Native calls](architecture/runtime/calls.md#native-call-prepared-slots-and-mutable-services),
  [publication/reload](architecture/runtime/reload.md),
  [payload binding](../crates/kagari-runtime/src/native/storage.rs),
  [GC object protocol](../crates/kagari-gc/src/object.rs).
- [rustc HIR 1.91.0](https://github.com/rust-lang/rust/blob/1.91.0/compiler/rustc_hir/src/hir.rs):
  arena references/owner-local indexes; retained snapshots differ.
  [Checkout 5f4effd5795](https://github.com/rust-lang/rust/blob/5f4effd5795/compiler/rustc_hir/src/hir.rs)
  also supplies foreign declarations without script bodies, not Kagari's Host safety model.
- [bumpalo 3.20.3](https://docs.rs/bumpalo/3.20.3/bumpalo/): allocation/destruction contract.
- AngelScript [registration](https://www.angelcode.com/angelscript/sdk/docs/manual/doc_register_func.html)
  / [bytecode](https://www.angelcode.com/angelscript/sdk/docs/manual/doc_adv_precompile.html):
  compile declarations, link implementations; do not copy raw ABI/bytecode trust.
- Wasmtime [imports](https://docs.wasmtime.dev/examples-hello-world.html)
  / [precompilation](https://docs.wasmtime.dev/examples-pre-compiling-wasm.html):
  separate compiled modules from bindings/state, not Kagari's artifact trust model.
  Online manuals reviewed 2026-11-14, releases unpinned.
- [JDK 25 RegisterNatives](https://docs.oracle.com/en/java/javase/25/docs/specs/jni/functions.html#registernatives):
  declaration-to-entry binding, not the complete typed boundary.
- Lua 5.4.9 [libraries](https://www.lua.org/source/5.4/linit.c.html),
  [registration](https://www.lua.org/source/5.4/lauxlib.c.html),
  [C values](https://www.lua.org/source/5.4/lapi.c.html): per-state initialization remains;
  C libraries do not require runtime declaration/type analysis.
- [Prior AST storage study](performance-baseline.md#arena-and-typed-storage-investigation):
  not evidence of a HIR benefit.
