# Native model: declarations, HIR and implementations

**Status:** NM00 proposal prepared against `c19a0ffbf`. User review limits this implementation
to Native HIR representation and declaration parsing/analysis, not the overall design discussion.
The subsequent user instruction authorizes unattended NM01–NM04 implementation and phase commits.
The integrated NM01 frontend substrate is implemented; final phase acceptance remains in progress. The
[execution plan](native-model-plan.md) owns sequencing, evidence and carried errors.
This replaces the attachment-only draft. Current contracts remain in
[HIR architecture](architecture/hir.md), [host interop](spec/host-interop.md) and
[standard declarations](spec/standard-declarations.md).

**Model:** Native declarations are ordinary Kagari declarations with explicit implementation,
representation and boundary contracts. Rust implementations bind separately to checked declaration
identities. This change unifies their compiler-facing representation, not the registration or
execution protocol.

**Current scope:** common Native function/type/member/impl records, direct structured lowering,
parsing exported .kgr declarations, shared semantic checking and matching navigation origins.
Ref/RefMut recognition and explicit reference-type records are included; no new reference operations,
borrowing, projection, implicit adaptation or method forwarding are implemented. Preserve existing
supported reference routes and safety checks rather than disabling them.
Declared fields are in scope for HIR representation, read/write type checking and export parsing;
new Rust field adapters and end-to-end Runtime field execution require separate authorization.
HIR is the implementation owner. Upstream and downstream changes accompany its new records,
inputs and consumers, including necessary signature/type/call-site changes; they are not frozen.
Do not interpret this as edits confined physically to hir/, or use it to redesign registration,
execution or tooling ownership. Their broader separation remains later work. The execution plan's
implementation inventory owns the concrete slices and acceptance boundary.

**Deferred:** registration/preparation redesign, LibraryContracts/NativeProvider cutover, binary
prebuild/code generation, MIR/shared-library export, installation/linking/caches, directory
publication/watch integration, output/reflection extraction and new Ref/RefMut behavior. Earlier
whole-stack sketches below remain recorded future design directions, not abandoned/rejected
ideas or frozen implementation commitments. Keep their structures, rationale and open decisions;
they are not prerequisites or acceptance criteria for this HIR change. Reflection and current
compiler/Runtime behavior remain unchanged.

The record sketches below describe the reviewed design, not literal Rust API declarations.
Current HIR/input implementations and final acceptance are tracked in the execution plan. Keep owned category tables, owner-qualified IDs and current weak graph retention;
no arena, universal node identity, per-node Arc or new crate is required.

## 1. Inputs and ownership

```text
compilation: Rust structured declarations --+
LSP input:   exported .kgr -> parsing -------+-> common declaration input
user source: ordinary parsing --------------+             |
                                                         v
                                            HIR / resolution / checking
                                                 |                 |
                                       existing compiler       analyzer queries
                                             handoff           and navigation
```

| Owner | Proposed responsibility |
| --- | --- |
| kagari-hir::authoring | Unchecked source/structured inputs, reservation handles, mixed-closure checking and checked body ownership |
| kagari-source | Minimal input/origin/reporting vocabulary; no generated-document or LSP lifecycle state |
| kagari-common | Existing context-scoped DefinitionId and explicit definition import/remapping |
| kagari-types | Shared type, representation, access and declaration vocabulary; no authoring session or second checker |
| kagari-analyzer | Queries/navigation over matching common analysis, including exported declaration inputs |
| Existing compiler/SDK/Runtime consumers | Necessary call-site changes only; preserve current registration, binding and execution behavior |

Proposed module names indicate placement, not permission to create empty facades or new
contract-product dependencies. Keep current dependency direction: compiler queries do not depend
on analyzer; Runtime stays independent of HIR/syntax/compiler. Future product owners are discussed
separately below, not implemented as part of this HIR change.

## 2. Entry into HIR

### Reservation, references and checking

The handle/session sketch describes valid input correspondence and forward references, not a
requirement to ship a new public Rust builder, reservation framework or authoring DSL. For this
HIR change, adapt existing upstream declaration records with only the construction machinery
actually needed by common lowering/checking; upstream API design remains a separate task.

```text
AuthoringSessionId                     // fresh, process-local; never reused
DeclarationHandle { session, slot }
BinderHandle { owner: DeclarationHandle, position }

DeclarationSlot {
    header: { module, category, parent, name, own_binders },
    state: Reserved | Filled(DeclarationInput),
}

DeclarationRef = Local(DeclarationHandle)
               | Imported(ContractDeclarationRef)
               | Pending(ExactSelector, expected_category_and_arity)
```

The session owns all mutually dependent unchecked modules. Source header collection exposes
handles tied to the exact retained parse; structured headers use the same collection path.

1. Register module owners and reserve declarations/members and their own binders.
2. Fill each slot once; its category, parent and binder layout cannot change.
3. Freeze immutable inputs; reject unfinished slots, duplicate fills and foreign/escaped handles.
   Resolve pending selectors against actual headers, including unused references.
4. Allocate ordinary HIR category IDs and record handle -> HIR ID -> DefinitionId correspondence.
   Run common resolution, signature, trait/impl and body checking.
5. Only successful checking issues contract references and binding-key mappings.
   Cancellation/failure publishes no installable Provider authority.

No slot recycling or per-slot generations: a new editing session is a fresh generation. A frozen
input can be cloned without making its builder mutable again. These handles belong to HIR
input construction; Runtime never consumes them. Interning a DefinitionId alone proves neither
a completed declaration nor successful checking.

```text
Input type head = Path | Declaration(DeclarationRef) | Parameter(BinderHandle) | Self
HIR type head   = Path | Declaration { target: DefinitionId, exposure: SelectedExposure }
                      | Parameter(owner, position) | Self(owner)
```

