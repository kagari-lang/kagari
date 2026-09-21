# Foundation Refactor

This track supersedes conflicting milestone behavior in implementation-roadmap.md.
It is a breaking replacement: no old API facade, dual semantic path, artifact
upgrade, or legacy interpreter. Runtime ABI/schema/authority checks remain required.

## Checkpoints

Each checkpoint requires implementation, focused verification, updated examples
and specifications, and a Conventional Commit with `Roadmap-Step: Rxx`.
Unchecked entries are not implemented claims. Full LSP, async, advanced GC,
complete replay, automatic state migration, and advanced JIT are later tracks.

- [x] R01: Authoritative value, failure, and activation contracts.
- [ ] R02: Source/result/diagnostic/host-effect conformance harness.
- [x] R03: Unified source database, revisions, identities, overlays, coordinates.
- [ ] R04: Recoverable HIR analysis; checked-only code generation.
- [x] R05: Immutable queries, cancellation, parse/body reuse and invalidation.
- [ ] R06: Offline host declarations and checked runtime bindings.
- [ ] R07: Nominal concrete identity, layouts, bounded reachable monomorphization.
- [ ] R08: Verified IR and linked-only runtime operands.
- [ ] R09: Canonical bounded artifact format and explicit fingerprint algorithm.
- [ ] R10: Shared immutable generations and runtime-local state.
- [ ] R11: Value semantics, owned handles/roots, nonmoving mark-sweep baseline.
- [x] R12: Execution sessions, synchronous host reentry, shared cleanup/budgets.
- [ ] R13: Failure-atomic standard mutation and dirty-record commit.
- [ ] R14: Acyclic initialization and isolated prepare/initialize/publish.
- [ ] R15: Compile-time capability and resource limits.
- [ ] R16: Injectable deterministic context and host trace fixtures.
- [ ] R17: Interpreter/JIT/debugger contract equivalence.
- [ ] R18: Obsolete-path audit, full validation, reproducible resource baselines.

## Validation

Focused tests accompany each semantic change. Final gates are `cargo fmt --all
-- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test
--workspace`, and `git diff --check`. Performance evidence records toolchain,
workload, repetitions and measurements; no unmeasured performance claims.

## Current implementation status

R03 acceptance evidence:

- [Source database tests](../crates/kagari-common/src/source_database.rs) cover
  logical module bindings, captured project roots, virtual URIs, overlay precedence,
  immutable revisions and checked Chinese/emoji/CRLF position conversions.
- [Source snapshot integration](../crates/kagari-embed/tests/source_snapshots.rs)
  covers compilation/tooling sharing, module rebinding, diagnostic provenance and
  source/artifact identity. Disk files and host text enter the same database.
- [Declaration navigation](../crates/kagari-hir/src/analysis/identity_tests.rs)
  covers same-named modules, declaration kinds/owners, generic owner/position and
  analysis-owned bindings. [Member tests](../crates/kagari-hir/src/analysis/member_tests.rs)
  cover field/variant arena ownership, exact declaration ranges, stable nominal
  identity after reordering, old snapshots and retained duplicate declarations.
- [Arena tests](../crates/kagari-hir/src/analysis/arena_tests.rs) and
  [body-owner tests](../crates/kagari-hir/src/analysis/owner_tests.rs) cover foreign
  local IDs, interleaved cache reconstruction, explicit function/constant ownership,
  synthetic nodes and rejection of cross-body edges. Declaration/signature/body
  query tests prevent old revisions from replacing newer cached results.
- The source_queries example exposes declaration-site member navigation alongside
  independent declaration, signature and function queries.

This completes source identity and query provenance. Applied generic types and
remaining executable layouts retain R04/R07 acceptance;
R03 does not imply those execution features are complete.

R05 acceptance evidence:

- [Declaration queries](../crates/kagari-hir/src/analysis/declaration_queries/tests.rs)
  cover standalone discovery, recovery, shared unchanged file results, dependency
  invalidation and rejection of stale/cancelled cache publication.
- [Signature queries](../crates/kagari-hir/src/analysis/signature_queries/tests.rs)
  cover checking without body analysis, independent reuse, rebased error locations,
  immutable lowering sharing and consumption by full analysis.
- [Function queries](../crates/kagari-hir/src/analysis/body_queries/tests.rs) cover
  selecting one body, absence of neighbor bindings/facts, incomplete member receiver
  types, exact-cache sharing, unchanged-body remapping, fresh local identities,
  same-named impl and cross-module identities, dependency signature invalidation,
  deletion and stale/cancelled publication. Results match fresh analysis.
- [Snapshot integration](../crates/kagari-embed/tests/source_snapshots.rs) compares
  artifacts after cache reuse with fresh compilation and checks source/profile
  invalidation. Existing analysis/identity/import tests cover queries on erroneous
  files, dependency facades and old snapshot navigation; source_queries exercises
  declaration, signature, one-body and full queries through the embedding API.

Full analysis batches bodies through the same resolver/type checker used by the
single-function query. Module constants remain shared semantic prerequisites and
are checked when querying a body. This is bounded query caching, not a complete
incremental dependency framework. R04 recovery coverage,
R15 resource limits and R18 performance measurements retain separate acceptance.

R12 acceptance evidence:

- [Execution sessions](../crates/kagari-runtime/tests/execution_sessions.rs) cover
  immutable root permissions, per-root budgets/peaks, cancellation, inherited
  scopes and root-version retention; independent calls start fresh budgets.
- [Execution frames](../crates/kagari-runtime/tests/execution_frames.rs) cover one
  session-owned stack, suffix cleanup, GC roots and cleanup after quarantine.
  Module instance borrows are confined to individual load/store/init operations;
  the existing explicit executor remains the single script driver.
- [Host scopes](../crates/kagari-runtime/tests/host_scopes.rs) cover registered
  temporary roots/leases, retained root lifetime, ordinary error/termination/fault
  cleanup, foreign and expired tokens, and declared borrow conflicts.
- [VM session fixtures](../crates/kagari-vm/src/tests/sessions.rs) cover synchronous
  reentry, GC-safe returned objects, nested cancellation/budget cleanup and pinned
  epochs through direct/encoded interpreter and existing JIT fallback routes.
  [Path fixtures](../crates/kagari-vm/src/tests/helpers.rs) also reenter during path
  validation/read/preparation, while commit reentry still quarantines the runtime.
- The host_reentry embedding example exercises rooted results, temporary scopes
  and complete-stack observation. No async API or cross-thread execution was added.

This completes R12, not R10/R11/R13/R17: interface/capture ownership,
the wider engine-invariant audit, lexical debug visibility and remaining backend
contracts retain their own acceptance requirements.

R01 specifies target behavior. Remaining runtime behavior must not be described
as conforming until its corresponding checkpoint and regression tests pass.

Implemented foundation slices:

- R02: one source/artifact/JIT fixture format now checks values, structured
  diagnostic codes, index traps, host failures, ordered host calls, committed
  host mutation records and final host state. It covers left-to-right evaluation,
  alias/identity/tuple behavior, short circuit, effects surviving traps, rejected
  host mutations and cached initialization failure. Run with `cargo test -p
  kagari-vm language_contract`. The remaining value/activation contracts still
  need implementation and fixtures before R02 can be checked off.
  Enum payload equality, distinct mutable enum members, shallow container copies,
  and rejection of interface equality now run through these same routes.
  Integer arithmetic traps, retained pre-trap host effects and budget precedence
  now use these routes too; selected fixtures require actual native invocation.
  Plain and compound assignment fixtures cover computed roots, single index
  evaluation, RHS replacement/removal/creation of a target, current-slot reads,
  root identity retention, tuple copy updates and arithmetic failure.
