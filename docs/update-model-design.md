# Compatible Hot Reload and State Replacement Design

Status: proposal for later implementation. This proposal defines two deliberately
different update paths: compatible code publication preserves existing contracts
and live values, while state replacement saves an explicit data root and restores
it into a fresh execution environment. The motivating application has one player
state struct modified by protocol handlers; behavior is replaceable, durable state
is the continuity boundary.

The user confirmed this split and excluded adding a new trait implementation to an
existing concrete type during compatible reload. Arbitrary in-place object, stack
or closure migration is out of scope. Exact APIs, snapshot codec and scheduling
choices remain proposals. This document authorizes no implementation or commit.
Existing specifications remain authoritative until implementation updates them.

The [package design](package-design.md) owns dependency selection and build identity;
the [host API plan](host-api-refactor.md) owns embedding entrypoints. This document
owns update compatibility and state cutover, using the interop, Native and execution
protection contracts rather than duplicating them.

## Current foundation and gaps

[Module activation](spec/module-activation.md) already defines validated staging,
per-runtime publication, no implicit execution on load/reload and retained execution
versions. [Runtime reload checks](../crates/kagari-runtime/src/reload.rs) currently
require equal module counts and equal lists of public ABI/path fingerprints. That
is stricter than additive compatibility, yet is not a complete frozen-definition
inventory of the model below.

[Trait objects](spec/traits.md) already retain verified implementation tables,
payloads and implementation versions. They are not automatically rebound by new
publication. This is a foundation for coexistence, not evidence that the proposed
new-type/old-collection update scenario has passed end-to-end acceptance.

There is no complete state snapshot/restore/cutover API established by these
foundations. Rust interop's value conversion and Serde adapter do not by themselves
serialize a script-owned player state or an entire VM. Those are separate contracts.
Re-audit completed predecessor code before implementation; no build result is
claimed by this source inventory. Existing
[prepared reload tests](../crates/kagari-embed/tests/native_artifacts.rs) and
[candidate session tests](../crates/kagari-runtime/tests/execution_sessions.rs)
are regression foundations, not substitutes for the new acceptance cases below.

## The two update modes

| Property | Compatible hot reload | State replacement |
| --- | --- | --- |
| Continuity | Existing values and executions remain valid under frozen contracts | Only explicitly exported state data and declared host resources continue |
| Existing declarations | Preserve identities and contracts | May change, provided new code and host bindings validate |
| State schema | Existing layouts remain unchanged | Same schema or an explicit successful migration |
| Executions | Old work may finish under its pinned code | Old work must drain or terminate before the state snapshot |
| Object identity | Preserve valid live objects; do not retarget behavior implicitly | New heap objects and new runtime/installation identities |
| Failure before commit | Old published code remains active | Old code/state remain available, subject to effects of an explicit drain/cancel policy |

Neither mode promises business-level equivalence or rollback of completed external
effects. Do not automatically fall back from rejected compatible reload to state
replacement: it has different latency, task, state and lifecycle consequences and
requires an explicit host choice.

## Compatible change rules

Treat compatibility as preservation of existing named declaration contracts plus
validated additions, not equality of a whole ABI list or success of version-range
resolution. The proposed conservative baseline freezes existing named definitions,
including private named types and callable contracts; function-local variables and
compiler-generated temporaries are not stable declarations to freeze.

| Change | Compatible mode |
| --- | --- |
| Existing function/method body | Allowed after full checking and affected-code rebuild |
| New free function, inherent method, module or nominal type | Allowed if it preserves old contracts and introduces no resolution conflicts |
| New concrete type implementing an existing trait | Allowed after normal coherence and interface checks |
| New trait implementation for an existing concrete type | Rejected; use state replacement |
| Deleting/renaming an existing definition or changing visibility/access contract | Rejected |
| Existing parameters, return types, generic bounds or sync/async mode | Must remain compatible under the frozen contract; first version requires equality |
| Existing struct fields/types/writeability or enum variants/payloads | Frozen, including additions to existing layouts/variant sets |
| Existing trait methods, parent requirements or associated-type contract | Frozen |
| Existing impl receiver/trait/associated outputs or dispatch relationship | Frozen; method bodies may change |
| Constant value | Proposed: allowed if dependent checking/rebuild succeeds and no frozen contract changes |