Keep ordinary application arguments, tuples, arrays, functions and qualified projections.
Associated bindings/projections can name an exact member as well as an unresolved spelling;
otherwise structured input would still need name reconstruction. HIR heads are unchecked type
expressions, not an escape hatch for inserting already-validated Ty values.

SelectedExposure retains the selected local/import/re-export route or the existing exact
importer-specific source-trait admission. Input references select that route; lowering must not
collapse it to the target ID alone. Common accessibility checking validates this evidence in the
referencing scope; it is not a second visibility checker or a general private-access permission.

Direct references skip spelling selection only. Common checking still enforces declaration
existence, category, visibility/exposure route, dependency membership, arity, bounds, associated
outputs and role eligibility. Preserve valid public re-export paths and reject private-path
bypasses. Import checked contexts with existing explicit remapping; reject conflicting semantic
records even when their remapped definition identities coincide. Imported checked modules are
contract-backed namespace entries, not newly lowered HIR modules: use a context-qualified
library/module reference without a fabricated SourceUnit, input file or LoweringId.

A generic binder remains `(declaring owner, own position)`. Impl binders retain their owner when
inherited by methods; method-local or shadowing binders never reuse the outer identity. Imported
open binders cannot escape independently of their declaration/application scope.

### Source declarations and ordinary defaults

Recommend `native fn` and `native type` in generated, read-only API views:

```text
pub native fn answer() -> i32;
pub native type Counter<T>;
```

Rust authoring supplies structured declarations directly during compilation; that path never
requires a render/parse round trip. Offline analyzer instead parses build-exported .kgr declarations
into the same unchecked declaration-input model, then uses common HIR/resolution/checking with
user source. These files are real declaration inputs for tooling, but neither proof of checked
Rust bindings nor Runtime installation authority. User-authored Native declarations in ordinary
source remain a separate, unapproved capability; parsing native syntax does not enable them.

Definition kind is independent of authoring location. A trait, function or struct authored as
Kagari text inside Rust is still a Script definition. Preserve its complete definition, including
function/default bodies and ordinary fields; Rust compilation does not turn it into a Native stub.

For offline LSP, a library's exported .kgr inputs include Native signatures/type interfaces plus
complete Script definitions not otherwise available as source dependencies. Rust-embedded strings
are retained/exported as real script source, not reduced to signatures. Structured authoring of an
ordinary Script definition must likewise retain enough content for a complete definition; support
for new body-building APIs remains deferred. Keep one semantic module/declaration identity per
owner, whether its physical source is a project file, library file or Rust-embedded text.

A Native impl references the actual trait owner, supplied either by original source or the complete
library export. Do not create duplicate/stripped traits or a marker for omitted defaults. A genuinely
missing dependency is diagnosed; it is not the intended export format.

**Parser recommendation:** one lexer/parser recognizes explicit native syntax and ordinary
Kagari syntax. Source-versus-exported input identity governs semantic admission, not a second
grammar mode. Replace current Source/Declarations parsing switches and update their consumers;
preserve context-specific member/visibility rules in normal grammar/semantic validation. This
does not grant handwritten Native declarations execution authority.

| Form | Meaning |
| --- | --- |
| Ordinary fn with body | Script implementation |
| Trait fn without body | Required method |
| native fn without body | Native implementation, not a missing/required body |
| native type | Native type with optional declared fields; ownership is separate metadata |
| impl Type / impl Trait for Type | Ordinary member ownership / checked trait implementation |

Access, override, Host policy and Native operation/result requirements stay in structured
contracts. Do not require call_access, override_policy, host_type/host_field/host_call attributes
or a contract { requires ...; produces ...; } source DSL merely to render an API. Preserve
required validation, supported Host passing/effects and ordered operation requirements. Export
all facts needed to reconstruct static API semantics, not just pretty signatures; documentation
alone cannot encode a restriction needed by checking. Rust-only adapter details stay in private
contracts/bindings. The minimal external-declaration encoding is an NM03 design obligation.
Do not infer Native authority from text or add new async/borrowed-return semantics.
See the callable declaration views below for method/associated-function/trait examples.

**Deferred body-authoring option:** a future structured Script implementation may retain authored
block source as below. A new fragment parser or NativeDefault execution-model replacement is not
required by the current HIR/declaration scope; preserve existing supported defaults.

For that future option:

```text
ScriptBodyInput { source: Arc<SourceFile>, block_range }
```

The frozen input retains the real source independently of any lowering. Each lowering assigns
its own SourceViewId; the same frozen block can be lowered again without reusing graph-local IDs.
Add a real block-fragment parser entrypoint, not a generated function/module wrapper. Lower under
the owning function's module imports, parameters, inherited/own binders, Self, bounds, result and
async context. Result: ordinary `Script(BlockId)`. No general expression-builder DSL, helper-name
injection, placeholder substitution or NativeDefault forwarding recipe. Reused physical inputs
retain parser limits/cancellation accounting rather than restarting budgets per attachment.

### Origins, not mandatory generated text

```text
ModuleInputVersion = Source { file, revision } | Structured(InputRevisionId)
ModuleUnit { module: ModuleHandle, input: ModuleInputVersion, lowering: LoweringId }

OriginSite = Source { view: SourceViewId, range: Span }
           | Structured { input, declaration_key, part }
           | Synthetic { anchor: AuthoredOrigin, reason }

ModuleOrigins {
    views: Vec<Arc<SourceFile>>,
    // Existing typed category tables and lowering/owner checks, now storing OriginSite.
}

LoweredModule { metadata, input, module: Module, origins: ModuleOrigins }
```

A sealed structured module gets a fresh opaque InputRevisionId. Clones preserve it; edits or
independent freezes get a fresh token. Include bodies, references, semantic metadata and
provenance in the immutable payload. This is equality, not a clock: retain existing request/
settings publication guards. Fresh lowering gets a fresh LoweringId even for the same input.
DefinitionId remains declaration correspondence, not a source/implementation version.

