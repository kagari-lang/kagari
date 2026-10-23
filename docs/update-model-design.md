# Compatible Hot Reload and State Replacement Design

Status: proposal for later implementation. This proposal defines two deliberately
different update paths: compatible code publication preserves existing contracts
and live values, while state replacement saves an explicit data root and restores
it into a fresh execution environment. The motivating application has one player
state struct modified by protocol handlers; behavior is replaceable, durable state
is the continuity boundary.

The user confirmed this split and excluded adding a new trait implementation to an
existing concrete type during compatible reload. Arbitrary in-place object, stack
or closure migration is out of scope. Exact APIs and snapshot codec remain proposals;
the compatible-update scheduling boundary below is confirmed. This document
authorizes no implementation or commit.
Existing specifications remain authoritative until implementation updates them.

The user also confirmed that ordinary Program and Module handles represent stable
identities within one installation and always query its latest published contents.
Existing function handles invoke current implementations after publication. No
Program/Module reacquisition or function rebinding is required. Internal immutable
versions and explicit source/debug snapshots remain separate from these ordinary
handles; this target is not implemented by the current pinned runtime.

On 2026-10-23 the user confirmed the update submission shape: construct an `Update`
containing function/file changes and submit it once to Runtime. There is no required
`begin_update`, caller-supplied version number or old-body hash. One Update is one
atomic publication batch for one installation. Exact method signatures and compiler
service wiring remain proposals; this records the direction, not implemented support.
The user also confirmed that compatible updates replace function/method bodies or
add declarations, never change existing signatures or type definitions. Adding a
trait implementation to an existing type remains excluded even when the trait is
new in that batch. Exact-version function-source inspection is a required follow-up,
not a claim that runtime source retrieval is already implemented.

The confirmed patch delivery direction is source batches compiled in the hosting
process, using the interpreter and optional runtime JIT. AOT patch construction,
distribution and installation are outside this update scope. Existing standalone
AOT deployment remains supported independently; wider JIT coverage is not a
prerequisite for source-update publication.

The user subsequently replaced the whole-dependency pinning direction: compatible
publication occurs only at the host's top-level scheduling boundary, with no active
script synchronous stack. A single-threaded Actor may receive an external update
request and process it before further script dispatch or async resumption. Existing
suspended computations retain their selected body/state, but subsequent ordinary
calls select the current implementation. This replaces, rather than supplements,
the earlier pinned-callable policy; it is not implemented yet. Existing specifications
and historical acceptance describe the old behavior until the migration lands.

The [package specification](spec/packages.md) owns dependency selection and build identity;
the [host API plan](host-api-refactor.md) owns embedding entrypoints. This document
owns update compatibility and state cutover, using the interop, Native and execution
protection contracts rather than duplicating them.

## Current foundation and gaps

[Module activation](spec/module-activation.md) already defines validated staging,
per-runtime publication, no implicit execution on load/reload and retained execution
versions. Current callable/session ownership pins dependency generations; the
confirmed current-call selection below requires a coherent migration, not merely
removing generation checks. [Runtime reload checks](../crates/kagari-runtime/src/reload.rs) preserve
module counts and nonstatic public contracts, with a narrow implemented exception
for adding statics in existing modules. Existing statics, including private ones,
retain their declaration/type/access and relevant applied-schema contracts. This
is not the general additive compatibility or complete frozen-definition inventory
proposed below.

[Trait objects](spec/traits.md) already retain verified implementation tables,
payloads and implementation versions. They are not automatically rebound by new
publication. This is a foundation for coexistence, not evidence that the proposed
new-type/old-collection update scenario has passed end-to-end acceptance.

