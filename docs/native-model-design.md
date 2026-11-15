# Native model: declarations, HIR and implementations

**Status:** design proposal; no implementation is authorized by this document update.
This replaces the attachment-only draft and consolidates the earlier Native model sketch.
Current behavior remains in [HIR architecture](architecture/hir.md),
[host interop](spec/host-interop.md) and [standard declarations](spec/standard-declarations.md).

**Decision:** Native declarations are ordinary Kagari declarations with an explicit external
implementation and boundary contract. Rust implementations bind separately to checked declaration
identities. Native registration is not another language/type-checking system.

## 1. Inputs and ownership

```text
Kagari source AST                 Rust declaration builder
       |                                  |
 source lowering                 declaration lowering
       +----------------+-----------------+
                        v
             ordinary owned, ID-linked HIR
                        |
        shared resolution / type / trait / impl checking
                        v
                 CheckedLibrary
              /                  \
     LibraryContracts         checked script bodies
              ^
       NativeProvider
   Rust entries / type bindings / Host access adapters
```

| Product | Owner and contents |
| --- | --- |
| Declaration input | Source or structured authoring of modules, types, functions, traits, impls and boundary contracts |
| HIR | Ordinary declaration/category tables, type expressions, implementation kinds, script bodies and origins |
| LibraryContracts | Immutable checked signatures, representation/access contracts, trait/impl relationships and Host paths |
| NativeProvider | Actual Rust entries, type bindings, codecs and access adapters for those contracts |
| Compiled Program | Checked applications, executable external demands and independent type/operation/result evidence |
| Runtime publication | Recipient-local identities, admitted providers and final call links |

A builder may offer declaration-and-binding conveniences, but must produce these separate products.
It cannot establish a trait impl or change a signature merely by supplying a Rust callback.

## 2. Entry into HIR

Both input routes construct the same HIR records and use shared allocation/identity rules.
Keep current Module category vectors, Function-owned parameters and owner-qualified Body tables;
no arena, universal node ID or per-node Arc is introduced.

```text
ModuleInput {
    metadata: ModuleMetadata,       // identity, imports/exports, dependencies, prelude, docs
    declarations: DeclarationInput,
}

DeclarationInput = Source(ParsedModule) | Structured(OwnedDeclarations)

LoweredModule {
    module: Module,
    origins: ModuleOrigins,
}
```

All structures in this document are **schematic target shapes**, not compiling Rust or final API
names. OwnedDeclarations is authoring input, not a second checked declaration database.
ModuleMetadata belongs to the shared module environment, not a Native registration attachment.

- Source lowering translates AST structure; declaration lowering translates typed authoring input.
- Reserve declaration/binder handles before filling references, supporting forward references and
  mutually dependent inputs. Publish matching local-HIR-to-definition mappings during collection.
- Both routes undergo the same namespace, visibility, arity, bounds, trait/impl and role checks.
- Replace the current ModuleDecl's mixed responsibilities: retain an optional typed declaration
  builder, move checked contracts to LibraryContracts and implementation bindings to NativeProvider.
- Do not retain a Native-only declaration/signature table beside the ordinary semantic tables.

### Type references and identity

A typed builder must be able to reference a script-defined type or trait without rendering its
identity into source and parsing it back. Source and structured references differ only in lookup:

```text
TypeHead = Path(Path) | Declaration(DeclarationRef) | Parameter(BinderRef) | SelfType
TypeApplication { head: TypeHead, arguments: [TypeRefId], associated_bindings: ... }
```

This sketches named applications; existing tuple, array, function and projection forms remain.
DeclarationRef carries a validated declaration scope/handle; BinderRef names the exact binder,
not a parameter spelling. Neither is a bare integer meaningful in an arbitrary context.

A declaration reference skips spelling lookup, **not** visibility, category, type-argument or
constraint checks. Reject foreign/stale handles. Paths use ordinary resolution; structured
references to not-yet-checked declarations confer no positive checking evidence.
The HIR type expressions are not pre-validated portable Ty values.

```text
HIR FunctionId -- matching declaration table --> DefinitionId
                                             --> Provider binding key
```

The function declaration's DefinitionId is also its implementation binding identity. There is no
second Native entry-ID namespace. Creating HIR records records correspondence directly: no name,
span or category-order join after lowering. Definition IDs remain context-scoped and are remapped
or validated at existing artifact/import boundaries, not serialized as process-global identities.

### Origins, not mandatory generated text

ModuleOrigins supports source-backed node spans and structured declaration origins. A structured
origin identifies its input and declaration/member for diagnostics without inventing a filename
or empty source span. Synthetic nodes retain an explicit derivation origin.