Adding a field to an old struct is not adding a new definition for these purposes.
Adding a default method to an old trait still changes that trait's contract. New
blanket/generic impls that would apply to old concrete types are not a loophole:
reject them in compatible mode. A new type's impl may be generic over its own
parameters only when it neither overlaps existing impls nor extends the old types'
implementation domain. Conservative rejection is preferable to solving arbitrary
cross-version trait remapping.

Native binding identities, passing rules and declared contracts remain checked.
This is script code update, not live replacement of arbitrary Rust machine code.
Changing a function body can change intended business effects, but cannot bypass
existing candidate restrictions or falsify trusted effect metadata.

Compare definitions by stable package/module/declaration identity, not source order,
line numbers or mutable table slots. Preserve every old contract and validate new
contracts. Exact build hashes change normally; compatibility descriptors are not
the same as exact artifact integrity/cache fingerprints. Update wire schemas and
verifiers where a complete compatibility inventory is currently unavailable.

## Update scope and publication

The host may edit any part of the loaded source dependency graph, including an
indirect dependency. The update builder computes affected installed roots. The
first implementation may rebuild each affected Program in full and reuse immutable
code where safe; it need not produce a minimal machine-code patch.

Generic monomorphizations, inlining and constant evaluation can embed dependency
behavior in callers. Signature equality alone is insufficient to reuse compiled
callers. Invalidate their actual compilation dependencies, or conservatively rebuild
them. Resolve/lock dependencies before preparing the candidate; the VM never fetches
packages or chooses new versions during publication.

Stage and validate a complete candidate Program and its linked host requirements.
Publication rechecks the expected base generation and swaps one coherent active
installation version. Validation failure leaves the active installation unchanged.
Loading and ordinary compatible publication execute no script initializer.

Multiple Script installations, players or Actors are separate publication domains.
The host coordinates rolling deployment or an explicit group barrier; a change to a
shared package is not a global mutation of all runtimes using it. Report which roots
are affected and which remain on old selections. A package version is not a runtime
generation or proof of compatible ABI.

## Calls, retained objects and new trait implementations

Fresh root calls through the proposed logical Function handle select the latest
compatible published generation. Ordinary static calls and reentry retain the
selected dependency context; they do not look up latest code at each call. Future
async continuations retain their existing ownership through every wait/resume.

Retained executable values are an explicit boundary: closures and trait objects
carry their own verified implementation generation. Dynamic invocation enters that
generation and its dependency context, then restores the caller context. It shares
the current root's cancellation/work protection rather than starting a fresh budget.
Do not retarget an old interface table or closure merely because its signature matches.

For the requested rule-collection scenario:

1. Keep the existing `DamageRule` trait contract unchanged.
2. Add `FixedDamageRule` and its `DamageRule` implementation in the candidate.
3. Publish the compatible candidate and construct a new interface object.
4. Business code explicitly inserts/replaces entries in an existing mutable
   collection of `DamageRule` after normal ownership/type checks.

The collection need not enumerate concrete implementations. Its old and new
elements may pin different implementation generations under the same compatible
trait contract. This requires focused cross-generation value-validation tests;
matching display names alone is insufficient. Existing interface objects retain
their original implementation. Merely appending FixedDamageRule does not remove
BuggyRule, and publishing code does not perform the collection mutation for the host.

Adding `impl SomeTrait for Player` when Player already existed is excluded even if
its fields would remain unchanged. There is no automatic old-object interface
upgrade, implementation replacement, runtime trait injection or monkey patching.
Use state replacement for that change. Lifetime retention and old-generation cleanup
remain bounded by explicit host policies, not by silently retargeting live objects.