- R03: source IDs/revisions, immutable snapshots, base/overlay precedence and
  checked UTF-8/UTF-16/CRLF coordinates. The engine now compiles and queries the
  same snapshots; disk loading, host text and overlays use one ingestion path.
  Relative paths resolve against a captured root; virtual URI schemes survive
  normalization. Embedding diagnostics carry file/revision ranges. Cross-module
  aggregate/member declarations share the same identity model.
  Logical package/module bindings now belong to source documents and survive
  overlays. Rebinding invalidates analysis; duplicate source bindings are rejected.
  Source-based HIR carries its origin through IR, bytecode and artifact metadata.
  The AST-only analysis entry and post-analysis identity overrides were removed.
  Declaration paths now include module, kind, owner and duplicate occurrence.
  Parameter/local identities include their body and analysis instance; stale local
  IDs cannot resolve in a new analysis. Definition navigation returns file/revision
  ranges and distinguishes same-spelled declarations in separate modules.
  Raw expression/block/statement/place/pattern/local/parameter/type-reference IDs
  now include the identity of their immutable lowering arena. Bare-index constructors
  were removed; semantic lookups reject foreign IDs and node/source-map access checks
  arena ownership before indexing. Body/signature reuse remaps IDs into the current
  arena. Equal source revisions may still require remapping when independently
  reconstructed lowerings meet caches at different query stages. Cross-lowering
  collisions and that interleaved-query case have regression coverage. Local IDs
  also carry an explicit HirOwner allocated during lowering: function, constant,
  or shared declaration context. Module initializer identity is allocated before
  its nodes, so interleaved declarations and synthetic spans cannot misassign it.
  Resolver scopes share the same BodyOwner and reject cross-body edges/bindings;
  nodes and source maps verify stored owners. Shared impl receiver types keep their
  declaration owner. Storage remains in immutable module arenas, with each body's
  ownership explicit in its IDs. Field and variant IDs also carry arena, owner and
  slot; signature reuse remaps field keys. Enum variants have nominal owner/name
  paths and exact source-map entries, including separate duplicate occurrences.
  Declaration-site member queries work before body analysis; duplicate variants
  remain queryable with diagnostics and cannot pass code generation.
  Generic parameters now have owner/position identities and declaration ranges;
  inherited trait/impl parameters keep their original owner in method signatures.
  Snapshots now resolve standard, offline host and registered source imports through
  one import fact table. Package-qualified source namespaces and public items retain
  file/revision identity; conflicting namespaces and private imports are diagnosed.
  Cross-file definition queries follow public source facades and keep old snapshot
  locations. The source_modules embedding example prints a dependency-first graph.
