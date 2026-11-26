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

Read/prepare callbacks borrow ValueViews in their HostCallContext. Callback path
contexts expose effective access, labeled dynamic arguments and an opaque protected
base value; inspect_path reads that base's actual component without executing Host
code. A read can return inline scalar values or text published with allocate_string.
Borrowed Host/Runtime results cannot escape the callback scope.

The direct Host API accepts genuine RootedValue inputs and returns genuine roots
for reads/path construction. host_root_value wraps a registered external root;
make_host_path_view builds the actual managed path from a root/path value and
rooted dynamic arguments. The descriptor supplies dynamic-argument TypeId labels;
Host source admission checks saved path labels against that registered descriptor.
Inline scalar results have as_i32/as_i64 and corresponding
typed getters. Dirty records own rooted base/argument/old/new values after commit.
host_dirty_paths snapshots share that ownership; clearing Runtime's ledger does
not release values still retained by a detached snapshot.
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

Host interop separates three concerns:

1. Kagari declarations of types and functions
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

`TypeBuilder::external` declares an external object identity. Its Kagari spelling
is `native host type Player;`, or a field-bearing declaration if the builder adds
fields. Each `field` adds a `FieldDef` with `HostFieldPolicy`. The exported field
shows its Kagari type, visibility and `val`/`var`, not that policy. Neither the
text nor the schema contains a Rust pointer to a live player. External root metadata, inherent methods and explicitly declared
Kagari trait impls use the same native module and callable installation as ordinary
registered APIs. A Rust struct's methods or trait impls are never inferred from a
data schema. Runtime-owned `Opaque<T>` storage and external Host roots and paths
use distinct checked adapters under the same callable contract.

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
and `TypeDeclaration` schemas without a second host type DSL. Lowering binds checked
Host field IDs to an exact path declaration; MIR carries the contract and bytecode
records the requirement in `BytecodeModule::path_declarations`. Source-free loading requires exact
installed schemas and path bindings, including unused path declarations.
`Runtime::resolve_host_path` looks up an installed scoped declaration without
registering another descriptor. The runnable
[atomic path example](../../crates/kagari-runtime/examples/atomic_host_path.rs)
shows the complete declaration, binding, installation and checked access sequence.

