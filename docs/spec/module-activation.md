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
supports cancellation. This graph is a compilation prerequisite; runtime bundle
initialization and dependency-version retention are still pending R10/R14 work.

Each runtime owns an independent instance for each executable generation.
Initialization runs at most once per instance, following:

`Uninitialized -> Initializing -> Initialized | Failed`

Repeated access to Initialized returns the cached instance. Repeated access to
Failed returns the recorded failure without rerunning initialization. A host may
explicitly start a new attempt with a fresh candidate instance. Ordinary import
does not silently retry prior effects. Public execution requires initialization
of the entry and its dependencies to have succeeded.

Top-level val/var remain private initialization bindings, not durable globals.
Existing scalar const-safe restrictions remain in force. Persistent state and
state migration are explicit host concerns.

## Reload

Reload consists of distinct operations:

1. Prepare: compile, verify, link, check ABI/schema and retain the expected base.
2. Initialize: construct isolated candidate instances and initialize dependencies.
3. Publish: recheck the expected base and atomically replace the entry generation
   for the selected runtime/publication unit.

Service candidate initialization permits pure computation, candidate-owned
allocation and mutation, and explicit immutable configuration. It rejects real
host-state modification, outgoing events, timers, and calls with unknown effects.
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
