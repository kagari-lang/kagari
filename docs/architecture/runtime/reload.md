# One Hot Reload through the Runtime

Start with the [Runtime model](../runtime.md). This scenario replaces compatible
version A with B in one installation. The [activation specification](../../spec/module-activation.md)
owns declaration/static compatibility and call-selection semantics; the
[embedding specification](../../spec/embedding-api.md#source-updates) owns host APIs.

## Prepare once, publish independently

```mermaid
sequenceDiagram
    participant Host
    participant Product as Immutable Program A
    participant Prep as Source / artifact / native preparation
    participant Runtime as Exclusive Runtime owner
    participant Store as Local records and current map
    Host->>Prep: Compilation.prepare_update(edits), or Program.prepare_replacement(B)
    Prep->>Product: Read checked code/history and exact base identity
    Prep-->>Host: Shared immutable Program B, weak exact base A
    Host->>Runtime: replace_program(installed, B, options), mutable borrow
    Runtime->>Runtime: Admit idle synchronous publication boundary
    Runtime->>Store: Validate exact current base and local provider authority
    Runtime->>Store: Stage records, state bindings and candidate call entries
    Runtime->>Store: Prepare final selections and rebound retained function tables
    Runtime->>Store: Recheck baseline, leases, bindings and cancellation
    Runtime->>Store: Commit current, routes and execution links together
    Runtime->>Runtime: Release guards, then drop displaced owners
    Runtime-->>Host: Success; same installation handles
```

Preparation parses/checks/lowers source independently of Runtime. The resulting
Runtime Program shares bytecode and checked source/debug metadata; SDK Compilation
separately retains source inputs and optional verified MIR. Program's
bounded cumulative demands and semantic declaration/binder history contain metadata,
not strong predecessor code owners. The expected base is exact opaque product
identity, not a label, hash or source position. Preparing against a different base
requires an explicit new candidate; stale local recipients reject.

Each recipient checks its own installed providers, identities, local state and
baseline. A failed/cancelled recipient cannot change the shared product or another
Runtime. Sharing does not transfer Rust callback captures, heap objects, statics,
scheduler state or execution authority. Portable inputs still require independent
verification at their process boundary.

The Runtime stages installation-local records without remapping the entire code,
export or source image. Fallible correspondence, layout/type admission and final
call-table allocation precede visibility. Low-level staged initialization remains
an explicit separate operation; ordinary SDK replacement executes no initializer
or callback. Staged handles cannot escape candidate execution authority.

Sources: [detached edits](../../../crates/kagari/src/program/update.rs),
[product preparation](../../../crates/kagari-runtime/src/program.rs),
[Runtime replacement](../../../crates/kagari-runtime/src/loading.rs),
[staging and commit](../../../crates/kagari-runtime/src/module/staging.rs),
[loading and validation](../../../crates/kagari-runtime/src/loading.rs).

## Publication boundary and atomicity

| Runtime condition | Publication |
| --- | --- |
| Host top level, no synchronous activation or conflicting Native loan | Admissible after exact target/baseline checks |
| Script, callback, observer, synchronous reentry or cleanup active | Rejected, including when an active-session pointer is temporarily hidden |
| Parked frames, cold Futures or waiting Tasks, no active drive scope | Permitted; no conservative draining requirement |
| Publication guard already held | Another publication, execution/resume or driver consumption rejects |
| Closed/quarantined Runtime, foreign/stale/retired installation | Rejected through the existing error boundary |

`begin_publication` requires `&mut Runtime`. Its guard retains that borrow and
provides a read-only view for checked metadata work. Runtime-local current access
uses short `RefCell` borrows, with no Installation current-cell Mutex or shared
handle-only current query. Lower-level execution views still enforce dynamic
admission, reentry and loan rules. Completion notifications can mark owned work
ready while parked; producer threads do not execute code or publish versions.

The final baseline check is followed only by non-failing commit operations: mark
the candidate published, remove its staged lease, replace current and prepared
routes, and swap every affected record's execution links. Guards end before
old descriptors/tables are dropped. No user callback runs under these guards.
Failure before commit preserves current and authoritative static state; abandoned
staging is retired through its existing lease/collection path.

## Exact old bodies, current ordinary calls

```text
Before: current A; a Future has selected resume_body@A and is parked.
Commit: current B; prepare A's outgoing ordinary calls directly against B.
After:  new helper() calls enter helper@B.
        the Future resumes resume_body@A at its saved PC/environment.
        helper() inside that old body enters helper@B.
```

Created closures, cold Futures, admitted frames and queued invocations keep their
selected bodies/providers and type environments. New ordinary named calls use
current implementations. Publication does not overwrite old instruction bytes or
change a suspended Future's remaining body. Saved named host bindings follow their
canonical declaration through current prepared routes rather than pinning old code.

Both candidate and retained records receive final generational target entries.
The candidate's ordinary Direct path starts with normal links. Retained A call
sites point directly to B; another publication points A and B directly to C, without
an A-to-B-to-C forwarding chain. Publication resolves declaration/application/binder
correspondence once. Shared generic calls transport checked prepared metadata;
actual supplied values, witnesses, receiver dispatch and loans still require their
normal checks. There is no runtime type inference or per-call correspondence search.

A parked execution releases function-link views for its entire stack before ending
the activation. Resume refreshes every frame's views, including parents waiting on
a child; exact body, PC, capture, lexical environment, values and return destination
are unchanged. Synchronous active frames cannot overlap publication. This closes
the stale-parent-link case without draining all outstanding Futures. Only frames
whose body needs runtime links allocate/stage refreshed links; all fallible staging
precedes commit. A retained old self-tail candidate also uses these refreshed
selections: a different current target takes ordinary Call/Return fallback.

Sources: [prepared route registry](../../../crates/kagari-runtime/src/module/dispatch/bindings.rs),
[function rebinding](../../../crates/kagari-runtime/src/module/linked_execution.rs),
[call links](../../../crates/kagari-runtime/src/module/linked_execution/calls.rs),
[metadata transport](../../../crates/kagari-runtime/src/module/dispatch/identity.rs),
[park/resume](../../../crates/kagari-runtime/src/frame/owned.rs),
[ordinary call path](calls.md#selection-and-frame-entry).

## Retention, native eligibility and failure

| Event | Result |
| --- | --- |
| Compatibility/linking/admission/cancellation failure | No publication or authoritative static-state change |
| Candidate expects A, recipient already runs B | Stale-base rejection; no implicit merge or skipped-base preparation |
| Old closure/Future/frame/queue or explicit root survives | Retain its required whole Program and matching metadata |
| Last executable dependency disappears | The one managed graph can retire old revision/static records and code owners; route registry holds weak identities |
| Saved named host binding survives old code collection | Its metadata identity still follows the current implementation |
| Runtime closes/drops | Revoke current authority and drain owned work; identity handles cannot revive it |

Repeated updates can retain many whole revisions if actual roots span those
revisions. Local records/bindings and publication preparation grow with retained
work; shared products do not impose a fixed version-count or memory bound. Hosts
own indefinitely retained computations. Bounded metadata history avoids executable
ancestry, but does not turn whole-version retention into per-body reclamation.

Actual RevisionState/static/environment/application edges participate in kagari-gc.
Current/staged publication and retained computation roots protect their origins;
lookup keys and code Arcs alone do not root mutable state. A callback/object/static
cycle is reclaimed after its genuine roots disappear.

The descriptor attaches the Runtime Program owner without a cycle: ProgramState's own
bytecode has no back-reference. Revision tokens, source observations and historical
demand/correspondence records carry no executable payload. Whole-version retention
is deliberate; [R13 type/layout provenance](../../review.md#r13--type-and-layout-provenance-retains-retired-code)
can independently retain an initial Program through inherited statics/types. No
Future-only or immediate per-body reclamation promise follows from publication.

`NativeCompiler` owns mutable compilation decisions outside Runtime. Immutable
NativeCode/pages can be distributed after compiler/backend drop. Each recipient
checks exact revision/root/configuration/ABI and local eligibility before installation;
wrong-version delivery leaves the prior entry and publication unchanged. Replacing
current changes eligibility separately from page lifetime: another recipient or
selected computation may still own pages. Unsupported calls/suspension in the
current backend decline before native entry. Native continuations, inlining,
deoptimization and automatic JIT delivery are separate work, not a hidden guarantee
of this reload model.

Sources: [native preparation/install](../../../crates/kagari/src/program/native.rs),
[managed collection](../../../crates/kagari-runtime/src/lib.rs),
[retention leases](../../../crates/kagari-runtime/src/module/retention.rs).

## Reference mechanisms

Consulted on 2026-11-09: the Wasmtime API pages identify themselves as 52.0.0-dev,
not a pinned Kagari dependency or source audit. [Module](https://docs.wasmtime.dev/api/wasmtime/struct.Module.html)
separates an immutable Send + Sync compiled representation from stateful instances.
[InstancePre](https://docs.wasmtime.dev/api/wasmtime/struct.InstancePre.html) permits
pre-instantiation import work but reuse across Stores depends on captured imports.
Kagari adopts shared code versus local state and that authority limitation. It
retains its own tracing heap, scoped nominal types and no-script-effects loading;
Wasmtime instantiation may execute a start function.

[JVMTI 25 RedefineClasses](https://docs.oracle.com/en/java/javase/25/docs/specs/jvmti.html#RedefineClasses)
preserves active old bodies while subsequent calls use replacements and static
values survive. [OpenJDK 25+36](https://github.com/openjdk/jdk/blob/jdk-25%2B36/src/hotspot/share/prims/jvmtiRedefineClasses.cpp#L3748)
tracks dependent compiled code and deoptimizes at a safepoint. Kagari adopts separate
body selection, executable lifetime and native eligibility, but publishes between
exclusive activations and additionally retains cold closures/Futures and parked
stacks. Its checked generic/witness transport and final per-installation bindings
are custom adaptations; no HotSpot concurrency/deoptimization implementation or
whole-old-call-graph freeze is assumed.