Multiple body fragments may belong to one structured module. SourceViewId is lowering-qualified;
source ranges address each view's unchanged bytes. Semantic scope comes from the owning function,
not the fragment's filename. Resolve through the exact retained SourceFile::span for copied/inline
physical provenance. Never associate diagnostics using bare Span across views or fabricate files.

Declaration origins and diagnostic labels expose one authoritative source/structured/synthetic
location. Exact source projection is optional; synthetic anchoring is a separate operation.
A real authored default block has source origins; only genuinely compiler-generated nodes are
synthetic. A body fragment is not the structured header's complete declaration source.

Source-only inspection skips explicitly non-source declarations; exact-source/edit requests
report unavailable. Invalid claimed source is still an error. Preserve semantic/body-owner
inventories independently of source inventories. Propagate unavailable debug source through MIR/
bytecode without fake zero spans or disabling liveness, root or executable checks. Debug source
selection is per body, not the first view in a module.

Complete KBC source packages keep their exact completeness contract: absent full declaration
anchors means no complete package can be emitted, not a relaxed validator. Static initializer
freeze evidence remains mandatory even if source is stripped; produce its existing canonical
facts from an authored initializer, or explicitly reject unsupported structured initialization.
Build-exported declaration text is offline analyzer input and a navigation target, not authored
Rust/Script implementation source. Its locations cannot authorize source updates, complete
implementation-source packaging or Runtime bindings. Compilation uses structured inputs directly.

Build data encodes library-relative declaration/member attribution and optional real source
coordinates, not process-local session/input/lowering IDs. Import requalifies those records in
the current contract/reporting context; labels alone cannot claim an exact retained source view.
ModuleUnit qualifies HIR-backed modules only; checked-library imports do not invent local HIR IDs.

### Library APIs versus compiler intrinsics

**Deferred by user decision:** leave type_of, get_field, set_field, set_index and their current
lookup, literal-field typing, validation and execution unchanged. Do not choose Field descriptors,
dynamic reflection values or a new intrinsic API in NM00. Internal origin/contract changes must
preserve their observable behavior. This is retained scope, not a newly added compatibility path.

Output is separate: NM04 may make print/log an ordinary declared library/Host service. Remove
only its special lookup/checking/lowering when replaced; retain reflection helper handling.
Scalar arithmetic and constant legality remain compiler language semantics. Any later reflection
migration needs separate authorization and must preserve evaluation, access and safety guarantees.

## 3. Declaration records

The following is the current target frontend model, not a complete replacement for unrelated
Module/Body fields or runtime protocols. Ordinary owned category tables and ID links remain.

```text
Module {
    // Existing source-order items, ordinary Script declarations and attributes.
    native_types: Vec<NativeTypeDeclaration>,
    functions: Vec<Function>,
    traits: Vec<Trait>,
    impls: Vec<Impl>,
    body: Body,
}

NativeTypeDeclaration {
    id: NativeTypeId,
    name,
    visibility,
    generic_params,
    bounds,
    fields: Vec<FieldDeclaration>,
}

FieldDeclaration {
    id: FieldId,
    name,
    visibility,
    ty: TypeRefId,
    writeability: Val | Var,
}

Function {
    id: FunctionId,
    // Existing kind, visibility, name, async, generics and bounds.
    params: Vec<Param>,
    return_type: Option<TypeRefId>,
    implementation: FunctionImplementation,
}

FunctionImplementation = Required | Script(BlockId) | Native(NativeContract)
TraitMethod { function: FunctionId, override_policy: Allowed | Forbidden }

NativeContract {
    callable_requirements: Vec<OperationRequirement>,
    concrete_result: Option<TypeRefId>,
}

OperationRequirement {
    receiver: TypeRefId,          // Self target type, not a runtime receiver value or loan.
    interface: TraitRef,           // Full trait application, including arguments/associated bindings.
    member: DeclarationRef,        // Trait member declaration; not its selected impl or callback.
    arguments: Vec<TypeRefId>,     // Method-owned generic type arguments, in declaration order.
}

Impl {
    id: ImplId,
    generic_params,
    bounds,
    self_ty: TypeRefId,
    trait_ref: Option<TraitRef>,
    methods: Vec<FunctionId>,
    // Existing associated types/constants.
}
```

NativeTypeId names the target declaration slot, replacing the misleading opaque-only concept;
field records reuse the common field vocabulary and owner-qualified identities, not a second
Native field schema or source-map allocation. The existing field owner must include its Native
declaration where needed. Impl without trait_ref is inherent; self in a function signature marks
an instance method, not a Rust loan. A function without self in that impl is associated.
NativeContract retains only required semantic/boundary descriptors; no Rust callback, storage
adapter, pointer or process-local borrow state enters these records.

### Native argument semantics and Rust adaptation

**Current decision:** remove host_call / HostCallContract from the target HIR NativeContract.
Existing HostCallPolicy is a current execution boundary, not deleted by a documentation change;
its replacement by explicit reference parameters and any effect-contract migration require later
implementation. Preserve actual binding validation and static initializer/effect checks meanwhile.

**Recommended argument model:** ordinary parameter types determine script argument semantics;
Rust DTO/Serde conversion is a separate binding decision, not a hidden second parameter type.

| Script parameter | HIR meaning | Rust boundary responsibility |
| --- | --- | --- |
| Scalar/ordinary value type | Existing language value semantics | Decode an admitted value; immutable data need not be physically deep-copied |
| Ordinary script/native object T | Pass the existing object's identity, not an implicit clone or Rust reference | Receive a protected/retained object handle through the appropriate adapter |
| Ref<T> | Explicit scoped shared-location capability | Validate/lend the mapped Rust location when supported |
| RefMut<T> | Explicit scoped mutable-location capability | Validate/lend exclusively; actual overlapping accesses trap |

