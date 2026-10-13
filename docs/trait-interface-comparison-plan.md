# Trait interfaces and comparison execution plan

Status: implementation activated on 2026-10-13; TI01-TI03 are complete; TI04 is next.
This plan owns the finite implementation sequence, acceptance, carried errors
and progress ledger for SA25/SA26 and the affected SA23/SA24 cases. The
[roadmap](implementation-roadmap.md#trait-interfaces-and-comparison) owns queue
placement. Implementation baseline: `5dc34273`; unrelated queued work stays
outside this execution.

## Outcome and scope

Make interface conversion deliberate, dynamic eligibility structural, equality
dependent on proven PartialEq, and identity comparison available only through
statically known object types. Replace the old paths coherently across source
checking, declarations, executable contracts, lowering, runtime and consumers.

The specifications own the accepted language decisions:

| Decision | Required outcome | Semantic owner |
| --- | --- | --- |
| Interface conversion | Use annotations, destinations and declared parameter/return/field types. Context-free if/match never search for a common trait/parent or use a sibling interface as a conversion target. | [Conversion contexts](spec/traits.md#accepted-interface-conversion-context) |
| Dynamic eligibility | Check applied arguments, ordinary associated outputs, complete member signatures/bounds and every parent. One incompatible member rejects the entire interface application, while static use remains legal. | [Compatibility rules](spec/traits.md#interface-compatibility-rules) |
| Member limits | Retain dynamic generic methods; reject unresolved non-receiver Self. No-receiver functions, constants and GATs remain static-only; no per-member opt-out. Standard traits follow the same structural checks. | [Compatibility rules](spec/traits.md#interface-compatibility-rules) |
| Equality | Require statically proven PartialEq for ==, !=, method and generic comparisons. No automatic identity fallback. User Struct/enum comparison is explicit; Array gains conditional content comparison. | [Accepted comparisons](spec/value-semantics.md#accepted-comparison-design) |
| Collections | Keep conditional concrete content protocols, content helpers and interface Debug. Remove automatic receiver PartialEq forwarding from all six collection interfaces and default identity Eq/Hash. | [Collection contracts](spec/collection-access.md#content-protocols-and-helpers) |
| Identity | Both operands must have known identity-bearing object types; different concrete object types are allowed. Reject interfaces, values, functions/closures and bare generics with unknown object category. No Object type/bound. | [Identity eligibility](spec/value-semantics.md#identity-requires-statically-known-object-operands) |

For example, `val x: Read = if flag { A {} } else { B {} };` converts each
selected branch to the requested interface. Without the annotation or another
external target, differing branch types are rejected. An already constructed
Read value can still receive dynamic method calls, but it cannot use identity
operators. Its hidden receiver's PartialEq does not supply Read-level equality.
Two Vec values retain content equality when their element bound holds and
identity comparison independently of that bound.

Preserve value-to-interface admission, same-interface branch merges, ordinary
literal/None inference, non-interface readonly adaptation and non-completing-arm
handling. Preserve once-only left-to-right evaluation, short circuiting, completed
effects, mutation commits, GC roots, host borrowing, owner/generation checks,
cancellation, depth limits, trap cleanup and generation-pinned calls. Equality
must not skip a selected user callback merely because both operands alias.

Excluded: source `impl TraitA for TraitB`, bare-generic blanket impls, derive
syntax, heterogeneous `PartialEq<Rhs>`, new downcast APIs, Object/dyn syntax,
SA22 generic interface-construction witnesses, allocation optimization, NC storage
migration and SA21 host-registration unification. Existing registered interface
implementations unrelated to removed equality forwarding remain supported.
Downcast must preserve object identity when implemented, but this plan does not
deliver it. No allocation count or performance improvement is promised.

## Evidence and implementation ownership

The review retains [SA23](review.md#sa23-mixed-concrete-and-interface-equality-depends-on-operand-order),
[SA24](review.md#sa24-collection-interface-equality-rejects-valid-enum-receivers)
and [SA25](review.md#sa25-common-interface-inference-includes-dynamically-incompatible-traits)
reproductions. Recheck affected consumers when their phase begins; do not rerun
the whole workspace to establish a baseline.

| Current path | Diagnosis and target responsibility |
| --- | --- |
| [HIR joins](../crates/kagari-hir/src/typeck/body/joins.rs), [relationship queries](../crates/kagari-hir/src/aggregates/relationships.rs) | Both common-interface search and sibling-derived hints can create conversions. HIR must distinguish supplied target context from inference hints; retain ordinary implementation and parent lookup. |
| [Local trait checking](../crates/kagari-hir/src/typeck/check/trait_surface.rs), [imported applications](../crates/kagari-hir/src/typeck/applications.rs), [language roles](../crates/kagari-types/src/language/role.rs) | Current signature checks coexist with a reserved-role exclusion. Replace exclusions with one structural contract and member-specific diagnostics. |
| [Interface contracts](../crates/kagari-contract/src/callable/interface.rs) | Source-independent declarations/proofs must express and validate the same signature, bound and parent requirements. No dependency on HIR or source annotations. |
| [HIR semantics](../crates/kagari-hir/src/language/semantics.rs), [operator checking](../crates/kagari-hir/src/typeck/body/operators.rs), [equality lowering](../crates/kagari-compiler/src/source/lower/expr/equality.rs) | Establish eligibility and selected operations once; lowering consumes accepted operand adaptations and never selects solely from an unadapted operand. |
| [Stdlib comparison catalog](../crates/kagari-stdlib/src/catalog/comparison.rs), [interface protocols](../crates/kagari-stdlib/src/catalog/interface_protocols.rs), [comparison bindings](../crates/kagari-stdlib/src/bindings/comparison.rs) | Library-owned conditional content behavior and helpers remain; remove collection-interface receiver equality and keep Debug. |
| [Callable planning](../crates/kagari-compiler/src/source/lower/instances/callables.rs), [primitive proofs](../crates/kagari-contract/src/types/proofs/composition.rs), [runtime value semantics](../crates/kagari-runtime/src/value_semantics.rs) | Identity certificates, negative eligibility and retained specialized comparison dispatch support the superseded fallback/forwarding model. Remove them with their consumers, retaining ordinary selected-call machinery. |
| [Interface wire records](../crates/kagari-bytecode/src/artifact/wire/interface.rs), [GC interface metadata](../crates/kagari-runtime/src/gc/interfaces.rs), [value semantics](../crates/kagari-runtime/src/value_semantics.rs) | Update portable and runtime representations together; independently validate types/proofs and preserve roots, object identities and supplying generations. |

Shared eligibility operations belong with source-independent declaration/type or
contract owners according to their data dependencies. HIR adds diagnostics and
source context; backends consume checked facts. Choose exact module placement
from the existing owners during TI02, without a forwarding crate, parallel
checker or protocol-name exception list. Keep source static-trait use distinct
from constructing or invoking an interface; declaration validation must not ban
otherwise legal static traits.

## Phase sequence and build policy

Run TI01 -> TI02 -> TI03 -> TI04. Each phase updates this ledger and the affected
consumers at the semantic change, rather than postponing all fixtures until the
end. Default to buildable, reviewable checkpoints. Changes inseparable across
HIR, contracts, library registration and execution belong in one coherent
checkpoint; no production stubs, weakened validation or second semantic model.

If an intermediate boundary fails to build, attempt the relevant focused check
and record the command, representative diagnostic, cause and owning phase here.
Do not repeat unchanged failures before their repair; phase scope completion is
not build acceptance. Temporary bridges require a named removal checkpoint no
later than TI04. Delete obsolete code when its last consumer is removed.

Replace unpublished APIs and formats directly. Keep format/runtime ABI identifiers
unless publication or an explicit compatibility requirement calls for a bump.
Regenerate affected disposable artifacts at coherent checkpoints, preserving
source-free coverage. Do not add old-format readers or compatibility aliases.
Future implementation commits use `Plan: trait-interface-comparison-plan TI0N`
trailers and Conventional Commits, with breaking changes marked `!`.

### TI01: Require supplied interface conversion targets

- Trace variable annotations, assignment destinations, parameter/return/field
  types and nested branch propagation. Keep their checked conversion path.
- Remove context-free common-trait/common-parent selection for both if and match.
  Prevent sibling-derived interface hints from coercing another branch in either
  order; preserve literal/constructor inference and completion analysis.
- Keep identical interface/concrete branch types and existing non-interface
  readonly adaptation. Diagnose incompatible branches with a request for a target
  type; retire unused join-only ambiguity/dominance search after consumer review.
- Rewrite execution fixtures that relied on inferred interfaces with explicit
  targets, preserving effect counts, associated outputs, generic calls and routes.

Exit: accepted annotated/signature-context branches execute, unannotated mixed
branches fail source checking, same-interface branches and ordinary inference
remain valid. No public-interface selection depends on adding an unrelated impl.
Retain dynamic eligibility checking as it currently exists until TI02; do not
disable it during this phase.

### TI02: Unify structural dynamic compatibility

- Apply trait arguments and associated output bindings before checking the whole
  interface and parent closure. Keep caller-known generic outputs and existing
  bound checking, cancellation and cycle/search limits.
- Validate nested parameter/result Self dependencies and method constraints,
  including `T: Related<Self>`. Receiver Self and ordinary body-local Self use
  remain valid; the interface's own implementation evidence is not an extra
  forbidden call requirement.
- Reject the entire interface application for any incompatible member/parent;
  keep static traits usable. Retain shared generic methods and enforce the
  no-receiver/constant/GAT limits without member exclusion markers.
- Remove reserved-role dynamic exclusions. Apply the same contract to standard,
  user and registered native declarations, local and imported uses, and portable
  interface construction/calls. Preserve unrelated registration/host restrictions.
- Produce member/parent-specific diagnostics. Check representative compatible
  standard protocols and incompatible PartialEq/Eq; do not whitelist examples.

Exit: equivalent signatures have equivalent eligibility at every admission
boundary; source-free forged/mismatched contracts are rejected without source
analysis. Existing generic method and associated-output dispatch still execute.

### TI03: Replace equality and identity selection end to end

- Remove implicit object identity PartialEq/Eq/Hash and implicit user-enum
  comparison. Require applicable PartialEq evidence uniformly in operators,
  explicit method calls, generic obligations and member composition.
- Preserve builtin scalar/float/Tuple behavior and standard-library Option/Result
  conditional semantics. Distinguish standard enum implementations from a blanket
  user-enum default; preserve explicit custom enums that compare across variants.
- Add conditional Array content equality and preserve applicable Vec/HashMap/
  HashSet content protocols. Do not grant residual identity hashing to content
  equality types; Eq/Hash eligibility must remain consistent with comparison.
- Remove six collection-interface forwarding PartialEq registrations, their
  native bindings and comparison-only construction metadata. Preserve interface
  Debug, content helpers and unrelated registered interface implementations.
- Admit identity operators only for known object shapes, including `Vec<T>` with
  arbitrary T. Admit different concrete object types without interface coercion;
  reject either interface/value/function/unknown-category operand in source and
  independent executable verification. Keep stable identities and handle checks.
- Update selected calls, MIR/bytecode verification and runtime together. Remove
  receiver comparison certificates, negative evidence, dispatch, codecs and SDK
  entrypoints that lose their consumers; retain generic witnesses and rooted,
  generation-pinned invocation. Inspect previous fast paths for identity shortcuts
  that would bypass custom comparison or IEEE element behavior.
- Migrate affected examples/tests at this checkpoint. SA23 mixed collection/
  interface equality and SA24 interface equality now fail source checking;
  interface construction, methods and content helpers still work.

Exit: the comparison specification is enforced from source through encoded
execution; unsupported equality never becomes identity or a missing-capability
trap. Old receiver forwarding is no longer an alternative semantic path.

### TI04: Complete integration and retire the migration

- Audit changed mechanisms for unused role exclusions, join-only inference,
  receiver comparison metadata and obsolete public entrypoints. Remove any
  explicitly recorded bridge and resolve all carried errors.
- Finish affected registered declaration examples, tooling expectations and
  artifacts in one coherent update. Preserve useful source-free, callback failure,
  GC/ownership and pinned reload cases when obsolete identity fixtures disappear.
- Complete the remaining selected local gates below; reuse passing evidence for
  unaffected code. Report unrun CI separately, without a local workspace sweep.
- Replace migration-status notes with implemented contracts; remove superseded
  comparison descriptions. Close SA23-SA26 only when their respective outcomes
  are validated. Transfer remaining independent work to the roadmap/review.
- Move lasting ownership facts into architecture, update NC cross-references,
  then retire this plan when implementation and local handoff are complete.
  Pending CI alone belongs in the roadmap, not a retained completed phase ledger.

## Focused verification and acceptance

Reuse [testing preparation](testing.md) and existing contract owners. Update cases
that intentionally assert old semantics rather than weakening assertions or
adding duplicate smoke suites. Keep distinct observable, ownership and artifact
boundaries. These are planned gates, not claims that commands have passed.

| Phase | Selected contracts and evidence |
| --- | --- |
| TI01 | [HIR branch tests](../crates/kagari-hir/src/analysis/generic_type_tests/joins.rs): explicit variable/argument/return/field targets, both branch orders, distinct children/common parent, same interface, literals/None and non-completing arms. Two migrated [interface execution](../crates/kagari-embed/tests/collection_interfaces.rs) cases retain once-only effects and associated-output adaptation through source and encoded routes. |
| TI02 | [Interface contract tests](../crates/kagari-contract/src/callable/interface/tests.rs) and [compiler signature tests](../crates/kagari-compiler/src/tests/interface_signatures.rs): applied signatures/parents, method bounds, nested Self, complete rejection and legal static use, generic methods, compatible standard roles, local/imported agreement and tampered portable evidence. Select focused HIR cases for diagnostics and a shared-generic execution regression. |
| TI03 | [Standard protocols](../crates/kagari-embed/tests/standard_traits.rs), [collection interfaces](../crates/kagari-embed/tests/collection_interfaces.rs), [identity contracts](../crates/kagari-embed/tests/interface_identity.rs): missing evidence, explicit Struct/enum comparison, conditional content, IEEE/no identity shortcut, generic eligibility, helper/Debug preservation and concrete identity admission. Reuse malformed artifact checks at their validator owners. |
| TI04 | Remaining directly affected cases for callback traps/reentry, roots, pinned comparison calls and interface adaptation/reload; audit migrated fixture routes and source-free loading. Do not rerun unrelated passed suites or absorb other queued migrations. |

Starting command selections, using existing owner/filter names at the planning
baseline (record renamed filters and added distinct cases in the ledger):

```text
cargo test -p kagari-hir --lib analysis::generic_type_tests::joins::
cargo test -p kagari-embed --test collection_interfaces explicit_interface_
cargo test -p kagari-contract --lib callable::interface::tests::
cargo test -p kagari-compiler --lib tests::interface_signatures::
cargo test -p kagari-embed --test standard_traits custom_comparisons_short_circuit_without_identity_shortcuts
cargo test -p kagari-embed --test standard_traits comparison_only_types_do_not_inherit_identity_hashing
cargo test -p kagari-embed --test collection_interfaces concrete_content_protocols_and_interface_helpers
cargo test -p kagari-embed --test collection_interfaces collection_content_composes_and_preserves_float_comparison
cargo test -p kagari-embed --test interface_identity
cargo check -p kagari-embed --no-default-features --lib
```

These selections are a starting set, not an exhaustive fixed script to repeat
after every edit. Add the distinct newly specified positive/negative boundaries
at their existing owners and record exact filters when introduced. Retain the
standard/custom enum key and comparison reload coverage relevant to changed
paths. Replace deleted proof-layout tests with behavior/admission assertions;
do not remove the independent source-free rejection boundary. A successful
filtered invocation must execute the intended tests; zero matches do not pass.
Use `--no-fail-fast` for multiple selected targets; after failures rerun only
failed/affected cases and finish any selected cases skipped by an earlier exit.

At implementation checkpoints review imports, visibility/reexports, module
ownership, effective LOC and mixed responsibilities. Run:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
git diff --check
```

Scope Clippy/builds to changed packages and selected targets. Use workspace
profiles, default target/parallelism and uv-managed Python. Documentation-only
checkpoints need content/link/diff checks, not Cargo or structure runs. Exhaustive
workspace tests, full Clippy and complete feature/backend matrices remain CI-only.
No compilation/execution speed claim follows from removing semantic branches.

## Progress and error ledger

This is the only phase/progress ledger for this migration. Specifications own
semantics and review entries own reproductions; neither keeps a duplicate phase
checklist. Update this section when implementation scope, design or carried
errors change. Successful command transcripts belong in Git history after handoff.

- [x] Record accepted design, bounded ownership, TI01-TI04 and local/CI gates.
- [x] TI01: Require supplied interface conversion targets.
- [x] TI02: Unify structural dynamic compatibility.
- [x] TI03: Replace equality and identity selection end to end.
- [ ] TI04: Complete integration and retire the migration.

Current build status: TI01-TI03 libraries build and selected local checks pass.
Initial compile/comparison failures are repaired; no carried production build
failure remains. TI04 reload/fixture migration and documentation handoff are
pending. Full workspace, feature/backend and CI acceptance remain unverified.
SA22 and unrelated roadmap work remain outside this plan.

TI03 implementation and verification:

- Static equality requires PartialEq. Explicit Struct/user-enum and registered
  interface implementations remain available; core Option/Result/Ordering/Bound/
  ControlFlow use conditional member protocols. Array has a library-owned
  conditional PartialEq implementation with checked element callbacks.
- Removed six interface PartialEq registrations, receiver comparison certificates,
  negative eligibility facts, dispatch, wire records and runtime retention edges.
  Interface demands now use ConcreteFunctionIdentity directly. Ordinary callable
  bindings, parent views, result adapters and their generation/GC owners remain.
- Removed primitive object equality/hash defaults. MIR and KBC identity checks
  require known object types; linked primitive equality/hash requires exact
  aggregate type/protocol evidence and rejects bypassing selected implementations.
  Removed the unused catalog-free TypeId equality predicate. Portable host schema
  constraints now follow member capabilities and reject interface equality.
- Passed four standard-trait cases for missing equality evidence, comparison-only
  hashing rejection, Array callbacks/IEEE/nesting and alias callback execution.
  Passed eight further selected standard-trait cases for explicit identity keys,
  imported/supertrait bounds, explicit recursive/custom enums, key callbacks,
  stable enum hashing and concrete identity. Each executing case uses source,
  artifact and artifact JIT routes unless its existing purpose is artifact-only.
- Passed five selected collection-interface cases: concrete content/helper behavior,
  nested/IEEE content, upcast alias mutation, generic boxing methods, and all six
  interfaces rejecting operator/method/generic/tuple/identity comparison.
- Passed all four interface_identity cases, including operand effects/root cleanup
  and forged KBC identity/equality instructions (interface operands or a custom
  concrete implementation bypass). Passed ten contract proof tests and the
  portable host-constraint test.
- Initial strict-proof failures exposed missing semantic registers in generated
  protocol parameters and hash-composition tuples; lowering now records them.
  A closed tuple callback had been emitted as a forwarded shared witness without
  a native application scope. Closed tuple requirements now select the existing
  concrete adapter; symbolic tuple requirements retain the same shared adapter
  body. Affected key/enum hash cases passed after repair. No second comparison
  implementation or compatibility bridge was added.
- Scoped library Clippy passed for types, HIR, contract, MIR, bytecode, compiler,
  runtime and stdlib after removing stale imports and simplifying interface-demand
  comparisons. Both runtime value-semantics tests passed, retaining nominal reload
  behavior, IEEE values and rejection of implicit object-member equality. Structure,
  formatting and diff checks passed; no structural exception was added.
- TI04 owns the older generic_reload receiver-forwarding/identity fixtures: replace
  their obsolete interface comparison expectations with concrete comparison or
  explicit helper calls while preserving pinned generation and root assertions.
  Audit remaining dynamic-standard-trait negative fixtures and callback/reentry
  cases there; do not interpret their unrun status as a pass.

TI02 verification:

- `cargo test -p kagari-hir --lib tests::typeck::traits::` passed twelve existing
  trait cases. Subsequent focused runs passed the added `dynamic_trait_eligibility`,
  `dynamic_standard_traits`, `imported_interface_eligibility` cases and
  `generic_interface_method_checks_bounds_and_forwards_generic_arguments`.
- `cargo test -p kagari-contract --lib callable::interface::tests::` passed seven
  tests, including malformed uncalled members and Self-dependent call/declaration
  constraints. `cargo test -p kagari-compiler --lib tests::interface_signatures::`
  passed both encoded-boundary and tampered-normalization cases.
- `cargo test -p kagari-embed --test standard_traits
  structurally_dynamic_standard_traits_dispatch_through_parents` and
  `cargo test -p kagari-embed --test collection_interfaces
  explicit_interface_target_preserves_iterator_result_adaptation` each passed one
  test through source, artifact and artifact JIT routes.
- `cargo clippy -p kagari-hir -p kagari-contract -p kagari-bytecode
  -p kagari-compiler --lib -- -D warnings`, structure checking (1116 Rust files,
  zero violations), formatting and diff checks passed.

Local and imported dynamic eligibility now share the completed applied catalog,
including fields and enum payloads. Portable construction and calls share whole
surface admission; static implementation tables omit structurally ineligible
parent views, while dynamic admission rejects the whole incompatible closure.
No reserved-role exclusion remains. An interface's own receiver evidence is not
passed as an additional dynamic method witness. Testing exposed and fixed Hash
interface calls incorrectly taking the intrinsic value-hash path; explicit
standard protocols and inherited views now dispatch through their interface slots.
No new compatibility bridge, re-export or structural exception was introduced.

TI01 verification: `cargo test -p kagari-hir --lib --
analysis::generic_type_tests::joins:: body_constraints_use_later_arguments_and_local_uses
return_context_reaches_control_flow_and_composite_constructors
branch_and_array_merges_recover_complementary_member_facts` passed all nine tests.
`cargo test -p kagari-embed --test collection_interfaces explicit_interface_`
passed both tests across source, artifact and artifact JIT routes. Structure
checking (1115 Rust files, zero violations), formatting and diff checks passed.

The branch policy now masks inferred interface targets, including nested types
and facts recovered across solver rounds. Compatible branches unify unresolved
constructor payloads without converting values, preserving Option inference.
Initial tests exposed the solver feedback path and a masked None payload; both
are fixed and the affected tests passed. Supplied targets still propagate through
assignments, arguments, returns, fields and nested branches. Existing comparison
operators still consume `common_branch_interface` / `common_interfaces` and its
ambiguity diagnostic; TI03 owns their removal with those operator paths. No new
parallel compatibility path or structural exception was introduced.

| Checkpoint | Evidence / disposition |
| --- | --- |
| 2026-10-13 TI03 | Removed automatic equality/identity fallback and receiver certificates across source, portable contracts, library and runtime. Array content comparison and strict object identity verified with focused source/artifact/JIT, malformed KBC, proof and runtime checks; scoped Clippy passed. TI04 fixture/reload migration, documentation handoff and CI remain pending. |
| 2026-10-13 TI02 | Unified applied structural eligibility and portable admission; retained generic methods, checked declaration/method constraints and standard protocol dispatch. Selected HIR, contract, compiler and multi-route execution tests plus scoped Clippy passed. No carried errors; TI03-TI04 and CI pending. |
| 2026-10-13 TI01 | Supplied-target branch checking and solver hint separation implemented; nine HIR tests and two multi-route execution tests passed. Existing operator-only common-interface search remains until TI03. No carried errors; TI02-TI04 and CI pending. |
| 2026-10-13 planning | Read current implementations/tests at 981b7070 and accepted specifications; checked existing module/test filter names without running Cargo. Content review, 82 added local links/anchors across nine documentation files, code fences, preserved CRLF, whitespace and git diff checks passed. TI01-TI04 and all local/CI implementation gates remain pending. |

For each implementation checkpoint record: phase and changed contract; actual
commands/filters and results; carried diagnostic/cause/repair owner; any bounded
structural debt or temporary bridge and removal phase; pending local and CI scope.
