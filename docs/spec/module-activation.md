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
retirement checks remain mandatory; labels confer no authority. Runtime owns the
installation: dropping Program/Module handles does not unload it, cancel tasks or
reset statics. Independently retained callable handles remain usable while it is
live. Runtime close/drop clears live metadata and ends execution/object-access
authority; a retained exact descriptor cannot revive it. `root()` can still construct
an identity handle after close, but subsequent lookup/binding rejects it. Explicit
unload remains separate from compatible publication.

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
Old executable payloads remain while exact computation, explicit executable owners
or retained layout/native evidence need them, not merely because source diagnostics
or a named binding survive. [Type-only retention separation](../review.md#r13--type-and-layout-provenance-retains-retired-code)
remains deferred; releasing final actual owners is not proof that R13 is implemented.

## Compatible declaration changes

Compatibility preserves every existing named declaration contract, including private,
unused and generic declarations, while admitting checked additions. It is not whole
artifact equality, public ABI equality alone or a test of currently live values.
"Existing" includes declarations introduced by any earlier successful publication.

| Change | Compatible publication |
| --- | --- |
| Existing function or method body, including Hash/Eq and ordinary initializer helpers | Allowed after checking and retained-demand coverage |
| New free function, inherent method, module, struct, enum or trait | Allowed when old contracts and public namespaces remain valid |
| Trait implementation for a newly introduced outer nominal type, including `New<T>` | Allowed with normal coherence and checked implementation evidence |
| New trait implementation for an existing type, including a newly added trait | Rejected; primitive, tuple and blanket extensions are not loopholes |
| Delete, rename or change visibility/access of an existing declaration | Rejected |
| Existing parameters/receiver, result, generic parameters/bounds or sync/async mode | Frozen |
| Existing struct fields/types/writeability or enum variants/payloads | Frozen, including layout additions |
| Existing trait members, parents or associated-type requirements | Frozen |
| Existing implementation receiver, trait, associated outputs or dispatch relationship | Frozen; method bodies may change |
| Existing named constant value/type | Frozen |
| Existing static's direct initializer expression and checked direct bindings | Frozen, ignoring trivia, independently of initialization state; ordinary called helpers may change |
| Existing public function alias | May retarget under the separate public-namespace contract; retained bindings keep their declaration |

For example, a batch may add `FixedDamageRule` implementing an existing `DamageRule`
trait. Business code may then construct it and insert it into an existing collection
of `DamageRule` through normal type/ownership checks; publication does not mutate the
collection. Existing elements retain their concrete types and implementation
relationships, while subsequent method calls select current bodies. Once published,
`FixedDamageRule` is an existing type: a later batch cannot extend its trait
relationships or fields. No automatic interface upgrade or object migration occurs.

Declaration correspondence uses checked package/module/authored-owner identity, not
source order, numeric executable slots or same-shaped signatures. Semantic impl
headers and binder correspondence preserve method ownership across placement changes.
Named block-local functions are declarations, unlike generated closures/resume bodies.
Ambiguous same-name locals require unchanged enclosing text and indexed executable
evidence; they are not paired by occurrence or signature when the owner changes.
Ordinary local variables, temporaries and generated capture layouts may change with
new bodies; already retained computations keep their matching old body/layout.
Native registrations and host passing/storage contracts remain checked; compatible
script publication does not replace arbitrary Rust code.

## Public namespace continuity

Compatible publication preserves each previously public canonical namespace as a
reachable query start, including namespaces retained through an earlier alias.
Equivalent root path spellings alone do not justify removing a namespace held by
an existing `Module`. Every query captures one latest publication; declaration
lookup and executable-target lookup cannot mix publications. Failed publication
leaves the same handles observing the old contents; success exposes additions
without reacquisition.

Existing public paths and signatures, nominal type applications and the separate
root `main` contract remain checked. Bounded cyclic-graph comparison permits
compatible additions and equivalent graph shapes, not removed/private paths.
A function alias may retarget to another public declaration with the same checked
signature: fresh binding chooses the new declaration, while retained bindings call
the original declaration's current implementation. Type aliases instead preserve
normalized nominal target and arguments; another same-shaped type is not equivalent.
[Artifact type exports](artifacts.md#public-type-exports) owns that comparison's
portable evidence. Frozen declarations and direct static-initializer bindings remain
additional requirements, not exceptions to be bypassed through aliases.

Type identities and trait implementation relationships are fixed within an
installation; that does not freeze method bodies. Independently loaded copies do
not share script nominal or trait authority merely because names/layouts match.
Shared native-trait authority requires actual installed native imports, not a
reference or an empty same-name provider. Dependency-only private-type evidence
can prove layout/bound validity but grants no public name or private-member access.

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

One publication covers one complete Program installation, including affected imported
modules. It is not a global update of every installation using a package. Code
atomicity is not a business-state transaction: an operation can call a helper,
suspend, then call its new body after publication. If unchanged storage acquires a
new business meaning, the host owns any necessary admission/routing coordination.
Ordinary compatible body fixes do not require all parked work to drain, and
publication does not cancel tasks or roll back completed script/external effects.

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
and retains that generation's recipe body, not an old transitive call graph.
For example, if A introduces an unread `COUNT = make_count()`, B may change the
body of `make_count` while preserving the direct expression and its checked binding.
A first read after B publishes enters A's recipe, whose ordinary call selects B's
`make_count`. Once Ready, old and new generations observe and update the same cell;
publishing B neither reruns the recipe nor resets it. Failed cells remain Failed;
changing a helper does not retry a failed initializer.

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
| Change an existing direct initializer expression or its checked direct bindings | Rejected; trivia is ignored and the original recipe remains fixed |
| Change an ordinary helper called by an initializer | Allowed after normal checking; calls select its current implementation |
| Reorder declarations without changing contracts or direct initializer meaning | Allowed after full checking |
| Delete, rename or move an existing static | Rejected |
| Change an existing static's type, mutability or visibility | Rejected, including private statics |
| Change a relevant applied type/layout, native or interface contract | Rejected |
| Add/reorder unrelated generic specializations | Not a change to the existing static's applied schema |

Compatibility follows the declared applied type's recursive schema closure, not
just currently stored values or the inventory of every specialization of a nominal
type. `None` and empty collections do not excuse a changed declared contract.
Existing declaration, public path, callable, type and implementation contracts
remain frozen. Additions, including modules, must satisfy complete candidate checking,
retained application demands and local initializer comparison. Code rollback is another
checked publication, not state rollback: it cannot remove an introduced static.

## Local initializer comparison and body provenance

Compatibility compares each existing static's direct initializer expression and
checked direct bindings, not the transitive implementation of its callees. Non-trivia
tokens remain equal even if different expressions would optimize to the same result;
comments and formatting may change. Changing an import or alias so that the same
expression binds to a different declaration is incompatible. The rule applies to
cold, Ready and Failed cells alike, independently of live heap values.

Explicit authored body provenance identifies the initializer's own executable
recipe and lexically owned generated bodies, such as a closure written directly
in the initializer. These local bodies and their checked call/type facts remain
part of comparison. An ordinary named helper and generated bodies owned by that
helper are not part of the initializer merely because it calls them. There is no
Hash/Eq protected-root graph, transitive body freeze or installation-wide `Unknown`
freeze. Dynamic/interface/native calls do not expand local initializer ownership.

Frozen declarations, signatures, types, layouts and trait relationships remain
mandatory. MIR/bytecode validation independently reconciles explicit authored
provenance, preoptimization body evidence, surviving executable instructions and
checked selections with the executable inventory. Missing or forged evidence is
not accepted as an empty record. Source-free evidence declares compiler facts,
not authenticated original source; removing transitive protection does not remove
executable verification or [retained-demand coverage](#retained-demand-coverage-and-bounds).

Hash/Eq bodies and their ordinary helpers follow normal compatible body updates.
Users must preserve stable key hashing/equality while keys remain stored. Publication
neither rehashes a table nor pins old protocol execution to preserve lookup results.
Engine memory, GC and host-borrow safety remain enforced, but successful lookup is
not guaranteed after the user breaks the key contract. See
[stored key protocols](collection-access.md#stored-key-protocols-across-reload).
Failure to admit a candidate leaves the old publication and statics unchanged;
publication never implicitly migrates state.

## Retained-demand coverage and bounds

The installation retains a deduplicated, monotonic union of previously published
application/body requirements and each successful candidate's requirements. These
summaries are metadata, not executable leases. Candidate staging keeps prospective
history separate; rejection cannot advance the installation's authoritative envelope.
Historical generated bodies contribute their old call/creation edges, but ordinary
callee requirements must resolve to **candidate** implementations. A generic
application no longer reachable from new source still needs current executable
coverage if old retained work can call it.

Compiler preparation closes function, shared-body, interface, construction and
layout demands with checked type/witness evidence. Dynamic receiver coverage includes
admissible new implementations for new types without changing old relationships.
Prepared/source-free reload rejects missing current coverage rather than substituting
an old specialization, inferring types or compiling source in Runtime. Candidate
claims do not replace the installation's own history or executable verification.
Cross-function optimization must not fold, inline or remove a replaceable call in a
way that hides its required current-version selection from retained callers.

Shared hard admission limits are 64 MiB of canonical records, 1,000,000 records/edges
and fixed-point work visits, 65,536 body applications and 4,096 entries per nested
list, with existing type depth/transform limits. Requests may tighten these bounds.
Traversal observes cancellation and rejects overflow/exhaustion; it never truncates
history or evidence to admit a publication. Retirement releases the summary; there
is no claim of precise liveness pruning or an unlimited number of updates. The
contract owner is [updates](../../crates/kagari-contract/src/updates.rs), shared by
compiler, verifier and runtime rather than independently reimplemented solvers.

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