- R04: analysis retains facts and diagnostics; unknown/missing expressions are
  represented explicitly, and codegen requires a sealed CheckedAnalysis. More
  semantic-target and source-owner integration remains outstanding.
  Host calls with missing or extra arguments preserve their declared return type
  and downstream member facts. Available arguments still receive type checking;
  arity/type diagnostics reject code generation. Free-function and method recovery
  tests retain neighboring function queries and known host field declarations.
  Failed generic-call inference substitutes error arguments into the return type
  as well as the recorded instantiation. Callee binders cannot leak into callers;
  known arguments, caller-owned binders and nominal member identities survive.
  Inference traverses partially erroneous composite arguments to use their valid
  members. Partial whole-type candidates merge complementary facts from later
  arguments through structural recovery, while conflicts remain diagnosable.
  Recovery now fills independent member holes even when another member conflicts;
  established facts and conflict diagnostics remain intact. Different nominal owners,
  kinds and arities cannot contribute recovery facts across their shape boundary.
  Member recovery and conflict comparison also use explicit stacks; a 10,000-level
  regression checks hole filling, compatible comparison, conflicting leaves and
  preservation of established facts without recursive member traversal.
  Tuple and nominal-constructor regressions cover
  this recovery. Annotation resolution now returns recoverable types directly,
  preserving composite shapes containing unknown members. Parameters, returns,
  fields, enum payloads, local annotations and constants retain facts and report
  unresolved-type diagnostics; a 24-case matrix verifies queries and codegen
  rejection. The old optional whole-annotation result is removed.
  Index analysis uses the known outer composite shape even if an unrelated member
  is erroneous. Tuple and nested-array queries retain selected member types;
  out-of-range and noninteger tuple indexes still diagnose and reject codegen.
  Type-conflict checks now compare known composite structure recursively. Recovery
  members suppress only their own dependent mismatch; unrelated member conflicts,
  nominal identities and arity differences remain diagnosable.
  If/match branches and array elements merge complementary recovery facts through
  the same structural operation. Original diagnostics continue to reject codegen;
  subsequent real conflicts remain errors and cannot overwrite established facts.
  All local module-level declarations and import aliases now share an immutable
  name table. Calls, annotations, constructors, bounds and impl headers consume
  it; duplicate names retain identities but have no winning target or codegen.
  Regressions cover all 36 ordered declaration-kind combinations, unaffected
  neighbors and invalidation when a collision is introduced and removed. Duplicate
  imports revoke every target, and failed imports block fallback. Qualified source,
  host and standard-library calls respect lexical root shadowing; the checker no
  longer reconstructs standard-library targets from expression strings. Runtime
  helpers now have explicit prelude resolver targets; the checker's remaining
  helper string fallback was removed. Arity errors retain those targets, local
  declarations/bindings shadow every helper, and body cache reuse rebases calls.
  Bare function/module/type items now fail with a structured HIR diagnostic rather
  than giving a bare function its return type and reaching an IR rejection.
  Source/artifact/JIT fixtures exercise helper effects, shadowed print and those
  diagnostics. Impl trait headers now retain the same typed references as bounds,
  including generic arguments and exact source spans; validation and ABI lowering
  consume the checked targets. Applied traits cannot silently erase arguments or
  register an implementation, and generic binders shadow trait/standard-constraint
  names consistently with annotations. Imported trait targets retain nominal
  identity/navigation through facades while their execution remains explicitly
  unsupported. Regressions cover recovery, signature-cache rebasing and cross-module
  targets. Trait constraints, implementation keys and trait-method call targets now
  carry nominal declaration IDs instead of local trait/method numbers; query and
  monomorphization consumers use those identities. Regressions reject foreign
  trait/method IDs even with matching local slots, preserve targets across declaration
  reordering and rebase cached receivers. Source/artifact/JIT fixtures distinguish
  two same-named trait methods implemented by one receiver type. Imported trait
  execution and other semantic contracts retain the R04 audit. The shared aggregate
  catalog now owns checked trait/method contracts, including nominal parameters,
  bounds, Self types and source targets. Method calls and navigation consume this
  catalog for local and imported interface annotations; local HIR method lookup is
  removed. Ambiguous bound methods and duplicate method declarations reject codegen.
  Tests cover facade imports, same-named methods across modules, signature errors,
  unchanged caller-body reuse and dependency signature invalidation. Dependency
  source revisions still conservatively invalidate consumers, even for body-only
  dependency edits; this does not claim dependency-level incremental reuse.
  Checked function signatures now own generic constraint maps. Parameter/return
  validation, body environments, calls and method catalogs consume these maps;
  repeated downstream HIR constraint reconstruction is removed. Signature-query
  tests cover inline/where impl bounds under method shadowing, partial constraint
  errors, body-edit reuse and bound-change invalidation. The source_queries example
  exposes constraints before body analysis; execution fixtures forward checked
  where bounds through source, artifact and existing JIT fallback routes.
  Type applications now resolve their base through the same declaration/binder
  lookup as named annotations. Explicit bindings block standard constructor
  fallback, and empty argument lists are retained instead of becoming bare types.
  Invalid applications retain base and argument targets; navigation selects the
  innermost annotation even when its target is unknown. Regressions cover all four
  standard constructors, facades, partial arguments, signature rebasing and repair.
  Source struct/enum applications now retain declaration-owned binders and check
  arity and bounds. Constructors infer arguments from fields/payloads; field access
  substitutes the receiver's arguments. Signature reuse preserves binder identity.
  Semantic Struct/Enum/Trait identities now carry a NominalType with declaration
  identity and ordered arguments. Substitution, inference, concreteness, recovery
  and display recurse through those arguments. ABI types preserve the same shape;
  format 22/runtime ABI v23 reject older products. Layout verification rejects
  applied nominal payloads until their concrete layout exists, rather than binding
  them to zero-argument declarations. Reachable struct/enum layouts are now emitted
  per concrete instance and deduplicated. Layouts and function instances share the
  program-wide instantiation budget; recursively growing layouts terminate with a
  structured diagnostic. Public generic enum ABI templates validate against each
  local/imported instance, including when the owner emits no instance. Tests cover
  facades, distinct field representations, bounds, malformed binders, source/artifact/
  JIT fallback behavior and the standard-library example. Backwards generic-call
  constraints, generic impl specialization
  and interface tables remain pending. The shared argument-inference entry for
  calls and enum payloads is now used by
  concrete and trait parameter contexts too, with cancellation between operands.
  Constraint matching now uses an explicit work stack and checks analysis cancellation
  between members, preserving left-to-right inference precedence. A 10,000-level
  synthetic composite test covers traversal without recursive stack growth; type
  cloning and other operations retain their separate depth-limit work.
  Constraint inference returns explicit cancellation; return-context, constructor
  field and generic-call consumers stop before deriving missing-argument diagnostics
  or instantiations from a cancelled argument batch.
  Parameter contexts retain known composite members beside uninferred callee
  binders, which enter the context as Unknown holes rather than escaping into
  caller facts. Constructor/call finalization diagnoses remaining Unknown holes;
  Error members retain their existing diagnostics. Source/artifact/JIT fixtures
  exercise independently contextual enum and struct members in tuple arguments,
  while unseeded constructors and containers remain rejected.
  Struct fields now use that same context operation as call and enum payload
  arguments, preserving independent tuple members even before the enclosing
  binder is inferred. Context reconstruction reads substitutions directly instead
  of cloning the substitution map per argument. Struct/enum execution fixtures
  and negative constructor cases cover the unified path.
  Failed argument finalization is shared by calls and constructors: it reports
  uninferred holes and converts only those leaves to Error, preserving known
  tuple/container members and nominal identities for tooling. Regression cases
  inspect retained local-reference facts and reject codegen in all three paths.
  Finalization now also updates the shared substitution before constructor member
  validation. Result facts and field/payload diagnostics use identical recovery
  types; known sibling conflicts still diagnose after missing positions become
  Error. The former function-only substitution update is removed.
  Profile-gated set_field/set_index helpers now pass checked target types to their
  RHS expressions and compare known members using the same conflict predicate
  as assignment. Missing members suppress dependent mismatches without hiding
  independent conflicts; analysis and source/artifact/JIT fixtures cover both.
  Reflection field helpers reject nonexistent members in HIR instead of inventing
  Unit results; reflective indexing rejects known invalid receiver/index pairs.
  Operand recovery errors suppress dependent diagnostics. Focused tests ensure
  invalid helpers cannot obtain checked codegen input and unknown operands emit
  only their original name diagnostic.
  Reflective field writes now check declared field writeability in HIR, matching
  the runtime layout guard. Read-only fields still supply RHS type context and
  preserve independent type mismatch diagnostics; reads remain permitted.
  Reflection field names now have an explicit HIR constant-String check. Invalid
  names retain their helper call target and Error result instead of falling through
  to ordinary function resolution. Name-type, nonconstant-name and RHS errors keep
  distinct diagnostics; existing name errors do not gain cascading call errors.
  Resolved reflection field-name expressions retain declaration-owned field
  targets in the semantic table. Definition queries reuse the normal field-fact
  path, including invalid writes; same-spelled fields remain distinct. Tests
  verify navigation after body reuse and Unicode/CRLF shifts and retain old
  snapshot locations without changing the field-name expression's String type.
  Ordinary and reflection field reads now share member resolution, target fact
  recording and missing-member recovery. A known Struct with erroneous generic
  arguments still diagnoses absent fields, while valid fields keep their types
  and declaration targets. A wholly unknown receiver suppresses dependent errors;
  incomplete dot access retains its explicit missing-name diagnostic.
  Ordinary reads and reflective index writes share checked index diagnostics.
  Partially erroneous composite indices still diagnose their known noninteger
  shape; only whole Unknown/Error indices suppress dependent diagnostics.
  Positive recovery cases and independent invalid-index errors cover both paths.
  Standard-library argument checks now use the same known-member conflict
  predicate as script calls and assignments. Whole erroneous operands and missing
  operands no longer duplicate primary expression/arity diagnostics. Array method
  and qualified calls plus String functions cover recovery and independent
  mismatches; partially erroneous non-container operands still diagnose.
  Standard container arguments now receive element/key/value context after the
  receiver is checked in source order. Method and qualified calls share this
  path, including Set algebra and Option/Result unwrap_or fallback context.
  Constructor positive/negative cases and source/artifact/JIT execution verify
  Array and Map mutation plus Option fallback; the standard-library example
  includes a context-inferred generic element constructor.
  Binary recovery preserves bool results for comparison/logical operators and
  checks independent known operand conflicts rather than discarding all facts
  when any member is erroneous. Equality checks known Comparable members on both
  sides; numeric and logical operations reject known incompatible shapes even
  beside an unknown operand. Erroneous expressions still cannot pass codegen.
  It still propagates constraints forward; backwards contextual inference remains
  an outstanding solver change. Independent signature queries construct
  the shared aggregate catalog and check applied bounds in signatures. Full/body
  analysis consumes that result instead of repeating header validation. Tests cover
  facade dependency changes, repair, body-edit diagnostic rebasing, unchanged query
  sharing and a correct function beside an invalid applied signature. The
  source_queries embedding example demonstrates the separate diagnostic boundaries.
  Enum payload annotations now survive lowering as declaration-owned type references.
  Signature checking retains every member, including missing/unknown Error facts,
  and exposes types before body analysis. Nominal enum/variant payload contracts
  enter the shared aggregate catalog for reachable dependencies; body-only edits
  rebase their references and payload contract edits invalidate dependent body reuse.
  Public variant ABI records encode checked structural/nominal payload types in
  format 11, so changing payload types rejects reload before publication. HIR and
  embedding regressions cover recovery, cache reuse, cross-module identity,
  encoding round trips and unchanged runtime entry after ABI rejection.
  Qualified constructor paths now retain resolved owner facts and nominal enum/
  variant targets. Payload arity/type errors preserve target navigation and result
  types; missing variants retain their enum owner while arguments remain checked.
  Source imports/facades, local shadowing and independent cached body queries share
  those facts. IR now emits nominal enum layouts and constructor operands; bytecode
  links them to checked enum/variant slots. Layout validators check ownership,
  payload references, counts and representations, and reject cross-module conflicts.
  Runtime variants retain their executable generation; allocation validates runtime,
  payload and schema before charging resources. Arbitrary string enum allocation and
  Option/Result string dispatch were removed in favor of typed tags. Value equality,
  mutable-member identity and evaluation order share source/artifact/JIT-fallback
  conformance fixtures; embedding tests cover foreign runtimes, changed nested
  schemas, rooted old-version survival, same-named standard types and dependencies.
  Format 22/runtime ABI v23 reject older products. Remaining interface/layout
  contracts retain R07/R08 acceptance.
  Erroneous signatures retain parameter slots with Error types. Invalid local
  annotations, call targets and indices produce diagnostics; unresolved operands
  suppress dependent mismatch diagnostics. Empty Map/Set constructors retain
  concrete parameters inferred from binding annotations.
  Scalar const values are now HIR facts. Arithmetic failures and annotation
  mismatches reject code generation while retaining unrelated facts. IR no longer
  evaluates const syntax or rebuilds legacy aggregate const objects.
  Literal expressions and match patterns now carry checked scalar facts, including
  the i32 minimum spelling. Invalid literal ranges and pattern type mismatches
  are source diagnostics. IR consumes these facts without reparsing literal text.
  Name resolution now owns lexical scopes, binding introduction points and match
  arm bindings. Tool scope queries consume these facts instead of reconstructing
  scopes from HIR blocks. Navigation uses resolved expression and assignment names;
  cross-module definitions also use the shared import graph. Type references retain checked types and
  declaration targets in signatures, field/const/local annotations and impl headers.
  Unknown composite members do not discard later members. Type navigation and
  interface permission checks use these facts; IR no longer formats impl type syntax.
  Semantic user types now carry declaration identities, and generic TypeId values
  use owner/position identity; diagnostic names do not affect equality or hashing.
  Generic constraints now resolve once to standard or trait targets. `where`
  targets must name generic parameters; impl constraints are inherited by methods
  without leaking into shadowing parameters or sibling method scopes. Diagnostics
  use individual bound ranges and inherited invalid references report once.
  Navigation, generic checks, trait method selection and bound ABI metadata consume
  these facts. Applied trait constraints reject codegen rather than discarding
  arguments; concrete instantiation remains pending R07.
  Struct fields have owner/slot HIR identities and module-owned declaration paths.
  Field types, read/write targets and initializer targets are retained as HIR facts;
  IR field operations and field ABI metadata consume them. Invalid field types retain
  navigation targets without unknown-member cascades; duplicate fields reject codegen.
  Resolved field and struct-initializer targets now use nominal declaration IDs
  instead of file-local struct/field IDs. A checked aggregate catalog supplies field
  types, declaration order, writeability and source locations for local and imported
  accesses. Nested imported struct construction/read/write uses the same checks;
  IR consumes the catalog instead of looking fields up in the current HIR module.
  IR now carries nominal struct layouts and owner/slot operands for construction,
  reads and writes. Its verifier checks layout/field identities, slot completeness,
  representations and write permissions before bytecode emission. The old IR field
  name operands and string-based field interning were removed. Bytecode carries
  these layouts and uses StructId/FieldRef slots instead of named field records.
  Heap structs retain a verified version layout and positional values. Reads check
  the actual layout; writes also check permissions and value representations before
  committing. Explicit reflection resolves names from layouts and shares slot
  mutation checks. Allocation rejects foreign-runtime layouts and invalid field
  values before accounting. Old objects retain their layout; compatible layouts
  across versions require equal identities, slots and schemas. Initializer values
  are emitted in layout order after source-order evaluation. The layouts IR example
  shows these contracts. Full nominal field schema verification and interface
  tables remain pending, so R07/R08 are not complete.
  Calls now carry one HIR target and an explicit receiver. IR and reflection
  permission checks consume that target; duplicate Array/String method checking,
  backend builtin-name classification and fallback call dispatch were removed.
  Source String length uses the specified len_bytes/len_chars intrinsics; the old
  String.len source entry is rejected. The old BuiltinMethod declaration tables,
  IR/bytecode operands and runtime dispatch API are removed. StandardIntrinsic is
  the sole standard-library execution path, including manually built bytecode.
  Array/string iteration and empty pop now have source/artifact/JIT fallback
  fixtures covering Option results, Unicode indexing and shared-array mutation.
  Trait-call targets support navigation even with argument errors; executable
  interface dispatch still awaits linked implementation tables in R07/R08.