Declaration text is an optional documentation/navigation projection. Analyzer owns its generated
position index; it is not required for compilation or an authority to attach a Rust entry.
Caller-supplied display text must be checked or treated as non-authoritative; it cannot replace
checked declarations. Ordinary source diagnostics retain exact physical/copied-source provenance.
The source spelling for Native declarations remains an open syntax decision, not an implicit
interpretation of every bodyless function.

### Library APIs versus compiler intrinsics

**Recorded direction, not an implemented migration:** output and reflection API names belong
to libraries, not the HIR resolver's unconditional helper fallback. Availability follows ordinary
installed declarations, imports and prelude selection; NativeProvider supplies implementations.

| Concern | Target owner |
| --- | --- |
| print/log API and output destination | Library declarations plus Host-provided service; current print already lowers to host::log, not necessarily stdout |
| type_of and reflection objects/API | Reflection library over Runtime-managed type metadata |
| Safe object/metadata access | Runtime primitives enforcing identity, declared access, writeability, roots and scoped loans |
| Necessary compile-time reflection | Explicit checked intrinsic contracts identified by declaration identity, not bare spelling |
| Scalar arithmetic/constant rules | Compiler language semantics; not part of the reflection/output extraction |

Current get_field/set_field require a string-literal field name and derive/check the field type
at compile time. Moving them to ordinary Native functions without an equivalent proof would
lose static guarantees. Choose a typed Field<Owner, Value> descriptor, a dynamic checked API
with explicit lookup/type failures, or retain that static behavior as an explicit intrinsic.
set_index should reuse ordinary checked indexing rather than duplicate its rules. Exact public
signatures and the reflection choice remain open; no compatibility helper is required.

Remove BuiltinFunction name fallback when its consumers are replaced. Preserve once-only,
left-to-right evaluation, trap order, completed effects and current access/borrow validation.
A library wrapper cannot read raw Runtime memory or grant itself intrinsic authority merely
by declaring a matching name. This changes API ownership, not the Runtime safety boundary.

## 3. Declaration records

### Functions and boundary contracts

```text
Function {
    id: FunctionId,
    // Existing kind, visibility, name, async, generics and bounds.
    params: Vec<Param>,
    return_type: Option<TypeRefId>,
    access: CallAccessContract,
    implementation: FunctionImplementation,
}

TraitMethod {
    // Existing member identity and name.
    function: FunctionId,
    override_policy: OverridePolicy,
}

OverridePolicy = Allowed | Forbidden

FunctionImplementation = Required | Script(BlockId) | Native(NativeContract)

CallAccessContract {
    receiver: Option<ReferenceAccess>,
}

NativeContract {
    host_call: Option<HostCallContract>,
    callable_requirements: Vec<OperationRequirement>,
    concrete_result: Option<TypeRefId>,
}

OperationRequirement {
    receiver: TypeRefId,
    interface: TypeRefId,
    member: DeclarationRef,
    arguments: Vec<TypeRefId>,
}
```

| Kind | Meaning |
| --- | --- |
| Required | A trait requirement; checking rejects it where an implementation is mandatory |
| Script | An ordinary Kagari body to resolve, check and compile |
| Native | An external implementation contract; no Kagari body or Rust entry is stored here |

**Selected ownership:** remove the combined MethodPolicy from the target model.

| Concern | Owner | Rule |
| --- | --- | --- |
| Default replacement | TraitMethod.override_policy | Required methods must allow implementation; Forbidden requires a supplied Script or Native default |
| Declared receiver access | Function.access | Independent of implementation kind; required methods can also declare it |
| Actual Rust loan/conversion | NativeProvider binding | Must satisfy the checked callable access contract; does not redefine it |

CallAccessContract.receiver describes a scoped loan of Native-managed data: Shared for a
len-like adapter, Mutable for a push-like adapter. It does not control parameter rebinding or introduce a new
language-wide receiver mutability model. None means no such direct data-loan requirement,
not purity or absence of mutation. Do not duplicate receiver access inside NativeContract.

Checking preserves access requirements through trait/default selection and provider admission;
checked/source-free contracts retain them. Runtime still checks actual alias conflicts when
acquiring the loan; a declared Mutable requirement is not a compile-time proof of exclusivity.
HostCallContract describes external passing/access effects, not a Rust function-pointer ABI.
A missing/recovered body does not create Native authority; erroneous source remains ineligible.

Operation requirements declare Kagari calls made by a Rust algorithm. Bounds prove capability;
compilation selects witnesses and records their applications. A concrete result needs a checked
conversion to the public result type. The checked form replaces type expressions with scoped
semantic types and retains the corresponding proofs; no operation resolution is deferred to a
callback's arbitrary string lookup.

