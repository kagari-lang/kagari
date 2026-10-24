# Module loading and activation

Import resolution is a compile-time and link-time operation. It does not execute script code. Cyclic imports are allowed when their declarations and linked contracts are valid.

Each runtime owns its mutable module instances, heap, cancellation state, host bindings, and installation handles. Verified code, layouts, and native compilation products may be shared by an immutable prepared program; its native compilation cache is not duplicated per runtime. There is no cached module result or eager module initialization pass. A function executes through an explicit call or the internal lazy-static initializer entry described below.

Artifact preparation validates the entire executable closure and any supplied native compiler input before runtime linking. Loading preserves that prepared program identity while linking installation-owned runtime state. Compatible reload preserves existing static cells and stages new generation bindings, so native compilation results can be reused without sharing state or host bindings between installations. Loading validates runtime bindings before publishing a handle. Reload prepares and stages a candidate, checks its schema, ABI, host bindings, expected base version, and candidate-owned object references, then publishes. Ordinary reload does not run any candidate function. The current entry is unchanged if a check fails. Staged candidates are discarded when dropped.

An embedding host may explicitly call functions during a restricted candidate session. That session can mutate candidate-owned nonstatic objects, perform pure computation, and read explicitly immutable configuration; external modifications are denied. All static access is denied, including reads of Ready cells, access to newly staged cells, and indirect or host-reentrant access. Switching candidate/session context cannot escape this restriction. Termination and resource errors invalidate the candidate. Publication rechecks the base version and candidate references after the session ends. Publication is per runtime; coordination across actors belongs to the host.

## Live installations and call selection

Each load creates an independent installation, even for the same prepared code or
diagnostic label. `Program` and its `Module` handles are live installation handles;
`ExecutableModule` is an exact executable-version owner. Reload and source update
return `ReloadResult<()>` in the SDK, advancing the installation rather than
returning a replacement handle. Runtime ownership, installation identity and
retirement checks remain mandatory; labels confer no authority.

Ordinary direct, shared generic, recursive, interface and native-callback calls
select the current implementation of their already checked declaration/application.
Typed `Function` replaces `PinnedFunction`. Named function values and live callable
bindings do not lease obsolete executable code. Public path lookup happens at
binding: changing an alias does not retarget a retained binding to another declaration.
Implementation selection does not repeat trait inference or erase lexical type,
receiver, argument, result or host-borrow checks.

Created closures, cold Futures, admitted frames and admitted queued invocations
retain their selected body/provider and matching type environment. An old resumed
body's ordinary inner calls still use current implementations. For example, if a
parked v1 Future resumes after v2 changes `helper`, it continues its v1 resume body
but an ordinary `helper()` call enters v2. A named function value saved before the
publication also enters current code when called; it is not a closure snapshot.
Old executable payloads remain only while exact computation/explicit executable
owners need them, not merely because source diagnostics or a named binding survive.

## Publication boundary

Publication is serialized at a host-top-level boundary with no active synchronous
execution or driver scope anywhere in the runtime. Observers, native callbacks,
synchronous reentry and their cleanup still count as active scopes even if the
active-session pointer is temporarily hidden. Parked frames, waiting tasks and
producer-side completion notifications do not prevent publication.

`Runtime::begin_publication` admits an exclusive guard before SDK source preparation.
While held, script entry/resumption, driver consumption and another publication
reject. Staging and commit are metadata-only: no initializers, callbacks or GC-driven
host destruction run as preparation effects. Commit rechecks installation/base
identity, bindings and admission. Rejection or cancellation before commit preserves
both the executable and authoritative source baseline. Explicit candidate execution
is separate and cannot run under the publication guard.

Native declarations, explicit impls, external schemas and local projector/storage
capabilities remain part of that checked dependency contract. Reload does not
redirect an existing method or field descriptor to a different Rust layout.
Pinned code still validates receiver liveness: an opaque receiver taken out of
runtime storage remains unavailable to old methods, and a revoked external view
remains invalid after publication. A retained generation never extends a host
scope or resurrects removed payload storage.

## Lazy static activation