- R05: unchanged files share parse/analysis results; identical function bodies
  reuse remapped type facts, while declaration changes invalidate that reuse.
  Scope/type/member-receiver queries work on erroneous files. Shared cancellation
  now reaches lexer character iteration, parser traversal, HIR expression/block
  lowering, name resolution and body checking. Cancelled snapshots do not publish.
  Query ownership is now explicit; compile-time limits remain in R15.
  Language profiles participate in cache reuse; old queries cannot publish over
  a newer revision. Engine snapshot compilation exposes cancellation explicitly.
  Reused bodies remap scalar expression and pattern facts; emitted artifacts are
  checked against fresh analysis after preceding code changes shift arena IDs.
  Call facts remap their receiver IDs on body reuse and share this artifact check.
  Inferred generic call arguments are retained on reuse; the cache/fresh artifact
  comparison now includes a reused generic template and a call to its i32 instance.
  Field access, assignment and initializer facts also remap on body reuse; signature
  changes invalidate field slot reuse while named field identity survives reordering.
  Body-local type-reference facts are remapped too, including after preceding edits
  shift type arenas. Shared implicit receiver references retain their impl context.
  Stable named declarations can be located in a new snapshot, while local binding
  handles remain scoped to their original analysis, including on profile changes.
  `cargo run -p kagari-embed --example source_queries` exercises these tool APIs.
  Source dependency revisions and public export sets now participate in analysis
  reuse. Adding/removing an overlay-only dependency invalidates unchanged importers;
  cached old snapshots retain their original targets. This is conservative file
  dependency invalidation. Function signatures, field types and impl contracts now
  have a separate immutable query result, prepared for all files before body checking.
  Imported function calls and public source facades consume those checked signatures;
  scalar/composite argument checks preserve nominal declaration identity. Signature
  facts survive dependency body/constant errors. Transitive facade signature changes
  invalidate callers even when their direct import revisions did not change; unrelated
  file results and unchanged local signature results remain shared. Edits confined
  to user/impl function bodies now reuse checked signatures, including erroneous
  signatures. Type-reference arena IDs and diagnostic ranges are rebased onto the
  new lowering; generic/field/impl targets retain their declaration identities.
  Declaration text, imported type bindings or host declaration changes invalidate
  reuse. Cached and fresh analyses are compared for types, navigation, diagnostics
  and emitted artifacts, including CRLF/Unicode edits and transitive type changes.
  `FileAnalysis::signatures_reused()` reports whether its signature query reused
  checked facts; unchanged query results keep their original statistic.
  `AnalysisDatabase` and `KagariEngine` now expose independent `declarations` and
  `signatures` queries with immutable file results. Both stop before body name
  resolution, local binding collection, type checking and const evaluation.
  Full analysis consumes these same cached queries and creates fresh local binding
  identities only when bodies are analyzed. Lowered HIR is shared immutably across
  declaration, signature and full results instead of deep-cloned per stage.
  Complete snapshots expose the exact
  declaration/signature snapshots they consumed. Each query publishes caches only
  after cancellation checks and cannot replace newer source revisions. Partial
  declaration and signature results retain their own diagnostics and can be reused
  without any previous full analysis. `body(source, definition, cancel)` now queries
  one function independently and retains its own immutable source/signature context,
  scopes, bindings and type facts. Neighbor bodies are not resolved or checked.
  Its cache handles source/dependency changes, deleted declarations and old queries;
  unchanged user/impl bodies remap facts by declaration-order function identity,
  including same-named methods. Full analysis uses the same checker in batch form.
  All module declarations are now available before signature checking. Public struct,
  enum and trait type annotations resolve through direct imports, qualified module
  aliases and source type facades. Parameter, return, field and local annotations
  retain nominal identity and dependency-owned navigation targets. Type import changes
  invalidate local signatures and bodies, including through unchanged facade files.
  Function exports may use types imported from another source module. Public facade
  target traversal is shared by type imports, imported calls and definition queries;
  cyclic or stale targets cannot escape their snapshot. Imported trait implementation
  contracts and generic impl specialization remain pending.
  Aggregate contracts from reachable dependencies participate in body invalidation,
  including when a function's nominal return type stays unchanged but its fields
  change. Unrelated module results remain shared. Local body edits can reuse nominal
  field targets while navigation uses the new catalog's declaration positions.