There is no additional HIR passing = Owned/SharedBorrow/UniqueBorrow array. HIR Param stores
TypeRefId; its resolved TypedParameter stores TypeId, including Reference(target, access).
Ordinary Native self is an object argument, never an implicit Rust &self/&mut self contract.
Hidden NativeContext/GC handle conversion belongs to the Rust adapter, not the script arity.
Tuples/enums/composites retain their current value/identity rules, not blanket deep-copy semantics.

For ordinary object input T, an implementation may explicitly extract supported data into a Rust
DTO/Serde<T> snapshot. This is in-process schema conversion, not necessarily byte serialization.
The supported schema/representation and converters must match the declaration; arbitrary Native
payloads and scoped references cannot implicitly become owned Rust data. Snapshot extraction
does not mutate or detach the original, transfer its ownership, preserve every graph identity, or
write Rust-local edits back automatically. A binding promising shared-object behavior must actually
operate on the original handle; a DTO conversion cannot silently stand in for such behavior.

No new HIR Serialization/ByValue/Object passing flag is proposed just to select a Rust callback
parameter type. If an API must promise a call-boundary snapshot/clone independently of implementation,
that is a distinct observable operation/contract and needs an explicit language/API design, not a
hidden adapter switch. This choice remains reviewable; do not implement new conversion machinery.
Existing Object<S>/Opaque<T> identity-preserving adapters and FromKagari owned-data conversion
illustrate the separation; do not merge or bypass their representation and lifetime checks.
Ref/RefMut rows describe future execution direction; this phase implements recognition only.

Type syntax and resolved types remain separate:

```text
// TypeRef syntax: ordinary paths/applications; no new special parser syntax required.
Vec<i32>        -> Path(Vec, [Path(i32)])
RefMut<Player>  -> Path(RefMut, [Path(Player)])

// Relevant variants in the existing semantic TypeId model, not a new parallel type system.
NativeObject(NominalType { declaration: DefinitionId, arguments, associated_types })
Reference { target: Box<TypeId>, access: ReferenceAccess::Shared | Mutable }
// Builtin(String/i32/...) and Range(element, kind) remain language-owned variants.
```

Resolution recognizes Ref/RefMut by builtin declaration identity, checks constructor shape and
resolves the target type. Both constructors use Reference with different access, sharing the same
target declaration. Recognizing that type grants no new executable capability.

Required is legal only where an implementation may be supplied; Forbidden requires an existing
Script/Native default. The earlier universal Function.access / CallAccessContract proposal is
withdrawn: ordinary trait methods do not acquire Rust receiver-loan requirements. Preserve existing
Native adapter access metadata, scoped-reference method eligibility, Host passing restrictions
and declaration/binding agreement; NM01 must settle their narrow ownership after tracing current
consumers, not expand them into language-wide access semantics. Actual alias conflicts remain
Runtime-owned. Ordinary NativeType impls and trait implementations use normal self without
shared/mutable receiver annotations. Explicit Rust-location access is expressed by Ref/RefMut
parameter types; internal Rust &T/&mut T access is adapter/storage behavior, not another HIR
function qualifier. This does not automatically forward all target methods to Ref/RefMut;
preserve existing supported reference-method checks without designing new forwarding rules here.
Only Ref/RefMut recognition/representation is included; new behavior is deferred as specified below.

### Native operation requirements

A requirement describes an operation a Rust algorithm needs from Kagari, not another function
signature or a runtime argument pack. For <T as Convert<String>>::convert<i32>:

| Field | Value |
| --- | --- |
| receiver | T, retaining the Native function's binder owner |
| interface | Convert<String>, including any associated-type equalities |
| member | The convert member declared by Convert, not an impl method identity |
| arguments | [i32], the method's own type arguments; excludes trait arguments and actual values |

TraitRef reuses ordinary trait application syntax. DeclarationRef uses the common local/imported/
pending declaration reference machinery; it must resolve to a member of the selected declaring
trait through normal lookup/access checks. No Rust callback/implementation selector is supplied.
The member declaration owns parameter/result types: derive the applied signature by substituting
Self, trait arguments/associated bindings and method type arguments, then check all bounds.
Do not copy a second params/result signature into OperationRequirement.

Resolved forms use existing semantic types and scoped DefinitionId. The current
NativeCallableRequirement already carries receiver: Ty, interface: NominalTy, member: DefinitionId
and arguments: Vec<Ty>; reuse/refine that vocabulary rather than create a parallel checked-call IR.
Requirements remain ordered slots owned by the Native function's contract; preserve multiplicity.
Ordinary Native functions need none. Rust bodies are opaque to HIR, so required Kagari operations
must be declared explicitly; Script bodies instead expose their calls to normal analysis.

Bounds prove capability; application compilation selects concrete operations or checked shared
evidence, independently of whether the selected body is Script or Native. Runtime receives the
resulting executable links/evidence, not a method name to resolve with a trait solver. Selection
and new runtime application behavior remain outside this HIR redesign; preserve existing paths.
Reflection lookup and callback string dispatch are not substitutes.

Native declaration identity is independent of Runtime-GC versus Host management. Do not classify
HIR Native declarations as ScriptManaged/NativeManaged/ExternalHost; those earlier proposed
categories mixed declaration kind with execution ownership. Preserve existing representation and
ABI facts at their required compiler/binding boundaries until their consumers are adjusted;
unifying declarations is not permission to erase current Host identity/path or safety checks.
Opaque<T> is a Rust owning handle, not an extra script type. HIR models Player and reference
applications targeting Player, not duplicate Player declarations for each storage/access form.

### Language built-ins versus Native types

**User decision:** String and Range belong to language built-ins, like numeric types in ownership
of their semantics, not to the ordinary Native type family. Built-in does not mean scalar: Range
and reference/array constructors keep their own structured type rules.