[Persistent static cells](spec/modules.md#persistent-module-statics) already share
state across compatible generations of one installation, with fixed initializer
recipes and no implicit reset. They do not establish a state snapshot/restore/cutover
API or persistence across process restarts. Rust interop's value conversion and
Serde adapter do not by themselves serialize a script-owned player state or an
entire VM. Those are separate contracts.
Re-audit completed predecessor code before implementation; no build result is
claimed by this source inventory. Existing
[prepared reload tests](../crates/kagari/src/program/tests/native_artifacts.rs) and
[candidate session tests](../crates/kagari-runtime/src/session/execution_sessions.rs)
are regression foundations, not substitutes for the new acceptance cases below.

## The two update modes

| Property | Compatible hot reload | State replacement |
| --- | --- | --- |
| Continuity | Existing values and executions remain valid under frozen contracts | Only explicitly exported state data and declared host resources continue |
| Existing declarations | Preserve identities and contracts | May change, provided new code and host bindings validate |
| State schema | Existing layouts remain unchanged | Same schema or an explicit successful migration |
| Executions | Retain selected computation bodies/state; subsequent ordinary calls select current implementations | Old work must drain or terminate before the state snapshot |
| Object identity | Preserve values, layouts and implementation relationships; named method bodies may update | New heap objects and new runtime/installation identities |
| Failure before commit | Old published code remains active | Old code/state remain available, subject to effects of an explicit drain/cancel policy |

Neither mode promises business-level equivalence or rollback of completed external
effects. Do not automatically fall back from rejected compatible reload to state
replacement: it has different latency, task, state and lifecycle consequences and
requires an explicit host choice.

## Compatible change rules

The following is the confirmed target policy, not a description of all currently
accepted additions. Implemented static-only additions remain governed by
[module activation](spec/module-activation.md#static-publication-compatibility);
nonstatic additions below are not enabled by that exception.

Treat compatibility as preservation of existing named declaration contracts plus
validated additions, not equality of a whole ABI list or success of version-range
resolution. The confirmed policy freezes existing named declaration contracts,
including private named types and callable contracts. This is a fixed boundary,
not a first-version restriction awaiting automatic signature/layout adaptation.
Function-local variables, control flow and compiler-generated temporaries may change
with the body and are not stable declarations to freeze.

| Change | Compatible mode |
| --- | --- |
| Existing function/method body | Allowed after full checking and affected-code rebuild |
| New free function, inherent method, module, struct, enum or trait | Allowed if it preserves old contracts and introduces no resolution conflicts |
| New concrete type implementing an existing or same-batch new trait | Allowed after normal coherence and interface checks |
| New trait implementation for an existing concrete type, including a same-batch new trait | Rejected; no cross-version implementation extension in compatible mode |
| Deleting/renaming an existing definition or changing visibility/access contract | Rejected |
| Existing parameter/receiver contract, return type, generic parameters/bounds or sync/async mode | Frozen; no signature adaptation |
| Existing struct fields/types/writeability or enum variants/payloads | Frozen, including additions to existing layouts/variant sets |
| Existing trait methods, parent requirements or associated-type contract | Frozen |
| Existing impl receiver/trait/associated outputs or dispatch relationship | Frozen; method bodies may change |
| Existing named constant value/type | Frozen under the body-replacement/addition scope; body-local expressions may change |

"Existing" means published before the selected batch, not merely present in the
initial deployment. A type/function/trait added by batch 1 becomes an existing
declaration for batch 2. Its function/method bodies can be replaced again, but its
signature, type definition and established trait relationships stay frozen. A new
trait's definition is frozen after publication too; no separate permissive rules
apply to code originally delivered as a patch.

For example, batch 1 may add `PlayerExtension` and implement an existing trait or a
trait also added in batch 1. Batch 2 may replace those method bodies, but may not
add `Inspect for PlayerExtension`, even if Inspect is new in batch 2. The existing
version-pinned interface tables are useful foundations, not proof that introducing
new implementation relationships for old values is a trivial validation change.
That capability would require additional cross-version conversion, generic/witness,
lookup and retention contracts. It is deliberately deferred to keep reload simple,
not classified as equivalent to object-layout migration or technically impossible.
Prefer an ordinary new function over the old type, or a new adapter type retaining
the original object and implementing the new trait. Do not automatically wrap,
copy state or change implementation relationships on the caller's behalf. Replacing
an already established method body is distinct and follows current-call selection.

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
indirect dependency. Preparation determines affected code within the selected
installation; edits are not global mutations of every installation using a package.
The first implementation may rebuild the affected Program in full and reuse
immutable code where safe; it need not produce a minimal machine-code patch.

### Change lists and one-shot submission

Separate four granularities:

| Boundary | Proposed contract |
| --- | --- |
| Submitted changes | One function/method, one file or several related replacements |
| Recompilation | Changed code and actual compilation dependents; conservative rebuilding is allowed |
| Publication | All changes in one Update form one coherent Program candidate and commit together |
| Execution | Each admitted computation retains its selected body/state; subsequent ordinary calls select the current implementation, including from old resumed code |

`Update` is a detached change list. Constructing or adding entries does not read an
active generation, compile code, execute script or publish anything. The caller
chooses which edits belong together; compilation cannot infer business-level atomic
relationships merely from the dependency graph. A one-function fix is a batch with
one entry, not a separate in-place function-patching mechanism.

Illustrative API only; these methods do not exist yet:

```rust,ignore
let mut update = Update::new();
update.replace_function("combat::damage", damage_source);
update.replace_function("inventory::consume", consume_source);
update.replace_file("protocol/claim.kgr", claim_source);

runtime.update(&program, update)?;
```

The Program/Module names follow the [Host API plan](host-api-refactor.md). Existing
Program/Module handles query the newly published contents, including added public
paths, and existing callable bindings select current implementations. No replacement
handle is required. The confirmed update/reload success result is `()` via the
appropriate existing `Result<(), E>` error family, with no mandatory update report.
There is no required public begin/prepare/commit ceremony, version number or
`expected old body hash`. Internal staging remains mandatory; an advanced prepared-product workflow
may reuse it without becoming prerequisite boilerplate for ordinary callers.

The first batch contract rejects duplicate targets and overlapping whole-file and
contained-function replacements rather than applying an implicit last-write-wins
order. Resolve targets by checked module/declaration identity, not source line
numbers. Function replacements must identify one existing declaration; an unknown
or ambiguous target is an error, not an implicit addition. Methods require their
owning type/impl identity where a simple path is ambiguous. Replacement source is
checked in the original module's imports, visibility and generic context, against
the candidate formed by the complete batch. Do not require intermediate edits to
be independently valid or splice source with unvalidated string matching.

Replacement must preserve the applicable callable contract, including parameters,
return type, generic bounds and sync/async mode. Imports or helper declarations may
be changed through an appropriate file replacement in the same batch. New
functions/types/modules remain explicit additions subject to the compatible-change
policy; this input convenience does not enable additions rejected by current reload.

### Host scheduling boundary and serialized publication

On submission the facade validates the target installation and obtains its current
source/preparation context. The stable Program handle remains usable after earlier
compatible publications; it is not rejected merely because it was obtained before
them. Foreign or retired-installation handles are rejected and never retargeted to
a same-named installation. No caller computes or copies a generation value.

The implementation captures its preparation basis internally, constructs an isolated
candidate, checks/compiles affected code and links dependencies, then publishes the
whole batch through the existing validated transition. A failed batch leaves active
code, static cells and the authoritative source context for subsequent updates
unchanged. Any retained failed candidate is diagnostic data, not the next update's
baseline. Recheck compatibility and publication authority before the transition;
no candidate script or host initializer runs as a side effect of ordinary updating.

The supported flow is external request -> host/Actor message -> top-level update
processing -> subsequent script dispatch. One Runtime has one exclusive driver.
Preparation/publication is serialized with other updates; publication cannot run
while a synchronous script stack is active, including inside a script-invoked host
callback. Reject reentrant publication rather than supporting stack migration or
nested update transactions. No new Actor framework or runtime-owned scheduler is
required: the host owns message delivery and scheduling.

Suspended async tasks may remain. Publication does not poll/resume them or invoke
script/host initializers. External operations may complete, but their notices are
consumed only after publication returns to the host driver. Script execution cannot
observe intermediate installation steps. Perform fallible checks/allocation/linking
before commit, or provide an equally explicit rollback discipline; sequential
execution alone does not prevent half-installed state after an error.

The initial design needs neither concurrent-reader synchronization nor lock-free
call-target swaps, concurrent update merging or a background compilation protocol.
The internal preparation basis still prevents applying a prepared candidate to the
wrong installation/version. Do not silently replay or merge requests. Off-thread or
interleaved preparation would require a separately reviewed stale-base check before
commit; it is not part of this delivery and exposes no user-managed hash/version.

### Source availability and incremental work

Function-source submission requires a matching compilation context. A source-capable
SDK facade orchestrates the in-process compiler and matching source context; the
low-level runtime must remain independent of HIR and source analysis. Source batches
compile to checked executable products before runtime publication; JIT consumes
verified MIR, not source text. Existing prepared-candidate loading remains a separate
entrypoint, not a requirement to build a second patch delivery system. Exact facade
ownership and source-context commit coordination must be resolved before implementation;
do not implicitly attach an arbitrary editor
overlay or require Runtime to discover files/network dependencies during publication.
An artifact-only deployment without sufficient source/compiler metadata must reject
source updates clearly, not pretend public signatures contain private/generic bodies.

Submitting a small patch does not guarantee equally small compilation or installation
cost. Measure input transfer, analysis/lowering, verification, linking/preparation and
commit separately. Current reload accepts complete candidates; a function-source
patch frontend and reliable fine-grained compilation/installation reuse are not yet
implemented. Conservative complete candidate preparation is an acceptable first
correct implementation, not a claim of constant-time function updates.

A logical full Program version may share unchanged immutable code. Call sites must
retain checked declaration/application identities, not permanently bind ordinary
calls to old implementations. If old suspended `process` resumes after `damage` was
replaced, its next ordinary call selects current `damage`. Reuse still requires
correct installation ownership, callable contracts and executable evidence; an old
prelinked target cannot bypass current selection merely because the signature matches.

Generic monomorphizations and other compilation dependents still require checked
candidate preparation; every reachable supported application, including calls from
retained old bodies, needs a valid current target. Runtime does not infer types or
specialize arbitrary source. Preserve frozen named constants and compilation inputs.
Cross-function inlining or folding a replaceable call into a retained old body would
hide a required future current-version call; rebuilding only new callers cannot fix
that. Do not perform those transformations across replaceable call boundaries in the
initial model. Resolve/lock dependencies before preparing the candidate; the VM never
fetches packages or chooses package versions at execution time.

Stage and validate a complete candidate Program and its linked host requirements.
Publication rechecks the expected base generation and swaps one coherent active
installation version. Validation failure leaves the active installation unchanged.
Loading and ordinary compatible publication execute no script initializer.

Multiple Program installations, players or Actors are separate publication domains.
The host coordinates rolling deployment or an explicit group barrier; a change to a
shared package is not a global mutation of all runtimes using it. Report which roots
are affected and which remain on old selections. A package version is not a runtime
generation or proof of compatible ABI. A batch spanning modules within one Program
is atomic; sequential reloads of different installations are not a group transaction.
Cross-installation or cross-Runtime atomic publication needs a separately designed
host coordination/group protocol, not an implicit extension of Update.

### Interpreter/JIT execution, not AOT patch delivery

Source updates reuse the [interpreter/JIT execution contracts](spec/jit.md).
The source-capable host carries the compiler and compilation context; eligible
functions can be prepared for JIT execution under the configured policy, while
unsupported valid functions use the interpreter. Current JIT coverage is narrow.
This decision neither promises native execution for every patch nor activates
automatic first-call/hotness compilation, new optimization tiers or the separately
queued [native expansion](cranelift-expansion-plan.md).

Native products must still match their exact compiled body, metadata and runtime
ABI. Never overwrite code retained by a suspended computation. Ordinary native calls
must use the same current implementation selection as interpreted calls; an old
`attack` body must not permanently direct later calls to old `damage`. A stable
runtime-owned call entry is the initial design direction, not unchecked machine-code
patching. Retain the selected executable owner for each admitted computation.

Do not inline replaceable callees or retain raw direct calls across publication
without a proven equivalent selection mechanism. Version-specific products and
conservative preparation are acceptable; cross-version machine-code reuse and
invalidation/deoptimization machinery are not prerequisites. Native preparation timing
remains an implementation gate, not a promise of eager compilation or automatic lazy
JIT. Existing exact-version preparation tests remain evidence of product safety, not
proof that current-call dispatch is implemented.

Use the existing pre-entry fallback rules: unsupported valid code or explicit policy
may select interpretation. Compilation/link errors, invalid products and failures
after native entry are errors, not permission to silently retry script effects.
Exact-version source inspection applies equally to interpreted and JIT targets.

[AOT deployment](spec/aot.md) remains an independent supported route. This plan does
not add native patch bundles, per-platform patch builds, DLL distribution/loading,
AOT-to-JIT patch integration or an AOT-required update mode. Such integration needs
separate authorization; it must not expand HM or block source-batch implementation.

### Code atomicity versus business-state cutover

Atomic code publication does not stop old suspended tasks from later accessing shared static
cells or other live state. If an update changes the business meaning of unchanged
storage (for example, an integer amount changing from whole units to fractional
units), code compatibility alone cannot make old and new operations safe together.
The host must explicitly gate affected entrypoints, drain or cancel relevant old
work, perform any required state migration and switch routing before resuming.
These steps are not mandatory for ordinary compatible body fixes and do not promise
rollback of completed script or external effects.

Ordinary function bindings keep stable installation/declaration identity and select
the current implementation on each new call, without host rebinding after a body
update. Publication does not add or remove entries in arbitrary host routing tables,
rewrite closure captures or migrate task state. State replacement remains a distinct
lifecycle, not a hidden update option.

An operation may call old `B`, suspend, then call new `B` after publication. This is
intentional. Neither strict dependency pinning nor current-call selection guarantees
business consistency for shared state or external effects. The host decides whether
a particular patch needs admission gating/draining; ordinary compatible patches do
not require all suspended tasks to finish. Runtime still owns type/memory safety,
atomic publication and failure isolation, not business-state transactions.

As a reference, [Erlang/OTP code replacement](https://www.erlang.org/doc/system/code_loading.html#code-replacement)
(living system documentation reviewed 2026-10-23) uses module-level replacement and
allows fully qualified calls to select current code while local calls can stay in
old code. Reuse the distinction between retained executing bodies and current call
targets. Kagari deliberately applies current selection to ordinary local, qualified,
method and named-function-value calls alike; it does not copy Erlang's local-call
exception, two-version limit, process purge or load hooks. Kagari's statically checked
contracts and single-driver top-level publication remain its own constraints. This
reference motivates a mechanism, not an unmeasured performance claim.

## Calls, retained objects and new trait implementations

Separate stable callable identity from a concrete executable selection. Ordinary
script calls and host function handles identify an installation, checked declaration
and applicable type arguments. Each new call resolves the current implementation
through runtime-owned checked metadata, not repeated source/string lookup. The
selected body and its required resources remain valid through that computation;
subsequent ordinary calls resolve current implementations again. Synchronous reentry
shares existing cancellation/depth/root protections and cannot publish an update.

| Retained value or operation | Target selection after publication |
| --- | --- |
| Direct/local/qualified call, recursive call, named function value or ordinary host function handle | Current implementation of the same checked declaration/application |
| Inherent or trait method call | Current body of the established method/implementation identity; no new trait relationship or type cast |
| Suspended async invocation, including caller frames waiting for a nested async callee | Resume each retained body with its matching locals and state; subsequent ordinary calls select current implementations |
| Already-created Future, including an unpolled one | Retain the selected computation and arguments/state; do not silently recreate it from a new body |
| Existing closure/async-block value | Keep its generated body and matching captured layout; ordinary calls made inside it select current implementations |

A cold Future is not a stable named-function handle: creation has already chosen its
computation even if execution starts later. Generated resume/closure targets are not
independent replaceable declarations. An existing closure does not become a new
closure just because its signature matches. New closure values created by a new
factory body use that body's generated code. Preserve existing initializer recipe
contracts as well; current-call selection does not authorize static reset or recipe
replacement.

Concrete old code and metadata remain retained only as required by actual frames,
Future/closure state, roots and other legitimate owners. This does not promise
function-granular reclamation or solve R13's type/code ownership split, but ordinary
call edges must no longer require a semantic lease on the entire old dependency
selection. Retention alone must not grant foreign-runtime or retired-installation
admission. Type/layout identity, native ABI, argument/result and candidate-isolation
checks remain mandatory when invoking new code with retained old values.

For the requested rule-collection scenario:

1. Keep the existing `DamageRule` trait contract unchanged.
2. Add `FixedDamageRule` and its `DamageRule` implementation in the candidate.
3. Publish the compatible candidate and construct a new interface object.
4. Business code explicitly inserts/replaces entries in an existing mutable
   collection of `DamageRule` after normal ownership/type checks.

The collection need not enumerate concrete implementations. Old and new elements
retain their concrete type and established trait relationship; calls select the
current method body for that relationship. Existing interface tables must preserve
checked identity/evidence without freezing ordinary method-body selection. This
requires focused cross-generation value-validation and dispatch tests; matching
display names alone is insufficient. Appending FixedDamageRule does not remove
BuggyRule, while replacing BuggyRule's existing method body changes subsequent calls
on its existing objects. Publication itself performs no collection mutation.

Adding `impl SomeTrait for Player` when Player already existed is excluded even if
its fields would remain unchanged. There is no automatic old-object interface
upgrade, implementation-relationship replacement, runtime trait injection or monkey
patching. Use state replacement for that change. Lifetime retention and old-code
cleanup remain bounded by actual owners and explicit host policies, not forced
migration of live captures/continuations.

## Exact-version function-source inspection

The host needs to inspect the effective source after repeated updates, without
reconstructing a chain of patches or assuming disk files match running code. This
is a confirmed requirement with proposed API details; it belongs to the update
owner, not an expansion of HM00-HM04 or a second debugger implementation.

### Query targets and results

| Query target | Required result |
| --- | --- |
| Current installation plus an ordinary callable identity/path | The implementation currently selected for a new call, captured with its actual version |
| An explicit historical version/source snapshot plus declaration/path | Source in that immutable snapshot; ordinary Program/Module handles do not select historical versions |
| A retained closure or already-created Future | The matching selected generated/computation body, not a new body with a coincidentally compatible signature |
| An observed execution frame or suspended task position | The source matching that captured frame/continuation target and generation, not the current ordinary call target |

A current-source query is a snapshot of selection at query time, not proof that a
later call ran it. A stable ordinary function/interface-method binding is not itself
evidence of an old executed body. Actual-execution queries must use observed target
evidence; explicit historical snapshot lookup must never be mislabeled as ordinary
Program/Module lookup or current dispatch.
A task may contain several frames and is not necessarily one function; expose the
available frame/continuation positions rather than inventing a single latest target.
Use existing safe observation/snapshot boundaries; report an unavailable position
when necessary instead of adding unsafe concurrent stack inspection or requiring a
new interactive debugger as a hidden prerequisite.

Return the complete matching function definition (signature and body) when available,
with module/declaration identity, installation/version identity, actual callable
identity where applicable, source URI/range and update provenance. Distinguish the
Program generation from the batch that last changed that function's source. For
example, Program v8 may still use the body last changed in batch 6. Identifiers and
content fingerprints are generated internally; users do not calculate hashes.

Generic instances and generated async/closure bodies must map to the correct retained
source origin and indicate their concrete/generated target where available. Source
is the input definition, not a claim that optimized bytecode or native instructions
have a one-to-one textual form. For a native Rust target, report host/native origin
and available binding identity; do not fabricate Kagari or Rust implementation text.
Exact public method names and result types are decided before implementation.

### Source ownership and publication

Retain immutable source snapshots and compiler-produced function/source associations
for source-query-enabled deployments. Candidate source associations become the
published version's associations only when that candidate commits. Failed patches
must never appear as active source. Unchanged source can share storage across
versions; reading a function must not replay every earlier patch.

Version and target evidence selects the source snapshot first; its checked range
then selects the text. Validate identity, bounds and UTF-8 positions, preserving
original text/line endings. Never substitute current disk/editor text, resolve by
name alone, guess a span from another version or present decompiled text as original
source. Resolving an external source record must verify its match to the compiled
snapshot; a content hash is a consistency check, not proof of trusted authorship.

Retained live old code must remain queryable under the chosen source-retention mode.
Share immutable source ownership with the corresponding code versions or retain it
in a host-managed versioned source store. Source text/history must not own Runtime,
GC roots or executable leases solely to keep diagnostics available. Historical source
may outlive code without keeping old code executable; once no live target requires a
snapshot, additional history retention is a separate bounded host policy. Do not
create a process-global unbounded source cache or claim to solve R13 here.

A source-free artifact can carry a matching source package or use an explicitly
configured external source store; it need not link HIR/the compiler to serve text.
Retain retrieval metadata independently of optional stepping/local-variable debug
information where this feature is enabled. If source was omitted, stripped, expired
or cannot be matched, return an explicit unavailable/mismatch result plus available
target/version identity. Do not silently fall back to newest source. Publish source
availability so deployments requiring inspection can check it in advance; do not
promise retrieval from artifacts that never included the necessary data.

Queries are read-only host diagnostics: they must not execute script, initialize
statics, mutate routing or grant private callable/member access. Hosts control access
to source text and external stores; there is no implicit filesystem/network fetch or
script-visible visibility bypass. Current
[error frames](../crates/kagari-runtime/src/error_trace.rs) already retain epoch,
code fingerprint, callable target and source positions, while
[pinned callable handles](../crates/kagari-runtime/src/native/function_handle/mod.rs)
retain execution owners. Reuse these identities and
[origin contracts](spec/error-reporting.md#bounds-versions-and-source-positions);
position metadata alone is not full source retention or a completed query API.

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

The snapshot boundary is an explicitly designated rooted data value, not an
automatic enumeration of module statics or the runtime graph. A handler may receive
that root directly; an object also reachable from a static is included only by
explicit export through the chosen data contract. Quiescence must account for all
writers to that object, including static aliases.

Static storage and snapshot eligibility are different contracts. A cell may retain
closures, interfaces, native selections or Future/Task values under ordinary runtime
rules without making those values serializable. The snapshot does not implicitly
include cell identities, fixed recipes, Failed diagnostics or pinned executable
and task/provider graphs. Compatible code rollback also does not restore earlier
cell values or permit deleting statics introduced since that code version.

A fresh environment receives newly restored explicit state and has fresh static
cells under the ordinary loading contract. Transferring exported data into selected
new cells would require a separate explicit restore contract; no automatic transfer
or replay of old initialization is provided. Only declared business resources may
be reattached; raw script Values, GC IDs, old Function/Script handles and operation
IDs do not cross into the new runtime.

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
bounded-cutover guarantee. Runtime-owned Opaque handles cannot cross into a new
runtime. Explicitly shared external resources inside payload wrappers are not a
shortcut around writer quiescence or candidate side-effect isolation; ownership
extraction also requires its own admission and alias-invalidation checks.

## Host API integration

Keep compatible publication and state replacement visibly distinct. The proposed
`runtime.update(&program, update)` prepares and publishes one source-change batch;
`reload_program` accepts an already prepared compatible candidate. Both should
converge on one validation/publication owner, not separate update semantics. The
host-managed state-replacement operation must not look like an interchangeable
`ReloadOptions` boolean.

Program/Module handles identify one installation and its modules, not immutable
public version views. Each lookup reads the latest coherently published namespace;
previously obtained modules can discover newly added public declarations. Ordinary
function handles select current implementations in that same installation without
host rebinding. Explicit historical source/debug snapshots and selected computation
lifetimes remain separate. HM00 must settle current-publication access, stable
installation/module/declaration identities and retention before UP integration;
the live-lookup policy is confirmed, not an open alternative. No parallel legacy
pinned-call or version-bound ordinary module API is required. Binding resolves the
public path once to a stable declaration/application; each call selects its current
implementation without re-resolving that name or switching declarations through an
alias. Runtime owns installations: dropping Program/Module handles does not unload,
cancel tasks or reset state, and independently retained callables stay usable while
the installation is live. Runtime shutdown still ends execution/object-access
authority. Explicit unloading remains separate.

The replacement dispatch contract is owned by UP, not silently added to HM's
namespace/type-export implementation. Existing pinned implementations/specifications
remain truthful until the coherent migration replaces affected callers and tests.
After fresh-runtime state replacement, old handles are invalid for the new runtime;
the host rebuilds routing and resolves new handles before commit. State constructors
and migration hooks are explicit calls with checked contracts, never implicit module
initializers. Exact exported function signatures for codec/migration are a review
gate; use the host API's outer-tuple argument rule when calling them.

## Review choices and implementation sequence

Confirmed boundaries: two explicit modes; existing signatures and named type/trait
contracts are frozen for compatible updates; function bodies may change and legal
new declarations may be added. New types may implement existing or same-batch new
traits; no new trait relationship is allowed for a previously published type.
Later batches treat earlier additions as existing declarations. Arbitrary in-place
migration is excluded; state continuity uses data. Exact-version function-source
inspection distinguishes current call selection, historical snapshots and retained
computation bodies without exposing manual hashes. Publication occurs at the host's
top-level scheduling boundary, with no active synchronous script stack or reentrant
publication. Suspended work may remain, and its subsequent ordinary calls use current
implementations. This supersedes the earlier whole-dependency pinning direction.
Source delivery with in-process compilation and interpreter/optional JIT execution
is confirmed; AOT patch delivery and broader native coverage are separate work.

Confirm these recommendations before implementation:

1. Audit complete descriptors for the confirmed frozen contracts, including private
   named declarations, without freezing ordinary locals or generated body shapes.
2. Rebuild affected Programs conservatively before optimizing incremental invalidation.
3. Use an explicit state root and tree/DTO codec initially, with versioned migration.
4. Start with a replaceable isolated state-owner runtime; shared heaps need group
   boundaries rather than implicit per-player extraction.
5. For state replacement, drain old jobs by default; cancellation is an explicit
   deployment policy with business consequences and no continuation rollback.
   Ordinary compatible publication does not require draining suspended tasks.

The [roadmap](implementation-roadmap.md) coordinates update work with the
[HM Program/Module plan](host-api-refactor.md) and current package identities.
HM does not implement source-change batches; its handle contracts must be settled
before batch integration. Reuse implemented conversion/root and async infrastructure,
not a second runtime. The new rule covers ordinary calls inside retained bodies,
not only the former HA proposal's latest-version root entries. State replacement
acceptance must cover existing async quiescence/completion boundaries where affected.
UP00-UP02 own compatible-update migration; state snapshot/cutover in UP03-UP04 is
separate work and not a prerequisite for publishing ordinary body patches.

- [ ] UP00: Re-audit compatibility descriptors and all pinned selection owners:
  direct/recursive/generic calls, host and named-function handles, trait methods,
  collection protocol callbacks, initializer recipes, Future/closure creation,
  suspended caller frames and native products. Specify stable identity/current
  implementation selection versus body/state retention; coordinate HM00 live
  Program/Module lookup and admission. Preserve specialized collection/initializer invariants rather
  than blindly replacing every stored version with latest lookup. Freeze the host
  top-level publication gate, failure-isolation strategy and current-target evidence
  for supported generic applications reachable from retained code. Resolve batch
  overlap rules, in-process source ownership, native preparation timing and source
  query/retention contracts. No new scheduling tiers or AOT patch delivery.
- [ ] UP01: Implement frozen-contract/addition validation and checked stable-callable
  target evidence, including impl-domain restrictions, generic application coverage,
  compiler/verifier/artifact changes and legal dependency invalidation. Keep existing
  dispatch until UP02 coherently migrates it; no ignored validation or execution stubs.
- [ ] UP02: Replace whole-dependency-pinned ordinary dispatch with current-call
  selection across runtime/VM/host/interface/native owners; retain selected
  Future/closure/frame bodies and cleanup. Integrate one-shot Update and prepared
  reload with host-top-level serialized publication and common candidate validation.
  Test latest calls from old resumed bodies, existing host handles and trait objects,
  atomic multi-edit submission, reentrant rejection and source-context consistency.
  Add exact-version source association/publication and inspection distinguishing
  current selection from retained/historical bodies. Migrate affected specifications,
  examples, repository invariants and tests in this checkpoint; do not leave two
  public semantic modes or assert that the old pinned suite is final acceptance.
  No new trait relationships for old types.
- [ ] UP03: Implement bounded script-state export/restore, schema checks and explicit
  migration. Reject runtime resources and unsupported alias/cycle forms precisely.
- [ ] UP04: Implement host cutover integration, candidate restrictions, admission
  gates and failure handling. Include synchronous examples and existing async task tests.
- [ ] UP05: Finish remaining state-replacement specifications/examples, run selected
  final verification and record carried limitations. Async is already implemented;
  use its existing task/Future contracts, not placeholders or conditional acceptance.

## Acceptance and verification

Test positive body/addition updates and rejection of removed definitions, changed
fields/variants/signatures, named constant changes and added impls for old types,
including blanket impls and implementations of a newly added trait. Cover a new type
implementing an existing or same-batch new trait, then a later batch replacing its
method body successfully while rejecting definition/signature/relationship changes.
Test order-independent compatibility, checked generic current targets, frozen constants,
rejection/prevention of stale inlined replaceable calls, stale-base rejection and new
modules without weakening artifact validation.

For batch submission, cover a single function and dependent edits across modules,
all-or-nothing failure, unknown/ambiguous and duplicate/overlapping targets, original
module imports/private context and signature mismatch. Verify no public begin or
caller-supplied hash/version is needed. Failed preparation must leave the authoritative
source context unchanged; the next successful update must include previously committed
changes rather than the failed candidate. Reject stale internal candidates and
foreign/retired installations, not live Program handles obtained before an earlier
compatible publication. Verify retained Module handles discover new public paths
without reacquisition, and verify host-top-level publication, rejection inside script-invoked host callbacks, sequential
update ordering and no script polling/resumption while committing. Queue external
completion notices until publication finishes. Inject pre-commit failures and verify
current targets, source associations and static state remain unchanged. Concurrent
publication/background compilation is not an acceptance requirement.

Test missing compilation context without adding frontend dependencies to artifact-only
execution. Existing ordinary host handles and named-function values must select new
bodies without rebinding. Resume old async code and verify its next local/qualified,
recursive, generic and method calls select current implementations; retain the exact
old frame state instead of substituting a new resume body. Include an unpolled Future
created before publication, suspended nested callers and an old closure with changed
capture layout in its replacement factory. Verify old generated bodies remain valid
while their ordinary calls use current code. Shared static state is not reinitialized.

For the supported JIT subset, verify exact candidate/native correspondence, current
ordinary host-handle selection and retained executable cleanup; distinguish actual
native execution from valid pre-entry interpreter fallback. Current native coverage
does not include general calls or suspension, so interpreter tests own those cases
until the backend supports them; later native expansion must enforce the same call
selection contract. Failed native preparation must not install an invalid product
or retry effects after native entry.
Reuse existing backend contract tests; wider JIT coverage and AOT patch delivery are
not acceptance prerequisites. These are proposed acceptance cases, not executed
results or authorization to implement the batch API.

Exercise a rooted mutable trait collection containing old implementations, insert
a new-type implementation after publication, remove the buggy element explicitly,
and verify receiver-specific dispatch, shared root protection and eventual version
reclamation. Also replace an existing implementation's method body and verify an old
interface object calls the new body without changing its concrete type/trait identity.
Wrong trait identities, foreign runtimes and forged tables must fail.

For source inspection, use several successful batches and a failed candidate. Check
full function text for current ordinary callable selection, explicit historical
source/version snapshots, retained closure/Future bodies and observed suspended frames,
including an unchanged function reused in a newer internal program generation. Distinguish
program generation from last source-change provenance. Cover closure/generic origins,
interface implementation versus trait declaration, CRLF/Unicode ranges, stripped debug
with separately retained source, missing/mismatched source packages and native targets.
Queries must not execute code or initialize statics. Verify live-old-source availability
under the selected retention mode and that retained diagnostic/source history does not
keep retired executable payloads alive. Reuse existing source-free and origin owners;
these cases do not authorize a new stack inspector or bypass runtime access checks.

For state replacement, test unchanged/changed schemas, missing migration, unknown
variants, numeric limits, invalid business references, cyclic/aliased state and
forbidden Opaque/callable/task fields. Inject failures at every pre-commit step:
old code/state must remain usable, with cancelled-work caveats reported honestly.
Test commit/abort mailbox ordering, handler incompatibility, shared-writer refusal,
post-commit cleanup errors and bounded peak memory while both environments coexist.

For the existing async implementation, include drain timeout, cancellation during RPC, nested
launch rejection, completion races, duplicate/late notices and abort after job
cancellation. No old continuation may mutate restored state and no test may claim
remote-effect rollback. Test the same contracts through source/artifact and supported
backend routes. Select focused local tests for affected contracts; complete workspace
and feature/backend/platform matrices remain CI-only. Local implementation checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Record actual commands, errors and owning phases here. Default checkpoints must
build; any exception needs activation-time approval. Measure preparation, quiescence,
snapshot, restoration and commit separately, recording machine/toolchain/profile,
features, default Cargo parallelism, cache state and workload. No latency target
or success result is invented by this proposal.
