# Kagari Implementation Roadmap

This is the single queue and progress owner for pending work. No code migration
is currently active. Documentation cleanup does not activate any queued proposal.
Implemented behavior belongs in [architecture](architecture.md) and
[specifications](README.md#language-and-execution-specifications); completed phase
checklists, intermediate errors and execution logs remain in Git history.

## Current baseline

The compiler-to-MIR/bytecode pipeline, source-free artifact validation, synchronous
native calls, registered GC storage, shared generic interface methods, collection
and String APIs, scoped definition identities, installation-based access and
cooperative cancellation are implemented. Current foundation declarations remain
compiler-owned; the cleanup below proposes a new owner/analysis split.

Immutable preparation, foundation registration reuse and shared interface metadata
are implemented. Remaining measured costs and reproduction commands live in
[performance measurements](performance-baseline.md). No carried build/test error
is recorded by the last implementation checkpoint; this documentation edit does
not rerun or renew that acceptance.

## Contract and common responsibility cleanup (queued)

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
Implementation remains queued; no new parser support is claimed.

Agreed priority: complete **AC01, the ABI/contract split, first**. Source-authored
core traits and `#[lang]` handling follow in AC02; native-generated declaration
integration and collection-policy replacement follow in AC03. This documentation
agreement does not activate Rust implementation. See the
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

- [ ] **AC01: Extract the narrow ABI and semantic contract boundary.** Inventory
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
- [ ] **AC02: Analyze core language traits and collect language roles.**
  Parse/lower the new attribute; collect declaration IDs after headers are known,
  before semantic rules need them. Validate role uniqueness, origin, required
  declaration/member shapes and missing required roles. Collect headers before
  checking role-dependent bodies without injecting duplicate foundation records.
  The parser recognizes declarations/attributes; HIR owns semantic selection.
  Cover both the 21 syntax roles and the three retained implicit-value roles.
  Exercise a source-declared addition trait, operator selection and a same-named
  application trait; add no second trait model or compiler-wide string matching.
- [ ] **AC03: Analyze native-generated declarations and localize collection policy.**
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
- [ ] **AC04: Integrate loading, registration, tooling and common ownership.**
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

The starting status is AC01-AC05 unchecked, with no code split performed. Current
names and Rust-owned foundation catalogs in source are expected, not evidence
that the target design was rejected. Start with AC01 when implementing this
track; complete its acceptance before AC02. Update that status and the checkboxes
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
| ABI/contract model and verification | [ABI root](../crates/kagari-abi/src/lib.rs), `types/`, `callable/`, `layout.rs`, `slots.rs`, `contracts.rs`, `native_import/` under that crate |
| Physical representation and native boundary | [Value representations](../crates/kagari-abi/src/representation.rs), [native calls](../crates/kagari-abi/src/native_call.rs), [native products](../crates/kagari-abi/src/native.rs) |
| Existing foundation definitions and generated source | [Language catalog](../crates/kagari-abi/src/language/catalog/mod.rs), [native declarations](../crates/kagari-abi/src/declaration/mod.rs), [source renderer](../crates/kagari-abi/src/declaration/render.rs) |
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