- String resolves to the existing builtin String type; Range resolves to the existing range type
  constructor with its kind/element rules. Literals and explicit type annotations must agree.
- NativeTypeKind currently mixes ordinary Native storage/Host descriptions with String, Range
  and Reference constructor mappings. Move frontend builtin interpretation to the language/type
  semantic owner; registration may still supply methods and validated declaration correspondence.
  Keep existing arity, eligibility, identity and installation validation, not spelling-based magic.
- A Rust-implemented method on String or Range is a Native function on a builtin type. It does not
  make that type a native type declaration. Exported declarations and direct inputs must resolve
  these types identically, without declaring a fake Native String/Range or adding a second type.
- Recognize Ref/RefMut under the reference-type owner and preserve their explicit target/access
  representation. Do not implement new reference behavior or an open-ended intrinsic constructor DSL.
  Failure variants, language roles and conversion derivation retain their existing semantic owners.

### Native fields (in scope)

**User decision:** Native types may expose fields, including Kagari-managed Native objects.
This supersedes the earlier NativeManaged-is-always-opaque rule. After scope narrowing, this
phase implements declarations, HIR lookup/type/writeability checks and exported-declaration
parsing; new runtime read/write adapters are a follow-up, not silently included. The intended
field behavior remains recorded here. The declaration view may expose:

```text
pub native type Player {
    var hp: i32,
    val object: Object,
}
```

- Fields describe a script interface, not Rust layout, offsets or unrestricted references.
  An opaque declaration may omit fields; exposure is explicit and follows normal visibility.
- Reading a scalar copies its value. Reading an object field of an ordinary managed Player
  returns an ordinary Object handle, not Ref<Object> or RefMut<Object>.
- var permits field replacement; val forbids replacement, not mutation through the returned
  object's otherwise writable interface. Assignment checks use the declared field type and
  existing evaluation/commit/trap order.
- Common HIR/type checking owns field lookup, types and writeability. Compiler and portable
  contracts carry checked field accesses; Rust bindings supply matching read/write adapters.
  Runtime preserves object identity, roots and mutation publication. Nested object handles must
  retain required storage/ownership; no unprotected interior Rust pointer may escape.
- Build exports contain these same field declarations; analyzer parses them and consumes common
  semantic results. This is ordinary typed field access, not a change to deferred reflection APIs.

### Native callable declaration views

**Required coverage:** every script-exposed Host-provided function, inherent method, associated
function and trait implementation has an explicit declaration/relationship in the common model
and a corresponding navigable API view. Unregistered Rust members are not discovered implicitly.

Use ordinary impl syntax; mark the individual function whose implementation is Native:

```text
use app::traits::Damageable;

pub native type Player {
    var hp: i32,
    val object: Object,
}

impl Player {
    pub native fn new(hp: i32) -> Self;
    pub native fn heal(self, amount: i32);
}

impl Damageable for Player {
    native fn damage(self, amount: i32);
}
```

The imported trait remains an ordinary Kagari declaration at its actual owner, for example:

```text
pub trait Damageable {
    fn damage(self, amount: i32);
}
```

| Declaration | Script use | Meaning |
| --- | --- | --- |
| Free native fn | answer() | Ordinary free function, Rust implementation |
| Native fn without self in impl Player | Player::new(100) | Associated function; new is not special constructor syntax |
| Native fn with self in impl Player | player.heal(10) | Inherent instance method |
| impl Damageable for Player with Native member | player.damage(10), or a checked trait-bound call | Ordinary trait implementation with Rust-provided method body |

- No native impl/native trait category or receiver-loan attributes are needed. A Native type may
  have Script methods; a Script type may have a Native implementation where the existing safe
  binding model supports it. Do not infer member implementation kind from type provenance.
- Trait members inherit trait visibility and match its checked signatures/binders/bounds;
  inherent members retain ordinary visibility. Associated trait functions also omit self.
  Render generic impls, method binders, where bounds and associated type/constant assignments
  using ordinary Kagari declaration forms, not Rust-only selector strings.
- Native marks each supplied implementation, not the whole impl. Ordinary Script members may
  coexist. Inherited defaults retain their actual trait body and navigation target; do not emit
  synthetic Native copies or display a Script default as an unsatisfied Required method.
- Imported/source-defined traits keep their actual definition identity and source navigation.
  A generated impl references that trait; it must not manufacture a second trait declaration.
- Each member has its own declaration DefinitionId for provider binding and display mapping;
  impl metadata records receiver/interface/arguments and associated bindings. Normal checking
  validates the relationship. Neither rendered spelling nor matching a Rust method name binds code.

These examples specify proposed build-exported declarations parsed by tooling, not approval for
user-authored Native declarations in ordinary source. The referenced trait is analyzed from its
original source or the complete Script definition exported by its owning library, including
Rust-embedded source. Never strip default bodies or emit duplicate/synthetic implementations.

### Ref / RefMut: recognition now, behavior later

**Updated user decision:** include builtin recognition, constructor/target resolution and explicit
HIR/semantic reference-type records in this migration. Parse/structured-input tests assert the
same target identity and Shared/Mutable distinction; no new executable feature is provided.
Do not add borrowing syntax, runtime loans, reference field projection, implicit conversions,
method/trait forwarding or lifetime/escape capabilities. Preserve existing supported routes and
all their scoped-reference, Host path and borrow checks; recognition is not blanket admission.

The following behavior remains recorded future direction, not new implementation requirements:

- Ref<T> / RefMut<T> are built-in, namespaced semantic wrappers for scoped Rust locations,
  not script-owned Rust references. Exact namespace/spelling remains a follow-up decision;
  ordinary generic path parsing is sufficient. Only eligible Rust target mappings are admitted.
