# Compatible Hot Reload and State Replacement Design

Status: implementation and phased commits authorized on 2026-10-23. Joint
HM00/UP00 audit started at `3b898e80` and resumed at `bbe30666` after the user
reauthorized the complete phased sequence on 2026-10-24. UP00's design is frozen:
the user resolved transitive selection by protecting published key protocols,
initializer expressions and their dependencies, with no pinned-call exception.
The joint HM00/UP00 design checkpoint was committed as `bbfc0af6`; HM01 is locally
complete and committed as `3523c4c8`. UP01 is locally complete and committed as
`cd79fc80`; the [UP01 ledger](#up01-implementation-and-integration) retains evidence.
[HM02 integration](host-api-refactor.md#hm02-integration-and-up02hm03-handoff) is
locally complete and committed as `f0e1917d`. [UP02 integration](#up02-implementation-and-integration)
is locally complete with no carried errors or pending local checks; HM03 is next.
Full CI remains unrun. The default-foundation `Unknown` protection described in
the ledger is a material delivery limit, not a promise of unrestricted body patches.
The user explicitly prioritized
transparent generic type aliases first; the authorized prerequisite and its sole
progress ledger are in the [host API plan](host-api-refactor.md#transparent-generic-alias-prerequisite).
This design defines two deliberately
different update paths: compatible code publication preserves existing contracts
and live values, while state replacement saves an explicit data root and restores
it into a fresh execution environment. The motivating application has one player
state struct modified by protocol handlers; behavior is replaceable, durable state
is the continuity boundary.

The user confirmed this split and excluded adding a new trait implementation to an
existing concrete type during compatible reload. Arbitrary in-place object, stack
or closure migration is out of scope. Compatible-update APIs are frozen below;
the snapshot codec retains its UP03 gate. The compatible-update scheduling boundary
below is confirmed. Authorization follows the recorded phase order and does not
waive unresolved design or scope gates.
Existing specifications remain authoritative until implementation updates them.

The user also confirmed that ordinary Program and Module handles represent stable
identities within one installation and always query its latest published contents.
Existing function handles invoke current implementations after publication. No
Program/Module reacquisition or function rebinding is required. Internal immutable
versions and explicit source/debug snapshots remain separate from these ordinary
handles. UP02 has implemented and locally validated this separation; final focused
behavioral and local check evidence is recorded in its ledger.

On 2026-10-23 the user confirmed the update submission shape: construct an `Update`
containing function/file changes and submit it once to Runtime. There is no required
`begin_update`, caller-supplied version number or old-body hash. One Update is one
atomic publication batch for one installation. The exact UP02 API and compiler
ownership are frozen below; integrated implementation and remaining acceptance
are recorded in the UP02 ledger.
The user also confirmed that compatible updates replace function/method bodies or
add declarations, never change existing signatures or type definitions. Adding a
trait implementation to an existing type remains excluded even when the trait is
new in that batch. Exact-version function-source inspection is implemented in UP02;
source-review findings are closed.

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
the earlier pinned-callable policy. UP02 integrates the replacement; historical
pinned-call acceptance is not evidence of current-dispatch acceptance. Lasting
specification migration is complete through the assigned documentation owner.

The [package specification](spec/packages.md) owns dependency selection and build identity;
the [host API plan](host-api-refactor.md) owns embedding entrypoints. This document
owns update compatibility and state cutover, using the interop, Native and execution
protection contracts rather than duplicating them.

## Current foundation and gaps

Historical baseline before UP01/UP02: the inventory below explains the original
gaps, not the current integrated implementation. The [UP02 ledger](#up02-implementation-and-integration)
owns current behavior, evidence and remaining acceptance.

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
| Existing function/method body | Allowed after full checking and affected-code rebuild, except protected code below |
| New free function, inherent method, module, struct, enum or trait | Allowed if it preserves old contracts and introduces no resolution conflicts |
| New concrete type implementing an existing or same-batch new trait | Allowed after normal coherence and interface checks |
| New trait implementation for an existing concrete type, including a same-batch new trait | Rejected; no cross-version implementation extension in compatible mode |
| Deleting/renaming an existing definition or changing visibility/access contract | Rejected |
| Existing parameter/receiver contract, return type, generic parameters/bounds or sync/async mode | Frozen; no signature adaptation |
| Existing struct fields/types/writeability or enum variants/payloads | Frozen, including additions to existing layouts/variant sets |
| Existing trait methods, parent requirements or associated-type contract | Frozen |
| Existing impl receiver/trait/associated outputs or dispatch relationship | Frozen; method bodies may change |
| Existing named constant value/type | Frozen under the body-replacement/addition scope; unprotected body-local expressions may change |
| Published key Hash/Eq implementation or static initializer expression | Frozen, including transitive dependencies, independently of live values/cell state |
| Existing public function alias | May select another public declaration with the same checked callable contract; retained bindings never retarget |

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

### Protected code and dependency closure

The user resolved this boundary on 2026-10-24: **there is one ordinary current-call
rule and no special pinned execution domain**. Reject compatible updates changing
published key Hash/Eq protocols, static initializer expressions or their transitive
dependencies. Protection starts at publication, including unused declarations and
uninstantiated generic templates; it never depends on live container counts, empty
containers, whether a static has initialized, or whether its initializer failed.
Later-added roots become protected at their introducing publication.

Protect every published implementation supplying key Hash/Eq, including inherited
Eq/PartialEq methods, defaults, generated adapters, selected native requirements and
their argument/associated-output evidence. Protect every published static's checked
initializer expression and executable recipe. Keep lazy initialization: no eager
execution, reset, automatic rehash, value migration or user callback during update.
The recipe's exact generated entry remains retained, but ordinary calls it makes
select current implementations; compatibility proves those dependencies unchanged.
Stored membership selections retain their relationship/type authority, not a second
old-code dispatch policy. Content comparison remains an ordinary current call.

UP01 emits a contract-owned protected-dependency graph alongside complete declaration
contracts. Each root records its declaration/application, root kind, canonical
initializer expression when applicable, and executable body/selection evidence.
Edges cover direct/recursive calls, named-function values, closure creation/invocation,
interface/witness dispatch, default/adaptor code, native callback requirements,
constant/type inputs and resolved declaration targets. Compare canonical records,
not source positions, executable slot numbers or hashes alone. A source initializer's
non-trivia token sequence and checked resolutions are frozen even when optimization
would fold two different expressions to identical instructions. Comments/formatting
may change. Literal payloads and meaningful expression tokens may not.

For executable equality, use bounded canonical instruction/constant/semantic-contract
records with local operand numbering and stable declaration references; omit debug
locations. Preserve protected code before optional body simplification, so an
optimizer cannot erase a dependency and make an unsafe edit look independent.
Compiler lowering emits both the protected declaration/template dependencies and
executable evidence; MIR/bytecode verification checks every surviving executable
edge and its target/contract against that evidence. The compiler owns completeness
for unused source templates and source-expression correspondence; a source-free
artifact declares those contracts, not authenticated original source. Reload compares
all prior protected records, and never accepts a missing certificate as an empty set.
Matching supplied source additionally validates its expression records and associations.

Close the graph conservatively. A dynamic call with a finite proved target set
protects every member and that set's selection contract; additions that enlarge its
possible targets require revalidation and reject if they change protected behavior.
A native callback is closed only when installed, trusted declaration evidence gives
its complete callable dependencies (or a sealed primitive proves no script call).
Do not interpret existing effect flags or an empty callback list as that proof.
Unresolved function values, dynamic dispatch or native reentry use `Unknown`, never
an empty edge set. Initially `Unknown` protects the entire installation's executable
inventory and selection relationships: reject body/initializer/native-selection
changes and executable additions unless complete evidence proves independence.
This deliberate conservative limitation may block otherwise useful patches; report
the protected root and unknown edge rather than silently pinning or guessing.
In the integrated SDK, the default standard foundation can trigger this rule even
for a simple Engine body patch. Finite-environment success fixtures do not establish
default-foundation patch availability; the UP02 ledger records that delivery limit.

A helper shared by a handler and Hash/Eq is protected even when edited through the
handler's file or a public alias. Compare resolved dependencies too: unchanged text
with changed imports/alias resolution is not unchanged protected code. Native Rust
code/registration cannot be replaced through this API. Mutable key contents and
external state stability remain the author's duty; frozen code is not a purity
proof and cannot roll back host effects.

### Stable identity and executable ownership

Use HM00's single installation owner and metadata cell, not a second UP registry:

```text
Installation = { HostRegistryId, non-reused InstallationId, root identity,
                 Mutex<Option<Arc<ProgramDescriptor>>> current }
ProgramDescriptor = one immutable publication, exact VersionToken,
                    checked namespaces/contracts/current targets/source association
ExecutableModule = { publication, generation-local ModuleRef }
CallableKey = { installation, StableDeclarationId, canonical applied arguments }
SelectedComputation = { exact ExecutableModule, CallableTarget,
                        checked type/witness environment, executable retention }
```

The ModuleStore owns installation lifetime, stable declaration/application indexing,
persistent cells and the retained-demand envelope. Program/Module handles share only
the installation access cell; `None` is retired, never lookup-by-name fallback.
Runtime shutdown clears the cells. Copy current metadata under a short lock, release
it before execution/conversion/callbacks, and retain selected executable owners through
result rooting. Ordinary callables retain stable contract/type authority, not a
binding-time executable lease. Frames, closures, cold Futures and native producers
retain their exact body/capture/resume owners. Ordinary subsequent calls, including
recursive and unqualified calls, select current code in that same installation.
R13's physical type/code retention split remains separate; metadata alone never
revives admission. Runtime remains Send/not Sync; HM owns the exact handle auto traits.

`StableDeclarationId` is a non-reused installation-local index, not a wire ID.
Portable `DeclarationKey` records identify package/module/kind/authored owner/name;
impl owners use a canonical checked receiver/trait header with alpha-normalized
binder positions and constraints, not anonymous source-order occurrences. Inherent
members use that semantic owner plus member name; equivalent impl-header groups are
merged for correspondence, with duplicate members rejected. Method signature/access
and associated outputs remain frozen contracts. Publication-local DefinitionIds and
generic binder owners map explicitly to these keys; executable placement IDs never
stand in for authored declarations. Preserve this correspondence across explicit
identity import and apply it to witness/cache/type comparisons, not just reload errors.

Named block-local functions are named declarations too, not automatically generated
closures. Key them under their authored callable owner; same-name local declarations
that cannot be uniquely matched without mutable block/occurrence order have no safe
cross-publication correspondence. Conservatively reject a candidate changing their
containing body unless exact unchanged-body evidence preserves the old mapping;
never pair ambiguous locals by signature or position. This finite rejection is not
a ban on compiling/executing such source. UP01 separates authored local identity from
imported default-body placement. Named-function values carry an explicit checked
live-callable classification; zero captures do not establish it. Actual closures,
async blocks and resume entries are body-owned targets with checked origin/capture
contracts, never independently rebound merely because signatures match.

Inventory every old named contract, including private, unused and generic declarations,
normalized aliases and constants. Compare preservation by stable identity, allowing
new modules/declarations without freezing local slots or generated body shapes.
A new trait impl is allowed only for a newly introduced outer nominal receiver
(such as `New<T>`), with normal coherence; reject old nominal, primitive, tuple and
blanket extensions. New inherent methods may extend old types without changing old
members. Old type definitions and trait relationships remain fixed.

Public path compatibility is deliberately different from bound identity. Preserve
old function-path callable contracts, but allow same-contract retargeting to another
public declaration as HM00 specifies. Both old declarations remain present; an old
binding calls its original declaration's current body, while fresh lookup follows
the new alias. Protected dependency resolution may still reject that retarget.
Type exports preserve normalized nominal target/application, not merely structural
layout. Preserve every prior public canonical namespace as a live query start.
Use bounded directional graph-pair traversal, including cycles; do not change the
old equivalence union-find into an unproved subset algorithm.

### Bounded retained-demand evidence

Use one contract-owned record family, composed from existing `Ty`, `GenericBody`,
`OperationWitness`, construction evidence and canonical declaration references:

```text
DemandEnvelope = deduplicated { concrete applications, body requirements,
                               implementation/construction/layout requirements }
BodyRequirement = { exact origin key, binder parameters, checked call/creation edges }
CallRequirement = Direct(application)
                | Shared(declaration, argument expressions, operations, constructions)
                | Interface(relationship/member, argument expressions, witnesses)
                | RetainedBody(origin, argument expressions)
                | Native(declared application and callback/result-adapter evidence)
CurrentTarget = { stable callable application, exact executable slot,
                  instantiated callable contract or shared-body binder contract }
```

At publication retain the union of all previously published demand summaries and the
candidate's requirements, deduplicated; no executable lease is held solely by this
monotonic summary. This conservatively covers old retained bodies without a new GC
liveness pass. Historical generated bodies contribute their own old edges; ordinary
callee edges demand **candidate** implementations. New current bodies contribute new
edges. A generic application removed from new source reachability must still be
compiled if an old body could call it. Shared bodies keep their existing checked
witness/environment route; do not convert every generic to shared execution or infer
new types/specialize source in Runtime.

Close these demands under substitution, selected interface methods, callbacks and
construction/layout requirements, with one visited set per exact body application.
Dynamic receiver coverage includes every admissible checked implementation in the
candidate inventory, including new types used by old interface callers. Preserve
selected relationships and complete associated-output evidence; never search for a
replacement impl at runtime. Source update seeds InstancePlanner from the envelope;
prepared/source-free reload rejects missing current coverage with the absent
application identified. It never substitutes an old specialization or silently
recompiles an artifact with unrelated source.

Hard admission limits for the combined envelope and protected evidence: 64 MiB of
canonical encoded records, 1,000,000 total records/edges and fixed-point work visits,
65,536 distinct body applications, 4,096 entries per nested list, type depth 64 and
8,192 nodes per type transformation. These reuse existing artifact/type scales;
compiler request limits may be stricter. Charge before allocation/enqueue, check
cancellation during traversal, and fail publication on overflow/exhaustion. Never
truncate history/evidence to pass a limit. Installation retirement releases it;
precise pruning is deferred and no unlimited update-count claim is made.

Compiler, MIR and bytecode share the contract algorithms, not duplicate semantic
solvers. Derive executable demands from actual call/creation/interface/native records
as the existing construction verifier does. Validate declared body classification,
identity correspondence, signature/binder application, all operation/construction
witnesses and target/layout coverage; serialized completeness flags grant no authority.
Compare candidate coverage with the runtime's own envelope, not a candidate-supplied
replacement baseline. Protect ordinary call boundaries from cross-function inlining,
folding or removal based on a replaceable callee's current body/effects. Existing
local optimizations may continue when they preserve those boundaries.

## Update scope and publication

The host may edit any part of the loaded source dependency graph, including an
indirect dependency. Preparation determines affected code within the selected
installation; edits are not global mutations of every installation using a package.
The first implementation may rebuild the affected Program in full and reuse
immutable code where safe; it need not produce a minimal machine-code patch.

### Change lists and one-shot submission

Separate four granularities:

| Boundary | Frozen UP02 contract |
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

Frozen UP02 API, not currently implemented:

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

Exact SDK declarations (new update/source records belong to the SDK; executable
contracts remain in kagari-contract):

```text
Update::new() -> Update
Update::replace_function(&mut self, target: impl Into<FunctionUpdateTarget>,
                        definition: impl Into<String>) -> &mut Self
Update::replace_file(&mut self, name: impl Into<String>, text: impl Into<String>) -> &mut Self
Update::add_file(&mut self, module: ModuleIdentity, name: impl Into<String>,
                 text: impl Into<String>) -> &mut Self
Update::with_cancellation(self, CancellationToken) -> Self
FunctionUpdateTarget = Path(String) | Declaration(SourceDeclaration)
Runtime::update(&self, &Program, Update) -> ReloadResult<()>
Runtime::reload_program(&self, &Program, &PreparedProgram, ReloadOptions) -> ReloadResult<()>
```

`&str`/`String` targets convert to `Path`. Paths resolve once in the captured root's
current public function graph; no ad-hoc method/type-expression grammar is added.
`SourceDeclaration` is an opaque installation-scoped stable authored declaration
locator obtained through source inspection below. It selects private functions,
methods and named locals without granting callable/private-member access. Retained
locators survive compatible publications; foreign, ambiguous or non-function targets
reject. A replacement supplies one complete function definition, not a body fragment;
parse in its original owner/import/binder context and preserve its frozen contract.
Methods that cannot be uniquely named through a public path use the locator.

File names are normalized captured document names, not filesystem read instructions.
`replace_file` requires an existing captured document; `add_file` requires a new
physical document and explicit module mapping in an already captured package. It
may add modules, not acquire a new package/dependency from disk/network. Changes to
inline modules use their containing file. No delete-file convenience is needed:
deleting old declarations rejects, while unused input cleanup can be a later input
maintenance operation. Empty batches reject. Builders only own data; submission
validates at most 1,024 edits and 64 MiB replacement UTF-8 bytes, plus existing source,
parser and executable limits. Cancellation defaults to a fresh token, never a stale
cancelled preparation token; `with_cancellation` selects the request's token.

Use existing `EmbeddingError` families: `Source` for missing compilation context,
unknown/overlapping targets or malformed input; `Diagnostics`/`Compilation` for
checking/lowering; `Cancelled` for cancellation; `ReloadValidation` with specific
core codes for frozen/protected-contract, coverage and stale-base failures. Runtime
phase/resource/stale-handle failures retain their existing runtime categories.
No new global error hierarchy or mandatory update report is introduced.

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

**Core admission and commit.** Track active synchronous entry/driver scopes separately
from parked session/frame retention and the replaceable active-session pointer.
The count includes native/script entry, observers, script-invoked host callbacks,
synchronous reentry and candidate scopes, even while a nested scope hides another
session. One RAII publication guard rejects when that count is nonzero, another
publication is in progress, or runtime borrow/liveness admission fails. All public
reload/update publication paths use it; no name/candidate-root-only shortcut remains.
Reject before source compilation for ordinary update, and recheck immediately before
commit. Suspended work does not itself increment the synchronous count. Script
entry/resumption rejects while the guard is held; completion notices may enqueue but
are not consumed. Existing cancellation/depth/candidate restrictions still apply.

Prepare candidate records, complete current target tables, static binding plans,
protected/retained evidence, native decisions, source associations and cache invalidation
plans before visibility changes. Acquire the installation cell/store guards and reserve
all needed storage before commit; recheck installation identity and expected baseline.
Commit updates only preallocated store state and replaces the single current descriptor
pointer. There is no fallible suffix, callback, initializer, allocation-dependent map
insertion, hash recomputation or old-descriptor destruction under the commit lock.
Release guards before dropping old owners. Lock poison fails admission as EngineFault;
shutdown may recover only to clear authority. Do not turn a postcommit cleanup problem
into a claimed rollback. Candidate leases own abandoned storage cleanup; unused epoch
IDs and interned identities may remain, but no failed candidate becomes active or the
next source baseline. SDK association failure happens before core commit.

Beyond HM's short metadata-cell mutex, the initial design needs neither concurrent
execution-reader coordination nor lock-free call-target swaps, concurrent update
merging or a background compilation protocol.
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
entrypoint, not a requirement to build a second patch delivery system. The capsule
and commit owners below are the UP00 decision. Do not implicitly attach an arbitrary
editor overlay or require Runtime to discover files/network dependencies during
publication.
An artifact-only deployment without sufficient source/compiler metadata must reject
source updates clearly, not pretend public signatures contain private/generic bodies.

**Compilation capsule owner.** Extend SDK source preparation to retain an immutable
`CompilationInputs` capsule through `CheckedModule`/`Compilation` into PreparedProgram.
Capture exact physical document bytes/names and module mappings, root/package selections
and dependency aliases, installed native/offline Host declaration snapshots and their
identity scopes, parser/constant/diagnostic limits, compiler contract identity and
lowering options. Preserve package acquisition provenance where present; direct source
and explicit snapshots remain valid without inventing a package lock. Do not store
runtime callback captures, mutable analysis databases, filesystem discovery or the
PreparedProgram Rc/cache. Separate semantic settings from per-request cancellation.
The capsule's owned data must be Send + Sync; reconstruction creates a temporary
source database/analysis session using declaration-only inputs, not a new Runtime
or native registration execution. Capture declaration inputs at checking time, not
later from a possibly changed engine.

The existing `PackageBuildInput` retains ProgramInputs and an environment fingerprint,
not the environment needed to reconstruct compilation. Extend that preparation owner
rather than treating the fingerprint as reconstruction evidence. Existing
`analyze_source` and `analyze_package` mutate the engine's source database before
semantic success; never use the host engine itself as the update transaction.
Apply the full batch to detached capsule data, rebuild conservatively with retained
demand roots, and publish the new capsule only with the matching executable.

SDK Runtime stores capsule associations by installation and exact publication token;
core Runtime remains independent of HIR/compiler/package acquisition. Stage the
association before the core visibility switch, with rollback/removal on rejection;
current source context is looked up through the core's current publication token,
not a separately advanced SDK 'latest' pointer. This also handles low-level prepared
reload: a publication without a matching capsule yields MissingCompilationContext,
never the previous capsule. Discard superseded compilation capsules once no pending
update needs them; exact-source history has its separate bounded owner below.

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
is frozen as explicit pre-entry preparation, not automatic lazy JIT. Existing
exact-version preparation tests remain evidence of product safety, not proof that
current-call dispatch is implemented.

`update()` conservatively publishes verified interpreter-capable code and retains
matching MIR only when the captured PreparationOptions requested native input.
It does not retain a CodegenBackend or invoke a hidden compiler callback. Reuse the
existing `Runtime::prepare_native` workflow after publication: it must capture the
Program's current target and validate exact PreparedProgram correspondence before
installation. For source-updated Programs, add the native-feature SDK accessor
`Runtime::prepared_program(&self, &Program) -> RunResult<PreparedProgram>`: construct
a fresh preparation-side Rc/cache wrapper over the current immutable verified
bytecode/optional MIR, without retaining Rc in Runtime. This is explicit advanced
preparation access, not an ordinary version-view handle or a requirement for calls.

Ordinary dispatch selects the current script body first and consults only native
products indexed by that exact selected body/version/configuration. An old native
product is never reused by stable callable ID alone. No installed current product
means interpreter execution; existing policy/unsupported-code fallback remains
pre-entry only. Explicit `execute_prepared` with a mismatched old product rejects;
it must not silently reinterpret that handle as current. Native preparation failure
publishes no product and changes no code/static/source state; it does not roll back
an already successful source update. If an explicitly supplied candidate workflow
prepares native code before reload, all product checks/install planning occur before
commit and failure leaves the old publication unchanged. This separation makes the
ordinary Update API usable without a new backend service or eager native matrix.

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
is a confirmed requirement with frozen UP02 API details below; it belongs to the update
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
The exact UP02 query surface is frozen below; it does not replace live HM lookup.

### Frozen source inspection API

Reuse existing scoped identity, rooted-value and frame-observation boundaries, not
HIR queries from Runtime. The current `SourceOrigin`/error frames carry positions,
not full text; the SDK's AnalysisSnapshot is a preparation/tooling object, not an
execution-source owner. New diagnostic records live at the source-free contract/
runtime boundary, with SDK forwarding and no source feature needed to read an
attached source package. Design signatures:

```text
Runtime::source_snapshot(&self, &Program) -> RunResult<ProgramSourceSnapshot>
ProgramSourceSnapshot::declarations(&self) -> &[SourceDeclaration]
ProgramSourceSnapshot::function(&self, &SourceDeclaration) -> SourceQuery
Runtime::function_source(&self, &FunctionSourceTarget) -> RunResult<SourceQuery>
Runtime::value_source_target(&self, &RootedValue) -> RunResult<FunctionSourceTarget>
Runtime::frame_source_target(&self, &ExecutionFrame) -> RunResult<FunctionSourceTarget>
Function<A,R>::source_target(&self) -> FunctionSourceTarget
AsyncFunction<A,R>::source_target(&self) -> FunctionSourceTarget
Method/AsyncMethod/InterfaceMethod/AsyncInterfaceMethod::source_target(&self)
    -> FunctionSourceTarget
SourceQuery = Available(FunctionSource) | Unavailable(SourceUnavailable)
```

`Function` is the UP02 replacement for the ordinary PinnedFunction name, without an
alias. FunctionSourceTarget is opaque: handle constructors produce a stable current
callable/application target (including receiver-specific impl evidence for bound
interface methods); value/frame constructors produce an exact observed target.
Value inspection accepts a rooted closure or script/native Future, never polls or
claims it, and reports unavailable position when a consumed/composed Future has no
single inspectable body. Frame conversion is allowed only inside the existing safe
observer/suspended-frame view, validates its Runtime/installation, and copies exact
identity, not borrowed frame storage. It does not create a new stack inspector.
A multi-frame task is queried through its available observed frames, not flattened
to a fictitious single function. Historical snapshot lookup uses authored declaration
identity and never resolves against current code. Locator mismatch yields Unavailable;
foreign/expired execution handles remain ordinary admission errors.

ProgramSourceSnapshot is an immutable diagnostic owner for the selected publication,
not an execution lease. SourceDeclaration exposes read-only authored kind/name/module/
owner information, plus an opaque installation/declaration locator used for edits;
no raw numeric ID or source offset is accepted as update authority. The list includes
private authored declarations for host diagnostics without granting private calls.
SourceQuery's Available record contains complete definition text, URI, checked byte
range, authored and actual executable/application origin, installation/publication
identity and last-source-change provenance. Generated closure/resume queries name
the generated target and return its authored enclosing function (or initializer)
with a generated-origin marker; generic instances return their authored definition
and checked application. Interface queries identify the implementation, not just the
trait declaration. Host-native targets report binding/origin without fabricated text.

Unavailable reasons are `NotRetained`, `MissingPackage`, `Mismatch`, `NoObservedBody`,
`UnknownDeclaration` and `NativeOrigin`; retain known target/version/native identity
in the response. Invalid runtime access is an error, not an unavailable-source result.
An exact target token retains source records or an unavailable marker, never executable
admission. Current-source queries capture current selection when queried; querying
one does not prove a later call used it.

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

Frozen retention policy: source preparation retains immutable exact source packages
by default; artifact export includes them only with a new explicit
`ArtifactOptions.retain_source: bool` (default false). Execution from stripped input
remains valid with explicit unavailable results. Attached packages contain bounded
bytes and compiler associations keyed to the canonical executable product; verify
identity/content, UTF-8 range and generated origins at preparation. This package is
independent of optional stepping metadata and portable MIR. No implicit external
source fetch or filesystem fallback is implemented. An artifact's matching embedded
source supports inspection, not reconstruction of missing compiler/declaration inputs.
The optional package is limited to 64 MiB and existing bounded table counts; source
preparation fails rather than silently dropping required associations when its
retention limit is exceeded.

Retained live old code must remain queryable under the chosen source-retention mode.
Share immutable source ownership with the corresponding code versions or retain it
in a host-managed versioned source store. Source text/history must not own Runtime,
GC roots or executable leases solely to keep diagnostics available. Historical source
may outlive code without keeping old code executable; once no live target requires a
snapshot, Runtime retains no automatic history. Hosts retain explicit
ProgramSourceSnapshot values when history is required and own the count/byte budget. Do not
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

Keep compatible publication and state replacement visibly distinct. The frozen UP02
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
lifetimes remain separate. HM00's frozen representation supplies current-publication
access, stable installation/module/declaration identities and retention for UP
integration;
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
contracts are frozen for compatible updates; unprotected function bodies may change
and legal new declarations may be added. New types may implement existing or
same-batch new traits; no new trait relationship is allowed for a previously published type.
Later batches treat earlier additions as existing declarations. Arbitrary in-place
migration is excluded; state continuity uses data. Exact-version function-source
inspection distinguishes current call selection, historical snapshots and retained
computation bodies without exposing manual hashes. Publication occurs at the host's
top-level scheduling boundary, with no active synchronous script stack or reentrant
publication. Suspended work may remain, and its subsequent ordinary calls use current
implementations. This supersedes the earlier whole-dependency pinning direction.
Source delivery with in-process compilation and interpreter/optional JIT execution
is confirmed; AOT patch delivery and broader native coverage are separate work.

UP00 freezes the compatible-update decisions in this document. State-replacement
implementation gates remain explicit at UP03/UP04 entry; the following boundaries
are the selected direction, not a claim that their codec/cutover APIs already exist:

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

The [alias prerequisite](host-api-refactor.md#transparent-generic-alias-prerequisite)
is locally complete. The authorized order remains joint HM00 + UP00,
followed by HM01, UP01, HM02, UP02, HM03, HM04, UP03, UP04 and UP05. The user has
authorized completion and staged commits. UP00 design closure is recorded below;
the joint checkpoint records design closure, not implementation.
Commit the completed joint design checkpoint with both Plan-Phase trailers, then
each completed phase separately. The user resolved the former alias scope gate by
authorizing the prerequisite, not by waiving HM00/UP00 design gates or reopening
confirmed live-handle/current-call semantics. MS and cross-installation repair
linking remain separate.

- [x] UP00 (design only): Re-audit compatibility descriptors and all pinned selection owners:
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
- [x] UP01: Implement frozen-contract/addition validation and checked stable-callable
  target evidence, including impl-domain restrictions, generic application coverage,
  compiler/verifier/artifact changes, protected-dependency rejection and legal
  dependency invalidation. Migrate validation expectations affected by the new
  rejection policy in UP01. Keep existing dispatch until UP02 coherently migrates
  it; no ignored validation or execution stubs. Locally complete with no carried
  errors; the [UP01 ledger](#up01-implementation-and-integration) records acceptance
  evidence. Committed as `cd79fc80`; HM02 is committed as `f0e1917d` and UP02 is locally complete.
  HM02 acceptance is recorded in the [HM ledger](host-api-refactor.md#hm02-integration-and-up02hm03-handoff).
  Full CI remains unrun.
- [x] UP02: Replace whole-dependency-pinned ordinary dispatch with current-call
  selection across runtime/VM/host/interface/native owners; retain selected
  Future/closure/frame bodies and cleanup. Integrate one-shot Update and prepared
  reload with host-top-level serialized publication and common candidate validation.
  Test latest calls from old resumed bodies, existing host handles and trait objects,
  atomic multi-edit submission, reentrant rejection and source-context consistency.
  Add exact-version source association/publication and inspection distinguishing
  current selection from retained/historical bodies. Migrate affected specifications,
  examples, repository invariants and tests in this checkpoint; do not leave two
  public semantic modes or assert that the old pinned suite is final acceptance.
  No new trait relationships for old types. Implementation is locally complete;
  HM03 is next. The [UP02 ledger](#up02-implementation-and-integration) records closed
  reviews, fresh artifacts and final passing checks. No carried errors or pending
  local validation remain. Full CI remains unrun.
- [ ] UP03: At phase entry freeze the codec/envelope format, exact export/restore and
  migration-hook signatures, schema evolution rules, numeric/size/work limits and
  alias/cycle diagnostics against existing conversion APIs; record that gate here
  before implementation. Then implement bounded export/restore and migration.
  UP00 settles the separate-mode boundary, not these codec implementation contracts.
- [ ] UP04: Before implementation freeze the non-cancelling drain/admission state
  machine, candidate resource restrictions, mailbox/routing commit API and abort/
  timeout ownership, using UP03's codec. Implement cutover and failure handling with
  synchronous examples and existing async task tests. These gates are not UP00-complete.
- [ ] UP05: Finish remaining state-replacement specifications/examples, run selected
  final verification and record carried limitations. Async is already implemented;
  use its existing task/Future contracts, not placeholders or conditional acceptance.

## Acceptance and verification

Test positive body/addition updates and rejection of removed definitions, changed
fields/variants/signatures, named constant changes and added impls for old types,
including blanket impls and implementations of a newly added trait. Cover a new type
implementing an existing or same-batch new trait, then a later batch replacing its
unprotected method body successfully while rejecting definition/signature/relationship
changes and edits to protected methods/dependencies.
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
not acceptance prerequisites. These are required future acceptance cases, not
executed results; UP01 evidence below does not establish UP02 dispatch acceptance.

Exercise a rooted mutable trait collection containing old implementations, insert
a new-type implementation after publication, remove the buggy element explicitly,
and verify receiver-specific dispatch, shared root protection and eventual version
reclamation. Also replace an existing implementation's unprotected method body and
verify an old interface object calls the new body without changing its concrete
type/trait identity.
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

## Execution ledger

### UP02 implementation and integration

**Status: locally complete; no carried errors or pending local checks. HM03 next.**
Started from HM02 commit `f0e1917d`. Dispatch, publication, SDK Update, exact-source
work, artifact regeneration, review closure and durable specification/conformance
migration are complete, including final local validation. Full CI remains unrun.
This is the single UP02 ledger, consolidating the earlier publication-owner and SDK logs.

**Material delivery limit — default Engine body patches can reject.** The standard
foundation can introduce protected `Unknown` dependencies, conservatively freezing
installation inventory even for a seemingly unrelated, simple body replacement.
This is intended protection, not a remaining dispatch bug. Passing update fixtures
with a finite independent declaration environment demonstrate the real SDK pipeline
under those inputs; they do not promise that the default Engine accepts the same
patch. The standard-foundation rejection test preserves code, source and token.
There is no special old-version Hash/Eq/static call domain, eager initialization,
automatic rehash or weakening of unknown-dependency protection.

#### Integrated responsibilities and boundaries

- Runtime callable identities use stable, nonreused mappings and a current-target
  table per installation. Ordinary direct/shared/interface calls, native callbacks
  and host `Function` select current implementations; no `PinnedFunction` alternative
  remains. Named function values no longer retain old executable code solely as
  ordinary bindings. Selected generated/closure/resume bodies and queued native or
  interface selections retain their exact execution evidence; they are not rebound
  midway through an operation. Calls made by retained work use ordinary current
  selection where required, not a second protected-code dispatch domain.
- Runtime-global synchronous activation is distinct from parked execution sessions.
  Entry, nested/candidate scopes, observers/callbacks and scheduler turns retain
  activation through cleanup; parking releases it. `Runtime::begin_publication`
  requires no active synchronous scope, competing publication or native storage
  borrow and a live target/runtime. Entry/resumption and driver consumption reject
  while the guard is held; producer notifications remain independent.
- Source updates acquire `PublicationGuard` before compilation, stage through
  `stage_reload_verified_program` and commit with `guard.publish`. The existing
  `publish_staged_reload` wrapper acquires the same guard, not a nested one. Metadata
  staging defers GC/abandoned-record cleanup and runs no initializer or callback.
  Initial load uses `stage_load_verified_program` and `publish_staged_load`; ordinary
  load composes them. `StagedLoad::publication_token` exists before visibility, with
  no live Program/executable handle exposed. SDK source association reserves/inserts
  by this exact token before commit, rolls back on rejection/unwind and performs only
  infallible completion afterward; no post-commit allocation/token lookup is needed.
- Immutable Send-compatible compilation capsules capture physical source/module
  mappings, declarations, package environment, settings and provenance, not callbacks,
  mutable analysis databases, PreparedProgram caches or executable leases. Update
  builders own replacements/additions; resolution rejects duplicate aliases and
  overlapping edits, then rechecks one detached candidate using retained demands.
  Failed preparation/publication cannot advance the authoritative source baseline.
  Successful publication prunes superseded capsules; a low-level publication without
  its own capsule reports `MissingCompilationContext`, never reuses an older one.
- Exact source queries distinguish current callable selection from observed/historical
  bodies. Prepared products carry verified source packages; artifact retention is
  opt-in via `ArtifactOptions::retain_source` (default false). Source-free artifact
  preparation preserves/verifies retained source without frontend dependencies.
  Compiled source-manifest anchors close the same-document range-swap hole. Physical
  source identity is `(module, uri)`: multiple packages may share a presentation URI.
- Public generic authored declarations and aliases remain in the checked export graph
  even without a concrete executable slot. Source update resolves declaration identity,
  not a fabricated callable or source-name fallback. Generic signature/arity/bounds
  comparison uses scoped binder correspondence; equivalent aliases may retarget while
  changed bounds/results and private/generic-entry forgeries reject. A declaration
  export alone does not authorize a typed executable binding.
- SDK batch preparation preflights final physical-document and aggregate text sizes
  before allocation/replay, including unchanged text, replacements and additions.
  Checked arithmetic, UTF-8 extent/overlap validation and shared `MAX_SOURCE_BYTES`
  apply independently of edit order; materialization rechecks before cloning. Text
  copy observes cancellation at UTF-8 boundaries at most 64 KiB apart. Failures use
  the same RAII association/publication cleanup. The latest SDK follow-up reports
  all three focused preflight tests passing after the manifest handoff.

Mechanism reference: the previously consulted Java SE 25 `Instrumentation.redefineClasses`
in the [resumed audit](#resumed-up00-audit-at-bbe30666) retains old active frames,
selects new code on new invocations, preserves statics and publishes a batch atomically.
UP02 applies that separation with Kagari's exclusive host-top-level publication,
checked generic targets and retained generated/resume state, not JVM concurrent
redefinition or its schema rules.

#### Reported focused evidence

Results below are owner/coordinator reports from selected runs, not a new sweep or
additive coverage totals; earlier and latest selections can overlap.

| Boundary | Passing evidence |
| --- | --- |
| Core dispatch | 4 dispatch cases; independent review confirmed 2 dispatch issues fixed |
| Publication / initial load | 5 guard cases and 3 staged-load cases |
| Native/JIT and drivers | 11 JIT/native cases, 2 owned-driver cases and source-free frame coverage; 3 old-native-admitted/current-callback cases |
| Retained values and cleanup | 12 named/closure/async/source-cleanup cases after the root-retention fix |
| Method and queued execution | 14 method/queued cases; earlier 6 transport and 5 callback cases |
| Typed SDK targets | 3 selected typed-function/async/source targets |
| Runtime source | 6 source-runtime and 2 product-attachment cases |
| Source packages / codecs | Compiler source-package 8, contract 3, bytecode 3 and disassembly 7 |
| SDK source updates | Original 9 selected cases including rollback, plus 2 protection/generic-alias cases |
| Capsules / preflight | Existing 4 capsule cases, including repaired equal-URI reconstruction; latest SDK follow-up: all 3 new preflight cases passed without further repairs |
| Generic declaration exports | Earlier compiler package exports 2 and runtime reload exports 7 |
| Native contracts / VM repairs | Native contract 10 and dynamic-empty 3; 7 previously failing VM cases now resolved, including the last interface fixture |

Final reported closure evidence:

- Review findings 1 (source-manifest anchors) and 2 (preflight) are reviewer-closed.
  Finding 3 (ordinary bound-interface current selection) and the `BodyOwned`
  regression are repaired; 2 focused tests passed, including the exact diagnostic
  after old-code collection. No review findings remain open.
- Parent regenerated the artifact with the fresh compiled source manifest using
  source-enabled `portable_artifact_executes_without_source_compilation`, then ran
  it source-free (`--no-default-features`); both passed.
- Parent's final chained workspace `cargo clippy --workspace --all-targets -- -D warnings`,
  formatting, structure and diff checks all passed, including closure of the final
  conformance-test check. The earlier blank-line issue is resolved.
- The migrated publication conformance case queues the old body, then publishes at
  the host top level: 43 before publication, 100 from the queued old body calling
  the latest inner function, and 101 from a new current call. Stale candidate/native
  product rejection and fallback are covered. This single case passed across
  source/artifact input and interpreter/JIT routes, including actual native scalar
  prepared execution; it is not the full conformance/backend matrix.
- The documentation owner completed the 26-document durable contract migration,
  alongside conformance migration. URI identity and source-manifest integration
  failures are resolved; no fixture regeneration or source-review work is carried.

Earlier guard fixtures, staged-load visibility assertions, generic export lookup,
equal-URI source attachment, root cleanup and native/interface fixtures are repaired;
their failed intermediate runs are not current blockers. The anchored-manifest
handoff resolved missing `source_manifest` initializers and E0624 on the private
fingerprint accessor. The SDK owner then reported all 3 exact preflight tests passed
with `--no-default-features --features source --lib`, without further SDK repairs:
`resulting_source_size_preflight_counts_retained_replacements_and_additions`,
`source_text_copy_observes_cancellation_between_utf8_chunks`, and
`function_size_preflight_validates_utf8_and_overlap_before_building_text`.
Changed-file rustfmt and diff checks also passed. Earlier update tests were not
repeated; this follow-up did not run workspace Clippy or the full suite.

#### Final checkpoint validation

All final local checks are complete. The host-objects example now reuses the bound
`make` handle to observe the current result of 200 without rebinding, using a finite
Future provider. `async_facade::typed_members_and_current_reload_workflow` passed
(1 case). Parent's final check chain and the contract owner's final workspace Clippy,
structure, formatting and diff checks all passed; no carried errors remain.

The default-environment control exposed and resolved a correspondence identity bug:
merging semantically equivalent impls produced duplicate physical `SourceManifest`
IDs in comparison. The contract update comparison now projects semantic evidence
without carrying the physical manifest; the original manifest is fully validated
before comparison. Original executable anchors, canonical source/dependency evidence
and `Unknown` protection remain unchanged. Five correspondence tests, including
negative cases, passed.

With the original host example's default environment, unchanged recompilation/reload
and identical source replacement now pass. A real `hp: 100` to `hp: 200` body change
still rejects through protected `Unknown` dependencies, as intended; the default
protection test passed. This separates the repaired identity defect from the material
patchability limit above, rather than treating every rejection as intended.

Full-workspace tests and the complete feature/backend/platform matrix remain
CI-only and unrun. HM03 is next for unified relative leaf binding; state replacement
and MS remain unstarted. No performance or complete CI claim.

### UP01 implementation and integration

**Status: locally complete; committed as `cd79fc80`, no carried UP01 errors.**
HM01 is committed as `3523c4c8`. The compiler owner confirmed the final focused
case and 11 related tests pass on the repaired tree. HM02 is committed as `f0e1917d`;
its evidence stays in the [HM ledger](host-api-refactor.md#hm02-integration-and-up02hm03-handoff).
UP02 is locally complete with final local checks passed and no carried errors;
its evidence is in the ledger above. HM03 is next.
Full CI remains unrun.

Implemented scope:

- Frozen named declaration inventory, semantic impl identity/correspondence and
  directional public namespace graph additions; occurrence/placement identity is
  not declaration identity.
- Mandatory checked preoptimization canonical body, actual emitted body and source
  template evidence, including unused declarations, with bounded MIR/KBC codecs
  and independent validation.
- Protected static initializer expressions, published Hash/Eq and their transitive
  dependencies, regardless of live containers or initialized state. `Unknown`
  dependencies conservatively freeze installation inventory; no old-version call
  domain, eager initialization or automatic rehash is introduced.
- Monotonic retained-demand metadata, not executable leases. Runtime candidate
  staging isolates prospective history so rejected candidates do not advance the
  current history. Compiler expansion covers five queues: functions, shared,
  interfaces, constructions and layouts. Historical demands do not grant candidate
  proof authority.

Resolved integration repairs: backend-generated interface adapters are classified
accurately; native application validation checks every placement/witness variant,
not an arbitrary representative. Another placement with the same application
identity cannot substitute for an exact historical native import. Strengthened
named-bound forgery validation is repaired.

Local evidence reported by the implementation/validation owners (overlapping
selections are not totals):

- `cargo test -p kagari-contract --lib updates:: --no-fail-fast`: 24 passed.
- SDK `portable_artifact_executes_without_source_compilation` passed with source
  enabled and again source-free (`--no-default-features`); the fixture is fresh.
- Existing runtime `update_admission`: 6 passed; `named_inventory`: 3; VM static: 4.
- Compiler focused selection: 8 passed (earlier selection: 14); MIR updates: 5;
  KBC: 4; named bound: 4 after the bound-forgery repair; proof derivation: 7
  (earlier selected run: 22); examples: 3.
- Final compiler verification: 12 focused tests passed on the repaired tree:
  `shared_arithmetic_preserves_source_width_and_checks_callable_evidence` passed
  independently, plus 11 related numeric, retained-request, shared-binder,
  cross-module, generic, default and interface cases. No further code change was
  needed for this rerun.
- Latest agent-reported `cargo clippy --workspace --all-targets -- -D warnings`,
  structure, formatting and diff checks passed.

The former `MissingBinding("shared method declaration")` failure is resolved:
backend/contract adapter-classification repairs prevent generated adapters from
entering source shared-method requests. No current blocker remains.

Full-workspace tests and the complete feature/backend/platform matrix remain
CI-only and unrun. UP01 did not deliver live handles or dispatch. Subsequent HM02
work implements live Program/Module metadata and navigation with separate exact
`ExecutableModule` ownership and has passed local acceptance. UP02 now integrates
current dispatch/source Update with local acceptance complete; its final passing
checks are recorded above. HM03 next unifies relative
function/type leaf binding. State replacement and MS remain unstarted.

### Joint HM00/UP00 audit at 3b898e80

Historical status at the pause (superseded by the UP00 freeze below): incomplete.
The user resolved the audit's alias scope question
by explicitly prioritizing the transparent generic alias prerequisite before HM/UP.
Its completed semantics, review repairs and focused validation have one owner: the
[host API prerequisite ledger](host-api-refactor.md#transparent-generic-alias-prerequisite).
The prerequisite is locally complete; HM/UP remain paused and incomplete. No UP
implementation or completed joint design checkpoint is claimed; alias implementation
does not establish host type exports, layout roots or update support.

Audited source-update and publication boundaries:

- SDK Runtime currently owns Vm and execution defaults, not the engine's source
  context. PreparedProgram drops the source/environment association at lowering.
  Its Rc/RefCell owner cannot be retained wholesale inside movable Runtime.
- Capture an immutable, Send-compatible compilation-input capsule for source-prepared
  products: physical source bytes/module mappings, root/package/dependency aliases,
  declaration inputs and relevant compiler settings. Keep exact-source inspection
  separate from the stronger ability to recompile. No implicit editor/disk inputs.
  A fresh temporary compiler session can reconstruct those inputs for conservative
  whole-program rebuilding; do not retain a non-Send analysis session in Runtime.
- Existing analyze_source/analyze_package mutate the engine's source database before
  semantic success. They are not an atomic update transaction. Apply an Update to
  detached captured inputs and publish new authoritative inputs only with successful
  executable publication. A failed batch must not become the next batch's baseline.
- Function replacements resolve canonical declaration identity and validated syntax
  spans; reject aliases of the same target and overlapping physical edits, including
  a file plus its function or a containing/nested function pair. Preserve the original
  import/generic context and check the complete batch, not each intermediate edit.
- Stage source association/capsule storage and all fallible bookkeeping before
  committing code. Publish code and its matching source basis under one no-fail
  visibility transition; no initializer or user callback runs during commit. A
  prepared reload without matching source metadata must clear update capability,
  never reuse obsolete source input as if it matched the new executable.
- Current publish_staged_reload only rejects an executing candidate root, not all
  active synchronous script stacks. UP02 must enforce the confirmed host-top-level
  boundary, including rejection from reentrant callbacks, for all public paths.
- Ordinary PreparedFunction retains an exact module, target and lease today. Stable
  declaration/application selection must replace this for ordinary calls, while
  selected Future/closure/frame/type/native evidence keeps its necessary owners.
  Do not apply current lookup indiscriminately to initializer recipes, capture layouts
  or collection protocol relationships. Those need their own audited contracts.

Source query handoff: compiler-emitted associations must map exact executable
module/target and generated origins to retained document bytes, digest and checked
ranges, independently of optional stepping metadata. Current SourceOrigin contains
no text. Current-call queries capture current selection; historical and observed
frame/Future/closure queries use explicit exact-target evidence. Source records must
not retain executable leases or Runtime roots solely for history. Missing compiler
context, missing source and mismatched source must remain distinguishable failures.

State replacement remains UP03/UP04: an explicit root and bounded tree/DTO codec,
with global alias/cycle detection and precise resource rejection, are the bounded
starting direction. Exact schema/hook signatures remain to be frozen before those
phases. Existing DTO/Serde conversion is not a quiescent state snapshot. TaskScope
close currently cancels; it cannot implement the proposed default non-cancelling
drain. A later replacement gate must stop new admission, allow existing work to
drain, establish no script/host writers, then restore a fresh isolated Runtime and
commit host routing. Shared Rust callback captures are not automatically isolated
by creating a fresh Runtime. Do not claim rollback of completed host effects.

Primary reference checked: [Java SE 25 Instrumentation.redefineClasses](https://docs.oracle.com/en/java/javase/25/docs/api/java.instrument/java/lang/instrument/Instrumentation.html#redefineClasses(java.lang.instrument.ClassDefinition...))
retains active old method frames, selects redefined code on new invokes, preserves
static values and applies a batch atomically. Reuse those semantic boundaries, not
JVM concurrent redefinition, initialization or class-schema rules. Kagari additionally
needs suspended state-machine, generic application and exclusive host-driver safety.

Selected later test owners: SDK `source_preparation`, `source_snapshots`,
`package_inputs`, `native_artifacts`, async lifecycle/debugger tests; source database
capture/UTF-8 range tests; runtime execution-session candidate isolation; existing
Serde snapshot tests as codec foundations only. Add distinct batch failure/source
retention and non-cancelling drain cases at those owners. Current pinned expectations
are changed only with the UP02 semantic migration, not weakened in advance.

Historical audit validation is source/document inspection and the narrow existing
syntax test recorded in HM00 (1 passed); it is not alias prerequisite acceptance.
Completed prerequisite local acceptance and its evidence belong to the linked
ledger. No dispatch, source update or state replacement behavior has been implemented
or validated by that historical audit. Identity, compatibility and source-contract
closure on resumption is recorded in the UP00 freeze below. Full CI is unrun.

### Resumed UP00 audit at bbe30666

**UP00 design freeze: complete; joint HM00/UP00 committed as `bbfc0af6`.**

On 2026-10-24 the user reauthorized the full phased sequence and resolved the last
semantic blocker: no pinned-call domain; reject changes to published key Hash/Eq,
static initializer expressions and their transitive dependencies regardless of live
containers or initialized cells. Unknown dependencies conservatively prevent unproved
changes. Keep lazy initialization and author responsibility for mutable/external state.
The preceding frozen representation, evidence limits, source/batch/query APIs, native
timing and publication protocol close UP00. The joint HM00/UP00 design checkpoint
was committed as `bbfc0af6` with both Plan-Phase trailers. This historical design
audit does not establish implementation acceptance; current UP01 status is recorded
in the [implementation ledger](#up01-implementation-and-integration).

**Audit and ownership evidence:**

| Inspected owner | Finding and migration responsibility |
| --- | --- |
| [ABI collection](../crates/kagari-compiler/src/source/lower/abi.rs), [reload](../crates/kagari-runtime/src/reload.rs), [exports](../crates/kagari-runtime/src/reload/exports.rs) | Private/unused inventory is incomplete; exact module/list equality blocks additions. UP01 adds preservation checks, protected records and distinct function/type alias rules. |
| [HIR declarations](../crates/kagari-hir/src/declarations.rs), [instance planning](../crates/kagari-compiler/src/source/lower/instances.rs), [local function values](../crates/kagari-compiler/src/source/lower/expr/local_functions.rs) | Impl/local occurrences and generated placement are not stable identity; empty-capture MakeClosure conflates named values with closures. UP01 supplies correspondence/classification and bounded retained demands. |
| [Construction evidence](../crates/kagari-contract/src/callable/construction.rs), [source-free verification](../crates/kagari-bytecode/src/program/construction.rs) | Existing bounded substitution/witness traversal is the shared foundation, not proof of retained-old-body coverage. Extend it in UP01. |
| [Prepared calls](../crates/kagari-runtime/src/module/execution/calls.rs), [frame calls](../crates/kagari-runtime/src/frame/calls.rs), [method views](../crates/kagari-runtime/src/objects/method_view.rs) | Local slots and receiver witnesses select retained bodies today. UP02 selects current implementations without replacing relationship/layout authority. |
| [Host handles](../crates/kagari-runtime/src/native/function_handle/mod.rs), [closures](../crates/kagari-runtime/src/closure.rs), [Futures](../crates/kagari-runtime/src/native/future.rs) | Split ordinary binding from exact selected body; preserve captures, cold producers, resume state and cleanup in UP02. |
| [Stored selections](../crates/kagari-runtime/src/native/stored_selection.rs), [Hash/Eq callbacks](../crates/kagari-runtime/src/native/collections/hash.rs), [static frames](../crates/kagari-runtime/src/frame/statics.rs) | Preserve storage/relationship and initializer authority; protected dependencies make current ordinary calls safe without an execution exception. |
| [Publication](../crates/kagari-runtime/src/loading.rs), [staging](../crates/kagari-runtime/src/module/staging.rs), [session admission](../crates/kagari-runtime/src/lib.rs) | Candidate-root-only rejection is insufficient; UP02 counts synchronous scopes, including hidden nested sessions, and commits one prepared publication. |
| [Source APIs](../crates/kagari/src/engine/source.rs), [package capture](../crates/kagari/src/engine/packages.rs), [preparation](../crates/kagari/src/program/mod.rs) | Captured bytes/fingerprints do not reconstruct the environment, and PreparedProgram drops source ownership. UP02 captures immutable declaration/settings inputs without Rc/RefCell in Runtime. |
| [Native preparation](../crates/kagari/src/program/native.rs), [VM native entry](../crates/kagari-vm/src/vm/native.rs) | Existing explicit preparation/exact correspondence remains; UP02 indexes products after current target selection, with no hidden lazy backend. |

**Finite checkpoints:**

- UP01 owns contract records/wire/reference mapping; HIR authored correspondence;
  compiler ABI/instance/closure/future evidence; MIR/bytecode codecs and verification;
  runtime reload/export/cache compatibility and protected/retained coverage. Include
  alias evidence from HM01 and existing static schema checks. Update tests expecting
  protected-body/initializer edits to succeed in this checkpoint; preserve their
  storage/cleanup coverage using unchanged protected definitions and add rejection
  assertions. Dispatch stays pinned and buildable until UP02; no compatibility
  reader, routine ABI bump or stubs.
- HM02 supplies the one installation cell after UP01; UP02 integrates its current
  target table with frame/host/interface/witness/native calls, retained GC metadata,
  session/driver admission and static/native collection owners. SDK Runtime and
  engine/source/preparation own Update, capsule and exact source orchestration.
  Migrate affected VM/native/SDK/CLI consumers, specifications and pinned expectations
  together; no second public semantic mode. HM03 follows this migration.
- UP03 starts with its explicitly recorded codec/schema/hook/limit gate. UP04 starts
  with the drain/cutover/resource/commit API gate. Neither is claimed frozen or
  implemented by UP00, and neither blocks compatible body-update implementation.

**Selected behavioral owners and cases:**

- Compiler ABI/instance and MIR/bytecode program/artifact tests: private/unused
  declaration changes, impl reorder, bounded/ambiguous identity correspondence,
  legal additions and forbidden old-type impls; forged/missing target, witness,
  protected-edge and historical demand records. Exercise source-free decoding.
- SDK `runtime/tests/embedding/generic_reload.rs`: old named handles and interfaces
  call current methods; old captures keep layout; generic applications needed only
  by old bodies remain present; same-signature alias retarget affects fresh lookup
  only. Migrate Hash/Eq-body-success cases to rejection with UP01 validation; migrate
  remaining ordinary pinned-call expectations only with UP02 dispatch.
- VM `tests/execution/statics.rs` plus SDK generic/static tests: unchanged lazy
  initialization succeeds; edits to initializer expressions, key methods, direct/
  transitive/generic helpers or resolved aliases reject with no state changes,
  including never-created/empty containers and cold/Ready/Failed statics. Unknown
  dynamic/native callback evidence rejects unproved changes. Add a non-protected
  helper control case; no eager initialization or automatic rehash is observed.
- SDK async lifecycle/group/static tests: old unpolled Futures and suspended nested
  frames retain bodies while subsequent direct/recursive/generic/method calls use
  current code; publication neither polls nor cancels them. Preserve all cleanup.
- Runtime execution-session tests: callback/native/candidate-hidden reentry rejects;
  parked work allows publication; stale bases and foreign installations reject.
  Inject each fallible precommit failure and check code/statics/source isolation.
- SDK source_preparation/source_snapshots/package_inputs: detached multi-edit batches,
  method/alias duplicate targets, overlap and CRLF/Unicode ranges, failed baseline
  isolation, missing capsule and exact generated/native/historical source queries.
- SDK native_artifacts/native_preparation and VM native tests: actual native execution
  for supported current targets, stale product rejection, honest pre-entry fallback,
  failed preparation isolation and eventual executable reclamation. General calls/
  suspension remain interpreter coverage; no native expansion or AOT patch delivery.

Primary references rechecked during this audit: [Java SE 25 redefinition](https://docs.oracle.com/en/java/javase/25/docs/api/java.instrument/java/lang/instrument/Instrumentation.html#redefineClasses(java.lang.instrument.ClassDefinition...))
(retained active frames/new invokes/preserved statics/atomic batches),
[Erlang/OTP code replacement](https://www.erlang.org/doc/system/code_loading.html#code-replacement)
(retained bodies versus current targets; living documentation reviewed 2026-10-24),
and [Rust monomorphization collection](https://rustc-dev-guide.rust-lang.org/backend/monomorph.html)
(explicit concrete-instance roots; living documentation reviewed 2026-10-24).
Kagari additionally roots retained-body demands, retains checked shared evidence,
protects storage-sensitive code and publishes at an exclusive host boundary. No
JVM concurrency, BEAM local-call exception/two-version purge or speedup is inferred.

Validation for this documentation checkpoint: source/API inspection and document
content/link/diff checks only; no Rust builds/tests or complete CI acceptance claimed.
Implementation phases run affected contract tests and required local lightweight/
workspace Clippy checks; full workspace/backend/platform testing remains CI-only.
