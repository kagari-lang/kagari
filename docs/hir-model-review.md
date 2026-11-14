# HIR Model Review Notes

**Status:** the storage/attribute design is in HA00 implementation; the boundary proposals
in this review remain deferred. No Native/tool-crate redesign or dependency change is activated.
**Baseline:** `main`, `43d55706a` (Runtime refactor merged in PR #61).
**Working branch:** `design/hir-native-model`.
[Roadmap](implementation-roadmap.md#hir-storage-and-attribute-refactor) owns activation;
[current HIR](architecture/hir.md) and [Runtime](architecture/runtime.md) describe implemented behavior.

## Selected storage and attribute scope

[HIR storage design](hir-storage-design.md) owns the target graph, caller-owned arena and
borrowed-result lifetimes, owner-local identity, collection naming, source sites and attributes.
HA00 implementation has started; the boundary proposals below remain deferred.

## Compiler and tooling boundary

**Preferred direction, for later review:** keep one compilation semantic core; move
editor-specific derived data into a tool-semantic crate, not directly into the LSP server.
Crate name is undecided; no new crate, dependency or feature is activated.

```text
syntax → HIR / shared semantic checks
          ├─ compiler → owned MIR / bytecode
          └─ tool-semantic crate → navigation / completion / reference indexes
                                  └─ LSP protocol, document sync and request scheduling
```

| Shared semantic core | Tool-specific processing |
| --- | --- |
| Declaration identities, scopes, resolution, types, selected members/calls and constraints | Position-to-target indexes, completion organization/ranking, references and rename preparation |
| Recovery facts, structured language diagnostics and necessary source locations | Interactive queries over incomplete source and editor-facing results |

- Tools consume checked/recoverable facts and shared semantic lookup APIs; do not implement
  another resolver, type checker or visibility policy. Optional lint rules keep their owner.
- Build derived tool indexes on demand. Features may compose optional crate dependencies,
  but enabling a feature is not permission to prepare tool data on every compilation.
- Confirmed candidate: [type_hits](../crates/kagari-hir/src/analysis/type_targets.rs) builds
  navigation provenance unconditionally during new FileAnalysis publication. Separating it
  must preserve semantic checking and diagnostic sites. No cost percentage/speedup is measured.
- Compilation still checks all bodies in the selected module/dependency closure. Sharing
  diagnostics does not require compilation to produce every editor index or final LSP message.
- Tool indexes need explicit analysis-version, invalidation and HIR-retention contracts.
  Crate separation/external arena ownership alone does not solve LSP reclamation. The storage
  design selects module-lowering regions; safe cache/request handles and cross-version reuse
  remain open. Do not feed edits into one unbounded project-wide Bump.

This separation is not part of the current storage/attribute cutover. Audit shared consumers
before moving data; a source location or semantic fact is not tool-only merely because LSP uses it.

## Deferred LoweredModule boundary

LoweredModule should describe structural HIR and origins, not library configuration or
checked contracts. Its physical snapshot shape follows the storage design. Optional source
text/direct Rust authoring and the Native field redistribution below are not part of that cut.

Module owns ordinary declarations and owner-local node/attribute indexes. Each function
owns its implementation descriptor; each nominal type owns its representation descriptor.
HIR descriptors contain unresolved type references, not the checked Ty records below.
Source and Rust authoring use the same declaration identities and resolution pipeline.

| Current root data | Target owner |
| --- | --- |
| `source`, `module` | LoweredModule borrowing the external arena |
| `source_map` | Owner-local source indexes |
| `attributes`, attribute target matching | Owner-local attribute lists keyed by node identity |
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
