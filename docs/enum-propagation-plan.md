# Nominal enums and protocol-based propagation

Status: active execution design. The user activated the continuous EN01-EN05 goal
on 2026-10-04, with one accepted checkpoint commit per phase.
CR01-CR02 and LR01-LR03 remain complete.
The [roadmap](implementation-roadmap.md#nominal-enums-and-propagation-en01-en05-active)
owns phase order, activation, checkboxes and the execution ledger. This document
owns the detailed implementation contract and acceptance criteria for EN01-EN05.

## Problem and intended behavior

Standard-library declarations now use the same Engine installation and generated
KGR pipeline as application native declarations. However, seven library enums
still use a closed `StandardEnum` inventory, a distinct semantic type and dedicated
execution operations. HIR and compiler lowering directly recognize Option/Result
to implement `?`. Moving these declarations into the compiler would preserve that
coupling rather than resolve it.

The target is one nominal enum model for source, standard-library and application
native declarations. Option/Result remain core-library definitions. The compiler
implements `?` through checked propagation protocols; libraries implement the
success/failure behavior. Runtime and backends consume generic enum layouts and
selected calls, without a concrete standard-library catalog.

The existing registration flow remains:

```text
stdlib / application registration
  -> portable declarations plus local Rust bindings
  -> atomic Engine installation
  -> generated documented KGR views for source-enabled analysis
  -> ordinary parsing, lowering and declaration correspondence checks
  -> checked calls and nominal enum operations
```

Source-free installation uses the same declaration records and bindings directly.
Generated KGR is a tooling projection, not an execution or installation input.
This work introduces no declaration binary, crate metadata product or build-time
frontend dependency. User-program KBC/MIR artifacts remain checked execution
products and are still in scope for validation.

## Baseline and concrete migration sites

Inspect these owners and their consumers before each implementation phase:

| Current owner | Existing coupling | Intended replacement |
| --- | --- | --- |
| `kagari-types/src/surface.rs` | `StandardEnum` defines arity, variant order and payload slots for Bound, ParseError, TryFromIntError, Infallible, Option, Result and Ordering | Library-owned enum declarations, ordinary `TypeDefKind::Enum` / `VariantDef` and nominal type applications |
| `kagari-types/src/ty.rs` and its wire/substitution/mapping helpers | `Ty::StandardEnum` is separate from `Ty::Enum` | `Ty::Enum(NominalTy)` for all these declarations |
| `kagari-types/src/declaration/native.rs` | `NativeTypeConstructor::Enum` selects that closed inventory | Registered nominal enum declarations; no special constructor family |
| `kagari-runtime/src/native/builder/type_builder.rs` and `native/types.rs` | Public type authoring expects native storage; `TypeRef::apply` always produces NativeObject | Separate enum authoring, with kind-correct shared type handles |
| `kagari-runtime/src/native/context/operations.rs` | Native construction accepts `EnumTag` and payload Values | Checked applied enum and variant handles with scoped payload validation |
| `kagari-hir/src/typeck/body/standard.rs` | `infer_propagation` matches Option/Result and directly selects From | Checked Try/FromResidual selection and associated output constraints |
| `kagari-compiler/src/source/lower/expr.rs` and `expr/standard.rs` | `?` and constructors emit dedicated StandardEnum operations | Generic enum operations and ordinary selected protocol calls |
| `kagari-contract/src/operations.rs`, MIR, bytecode and VM aggregate operations | Closed enum operation and payload contracts | Declaration-driven checked layouts, discriminants and field access |
| `kagari-stdlib/src/catalog` and implementations | Registrations and Rust bodies author fixed standard tags | Nominal declarations and checked library-local handles/adapters |

Also audit iteration, PartialOrd/Ord, range-bound APIs, conversion adapters,
equality/hashing, reflection, result reporting, native codecs, executable proofs,
GC tracing and reload compatibility. These are actual consumers of the existing
enum representations, not unrelated refactoring opportunities.

## Ownership and dependency contract

| Layer | Responsibility |
| --- | --- |
| `kagari-stdlib` | Core enum definitions, full documentation, exports/prelude, protocol declarations and concrete implementations; native conversion/reporting policies |
| `kagari-types` | Nominal enums, generic binders, variant payload types, declaration validation, protocol role schemas and symbolic declaration/member references |
| `kagari-hir` | Resolution, inference, exhaustiveness, enum constructor/pattern checks, protocol selection and recoverable diagnostics |
| `kagari-compiler` | Lower checked constructors/matches and propagation into enum operations, calls, branches and returns |
| `kagari-contract` / MIR / bytecode | Executable enum layouts, checked callable targets, bounded executable metadata, operation validation and linked dependency checks |
| `kagari-runtime` / VM | Installed nominal layouts, generic enum allocation/access, tracing, roots, provenance storage and pinned execution |
| ABI / backends | Physical representation and calls; consume checked facts without resolving syntax or choosing trait implementations |

Retain CR02's dependency restrictions. In particular, HIR/types do not depend on
contract, ABI, runtime or stdlib; runtime/VM and executable validation do not depend
on source analysis. The Engine composes providers. Do not create a new crate,
forwarding module, compatibility alias or common catalog to connect these layers.

## Enum model and registration API

Reuse the existing `TypeDef`, `VariantDef`, `NominalTy`, generic owner identities
and ordinary enum representation. Do not introduce a parallel native enum type
system. Variant identity is owned by the enum declaration; a discriminant is only
an index in a validated, generation-pinned layout. Names and matching payload
shapes cannot substitute for nominal identity.

The public authoring surface follows ModuleBuilder's existing style. These are
target APIs, not currently implemented calls:

| API | Contract |
| --- | --- |
| `module.define_enum(name) -> EnumBuilder` | Begin a nominal enum declaration without requiring NativeStorage |
| `enum_builder.type_parameter(name) -> NativeResult<ParameterRef>` | Declare an enum-owned type parameter using the existing binder model |
| `enum_builder.variant(name, payload) -> NativeResult<VariantRef>` | Declare a unit or tuple-payload variant, preserving declared order |
| `enum_builder.documentation(text)` / `variant_documentation(handle, text)` | Store complete Markdown and validate member ownership |
| `enum_builder.finish() -> NativeResult<TypeRef>` | Validate and publish the completed declaration into the module's authoring providers |
| `type_ref.apply(arguments) -> NativeResult<Type>` | Produce the nominal semantic type appropriate to the declaration kind |
| `type_ref.variant(name) -> NativeResult<VariantRef>` | Resolve a checked member handle for later registration or native use |
| `cx.enum_value(applied, variant, fields) -> NativeResult<Value>` | Allocate against an installed, scoped enum application and its validated variant |

Illustrative authoring, to become a compiling example during EN01-EN02:

```rust
let mut event = module.define_enum("Event");
event.documentation("An event produced by the application.");
let item = event.type_parameter("T")?;
let data = event.variant("Data", [item.ty()])?;
event.variant_documentation(&data, "Contains one application item.")?;
event.variant("Closed", [])?;
let event = event.finish()?;
let event_of_i32 = event.apply([Type::i32()])?;
```

Enum values use managed enum storage, not arbitrary opaque Rust payload storage.
Keep the existing source-supported variant forms; new record-variant syntax and
mutable enum payload APIs are outside scope. Support unit variants, multiple
tuple fields, generics, nested/recursive references and empty enums. Reject
construction of an empty enum and invalid or foreign variant handles.

Native construction must validate the installed owner/generation, substituted
payload count/types, nested heap handles and declared access. CallContext must
obtain executable scope from its selected call/installed registry, not trust an
unresolved authoring Type. Preserve existing rooting obligations before further
allocation or synchronous reentry. Native inspection must use the same checked
layout and reject wrong owners, variants and payload indices. No raw fixed tag
API remains as an alternate unchecked construction route at final acceptance.

Full docs and owning spans survive rendering, cache materialization, re-exports,
navigation, hover and doc-only refresh. Finishing registration is not a second
source parser and does not silently install a module into an Engine.

## Library migration and remaining language bindings

Migrate all seven current StandardEnum families in EN03. Keep their canonical
core modules, prelude visibility, variant ordering and observable behavior. A
library-local registration handle set may name Option/Result/Ordering and their
variants; it is not a generic compiler/runtime catalog.

Some operations still need library bindings: Iterator.next returns Option,
comparison protocols return Ordering or Option<Ordering>, range APIs expose Bound,
and installed numeric conversion adapters describe their error/result types.
Replace fixed enum families with explicit declaration/member references. Validate
those references and their complete signatures at installation/analysis/loading.
Remove duplicate shape tables; derive ordinary payload shapes from declarations.
Source-free proofs carry the required references and checked facts.

Generic numeric evaluation remains shared. Concrete ParseError/TryFromIntError/
Infallible type selection belongs to registered conversion declarations and
adapters, not a closed enum choice in the generic numeric algorithm. Preserve
checked conversion behavior without expanding the conversion API surface.

Keep existing implicit equality/hash/ordering eligibility and explicit override
rules. Unification must not accidentally change object identity, enum payload
composition, float eligibility or key stability. Any necessary checked library
implementation/default records must preserve those rules without restoring a
global standard-enum switch.

## Propagation protocol contract

EN04 adds `core::ops::Try`, `core::ops::FromResidual` and
`core::ops::ControlFlow` through ordinary standard registrations. They are publicly
accessible under canonical paths and checked std re-exports. They are not added to
the implicit prelude; Option/Result and their existing variants remain there.

The intended signatures use explicit generic arguments instead of requiring Rust
generic defaults. This is a target declaration sketch to validate against Kagari's
bounded projection and supertrait machinery during EN04:

```kagari
pub enum ControlFlow<B, C> { Break(B), Continue(C) }

pub trait FromResidual<R> {
    fn from_residual(residual: R) -> Self;
}

pub trait Try: FromResidual<Self::Residual> {
    type Output;
    type Residual;
    fn from_output(output: Self::Output) -> Self;
    fn branch(self) -> ControlFlow<Self::Residual, Self::Output>;
}
```

Reserve and validate the Try/FromResidual declaration roles, associated members,
selected methods and ControlFlow type/variant bindings needed by lowering. They
remain ordinary library declarations with known language duties. Parse roles
through syntax nodes. Reject unknown, duplicate, missing, forged or incorrectly
shaped bindings. A user type may implement the installed protocol; a same-named
application trait cannot acquire its reserved role.

Propagation uses two obligations: operand `A: Try`, and enclosing return type
`R: FromResidual<A::Residual>`. Its expression type is `A::Output`. It evaluates
the operand and branch method once, returns the Continue payload, or invokes
the selected FromResidual method once and returns from the nearest function or
closure. Lowering evaluates each required expression left-to-right and records
the concrete calls/associated outputs in the checked type table.

Default implementations preserve existing standard behavior:

| Carrier | Output | Residual | Standard conversion |
| --- | --- | --- | --- |
| `Option<T>` | `T` | `Option<Infallible>` | None returns None into Option; no automatic Option-to-Result bridge |
| `Result<T, E>` | `T` | `Result<Infallible, E>` | `Result<U, F>` accepts the residual when `F: From<E>` |
| `ControlFlow<B, C>` | `C` | `ControlFlow<B, Infallible>` | Break propagates into ControlFlow with the same B |

Distinct residual types prevent unrelated carriers from mixing automatically.
Custom nominal source and registered native types may supply explicit Try and
FromResidual implementations under the existing checked implementation rules.
Try/FromResidual are static protocols for this track; dynamic interface propagation
is not added. Permit source implementations for eligible types in their defining
module, validate generic scopes and reject ambiguous/overlapping implementations.
Do not permit arbitrary overrides of the installed standard implementations.

Preserve current Option/Result inference: surrounding constraints can supply
unconstrained constructor types; success types may differ; a source error is inferred
before conversion selection and an independently constrained error is never rewritten
to force a fit. Unconstrained standard error fallback remains bounded by the
current rules. Generic `T: Try` and `R: FromResidual<T::Residual>` paths must work
through the existing bounded solver, with explicit associated output constraints
when inference cannot determine a type. Do not add conversion-chain search,
implicit assignment conversion, TryFrom fallback or a full Rust trait solver.

The default Result FromResidual implementation selects From through ordinary
checked native callable requirements or source calls. Compiler `?` handling no
longer directly selects From. Audit whether From retains another language-role
consumer before removing that role; do not remove the ordinary From declaration,
identity conversion or registered conversion adapters. Update the actual role and
trait inventories rather than preserving the current fixed counts.

Rust's protocol is an architectural reference, not a feature-stability constraint
on Kagari. Kagari's acceptance includes user-defined carriers. No Rust borrow,
trait specialization, try blocks, exception syntax, generic default arguments or
new Error trait is introduced.

## Failure provenance and runtime invariants

[Error reporting](spec/error-reporting.md) remains authoritative. New Result Err
captures a source/stack origin; forwarding and converted propagation preserve the
original origin. Reconstructing Err from an extracted payload captures a new origin.
None carries no failure origin; equality/hash ignore provenance; a top-level Err
remains successful VM execution with an optional diagnostic failure preview.

Use optional value-attached provenance in the managed enum representation and
explicit checked library construction/reporting bindings. Capture policy is tied
to the installed Result Err member, never a spelling or global fixed enum tag.
Try.branch transports the original provenance with its residual; FromResidual
transfers it to the returned Result after a successful payload conversion. The
library's checked native implementation can use runtime provenance operations.
The generic `?` compiler path must not need a Result-specific instruction.

Do not use a global "last error" slot: nested calls, retained values and reentry
must keep independent origins. A newly introduced custom carrier does not
automatically become a CLI diagnostic failure or gain Result provenance policy.
Invalid bindings and unauthorized access to provenance adapters must be rejected.
Native construction inside a script call captures the script call site, not
generated declaration locations or synthesized Rust frames.

Traps from branch, conversion or FromResidual retain their own stack and completed
effects; they do not become Err. Preserve roots, cleanup, cancellation, call depth,
once-only operand evaluation and generation-pinned calls. Old enum values retain
their installed layout/version across reload; current registry lookups cannot
reinterpret their discriminants. Layout/variant/signature compatibility remains
checked before publication. Artifact validation bounds all carried descriptors,
payload types, indices, call targets and provenance policies before execution.

## Phase contracts

### EN01: Author and validate ordinary native enums

Implement the shared nominal declaration/registration APIs and kind-correct
TypeRef application. Reuse existing source enum semantics. Add complete enum and
variant documentation, renderer correspondence and tooling identity support.
Specify the scoped applied enum/variant handles consumed by execution in EN02.

Acceptance: valid generic, unit, multi-field and empty enum declarations are
validated/rendered; duplicate members, foreign binders, incorrect arguments and
invalid handles fail without partial module installation. Generated complete
modules parse with correct docs/spans. Source-independent declarations require
neither a runtime heap nor a frontend. Test public authoring APIs in their actual
owner, not copied fixture-only builder logic.

### EN02: Execute registered enums through generic layouts

Connect nominal enum installation to existing source enum layouts and executable
validation. Implement checked native construction/inspection and native argument/
result handling. Use generic MIR/bytecode/VM enum construction, matching and payload
access, supporting multiple payload fields. Carry installed generation/layout and
optional provenance storage through heap tracing and executable linking.

Acceptance: an application Event<T> registered in Rust is constructed by a native
function, matched/returned by KGR and round-tripped through a checked native call.
The same executable loads in an artifact-only host. Incorrect identity, variant,
payload count/type, stale handles and malformed artifact layout are rejected.
Nested traced values survive collection/reentry and old values keep their pinned
layouts. No per-application edit to compiler, contract, ABI or VM inventories is
needed. Existing source enum behavior remains covered.

### EN03: Replace the seven standard enum families

Register all seven families as ordinary enums; migrate library implementations,
iteration/comparison/conversion bindings, reflection, reporting and executable
consumers. Remove StandardEnum, StandardVariant, NativeTypeConstructor::Enum,
Ty/TypeId standard-enum branches, fixed standard EnumTags and dedicated standard
enum instructions/wire nodes. Keep `?` temporarily restricted to its existing
Option/Result behavior using validated nominal member bindings and generic enum
operations until EN04 replaces that policy.

Acceptance: existing Option/Result propagation, From conversion, inference,
iteration, ordering, ranges, numeric conversions, equality/hash and diagnostics
pass. The stdlib registrations expose complete enum source and docs; no duplicate
shape catalog remains. Source-free native results and artifact loading work using
the same declarations. Err construction, converted propagation and top-level
reporting preserve the error-reporting contract. Resolve carried EN01-EN02 build
failures before completing this checkpoint.

### EN04: Implement Try/FromResidual propagation

Register the protocols and ControlFlow, implement the standard carriers and
replace direct Option/Result selection/lowering with the two protocol obligations.
Add checked role/member schemas, ordinary selected calls, bounded projections,
appropriate local source/native implementation support and precise diagnostics.
Remove the interim EN03 propagation policy and any unneeded From language role.
Publish the final protocol signatures and supported implementation/inference
rules in the existing specifications and runnable examples.

Acceptance: existing standard propagation remains compatible; ControlFlow and a
custom source enum/struct carrier work, including generic bounded functions and
independent closure return contexts. A registered native carrier follows the same
protocol. Invalid output/residual contracts, missing or ambiguous implementations,
forged roles and incompatible return carriers fail before execution. Branch and
FromResidual are called exactly once with correct effects/traps/cancellation.
Converted Err retains its original trace without adding conversion frames.
Source-free validation links the already-selected methods without trait inference.

### EN05: Remove migration remnants and complete integration

Delete obsolete helpers, fixed enum inventories, decoder branches and fixture
producers. Update affected artifact fixtures once at this coherent schema
checkpoint; add no old readers or aliases. Update architecture/specs/API examples
to actual implemented behavior, dependency checks and standard role inventories.
Run final behavioral/feature/backend validation and resolve all carried failures.

Acceptance: the complete matrix below passes, crate boundaries remain enforced,
and code search finds no closed standard-enum type/operation/tag route or direct
Option/Result switch in compiler propagation. Necessary library handles and
explicit checked language/reporting bindings remain documented with their actual
consumers. No production todo, disabled validation or misleading backend success
is accepted.

## Checkpoints and intermediate builds

At activation, execute EN01 -> EN02 -> EN03 -> EN04 -> EN05. Create one coherent
Conventional Commit per completed phase with `Roadmap-Step: EN01` through `EN05`.
Mark breaking public registration/executable changes with `!` and explain them.
The user has authorized implementation and checkpoint commits; include the
previously uncommitted execution design in the first implementation checkpoint.

EN01-EN02 may carry bounded build failures while declaration and executable
consumers migrate. Attempt relevant checks and record commands, representative
diagnostics, cause and owning follow-up phase in the roadmap. Existing legacy
standard enum paths can remain only until EN03; do not add a compatibility reader
or a second semantic implementation. EN03 must restore the baseline build and
focused behavior before protocol changes. EN04 must close its own integration
failures before EN05. Never weaken validation to make an intermediate phase pass.

Each implementation checkpoint includes the repository structural review,
`uv run --locked scripts/check_structure.py` and `git diff --check`. Run focused
behavioral checks where affected units build. Do not repeat unchanged failures or
full feature matrices at each checkpoint. Keep durable decisions/carried errors
in the roadmap and temporary logs under ignored `target/`.

## Final behavioral and feature matrix

| Area | Required evidence |
| --- | --- |
| Enum semantics | Unit/multiple-field variants, generic/nested/recursive payloads, empty enums, constructor inference, aliases/prelude shadowing, nested/alternative patterns and exhaustiveness |
| Native registration | Application enum with full docs; native creation/inspection/round trip; foreign handles, scope/payload mismatches and atomic failure |
| Standard library | All seven families; iteration, ordering, ranges, parsing/numeric conversion, equality/hash and reflection retain specified behavior |
| Protocol propagation | Option/Result regressions; ControlFlow; local source enum/struct and native carriers; generic bounds/projections; closure returns and postfix/nested `?` |
| Invalid protocols | Missing/forged roles, wrong method/associated output contracts, ambiguity, residual mismatch and unchecked executable targets rejected |
| Effects and safety | Once-only evaluation/selection; conversion only on failure; traps and completed effects; GC/reentry roots, cancellation, call depth and cleanup |
| Reporting | New versus forwarded Err, successful/failed conversion, nested errors, native call-site traces, None, detached previews and CLI exit behavior |
| Tooling | Full module/type/variant/protocol docs, owning spans, re-export navigation, materialized immutable cache and doc-only refresh |
| Executables/reload | Source-free linking, malformed/bounded enum descriptors and payloads, selected-call proofs, wrong provider generations, pinned values/calls and incompatible update rejection |
| Features/backends | Independent artifact-only/source/native/source+native consumers, enforced production/build dependency graphs, interpreter conformance and actual supported JIT execution |

Use existing meaningful coverage such as embedding `result_option`, `enum_payloads`,
`native_artifacts`, `native_provider_reset`, `generic_reload` and `artifact_features`,
VM native boundary/GC suites, compiler/bytecode verifier suites and CLI JIT tests.
Add focused cases for new registration/protocol boundaries rather than assertions
that merely mirror enum implementation tables.

Final commands:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv run python scripts/check_features.py
cargo test -p kagari-cli --features jit
git diff --check
```

Check changed documentation links and examples. Unsupported native enum/protocol
operations must select an existing checked interpreter fallback before entry;
do not claim actual JIT execution from fallback success. No broader JIT feature
expansion is required beyond preserving current supported behavior.

## Scope limits and references

This track does not reopen CR/LR registration ownership, add another source of
library signatures, reintroduce binary declarations or redesign every intrinsic
type. String, collection storage families, writable indexing and RangeKind are
changed only where their concrete enum consumers require it. Separate/parallel
crate compilation, Rust Serde derives/full value binding, arbitrary Rust enum
layout compatibility, async, exceptions, a generalized Error framework, no_std,
live registration mutation and a standalone LSP server remain separate work.

Relevant current contracts: [architecture](architecture.md),
[native declarations](spec/standard-declarations.md), [traits](spec/traits.md),
[Option/Result](spec/builtins.md#option-and-result),
[value semantics](spec/value-semantics.md),
[error reporting](spec/error-reporting.md),
[artifacts](spec/artifacts.md) and [activation](spec/module-activation.md).
Rust reference points are [core Option](https://doc.rust-lang.org/stable/src/core/option.rs.html),
[core Result](https://doc.rust-lang.org/stable/src/core/result.rs.html),
[Try declarations](https://doc.rust-lang.org/stable/src/core/ops/try_trait.rs.html)
and [the question-mark operator](https://doc.rust-lang.org/stable/reference/expressions/operator-expr.html#the-question-mark-operator).
