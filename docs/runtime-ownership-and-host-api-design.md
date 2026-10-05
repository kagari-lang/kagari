# Runtime ownership and host object API

Status: implementation authorized on 2026-10-05 after EN01-EN05 completed. The
[roadmap](implementation-roadmap.md#runtime-ownership-and-host-objects-go01-go06-active)
owns activation, phase order and progress. Examples below describe target APIs;
they are not currently callable interfaces.

This design covers two connected changes: concentrate runtime ownership in checked
stores instead of a graph of Rust reference-counted owners, and provide a complete
host API for registration, values, object mutation and checked calls. A host should
be able to retain a result, edit an object and call its trait methods without
managing raw Values, GC roots, argument slots or method-table ordinals.

The target runtime is movable between host threads and executes exclusively at
any instant. This changes the current fixed-thread ownership model. It does not
add concurrent access to one script heap or asynchronous script execution.

## Baseline and scope

The table records the starting point before GO01. The roadmap ledger identifies
completed changes; current behavior is documented in architecture and specifications.

The script heap already uses an object table and checked handles. This is not a
migration from reference-counted script objects to tracing GC. The existing
collector is nonmoving, stop-the-world mark-sweep; the work changes its surrounding
ownership, root representation and integration with execution metadata.

| Existing owner | Current representation or API | Change required |
| --- | --- | --- |
| [GC heap](../crates/kagari-runtime/src/gc.rs) | Object slots plus owner/slot/generation IDs; roots are shared mutable Value vectors | Keep checked object identity; centralize root and frame storage |
| [Runtime](../crates/kagari-runtime/src/lib.rs) and [frames](../crates/kagari-runtime/src/frame.rs) | Runtime, sessions and frames share heap/resource owners through Rc | Runtime owns mutable stores; execution accesses them through a scoped context |
| [Sessions](../crates/kagari-runtime/src/session.rs) | Owned sessions and shared frame stacks provide cleanup and synchronous reentry | Preserve those semantics with session/frame identities and structured cleanup |
| [Modules](../crates/kagari-runtime/src/module.rs) | Linked programs, module stores and retention guards have shared Rust ownership | Centralize runtime links and version retention; share only immutable code independently |
| [Generic environments](../crates/kagari-runtime/src/frame/types.rs) and [selected operations](../crates/kagari-runtime/src/frame/types/operations.rs) | Rc/Weak graphs retain scopes, selected methods and version owners | Typed environment and operation IDs with explicit reachability |
| [Function adapters](../crates/kagari-runtime/src/native/functions.rs) and [outputs](../crates/kagari-runtime/src/native/returns.rs) | Scalar conversion and selected special views; composite output often uses raw Value | Recursive fallible conversion and automatically retained object handles |
| [Callable handles](../crates/kagari-runtime/src/native/callable.rs) | Scoped, stored and rooted callables with raw result paths | One checked call implementation, with explicit internal edges and automatic public retention |
| [Embedding runtime](../crates/kagari-embed/src/runtime.rs) | Execution reports expose unrooted Value results; entry arguments are restricted | Typed arguments/results and an owning dynamic result API |

Retain static typing, EN nominal enum/protocol semantics, generation-pinned calls,
declared access, cooperative cancellation and source-free executable validation.
Incremental/moving/generational GC, packages, schema-backed Serde, general derives,
script async syntax, general JIT expansion and latest-version entry policy are
outside this track. No performance improvement is claimed without measurement.

## Relationship to existing proposals

This document owns the runtime stores, automatic script-object retention,
thread-transfer contract, typed native adapters, managed object access, scoped
payload editing and object/function/trait invocation described below.

The [Rust interop proposal](rust-interop-design.md) continues to own ordinary Rust
DTO derives, schema-backed Serde and independently retained host-owned Opaque
objects. Its shared conversion concepts are reused here. Its fixed-thread policy
and deferral of managed payload editing do not override this design. Adding safe
editing of a runtime-owned payload does not manufacture exclusive access to an
externally shared Opaque object.

The [host facade proposal](host-api-refactor.md) continues to own the cohesive
prepare/load/reload workflow and proposed logical latest-version Function handles.
Its confirmed outer-tuple argument rule applies here. This track supplies its
conversion, roots and object-call foundation. Pinned callable handles here do not
activate or implement that proposal's latest-version policy.

The [async](async-execution-design.md) and
[task-scope](host-task-scope-design.md) proposals own suspension and scheduling.
They must use this ownership model when implemented, but completing them is not
required to move a quiescent synchronous runtime between Tokio tasks/threads.
Existing specs describe the implemented baseline until the relevant phase lands.

## Central ownership and checked identities

Runtime owns the following logical stores. These are responsibility boundaries
inside existing crates, not a requirement to create new crates or one generic
untyped arena.

```text
Runtime
  Heap             script objects and registered managed payloads
  ExecutionState   sessions, frames, argument/result slots and temporary roots
  ProgramStore     runtime links, module instances and pinned program versions
  EnvironmentStore generic scopes, layouts and selected operation groups
  RootTable        externally retained values and callable/member handles
```

Mutable runtime state has one owner. Components refer to it through typed IDs such
as ObjectId, FrameId, EnvironmentId and ProgramVersionId. An ID is a reference,
not an ownership count. Store access validates runtime identity, slot generation
and the expected kind before exposing data. Cross-store IDs are not interchangeable.
Reused slots advance their generation; generation exhaustion retires a slot.
Deleting entries must not renumber live identities. Runtime-local IDs are never
serialized into portable executable artifacts.

Replace frame/session ownership of the heap with access through ExecutionContext
or NativeContext. Replace operation-to-group Weak back-references with group IDs
and validated member indices. Program/environment references retain exact lexical
type owners and dependency versions, including associated outputs from the already
selected implementation. An ID lookup must not become runtime trait inference.

Immutable, runtime-independent code and verified descriptors may use Arc when
they are proven Send + Sync. Mutable caches and installed callbacks do not become
shareable merely by placing the enclosing object in Arc. RefCell may remain in a
small, justified local boundary; it is not the ownership model. No blanket unsafe
Send/Sync implementation or mechanical Rc-to-Arc replacement is permitted.

## GC boundaries and extension

Central ownership alone does not isolate the collector. The current GcHeap also
implements field/collection operations, runtime type matching and enum failure
origins. Move those policies to their object/execution owners as the affected paths
are migrated. Runtime coordinates focused modules; it must not become another
container for all of these implementations.

| Boundary | Responsibility |
| --- | --- |
| Host adapters | Typed conversion, automatic retention and ergonomic get/set/call entrypoints |
| Object operations | Nominal type/access checks, field and collection semantics, calls and error-origin policy |
| Heap and metadata stores | Checked identities, allocation/accounting, physical storage, reference traversal and controlled edge writes |
| Collector and collection driver | Safepoint eligibility, root traversal, worklists, liveness decisions and coordinated reclamation |

The collector consumes storage references and tracing descriptions. It does not
interpret standard-library type names, Result branches or trait dispatch rules.
Stored error origins and callable metadata are traced or released according to
their storage descriptions; their semantic interpretation stays with execution.
These are module responsibilities in existing crates, not new crate requirements.

Three internal boundaries are required during this migration:

1. **Root enumeration.** Frames, native temporaries, public handles and installed
   state expose roots through runtime-owned records. Typed adapters establish and
   transfer roots automatically. An ordinary new native function does not add a
   custom root provider. Adding a new category of runtime root requires explicit
   registration with the collection driver and cleanup coverage.
2. **Reference traversal.** Ordinary structs/enums use their checked layouts;
   collections and managed payloads expose all stored script and metadata edges
   through registered storage descriptions. Traversal does not invoke script code,
   reenter execution or mutate the graph. A new registered type using an existing
   storage representation does not add a case to the mark/sweep algorithm. A new
   physical representation may require a storage adapter and traversal support.
3. **Reference mutation.** Internal operations and public adapters use the same
   controlled storage writes. Initialization/publication, replacement, removal,
   bulk collection edits, frame/root slots and program/environment links are all
   covered. The boundary identifies the owning record and the affected references
   before discarding old edges or publishing new ones. It preserves validation,
   once-only evaluation, atomic commit boundaries and temporary protection across
   safepoints. Mutable traced slots are not exposed to arbitrary callers.

The current stop-the-world collector needs no generational or incremental write
barrier. Establish the write boundary now without implementing a second collector,
a configurable GC framework or speculative barrier state. A later collector can
attach its required barrier at that boundary; the boundary itself is not proof
that a particular future barrier protocol is correct. Safepoints remain coordinated
with execution, and no collection/reentry occurs while incompatible storage or
payload leases are active.

| Later change | Expected affected boundary and remaining work |
| --- | --- |
| Ordinary library function or registered struct/enum | Registration/object operations and existing layout traversal; no collector algorithm change |
| Managed payload with script references | Registered traced storage and checked mutation adapters; no collector algorithm change |
| Collection threshold or trigger policy | Accounting and collection driver |
| Generational or incremental collection | Collector, barriers at all reference/root writes, initialization and safepoint protocols; validate metadata-store edges too |
| Moving or compacting storage | ID-to-storage lookup, relocation and borrow/address-stability rules; Rust payloads may require separately allocated or pinned storage |
| Concurrent collection | A separate synchronization and execution design; Runtime being Send supplies no concurrent-collector guarantee |

Object IDs identify stable logical objects, not stable payload addresses. Slot
generations detect stale IDs; they are unrelated to object age in a generational
collector. Public handles and prepared bindings must retain checked identities,
not cached payload pointers. Incremental, generational, moving and concurrent
collectors remain outside GO01-GO06; this track establishes and tests the boundaries
needed to evaluate them later without promising a cost-free algorithm replacement.

## Reachability and reclamation

Keep the current mark-sweep algorithm and safepoint scheduling. Its root inputs
become explicit runtime records: active installation state, active frames and
sessions, native temporaries, debugger retention, staged initialization state,
prepared mutations and externally retained handles. Arbitrary Rust stacks and
host object graphs are not scanned.

The collector walks typed edges with an explicit worklist and visited sets:

| Edge source | Edges that must be visited |
| --- | --- |
| Structs, enums and collections | Stored script values and required layout/type environments |
| Closures and interface values | Captures/receiver, selected callable information, environments and pinned program |
| Managed Rust payloads | Declared traced script fields and stored callables |
| Generic environments and operation groups | Parent scopes, associated-type owners, required selected groups and program versions |
| Retained program versions | Exact dependencies, reachable module state and installed executable owners |

Use one coordinated reachability decision across these stores, even if each store
has a separate sweep. A program/environment table entry is not a root solely
because it is allocated. Active installations are roots; superseded versions need
a live frame, object, host handle or other reachable version to retain them.
Cycles between old module state, closures and environments must be collectable
once their external roots disappear. Immutable code independently held by a
PreparedProgram may remain allocated without retaining runtime module state.

Freeze the live sets before sweeping. Detach unreachable records, then release
their storage without dereferencing already removed dependencies. Ordinary Rust
payload destruction runs once and cannot invoke scripts; a destructor panic
terminates/quarantines the affected operation according to the existing panic
policy, without permitting reuse of partially swept state. Process aborts remain
outside recoverable-error guarantees.

Frame records are removed on return or unwind. Environments that escape through
closures/interfaces remain reachable after frame removal. Non-escaping temporary
state can be released immediately. Trap, cancellation and call-depth failure use
the same cleanup path. Existing heap-unit counters remain logical accounting,
not measurements of all Rust allocations; metadata liveness also needs bounded
retention tests rather than relying only on script-object counts.

## Public retention and internal references

The public API must never return a safe, long-lived unrooted script object.
Object, collection, interface and function handles carry automatic retention.
Clone retains the same identity, and dropping the last handle removes its root
eligibility. Host code does not call root_value for ordinary results or fields.

The concrete target is a runtime-owned RootTable plus small Arc-backed root leases:

```text
Host handle: runtime identity + root identity + Arc<RootLease>
RootTable entry: checked value/target + Weak<RootLease>
```

A lease does not own Runtime, heap storage or a payload pointer. It can be cloned
or dropped on another host thread without accessing the runtime. RootTable prunes
expired leases when collecting or servicing root operations. A lease still alive
at a collection snapshot may conservatively retain its value for that collection.
Dropping a lease removes retention eligibility; it does not promise immediate GC.
The collector may hold a temporary lease during traversal. A successfully cloned
live lease must never lose its object during a concurrent handle drop.

Runtime teardown releases all stores even if host handles remain. Subsequent use
against another runtime is rejected by identity checks; dropping stale handles
is harmless. No public handle can dereference an object without a checked runtime
context. Closed and foreign runtimes, stale roots and incompatible member handles
have structured errors.

Runtime-owned objects store ordinary traced IDs, not public root leases. Reading
an object-valued field creates a protected public handle before any safepoint;
writing one validates the value and stores its traced edge. Returning from native
code publishes the converted value in caller-owned slots before releasing native
temporaries. Failed conversion releases all unpublished temporaries.

Native payloads containing script references use declared traced storage fields
or a checked traced-field adapter. Payload-to-closure-to-payload cycles must be
collectable. A public owning Object/Function handle is not a valid traced field
representation: hiding a permanent root inside a payload would leak such cycles.
Safe generated/registered storage adapters enforce that distinction; manually
authored low-level tracers remain an explicitly trusted advanced boundary.
Registered host callbacks that capture owning script handles count as external
roots until the callback registration is released. The engine cannot discover
arbitrary references hidden inside a Rust closure.

## Thread transfer contract

The ordinary Runtime is Send and not Sync. A host can move it and operate it on
another thread; all heap operations and execution require exclusive runtime access.
NativeContext borrows that exclusive execution context for one synchronous call
and cannot escape or survive an await. Public object leases can cross threads,
but accessing the object still requires its owning runtime.

All exclusively runtime-owned Rust payloads, observers and services must be Send.
They need not be Sync when they are only accessed exclusively. Callbacks/descriptors
shared through Arc across engine installations must be Send + Sync. Mutable
callback state belongs in an exclusively accessed runtime service or a host-owned
synchronized service. This intentionally replaces acceptance of arbitrary captured
Rc state; do not add a second local-only runtime variant during this migration.

Executable code owners and backend caches must be audited for transfer and
destruction on the receiving thread. A genuinely thread-affine external resource
must remain in a host service and be accessed through messages/validated IDs.
An immutable portable Program can be shared only after all of its contents satisfy
the corresponding traits. Engine analysis state need not be Sync to make Runtime
Send; source preparation can remain independently owned.

Transfer occurs outside active synchronous calls, payload borrows and mutation
leases. A Tokio task may own Runtime across message-receive awaits. The VM has no
Tokio dependency; a development-only integration test proves the future is Send,
and a deterministic OS-thread handoff test proves operation after transfer. Long
synchronous scripts still occupy their executing thread; Send is not asynchronous
execution or a promise of cooperative Tokio scheduling.

## Registration and conversion API

Engine/module registration owns declarations and callback bindings. Invocation
receives NativeContext for the current runtime, never an unvalidated global engine
lookup. The existing declaration catalog is the sole semantic owner. Concrete
Rust signatures can generate their parameter/result types; explicit declarations
remain available for generics, trait bounds and associated outputs. Both routes
use the same validation and binding implementation.

Reuse the KagariType, IntoKagari and FromKagari concepts from the RI proposal.
Conversions are fallible and receive a scoped context for type identity, allocation
and temporary roots. The current infallible NativeOutput::encode is insufficient.
One adapter handles parameters, results, fields and collection elements. Separate
input/output capabilities allow return-only types.

| Rust boundary form | Meaning |
| --- | --- |
| Scalars and owned String | Checked values; String conversion may copy |
| Vec<T>, supported tuples, Option<T> and Result<T, E> | Owned data conversion using installed ordinary declarations |
| Object<S>, ScriptVec<T>, ScriptMap<K, V>, ScriptSet<T> | Retained reference to an existing script object, preserving aliases |
| PinnedFunction<A, R> and Interface<S> | Retained callable/interface with a verified signature |
| NativeObject<T> | Checked handle to a registered runtime-owned Rust payload |
| ScriptValue | Retained value with the exact declared type and access view of a generic parameter or result |

S is a registered schema/declaration marker, not a claim that a KGR object has
Rust T's memory layout. Dynamic Object handles can bind typed members without a
generated marker. Owned collection conversion creates a snapshot; a ScriptVec
operates on the original collection. Recursive owned conversion has depth/work
bounds and rejects cycles it cannot represent. Handle conversion preserves them.
Conversion of standard nominal types uses the installed declaration identities;
absence of the required provider is an error, not implicit library installation.

`ScriptValue` lets a generic Rust callback forward scalars, objects, collections,
interfaces and closures through one adapter. It does not introduce an `Any` type
into Kagari. Conversion keeps the installed type and access view; a different
destination type or a readonly-to-mutable upgrade fails. `value.decode::<T>(cx)`
checks that retained declaration before producing owned Rust data or a more
specific retained handle. It never exposes a raw object identity or borrowed
payload reference.

The outer tuple is always the argument list, including native callback arguments.
Receiver is passed separately for methods. () is zero arguments; ((a, b),) is one
tuple argument. Returns are one value. Support and test tuple arities 0 through 12
in the first adapter implementation, with no runtime argument-count limit implied
by that Rust convenience bound.

NativeResult<R> is the outer runtime-failure channel. A business Result<T, E> is
ordinary data: NativeResult<Result<T, E>> preserves Err without converting it into
a trap. Return mismatches known from the declared signature fail before execution;
data-dependent conversion failures preserve already completed effects.

Illustrative documented String registration:

```rust
string_methods.add_method(
    MethodSpec::new("split")
        .parameter_names(["separator"])
        .returns(list_of_string.clone())
        .documentation("Split this string around each occurrence of separator."),
    |cx, text: String, (separator,): (String,)| -> NativeResult<Vec<String>> {
        cx.collect(text.split(&separator).map(str::to_owned))
    },
)?;
```

list_of_string is the installed List<String> declaration handle. The existing
script signature returns that interface, while the Rust callback produces Vec<String>.
The adapter retains this distinction: validate the concrete result against the
declared interface and use the already checked result adapter to wrap it. It must
not silently change the standard method's return type or search for an implementation
after the callback runs. The same mechanism serves user-defined interface results.

The callback does not inspect slots, construct Value::Str, find a result type
parameter or allocate a physical sequence. The adapter performs that work after
any scoped argument borrow ends. cx.collect polls cancellation and handles Vec
capacity-growth errors; it does not promise recovery from every Rust allocator
failure or preemption inside an arbitrary iterator's next method. Handwritten
long-running native loops still need cooperative polling.

The baseline uses owned strings. A scoped read adapter can avoid copies while
preventing collection/reentry during a storage borrow; adding an unrestricted
&str tied to mutable argument slots is not acceptable. Zero-copy optimization
requires evidence and the same callable semantics.

Module, type, trait, method, parameter and return documentation continue through
the existing declaration model into full generated KGR. Parameter names are
explicit because Rust closure reflection does not supply them. Source-enabled
analysis parses that KGR normally; materialized content-addressed files support
LSP navigation. Source-free installation consumes checked declarations directly.
This introduces neither declaration binaries nor a second language type catalog.

## Object access and mutation

Construction also belongs to the public API. A checked object builder takes an
applied type and typed field handles, converts/root-protects each initializer,
and publishes only after required fields and access rules validate. Failed builders
release their temporaries and expose no partially initialized object. Enum creation
takes an applied variant handle and its payload tuple, preserving ordinary nominal
identity and empty/unit variant rules. Collection factories take the declared
element/key/value types; native factories take a registered Rust payload and its
validated storage descriptor. Constructors never require raw object IDs or Value
assembly. Existing constructor visibility and initialization rules remain enforced.

Enum preparation uses `bind_enum_type` (or `bind_enum_type_declaration` with a
registration's `TypeRef::id`) followed by `bind_enum_variant::<PayloadTuple>`:

```rust
let packet = runtime.bind_enum_type(
    &loaded, "Packet", &[item_type.type_argument().clone()],
)?;
let data = runtime.bind_enum_variant::<(Object, String)>(&packet, "Data")?;
let value = data.create(cx, (item.clone(), "payload".to_owned()))?;
```

The result is a retained `ScriptValue` with the exact applied enum type. It can be
passed to a typed script call or decoded to a matching Rust mapping, including
ordinary Option/Result mappings. It does not erase the enum's nominal identity.
An empty variant takes `()`; a variant containing one unit value takes `((),)`.
Binding checks the complete Rust payload tuple before any converter runs. Creation
roots every converted field, then validates and publishes one ordinary enum using
the shared allocator, including its existing failure-provenance behavior. Conversion
failure releases temporaries and publishes no partially initialized enum.

For a registered `VariantRef`, `packet.variant_declaration(runtime, variant.id())`
produces an `EnumMember` for `bind_enum_variant_declaration`; the full identity must
belong to that applied enum. Public access, arity, payload scopes, runtime ownership
and program retention are checked. Reusing or cloning `EnumVariant` reuses its
prepared payload descriptor; creation performs no variant-name lookup. Registered
unconstrained enum templates may be applied without script constructor roots.
Declared bounds require a compatible concrete layout in the installed product;
an open representation template alone does not establish a new bounded application.
Concrete layouts admit supplied scopes only when their complete layouts agree.

For maps and sets, `NativeContext::create_map::<K, V>()` and `create_set::<T>()`
use the installed public zero-argument `new` application. Dynamic object mappings
use `create_map_with_types(&key, &value)` or `create_set_with_type(&element)` with
explicit scoped `TypeArgument` values. Repeated construction can prepare once:

```rust
let factory = runtime.bind_map_constructor::<Object, i32>(
    &loaded, key_type.type_argument(), &i32_type,
)?;
let map = factory.call(cx, ())?;
map.insert(cx, key.clone(), 42)?;
```

`bind_set_constructor` follows the same model. These return ordinary cached
`PinnedFunction` handles, with their usual owner, generation and execution checks.
The installed program must contain a concrete constructor entry or a closed shared
call for that application. Merely having a generic declaration does not supply its
Hash/Eq evidence. Missing evidence, ambiguous constructors, incompatible supplying
scopes and incorrect Rust mappings fail before allocation. Construction executes
the registered `new`, preserving its selected operations, storage validation and
failure behavior; the host adapter does not assemble raw hash storage. Each
successful call returns a fresh, automatically retained collection. Stored key
operations remain executable metadata edges, so releasing the host handles can
reclaim an otherwise unreachable old program and collection graph.

Member lookup produces a checked field or method handle from a registration or
loaded program. Names are accepted at the explicit lookup boundary, not searched
on every access. A handle records declaration identity, applied type, access,
owning program generation and any required generic scope. It does not become valid
for a foreign runtime or a same-shaped unrelated type.

```rust
// Once after loading/binding this applied type and program version.
let bindings = PlayerBindings {
    hp: runtime.bind_field::<i32>(&player_type, "hp")?,
    on_damage: runtime.bind_method::<(i32,), ()>(&player_type, "on_damage")?,
};

// Repeated calls reuse this application-owned binding bundle.
let previous = player.get(cx, &bindings.hp)?;
let remaining = previous.saturating_sub(damage);
player.set(cx, &bindings.hp, remaining)?;
player.call(cx, &bindings.on_damage, (damage,))?;
```

For direct declaration binding, `player_type.method("on_damage")` returns an
`InherentMember`; `runtime.bind_method_declaration::<(i32,), ()>(&member)` prepares
the same checked call without repeating the member-name lookup. The method tuple
excludes `self`. Static members use `bind_associated_function` or its declaration
form and return a `PinnedFunction<A, R>`; a public `Player::create(hp)` can therefore
be bound as `(i32,) -> Object` and called through the ordinary VM/SDK call API.
Both forms consume public inherent-member metadata from the installed product,
including the declaring impl and receiver scope. A function body's presence alone
does not grant host member access.

Script `ObjectType` and registered `NativeType<T>` implement the same `InherentType`
binding boundary. `NativeObject<T>::call` uses the same checked method descriptor,
receiver conversion, execution stack and cache as `Object::call`. Calling while a
payload borrow is active fails before execution. `method_declaration` accepts a
full declaration identity, including the `FunctionRef::id()` returned by native
registration, without a name lookup. Same Rust payload types do not make different
nominal applications or program generations interchangeable.

Native inherent registration uses `InherentMethodsBuilder::bind_typed_method` for
`Fn(&mut NativeContext, receiver, arguments)` callbacks and `bind_typed` for static
members or complete argument tuples. `method(&reference, configure)` supplies the
ordinary `FunctionBuilder` for method-local parameters, bounds, concrete interface
results and selected callable requirements. Impl parameters precede method-local
parameters in the executable signature; moving a declaration into a later impl
group substitutes all receiver-dependent facts together. Callback code uses retained
handles and selected calls without decoding raw slots or managing roots manually.

`bind_method_application` and `bind_associated_function_application` (and their
`_declaration` forms) take method-local `TypeArgument` values. Impl parameters are
inferred from the retained receiver, including nested receiver patterns. Each
inferred argument preserves its supplying layout scope; repeated parameters must
agree in that scope as well as nominal identity. Preparation requires an existing
concrete entry or closed shared-call witness for the combined impl/method
arguments. Bounds and selected operations come from that validated evidence.
Missing applications and incorrect Rust mappings fail before execution. Public
script inherent templates can be emitted by source analysis, while ordinary public
generic script free functions remain unsupported. A compiled concrete struct
layout cannot be rebound to incompatible type-argument layouts from another
program with the same nominal IDs.

This callback explicitly chooses saturating damage arithmetic; ordinary Kagari
numeric operators retain their specified checked behavior. Object-valued reads
return automatically retained handles. Writes check
declared writeability, the receiver's access view, nominal type, incoming object
ownership and active leases before committing. Readonly views remain shallow.
Value conversion and any fallible preparation happen before the individual field
commit. A later callback failure does not roll back an earlier successful write.
Compound source operations retain their specified once-only evaluation and commit
rules; this API does not add general transactions.

Collection handles expose get/set/push/insert/remove and bounded iteration through
their existing checked storage operations. Scalar items can return owned copies;
object items return protected handles. Iteration and key lookup preserve existing
structural-mutation restrictions, callback ordering and hash/equality contracts.
Mutable-buffer leases are advanced operations with scoped access, not raw pointers.

Managed Rust payload editing uses NativeObject<T> and a short borrow. In this
example PlayerState is registered as containing no script references:

```rust
native_data! {
    pub struct PlayerState {
        pub hp: i32,
        pub position: [f32; 3],
    }
}

let mut declaration = module.define_type("Player");
declaration.native_storage(NativeStorage::data::<PlayerState>())?;
let player_declaration = declaration.finish()?;
// Prepare once for the installed program. Clones reuse this descriptor.
let player_type = runtime.bind_native_type::<PlayerState>(&loaded, &player_declaration, &[])?;
let player = player_type.create(cx, PlayerState { hp: 100, position: [0.0; 3] })?;
player.edit(cx, |state: &mut PlayerState| {
    state.hp = state.hp.saturating_sub(damage);
    Ok(())
})?;
player.call(cx, &bindings.on_damage, (damage,))?;
```

Direct edit with &mut T requires a registered storage capability establishing that
T contains no script references; an arbitrary user assertion is not a safe proof.
Safe registration uses supported field descriptions, while a manually supplied
storage contract belongs to the trusted low-level boundary. edit checks the
registered Rust type and this capability and obtains an exclusive payload lease.
Its closure cannot return a borrowed reference or keep a runtime context for reentry.
The lease ends before the following call. A simple add_method_mut registration can
automatically apply this pattern; it receives a restricted non-reentrant context.
Complex methods receive the object handle and explicitly alternate edits and calls.
Nested conflicting access produces a structured borrow error, not a RefCell panic.

The implemented `native_data!` form defines the complete Rust struct and checks
every field against `NativeData`. Supported scalars, fixed arrays and tuples compose
recursively. Its blanket payload implementation supplies empty tracing and fixed
heap accounting. `NativeStorage::data` explicitly enables direct editing;
`NativeStorage::payload` alone does not. Direct edits do not permit variable-size
owned containers or script handles. This keeps allocation accounting invariant,
including when an edit fails or unwinds after completing writes. Manual unsafe
`NativeData` implementations are part of the trusted storage boundary and must
uphold the documented recursive field contract; ordinary users need no unsafe code.

`NativeObject<T>::read` lends a shared payload reference only for its closure.
`NativeType<T>::create` validates the registered Rust representation and traced
edges before allocation, roots the result before a safepoint and retains its exact
applied scope. `bind_native_type_declaration` accepts the portable registration
identity directly. Typed constructor callbacks can use `cx.create_native(payload)`
to inherit their checked result application, including generic arguments, without
capturing a handle from a particular runtime installation. Contextual conversion
checks both the Rust payload mapping and the Kagari nominal scope; registering the
same Rust representation under two type names does not make them interchangeable.
Handwritten `NativePayload` remains an advanced tracing interface and receives no
unrestricted mutable access through these handles.

This facility applies to a payload exclusively owned by runtime storage. Arbitrary
KGR fields do not become Rust &mut T. Externally shared host state continues to use
its declared host interface, typed paths or host-managed synchronization. Ordinary
object setters cannot bypass host-path preparation/commit or borrow validation.
Payloads with script references instead expose a restricted edit view with
registered setters/edge wrappers. It provides no unrestricted &mut T or mutable
access to traced slots. Writing a script handle validates ownership/type and
produces a traceable internal edge through the common mutation boundary rather
than embedding a host root lease. Replacing an entire payload must use that same
checked path. Scanning after an unrestricted edit is not a substitute for this
contract: an eventual barrier may need an old edge before it is overwritten.

The checked representation is `Managed<T>`, where T is fixed `NativeData` and
script-valued fields are private storage. Author the field schema before installing
the native type. A registration token identifies its schema and slot; binding the
token checks the Rust mapping against the installed field type and pins its scope:

```rust
let mut storage = ManagedStorage::<PlayerState>::new();
let name = storage.field::<String>("name", String::kagari_type(&catalog)?)?;
let on_damage = storage.field::<PinnedFunction<(i32,), ()>>(
    "on_damage", Type::function([Type::i32()], Type::unit()),
)?;
let mut declaration = module.define_type("Player");
declaration.native_storage(storage.finish())?;
let player_declaration = declaration.finish()?;

// After installation, retain these bindings for repeated access.
let player_type = runtime.bind_native_type::<Managed<PlayerState>>(
    &loaded, &player_declaration, &[],
)?;
let name = player_type.bind_field(&runtime, &name)?;
let on_damage = player_type.bind_field(&runtime, &on_damage)?;
let mut builder = player_type.build(PlayerState { hp: 100, position: [0.0; 3] })?;
builder.set(cx, &name, "Player".to_owned())?;
builder.set(cx, &on_damage, callback)?;
let player = builder.finish(cx)?;

player.edit_data(cx, |state| { state.hp = state.hp.saturating_sub(1); Ok(()) })?;
player.set(cx, &name, "Renamed".to_owned())?;
let callback = player.get(cx, &on_damage)?;
callback.call(cx, (1,))?;
```

These are private Rust payload fields, not dynamically added Kagari members.
They may use the registered native type's generic parameters. Applied field types
retain the supplying argument scopes even when the caller uses another program
version. Field bindings can be cloned and reused; shared schema records have an
identity fast path, while independently prepared applications require compatibility
validation. No binding cache or performance claim depends on names alone.

`ManagedBuilder` roots each converted initializer. Missing fields or conversion
failures cannot publish a partial object; an unsuccessful initializer replacement
preserves the previous initializer. `builder.replace(cx, &player)` replaces the
complete data/field set only after all values pass checks. Ordinary setters perform
conversion and rooting before the individual commit. Both write paths see old and
new edges before overwriting; they store Values traced through the existing payload
descriptor, never public Object/Function root leases. Reading a function field
returns a retained PinnedFunction, and its invocation occurs after the storage
borrow ends. A payload/closure cycle is therefore retained by an external handle
and reclaimed when no external roots remain.

`edit_data` exposes only T. The complete Managed payload and its traced slots cannot
be lent mutably, and the short data borrow cannot escape. Fixed data and field count
keep heap accounting stable during edits and whole-payload replacement. Dynamic
strings, collections, ordinary objects, interfaces and callbacks use checked field
conversion instead of unrestricted Rust mutation. Typed native constructors can
obtain this builder's applied type from `cx.result_native_type::<Managed<T>>()`.
As with other source-free bindings, field types must have their required checked
layouts in the installed product; binding does not compile missing declarations.

## Binding preparation and reuse

bind_field, bind_method and bind_function are preparation operations. Perform them
when installing a program, initializing a host service or first using an applied
type, then retain their handles. Binding does not generate machine code. The example
above uses an ordinary application-owned PlayerBindings struct to make this lifetime
explicit. Neither object access nor a native callback implicitly repeats binding.

Binding resolves the declaration once, checks visibility and the Rust-facing type
mapping, and prepares a descriptor containing a field slot or checked callable
target plus its scoped signature. The steady-state scalar field path is checked
root/object lookup, receiver/layout/access validation and indexed slot access.
It performs no member-name lookup, structural signature comparison or allocation
of another binding descriptor. The receiver/layout check can compare a prepared
identity rather than reconstructing its generic semantic type. Mutable access,
generation and active lease checks remain necessary on each operation.

Method calls reuse the prepared target or an interface's verified method ordinal.
Argument conversion, temporary retention, frame setup, cancellation/depth checks
and execution still have costs; trait implementation search is not among them.
Scalar get/set needs no new owning object handle. Object-valued reads/results may
need a host lease, and owned String/Vec conversion can allocate and copy. An ID
lookup is not a claim that these operations cost the same as a direct Rust field
access. Measure first binding separately from repeated scalar access, object
access, ordinary calls and trait calls before making performance claims.

Runtime may deduplicate binding records. A cache key includes the runtime/install
identity, pinned program/provider generation, applied receiver type and lexical
scope, member declaration, access view and Rust argument/result mapping. Selected
generic operations also include their verified selection identity. Names or a Rust
TypeId alone are insufficient. Repeated binding can reuse a record, but retaining
the returned handle still avoids even the cache lookup on the normal hot path.

Cache entries hold runtime-local descriptor IDs and do not introduce permanent
roots. Exported live handles pin their required program; the cache itself must
not retain an otherwise unreachable old version. Release cache records with their
owning version or remove them when their descriptor generation becomes stale.
After reload, a retained binding continues to address its original version. Bind
once for the new version when it is selected; never overwrite an old descriptor
with a new field slot or method target. Existing schema and version checks decide
whether a particular object can use a binding.

Registration already produces declaration/member handles. Provide a binding path
that consumes those handles directly, so native-authored members need no string
lookup at all. Generated Rust binding bundles can use the same path:

```rust
// Optional generated view: preparation still checks the installed program.
let player_api = PlayerApi::bind(&mut runtime, &loaded)?;
let hp = player_api.hp(cx, &player)?;
player_api.set_hp(cx, &player, hp.saturating_sub(damage))?;
player_api.on_damage(cx, &player, (damage,))?;
```

A generator emits schemas, member references and thin wrappers, not unchecked
physical offsets or permanent method-table ordinals. Loading still validates the
actual program and resolves generation-specific slots. Generated calls and manual
binding bundles share descriptors and execution paths. GO05 requires cached binding
and direct declaration-handle binding; an optional whole-program Rust code generator
is a later tooling consumer, not another prerequisite or semantic implementation.

## Functions and trait methods

Function and method invocation share one conversion, rooting and execution path.
The normal API returns typed data or retained handles instead of a mandatory raw
ExecutionReport. Observers provide optional execution statistics.

```rust
let make = runtime.bind_function::<(), Object>(&loaded, "make_player")?;
let player = runtime.call(&make, ())?;
let accepted: bool = predicate.call(cx, (player.clone(),))?;
```

bind_function returns PinnedFunction<A, R>. It verifies visibility and the complete
signature before running code, then retains the selected version. Concrete generic
applications must have checked executable evidence. Source-free calls cannot ask
HIR to specialize a missing instantiation. Unsupported bindings fail before script
effects; they do not synthesize implementations or pick similarly named methods.
Existing language export rules still apply: ordinary script generic free functions
remain private templates exposed through concrete public wrappers. Registered
native generic functions and supported generic trait members use their checked
applications; host binding does not make a private declaration public.

A PinnedFunction for an installed native entry converts to a script function value
through the ordinary closure heap representation. It stores the checked native
import, scoped signature and applied environment; it does not require a generated
script wrapper. Script invocation enters the existing native frame, sharing argument
checks, synchronous reentry, result adaptation, cancellation and call-depth cleanup.
The closure traces its owning program and environment as graph edges, without an
internal host ProgramLease or RootSet. It can be returned, captured by another
closure or stored in a managed field, and obsolete module-state cycles remain
collectable. Both concrete imports and installed closed shared-native applications
are supported. An open template without application evidence still cannot be bound
or boxed. Diagnostic closure snapshots distinguish script and native targets;
copying a snapshot does not authorize execution or republish collected metadata.

Trait calls support two existing forms:

| Form | Selection and target API |
| --- | --- |
| Interface<S> value | Parameter/binding validation establishes the applied interface; call uses its checked dispatch table, including supported inherited/default members |
| Generic native parameter with a trait bound | Registration names the required member; the compiler/provider supplies the selected operation witness for the actual type |

```rust
let method = damageable.method::<(i32,), ()>("take_damage")?;
target.call(cx, &method, (damage,))?;
```

damageable is an installed trait declaration handle, and target is already a
checked Interface for its applied type. A generic SelectedMethod handle exposes the
same call conversion surface without making the author select an integer witness
slot. Associated outputs retain their supplying scope. Static trait functions use
their actual argument list and a selected operation, without inventing a receiver.
Trait implementation selection occurs during source checking or validated binding;
execution does not perform open-ended trait search.

For a native generic function, `FunctionBuilder::requires` returns an opaque
`SelectedCall` token tied to the authoring declaration. Bind the callback with
`ModuleBuilder::bind_typed`; its tuple mappings are checked against the concrete
invocation before converters or callback effects. Within that callback:

```rust
let read = cx.selected_method::<(ScriptValue,), ScriptValue>(&required_read)?;
let result = read.call(cx, (receiver,))?;
```

The returned `SelectedMethod<A, R>` owns the selected code version, generic
environment and lexical signature. Retain or clone it for repeated calls, even
after the native callback returns. Its tuple is the actual selected function
signature: instance operations include the receiver first; static operations have
only their declared arguments. Prepared primitive operations use the same typed
conversion and execution policy. Source checking supplies all selection evidence.
A token from another declaration is rejected even if its internal ordinal matches.
Typed native trait defaults use `TraitBuilder::bind_default_method`, whose Rust
callback receives `(cx, receiver, argument_tuple)` separately; its tokens keep the
authored method identity when the body lowers to a private native function.

The concrete interface binding API resolves an `InterfaceMember` and prepares
`InterfaceMethod<A, R>`. A returned view can supply its applied type directly:

```rust
let member = target.member(runtime, "take_damage")?;
let method = runtime.bind_interface_method::<(i32,), ()>(&member)?;
target.call(cx, &method, (damage,))?;
```

For preparation before a value exists, use
`runtime.interface_member(&loaded, &applied_interface, "take_damage")`, where
`applied_interface` is a checked `TypeArgument`. The corresponding
`interface_member_declaration` accepts a registered `MethodRef::id()` or another
verified declaration identity. It resolves inherited members in the applied view
and checks public access. Retain the resulting method for repeated calls on values
of that view and program generation. Name lookup occurs during member preparation;
calls use the existing checked dispatch ordinal and result adapters.

Method-local type arguments use
`runtime.bind_interface_method_application::<A, R>(&member, &type_arguments)`.
The installed program must contain a checked closed call for that applied member
and argument list. Binding retains its lexical type scopes and prepares the
selected bound operations once; execution combines them with the receiver's
dispatch selection. Missing executable evidence fails at preparation. This API
does not request source specialization or infer an implementation at call time.

Applied interface signatures retain the compiler's associated-type normalization
facts when substitution alone leaves a projection. Artifact validation proves each
source/result equality against the linked declarations and implementations before
the VM or host binder consumes it. Receiver and call-selected operation facts must
enter the method environment before its parameter and result scopes are resolved;
adding them after signature preparation is too late for associated outputs.

The invocation sequence is: validate/pin target and signature; convert/protect
arguments left to right; release storage borrows; enter the existing execution
session; invoke; protect and validate the result; transfer it to the caller;
release temporaries on every exit. Synchronous native-to-script-to-native reentry
shares cancellation, call depth and the pinned dependency graph. Object and method
version checks prevent accidentally applying a new layout to an old object.
Old closures/interfaces continue to invoke their retained implementations after
reload. Failed publication leaves existing handles usable.

## Crate responsibilities

| Owner | Responsibility |
| --- | --- |
| kagari-runtime | Stores, identities, tracing, leases, object access, conversion context, native binding validation and call services |
| kagari-vm | Drive checked executable calls using runtime-owned frames; preserve return/unwind ordering |
| kagari-embed | Ergonomic registration and typed call/binding entrypoints composed from runtime mechanisms |
| kagari-types | Existing source-independent declarations, nominal schemas, bounds, member access and documentation |
| contract, MIR and bytecode | Checked member/call evidence, scoped signatures and bounded executable validation |
| compiler/HIR | Existing analysis, trait selection and production of required executable evidence |
| kagari-stdlib | Ordinary registrations and implementations using the same host API |
| ABI and backends | Physical execution, code ownership and supported safepoints; no Rust authoring policy |

Do not create separate GC, registry or adapter crates without a concrete dependency
need. Do not move mixed ownership into common or make executable consumers depend
on source analysis. High-level APIs and any necessary internal fast paths lower
to the same checked operations; obsolete raw public entrypoints are replaced.

## Implementation checkpoints

The roadmap holds the active checkboxes and progress ledger.
Each accepted checkpoint uses its GO phase trailer and one
coherent Conventional Commit. Earlier EN acceptance is not reopened.

| Phase | Scope | Acceptance before progressing |
| --- | --- | --- |
| GO01 | Checked central root storage, common root enumeration, host root leases and object/slot identity | Retention/clone/drop, foreign/stale handles, root transfer, teardown and forced-GC coverage pass |
| GO02 | Central frames/sessions/programs/environments; separate object policy, storage traversal/writes and collection | Coordinated reachability, reentry/unwind, escaped environments, cyclic metadata and old-version release pass; internal edge writes use controlled storage paths |
| GO03 | Runtime transfer and callback/payload ownership constraints | Runtime Send/not-Sync contract, deterministic thread handoff, Tokio task acceptance and supported backend transfer pass |
| GO04 | Recursive typed adapters, documented registration and owning public results | Strings/composites, argument tuples, business Result, typed entry arguments, KGR docs/navigation and source-free conversion pass |
| GO05 | Object/collection mutation, restricted traced-payload editing, prepared member bindings and methods/closures/trait calls | Alias visibility, binding reuse, access/borrow failures, checked edge writes, GC during reentry, selected static/default/generic methods and pinned reload pass |
| GO06 | Standard-library adoption, extension exercise, removal of superseded APIs and integration | New registrations use storage descriptions without collector changes; public examples, all behavior/feature/backend checks and current specifications pass |

Keep intermediate checkpoints building. Work may replace a vertical internal path
within one phase, but no accepted checkpoint carries an unresolved build failure.
Do not retain compatibility aliases or duplicate runtime graphs to preserve obsolete
callers. Unpublished generated artifacts are disposable and regenerated at a coherent
checkpoint when their representation changes, without old readers or routine ABI
version increments. Every implementation checkpoint includes structural review,
focused behavioral checks and git diff --check; full integration is required at GO06.

## Acceptance matrix

| Area | Required evidence |
| --- | --- |
| Object graph | Shared aliases, cycles, nested enum/collection fields, payload/callback cycles and rootless reclamation under collection threshold 1 |
| Metadata graph | Escaped generic environments, selected operation groups, old module-state cycles, exact dependency retention and release after last owner |
| GC boundaries | Review object/type/error policy outside the collector; add an ordinary registered type and a managed payload with script edges through existing descriptors, then demonstrate retention and cyclic reclamation without mark/sweep changes |
| Reference writes | Force collection around field/collection replacement, removal, bulk edits and publication; exercise frame/root and metadata link changes; new referents survive and detached rootless graphs are reclaimed |
| Payload editing | Reject unrestricted edits of traced payloads and escaped traced-slot borrows; checked reference replacement preserves aliases and ownership/type checks; errors/unwind release leases and preserve completed writes |
| Public handles | Automatic retention of return values/field reads/callback results, clone/drop across host threads, wrong runtime/generation and handles surviving runtime teardown |
| Transfer | Runtime compiles as Send and rejects shared concurrent access; move live rooted objects between threads; native service/payload bounds; destruction on receiving thread |
| Conversion | Supported scalar widths, UTF-8 strings, nested composites, installed nominal Option/Result, empty collections, bounded owned-cycle rejection and alias-preserving handles |
| Registration/tooling | Full docs, parameter names, nominal identity, atomic invalid binding rejection, generated KGR parsing and materialized LSP targets |
| Construction and mutation | Atomic object/enum/payload creation, shared-object updates, readonly/private/type rejection, short payload borrows, no escaped references, no callback during incompatible leases and completed-effect preservation |
| Calls | Ordinary methods, closures, inherited/default/static trait members, generic witnesses, associated outputs, wrong signatures and source-free execution |
| Binding reuse | Repeated access uses prepared descriptors; direct declaration-handle binding matches name-based binding; runtime/generic/access cache separation; old bindings remain pinned and caches do not leak retired versions |
| Reentry/cleanup | Callback-triggered GC, result transfer without a root gap, traps/cancellation/depth failure, no outstanding frames/leases/roots after exit |
| Reload | Live old objects/functions use old layouts/dependencies, new handles use their selected version, incompatible member binding rejected and failed reload leaves old handles valid |
| Standard library | String split, object-valued Vec mutation, callback-based collection operations and trait-driven native algorithms use the public typed path without raw root/slot boilerplate |
| Features/backends | Artifact-only/source/native/combined consumers; existing actual scalar JIT behavior and checked fallback; no claim of unsupported native GC-object execution |

Use the existing runtime GC ownership, VM GC/reentry, embed generic reload/native
artifact and standalone feature suites. Add behavioral tests for the listed new
contracts, including compile-fail cases for escaping borrows and non-Send captures.
Use repeated collection and version-count assertions to detect retention bugs.
Benchmark any claimed improvement with the repository measurement policy; object
counts alone do not establish lower memory usage or faster execution.

Final integration commands:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv run python scripts/check_features.py
cargo test -p kagari-cli --features jit
git diff --check
```

Documentation-only changes require local-link/content and diff checks; implementation
checkpoints require the phase evidence above.

## Reference designs

- [Lua 5.4 GC](https://www.lua.org/manual/5.4/manual.html#2.5) supplies an example
  of tracing with incremental/generational modes. Its
  [registry reference API](https://www.lua.org/pil/27.3.2.html) illustrates
  centrally retained values accessed by integer references.
- [QuickJS](https://bellard.org/quickjs/quickjs.html) uses reference counting with
  cycle removal and explicit JS_DupValue/JS_FreeValue at its C boundary. That is
  a different collector policy; adopting automatic host retention does not require it.
- [V8 handles](https://v8.dev/docs/embed#handles-and-garbage-collection) distinguish
  scoped and persistent retention. Its moving collector does not imply Kagari needs
  object relocation to provide safe handles.
- [mlua functions](https://docs.rs/mlua/0.11.6/mlua/struct.Lua.html#method.create_function)
  and [methods](https://docs.rs/mlua/0.11.6/mlua/trait.UserDataMethods.html) are
  references for typed argument/result conversion and receiver injection. Kagari
  preserves static nominal declarations and single-value returns rather than Lua's
  dynamic table/multiple-return conventions.
