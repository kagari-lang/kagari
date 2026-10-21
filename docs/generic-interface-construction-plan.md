# Generic bound interface construction execution plan

Status: proposed on 2026-10-21; implementation has not started. This plan owns
SA22 phases BI01-BI06, implementation decisions, acceptance and the progress ledger.
The [roadmap](implementation-roadmap.md#review-follow-up-order) owns sequencing.
SA11 is deferred by the user; preparing this plan does not activate a goal.

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

Execute only after authorization. Each phase is a coherent Conventional Commit
with Plan-Phase: BI0N. Mark exposed breaking Rust model changes with ! and explain
consumer impact. Keep intermediate checkpoints buildable, with staged rejection
at unsupported executable boundaries until BI04 completes both execution paths.

| Phase | Work and acceptance | Suggested commit |
| --- | --- | --- |
| BI01 | Freeze capability/selection representation, declaration-derived shared requirements, associated-output handling and source-free proof rules. Reuse the current rejection and inventory the complete instruction/contract consumers. Record concrete cases for static, shared, empty, projected and parent bounds. | docs(types): Define generic interface construction contracts |
| BI02 | Implement portable construction requirements/selections, substitution/remapping, canonical ordering, bounded graph propagation and structural/linked validation. Extend codecs and existing construction environments coherently; source acceptance remains gated. Test malformed witnesses and source-free round trips at the contract owner. | feat(contract)!: Carry generic interface construction evidence |
| BI03 | Record bound-based HIR coercions and integrate specialization/call preparation, including associated outputs and declaration-based shared forwarding. Preserve query/navigation/reuse ownership. Keep an explicit compiler rejection for any shared path not executable yet; no valid checked product may silently omit the conversion. | feat(hir): Check bound-derived interface coercions |
| BI04 | Complete MIR/bytecode conversion, verified call admission and runtime construction through the existing interface owner. Handle scalar/object/host categories, empty traits, parent adaptation and imported generic defaults. Remove staged gates. Static and genuinely shared calls must execute from decoded artifacts. | feat(runtime): Construct interfaces from generic evidence |
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
- User requested an SA22 proposal and deferred SA11. No goal or implementation
  activation has been requested for this plan.
- Architecture inspection found existing construction graph, finite retained-program
  scope, shared method-operation witnesses and interface snapshot owners to extend.
- Material decision: construction evidence is separate from method evidence;
  empty traits and shared declaration contracts are required acceptance boundaries.
- Open BI01 details: concrete requirement/selection fields and instruction target
  representation; canonical associated-output capability descriptors and admission
  rules for shared declarations. These are bounded design gates, not permission
  to omit shared execution or weaken interface eligibility.
- Planning validation: documentation content/link checks and git diff --check;
  no implementation tests, builds or performance measurements required.
- Carried implementation errors: none; implementation not started.
- Structural debt: no new exception proposed; review changed responsibilities at
  each implementation checkpoint. SA11 and broader caching work remain separate.
- [ ] BI01 Contracts and integration inventory.
- [ ] BI02 Portable construction evidence and validation.
- [ ] BI03 HIR proof and call preparation.
- [ ] BI04 Executable shared construction.
- [ ] BI05 Lifecycle and source-free acceptance.
- [ ] BI06 Durable documentation and closure.
