# Generic bound interface construction execution plan

Status: active on 2026-10-21; BI01-BI02 are complete; BI03 is in progress. This plan owns
SA22 phases BI01-BI06, implementation decisions, acceptance and the progress ledger.
The [roadmap](implementation-roadmap.md#review-follow-up-order) owns sequencing.
SA11 is deferred by the user. The user authorized goal execution and phase commits.

## Outcome and scope

Allow a value whose generic bounds prove a compatible trait application to become
an interface value at an existing coercion site. The current
[SA22 limitation](review.md#sa22-generic-bounds-do-not-supply-interface-construction-witnesses)
rejects such conversions even though bound method calls work.

Proposed executable example:

```kagari
trait Read { fn read(self) -> i32; }
struct Number { val value: i32 }
impl Read for Number { fn read(self) -> i32 { self.value } }
fn erase<T: Read>(value: T) -> Read { value }
fn main() -> i32 { erase(Number { value: 42 }).read() }
```

The result must be 42 through source compilation and a decoded artifact. The
conversion evaluates value once and preserves its payload and selected implementation.
A shared generic method must support the same conversion while T remains symbolic
in its executable body. A monomorphized-only implementation does not complete SA22.

Include ordinary static specialization, shared generic methods/defaults/overrides,
forwarding through generic calls and retained closures, scalar and managed receivers,
applicable installed host implementations, inherited bounds and associated outputs.
Include a projection receiver when the existing bound solver proves its requirement;
this does not add inverse projection inference or new trait solving rules.

Exclude Any/downcasts, universal object bounds, new identity/comparison permissions,
blanket or overlapping impls, covariance, changes to implicit conversion sites,
public generic free-function exports, broader borrowed host values, SA11/SA23,
general incremental compilation, GC redesign and native backend expansion.
No performance target or allocation-free conversion guarantee is asserted.

## Semantic contract

| Area | Proposed rule |
| --- | --- |
| Target selection | Reuse an explicit annotation, assignment destination, parameter, return or field type. Existing if/match propagation applies. Do not infer a common trait or insert nested container conversions. |
| Bound evidence | The source's declared bounds and existing implication/parent rules must prove the exact destination application. Same spelling is not identity. Insufficient, ambiguous or incompatible evidence remains an error. |
| Dynamic compatibility | Keep current interface eligibility and associated-output checks. A static-only trait does not become an interface because it is a bound. |
| Arguments and outputs | Trait arguments remain invariant. Retain required associated outputs and checked normalization equalities. Missing bindings or mismatches cannot be filled by runtime guessing. |
| Empty traits | A methodless trait with a proven implementation can be constructed. Construction evidence exists independently of method-operation lists. |
| Receiver categories | Preserve scalar, managed object, nominal interface and currently permitted host categories. A bound does not imply an object receiver. Existing interface copy/upcast behavior applies when the supplied type is already an interface and satisfies existing bound rules. |
| Generics | Binder identity is owner plus position. Own method/local-function binders, projection receivers and forwarded constraints retain that identity through substitution. |
| Lifetime | An escaped interface retains its payload, selected implementation, associated type provenance and necessary executable dependencies. Retained callers/closures keep their supplied generations after reload. |
| Failures | Missing or forged executable evidence is rejected by verification/admission. Ordinary GC, cancellation, depth limits, host borrow checks and trap cleanup remain active. |

Examples of additional acceptance boundaries:

```text
fn erase_empty<T: Marker>(value: T) -> Marker { value }
fn erase_parent<T: Child>(value: T) -> Parent { value } // Child declares Parent.
fn erase_reader<T: Reader<Item = i32>>(value: T) -> Reader<Item = i32> { value }
fn forward<T: Read>(value: T) -> Read { erase(value) }
```

These are contextual sketches, requiring the named declarations. An unbounded T,
an unrelated bound, incompatible trait arguments, or a static-only destination
must still fail. An interface value gains no comparison or identity capability.

## Baseline and root cause

Planning baseline: b4bcbd88, inspected with a clean working tree on 2026-10-21.
This is a missing end-to-end evidence path, rather than a missing parser form.

| Owner | Current behavior and change boundary |
| --- | --- |
| [HIR coercion](../crates/kagari-hir/src/typeck/body/methods.rs) | apply_interface_coercion accepts an existing interface upcast, a selected declared implementation or a concrete intrinsic implementation. Generic/projection sources do not obtain construction evidence from their bounds. Add a checked bound-based alternative without weakening the existing gates. |
| [Checked HIR facts](../crates/kagari-hir/src/typeck/table.rs) | ResolvedInterfaceImplementation contains Upcast, Declared and Intrinsic. Record the source binder/type, requested applied interface and proof origin for a bound-based conversion; update identity visitors/remapping and body-query reuse. |
| [Compiler conversion lowering](../crates/kagari-compiler/src/source/lower/expr.rs) | Emits UpcastInterface or MakeInterface with a selected implementation and arguments. Consume checked bound evidence; specialize it when concrete, or retain a construction requirement when shared. |
| [Shared generic preparation](../crates/kagari-compiler/src/source/lower/instances/shared.rs) | bound_operations enumerates methods under bounds, including parent and associated constraints. A methodless trait emits no method operation. Do not fake a method to represent interface construction. |
| [Construction contract](../crates/kagari-contract/src/callable/construction.rs) | ConstructionBody/ConstructionStep and closed_constructions already enumerate reachable checked applications through calls and dispatch without runtime trait search. Extend this owner for bound-selected construction and forwarding. |
| [Method witnesses](../crates/kagari-contract/src/callable/witness.rs) | OperationWitness carries selected/shared/forwarded callable requirements. Preserve its method meaning; reuse selected implementation proof components where appropriate, without creating a second method solver. |
| [Artifact coverage](../crates/kagari-bytecode/src/program/construction.rs) | Rechecks closed construction coverage from executable slots. Extend source-free checks for selected and forwarded construction evidence, including malformed inputs. |
| [Shared runtime frames](../crates/kagari-runtime/src/frame/shared.rs) | Prepare type arguments, operations and the construction scope in EnvironmentRecord. Extend that owner for exact construction selections; preserve preparation identity and cleanup. |
| [Construction scope](../crates/kagari-runtime/src/frame/types/construction.rs) | Retains a finite set of supplying programs, never the current module-store search space. This scope alone does not prove a symbolic type implements a target trait. |
| [Interface construction](../crates/kagari-runtime/src/objects/interfaces.rs) | Builds checked interface snapshots with shared descriptions, receiver data and associated arguments. Both concrete and bound-selected paths must converge here. |

The existing rejection is in
[dynamic_standard_traits_and_caller_known_outputs_use_structural_rules](../crates/kagari-hir/src/tests/typeck/traits.rs).
Convert its bound-only Debug case to positive coverage only when the downstream
executable path exists; keep the neighboring static-only trait rejections.

## Proposed evidence flow

Use a construction requirement distinct from a method requirement. Conceptually,
InterfaceConstructionRequirement contains a source type and an applied destination
interface, both in the declaring binder scope. Exact Rust names are settled in
BI01; no public raw slot or user-controlled witness API is required.

1. HIR proves that the source bounds entail the target and records the conversion.
   It preserves evidence origin, parent adaptation and output equalities instead
   of merely returning the target type.
2. Source preparation selects the actual declared/intrinsic implementation at a
   closed application, using the existing implementation-selection owner. For an
   already compatible interface, select the existing copy/upcast operation.
   A symbolic caller forwards its checked construction evidence.
3. Shared executable contracts retain construction requirements and their supplied
   selections alongside the existing type/method environment. A selection retains
   the implementation application, required operations and associated type arguments.
   No arbitrary method from the trait is used as a stand-in for this capability.
4. The existing construction graph enumerates reachable closed requirements through
   direct/shared/interface calls. It follows explicit selected/forwarded evidence;
   the source-free verifier checks the same contract without discovering impls.
5. Runtime resolves an already verified selection in the retained generic environment,
   roots the input, and invokes the existing interface construction/upcast owner.
   The resulting value retains the supplying generation, not the latest module version.

For erase(Number { value: 42 }), the selected witness describes Number's Read
implementation and its read target. For an i32 Debug conversion, it identifies the
existing intrinsic adapter. A shared erase<T> consumes either selection without
assuming T is a heap object or performing runtime trait resolution. A Marker
selection contains valid construction/type evidence even with zero method entries.

### Shared method declaration boundary

A caller can know a generic method declaration without depending on one chosen
default/override body. Construction capabilities must therefore follow its checked
bound contract, including parent and associated-output requirements. Derive a
canonical capability set from eligible declared bounds; materialize closed demands
through the existing construction graph. Do not inspect one method body to decide
which proofs another implementation is allowed to require.

Bounds for traits that cannot form interface values must continue to support their
ordinary static operations. Do not reject otherwise valid calls because an unused
bound lacks dynamic compatibility. BI01 must fix how output projections are carried
in capability descriptors and how eligible capabilities are selected without an
unbounded expansion of associated types or method applications.

### Ownership and executable representation

Extend the existing construction contracts and EnvironmentRecord.constructions
responsibility rather than building a parallel dictionary/boxing service. Keep
finite program retention and exact selection lookup distinguishable: possession
of a retained program is not an implementation proof. Selected type arguments
retain their originating identity/generation information.

Prefer one interface-construction instruction family whose target distinguishes
a declared application from a checked bound selection. Update its logical types,
operand/identity visitors, physical lowering, effects, wire encoding, verifier,
disassembly and backend capability checks together. Existing concrete construction
and parent adaptation remain the common execution machinery. If the current
instruction shape cannot represent this cleanly, BI01 records the exact bounded
replacement before editing; no permanent duplicate semantics or fake implementation ID.

Extend existing preparation/cache keys with the selected evidence's scope and
identity wherever it changes the prepared result. Keep the same owner/lifetime,
validate before publication, and retain old snapshots. Do not add global mutable
implementation caches or runtime name-based lookup. Bound traversal, metadata
expansion and graph walks remain bounded and cancellation-aware.

Keep unpublished artifact/runtime identifiers unless a real compatibility
commitment requires a bump. Regenerate affected development fixtures at coherent
checkpoints; retain source-free and forged-input coverage.

### BI01 representation decisions

- `ConstructionRequirement` lives with executable construction contracts and has
  `receiver: Ty` and `interface: NominalTy`. It has no method or slot ID.
- `ConstructionWitness` pairs a requirement with `ConstructionSelection`:
  `Declared` retains a `ConcreteFunctionIdentity` and the selected implementation's
  receiver-bound operation/construction dependencies; `Interface` retains the
  existing receiver's checked copy/upcast; `Forward` consumes a capability from
  the caller's declared bounds. Intrinsic adapters use their existing synthesized
  implementation tables and therefore use `Declared` too.
- Shared calls and interface-call contracts carry a separate construction list.
  Shared method selections carry the dependencies of their selected receiver
  implementation. No `OperationWitness` variant represents a methodless proof.
  Wire traversal must bound the combined dependency graph, including alternating
  operation and construction nodes, before recursive conversion/validation.
- Requirements come from declared bounds, their parent closure and immediate
  non-generic associated-type bounds, matching the existing operation expansion
  boundary. Fill an unspecified non-generic associated output with its explicit
  receiver projection before testing dynamic eligibility. Do not recursively expand
  associated bounds. Sort and deduplicate by semantic receiver/application identity;
  normalization at a closed call can merge equivalent requirements.
- Eligibility uses `dynamic_interface_valid` with the caller's binder scope.
  Static-only bounds supply operations but no construction capability. A requested
  interface must retain all required output equalities; a forwarded proof may only
  narrow a declared compatible capability through the existing parent rules.
- MIR `MakeInterface` will own a target enum: declared implementation/application
  or bound construction requirement. Bytecode uses the corresponding linked table
  target or requirement. Keep one instruction's effects, operands and runtime
  publication path; `UpcastInterface` remains the existing direct upcast operation.
- Linked validation checks the exact selected table, substituted receiver, target
  application, associated outputs and receiver-bound dependencies. `Forward` must
  be implied by the enclosing `GenericBody.bounds`. Declaration-derived call lists
  must be complete and unique, including empty traits. Program construction graph
  coverage includes every selected application; retained programs alone never
  establish the proof.
- Runtime stores selected capabilities under `EnvironmentRecord.constructions`,
  alongside its existing finite program scope. Entries retain the supplying module,
  type-argument provenance and receiver-bound environment. Forwarding retains that
  selection. Existing preparation keys must include construction evidence through
  the call contract and environment identity; execution never searches current impls.

Integration inventory: HIR table mapping/references and coercion lowering;
shared-call, interface-call and shared-method witness creation, mapping, codecs,
admission and cache keys; MIR/bytecode instruction operands, rewriting, identity
mapping, wire records, semantic and physical verification, application validation,
construction graph extraction, program dependencies, disassembly and native
capability rejection; VM dispatch and runtime snapshot publication/metadata tracing.
The existing `required_operations` associated-bound expansion will become a shared
declaration helper rather than being copied into a second solver.

Selected local filters start with
`cargo test -p kagari-hir dynamic_standard_traits_and_caller_known_outputs_use_structural_rules`
for the unchanged rejection boundary, construction contract tests under
`cargo test -p kagari-contract callable::construction`, compiler interface filters
and the SDK's existing `generic_reload` owner. Later phases record the precise
selected test names and outcomes; none of these authorize a package/workspace sweep.

## Mature implementation references

Consulted on 2026-10-21; these are living documents, not pinned release claims.

- [Rust monomorphization](https://rustc-dev-guide.rust-lang.org/backend/monomorph.html)
  selects reachable concrete generic instances. Reuse that separation for Kagari's
  already-specialized path; making all shared methods monomorphic would discard
  Kagari's existing execution model and is not the proposed solution.
- [Rust coercion sites](https://doc.rust-lang.org/reference/type-coercions.html)
  provide a reference for conversions driven by explicit type contexts. Preserve
  Kagari's existing sites and eligibility; do not copy Rust borrowing or dyn syntax.
- [Swift generic argument metadata](https://github.com/swiftlang/swift/blob/main/docs/ABI/TypeMetadata.rst#generic-argument-vector)
  carries type metadata and protocol conformance witnesses separately. Adopt the
  principle that a bound needs executable conformance evidence, including for
  shared code. Kagari uses its own verified artifacts, precise GC, scoped host
  access and generation pinning; Swift metadata layout and lifetime rules are not
  imported. The existing Kagari construction owner is the integration point.

## Phases and commits

Execution is authorized. Each phase is a coherent Conventional Commit
with Plan-Phase: BI0N. Mark exposed breaking Rust model changes with ! and explain
consumer impact. Keep intermediate checkpoints buildable, with staged rejection
at unsupported executable boundaries until BI04 completes both execution paths.

| Phase | Work and acceptance | Suggested commit |
| --- | --- | --- |
| BI01 | Freeze capability/selection representation, declaration-derived shared requirements, associated-output handling and source-free proof rules. Reuse the current rejection and inventory the complete instruction/contract consumers. Record concrete cases for static, shared, empty, projected and parent bounds. | docs(types): Define generic interface construction contracts |
| BI02 | Implement portable construction requirements/selections, substitution/remapping, canonical ordering, bounded graph propagation, codecs and structural/linked validation. Source acceptance remains gated. Test malformed witnesses and source-free round trips at the contract owner. | feat(contract)!: Carry generic interface construction evidence |
| BI03 | Record bound-based HIR coercions and integrate specialization/call preparation, including associated outputs and declaration-based shared forwarding. Add construction fields to call/selected-method contracts together with their producers, mapping and unified evidence codec. Preserve query/navigation/reuse ownership. Keep an explicit compiler rejection for any shared path not executable yet; no valid checked product may silently omit the conversion. | feat(hir)!: Check bound-derived interface coercions |
| BI04 | Complete MIR/bytecode conversion, verified call admission and runtime construction/environment integration through the existing interface owner. Handle scalar/object/host categories, empty traits, parent adaptation and imported generic defaults. Remove staged gates. Static and genuinely shared calls must execute from decoded artifacts. | feat(runtime)!: Construct interfaces from generic evidence |
| BI05 | Validate retained interfaces/closures across reload, supplying-generation provenance, forced GC, host reentry where supported, cancellation/depth/trap cleanup and forwarding across modules. Cover forged/missing/stale evidence and unchanged ordinary conversions. | test(language): Verify generic interface construction lifetimes |
| BI06 | Resolve carried errors; finish focused acceptance and required quality checks. Update trait/architecture/host boundaries and an existing example, remove SA22's resolved limitation and retire this plan. Keep pending exhaustive CI explicit. | docs(traits): Publish generic interface construction semantics |

If changes at an executable model boundary cannot be split without fake records
or unchecked paths, combine its directly dependent consumers in the owning phase.
Record a real build failure, diagnostic, cause and owning follow-up before moving
on; do not repeatedly run unchanged known failures or mark phase scope as acceptance.
No implementation phase may leave a production todo or disabled validator.

## Validation

Tests belong at current contract owners, with new cases only for distinct uncovered
semantics. BI01 fixes exact filters after selecting cases; these are the intended
boundaries, not a request for package-wide local sweeps.

| Boundary | Selected evidence |
| --- | --- |
| HIR | Existing traits/coercion tests: direct/where/inherited bounds, empty trait, generic arguments, known associated outputs and bounded projections; missing/wrong/static-only targets; return/argument/field/branch contexts. Query an edited bound/output and retain old navigation. |
| Executable contracts | Existing callable/construction and MIR/bytecode verification tests: wrong binder/receiver/target/implementation, missing outputs or parent proof, missing dispatch coverage, forged forwarding, excessive/cyclic expansion and cancellation. |
| Static and shared execution | Existing compiler interface/source-program tests plus SDK generic/interface tests. Exercise an interface-dispatched generic method with method-local T, both default and override bodies; a free-function-only example cannot prove shared support. |
| Receiver categories | Scalar intrinsic and declared implementations, managed receivers, already-compatible interfaces and currently legal host values. No raw host references escape. |
| Source-free products | MIR/KBC encode-decode-verify-load-execute with no HIR at runtime. Alter one selected construction fact and assert the correct verifier boundary rejects it. |
| Lifecycle | Extend generic_reload and existing interface/GC/error owners: retain old interface and closure, reload provider/caller, invoke old/new values, collect during construction, then release roots and confirm cleanup. Cover future suspension only through existing supported environment retention, without new async capabilities. |
| Regression set | One existing concrete generic-receiver conversion, ordinary parent upcast and generic bound-method dispatch case, plus affected source-free/host boundaries. Leave the full matrix to CI. |

At each implementation checkpoint run the small selected set plus:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Review production imports/re-exports, definition remapping, visibility, effective
LOC and module responsibilities before the checkpoint. Split cohesive owners when
needed; avoid widening visibility or adding forwarding modules to shorten paths.
Full workspace tests and the complete feature/backend/platform matrix remain
CI-only, including final acceptance. Unsupported native conversion paths retain
explicit capability rejection/pre-entry fallback; do not claim native coverage
from interpreter fallback. Planning-only changes need content/link/diff checks.

## Progress ledger

- Baseline: b4bcbd88; working tree clean before planning.
- User authorized SA22 goal execution and phase commits after accepting the proposal;
  SA11 remains deferred.
- Architecture inspection found existing construction graph, finite retained-program
  scope, shared method-operation witnesses and interface snapshot owners to extend.
- Material decision: construction evidence is separate from method evidence;
  empty traits and shared declaration contracts are required acceptance boundaries.
- BI01: fixed the representation, declaration-derived capability boundary,
  associated projections and source-free admission rules above. Inspected both
  MIR and bytecode construction graph consumers and the existing linked proof owner.
- BI01 validation: the selected HIR structural-rule test passed (1 test), preserving
  the current rejection until executable support lands. Documentation diff checked;
  no implementation or full-workspace validation claimed.
- BI02 integration split: portable evidence and graph support land before changing
  executable call records. Call-record fields and their producers move together in
  BI03; runtime environment storage moves with its actual consumers in BI04. This
  preserves buildable checkpoints without inventing placeholder selections. Final
  shared/source-free/lifecycle scope is unchanged.
- BI02: method/constructor enumeration shares one immediate associated
  bound expansion owner. Construction requirements include non-generic output
  projections and skip static-only traits; empty traits retain a capability.
  Explicit table selections, interface views and forwarding have independent
  structural/linked checks. Construction graph steps retain selected applications.
- Codec migration checkpoint: BI02 flat construction records can contain existing
  flat operation records (only construction-to-operation edges exist). Before BI03
  adds operation-to-construction dependencies, replace these nested envelopes with
  one flat heterogeneous evidence tree and a combined node/depth budget. Never
  introduce mutually recursive wire deserialization.
- BI03: call/interface/selected-method records now carry construction
  capabilities. Identity mapping, portable admission and disassembly include them.
  Replaced the BI02 codec bridge with one flat heterogeneous evidence tree and a
  shared traversal budget; the two migrated codec test groups passed. The old
  operation/construction tree envelopes have been removed, not retained as readers.
- BI03 source preparation derives method and constructor capabilities in one pass
  through the existing HIR bound expansion. Shared selected implementations retain
  both kinds of receiver dependency. The MIR/bytecode graph now follows selected
  evidence through both kinds of dependency and native import adapters.
- BI03 transient compile failures: the native-import codec still referenced the
  replaced operation record, then BoundEvidence fields were scoped too narrowly for
  expression lowering. Both owners were updated; `cargo check -p kagari-compiler`
  passed afterward. No carried build error from these failures remains.
- BI03 HIR now records the declared application that proves a generic/projection
  coercion. Static lowering selects the existing declared/intrinsic/upcast path.
  A symbolic forwarded conversion still reports an explicit compiler unsupported
  boundary until BI04 introduces the executable target. The expanded HIR structural
  rule test passed (1 selected test), as did the static compiler specialization
  case (1 selected test). Shared execution remains a BI04 boundary.
- BI02 first focused test run found two test-fixture compile errors (an obsolete
  AssociatedTypeDef field and a nonexistent DefinitionDecoding.context). Both were
  corrected at their owners. All five selected construction tests passed, covering
  methodless/forged/forwarded evidence, known/projected outputs and static-only
  exclusion, parent/immediate-associated expansion, closed call graph substitution,
  cancellation/limits, and file-local encode/decode round trips.
- BI02 validation: `cargo test -p kagari-contract callable::construction` passed
  (5 tests); `forwarded_parameters_require_the_callers_scope_and_bound_evidence`
  passed (1 test); `partial_associated_outputs_require_scoped_receiver_evidence`
  passed (1 test). Workspace Clippy with all targets and denied warnings passed.
  The structure checker found one missing definition separator, repaired without
  semantic changes; its rerun passed (1416 Rust files, no exceptions). Formatting
  passed; final diff/line-ending checks passed. Full workspace tests and the feature/backend/platform matrix were not run.
- Planning validation: documentation content/link checks and git diff --check;
  no implementation tests, builds or performance measurements required.
- Carried implementation errors: none. BI04 owns the explicit shared-conversion gate.
- Structural debt: no new exception proposed; review changed responsibilities at
  each implementation checkpoint. SA11 and broader caching work remain separate.
- [x] BI01 Contracts and integration inventory.
- [x] BI02 Portable construction evidence and validation.
- [x] BI03 HIR proof and call preparation.
- [x] BI04 Executable shared construction.
- [ ] BI05 Lifecycle and source-free acceptance.
- [ ] BI06 Durable documentation and closure.

- BI03 focused contract validation passed: heterogeneous codec (2 tests),
  construction semantics/graph (4 tests), and scoped shared-call bound evidence
  (1 test). Mixed operation/constructor depth and node-kind rejection are covered;
  empty-bound missing/duplicate capabilities are rejected independently of methods.
- BI03 SDK integration found `InvalidGraph` for the two selected shared Fn tests.
  Root cause: intrinsic interface bridges are intentionally excluded from ordinary
  implementation search, but exact construction selections must retain them.
  Proof preparation now partitions bridges into dependency-scoped construction-only
  records; bridge selections still require existing conformance and executable
  bridge validation. Both selected SDK tests passed after the repair.

- BI03 acceptance: the two SDK tests were
  `shared_function_call_arguments_and_operations_are_verified_without_source` and
  `shared_helpers_forward_bounds_and_retain_nested_types`. The methodless contract
  case also passed with a forged intrinsic bridge, confirming retained bridges do
  not invent conformance. Workspace Clippy passed with all targets/denied warnings;
  structure (1418 Rust files, no exceptions), formatting and diff checks passed.
  New shared source behavior remains gated until BI04; exhaustive CI is pending.
- BI03 structural review: one heterogeneous codec/traversal owns mixed evidence
  limits; no compatibility reader, re-export, visibility exception or new cache.
  HIR proof selection and compiler capability preparation retain their existing
  owners. Intrinsic bridges are partitioned from ordinary implementation lookup.

- BI04 in progress: MakeInterface now uses an explicit declared/bound target in
  MIR and bytecode. Both wire owners, identity traversal and type/application
  admission are being migrated together. The first scoped compiler check found
  one missing disassembly-descriptor arm for a bound target; that target has no
  concrete table descriptor to collect. BI04 owns the remaining executable build
  integration and runtime environment consumers; no phase acceptance claimed.

- BI04 scoped integration checks: `cargo check -p kagari-compiler` and
  `cargo check -p kagari-vm` passed after the target/consumer migration.
  Runtime selections now belong to ConstructionScope alongside its existing
  finite program inventory. Keys include receiver/target type provenance and
  selected table/dependency-environment identity; existing application/description
  caches use that complete key. No new cache or runtime trait search was added.
- BI04 associated outputs: construction selections supply immutable type-only
  facts even for methodless traits. Output TypeArguments preserve lexical layout
  provenance through projections; selected environments remain explicit traced
  metadata edges. Shared calls, method applications, adapters and supported host
  interface applications bind the same construction owner.
- BI04 focused acceptance in progress: one SDK scenario combines shared defaults
  and overrides, forwarded free helpers, retained closures, scalar/managed/already
  boxed receivers, inherited bounds and a methodless associated-type provider.
  It uses existing source, decoded-artifact and JIT/fallback routes under forced
  GC. Native execution of the new conversion itself is not claimed.

- BI04 scenario correction: custom interface values do not currently satisfy
  same-trait generic bounds in HIR, despite portable view proofs supporting them.
  The plan preserves existing bound rules, so this broader conformance expansion
  remains deferred. Interface reuse is tested through supported Iterator bounds.
  The projection scenario exposed missing post-inference normalization in generic
  method argument checking; the fix reuses AggregateCatalog normalization exactly
  as ordinary function calls already do, without new inference or solving rules.
- BI04 structural review split interface construction type flow out of access.rs
  after the owner exceeded 1200 effective LOC. Artifact aggregate accounting now
  traverses both evidence kinds through their shared child-edge owner.

- BI04 selected SDK construction tests passed (3 tests): default/override shared
  execution, helper/closure forwarding, generic implementation dependencies,
  methodless and associated bounds, intrinsic Fn and existing Iterator views;
  five source-free instruction forgeries reject missing bounds, substituted
  inputs, invalid receivers/binders and malformed targets.
- BI04 repaired two integration defects exposed by these scenarios: release the
  VM frame borrow before interface allocation; normalize closed associated
  projections before testing dynamic eligibility. The latter previously dropped
  a methodless Source capability after applying its concrete receiver. Portable
  capability enumeration now receives the existing ProofCatalog normalization
  owner. A temporary closure-annotation compile error and misplaced test delimiter
  were corrected; the selected tests subsequently passed.
- BI04 structural review: extracted interface coercion lowering from the general
  expression dispatcher, merged changed same-owner imports, and made Forward
  consult only the original caller capability scope. Mixed evidence traversal is
  public only for the artifact budget consumer; there are no new re-exports or
  structure exceptions. Existing calls/allocation and instruction polling retain
  execution-policy checks; bounded metadata preparation adds no execution loop.
- BI04 quality checks so far: workspace Clippy with all targets/denied warnings
  passed after removing a redundant field initializer; structure passed (1422 Rust
  files, zero exceptions). Focused regression checks are in progress.

- BI04 focused regression passed: two compiler interface/table lowering cases,
  four portable construction cases, and the retained associated-type snapshot
  ownership case. After extracting lowering and tightening MIR receiver/result
  semantic facts, all five selected SDK cases passed (construction group plus
  forwarded helper and generic parent-view regressions). No full package/workspace
  test sweep was run. BI05 owns durable source-free fixture refresh and lifetimes.

- BI04 complete: final workspace Clippy, formatting, structure and diff checks
  passed; no carried failures remain. The explicit shared-conversion gate is gone.
  Instruction models and internal artifact shapes changed without an unpublished
  ABI/version bump. Full workspace/backend/feature/platform acceptance remains CI-only.