- R03/R14: an immutable import graph detects strongly connected components with
  explicit stacks and rejects reachable cycles before compilation. Its deterministic
  dependency-first order visits diamond dependencies once and ignores unrelated
  cycles. Embedding errors preserve the dependency file and revision.
  CheckedProgram owns the complete immutable source closure;
  dependency body/constant errors reject compilation with their own file/revision.
  IR retains module boundaries and dependency edges, and verifies imported declaration
  contracts against module/function link slots, including public source facades.
  Same-spelled functions and stale document targets remain distinct. Generic-instance
  and instruction limits are shared across the closure. CheckedModule now exposes
  this program instead of a single analyzed root. BytecodeProgram now encodes this
  closure in dependency-first order. Imported calls use module/function slots;
  validation checks graph reachability, signatures and shared layout/host contracts.
  Runtime initialization visits every dependency once, including unused imports,
  and caches failures per runtime and version. Root calls retain one shared program
  version across module calls. VM and embedding reload now stage a verified/linked
  candidate, execute its dependency-first initializers in a restricted root session,
  and only then publish. Failed initialization releases candidate instances and
  module quota without changing the active entry or invoking forbidden callbacks.
  Suspended ordinary calls regain their original session and dependency version.
  The runtime no longer exposes direct reload-and-publish APIs; low-level drivers
  stage a candidate, initialize it, then request publication. Publication rejects
  uninitialized members and rechecks the baseline and host links before activation
  and cache invalidation. Stale candidates cannot alter the newly published entry.
  Candidate instances remain reachable during collection; publication preserves
  initialized results and dropping a candidate removes all of its members.
  Candidate sessions reject external-service, host-mutation, suspension and borrowed
  host-call contracts before callbacks, and reject typed paths before adapters.
  Pure host calls remain subject to permissions and budgets. Immutable configuration
  reads have an explicit offline effect contract with owned value-only signatures;
  mutable containers and opaque handles are rejected. KHI v6, format 22 and runtime
  ABI v23 reject old encodings. A full isolation/ownership audit remains pending.
  Module admission and release belong to the module store, including staged and
  unreachable versions. Cleanup releases quota even after quarantine. Epoch
  reservation is separate from activation; discarded identities are never reused
  and exhaustion is rejected before installation. Source and encoded program tests
  cover forbidden host effects, initializer traps, successful pre-publication
  initialization, old-call restoration and retained old-version execution.
  Ordinary execution rejects unpublished modules at session entry, so a candidate
  handle cannot bypass effect restrictions. Candidate sessions borrow their staged
  owner; publication also rejects candidates with an active execution session.
  Session cleanup checks its own frame scopes and host resources rather than the
  runtime's aggregate call depth. Ending a suspended empty session cannot mistake
  candidate frames for leaked resources; using a suspended execution stack is an
  engine fault, and quarantine still allows all candidate/old resources to unwind.
  Heap slots record candidate allocation ownership. Candidate host arguments/results
  and script call arguments traverse their reachable graph and reject mutable
  objects allocated outside that candidate, including old objects inside fresh
  wrappers. Immutable enums are traversed by member rules. Candidate allocations
  remain usable through collection and publication; ordinary execution is unchanged.
  All mutable object access now checks candidate ownership before opening the target
  for writes. Direct array/map/set edits against preexisting objects fail without
  changing values or resource counters; candidate-owned edits remain permitted.
  Script-visible reads of external mutable objects are also rejected, including
  snapshots and struct fields. Collector tracing retains its internal access so
  old roots survive candidate collection. Host results undergo ownership checks
  before shape traversal. Module snapshots and mutable instance borrows are limited
  to the candidate program at the store boundary. Public initialization operations
  reject external instances, while existing lifecycle guards retain an internal
  failure-cleanup path. Publication independently checks every candidate module slot
  and initialization result against the candidate allocation owner after the session
  ends. Late external references inside candidate containers reject publication and
  release candidate quota without changing the active entry. Other ingress paths
  and complete checkpoint acceptance remain under audit. Embedding tests now exercise
  a diamond program whose shared dependency allocates before a later dependency
  traps: all candidate members and quota are released, unreachable candidate objects
  are collected, and repeated fresh attempts leave the old root usable. A subsequent
  successful reload initializes every member before publication; old/new root calls
  continue to observe their own dependency implementations, for source and encoded
  artifacts. Cancellation at candidate entry and instruction-budget exhaustion during
  initialization also have source/encoded coverage: both restore the old session,
  release frames, temporary roots, candidate quota and retention, and leave a later
  ordinary root and a fresh reload attempt usable without quarantining the runtime.
  Candidate handles cache terminal initialization errors across session teardown.
  Entry cancellation, cancellation observed on session exit, and budget exhaustion
  prevent both retry and publication even for modules without initializer code.
  Staged root sessions require the dedicated candidate entry; setting the phase on
  an ordinary execution request cannot bypass that lifecycle.