- Static checking enforces target types and access capabilities: RefMut<T> may be used as Ref<T>,
  but not conversely. Passing one RefMut<T> twice to two mutable parameters passes static
  checking; actual overlapping loans trap at runtime before callback entry. No static alias solver.
- Handles encode location/capability and validity, not a continuously held Rust loan. Runtime
  checks scope/generation and overlapping paths, including parent/child overlap; failure releases
  reservations. Creating or copying a handle does not create a live Rust &mut T.
- A scalar field read returns a value. Object-field projection from Ref<Player> yields Ref<Object>;
  projection from RefMut<Player> yields RefMut<Object>. Parent capability limits access; val
  prevents field replacement, not automatically a mutable projection through a mutable parent.
  Ordinary managed Native parents still yield ordinary object handles, without wrappers.
- Common semantic analysis would own projection types; LSP would consume them. Native Rust
  adapter loans must not become mandatory call_access attributes on ordinary trait methods.
  Existing receiver-access placement needs review, not expansion into a new language constraint.

Native field semantics must not depend on new reference behavior. Unsupported new uses remain
unsupported or report explicit diagnostics; they must not gain fake bodies, unchecked access or
success merely because a reference type can now be represented.

### Mixed input example

```text
app::model source: trait Readable { fn read(self) -> i32; fn twice(self) -> i32 { ... } }
host::counter input: NativeManaged Counter; impl Readable for Counter { Native read }
```

Collect the actual Readable and Counter headers, check their impl/default and applicable Native contracts
together, then bind Rust to the checked read/helper declarations. The default is an ordinary
Script call to a Native helper; no per-impl synthetic function or NativeDefaultApplication.
Forwarding elimination is optional optimization, not a correctness prerequisite or free-frame claim.

If the script trait does not exist at Rust build time, keep the dependent authoring/binding
request explicitly pending outside checked contracts. Finalize against the actual source owner
in source-enabled compilation/preparation; Runtime cannot finalize it. An independently checked
subclosure may be produced separately, but unfinished requests never certify the whole library.
Preserve existing exact importer-specific admission for a referenced private script trait when
Host composition supplies it; expected contracts are comparison obligations, not substitute trait
definitions or general source visibility. Contract changes invalidate reuse even without calls.

## 4. Checked libraries and Rust providers

**Deferred whole-stack sketch:** not implemented or required by the current HIR scope. A future
registration/preparation redesign should directly produce a usable library; binary encoding is
an optional persistence/build integration choice, not a mandatory render/parse/register cycle.

```text
CheckedLibrary { contracts: Arc<LibraryContracts>, bodies: CheckedBodies }

LibraryContracts {
    definitions: DefinitionTable,
    modules: CheckedModuleMetadata,
    declarations: CheckedDeclarations,
    host_paths: HostPathContracts,
    body_contracts: BodyContracts,
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

LibraryContracts owns checked declarations/relationships and body identities/entry requirements,
not HIR, code, callbacks or whole Programs. CheckedBodies owns frontend body facts separately.
A function's own DefinitionId is its binding identity; generated keys identify its ordinary
encoded definition-table entry, not another Native entry namespace. NativeImportId may still
index an executable import table. Runtime imports/remaps identities before accepting binding keys.

RustTypeBinding supplies a declared NativeManaged implementation, not automatic exposure of a
Rust type. GcProviderBinding uses the existing layout/scan/accounting/drop protocol, not another
heap or duplicate hook registry. Runtime drives roots, loans and mutation publication. Compiled
binding setup creates actual Rust TypeIds/codecs/GC adapters in the process and injects live
services; none are serialized. Only suitably immutable bindings share across recipients.

Admission rejects undeclared/duplicate/wrong-context bindings, missing demanded implementations
and incompatible required declaration/type/trait-reference/path metadata, including metadata unused
by a Native call. An origin marker, fingerprint or serialized checked flag grants no authority.

## 5. Calls, generics and reload

**Deferred execution design:** preserve current call/generic/reload behavior in this HIR change;
no new fixed-link preparation, application cache or provider lifecycle is required here.

| Case | Compilation | Recipient preparation |
| --- | --- | --- |
| Closed call | Check application, choose operations/results and demand closure; specialize where appropriate | Prepare identities, codecs, access and final links before publication |
| Shared entry, known closed environment | Emit shared code plus checked type/operation/construction arguments | Prepare the closed mapping before publication |
| Environment supplied later | Prebuild shared code and environment requirements | Authenticate/map supplied evidence at application admission, before entry; no HIR, trait solver or Rust codegen |
| Repeated admitted environment | No new compilation | Reuse valid prepared facts; retain actual-value/protection checks |

Reuse existing shared MIR semantics: ValueType::Generic describes tagged transport, while
SemanticSlots, binder/bound contracts, operation and construction evidence determine its type
and legality. A physical Generic tag alone is neither a template nor proof. Source-free Runtime
always receives executable shared entries, not a promise to lower a compiler template later.

```text
LinkedNativeCall { binding, signature, mapping, operations, result_adapter }
```

Final links belong to recipient publication/application records. Extend fixed-link preparation
for known closed shared/Native applications rather than only warming an evictable cache.
Keep dynamically supplied applications in existing bounded Runtime caches: exact linked member,
import, full generational environment, selected provider/operation dependencies and protection.
Current descriptor indexes use a 128-entry FIFO bound; do not introduce a global name/type cache.
Eviction/retirement can require preparation again; cached descriptors do not keep active managed
applications alive or grant authority. Retain actual environment/body/provider edges and roots.

NativeCall is a temporary exclusive Runtime activation over protected operands and pending results.
End physical loans before GC/reentry; all exits retire the admitted suffix. Actual values,
generations, loans, roots, cancellation and result publication remain checked. Ordinary calls use
current implementations; closures/Futures/admitted frames retain selected bodies/providers, and
their ordinary inner calls still use current links. Refresh target-dependent mappings on reload.

## 6. Preparation and standard-library scale

**Deferred build/tooling integration:** retained discussion, not this migration's deliverable.
Binary products versus generated Rust tables, producer ownership and startup-cost work need a
separate approved design. Parsing exported declarations is in scope; generating/publishing them
after Cargo builds, directory watching and new binary products are not.

### Private product and compiler/bootstrap boundary

```text
LibraryBuildProduct {
    semantic_key,       // model + explicit config + authored inputs + dependency semantic keys
    target_requirements, // portable applicability constraints, not HOST-selected layouts
    contracts: LibraryContracts,
    executable_code: SharedBytecodeLibrary,
    compiler_templates: MirLibrary,
}