## State replacement boundary

State replacement treats the designated player state as data, not as a VM image.
The proposed simplest isolation boundary is one replaceable runtime/heap and task
scope per state owner. This is a recommendation for the first implementation, not
a requirement that every Kagari host run one VM per player.

If several players share a heap, roots or mutable host state, the host must identify
and replace a consistent group or prove an isolated boundary. Do not promise that
one player's pointers can be extracted from an arbitrary shared heap independently.
All writers to the exported state, including external host mutation, must participate
in the quiescence protocol. Otherwise a script-only pause cannot yield a consistent
snapshot.

The state is an explicit rooted value passed to handlers, not hidden mutable module
globals. A new environment receives newly restored state. Only declared business
resources may be reattached; raw script Values, GC IDs, old Function/Script handles
and operation IDs do not cross into the new runtime.

The player-state schema may remain identical while code changes freely, or it may
change through a defined migration. New code must still compile, link to available
host APIs and validate required protocol handlers. State replacement is not a way
to bypass static typing, ABI verification or Native safety.

## State snapshot and schema

Provide an explicit script-state codec with a host-independent data representation.
It may use Serde internally or export a Rust DTO, but do not assume `Serialize` on
a Rust struct automatically describes a script-owned GC graph. The format and API
are review gates, not an imposed JSON/binary choice.

A snapshot envelope should identify the logical state schema, schema version,
source deployment/build for diagnostics, state owner and bounded payload. Schema
identity is stable data identity, not a runtime type slot or package code generation.
Field/variant keys must be stable rather than compiled offsets. Validate input size,
nesting, collection counts, numeric ranges and schema tags before publishing state;
an integrity digest alone is not validation or authorization.

Recommended initial data model: primitives, strings, supported collections and
ordinary structs/enums composed of these, plus stable business IDs. Preserve enum
tags and explicit Option/Result data. A tree/DTO format is sufficient if shared
references/cycles are rejected or deliberately projected to business IDs by an
explicit application mapping. Never silently duplicate identity-sensitive aliases.
General object-graph identity serialization is deferred unless required by examples.

Exclude closures, function handles, trait implementation objects, pending tasks,
iterators/guards, borrows, raw host handles and Opaque payloads. Represent behavior
as rule IDs and data; rebuild behavior objects with new code. Represent configuration
references as stable IDs and rebind through host policy. Persistent resources are
host-owned, not serialized object addresses.

For unchanged schemas, restore validates the complete declared data contract. For
changed schemas, use explicit versioned migrations or explicitly declared decode
rules such as a default for a new field. Unknown/missing fields, changed variants
or renamed types must not be silently guessed. A migration consumes old-schema
data and produces data validated against the new schema; it need not load the old
runtime ABI into the new VM. Validate business invariants and referenced IDs after
structural decoding. Decodability alone is not business correctness.

An in-memory snapshot is enough for a live transition. Crash-durable persistence,
encryption and storage transactions are host responsibilities and are not promised
by successful in-memory serialization. Future durable recovery needs an explicit
deployment/checkpoint protocol.

## Quiescence and cutover

The following is an explicit host lifecycle, not an automatic fallback inside
`reload`. The Actor remains responsive to lifecycle/completion notices and other
Actors continue running; waiting for old work must not block the Actor thread.

1. Prepare new code and validate its package graph/host requirements while the old
   environment serves normally. Record the expected active deployment identity.
2. Close admission for new business handlers and child jobs that can touch the
   state being replaced. Queue messages with a bound or apply declared backpressure.
   Continue the scheduling needed to settle already admitted work.
3. Drain existing work by default. An explicit host policy may instead cancel it;
   wait for cooperative termination and cleanup, not just a cancellation request.
   If quiescence cannot be established within policy, abort/defer replacement.
