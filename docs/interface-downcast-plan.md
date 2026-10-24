# Interface type tests and downcast execution plan

Status: agreed design recorded on 2026-10-16; implementation has not started.
The [roadmap](implementation-roadmap.md#interface-type-tests-and-downcast) owns
queue placement. This plan owns DC01-DC04, remaining design decisions, acceptance
and the progress ledger. Recording this plan does not start implementation.
Examples below are proposed behavior, not currently executable APIs.

Storage inventory refresh, 2026-10-18: the prioritized
[runtime storage redesign](implementation-roadmap.md#runtime-storage-redesign) and
[object overhead work](implementation-roadmap.md#object-storage-overhead) are locally complete.
DC01 remains queued and unstarted. The inventory reflects runtime `65bbf477`:
shared descriptions, compact directory entries, packed fields and direct snapshot
locations. Reuse these owners without recreating wrapper records or per-receiver
descriptions. Downcast semantics and DC01's remaining design gates are unchanged.

## Outcome and bounded scope

Implement checked recovery of the concrete value hidden by a trait interface:

```text
x.is::<T>()       -> bool
x.downcast::<T>() -> Option<T>
```

Reuse interface payloads, nominal type identities, generic environments, GC roots
and retained execution versions. No Any trait or universal value container is
required. This work does not add implicit downcasts, automatic type narrowing,
arbitrary trait membership queries, interface-to-interface discovery, reflection
invocation, a public raw TypeId API or user-supplied type identity methods.

The completed local storage work is a separate predecessor to DC01-DC04.
NC collection migration, extensions to generic interface construction, blanket impls,
derive and Cranelift interface expansion remain separate. Restoring a value from an
already valid interface must not require reopening those projects, adding state
migration or changing the implemented compatible-publication contract.

## Baseline and implementation evidence

Planning baseline: `f5de6109`, inspected on 2026-10-16 with a clean working tree.
No implementation changes or Cargo validation were performed during planning.

| Owner | Evidence and intended use |
| --- | --- |
| [Trait specification](spec/traits.md#planned-downcast) | Already proposes concrete type tests and recovery, explicitly unimplemented. This plan completes the execution contract without treating the sketch as a shipped API. |
| [Interface metadata](../crates/kagari-runtime/src/gc/interfaces.rs) | InterfaceValueSnapshot retains data and receiver-specific parent-view cells; its shared InterfaceDescription retains the concrete type/expression, generic environment and implementation version. Reuse these facts without copying description or receiver caches; interface identity alone is not the payload type. |
| [Snapshot store](../crates/kagari-runtime/src/execution_metadata/interfaces.rs) | Heap wrappers locate snapshots directly; full metadata IDs remain separately checked. Multiple heap aliases and metadata-only roots have distinct lifetimes. Recover through the checked live snapshot, never an obsolete wrapper payload or unrooted slot index. |
| [Interface construction and upcasts](../crates/kagari-runtime/src/objects/interfaces.rs) | Construction and parent views validate payloads and retain supplying metadata. Reuse receiver admission and lifetime rules on extraction, including mutable host liveness checks. |
| [Type compatibility](../crates/kagari-runtime/src/frame/types/compatibility.rs) | TypeView carries lexical provenance, normalizes applied types and compares compatible layout graphs. Audit its exact suitability for extraction; a scoped ID, closed spelling or Rust TypeId alone is insufficient. |
| [Checked storage](../crates/kagari-runtime/src/gc/directory.rs), [field codec](../crates/kagari-runtime/src/gc/fields.rs) | Stable full identities locate typed payloads and packed struct bytes. Downcast returns the same checked payload, not a field copy or arena address; preserve owner/generation and scoped-view exclusion. |
| [Runtime values](../crates/kagari-runtime/src/value.rs) | Value is unrooted transport. Copying its tag/handle neither keeps the payload alive nor establishes a complete semantic type. |
| [HIR method checking](../crates/kagari-hir/src/typeck/body/methods.rs) | Owns member resolution and records checked operations separately from spelling. Resolve intrinsic/member collisions here, not by backend string matching. |
| [Grammar](kagari.ebnf) | Existing call suffixes use `::<T>()`; no new generic-call syntax is needed. |
| [Identity tests](../crates/kagari/src/runtime/tests/language/interface_identity.rs), [host interfaces](../crates/kagari/src/runtime/tests/embedding/host_interfaces/mod.rs), [generic reload](../crates/kagari/src/runtime/tests/embedding/generic_reload.rs) | Existing owners cover sharing, source/artifact routes, host admission and pinned generic operations. Extend distinct missing contracts rather than duplicating smoke suites. |

For example, a Player becomes an Actor interface through a checked implementation
table. Today the interface can invoke Actor methods and retain Player's identity,
but source code cannot recover Player. After this work, downcast validates the
live receiver, compares its concrete type against the caller's resolved Player
contract and returns Some containing the same object. An unrelated Monster
produces None. Neither path searches for a new trait implementation.

## Accepted semantic contract

### Surface and admissible types

- The receiver is an interface value. No automatic boxing of an arbitrary concrete
  receiver is added merely to invoke these operations.
- Both operations take exactly one explicit type argument and no value arguments.
  `is` returns bool; `downcast` returns the ordinary nominal Option<T>.
- T identifies a recoverable concrete value type, including eligible primitives,
  structs, enums, applied collections and registered durable host values. The
  target must satisfy its ordinary type arguments, bounds and visibility rules.
- A generic T or associated projection is supported when existing checked type
  environments can resolve it to an admissible concrete target. DC01 must specify
  how eligibility is proved for shared generic bodies and independently verified
  at executable boundaries; unresolved or forbidden instantiations are rejected.
- A top-level trait interface target is excluded: `is::<OtherTrait>()` is not an
  implementation query, and `downcast::<OtherTrait>()` does not build a new vtable.
  This does not prohibit a concrete type such as Vec<Actor> from containing
  interface-typed members. Existing parent-interface upcasts remain unchanged.
- Scoped host references, borrow tokens and path views cannot be recovered as
  escaping values. Type erasure does not make them valid interface payloads.
  DC01 records the finite target-category table, including function/callable and
  special runtime values, using existing admission contracts rather than silently
  promising every runtime Value variant is supported.

### Matching, evaluation and failures

Matching uses concrete nominal identity, complete ordered type arguments and the
applicable version compatibility contract. It is not field-shape equivalence,
numeric conversion, recursive container conversion or an implicit coercion.

| Hidden concrete type | Target | Result |
| --- | --- | --- |
| Player | Player | true / Some of the original value |
| Player | Monster with identical fields | false / None |
| Vec<i32> | Vec<i64> | false / None |
| i32 | i64 | false / None |

Evaluate the receiver exactly once and preserve completed side effects and trap
order. Neither operation consumes it or mutates the hidden value. `is` does not
narrow the receiver's static type. Prefer a single downcast when the result is
needed; users are not required to perform a preceding type test.

For a valid receiver and target, both operations use the same matching predicate.
A valid mismatch returns false or None. Invalid/foreign/stale handles, revoked
host storage and corrupt executable metadata remain errors, even when the target
would not match. Do not fold an apparently impossible match to false in a way
that skips receiver evaluation or required validation. No user conversion,
comparison or reflection callback participates in matching. Ordinary allocation
failure and cancellation behavior still apply when constructing a result.

### Values, roots and access

Recovery follows [ordinary value semantics](spec/value-semantics.md): structs and
collections retain shared object identity; scalars, strings, tuples and enums
follow their existing value semantics. Copying an aggregate preserves sharing of
its object members. No deep copy, consuming move or Rust-style reference-return
API is introduced.

The interface, its payload and the Option result must remain correctly traced
through allocation, collection, traps and host reentry. A successful result must
retain the necessary type/operation generations after the original interface is
dropped. Recovering a host value retains its checked handle, not a Rust reference;
registration, member access, borrow checks and later liveness checks still apply.

Ordinary read-only collection interfaces hide mutators but do not prevent an
explicit checked recovery of their concrete storage:

```text
val storage = Vec::from([1, 2])
val readable: List<i32> = storage
// readable.downcast::<Vec<i32>>() returns Some(storage).
// The recovered Vec can mutate the same collection through its normal API.
```

This is an accepted language decision, not an inference from the existing rule
that another alias may mutate. Document it explicitly at delivery. A dedicated
wrapper restores only that wrapper, never its internal storage. Host access
restrictions and non-interface readonly adaptations are not lifted by downcast.
No collection-name exception or generic permission matrix is introduced.

### Reload and execution provenance

The accepted direction permits recovery across compatible code publication when
the nominal declaration and applied arguments match and the value satisfies the
existing cross-version admission contract. Do not require exact whole-Program
version equality merely because method bodies changed. Conversely, equal names
or equal local table indices never authorize interpreting an incompatible layout.

Recovery does not migrate objects, rewrite original interface dispatch tables or
redirect an active call to the newest publication. DC01 must trace the recovered
value through field access, concrete method calls, generic operation bindings and
conversion back to an interface, recording which retained version supplies each
operation. Preserve [implemented current-call selection](spec/module-activation.md#live-installations-and-call-selection)
and exact retained computation bodies; do not invent an independent downcast
dispatch policy or bypass compatible-publication validation.

If existing admission lacks evidence required for safe extraction, make the
smallest shared correction with its owner and acceptance cases recorded here.
Do not weaken validation or silently expand this task into reload redesign.
Valid incompatible types yield mismatch; invalid ownership/liveness still traps.

## Ownership and executable handoff

| Layer | Responsibility |
| --- | --- |
| syntax / HIR / analysis | Parse existing call syntax; resolve names, target types and eligibility; record an explicit checked operation; provide diagnostics, completion and target-type navigation. |
| types / contract | Own source-independent admissibility and operand/result contracts, including generic environments and Option identity. No source-analysis dependency. |
| MIR / compiler / bytecode | Carry and verify the checked operation and complete target identity through lowering, serialization and definition remapping. Do not rediscover it from member names. |
| runtime | Own receiver validity, concrete matching, version compatibility, payload recovery and root/metadata lifetime. Both operations share one semantic predicate. |
| VM / native preparation | Execute the checked operation through runtime semantics. Unsupported native bodies select the interpreter before entry; no partial native execution followed by fallback. |

DC01 chooses a bounded opcode/helper representation after inspecting current
contracts. Preserve source-free verification of receiver, target, generic binders
and result shape: malformed artifacts must not forge a successful typed result.
Do not publish raw unchecked extraction as a second API. Retain format/helper ABI
identifiers without routine unpublished-version bumps; invalidate affected local
products and regenerate affected fixtures at coherent checkpoints.

Native interface expansion remains owned by J01-J06. A JIT-enabled test using
interpreter fallback proves selection/semantics, not native downcast coverage.
If the relevant native path is supported when implementation starts, it must
consume the same checked contract and receive actual-native boundary coverage.

## Execution phases

Execute DC01 through DC04 in order. Keep coherent checkpoints buildable; attempt
focused checks at changed architecture boundaries. Any carried failure must name
its reproduction, diagnostic, cause and owning phase in the ledger. Never use
production stubs, disabled checks or a second semantic implementation to bridge
phases. Implementation commits use a `Plan-Phase: DCxx` trailer.

### DC01 — Close the remaining contract decisions

- Recheck the inventory and existing tests against the implementation baseline.
- Specify intrinsic/member name resolution, including a user method named `is`
  or `downcast`, qualified access and diagnostics. Do not silently replace a
  user method with an intrinsic. Record a deterministic, usable resolution rule
  before adding source execution; changing the agreed surface requires an explicit
  scope decision here.
- Freeze receiver/target eligibility, generic proof and associated-projection
  handling. Avoid introducing Any or widening generic construction rules as an incidental dependency.
- Trace cross-version extraction and subsequent calls as described above; settle
  the identity/admission relation and enumerate its meaningful acceptance cases.
- Select checked operation encoding, Option construction, root ownership and the
  minimal shared runtime entrypoint. Record all decisions in this plan.

Acceptance: no unresolved name-resolution, target-admission, ownership or
cross-version call-policy question remains before DC02. This is a design/code
inspection gate, not a claim that executable behavior exists.

### DC02 — Implement verified executable and runtime semantics

- Add source-independent operation contracts, MIR/bytecode validation and codec
  remapping; update affected consumers coherently.
- Implement the shared receiver-validation/matching/recovery path with checked
  generic/version provenance and traced result construction.
- Wire interpreter execution and native support selection. Exercise operations
  through verified executable fixtures before source syntax is enabled.

Acceptance: focused valid/mismatch, malformed-artifact, GC and host-liveness cases
pass without source analysis in the execution path. Existing interface construction
and upcasts retain their assertions and behavior.

### DC03 — Connect source checking, lowering and tooling

- Implement the DC01 resolution rule, explicit target checking and diagnostics;
  lower checked operations without another resolver/type checker in the backend.
- Cover shared generic targets and projections within the agreed eligibility
  rules, parent-interface receivers, user-name collisions and rejected targets.
- Connect completion/navigation to the target type and the intrinsic operation;
  ordinary user methods keep their own identity.
- Add a small runnable example using Option matching and shared object recovery.

Acceptance: focused source-to-artifact execution agrees with DC02; typing failures
are diagnosed before lowering. The example uses existing syntax without relying
on the old `if val` downcast sketch or implicit type narrowing.

### DC04 — Validate composition and publish lasting contracts

- Verify retained results after wrapper release/GC, once-only evaluation and trap
  cleanup, ordinary readonly-to-concrete recovery, host access and revocation,
  generic/version mismatches and compatible reload with pinned calls.
- Retain source-free roundtrip, malformed-program and backend-selection evidence;
  add actual-native coverage only where the backend supports this operation.
- Update traits, value/collection semantics, reflection/runtime identity guidance,
  syntax coverage and architecture at their existing owners. Remove obsolete
  planned examples; keep one authoritative contract per concern.
- Move unresolved out-of-scope work and pending CI acceptance to roadmap/review,
  then retire this plan once implementation and required local checks complete.
  Git history retains its decisions and phase ledger.

Acceptance: the bounded source/runtime/artifact behavior is implemented, selected
local tests and implementation checks pass, and remaining CI scope is explicitly
reported without claiming it passed.

## Validation policy and selection

Use [repository verification policy](../AGENTS.md#verification-and-tooling).
Select affected contract tests and a small adjacent regression set, not a local
workspace sweep. Reuse the identity, host-interface and generic-reload owners
linked above, HIR trait tests and existing executable verifier tests. Add cases
only for uncovered rules; backend, source-free and ownership boundaries remain
distinct evidence even when they share a script fixture.

| Boundary | Required evidence |
| --- | --- |
| Source typing/tooling | Correct bool/Option types; explicit arity; same-named user members; concrete/generic/projection targets; top-level interface and scoped-target rejection; type navigation. |
| Runtime values | Match/mismatch, same-shaped distinct nominal types, ordered generic arguments, primitive/value payloads, parent views and preserved shared identity. |
| Evaluation/lifetime | Once-only receiver effects, unchanged trap order, roots during result allocation, result surviving wrapper release, and cleanup on failure. |
| Access/host | Ordinary List-to-Vec recovery, wrapper non-transparency, durable registered host recovery, revoked/foreign handles and preserved scoped borrow/member checks. |
| Versions | Compatible body reload, incompatible concrete contracts, retained generic provenance, old interface dispatch and subsequent recovered-value calls. |
| Executable products | Source-free serialized execution, definition remapping, forged target/result/generic evidence rejection and pre-entry backend selection. |

At each phase, record exact selected commands, rationale, outcomes and skipped
cases in the ledger. Test names for new operations are established with their
owning test modules; planning does not claim that such tests already exist. Use
package/target/test-name filters, `--no-fail-fast` for multiple selected targets,
workspace profiles, default target and default build parallelism.

Local implementation acceptance also requires:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Review changed handwritten Rust ownership/imports/re-exports and effective LOC
before checkpoints. No blanket exceptions. Complete workspace tests and the
feature/backend/platform matrix belong to CI, including final acceptance.
Documentation-only planning requires content/link/diff checks, not Cargo builds.
No performance claim or speculative matching cache is part of this task.

## Progress ledger

- [x] Planning: accepted semantics, finite scope, owner inventory and DC01-DC04
  recorded on 2026-10-16; roadmap and specification cross-links added.
- [x] Refresh the storage inventory after locally accepted OM, 2026-10-18;
  no downcast implementation or design-gate waiver is implied.
- [ ] DC01: resolution, target eligibility and cross-version call audit.
- [ ] DC02: executable contracts, runtime and interpreter.
- [ ] DC03: source and tooling integration.
- [ ] DC04: composition acceptance and documentation handoff.

Planning validation: content review, 157 local Markdown link/anchor checks across
the four changed documents, phase/checklist consistency and code-fence checks
passed. Diff whitespace checks passed. No known intermediate build failure was
established; implementation and its focused local/CI checks are unrun. Remaining
design decisions belong to DC01 above, not a separate queue.
