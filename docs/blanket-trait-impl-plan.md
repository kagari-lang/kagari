# Blanket trait implementations and library conversion forwarding

**Status: BI01–03 implemented; BI04 next.** This plan owns the migration;
[other HIR review questions](hir-model-review.md#analysis-flow-and-naming-review) remain separate.

## What should work

Given `impl<T: Marker> Ready for T`, any `T` for which the checker can prove `Marker` should
satisfy `Ready`. Today only a receiver with a known outer shape, such as `Wrap<T>`, gets past
this particular [trait header check](../crates/kagari-hir/src/typeck/check/trait_surface.rs).
This is one conditional implementation, not a default that a more specific impl can override.
No new parser syntax is needed.

The first library application is `val text: String = id.into()` when `String: From<UserId>`.
Today HIR uses reverse conversion metadata to infer `String`, and the compiler rewrites the
reverse call to the forward method. After migration the same expected type should identify
`UserId: Into<String>`; its ordinary library body calls `String::from(id)` once. For `TryInto`,
the result carries **the selected forward impl's** `Error`, not a fresh or inferred error type.
The independent executable verifier must reach the same conclusion without source HIR.

Reference: Rust 1.85.1 [`core::convert`, “From implies Into” and “TryFrom implies TryInto”](https://github.com/rust-lang/rust/blob/1.85.1/library/core/src/convert/mod.rs).
The library owns the forwarding body and associated error; ordinary trait selection proves its
bound. Borrowing, Rust crate orphan rules and Rust's direct-`Into` policy are not prerequisites.
Kagari instead uses checked dependency closures, structured library installation, GC references
and compatible hot reload. Reuse those owners rather than importing rustc's solver or adding a
second converter registry.

**Scope decisions:**

- Trait impls only; keep inherent impls, interface eligibility, existing standard-protocol owner
  checks and the [package coherence policy](spec/packages.md#checking-and-host-entries) intact.
  No specialization, negative impls, lifetime parameters, borrowed `self`, new casts or general
  lang-item shape DSL. Unrelated `Iterator` method-slot cleanup is not a BI phase.
- Preserve the [current user-visible ban](spec/builtins.md#conversion-protocols) on directly
  implementing the canonical `Into`/`TryInto`. The ordinary blanket impl in the defining library
  is the contract, not permission for a second impl whose bound happens not to hold today.
  Enforce this as declaration/admission policy, not as special proof or execution. Do not grant
  authority by spelling, alias, claimed owner string or an unchecked producer flag. Relaxing the
  public rule would require a separate language decision.

## BI01 — Admit bare trait receivers with coherent implementation headers

- Start with an ordinary user trait so `ConversionAdapter` cannot satisfy the witness. Reuse the
  parser and impl-owned binder IDs. Address [local overlap](../crates/kagari-hir/src/typeck/check.rs)
  and [reachable-catalog overlap](../crates/kagari-hir/src/aggregates/implementations.rs) **in the
  same checkpoint** as relaxing the bare-receiver guard. Check portable impl-table admission too;
  source rejection must not be the only barrier to conflicting executable evidence.
- Compare the receiver **and** applied trait arguments, treating each impl's binders as local to
  that impl. A bare parameter can match a concrete type or another binder; repeated occurrences
  must agree. Reject potential overlap conservatively when bounds do not prove disjointness; keep
  provably distinct applications (such as different concrete trait arguments). Do not choose by
  declaration order or defer conflicts until a method is called.
- Gate: a non-overlapping bare-receiver declaration admitted; a blanket/concrete conflict and
  two renamed equivalent blanket headers rejected with an impl-overlap diagnostic, including an
  unused conflict across sibling dependencies. Preserve existing `Wrap<T>` and disjoint cases.

## BI02 — Select and execute an ordinary conditional impl, without conversion metadata

- The [HIR matcher](../crates/kagari-hir/src/typeck/table.rs) and [portable pattern matcher](../crates/kagari-types/src/ty/matching.rs)
  already combine the receiver and applied interface into one substitution. Fix callers that only
  enumerate [interfaces from the receiver](../crates/kagari-hir/src/aggregates/relationships.rs)
  or only consult direct assumptions for a bare receiver. Use argument/expected-result constraints
  to construct a requested application; a possible candidate is not evidence that its bounds hold.
- For `S: Reverse<D> where D: Forward<S>` (ordinary test traits), infer `S` and `D` independently,
  substitute the impl bounds and prove them from checked implementations or the caller's bounds.
  `D: Forward<S>` should suffice in a generic caller; do not require it to also state `S: Reverse<D>`.
  Use the same evidence for call and type-application checking. No escaping impl binders, guesses
  for an ambiguous destination, circular self-proof or unbounded recursion; preserve cancellation
  and search limits. Normalize a forwarded associated output *after* substitution when checking a
  requested equality. Preserve definition scope and ordered impl/method type arguments at lowering.
- Gate: an ordinary forwarding method runs for concrete and generic callers; include a forward
  impl whose binder is determined by the trait argument rather than its receiver. Assert selected
  method/arguments and observable result. Missing evidence, an undetermined destination, a wrong
  associated output and a cycle must not become successes. Sample the independent
  [contract proof path](../crates/kagari-contract/src/types/proofs/search.rs). BI01–02 leave the
  old standard conversion path intact; no conversion test counts as proof of these phases.

## BI03 — Supply a verifiable ordinary Script impl body from structured registration

- The [canonical stdlib registrations](../crates/kagari-stdlib/src/foundation/protocols/conversion.rs)
  currently supply `Reverse` IDs, not forwarding bodies. The [draft builder](../crates/kagari-runtime/src/native/assembly/draft.rs)
  can describe impl headers but its method bindings are not an authored generic Script impl body.
  Extend the ordinary Script route only as needed to submit a body with its impl-owned binders.
- Exercise an unambiguous qualified forward trait call in that body. Reuse [structured expressions](../crates/kagari-types/src/declaration/script.rs)
  and ordinary HIR trait-call checking. If the existing expression cannot represent the typed call,
  add the smallest general expression needed and cover rendering, ID remapping and reference
  validation. No `Into` expression variant, new native callback, or parsing generated `.kgr` as the
  default compilation route. Structured and offline declarations must agree on impl identity,
  bounds and associated outputs; offline analysis grants no execution authority.
- Gate: register a nonstandard Script forwarding impl through the same path the library will use.
  The [installed-body verifier](../crates/kagari-runtime/src/native/registry/scripts.rs) must accept
  its checked call and reject a substituted callee of the same signature. Verify ordinary generic
  dispatch, not just matching text or a producer-generated certificate. Keep this checkpoint usable
  without publishing duplicate standard impls.

**BI03 declaration/HIR slice.** `ScriptExpr::TraitMethod` names a trait-owned static method
only as the callee of an ordinary `Call`. For `<T2 as Forward<T1>>::route::<M>(self, route)`,
`T1` and `T2` belong to the impl, `M` belongs to the method, and only the argument `route`
is a lexical value. Structured lowering uses the ordinary qualified HIR projection; selection
still requires the bound on `T2`. The [HIR equivalence test](../crates/kagari-hir/src/native/api/tests/forwarding.rs)
checks the same method, receiver, signature, impl bounds and associated output in direct
registration and independently parsed rendered source. A same-named member of another owner
is invalid before rendering. This slice does not change the standard conversions.
Local checks passed: 13 HIR `native::api::tests::` cases (including both
`qualified_script_callee` tests), two `blanket_method_applications` cases and the types
`native_compiler_input_uses_portable_complete_contracts` test. Workspace Clippy, structure,
format and diff checks passed; the full workspace test suite remains CI-only.

The integrated `registered_blanket_forwarding_authenticates_the_selected_member` test executes
an ordinary structured blanket from decoded bytes. A same-signature replacement produces a
different result with its own provider and is rejected by the original installation before any
modules publish. Workspace structure, fmt, Clippy and diff checks passed on the integrated changes.

## BI04 — Switch `Into` and `TryInto`; remove the reverse shortcut

One integrated cutover: ordinary library impls become the only source of reverse evidence. No
period in which a fallback masks a missing impl, and no parallel legacy reader after the switch.

| Owner | Required change |
| --- | --- |
| `core::convert` and admission | Register `Into<D> for S where D: From<S>` and the `TryFrom` counterpart with a projected `Error`. Enforce the authorized canonical impl declaration in HIR and [executable admission](../crates/kagari-contract/src/types/proofs/ownership.rs), including installed header, output and body checks. A same-named user trait remains ordinary. |
| HIR | Remove reverse-only branches for [expected-result inference](../crates/kagari-hir/src/typeck/body/methods.rs), bound search, associated outputs and [relationship exposure/shape checks](../crates/kagari-hir/src/language/items.rs). Keep exposure, signature and owner validation for remaining forward declarations; do not delete the whole shape validator. |
| Compiler and executable contracts | Remove the [reverse call rewrite](../crates/kagari-compiler/src/source/lower/instances/methods.rs) and intrinsic reverse bound/projection proofs. Emit the checked blanket method and its ordinary forward call. MIR/bytecode admission must reject missing impl evidence even if a suitable forward impl exists. No new VM instruction. |
| Registration, tools and portable records | Remove the `Reverse` payload from declarations, static sidecars, ID visitation/remapping, exports and [wire codecs](../crates/kagari-types/src/declaration/wire/leaf.rs). Migrate other reverse relationship producers, if found, to ordinary impls. Retain `TryFrom`'s forward method/error/result validation; no dummy reverse fields. Invalidate affected unpublished artifacts; do not bump format/ABI identifiers. |

**Structured registration recipe for BI04.** Obtain `forward`/`reverse` with `reference_trait`,
`forward_method`/`reverse_method` with `reference_method` (method arity zero), and, for
`TryFrom`/`TryInto`, `forward_error`/`reverse_error` with `reference_associated_type` (arity
zero). Use the actual `core::convert` owners. The fallible registration in the stdlib recipe
uses the existing foundation `bound` helper as follows:

```rust
module.trait_implementation_with(2, |types| {
    let source = types[0].clone();
    let destination = types[1].clone();
    let from = forward.apply([source.clone()])?;
    let into = reverse.apply([destination.clone()])?;
    let error = forward_error.projection(destination.clone(), from.clone(), [])?;
    Ok(TraitImplementation {
        receiver: source,
        interface: reverse_error.bind(into, error)?,
        bounds: vec![bound(&destination, from.clone())],
        bindings: vec![],
    }
    .with_script_method(reverse_method, move |body| {
        body.trait_call(&forward_method, destination, from, [], [body.parameter(0)?])
    }))
})
```

For `Into`, omit the projection and use `interface: into`. Both bodies submit an ordinary
`Call(TraitMethod { .. }, ...)`: `body.parameter(0)` addresses the projected reverse method's
`self`; it is the *value argument* to the static forward method, not its type receiver `D`.
A generic method uses `body.type_parameter(i)` for its own binder, excluding the impl prefix.
The [assembly regression](../crates/kagari-runtime/src/native/assembly/draft/traits/tests/script.rs)
checks that distinction after preparation and checks that no native entry exists. All 12 tests
in `cargo test -p kagari-runtime --lib native::assembly::draft::traits::tests::` passed; this
covers submission and preparation, not execution of the new forwarder. The builder
runs at seal to produce the expression, never when the method executes. This is not authority
to bypass ordinary Script checking or installed-body authentication.

**Acceptance:** strengthen the [existing reverse fixture](../crates/kagari/tests/fixtures/language/conversion_traits/reverse-conversions-call-the-destination-implementation.kgr)
for an observable single forward call and exact `Ok`/`Err` payloads. Its [bulk fixture runner](../crates/kagari/src/runtime/tests/language/conversion_traits.rs)
is commented out: wire a focused case into an active target. Compile with the real foundation;
repeat from bytes with a frontend-free consumer. Assert the reverse impl was selected and that
removing its evidence or forging the projected error/callee rejects. Preserve direct-impl
rejection for the intended reason, identity `From` (including object identity), `?` error
conversion and checked scalar/character conversions. Forwarding does not copy objects or grant
Rust loans; reuse normal rooting and trap cleanup. Install blanket impls at initial publication;
[reload](spec/module-activation.md#compatible-declaration-changes) cannot add blanket behavior to
existing types. A retained caller keeps its body; its next ordinary forward call selects the current
implementation. No new cache, GC representation or dispatch semantics.

## Checkpoints and validation

BI01 introduced bounded two-sided header comparison shared by HIR and portable named-header
admission (not specialized dispatch instances). Four `impl_headers_overlap` tests, local and sibling
header cases, standard override protection, portable header-only conflicts and linked proof visibility
were checked. The latter previously admitted competing sibling impls in the root inventory; it now
requires `InvalidPublicAbi` while retaining the separate module-local proof visibility assertions.
Structure, fmt, workspace Clippy and diff checks passed. Full workspace tests remain CI-only. Keep
intermediate checkpoints buildable; changes that must switch together belong in BI04. Record
actual results, carried failures and the next owner here. Use `Phase: BI0n` on requested commits.

**BI02 HIR method selection.** For `source.deliver(label)` in a `Packet<D>` context,
[method discovery](../crates/kagari-hir/src/typeck/body/trait_calls.rs) matches the declared
result and argument types to request `Reverse<D, Label>`. It reads established expression
facts rather than typing arguments under competing signatures. Receiver-only discovery cannot
publish an impl-local binder. [`selected_declared_interface`](../crates/kagari-hir/src/aggregates/relationships.rs)
uses bounded exact implementation search with the caller's assumptions and returns the substituted
header; a request alone does not create a checked call. Qualified static calls use the same proof
boundary. Reverse conversion metadata remains in place for BI04.

`cargo test -p kagari-hir --lib blanket_method_applications` exercises the
[ordinary trait fixture](../crates/kagari-hir/tests/fixtures/typeck/traits/blanket-method.kgr):
selected method and ordered trait arguments for concrete and open callers, argument-only static
selection, a qualified forward body, and rejection without a destination or forward evidence.
The existing reverse-conversions fixture also passed `kg check` (source checking only).
A broader `tests::typeck::traits::` run found a separate failure in
`trait_name_is_not_a_self_parameter_in_where_bounds`: it expected an `InvalidBoundTarget`
diagnostic on `Sequence`. This declaration has no call or method body; bound-target validation
is outside the method-selection slice. The same test failed on an isolated BI01 checkout at
`f38155423` (missing `InvalidBoundTarget`); it is not a BI02 failure. Portable bound/projection
proofs and the decoded-artifact `conditional_blanket_methods_execute_for_concrete_and_open_callers`
passed: both concrete and generic calls selected the expected method IDs and ordered arguments,
and execution returned 42. Structure, fmt, workspace Clippy and diff checks passed.

Start with the closest existing cases below and strengthen them where they lack a distinct
oracle. These are local filters, **not results**; verify a nonzero test count and that the new
scenario ran. Record the exact new filters as the cases are wired in.

```text
cargo test -p kagari --lib dependency_generic_implementation_overlap_respects_trait_arguments
cargo test -p kagari-hir --lib applied_impl_bounds_reuse_the_callers_generic_assumptions
cargo test -p kagari-contract --lib linked_proofs_discharge_generic_bounds_and_reject_ambiguity_and_cycles
cargo test -p kagari --lib imported_generic_conversions_and_iterators_link_to_their_defining_module
cargo test -p kagari --lib executable_from_adapters_reject_forged_source_types_and_targets
```

The last two protect existing `From`/`?` behavior, not reverse execution. For BI03–04 sample
structured vs offline foundation analysis, installed Script authentication and the existing
[source-free artifact consumer](../crates/kagari/tests/artifact_features.rs) with matching regenerated
input and providers. Keep the frontend-free feature split; a round trip in a frontend-enabled
binary is useful but not equivalent. Run the repository structure check, fmt check, workspace
all-target Clippy and `git diff --check` at implementation checkpoints. Full workspace tests and
exhaustive feature/backend/platform combinations are CI-only. On completion, move lasting
semantics to the trait/conversion and architecture owners, then retire this plan.
