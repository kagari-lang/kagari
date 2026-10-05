# Kagari Implementation Roadmap

This is the single queue and progress owner for pending work. GO01-GO06 are complete,
as are the earlier AC, CR, LR and EN tracks. Other queued proposals require separate
activation; this completed goal does not start another architecture migration.
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
Source/tooling ownership, installation checks and final replacement acceptance
are complete.

Immutable preparation, foundation registration reuse and shared interface metadata
are implemented. Remaining measured costs and reproduction commands live in
[performance measurements](performance-baseline.md). No carried build/test error
remains after GO06 final integration. Performance changes from this cleanup
have not been measured.

## Crate responsibility migration (CR01-CR02, design agreed)

CR01 and CR02 establish the semantic/executable boundaries before LR01-LR03
integrates the library registration flow. The [agreed crate target](architecture.md#crate-responsibility-target)
separates shared declaration/type semantics from executable contracts and physical
ABI. It extends the earlier library-only proposal explicitly; it does not mark
the completed AC01-AC05 architecture as failed or reopen unrelated features.
Implementation is active under the continuous goal. The user authorized one
commit per completed phase, in order: CR01, CR02, LR01, LR02 and LR03.

- [x] **CR01: Establish the shared semantic owner.** Populate `kagari-types`
  with the existing source-independent types, signatures, trait/generic/member
  definitions, module declarations, documentation and semantic language-role
  metadata. Move general substitution, matching, inheritance and declaration-level
  constraint checks there. Move common's collection access, range forms, numeric
  evaluation algorithms and offline host declarations/access schemas to focused
  modules in types. Preserve one numeric implementation for compile-time and
  runtime evaluation. Keep common focused on spans, cancellation and definition
  identity/table/mapping infrastructure. Split mixed records before moving them:
  pure declarations and symbolic defaults belong to types; selected executable
  targets, concrete native imports, result/callback adapters and physical bindings
  do not. Classify proof operations by inputs: declaration-level type obligations
  belong to types, linked executable dependency checks remain in contract. Types
  must not depend on ABI, contract, source, syntax, HIR or runtime. Retain bounded
  encoding/checks with the records they protect, without introducing a metadata
  product, compatibility facade or empty forwarding crate. Migrate affected
  producers/consumers directly and record necessary intermediate errors here.
- [x] **CR02: Enforce semantic and execution boundaries across consumers.**
  Remove HIR's unused direct ABI dependency and all contract references. HIR uses
  source, syntax, types and common; inference variables, unknown/error states,
  resolution and trait selection remain in HIR. Replace declaration fields that
  expose execution models with semantic records and symbolic declaration links.
  Move `Ty::representation()` and host-to-slot conversion to contract's focused
  representation lowering API, invoked by compiler lowering and executable
  verification, not by HIR/types. Keep logical slots/layouts, selected call and
  native dependencies, operations, effects, executable envelopes and linked checks
  in contract; keep physical representations/calls in ABI. Migrate compiler,
  MIR, bytecode, backends, runtime, VM and embed to import from actual owners.
  HIR declaration ingestion/rendering receives explicit types records and providers;
  remove contract catalog injection/fallback at this checkpoint. Existing Engine
  and standalone caller/test producers supply the current foundation explicitly
  until LR replaces its authoring owner. Do not move the catalog into types/HIR
  to remove a dependency. Preserve generic scopes, interface defaults, host
  signatures/access/borrows, artifact checks and generation identity. Update
  production dependency assertions and source-free feature consumers.

CR acceptance requires:

- HIR's project dependencies are source, syntax, types and common; its production
  transitive graph contains no ABI, contract, runtime, stdlib, MIR or backend.
- Types depends only on common among project crates. Common has no host/type/API
  inventory; ABI remains independent of semantic models. No re-export/forwarding
  facade or duplicate semantic implementation hides forbidden dependencies.
- The same declaration models serve source analysis, registration and source-free
  loading. HIR's recoverable type state remains distinct from checked portable
  semantic records; it is not forced into an executable or physical type model.
- Compile-time and runtime numeric behavior remain consistent. Native/script
  generics, traits, associated outputs, defaults and host-path validation retain
  meaningful coverage. MIR and bytecode continue independent bounded validation;
  runtime/VM remain frontend-free.

CR01 may carry bounded compilation failures while mixed records and consumers
move. Record commands, representative diagnostics and the owning next checkpoint;
do not disable validation or add compatibility entrypoints to make a checkpoint
green. CR02 must close carried boundary-migration failures before LR01 starts.
Run focused semantic/HIR/MIR/artifact/native checks and the relevant standalone
feature consumers at CR02; run the full combined matrix once at LR03 unless a
concrete broad failure requires it earlier. Use `Roadmap-Step: CR01`/`CR02` when
commits. No additional host/numeric/trait/registration
crates, new MIR stages or merged compiler/runtime crates are in scope.

## Unified library registration (LR01-LR03, design agreed)

After CR01-CR02, the library migration follows the
[agreed registration target](architecture.md#unified-library-registration).
Standard and application native modules use one Engine registration path.
Registration owns signatures and Rust bindings; source-enabled analysis receives
generated `.kgr` views of those same declarations. Types supplies semantic models
and declaration checks; contract supplies executable checks. Neither owns a
built-in standard-library catalog. The registration path has no binary
intermediary. Crate metadata and separate/parallel compilation are
deferred to their own future design.

The [registration API](architecture.md#registration-api) makes the
provider input, mutable engine builder, full-doc registration and optional source
cache concrete. The implemented ownership and mutable API follow this single
registration flow; acceptance is recorded in the phase ledger below.

This decision supersedes AC02/NS01's handwritten core-trait authority and checked
`traits.bin` product for the migration. Those checkpoints remain completed history;
their passing checks describe those historical checkpoints, not LR acceptance.
The following phase checklist and ledger record the current implementation.

- [x] **LR01: Give the standard library its own registration owner.** Populate
  `kagari-stdlib` with the existing contract library recipes, documentation,
  prelude/re-export inventory and runtime foundation/collection implementations.
  Provide consistent full-Markdown documentation APIs for modules, traits, types,
  functions, methods and exposed members; preserve docs through builder completion
  and inherited method registration. Standard-library docs include module overviews,
  behavior, relevant failure/mutation rules and meaningful usage examples.
  Express all 38 traits through explicit registration records, preserving names,
  constraints, defaults, identities and semantics. Keep types' semantic models
  separate from contract's execution facts and linked validators. Audit
  `library::namespaces`,
  `language`, `standard`, native constructors and ownership checks individually:
  move API inventories to stdlib; preserve necessary semantic/representation checks
  through explicit validated bindings. Do not relocate source-free checks into
  HIR or make generic runtime depend on stdlib. Replace ModuleBuilder's hidden
  LanguageContracts default with explicit providers. Native implementation moves
  use checked runtime operations, not newly public heap internals.
- [x] **LR02: Connect Engine registration to analysis and runtime installation.**
  Assemble default standard modules and selected application modules through one
  validated registration path. Give HIR the complete registered declaration set;
  retain CR02's explicit provider ingestion with no default catalog fallback.
  Generate core and application `.kgr` views uniformly, with docs, re-exports, aliases,
  signatures and checked role metadata. Parse complete generated modules; recognize
  language attributes through syntax nodes and their owning declarations. Replace
  handwritten-core splicing and copied-text navigation with renderer-recorded
  identity/range mappings and parsed spans in the generated snapshot. Any remaining
  handwritten input during migration uses complete-module parsing and AST ranges,
  not exact attribute strings or blank-line separators. Provide tooling-host
  materialization into a generated-source cache with stable content/version paths
  and file/range navigation matching the analyzed snapshot; keep filesystem IO
  outside HIR, types and contract. Render module `//!` and item/member `///` docs with complete Markdown,
  including fenced examples. Connect module/item documentation queries and hover
  to the same content, with correct spans after multiline docs. Move implicit
  foundation installation out of Runtime construction into Engine composition.
  Preserve atomic dependency
  validation, source/record correspondence and existing snapshot/version ownership.
  Replace old-product shape checks with installed declaration correspondence plus
  explicit language-role requirements. Preserve existing default Engine behavior;
  live registry mutation and a no_std product mode are outside scope.
- [x] **LR03: Remove obsolete products and complete integration.** Delete
  `traits.bin`, its decoder/regenerator and independent handwritten core trait
  declarations once their consumers use registration records; do not substitute
  another binary or generated Rust snapshot. Delete `trait_source`,
  `core_text`, their handwritten-source imports/splicing and copied-text provenance
  searches after LR02 replaces their consumers. Migrate direct HIR/runtime test
  producers to explicit module inputs and update specs, examples and tooling
  navigation expectations. Retain user-program artifact/source-free coverage.
  Run final validation and resolve all carried failures before acceptance.

Acceptance must demonstrate:

- A registered application module's new type/function/trait is analyzed through
  generated `.kgr` and executed without editing types, contract, HIR or VM inventories.
- The CR01-CR02 dependency boundaries remain enforced after stdlib integration:
  HIR/types do not acquire execution dependencies, and generic runtime, compiler,
  bytecode and backends do not acquire a concrete stdlib dependency.
- Standard modules follow that same route; no HIR or generic runtime fallback
  silently restores unregistered declarations. Default Engine still installs the
  full standard library, including the bundled iterator adapter.
- All 38 traits, syntax/implicit-value roles, canonical core/std identity, explicit
  prelude visibility, Vec/Iterable and generated navigation/docs remain covered.
- Standard and application declaration navigation opens actual generated `.kgr`
  files with correct ranges. Unchanged documents reuse paths, changed documents
  preserve active snapshot targets, and in-memory embedding requires no disk IO.
- Core declarations use complete generated-module parsing and ordinary HIR lowering.
  Valid attribute whitespace, blank lines inside traits, multi-paragraph docs,
  fenced examples and attribute-like text in comments do not affect declaration
  selection or navigation. No `trait_source` or equivalent textual extraction
  remains; malformed generated syntax reports an error without partial publication.
- Registration docs survive for modules, traits, types, functions and exposed
  members. Cached source and documentation queries retain headings, paragraphs,
  lists, links and fenced examples. Check inherited/overridden member docs and
  canonical re-export navigation. Doc-only changes refresh tooling output without
  changing executable compatibility or requiring ABI version changes.
- Missing providers, mismatched native signatures, duplicate identities and forged
  roles are rejected. Failed registration publishes no partial module set.
- Artifact-only and native-only hosts use registered declarations without a source
  frontend, declaration binary, generated-source parsing or build-time frontend.
- Existing roots, scoped borrows, cancellation, failure ordering and generation-
  pinned execution/reload checks pass after native implementation moves.

Use `Roadmap-Step: LR01` through `LR03` for implementation checkpoints when commits
are authorized. Use focused checks while migrating; record necessary intermediate
failures with their command,
cause and owning phase here. Final integration requires structure, formatting,
strict workspace Clippy, workspace tests, all four standalone SDK feature consumers,
CLI JIT tests and diff checks. Update dependency assertions for the concrete stdlib
owner, narrow common and semantic types owner; retain the frontend-free
types/ABI/contract/runtime/VM/backend boundaries. Resolve all CR/LR carried errors.

Historical design ledger (implementation follows below): inspected production
manifests and then-current registration consumers.
Concrete coupling exists in contract's catalog/product, HIR's default catalog and
old-product shape checks, Runtime's implicit foundation installation, and
ModuleBuilder/LanguageContracts defaults. Engine already forwards application
declarations to HIR and native bindings to runtimes; extend that path to the whole
standard library. No source or runtime implementation changed in this design
checkpoint. Documentation links/content and `git diff --check` are its validation.
Subsequent design refinements require real cached `.kgr` files for editor
navigation and complete module/item/member docs authored through registration.
At that design checkpoint, implementation had FunctionDecl documentation and an item-ID doc map,
but module/trait/type/method builder coverage and module-doc queries are incomplete;
LR01/LR02 own those gaps.

Source-path audit also found pre-parser exact-attribute/blank-line extraction in
`hir::language::source::trait_source`, splicing through `native::render::core_text`,
and copied-text searches for navigation in `native::api::import_source`. LR02
replaces their consumers with complete generated-module parsing and explicit
ranges; LR03 removes the obsolete helpers. This refinement updates the design
only; the execution ledger below records its subsequent implementation.

Crate audit refinement: HIR declares ABI but has no direct source use; contract's
`Ty::representation()` still couples its shared type model to ABI. Common mixes
identity/span/cancellation infrastructure with numeric semantics, collection/range
forms and a complete offline host schema. CR01/CR02 now own this split before LR01.
The authorized order is CR01 -> CR02 -> LR01 -> LR02 -> LR03. Existing library
requirements, full docs/cache navigation, native safety and source-free artifact
acceptance remain in scope.

Execution ledger (CR01-CR02 and LR01-LR03 complete): numerical evaluation,
collection/range semantics and offline host schemas now belong to `kagari-types`.
Scoped types, generic constraints, substitution, identity traversal, declarations,
symbolic defaults and narrow reserved language-role identities moved from contract.
Declaration shape/binder checks, callback requirement templates, ancestry,
matching and declaration application checks move with their semantic records.
Executable-envelope checks, selected signatures and linked proof catalogs remain
in contract. Physical representation is an explicit contract lowering function.
Module declaration validation receives an explicit receiver-ownership lookup;
the existing canonical provider supplies this until CR02/LR migration. Common
retains identity/location/cancellation and bounded decoding. Host-schema metadata
tests moved to types, preserving cross-scope, encoded-reference and cancellation
coverage without a common/types development dependency cycle.

CR01 validation passes: `cargo check --workspace --all-targets`, strict workspace
Clippy, formatting, structural checks (752 files, zero violations/exceptions),
eleven production dependency boundaries and `git diff --check`. Focused semantic
and HIR checks passed 519 tests; native builder/source-program checks passed 23.
Embedding checks cover source-free functions/nominal types, collection access,
provider resets and standard traits. An import cleanup mistakenly changed 13 raw
KGR fixtures; all were restored and the six resulting Hash failures resolved.
That concrete cross-crate failure justified a full `cargo test --workspace` run:
1640 passed, one ignored. All intermediate import/call/test errors are closed.
The standalone feature/backend matrix remains due at CR02/LR03 as specified.

CR02 implementation now moves logical callable signatures, protocol adapter
signature checks and intrinsic host-schema constraints into types. HIR's production
dependencies contain only source, syntax, types and common. Analysis ingestion,
one-source analysis and declaration rendering receive explicit providers;
one-source analysis returns registration failures through `AnalysisError`, the
current Engine and independent test/example producers choose their foundation.
Prelude selection, installed package spellings, array-interface context and
language-shape validation use those supplied records rather than contract's
catalog. Import catalog construction now has a focused owning module.

CR02 validation passes: 421 HIR unit/integration tests plus the new invalid
registration error regression; 118 types/contract/MIR/bytecode tests; compiler,
runtime and embedding suites, including the source/artifact/interpreter/native
observable matrix; and 207 VM tests with one manual performance test ignored.
The first combined execution run exposed three VM fixture failures from installing
the same standard module twice. Those explicit producers now replace the matching
foundation record; all three pass in the complete VM rerun. No carried failures
remain. All four standalone SDK feature/backend consumers pass, along with twelve
production boundaries, ABI/contract build graphs, strict workspace Clippy,
formatting, structural checks (756 files, zero violations/exceptions) and
`git diff --check`. The complete combined workspace matrix remains due at LR03.
Handwritten role extraction and runtime library ownership remain assigned to
LR01-LR03; they were not moved into types or HIR to hide a dependency.

LR01 completes `kagari-stdlib` ownership of all 38 explicit trait registrations,
concrete types, exports/prelude, Markdown documentation and Rust algorithms.
Runtime construction has no implicit standard installation; SDK construction and
low-level test/example producers explicitly install the modules. ModuleBuilder
receives a checked DeclarationCatalog. Constructor ownership comes from supplied
type bindings across HIR and executable proof checks, with only intrinsic scalar
identity retained as a language fact. Checked context operations expose selected
calls, enum allocation and sequence leases without widening heap internals.

Module, trait, type, function, method and associated-type documentation APIs retain
complete Markdown through completion and cross-module inheritance/overrides.
Standard docs include behavior, mutation/failure rules and analyzed usage examples.
The documentation inventory covers all 38 traits; enum-variant rendering now also
preserves its registered docs and navigation. The old trait product remains only
as a migration oracle, and the obsolete source splice/unused product files are
still assigned to LR02/LR03, not active runtime declaration providers.

LR01 validation passes strict workspace Clippy, formatting, structural checks
(762 files, zero violations/exceptions), thirteen production dependency boundaries,
ABI/contract build graphs and diff checks. The SDK passes 418 integration tests
and the separate source/artifact/interpreter/native observable matrix. HIR passes
414 unit checks and eight language-registration tests; contract/MIR/bytecode pass
79 tests including the verifier doctests. Runtime unit/host checks, eighteen
installation/builder cases, explicit ownership, all standard documentation examples
and the role migration oracle pass. VM passes its 107 unit checks and integration
coverage, including 71 native-boundary cases, seven prepared-backend cases and the
collection/GC/cancellation/reload fixtures; one manual measurement remains ignored.
Initial failures were producers relying on implicit installation, an accidentally
aliased KGR fixture import, and enum docs omitted by the renderer. Those failures
are closed by focused reruns; assertions and negative installation coverage remain.
Final complete workspace and standalone feature/backend matrices remain LR03 work.

LR02 starts by rendering all core and application declarations uniformly, including
module Markdown and parsed role attributes. Native import no longer splices or
searches handwritten trait text, and non-trivia correspondence covers core views.
The unused extraction modules remain on disk until LR03 deletes their last tests
and product workflow. The mutable, fallible Engine builder installs default and
application modules through the same atomic batch path, rejects duplicate module
identities and exposes its checked authoring providers. Build seals the module set
and checks complete rendered signatures; runtime installs that same sealed batch.
SDK materialization uses renderer-version/BLAKE3 content paths and atomic complete
file publication without replacement. Published normalized physical paths match
analysis source names, including spaces/Unicode. Memory mode performs no IO.
Module and item documentation queries retain Markdown and parsed owning locations.

Initial LR02 navigation-cache tests exposed SourceDatabase's existing file-URI
normalization; SDK views now publish the same normalized absolute paths. Four
registration/source/cache regressions pass. The HIR unit run passes 413 tests and
finds three obsolete negative producers that altered views after registration;
those producers now test the independent role/shape layer directly, with all six
role tests passing and separate production import forgery/syntax tests retained.
The three carried HIR failures are closed by focused reruns; its 416 unit cases
are covered. SDK validation passes all 424 unit/integration cases, including the
combined observable source/artifact/interpreter/native matrix and five registration,
documentation and cache regressions. Doc-only changes also load an artifact emitted
by the old registration. CLI passes all five pipeline/diagnostic tests. Strict
workspace Clippy, formatting, structure (764 files, zero violations/exceptions),
thirteen production dependency boundaries, ABI/contract build graphs and diff checks
pass. The ten initially reported qualified paths are corrected. No LR02 build or
test errors remain. Full workspace and standalone feature integration remain LR03.

LR03 removes the obsolete binary/decoder/regenerator, handwritten core declarations,
unused source extractor/renderer and orphan runtime library facade. The registered
source inventory now checks exact generated locations and module/item Markdown for
all standard declarations. Traits/architecture documents describe the implemented
provider path and defer crate compilation metadata. The first full workspace run
finds two Cranelift unit producers still assuming Runtime's implicit standard
installation (`cargo test --workspace`, runtime ModuleValidation at tests.rs:109/184).
LR03 changes that test factory to install the same explicit standard modules;
production backend dependencies and validation remain unchanged. The focused six
Cranelift cases pass and the full workspace rerun passes 1652 tests with one existing
manual measurement ignored. All carried CR/LR build and test errors are closed.

Final acceptance passes `cargo test --workspace`, strict workspace all-target Clippy,
formatting, structure (759 Rust files, zero violations/exceptions), changed-document
link/content checks and `git diff --check`. The standalone feature checker passes
all thirteen production boundaries and ABI/contract build graphs; independent
artifact-only/source/native/source+native consumers pass 6/7/8/9 artifact tests.
CLI JIT passes all five tests. The standard documentation inventory and examples,
all 38 traits/24 roles, generated navigation/cache/doc-only compatibility, native
GC/borrows/cancellation/effect order and pinned reload/source-free artifact coverage
remain exercised by those suites. Repository search finds no obsolete extractor,
binary decoder/regenerator or handwritten-core references in code, current specs
or architecture. Generated cache products stay ignored; no substitute declaration
artifact, ABI version bump, compatibility alias or frontend dependency is introduced.

The finite CR01 -> CR02 -> LR01 -> LR02 -> LR03 goal is complete. Crate metadata,
separate/parallel compilation, live registration mutation, no_std and a standalone
LSP server remain outside this goal; SDK queries/cache provide the LSP integration
surface. Performance differences from these ownership changes are not measured.

## Rust-style library namespaces (NS01, complete)

Authorized scope: replace `core::language` with responsibility-based `core`,
`alloc`, and `std` modules; rename ArrayList to Vec; retain Iterable and existing
Kagari execution/GC semantics. Define each declaration once, preserve its identity
through public re-exports, and use an explicit Rust-style prelude. Keep the
Kagari collection interfaces in `std::collections`, outside the implicit prelude.
This does not introduce Rust borrowing, no_std, new traits, or a package manager.

- [x] Split handwritten traits and installed declarations by module ownership.
- [x] Connect validated installed re-exports, explicit prelude, multi-module role
  collection, native registration, source navigation and source-free products.
- [x] Migrate consumers, examples and specifications, remove obsolete paths, and
  regenerate affected products at the integration checkpoint.
- [x] Verify identity-preserving core/std imports, prelude exclusions/shadowing,
  reserved-role validation, native installation and source-free execution; run
  structure, formatting, strict workspace Clippy/tests, feature/backend consumers
  and diff checks.

NS01 is one coherent implementation checkpoint with `Roadmap-Step: NS01`.
During migration, carry compilation/product mismatches here with reproduction
commands and resolve them before acceptance. Do not add old-path readers or bump
unpublished ABI/format versions for this migration.

Ledger: implementation started from a clean working tree. The single-module
catalog, role origin/completeness checks, native registration and source mapping
now use canonical module ownership. Installed module records carry checked public
alias targets; all six source-authored role modules match the checked trait product.
Consumers, examples and specifications use Vec and explicit imports where names
are outside the prelude. Portable proof and bytecode validation use canonical
receiver ownership without weakening native dependency checks. Test producers
analyze the complete installed native declarations, including bundled MapIterator.

Final validation passes: `cargo test --workspace` (1640 passed, one ignored),
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, `uv run --locked scripts/check_structure.py`,
`uv run python scripts/check_features.py`, `cargo test -p kagari-cli --features jit`,
`cargo run -p kagari-compiler --example regenerate_language_traits -- --check`
(the historical product check, removed by LR03),
and `git diff --check`. The structure checker reports zero violations and zero
exceptions. All four standalone SDK feature consumers and the production
crate/ABI dependency boundaries pass. Namespace tests cover identity-preserving
core/std imports and constructors through serialized artifacts, prelude exclusions,
local shadowing and rejected obsolete names. All migration failures are resolved;
no carried build/test errors or structural debt remain. GC/shared-object semantics
and Iterable are retained. No performance claim is made for this migration.

## Contract and common responsibility cleanup

Scope: narrow `kagari-abi` plus source-independent `kagari-contract`.
Syntax-required traits enter ordinary source analysis with language-role bindings;
collection and standard-library declarations remain Rust-authored native libraries
generating `.kgr` for compiler/LSP analysis. Library catalogs belong to those owners,
not generic compiler or executable models. The implemented partition is 24 core
language items (21 syntax consumers plus Eq/Hash/Ord implicit value semantics)
and 14 ordinary native-library traits; all 38 remain mandatory.
The [architecture](architecture.md#contract-and-common-responsibility-cleanup)
defines the [trait inventory](architecture.md#core-trait-inventory),
[ABI data inventory](architecture.md#narrow-abi-data-inventory), authority,
analysis/registration flows, syntax bridges and dependency rules.
AC01-AC05 implementation and acceptance are complete.

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
- [x] **AC05: Final integration and replacement acceptance.** Resolve all carried
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
| Common `source`, `source_database`, `line_index`, `diagnostic`, `literal` | Source/tooling ownership moves to kagari-source in AC04; shared span coordinates remain available to executable debug metadata |
| Common `arithmetic`, `integer`, `numeric`, `cancellation`, `decode_limits` | Shared mechanisms stay common; numeric behavior keeps one implementation |
| Common `host_interface`, `collection`, `range` | Portable host schema/access and range shape facts stay source-independent; AC04 confirms host schema ownership with its executable consumers |

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
AC03. RuntimePrimitive describes checked execution helpers, reviewed and retained in AC04.
Relocation does not count as replacing these policy consumers.

AC01 acceptance passes: workspace all-target compilation; nine production graphs
and ABI/contract build graphs; source-free SDK compilation; contract/bytecode and
Cranelift suites; runtime native execution, native builder, offline types and
installation access; HIR language contracts; structure, formatting and diff checks.
No carried build/test error remains. Temporary output lives in `target/ac-cleanup`.

Historical AC02 (superseded by LR01-LR03) replaced Rust constructors for the 24 core trait declarations with handwritten
core trait modules and their source-compiled `language/traits.bin` product.
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

AC05 resolves the source-free SDK error boundary exposed by the standalone
artifact-only consumer: source diagnostics, labels and the Diagnostics variant are
available with `source`; executable/native error APIs remain independent of the
frontend. SDK diagnostic mapping preserves authored provenance. Provenance checks reject mismatched revisions, inline/nonphysical origins,
rewritten text and overlapping fragments. README, architecture and specifications now describe the implemented
owners and the 24/14 inventory.

Final acceptance passes on the final source state:

- `cargo test --workspace`: 1637 tests pass across 95 target summaries, including
  doc tests; zero failures. The one ignored test is the existing manual sorting
  performance measurement, outside this functional acceptance.
- The required complete language-contract matrix passes: role/product checks,
  generics/associated members, conversions/numerics/formatting, native view
  correspondence, collections/ranges, source-free linking/reload, conformance,
  GC ownership, sessions and scoped host borrows. Native boundary covers 71 tests;
  native/Cranelift preparation and backend tests preserve actual invocation checks.
- `uv run python scripts/check_features.py`: all nine production boundaries and
  ABI/contract build graphs pass. Independent artifact-only/source/native/
  source+native consumers pass 6/7/8/9 tests; source-free Cranelift compilation
  executes real native code. The disposable feature artifact is regenerated from
  current source; no old reader or ordinary format/ABI version bump is introduced.
- `cargo test -p kagari-cli --features jit`: all five tests pass.
- Checked core source/product regeneration comparison, structure (733 Rust files,
  zero violations/exceptions), formatting, workspace all-target strict Clippy,
  local documentation targets and diff checks pass.

The initial standalone SDK compile error is fixed. The first workspace run's SDK
doctest E0463 occurred during dependency rebuilding on changing source; the final
complete rerun passes, including that doc target. No carried build/test error or
blocking issue remains. Final logs are under `target/ac-cleanup/ac05-*.log`; durable
acceptance is this checkpoint and its `Roadmap-Step: AC05` commit. Existing scalar
native support, explicit representation/syntax bridges and the finite library
surface are preserved; broader backend/GC expansion and performance claims remain
outside this goal.

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

Current implementation entrypoints:

| Work | Locations |
| --- | --- |
| ABI/contract model and verification | [Contract root](../crates/kagari-contract/src/lib.rs), `types/`, `callable/`, `layout.rs`, `slots.rs`, `contracts.rs`, `native_import/` under that crate |
| Physical representation and native boundary | [Value representations](../crates/kagari-abi/src/representation.rs), [native calls](../crates/kagari-abi/src/native_call.rs), [native products](../crates/kagari-abi/src/native.rs) |
| Existing foundation definitions and generated source | [Native library catalog](../crates/kagari-stdlib/src/catalog/mod.rs), [native declarations](../crates/kagari-types/src/declaration/mod.rs), [source renderer](../crates/kagari-hir/src/native/render.rs) |
| Attribute analysis and language selection | [Syntax attributes](../crates/kagari-syntax/src/ast/item.rs), [HIR entry](../crates/kagari-hir/src/lib.rs), HIR `lower/`, `language/`, `typeck/` and compiler `source/lower/` |
| Installation and executable consumers | Runtime `native/`, `loading.rs`, `backend.rs` and `backend/native.rs`; MIR, bytecode, VM, codegen and embed consumers of the checked contract model |
| Common ownership and dependency validation | [Common root](../crates/kagari-common/src/lib.rs), [source root](../crates/kagari-source/src/lib.rs), workspace/crate manifests and [standalone feature checker](../scripts/check_features.py) |

check_features.py covers contract production/build dependency boundaries and
rejects an ABI-to-contract edge. A workspace
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
in addition to this matrix. The AC05 ledger records the actual final runs.

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

### Nominal enums and propagation (EN01-EN05, complete)

The [execution design](enum-propagation-plan.md) replaces the closed StandardEnum
type/execution inventory with ordinary nominal enum declarations and layouts,
then connects `?` to library-authored Try/FromResidual protocols. Option/Result
remain core-library definitions installed through the completed LR registration
flow. This is a finite follow-up to CR01-CR02/LR01-LR03, not a replay of them.
It supersedes their exclusion of generic enum/protocol migration for this active
track; current specifications still describe implemented behavior until replaced.

Status: the user activated the continuous EN01-EN05 goal on 2026-10-04.
EN01-EN05 are complete. Final integration was accepted on 2026-10-05. Ordinary
source/library/native enums and checked protocol propagation pass the complete
behavioral, feature, backend and structural matrix.
Commit each accepted phase once, using `Roadmap-Step: EN01` through `EN05`.
The execution design and its index links enter the first implementation checkpoint.

- [x] **EN01: Author and validate ordinary native enums.** Reuse portable nominal
  enum models; add public enum/variant builders, kind-correct type handles,
  full-doc rendering and declaration correspondence. Define checked executable
  enum/variant handle contracts for EN02.
- [x] **EN02: Execute registered enums through generic layouts.** Connect installed
  declarations, native allocation/inspection and call codecs to existing source
  enum execution. Validate layouts, multiple payload fields, generations, roots,
  optional provenance and source-free executable dependencies.
- [x] **EN03: Replace the seven standard enum families.** Migrate library records,
  Rust bodies, iteration/comparison/conversion/reporting and all executable
  consumers. Delete dedicated standard semantic types, fixed tags and operations.
  Preserve existing Option/Result `?` temporarily through checked nominal bindings.
  Resolve any EN01-EN02 carried build failures at this checkpoint.
- [x] **EN04: Implement Try/FromResidual propagation.** Register protocols and
  ControlFlow, migrate standard carriers and compile propagation through selected
  calls/associated outputs. Support checked local source/native custom carriers
  and generic bounds; preserve inference, effect ordering and Err provenance.
  Remove direct compiler Option/Result propagation policy and audit From's role.
- [x] **EN05: Remove remnants and complete integration.** Update affected executable
  fixtures at one schema checkpoint, current docs and dependency/role inventories.
  Pass the design's complete behavioral, standalone-feature and backend matrix
  plus all repository final checks, resolving every carried failure.

Phase order is EN01 -> EN02 -> EN03 -> EN04 -> EN05. EN01-EN02 may carry bounded
compilation failures documented with reproduction/cause and the owning follow-up;
EN03 restores the baseline before EN04, and EN04 closes its failures before final
integration. Preserve validation without compatibility aliases or old readers.
Detailed API targets, failure-origin rules and acceptance live in the linked
design. Separate crate metadata, general Rust value/Serde binding and broader
intrinsic/backend redesign remain outside this track.

Execution ledger: EN01 reuses TypeDefKind::Enum, VariantDef and Ty::Enum rather
than adding a parallel native type model. EnumBuilder and owner-qualified
VariantRef support generic/unit/multiple-field/recursive/empty declarations,
full docs and kind-correct TypeRef applications. Native module validation,
catalog identities, rendering and parsed HIR attachment now accept ordinary
enums. Dependency closure traverses variant payloads and rejects missing providers,
wrong enum arity/kind and foreign binders; no raw discriminant authoring is added.
Execution resolves these authoring identities in an installed pinned scope in EN02.

EN01 validation passes the eight new registration/source/doc/correspondence tests,
fourteen existing native-builder tests, forty types tests, five native HIR ingestion
tests, focused production check, strict all-target types/HIR/runtime Clippy,
formatting, structure (761 Rust files, zero violations/exceptions), changed-doc
local links/anchors and git diff --check. Initial test-harness API mistakes and a
remaining native-signature enum exclusion were repaired before acceptance; no
build/test failure is carried. Full feature/backend acceptance remains EN05 work.

EN02 connects CallContext's scoped argument/result types to allocate_enum,
enum_argument_is and enum_argument_field. Variant membership, payload count/type,
heap liveness and pinned generations use the existing ordinary layout checks.
Registered generic enum templates are retained even without source constructor
roots; nested supplying scopes survive native construction and inspection.
Managed Value codecs keep exact declared nominal signatures. Existing enum heap
storage already carries optional provenance; library capture/forward policy is
EN03-EN04 work. No new executable inventory or concrete application enum hook is
introduced.

EN02 validation passes two native enum tests (five successful serialized/fresh-host
scenarios and five invalid operations), ten artifact consumer tests, four existing
enum payload/generation/ABI tests and the retained generic reload test. The
standalone artifact-only consumer passes seven tests without frontend features,
including nested native enum creation/inspection under collection threshold one.
Strict all-target runtime/HIR/compiler/embed Clippy, formatting, structure (764
Rust files, zero violations/exceptions) and diff checks pass. The initial fixture
put a struct literal in an unsupported match-scrutinee parsing position; binding
the payload before matching repaired it without a production semantic change.
No failure is carried. Full feature/backend acceptance remains EN05 work.

EN03 replaces all seven standard families with library-authored ordinary enums.
Numeric TryFrom pairs are ordinary registered implementations; their associated
Error is selected from declarations, while runtime numeric code only computes
scalar outcomes. Forward conversion metadata retains method/error/result identities
for inference and checked selection, without choosing concrete library errors.
Variant failure-reporting facts are explicit declaration/layout metadata; generic
ForwardEnumOrigin copies optional provenance after checked ordinary construction.
Constructor syntax now applies uniformly: unit variants are values and reject
empty parentheses, following Rust; previous source-only empty-call acceptance is
replaced, with corresponding positive/negative context tests and syntax docs.
Dedicated semantic types, fixed tags and standard enum bytecode operations are
removed. Native library producers use checked declaration handles and current
pinned layouts; cached authoring handles avoid repeated declaration validation.
Host Option/Result schemas retain the ordinary declaration identity and validate
the complete applied type, including empty variants. Linked application validation
requires the actual declaration/template and exact nominal kind/arity. Native
imports, callable signatures and concrete interface requests carry private nominal
layouts, including caller types referenced by shared standard adapters, without
introducing reverse dependency edges. Index-bound decoding also checks Bound<usize>
even for Unbounded. HIR owns semantic type raising; compiler imports it directly.

EN03 validation passes compiler (161), HIR (416), runtime (59), contract (32) and
VM (107) unit suites; standard enum/iteration/trait behavior (36), conversion and
propagation (27), native boundaries (72), collections (18), GC ownership (5),
source-free enum/composite types (9), and the embedding language-route conformance
suite. Instantiation's 48 unchanged cases pass; two obsolete unit-call fixtures
were updated and individually rerun successfully. The repaired boundary suite
includes shared dynamic iteration and wrong-type empty index bounds. Strict
workspace/all-target Clippy, formatting, structure (766 Rust files, zero violations
or exceptions), 100 changed-document local links and git diff --check pass.
Initial native producers/test consumers, incomplete imported private layouts and
obsolete Empty() fixtures were repaired in this phase; no build/test error is
carried. Serialized development fixtures and the full feature/backend matrix are
the scheduled EN05 checkpoint, with no old-format reader or identifier bump.

EN04 registers Try/FromResidual/ControlFlow and replaces compiler Option/Result
propagation with checked branch/residual calls and ordinary ControlFlow members.
Source enum/struct and registered native carriers execute from serialized artifacts
under collection threshold 1, including a fresh artifact-loading Engine. Generic
A: Try and R: FromResidual<A::Residual> functions retain checked projections;
argument contexts and applied trait bounds normalize after substitution. Closure
return inference, different success types, effect order and original Err traces
remain intact. Incorrect protocol signatures, missing parents/bounds, overlapping
implementations, forged roles and altered implicit From adapters fail validation.
The final signatures and implementation rules are published in the existing specs
and executable try-protocols example; no prelude names were added.

From remains a language role for intrinsic identity and lossless numeric conversion.
Result FromResidual requests its F: From<E> callback through ordinary checked
native requirements. Implicit adapter identities include the applied source
interface; bound forwarding now includes static members with unchanged scope and
signature proof checks. Native supertrait validation resolves associated outputs
before receiver/parameter substitution, supporting Try's parent contract without
another parser or library-specific executable route.

EN04 validation: Result/Option 12, protocol propagation/invalid witnesses 8,
reporting 13, role collection/schema 6, stdlib documentation/examples 1, portable
types 36 and contract units 32 pass. The initial HIR suite passed 415 cases; its
one inventory assertion was updated for ControlFlow (eight core enums) and rerun
successfully. Strict workspace/all-target Clippy, format, structure (772 Rust
files, zero violations/exceptions), 102 local documentation links and diff checks
pass. Initial callback selection, unresolved ABI encoding, owner partitioning,
projection normalization and fixture diagnostics were repaired here. No EN04
build/test failure is carried. EN05 owns the complete workspace/feature/backend
matrix and coherent disposable fixture regeneration.

EN05 expands the existing disposable feature artifact and its shared provider
with registered native Carrier<T> propagation and standard Option/Result/ControlFlow
residuals. The same serialized bytes exercise selected calls in artifact-only,
source-only, native-only and combined standalone consumers, with forced collection
and root/depth cleanup. The emitter and consumer use the same public provider.
Production searches find no StandardEnum/StandardVariant/StdEnum/MapResultError or
native enum constructor inventories. Remaining current embedding/syntax docs now
describe generic nominal layouts, scoped native allocation and checked propagation.
The first full workspace run found two compiler fixture failures: the public type
inventory now includes ControlFlow, and the old origin opcode test assumed the
interim EN03 lowering. The inventory is updated. ForwardEnumOrigin has no production
emitter after EN04; EN05 removes that obsolete MIR/bytecode/verifier/VM route and
replaces its test with source-free converted-origin behavior. Existing provenance
and malformed selected-call coverage stays intact. Reproduction: `cargo test
--workspace`; the final EN05 acceptance below includes these repairs and the full rerun.

The final reload case exposed incomplete shared-call support for
`A: Try<Output = i32>, R: FromResidual<A::Residual>` inside a captured default
closure. HIR and portable substitution now retain sibling associated constraints
on abstract projections. Shared lowering binds Self, supplies static as well as
receiver operations, and records semantic enum-field registers. Executable calls
permit partial outputs only with an explicit scoped receiver and verified bound
evidence; boxed calls still require complete outputs and a receiver member.
Checked call bounds normalize concrete projections before proof comparison.
Runtime projection evaluation consumes outputs from the already selected,
generation-pinned implementation table. Type-only scopes retain these immutable
output/layout facts and their lexical owners without keeping operation groups.
Static calls keep their actual parameter list. Reproduction: `cargo test -p
kagari-embed --test generic_reload --test try_protocols`; acceptance includes both
Continue and residual returns across reload, forced GC and complete root/value
cleanup. Contract tests reject missing scope/evidence and foreign output members.

The full workspace run also found an obsolete installation-access fixture: it
carried isolated core traits without the nominal enum owners/layouts required by
their signatures. The fixture now carries the registered declaration dependency
closure, ordinary types and enum templates, while retaining its reserved-role-only
trait inventory and every installed/forged/missing/private/reload assertion.
Reproduction: `cargo test -p kagari-runtime --test installation_access`.

EN05 was accepted on 2026-10-05. No build/test failure or structural debt is
carried. Final validation:

- `cargo test --workspace --no-fail-fast`: all unit, integration and doc tests pass;
  the rerun includes the repaired installation fixture, captured generic Try
  closures, all standalone language examples and 416 HIR tests.
- `uv run python scripts/check_features.py`: all 13 production crate boundaries
  and ABI/contract build graphs pass; independent artifact-only/source/native/
  combined consumers pass 9/10/11/12 tests respectively. The fixture was regenerated
  once as a disposable coherent product. Actual supported native execution and
  checked interpreter fallback are distinguished by the existing assertions.
- `cargo test -p kagari-cli --features jit`: all 5 tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all -- --check`: pass.
- `uv run --locked scripts/check_structure.py`: 772 Rust files, zero violations
  and zero documented exceptions. Changed imports, ownership, scope metadata,
  cross-target fixture sharing and effective LOC were reviewed.
- Changed documentation file links resolve; `git diff --check` passes. Current
  production searches contain no closed enum inventory, dedicated error-conversion
  opcode or obsolete origin-transfer instruction.

The EN01-EN05 goal is complete, with one accepted commit per phase. Library policy
stays in registered implementations; executable consumers retain checked nominal
layouts, scoped signatures, selected calls and generation-pinned facts. Separate
crate metadata, dynamic Try interfaces and broader backend expansion remain outside
this completed track.

### Runtime ownership and host objects (GO01-GO06, complete)

Status: GO01-GO06 accepted on 2026-10-06, with one commit per phase. The
[runtime ownership and host object API design](runtime-ownership-and-host-api-design.md)
defines central stores and checked IDs, automatic host retention, a Send runtime
with exclusive execution, typed native registration, object mutation and checked
function/trait calls. EN01-EN05 remain complete. The ledger below records phase
order, intermediate decisions and final acceptance.

The existing nonmoving mark-sweep collector remains the starting algorithm. This
track replaces the surrounding Rc/Weak ownership graph, not the ordinary nominal
enum model. It owns the shared conversion/root/call foundation previously proposed
under RI/HA. RI retains DTO derives, Serde and external Opaque ownership; HA retains
the prepare/load/reload facade and proposed latest-version entry policy. Async and
task-scope designs remain separate and adopt this thread-transfer contract.

The accepted [GC boundary design](runtime-ownership-and-host-api-design.md#gc-boundaries-and-extension)
separates host adapters, object semantics, storage and collection. Common root
enumeration, registered reference traversal and controlled reference writes are
required foundations. Traced payloads use restricted editing rather than arbitrary
&mut access. Ordinary new registrations must not require mark/sweep changes.
Additional collector algorithms and a configurable GC framework remain deferred.

Phase order and progress:

- [x] **GO01: Central roots and checked identities.** Runtime-owned root storage,
  common root enumeration, automatic host leases, stale/foreign identity checks
  and teardown semantics.
- [x] **GO02: Central execution and metadata ownership.** Session/frame/program/
  environment stores, synchronous reentry, coordinated reachability and pinned
  version reclamation, including cycles and escaped generic environments. Separate
  object policy from collection; establish storage traversal and controlled internal
  edge writes, including initialization, frame/root slots and metadata links.
- [x] **GO03: Runtime thread transfer.** Send runtime, exclusive access,
  callback/payload constraints, deterministic thread handoff and Tokio task coverage.
- [x] **GO04: Typed registration and results.** Recursive conversion, argument
  tuples, complete registration docs/generated KGR, typed entry arguments and
  automatically retained public results with source-free support.
- [x] **GO05: Host objects and checked calls.** Fields/collections, scoped native
  payload edits with restricted traced-field views, prepared/cached member bindings,
  direct declaration-handle binding, functions/closures and ordinary/generic/interface
  trait methods.
  Route public reference mutations through the GO02 storage boundary and reject
  unrestricted mutable access to traced payloads.
  Optional generated Rust views consume this path as later tooling work.
- [x] **GO06: Standard library and final integration.** Exercise public adapters
  in standard bindings and new registered types/payloads without collector changes;
  verify reference replacement/removal and cyclic reclamation, remove obsolete
  APIs, update specs and pass the design's complete behavior/feature/backend matrix.

The design owns detailed contracts and acceptance criteria.
Use one accepted commit per phase with `Roadmap-Step: GO01` through `GO06`; keep
checkpoints building and perform final integration at GO06. No performance gain
is assumed. Open implementation errors and resumption state belong here.

#### GO progress ledger

- GO06 accepted on 2026-10-06. GO01-GO06 are complete, with no carried build/test
  failure or unresolved structural exemption. Standard String and Vec adapters,
  typed callback edits, selected trait calls, SDK host access and retained callable
  cleanup meet the design's integration scope. Advanced storage adapters remain
  where they serve iterator/representation or precommit-result semantics; ordinary
  host authoring uses typed values and automatically retained handles.
  Final validation:
  - `cargo test --workspace`: 1885 passed across 113 test suites, zero failures;
    one existing manually invoked performance test remains ignored. Includes the
    source/artifact/native language-route contract, standalone language examples,
    HIR/runtime/VM suites, all 158 native-boundary tests, reload/thread-transfer
    checks and compile-fail/doc tests.
  - `uv run python scripts/check_features.py`: 13 production crate boundaries
    plus ABI/contract build graphs pass. Independent artifact-only/source/native/
    combined consumers pass 9/10/11/12 tests; the disposable artifact was regenerated
    once for the current schema. Actual scalar JIT and checked fallback stay distinct.
  - `cargo test -p kagari-cli --features jit`: all 5 tests pass.
  - `cargo clippy --workspace --all-targets -- -D warnings`, formatting, structure
    (896 Rust files, zero violations/exceptions), documentation links and diff checks
    pass. Changed ownership, imports, public APIs and storage writes were reviewed.
  - `cargo run -p kagari-embed --example host_objects` passes; the final SDK typed-call
    test also passes with cancellation and observer coverage. Logs: target/go06/.
  The starting nonmoving mark-sweep algorithm is preserved. Additional collectors,
  generated Rust binding tools, asynchronous script execution and proposed broader
  DTO/facade work remain explicitly deferred; no performance gain is claimed.

- GO06 implementation (included in the accepted phase commit): all String methods
  now use typed receivers/results;
  split uses NativeContext::collect without raw slots or roots. Vec reads and
  push/set/insert/clear use retained ScriptVec/ScriptValue adapters; retain/dedup use
  typed buffer callbacks and compiler-selected equality. Pop/remove retain their
  existing prepare-result-before-removal path. Preauthored catalogs attach typed
  entries and recover exact requirement tokens through the same validated boundary.
  Typed bulk edits share the existing exclusive buffer/GC lease and poll around
  callbacks; detached storage reads fail instead of returning an empty result.
  String (5), list algorithms (8), collection access (5), standard documentation,
  157 native-boundary regression tests and the new bulk-edit GC/error/unwind test
  pass. Logs: target/go06/typed-*, string-tests.log, vector-edit-tests.log.
  RootedCallable has been removed; retained callback/reload/thread consumers use
  PinnedFunction and advanced StoredCallable keeps only its traced identity.
  The five callback-boundary tests pass. SDK with_context and the host_objects
  example expose checked member calls under explicit execution policy. Their
  focused verification and executable example now pass, including SDK cancellation
  before mutation and observer events during prepared method execution. The retained
  callable unit suite passes all three lifetime/generation/teardown tests. Workspace
  strict Clippy passes after replacing two verbose test callback types with aliases.
  CLI JIT tests pass (5); independent feature and full workspace acceptance is
  recorded above.
  No raw storage algorithm was replaced merely to change syntax: advanced cursor,
  generic list defaults, hash storage and precommit removal adapters retain their
  concrete duties. The obsolete host-owned callback wrapper and migrated raw String/
  Vec bodies are removed. Extension acceptance uses registered enums and Managed<T>
  payload tests from GO05, including traced callback cycles and retired-program
  reclamation without changes to mark/sweep. The phase is accepted.

- GO05 accepted on 2026-10-06. The substep entries below record implementation
  history; all GO05 work is included in one accepted phase commit. Fields, sequences,
  hash collections, enums, native data/managed payloads and prepared script/native/
  interface/selected calls use retained typed handles and checked storage boundaries.
  Generic application evidence and lexical type scopes survive source-free loading
  and reload; weak binding caches retain no retired program by themselves.
  Final phase evidence: all 157 native-boundary tests pass; previous unchanged
  runtime unit (132), runtime doctest (11), compiler/HIR and embedding focused
  checks are recorded below. Workspace all-target strict Clippy, structure (892
  Rust files, zero violations/exceptions), formatting and diff checks pass. Changed
  module ownership, imports, public declarations, macros and retained graph edges
  were reviewed. No carried failure or structural exemption remains. No performance
  claim is made. GO06 owns standard-library adoption, obsolete public adapter cleanup,
  extension acceptance and the full workspace/feature/backend matrix.

- GO05 in progress (uncommitted): source field visibility now belongs to
  kagari-types and is retained in declarations and executable struct layouts.
  MIR/bytecode verification rejects declaration/layout access mismatches, and
  source-free artifact round trips preserve private, parent and public access.
  No compatibility reader or routine format identifier bump was introduced.
- GO05 object foundation: Object<S>/dynamic Object retain host roots; ObjectType,
  ObjectField and Field<T> prepare public field access and exact applied types.
  Runtime binding caches contain weak records; live handles pin the program,
  while dead caches retain no version. Field access compares prepared identities
  and uses indexed checked storage operations. Layout clones carrying lexical
  environments now also have a pointer-identity fast path. ObjectBuilder retains
  initializers, rejects duplicate/missing/private fields and publishes only a
  complete object. Conversion failures preserve previous successful mutations.
  Contextual type checks support dynamic handles inside Vec/Option/Result/tuples
  without a language Any type or erased expected signature.
- ScriptVec<T> now retains a collection view and its element scope. Its public
  get/set/push/insert/remove/pop/clear/truncate and bounded for_each operations use
  existing checked storage mutations and iteration leases. Explicit readonly
  views cannot widen on conversion. Factories retain every initializer until
  publication; removed objects gain an independent host lease before a safepoint.
  Typed native registration can accept and return ScriptVec directly, preserving
  script aliases. Dynamic Object also crosses typed VM entries with no raw IDs.
- GO05 hash collection access (uncommitted): ScriptMap<K, V>/ScriptSet<T>
  retain exact key/value scopes, outer access and the Hash/Eq implementation chosen
  at construction. Get/contains/insert/remove/clear and rooted snapshot traversal
  use the existing storage commits, key lookup guards and iteration leases.
  Custom comparisons run outside storage borrows through checked retained calls;
  owned adapters never substitute Rust Hash/Eq. Clones preserve aliases; readonly
  views reject writes and cannot widen on conversion. Removal commits before
  result conversion, matching the documented host mutation failure policy.
  Hash factories now preserve custom selected evidence on the native object as
  ordinary executable graph edges. Stored records contain no host root lease;
  host handles promote those edges to retained calls. Builtin-key data needs no
  executable retention. Payload replacement preserves the selected evidence.
  This adds native storage metadata traversal, without changing collector policy.
- Hash handle evidence: five source-free VM tests pass for typed native arguments
  and results, builtin/custom keys, applied key types, aliases/readonly/foreign
  runtime rejection, independent removal retention, snapshot retention during
  replacement and GC, mutation rejection during Hash/Eq, trap cleanup and old
  selected code after reload. The old map/module cycle is reclaimed after the
  final external handle drops. A generic-impl fixture needed identical Eq/Hash
  bounds as required by the language; the fixed fixture passes. It uses a closed
  applied implementation, so no claim is made that this test exercises an erased
  method environment. Dedicated generic/interface call coverage remains GO05.
  Runtime check, strict runtime/VM all-target Clippy, structure (846 Rust files,
  zero violations/exceptions), format and diff checks pass. The complete native
  boundary regression passes all 103 tests. Logs are under target/go05/hash-*.
  Standalone construction is now covered by the checked factory boundary below;
  these earlier tests exercised the collection operations through KGR entries.
- GO05 callable foundation (uncommitted): PinnedFunction<A, R> wraps public
  closed script/native entries or retained closures. Runtime binds names or
  declaration identities against verified executable evidence; weak descriptor
  caching retains no dead program. NativeContext inherits the invoking backend,
  and VM/SDK call entrypoints use the same rooted typed conversion and existing
  execution stack. Explicit root options preserve SDK cancellation/JIT policy.
  Checked retained-function entry permits old versions outside the caller's
  current dependency graph while rejecting external entries during candidate
  initialization. Closures keep captures and environments alive; their calls,
  parameters and results use scoped signatures. Script entry handles can convert
  back to function values. Boxing a native entry still requires a checked script
  wrapper; it does not synthesize bytecode at runtime.
- GO05 generic function applications (uncommitted): bind_function_application
  and its declaration-identity form check supplied type scopes and select only
  installed concrete entries or closed shared-call witnesses. Missing applications,
  private declarations, wrong arity and incompatible Rust signatures fail before
  native/script effects. Cache identity includes the owning generation, target
  and shared witness location; steady-state calls reuse the retained descriptor.
  Shared environment/operation preparation now has a lexical-scope entry used by
  both the execution stack and host binding. Runtime argument validation still
  precedes environment publication. Plain entries retain only their existing
  program lease; shared entries additionally root their executable environment.
- Application evidence: four source-free native application tests cover direct
  declaration/name binding, missing applications, selected trait operations with
  GC, object/vector/closure returns, failed schema publication, pinned old values
  after compatible reload and weak-cache release. A fifth artifact test removes
  the specialized native import and emits a verified closed call of its shared
  template; binding and repeated calls survive GC, and dropping the last handle
  releases the environment. Existing source restrictions on exported script
  generic free functions remain enforced; tests use supported registered exports.
  An initial broad regression found redundant root records on plain function
  bindings. Preparation now creates additional metadata roots only for shared
  environments, and all eleven then-current callable tests pass with their
  original zero-temporary-root assertions. Logs: target/go05/application-* and
  target/go05/shared-application-tests.log. The final native-boundary regression
  passes all 108 tests. Strict runtime/VM all-target Clippy, structure checks
  (849 Rust files, zero violations/exceptions), formatting and diff checks pass.
  No carried build/test failure or new structural exception remains.
- GO05 inherent member metadata (uncommitted): portable InherentTable records
  preserve declaring impl identities, receiver types, binder scopes, bounds and
  public methods. Private and parent-visible methods remain absent. Native
  members retain the original registered templates, including projections;
  module validation matches their recombined scopes/signatures against native
  declarations. Script bodies are checked against exported signatures in both
  MIR and bytecode. Definition mapping, artifact limits, type applications,
  host-reference traversal and ABI fingerprints include the new records.
  Compiler tests cover source-free round trips, signature/receiver/native-template
  tampering, distinct same-named members and private signature changes.
- GO05 closed object methods (uncommitted): ObjectType::method resolves an opaque
  InherentMember declaration handle; bind_method[_declaration] produces Method<A, R>
  with arguments excluding self, and Object::call checks the exact retained
  applied type/generation before converting arguments. Associated functions use
  bind_associated_function[_declaration] and ordinary PinnedFunction handles.
  Preparation shares the existing weak function cache and checked entry evidence;
  repeated calls prepend a rooted receiver to the existing typed argument path.
  No collector or backend call semantics were added. Three source-free VM tests
  pass for access/signature rejection, unrelated/foreign receivers, aliases, GC
  during native callbacks, committed mutations before traps, pinned reload and
  reclamation after the final old binding drops. Compiler regression passes all
  180 tests (166 unit and 14 integration); the native boundary suite passes all
  111 tests. Strict all-target Clippy for runtime/compiler/VM/SDK, structure checks
  (853 Rust files, zero violations/exceptions), formatting and diff checks pass.
  The module-level declaration validation test now includes native constructor
  declarations when checking an inherent table, since receiver ownership depends
  on those declarations; its previous isolated-record assumption was invalid.
  Workspace all-target checks also pass. Full SDK tests pass all 442 tests
  (`cargo test -p kagari-embed`, including the multi-route language contract).
  Logs for this boundary are target/go05/method-* and target/go05/object-method-*.
- GO05 interface value foundation (uncommitted): Interface<S> retains the existing
  boxed interface and its declared lexical type scope. DynamicInterface accepts
  only contextual trait applications, while named schemas supply a precise
  registered trait type. Cloning preserves aliases; conversions neither infer a
  new implementation nor widen access. The existing heap snapshot retains the
  selected implementation and inherited/generic metadata; the host view also
  leases its declared type's program. Three source-free VM tests pass for generic
  trait applications, Option-wrapped handles, typed native input/output with GC,
  readonly rejection, foreign runtimes, thread transfer, old dispatch after
  changed code/data publication and last-handle version reclamation. Logs:
  target/go05/interface-handle-*. These tests invoke retained interfaces through
  checked script entries; direct host calls are covered separately below. Runtime
  check, strict runtime/VM all-target Clippy, structure (855 Rust files, zero
  violations/exceptions), formatting and diff checks pass.
- GO05 interface member calls (uncommitted): InterfaceMember resolves public
  applied trait members and inherited declarations; Runtime also prepares members
  from an installed TypeArgument without constructing a receiver. Declaration
  binding accepts registration MethodRef identities through its new id accessor.
  bind_interface_method<A, R> validates the complete Rust mapping and weakly caches
  a descriptor scoped to the applied interface and owning program generation.
  Interface::call uses the existing verified method ordinal and dispatch snapshot
  for each receiver, including default methods and native result adapters.
  Rooted selections retain their environment while the normal runtime/VM frame
  executes; cloned selections share the same immutable selection and root lease.
  Ordinary and interface calls now share rooted argument encoding. No new
  collector policy or open-ended implementation search was added.
  Five source-free tests pass for descriptor reuse across receivers, inherited and
  default calls, direct registration identity binding, private access rejection,
  associated object outputs, native iterator adapters, GC/reentry, cancellation,
  traps, candidate isolation and pinned reload/cache reclamation. Runtime/VM
  checks and strict all-target Clippy pass; the full native-boundary regression
  passes all 119 tests. Structure checks pass for 858 Rust files with zero
  violations/exceptions; formatting and diff checks pass.
  Logs: target/go05/interface-method-*.
- GO05 generic interface applications (uncommitted):
  bind_interface_method_application accepts method-local TypeArguments only when
  an installed nongeneric carrier contains a checked closed interface call.
  Binding prepares and roots its selected operations and lexical type scopes;
  calls reuse those operations with the receiver's verified dispatch selection.
  Missing applications and mismatched mappings fail before effects. Two tests
  cover inherited generic defaults, object/closure results, bound operations,
  GC/reentry, pinned reload and last-descriptor reclamation. The full native
  boundary passes 121 tests; strict runtime/VM all-target Clippy, structure
  (860 Rust files, zero violations/exceptions), formatting and diff checks pass.
  Logs: target/go05/interface-application-*. Generic inherent applications
  remain pending.
- GO05 selected native calls (uncommitted): NativeContext::selected_method
  promotes a registered requirement to SelectedMethod<A, R>, retaining the exact
  selected target, environment and lexical signature. It reuses PinnedFunction
  execution/conversion, including prepared primitive operations; no runtime trait
  search or integer-slot authoring is added. SelectedCall now checks its authoring
  declaration, preserving that identity through native-default lowering.
  ModuleBuilder::bind_typed checks concrete contextual mappings before converters
  and callbacks; TraitBuilder::bind_default_method supplies its receiver separately
  from the tuple. Three tests pass for generic/default/static/primitive operations,
  wrong token/type/application/runtime rejection, GC, reentry, trap cleanup,
  candidate isolation, pinned reload and last-handle reclamation. Selected calls
  preserve the existing cross-version layout-compatibility rules for arguments;
  compatible old data can enter newer selected code without rebinding that code.
- GO05 generic value adapter (uncommitted): ScriptValue retains the exact declared
  TypeArgument, access view and root for a generic native parameter/result. It
  forwards values through selected calls or decodes them to owned Rust data and
  retained handles; it introduces no erased script type or raw storage access.
  Its successful source-free test covers generic scalar/string/object/collection/
  interface/closure forwarding, nested retained reads, readonly upgrade rejection,
  wrong destination/foreign runtime rejection and last-handle collection.
- GO05 associated-call normalization (uncommitted, carried regression resolved):
  native default lowering now keeps the ordinary receiver bound and independent
  associated-output constraints instead of emitting an equality between an
  associated type and its own projection. InterfaceCallContract retains bounded
  source/result normalization facts for applied signature types. Compiler lowering
  supplies them; linked ProofCatalog validation proves every equality against the
  installed declarations/implementations. Identity mapping, decoding limits,
  application validation and artifact accounting include these facts. Missing,
  duplicate, disconnected, identity and forged same-representation facts reject;
  neither a claimed signature nor a physical register shape establishes validity.
  Runtime method preparation now adds receiver and call-selected operation facts
  before resolving the applied signature. The resulting entry environment keeps
  those operations and their supplying scopes. Call-specific operations do not
  enter a receiver-only application cache. Superseded late environment extension
  was removed; the storage lifecycle tests still exercise immutable record forks.
  The formerly failing associated-collection case passes source-free script calls
  and direct host generic-interface binding, including GC and typed mutation of
  its returned objects. Two compiler tests cover MIR/KBC round trips and tampered
  facts; a contract test independently rejects an invented normalization equality.
  Validation passes all 126 native-boundary tests, the expanded direct-host case,
  all 365 unit tests across contract/bytecode/MIR/compiler/runtime, seven focused
  environment tests and six interface-contract tests. Strict all-target Clippy for
  those crates plus VM, structure (865 Rust files, zero violations/exceptions),
  formatting and diff checks pass. Logs: target/go05/associated-normalization-*.
  No carried failure remains in this boundary; GO05 is not yet accepted/committed.
- GO05 inherent applications now bind instance and static members using receiver-
  inferred impl arguments and explicit method-local arguments. Name and declaration
  APIs share the existing callable cache and require installed concrete entries or
  closed shared-call witnesses, including selected bound operations. Bounded receiver
  matching is shared with semantic template matching; inferred argument scopes stay
  intact, and repeated parameters reject incompatible nominal layouts. Concrete
  struct bindings additionally reject incompatible supplied argument scopes instead
  of attaching an ineffective environment to an already specialized layout.
  Public script inherent templates are now permitted locally and across source
  modules; public generic script free functions remain rejected. Imported concrete
  calls retain their arguments during lowering so existing instance planning can
  find the required body. No runtime proof search or compilation was added.
  Four source-free VM tests cover arity/mapping/missing evidence, generic static
  members, nested nominal outputs, selected bounds, cache reuse, GC, old-code calls
  and mixed scopes in both concrete and shared struct layouts. A source/artifact
  cross-module test covers the frontend/lowering path. Validation passes 620 unit
  tests across types/HIR/compiler, 38 source-module/type-inference tests, all 130
  native-boundary tests and ten runtime host-object tests. Strict all-target Clippy
  for types/HIR/compiler/runtime/VM/embed, structure (867 Rust files, zero violations
  or exceptions), formatting and diff checks pass. Logs: target/go05/inherent-*.
  Initial diagnostics and the scope regression are resolved. GO05 remains unaccepted
  and uncommitted; collection and enum constructors are recorded below, with native
  factories and restricted payload editing still pending.
- GO05 collection factories (uncommitted): `bind_map_constructor` and
  `bind_set_constructor` prepare the installed public zero-argument `new`
  application as an ordinary cached PinnedFunction. Key/value scopes are explicit,
  Rust mappings are checked, and a concrete entry or closed shared-call witness
  supplies the constructor's selected Hash/Eq evidence. The NativeContext
  `create_map`/`create_set` convenience APIs infer static Rust mappings; explicit
  scoped-type forms support dynamic Object handles. Constructors execute registered
  code through the existing adapter, without assembling raw storage or searching
  for new trait proofs. Missing applications fail before allocation.
  Three new source-free VM tests cover independent allocations, cache reuse,
  convenience and explicit forms, custom key callbacks collecting during Hash/Eq,
  trap cleanup, typed native reentry, old-factory behavior after reload, foreign
  runtime rejection and old-program reclamation. All eight hash-handle tests pass,
  as do strict runtime/VM all-target Clippy, structure (870 Rust files, zero
  violations/exceptions), format and diff checks. Initial fixture syntax mistakes
  were corrected; no implementation failure remains. Logs: target/go05/hash-factory-*.
  Applied enum constructors are recorded below. Native payload construction and
  restricted editing remain; GO05 has no accepted phase commit yet.
- GO05 enum factories (uncommitted): EnumType/EnumMember/EnumVariant prepare public
  nominal enum applications and full Rust payload tuples. Name lookup and direct
  TypeRef/VariantRef declaration identities share preparation. Cloned variant
  handles reuse their scoped payload descriptor and program retention. `create`
  converts and roots fields before ordinary enum allocation, then returns an exact
  retained ScriptValue. Empty `()` and one-unit `((),)` payload packs remain distinct;
  existing failure-provenance allocation is reused. No raw IDs or Values are needed
  at the host call site. Concrete layouts require compatible supplied scopes;
  retained declaration templates preserve each generic argument's original scope.
  Registered unconstrained templates need no script constructor roots. Declared
  bounds additionally require a compatible installed concrete layout, including
  when a shared open layout is available, rather than accepting arbitrary new
  bounded applications from representation alone.
  Six source-free factory tests cover access/arity/type/foreign checks, aliasing,
  GC during failed conversion and cleanup, empty/unit variants, direct registration
  identities, Option decoding, scoped layout rejection, constrained shared templates,
  old-layout retention/reclamation and exclusive runtime thread transfer. Validation
  passes all 139 native-boundary tests, 18 runtime enum-declaration/conversion tests,
  and 16 SDK enum/Option/Result tests. Strict runtime/VM all-target Clippy, structure
  (873 Rust files, zero violations/exceptions), formatting and diff checks pass.
  A redundant clone and integration-test discovery issue found by Clippy were fixed.
  Logs: target/go05/enum-factory-*. No carried failure remains in this boundary.
  Native construction and fixed-data editing are recorded below. Traced payload
  setters, callable boxing completion and GO05 acceptance remain pending.
- GO05 native payload handles and fixed-data editing (uncommitted): NativeType<T>
  prepares an installed opaque native type from its registration identity and
  scoped arguments; NativeObject<T> retains the object and exact applied type.
  Clones share the descriptor/root. Factories validate the Rust representation
  and traced edges before allocation and root before collection. Typed callbacks
  use `cx.create_native(payload)` with their declared result scope. Contextual
  conversion rejects a different registered nominal type, incompatible argument
  layout or foreign runtime even when the Rust payload type is identical.
  `read` lends a short shared reference. `native_data!` defines a complete struct
  and checks every field against the recursive NativeData contract; supported
  scalars, fixed arrays and tuples receive empty tracing and fixed accounting.
  Only NativeStorage::data registration enables `edit`. Arbitrary hand-authored
  NativePayload registrations do not grant mutable access. Manual unsafe NativeData
  implementations are explicitly trusted storage contracts, not ordinary safe
  registration. An edit publishes its revision before lending data, preserves
  completed writes on error/unwind and releases its exclusive borrow on every exit.
  It cannot grow allocation or overwrite script edges. This intentionally leaves
  variable-sized fields and script references to the checked setter boundary.
  Six source-free tests cover typed constructors, shared aliases, rejection before
  publication, tracing/cleanup, nominal and runtime identity, exact generic layout
  scope, GC/reentry rejection during borrows, completed edit effects after failures,
  unchanged heap accounting, explicit capability checks, old-version retention and
  exclusive runtime thread transfer. All 145 native-boundary tests and nine runtime
  doctests pass; compile-fail examples reject a heap identity in ordinary data and
  an escaping mutable reference. Strict runtime/VM all-target Clippy, structure
  (878 Rust files, zero violations/exceptions), formatting and diff checks pass.
  Logs: target/go05/payload-*. No carried implementation failure remains.
  Managed traced storage and its cycle exercise are recorded below. Native callable
  boxing and GO05 acceptance remain. No substep commit was made.
- GO05 managed traced payloads (uncommitted): ManagedStorage<T> fixes private field
  declarations and opaque registration tokens before native type installation.
  NativeType<Managed<T>>::bind_field checks each Rust mapping against its scoped
  field application; cloned bindings reuse the descriptor. Generic fields preserve
  the source of each native type argument. Managed<T> keeps fixed NativeData and
  private Values separately, with no public mutable field/payload view or embedded
  host root lease. ManagedBuilder retains each initializer until construction or
  replacement commits, rejects missing fields, and preserves prior values when a
  later conversion fails. Typed native constructors use result_native_type to build
  against their declared application without capturing an installation-specific
  handle. `get` returns ordinary converted data or retained handles. `set` and whole
  builder replacement validate new values before overwriting; their storage commit
  boundaries retain access to the old edges for future barriers. `edit_data` lends
  only fixed data, preserving accounting and completed effects on error/unwind.
  These additions use existing payload tracing and metadata traversal; no collector
  algorithm, graph marker or sweep code changed.
  Five source-free integration tests cover alias visibility, failed and incomplete
  construction/replacement, conversion with GC, callback reentry after releasing
  storage, borrow cleanup, old-code calls, foreign runtime/schema rejection, and
  incompatible same-ID generic layout scopes. A payload/closure cycle remains live
  through an exported callback and is reclaimed after external roots disappear;
  its obsolete program is reclaimed as well. Compile-fail examples reject whole
  Managed mutation and escaped data borrows. All 150 native-boundary tests and 11
  runtime doctests pass. Strict runtime/VM all-target Clippy, structure (884 Rust
  files, zero violations/exceptions), formatting and diff checks pass. A nested
  mutable borrow in a new test was corrected before acceptance; no carried failure
  remains. Logs: target/go05/managed-*. Native callable boxing is recorded below;
  GO05 remains unaccepted and uncommitted.
- GO05 native callable boxing (uncommitted): the closure representation now
  distinguishes script and checked native targets. Converting a PinnedFunction
  for a native entry records its import, exact signature and applied environment,
  then uses the ordinary closure roots and native execution frame. No script wrapper,
  source specialization or secondary interpreter is required. The heap stores no
  LinkedCallable root lease: existing closure program/environment graph edges retain
  executable dependencies. Signature inspection and the advanced borrowed-call
  adapter support both targets. Diagnostic snapshots no longer assume every closure
  has a script FunctionRef; copies still cannot republish released metadata.
  Four new source-free tests cover native value round trips, script captures,
  typed and advanced native reentry with GC, exact generic nominal scopes, failure
  and cancellation cleanup, missing nested application rejection, old-version
  execution and module-state/native-closure cycle reclamation. The existing shared
  application artifact test now boxes an entry whose specialized import is absent,
  executes it from script and host, and verifies environment release after the last
  handle. All 154 native-boundary tests and 132 runtime unit tests pass. Workspace
  all-target compilation, strict runtime/VM all-target Clippy, structure (886 Rust
  files, zero violations/exceptions), formatting and diff checks pass. The expanded
  advanced-call test also passes its focused rerun. Logs: target/go05/native-box-*.
  No carried build or test failure remains. Acceptance review identified a remaining
  designed API gap: NativeObject does not yet share the prepared inherent-member
  call surface implemented for script Object. Complete native payload instance and
  associated member binding through the same checked evidence/cache path, including
  generic applications and direct declaration binding, before GO05 acceptance.
  Then perform phase integration and make the single GO05 commit; GO06 owns standard
  library adoption, obsolete API removal and the final feature/backend matrix.
- GO05 native inherent members (uncommitted): script ObjectType and native
  NativeType now share InherentMember/Method resolution and checked entry caching.
  Both support name and full declaration-identity binding, instance calls and
  associated functions, impl parameters and method-local applications. NativeObject
  rejects reentry during payload borrows and retains exact receiver/program scopes.
  InherentMethodsBuilder offers typed receiver/argument callbacks and FunctionBuilder
  configuration; later impl groups substitute receiver facts in signatures, bounds,
  concrete results and selected requirements while preserving method-local binders.
  Three new source-free tests cover native/script calls, repeated binding, different
  generic applications, method parameters, selected trait operations with GC,
  registration identities, foreign runtimes, borrowed payloads and pinned reload.
  All 157 native-boundary tests pass, including the preexisting script member and
  cache-release cases. Structure checks pass (892 Rust files, zero violations or
  exceptions). Phase-wide Clippy found an obsolete mutable borrow in the benchmark
  caller of the shared execution facade; the caller was updated. Stage acceptance
  follows below; no substep commit was made. Logs: target/go05/native-method-*.
- Callable evidence so far: source-free tests cover visibility/signature rejection
  before effects, object results, closure/native reentry, returned captures,
  old-version calls and weak-cache reclamation, script function value conversion,
  direct registered native declaration binding, cancellation/depth/trap cleanup
  and candidate isolation. The SDK context/retained-result test passes. A native
  binding test found that import operands belong to their using module, not the
  declaration module; lookup now retains that checked carrier while checking
  public visibility at the declaration. All seven callable tests and the SDK
  typed-call test pass, as does strict runtime/VM/embed all-target Clippy. Structure
  checks report 840 Rust files with zero violations/exceptions; fmt and diff
  checks pass. The full native-boundary regression passes all 98 tests, including
  existing reentry, borrow, cleanup, GC, artifact and thread-transfer coverage.
  No phase acceptance or commit yet.
- GO05 focused evidence so far: two compiler visibility tests, all 33 HIR import
  tests and ten runtime
  host-object tests pass, along with all ten existing conversion tests. Coverage
  includes aliases, private/readonly access, foreign runtimes, nominal mismatch,
  applied fields, source-free loading, failed construction/conversion, retained
  nested reads, cyclic reclamation and old-version retention/cache release, plus
  collection identity/access, removal retention, iteration cleanup and transfer
  of live typed handles/bindings with exclusive runtime ownership. All ten
  VM native-conversion tests pass, including source-free typed Object entries
  and ScriptVec native callbacks sharing script storage.
  The initial workspace all-target visibility check and runtime object check
  passed. Strict runtime/VM all-target Clippy and the structure check (834 Rust
  files, zero violations/exceptions) passed. Logs are under target/go05/.
  GO05 is not accepted: native factories, restricted payload edits, callable boxing
  completion and the full acceptance coverage remain pending.
- GO04 complete: KagariType/IntoKagari/FromKagari provide one fallible conversion
  boundary for scalar widths, owned UTF-8 String, Vec, installed ordinary
  Option/Result and value/argument tuples of arity 0-12. Conversion scopes pin the
  program, protect unpublished values, snapshot owned inputs before custom
  conversion/reentry, poll cancellation and bound depth/work/string bytes. Owned
  cycles reject; identity-preserving adapters can retain them. Error/panic cleanup
  releases temporary roots. Nominal origins and declared access remain exact;
  conversion does not erase readonly qualifiers or implicitly install providers.
- NativeBinding::typed/typed_method use NativeContext, outer tuple arguments and a
  separate method receiver. ModuleBuilder::add_function generates concrete types
  from Rust while FunctionSpec supplies names and full Markdown parameter/return
  documentation. It validates the candidate before publishing declarations or
  bindings. Generated KGR parsing, materialized file content, docs and navigation
  have integration coverage. Concrete results use the compiler-selected interface
  adapter, without post-callback trait lookup. NativeContext::collect polls around
  iterator steps and checks capacity growth. The typed_native example runs this
  ordinary String/Vec registration and typed host-entry path.
- VM/SDK execute_typed converts host arguments and returns owned data or retained
  handles using the same runtime scope. Known signature failures precede script
  effects; data-dependent failures preserve completed effects. It consumes existing
  closed entry metadata, without source-time specialization. Explicit named-entry
  policy is unchanged; cached visibility-checked function/member bindings are GO05.
  ExecutionReport::return_value is now RootedValue for interpreter and prepared
  native/fallback execution. Moving it out of the report transfers retention;
  cloning/dropping follows the central lease model. Consumers, examples and
  benchmarks were updated directly, with no raw-result compatibility alias.
- GO04 acceptance: the runtime/VM/embed subsystem command
  `cargo test -p kagari-runtime -p kagari-vm -p kagari-embed --no-fail-fast`
  exercised 953 tests plus one pre-existing ignored manual measurement. Its 100
  failures in 19 targets were obsolete cleanup expectations or redundant manual
  rooting after reports gained ownership. Those targets were corrected and rerun:
  all 173 tests in the 18 affected embed targets and all 107 VM unit tests passed.
  Cleanup assertions still require zero roots after result release. Together with
  the added collect-cancellation test, 954 distinct tests pass. Final focused
  conversion (10), typed/source-free VM (8), materialized docs and Tokio handoff
  checks pass; the handoff now moves the returned root without manual registration.
  Workspace all-target check, strict all-target runtime/VM/embed Clippy, structure
  check (824 Rust files, zero violations/exceptions), formatting, documentation
  links and diff checks passed. Production imports, visibility, macro boundaries,
  ownership and affected file responsibilities were reviewed. No carried build/
  test error or structural debt remains. Logs under target/go04/ are disposable.
- GO05 continues with retained collection/payload access and cached checked calls.
  Existing low-level scalar/view NativeFunction/NativeOutput/CallArguments callers
  remain migration consumers until GO05 supplies their retained handle/call forms;
  GO06 owns their standard-library adoption and removal. They are not aliases or a
  separate runtime graph, and the new ordinary API uses only the fallible shared
  adapter. Final workspace/feature/backend matrices remain GO06 acceptance.
- GO01 complete: root values now live in a heap-owned generational table.
  gc::roots::RootedValue and RootSet hold checked identities and Arc leases;
  reads require the owning heap. Expired entries are pruned at registration/GC,
  exhausted generations retire slots, and borrowed slot conflicts reject safely.
  Frame/native/debug roots share this path. Leases are Send + Sync and do not own
  storage; runtime teardown releases a retained native payload exactly once.
- GO01 validation passed: cargo check --workspace --all-targets; strict all-target
  Clippy for kagari-runtime/kagari-vm/kagari-embed; structure check (774 Rust files,
  no violations); cargo fmt --all -- --check; git diff --check; 226 local doc links.
  The following command passed 283 tests, including language-contract routes,
  root identity/reuse/transfer/teardown, GC/reentry and native callbacks:
  `cargo test -p kagari-runtime -p kagari-vm -p kagari-embed --lib --test gc_ownership --test runtime_substrate --test execution_sessions --test native_boundary --test native_provider_reset`.
  Initial test/example root-read signature errors were resolved. No carried build
  or test error remains. Logs under target/go01-* are disposable.
- GO02 complete: execution, program and metadata stores are runtime-owned; graph
  traversal and checked reference publication cover the accepted internal scope.
  Exact generations, immediate session cleanup, scoped reentry and escaped generic
  environments are preserved. No carried compilation/test error or structural debt
  remains at the accepted GO02 commit 4527ed5b.
- GO03 complete. Audited immutable type arguments/bindings, layout
  and storage contracts, selected scoped signatures and stored callable/cursor
  descriptors now use Arc; compile-time Send + Sync checks cover these types and
  Value. They contain checked IDs/type facts, not mutable runtime storage. The
  type-origin test transfers retained facts through an OS thread after collecting
  their environments and still resolves the original nominal layout.
  NativeCodeOwner now requires Send + Sync and products/installed handles use Arc.
  Cranelift 0.132.0's JITModule is Send but not Sync; its private CodeMemory wraps
  the module in Mutex, accessing it only through exclusive get_mut during compile
  and final destruction. Finalized invocation takes no owner lock. A real-code
  test compiles, installs and invokes two runtimes before moving them to separate
  worker threads for further invocation and final owner release. No unsafe Send/Sync implementation was added.
  Shared native/host callbacks, path adapters and storage factories now use Arc
  with Send + Sync bounds. Exclusive NativePayload storage requires only Send.
  Captured mutable host services in examples/tests use synchronized state and
  release guards before callback reentry. Mapped iterator operation exclusion now
  lives in the heap lease table; argument-scoped guards release on every exit.
  Runtime owns a boxed Send observer. Installation/replacement requires quiescence;
  sessions activate it once and VM debugger access borrows the same owned state.
  Runtime is statically Send/not-Sync. A deterministic two-thread handoff preserves
  Cell payload/observer state, exact old closure dependencies and receiver-thread
  destruction. Tokio dev-only coverage owns KagariRuntime and live rooted values
  across message-receive awaits and returns the runtime to the host.
  PreparedProgram's Rc/RefCell native cache remains preparation-side, never retained
  by a runtime; concurrent/shared preparation is not claimed. Finalized products
  are transferable independently of this cache.
- GO03 acceptance: `cargo test -p kagari-runtime -p kagari-vm -p kagari-embed -p kagari-stdlib -p kagari-codegen-cranelift --no-fail-fast`
  passed 942 tests, including five compile-fail cases, source/artifact/interpreter/JIT
  behavior, live-runtime OS-thread/Tokio transfer, observer ownership, native
  operation unwind and receiving-thread destruction. One pre-existing manual
  performance measurement remains ignored. Workspace all-target check, strict
  all-target Clippy for runtime/VM/embed/stdlib/Cranelift, structure (810 Rust files,
  zero violations/exceptions), formatting, 98 local documentation links and diff
  checks passed. Initial callback-consumer and integration-test placement errors
  were corrected without weakening behavior checks. No carried build/test error or
  structural debt remains. Disposable logs: target/go03/*. Final workspace/feature
  matrices remain GO06 acceptance. Next phase is GO04: recursive conversion,
  tuple arguments, documented typed registration and retained public results;
  GO05-GO06 remain pending. No GO04 implementation is included in the GO03 commit.
- GO02 acceptance: `cargo test -p kagari-runtime -p kagari-vm -p kagari-embed --no-fail-fast`
  initially passed 925 tests and found one obsolete lease-count assertion in
  library_mapping::retained_map_uses_its_original_callback_after_reload. Replace
  that ownership-detail assertion with installed-record reachability and actual
  reclamation after root release; retain old/fresh callback result assertions.
  `cargo test -p kagari-vm --test library_collections` then passed all 18 tests.
  Combined coverage is 926 tests passing, including compile-fail scope tests and
  source/artifact/interpreter/JIT behavior; one pre-existing manual performance
  measurement remains ignored. No test was disabled or behavioral check removed.
  Workspace all-target check, strict all-target Clippy for runtime/VM/embed, focused
  Clippy after the assertion update, structure (807 files, zero violations and
  exceptions), formatting, 88 local documentation links and diff checks passed.
  Ownership, production imports, visibility, macro boundaries and changed modules
  were reviewed. Disposable logs: target/go02-acceptance-*. Final workspace-wide
  tests, strict Clippy and feature/backend matrix remain GO06 acceptance.
- Collector/storage boundary: the marker consumes identities and an edge visitor;
  storage owns physical value/reference traversal. The coordinated graph covers heap
  objects, executable metadata and program instances. Latest/live-staged programs,
  explicit leases and active roots seed traversal. Reached programs trace exact
  dependency-member slots. Type-only argument origins and layout provenance do not
  retain mutable instances. Obsolete module/closure/environment cycles retire in
  one pass; no independent module-only sweep remains. Marking/accounting and storage
  borrow validation precede all detachment. Tracing/destruction cannot reenter or
  mutate roots; trace failure prevents sweeping, and destructor panic is contained
  while disposal continues outside table borrows.
- Execution storage: Runtime owns GcHeap directly; SessionStore owns session records
  and separately borrowed frame stacks. SessionId checks owner/slot/generation and
  retires exhausted slots. Frames require the owning Runtime. Session end immediately
  invalidates IDs and releases state/roots/version/iteration leases; only empty frame
  buckets can await pruning when another session's view is borrowed. Heap-owned
  exclusion tables replace shared mutable iteration/mutation/hash counters; cursor
  leases expire without heap scans and warmed short native leases add no allocations.
  ResourceState now lives by value in GcHeap; runtime components no longer own
  Rc/Weak resource state. ExecutionSession, CandidateSession and ExecutionStack
  borrow storage, preventing runtime replacement/destruction during execution.
  HostBorrowTable owns records directly; borrowed HostCallGuard keeps the resource
  gate, and HostResourceScope holds central root leases plus a registered frame ID.
  Scope unwinding releases leases before the last session ends. Public root and
  collection leases still outlive runtime teardown without retaining payloads.
  ModuleStore also owns epoch reservation. VM/SDK execution and reload publication
  take checked shared references so active old-version sessions can coexist with
  publication. Runtime remains non-Sync; this supplies no concurrent execution or
  Send claim. Registration and owner replacement still need mutable access.
  Existing consumers now construct the VM before beginning a borrowed session;
  compiler-suggested unused mutable bindings were removed across callers.
- Program storage: Runtime directly owns non-Clone ModuleStore. ProgramLease and
  StagedProgram carry lease tokens, never store ownership. Publication checks the
  actual installed code and exact staging token. Last staged-handle drop immediately
  hides all candidate members from access and retention; coordinated collection
  disposes of the records. Automatic safepoints and validated load/stage operations
  collect abandoned candidates; disabled automatic GC remains explicit-only.
  ModuleStore::loaded_count replaces duplicate resource admission counters.
  ModuleRecord now owns each instance, native links and mutable layout caches by
  value; collection detaches them together before disposal. LoadedModule shares only
  an immutable Arc<ProgramDescriptor>, proven Send + Sync and readable across threads
  after runtime teardown. It cannot retain native callbacks or caches. Central lookup
  checks exact descriptor identity, owner and epoch; copying keys and binding facts
  cannot authorize execution. Native invocation drops the store borrow before Rust
  callbacks. Cached layout application uses installed records; detached type facts
  remain readable through pure descriptor application without reviving runtime state.
  ModuleStore no longer owns Rc<ResourceState>. Runtime owns execution gates,
  candidate instance isolation and public version retention; private store methods
  operate on checked records. Runtime::retain_module replaces public retention on
  ModuleStore, and instance access goes through Runtime. Load entry checks the
  execution gate before linking/reserving a version. Collection-time retention
  cannot resurrect records, and quarantine blocks retention, instance access,
  loading and candidate publication without changing the current version.
- Reference writes: Runtime::read_module_slot/write_module_slot replace unrestricted
  module_instance_mut. Bytecode driver and host tests use the same checked path;
  slot declarations, mutability, representations, heap identity/liveness and candidate
  ownership are checked before replacement. Snapshot edits are detached. Storage
  fault injection remains test-only and still proves quarantine and frame/root/depth
  cleanup. GC tests cover failed writes, replacement/removal, obsolete program cycles
  and transitive candidate validation at publication.
  Executable root metadata is now written through Runtime; the root setter validates
  each incoming program/metadata edge before replacing the old references. Frame,
  selected-call and prepared-method roots share this boundary. Failed replacement
  leaves old dependencies reachable, and stale/foreign roots cannot revive records.
  Collector atomicity tests use explicit test-only corruption after asserting that
  normal publication rejects invalid IDs. Parent-interface, method-application and
  receiver-operation caches now expose reads only; execution_metadata::links owns
  publication after checking destination identity/slot and incoming dependencies.
  Shared/method receiver cells are validated together before either changes. Invalid,
  stale, foreign and duplicate writes preserve the old graph; published edges retain
  their referents until the owner becomes unreachable. Tests retain explicit
  corruption hooks solely for collector failure coverage.
- Executable metadata: central root records trace active frames, prepared methods
  and selected native calls. Publication/frame entry validates executable edges;
  detached diagnostic data cannot resurrect released program or operation records,
  even when supplying code remains installed. GcHeap owns OperationGroupStore,
  ApplicationStore, InterfaceStore and EnvironmentStore by value. Their typed
  generational IDs check owner/bounds/generation, do not create roots and contribute
  to safepoint pressure.
  Allocation/view-borrow failures reject without partial sweeping or accounting.
- Operation groups own BoundOperation entries by value. OperationId selects a stable
  member ordinal; forwarding reuses that ID. Tracing a selected witness reaches its
  supplying group without exposing sibling witnesses. Associated type facts are
  copied separately. Checked MethodView reads replace Rc descriptor ownership;
  public method metadata inspection requires Runtime. Closed applications reuse
  receiver environments; method-local generic applications remain call-specific.
- Interface snapshots live by value behind InterfaceSnapshotId. Heap wrappers,
  parent caches and prepared methods hold ID edges. Metadata roots reach receiver
  objects and exact program dependencies without retaining heap wrappers. Repeated
  upcasts share parent records while keeping wrapper identities. Prepared methods
  copy receiver/type facts and cannot own snapshot records past runtime teardown.
  Invalid declaration/slot access preserves its prior error classification.
- Closure records now live by value in existing heap slots. PreparedClosure holds
  only a Value identity; snapshot inspection requires Runtime and returns a scoped
  view. Frame entry accepts a closure Value, validates it and roots captures without
  a temporary capture vector. Native invocation releases the view before executing
  script. Stored callbacks trace identities; the then-current RootedCallable lease
  was later replaced by PinnedFunction in GO06.
  Neither keeps closure storage alive after teardown. Explicit diagnostic copies
  in stale-environment tests are not execution capabilities. Central checked mutable
  slot access replaces panicking heap borrow_mut paths; allocation and mutation
  under a scoped read reject without changing contents or counters.
- TypeBindings now owns only immutable substitutions, associated-type facts and
  type-only parent scopes. Layouts, type origins and compatibility checks use these
  snapshots without retaining executable environment records. EnvironmentStore owns
  EnvironmentRecord values; TypeEnvironment is an EnvironmentId plus immutable type
  facts, not a storage owner or root. Frames, closures, interface applications and
  selected native calls trace environment IDs. Publication rechecks parent/selection
  edges; extensions allocate a new record without changing existing handles. The
  coordinated collector marks parents and selected groups, validates all table
  borrows before detachment and includes environment occupancy in safepoint pressure.
  Retained handles cannot prevent reclamation or alias recycled slots. Type facts
  remain usable after their executable records are collected.
- Reference-write validation passed: `cargo check --workspace --all-targets`;
  `cargo test -p kagari-runtime -p kagari-vm --lib --test execution_sessions --test execution_frames --test offline_host --test native_boundary --test native_allocations --no-fail-fast`
  (345 tests); `cargo test -p kagari-embed --test generic_reload --test callable_traits --test trait_inheritance --test try_protocols --test cranelift_preparation --test associated_types --no-fail-fast`
  (44 tests). Strict all-target Clippy for runtime/VM/embed, structure (806 files,
  zero violations/exceptions), formatting and git diff --check passed. Disposable
  logs: target/go02-reference-writes-*. No carried error remains.
- Previous scoped-resource validation passed: `cargo check --workspace --all-targets`;
  `cargo test -p kagari-runtime -p kagari-vm --lib --test execution_sessions --test execution_frames --test host_borrows --test host_scopes --test native_boundary --test native_allocations --test prepared_execution --no-fail-fast`
  (351 tests, including host-scope unwind with/without an execution session);
  `cargo test -p kagari-embed --lib --test error_traces --test result_option --test generic_reload --test source_modules --test embedding_api --test cranelift_preparation --no-fail-fast`
  (70 tests, including the source/artifact/interpreter/JIT language-contract matrix);
  `cargo test -p kagari-runtime --doc` (3 compile-fail tests, including runtime/table
  teardown while a borrowed execution/host scope remains live). Strict all-target
  Clippy for runtime/VM/embed, structure (804 files, zero violations/exceptions),
  formatting and git diff --check passed. Initial borrowed-consumer compile errors
  and one needless-borrow lint were fixed without changing behavioral assertions.
  Ownership probes now observe a test-only destruction token on the value-owned
  ResourceState. Disposable logs: target/go02-borrowed-sessions-*.
- Previous module-access validation passed: `cargo check --workspace --all-targets`;
  `cargo test -p kagari-runtime -p kagari-vm --lib --test execution_sessions --test execution_frames --test runtime_substrate --test native_boundary --test offline_host --no-fail-fast`
  (343 tests, including collection-time retention and quarantine regressions);
  `cargo test -p kagari-embed --test source_modules --test generic_reload --no-fail-fast`
  (31 tests); strict all-target Clippy for runtime/VM/embed; structure (804 files,
  zero violations/exceptions), formatting and git diff --check. Disposable logs:
  target/go02-module-access-*. No carried build/test error remains.
- Previous program-record validation passed: `cargo check --workspace --all-targets`;
  `cargo test -p kagari-runtime -p kagari-vm --lib --test native_boundary --test native_allocations --test struct_layouts --test native_execution --test offline_host --no-fail-fast`
  (335 tests, including five new module-record lifetime/cache/identity regressions);
  `cargo test -p kagari-embed --test generic_reload --test callable_traits --test trait_inheritance --test try_protocols --test iteration_traits --test native_provider_reset --test associated_types --test source_modules --no-fail-fast`
  (100 tests); strict all-target Clippy for runtime/VM/embed; structure (804 files,
  zero violations/exceptions), formatting and git diff --check. Logs under
  target/go02-program-records-* are disposable. Earlier GO02 checks also covered runtime
  frame/session/GC/native/layout integrations and embed source_modules,
  embedding_api and cranelift_preparation; final integration still belongs to GO06.
  Existing tests cover candidate abandonment/borrowed cleanup, foreign/stale and
  exhausted identities, sibling-witness visibility, metadata-only scheduling,
  cached native applications, parent views, heap/program cycles, exact old-version
  execution, cancellation, traps, callback reentry and panic-safe collection.

### Other proposals

These are design documents, not additional active execution plans. Activation and
exact scheduling must be agreed within the user's scope. Completed migrations are
not prerequisites to replay.

| Track | Scope and dependencies |
| --- | --- |
| [Rust interoperability](rust-interop-design.md) — RI00-RI05 | DTO derives, optional schema-backed Serde and independently retained opaque objects; reuse GO conversion/root contracts. External host mutation remains distinct from GO managed-payload editing. |
| [Host API](host-api-refactor.md) — HA00-HA05 | Preparation/load/reload facade and logical entry policy; reuse GO typed calls/roots and RI value extensions. Package identity and update compatibility must be frozen before affected phases, without requiring both entire tracks first. |
| [Packages](package-design.md) — PK00-PK04 | Cargo-style manifests, exact dependency graphs and source/module identities. First source kinds, single-selection policy and defaults remain review choices. |
| [Update model](update-model-design.md) — UP00-UP05 | Compatible publication versus explicit state replacement. GO supplies calls/roots, RI value extensions, PK identities and HA the facade; lower-level cutover belongs to UP. |
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