- R06: host functions now take a separate declaration containing nominal identity,
  typed scalar/opaque signatures, borrowing, effects, capabilities, cost and docs.
  The old metadata API, string type names and caller-chosen function fingerprints
  were removed. Offline KHI encoding is canonical and bounded; explicit registry
  linking checks complete contracts without executing callbacks. HIR print checking
  and CLI logging share the same declaration. The offline_host example exercises
  export, decode, binding verification and invocation. Scalar host function and
  module imports, aliases and qualified calls now compile from immutable offline
  declaration inputs. Lowering retains all import paths and source spans; name
  resolution diagnoses unknown paths and conflicts instead of silently discarding
  nonstandard imports. Host revisions invalidate semantic/body caches and old
  snapshots retain their original declarations, types and docs. Protocol-neutral
  callee queries return offline declarations. The offline_compile embedding
  example needs no runtime registration. Host calls require the language profile
  and cannot execute in scalar constants. Composite scalar/container declarations
  now cover Tuple, Array, Map, Set, Option and Result; HIR retains nested types and
  runtime boundaries check complete member shapes with cancellation. KHI v6 and
  artifact format 22/runtime ABI v23 reject old products; host types use bounded flat nodes
  with 4096-node and 64-depth limits. Tests cover malformed type encodings, nested
  source mismatches, wrong callback results, nested borrow escape, foreign/stale
  handles, callback GC and retained roots through source/artifact/JIT fallback.
  The offline_compile example
  compiles array contracts through a facade. Artifact and verification-metadata
  constructors are now fallible and verify before fingerprinting. In-memory loader
  checks reject malformed host types before serialization; invalid root fallback
  metadata was removed. Host field/path and trait integration
  remains outstanding; this does not mark R06 complete.
  Host type registrations now carry declaration identities independent of labels.
  Duplicate/invalid identities reject before general metadata registration, and
  linking checks nested opaque references before module publication. Calls match
  nominal root/borrow types; root handles carry registry ownership and no public
  unchecked constructor remains. Tests cover same-named declarations, renamed
  exports, failed-registration cleanup, foreign roots with coincident numeric IDs,
  borrowed type mismatches and foreign path-view chaining. The atomic_host_path
  example uses an explicit declaration identity. Portable HostTypeDeclaration now
  owns field/method identities, signatures, access, borrowing, effects and docs.
  Runtime registration consumes this definition and generates slots/fingerprints;
  the old caller-supplied runtime member/fingerprint registration model was removed.
  Batches support mutual references and publish metadata only after all types
  resolve. KHI v6 and artifact format 22 carry type declarations; linking verifies
  the complete contract and ignores documentation for ABI comparison. Offline
  queries reject stale catalog IDs, and offline_compile reads field declarations
  without a runtime. Tests cover canonical encoding, invalid member owners,
  reference closure, batch failure atomicity, generated metadata and ABI changes.
  Source host types now enter imports, annotations, call signatures and facade
  re-exports with declaration identities. HIR exposes host_type_at, preserves
  erroneous application targets and invalidates query reuse on host revision
  changes. Script structs and host types remain distinct; host generic equality
  and script construction are rejected. Generic script functions instantiate
  Host types, and public ABI carries their nominal identities. IR/bytecode use a
  separate HostHandle representation and carry the required type/member reference
  closure, including annotation-only dependencies. Unused catalog types are omitted.
  Tests cover offline queries, shadowing, source mismatches, missing declarations,
  transitive linking, callback GC and source/artifact/existing-JIT fallback call
  traces. The offline_compile example exports a facade-typed host signature without
  runtime registration. The obsolete unsupported-host-type diagnostic and duplicate
  IR host-contract validation path are removed. Host methods now derive executable
  contracts from their member declarations, with explicit nominal receivers and
  declared borrowing, effects, capabilities and resource cost. Offline analysis
  resolves receiver calls and preserves their target/docs under argument errors.
  Registration rejects missing owners or changed contracts before installing
  callbacks, and linking rejects missing methods. Lowering evaluates receivers
  before arguments once and uses existing linked host slots. Common/HIR/embedding
  tests cover corrupted contracts, offline queries, failed registration, missing
  callbacks, capability denial, exclusive receiver reentry conflicts and repeat
  execution after cleanup across source/artifact/JIT fallback routes. The
  offline_compile example compiles a declared method without runtime registration.
  Field path registration now accepts declaration identities instead of hand-written
  slots, types, permissions and member fingerprints. It resolves fields against the
  current nominal owner and generates runtime segments from declaration metadata.
  Tests reject same-named foreign fields, private/disabled fields, access escalation
  and wrong result types before publication. Existing VM, embedding and GC path
  fixtures now declare their actual fields; atomic_host_path keeps the field ID
  directly from its declaration. Host trait implementations
  and declared index/virtual paths remain pending. Whole-path fingerprints now use
  a versioned fixed-order encoding of resolved contracts, permissions and schema;
  the caller-supplied fingerprint input is removed. Types without portable host
  contracts reject registration. Tests prove unrelated runtime slot shifts and
  documentation edits preserve fingerprints, while field type, permissions,
  capabilities and schema changes alter them.
  The canonical encoder now lives in common declarations as HostPathContract;
  runtime registration no longer owns a separate encoding implementation.
  HostInterface::field_path_contract resolves offline nominal field chains with
  ownership/access checks. Tests compare offline and registered fingerprints,
  nested interface round trips and invalid chains; offline_compile generates a
  field path contract without starting a runtime. Source host index paths remain pending.
  KHI v6 now persists HostFieldPathDeclaration records with nominal field chains,
  schema, access and capabilities. Runtime field-path registration consumes these
  declarations, exported interfaces retain them, and linking rejects missing or
  ambiguous required paths. Canonical-order, duplicate, length-limit and binding
  tests plus offline_compile/atomic_host_path exercise the same declaration model.
  Format 22/runtime ABI v23 reject prior products; source host index paths are pending.
  Source host field reads now resolve a unique declared chain in HIR, preserving
  member/type facts and offline docs under missing/ambiguous-path diagnostics.
  Body reuse remaps path root IDs. IR consumes the checked root/contract and emits
  one ReadPath for nested chains; required host types and path declarations flow
  into bytecode and linking. Tests cover root-once order, capability denial, GC
  cleanup, source/artifact/JIT fallback agreement and snapshot reuse. offline_compile
  now compiles both field reads and method calls without runtime registration.
  Source field writes and compound assignments now own checked root-place/path
  facts in HIR and lower to SetPath/ModifyPath after capturing the root before RHS.
  Readonly paths and disabled path-mutation profiles reject compilation. Tests
  verify root/RHS/read/prepare/commit order, dirty old/new values, and no final write
  after division by zero, overflow or RHS target deletion; completed RHS effects
  remain. Source/artifact/JIT fallback and reused-body place-ID checks pass.
  offline_compile includes parameter-rooted assignment and compound assignment.
  Protocol-independent host field queries also consume assignment-place facts,
  including readonly-path diagnostics and reused bodies. Tests retain the old
  snapshot's queries after edits and declaration changes.
  Mixed script/host field-chain analysis now finds the first host receiver and
  resolves its complete suffix, without requiring independently declared prefix
  paths. This preserves abstract member facts; host handles remain prohibited as
  script heap payloads. Execution tests replace a local host root during RHS and
  verify that successful writes and failed updates still address the captured root
  across source bytecode, encoded artifacts and existing JIT fallback.
  IR/bytecode path records now require the contract fingerprint. Load and reload
  link module-local paths to runtime descriptors before publication, reject missing
  or ambiguous bindings and validate operand/write contracts. VM execution consumes
  immutable version bindings instead of casting PathId to a descriptor number.
  Linking also checks dynamic index argument counts and register representations
  for all four path operations; tests cover missing/extra/wrong-type arguments,
  successful bindings and rejection without module publication.
  Source-bytecode, encoded-artifact and existing JIT fallback tests bind path 0 to
  descriptor 1, ignore debug labels and preserve old bindings after registration.
  Artifact path ABI hashes exclude diagnostic labels. Format 22/runtime ABI v23
  reject previous products; source host index paths remain pending.
  Runtime path registration also rejects disconnected field owners
  and index collections before publishing or consuming a descriptor slot; tests
  cover both root and intermediate mismatches. Format 22/runtime ABI v23
  reject prior products. R06 remains unchecked.
  Source facades now re-export host functions and modules using one final import
  binding table. Name resolution, imported signature/type catalogs and navigation
  share those targets; downstream facade traversal and the unsupported-host-export
  diagnostic were removed. Original source edges preserve initialization order.
  Offline query tests cover aliases, chained facades, namespace calls, shadowing,
  duplicate exports and host revision invalidation. Embedding tests verify missing
  bindings reject before publication, facade initialization runs once, and source,
  encoded artifacts and existing JIT fallback produce the same host call trace.
  The offline_compile example uses a source facade without runtime registration.
  Required host declarations now link at bytecode/artifact load and reload before
  publication, resource counters or initialization. Calls carry HostImportId and
  resolve to registry-owned slots; execution has no host-symbol fallback.
  IR/bytecode checks verify call representations and reject conflicting imports.
  Runtime scalar callback arguments/results are also checked. Nominal opaque
  object validation remains open.
- R07/R08/R09: public function parameters/results, const types, struct fields,
  trait methods and interface targets now encode checked structural AbiType facts,
  preserving module/declaration identity and nested arguments. Generic binders
  encode owner and position; checked standard/nominal trait bounds are merged,
  sorted and deduplicated. Renaming a binder or reordering equivalent constraints
  preserves ABI. Shared IR/bytecode validation rejects foreign/free parameters,
  escaped Self, wrong nominal kinds and invalid standard-enum arity; exported
  top-level functions still require concrete signatures. Format 22/runtime ABI v23
  reject previous products. Tests cover artifact round trips, distinct same-named
  dependency types, malformed signatures and ABI rejection before reload publication
  with the old entry intact. The source_modules example inspects a dependency-owned
  public result type after encoding. Cross-module trait constraints, full semantic
  signature/layout agreement and complete artifact resource bounds remain open.
- R08/R09/R10: LoadedModule is an immutable shared Arc handle; public raw store
  loading and post-load bytecode mutation were removed. Module queries share code.
  Loaded handles and host slots reject cross-runtime use. Format v10 rejects v1–v9;
  required host fingerprints derive from declarations rather than caller options.
  The empty string host-dependency side table was removed. Struct layout handles
  retain executable versions. LoadedModule members share an Arc-owned program;
  retaining an active call on any member keeps all its module instances reachable.
  Root-version dependency pinning is covered across reload. Roots and full instance/
  code lifecycle reclamation remain open. Runtime and embedding load/reload entry
  points now accept whole programs; the old module-loading APIs were removed.