Each [static declaration](modules.md#persistent-module-statics) has one checked
cell with explicit `Uninitialized`, `Initializing`, `Ready` or `Failed` state.
`Unit` is a valid value, never an initialization sentinel. Loading, imports,
publication and diagnostic inspection do not force a cell.

The initializer recipe is fixed to the generation that introduced the cell.
Its hidden synchronous, zero-argument function returns the declared semantic type
and retains that generation's recipe body. Its transitive dependencies are
protected by compatibility validation: if A introduces an unread
`COUNT = make_count()`, B cannot change `make_count` through compatible publication.
Once Ready, old and new generations observe and update the same cell; publishing B
neither reruns the recipe nor resets it. Ordinary call selection remains current;
protected dependency equality preserves the initializer's behavior.

The first executing access admits an initializer frame in the existing execution
session. It uses the recipe's loaded module, not the caller's module slot or
current generation. Completion validates and commits the result before retrying
the accessing instruction; already evaluated arguments, indexes and RHS values
are not evaluated again. A store cannot substitute its operand for the
initializer's result or rebind a `val`: a cold access must complete the recipe
first. The compiler ensures a static root before evaluating an assignment RHS;
[assignment ordering](value-semantics.md#evaluation-and-assignment) defines the
separate root-capture and projection-commit rules.

Initializer functions are not ordinary callable entries, even in source-free
artifacts. Only internal initializer admission may enter them. Initialization
shares cancellation, call depth, roots, observers and synchronous reentry with
the caller; it does not create a nested interpreter or independent root session.
[Failure semantics](failure-semantics.md#static-initialization-failures) defines
cycle poisoning and cleanup. [Runtime ownership](runtime.md#persistent-static-cells)
defines the cell/value/recipe graph and non-forcing inspection.

## Static publication compatibility

Staging builds a per-generation binding plan: existing declarations share their
installation cells, while additions remain private until commit. No initializer
runs during staging or commit. Commit rechecks the expected base and publication
authority and publishes atomically without callbacks. Dropping a candidate or
rejecting a stale or invalid publication does not change existing cell states,
values, or active bindings. A candidate cannot be transferred to another runtime
as a way to acquire its installation state.

| Change | Compatible publication |
| --- | --- |
| Add a static in an existing module | Allowed after full checking; initially Uninitialized |
| Change an existing initializer expression | Rejected by protected dependency comparison; existing recipe remains fixed |
| Reorder declarations without changing contracts or protected bodies | Allowed after full checking |
| Delete, rename or move an existing static | Rejected |
| Change an existing static's type, mutability or visibility | Rejected, including private statics |
| Change a relevant applied type/layout, native or interface contract | Rejected |
| Add/reorder unrelated generic specializations | Not a change to the existing static's applied schema |

Compatibility follows the declared applied type's recursive schema closure, not
just currently stored values or the inventory of every specialization of a nominal
type. `None` and empty collections do not excuse a changed declared contract.
Existing declaration, public path, callable, type and implementation contracts
remain frozen. Additions, including modules, must satisfy complete candidate checking,
retained application demands and protected dependency rules. Code rollback is another
checked publication, not state rollback: it cannot remove an introduced static.

## Protected dependencies and conservative rejection

Compatibility compares the transitive dependency closure of existing Hash/Eq and
static-initializer roots, including source templates without executable applications,
called bodies and selected operations. This is installation-inventory validation,
not a scan of live tables or Ready static values. An empty/dropped collection or an
unread/already initialized static does not remove the obligation.

An `Unknown` interface, native, witness or dynamic-call edge in a protected closure
conservatively freezes the full installation inventory. Missing callback metadata
or effect flags are not proof of a sealed native implementation. Consequently,
standard-foundation protected unknown edges can reject even a simple body patch
prepared with the normal engine. This deliberate conservative policy lets the host
choose an appropriate installation/provider closure; the source update API is not
a guarantee that every standard-library program accepts body changes.

Failure leaves the old publication and statics unchanged. No table reindexing,
state migration or liveness-dependent relaxation is implicit. See
[stored key protocols](collection-access.md#stored-key-protocols-across-reload).

## Async activation

The implemented [async executable contracts](../async-execution-design.md#executable-contracts)
define the checked handoff.

Cold Futures and waiting executions retain their original body, types and native
provider contracts across compatible publication; ordinary calls made by those
bodies select current implementations. Candidate
execution rejects Future creation, spawn and await; candidate output graphs
cannot contain Future, Task or scope capabilities through aggregates or aliases.
These checks apply to source-free execution and actual mutable graph state.
State replacement and suspended-state migration are outside this implementation.

While any static initializer is active in a session, cold Future creation (script
or native), spawn, creation of an async scope and await are rejected before their
effects or parking. Indirect calls and synchronous host reentry inherit this
marker. An initializer cannot suspend; owned execution slicing continues to a
valid parking boundary rather than stranding an Initializing cell. Cooperative
cancellation remains active.

An async closure may itself be a stored value: storing the closure is not creating
its Future. During ordinary execution, Future/Task handles may be stored under
normal durable-storage and scope rules, for example in an initialized `Option`.
A static does not extend a scope, revive a cancelled task, permit consuming a
Future twice, or rebind its body/provider generation.
