# Module loading and activation

Import resolution is a compile-time and link-time operation. It does not execute script code. Cyclic imports are allowed when their declarations and linked contracts are valid.

Each runtime owns its mutable module instances, heap, cancellation state, host bindings, and installation handles. Verified code, layouts, and native compilation products may be shared by an immutable prepared program; its native compilation cache is not duplicated per runtime. There is no cached module result or eager module initialization pass. A function executes through an explicit call or the internal lazy-static initializer entry described below.

Artifact preparation validates the entire executable closure and any supplied native compiler input before runtime linking. Loading preserves that prepared program identity while linking installation-owned runtime state. Compatible reload preserves existing static cells and stages new generation bindings, so native compilation results can be reused without sharing state or host bindings between installations. Loading validates runtime bindings before publishing a handle. Reload prepares and stages a candidate, checks its schema, ABI, host bindings, expected base version, and candidate-owned object references, then publishes. Ordinary reload does not run any candidate function. The current entry is unchanged if a check fails. Staged candidates are discarded when dropped.

An embedding host may explicitly call functions during a restricted candidate session. That session can mutate candidate-owned nonstatic objects, perform pure computation, and read explicitly immutable configuration; external modifications are denied. All static access is denied, including reads of Ready cells, access to newly staged cells, and indirect or host-reentrant access. Switching candidate/session context cannot escape this restriction. Termination and resource errors invalidate the candidate. Publication rechecks the base version and candidate references after the session ends. Publication is per runtime; coordination across actors belongs to the host.

The root call pins a dependency program. Nested calls and synchronous host reentry use that same version set and cancellation state. A later publication does not redirect existing calls. Old versions remain usable while reachable and are reclaimed after their remaining references and calls end.

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
and retains that generation's dependency context. For example, if generation A
introduces an unread `COUNT = make_count()` and B changes `make_count`, the first
read from B still runs A's recipe. Once Ready, old and new generations observe
and update the same cell; publishing B neither reruns the recipe nor resets it.
This storage sharing does not redirect ordinary calls to the latest code.

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
| Change an initializer expression or declaration order | Allowed under the other contract checks; existing recipe remains fixed |
| Delete, rename or move an existing static | Rejected |
| Change an existing static's type, mutability or visibility | Rejected, including private statics |
| Change a relevant applied type/layout, native or interface contract | Rejected |
| Add/reorder unrelated generic specializations | Not a change to the existing static's applied schema |

Compatibility follows the declared applied type's recursive schema closure, not
just currently stored values or the inventory of every specialization of a nominal
type. `None` and empty collections do not excuse a changed declared contract.
Public static additions are a narrow exception to exact public inventory matching;
ordinary nonstatic module, callable and type checks remain in force. They do not
authorize general additive reload. Code rollback is another checked publication,
not state rollback: it cannot remove a static introduced by a later generation.

## Async activation

The implemented [async executable contracts](../async-execution-design.md#executable-contracts)
define the checked handoff.

Cold Futures and waiting executions pin their original body, types, dependencies
and native provider contracts across compatible publication. Candidate
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