MirLibrary {
    definition_scope, contracts_identity,
    modules: existing_typed_MIR_records,
    typed_imports, body_owners_and_default_applications,
    source_availability,
}
```

These are sections owned by contract/bytecode/MIR, not one large record forced into contract.
Compiler assembles them. Reuse existing MIR instructions, SemanticSlots and scoped types with
library import metadata; do not invent a second body IR. Runtime reads contracts/executable code;
application tooling reads templates. No HIR handles, callback pointers, Rust TypeIds, physical GC
hooks or live Runtime environments are encoded.

The library producer checks every owned Script/default body, even unused ones, and lowers it
under canonical Self/trait/method binders. External dependencies remain typed imports and bound
operations remain evidence slots. Include owned closure/resume bodies, but do not enumerate
impl-inherited defaults or recursively specialize imported bodies just to export one library.
Body/default-application metadata records ownership and relationships, not an all-impl closure.
Executable imports resolve to existing shared executable entries with checked applications. Only
application compilation may create specializations from MIR templates. A source-free link with
missing executable coverage fails; it never invokes the producer or compiler. The existing
program demand scheduler closes actual application executable requirements.

Verify local bodies against explicit import contracts, then verify the complete selected executable
graph after relocation. Factor shared verification, never weaken complete-program checks or emit
placeholder bodies. Symbolic default export and import-aware verification are required compiler
work, not claims that today's program compiler can already emit this product unchanged.

### Build integration shared by stdlib and business libraries

```text
library build.rs -> compiler[source] -> HIR / MIR / bytecode / contract
                 -> dependency canonical build-data sections

library target code -> runtime / contract / bytecode + target dependencies
```

The producer has no production dependency on stdlib or the crate it is building. Owned .kgr/
structured inputs are readable by build.rs without calling that target crate's modules() or
binding constructors. Producer outputs private data and declaration-key glue to OUT_DIR; target
code embeds matching data and compiles adapter setup. No new producer crate or recursive Cargo.

Do not consume a build-dependency's HOST-selected embedded product as if it described TARGET.
Start with one explicit canonical semantic configuration; libraries needing variants expose a
finite explicit configuration set independently of HOST feature selection. The proposed accessor is:

```text
dependency.canonical_data(configuration)
    -> embedded bytes + frontend-free identity/requirement descriptors
```

It returns already-produced canonical contracts/MIR/bytecode, independent of the accessor's
HOST-selected features; it performs no checking, MIR production or binding construction.
The actual TARGET-selected semantic key is exported separately. HOST build-dependencies may
include the frontend; the deployed TARGET normal dependency graph must not.
Consumer build.rs
selects dependency configurations and validates requirements against TARGET/CARGO_CFG_TARGET_*;
relevant features map to explicit semantic inputs, not hidden host cfg! decisions.

Generated target integration checks actual target dependency semantic keys against the selected
keys. Mismatch fails the build; do not guess dependency features, search OUT_DIR paths, or silently
substitute HOST semantics. Physical layouts, Rust code/hooks and native/AOT objects stay target-
local. Target-specific machine code is not part of these canonical data sections.

Cargo tracks author files/configuration and producer dependencies. Compare HOST/TARGET selections
using semantic keys, not entire product envelopes. Semantic target differences require an explicit
semantic configuration. Validate portable target requirements separately; target-specific prepared
code/artifacts carry their actual target/configuration identity. Write outputs only when contents
change. Reuse unchanged builds, invalidate/rebuild stale private data. No public binary compatibility,
legacy reader or migration layer; users neither manage the encoding nor deploy an intermediate
file. Embedded data survives in its matching executable. Existing KBC/ABI contracts remain separate.

### Native API discovery for tooling

**User decision:** keep private binary prebuild products for compilation/Runtime, but deliver
build-generated .kgr declarations to LSP. These are different outputs from the same selected,
checked library model; LSP does not require binary contracts, MIR, bytecode or Rust bindings.

```text
Host/library build -> common declaration checking
                   +-> private contracts / shared bytecode / MIR -> compiler and Runtime
                   +-> exported .kgr declarations + docs         -> analyzer