### Types, traits and defaults

```text
NominalTypeDeclaration {
    // Existing identity, visibility, generics and structural declarations.
    representation: ScriptManaged | NativeManaged | ExternalHost(HostTypeContract),
}
```

These are nominal representation contracts, not a classification of all language types.
ScriptManaged uses language-defined layouts; NativeManaged uses a Native data implementation
with a Runtime-owned GC lifetime. ExternalHost remains Host-owned, with declared identity and
permitted typed paths. Ordinary structs/enums keep their checked language layouts; opaque
declarations can describe Native-managed or external Host objects.

NativeManaged contains no Rust layout, scan or drop function. Builtin/value representations keep
explicit semantic owners; do not add collection-specific cases or a catch-all Intrinsic
representation. Unrestricted Rust references never enter the script heap.

Traits and impls stay ordinary declarations. Native types may implement script-defined traits;
methods in an impl can be Script or Native. Failure variants and language roles belong to their
normal declaration/semantic owners. Conversion derivation uses ordinary checked relationships,
not a registration-only trait table or a privilege inferred from Native provenance.

```text
trait default body: Script -> helper<Self, T, Self::Iter>(self)
helper: Native(contract)
```

There is **no NativeDefault implementation kind or application recipe**. Substitutions,
projections, bounds and results are checked as ordinary calls. Forwarding elimination is an
optimization and must preserve evaluation/effects and reload/provider selection.

## 4. Checked libraries and Rust providers

```text
CheckedLibrary {
    contracts: Arc<LibraryContracts>,
    bodies: CheckedBodies,
}

LibraryContracts {
    definitions: DefinitionTable,
    modules: CheckedModuleMetadata,
    declarations: CheckedDeclarations,
    host_paths: HostPathContracts,
}

NativeProvider {
    contracts: Arc<LibraryContracts>,
    functions: Map<DefinitionId, RustFunctionBinding>,
    types: Map<DefinitionId, RustTypeBinding>,
    paths: Map<HostPathId, RustPathBinding>,
}

RustTypeBinding {
    rust_type: RustTypeId,
    provider: GcProviderBinding,
    access: TypeAccessAdapter,
}
```

LibraryContracts is the common checked view, not RegistrationContracts under a new name. It
retains signatures, representation/access contracts, relationships and body identities, but no
owning HIR graph, Rust callbacks or whole Program. CheckedLibrary separately owns checked bodies.
Existing retained-analysis APIs can still own HIR; this design does not change their cache policy.

RustTypeBinding supplies the implementation of a declared NativeManaged type, not a new type
declaration or automatic exposure of arbitrary Rust types. External Host access remains separate.

Provider keys must name the exact retained contract context. Match Rust codecs, receiver/Host
policy, type representation contracts and required operations before admitting execution. Providers cannot
add undeclared methods, impls or access. Native provenance, a role attribute or a supplied
signature alone grants no installation authority.

GcProviderBinding uses the existing GC provider protocol for layout, scanning, accounting and
drop; it is not another heap or duplicate hook registry. Rust implementations define instance
data and managed edges; Runtime drives the heap, roots, scoped loans and mutation publication. Mutable runtime-specific
services remain Runtime-local; only suitable immutable bindings can be shared.

## 5. Calls, generics and reload

For a schematic Vec<T>::len declaration:

```text
Declaration: Vec<T>, List<T>, impl List<T> for Vec<T>, len signature/loan policy
Provider:    Vec Rust type implementation + Rust len adapter
Compile:     select len for Vec<i32>, check arguments/bounds and record application
Link:        match the admitted provider and prepare conversions/operation witnesses
Invoke:      borrow the protected receiver, run the adapter, publish the scalar result
```

Compilation does not require an already installed Rust callback. Missing or incompatible
bindings must be rejected before entering the admitted computation, not masquerade as a type
inference failure. Generic Rust support uses declared adapters/templates; it does not ask rustc
to instantiate arbitrary Rust generics for every runtime-created Kagari type.

```text
LinkedNativeCall {
    binding: RustFunctionBinding,
    signature: ScopedSignature,
    mapping: PreparedMapping,
    operations: SelectedOperations,
    result_adapter: Option<PreparedResultAdapter>,
}
```

Final links belong to recipient publication/application records, not shared declarations or
LibraryContracts. NativeCall is a temporary exclusive Runtime activation over protected operands
and pending results; it is not a persistent callback/linked-call container.

No runtime name inference or trait search. Actual values, owner/generation, loans, roots,
cancellation and result publication still require checks. End physical loans before GC/reentry;
trap/cancellation/unwind clean up the admitted suffix. Arc code ownership is not a managed root.
Ordinary calls select current implementations; closures/Futures/admitted frames retain their
selected bodies/providers, while their ordinary inner calls still select current links.

