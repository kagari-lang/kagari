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

Proposed phases, in order, to be activated explicitly for implementation:

- [ ] **AC01: Extract the narrow ABI and semantic contract boundary.** Inventory
  every current ABI/common module and its production/build consumers. Retain
  physical representations, helper signatures, entry descriptors and physical roots
  in ABI; extract semantic types, declarations, logical layouts, call records
  and focused verification into contract. Move source generation to tooling.
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