- R07: semantic Struct/Enum/Trait types use DefinitionId, and generic parameters
  use their declaring owner and position. Same-spelled cross-module types and
  shadowed generic parameters are distinct. Implicit Self types belong to a trait;
  impl checking and trait calls share one substitution operation that only replaces
  that owner's Self. Standard Option/Result types use StandardEnum identities.
  Empty constructor inference uses Unknown instead of invented generic names.
  Declaration collection precedes checking, and lowering retains the source origin;
  the origin-free AST lowering and standalone type-check API were removed. Body
  reuse checks module identity as well as declaration text. Calls infer function
  arguments structurally and check bounds; public generic functions are rejected.
  Private function templates now emit reachable concrete instances, deduplicated
  by declaration plus arguments. IR InstanceId replaces HIR IDs as execution call
  targets, and signatures/locals/temporaries use instantiated types. Static trait
  calls on existing concrete implementations use HIR implementation targets.
  Return/break/continue terminate block lowering, preventing later effects and
  unreachable generic calls from being emitted. Generic impl specialization,
  applied trait arguments and dynamic implementation tables remain outstanding;
  public ABI labels still need linked identities. Generic struct/enum instances
  now have concrete layouts keyed by declaration and arguments; backwards
  contextual constraints for generic-call arguments
  remain outstanding. Explicit Struct constructor arguments now retain owned type
  references and share annotation resolution, arity and bound checks. Tests cover
  nested invalid container constraints, facade imports, distinct phantom layouts,
  caller-owned binders and type queries after unchanged-body reuse. Source, artifact
  and JIT fallback routes share the same concrete layouts.
  Explicit enum paths (`Token<T>::Variant`) now retain a declaration-owned type
  reference on HIR names. Unit and payload constructors share Struct annotation
  resolution and constraint validation. Wrong arity, unknown variants/types,
  invalid bounds and conflicting context reject before code generation. Tests
  cover facade imports, caller binders, distinct concrete instances, lossless
  syntax and type queries after body reuse across snapshot revisions.
  Error-bearing files no longer disable all body reuse. Unchanged functions with
  no owned diagnostics can reuse their facts while erroneous functions recheck;
  unowned or file-level diagnostics conservatively prevent reuse. Navigation tests
  distinguish enum owner, type argument and variant after payload/type errors,
  preserve old snapshots, and verify repair beside a reused correct function.
  Focused invalidation tests also verify spanless/file-level diagnostics and
  zero-width diagnostics inside a body, including the distinction between the
  affected function and an unaffected neighbor.
  Annotated locals now supply Struct type arguments, including
  phantom parameters, and propagate checked context into nested Struct fields.
  Declaration identity must match; fields and bounds retain normal validation.
  HIR rejection cases, source/artifact/JIT fallback execution and the standard-library
  example cover this inference boundary.
  Return annotations now reach tail expressions and explicit return operands;
  blocks, if/match branches, Tuple members and Array elements propagate context.
  Execution fixtures cover these routes across source/artifact/JIT fallback.
  Enum constructors now consume matching declaration-owned context too. Unit
  variants infer phantom arguments; instantiated payload types supply nested
  constructor context. Arity, payload and foreign-declaration failures reject
  codegen; source/artifact/JIT fallback exercises unit equality and nested payloads.
  Concrete local/imported function parameter types now provide constructor context
  before argument checking, including source facades. Extra arguments still get
  checked; shadowing cannot borrow another function's signature. Regressions cover
  arity/type failures and source/artifact/JIT fallback for imported constructors.
  Trait method signatures share that argument-context entry after Self substitution.
  Invalid arguments retain method targets; static trait execution tests cover
  contextual Struct/enum arguments across source/artifact/JIT fallback. Caller-owned
  generic binders now supply context too, including trait Self substitution and
  recursive calls. Same-spelled uninferred callee binders cannot supply context.
  Source/artifact/JIT fixtures cover generic forwarding and Marker<Self> arguments.
  Local generic calls now infer from expected results and pass concrete arguments
  established by preceding operands into later constructors. Source/artifact/JIT
  fixtures cover phantom function results and nested constructors. Later operands
  do not yet constrain earlier constructors; no argument is analyzed twice.
  Assignment RHS inference now consumes the checked target type. Local, field,
  array-index and enum replacement fixtures cover source/artifact/JIT fallback;
  immutable targets and incompatible payloads remain compilation errors.
  Empty Array/Map/Set constructors now consume context in expression analysis.
  The local-binding-only Map/Set type rewrite is deleted. Return, field, argument
  and assignment fixtures execute through source/artifact/JIT fallback, while
  constructor arity, container-kind and element-type mismatches still reject.
  Local container annotations now share the signature HashKey validator. Invalid
  scalar keys, nested key types and unconstrained generic keys reject in HIR;
  properly constrained generic keys remain valid.
  Partial annotations now validate known container constraints independently of
  unknown sibling types: `Map<f32, Missing>` reports both issues, while an unknown
  key alone does not invent a HashKey failure. Validation descends into key/element
  applications too. Regressions cover fields, enum payloads, parameters, returns,
  constants, locals and explicit Struct constructor arguments.
  Applied user-type bounds also check known outer shapes in partial arguments.
  An Array with an unknown element still fails HashKey/numeric bounds and meets
  Iterable. Trait implementation lookup and recursive Comparable checks defer
  while their required member identities remain unresolved; unknown leaf types
  do not produce extra constraint failures.
  Standard bound satisfaction and diagnostic reasons are now shared by calls,
  aggregate applications and operators. Comparable recursively respects caller
  bounds inside Tuple and standard-enum members in every path. Tests accept a
  constrained Tuple binder in both call and nominal arguments, reject the same
  unconstrained binder, and execute concrete layouts through all existing routes.
  Struct fields and enum payloads now update substitutions after each source-order
  member, supplying context to later nested constructors. Known member types no
  longer wait for unrelated binders. HIR and source/artifact/JIT fixtures cover
  phantom payloads, caller-owned binders and rejected member mismatches.
  Function fallthrough now uses a separate normal-completion analysis. Explicit
  returns and loops without reachable breaks no longer synthesize a Unit return;
  if/match type joins exclude branches that cannot complete. Unreachable statements
  retain diagnostics. While loops conservatively retain their zero-iteration path.
  IR joins without predecessors terminate as Unreachable, including loop exits.
  Regressions cover nested loop breaks, missing returns, mixed returning/value
  branches and source/artifact/JIT execution of explicit returns.
  Call arguments and aggregate members stop lowering once a member terminates
  control flow. Later generic calls no longer consume the instance budget, and
  terminating initializers do not emit a local store. Tests cover Tuple, Array,
  enum and Struct members, empty unreachable joins and source/artifact/JIT routes.
  Primitive operators and reflection helpers now propagate terminating operands
  too. Short-circuit joins preserve the path that skips the right operand;
  terminating while conditions and assignment RHS expressions retain their return
  instead of emitting a branch or write. Tests cover both short-circuit outcomes,
  helper suppression and unreachable generic operands across execution routes.
  Assignment target preparation now returns no location when a root or index
  terminates control flow. Recursive projections and host-root preparation propagate
  that result before evaluating further indexes, RHS expressions or writes.
  Nested-index and temporary-root regressions cover assignment and compound
  assignment, with unreachable generic instances forbidden by a zero budget.
  Match wildcard and binding patterns now share an irrefutability predicate.
  Later arms retain independent diagnostics but do not contribute result types,
  completion paths or generated instances. Named and wildcard arms share their
  IR body lowering; regressions cover shadowed generic recursion and bindings
  across source/artifact/JIT execution.
  Normal-completion queries now propagate cancellation separately from their
  boolean facts. Sequential operands are visited in source order and stop after
  termination; cancellation unwinds expression/member lists immediately instead
  of substituting a normal-completion result. Deterministic tests cover empty
  blocks, expression queries and cancellation during an operand sequence.
  Body checking now stops Tuple, Array, match-arm and call-argument traversal at
  cancellation boundaries. Contextual and ordinary argument checking share one
  loop; Array element types merge during traversal without retaining a second
  full element list. Existing partial-type recovery and query cancellation tests
  cover these shared paths.
  Standard container constraint traversal also uses an explicit stack with
  cancellation checks between type nodes. A synthetic 10,000-level Array input
  verifies traversal depth independently of parser limits, and a cancelled input
  produces no constraint diagnostics. Source-order constraint reporting is retained.
  Generic instantiation now reconstructs templates and substitution values with
  an explicit work stack. Inserted types preserve caller binders without applying
  the same substitution again. A 10,000-level template and equally deep replacement
  verify both paths; nominal-owner and ordered-member regressions remain in place.
  Trait Self substitution now shares that reconstruction walk instead of owning
  a recursive copy of every type case. Deep Self replacements preserve their
  own binder, and foreign trait owners remain untouched. Existing impl-check
  and static trait-call regressions exercise the shared operation.
  Other recursive type operations and configurable depth limits remain R15 work.
- R08: bytecode generation now requires an immutable VerifiedIrModule. The IR
  verifier checks instance identities, direct-call signatures, operand types,
  control flow, parameter layout, debug alignment, effects and definite
  initialization across branches and loop backedges. Editing IR invalidates its
  verification handle. Standard intrinsic and numeric representation contracts
  are shared with bytecode validation; intrinsic arity uses HIR declarations.
  Entry block order is preserved, and IDs are bounded before narrowing. Verification
  supports cancellation and bounds its dataflow matrix to 64 MiB. Nominal field
  layouts and scalar host signatures are checked; dynamic interface tables, root maps and the final
  linked-only runtime boundary remain outstanding.