## 6. Preparation and standard-library scale

| Stage | Required work |
| --- | --- |
| Stdlib build | Analyze fixed declarations/bodies, emit checked templates/indexes and compile Rust adapters |
| Script compile | Consume checked contracts; check demanded calls, generic/default applications and operation/result proofs |
| Detached preparation | Prepare immutable executable products and adapters without recipient Runtime state |
| Install/publication | Validate recipient contracts/providers and atomically create final local mappings/links |
| Invocation | Execute prepared adapters with actual-value and execution-safety checks |

Do not reparse/recheck the standard library at each engine startup. Separate declaration metadata
availability from executable preparation: include the demand closure of layouts, interface slots,
callbacks and retained computations, not every unrelated library body.
Genuinely new runtime-supplied generic environments may need application admission/mapping; reuse
compile-time proof and avoid repeated preparation for an already admitted application. Its owner,
validity and bounded retention must be specified before promising zero first-call preparation.

Current code still prepares some generic applications during execution; see
[native/application.rs](../crates/kagari-runtime/src/native/application.rs). Neither prebuilt
library products nor the complete demand-driven path is implemented by this proposal. Measure
build, compile, prepare, install and first/repeated calls separately, including unrelated-library
growth and memory. No startup ratio or execution speedup is claimed.

## 7. Replacement boundaries and acceptance

This is a design sequence, not an activated implementation schedule. Replace obsolete APIs and
models directly; do not retain compatibility readers, Native entry-ID bridges or duplicate
semantic tables. Invalidate affected caches/products when their contracts change.

| Boundary | Required replacement and evidence |
| --- | --- |
| Declaration entry | Source/structured inputs construct common HIR directly; explicit implementation/type contracts; test forward/foreign references and same-named categories without text or ordinal matching |
| Shared checking | Both inputs receive identical namespace, type, bounds, impl and access rules; validate trait override policy separately from callable access; required/error bodies remain distinct from Native; defaults use ordinary checked calls |
| Library APIs | Output/reflection follow installed declarations and normal imports; replace helper-name fallback, retain static field proofs or explicitly design dynamic errors, and preserve field/index evaluation and safety contracts |
| Contracts and providers | Common checked contracts separate from HIR/body ownership and Rust bindings; reject wrong-context, missing, incompatible and undeclared bindings |
| Executable path | Compiler and source-free products retain independent external/operation/result evidence; migrate linking, defaults and actual-native calls through atomic publication |
| Scale | Prebuilt fixed library plus demanded preparation; measure cold startup, repeated runtimes, unrelated library growth and dynamic generic applications |

Use a small end-to-end slice: a Native function accepting a Kagari enum, a NativeManaged type with
methods, a Native implementation of a script trait, and a generic default calling a Native
helper. Preserve independent GC/reentry, cancellation, old-snapshot, source-free and reload
contracts. Full backend/platform matrices remain CI-only.

Collection restoration stays with [NC](native-collections-plan.md#runtime-and-gc-handoff); this
proposal neither restores paused fixtures nor settles dynamic collection/managed-type construction.
Rust-specific layout and ABI details, JIT/AOT expansion, GC redesign, HIR arena storage and LSP
scheduling are not authorized as incidental rewrites.

## 8. Remaining design decisions

- Source declaration syntax and the exact structured builder surface, including forward/binder
  handles. Both must expose the same contracts without granting installation authority.
- ModuleOrigins representation and diagnostic/tool projections for non-source inputs; no forced
  generated source or loss of existing exact-source diagnostics.
- Checked-library artifact schema and context import, dependency/layout closure, and generic
  application admission/cache ownership for runtime-supplied environments.
- Reflection API form: typed descriptors, explicit dynamic failures, or checked compile-time
  intrinsics; decide how to replace current statically typed field helpers before removing them.
- Exact field/crate placement for Host paths and conversion derivation; preserve semantic owners,
  not the current registration table layout.

## References

- [rustc HIR at 5f4effd57950](https://github.com/rust-lang/rust/blob/5f4effd57950/compiler/rustc_hir/src/hir.rs):
  ForeignItemKind::Fn stores signature/generics without a body or machine callback. Borrow the
  declaration/implementation distinction, not C ABI rules, arena lifetimes or its generic limits.
- [Current Native calls](architecture/runtime/calls.md#native-call-prepared-slots-and-mutable-services),
  [reload](architecture/runtime/reload.md), [current Rust storage binding](../crates/kagari-runtime/src/native/storage.rs)
  and [GC provider protocol](../crates/kagari-gc/src/object.rs) own execution safety to preserve.
