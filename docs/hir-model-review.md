# HIR Model Review Notes

**Status:** HR04–HR08 compiler/tool separation is implemented on owned, ID-linked HIR.
Required acceptance remains open in the
[roadmap](implementation-roadmap.md#hir-storage-and-attribute-refactor).
The arena/reference migration is withdrawn, not completed. Native frontend phases 1–3 are
implemented. SO00–SO03 integrates origins and record-based IDs into the owned HIR model. The
roadmap retains local acceptance limits and separate follow-ups.
[Current HIR](architecture/hir.md) and [Runtime](architecture/runtime.md) own implemented contracts.

## Selected storage and attribute scope

`Module`/`HirNodes` retain ordinary owned category tables and ID links. `Module.nodes` is shared
module-wide storage; semantic body names such as `BodyOwner`, `BodyRoot` and `CheckedBodyInput`
are unchanged. `Function.params` owns parameter positions; `Module` owns typed attribute
attachments. `LoweringId` identifies one lowering, not an allocator. There is no borrowed graph,
universal owner table or session cutover.
See [storage and IDs](architecture/hir.md#storage-and-ids) for current data structures.

## Compiler and tooling boundary

The proposed retain/move/split inventory is implemented. The
[architecture owner](architecture/hir.md#editor-query-views-and-indexes) documents `.queries()`
views, canonical bindings, demand-built scope/site/navigation indexes and shared semantic
member eligibility. HIR retains checked admission, `ResolvedTypeRef.target`, import solver
observations and diagnostic/debug/reload origins. Lint projections are private to lint;
compiler, SDK and HIR production targets do not depend on analyzer. No new LSP capability,
references/rename index or scheduling model is implied.

### Cache policy is separate

Current caches retain owned declaration/signature/body records but only weak graph/query
handles; full-file bundles are weakly indexed. Without a live matching graph, queries lower/check
afresh. Native preparation retains validated registration/source/parse, not graph-local HIR.
Body facts remain separate from query context: sharing a body cannot retain stale dependency
validity or eligibility. See [cache ownership](architecture/hir.md#query-work-reuse-and-publication).

HR04–HR08 did not change this policy. The [single-run frontend plan](#single-run-frontend-plan)
now supersedes the strong-cache follow-up: remove retained source-stage reuse first. Revisit
incremental analysis only after the fresh compilation target is met and a workload justifies it.

`LoweringId`, `ResolvedTypeTarget` and internal `PreparedSignatures` name current roles;
`VisibleBinding`/`CompletionCandidate` name analyzer products. `FileAnalysis`/`AnalyzedModule`,
`BodySignatures` and `update_source` stay unchanged. Broader naming, incremental parsing,
function-granularity reuse/reclamation and LSP scheduling require separate scope.

## Crate-wide ownership and sharing review

**Status: review requested; no migration selected.** Audit the whole `kagari-hir` crate,
not just source preparation. `Arc` can share one allocation while adding multiple retention
paths. Every such path should identify its owner, consumers, identity and lifetime contract.
This is an ownership review, not an `Arc` count or a cache-capacity change.

| Starting point | Ownership question |
| --- | --- |
| [`PreparedSource`](../crates/kagari-hir/src/analysis/source_preparation.rs) and [module assembly](../crates/kagari-hir/src/analysis/declarations.rs) | Keep the parse and lowering paired in a named product rather than unpacking and reconstructing the pair. Their independent lifetimes are intentional. |
| [`FileSignatures`](../crates/kagari-hir/src/analysis/signatures.rs) | Its declaration and prepared signatures both retain the same lowering. Give the live result one lookup path without losing the cache's old-record/old-lowering checks. |
| [`FileAnalysis`](../crates/kagari-hir/src/analysis.rs) | It stores `source` separately from `result.facts().lowered.source`. Check whether the separate strong reference owns any independent lifetime or identity. |
| [`FunctionAnalysis` and `FunctionFacts`](../crates/kagari-hir/src/analysis/body.rs) | Keep shareable body facts distinct from the current request's signature context and eligibility. |

Map each product's authoritative owner, strong retention paths, weak cache entries and read-only
borrowers. Cover source/parse/HIR, declarations/imports, signatures/body facts, scoped definition
metadata, snapshots and checked compiler input. Trace a retained old snapshot through a new edit
and a cache entry whose matching HIR has expired; document what each handle keeps alive. Prefer
`&T` for read-only access and `Arc<T>` only where independent ownership is required. Audit pointer
identity checks before removing or relocating handles.

Review output: one ownership map and a bounded retain/remove/merge proposal, coordinated with
[flow and naming](#analysis-flow-and-naming-review). Preserve in-request parse sharing,
old-snapshot validity, scoped identity and checked compiler admission. Assess retained graphs
against the [single-run frontend plan](#single-run-frontend-plan), not as a reason to preserve
caches scheduled for removal. No whole-crate `Arc` removal or storage redesign is implied.

## Immutable HIR-to-compiler handoff

**Decision for follow-up:** HIR hands off checked facts and their immutable `DefinitionTable`
for the admitted dependency closure, not a handle to its writer. Today the facts and their
retained tables are already immutable, but [`CheckedCompilation::definition_context`](../crates/kagari-hir/src/compile_input/program.rs#L475)
and [`CheckedAnalysis::definition_context`](../crates/kagari-hir/src/lib.rs#L220) also expose the
shared append-only allocator. [`lower_program`](../crates/kagari-compiler/src/source/program.rs#L69)
passes that allocator to lowering, so newly generated compiler identities share HIR's writer.
Removing this dependency does not by itself remove the lock from other `DefinitionContext` users.

| Change point | Direction |
| --- | --- |
| HIR admission and checked APIs | Expose the checked closure's retained table, not a fresh snapshot of the live writer. Remove the writer accessors and audit their consumers. Analysis and declaration adoption still own their construction state. |
| Compiler input and lookups | Split the read-only source table from executable construction through `lower_program`, `Scheduler`, `ModuleLoweringInputs` and `InstancePlanner`. Resolve existing roots and protocol members by lookup or checked identity; a missing declaration is an error, not an interning request. |
| [Intrinsic interface bridges](../crates/kagari-compiler/src/source/lower/interface_bridges.rs) | Use selected ordinary impls where available. For checked intrinsic applications, represent the executable target without inventing a source `impl` and method definitions. Preserve intrinsic behavior, including tuple arity support. |
| Compiler-generated entries | Retain the checked closure-body origin; identify resume/initializer bodies and callable/protocol adapters as executable entries, not source functions. Keep a named local function's authored identity separate from its code placement. The exact executable key and storage are still open. |
| Executable publication and reload | Update MIR/KBC references, verification and retained-demand correspondence where they consume generated IDs. Keep generated data under compiler-owned construction and publish it frozen. A local function slot is meaningful only with its owning product; cross-scope references still require checked mapping. Do not fork a writer under the same table ID. |

Example: `static val X: i32 = compute()` already has a source ID for `X` and a checked
initializer body. Compilation adds `X -> initializer entry` to the executable product;
it need not add a source definition called `$static_init_X`. First access still runs the
initializer, with the existing generation and [reload rules](spec/module-activation.md#lazy-static-activation).

Rust reference (`5f4effd5795`): [definition collection](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_resolve/src/def_collector.rs#L432)
assigns closure identity before MIR; [codegen instances](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_middle/src/ty/instance.rs#L60)
identify shims separately. Borrow the identity split, not Rust's static initialization or a claim
that its definition storage is lock-free.

Acceptance: no writable HIR handle through checked outputs; repeated compilation of the same
checked input does not extend its source identity scope. Sample declared/intrinsic interface
calls, source-free verification, lazy statics and retained closures/Futures across reload.
No implementation authorized here; global lock removal and unrelated cache changes are out of scope.

## Single-run frontend plan

**Status: HC01–HC03 implemented; HC04 pending.** Make a standalone source compilation the baseline.
A new request computes source facts from its current inputs, not from a previous result.
Independent `kg build` processes cannot use the existing in-memory reuse paths; they have no
persistent HIR cache. Long-lived sessions will do more work after edits, and repeat requests
will check again unless the caller reads an already-held result. The
[retained-session baseline](performance-baseline.md#persistent-hir-baseline-windows-2026-11-07)
shows why this tradeoff must be measured separately from first compilation.

```text
Current source snapshot + library inputs + analysis policy
    -> parse and lower -> declarations/imports -> signatures -> bodies
    -> immutable facts and diagnostics -> checked admission -> compiler
```

The shorter `declarations`, `signatures` and selected `body` requests stop at their existing
boundaries and use the same stages. There is no `previous` input to source-stage computation
and no cache feature, fallback implementation or duplicate frontend. A caller can keep and
query an old immutable result; the next request neither modifies nor consults it.

### Implementation phases

| Phase | Remove | Keep and verify |
| --- | --- | --- |
| **HC01 — Check bodies fresh** | [`BodyReuse`](../crates/kagari-hir/src/typeck/reuse.rs), `TypeTable::restore_function`, `FileAnalysis::{can_retain, can_reuse_body}`, the full-file `Weak` index and the selected-body `BodyCacheEntry`/publication paths. Remove previous-result parameters, reuse counters and now-unused predicates in both full and selected routes. | `check_bodies_controlled` checks the requested bodies with current signatures and constants. Keep positive body eligibility, diagnostics, and selected-body queries that do not check unrelated bodies. Read old snapshots after an edit and compare current facts with a fresh database; check call targets/types, not pointer identity. |
| **HC02 — Check signatures fresh** | [`reuse_signatures`](../crates/kagari-hir/src/typeck/signature_reuse.rs), `previous_analysis` in `DeclaredAnalysis::check_signatures`, `SignatureCache{,Entry}`, `publish_signatures`, signature-reuse statistics and orphaned cross-revision token/namespace/type comparisons. | Build signatures from the current declarations and imported types, then complete aggregate contracts. Preserve signature diagnostics, generic and associated-type facts, and the shared `BodySignatures`/constant prerequisites used within the request. Check an imported signature edit that changes the consumer or its diagnostics. |
| **HC03 — Prepare source and declarations fresh** | [`DeclarationCache{,Entry}`](../crates/kagari-hir/src/analysis/records.rs), `publish_declarations`, old-result branches in `prepare_declarations`/`prepare_external`, `SourceParseCache` and `prepare_syntax`'s `published` callback. Remove `LoweringInputKey`/`LibrarySourceKey` fields used only for reuse; keep the parse-owned module locator. Remove now-unused cache invalidation and revision watermarks. | Parse each physical document once **per request** and retain that parse for all its inline views. Lower each logical module from that view and attach its current root sidecar before declaration checking. Keep original source extents, physical diagnostic ownership, external input validation, scoped/synthetic IDs and cancellation. Check inline recovery and sidecar changes, including old-snapshot queries. |
| **HC04 — Validate and measure the standalone route** | Delete dead cache-only APIs, assertions and benchmark counters left by HC01–HC03; do not add aliases. Replace tests concerned solely with removed hits with existing behavioral coverage where possible. | Exercise a fresh source-to-executable route, analyzer reads from a retained result and one source-free or update boundary. Update the HIR guide and benchmark descriptions to reflect the surviving routes. |

HC01 checks both full-file and selected bodies fresh. Focused local coverage samples selected
body isolation and prerequisites, edited signatures and call targets, cancellation, and retained
snapshot/binding ownership rather than cache hits. The declaration and signature reuse paths
were left for their own phases. HC02 removes exact signature hits and cross-revision remapping,
while keeping declaration publication after successful, cancellation-checked requests until HC03.
Focused coverage checks current imported signature facts and constraints, diagnostic provenance and
retained results. HC03 removes the remaining source parse/declaration reuse, including weak
Native lowering reuse (validated Native metadata and presentation parses remain reusable inputs).
Its local sample covers logical inline inputs and physical diagnostics, external sidecar and
source-view validation, import-solver failure and cancellation. The pre-change baseline uses `plain` and `dependency_edit` at size 96
(five timing runs) and a browser build of the minimal source compile/load workload; report the
matched comparison in HC04.

Keep request-local sharing, parser/checker limits, source and definition provenance, and
independent verification. `Arc` used to own a returned snapshot is not a cache. Keep existing
Native provider input preparation (`NativeFiles`), which retains validated registration inputs,
not old checked user graphs. Do not remove definition import/remapping used by Native input,
artifact or reload boundaries. The separate [immutable compiler handoff](#immutable-hir-to-compiler-handoff)
owns removal of the existing definition-writer escape; this plan does not change codegen or reload.

### Acceptance and performance gate

- No source-stage cache or previous-result parameter remains in the analysis pipeline. Checking
  edited inputs with or without a retained old result gives equivalent diagnostics and checked
  facts, with each result's own source locations. Cancelled work returns no partial result.
- Before changes, capture a matched baseline for the [HIR benchmark](frontend-benchmarks.md#hir-workloads-and-query-routes):
  fresh checked `plain` and `dependency_edit`, plus a representative retained/edited request to
  expose the tradeoff. After the cut, remeasure equivalent operations, not cache-hit counters.
  Also measure the actual fresh source-to-load workload from the
  [compile/load target](implementation-roadmap.md#fast-compilation-and-eager-startup).
  Keep toolchain, profile, workload and cache state explicit. No speedup is assumed.
- Run focused tests per phase, plus structure, formatting, workspace all-target Clippy and diff
  checks. Full-workspace tests and backend/platform matrices stay in CI.
- Treat any remaining first-compile gap as a profiling task on the simplified pipeline. The
  existing **500 ms compile/load target** is unchanged; do not use warm-session numbers to meet
  it. Defer disk caches, cross-edit semantic reuse, incremental parsing and a query framework
  until the cold target is met and there is a measured need. This cleanup is not a claim that
  performance already meets the target.

## Analysis flow and naming review

**Status: discussion recorded; revisit after the source walkthrough.** Final names, module moves and implementation phases remain open.

The main flow is hard to discover: declaration/signature/body orchestration is split between
`lib.rs`, `analysis.rs` and `analysis/{declarations,signatures,body}.rs`. Names also mix stage
results, retained facts and query views without consistently identifying those roles.

The conceptual full-analysis path is:

```text
Prepare structure     source AST / structured declarations -> LoweredModule per module
Build declarations    module namespaces + imports + declaration identities/conflicts
Analyze signatures    semantic types, generics, bounds and aggregate contracts
Analyze bodies        constant prerequisites, lexical resolution, types and selected operations
Admit compilation     checked dependency closure + matching facts/origins -> CheckedCompilation
```

HIR already contains declaration contents. "Build declarations" means establishing lookup and
semantic identity around those records, not constructing another Function/Struct definition.
Calls and field accesses are checked in body analysis; registering an impl does not validate its
trait obligations. This is a dependency outline, not a mandatory whole-input pass sequence:
declaration-only, signature-only and selected-body queries retain their existing depth boundaries.

### Direction to evaluate

- Put a short input/output/non-responsibility contract at each stage entry and link one reading
  path from the crate root. Distinguish per-module work from whole-input graph/catalog preparation.
- Prefer the existing stage owners under `analysis/`; move core orchestration out of `lib.rs`
  where responsibilities fit. Keep import solving in `imports`, name-resolution algorithms in
  `resolver`, checking algorithms in `typeck` and compilation admission in `program`/`compile_input`.
- Separate semantic stage work from database scheduling, cache reuse and snapshot retention.
  Full and selected-body routes should share applicable mechanisms without eagerly checking more.
- Review stage results, retained facts and file/query wrappers together before renaming; merge
  overlapping ownership where demonstrated, rather than add forwarding wrappers or aliases.

| Current structures | Review question |
| --- | --- |
| `DeclaredAnalysis`, `PreparedSignatures`, `AnalyzedModule` | Can names consistently identify the completed stage? `DeclarationAnalysis`, `SignatureAnalysis`, `ModuleAnalysis` are candidates only. |
| `DeclarationRecord`, `SignatureRecord` | Can names express retained semantic facts and distinguish them from graph-bearing stage views? |
| `FileDeclarations`, `FileSignatures`, `FileAnalysis` | Which wrappers own distinct source/query context, and which overlap internal stage results? Do not merge away real lifetime/retention boundaries. |
| `TypeTable`, `ModuleFacts`, `ModuleSignatures`, `BodySignatures` | Make types, selected operations, aggregate results and signature normalization stages discoverable without multiple ambiguous "facts" owners. |

Before implementation, agree on a stage input/output diagram, an existing-type retain/merge/rename
map and a file-ownership map. Preserve recoverable diagnostics, positive completion evidence,
source-free input, scoped identities, checked admission and existing cache/retention contracts.
This is not authorization to redesign storage, introduce a pipeline framework or change semantics.


### Language protocol shapes and member selection (deferred)

For `for x in xs`, two pieces of code must agree on what `next` means:

- When checking the `Iterator` **declaration**, HIR's [shape checker](../crates/kagari-hir/src/language/items/shapes.rs)
  looks up `next` by name and requires it to be the only method, with signature
  `next(self) -> Option<Self::Item>`. The rules for `Item`, the method signature and the
  method count appear in separate matches, making the complete protocol hard to read in one place.
- When [lowering the loop](../crates/kagari-compiler/src/source/lower/stmt.rs), the compiler
  asks for `protocol_method(Protocol::Iterator, 0)`. This [indexes the checked method list](../crates/kagari-compiler/src/source/lower/support.rs);
  it does not scan for `"next"` per `for` or per iteration. Slot `0` means `next` only because
  of the declaration rules above. The concern is keeping those rules and their consumer in sync,
  not the cost of a name search.

**Proposed direction (deferred): one contract per language protocol, one shared validator.**
Compiler code would declare what each language feature requires. The validator would compare that
contract with the actual `TraitSignature` from `.kgr` or structured input. The declaration being
checked does not supply its own expected shape. Each required member's spelling and signature
belong together in the contract; the comparison rules should not repeat names such as `"next"`.

The contract describes compiler requirements, not an exact copy of the standard library trait.
For example, `Iterator` requires `next(self) -> Option<Self::Item>` and forbids extra methods;
`Iterable` permits extra methods with default bodies. Adding such a helper to `Iterable` should
require no compiler change. Adding a language protocol expressible with existing rules should
require a contract but no new validation code. Extend the validator only for a new *kind of
requirement*. Ordinary user traits still use ordinary type checking, not compiler-owned contracts.

The contract representation is undecided. Whether lowering should refer to a named operation
instead of a numeric slot remains a separate question.

### Conditional blanket trait implementations

[Blanket trait impls](spec/traits.md#blanket-trait-implementations) now support ordinary
[library conversion forwarding](spec/builtins.md#conversion-protocols). This is separate from
the deferred design of the general language-protocol shape validator above.

### Other stable Rust `impl` differences (not conversion prerequisites)

These are separate design questions, not a Rust parity checklist. GC affects receiver and
lifetime semantics; it does not decide whether constants, async methods or opaque types are useful.

| Feature | Kagari today | Direction to evaluate |
| --- | --- | --- |
| Inherent associated constants | The [impl grammar](kagari.ebnf) admits associated constants in trait impls, not inherent impls. | A candidate for type-owned constants such as `Limits::MAX` that should not require inventing a trait. Start with existing const-safe values; this is not static mutable storage. |
| More general `where` predicates | Bounds on type parameters and their associated projections suffice for `U: From<T>`. | Defer predicates on composite types such as `where Pair<T>: Encode` until a library needs to require the capability of the pair itself rather than of `T`. |
| Const generic parameters | Impl binders are types, not values such as `const N: usize`. | Independent type-system feature, not excluded by GC. Revisit for a use case where the value must distinguish types, such as matrix dimensions. |
| Lifetime parameters, borrowed receivers and `unsafe impl` | Scripts use `self`; Rust host loans and their validation stay at the host boundary. | Do not import Rust's lifetime/aliasing model or let script impls assert unchecked host invariants. |
| Async trait methods | Inherent methods may be async; [general async trait methods](spec/syntax.md#async-syntax) are not supported. | Separate candidate. Specify how `async fn` in a trait relates to its `Future<T>` call result, including interface calls and retained futures across reload. |
| `impl Trait` in type position | Kagari has named generic parameters and interface values, not Rust's anonymous parameter or opaque return syntax. | A parameter `x: impl Read` is largely shorthand for a fresh `T: Read`. A return `-> impl Read` hides one concrete type chosen by the function; an interface value is not the same promise. Evaluate the two separately. |

## Generic substitution and trait resolution

**Status: architecture research and proposal; no implementation authorized.** This review is about
HIR semantic ownership, not Rust feature parity or a new generic-instance planner. Current behavior
remains owned by the [trait spec](spec/traits.md) and [HIR architecture](architecture/hir.md).

### Current flow and reading order

```text
Declaration checking -> binders, signatures, GenericBounds, impl contracts
                                      |
Call + expected type -> infer a substitution for this use
                                      |
                 instantiate signature and obligations
                             /                  \
             normalize projected types      prove trait obligations
                             \                  /
                      check operands and publish ResolvedCall
                                      |
                compiler: apply enclosing instance arguments and materialize
```

The middle is a dependency sketch, not a single pass. During inference, argument/result context
and known associated outputs can already require normalization. The final check still has to prove
obligations and compare operand types; inference success is not proof.

| Read in this order | What to look for |
| --- | --- |
| [Binder identity and substitution](../crates/kagari-hir/src/types.rs#L16), `TypeId::instantiate` | `TypeSubstitution` binds `(declaring owner, position)` and owner-qualified `Self` separately. One substitution is simultaneous: `(A, B)` with `A -> B, B -> i32` becomes `(B, i32)`. The inserted `B` is not revisited. No trait lookup. |
| [Constraint lowering](../crates/kagari-hir/src/typeck/constraints.rs#L146), [call inference](../crates/kagari-hir/src/typeck/body/calls.rs#L215), [body solving](../crates/kagari-hir/src/typeck/body/solving.rs#L339) | `GenericBounds` maps semantic subject types to applied trait requirements. Call inference gathers binder substitutions from explicit arguments, expected result and argument expressions. The body solver separately owns temporary `Inference` variables and defaults. |
| [`qualified_projection` and `normalize`](../crates/kagari-hir/src/typeck/associated.rs#L597), [`normalize_type`](../crates/kagari-hir/src/aggregates/implementations.rs#L472) | Syntax resolution identifies the associated member and can discharge a known equality early. Later normalization walks child types, then tries the qualifier's equality, the receiver's interface equality and a lookup callback. |
| [`proves_requirement`](../crates/kagari-hir/src/aggregates/implementations.rs#L817), [`implementation_matches`](../crates/kagari-hir/src/aggregates/implementations.rs#L994), [`projection_output`](../crates/kagari-hir/src/aggregates/implementations/projections.rs#L94) | Exact search walks applicable impls: bind receiver and trait inputs, prove substituted bounds, then compare requested associated outputs. This path carries caller assumptions, cancellation and shared search limits. |
| [Checked call facts](../crates/kagari-hir/src/typeck/table.rs#L87), [compiler instance planning](../crates/kagari-compiler/src/source/lower/instances/calls.rs) | `ResolvedCall` retains the selected declaration/member, applied signature and ordered type arguments. Those arguments can still name enclosing binders. The compiler applies a concrete instance and picks its execution form; it does not infer source arguments again. |

For `fn get<T: Family<Item = i32>>(x: T) -> <T as Family>::Item { x.item() }`,
`T` stays generic. The `where`/inline environment already proves the `Item` equality, so no concrete
impl is needed to determine the result. For `T: Family` alone, a checked signature may keep a
symbolic projection; a concrete use can later select an impl and reduce it. For
`<Wrap<i32> as Family>::Item<bool>` with `impl<T> Family for Wrap<T>` and `type Item<U> = (T, U)`,
matching the impl binds `T = i32`; applying the member binds `U = bool`; the output is `(i32, bool)`.
These are separate substitutions, neither of which proves the impl's bounds.

### Findings to verify before changing behavior

| Finding | Code evidence and consequence |
| --- | --- |
| Equality ownership is hard to trace | `qualified_projection` can replace its local qualifier with the matching caller bound and reduce immediately. Other routes carry equalities on `Projection.interface`, on the interface value, or in search assumptions. The same source expression does not necessarily reach all consumers in the same representation. |
| Normalization and proof have different impl-selection rules | `normalize_type`'s ordinary fallback calls [`match_implementation`](../crates/kagari-hir/src/typeck/table.rs#L830), which matches headers and compares substituted associated equalities but does not itself prove candidate impl bounds. `normalize_for_search` uses `explicit_applications` and proves those bounds. Thus two locally similar lookups have different guarantees; audit callers before classifying any divergence as a language bug. |
| Protocol logic overlaps general lookup | `normalize_type` tries primitive/iteration outputs before its interface and impl branches. [`iteration_outputs`](../crates/kagari-hir/src/language/semantics.rs#L410) can itself search parent interfaces and explicit impls, then reenter `normalize_type` with an `Iterator` projection. The interface branch can return `None` without reaching the ordinary impl scan. These are not four independent fallback providers; do not move branches purely for readability. |
| The two walks have different failure contracts | `associated::normalize` uses its own node/depth limits and an `Option` lookup. Exact search has a shared work budget, active-goal tracking and typed errors. Some `normalize_type` callback paths start fresh searches or convert errors to `None`. A symbolic generic projection, ambiguous evidence and an exhausted/cancelled pass must not become interchangeable at a checked boundary. |
| The applied call signature is calculated twice | In the ordinary call route, `check_function_arguments` and `record_callable_application` in [body calls](../crates/kagari-hir/src/typeck/body/calls.rs#L330) instantiate and normalize parameter types from the finalized substitution. Review sharing that result, not dropping operand/coercion validation. Compiler bounded substitution and source-free `Ty` are separate boundary work, not candidates for deleting checks. |

### Rust reference and the part to borrow

Reviewed the local rustc checkout at
[`5f4effd579500175af844241eb179e756190f1e7`](https://github.com/rust-lang/rust/commit/5f4effd579500175af844241eb179e756190f1e7).
[`EarlyBinder::instantiate`](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_type_ir/src/binder.rs#L652)
returns `Unnormalized`: substitution does not solve the resulting type.
[`AliasTy`](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_type_ir/src/ty/alias.rs#L21)
represents a projection by member identity and ordered `Self`, trait and member arguments.
[`ParamEnv`](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_middle/src/ty/mod.rs#L1018)
retains caller clauses separately; a trait object can still own its own associated equalities.
The [next solver](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_next_trait_solver/src/solve/normalizes_to.rs)
solves `NormalizesTo` under that environment. It matches an assumption or applicable impl,
proves obligations and instantiates the output. The [old solver](https://github.com/rust-lang/rust/blob/5f4effd579500175af844241eb179e756190f1e7/compiler/rustc_trait_selection/src/traits/project.rs)
also separates candidate assembly and confirmation; its candidate precedence is not the next
solver's contract. Borrow the separation of substitution, question, assumptions and proof.
Do not import Rust lifetime universes, specialization, canonical query caches or cycle semantics.

### Proposed Kagari architecture

```text
Type checking / compiler specialization
    |-- infer arguments when needed (HIR only)
    |-- instantiate known binders and Self (pure transform)
    `-- request a trait proof or normalize a type
         |
         +-- normalize: walk child types; ask to solve each projection; replace proven results
         `-- trait resolution: one implementation-applicability path
              reads caller evidence + interface view + language rules + impl catalog
              owns search limits, cancellation and active goals
```

Suggested internal data boundaries, not final Rust APIs:

```text
ProjectionKey { receiver, trait_def, trait_args, member, member_args }
ProjectionRequest { key: ProjectionKey, required_equalities }
ResolutionContext { definitions, caller_bounds, catalog, cancel, limits }
SearchState { active_goals, remaining_work }
ProjectionAnswer = Known(TypeId) | Deferred
// No solution, conflicting/ambiguous evidence, cancellation and resource failure
// are distinct outcomes; callers decide which to diagnose at their boundary.
```

- **Keep `TypeSubstitution` pure.** `resolve` or `normalize` should never be a hidden side effect
  of `instantiate`. Keep the HIR body's inference workspace separate, and preserve bounded
  substitution during compiler materialization.
- **Separate requirements from evidence.** The qualifier may require an explicit equality;
  it cannot prove itself. A caller bound is scoped evidence, not a property of the projection
  key. Interface values keep their own equalities and inherited interface surface. The initial
  change can derive the request from the existing `Projection`; only consider changing `TypeId`
  after auditing every retained projection and its environment across signatures, snapshots and
  compiler specialization. Keep member identity and full applied trait arguments in lookup.
- **Reuse exact applicability**, rather than routing proof through the current best-effort
  fallback or building a new solver. Match inputs, prove substituted bounds, then compute
  the associated result (including GAT arguments). Check required output equalities against
  results; never infer impl binders from the requested associated output. Source recovery may
  preserve a deferred type; executable use must not treat a deferred answer as proof.
- **Give protocol rules a named owner.** Primitive outputs and derived `Iterator -> Iterable`
  supply explicit evidence under the same search state. Preserve the distinction between a
  concrete `Iter` output and the erased interface view; settle precedence against explicit
  impls and inherited outputs with tests, not branch position. No new general protocol-shape
  framework is implied by this review.
- **Preserve the checked handoff.** The compiler consumes checked calls and specializes them
  under retained assumptions; it does not repeat name lookup or body inference. The independent
  [source-free proof path](../crates/kagari-contract/src/types/proofs/normalize.rs) continues
  to validate contracts without HIR. Compare behavior at that boundary rather than introduce
  an executable-to-frontend dependency. No performance claim or new cache is proposed.

### Review and migration order

1. **Classify callers before editing.** Mark inference hints/recovery, signature normalization,
   exact bound proof and concrete specialization. Trace a conditional impl whose header matches
   but whose bound fails, an inherited interface equality, and explicit versus derived iteration.
   Compare both HIR routes on the same inputs; a source bug requires a witness at its owner.
2. **Consolidate lookup rules before changing storage.** Keep the existing representation, pass
   the current environment through normalization, and give proof and normalization a shared
   candidate/applicability path and search state. Preserve existing bounds, cycles, work limits
   and cancellation. Reuse one finalized applied signature where facts and validation agree.
3. **Review representation after the lookup change.** Only remove copied environment equalities
   from projections when generic signatures, inheritance and specialization can recover the same
   evidence without redoing source analysis. Leave wider `Ty`/ABI or language changes out.

Prefer extending existing [generic inference](../crates/kagari-hir/src/analysis/tests/generic_type/context.rs),
[GAT](../crates/kagari-hir/src/analysis/tests/gat.rs), [trait proof](../crates/kagari-hir/src/tests/typeck/traits.rs)
and [compiler handoff](../crates/kagari-compiler/src/source/elaborate/tests.rs) scenarios. Verify exact
result/target or error kind for conditional bounds, receiver and trait-argument distinctions, inherited
outputs, GAT inputs, explicit/derived iteration, ambiguous/cyclic/limited search and cancellation;
include one source-free contract regression. This is a proposed validation set, not test results.

## LoweredModule boundary remediation

[Native input implementation](architecture/hir.md#native-declaration-inputs-and-origins) supports
direct lowering and parsed inputs with common binding-free HIR and checking regardless of
LSP/execution intent. A–F were implemented in `4e6cc2d3a`. `LoweredModule` owns the source and module, and
`Module.nodes` holds `HirNodes`. SO02 moves the remaining origins onto the records. Common import scopes and actual
declarations own static facts, not registration attachment maps. Native implementations are matched
at linking, outside HIR. The [boundary contracts](native-model-design.md#9-frontend-and-linking-boundary-remediation)
describe these owners and preserved validations, not pending migration. Broader provider/prebuild
products stay deferred.

## Deferred reverse lookup and remapping scans

HIR IDs select records and their origins directly. A general span-to-node query would return
candidates, since distinct records can cover the same range. Existing editor indexes are
derived by the analyzer from the matching source and semantic facts.

Body reuse knows the target function. Each function that reaches remapping still scans five
module-wide node collections in each lowering (expressions, locals, places, patterns and type
references). It filters on the matching owner and function extent, then pairs entries in slot
order using relative ranges. Signature reuse scans the old and new type-reference collections
once per attempt and pairs ordered IDs by owner and projected range. Both preserve repeated
occurrences. See [body remapping](../crates/kagari-hir/src/typeck/table.rs) and
[signature remapping](../crates/kagari-hir/src/typeck/signature_reuse.rs).

General reverse lookup and scan reduction remain deferred after SO00–SO03. A later body-reuse
review can evaluate enumeration from the known function to its ordered node IDs. Evaluate the
cost on a representative edit workload and preserve owner, relative-range and repeated-occurrence
checks.

## HIR visibility follow-up

Origin validation follows the IDs carried by each record, not the owning module's input
kind. This is scoped identity validation; it does not define a new mixed-origin policy.
`typeck::reuse` and `BodyReuse` are crate-visible; `environment_matches` and `restore`
are restricted to `typeck`. The remaining visibility candidates are separate from the origin fix:

- **Internal helpers:** `CheckedAnalysis::to_unverified` has only HIR admission test callers; keep `FileAnalysis::to_unverified` separate because analyzer/compiler
  tests use it. `ModuleSignatures::type_bounds` and `statics` have only HIR callers.
- **ID construction:** make plain local HIR ID constructors crate-visible after moving
  the analyzer test from `StructId::new(0)` to the ID of the actual lowered declaration.
  Keep public ID types and accessors. Plain IDs still require the matching module.
- **Coupled storage:** keep `LoweredModule` source, input kind and module together. Give
  consumers shared access to `Module` declaration slices and node lookup, not writable
  vectors or independent replacement of `HirNodes`. Keep record origins/name ranges,
  IDs, binder lists and body/initializer roots writable only on the construction side.
  Check `DeclarationHeaders` and its binder access when migrating these readers.
- **Published analysis:** `AnalyzedModule.signatures` already has external read routes
  through file-stage APIs. Other facts have actual external readers; preserve those
  reads through the owning views before restricting fields. Keep checked admission
  and owner-scoped body access intact. Public read-only enums and ID accessors do not
  require a replacement model.

Confirm the changes against compiler, analyzer, lint, SDK examples and benchmarks;
workspace-only caller evidence does not establish downstream compatibility. Do not add
mutable accessors or another HIR model just to preserve field access.

## Native model

Phases 1–3 remove the registration-dependent frontend split, dedicated default recipes and mixed
HIR type/storage classification. Implementation is not all-green acceptance: the
[roadmap](implementation-roadmap.md#native-model-replacement) retains known baseline failures and
local/CI limits. The [boundary contracts](native-model-design.md#9-frontend-and-linking-boundary-remediation)
record implemented ownership alongside explicitly deferred whole-stack discussion, not a second queue.

- Ordinary HIR declares Required/Script/Native functions, fields, methods/associated functions
  and trait impls. Rust implementation code stays outside HIR; no general trait receiver loans.
- Compilation consumes structured Rust declarations directly; analyzer parses exported .kgr into
  the same unchecked input and uses common analysis with user source and exact navigation origins.
- Native field names/types, var/val and ordinary child-object handles are frontend requirements.
  New Rust field adapters/execution are deferred after scope narrowing, not implied by type checks.
- 9G removes script reference wrappers. Neither same-named user types nor ordinary
  Native declarations gain borrowing, projection or forwarding. Independent Host
  path and Opaque loan contracts remain.
- Ordinary Script definitions/calls replace dedicated NativeDefault recipes; language constructor
  interpretation is separate from Native declarations and execution storage. Binding checks belong
  at linking, not HIR. Existing defaults, Host safety and executable validation remain.
  No new prebuild producer or generalized registration framework was required.
- The design retains future library/provider/build/cache and directory-refresh discussion as
  explicitly deferred direction, not current acceptance.

[Replacement boundaries](native-model-design.md#7-replacement-boundaries-and-acceptance) and
[review limits](native-model-design.md#8-review-decisions-and-limits) preserve existing safety
while preventing a HIR cleanup from silently becoming a whole Runtime migration.