- R15: IR generation has configurable instance, type-node, type-depth and generated
  instruction limits plus cancellation. Expansion counts nodes while copying,
  including replacement trees. Embedding returns revision-owned structured
  diagnostics and keeps checked analysis usable after failure. Parser, const
  evaluation and diagnostic-count limits remain outstanding. Source/artifact/JIT
  fallback fixtures cover generic values, recursion, numeric overflow, effects,
  constraints and distinct concrete types sharing a runtime representation.
  The existing native JIT still only supports zero-argument scalar entries; this
  does not claim native compilation of parameterized generic instances.
- R09: format v10 uses fixed little-endian encoding, bounded decoding and strict
  trailing-data rejection. Compatibility fingerprints use canonical serialization
  and explicit FNV-1a-64 rather than Debug; content checks cover header metadata.
  Old formats are rejected even if callers request their version. Full linked
  identity integration and per-table count/depth limits remain outstanding.
  Public const ABI values use tagged scalar encoding with explicit float bits and
  UTF-8 string lengths; older Debug-based const ABI artifacts are rejected.
  Language semantics now has its own `kagari-language-v1` identity, independent
  of Rust crate versions. Legacy language identities cannot be opted into at load.
  The artifact-only module identity type was removed. Bytecode, artifact headers
  and loader metadata share the source ModuleIdentity and must agree before load.
  Runtime lookup names remain separate display/entry labels pending R10.
  Dependency fingerprints now derive from actual program members and carry typed
  ModuleIdentity; build options cannot supply them. Loader pins are optional,
  while payload agreement is always checked. Host fingerprints cover all members.
  Source and artifact loads derive the same execution-version fingerprint. Root
  verification summaries are checked against the program, including after a
  recomputed checksum. Per-member debug/function metadata stays in its module.
- R11 prerequisite for R02: the VM and standard equality assertion now call one
  script equality operation instead of Rust Value::PartialEq. Tuples/enums compare
  members, mutable objects compare identity, and unsupported categories trap.
  HeapObjectId now carries unique heap ownership, slot and generation. Collection
  reclaims unreachable slots and increments generations on reuse; foreign/stale
  handles and wrong value tags reject reads, writes, roots and script equality.
  RootedValue replaces naked root IDs; clones share retention and last drop releases
  it. Frames use registered RootSet storage with short accesses. The nonmoving
  mark-sweep baseline traces cycles and deep heap chains with an explicit work stack.
  Module state/results, debug bindings, path arguments and pending dirty records
  participate in tracing. Interpreter and scalar-JIT safepoints schedule collection;
  trap/budget exits release frame roots. Runtime callbacks are local to one thread
  and can retain explicit rooted values. The rooted_values example records pause
  data for a 10,000-object chain, recorded in [performance-baseline.md](performance-baseline.md).
  Enum nominal identity and complete interface/capture ownership remain open;
  this does not mark R11 complete. Host reentry is covered by the R12 evidence above.
- R12/R17: execution frames now carry their owning program member, and module-state
  access uses short borrows. Debug frames and breakpoints distinguish member IDs
  even when local function IDs coincide. BackendFunctionInput can only select a
  function from an immutable linked version; cross-module calls pass through the
  existing interpreter fallback. Root execution sessions now pin the dependency
  program and immutable permissions/host policy. Initialization, entry and backend
  fallback share root budgets and cancellation; nested scopes inherit them and the
  last scope releases version retention. Context resource overrides now apply per
  call without changing runtime defaults. Runtime counters remain cumulative while
  ExecutionCounters reports root usage and peaks. Cooperative cancellation and
  wall-time limits preserve completed effects and release frames; resource
  termination stays recorded until the session ends. HostCallContext now owns a
  scoped borrow guard and exposes synchronous reentry into initialized linked
  functions of the pinned program. Reentry reuses the explicit executor, validates
  argument/result representations and returns an explicit rooted result. Tests
  cover GC during nested calls, retained results, borrow conflicts and cleanup,
  swallowed termination, ordinary nested traps and rejection of other epochs.
  Direct registry invocation bypasses are removed. The session now owns one frame
  stack across interpreter, reentry and VM native scopes. Scope guards unwind only
  their own suffix after traps, cancellation or quarantine. Frames retain their
  loaded version and roots; the old VM frame type and native depth guard are gone.
  The root observer shares debugger state through short borrows, and nested pauses
  contain suspended callers at the actual call instruction. Runtime tests cover
  stack/root ownership and invariant quarantine; direct/encoded interpreter and
  JIT fallback fixtures cover nested breakpoints/traps. Host resource scopes now
  register temporary roots and object leases in the session; callback/path scopes
  release both before dropping their session handle. Borrow ownership is checked
  across runtimes and the unchecked host-frame entry is removed. Path callbacks
  receive the same checked call context for synchronous reentry before commit.
  Lexical visibility and stable debug frame identities remain R17 work.
  Tests cover direct/encoded
  programs, dependency-first initialization/failure caching, stale reloads, old
  dependency calls, malformed program rejection and interpreter/JIT fallback parity.
- R13/R17 prerequisite for R02: interpreter arithmetic, typed-path arithmetic and
  integer abs use checked operations. Existing native i32 add/subtract/multiply/
  negate check each operation, including intermediate overflow, and preserve
  structured resource/trap errors. IR records trapping arithmetic effects. Runtime
  ABI is v11 and JIT helper ABI is v5. Path arithmetic failure produces no
  commit action or dirty record. Heap allocations and standard container growth
  now share resource counters: validation, budget checks and capacity preparation
  precede mutation and accounting commit. Failed operations charge no units;
  removals/GC release live units without refunding cumulative allocation budget.
  Standard array/map removals prepare their Option result before removing an
  entry, and structured allocation errors survive builtin/VM boundaries.
  Direct/encoded programs and existing JIT fallback verify the same resource
  failures, counters and frame cleanup. Typed paths now use fallible preparation
  returning an infallible commit action; the old write/dirty callbacks are removed.
  Old-value read errors propagate. The engine reserves ledger capacity and checks
  its dirty-record quota before committing. Prepared resources drop on rejection;
  heap temporaries survive collection during preparation. Commit panics or execution
  attempts quarantine the runtime and preserve EngineFault through VM/embedding/JIT
  boundaries; cleanup releases frame roots and call depth. Direct/encoded programs
  and existing JIT fallback cover commit faults and subsequent execution rejection.
  Full engine-invariant coverage outside path commits and unified frame/host-borrow
  cleanup remain open; this does not complete R13.
  Initialization now owns a lifecycle guard and version retention with no long
  module-state borrow. Success validates the stored result; every unfinished exit
  records failure and releases retention. Failure cleanup is allowed after runtime
  quarantine, so an initializer commit fault cannot trigger a second panic while
  attempting an ordinary module-state write. Direct/encoded interpreter and JIT
  fallback fixtures cover both entry and initializer faults. Root sessions provide
  cancellation and shared budgets; synchronous callback reentry inherits them.
  Frame stacks, nested debugging and temporary host-resource ownership now share
  the session; the R12 audit is recorded above.
  Const evaluation shares checked arithmetic and
  honors short circuit, with cancellation checks. Narrower integer layouts and the other backend/
  debugger contracts remain open; compile-time quotas are still pending R15.
  Assignment lowering now retains a location before RHS execution and resolves
  its projections afterwards. Tuple updates prepare temporary values before one
  enclosing object/slot commit; shared-object ancestors are not rewritten.
  Source syntax supports `+=`, `-=`, `*=`, `/=` and computed receivers. Tuple
  member replacement requires a writable enclosing slot. Runtime mutation records,
  other mutation observations and full failure-state observation remain pending.
  Line comments are retained as CST trivia, with Unicode/CRLF coverage. The
  standard-library example now runs through the CLI as part of this validation.

Validation: workspace tests pass after the source/analysis changes. Workspace
clippy with `-D warnings` passes after correcting baseline lints and marking the
raw-pointer JIT helper's caller contract unsafe. The complete track remains open.
