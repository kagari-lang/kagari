# Host Program, Module and public-path execution plan

## Status and authority

Status: implementation and phased commits authorized on 2026-10-23. The user
subsequently prioritized implementing transparent generic type aliases before HM/UP.
That finite language prerequisite is locally complete. Its sole progress owner is
the [alias prerequisite ledger](#transparent-generic-alias-prerequisite); exhaustive
CI acceptance remains pending.
The user reauthorized the complete phased sequence on 2026-10-24. The HM-side
read-only implementation audit resumed at `bbe30666`; its resolved design and
handoff are recorded below. HM00 design acceptance is complete. The user resolved
UP00's transitive-selection gate by freezing published key Hash/Eq, static
initializer expressions and their transitive dependencies during compatible update.
The [UP plan](update-model-design.md#resumed-up00-audit-at-bbe30666) owns that policy
and its implementation. The joint HM00+UP00 design checkpoint was committed as
`bbfc0af6`. HM01 is locally complete, including the private-type evidence repair
and post-repair validation; no carried errors remain. Next is UP01. Full CI remains
pending. HM02-HM04 and UP implementation remain
unstarted; MS remains unstarted. The public API still uses `LoadedModule` and pinned
calls until HM02/UP02; HM01 delivers source-free type exports, not live dispatch.
Originally requested against `67d9e36f`, this document owns HM00-HM04, decisions,
acceptance and progress for the [Host Program and module lookup finding](review.md#host-program-and-module-lookup).

This replaces the former broad HA00-HA05 proposal in this file. On 2026-10-23 the
user confirmed ordinary calls through stable callable identities to current code,
replacing the former pinned-dependency target, and confirmed that ordinary Program
and Module handles always query the latest published contents of their own
installation. Neither lookup nor ordinary invocation requires reacquiring these
handles or rebinding functions after update. This supersedes this plan's former
immutable version-view public model. It is a target design, not current runtime
behavior. The [update model](update-model-design.md) owns the UP dispatch migration;
HM00 settles representation and the identity/lifetime sequencing handoff below;
it does not reopen these public semantics or expand HM01-HM04 beyond namespace/
type-path work.

Remaining preparation/tooling proposals are retained under
[Deferred work](#deferred-work), not as a second phase queue.
Existing specifications describe the pinned implementation and remain authoritative
until migration updates the affected contracts.

Reuse the implemented [typed invocation/object facade](spec/embedding-api.md),
[loading and activation](spec/module-activation.md), [package contracts](spec/packages.md),
[persistent state](spec/modules.md#persistent-module-statics) and
[identity ownership](architecture.md#definition-identity-ownership).
The [roadmap](implementation-roadmap.md) owns activation. No new VM, call API,
registration system or independently maintained progress document is required.

## Outcome and finite scope

A host loads a complete Program, obtains module handles through public paths,
and binds functions and object types relative to the supplied module. Ordinary
Program and Module handles identify a stable installation and a member within it;
every query uses that installation's latest published contents. An ordinary callable
binding represents a stable declaration in that installation and selects its latest
implementation on invocation. Update/reload publishes to the same installation;
existing Program, Module and function handles remain usable without reacquisition
or rebinding. Internal immutable versions and retained computation bodies are not
ordinary public metadata snapshots. HM00 gates representation and the transition to
UP, not these confirmed semantics; the current runtime does not yet provide this
complete contract.

Required outcomes:

- Separate the prepared product, a stable installed Program and a live module handle.
- Make ordinary Program/Module queries use their installation's latest published
  contents; reserve exact-version inspection for explicit UP/debug APIs.
- Unify public module/function/object-type paths, including aliases and re-exports.
- Carry checked type export facts through source-free artifacts and verification.
- Include type paths in staged reload compatibility before publishing candidates.
- Migrate affected consumers without compatibility aliases or root-reset fallback.
- Preserve typed invocation, roots, static state, cancellation and runtime/type
  safety; distinguish these invariants from the dispatch policy UP will replace.

Non-goals for HM01-HM04: the full current-implementation dispatch migration,
cross-installation repair-program linking, new generic specialization at runtime,
arbitrary type reflection, R13 code/type retention separation, SA21 call optimization,
artifact pruning, new unload semantics, compiler preparation redesign and native
coverage expansion. The [multi-Runtime sharing plan](multi-runtime-sharing-plan.md)
separately owns shared frozen products, identity-admission copying and per-player
installation costs; HM00/MS00 must coordinate overlapping identity owners without
absorbing that migration into HM. HM00's UP handoff is required design work, not
authorization to implement these mechanisms or a concurrent publication system in
HM phases.

## Evidence and ownership inventory

Initial inventory at `67d9e36f`, rechecked for this HM00 audit at `bbe30666`.
No behavioral reproduction or performance measurement was run for this docs-only
audit. The decisions below distinguish observed implementation from required work.

| Owner | Current evidence | Required change |
| --- | --- | --- |
| [SDK runtime](../crates/kagari/src/runtime.rs) | `load_program` and `reload_program` return `LoadedModule`; reload accepts a module although it replaces a Program | Load returns Program; reload targets its stable installation without requiring a replacement handle |
| [Runtime module ownership](../crates/kagari-runtime/src/module.rs) | `LoadedModule` holds shared `ProgramDescriptor` plus `ModuleRef` | Review owner representation for live Program/Module capabilities; distinguish internal versions from public handles |
| [Runtime exports](../crates/kagari-runtime/src/module/exports.rs) | Function paths always start at Program root, even on a child handle | Make module binding relative; retain explicit Program-root entry execution |
| [Object binding](../crates/kagari-runtime/src/native/objects/cache.rs) | `bind_type` constructs a struct declaration path from the supplied module plus one name | Consume checked exported type targets, then reuse layout admission/cache |
| [HIR exports](../crates/kagari-hir/src/program/exports.rs) | Only public module edges and concrete script-function leaves are collected | Collect the selected public type/alias facts from existing checked resolution |
| [Compiler exports](../crates/kagari-compiler/src/source/program/exports.rs) | Source references lower to scoped definition identities | Lower type facts once; retain no HIR references downstream |
| [Contract exports](../crates/kagari-contract/src/exports.rs) | Graph shape, bounds, reachability and function authority are checked | Extend type contracts, mapping and bounded codecs coherently |
| [Bytecode exports](../crates/kagari-bytecode/src/program/exports.rs) | Verified function declarations bind to executable slots | Validate exported type evidence independently of source |
| [Reload exports](../crates/kagari-runtime/src/reload/exports.rs) | Compares module/function path languages and the separate entry, including cycles | Compare normalized type contracts without flattening paths |

The existing source-to-contract boundary is sound. The defects are incomplete
public type metadata and inconsistent handle/path responsibilities, not a reason
to move source resolution into runtime or introduce another namespace cache.

## Proposed host contract

### Program and module responsibilities

The user confirmed `PreparedProgram` -> `Program` -> `Module` naming on 2026-10-23.
`Program` and `Module` are shared live handles, not copies of executable code,
business state or public immutable metadata snapshots. Each query reads the latest
published contents of the handle's own installation, never a different installation
with the same name. Existing handles observe successful publication without being
reacquired. Internal immutable versions may support checked lookup and execution,
but exact-version inspection requires an explicit UP/debug API, not an ordinary
Program/Module handle. Renaming alone implements neither live lookup nor stable
callable dispatch. Current implementation names in the evidence inventory remain
unchanged until HM02.

| Object | Meaning | Lifetime and authority |
| --- | --- | --- |
| `PreparedProgram` | Verified preparation product | Keep its name and current preparation/thread-transfer rules |
| `Program` | One stable installation in a Runtime | Queries that installation's latest published contents; remains usable across compatible update/reload |
| `Module` | One checked module identity in that installation | Shares the installation owner; queries its latest published contents without reacquisition |
| Ordinary function/method binding (target) | Checked stable declaration in one installation | Selects the latest implementation on invocation without rebinding; retains runtime/contract authority, not the body present when bound |
| Internal immutable version | Checked metadata/code for a concrete publication | Supports internal validation and retained execution; not the ordinary public Program/Module model |
| Selected computation, type/layout and native evidence | Distinct retained execution or validation products | Body/resume/capture lifetime, type authority and native validity are separate from live lookup and ordinary callable identity; representation is frozen by HM00 below |
| Explicit exact-version inspection (UP/debug) | A deliberately selected historical version or observed computation target | Separate diagnostic authority and retention; an ordinary Program/Module is not an exact-version snapshot |

`load_program` returns `Program`. `reload_program` accepts that stable Program
installation as its target and publishes the validated replacement there; it does
not require a replacement Program handle. UP's source update targets the same
stable installation. The confirmed success result for both operations is `()`
(`Result<(), E>` using the appropriate existing error family), not a replacement
handle or mandatory update report. A child module must not be accepted
accidentally as a whole-program reload target.
Keep compiled identities distinct from optional installation display labels;
rename misleading option fields only where their Program responsibility requires
it, with direct consumer migration rather than compatibility fields.

Program exposes `root()` and `module(path)`; Module exposes `module(path)`.
`root()` is the explicit zero-segment operation; empty strings are not alternate
root syntax. `module(path)` is fallible public lookup in the latest published
contents of the supplying installation, not code acquisition or version selection.
Function/type binding continues through Runtime with a `Module`, not a new
polymorphic owner trait or duplicate Program-specific binding methods.

Target usage sketch, not currently callable code:

```rust,ignore
let program: Program = runtime.load_program(&prepared, Default::default())?;
let root: Module = program.root();
let users: Module = program.module("users")?;
let profiles = users.module("profiles")?;
let same_profiles = program.module("users::profiles")?;

let find = runtime.function::<(i32,), Object>(&users, "find_user")?;
let user_type = runtime.object_type(&users, "User")?;
let same_type = runtime.object_type(&root, "users::User")?;
let user = runtime.call(&find, (42,))?;

runtime.reload_program(&program, &replacement, Default::default())?;
// Confirmed target, not implemented by the current pinned runtime:
// Reuse the original handles; both queries read the latest published contents.
let profiles_after_update = users.module("profiles")?;
let same_profiles_after_update = program.module("users::profiles")?;
let updated_user = runtime.call(&find, (42,))?; // Latest body; no rebinding.
```

The [frozen HM00 representation](#hm00-frozen-installation-representation) separates
stable installation/module/callable identities and live public lookup from internal
immutable versions, selected body/resume/capture lifetime, type/layout authority
and native-product validity.
The current owner model uses explicit version leases for prepared functions, types,
active calls and native products; descriptor sharing alone is not an execution lease.
Do not assume these leases already implement the target or preserve their coupling
as a final requirement. Pure lookup reads checked metadata; binding/execution must
still validate runtime/installation identity, generations where applicable, type
contracts and admission. A retained live handle must not resurrect a retired
installation or admit stale native code. Runtime owns the installation; Program
and Module are cloneable access handles. Dropping these handles does not unload
the installation, cancel tasks or clear static state. Independently retained
callable handles remain usable while the installation is live. Runtime close/drop
ends execution and runtime-object access authority even if handles remain; exact
metadata retention does not restore that authority. Explicit unload is separate
work, not an implicit last-handle-drop operation.

The exact representation, admission and UP migration ordering are frozen below. Use one ordinary public binding surface, not
parallel legacy-pinned and current-code entrypoints. Runtime remains Send and not
Sync; metadata synchronization must not make the heap concurrently accessible.

### Relative paths and visibility

- Program lookup starts at its root; Module lookup starts at the supplied module.
  Both query the latest published contents of their own installation, including
  through handles acquired before update. Sync and async function binding use that
  same start, with no fallback to root.
- Paths are nonempty public name segments separated by `::`, bounded by the
  existing path limit. Reject empty components, leading/trailing separators and
  unsupported syntax. Do not introduce `crate`/`self`/`super` navigation, filesystem
  paths, generic-expression parsing or implicit package search through this API.
- Resolve through the checked public graph, not canonical path concatenation.
  Module and function namespaces retain existing language collision rules; type
  entries must respect the source type/module namespace rather than permit new
  ambiguities merely because storage uses separate maps.
- Preserve alias cycles as bounded graph edges. Do not flatten all possible paths.
- A public item re-export from a private module grants only the item path. For
  `pub use internal::User as Player`, `Player` may bind while `internal` remains
  inaccessible through public module lookup.
- Function paths identify checked declarations in the supplying installation's
  latest published namespace. Target ordinary invocation uses the latest
  implementation, not the body present at binding time. Binding resolves the path
  once to a stable declaration/application identity; invocation does not resolve
  that original string again or silently retarget to another declaration through
  a changed alias. Existing callable signatures and nominal type identities remain
  fixed under compatible publication. Associated methods
  and explicit declaration/application binding remain distinct checked capabilities,
  not fallback searches or alternate legacy dispatch surfaces.
- `members()`/slot access remain internal or explicitly advanced facilities where
  concrete consumers need them. Ordinary examples must not scan internal IDs.
  A function/type binding must not expose its private owner as a new public module
  navigation capability. Audit existing diagnostic handles separately; do not
  widen their authority as an accidental part of relative lookup.

Reuse one contract-owned path traversal for executable consumers. Source checking
still owns source visibility/name resolution. Missing/private paths can share a
not-public-path diagnostic; invalid paths, wrong target kind, unsupported object
applications and foreign handles should retain useful existing error categories.
Do not add a workspace-wide error redesign.

### Type export scope and alias evidence

Required public object paths include directly exported script structs, renamed
`pub use` exports and closed transparent aliases resolving to a script struct,
including closed generic applications when existing executable layout evidence
supports them. Keep the SDK `object_type` zero-explicit-argument interface; preserve
the core binder's existing explicit `TypeArgument` route for generic declarations.
This finite scope is confirmed: ordinary public structs, renamed exports and
supported closed struct aliases use `object_type`; open generic declarations keep
the existing advanced explicit-type-argument route. Strings such as
`Container<Vec<User>>` are not parsed as type expressions. No new host-side
type-expression parser or specialization engine is included.

The [frozen type export record](#hm00-frozen-type-export-record) retains the
canonical target, applied arguments and necessary binder/visibility evidence.
A declaration ID suffices for a renamed declaration, not an applied alias.
Normalization uses existing checked type semantics; runtime never expands syntax
or infers types.

New host binding of open generic aliases is deferred; this does not defer source
language generic aliases in the authorized prerequisite. HM00 inventories existing
advanced application support and records precise rejection diagnostics, without
expanding the confirmed public host scope or omitting a required closed alias.
Non-object aliases and enum/trait/scalar/opaque exports must not become ObjectType
handles. Their valid source declarations must still compile; this plan does not
require a general host type-reflection API. Record enough classification to reject
unsupported object lookup consistently, without creating a second type model.

An exported type identity alone does not prove a usable executable layout. Check
existing layout preparation/materialization rules, including an otherwise-unused
public struct and a closed alias of a generic struct. Preserve current generic
layout capability; if a supported exported closed application needs preparation,
make it an explicit compiler-owned demand root, not a runtime compilation fallback.
Unsupported applications fail before allocation or script execution.

Extend all affected identity traversals, MIR/bytecode codecs, bounds checks and
verification owners together. Source-free validation must reject wrong declaration
kinds, dangling/out-of-scope IDs, malformed arguments/binders, incompatible layout
claims and non-public targets using retained public contracts. Alias metadata may
not grant unchecked private-field/member access. A graph is verified against the
artifact's declared authority, not authenticated against unavailable original source.

Keep executable runtime dependencies source-free. Batch affected development
fixture regeneration at coherent checkpoints; retain meaningful decoded-artifact
coverage. No routine format/runtime ABI bump or legacy reader is required for this
unpublished schema change.

### Reload, entry execution and observable behavior

Extend the existing graph comparison to type leaves in the same change that starts
emitting them. Normalize identities into the existing common comparison context;
never compare unrelated numeric IDs or accept two nominal types merely because
their fields match. Preserve current module/function compatibility rules rather
than using this work to loosen or tighten all reload policy.

The same public object path must preserve its normalized nominal target and applied
type contract. An equivalent alias spelling may change without changing that
contract; redirecting the path to a different nominal type is incompatible. Keep
existing layout/member/static compatibility validation and bounded cycle handling.
Failed or incompatible publication leaves the previous published state intact,
so existing Program/Module queries and ordinary calls remain unchanged. Successful
publication makes the latest contents visible through those same handles and the
latest implementations callable through existing function bindings. These are
target semantics, not claims about the current runtime. Retain applicable internal
stale-candidate validation without making public handles version-specific or
requiring concurrent publication.
Neither lookup nor publication executes script/code initialization or host handlers.

`ProgramEntry::Main` stays the separate root-only capability, including a private
zero-argument root main. Program-level `execute`/`execute_prepared` and native/CLI
entry preparation must use an explicit Program/root context. Module-relative host
binding must not silently change their root-entry behavior. Migrate affected
signatures and callers coherently; do not add overloads that guess intent from the
supplied member slot.

Current implementation: ordinary bindings, calls and retained executable values
use pinned versions/dependencies. HM namespace changes must not claim to have
migrated this behavior. Until UP changes the relevant paths and specifications,
existing pinned tests remain regression evidence, not the final dispatch target.

Confirmed target, coordinated through HM00; UP owns dispatch/publication migration:

- Ordinary Program and Module handles always query the latest published contents
  of their own installation. Update/reload targets that stable installation, not
  a public version view. No Program/Module reacquisition is needed; exact-version
  inspection is a separate explicit UP/debug capability.
- Runtime publication is serialized at a single-threaded Actor-style host top-level
  boundary, with no synchronous script stack executing and no publication from a
  reentrant host callback. Async tasks may be suspended; no concurrent publication
  mechanism is required.
- Every ordinary function/method call selects the current implementation through
  stable installation/declaration identity, including calls made by an old resumed
  body. Ordinary host function handles do not require rebinding after publication.
- An active or suspended computation keeps its selected body/resume state. A
  precreated Future keeps its selected computation. A closure retains the body
  matching its captured layout. Ordinary calls inside each use current code.
- Existing type identity and trait implementation relationships stay fixed; those
  facts do not pin method bodies. Ordinary trait method calls select the current
  implementation within the unchanged checked relationship.

For example, after publication of v2, the original `program` and `users` handles
query v2's published contents and a previously bound host `find_user` handle calls
v2 without reacquisition or rebinding. A suspended v1 handler still resumes its v1
state machine, but its next ordinary `find_user` call uses v2. Keeping that handler's
executable state safe is different from freezing public lookup or every callee at
v1. Inspecting the handler's v1 body requires explicit selected-target evidence in
UP/debug APIs, not treating `program` or `users` as historical snapshots.

Preserve persistent static state, outer-tuple arguments, preflight before effects,
rooted outputs, runtime/type/borrow checks, cancellation, cleanup and once-only
evaluation. UP owns the dispatch and publication migration; HM owns namespace/type
paths and the explicit HM00 handoff, not a second execution semantic implementation.

### HM00 frozen type export record

Decision at `bbe30666`: add `types: BTreeMap<String, TypeExport>` to the existing
contract `ExportNamespace`. Use the following semantic sum; names belong to the
map, not to duplicate leaf fields. HM01 implements this semantic schema; the
lasting artifact contract is [public type exports](spec/artifacts.md#public-type-exports):

```text
TypeExport = Declaration(DefinitionId)
           | Closed(Ty)
           | OpenAlias { arity: u32 }
```

- `Declaration` is the canonical public struct/enum/trait/opaque/native type
  declaration, including a `pub use` rename. Kind, generic parameters, bounds and
  visibility come from its owning retained `PublicItem::Type`/`Trait` and native
  contracts. Do not duplicate them in the leaf or infer them from a name alone.
- `Closed` is an alias with **zero declared alias parameters**, carrying its fully
  normalized existing `kagari_types::ty::Ty`. `Ty::Struct(NominalTy)` already carries
  the nominal declaration, ordered applied arguments and associated bindings; no
  separate target ID/argument vector, alias ID, source spelling or layout slot is
  needed. A chain of closed aliases erases completely. A closed alias of an open
  alias, such as `type IntCell = Identity<Cell<i32>>`, follows this branch after
  HIR checks/substitutes all alias inputs and bounds.
- `OpenAlias` records only a positive, bounded declared arity. It is an explicit
  **rejection-only** public name: it grants no type, layout, binder or application
  authority, even to the advanced binder. Do not serialize an unused alias template,
  fake nominal identity or source-only bounds just to reject a host operation.
  `type Erased<T> = User` remains open despite its concrete target. This distinction
  must use `CanonicalAlias.parameters`, not `target.is_concrete()`.

This is the smallest HM record because supported identity/application information
already has owners, and an unsupported open alias cannot exercise its template.
The artifact declares that rejection-only name/arity; source-free validation does
not authenticate its original source declaration. Changing its erased target or
bounds is not checked by this marker: UP01's frozen source/declaration inventory
must own that restriction, not treat the HM export graph as a complete language
ABI. UP01 must carry that inventory as verified portable metadata for prepared/
source-free reload too; absence of source is not permission to skip preservation
checks. Open host alias application remains deferred. All closed non-object aliases
retain `Closed(Ty)`, so their nominal target/application can still participate in
reload comparison without becoming a reflection API.

**Checked source and lowering.**
[CanonicalAlias](../crates/kagari-hir/src/imports/types/aliases.rs) already retains
`parameters`, expanded `target`, checked `bounds` and a recovery `error`. HIR export
collection selects only checked public bindings with the existing `public_target`
visibility/ambiguity rules. Direct declarations/re-exports retain canonical source
targets; zero-parameter aliases lower their checked target through the existing
semantic type encoder; parameterized aliases emit the rejection marker. Recovery
errors never reach codegen. The source checker continues enforcing every alias
bound, including erased arguments; neither runtime nor the contract codec repeats
source name resolution. Type and module names occupy the same source namespace:
reject a type/module spelling collision in decoded graphs too. Functions remain
an independent namespace.

**Source-free authority and binder proof.** HM01 extends the contract/MIR/bytecode
export validators; decoding alone grants no authority:

1. Validate graph reachability, duplicate names, total export limits and identity
   scope as today. Count type leaves in `MAX_EXPORT_RECORDS`; bound marker arity
   with `kagari_types::decode_limits::MAX_NESTED_RECORDS` (4096).
   `OpenAlias { arity: 0 }` is invalid.
   Include every ID inside `Closed(Ty)` in reference visitation, remapping and
   canonical encoding, using the existing bounded type wire owner. Reject unknown
   tags, dangling/foreign IDs and type/module collisions before runtime adoption.
2. A `Declaration` ID must be a top-level occurrence-zero type/trait of the exact
   kind recorded in its defining executable module's public contract. A table entry
   alone is not a declaration/publicity proof. A private containing module is fine;
   an absent/private nominal declaration is not. Native/opaque declarations stay
   their existing kind and never masquerade as script structs. A nongeneric script
   struct declaration also requires the matching verified zero-argument layout,
   with the same field/authority checks as a closed struct alias in step 4.
3. For an open nominal declaration, derive binders from that **exact** public
   declaration: contiguous ordered positions `0..N`, all owned by its declaration
   ID. Validate the declaration/bounds with existing declaration/application owners.
   Any candidate layout template must use those exact owner/position binders and
   checked substitution, not merely the same count or a set of arbitrary parameters
   accepted by `types_in_scope`. Concrete layouts must match canonical declaration
   plus all ordered arguments. Reuse linked proof checks for required bounds and
   field/layout agreement. An unrelated function's generic environment cannot prove
   a public struct application. HM01 adds this export proof around existing helpers
   with graph-local indexes, bounded work and cancellation.
4. A `Closed` target has an empty lexical parameter environment and must pass both
   bounded type/declaration validation and concreteness checks. Reject parameters,
   `SelfType`, unresolved projections, wrong nominal kinds/arities and invalid
   associated bindings anywhere in the tree. Verify each nominal target/argument
   against retained public declaration authority and linked application rules.
   A closed struct requires matching verified executable layout evidence; a forged
   missing-layout or mismatched-layout claim fails artifact verification, not a
   later allocation. Validate required nominal bounds after substitution against
   linked proof evidence; well-shaped types and matching arity alone are insufficient.
   Field/method access still uses its existing public contracts. Private field types
   may occur inside a verified layout; retaining them does not export those types
   or grant private-field access.
5. `OpenAlias` has no binder scope to prove and can never be upgraded to an object
   application by passing host arguments. Substituting `Closed` for a marker cannot
   bypass validation: the new target must independently pass all closed public and
   layout checks. As with current export edges, verification checks declared
   artifact authority, not authenticity against unavailable source.

Classification is **derived after verification**, not a serialized trusted flag:

| Verified leaf | SDK `object_type` / core `bind_type` |
| --- | --- |
| Nongeneric public script struct declaration | Bind with no arguments; require executable layout |
| Generic public script struct declaration | SDK rejects missing arguments; core accepts exact explicit `TypeArgument` arity only when existing checked layout/template evidence admits it |
| Closed alias to a script struct | Bind its canonical application with no host arguments, including supported generic applications; reject extra arguments |
| Enum, trait, scalar, tuple/collection/function/reference, native object or opaque/host target | Report non-script-object type; never produce `ObjectType` |
| Open alias, including erased generic aliases | Report unsupported open generic alias, irrespective of supplied arguments |

All `Declaration` and `Closed` leaves are metadata-reachability roots, including
unsupported non-object types. Retain their defining public contracts and transitive
application/proof dependencies even when no body references them and the containing
module has no public navigation edge. This is contract retention, not permission to
add a private namespace to the export graph. Decoding/verification cannot consult
HIR or reconstruct a missing declaration from a type name. Open-alias markers need
no layout or template; UP01's separate frozen inventory remains mandatory.

**Layout demand is compiler-owned.**
[Layout collection](../crates/kagari-compiler/src/source/lower/layouts.rs) already
seeds unused nongeneric structs; generic layouts come from signatures, native/
interface requirements and reachable expression roots. HM01 adds each distinct
closed exported script-struct application as an explicit root before layout
collection, routed to its supplying source module through Program lowering. Nested
struct applications and enum payload layouts needed by structural bound proofs
also contribute roots, including inside non-object aliases; that evidence does
not grant ObjectType authority to enums or other non-object types.
Collect these demands before per-module layout queues are finalized, including
aliases exported by one module whose canonical target is in another; propagate
transitive field/application demands through the existing bounded work queue.
Deduplicate by canonical type and order roots by portable definition/type order,
not process-local IDs or alias spelling. Reuse the existing transitive field/layout
queue, cancellation and instantiation budgets. A supported alias must work even
when no script expression constructs it. Do not demand every specialization of an
open declaration, generate executable bodies merely for lookup, or turn unsupported
non-object/open aliases into compilation errors. Existing advanced generic binding
remains conditional on carried executable evidence, not a promise to synthesize it.

For example, `pub type IntCell = hidden::Cell<i32>` emits `Closed(Struct(Cell,[i32]))`,
not a nominal `IntCell`. The compiler requests that application once, MIR and KBC
prove it, and binding uses the supplying installation's verified layout. Replacing
its source spelling with another alias of `Cell<i32>` preserves the contract;
`Cell<bool>` or a structurally identical `OtherCell<i32>` does not. The private
`hidden` module remains un-navigable and private fields remain inaccessible.

**Reload.** HM01 adds type labels to the existing bounded cyclic graph equivalence
check and imports identities/types into its comparison context. Compare supported
leaves by normalized semantic target/application, so a nongeneric declaration and
an equivalent closed alias compare equal; open nominal declarations retain canonical
owner/binder contract, non-object closed aliases retain their canonical `Ty`, and
open-alias markers compare rejection class plus arity. Preserve separate entry and
existing function-signature comparison. Do not compare serialized tags, raw numeric
IDs, source alias spelling or field shape as nominal identity. Existing member/
static checks still apply; UP01 owns additions and the broader frozen inventory.

### HM00 frozen installation representation

Decision: retain the current immutable `ProgramDescriptor` as a **publication**,
not as a public handle. Rename the existing descriptor-plus-`ModuleRef` role to
`ExecutableModule` and migrate its actual execution/layout/native consumers; do
not mechanically turn those consumers into live `Module` lookups. Where cross-crate
backend APIs need this exact-version type, keep that narrow advanced surface and
review its re-export entries; it is not an ordinary module navigation capability.

Use one Runtime-owned installation record and one shared metadata cell:

```text
ModuleStore.installations[InstallationId] -> Arc<Installation>
Installation = immutable { registry_owner, id, root_identity }
             + Mutex<Option<Arc<ProgramDescriptor>>> current
Program = Arc<Installation>
Module  = { installation: Arc<Installation>, namespace: ModuleIdentity }
ExecutableModule = { publication: Arc<ProgramDescriptor>, slot: ModuleRef }
```

`None` means closed/retired, never an invitation to find another installation.
Use a checked monotonic Runtime-local `InstallationId`, paired with `HostRegistryId`
for cross-Runtime authority; do not recycle IDs. Every load allocates a fresh ID,
including equal prepared products and equal display labels. Key member allocation
by `(InstallationId, ModuleIdentity)` and current versions by installation, replacing
`ids_by_member`'s name key and `latest_by_name` authority. Epoch allocation and staged
expected-base checks likewise belong to that installation, not its label. Keep
compiled module identity, installation display label and publication epoch distinct.
Each namespace identity is logical: a public prefix namespace need not have a
physical executable slot. Do not store `ModuleRef` in a public `Module`.

The store is the installation's lifecycle owner. Handles share the cell, not heap,
statics, host callbacks or store ownership. A query locks only long enough to clone
one current descriptor, releases the lock, then traverses that one verified graph.
The cell is not a namespace cache and does not duplicate metadata. Publication
prepares all fallible work first and swaps that pointer only after store records,
static bindings and UP current-target evidence are ready; the commit has no fallible
suffix or script/host callbacks. UP02 integrates source associations in the same
publication boundary. Never hold the cell lock during execution, user callbacks,
store retention registration or destruction of the previous descriptor. Acquire/
check the cell lock before the non-failing commit section. Treat lock poison as
`EngineFault`, not as permission to use stale contents.

A plain `Arc<RefCell<_>>` would make the Runtime non-Send; `Rc` would do so too.
A short standard `Mutex` over immutable metadata preserves safe ownership transfer
and independent clone/drop of handles without synchronizing the Runtime/GC itself.
Keep `ModuleStore`'s exclusive-driver `RefCell` storage and Runtime's not-Sync contract;
no unsafe auto-trait implementations, concurrent publication, lock-free dispatch,
new synchronization dependency or MS product-sharing work is needed. Public
Program/Module metadata handles can be Send + Sync; all runtime operations still
require the supplying Runtime and its existing execution/borrow admission.

On close/drop, clear every installation cell before releasing Runtime state;
recover a poisoned guard only to replace its contents with `None`, never to admit
execution. Drop removed descriptors outside the guard. An external handle then
retains only identity, not an executable publication or a live
Runtime. `root()` remains an infallible identity-handle constructor; all subsequent
lookup/binding rejects a closed cell. A metadata query already holding a cloned
immutable descriptor may finish, but cannot grant execution after close. Dropping
Program/Module handles has no unloading, cancellation or static-reset effect.

**Admission and retained bodies.** Runtime binding checks registry owner, exact
installation registration/cell identity and liveness, then captures current metadata
once. Execution additionally validates the selected executable records and required
leases. Existing `ProgramLease` registrations remain the owners for active calls,
runtime values/type layouts and native artifacts until UP explicitly replaces the
affected selection policy. Sharing a descriptor or installation cell is not such a
lease. Keep old publication records only for actual retention owners; ordinary live
Program/Module handles do not pin old publications. Type-only payload separation
(R13) remains deferred: an ObjectType may still retain more code than logically
needed, but must remain safe. Native products keep exact version/dependency validity;
metadata publication does not make stale machine code admissible.

Installation identity must also reach layout/type admission and static ownership.
In particular, the current fallback comparisons using only registry owner plus
nominal declaration/arguments must not equate independent installations in one
Runtime. HM02 reviews `StructLayoutRef`, `EnumVariantRef`, lexical TypeArgument
scopes, admission/cache keys and static binding together. Equal declaration paths
in two fresh installations are not automatic object/state interchange authority;
compatible publications within one installation retain existing checked admission.
No cross-installation linking is introduced.

**UP handoff and sequencing.** UP01 supplies checked stable callable/application
identity and frozen-contract evidence before HM02; HM02 supplies the unique
installation and live metadata cell before UP02. A callable's lookup result is
installation + canonical declaration + checked application/signature, never the
original path or a display name. UP02 replaces its body pin with current selection,
while a selected frame/Future/closure retains its body and lease. Ordinary calls
inside old work select through the same installation. HM03 comes after UP02, so
module-relative bindings use that one current-call surface. HM02 may retain existing
pinned-call regressions until then, but must already implement genuinely live
metadata, not a temporary public snapshot facade.

UP01 must preserve every previously public canonical namespace identity as a query
start in compatible candidates, including a namespace held through an earlier
alias. A path alias is not the identity of the returned Module. Comparing only the
root path language, as today, is insufficient for this new handle contract: dropping
a canonical namespace must not make an old handle stale merely because another
namespace exposes equivalent paths. Require its namespace contract to remain in
the candidate's reachable public graph and validate continuity; reject its removal.
Do not admit unreachable graph records or retain an old public snapshot to answer
it. This is the necessary live-handle addition to compatibility, not permission to
rewrite unrelated function-path rules.

**Function-alias retarget policy:** compatible publication may retarget an existing
public function alias to a different public declaration with the same checked
signature. A retained binding keeps its original declaration/application and calls
that declaration's current implementation; only fresh path lookup selects the new
target. UP01 preserves both existing declarations and path signature compatibility,
not unconditional equality of function-export target IDs. This does not permit
nominal type-export retargeting, signature changes or changing the meaning of a
protected Hash/Eq/initializer dependency. UP01's directional graph comparison must
apply these distinct leaf rules rather than freeze every public path target.

**Resolved transitive-selection handoff:** compatible updates prohibit changes to
published key Hash/Eq implementations, static initializer expressions and their
transitive dependencies, regardless of live containers or initialized static cells.
UP01 owns the protected-dependency inventory and conservative rejection when unknown
dependencies prevent proving a change safe, including resolution changes through
aliases. UP02 enforces it for both source updates and prepared/source-free reload
before publication. There is no special old-version call domain, eager static
initialization or automatic rehash. Ordinary calls retain the one current-selection
rule; the protected code cannot change through a compatible publication. The UP plan
owns detailed dependency evidence and tests, not a second HM validation mechanism.

Example: loading the same prepared `game` twice under label `player` creates A and
B with separate statics. Publishing A2 changes an A1-era `users` handle's query
result to A2; B's handles still query B1. A suspended A1 body retains its executable
lease, not public lookup authority for A1. After UP02 its next ordinary call selects
A2. Closing A's Runtime makes A handles stale; neither label `player` nor a retained
A1 descriptor can redirect them to B or revive A.

### HM00 exact HM API and errors

HM02 changes the SDK declarations below, directly migrating consumers (design
signatures; existing `PreparedProgram`, options and result aliases keep their owners):

```text
Runtime::load_program(&mut self, &PreparedProgram, LoadOptions) -> LoadResult<Program>
Runtime::reload_program(&self, &Program, &PreparedProgram, ReloadOptions) -> ReloadResult<()>
Program::root(&self) -> Module
Program::module(&self, path: &str) -> Result<Module, RuntimeError>
Module::module(&self, path: &str) -> Result<Module, RuntimeError>
Runtime::object_type(&self, &Module, path: &str) -> RunResult<ObjectType>
core Runtime::bind_type(&self, &Module, path: &str, &[TypeArgument]) -> NativeResult<ObjectType>
```

Keep reload's existing `&self`; runtime execution-phase admission, not Rust receiver
mutability, enforces the host-top-level boundary in UP02. Rename `module_name` to
`program_name` in both options; it is an optional diagnostic display label only.
`ReloadOptions.program_name` changes that label on success, never installation or
compiled identity. Pure module navigation returns core `RuntimeError`, allowing the
existing SDK `From` conversion rather than adding SDK wrapper handles.

Sync/async `Runtime::function`/`async_function` keep existing generic bounds and
result/error families and replace only `&LoadedModule` with `&Module` in HM02;
UP02 owns the ordinary callable type/dispatch migration. `bind_type_declaration`
remains the distinct advanced declaration/application route, scoped to a `Module`'s
installation with the same public/type/layout checks; it grants no private namespace
navigation. `execute` and `execute_prepared` replace their first handle parameter
with `&Program`, keeping other arguments/results, and select the current root once.
Native entry preparation uses the same explicit Program root plus selected-version
evidence; it cannot infer root intent from a child slot. No Program-to-Module Deref,
implicit conversion, overloaded owner trait or legacy `LoadedModule` alias.

HM03 uses one contract-owned bounded traversal for module/function/type lookup.
Validate the whole path syntax before graph traversal: 1..64 nonempty identifier
segments, `::` separators, no `crate`/`self`/`super` navigation or type-expression
syntax. Reuse the language identifier rules without a runtime frontend dependency.
At each step use the requested namespace; a same-spelling function does not shadow
a type/module. Private and absent names are indistinguishable. Error mapping is
fixed without introducing a new workspace-wide error hierarchy:

| Condition | Core error kind and diagnostic detail |
| --- | --- |
| Empty/malformed/overlong path | `TypedPathValidation`: `invalid public path` |
| Missing/private intermediate or leaf | `TypedPathValidation`: `public path not found` |
| Existing public leaf of another requested kind | `TypedPathValidation`: `public path has wrong target kind` |
| Valid non-object type leaf | `TypedPathValidation`: `type is not a script object` |
| Open alias | `TypedPathValidation`: `open generic alias is not supported for host binding` |
| Wrong argument count / arguments on a closed alias | `TypedPathValidation`: `type argument count mismatch` |
| Valid advanced generic application lacks executable evidence | `ModuleValidation`: `missing executable struct application` |
| Foreign-runtime, unknown installation or closed handle | `StaleHandle`, with owner/liveness detail, never name fallback |
| Reentrant/active-stack publication | `ExecutionPhaseViolation` (UP02), before candidate effects |
| Poisoned metadata cell / impossible verified target | `EngineFault`, not a missing-path fallback |
| Forged export/layout/binder during artifact verification | Existing `InvalidProgramExports` / contract validation error family |

Runtime binding checks authority before path resolution, then syntax, visibility/
kind, argument contract and layout admission. SDK keeps existing mapping:
`TypedPathValidation` to its matching failure kind, `StaleHandle` to
`StaleModuleOrHostRoot`, and module verification failures to `BytecodeVerification`;
reload wraps validation/phase failures in the existing reload error family. Include
the path/expected kind in messages without exposing private candidates. No lookup
failure allocates a script object, runs initialization or invokes script/host code.

## Mature references and deliberate adaptations

Initial review on 2026-10-23; Rust type-alias and JVMS references fetched and
rechecked on 2026-10-24 for this audit. The Mutex reference below was also consulted
on that date. These are design precedents, not measurements or dependency additions:

- [Rust Reference, use declarations](https://doc.rust-lang.org/reference/items/use-declarations.html),
  living reference, sections `use` Visibility and `as` renames: a public alias names
  a canonical target and may expose an item whose containing path is private.
  Reuse that distinction and existing Kagari checked namespaces. Do not copy Rust
  edition-specific root lookup or its complete import/namespace rules into the host
  string API. Kagari's finite module-alias graph retains its existing cycle rules.
- [Rust Reference, type aliases](https://doc.rust-lang.org/reference/items/type-aliases.html),
  living reference, `items.type.intro`: an alias introduces a synonym in the type
  namespace, not a new nominal type. Apply this to declaration-context normalization
  and substitution in HIR; preserve a source declaration for binders/navigation,
  while executable consumers receive the target's checked type identity. Kagari
  deliberately enforces alias parameter/`where` bounds at every application,
  including erased arguments, unlike Rust's unenforced ordinary type-alias bounds.
  Kagari's struct/enum constructor rules are its own contract, not Rust's
  tuple/unit-struct constructor restriction. No runtime alias expansion or new
  nominal identity mechanism is needed.
- [Java Virtual Machine Specification, Java SE 25, section 5.3](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-5.html#jvms-5.3):
  a type's defining loader matters in addition to its name; sections 5.4/5.4.3
  separate verification/preparation from resolved symbolic targets. Apply the
  ownership principle: spelling or matching structure cannot substitute for the
  supplying installation and checked identity. Do not copy class-loader delegation,
  class initialization, mutable module access or JVM concurrency into Kagari.

- [Rust standard library, Mutex](https://doc.rust-lang.org/std/sync/struct.Mutex.html),
  living stable API documentation, trait implementations and poisoning sections:
  `Mutex<T>` requires `T: Send` for Send/Sync and provides scoped RAII access. Use
  stable `lock` plus clone/replace for the metadata pointer only; poisoning is an
  error signal, not a safety proof. Existing descriptor Send/Sync assertions in
  [runtime type identity tests](../crates/kagari-runtime/src/frame/types/arguments/identity_tests.rs)
  support this narrow choice. Do not use nightly pointer/value APIs, wrap the heap
  in a lock or infer Runtime Sync from a synchronized descriptor cell.

The concrete adaptation is loader-qualified installation identity (JVMS 25),
canonical alias targets (Rust Reference) and a short metadata publication cell
(Rust std), while retaining Kagari's explicit leases and exclusive Runtime driver.
Neither reference supplies Kagari's reload semantics: UP owns current-call target
selection and selected-body retention. No custom lock-free or concurrent runtime
mechanism is justified by this task.

The existing ProgramDescriptor and verified namespace graph supply the namespace
foundation; current descriptors and prepared bindings are implementation evidence,
not ready-made live Program/Module handles or a stable-callable dispatch mechanism.
Extend the namespace owners instead of inventing a loader hierarchy, global name
cache or parallel reflection subsystem.
Dispatch mechanism research and migration belong to UP, with HM00 recording the
handoff. No speedup is claimed; new persistent caches require concrete justification.

## Phase order and acceptance

The [transparent generic alias prerequisite](#transparent-generic-alias-prerequisite)
is locally complete. The newly reauthorized order is joint HM00 + UP00, then HM01,
UP01, HM02, UP02, HM03, HM04, UP03, UP04 and UP05. The joint design checkpoint is
committed as `bbfc0af6`; HM01 is locally complete after private-type evidence
integration and validation. UP01 is next. The joint design commit has both
Plan-Phase trailers;
subsequent completed phases have their own coherent commits. MS remains unstarted.
Record each phase in its existing owning plan, not a third integration ledger.
Alias completion alone did not complete HM00/UP00; the resumed design audit and
explicit policy decision resolve the representation/type-export/lifetime handoff.

Default checkpoint policy: each implementation phase is buildable, with affected
consumers migrated in that phase. HM00 is a contract checkpoint, not permission to
leave failing tests committed. A cross-layer schema change belongs in one coherent
checkpoint even when it spans several crates. No disabled checks, fake targets or
production stubs are allowed. Record any unexpected intermediate failure here and
repair it before proceeding; do not repeatedly rerun an unchanged known failure.

### HM00 — Freeze contracts and test owners

Status: design complete at `bbe30666` with the subsequent user-resolved protection
policy. The following scope is covered by the frozen decisions and handoff; this
status does not claim production code or behavioral validation.

- Audit current load/reload, entry/native consumers, retained handles, alias facts
  and executable layout availability against the current HEAD.
- Decide representation for the confirmed live Program/Module and stable callable
  identities, distinct from internal immutable versions, selected body/resume/capture
  lifetime, type/layout identity and native validity. Audit ordinary functions,
  methods/trait calls, prepared calls/Futures and binding through retained live
  handles; do not infer support from naming or reopen the confirmed public semantics.
- Resolve installed handle retention/admission, public versus advanced module access,
  supported alias/application evidence, exact signatures and diagnostic mapping.
  Preserve confirmed unit-success update/reload results, Runtime-owned installations
  and non-unloading Program/Module Drop; these are not open policy decisions.
- Record the UP dispatch/publication migration checkpoint and ordering dependency,
  including which current pinned regressions remain valid before that transition.
  Preserve the confirmed live-handle model across the implementation handoff.
  Do not require Program/Module reacquisition or function
  rebinding as the final model, or add dual public entrypoints.
- Select existing semantic test owners and concrete cases. Update this document
  with resolved representation and handoff decisions before HM01; full dispatch
  migration stays outside HM01-HM04.

Acceptance: no unresolved representation question affecting type authority, runtime
retention, live installation/module/callable identity versus internal versions, or
closed-alias support; an explicit UP handoff, finite consumer/validation inventory
and no implementation claim from documentation alone. Latest-published public
lookup and invocation without reacquisition/rebinding remain confirmed requirements.
The required separation is recorded: UP01 precedes HM02, and UP02 precedes HM03;
no dispatch implementation is hidden in HM.

### HM01 — Checked type exports through artifacts and reload

Status: locally complete, including private-type evidence and post-repair checks;
see the [HM01 ledger](#hm01-integration-and-up01-handoff). Full CI remains pending.
Public handle and dispatch migration remain HM02/UP02 work.

- Extend HIR export collection, compiler lowering, contract records, mapping,
  codecs, MIR/bytecode verification and required closed-layout demand together.
- Extend staged reload type-path comparison immediately; do not publish type
  export support while reload ignores its compatibility.
- Reuse existing public authority and type normalization owners; keep runtime
  consumers independent of source/HIR and preserve canonical serialization.
- Regenerate only affected development fixtures, preserving negative validation.

Acceptance: selected direct/re-export/closed-alias cases survive source-to-artifact
round-trip and identity import; forged metadata is rejected; equivalent type
exports compare correctly and nominal retargeting rejects before publication.
Existing function exports/private main behavior remains unchanged in this phase.

### HM02 — Installed Program and module capabilities

- Introduce stable installed `Program` and live `Module` handles using HM00's
  reviewed ownership representation; replace the current `LoadedModule` public
  handle without compatibility aliases or implicit conversion/Deref from Program
  to Module. Load returns Program; update/reload targets its stable installation
  without requiring a replacement handle. Make Program entry operations explicitly
  Program-scoped, migrating core/SDK/backend/native/CLI callers as affected.
- Review public names and import collisions at their actual owners. Use explicit
  module imports or meaningful aliases; do not rename unrelated HIR/MIR/bytecode
  models or add forwarding types solely to obtain the shorter host-facing names.
- Preserve one installation owner, static storage, registry identity and required
  safety leases under the HM00/UP handoff. Ordinary Program/Module queries must
  read that installation's latest published contents; do not expose internal
  immutable versions as ordinary handles. Honor any UP prerequisite recorded by
  HM00 rather than substitute snapshot semantics. Add `root()`; keep slot APIs at
  their reviewed responsibility.
- Update exact re-export whitelist entries only for justified public facades.

Acceptance: load returns a complete installed Program; reload publishes to that
same installation. Existing Program/Module handles query its latest published
contents without reacquisition. Member handles share that installation; operations
with foreign-runtime handles reject and descriptor-only retention cannot revive admission.
Selected bodies, types and native products retain their required lifetimes. Before
UP dispatch migration, pinned-call behavior is regression coverage only; after
migration, existing function handles use the latest implementation without rebinding.
Program main/native entry behavior remains root-scoped.

### HM03 — Relative public lookup and binding

- Add Program/Module `module(path)` through the supplying installation's latest
  published verified public edges, including when the handle predates publication.
- Replace root-reset function lookup with module-relative lookup for sync and
  async binding. Resolve object paths through HM01 type evidence and reuse current
  type/layout preparation. Preserve explicit declaration/application APIs where
  they represent distinct advanced capabilities, not compatibility spellings.
- Migrate SDK examples and direct consumers; remove the superseded root-reset rule
  and any temporary private migration adapter by the end of this checkpoint.

Acceptance: nested/aliased module lookup and root-versus-member function/type paths
agree on checked installation/declaration identity in the latest published contents,
including through retained Program/Module handles after update. Binding does not
pin ordinary call bodies to the publication present at lookup. Private modules do
not leak through item exports; closed aliases bind only supported layouts;
malformed/missing/unsupported paths fail without script effects. Existing private
main access remains separate.

### HM04 — Integration, contract publication and handoff

- Run the selected source/source-free host scenario with object access, sync/async
  bindings, compatible and rejected reload, persistent static state and retained
  old work. Reuse original Program/Module handles for latest-published queries;
  coordinate existing function-handle latest-call evidence with the UP checkpoint.
  Exercise cleanup after final actual owners release, without claiming the separately
  queued R13 split is implemented.
- Update embedding, module loading/activation, architecture and affected examples
  with the actually implemented contract. Record the UP handoff and remaining
  dispatch migration explicitly; do not claim current-code calls from namespace
  acceptance alone or publish snapshot handles/pinned calls as the final target.
  Remove resolved lookup prose from review while retaining cross-installation repair
  requirements and unrelated findings.
- Publish local results, failures and pending CI separately. Move deferred proposals
  to the roadmap, retire this completed plan and fix inbound links at completion.

Acceptance: all selected tests and local implementation checks pass, consumers use
one coherent API, no carried errors remain, and exhaustive CI scope is explicitly
pending until run. HM namespace completion is separate from UP dispatch acceptance;
the recorded handoff must survive plan retirement. No new performance threshold
is required.

## Focused verification

Use existing owners before adding tests:

- [Compiler export contracts](../crates/kagari-compiler/src/tests/package_exports.rs):
  source graph, lowering, corrupted authority and artifact verification.
- [Contract export codec tests](../crates/kagari-contract/src/exports/wire/tests.rs):
  bounded encoding, identity remapping and malformed records.
- [Runtime export/reload tests](../crates/kagari-runtime/src/module/tests/package_exports.rs):
  cyclic paths, current pinned implementation regressions and compatibility.
- [SDK binding facade](../crates/kagari/src/runtime/bindings.rs) and existing embedding
  tests: observable lookup, typed calls, object access, async and static reload.
  HM00 selects exact existing test filters/targets; new tests cover distinct missing
  contracts, not every modified helper.

Required coverage matrix:

| Boundary | Selected behavioral evidence |
| --- | --- |
| Public paths | Nested paths, aliases, source namespace collisions, malformed/overlong paths and private rejection |
| Type authority | Direct and re-exported struct, closed alias/application, wrong kind, missing layout, forged IDs/arguments |
| Privacy | Public leaf from private module without a public module handle or private-member access |
| Source-free | Decoded artifact loaded without frontend dependencies, equivalent binding and corrupt-record rejection |
| Installation | Same preparation installed independently; live handles query only their own installation even after one is updated; foreign handles rejected; no accidental extra state |
| Reload | Equivalent aliases, incompatible nominal retargeting, failed-candidate isolation and static state; reuse original Program/Module handles to query latest published contents after success and unchanged contents after rejection; no replacement handle required |
| Lifetimes | Internal immutable versions, selected bodies/resumes/captures, type evidence and native products retain required owners independently of live public lookup; descriptor retention does not revive admission; final owner cleanup |
| UP handoff (not HM dispatch implementation) | Existing function handles invoke latest code without rebinding or Program/Module reacquisition; current calls from old resumes/Futures/closures and fixed trait relationships with current method bodies; dispatch evidence follows the recorded HM00/UP checkpoint |
| Exact-version inspection (UP/debug, not HM lookup) | Explicit historical/observed-target selection differs from live Program/Module queries and later ordinary calls; no ordinary handle is a historical snapshot |
| Entries/backends | Private root main remains explicit; representative existing native preparation consumer still uses a Program root |

Initial existing filtered commands (not a promise of coverage for newly added tests):

```text
cargo test -p kagari-compiler --lib tests::package_exports --no-fail-fast
cargo test -p kagari-runtime --lib module::tests::package_exports --no-fail-fast
```

The HM00 handoff below records selected SDK/source-free/codec/native targets after
checking their feature gates. Use default Cargo profiles, target and
parallelism. Run Cargo jobs serially when sharing the target directory. Do not
run full-workspace tests locally or approximate them with an all-package sweep.
A small actual native test is needed only where its consumer changes; interpreter
fallback alone does not establish that native path passed.

Local implementation checkpoint/final checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Review changed Rust ownership, imports, visibility, exact re-export whitelist,
effective LOC and module boundaries in addition to the checker. Do not rerun
unaffected passing checks solely for a new transcript. Full-workspace tests and
complete feature/backend/platform matrices are CI-only. Documentation-only edits
need content/link/diff checks, not Rust builds or Clippy.

## Deferred work

Work beyond HM remains with its existing owners, not additional HM phases.
Confirmed live-handle and UP call requirements below are deferred implementation,
not undecided policy; HM00 has resolved representation, the handoff and ordering
without reopening public semantics.

- Cohesive text/file/artifact preparation and explicit tooling snapshots; avoid
  accidentally compiling editor overlays or mandatory encode/decode round trips.
  Preserve multi-module diagnostics and artifact-only consumers. Any shareable
  preparation product needs proven ownership, not unsafe Send/Sync additions.
- [One-shot source Update batches](update-model-design.md#change-lists-and-one-shot-submission):
  the user assembles function/file replacements and submits once to Runtime, without
  begin, version numbers or body hashes. The update design owns compilation context,
  validated all-or-nothing publication at the host top-level boundary. Update and
  prepared reload target a stable installation; neither requires replacing its
  Program/Module handles. HM supplies live namespace handles and the HM00
  identity/lifetime handoff, not this batch frontend, dispatch migration or
  incremental compilation. No concurrent publication mechanism is required;
  publication must not execute initialization code.
  Existing signatures/type definitions and trait relationships remain frozen;
  adding a new trait does not authorize implementing it for an existing type.
- [Exact-version function-source inspection](update-model-design.md#exact-version-function-source-inspection):
  use explicit UP/debug APIs to select a historical internal version or a retained
  body/frame/task position, with explicit unavailable results and independent source
  retention. Ordinary Program/Module handles query latest published contents, not
  historical snapshots, and an ordinary stable callable is not an exact-body snapshot.
  A current-source query captures selection at query time, distinct from later
  invocation. Exact query types and retention belong to UP; they must not restore
  immutable version views as the ordinary public handle model.
  This requirement belongs to the update design, not HM's public-path implementation
  or a new interactive-debugger phase.
- The confirmed [UP target](update-model-design.md) replaces pinned ordinary dispatch,
  including calls from old resumed bodies, through stable callable identity.
  Computations/Futures keep their selected state; closures keep capture-compatible
  bodies; ordinary calls inside use current implementations. Fixed type/trait
  relationships are not fixed method bodies. Existing function handles invoke latest
  code without rebinding, just as existing Program/Module handles query latest
  published contents without reacquisition. HM00 resolves representation and ordering,
  not these semantics; HM01-HM04 do not implement the full migration or preserve a
  second legacy surface.
- Wider configuration/error/tooling cleanup: owner-scoped options, existing call
  overrides and structured stage-specific causes, without resetting nested
  protection or adding generic permission matrices. Backend fallback must remain
  distinct from invalid code/runtime failure; no new Tokio/core scheduler required.
- Explicit state replacement and unload policy remain separate from compatible
  reload; do not infer transaction rollback, cross-Actor atomicity or cancellation
  from dropping a handle.
- [Repair programs referencing existing installations](review.md#runtime-loaded-programs-referencing-existing-installations)
  require versioned compilation contracts and explicit installation linking. Public
  module lookup alone does not share independently installed state. Repairs stay
  outside the game's replacement inventory. Retained computation/type/native
  lifetimes remain distinct from ordinary calls into the game's current code;
  exact cross-installation linking remains deferred, not a pinned-dispatch exception.

Implemented outer-tuple arguments, one-value returns, business Result versus runtime
errors, typed conversion, host registration and async scheduling remain owned by
current specifications, not tasks to repeat here. R13, SA21 and native expansion
retain their existing review/plan owners.

## Resumption and progress ledger

- [x] Plan authored against `67d9e36f`; scope narrowed to Program/Module/public paths.
- [x] Confirmed live-handle/current-code target recorded on 2026-10-23; current pinned
  implementation labeled. Ordinary Program/Module handles always query the latest
  published contents of their own installation; existing function handles invoke
  latest implementations without reacquisition or rebinding. Load returns Program;
  update/reload targets the same stable installation without a replacement handle.
- [x] Former immutable public version-view model removed from this plan. Internal
  immutable versions and selected body/resume/capture lifetimes remain distinct;
  exact-version inspection belongs to explicit UP/debug APIs. Dispatch/publication
  migration remains with UP, not added to HM01-HM04; subsequent implementation
  authorization is recorded in the status and audit below.
- [x] Confirmed interface details: update/reload return unit success; Runtime owns
  installations and dropping Program/Module handles does not unload/cancel/reset;
  callable binding resolves a stable declaration once; public object lookup has a
  finite struct/closed-alias scope without string type parsing or specialization.
- [x] HM-side design audit at `bbe30666`: concrete type-export schema and binder
  proof, layout demand, live installation representation, HM API/error mapping and
  phase/test handoff frozen below. Implementation was read-only; only this plan edited.
- [x] HM00 — Representation, type exports/source-free proof/layout demand, exact HM
  API/errors, test owners and UP01/UP02 handoff resolved. The user closed the
  transitive-selection gate with unconditional protection of published key Hash/Eq,
  static initializer expressions and transitive dependencies. Function-alias
  retarget semantics are explicit below. Design acceptance only; UP00's own checklist
  stays in its owning plan. Joint HM00+UP00 committed as `bbfc0af6`.
- [x] HM01 — Type export pipeline, private declaration evidence, layout demand and
  reload comparison locally complete. Post-repair focused tests and all local
  checks passed; no carried errors. Next: UP01. Full CI remains pending.
- [ ] HM02 — Installed Program/module capability separation and consumer migration.
- [ ] HM03 — Relative module/function/type binding and ordinary examples.
- [ ] HM04 — Integration, lasting documentation and handoff.

HM00/UP00 design is committed; HM01 is locally complete as recorded below.
The public API remains `LoadedModule` with pinned calls: live Program/Module handles,
relative host type binding and current-call dispatch are not implemented by HM01.
UP01 owns the next frozen-inventory/protected-dependency/stable-callable checkpoint;
HM02 follows it, and UP02 owns dispatch. MS remains unstarted. Older audits and the
alias ledger are historical evidence, not current pauses or authorization gates.

### HM01 integration and UP01 handoff

Implemented: checked HIR type exports; canonical declaration/closed-alias records
and rejection-only open aliases; identity mapping and bounded wire encoding;
independent MIR/KBC authority, binder, nested-bound and layout proof; export-only
cross-module struct/enum proof-layout demand; normalized type-leaf reload comparison.
The lasting wire/verification contract is in
[artifacts](spec/artifacts.md#public-type-exports). No public handle or dispatch
migration, runtime specialization, ABI/version bump or compatibility reader was added.

Local evidence reported by the implementation/validation owner:

- Earlier contract export wire tests: 4 passed; retained as unaffected evidence.
- Post-repair source-free bytecode `type_exports`: 10 passed; MIR type-export test:
  1 passed; private-bound artifact test: 1 passed; MIR private-enum codec: 1 passed.
- Compiler: 9 selected tests passed, including `Cell<Hidden>`, private bounds and
  portable round-trip coverage. Contract applications: 6 passed.
- Parent reran runtime `module::tests::package_exports`: 6 passed, and SDK
  `portable_artifact_executes_without_source_compilation`: 1 passed with source
  enabled to regenerate the artifact, then 1 with `--no-default-features`.
- Post-repair `cargo clippy --workspace --all-targets -- -D warnings` passed;
  structure check: 1522 files, zero violations; formatting and diff checks passed.

Resolved transient failures: a missing helper-module declaration and a test using
converter identity without the required admission. Review repairs cover nested
application bounds, private-supertrait proof dependencies without public exposure,
indexed bounded validation/cancellation and enum payload proof-layout roots.

The final `Cell<Hidden>` failure is resolved: compiler lowering emits
`private_types: Vec<TypeDeclaration>` separately from `public_items`; contract
mapping/bounded codecs and MIR/KBC records/verifiers retain and validate full
canonical declaration/binder/bound evidence for dependencies. Private evidence
never grants public export authority, and unknown constraints are not skipped.
Private-bound and source-free round-trip tests cover the repaired boundary;
affected fixtures were regenerated. No carried HM01 errors remain. This finite
metadata addition is not UP01's complete frozen inventory.

This selection covers source lowering, portable/source-free authority, bounded
verification and reload compatibility without a workspace test sweep. HM01 local
acceptance is complete. Full-workspace tests and the complete feature/backend/
platform matrix remain CI-only and unrun; local results are not CI acceptance.

Next, UP01 reuses these checked exports but supplies the separate complete frozen
inventory, protected Hash/Eq/initializer dependency evidence and stable-callable
application coverage. Export metadata is not that inventory. Existing function
alias retarget policy and private root-entry behavior are unchanged; HM01 does not
implement protected-update enforcement or current-call selection.

### HM00 audit at bbe30666: implementation slices and parent handoff

Audit scope: read-only implementation inspection of `LoadedModule`, `ModuleStore`,
staging, retention, layout/admission and object-binding owners; checked HIR aliases,
compiler layout demand, contract/wire/type proof, bytecode verification, reload
comparison, SDK signatures/errors and existing test feature gates. Working tree was
clean at audit start. Only this document is edited. No HM or MS code was implemented.

The schema, installation model and HM API/errors above complete HM00's design
checkpoint. The user-resolved protection rule removes the former semantic blocker;
The joint docs checkpoint was committed as `bbfc0af6`; UP owns source/state
contracts. The implementation handoff remains finite:

- UP01 owns full frozen source/declaration inventory, including open alias bounds/
  targets not represented by HM's rejection marker; it also owns retained namespace
  and stable callable continuity. Preserve function-path signatures while allowing
  same-signature alias retargets for fresh lookup; retained bindings never retarget.
  Carry the inventory and protected Hash/Eq/initializer dependency evidence through
  source-free artifacts. HM type exports are not that whole inventory.
- UP01 supplies stable callable/application evidence without changing dispatch.
  HM02 establishes unique installations/current descriptors. UP02 owns the sole
  top-level publication gate and current-call migration, atomically coordinating
  its source associations with the installation pointer. No second publication
  owner or source frontend in core runtime.
- HM03 is deliberately after UP02: its final relative bindings must use the same
  ordinary current-call surface. Source inspection/state replacement are not HM
  implementation work. MS and R13 remain unstarted/separate respectively.

Concrete next implementation slicing (substeps are one coherent phase commit, not
permission to commit a broken cross-crate schema):

| Checkpoint | Ordered implementation work | Selected contract evidence |
| --- | --- | --- |
| HM01 (locally complete) | Add contract type leaf/mapping/bounded wire and normalized comparison; collect checked HIR exports; route closed export layout roots before lowering; extend MIR/KBC verification and identity/fingerprint traversal; regenerate affected fixtures once | Extend compiler `tests::package_exports` round-trip/forgery tests, contract `exports::wire::tests`, runtime `module::tests::package_exports`; exercise one decoded artifact without source |
| UP01 | Frozen inventory, protected transitive dependencies, stable target/application evidence and the continuity/alias-retarget rules above | UP-owned selections; no dispatch migration yet |
| HM02 | Separate `ExecutableModule` from live Program/Module; replace name-key authority throughout staging/current roots/collection/epoch allocation; install current cell and close invalidation; carry installation identity through type/static admission; migrate SDK/core/VM/native/CLI entry and handle consumers | Runtime export/staging/retention cases, same-label independent loads and cross-installation rejection, SDK runtime transfer and one real native entry |
| UP02 | Current dispatch and serialized publication with common protected-dependency rejection; source Update/inspection integration using that installation | UP-owned evidence, including retained work, reentrant rejection and no alternate old-version call domain |
| HM03 | One bounded relative graph traversal; public module navigation, relative sync/async functions and type classification/application binding; migrate host examples without root-reset adapters | Extend SDK typed-member workflow and async binding cases; reuse artifact-only case for closed object aliases; runtime export privacy/cycle cases |
| HM04 | Integrated host/reload scenario, lasting contracts and handoff publication | Selected shared scenario plus local checks; exhaustive matrix remains CI-only |

Distinct cases to add at these owners, not duplicate smoke tests:

- **HM01:** unused `pub struct`; renamed public type in private module; otherwise
  unused closed generic struct alias; alias-of-alias equivalence; closed enum/scalar
  and open/erased alias classification without compilation rejection. Round-trip
  MIR/KBC and import to another definition context. Corrupt public kind/ID, nominal
  arity, lexical parameter owner/position, missing layout, associated bindings,
  type/module collision, duplicate/bounded wire records and zero/oversized open
  alias arity. Include an export-only cross-module application and a non-object
  export whose declaration/proof metadata has no body reference. Same canonical
  type via different alias spelling reloads; changed
  nominal target/argument rejects before publication. Existing separate private
  main and cycle comparisons remain covered.
- **HM02:** two loads with the same prepared product and label keep distinct
  installation/statics/layout authority; update one only. Reuse pre-update Program/
  Module handles to query current metadata after success and unchanged metadata
  after rejection, including canonical module-alias identity. Dropping handles
  neither unloads nor resets; closing Runtime invalidates retained handles. Selected
  executable/type/native leases still delay collection and release after last
  actual owner; descriptor-only retention cannot regain admission. Test Runtime
  Send/not-Sync and handle Send/Sync without unsafe implementations. Keep old
  pinned-call expectations only until UP02 replaces them.
- **HM03:** root versus child relative binding with deliberately colliding function
  names; nested/cyclic module aliases; item-only re-export that never yields its
  private owner Module; zero-argument closed aliases and advanced direct generic
  applications; every error row above before effects, including rejected open alias
  with supplied arguments. Exercise both sync and async binding. After UP02 reuse
  a bound function following update; retargeting a path affects only fresh binding.

Selected command inventory, to use **per affected phase**, not as a full sweep:

```text
cargo test -p kagari-contract --lib exports::wire::tests --no-fail-fast
cargo test -p kagari-compiler --lib tests::package_exports --no-fail-fast
cargo test -p kagari-runtime --lib module::tests::package_exports --no-fail-fast
cargo test -p kagari-runtime --lib module::retention::tests --no-fail-fast
cargo test -p kagari --lib runtime::tests::embedding::runtime_transfer --no-fail-fast
cargo test -p kagari --lib native_overflow_reports_the_same_instruction_as_the_interpreter
cargo test -p kagari --test async_facade typed_members
cargo test -p kagari --test async_facade typed_direct_await_drives_tasks_without_a_pump_and_preserves_context
cargo test -p kagari --test artifact_features portable_artifact_executes_without_source_compilation
cargo test -p kagari --no-default-features --test artifact_features portable_artifact_executes_without_source_compilation
```

SDK defaults enable source/native; `runtime_transfer` and `async_facade` require
source, the native-overflow entry test requires both and asserts actual native
selection. The source-enabled artifact test regenerates the ignored fixture before
the source-free command consumes it. Extend those existing fixtures/assertions for
HM behavior; their present names do not prove coverage of new cases. Run the small
new staging/installation/lookup test filters added within these owners when they
land, plus affected auto-trait/doctest coverage; do not run whole subsystem trees
merely because they are listed. HM02's consumer inventory also includes CLI and
native preparation signatures, covered structurally by workspace Clippy and by the
selected actual native-entry test, not an exhaustive backend matrix.

Final HM00 documentation validation: `git diff --check` passed; a `uv run python`
content check resolved all 39 local links to tracked files and all 6 same-document
anchors. This audit/finalization edits only this document; concurrent parent/UP
changes to the roadmap and UP plan are left untouched. Working-tree CRLF and index
LF match the repository's `core.autocrlf=true` configuration. No Rust
build/test/Clippy was run or claimed for this documentation-only change; no
performance conclusion. Required implementation checks remain the four local
commands above, with focused behavior tests and full CI reported separately.

### HM00 audit at 3b898e80: scope gate and implementation handoff

**Historical finding at `3b898e80`:** required closed transparent aliases were not
an existing source capability. Ordinary parser dispatch accepted top-level `type`
only in declaration mode; native opaque types and `pub use ... as` declaration
renames did not supply transparent type application. The assumption that
`pub type IntHolder = Holder<i32>;` could reuse an implemented alias owner was wrong.

**Resolved scope decision:** the user explicitly authorized transparent generic
aliases first, rather than narrowing HM to declaration renames. The prerequisite
below supersedes the former awaiting-scope-decision gate. It supplies source alias
semantics, not HM type-export records, layout-demand roots or host lookup. HM00/UP00
were paused and incomplete at that audit; the resumed checkpoint is recorded above.

Historical source-derived handoff findings (resolved by the resumed design above):

- Current LoadedModule is an immutable exact executable member, used by frames,
  layouts, candidates and native products. Preserve that responsibility under an
  explicit internal executable-reference name; do not mechanically turn every use
  into a live public Module. Stable Program/Module handles need a Runtime-owned
  installation record and canonical namespace identity, separate from physical slots.
- The current ModuleStore uses display-name/member keys and latest_by_name. New
  loads need distinct installation authority even under the same display name;
  reload targets that installation. Retained descriptors cannot revive closed state.
- Existing function-export reload checks allow a same-signature alias retarget.
  A retained binding must keep its original declaration while a fresh path lookup
  may resolve another declaration. Do not implement latest calls by redoing names.
- Unused nongeneric structs already receive compiler layout roots. Generic layouts
  require existing executable demand/application evidence; exported closed
  applications need compiler-owned roots in HM01, not runtime compilation. The
  alias prerequisite does not implement these export-driven roots.
- Public root/member lookup must not expose a private module through a re-exported
  callable's executable owner. Function/method authority is not namespace authority.

Selected existing correctness owners for later phases: compiler `tests::package_exports`,
runtime `module::tests::package_exports` and `module::retention::tests`, contract
`exports::wire::tests`, VM `method_applications`, SDK `async_facade` and source-free
`artifact_features`. Select the affected subset per phase; this is not a command
matrix to run at every checkpoint. Existing root-reset/pinned assertions change
only with their owning HM/UP transition and equivalent boundary coverage.

Executed narrow baseline evidence:

```text
cargo test -p kagari-syntax --lib opaque_native_types_are_restricted_to_declaration_mode
```

Historical result at the audit: 1 passed, 0 failed, 111 filtered out. This test
established the then-current declaration-mode boundary, not acceptance of the new
alias implementation. The scope mismatch is now resolved by explicit authorization;
current prerequisite evidence and completed local acceptance are recorded below.

### Transparent generic alias prerequisite

This is the single prerequisite ledger; the roadmap and UP plan link here rather
than maintaining parallel checklists. Scope is finite: module-level transparent
aliases, including generic parameters and enforced bounds, through source parsing,
HIR checking/imports/navigation, ordinary lowering and affected syntax tooling.
Lasting semantics belong to [syntax](spec/syntax.md#transparent-type-aliases),
[modules](spec/modules.md#transparent-alias-names) and
[traits](spec/traits.md#type-alias-bounds-and-interface-values).

Ownership/model: HIR resolves each target and its bounds in the declaring module
with alias-owned generic binders, then substitutes checked use-site arguments.
Source declaration identity supports imports and navigation; it is not a nominal
runtime type. Compiler/backend consumers receive normalized checked targets, not
alias syntax to resolve again. A closed `type IntCell = Cell<i32>;` therefore
constructs a `Cell<i32>` and keeps that fixed receiver application for
`IntCell::make(42)`; `IntCell::make(true)` must not re-infer it as `Cell<bool>`.

- [x] User authorized and prioritized this prerequisite before HM/UP; the former
  scope question is closed. Rust synonym precedent and Kagari's enforced-bound
  difference are recorded under [mature references](#mature-references-and-deliberate-adaptations).
- [x] Parser/tooling integration: `type A = Target;`, generic parameters, inline
  bounds and at most one `where` clause before or after the target; ordinary aliases
  remain distinct from native opaque and associated declarations.
- [x] HIR normalization and source integration: forward references,
  import/re-export/navigation, declaration-context lookup, explicit generic
  constructor arguments, closed-alias associated functions/patterns, invalid-unused
  definitions, cycles and visibility. Interface-value aliases remain distinct from
  trait bounds and the trait side of `impl Trait for Type` headers.
- [x] Bound/refinement review: enforce erased argument and projection predicates;
  resolve alias `where` projections and member-local generic associated type bounds
  in their owning contexts; refine imported and local supertraits/alias arguments
  coherently. Bound substitution is preflighted before materialization.
- [x] Boundedness/native validation: deterministic 64 alias-declaration levels
  (64 accepted, 65 rejected regardless of declaration order); at most 4096 expanded
  type nodes and 64 type-nesting steps. Replaced placeholders are not counted twice,
  intermediate expansions are not summed as retained nodes, and a composite's
  retained-node budget is checked before materializing siblings. `Identity<Big>`
  accepts a 4095-node target. These are compiler resource limits, not nominal type
  semantics. Cancellation remains enforced. Declaration aliases reject
  `builtin_type`, `lang` and `native` metadata; `meta`/`tool` metadata remains allowed.
- [x] Focused semantic, compiler/artifact, tooling and structural evidence recorded
  below; transient integration and review failures are resolved.
- [x] Final local acceptance: parent confirmed post-repair compiler/artifact checks
  and `cargo clippy --workspace --all-targets -- -D warnings` passed. The alias
  prerequisite is locally complete; HM00/UP00 were not completed by this work.

Concise validation evidence reported by the implementation owners:

- Syntax package: 118 passed. HIR alias contracts: 24 passed after review repairs,
  plus directly affected bound/GAT/supertrait/native regressions. Focused formatter
  and lint checks passed.
- Compiler `public_generic_dependency_functions_specialize_through_source_aliases`:
  1 passed. SDK `portable_artifact_executes_without_source_compilation`: 1 passed
  each in source-enabled and `--no-default-features` configurations after the last
  HIR repair. The regenerated fixture exercises shared mutation, enum aliases,
  re-exports and GC cleanup.
- Selected editor checks: 8 passed; site checks: 4 passed; coverage audits: 5 passed;
  generated reproducibility passed. Latest structure check: 1515 files, zero
  violations; formatting and diff checks passed.

The selection covers declaration recovery, semantic/namespace boundaries,
source-free execution and tooling without a full-workspace test sweep. Transient
integration failures from missing alias variants/exhaustive matches were repaired.
Review failures in erased projection predicates, bound substitution, projection/GAT
contexts, supertrait refinement, depth/order accounting and composite retention
were repaired and their affected checks passed; none remain carried errors.
Full-workspace tests and the complete feature/backend/platform matrix remain
CI-only and unrun locally; focused
local evidence is not complete CI acceptance.

Excluded: HM host export schema/codec changes, export-only layout roots, public
ObjectType binding, reload compatibility/current-call dispatch, trait aliases,
new nominal runtime identities, generic-alias patterns, synthetic
`use Alias::Variant` imports and open-generic-alias associated-function inference.
Generic alias constructors require explicit alias arguments; closed aliases retain
their fixed arguments in constructors, member paths and patterns. No HM/UP feature
or completed-phase acceptance follows from this language prerequisite.

For each authorized checkpoint, record changed contracts, selected tests and their
rationale, actual results, outstanding errors with owning phase, and pending CI.
Keep temporary logs under ignored `target/`; this ledger retains conclusions needed
to resume without those files. Commit only when authorized; use Conventional Commits,
mark breaking public changes with `!` and include a `Plan-Phase: HMxx` trailer.
