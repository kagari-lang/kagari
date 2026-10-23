# Host Program, Module and public-path execution plan

## Status and authority

Status: implementation and phased commits authorized on 2026-10-23. The user
subsequently prioritized implementing transparent generic type aliases before HM/UP.
That finite language prerequisite is locally complete. Its sole progress owner is
the [alias prerequisite ledger](#transparent-generic-alias-prerequisite); exhaustive
CI acceptance remains pending.
The joint HM00/UP00 audit started at `3b898e80` and remains paused and incomplete;
HM01-HM04 and UP implementation phases have not started. Authorization does not
waive their remaining design gates.
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
HM00 must settle representation and the identity/lifetime sequencing handoff, not
reopen these public semantics or expand HM01-HM04 beyond namespace/type-path work.

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

Source inspected at `67d9e36f`; no new behavioral reproduction or performance
measurement was run for this plan.

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
| Selected computation, type/layout and native evidence | Distinct retained execution or validation products | Body/resume/capture lifetime, type authority and native validity are separate from live lookup and ordinary callable identity; representation is gated by HM00 |
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

HM00 must choose a representation separating stable installation/module/callable
identities and live public lookup from internal immutable versions, selected
body/resume/capture lifetime, type/layout authority and native-product validity.
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

Before HM01, record the exact handle/lifetime representation, admission for retained
live handles and the UP migration owner/checkpoint. The latest-published public
lookup and no-rebinding call semantics are settled, not HM00 options. If live lookup
or binding cannot be separated safely from UP, record a sequencing dependency
rather than silently adding dispatch implementation to HM or shipping immutable
public views as the target. Use one ordinary public binding surface, not parallel
legacy-pinned and current-code entrypoints. No new Send/Sync promise is implied.

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

HM00 must inspect the actual checked alias representation and freeze the smallest
portable record that retains the canonical target, applied arguments and any
necessary binder/visibility evidence. A simple declaration ID is sufficient for a
renamed declaration, not for an alias applying type arguments. Normalize using
existing checked type semantics; runtime must not expand syntax or infer types.

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

## Mature references and deliberate adaptations

Reviewed on 2026-10-23:

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

The existing ProgramDescriptor and verified namespace graph supply the namespace
foundation; current descriptors and prepared bindings are implementation evidence,
not ready-made live Program/Module handles or a stable-callable dispatch mechanism.
Extend the namespace owners instead of inventing a loader hierarchy, global name
cache or parallel reflection subsystem.
Dispatch mechanism research and migration belong to UP, with HM00 recording the
handoff. No speedup is claimed; new persistent caches require concrete justification.

## Phase order and acceptance

The [transparent generic alias prerequisite](#transparent-generic-alias-prerequisite)
is locally complete. On resumption, the authorized order remains joint HM00 + UP00,
then HM01, UP01, HM02, UP02, HM03, HM04, UP03, UP04 and UP05. HM/UP remain paused
and incomplete; prerequisite acceptance does not automatically resume them.
The completed joint design checkpoint has both Plan-Phase trailers;
subsequent completed phases have their own coherent commits. MS remains unstarted.
Record each phase in its existing owning plan, not a third integration ledger.
Alias completion does not itself complete HM00/UP00 or authorize skipping their
representation, type-export and lifetime handoff decisions.

Default checkpoint policy: each implementation phase is buildable, with affected
consumers migrated in that phase. HM00 is a contract checkpoint, not permission to
leave failing tests committed. A cross-layer schema change belongs in one coherent
checkpoint even when it spans several crates. No disabled checks, fake targets or
production stubs are allowed. Record any unexpected intermediate failure here and
repair it before proceeding; do not repeatedly rerun an unchanged known failure.

### HM00 — Freeze contracts and test owners

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
If separation requires UP first, record that gate, not a hidden expansion of HM.

### HM01 — Checked type exports through artifacts and reload

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

HM00 records the remaining selected SDK/source-free/codec/native targets after
checking their actual feature gates. Use default Cargo profiles, target and
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
not undecided policy; HM00 must resolve representation, the handoff and any ordering
dependency, not reopen public semantics.

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
- [ ] HM00 — Live-handle/callable representation, internal version/lifetime/native/type
  distinctions, UP sequencing and linked-design alignment, alias decisions, exact
  API signatures and selected test inventory; confirmed public semantics stay fixed.
  Paused and incomplete: source audit recorded namespace/ownership boundaries;
  the user authorized the alias prerequisite below before further HM/UP work.
- [ ] HM01 — Type export pipeline, independent validation and reload comparison.
- [ ] HM02 — Installed Program/module capability separation and consumer migration.
- [ ] HM03 — Relative module/function/type binding and ordinary examples.
- [ ] HM04 — Integration, lasting documentation and handoff.

No implementation, tests, build or performance acceptance is claimed at plan
creation. No known implementation failure has been introduced. HM00 must resolve
the explicit representation/lifetime/UP sequencing gates before implementation
advances. The representation and admission of retained live handles, internal
version retention, native dependency validity and UP ordering remain design choices.
Latest-published lookup, latest ordinary calls without reacquisition/rebinding,
unit-success update/reload, non-unloading handle Drop and the finite public object
lookup scope are confirmed. Neither a handle rename nor this docs update
establishes runtime support. The UP design, roadmap and review record the same
confirmed public model. The authorized sequence is recorded above; the joint
contract checkpoint is paused and incomplete, and HM/UP implementation phases
remain unstarted. The separate language prerequisite below is locally complete;
HM/UP have not resumed.

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
remain paused and incomplete; this is not a completed joint design checkpoint.

Other source-derived handoff findings (representation details still need final
review when HM/UP resume):

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
  prerequisite is locally complete; HM00/UP00 remain paused and incomplete.

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