Rust declarations --------------------+-> common input -> HIR -> checking (compilation)
exported .kgr -> declaration parser ---+-> common input -> HIR -> checking (LSP)
user source joins the corresponding common analysis in both routes
```

LSP reads the declaration directory; proposed default: <project-root>/target/kagari/native-api/.
A project override is shared by build integration and analyzer. No individual binary artifact
paths need configuring. Only necessary library/module/dependency/configuration attribution
accompanies the declaration files; it is not a second binary semantic input requirement.

- Build integration renders the selected Host project's library/dependency set from the same
  checked declarations used for private products. Pure compilation/Runtime never needs to parse
  this text; generating it is a separate build output for tooling, not a required semantic stage.
  Do not inspect Runtime memory or execute target Rust registration callbacks for offline LSP.
- Analyzer loads .kgr on startup and reloads on publication, parses external declarations, and
  analyzes them with user source through common HIR. Definition locations point into the exported
  read-only documents; analyzer owns navigation indexes. Parsed IDs are scoped to its own context,
  not imported raw process-local IDs from the build. No second type checker or Rust binding
  validation runs in LSP; it does not establish executable/provider authority.
- Preserve module identity, visibility, fields, generics/bounds, impl relationships and every
  caller-relevant static contract. Export complete Script definitions owned by the library,
  including Rust-embedded traits, functions, structs and real default bodies, alongside Native
  declarations. Already available source dependencies remain referenced without duplicate owners.
  No body stripping, default-stub grammar or Rust-compiled-string-to-Native conversion is allowed;
  report genuinely missing dependencies rather than fabricate definitions.
- Refresh when declarations, dependencies, configuration or exported documentation change;
  implementation-only edits need not rewrite identical declarations. Isolate configurations and
  select explicitly, never by newest timestamp. Removing a declaration removes its analyzer API.
- Stage output and atomically publish a complete snapshot after selected build/key checks succeed.
  build.rs alone cannot certify enclosing build success. Failed builds retain explicitly
  last-successful declarations. Missing/invalid exports produce an actionable build/configuration
  diagnostic, not an empty Native API. Loading exports cannot enable Native user-source authoring.
- Tooling files are disposable and unnecessary for execution deployment. Binary prebuild products
  remain separately embedded/consumed and independently verified; .kgr is not their replacement.
  Directory refresh does not authorize a new LSP scheduler or incremental compiler.

The shared declaration-input boundary and equivalent analysis belong to current NM02/NM03.
Build export, directory publication/configuration and watch/refresh integration are deferred;
they have no implementation phase in the current HIR plan. Neither route is implemented yet.

### Production versus installation

| Stage | Work |
| --- | --- |
| Host/library build | Check fixed inputs, export typed templates/shared code, compile Rust adapters |
| Business script compile | Finalize actual source dependencies, check calls/applications, select operations and demanded products |
| Detached preparation | Validate/share immutable products without recipient authority |
| Install/publication | Inject live services, import identities, match Provider, prepare necessary mappings and atomically link |
| Invocation | Execute prepared adapters with actual-value and execution checks |

Runtime never reparses/rechecks fixed library declarations. Installing a large library is not
promised O(1): decoding/verification, recipient identities and final links remain necessary.
Demand closure includes layouts, interface slots, callbacks, result adapters and retained
computations; arbitrary Rust callbacks do not imply no dependencies. Share immutable data where
valid, not Runtime authority. Some dynamic environments still need first-admission preparation.

## 7. Replacement boundaries and acceptance

The [execution plan](native-model-plan.md#execution-phases) owns current HIR-only phase exits.
Replace obsolete frontend representations directly; preserve existing registration/execution
protocols as active boundaries, not hypothetical compatibility APIs. Do not require a new
LibraryContracts/NativeProvider model, NativeDefault wire/execution migration or binary product
producer to complete this change.

The decisive witness analyzes the same API from direct structured input and parsed external
.kgr declarations, each together with user source: Native type fields, inherent/associated
functions and a Native impl of a real Kagari trait. Assert equivalent field/call types, var/val
checks, trait selection and diagnostic kinds; exported documents provide navigation locations.
No Rust implementation, running Host or new binary artifact is required for offline analysis.
Do not claim new Runtime field execution from passing frontend checks.

Existing Script defaults, reflection/output handling, source-free validation, GC/reentry,
cancellation, snapshots and reload remain intact. Preserve semantic/body inventories and
initializer/root/liveness evidence independently of optional source. Full pipeline redesign and
performance improvements are deferred, not hidden acceptance requirements.

## 8. Review decisions and limits

Current review decisions: explicit native syntax with unified parsing and semantic input admission;
shared unchecked declaration input;
ordinary HIR function/type/field/impl ownership; direct references with common accessibility;
real structured/exported-source origins; and equivalent static analysis without Runtime bindings.
There is no signature-only Script-default export requirement: analyze actual trait source.
Resolve any remaining caller-relevant Native restrictions explicitly at their semantic owner,
not through fake bodies, hidden grammar modes or unchecked assumptions.

Required consumer adjustments must preserve actual Native binding/access and current executable
behavior. If a frontend change proves inseparable from a new Runtime capability, stop that slice
and explicitly revisit scope rather than silently starting whole-stack migration.
The historical NM00 baseline has carried failures; see the plan before acceptance. Sections 4–6
retain future direction only: their product schemas, prebuild/binary-versus-generated-code choice,
producer/bootstrap, installation/caching and directory publication need separate authorization.
Ref/RefMut redesign, output/reflection extraction, GC changes and LSP scheduling are also deferred.

## References

Mechanisms inspected in local Rust commit **5f4effd579500175af844241eb179e756190f1e7**:

- [ForeignItemKind / TraitFn](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_hir/src/hir.rs): explicit bodyless foreign signatures and ordinary required/provided trait bodies; no machine callbacks in HIR.
- [Definition construction](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_ast_lowering/src/lib.rs): record node/definition correspondence during lowering, not rendered-text joins.
- [MIR export](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_metadata/src/rmeta/encoder.rs): should_encode_mir/encode_mir export reusable generic compiler data separately from code generation.
- [Mono collection](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_monomorphize/src/collector.rs): instance collection is a distinct concern; do not eagerly enumerate generic defaults.

Borrow those mechanisms, not Rust's arena lifetimes, pointer ABI, object-safety limits or source
span conventions. Kagari needs shared generic interfaces, managed roots, exact source projections
and reload selection. Build semantic configurations are an explicit Kagari Cargo-integration rule,
not a claim that rustc's metadata format solves HOST/TARGET selection for us.
Execution safety remains with [Native calls](architecture/runtime/calls.md#native-call-prepared-slots-and-mutable-services),
[reload](architecture/runtime/reload.md), [Rust storage](../crates/kagari-runtime/src/native/storage.rs)
and the [GC provider protocol](../crates/kagari-gc/src/object.rs). No performance benefit is proven.
