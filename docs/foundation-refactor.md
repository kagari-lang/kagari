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
- [ ] R03: Unified source database, revisions, identities, overlays, coordinates.
- [ ] R04: Recoverable HIR analysis; checked-only code generation.
- [ ] R05: Immutable queries, cancellation, parse/body reuse and invalidation.
- [ ] R06: Offline host declarations and checked runtime bindings.
- [ ] R07: Nominal concrete identity, layouts, bounded reachable monomorphization.
- [ ] R08: Verified IR and linked-only runtime operands.
- [ ] R09: Canonical bounded artifact format and explicit fingerprint algorithm.
- [ ] R10: Shared immutable generations and runtime-local state.
- [ ] R11: Value semantics, owned handles/roots, nonmoving mark-sweep baseline.
- [ ] R12: Execution sessions, synchronous host reentry, shared cleanup/budgets.
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
  resolution, complete HIR body scoping and concrete type identity remain outstanding.
  Logical package/module bindings now belong to source documents and survive
  overlays. Rebinding invalidates analysis; duplicate source bindings are rejected.
  Source-based HIR carries its origin through IR, bytecode and artifact metadata.
  The AST-only analysis entry and post-analysis identity overrides were removed.
  Declaration paths now include module, kind, owner and duplicate occurrence.
  Parameter/local identities include their body and analysis instance; stale local
  IDs cannot resolve in a new analysis. Definition navigation returns file/revision
  ranges and distinguishes same-spelled declarations in separate modules.
  Generic parameters now have owner/position identities and declaration ranges;
  inherited trait/impl parameters keep their original owner in method signatures.
- R04: analysis retains facts and diagnostics; unknown/missing expressions are
  represented explicitly, and codegen requires a sealed CheckedAnalysis. More
  semantic-target and source-owner integration remains outstanding.
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
  cross-module imports remain pending. Type references now retain checked types and
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
  Runtime field records still encode names pending concrete layouts and linking in
  R07/R08; this slice does not claim runtime field lookup has become slot-only.
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
  Remaining work includes dependency-query ownership and compile-time limits.
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
  and cannot execute in scalar constants. Composite declarations, nominal host
  type/member integration and facade re-export linking remain outstanding; this
  does not mark R06 complete.
  Required host declarations now link at bytecode/artifact load and reload before
  publication, resource counters or initialization. Calls carry HostImportId and
  resolve to registry-owned slots; execution has no host-symbol fallback.
  IR/bytecode checks verify call representations and reject conflicting imports.
  Runtime scalar callback arguments/results are also checked. Nominal opaque
  object validation remains open.
- R08/R09/R10: LoadedModule is an immutable shared Arc handle; public raw store
  loading and post-load bytecode mutation were removed. Module queries share code.
  Loaded handles and host slots reject cross-runtime use. Format v7 rejects v1–v6;
  required host fingerprints derive from declarations rather than caller options.
  The empty string host-dependency side table was removed. Full version-owned
  layouts, dependency pinning, roots and lifecycle reclamation remain open.
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
  applied trait/type arguments, concrete layouts and dynamic implementation tables
  remain outstanding; ABI labels/runtime fields still need linked identities.
- R08: bytecode generation now requires an immutable VerifiedIrModule. The IR
  verifier checks instance identities, direct-call signatures, operand types,
  control flow, parameter layout, debug alignment, effects and definite
  initialization across branches and loop backedges. Editing IR invalidates its
  verification handle. Standard intrinsic and numeric representation contracts
  are shared with bytecode validation; intrinsic arity uses HIR declarations.
  Entry block order is preserved, and IDs are bounded before narrowing. Verification
  supports cancellation and bounds its dataflow matrix to 64 MiB. Nominal field
  layouts, host signatures, dynamic interface tables, root maps and the final
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
- R09: format v7 uses fixed little-endian encoding, bounded decoding and strict
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
- R11 prerequisite for R02: the VM and standard equality assertion now call one
  script equality operation instead of Rust Value::PartialEq. Tuples/enums compare
  members, mutable objects compare identity, and unsupported categories trap.
  Enum nominal identity still uses the current representation pending R07; owned
  heap handles, rooted host handles and collecting GC remain outstanding.
- R13/R17 prerequisite for R02: interpreter arithmetic, typed-path arithmetic and
  integer abs use checked operations. Existing native i32 add/subtract/multiply/
  negate check each operation, including intermediate overflow, and preserve
  structured resource/trap errors. IR records trapping arithmetic effects. Runtime
  and JIT helper ABI fingerprints are v2. Path arithmetic failure produces no
  write callback or dirty record. Const evaluation shares checked arithmetic and
  honors short circuit, with cancellation checks. Mutation resource commit,
  narrower integer layouts and the other backend/
  debugger contracts remain open; compile-time quotas are still pending R15.
  Assignment lowering now retains a location before RHS execution and resolves
  its projections afterwards. Tuple updates prepare temporary values before one
  enclosing object/slot commit; shared-object ancestors are not rewritten.
  Source syntax supports `+=`, `-=`, `*=`, `/=` and computed receivers. Tuple
  member replacement requires a writable enclosing slot. Runtime mutation records,
  commit resource accounting and full failure-state observation remain pending.
  Line comments are retained as CST trivia, with Unicode/CRLF coverage. The
  standard-library example now runs through the CLI as part of this validation.

Validation: workspace tests pass after the source/analysis changes. Workspace
clippy with `-D warnings` passes after correcting baseline lints and marking the
raw-pointer JIT helper's caller contract unsafe. The complete track remains open.