4. Once no old execution or host writer can mutate state, export a snapshot at a
   defined mailbox/state revision boundary. Retain the old environment unchanged.
5. Restore/migrate the data into an isolated candidate runtime, reattach approved
   resources, rebuild transient behavior and validate state plus handler routing.
   Candidate work must be bounded, non-suspending and unable to mutate live external
   resources; trusted Native implementations must enforce this restriction.
6. Recheck the expected old deployment, quiescence and snapshot/state revision.
   On the owner's scheduling thread, commit the new runtime, state, routing and
   task-scope identity together with no fallible script work inside that switch.
7. Resume queued business dispatch into the new environment and retire the old
   ownership domain. Release old roots/resources safely; cleanup failure after
   commit is reported as post-commit cleanup, not a rolled-back deployment.

Candidate construction may prepare handlers early, but only the final quiescent
snapshot is authoritative. Protocol messages queued across the boundary must have
defined decoding/routing under the new handler set; an incompatible protocol change
requires host coordination, not blind replay. Preserve mailbox ordering and avoid
double consumption during commit/abort.

Draining uses a host admission gate while admitted jobs can still run. Do not
implement it by calling the task scope's existing Closing transition, which cancels
jobs. Restoring admission after an aborted drain and creating a new scope after an
explicit cancellation are distinct lifecycle operations.

Before commit, failures discard the candidate and preserve the old code/state.
Admission can reopen after abort according to host policy. If old jobs were already
cancelled, rollback does not resurrect their continuations or undo their effects;
the host must reconcile/restart business work explicitly. After commit and new
effects, switching back is another state transition, not a free rollback to the old
snapshot. Ordinary code updates provide no distributed transaction guarantee.

## Asynchronous work and external effects

Compatible publication may coexist with old suspended jobs. State replacement may
not: no job capable of accessing old state can remain runnable or resumable at the
snapshot boundary. Scoped launch admission must close alongside message admission.
The [async](async-execution-design.md) and [task scope](host-task-scope-design.md)
contracts own cancellation, roots and cleanup; this protocol composes with them.

Remote RPC cancellation is best-effort and cannot undo already committed remote
changes. Delayed or duplicate completions carry the old runtime/scope/operation
generation and cannot resume or mutate the replacement state. Dropping their local
delivery does not reconcile business effects: use host request IDs, idempotency,
outbox/reconciliation or an explicit drain policy as needed. Do not claim exactly-once
RPC behavior from local code/state cutover.

Preparation and restoration must not synchronously wait for external IO on the
Actor thread. Fetch required resources before the final quiescent phase and use
bounded cooperative work where available. Large snapshot/conversion work must also
have a bounded driving strategy, or a documented host-selected pause budget; owned
data can move off-thread only under its actual Rust thread-safety contract. The
engine cannot forcibly interrupt arbitrary Rust code: a timeout can abort the
transition only when control is available. Noncooperative Native code prevents a
bounded-cutover guarantee. Opaque objects shared with the old runtime are not a
shortcut around writer quiescence or candidate side-effect isolation.

## Host API integration

Keep compatible publication and state replacement visibly distinct. Illustrative
names are `runtime.reload(&script, &program)` for compatible code and a host-managed
state-replacement operation for the lifecycle above. The latter must not look like
an interchangeable `ReloadOptions` boolean.

Stable Function handles may follow compatible versions within their installation.
After fresh-runtime state replacement, old handles are invalid for the new runtime;
the host rebuilds routing and resolves new handles before commit. State constructors
and migration hooks are explicit calls with checked contracts, never implicit module
initializers. Exact exported function signatures for codec/migration are a review
gate; use the host API's outer-tuple argument rule when calling them.

## Review choices and implementation sequence

Confirmed boundaries: two explicit modes; frozen existing contracts for compatible
updates; new concrete implementations are allowed; new trait relationships for old
types and arbitrary in-place migration are excluded; state continuity uses data.

Confirm these recommendations before implementation:

