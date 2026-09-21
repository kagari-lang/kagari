# Module Initialization and Activation

This is the authoritative v1 lifecycle contract. See modules.md for declaration
and top-level syntax. Shared executable code and mutable module instances have
different ownership.

## Initialization

The module import graph must be acyclic. Report a cycle before execution; never
expose a partially initialized instance. Dependencies initialize before their
importer in deterministic module order. Functions within a module may recurse.

The implemented analysis graph provides this order for registered sources: among
modules whose dependencies are ready, select the smallest package/path identity.
Each reachable dependency occurs once. Cycle analysis uses explicit stacks and
supports cancellation. The linked execution program preserves this dependency
order and pins member versions for cross-module calls. Ordinary runtime
initialization follows the program graph. VM reload initializes a staged candidate
in a restricted session before publication; failures discard the candidate.

Each runtime owns an independent instance for each executable generation.
Initialization runs at most once per instance, following:

`Uninitialized -> Initializing -> Initialized | Failed`

Repeated access to Initialized returns the cached instance. Repeated access to
Failed returns the recorded failure without rerunning initialization. A host may
explicitly start a new attempt with a fresh candidate instance. Candidate
handles also cache cancellation and resource termination across session teardown;
these candidates cannot be restarted or published, including when every module
has no initializer code. Ordinary import
does not silently retry prior effects. Public execution requires initialization
of the entry and its dependencies to have succeeded.

Runtime initialization owns a `ModuleInitializationGuard`, which retains the
execution generation without holding a mutable module-store borrow across script
execution. Finishing validates and caches the result as a persistent GC root.
Dropping an unfinished guard records failure and releases version retention.
This cleanup remains permitted after quarantine: an initializer's EngineFault
must return without a second panic from an ordinary state-access permission check.
Failure cleanup cannot downgrade an already initialized instance. It neither
reopens execution nor retries initialization. Initialization shares the root-call
session's budget and cancellation with the entry; its lifecycle guard establishes
the failure cleanup boundary. Synchronous host reentry only calls already
initialized members of the pinned program. Reentry into initializing, failed or
uninitialized members is rejected without retrying initialization. Initializers,
entries and synchronous nested calls use scopes of the session-owned frame stack;
failure unwinds the failing scope before initialization failure cleanup runs.

Top-level val/var remain private initialization bindings, not durable globals.
Existing scalar const-safe restrictions remain in force. Persistent state and
state migration are explicit host concerns.

## Reload

Reload consists of distinct operations:

1. Prepare: compile, verify, link, check ABI/schema and retain the expected base.
2. Initialize: construct isolated candidate instances and initialize dependencies.
3. Publish: recheck the expected base, host bindings, initialization state and
   candidate ownership of reachable module values, then atomically replace the
   entry generation for the selected runtime/publication unit.

Service candidate initialization permits pure computation, candidate-owned
allocation and mutation, and explicit immutable configuration. It rejects real
host-state modification, outgoing events, timers, and calls with unknown effects.
The host effect `may_read_immutable_configuration` declares reads from a host-provided
immutable snapshot. Configuration functions accept and return only scalars, String,
Tuple, Option and Result composed of those values, with owned parameters. Shared
containers and opaque handles are rejected at declaration validation. The host owns
the snapshot and must keep it immutable; this effect does not authorize ordinary
service access, mutation, suspension or borrowed host parameters. Those additional
effects still cause candidate execution to reject the call.

Candidate host-call arguments/results and script-call arguments cannot introduce
mutable objects from another generation's allocation scope. Validation traverses
reachable values, including enum payloads and fresh wrappers around old objects.
Pure host allocators may return candidate-owned objects.
The heap mutation boundary also enforces candidate ownership, including direct
container and struct slot edits made by host callbacks through runtime APIs.
Script-visible container and struct reads obey the same ownership restriction.
Collector tracing remains independent of that restriction and retains rooted old
objects while a candidate runs.
Module instance snapshots and mutable borrows are likewise restricted to the
candidate's pinned program. Existing initialization guards may still record failure
when unwound; that internal cleanup does not grant ordinary access to old state. This runtime boundary
supplements the trusted host's obligation to honor its declared effects.

Candidate failure leaves the active entry and external business state unchanged.
Ordinary CLI execution can grant explicit capabilities for effectful top-level
code; those capabilities are not inherited by reload preparation.

New root calls enter the published generation. Existing calls and nested calls
retain their original dependency set. Reachable old values, implementation tables,
frames, and code remain valid until their owners release them. Publication does
not immediately invalidate old instances. No automatic persistent-state migration
or server-wide multi-actor atomicity is promised. The host coordinates rollout
and explicit effectful lifecycle operations.

## Acceptance examples

- Diamond imports initialize their shared dependency once per runtime/generation.
- Cycles fail before any initialization effect.
- Failed initialization is cached; ordinary import never retries it.
- A candidate that attempts external mutation cannot publish or modify the host.
- A prepared candidate based on a superseded generation cannot publish.
- An active old call and its cross-module callees finish on the old dependency set.
