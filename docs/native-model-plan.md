# Native HIR declaration implementation plan

**Status:** The user authorized unattended NM01–NM04 implementation and phase commits after
scope review. NM01 implementation and its integrated frontend prerequisites are complete locally;
NM02 editor/grammar acceptance and NM03 shared-analysis/SDK/executable sampling are complete
locally. NM04 documentation consolidation and final review remain.
Baseline: design/hir-native-model, c19a0ffbf.
The [design](native-model-design.md) owns structures and semantics; this plan owns sequencing and
acceptance. The [roadmap](implementation-roadmap.md#native-model-replacement) owns activation.
This replaces the unstarted whole-stack NM01–NM07 sequence, not a second migration queue.

## Outcome and scope

```text
Rust structured declarations --------------+
exported Native .kgr -> declaration parser -+-> common input -> HIR -> shared checking
user source -> ordinary parser ------------+                     |             |
                                                       existing compiler   analyzer
                                                           handoff         queries
```

**Include:**

- Owned, ID-linked HIR with explicit Required/Script/Native function implementations and ordinary
  type, field, method, associated-function and trait-impl records. No arena or universal node IDs.
- Direct structured declaration entry for compilation, without rendering/parsing text first.
  Adapt existing upstream declaration data; a new Rust registration DSL, builder/session framework
  or generalized authoring API is not a deliverable merely to construct this input.
- External Native .kgr declaration parsing into the same unchecked input, analyzed together with
  user source. Tooling declarations cannot grant Runtime binding authority.
- Native fields: names/types/visibility, var/val checks, scalar-value versus ordinary managed
  object-handle results. Native declarations need not be opaque. New Runtime field adapters/execution
  are not included after scope narrowing; record the intended field behavior without claiming it.
- Ref/RefMut builtin recognition and explicit reference-type records: parse ordinary type
  applications, resolve builtin identity/target and retain Shared/Mutable. No new borrowing,
  projection, implicit conversions, method forwarding or execution capability is included;
  preserve all existing supported routes and validation.
- Necessary syntax/AST/lowering/shared-vocabulary and consumer adjustments. Scope is the frontend
  responsibility, not a promise that only files physically under kagari-hir/src/hir will change.
- Matching origins, diagnostics and navigation to exported read-only declarations; common semantic
  rules, not an analyzer-only type checker.

**Defer:**

- Native registration/preparation API redesign, new LibraryContracts/NativeProvider products,
  provider key/wire cutover, NativeDefault execution replacement and new GC/storage adapters.
- Binary prebuilding, generated Rust tables, MIR/shared-library export, build.rs/bootstrap,
  installation/link preparation, application caches and startup/performance work.
- Build-time .kgr production/publication and LSP directory watching. Retain the agreed configurable
  directory/default and refresh design, but current tests load declaration inputs explicitly.
- New Ref/RefMut behavior, borrowing/execution, child-reference projection and method forwarding;
  recognition/data representation alone is included above. Preserve existing capability checks.
- Output/reflection extraction, collection restoration, general LSP scheduling and cache policy.

## Ownership and boundaries

| Owner | Current responsibility |
| --- | --- |
| kagari-syntax and required grammar/formatter consumers | Unified Native/ordinary syntax and AST, independent of source-versus-exported semantic admission |
| kagari-hir | Common inputs, direct/parsed lowering, ordinary HIR records, resolution/checking and matching origins |
| Existing common/types/source owners | Minimal shared identity, declaration and reporting vocabulary changes where actually needed |
| kagari-analyzer | Queries/navigation over matching semantic results, including explicitly supplied exported declarations |
| Existing compiler/SDK/Runtime consumers | Necessary call-site adjustments preserving current binding, admission and execution behavior |

No new product dependency, facade crate or Runtime-to-HIR edge. Replace obsolete frontend models
at their owner; the existing Runtime/registration boundary remains active, not a speculative old
reader. Changes must preserve ordinary current Native routes without inventing a new provider API.
Upstream/downstream code may change where it directly produces or consumes the new HIR. This
is not an API freeze: update necessary signatures, records, names and call sites together, without
compatibility aliases, duplicate entrypoints or mapping back to obsolete HIR. The existing layers
remain responsible for their current behavior; making their overall architecture cleaner is later
work. If a frontend slice requires a new execution capability, revisit scope explicitly first.

## Implementation inventory

This is the owned change list, not authorization to redesign every crate touched by a type rename.
Paths identify current responsibility owners; exact file splits follow implementation review.

| Slice | Principal current owners | Required change / limit |
| --- | --- | --- |
| Native type/field records | kagari-hir/src/hir/item.rs, hir/item/adt.rs, hir/ids.rs, source_map.rs | Replace opaque-only declaration naming/model with Native declarations and shared fields; extend field ownership where required; keep owned tables and checked IDs |
| Functions and signatures | kagari-hir/src/hir/item/function.rs, semantic/signatures.rs, lower/ | Explicit Required/Script/Native; ordinary params/self and shared impl links; preserve Function versus TypedFunction ownership, not a second Native signature table |
| Types and builtins | kagari-hir/src/types.rs, hir/ty.rs, language/, native.rs; necessary kagari-types vocabulary | Separate builtin String/Range/reference interpretation from Native declaration classification; retain Ref/RefMut target/access records; no new reference behavior |
| Declaration input/lowering | kagari-hir/src/native/api.rs, native/api/preparation.rs, lower.rs, lower/, declarations/ | Convert actual structured registration data directly to HIR, alongside parsed declarations; record correspondence on construction, not render/parse/name/span joins |
| Native facts/ownership | kagari-hir/src/lower.rs, native/, aggregates/, declarations/ | Move genuine declaration facts to type/function/member owners; retain required environment/installation/representation evidence at explicit existing boundaries; audit attachments individually, not blanket-delete native_* |
| Checking and publication | kagari-hir/src/typeck/, resolver/, imports/, analysis/, compile_input/ | Common field/call/trait/bound checks and checked handoff; include OperationRequirement target/trait/member/type arguments where actually analyzed; preserve context validity, cancellation and current weak retention |
| Syntax producer | kagari-syntax/src/parser.rs, parser/grammar/items.rs, parser/grammar/members.rs and required lexer/token/AST consumers | Add explicit native syntax and remove Source/Declarations grammar modes; update parsing/cache/visitor assumptions, retain recovery and context rules |
| Upstream adaptation | Existing kagari-types declarations, kagari-runtime Native registration descriptors, kagari/src/engine/declarations.rs and stdlib/derive callers where affected | Supply new HIR input and valid syntax/identities through current workflow; preserve full Rust-embedded Script definitions; no registration/provider/build API redesign |
| Downstream adaptation | kagari-hir/src/compile_input/, kagari-compiler/src/source/lower/ and directly affected ABI/MIR/contract/Runtime consumers | Read new owners/implementation kinds and preserve executable checks/behavior; necessary type/field/wire call-site adjustments only, no new ABI, execution route, product or linker architecture |
| Tooling and repository consumers | kagari-analyzer queries/navigation, existing formatter/editor grammar, fixtures and architecture/spec docs | Consume unified AST/HIR and real exported-source locations; preserve complete Script definitions; no exporter, directory watcher, scheduler, UI feature or renderer-owner migration |

Before removing an attachment or access field, list its static semantic and executable consumers.
Remove the superseded frontend representation only when those consumers use the intended owner.
Do not preserve an old HIR model through forwarding wrappers, or fabricate typed success for a
capability the current backend cannot execute. A correct frontend field type is not proof of
new Runtime Native-field support.

## Acceptance boundary

- Common analysis of equivalent structured and parsed APIs produces matching types, field
  writeability, member/trait selection and diagnostic kinds; complete Script bodies remain intact.
- Native syntax is parsed once under one grammar. Ref/RefMut are recognized with target/access
  data, but no previously unsupported borrowing/projection/forwarding route is enabled.
- Existing affected execution/portable verification still works after consumer adaptation. No
  Runtime callback, Rust signature, DTO-conversion mode or ownership category leaks into HIR.
- HIR no longer needs generated text as authority for structured input; source ranges remain
  diagnostic/navigation data. No new producer/library product or whole-stack cleanup is required.
- Focused tests plus required local gates are reported accurately. Startup speedups, prebuild
  delivery, full matrices and a globally redesigned upstream/downstream are not completion claims.

## NM00 — design and boundary review

The broad initial proposal and measured baseline are historical evidence, not an instruction to
implement the whole stack. Rust mechanisms inspected at commit
5f4effd579500175af844241eb179e756190f1e7 support ordinary foreign signatures, trait members and
construction-time identity correspondence. MIR export/mono collection references support deferred
work only; they do not justify adding that work to this plan.

Authoring location does not determine implementation kind: Kagari traits/functions/structs
embedded or authored in Rust remain complete Script definitions. LSP library inputs contain those
complete definitions, including real bodies, alongside Native signatures/type interfaces when no
original source dependency is available. Do not invent omitted-default encoding, strip bodies or
make a Native stub from Rust-embedded script text. Preserve one owner identity; only genuinely
missing dependencies are errors.
Preserve caller-relevant constraints explicitly, not in comments, fake bodies or binary inputs.
Unified native syntax parsing does not approve Native authoring in ordinary source or grant binding
authority. Source-versus-exported admission belongs to analysis, not a grammar-mode switch.

## Execution phases

The scoped sequence below replaces the earlier unstarted phases. The subsequent user instruction
authorizes implementation and phase commits. Each checkpoint integrates its affected consumers.

### Execution decisions and current evidence

The NM01 category/ownership replacement requires coordinated parser, declaration-input, optional
parse/origin and shared-checker changes to keep the compiler/analyzer consumers buildable. These
NM02/NM03 prerequisites are included in the atomic NM01 frontend checkpoint rather than retaining
obsolete HIR wrappers or introducing a temporary second checker. NM02 separately completes the
editor/generated-grammar and written-syntax integration; NM03 completes SDK/executable sampling.

- Functions use HIR Required/Script/Native. Checked signatures separately distinguish unbound
  Native declarations from installed Native bindings; this is authority separation, not a fourth
  HIR implementation kind. NativeDefault forwarding remains Script with existing binding evidence.
- Owned ModuleDecl records lower directly with construction-time identity correspondence.
  Explicit supplied documents retain complete Script definitions and validated structural slots.
  Exact-ID type inputs are unchecked; selected public exposure is retained and validated against
  the same catalog. ID-only inputs may select a unique reachable exposure, never private access.
- Native contracts reuse scoped Ty/NativeCallableRequirement records rather than duplicate a
  type-expression schema. Shared recursive checks enforce actual category, identity, exact arity,
  binder scope, bounds, associated-member ownership and concrete-result conformance before use.
  Projection proofs consume declared associated-output bounds with exact substitutions.
- Source-less registrations use an empty logical graph key under the current SourceUnit graph;
  it is not a physical declaration input. NodeOrigin/DeclarationOrigin retain structured identity,
  syntax/source-location APIs return None, and complete-source packaging rejects missing authored
  manifests. Executables without complete source remain valid; debug capture does not invent spans.
- ExternalModule explicitly supplies real source plus scoped static sidecars for constructors,
  Host schemas/paths, method restrictions, ordered operations/results and foundation/conversion
  traits. The sidecars are analysis inputs, not bindings or installed authority. Mixed inputs check
  constructor ownership and dependencies. Native fields have shared types/writeability; unsupported
  executable fields remain rejected. No new Runtime adapter or Ref operation is implemented.
- Default SDK preparation uses structured input. Explicit materialized declaration views retain
  the existing parsed/validated navigation route; publication/export architecture is unchanged.
- Baseline blockers were repaired at their narrow owners: placement iteration, removed Runtime
  documentation-query test consumers, dropped weak-graph reuse expectation (with retained-snapshot
  positive control), and syntax token spacing. No execution invalidation guarantee was relaxed.

NM01 local evidence (macOS, workspace defaults; no full-workspace test run):

| Selection | Result |
| --- | --- |
| HIR native selection | 40 passed |
| External/equivalent/foundation/Host inputs | 9 passed |
| Transitive signature invalidation | 1 passed |
| Analyzer Native queries/navigation | 19 passed |
| Compiler source-package/origin tests | 9 passed |
| Compiler source-free field visibility | 1 passed |
| Stdlib native_schema including DTO/Serde/identity and source-free schema | 16 passed |
| Runtime enum declaration/navigation tests | 8 passed |
| SDK registration, documentation/cache, execution | 6 passed |
| Structure / fmt / workspace all-target Clippy / diff | Passed; existing optional blake3 Cargo manifest warning remains |

Independent review exposed and prompted fixes for actual offline foundation roles, Host semantic
classification, public-reexport exact references, malformed contract applications, constructor-owner
conflicts and fabricated canonical source locations. Corresponding independent assertions remain
at their semantic owners. Broader CI backend/feature/platform matrices remain unrun.

NM02 local evidence: syntax/formatter package tests, typed-field integration tests and doctests
passed, including Native mixed definitions, malformed recovery, cancellation and lossless LF/CRLF
surfaces. Editor generated-asset consistency, Tree-sitter corpus/unit, Zed captures, VS Code
TextMate and shared-region checks passed. Optional actual-editor/WASM host and complete inventory
checks remain CI-owned/unrun; no manual editor acceptance is claimed. Grammar/spec inventories,
AST naming and reserved-keyword consumers are synchronized without parser-mode aliases.

A second independent review found one remaining NM03 contract-admission defect: a default Native
operation/result involving owner-qualified Self inherited generic-application deferral instead of
proving its trait obligation. The NM03 repair identity-binds Self for those proofs only; the focused
negative/supertrait tests reproduce and then reject the invalid contracts. This does not change
generic declaration application's intentional Self deferral. NM03 acceptance includes this repair.

NM03 integration evidence:

- SDK sessions (5), direct/source preparation (3), native update/preparation selection (34),
  async typed-call source observations (5), compiler source-package cases (9) and bytecode
  source-package cases (3) passed. The SDK session scenario now rebuilds Native declarations
  under a zero parser-nesting budget and asserts absent parse/source surfaces before execution.
- The first SDK artifact run exposed an unconditional exact-source-package build after the
  compiler correctly omitted its manifest. SDK Compilation now retains an optional package;
  ordinary preparation/export works without it, explicit retain_source/exact-splice requests
  report typed MissingManifest, and whole-file updates still use retained immutable inputs.
  Complete authored-source tests explicitly supply their documents and preserve forgery checks.
- Regenerated feature_artifact and async feature products under target/; the separate frontend-
  disabled artifact selection passed (3 selected tests), and source-free actual Cranelift
  compilation/execution passed (1). Native-only SDK cargo check passed. No format/ABI bump.
- Native generic identity and DTO/handle distinctions passed (2); synchronous Host reentry and
  retained-closure-generation replacement passed (2); prior blocked Runtime scalar-target and
  active-application lifetime scenarios passed (3). Contract tests (6) and complete external
  equivalence/foundation/Host tests (9) pass with the Self-proof repair.
- Required structure, workspace fmt, workspace all-target Clippy and diff gates passed.
  The existing optional blake3 Cargo manifest warning remains non-fatal.
- One deliberately sampled existing collection regression still fails:
  cargo test -p kagari-stdlib --lib direct_native_imports_run_from_source_and_decoded_artifacts.
  It reports UnknownTypeAnnotation Vec<i32>/UnknownName Vec::new before execution. This exact
  failure predates NM and is already recorded by the [NC owner](native-collections-plan.md).
  No substitute collection or weakened assertion was introduced. It is not green evidence for
  Native import execution; source-free artifact and generic/Host samples cover this change's
  executable handoff. Full workspace/exhaustive matrices remain CI-only.

### NM01 — unify Native HIR records and ownership

- Represent function implementation kind explicitly; ordinary declarations own signatures, generics,
  fields and member/impl relationships. Rust callbacks, storage code and borrowed Rust references
  stay outside HIR. Required trait members do not become Native because an implementation is Rust.
- Keep one Native declaration identity across storage-management choices; do not add
  ScriptManaged/NativeManaged/ExternalHost categories to its HIR declaration. Retain actual
  compiler/ABI/Host-path facts at existing boundaries while adjusting their consumers. Opaque<T>
  stays a Rust handle, not a new script type. Include explicit Ref/RefMut reference records with
  target type and Shared/Mutable; reuse the current semantic type model, not duplicate Native
  declarations or a separate reference type system.
- Separate builtin String/Range interpretation from NativeTypeKind's frontend storage/registration
  mapping. Preserve existing semantic types, literal/annotation agreement, identity/arity and
  admission checks. Native methods on builtins do not make their target a Native type. Do not
  redesign registration or invent a generic intrinsic DSL to perform this frontend separation.
- Remove host_call / HostCallContract from the target HIR NativeContract. Ordinary parameter
  TypeRefId/resolved TypeId describes value, object identity or explicit Ref/RefMut capability;
  do not duplicate this with a passing-mode array. Keep the current HostCallPolicy execution path,
  binding checks and initializer/effect evidence until their separately scoped replacement.
- Review object-versus-DTO semantics at the declaration boundary: ordinary object T is not an
  implicit clone, Rust reference or ownership transfer. Object/Opaque handles preserve identity;
  supported Rust DTO/Serde extraction is binding conversion without automatic writeback. No new
  HIR serialization flag or conversion engine is introduced. A mandatory observable snapshot
  would need an explicit API/semantic decision, not a hidden callback-adapter choice.
- Separate trait override semantics from existing Native adapter access metadata. The universal
  Function.access / CallAccessContract model is withdrawn. Preserve existing scoped-reference
  method eligibility, capability checks and declaration/binding agreement without new trait loans.
  Ordinary Native impl/trait methods use self without Rust shared/mutable annotations; explicit
  reference parameters carry capability types. Do not automatically forward every Native method
  to Ref/RefMut or expand the deferred reference-method design.
- Trace LoweredModule Native attachments and move declaration facts to their semantic owners where
  necessary. Retain environment/boundary facts that still serve the current implementation; do not
  delete runtime-required evidence or begin whole-stack wire/provider migrations.
- Update actual consumers through the existing handoff. Do not weaken compiler admission or
  executable verification to make a frontend-only checkpoint build.

**Exit:** Native declarations/members have explicit frontend ownership; Required/Script/Native
and var/val remain distinct. Existing routes still build and retain their semantics. No new Runtime
capability, general callable loan rule or duplicate frontend semantic catalog is introduced.

### NM02 — parse external Native declarations

- Support native fn and native type, optional declared fields, ordinary inherent impls and trait
  impls, self versus associated functions, generics/bounds and associated bindings.
- Recognize Ref<T>/RefMut<T> through normal path/application parsing and builtin declaration
  resolution. Check constructor shape/arity and target name resolution; preserve existing target
  eligibility and contextual restrictions. Assert explicit Shared/Mutable type records without
  implementing new reference operations or automatically forwarding target methods. Parse
  reference requirements in parameter types, not a parallel Host passing-policy declaration.
- Unify parsing: explicit native syntax identifies Native signatures/types without
  Source/Declarations grammar switches. Remove the superseded parser mode API and adjust parse
  storage/cache/formatter/consumer assumptions, rather than keep a second syntax entrypoint.
  Preserve normal context-sensitive rules. Source-versus-exported input identity remains a
  semantic admission fact, not a change in how the same text parses. Native signatures need no
  body, Rust implementation or running Host; syntax alone grants no installation authority.
- Analyze complete Script definitions from project/library files or Rust-embedded source.
  Library inputs may combine ordinary traits/functions/structs with Native declarations; retain
  real Script/default bodies and ordinary field definitions. Native impls reference the one actual
  owner, whether loaded from original source or a complete export. No default-stub grammar,
  stripped trait, fake body or signature-only Script encoding is required. Diagnose genuinely
  missing dependencies, not the normal presence of Rust-authored definitions.
- Retain complete exported-source ranges for diagnostics/navigation and real attribution for
  structured inputs. Avoid fake files/spans and source-map-based declaration matching. Preserve
  parser recovery, limits, cancellation and existing source programs.
- Update required AST visitors, grammar/highlighting and formatter consumers together; do not build
  a text renderer, Cargo exporter or LSP directory watcher in this phase.

**Exit:** declaration fixtures parse/lower without executable bodies; malformed inputs reach the
intended diagnostic layer. The same text has the same AST independent of input provenance;
source/export admission and ordinary required-versus-provided trait rules remain explicit in
analysis. Fields and member forms have explicit AST/HIR assertions. Exported
source remains a read-only analysis input, not Runtime proof or editable implementation source.

### NM03 — unify structured and parsed declaration analysis

- Feed existing Rust registration declaration data directly into the common input/lowering/checking
  path. Adapt upstream descriptors/entrypoints where necessary to supply it, without redesigning
  registration workflows; generating .kgr is not an intermediate step.
- Analyze equivalent structured and external-declaration APIs with the same user program. Compare
  resolved field/call types, generic and trait selection, visibility/arity/bound errors and var/val
  checks. Direct IDs skip spelling selection, never accessibility or eligibility checks.
- Include Native type fields, inherent methods, associated functions and a Native impl of an actual
  Kagari trait. Check object-field results as ordinary managed handles without Ref wrappers.
  Compare explicit Ref/RefMut parameter type records from both input routes, including the same
  target declaration and different access modes; this is structural/type recognition evidence,
  not a new borrowed-call execution or alias-analysis test.
- Update analyzer declaration/call/documentation/navigation queries to use the same facts and
  exported-source locations. Load fixtures through existing input boundaries, not a new filesystem
  discovery service. Reject conflicting dependencies, foreign/stale handles and invalid contexts.
- Include a Rust-embedded Script source fixture containing a trait/default, ordinary function
  and struct alongside Native declarations. Direct and offline source inputs retain complete
  definitions, equivalent analysis and correct origins without duplicate owners. Test the input
  contract, not a new Cargo exporter. Preserve existing defaults/evidence; new block-fragment
  authoring and NativeDefault execution replacement are not required for this equivalence.

**Exit:** common frontend analysis works without Rust callbacks or binary prebuild products for
LSP, while compilation still uses structured declarations and the existing executable handoff.
No separate checker, name/span join for binding identity or synthetic runtime authority remains
in the replaced frontend path. New Runtime field execution is not a completion claim.

### NM04 — focused integration and documentation

- Remove superseded frontend entrypoints/data only after their consumers use intended owners.
  Review handwritten imports, visibility, re-exports, source origins and effective LOC.
- Sample directly impacted compiler/SDK/stdlib and analyzer routes; preserve source-free/Host safety
  at touched boundaries. New provider protocols, build products, output APIs and cache policy are
  not acceptance requirements.
- Update implemented HIR/syntax/query contracts at their architecture/spec owners. Keep unresolved
  build/Runtime/Ref work in the existing deferred section/roadmap, not a parallel execution plan.

**Exit:** selected frontend and consumer regressions pass, carried failures remain accurately
reported, and required local gates pass or stay explicitly outstanding. Full workspace tests and
exhaustive matrices remain CI-only. Completing this phase does not implement the broader Native model.

## Validation selection

Inspect existing owners before adding cases. Strengthen existing scenarios where they already
exercise the contract; fixed ordinary Kagari programs belong in owning .kgr fixtures.

| Contract | Selected owner/evidence |
| --- | --- |
| Function/member representation | HIR required_script_and_native_methods_keep_distinct_signature_implementations; installed_non_overridable_default_rejects_a_script_replacement |
| Builtin versus Native types | Existing String/Range literal, annotation and registered-method scenarios; assert identical builtin types from direct and exported inputs, arity/eligibility and declaration identity rather than name-only matching |
| Native declaration syntax | Unified parser fixtures with fields/member assertions and recovery; semantic source/export admission, real-source trait defaults and missing-dependency diagnostics |
| Argument semantics/binding boundary | Existing identity-preserving Object/Opaque and owned conversion scenarios where touched; parameter types remain the frontend contract, snapshot extraction is not implicit ownership transfer/writeback or alias preservation |
| Ref/RefMut recognition only | Direct/parsed inputs retain exact target identity and Shared/Mutable, reject malformed constructor shape or unresolved targets, preserve restrictions and do not admit new projection/forwarding/execution routes |
| Structured versus parsed semantics | Same API/user program for field types and var/val, methods, associated calls, source trait impls, bounds and visibility; independent query assertions |
| Identity/origins/reuse | HIR admitted_bodies_keep_owner_arena_and_completion_evidence_together; native_cache_retains_preparation_but_not_graphs_across_user_revisions; analyzer native-view identity/navigation scenarios |
| Existing executable handoff | Directly impacted SDK/stdlib actual-Rust/shared/default routes; source-free/native-only checks only where changed consumers affect them |

The initial broad baseline below predates scope narrowing. It is not the current acceptance
matrix or evidence that proposed HIR/parser interfaces exist. No new benchmark, whole-library
prebuild, provider hot-swap or binary bootstrap witness is required by this frontend change.

At implementation checkpoints run the affected tests, then:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Use scoped packages/targets/filters and directly affected regressions. Full workspace tests,
exhaustive platforms/features/backends and broad Miri remain CI-only. Documentation-only edits
need content/link/diff checks, not another build or repetition of known baseline failures.

## Deferred whole-stack direction

The design retains future registration/library products, build-time preparation, binary versus
generated-code choices, generic execution and directory publication for later review. None has an
implementation phase here. A future preparation API should finish applicable work once; binary
encoding is not intrinsically required to make registration complete. Runtime-only work must not
creep into frontend acceptance through an artifact or performance checklist. Deferral limits
implementation scope, not discussion scope: retain the design and distinguish agreed direction
from unresolved mechanisms instead of deleting it or treating it as rejected.

| Future area | Recorded direction / remaining choice |
| --- | --- |
| Registration/preparation | Finish applicable declaration/binding preparation once; do not require a render/parse/re-register cycle. Exact API and late-dependent work remain open. |
| Fixed-library prebuild | Move reusable work before startup. Private encoded data versus generated Rust tables is a representation choice, not the reason for the speedup. |
| Runtime installation | Consume prepared data and compiled adapters; retain recipient-specific identity/binding checks, live service injection and execution safety. No current installation redesign. |
| LSP build export | Same declarations produce .kgr independently of execution products; configurable default directory and complete successful-build publication remain future integration direction. |

## NM00 local evidence

Historical baseline: c19a0ffbf, before HIR-only scope clarification; only documentation changed.
Windows 11 x64 10.0.26300, i9-12900K (24 logical CPUs), rustc 1.100.0 (0ef1bc386), Cargo 1.100.0,
MSVC; workspace dev/test opt-level 1 with debug info, default target/parallelism, mixed warm cache.
No full-workspace tests, exhaustive matrices or Miri ran.

| Selected route | Actual result |
| --- | --- |
| HIR callable/default/owner/native-cache/authority selections | 6 passed |
| Stdlib shared function, generic default, source-free environment, exact trait/declaration installation | 6 passed, including public/private exact traits |
| SDK source trait portable binding and detached preparation | 2 passed |
| Regenerated feature_artifact, then separate frontend-disabled artifact_features invocation | 2 passed |
| cargo check -p kagari --no-default-features --features native --lib | Passed |
| Runtime scalar_targets and active application lifetime selection | 3 scenarios blocked before execution by E0599 |
| HIR transitive type-change reuse selection | Failed initial signatures_reused assertion at types.rs:88 |
| Structure / normal dependencies / diff | Passed: 2012 Rust files, 0 violations, 6 existing exceptions; Runtime has no compiler/HIR/syntax dependency; native-only SDK has no source frontend |
| Workspace fmt / workspace all-target Clippy | Failed at the existing owners below |

Total: 16 selected tests passed, one failed, three blocked. SDK emitted the existing unused
optional blake3 build-dependency warning. The source-free evidence used, in order:

```text
cargo run -p kagari --no-default-features --features source --example regenerate_feature_artifact
cargo test -p kagari --no-default-features --test artifact_features -- source_free_native_bindings_execute_and_release_scopes structurally_valid_library_binding_mismatch_is_rejected_on_load
```

These existing artifact tests do not prove any proposed build.rs/bootstrap integration.

### Cost sample and limits

Ran cargo run -p kagari --example architecture_baseline without concurrent build/benchmark work.
The [tracked workload](../crates/kagari/examples/architecture_baseline/main.rs) is
fn main() -> i32 { 40 + 2 } with installed foundation/default source/native features. Rust example
compilation was excluded; its counting allocator adds atomic overhead. Environment/profile match
the baseline above.

| Boundary | Median | Samples |
| --- | --- | --- |
| Fresh engine plus source -> artifact | 547.2492 ms | 21 |
| Decode plus native-input verification | 133.9115 ms | 101 |
| New Runtime plus shared Program linking | 11.7997 ms | 101 |
| Cached machine-code preparation/install | 96.3 us | 101 |
| SDK interpreter entry | 2.5 us | 10001 |
| SDK machine-code entry | 3.7 us | 10001 |

Artifact: 817500 bytes; bytecode-only 407883; portable MIR payload 409589. Same-process samples,
not speedups or cold-process results: process-level stdlib data was initialized. Native here means
JIT-compiled Script, not Rust callback latency. No standalone prebuild, callback first-call/generic
admission, unrelated-library growth or cross-target measurement exists. These costs do not expand
this HIR change into a startup optimization project.

## Baseline limits and checkpoint policy

No unrelated failure was repaired during NM00. The
[existing queue](implementation-roadmap.md#hir-storage-and-attribute-refactor) owns:

- Workspace fmt: untouched kagari-syntax/src/ast/tokens.rs spacing.
- Workspace Clippy: needless_range_loop in kagari-compiler/src/bytecode/placement.rs:110.
- HIR imports/tests/types.rs:88 expects signature reuse after dropping the old graph. Reproduction:
  cargo test -p kagari-hir --lib imports::tests::types::body_edit_cannot_reuse_signatures_after_transitive_type_change -- --exact.
  Later transitive assertions are not reached; preserve invalidation and fresh-analysis oracles.
- Runtime module/tests/enum_declarations.rs:107/112 still calls documentation/module_documentation
  on DeclarationSnapshot after analyzer extraction. Reproduction:
  cargo test -p kagari-runtime --lib -- scalar_targets active_native_application_outlives_its_original_execution_without_host_roots.
  This is a HIR/tool-query migration test-consumer error, not a measured Native callback failure.

Do not repeat unchanged failures awaiting their owner or call them green. Resolve affected owners
when touched, otherwise report the external acceptance blocker. No commit/push is authorized by
scope review; when requested, use buildable Conventional Commit checkpoints and Plan-Phase: NMxx.
The historical failures above have been rechecked and repaired as recorded in current execution
evidence. NM01–NM03 are locally complete; NM04 documentation consolidation remains pending. No Runtime redesign,
new binary format, field execution or speedup is claimed.