`Runtime::register_host_root` issues a registry-local handle; its
[lifetime and retirement rules](#external-host-access-and-scope) are independent of code reload.
Calls, temporary scopes and path-view chaining reject
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

A source-enabled `KagariEngine::builder()` has verified core declarations for
registration authors, but no installed Native modules. To add stdlib, [compose
and install it explicitly](embedding-api.md). Application modules use the same
[registration API](standard-declarations.md#explicit-registration); neither a Rust
function signature nor a Rust trait implementation declares a Kagari API.

Declare a function with `FunctionDecl`, including its parameter/result `Type`
handles, then attach its callback with `ModuleBuilder::bind`. Signatures use the
ordinary scoped `Ty` tree. Manual external-ID callbacks explicitly attach a
`HostCallPolicy` for argument passing and effects. Common native entries retain
that checked declaration and its complete dependency closure; there is no second
runtime function registry or inferred signature authority.

Given `native host type Player;`, the declaration `native fn bounce(p: Player) -> Player;`
takes and returns a handle of that same type. Checking the call neither requires
a passing annotation nor infers a Rust loan from the caller’s aliases.
A Native entry exposing an external handle in a parameter or result, including
a nested type, requires a Host policy at registration and executable admission.
It must match the actual binding; absence does not mean `Owned`.
At invocation the runtime validates the actual handle and acquires any required
short loan before callback entry. A conflicting reentrant call fails before its
callback runs; call cleanup releases the acquired loan on success or failure.

Declared Host parameter types provide expression context before
argument checking, including nested containers and empty Array literals. The same
parameter comparison rules apply to script and host calls: an argument that exits
the enclosing function on every path produces no parameter value and does not
invoke the host callback. Arity checks, inner diagnostics and incompatible values
on normally completing paths still reject compilation.

External-ID callbacks are authored with `native::host::callback` and bound through
`ModuleBuilder::bind`. They receive `(&mut HostCallContext<'call>,
&[ValueView<'call>])` and return `Result<ValueView<'call>, HostError>`. The callback
must work for every call lifetime; it cannot keep a call's view in persistent Rust
state. The context exposes the exclusive Runtime and a call-scoped borrow guard;
it cannot be constructed by hosts.

The existing Native/caller source protects callback inputs. HostResourceScope
owns callback locals and borrow leases without copying all parameters into another
root pack. `protect` and `retain_temporaries` explicitly add local protection when
needed; `retain` creates a genuine escaping RootedValue after scope validation.
`convert_value` builds through the ordinary converter and publishes the result
into the Host scope before conversion locals retire. A returned managed value
enters the Native pending result before Host cleanup. Borrowed results reject
before their scope ends, and cancellation/resource termination is checked after
the callback even when it returns a business error or handles a nested error.

`read_string` lends protected UTF-8 to a closed callback. That callback can print
or copy a bounded prefix, but cannot retain the text or reenter Runtime while the
payload is borrowed. CLI, compiler-free AOT and browser output sinks use this
access; browser output also enforces byte and message caps. Host call traces
store bounded detached snapshots, not managed values or roots. Reentrant calls
append to the logical root's trace in invocation order; recording an error stores
its kind without reopening managed data after termination or quarantine.
The public registry/function invoke bypasses are removed; Host invocation goes
through Runtime effect, cancellation, signature and source validation.

`kagari_runtime::execution::reentry::reenter(context, loaded, function, args)` drives the existing explicit
frame executor synchronously. The loaded handle and FunctionRef select a linked
call route; ordinary call selection resolves its current implementation within the
active execution's program. Argument and result representations are checked
against the linked signature, and arguments
must contain valid references owned by this runtime. The return is a RootedValue;
keeping it alive retains the object across subsequent GC and host operations.
Outer frames and borrow guards remain live while nested calls run. Borrow conflicts
are checked across these scopes, and each scope releases only its own resources.
An ordinary nested trap may be handled by the host. Cancellation or resource
termination, including VM-stack, Native-stack and native conversion exhaustion,
remains recorded for the root even if the host ignores the error. The first termination
and origin survive ordinary observer failures; EngineFault/quarantine takes
precedence, as defined by [failure semantics](failure-semantics.md).
Nested calls share the session's frame stack and debugger observer. Breakpoint and
trap snapshots include suspended callers at their actual call instruction, plus
the nested frames. The synchronous callback path currently uses the interpreter.

`SharedBorrow` and `UniqueBorrow` Host parameters reserve the actual root object,
including when the argument is a path view. Shared passing narrows a writable
path view for the callback, without changing the caller's view. Conflicting
arguments reject before invoking the callback; scope cleanup releases reservations
already acquired for earlier arguments. A Rust-side borrow token identifies its
borrow table, frame, epoch, object, type and mode. It expires when the frame ends;
it is neither a script value nor a durable root handle.

Runtime::host_scope creates explicit temporary scopes and validate_host_borrow
checks tokens; the old enter_host_call and mutable borrow-table access are removed.
Scopes participating in a root retain its options/version until the last scope
ends. Root and lease cleanup is unconditional after traps, cancellation, stack exhaustion
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

Text output is opt-in. Construct [`kagari_stdlib::io::module`](../../crates/kagari-stdlib/src/io.rs)
with the engine's declaration catalog and the host's sink, then install the
returned `NativeModule`. It can be installed without the rest of stdlib. Its
`std::io::{print, println}` functions each take a `String` and call the sink
once with a protected view of the message and a suffix (`""` or `"\n"`). The
sink writes both in order. It may stream or copy within host limits; a sink
error is an ordinary host failure. No sink runs during authoring or compilation.

Source uses ordinary imports or qualified calls. Neither function is an implicit
name, and the compiler does not inject a provider. Standalone analysis may use
cold declarations without executing a host. An executable artifact that requires
`std::io` still needs a matching installed binding before publication; its own
metadata cannot supply the sink.

The compiler consumes immutable common ModuleDecl metadata. Standalone compiler
users supply cold declarations through AnalysisDatabase::set_native_modules.
SDK users bind and install NativeModule values through KagariEngineBuilder::install
or install_all, then build an engine with a sealed declaration and binding closure.
That engine can compile before creating a runtime; authoring and compilation do not
execute callbacks.
Source may call demo::echo directly, import demo::echo with an optional alias, use
a nested import group, or import demo as a module alias. Source paths use double
colons; the module identity and its actual declaration IDs select the binding.
The std namespace is reserved. Ambiguous function/module paths and unspellable
declaration paths are rejected. Unknown imports and duplicate import aliases
produce source diagnostics instead of disappearing during lowering.

Validated Native registration metadata can be shared across requests, not a
presentation parse or prior lowering. Each request lowers and checks the declarations it
needs against current inputs. Snapshots retain their checked declarations,
signatures, documentation and scoped definition IDs; a definition from another
context cannot resolve by coincident index.
Analyzer's `FileQueries::source_function_at`, opened with `file.queries()` after importing
`kagari_analyzer::AnalyzerQueries`, returns the ordinary callable target at a
free-function callee or import without executing code, including through source
facades. Definition and member queries use the same checked declaration identities.
New analysis uses the current catalog and parses ordinary source afresh, even if
its text is unchanged. Correct neighboring functions remain queryable after import
or call errors. Host calls require installed bindings and are excluded from scalar
constant evaluation.

The source call boundary supports scalar and nested Tuple, Array, Map, Set, Option
and Result signatures. Key/element eligibility follows the declared
[collection protocols](builtins.md#collection-types), not a fixed scalar whitelist:
HashMap keys and HashSet elements require checked `Eq + Hash` (including `char`);
Map/Set interfaces do not impose those bounds themselves.
Composite arguments use `Owned` passing: shared mutable objects retain their
identity, and tuples retain value semantics. This does not provide a Rust borrow
lease or make an unrooted view an owning GC root. Owned arguments reject frame-scoped
host borrows even inside nested tuples; script containers cannot store them.
Arguments stay rooted during the
callback; longer host retention requires an explicit rooted handle. Both argument
and result checks inspect nested members and standard enum tags, reject foreign
or stale heap references, and observe execution termination while traversing.
Argument mismatch prevents callback execution. A result mismatch rejects the
result without rolling back effects already performed by the callback.
External Host types resolve in source annotations and call signatures by declaration
identity. Direct imports, module aliases and source facade re-exports preserve that
identity. A field-bearing `native host type Player` is still a handle, not a
constructible struct or a runtime-owned opaque object. Its [comparison rules](value-semantics.md#identity-requires-statically-known-object-operands)
do not depend on how the declaration reached the checker.
Ordinary definition queries expose the checked declaration identity. A parsed
document supplies its own source location; it does not supply the Host schema.
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
| Runtime-owned opaque target | No implicit move | Checked payload loan | Checked payload loan with installed editing capability |
| Registered Host root/path | No implicit snapshot | No generic Rust conversion; use the installed Host adapter | No generic Rust conversion; use the installed writable Host adapter |

DTO schema registration exposes data fields only. Rust methods and Rust trait
implementations supply no script members or Kagari trait evidence. Declare each
host member and bind its callback explicitly. Opaque and external Host declarations
require bindings appropriate to their storage and declared access. A separate DTO
snapshot and opaque representation use distinct explicit binding types. The
[value conversion guide](embedding-api.md#typed-native-registration) describes
the finite derive/Serde subset and directional converters.

## Native payload accounting

Runtime-owned custom payloads implement the standalone collector's unsafe
`GcObject` provider contract, including their own resumable ScanState, complete
edge enumeration, constant-time `ObjectCharge`, mutation and Drop rules.
`NativePayload` selects an automatic schema registration; it no longer supplies a
`trace(Value)` or fallible accounting callback. `NativeData` and `native_data!`
retain the safe fixed-data route. Its payload is stored inline in an edge-free
GC representation; arbitrary traced providers construct through branded
`NativeType::create_with` and their reviewed GC constructors. NativeInit exposes
scoped allocation and storable object admission, not Runtime execution services.

One Native script identity traces its concrete payload and actual executable
dependencies. The GC identity retains a sealed payload descriptor, while the
installation owns its factory closure and captured Host state. Payload metadata
contains immutable type/provenance facts; data does not acquire a module-state
edge merely for being nominal. A fixed-data payload
and its identity are two physical GC objects, each shallow allocation accounted
once. Provider bytes include only exclusively owned indirect backing; referenced
objects, shared metadata and external host resources are separate. Report actual
capacities where available and identify unobservable allocations as excluded.
These contributions measure GC pressure, not process memory or a language quota.

Accounting/scanning callbacks cannot allocate, panic, reenter or inspect managed
peers. Allocation and checked mutation commit the actual owned charge. Sweep
releases that ledger without calling the provider again. Replacements publish the
new concrete payload before changing identity/revision; the old payload becomes
ordinary GC garbage. No RootedValue lease is stored in a managed field to hide a
missing edge. Fixed-data loans keep graph, scan coordinates and backing unchanged.
Completed writes survive a user error or panic; heap loans end before panic is
resumed. This grants no general mutable access to arbitrary traced fields.

These are implemented public provider APIs. Existing application-authored
[source-free managed payload tests](../../crates/kagari-stdlib/src/tests/execution/native_boundary/native_boundary_payload/managed.rs)
exercise private managed callback edges, alias mutation, reentry/GC, rootless
cycles and retained-old/current-new selection. Full collection reconstruction
remains with [NC](../native-collections-plan.md). See [GC accounting](runtime.md#gc-heap)
and the [reservation failure contract](failure-semantics.md#modification-guarantees).

## Registered payload loans

An instance `MethodDecl` may explicitly set `receiver_access` to
`ReferenceAccess::Shared` or `Mutable`. HIR selects the method by its Kagari `self`
and signature, not by a Rust receiver loan. Lowering takes the loan requirement
from the selected member's registration into the executable contract. The common
`bind` adapter and installation validator require agreement with the callback.
Owned/raw callbacks cannot silently claim the borrowed receiver contract.

The borrowed callback takes `&mut LoanContext` and an outer argument tuple whose
first element is `&T` or `&mut T`. It must work for every loan lifetime and return
an owned, static result. Temporary Native receivers use the call's protected
source slots rather than owning Opaque handles. All owned argument converters
finish before Rust references are lent. LoanContext borrows only execution polling
state; `LoanContext::poll` provides cooperative cancellation;
it exposes no script invocation or heap allocation. Captured runtime handles
still encounter the ordinary non-reentry checks during Opaque access.

Opaque adapters validate Rust mappings and installed receiver capabilities before
argument conversion, then validate/root the entire input pack and decode owned
arguments before acquiring the payload loan. Complete-pack roots remain until
result conversion finishes. The existing GC guard releases access before result
allocation and on errors/unwind; completed writes remain visible. Mutable access
requires the installed fixed-data proof, not merely the callback's `&mut T` type.
Traced and managed registrations do not gain arbitrary editing.
External Host path adapters enforce their declaration and binding facts through
the independent Host access boundary described below.

## Protected Native values

Ordinary inferred Native callbacks return scalars through NativeScalar or return
a ValueView with their Native call's lifetime. ValueHandle reads use the same
protected view; root() explicitly requests ownership and rejects non-storable
Host values. Raw payload reads lend a higher-ranked reference to the
actual provider's retained codec, excluding Runtime execution and collection for
that reference's duration. Operation guards keep source coordinates across
reentry and release their active flag on normal return, error and unwind.

Native enum/array allocation and enum payload reads return protected call-lifetime
views. A successful short conversion transfers only its result into the Native
suffix before removing preparation; failure and unwind remove the whole child
suffix. Ordinary scalar results need no source result cell. Enum field access
validates the supplied variant identity and ordinal before exposing the field.
Diagnostic origin forwarding preserves the target's layout/payload and the original
carrier's detached trace. Runtime named-enum construction uses explicit owning
inputs/results at the Host boundary instead of bare unrooted transport values.

## External Host access and scope

Host-owned state stays outside runtime-owned payload storage. A script receives a
registered Host root or typed path capability, not a Rust reference. A
`HostRootHandle` identifies the issuing registry, `HostObjectId`, runtime type,
schema epoch and ABI fingerprint. `retire_host_root` requires the exact live
handle and fails while that object has a shared or unique loan. After retirement
the object is absent from lookup and enumeration, but its ID is reserved for
the rest of that registry's lifetime. The schema epoch describes the schema;
it is not an object generation. Independent runtimes may use the same numeric ID.

A Future holding player A's handle does not pin A's registration or snapshot
its fields. A later path read resolves the complete handle, checks the path,
schema and access, and borrows the root for that operation. If A was retired,
the read fails before the adapter runs; registering player B with the same schema
cannot redirect it. Shared reservations may overlap; a unique reservation conflicts
with any other reservation on that object, including during synchronous reentry.

`Runtime::with_host_scope` and `Runtime::execute_scoped` support these independent
Host calls. Import a rooted Host value into the scope and invoke the checked entry
with an explicit `ExecutionContext`. The scope protects argument and result views
without creating owning roots for temporary transport. A call-lifetime `ValueView`
cannot escape its scope. `scope.retain` validates the value and returns an owning
`RootedValue`; this can protect a live Host root or path descriptor for Rust. It
neither extends a loan nor makes that value admissible in script storage.

Ordinary script heap fields, array elements and module statics do not accept a
direct Host root or path view, including inside nested tuples. Computations have
a different boundary: a managed Native value or same-registry root may be retained
in a closure, cold Future, capture cell or parked frame. Admission checks for
reachable active loans/operation guards and rejects path views even inside tuples
or behind mutable aliases. It need not prove a retained root is still registered;
retention alone never grants access. See [owned interpreter driving](execution.md#owned-interpreter-driving).
Scope and call cleanup release protected values and active loans on success,
error or supported Rust unwind. Completed writes are not rolled back. See [failure semantics](failure-semantics.md),
[scoped entry](embedding-api.md#scoped-external-host-entry) and the
[manual adapter tests](../../crates/kagari-stdlib/src/tests/execution/native_boundary/native_boundary_conversion/host_adapter.rs).

## Kagari-Side Semantics

Kagari script code does not need to spell Rust borrow syntax directly.
Normal member syntax may be backed either by ordinary script-owned values or by host-backed typed paths.

Rules:

- host metadata determines the host passing style
- script code uses normal calls and member access
- declared Host fields use field syntax; registering an index or virtual segment does not add script syntax
- a typed path describes checked access, not a retained Rust loan

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
instruction; missing or ambiguous bindings reject publication.

Bytecode path records require that contract fingerprint. The loader resolves all
records before publishing the program, rejects missing/ambiguous contracts and
checks result representation and write access. Each loaded version holds its own
path bindings. VM path operations consume those bindings; registering descriptors
later cannot retarget an existing version. Debug names are diagnostic text only.

For a field read or assignment, HIR checks the field's owner, visibility, type
and `val`/`var` declaration. It records each selected field ID and result type,
not an adapter or path contract. `FileQueries::field_at` uses that same identity
for navigation, even without an executable path. Lowering groups consecutive
Host fields and matches the root and ordered field IDs against the provider's
registered paths without looking up names again. It requires one exact chain,
matching result types at every step and write access for a write. A missing,
ambiguous or incompatible path fails lowering, not field lookup. The root is
evaluated once; `ReadPath`, `SetPath` or `ModifyPath` uses the whole chain
without separate reads of intermediate fields.

For `player.baseline.hp += 2`, `val baseline: Stats` forbids replacing
`player.baseline`, not following its Host handle. A writable path may traverse
that readable prefix to a `var hp`; a `val hp` cannot be assigned through either
a `val` or a `var` prefix. A readonly runtime view is a separate restriction:
even a writable leaf and matching writable descriptor cannot upgrade that view.

For a path containing Host index or virtual segments, a provider can expose an
ordinary Native method such as `player.add_score(first(), second(), rhs())`.
HIR checks the declared call. Its portable Native declaration pairs an operation
kind (`Read`, `Set` or `Add`) with the exact path. The call remains a Native
import, not a path opcode. Source-free loading requires the installed callable
to bind that same operation and path, the matching schema closure and an adapter
supporting the declared access. A same-signature method bound to another path
fails before any callback, even if both descriptors are installed.

The receiver, index arguments in segment order, and finally the RHS each evaluate
once, left to right. If the two index segments declare slots `1` and `0`, the
adapter receives `[second_value, first_value]`; neither argument is evaluated again.
`Read` returns the leaf, `Set` returns unit, and `Add` returns the updated numeric
leaf. As for field assignment, the captured target is validated and read after
the RHS. A successful write prepares and commits once and publishes one dirty
record with the root, descriptor, slot-ordered arguments, operation and old/new
values. Integer overflow traps after the old value is read and before write
preparation. Neither overflow nor a target invalidated by the RHS commits that
write or appends its dirty record; previously completed effects remain visible.

For every read, set, modify and view instruction, linking also checks dynamic
argument count and register representations against the bound descriptor's index
parameters. An incompatible instruction rejects the whole program before publication;
runtime checks still validate actual argument values and ownership during execution.

A Host-rooted path descriptor keeps its access restrictions, not an active loan.
An adapter acquires fresh checked access when the operation executes.

## Generic Host registrations

A Rust generic function does not imply an open Kagari API. The module may
expose distinct concrete functions, or explicitly declare Kagari binders, bounds,
parameter/result types and the operations a generic Native implementation needs.
The selected application must satisfy that declaration; a callback cannot add
new implementations during resolution. An ordinary generic native type and its
storage descriptor must agree on the supported nominal applications. An external
Host type itself has no binders, but it can be an argument to a generic function
or type. See
[explicit registration](standard-declarations.md#explicit-registration).

## Binder Traits

The implemented [conversion traits](../../crates/kagari-runtime/src/native/conversion.rs)
are KagariType, IntoKagari and FromKagari. KagariType describes installed Rust
mapping facts; bindings prepare their closed signature once. IntoKagari encodes
through ConversionContext to a protected ValueView; FromKagari returns owned Rust
data or a retaining handle. Input views cannot become borrowed Rust output.

Composite conversions share checked child types, bounded recursion/work and
source-local protection. Fixed mappings are separate from per-value conversion
effects. Neither Host access nor Opaque receiver loans provide blanket conversions
from arbitrary Rust references. Native callbacks decode one prepared argument
pack and stage their result through NativeCall.

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


The [trait table example](../../crates/kagari-stdlib/examples/host_trait_table.rs)
registers runtime-owned `Opaque<Counter>` separately from a Kagari `Readable` trait
and its explicit impl. The bound `get` callback takes a checked shared loan and calls
Rust's `RustReadable::read`. It then compiles and decodes KBC, installs the common
module, checks the linked trait table and invokes that native application with an
Opaque root. Run `cargo run -p kagari-stdlib --example host_trait_table`.

To pin a script trait for a native installation, select its defining module from
an independently compiled, verified program with `DeclarationCatalog::with_verified_modules`.
`ModuleBuilder::reference_trait` accepts the public trait's portable declaration
path and returns the `TraitRef` used by `trait_impl`. This records a mandatory
requirement for the importer; it does not define the trait or grant access to a
private declaration. Native adapters, reserved protocols and duplicate pins
cannot be introduced through this API.

The host declares each implemented method's own signature and callback with
`MethodsBuilder::define_method` (or `define_method_with` for method-local
parameters, bounds or requirements). For example, a `Counter` implementation of
`Reader<Item = i32>` declares `read(self: Counter, amount: i32) -> i32`.
`associated_type` binds `Item`; omitting a source default leaves its body with
the trait owner. Binding checks the adapter against the authored method. When the
application is compiled, the impl must also match the actual visible trait after
substitution. Changing only the owner's `amount` type does not change what the
host has declared.

A structured identity and a written path obey the same reachability and
visibility rules. Dependencies resolve in the importer's scope; they create no
global aliases. A missing owner or inaccessible trait cannot supply an impl.
MIR and bytecode carry the checked owner's trait ABI separately from its uses.
At source-free load, every mandatory identity and ABI must match the installed
importer's independent pin, even if no method was called. The compiler may carry
additional requirements inferred from checked uses; these too must match the
reachable defining module. Comparison covers unused members, associated output
contracts and which methods have defaults. A changed source default body
with the same ABI remains subject to frozen declaration, independent executable
and retained-demand checks, not transitive body protection. If admitted,
ordinary calls select its current implementation; retained computation bodies remain
exact as specified by [module activation](module-activation.md).

The [portable source-trait contract fixture](../../crates/kagari/src/runtime/tests/embedding/offline_nominal/traits.rs)
demonstrates explicit binding, an associated output, a script default and exact
ABI rejection.

A Kagari `impl` for a Host type can come from script or from registration; both
follow the [same trait and protocol rules](traits.md#standard-protocol-identities).
For a registered impl, `ImplBuilder` names the trait application, receiver and
associated outputs. Rust code can supply callbacks for declared Native members,
but Rust trait membership never creates a Kagari impl. Ordinary DTO schema registration
also creates no methods or trait impls.

Signature analysis and artifact verification check the complete output schema,
input/output bounds, receiver and substituted method signatures against the
owning trait's checked ABI, even without calls.
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
It pins the public `Reader` contract, binds `Item` and declares and binds `read`,
then uses the same installation for static calls and boxed
interface dispatch after KBC decoding. Its runtime-local Counter root remains
host-owned. Compilation imports cold declarations without executing callbacks.

Trait-system behavior is defined in [traits.md](traits.md).

### Host conversions

`From` and `TryFrom` may have a Host source or destination. They are Kagari
trait implementations, not Rust storage codecs, casts or requests to manufacture
a handle. The [orphan and overlap rules](traits.md#coherence-guidance), the
substituted signature (including `TryFrom::Error`), and the implementation body
are checked as for other conversions. A reverse `into` or `try_into` call still
requires the [canonical blanket impl](builtins.md#conversion-protocols).

For `impl From<Player> for HealthSnapshot`, a body reading `player.hp` reads the
live object through its checked field adapter. If that handle has been retired,
access fails **before** the adapter reads storage; creating another `Player`
does not redirect the handle. In the other direction, a body returning `Player`
must obtain a genuine handle, for example from a declared lookup. Returning an
`i32` instead is a type error; neither the impl nor generated `.kgr` supplies
storage, a callback or authority to construct a Host object.

## Interaction with Security

Host interop is part of the security boundary.

Behavior:

- host APIs are opt-in exposures
- host reflection is separately gated
- dynamic loading and powerful host services are available only through installed host services

Security behavior is defined in [security.md](security.md).

## Host interop scope

The implemented boundary includes explicit type and function registration, owned
value conversion, runtime-owned Opaque and checked Host root/path access. These
contracts do not grant arbitrary open Rust generic registration, automatic
reflection, borrowed returns or unrestricted Host mutation. Scope-bound Host
values cannot escape into script-owned storage or across suspension.

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
Native `NativeCall` exposes these admission/control operations without exposing
an independent task driver. `SpawnError` is the canonical `core::task` enum with
ScopeClosed, CapacityExceeded and DispatchUnavailable variants; it describes only
failed admission, not later execution or business failures.

Task terminal publication is once-only and precedes notification. Consuming a Host
report releases admission capacity; retained script Task aliases keep the same
cached result and remain independently awaitable. A retained scope capability
keeps closed-state observation, without extending the Host dispatcher's lifetime
after owner drop and cleanup retirement. Async admission validates actual retained
values transitively, including receivers hidden by interface views. Top-level
storability alone does not authorize retention across suspension.

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
The [authored core Future](standard-declarations.md#ownership-and-installation)
uses this Runtime-owned storage without an optional provider. Application
producers use the engine's checked declaration catalog. Typed SDK calls and
scheduling work without source/native features.

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

Backend `Runtime::start_future` remains a lower-level primitive: it queues a supplied
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

Custom `create_dispatched_task_scope` scopes retain `spawn_task`, explicit `Runtime::drive_task` and host
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