1. Freeze all existing named declarations initially, not only public ABI; do not
   freeze compiler-generated local shapes or ordinary local variables.
2. Rebuild affected Programs conservatively before optimizing incremental invalidation.
3. Use an explicit state root and tree/DTO codec initially, with versioned migration.
4. Start with a replaceable isolated state-owner runtime; shared heaps need group
   boundaries rather than implicit per-player extraction.
5. Drain old jobs by default; cancellation is an explicit deployment policy with
   business consequences and no continuation rollback.

The [roadmap](implementation-roadmap.md) must coordinate lower-level update work
with HA's facade and PK's identities, without requiring each completed plan before
the other can start. Freeze PK identity and HA handle contracts first; reuse RI
conversion/root infrastructure where applicable. Async work is not needed for
synchronous state replacement acceptance, but its activation must test the extended
quiescence/completion matrix. Existing ST/NR/EP scope is unchanged.

- [ ] UP00: Re-audit compatibility descriptors, cross-generation values, interface
  dispatch and state ownership. Confirm review choices and candidate effect policy.
- [ ] UP01: Implement identity-based frozen-contract/addition validation, including
  impl-domain restrictions, dependency invalidation and verifier/artifact changes.
- [ ] UP02: Integrate compatible publication with HA logical handles; test new
  concrete implementations in retained trait collections and old-version dispatch.
- [ ] UP03: Implement bounded script-state export/restore, schema checks and explicit
  migration. Reject runtime resources and unsupported alias/cycle forms precisely.
- [ ] UP04: Implement host cutover integration, candidate restrictions, admission
  gates and failure handling. Include synchronous examples; add task tests when async exists.
- [ ] UP05: Update specifications and examples, run final verification and record
  carried limitations. No placeholder async implementation is required to pass.

## Acceptance and verification

Test positive body/addition updates and rejection of removed definitions, changed
fields/variants/signatures and added impls for old types, including blanket impls.
Test order-independent compatibility, generic/inline/constant invalidation, stale-base
rejection and new modules without weakening artifact validation.

Exercise a rooted mutable trait collection containing old implementations, insert
a new-type implementation after publication, remove the buggy element explicitly,
and verify receiver-specific dispatch, shared root protection and eventual version
reclamation. Wrong trait identities, foreign runtimes and forged tables must fail.

For state replacement, test unchanged/changed schemas, missing migration, unknown
variants, numeric limits, invalid business references, cyclic/aliased state and
forbidden Opaque/callable/task fields. Inject failures at every pre-commit step:
old code/state must remain usable, with cancelled-work caveats reported honestly.
Test commit/abort mailbox ordering, handler incompatibility, shared-writer refusal,
post-commit cleanup errors and bounded peak memory while both environments coexist.

When async is available, include drain timeout, cancellation during RPC, nested
launch rejection, completion races, duplicate/late notices and abort after job
cancellation. No old continuation may mutate restored state and no test may claim
remote-effect rollback. Test the same contracts through source/artifact and supported
backend routes. Final checks include:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Record actual commands, errors and owning phases here. Default checkpoints must
build; any exception needs activation-time approval. Measure preparation, quiescence,
snapshot, restoration and commit separately, recording machine/toolchain/profile,
features, default Cargo parallelism, cache state and workload. No latency target
or success result is invented by this proposal.

## Progress ledger

- 2026-09-30: Recorded the compatible-update/state-replacement split. Excluded new
  trait impls on old types and arbitrary in-place migration; retained the new-type
  trait-collection use case. UP00-UP05 remain unstarted. No implementation or commit.
- Documentation review: aligned HA/RI/async/task-scope proposals with the two update
  modes, receiver-pinned dynamic dispatch and drain-versus-cancel distinction.
  Checked 173 local links, 13 heading anchors, whitespace and code-fence balance
  across ten related documents; `git diff --check` passed. No Rust build, runtime
  tests or benchmarks were run for this documentation-only work.
