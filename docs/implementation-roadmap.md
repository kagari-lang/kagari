# Kagari Implementation Roadmap

This is the single queue and progress owner for pending work. AC01-AC05 are active
under the continuous implementation goal. AC01-AC04 are complete; AC05 is in progress. Other queued proposals are outside this goal.
Implemented behavior belongs in [architecture](architecture.md) and
[specifications](README.md#language-and-execution-specifications); completed phase
checklists, intermediate errors and execution logs remain in Git history.

## Current baseline

The compiler-to-MIR/bytecode pipeline, source-free artifact validation, synchronous
native calls, registered GC storage, shared generic interface methods, collection
and String APIs, scoped definition identities, installation-based access and
cooperative cancellation are implemented. Physical ABI and semantic contracts are
separate. Core trait source and checked language-role collection are implemented;
native library ingestion and declaration-driven policy selection are implemented.
Source/tooling ownership and installation checks are integrated; final replacement
acceptance is AC05 work.

Immutable preparation, foundation registration reuse and shared interface metadata
are implemented. Remaining measured costs and reproduction commands live in
[performance measurements](performance-baseline.md). No carried build/test error
is recorded by the last implementation checkpoint; this documentation edit does
not rerun or renew that acceptance.

## Contract and common responsibility cleanup

Scope: narrow `kagari-abi` plus source-independent `kagari-contract`.
Syntax-required traits enter ordinary source analysis with language-role bindings;
collection and standard-library declarations remain Rust-authored native libraries
generating `.kgr` for compiler/LSP analysis. Library catalogs belong to those owners,
not generic compiler or executable models. The proposed partition is 24 core
language items (21 syntax consumers plus Eq/Hash/Ord implicit value semantics)
and 14 ordinary native-library traits; all 38 remain mandatory.
The [architecture proposal](architecture.md#contract-and-common-responsibility-cleanup)
defines the [trait inventory](architecture.md#proposed-core-trait-inventory),
[ABI data inventory](architecture.md#narrow-abi-data-inventory), authority,
analysis/registration flows, syntax bridges and dependency rules.
Implementation is active; core source and language-role parser/HIR support are complete.

Agreed priority: complete **AC01, the ABI/contract split, first**. Source-authored
core traits and `#[lang]` handling follow in AC02; native-generated declaration
integration and collection-policy replacement follow in AC03. This documentation
agreement fixes the implementation sequence. See the
[implementation order](architecture.md#agreed-implementation-order).

Finite scope: separate physical ABI from portable semantic contracts, replace
compiler-wide library recognition with normal declaration/trait analysis, collect
and validate actual language roles, connect generated native declarations to their
authoritative records, and migrate registration, tooling and affected consumers
together. Review common's source utilities, identities, numeric semantics and
host schemas in the same sequence. Resolve duplicate declaration ownership
rather than adding aliases.
The source/tooling and foundation-role owner names follow the consumer map; do
not create extra crates solely to satisfy this list.

Phases, in order. One implementation request may activate the complete AC01-AC05
track; that scope does not require a separate approval for each phase:

- [x] **AC01: Extract the narrow ABI and semantic contract boundary.** Inventory
  every current ABI/common module and its production/build consumers. Retain
  physical representations, helper signatures, entry descriptors and physical roots
  in ABI; extract semantic types, declarations, logical layouts, call records
  and focused verification into contract. Move source generation to tooling.
  Apply the [naming policy](architecture.md#naming-policy-for-the-split): Ty,
  NominalTy, FnDecl, TraitDef and other meaning-based names replace semantic
  Abi prefixes/suffixes. Do not substitute a blanket Contract suffix, copy rustc
  layouts or add speculative wrappers. Reconcile authoring/portable module records
  and remove redundant aliases while migrating consumers.
  Migrate direct imports with no forwarding API. Produce an acyclic graph with
  `contract -> abi` and no frontend dependency from executable consumers. Existing
  foundation declarations can retain their current authority at this checkpoint,
  but live with explicit language or native-library ownership rather than ABI.
  Audit the proposed 24/14 trait partition against every consumer, including
  implicit Eq/Hash/Ord, RangeBounds registration, conversion derivation and numeric
  adapters. Split logical slot maps and function/debug metadata from physical
  native artifacts; current logical stack maps are not native GC integration.
  Classify NativeTypeKind/NativeTypeConstructor,
  StandardEnum, CollectionAccess and storage layout consumers as actual syntax
  bridges, library policy or independently required representation/validation.
  Record bounded retained exceptions and their owners. Preserve independently
  required representation tags; relocating an enum does not complete the later
  replacement of its generic library-policy consumers.
  AC01 acceptance: affected consumers build against the new owners; ABI imports
  no semantic type/declaration model, foundation catalog or source renderer;
  contract remains source-independent and all existing loading checks survive.
  Retain current Rust-authored foundation definitions and language behavior outside
  ABI until their owning later phase. Implementing `#[lang]`, moving traits into
  source, replacing library recognition and broad common cleanup are not AC01
  prerequisites. Carried library recognition must be recorded for AC02/AC03,
  rather than reported as a completed policy migration.
- [x] **AC02: Analyze core language traits and collect language roles.**
  Parse/lower the new attribute; collect declaration IDs after headers are known,
  before semantic rules need them. Validate role uniqueness, origin, required
  declaration/member shapes and missing required roles. Collect headers before
  checking role-dependent bodies without injecting duplicate foundation records.
  The parser recognizes declarations/attributes; HIR owns semantic selection.
  Cover both the 21 syntax roles and the three retained implicit-value roles.
  Exercise a source-declared addition trait, operator selection and a same-named
  application trait; add no second trait model or compiler-wide string matching.
- [x] **AC03: Analyze native-generated declarations and localize collection policy.**
  Keep Rust native definitions authoritative for ordinary library traits, types,
  methods and implementations. Generate `.kgr` declarations for compiler/LSP
  analysis through the same declaration/selection machinery as handwritten traits.
  Validate source/record correspondence; remove duplicate injected library catalogs.
  Replace collection-specific type/trait recognition with nominal declarations,
  parent/implementation records and checked member/call identities. Keep a small
  explicit bridge for `[T]`, array construction and existing indexed assignment;
  do not use unchecked method names or introduce new traits/literal syntax.
  Private library registration/storage enums may remain, but an ordinary new
  container must not require a new generic HIR/contract/execution variant.
  Preserve signatures, bounds/defaults, mandatory availability, native storage
  checks and checked intrinsic facts. Representation exceptions follow AC01's
  consumer audit, not a blanket engine instruction redesign.
- [x] **AC04: Integrate loading, registration, tooling and common ownership.**
  Keep executable products and native registrations independent of syntax/HIR;
  check reserved roles and declarations against installation. Native bindings
  consume exact checked declarations; navigation uses handwritten language source
  or generated native views according to authority. Complete common
  ownership moves identified in AC01 without copying numeric semantics or weakening
  host storage restrictions. Review RuntimePrimitive entries as execution helpers,
  not trait definitions. Preserve interpreter/native backend behavior, roots,
  cancellation and generation-pinned reload. Do not add general downcast support.
- [ ] **AC05: Final integration and replacement acceptance.** Resolve all carried
  errors, update affected artifacts once at a coherent checkpoint, remove retired
  catalogs/paths and verify native source/record and language product correspondence.
  Review handwritten imports, visibility, module ownership and effective LOC.
  Run workspace structure, formatting, strict Clippy, tests and diff checks, the complete
  language-contract matrix, and standalone source-free/native backend consumers.

Use `Roadmap-Step: AC01` through `AC05` on future implementation checkpoints.
Each checkpoint should build and pass its focused checks; a necessary intermediate
failure must be bounded within its owning phase and recorded with command,
diagnostics and follow-up. Do not publish an executable bundle with unchecked
roles or contracts. Reuse successful checks until a relevant change warrants
rerunning them; run the full matrix at AC05.

### Continuous goal execution and phase commits

The intended full-track execution is one continuous goal covering AC01-AC05,
including implementation, verification and commits. The user may launch it
overnight and review the results later. Once that full scope is activated, finish
AC01, commit its checkpoint, then continue AC02-AC05 automatically. Phase
boundaries are reviewable commits, not requests for approval or reasons to end
the goal. Preparing these instructions does not itself launch implementation.

Create one coherent Conventional Commit per completed phase, with the exact
`Roadmap-Step: AC01` through `Roadmap-Step: AC05` trailer. Include that phase's
code, tests, affected documentation and checklist/status update in the same
commit. Do not squash the five phase checkpoints together or rewrite them during
later phases. Later integration fixes belong to the phase that performs them.

Before each checkpoint, review structure/imports and run the structure checker,
formatting/diff checks and focused checks appropriate to the phase. AC01 must
also demonstrate the ABI/contract dependency boundary. Resolve failures owned
by the phase before calling it complete. The existing bounded intermediate-error
policy applies during work; it does not waive a phase's acceptance. AC05 runs
the final workspace commands, behavior matrix and standalone feature/backend
checks, resolves every carried error, and only then creates its final commit.

Resolve routine module placement, API shape, import and fixture choices using
the documented ownership rules and inspected consumers. Record material design
decisions here without reopening settled boundaries. Do not wait for the user
between phases or stop after merely describing the next phase. Keep unrelated
queued designs outside this goal.

On continuation or restart, use the phase checkboxes, Git trailers, working-tree
diff and recorded outstanding issues to resume the unfinished phase. Preserve
uncommitted work; do not restart completed migrations or rerun unchanged checks
without a relevant reason. Keep resumable decisions/errors in this roadmap and
temporary command output under ignored `target/`.

If required external information, unavailable tooling or a material scope
conflict prevents further progress, record the exact blocker and finish any
independent authorized work. Do not fabricate success, weaken validation or mark
the goal complete with required work outstanding. Overnight execution expresses
the desired workflow, not a guarantee that a machine finishes by a clock deadline.

The final handoff for the user's review must state phase completion and commit
hashes, resulting crate/data ownership, validation actually performed, and any
remaining errors or limitations. AC01-AC05 checkboxes, current baseline and
architecture/specifications must reflect the implemented result. An incomplete
run must identify its unfinished phase and resumable work clearly.

### Active decisions and retained consumers

AC01 extracts `kagari-contract` and migrates direct consumers without forwarding
exports. ABI depends only on Serde; contract depends on ABI/common and has no
frontend/build dependency. Native declaration rendering belongs to HIR's
`native::render`, used only by source tooling and dev consumers. Runtime exposes
its checked declaration projection rather than generating source. Native products
pair contract-owned function identities, logical safepoints/slots and debug data
with ABI-owned `NativeArtifact` and executable page ownership. Register/Local maps
remain logical coverage; no physical GC publication is claimed.

The AC01 module/consumer audit is:

| Starting modules | Owner and production consumers |
| --- | --- |
| `representation`, `native_call`, `version`, physical `native` | ABI; contract lowering, codegen, Cranelift, runtime, VM and SDK |
| `types`, `callable`, `declaration`, `native_import`, `layout`, `slots`, `contracts` | Contract; HIR/compiler, MIR/bytecode verification, runtime linking and execution metadata |
| `scalar`, `numeric`, `operations`, `effects`, `standard`, `host`, `ids`, `decode_limits` | Contract; checked semantic/operation facts and bounded portable verification, shared with source and executable consumers |
| Starting `language`, `language/catalog`, `language/primitive` | Contract core roles/product/implicit semantics; AC03 moves Rust registration catalog to `library/catalog` and library capabilities/adapters into checked records |
| `declaration/render` | HIR tooling; no executable consumer or ABI dependency on generated source |
| Common `identity` and its map/metadata/reference/table modules | Portable identity machinery stays common; FileId/Revision/FileSpan source records move to source ownership in AC04 |
| Common `source`, `source_database`, `line_index`, `diagnostic`, `literal` | Source/tooling ownership move in AC04; shared span coordinates remain available to executable debug metadata |
| Common `arithmetic`, `integer`, `numeric`, `cancellation`, `decode_limits` | Shared mechanisms stay common; numeric behavior keeps one implementation |
| Common `host_interface`, `collection`, `range` | Portable host schema/access and range shape facts stay source-independent; AC04 reviews host schema ownership with its executable consumers |

`ModuleDecl` owns authoring registrations (documentation, exports, implementation
templates and callback requirements); `ModuleContract` owns the serialized checked
executable subset (public items, private traits and native declarations). They are
different responsibilities, not equivalent models or compatibility aliases.

The 24/14 trait partition is confirmed by consumers: operators, calls, indexing,
iteration, formatting and Result propagation select 21 syntax traits; implicit
value/composite/identity eligibility also needs Eq/Hash/Ord. RangeBounds already
uses registered implementation selection; range constructors remain syntax
bindings. Into/TryInto derive from From/TryFrom, and numeric conversion adapters
retain checked implementation facts. AC03 replaces library recognition with checked storage and conversion adapter
records; inherited methods follow normal declared parent closure.

Bounded retained representation exceptions: StandardEnum keeps Option/Result
propagation, enum payload validation and tracing; RangeKind keeps endpoint shape
validation; CollectionAccess keeps readonly/writable host/reference checks;
NativeStorageLayout keeps registered payload capabilities and parameter validation.
NativeTypeConstructor and HIR NativeTypeKind currently also recognize default
containers and cursor families; their generic library-policy consumers belong to
AC03. RuntimePrimitive describes checked execution helpers, reviewed in AC04.
Relocation does not count as replacing these policy consumers.

AC01 acceptance passes: workspace all-target compilation; nine production graphs
and ABI/contract build graphs; source-free SDK compilation; contract/bytecode and
Cranelift suites; runtime native execution, native builder, offline types and
installation access; HIR language contracts; structure, formatting and diff checks.
No carried build/test error remains. Temporary output lives in `target/ac-cleanup`.

AC02 replaces Rust constructors for the 24 core trait declarations with handwritten
`library/core/language.kgr` and its source-compiled `language/traits.bin` product.
Only source tooling includes the text; contract decodes and validates the bounded
product without a frontend. `regenerate_language_traits --check` checks source/
product correspondence through checked HIR and compiler declaration projection.
Core traits use ordinary trait lowering inside the foundation module; native
library ingestion remains the AC03 transition. A source-independent LangRole
inventory contains only the 24 language roles. HIR collects actual IDs after
headers, validates installed origin/uniqueness/presence/member shapes, and carries
that mapping through identity scoping into body selection. No new trait semantics
or optional library availability is introduced.

AC02 acceptance passes: workspace all-target compilation; all syntax/HIR/compiler
tests (80 syntax, 410 HIR unit, eight language contracts, 161 compiler unit,
four foundation and ten source-program tests); operator/callable/iteration,
standard/conversion, Result/Option and interpolation embedding suites; a same-named
application Add trait executes alongside the source-authored core Add through
direct, encoded and native execution. Six role tests cover all 24 roles and invalid
origin, presence, uniqueness, syntax, visibility, binders, parents and member shapes.
The checked core product regeneration comparison, structure (725 files), formatting
and diff checks pass. The discovered documentation/inventory and standalone indexed
assignment regressions are resolved. Native view documentation remains in native
registration records until AC04's source-navigation ownership integration. No carried
build/test failure remains; command logs are in `target/ac-cleanup/ac02-*.log`.

AC03 replaces direct native record-to-HIR ingestion with ordinary declaration
parsing/lowering. Non-trivia view correspondence is checked before installed
storage, bindings and default metadata attach; core traits retain semantic role/
shape checks. Rust registration ownership moves to `contract::library::catalog`;
its 38-entry RegistrationTrait key is private to that owner. `language::Protocol`
contains only the 24 core roles. No generic consumer recognizes library traits
through that enum.

Native TraitDef records carry installed storage access and conversion adapters.
Storage joins/inference, readonly portable matching, identity/equality and dynamic
call checks read declarations and implementation/parent records. The explicit
`builtin::array_bridge` owns `[T]` context and existing indexed assignment. Ordinary
new native containers use nominal NativeStorage registration and the same trait
records. Storage implementations retain object/nominal ownership checks. Key
eligibility is rechecked with the complete catalog after header collection.
Into/TryInto retain exact forward method/error identities; TryFrom carries its
checked scalar adapter. Portable adapter shape checks and exact installation
checks preserve authority without source dependencies or a new blanket feature.

AC03 acceptance passes: workspace all-target compilation; 412 HIR unit and eight
language-contract tests;
five collection-access, five collection-interface, 15 conversion, 17 provider-reset
and one standard-declaration embedding tests; checked source/product comparison;
51 contract unit tests, 29 bytecode tests, 13 native-default and two bytecode doc
tests; 13 runtime native-builder and three installation tests; six language-role
regressions; strict Clippy for contract/HIR/runtime/compiler/bytecode all targets.
Earlier cursor-name,
deferred interface-key and enum-based test-lookup failures are resolved. The four discovered earlier role/rendering Clippy findings and the new redundant
borrow are resolved. Structure (731 files, no exceptions), formatting, import/
ownership review and diff checks pass. No carried build/test failure remains.
Executable fixture regeneration and the full feature/backend matrix belong to
AC05; unmodified checked products are not repeatedly rebuilt. Logs are under
`target/ac-cleanup/ac03-*.log`.

AC04 migrates source documents, diagnostics, literal grammar, line indices and
FileId/Revision/FileSpan into `kagari-source`, with direct frontend imports,
optional compiler/SDK source dependencies and dev-only backend/VM consumers.
Portable identities/debug spans, numeric semantics, cancellation, decoding and
host/access/range schemas remain common mechanisms with executable consumers.
RuntimePrimitive's eight entries are checked execution helpers, not declarations.
Core documentation now belongs to handwritten source. Native views copy core
fragments exactly; retained source provenance makes navigation point into the
handwritten file while declaration-site analysis still uses generated offsets.
Snapshots retain both sources, and docs queries validate the translated location.
Reserved public/private roles and installed native capabilities are checked at
load and reload even without native imports; foundation products require all 24
public core declarations.

AC04 acceptance passes: workspace all-target compilation; 412 HIR and seven
source-tool unit tests; four installation tests including reserved forgery,
missing/duplicate/private roles and reload without native calls; 13 source-snapshot,
17 provider-reset, eight generic-reload, three Cranelift-preparation and one
native-preparation embedding tests. Strict Clippy passes for source/HIR/runtime/SDK
all targets. The checked core source/product comparison, nine production dependency
boundaries and ABI/contract build graphs pass. Structure (733 files, zero exceptions),
formatting, ownership/import review and diff checks pass. The missing benchmark
consumer, test-only Span import, old generated core navigation expectations and
private-test owning-module mistake are resolved; no carried error remains.
Command logs are under `target/ac-cleanup/ac04-*.log`. AC05 owns the complete
workspace/behavior/feature/backend checks and coherent executable fixture updates.

Acceptance includes both sides of the boundary: source-defined operator traits,
native-generated library traits and application traits use the same record/selection
machinery; malformed, duplicate, missing or counterfeit language roles are rejected;
generic/associated contracts, native callbacks, interface dispatch and primitive
behavior remain valid. Loading
a program without source/HIR must still verify contracts against the installed
foundation/native declarations. Generated declarations that disagree with their
Rust authority are rejected. Preserve List/MutableList behavior, custom collection
implementations, readonly/writable dispatch, array literal evaluation order and
construction/mutation failures. Ordinary library registration must not depend on
matching a central collection enum. A checked signature must not be treated as proof
of a trusted Rust body's effects. ABI must have no dependency on semantic type
records, source generation or a generated trait catalog.
Preserve existing `..`/`..=` range syntax, inclusive iteration and registered
RangeBounds implementations independently of the trait's declaration owner.

No new traits, containers, library algorithms, blanket standard-enum/storage
instruction replacement, execution-policy redesign, general downcasting,
compatibility workflows or external FFI implementation are included.
`kagari-ffi` remains only the future external C adapter boundary. This queue entry
owns phase progress until activation; no parallel plan or implementation has been
created. Design validation is documentation/link and diff review, not a workspace
build. Performance effects remain unmeasured.

### Handoff without conversation history

This track can be implemented from a checkout of the repository without the
original conversation. Transfer the committed source, tests, manifests/lockfiles
and documents together; this roadmap alone is not the complete contract. No
absolute machine path, prior chat, ignored `target/` cache or local measurement
log is an implementation prerequisite. Rust/Cargo compatible with the workspace
and `uv` are required. The rustc links explain naming only; the local naming table
and Kagari specifications define the target, independently of later rustc changes.

Read [AGENTS.md](../AGENTS.md), [project goals](project_goal.md), this track and
the [architecture cleanup](architecture.md#contract-and-common-responsibility-cleanup),
including its naming, trait and ABI inventories. Consult the linked specifications
for behavior. Inspect `git status`, relevant diffs and the latest AC checkpoint
before selecting work. This plan is queued until an implementation request selects
its scope; documentation agreement is not an instruction to start every track.

AC01 is complete with the ABI/contract split performed; AC02-AC05 are
unchecked. Rust-owned foundation catalogs remain deliberately transitional until
AC02/AC03. Complete AC01 acceptance before AC02. Update the status and checkboxes
when implementation advances. Record only material decisions, retained transitional
consumers and carried errors with their command, cause and owning follow-up phase.
Commit with the phase trailer above so another checkout can resume from Git.

Choose exact source/tooling/foundation module locations and registry APIs from
the production consumer graph during their owning phase. Those implementation
choices remain open; the dependency direction, declaration authorities, mandatory
availability and semantic preservation rules above are fixed requirements. If
the audit finds a conflict with the proposed trait partition or an unsupported
adaptation requirement, document the evidence here before changing the design.
Do not recover missing decisions by guessing what a previous conversation meant.

Current code entrypoints, to be updated when their owners move:

| Work | Starting locations |
| --- | --- |
| ABI/contract model and verification | [Contract root](../crates/kagari-contract/src/lib.rs), `types/`, `callable/`, `layout.rs`, `slots.rs`, `contracts.rs`, `native_import/` under that crate |
| Physical representation and native boundary | [Value representations](../crates/kagari-abi/src/representation.rs), [native calls](../crates/kagari-abi/src/native_call.rs), [native products](../crates/kagari-abi/src/native.rs) |
| Existing foundation definitions and generated source | [Language catalog](../crates/kagari-contract/src/library/catalog/mod.rs), [native declarations](../crates/kagari-contract/src/declaration/mod.rs), [source renderer](../crates/kagari-hir/src/native/render.rs) |
| Attribute analysis and language selection | [Syntax attributes](../crates/kagari-syntax/src/ast/item.rs), [HIR entry](../crates/kagari-hir/src/lib.rs), HIR `lower/`, `language/`, `typeck/` and compiler `source/lower/` |
| Installation and executable consumers | Runtime `native/`, `loading.rs`, `backend.rs` and `backend/native.rs`; MIR, bytecode, VM, codegen and embed consumers of the old ABI model |
| Common ownership and dependency validation | [Common root](../crates/kagari-common/src/lib.rs), workspace/crate manifests and [standalone feature checker](../scripts/check_features.py) |

Extend check_features.py in AC01 to cover the new contract crate's production
and build dependency boundaries, and reject an ABI-to-contract edge. A workspace
test can unify dev features; it does not alone prove a source-free consumer.

Reuse existing focused coverage rather than inventing structural tests that only
repeat the rename. This is the minimum behavior matrix for AC05, with affected
rows selected at earlier checkpoints:

| Behavior | Existing coverage / required extension |
| --- | --- |
| Foundation and role selection | HIR `language_contracts`, compiler `language_foundation`, embed `operator_traits`, `callable_traits`, `iteration_traits`; add malformed/duplicate/missing/counterfeit role cases in AC02 |
| Generic and associated contracts | Embed `associated_types`, `generic_associated_types`, `associated_constants`, `trait_inheritance`, `default_methods` |
| Value, conversion and formatting semantics | Embed `standard_traits`, `conversion_traits`, `numeric_operations`, `result_option`, `string_interpolation`, `string_methods` |
| Native declaration correspondence | Embed `standard_declarations`, `native_provider_reset`, `source_snapshots`; add analyzed/generated declaration mismatch rejection in AC03 |
| Collection and range behavior | Embed `collection_interfaces`, `collection_access`, `array_operations`, `list_algorithms`, `syntax_examples`; VM `library_collections`, `native_boundary` |
| Source-free linking and reload | Embed `artifact_features`, `offline_nominal`, `native_artifacts`, `generic_reload`; runtime `offline_types`, `installation_access` |
| Execution safety and native behavior | [Language conformance](spec/language-conformance.md), runtime `gc_ownership`, `execution_sessions`, `host_borrows`; embed `native_preparation`, `cranelift_preparation` and backend tests |

Target names above are Cargo integration tests except the linked shared
conformance suite and backend unit tests; check crate feature gates before running.
Keep explicit assertions of actual native invocation where supported; JIT fallback
is not evidence that arbitrary library or GC operations execute as native code.

On a fresh machine, fetch locked dependencies before offline feature checks:

```text
cargo fetch --locked
cargo run --locked -p kagari-embed --no-default-features --features source --example regenerate_feature_artifact
uv run python scripts/check_features.py
```

The [feature fixture](../crates/kagari-embed/tests/fixtures/README.md) is disposable
and can be recreated from checked-in source; the feature checker also regenerates
it before its standalone matrix. Generate it before running source-free tests
directly. Use the final commands in [verification policy](#execution-and-verification-policy)
in addition to this matrix. These commands are future implementation checks,
not a claim that they ran during documentation preparation.

A self-contained full-track goal request is:

```text
Goal: Complete AC01-AC05 of docs/implementation-roadmap.md in this checkout.
Read AGENTS.md and the linked architecture/specifications first.
Split kagari-contract from the physical kagari-abi and migrate its consumers,
using the documented naming table. Then implement source-authored core traits
and validated language roles, native-generated library declaration analysis,
library-policy localization, loading/tooling integration and common cleanup.
Preserve the documented language, storage, generic, source-free validation,
cancellation, GC and reload behavior. Keep unrelated queued designs out of scope.
Execute all five phases continuously; after each phase's acceptance, create one
Conventional Commit with the matching Roadmap-Step trailer (AC01 through AC05)
and continue immediately.
Do not wait for confirmation between phases. Resolve routine implementation
choices from the documented boundaries and actual consumers.
Keep this roadmap's checklist, current status and material decisions/errors
up to date so execution can resume without this conversation. Run the required
focused checks, then the complete AC05 acceptance matrix and workspace checks.
Complete the goal only when all five phases and required checks are finished.
For the user's later review, report the five commits, implemented boundaries,
actual validation and any remaining blockers or limitations.
```

## Other queued designs

These are design documents, not additional active execution plans. Activation and
exact scheduling must be agreed within the user's scope. Completed migrations are
not prerequisites to replay.

| Track | Scope and dependencies |
| --- | --- |
| [Rust interoperability](rust-interop-design.md) — RI00-RI05 | Recursive typed value conversion, optional schema-backed Serde and retained opaque objects. Preserve host-managed mutation; exposing Rust borrows is separate. |
| [Host API](host-api-refactor.md) — HA00-HA05 | Preparation/load/call/reload facade; depends on RI conversion/root contracts. Package identity and update compatibility must be frozen before affected phases, without requiring both entire tracks first. |
| [Packages](package-design.md) — PK00-PK04 | Cargo-style manifests, exact dependency graphs and source/module identities. First source kinds, single-selection policy and defaults remain review choices. |
| [Update model](update-model-design.md) — UP00-UP05 | Compatible publication versus explicit state replacement. RI supplies conversion/roots, PK identities and HA the facade; lower-level cutover belongs to UP. |
| [Async execution](async-execution-design.md) | Host-driven awaited native IO and owned execution lifetimes. Task/completion contracts need review; synchronous acceptance does not require async. |
| [Host task scopes](host-task-scope-design.md) | Scope-owned work launched by synchronous handlers, host/Actor dispatch and bounded driving; no Actor/Tokio policy inside the VM. |

Ordinary callback reentry remains synchronous. Async designs must later share
the same quiescence, cancellation, late-completion and pinned-generation rules.
No new restoration checklist for retired standard-library APIs is active.

## Outstanding review and performance questions

[The architecture review](architecture-review-2026-10-03.md#outstanding-findings)
retains R1/R2/R5/R7/R8 for current-code confirmation: raw-kind safety, prepared-JIT
policy, trait-search cancellation/bounds, repeated whole-program specialization
and semantic cache invalidation. R3/R4/R6 have subsequent implementations; their
remaining costs are measurement questions, not instructions to replay those fixes.
Native expansion requires checked call/root/effect contracts and actual-native
conformance. LLVM and broader interpreter optimization remain separate choices.

The identity measurements retain three finite follow-up costs: transient authoring
path projections, table-index copies on retained-prefix append, and normalized
metadata copies across independent runtimes. Compact IDs do not prove whole-module
sharing or universal speedup. These observations do not activate another migration.

## Execution and verification policy

At activation, use the selected track's finite phases and commit trailers. Keep
unfinished decisions and genuinely carried errors here or in its owning design;
record a command, cause and bounded follow-up. Replace obsolete internals directly,
preserving static typing, bounds, roots, declared access and generation validation.
Do not accumulate successful command logs or completed per-commit narratives.

Use focused checks during development. Final architecture integration runs the
track's behavioral/feature matrix, source-free/backend consumers and:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Documentation-only edits need content/local-link and diff checks. Measurements
must identify workload, machine, toolchain, profile/features/parallelism and cache
state, separating compilation, preparation and execution. Repository-wide workflow
and exception rules live in [AGENTS.md](../AGENTS.md).
