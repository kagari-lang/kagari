# Kagari Host Interop Specification

This document defines the host interop model for Kagari, with a focus on Rust embedding.

The main goal is to let Rust applications expose types and functions to Kagari while preserving Rust-side safety constraints and avoiding unnecessary data copies.

Runtime behavior is defined in [runtime.md](runtime.md).
Execution behavior is defined in [execution.md](execution.md).

Typed-path adapters provide `with_read`, `with_validate`, and `with_prepare_write`.
The last callback returns a `PreparedHostPathWrite` containing a single commit
action. The [failure contract](failure-semantics.md#modification-guarantees)
defines preparation, reservation cleanup, ledger publication and fault isolation.
Hosts inspect `Runtime::host_dirty_paths` after execution and clear consumed
records explicitly. There is no synchronous dirty callback. A runnable example is
`cargo run -p kagari-runtime --example atomic_host_path`; it declares and binds writable and readonly paths once, then checks atomic
commit and dirty records without changing the readonly target.
Backend abstraction is defined in [codegen-backend.md](codegen-backend.md).
Typed path mutation is defined in [typed-path-mutation.md](typed-path-mutation.md).

## Design Goals

- allow Rust functions and types to be registered into Kagari explicitly
- support efficient access to Rust-owned data without copying large objects into script memory
- preserve Rust aliasing rules at the host boundary
- keep borrowed host references scoped to a single call frame
- keep script-visible nested host field access separate from raw Rust borrowing
- support concrete registration of generic Rust APIs

## Non-Goals

The interop scope excludes:

- transparent automatic binding for arbitrary Rust APIs
- unconstrained registration of open-ended Rust generics
- escaping Rust borrows into GC-managed Kagari objects
- a full mirror of Rust's type system inside Kagari

## Core Split

Host interop is split into two concerns:

1. offline declarations of types and functions
2. runtime bindings checked against those declarations
3. borrow-boundary management

These concerns are related, but they are not collapsed into one mechanism.

Declarations decide what the script can name and call; runtime bindings supply implementations.
Borrow-boundary management decides how Rust-owned data may be accessed safely during execution.

## Type Registration

Portable type, function, method and impl facts belong to `ModuleDecl` and the
shared declaration types. `ModuleBuilder` authors these facts explicitly, `bind`
checks callbacks against the declared signature and access policy, and
`NativeModule` installs the checked records transactionally. A reusable
`NativeInstallation` produces independent runtimes with their own heap, roots,
borrow tables and physical metadata slots. Rust callbacks may share captured state
only when the application chooses to capture shared state.

`TypeBuilder::external` declares an external object identity and `field` adds common
`FieldDef` records with `HostFieldPolicy`. This descriptor contains no Rust pointer
or object storage. External root metadata, inherent methods and explicitly declared
Kagari trait impls use the same native module and callable installation as ordinary
registered APIs. A Rust struct's methods or trait impls are never inferred from a
data schema. Runtime-owned `Opaque<T>` and branded `Ref<T>`/`RefMut<T>` have their
own checked storage/lending adapters under this same callable contract.

Catalog imports retain implicit field identities from the checked TypeDef under
the canonical type owner, as they retain enum variant identities. Scoped type
references can therefore name a foreign field when declaring a manual path.
An identity alone grants neither field access nor a runtime binding; ordinary
visibility, schema, path capability and installed adapter validation still apply.

`ModuleDecl::paths` carries explicit manual paths owned by a root in that declaring
module. `ModuleBuilder::define_path` adopts a scoped declaration;
`bind_path` checks its read/prepared-write operations against declared access.
Finishing a module rejects missing, duplicate or foreign path bindings. Installation
prepares metadata, path descriptors and adapters before publication. Failure leaves
installed callables, physical slots and path descriptors unchanged. General method
callbacks still use `bind`; path read/write hooks implement storage operations,
including preparation, commit and dirty reporting.

`HostPathDeclaration` and its bounded wire representation belong to
`kagari_types::declaration::path` and `declaration::wire::path`. They use common `Ty`
and `TypeDeclaration` schemas without a second host type DSL. Source syntax selects
these exact declarations; MIR carries their checked facts and bytecode records
them in `BytecodeModule::path_declarations`. Source-free loading requires exact
installed schemas and path bindings, including unused path declarations.
`Runtime::resolve_host_path` looks up an installed scoped declaration without
registering another descriptor. The runnable
[atomic path example](../../crates/kagari-runtime/examples/atomic_host_path.rs)
shows the complete declaration, binding, installation and checked access sequence.

Root handles are created only by `register_host_root` and carry registry ownership,
schema epoch and ABI identity. Calls, temporary scopes and path-view chaining reject
foreign or expired roots even when object IDs, type slots and schemas coincide.
Native arguments match the installed result type of a path view and validate every
parent descriptor, dynamic argument and access capability against that root.
A shared parameter receives a rooted readonly alias of a writable path view;
readonly parents cannot create writable children or satisfy a unique parameter.
The caller's original view retains its capability. Manual path callbacks borrow
the actual whole root while running, preserving their existing reentry conflicts.
Metadata does not authorize unrestricted Rust references or turn a DTO into a
borrowed host object. Common declarations are the sole schema owner for both
source compilation and source-free path validation.

## Fixed-width numeric host boundary

Portable native numeric declarations use Rust `i64`/`u64` for the corresponding
script types; standard lengths, indices and counts use `u64`. Rust `isize`/`usize`
scalar convenience adapters map to script `i64`/`u64` with checked host-range
conversion. They do not register additional script identities or aliases. An
unrepresentable script argument fails before the Rust callback is entered; a
callback needing the full script domain must accept a fixed-width parameter.

Scalar conversion does not authorize slice reinterpretation. Zero-copy native
element views of script 64-bit integer buffers use `&[i64]`/`&[u64]`, never
`&[isize]`/`&[usize]`. Actual Rust allocation and storage positions can still use
`usize`, subject to the operation-owned checks described under
[machine and resource boundaries](runtime.md#machine-and-resource-boundaries).

## Character host boundary

Rust `char` maps exactly to Kagari `char` through the ordinary typed scalar
adapters (`KagariType`, `IntoKagari`, `FromKagari` and `NativeScalar`). Typed call
arguments/results, registered fields and supported generic containers preserve
that type. Rust `u32` and single-scalar String values are not substitutes for a
character; crossing those domains requires the explicit
[character conversions](builtins.md#character-scalar).

With the optional runtime/SDK `serde` feature, `KagariSchema` supports `char` and
`Serde<T>` uses Serde's character serialization/deserialization operations, including
inside supported aggregates. It does not serialize a character into a Kagari String
or admit an integer as a character. Normal schema, value, root and scoped-borrow
checks still apply; no host reference or unchecked scalar payload is exposed.
See [typed native registration](embedding-api.md#typed-native-registration) for the
shared conversion and optional Serde limits.

## Function Registration

Rust consumers import each API from its owning module. For example,
`KagariEngine` belongs to `kagari::engine`, `NativeApi` to
`kagari_runtime::native::api`, typed conversions to `kagari_runtime::native_value`,
and array handles to `kagari_runtime::native_value::array`. The `native_module`
attribute belongs to `kagari_native_macros`; consumers declare that direct
dependency. Native type wrapper expansions also name `kagari_types::ty::Ty`
and require a direct `kagari-types` dependency. Crate roots do not re-export these
items. Any future re-export requires a reviewed exact whitelist entry under the
[structure-check policy](../structure-checks.md).

The native package API accepts actual Rust functions through
`#[native_module("game::math")]` and `#[native]`. `#[native_type]` supports
type aliases and single-field tuple wrappers backed by `NativeRepresentation`.
The adapter's resolved Rust type supplies the closed engine constructor and enum
variant shape; registration checks generic arity and payload slots. Checked
`NativeArray<T>`, six immutable `NativeRange<T, Shape>` proxies and Rust `Bound<T>`
provide current storage representations; `#[native_trait]` and
`#[native_impl]` export actual Rust required traits and implementations. Rust
checks bodies, return types and trait conformance. `NativeValue` conversions
supply scalar, String, Option and checked array metadata, including Rust aliases.
Open generics require inline `T: NativeValue` and compile once using checked rooted
`GenericValue<slot>` proxies; this does not register arbitrary Rust generics.

`NativeResult<T>` returns T to script and propagates RuntimeError as a trap.
`#[context]` injects a checked NativeCall without adding a script argument.
NativeArray operations preserve bounds, aliasing and heap guards. Readonly
applications permit reads and reject mutations. NativeFn<(u64,), T> supplies the
checked index callback for the current array proof; NativeContinuation<T> reuses
the rooted resumable driver and cooperative polling boundaries. The registered ops Index is a by-value
Rust bridge for the existing script indexing contract. It returns checked values
and does not expose unrestricted Rust references. NativeRange preserves complete
integer widths without converting endpoints to signed offsets. Bound payloads
use ordinary NativeValue conversions and roots, including arbitrary script objects.

`module::native_api()` builds validated NativeModule records and typed factories.
`KagariEngine::builder().install(module::native_api()).build()` installs application
packages along with the optional default package. Generated `.kgr` provides
navigation and documentation, while compiler signatures are imported directly.
Advanced NativeApi::new/NativeFactory registrations retain runtime result,
application, ownership and callback validation. External root and path adapters
use the same declaration, bind and installation owners.

Declare a function with `FunctionDecl`, including its parameter/result `Type`
handles, then attach its callback with `ModuleBuilder::bind`. Signatures use the
ordinary scoped `Ty` tree. Manual external-ID callbacks explicitly attach a
`HostCallPolicy` for argument passing and effects. Common native entries retain
that checked declaration and its complete dependency closure; there is no second
runtime function registry or inferred signature authority.

Host parameter types from offline declarations provide expression context before
argument checking, including nested containers and empty Array literals. The same
parameter comparison rules apply to script and host calls: an argument that exits
the enclosing function on every path produces no parameter value and does not
invoke the host callback. Arity checks, inner diagnostics and incompatible values
on normally completing paths still reject compilation.

Callbacks receive `(&HostCallContext, &[Value])`. The context exposes the checked
runtime and a call-scoped borrow guard; it cannot be constructed by hosts. Runtime
entry registers a HostResourceScope in the active session and keeps argument roots
and borrow leases there until the call ends. Callbacks can add temporary values with
retain_temporaries; permanent retention still requires RootedValue. Borrowed
results are rejected before their scope ends. The public registry/function invoke
bypasses are removed; host invocation goes through Runtime effect, cancellation,
signature and heap validation.

`kagari_vm::reenter(context, loaded, function, args)` drives the existing explicit
frame executor synchronously. The loaded handle and FunctionRef select a linked
version, which must belong to the current root program.
Reentry does not switch to the latest epoch. Argument and
result representations are checked against the linked signature, and arguments
must contain valid references owned by this runtime. The return is a RootedValue;
keeping it alive retains the object across subsequent GC and host operations.
Outer frames and borrow guards remain live while nested calls run. Borrow conflicts
are checked across these scopes, and each scope releases only its own resources.
An ordinary nested trap may be handled by the host. Cancellation or resource
termination, including call-depth and native conversion exhaustion, remains
recorded for the root even if the host ignores the error. The first termination
and origin survive ordinary observer failures; EngineFault/quarantine takes
precedence, as defined by [failure semantics](failure-semantics.md).
Nested calls share the session's frame stack and debugger observer. Breakpoint and
trap snapshots include suspended callers at their actual call instruction, plus
the nested frames. The synchronous callback path currently uses the interpreter.

SharedBorrow/UniqueBorrow parameters acquire corresponding object leases when
passed a HostRoot. Forwarded borrow tokens must belong to this runtime, still be
live and satisfy the required mode; they cannot satisfy Owned passing. Conflicting
arguments reject before invoking the callback, releasing leases already prepared
for earlier arguments. Borrow tokens carry a table owner in addition to frame and
epoch, so coincident frame numbers in other runtimes confer no authority.
Formal shared arguments also weaken unique tokens before callback argument getters
can observe them. The alias retains its checked owner/frame/epoch and expires with
the original frame; it cannot validate unique access. The original token remains
valid with its original capability until that frame ends.

Runtime::host_scope creates explicit temporary scopes and validate_host_borrow
checks tokens; the old enter_host_call and mutable borrow-table access are removed.
Scopes participating in a root retain its options/version until the last scope
ends. Root and lease cleanup is unconditional after traps, cancellation, call-depth failures
and quarantine. Direct host operations outside script execution use the same scope
implementation with runtime defaults and do not implicitly create a script root.

Common native callbacks use their canonical qualified declaration name in host
call traces, such as `demo::limit` or `demo::api::limit`. The
[runtime offline example](../../crates/kagari-runtime/examples/offline_host.rs)
encodes shared declarations in KBC before installing an implementation, calls the
linked entry through ordinary rooted native frames, and retains trace inputs,
immutable configuration access and candidate initialization across reload.
Run `cargo run -p kagari-runtime --example offline_host`.

`NativeModule::to_declaration` produces cold, address-free `ModuleDecl` metadata
for analysis and tooling. It retains its definition table and imports IDs
explicitly when combined with another scope. KBC and portable MIR serialize their
common native, external schema and path contracts through the codec-owned records.
The standalone host declaration format and its reader are removed.

Loading checks all required common declarations against installed bindings without
calling them, including unused schemas, trait references and paths. It rejects
missing or conflicting identities, signatures, access policies, passing styles,
effects and storage capabilities. Documentation is outside the semantic contract.
Canonical encoding uses only referenced definitions and their ancestor closure;
independent scopes and unrelated definitions do not change semantic identity.
Binding compares complete contracts; a matching fingerprint alone is insufficient.
[Artifact limits](artifacts.md#logical-layout) apply before encoding and hashing.

`HostFunctionEffects::may_read_immutable_configuration` is part of the binding
contract and function fingerprint. It permits snapshot configuration reads during
candidate initialization, subject to declared effect and borrow contracts. Inputs and results
must be owned value-only shapes (scalars, String, Tuple, Option and Result);
containers and opaque handles are rejected, including nested occurrences. The host
must supply an immutable snapshot, not live mutable service state. Declaring this
bit does not override any other forbidden effect.

The source `print` entry and explicit CLI log binding use the common cold
`kagari_types::declaration::logging::declaration()` ModuleDecl. Adopt it with
`ModuleBuilder::from_declaration`, obtain `declared_function("log")`, bind a
HostCallContext callback and install the resulting NativeModule. The declaration
owns its String parameter, shared passing capability and service/trap/mutation
effects. No callback runs during authoring or compilation.

Print can compile without an installed logger: its checked declaration-only
provider enters the ordinary MIR/bytecode native module graph. Native imports,
link proofs and exact installed contract checks apply normally. This requirement
does not export a source namespace or supply a default callback; missing binding
rejects at load. An explicitly installed log module permits ordinary source
imports of `host::log`. Its canonical owner is the root `host` module, as for any
other common registration. The old `standard_log` factory is removed.

The compiler consumes immutable common ModuleDecl metadata. Standalone compiler
users supply cold declarations through AnalysisDatabase::set_native_modules.
SDK users bind and install NativeModule values through KagariEngineBuilder::install
or install_all, then build an engine with a sealed declaration and binding closure.
That engine can compile before creating a runtime; authoring and compilation do not
execute callbacks. The former KagariEngine::set_host_interface entrypoint is removed.
Source may call demo::echo directly, import demo::echo with an optional alias, use
a nested import group, or import demo as a module alias. Source paths use double
colons; the module identity and its actual declaration IDs select the binding.
The std namespace is reserved. Ambiguous function/module paths and unspellable
declaration paths are rejected. Unknown imports and duplicate import aliases
produce source diagnostics instead of disappearing during lowering.

Native declaration inputs participate in analysis caching and body reuse.
Snapshots retain their checked declarations, signatures, documentation and scoped
definition IDs; a definition from another context cannot resolve by coincident index.
FileAnalysis's source_function_at query returns the ordinary callable target at a
free-function callee or import without executing code, including through source
facades. Definition and member queries use the same checked declaration identities.
Changing the catalog invalidates semantic reuse while unchanged source can reuse
its parsed CST. Correct neighboring functions remain queryable after import or
call errors. Host calls require installed bindings and are excluded
from scalar constant evaluation.

The source call boundary supports scalar and nested Tuple, Array, Map, Set, Option
and Result signatures. Key/element eligibility follows the declared
[collection protocols](builtins.md#collection-types), not a fixed scalar whitelist:
HashMap keys and HashSet elements require checked `Eq + Hash` (including `char`);
Map/Set interfaces do not impose those bounds themselves.
Composite arguments use `Owned` passing: shared mutable objects retain their
identity, and tuples retain value semantics. This does not provide a Rust borrow
lease or make a naked Value an owning GC root. Owned arguments reject frame-scoped
host borrows even inside nested tuples; script containers cannot store them.
Arguments stay rooted during the
callback; longer host retention requires an explicit rooted handle. Both argument
and result checks inspect nested members and standard enum tags, reject foreign
or stale heap references, and observe execution termination while traversing.
Argument mismatch prevents callback execution. A result mismatch rejects the
result without rolling back effects already performed by the callback.
Opaque host types resolve in source annotations and call signatures by declaration
identity. Direct imports, module aliases and source facade re-exports share the
same semantic targets. Host types are distinct from script structs, even when
their names coincide; they do not support script construction or generic equality.
Ordinary definition queries expose the checked type identity and its owning
schema; declaration views supply their actual generated source locations.
Erroneous applications retain the base target, and immutable snapshots retain
their original native declaration facts after provider inputs change.
Nominal host references survive public ABI encoding and generic function
instantiation. Lowering collects required type declarations and their transitive
member references in identity order, including dependencies used only in source
signatures. Unused catalog types are omitted. Linking checks these complete
contracts before publication, even if there are no host calls in the program.
`HostHandle` is a separate execution representation; script heap objects cannot
satisfy an opaque host parameter's representation. Runtime nominal identity,
ownership, borrow and escape checks still apply. This does not authorize host
handles or borrows as default script heap payloads. The checked durable-root
exception for interface payloads is defined under [Interaction with Traits](#interaction-with-traits).
Explicit Kagari trait implementations use the ordinary ImplDecl, InterfaceTable
and NativeDeclaration contracts. Their concrete applied signatures require an exact
method roster and matching receiver, parameter and return types after associated-output
substitution. Source checking and portable verification retain these checks; Rust
trait implementations alone supply no Kagari witness. Passing styles, receiver
capabilities and effects remain checked native binding facts.

Host methods are callable through their receiver: `player.read_score()`. The common
function declaration owns the explicit self parameter and member signature once;
ModuleBuilder binds the callback to that declared method. Installation checks the
complete declaration/binding closure, and loading rejects missing or mismatched
native imports even if the external type schema is installed.

Analysis resolves method targets without starting a runtime. HIR retains the receiver
expression and member identity even when arguments have errors. source_function_at
returns the checked callable target; ordinary declaration/documentation queries use
its owning view. Lowering evaluates the receiver first and each explicit argument
once, then emits an ordinary native import. Methods are not free-function exports
through a type namespace. Canonical declaration identities select calls and retain
declared effects and borrow checks.
Receiver borrow leases use the ordinary host call scope, including cleanup after
failure and rejection of conflicting synchronous reentry. Method source calls,
artifacts and existing JIT fallback share this execution path. Opaque receivers
remain roots or valid borrow tokens; this does not make path views opaque objects.
Public host function, type and module re-exports retain their offline declaration
identities through source facades. The import graph resolves the final binding once; name resolution,
signature catalogs and navigation consume it. The original source dependency is
retained for linking even when all calls target hosts.
For example, `pub use demo::echo as call; pub use demo as service;` permits clients
to import `call` or `service` from the facade, or call `facade::call(...)` and
`facade::service::echo(...)` through a source module alias. These calls retain
ordinary lexical shadowing and require the original host symbol's identity and
matching runtime binding; re-exporting grants no additional authority.
The [offline compilation example](../../crates/kagari/examples/offline_compile.rs)
authors ordinary shared `ModuleDecl` records, queries imported type/field/method
identities, then compiles and decodes a KBC with exact shared schema and path
requirements. It creates no runtime, binding or callback. Run it with
`cargo run -p kagari --example offline_compile`.

Required bytecode declarations now link mandatorily before module
publication, including artifact reloads. Compiled host calls use registry-owned
slots; labels remain declaration and policy metadata. A wrong runtime cannot
reuse a coincident slot. Loading executes no callbacks. See [bytecode.md](bytecode.md)
and [artifacts.md](artifacts.md) for the executable and encoding contracts.

Rust functions are also registered explicitly.

Conceptually:

```rust
registry.register_fn("game.heal", |player: &mut Player, hp: i32| {
    player.hp += hp;
});
```

Function registration records:

- exposed symbol name
- parameter list
- passing style for each parameter
- return type

This aligns with the existing host runtime surface in [host.rs](../../crates/kagari-runtime/src/host.rs).

## Passing Styles

Host parameters distinguish between:

- owned values
- shared borrows
- unique borrows

This is the minimal set needed to model Rust interop safely.

Conceptually:

```text
Owned
SharedBorrow
UniqueBorrow
```

These remain host-boundary concepts even though Kagari itself does not implement Rust-style borrowing internally.

## Ordinary Owned Values

Owned values are appropriate for:

- small copyable Rust scalars
- values intentionally marshaled into Kagari
- script-owned wrappers

Examples:

- `i32`
- `bool`
- `String` when copying is acceptable
- small POD-like structs

Owned passing is simpler but may be too expensive for large host-owned structures.

The registered representation determines conversion and borrowing:

| Script input | Owned Rust `T` | Rust `&T` | Rust `&mut T` |
| --- | --- | --- | --- |
| Ordinary GC DTO | Independent schema-directed conversion | Rejected | Rejected |
| Runtime-owned opaque target | Explicit host `Opaque::take` only | Checked payload loan | Checked payload loan with installed editing capability |
| `Ref<T>` | No implicit snapshot | Checked scoped loan | Rejected |
| `RefMut<T>` | No implicit snapshot | Checked scoped loan | Checked scoped loan |

DTO schema registration exposes data fields only. Rust methods and Rust trait
implementations supply no script members or Kagari trait evidence. Declare each
host member once and bind its callback explicitly; supported opaque and external
receivers use that same member according to its declared access. A separate DTO
snapshot and opaque representation use distinct explicit binding types. The
[value conversion guide](embedding-api.md#typed-native-registration) describes
the finite derive/Serde subset and directional converters.

## Native payload accounting

Advanced runtime-owned storage implements
`NativePayload::charge(&self) -> NativeResult<HeapCharge>` instead of `units()`.
`HeapCharge` carries logical payload units excluding the heap identity, and bytes
for exclusively owned indirect backing. The runtime adds the erased payload Box's
shallow size exactly once; providers must not include it again. Referenced GC
objects, shared metadata and external host resources do not belong to that charge.
Report known capacities where available, distinguish estimates, and document
unobservable allocations as excluded rather than asserting an exact zero.
These are GC-pressure contributions, not process-memory totals or quotas.

The callback must be side-effect-free and must not reenter the runtime. Payload
preparation samples it once before publication; a failure publishes no payload or
charge. The runtime-owned native record then owns the authoritative charge.
Installed logical units and attributed backing must remain stable except through
checked runtime storage mutations; interior mutability is not permission to resize
that backing. Fixed-data loans cannot change its size, and managed replacements
prepare a new charge before committing. Sweep, extraction and cleanup release the
recorded charge without calling the provider again, including cleanup after
termination/quarantine. This does not authorize take or mutation after execution
has been rejected. See [GC accounting](runtime.md#gc-heap) and the
[reservation failure contract](failure-semantics.md#modification-guarantees).

## Registered payload loans

An instance `MethodDecl` may explicitly set `receiver_access` to
`ReferenceAccess::Shared` or `Mutable`. The required capability belongs to its
ordinary callable declaration, including portable metadata and trait/default
application; it is independent of Rust trait implementation. The common `bind`
adapter and installation validator require agreement with the selected callback.
Owned/raw callbacks cannot silently claim the borrowed receiver contract.

The borrowed callback takes `&mut LoanContext` and an outer argument tuple whose
first element is `&T` or `&mut T`. It must work for every loan lifetime and return
an owned, static result. `LoanContext::poll` provides cooperative cancellation;
it exposes no script invocation or heap allocation. Captured runtime handles
still encounter the ordinary non-reentry checks during Opaque access.

Opaque adapters validate Rust mappings and installed receiver capabilities before
argument conversion, then validate/root the entire input pack and decode owned
arguments before acquiring the payload loan. Complete-pack roots remain until
result conversion finishes. The existing GC guard releases access before result
allocation and on errors/unwind; completed writes remain visible. Mutable access
requires the installed fixed-data proof, not merely the callback's `&mut T` type.
Traced, managed and take-only registrations do not gain arbitrary editing.
External scope and path adapters consume the same declaration and binding facts
through the common invocation boundary described below.

## Borrowed Host Values

`core::host::Ref<T>` and `RefMut<T>` are declared through the ordinary native type
constructor path and exported by the prelude. Their semantic record contains the
target type and Shared/Mutable capability, never a Rust pointer, field offset or
second nominal target declaration. The initial target subset is ordinary nominal
structs and opaque native payload types; generic target binders are checked again
after application. Collection, Future and Task storage does not grant this payload
reference contract. Ordinary DTO values cannot implicitly become a Ref/RefMut.

Direct view parameters and locals may be forwarded with their checked capability.
An outer RefMut<T> may supply Ref<T> while retaining the same target; nested type
parameters remain invariant. At native entry, a shared formal parameter (or a
shared registered receiver) gets a distinct inert shared view. Raw callbacks,
ValueHandle and ArgumentView all see that capability; forwarding it to a mutable
callback rejects before effects. The caller's original mutable view is unchanged.
The restricted argument pack stays rooted across conversion, collection and
reentry, and its call-scoped root leases end on return, error or unwind. Calls
without actual capability narrowing keep their existing borrowed frame/scalar
argument representation and do not create this root pack.

Explicitly loan-capable native inherent methods use
the same registered member through Opaque, Ref and RefMut. Ordinary owned members
do not acquire view support automatically. Call applications retain their logical
nominal signature and carry checked required access for the actual storage view.
MIR/source-free flow validates the target and capability; linking validates access
against the original member declaration. This does not imply a Kagari trait impl
for any view type or provide Rust access without an installed scope adapter.
Views cannot be returned, captured, placed in owning fields/containers or retained
across suspension; shared capability cannot upgrade to mutable capability. A
function signature accepting a view is distinct from a function value capturing
one. Source checking revalidates callable parameter and result positions after
generic substitution and associated-type normalization. A generic identity
function cannot return a view through an instantiated `T`, even when its caller
immediately reads a field from that result; the diagnostic belongs to the call.
Functions that accept views and return owned values remain valid callable values.
Source-free verification checks targets, positions, flows and suspension
independently. A valid schema/signature alone does not grant runtime Rust access:
installed scope, target, owner/generation and loan capabilities are also required.

The runtime entry `Runtime::with_host_scope` reserves the runtime exclusively and
supplies a fresh invariant `HostScope`. `scope.borrow(&rust_type, &host_value)`
and `borrow_mut` lend external storage without copying or taking ownership.
`Runtime::bind_rust_type::<T>(&owner)` prepares a registered ordinary struct target;
`NativeType<T>::rust_type()` supplies that same metadata-only descriptor for native
storage. Both use one Rust target/owner/generation validator. External T needs
Send + 'static, without NativePayload or a blanket Sync requirement. Runtime-owned
creation still requires its storage proof.
The original host loan lasts until scope exit even if its branded view is dropped.
Scope/view values cannot transfer between threads. Execution stays within the scope;
the interpreter's `Vm::execute_scoped` requires explicit ExecutionOptions, uses the
ordinary checked public entry and validates its concrete parameters before execution.
The SDK facade uses Runtime::with_host_scope and its static execute_scoped
with explicit ExecutionContext; it forwards cancellation, inputs and observation,
and rejects unsupported JIT policies before entering the interpreter. Heap results remain rooted
until scope exit; an owning handle is required to retain them beyond that scope.

External addresses live in the existing host borrow frame owner. GC view headers
contain only owner/frame/epoch/object/access identities, never an address. Rust
type and nominal target checks precede access. A loan-capable method on an ordinary
struct accepts an external Ref/RefMut, while owned GC structs retain DTO semantics
and cannot call that Rust loan. Source and portable flow reject such reinterpretation. Compatible shared loans may
nest; any overlapping loan involving a writer fails. Distinct zero-sized Rust places
may share an address, so their reservations receive separate identities; aliases of
one view retain its original identity and conflict normally. Reservation and an active
native loan are distinct: lending a mutable root does not block its later methods.
The same loan-capable binding decodes owned arguments first, selects runtime-owned
or external storage, lends the receiver, and releases that loan before converting
the owned result. External edits do not need the runtime-owned data edit flag,
because their memory/accounting belongs to the host. Opaque edits retain that flag.
Errors and supported Rust unwind release loans while preserving completed writes.
Scope exit removes all external root records and revokes every old view identity;
retaining a naked script Value does not extend its Rust lifetime or permit access.

For derived ordinary inline structs, `Runtime::bind_rust_field::<T, F>(&target,
"field")` resolves the generated field proof once. `view.field(scope, &field)`
creates a branded child location with the parent's access capability, without
constructing a Rust reference or caching its address. Nested descriptors use the
registered child Rust target. Schema-only/Serde mappings and mismatched Rust field
types cannot supply this proof. Opaque fields describe retained handle wrappers;
they cannot reinterpret those wrappers as pointers to their payloads.

The existing borrow table records generated field-identity paths. The empty path
is the whole root; equal and prefix paths overlap, and divergent inline fields are
disjoint. Any overlapping loan involving a writer rejects before constructing a
Rust reference. Active ancestor/child loans block replacement; an idle view resolves
the current reserved root and raw field chain on its next access. Generated code
uses raw field addresses without creating an ancestor reference. Scope exit revokes
child locations along with their root.

Checked aggregate field operands distinguish GC fields from scoped receiver access.
HIR and source-free verification share the result rule: nested ordinary structs
produce child views; supported scalar/string and retained handle readers produce
owned values. A read-only field downgrades its child capability to shared. Runtime
access requires the root's generated direct field proof and leaf reader; schema-only
or Serde metadata supplies neither. Arrays, tuples, enums and collection element
projections remain unsupported. Rust owned containers have no implicit snapshot
reader. Opaque field reads clone the owning wrapper and retain its payload identity;
they cannot dereference the wrapper as a payload loan.

Assignment captures the receiver identity before evaluating its RHS. Owned field
decoding finishes before path acquisition, then the raw chain resolves current
storage and replaces exactly that field. Compound assignment reads the current
terminal value after the RHS, preserving ordinary assignment order. If the RHS
replaces player.stats with hp=9 and returns 2, player.stats.hp += rhs commits hp=11.
Rebinding the receiver local during the RHS does not retarget the prepared root.
The displaced Rust value is destroyed after the exclusive path loan ends; completed
RHS/conversion effects survive later conflicts. Active equal/ancestor loans block
replacement while proven sibling paths remain usable during synchronous reentry.

Roots retain their immutable local projector catalog and program lease. Unchanged
complete layouts remain compatible across reload under the existing rule; changed
registered schemas reject installation. Portable field records contain owner/type
identities, slot and access, never an address, offset or Rust function. New child
views and string/handle values retain execution roots through frame publication.
These allocating/reentrant operations have conservative native effects. Ordinary
GC struct field kernels retain their heap behavior.

The common bind adapter supports one borrowed free-function input or two borrowed
inputs in shared/mutable combinations, followed by owned inputs (at most twelve
arguments total). The first input can also be an Opaque method receiver declared
with the matching receiver access. Reference parameters require external locations;
they cannot reinterpret ordinary GC structs or retained handle wrappers as payload
pointers. Owned inputs convert before loan acquisition. The complete external pack
is checked/acquired before raw projection or reference creation. Conflicting packs
release partial acquisitions without callback entry; completed conversions/writes
remain observable. Loans release before owned result conversion. Opaque's original
GC payload loan remains active during its callback and does not acquire external
reentry capabilities by analogy.

Borrowed host values are the important case for low-copy interop.

Model:

- `&T` becomes a frame-scoped shared borrow handle
- `&mut T` becomes a frame-scoped unique borrow handle

These handles are script-visible only through host-boundary objects.
They are not ordinary Kagari values that may live freely in the GC heap.

This is the intended use case for:

- large game state objects
- ECS views
- UI trees
- simulation state
- other long-lived Rust-owned data

## Frame-Scoped Borrow Rule

All Rust borrows passed into Kagari are scoped to the dynamic extent of a single host-to-script call.

This is the core safety rule.

Conceptually:

```text
HostCallGuard {
  frame_id: FrameId,
  borrow_table: BorrowTable
}
```

Borrowed handles carry enough metadata to validate:

- which frame created them
- which runtime borrow table owns them
- whether the borrow is shared or unique
- which concrete host object they refer to

## No-Escape Rule

Borrowed Rust references must not escape the call frame that created them.

That means they cannot be:

- returned from script back as ordinary long-lived values
- stored inside GC-managed script objects
- stored in globals
- captured by closures
- carried across `yield`, `await`, coroutine suspension, or hot-reload boundaries

This rule is enforced by the host interop layer and the semantic/runtime validation passes.

## Enforcement Model

The no-escape rule does not rely on a single mechanism.

The enforcement model has three layers:

1. representation restrictions
2. static validation
3. runtime guard checks

This keeps the common case fast and makes failures easier to classify.

### Representation Restrictions

Borrowed host values are not modeled as ordinary freely storable script values.

Instead, they are represented as a special frame-scoped kind of runtime handle.

Conceptually:

- ordinary script values may be stored in locals, fields, globals, closures, and GC objects
- borrowed host handles are frame-scoped and non-storable by default

The following are illegal at the representation level:

- storing a borrowed host handle into a GC object field
- storing a borrowed host handle into a global slot
- placing a borrowed host handle into a closure environment

The representation excludes invalid states before runtime recovery would be required.

### Static Validation

The front end rejects obvious escape paths before execution.

Examples that produce diagnostics:

- returning a borrowed host value
- assigning a borrowed host value into a location that may outlive the current frame
- capturing a borrowed host value in a closure
- holding a borrowed host value across `yield`, `await`, or other suspension points

This does not require a full Rust-style borrow checker.
It only requires tracking which values are frame-scoped host borrows and which operations would cause them to escape.

### Runtime Guard Checks

Runtime checks are still necessary as a final line of defense.

Each frame-scoped host borrow token carries enough metadata to validate:

- which call frame created it
- whether the handle is still valid
- whether the current operation is compatible with the borrow kind

Metadata:

```text
FrameHostBorrowToken {
  frame_id: FrameId,
  object_id: HostObjectId,
  borrow_kind: Shared | Unique,
  type_id: TypeId,
  epoch: BorrowEpoch
}
```

Whenever a host borrow is dereferenced, the runtime verifies:

- the current frame matches `frame_id`
- the handle has not expired
- unique-versus-shared rules are respected

These checks prevent front-end omissions from turning into Rust undefined behavior.

## Failure Classification

[Failure semantics](failure-semantics.md) is authoritative for cleanup, resource
termination, partial business effects, and runtime reuse.

Violations of host-borrow rules are not normally handled with `panic!`.

Failure model:

- compile-time detectable escape: diagnostic error
- runtime misuse caused by script execution state: script trap or runtime error
- violated engine invariant: panic or debug assertion

Examples:

- `return borrowed_player` is a compile-time error
- using a host borrow after its frame expired is a runtime error
- dereferencing an already-invalid internal borrow handle without checking is an engine bug

This distinction matters because script mistakes are user-facing errors, while panics are reserved for Kagari implementation bugs.

## Suspension Boundaries

Borrowed host values are explicitly non-suspendable across `.await`.

Rule:

- a suspension point is illegal if any live local is a borrowed host handle

This can be enforced through a targeted liveness check rather than a full general borrow analysis.

## GC Interaction

Borrowed host values are not representable as ordinary GC-managed field values.

Rule:

- GC object layouts accept only storable script values
- frame-scoped borrowed host handles are excluded from that set

This provides a strong structural guarantee that host borrows cannot silently persist in heap state.

## Alias Checking at the Host Boundary

Even if Kagari itself does not implement Rust-style borrow checking internally, the host boundary still must preserve Rust's aliasing rules.

In particular:

- the same Rust object must not be exposed as two simultaneous unique borrows
- the same Rust object must not be exposed as both a shared borrow and a unique borrow at the same time

These conflicts are checked when arguments are marshaled into a host-call frame.

This rule applies to actual frame-scoped host borrows.
It does not forbid multiple script-visible host path views that share the same root, because path views are root-plus-path handles rather than live Rust borrows.
Conflicts for path operations are handled when each checked path read or mutation is executed.

## Runtime Representation

The runtime representation uses the host value categories present in [value.rs](../../crates/kagari-runtime/src/value.rs):

- `HostRef`
- `HostMut`

They are frame-scoped handles rather than unconstrained runtime values.

Conceptually:

```text
FrameHostBorrowToken {
  frame_id: FrameId,
  object_id: HostObjectId,
  borrow_kind: Shared | Unique,
  type_id: TypeId
}
```

## Kagari-Side Semantics

Kagari script code does not need to spell Rust borrow syntax directly.
Normal member syntax may be backed either by ordinary script-owned values or by host-backed typed paths.

Rules:

- host metadata determines the host passing style
- script code uses normal calls and member access
- host-backed field and index chains lower to typed path operations where appropriate
- frame-scoped host borrow handles are not the script-visible model for nested field access

`HostPathDeclaration` accepts `HostPathSegmentDeclaration`. Its `Field`
variant contains only a field declaration identity. Registration resolves that
identity against the current path owner's declared fields and generates the
field slot, name, result type, path permission and member fingerprint from the
registered declaration metadata. Private fields and fields without path access
are rejected. The old caller-supplied field metadata input is removed.

Registration also checks type continuity before publishing a descriptor: an index
collection must match the preceding segment's result (or the root for the first
segment), and field identities must belong to that current owner. The requested
result type and access must agree with the generated segments. Rejection neither
publishes a descriptor nor consumes its slot. `HostPathSegment` is the resolved
output exposed to adapters.

`HostIndexSegmentDeclaration` and `HostVirtualSegmentDeclaration` now carry
portable result and input types (`Ty`) rather than runtime `TypeId`
slots. Registration resolves these against the current runtime after validating
the declaration, rejects missing nominal dependencies before publishing a
descriptor, and derives the same path fingerprint even when unrelated runtime
type registrations shift slots.

Index and virtual registrations do not accept a caller-supplied member
fingerprint. They have no portable member declaration yet, so their member
fingerprint slot is zero. The versioned whole-path encoder still includes the
index slot and portable collection/index/result types, or the virtual name and
result type, plus access and schema. Identical resolved contracts
produce identical path fingerprints; changes to those inputs change the path
fingerprint. These segments can be included in an offline `HostPathDeclaration`.

The common declaration layer owns `HostPathContract` and the whole-path ABI encoder;
runtime registration cannot supply its own fingerprint. `HostPathDeclaration::contract`
resolves a path by declaration identity and portable segment contracts, validates root ownership, visibility
and path access, and returns its result type and portable contract without callbacks
or runtime registration. Runtime registration uses the same encoder. The
`kagari-host-path-v1\0` encoding uses FNV-1a 64 over fixed-order fields:
root declaration fingerprint, result type fingerprint, schema epoch, access,
segment count, then ordered segment records. Integers and lengths
are little-endian u64; tags and flags are bytes; virtual names are length-prefixed
UTF-8. Each segment includes its kind-specific input contract, result type, access
and member fingerprint. Types use portable host type contracts, never runtime IDs
or display strings. Types without such contracts reject registration. Field
declaration fingerprints and the root contract exclude documentation. Reordering
unrelated runtime type registrations therefore does not change a path fingerprint.

Common module declarations and KBC executable requirements retain portable
`HostPathDeclaration` records: root identity, ordered field/index/virtual segments,
access and schema epoch. Duplicate records, invalid chains, noncontiguous dynamic
argument slots and paths longer than 256 segments are rejected. Installation
derives physical descriptors from the same schemas and fingerprints before any
callback runs. Linking checks required contracts even if a module has no path
instruction; missing or ambiguous bindings reject publication. Source field and
index syntax selects one declared ordered path by field or virtual name and the
exact type of each index expression. The suffix can mix fields, multiple indexes
and virtual members. Each source index step must use a distinct declared dynamic
slot; its evaluated value is passed to that slot even when slot order differs from
source order. The root and each index evaluate once, left to right; assignment
captures them before its RHS, while the final read or modification uses one linked
descriptor. Intermediate segments are not read separately. Required host types
include nominal values reached through index and virtual segment results.

Bytecode path records require that contract fingerprint. The loader resolves all
records before publishing the program, rejects missing/ambiguous contracts and
checks result representation and write access. Each loaded version holds its own
path bindings. VM path operations consume those bindings; registering descriptors
later cannot retarget an existing version. Debug names are diagnostic text only.

Source field reads select a unique declared nominal field chain. HIR owns the
field identities, result types, root expression and selected contract; missing or
ambiguous paths produce `KG_HOST_PATH_INVALID` while keeping known field facts
available to tools. `FileAnalysis::field_at` selects the common field identity;
resolve its type/access facts in the owning shared schema and its documentation
in the declaring ModuleDecl's documentation map. External fields need not have a
source declaration location. These recovery facts do not supply an executable
path or make an invalid program loadable. Nested chains lower to one `ReadPath`, evaluating
the root exactly once. IR validates the selected contract against required host
types; bytecode carries the required field declaration and linked path record.
Reusing an unchanged function body remaps its path root to the new analysis arena.

Source field assignments and compound assignments use checked writable path
declarations. HIR records the target's root place and contract; readonly paths
produce diagnostics. Lowering
captures the root before evaluating the RHS, then emits SetPath or ModifyPath.
The runtime validates and reads the current target after RHS evaluation and prepares
the update before committing the target and dirty record. Arithmetic failure or
target removal by the RHS prevents the final write while retaining completed RHS
effects. Body reuse remaps write-root place IDs as well as read-root expression IDs.
For every read, set, modify and view instruction, linking also checks dynamic
argument count and register representations against the bound descriptor's index
parameters. An incompatible instruction rejects the whole program before publication;
runtime checks still validate actual argument values and ownership during execution.

Example:

```kagari
player.heal(10)
player.hp = 100
```

For host-owned state, assignment such as `player.hp = 100` lowers to a checked path mutation operation rooted at `player`.
It does not require the script runtime to store a Rust `&mut Player` or `&mut player.hp`.

This keeps Kagari syntax clean while still allowing efficient Rust-backed mutation.

Typed path mutation is the normal model for ergonomic field-style access to Rust-owned game state.
Frame-scoped host borrows remain useful for host function calls that require temporary `&T` or `&mut T` access, but they are not used to represent long-lived script field views.

## Registration of Generic Rust Functions

Kagari does not register open Rust generics directly.

Generic registration rule:

- register concrete instantiations separately

Example:

```rust
registry.register_fn("util.sort_i32", |xs: &mut [i32]| { ... });
registry.register_fn("util.sort_string", |xs: &mut [String]| { ... });
```

This is simpler because:

- Rust generic instantiation and Kagari generic instantiation are not the same system
- overload and resolution behavior remains explicit
- diagnostics are easier to keep understandable

## Registration of Generic Rust Types

Use the same strategy for generic host types in v1:

- register concrete instantiations explicitly

Examples:

```rust
registry.register_type::<Vec2>("math.Vec2");
registry.register_type::<InventorySlot<PlayerId>>("game.InventorySlot_PlayerId");
```

The host may expose script-facing aliases that hide the Rust naming detail.

## Deferred Generic Factory Model

A later version may support a generic registration factory.

Conceptually:

```rust
registry.register_generic_type("game.Buffer", resolver);
registry.register_generic_fn("util.sort", resolver);
```

The resolver would receive a concrete Kagari instantiation request and decide whether it can map it to a supported Rust specialization.

This is a later extension, not a v1 requirement.

## Binder Traits

A practical host binding layer can be expressed through conversion traits.

Conceptually:

```rust
trait FromKagariArg<'frame>: Sized {
    fn from_arg(arg: &Value, frame: &'frame HostCallGuard) -> Result<Self>;
}

trait IntoKagariValue {
    fn into_value(self) -> Value;
}
```

These conversions can then be implemented for:

- primitive owned types
- script-owned handles
- `&'frame T`
- `&'frame mut T`
- specialized host wrappers

## Guarded Call Boundary

Call sequence:

1. host creates a call frame guard
2. arguments are marshaled into frame-scoped values
3. script executes
4. frame guard is dropped
5. all host borrows associated with the frame become invalid

This structure provides a clean lifetime boundary without requiring Rust's borrow checker to be reproduced inside Kagari.

## Interaction with Reflection

Host interop and reflection remain distinct but composable.

Behavior:

- host types may opt into reflection
- host objects may expose metadata for tooling, diagnostics, or reload validation
- reflective reads over host objects are limited to declared reflection metadata and adapters
- reflective writes over host objects are privileged tooling operations, not the ordinary mutation model
- ordinary script mutation of host-owned structured state uses typed path mutation

Reflection is defined in [reflection.md](reflection.md).

## Interaction with Traits

Rust traits and Kagari traits have independent declarations. Implementing a Rust
trait on a struct exposes no script member or impl. A callback may use that Rust
implementation after the host explicitly declares or references a Kagari trait,
registers its impl and binds each required member through the ordinary module API.
Registering a DTO schema supplies neither those members nor Rust reference access.

A generic bound may forward an installed Kagari implementation into an interface
construction, including through a shared generic method. The
[host interface example](../../examples/host-interfaces.kgr) demonstrates this with
Reader<Item = i32>. The capability retains the selected native table, associated
outputs and supplying generation. It uses ordinary payload admission and scoped
native calls: durable host roots remain checked, borrowed references do not become
storable, and synchronous reentry retains borrow validation and cleanup.


The [trait table example](../../crates/kagari-runtime/examples/host_trait_table.rs)
registers runtime-owned `Opaque<Counter>` separately from a Kagari `Readable` trait
and its explicit impl. The bound `get` callback takes a checked shared loan and calls
Rust's `RustReadable::read`. It then compiles and decodes KBC, installs the common
module, checks the linked trait table and invokes that native application with an
Opaque root. Run `cargo run -p kagari-runtime --example host_trait_table`.

`ModuleBuilder::reference_trait` accepts a portable declaration path and scoped
expected `TraitDef` for a source-defined trait. It records the expectation in
`ModuleDecl::trait_references` and returns the ordinary `TraitRef` used by
`trait_impl`; it defines and exports no second trait. The contract may come from
the source module's checked public or private ABI. Required members still need explicit
bindings. Associated outputs use `MethodsBuilder::associated_type`, and omitted
source defaults retain their script bodies. Native adapters, reserved protocols
and duplicate/conflicting references cannot be introduced through this API.

The declaring script module must supply the actual trait. Checked native
declaration dependencies resolve only in that native importer's scope. An exact
registered trait reference admits its canonical private trait in that scope;
ordinary script imports retain their visibility checks. References create no
global source package aliases. Engine
construction validates the closed source-independent native set immediately.
Standalone source families use a valid generated qualifier in the native view;
only that native importer's explicit dependency set resolves it. This spelling
does not expose even a public sibling to an ordinary source import. Canonical
module identities remain unchanged, and colliding dependency spellings remain
ambiguous rather than selecting an arbitrary owner.
Source-dependent generated signatures/bodies are checked with the user snapshot;
a missing source provider cannot compile. MIR and bytecode carry the reference
requirements separately from trait definitions and compare them with the actual
reachable defining module's ABI. Source-free loading also requires that each
native module carries exactly its installed references: omission or forgery is
rejected, including unused requirements. Comparison covers unused members,
associated output contracts and default policies. A changed source default body
with the same ABI remains subject to frozen declaration, independent executable
and retained-demand checks, not transitive body protection. If admitted,
ordinary calls select its current implementation; retained computation bodies remain
exact as specified by [module activation](module-activation.md).

The [portable source-trait contract fixture](../../crates/kagari/src/runtime/tests/embedding/offline_nominal/traits.rs)
demonstrates explicit binding, an associated output, a script default and exact
ABI rejection.

Host types implement script-visible traits only through explicit common
implementation declarations. `ImplBuilder` names the trait application, receiver
and associated outputs; each member is declared and bound through the ordinary
native module. Rust trait implementation bodies can supply those callbacks, but
Rust trait membership never creates a Kagari impl. Ordinary DTO schema registration
also creates no methods or trait impls.

Signature analysis and artifact verification check the complete output schema,
input/output bounds, receiver and substituted method signatures against the
owning trait's public ABI or private execution contract, even without calls.
Linking requires an identical registered table and bound callbacks for mapped
methods. A script implementation of the same applied trait for that host type
is an overlap; associated outputs do not distinguish implementations.

A concrete host receiver satisfies static bounds and may convert to a fully bound
dynamic interface. The ordinary interface table selects a verified native import
through the shared installation, effect, cancellation and borrow checks. Artifact
verification checks the exact implementation identity, method signature and binding.
The interface retains only a checked durable `HostRoot`; temporary borrow tokens
and path views cannot be boxed. It keeps its linked execution version across
reentry and hot reload. Hosts must explicitly root retained interface objects;
this keeps the interface alive, without transferring host object ownership to GC.
Runtime downcast syntax remains future work.

Run `cargo run -p kagari --example host_interfaces` for
[the complete binding example](../../crates/kagari/examples/host_interfaces.rs).
It reads the private source Reader contract, explicitly binds its associated Item
and read member once, then uses the same installation for static calls and boxed
interface dispatch after KBC decoding. Its runtime-local Counter root remains
host-owned. Compilation imports cold declarations without executing callbacks.

Trait-system behavior is defined in [traits.md](traits.md).

## Interaction with Security

Host interop is part of the security boundary.

Behavior:

- host APIs are opt-in exposures
- host reflection is separately gated
- dynamic loading and powerful host services are available only through installed host services

Security behavior is defined in [security.md](security.md).

## V1 Feature Set

The first usable host interop version includes:

- explicit type registration
- explicit function registration
- owned, shared-borrow, and unique-borrow passing styles
- frame-scoped host borrow handles
- borrow conflict checks at the call boundary
- no-escape enforcement for borrowed values
- concrete registration of Rust generic instantiations

## V1 Exclusions

The first usable host interop version excludes:

- arbitrary open generic registration
- escaping borrowed host values into script-owned storage
- cross-yield borrowed host handles
- automatic reflection for all host types
- unrestricted host mutation without registration and policy checks

## Implementation Order

Incremental implementation order:

1. explicit host type and function registration
2. owned-value marshaling
3. frame-scoped shared-borrow support
4. frame-scoped unique-borrow support
5. borrow conflict checks
6. no-escape validation
7. concrete generic-instantiation registration

This gives Kagari a practical embedded scripting model early, especially for the "Rust owns the data, Kagari patches the behavior" use case.

## Native async producers

Native producers, source async functions/closures/blocks, host Task scopes and
structured composition share the owned interpreter. The SDK exposes borrowed driving
Calls and Runtime polling, not a separate native execution loop. The
[async executable contracts](../async-execution-design.md#executable-contracts)
define the checked handoff.

`TaskScope.spawn<T>(fn() -> Future<T>) -> Result<Task<T>, SpawnError>` uses ordinary
registered declaration/binding metadata and checked callable coercions, including
generic `F: Fn() -> Future<T>` forwarding. Admission does not invoke the factory.
`Task<T>.cancel()` requests cooperative cancellation without driving script.
Native `CallContext` exposes these admission/control operations without exposing
an independent task driver. `SpawnError` is the canonical `core::task` enum with
ScopeClosed, CapacityExceeded and DispatchUnavailable variants; it describes only
failed admission, not later execution or business failures.

Native Future registration separates cold typed input capture from an owned start
adapter. An operation identity and one result slot are reserved before external
submission. Immediate/deferred completion uses the same checked output type;
conversion into managed values occurs only under the owning driver. Endpoints
retain no runtime references, frame borrows or unrooted Values. Cancellation hooks
are bounded and invoked at most once; duplicate/stale completions are harmless.

### Completion transport foundation

`native::completion::CompletionRegistry` provides bounded operation reservations
with opaque registry/slot/generation identities. Reserve before submitting external
work; pass the typed `Completion<T>` endpoint to the provider and retain the single
`Operation<T>` consumer on the owning driver. Publication stores one owned payload
and then wakes; Accepted, Duplicate and Stale distinguish the outcomes without
executing script. Registering a waker rechecks already published data under the
same lock. Wakes are hints; the payload remains durable until claim or retirement.

The consumer polls Pending/Ready/Retired. Claim disarms the cancellation hook and
releases capacity; cancel/drop discards queued payload and invokes a bounded hook
at most once. Terminal cleanup detaches payload, waker, hook and reservation before
host destruction. Payload/waker destruction, hook invocation and disarmed-hook
capture destruction are independently contained: one unwinding panic cannot skip
the other cleanup or reservation release. Repeated retirement does not retry them.
Host callbacks and destructors run outside transport locks; waker clone and wake
are also contained. A cleanup panic faults the registry, prevents new admission and
is reported as EngineFault. If claim cleanup fails, the undelivered payload is
discarded under containment rather than converted or returned as success.

A live operation retains its first cancellation hook even when registration reports
an existing registry fault: already submitted external work still needs cancellation
on retirement. Duplicate or retired hook registrations are rejected. A poll that
observes an existing fault does not claim the result; the driver must retire that
operation. Cancel/drop remains available after a fault and releases capacity exactly
once. Exhausted generations never wrap or become reusable. The producer endpoint
keeps only weak references to result storage and the registry, so retained late
endpoints cannot keep completed payloads or reservations alive.

Rejected duplicate/stale payloads remain producer-side data; their destruction is
not consumer retirement. Containment applies to unwinding Rust panics, not process
abort or a double panic during unwinding. If destroying a caught panic payload
itself panics, the secondary panic payload is deliberately retained to prevent
recursive cleanup failure.

### Registered cold producers and VM waits

`ModuleBuilder::bind::<Async<A>, R>` binds an explicit `fn(A) -> Future<R>` declaration using the
catalog's unique checked `NativeStorageLayout::Future` role. `NativeStorage::future`
provides sealed runtime payload storage with exactly one invariant output parameter.
The foundation installs `core::future::Future<T>` and exposes it through the
prelude and `std::future`. Application producers use the engine's installed
declaration catalog. Typed SDK calls and scheduling work without source/native features.

Calling a producer captures traced script values without invoking its Rust callback.
The first await removes the cold state, roots the captured values, reserves bounded
operation capacity, converts owned arguments and invokes the callback once. It
receives `A` and `Completion<R>`, returning `NativeStart::Ready(R)` or
`NativeStart::Pending` with optional bounded cancellation. Submission failure
retires its reserved endpoint, including any result published during submission.
Custom result conversion runs only in the exclusive driver. Both the Future's
nominal type and the resumed output are checked against the waiting frame's scope.

`RuntimeConfig::async_limits.max_pending_operations` defaults to 1024 and must be
nonzero. A pending wait belongs to the owned execution session. Cancellation,
owner retirement and runtime destruction release it; provider cleanup panic
quarantines the runtime. Guarded retirement releases all frame windows before
destroying frame owners, contains pending-consumer and owner-control cleanup
independently, and continues releasing roots and sessions after a failure. A drain
that discovers quarantine also retires the remaining inactive owned sessions,
even if their handles are retained. Runtime destruction includes retained standalone
owners in this guarded drain. Late endpoints cannot restart or retain the execution.
MIR lowering, encoded bytecode `Await` and VM Waiting are implemented. Independent
bounded MIR/bytecode flow checks reject uninitialized resume reads and statically visible host capabilities live across an await.
Runtime checks remain mandatory for actual values and active native/host resources.
The SDK admits typed invocations through `call_async` (borrowed driving Call) or
`submit_async` (result-only PendingCall plus `Runtime::poll`). Argument conversion
and rooting precede cold admission; the factory and exactly one returned Future
layer run only when scheduled. Future-valued outputs are not implicitly flattened.
See [typed async calls](embedding-api.md#typed-async-calls-scheduling-and-native-waits)
for bounds, context and cleanup guarantees.

Backend `Vm::start_future` remains a lower-level primitive: it queues a supplied
script or native Future, which the first drive claims. Cancelling that queued owner
before its first drive releases its root without claiming the cold Future. Backend
owner wakers and retirement draining retain their low-level contracts; they are not
SDK start/drive aliases.

### Shared Task waits

The SDK host creates a managed scope with `Runtime::create_task_scope` and passes
its rooted `scope.handle()` as a typed TaskScope argument. Script spawn admits a
deferred factory; Runtime polling or a driving Call advances it. Awaiting a Task
registers a dependency, never recursively drives its producer. Successful and failed
outcomes are cached in the sealed Task payload. Managed terminal records retire
automatically, independently of retained Task handles and diagnostic event consumption.
Completion detaches registrations before waking waiters; wakes never execute script.

Core runtime manual scopes retain `spawn_task`, explicit `Vm::drive_task` and host
report consumption as a distinct backend integration. They are not the managed SDK
scope API. Structured `join`/`try_join` branches use the same native-wait lifecycle;
closing a branch detaches its Task wait without cancelling the shared producer.

`RuntimeConfig::async_limits.max_task_waiters` is a nonzero runtime-wide bound,
defaulting to 4096. It counts unresolved Task waits, including independent owned
Future roots. A wait exceeding this limit terminates its execution with resource
exhaustion. Completion, cancellation and execution retirement release registrations.
Self and transitive cycles trap before registering or parking; a queued target is
not driven implicitly to resolve a cycle. Waiter cancellation never cancels a target,
including a target owned by another scope. Target cancellation/failure terminates
dependent waiters with the original error class and originating Task identity.

TaskFailure diagnostics distinguish local cancellation causes from Dependency. The
RuntimeError and EmbeddingError task_origin accessors retain the original Task ID
and its original cancellation cause through any number of waiting executions.
Business Err remains a successful Task output and participates in ordinary `?`.

Managed `TaskEvent::Failed` carries detached task/scope identities, failure and spawn
origin, including failures already observed by script. It is a bounded failure stream,
not successful output delivery or an unhandled-error detector. Backend `TaskReport.origin`
likewise exposes detached factory identity and the script spawn site when available.
RuntimeError/EmbeddingError traces carry bounded logical Spawn/Await boundaries alongside the original failure stack. Module/function/instruction
identity remains usable without source maps; snapshots own no roots or executable
version leases. [Error reporting](error-reporting.md#async-terminal-reporting)
defines the ordering and truncation contract.
