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
  resolution and semantic definition identity integration remain outstanding.
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
- R09: format v3 uses fixed little-endian encoding, bounded decoding and strict
  trailing-data rejection. Compatibility fingerprints use canonical serialization
  and explicit FNV-1a-64 rather than Debug; content checks cover header metadata.
  Old formats are rejected even if callers request their version. Full linked
  identity integration and per-table count/depth limits remain outstanding.
  Public const ABI values use tagged scalar encoding with explicit float bits and
  UTF-8 string lengths; older Debug-based const ABI artifacts are rejected.
  Language semantics now has its own `kagari-language-v1` identity, independent
  of Rust crate versions. Legacy language identities cannot be opted into at load.
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
