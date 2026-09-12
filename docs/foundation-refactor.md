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

- R02: shared source/artifact/JIT value, identity, alias, short-circuit and negative
  diagnostic cases. Host effect trace fixtures remain to be connected.
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
- R05: unchanged files share parse/analysis results; identical function bodies
  reuse remapped type facts, while declaration changes invalidate that reuse.
  Scope/type/member-receiver queries work on erroneous files. Shared cancellation
  now reaches lexer character iteration, parser traversal, HIR expression/block
  lowering, name resolution and body checking. Cancelled snapshots do not publish.
  Remaining work includes dependency-query ownership and compile-time limits.
  Language profiles participate in cache reuse; old queries cannot publish over
  a newer revision. Engine snapshot compilation exposes cancellation explicitly.
- R09: format v2 uses fixed little-endian encoding, bounded decoding and strict
  trailing-data rejection. Compatibility fingerprints use canonical serialization
  and explicit FNV-1a-64 rather than Debug; content checks cover header metadata.
  Old formats are rejected even if callers request their version. Full linked
  identity integration and per-table count/depth limits remain outstanding.

Validation: workspace tests pass after the source/analysis changes. Workspace
clippy with `-D warnings` passes after correcting baseline lints and marking the
raw-pointer JIT helper's caller contract unsafe. The complete track remains open.
