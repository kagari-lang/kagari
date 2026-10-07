# Explicit Native Default Bodies

Status: planned; implementation has not started. The
[roadmap](implementation-roadmap.md#explicit-native-default-bodies) owns progress.

## Problem and intended result

Generated declarations currently show `List::sorted` as a bodyless requirement,
followed by a private `__default_List_sorted` function declaration. Registration
metadata supplies the missing relationship. Readers cannot tell from the trait
whether an implementation may omit the method.

Generate a real forwarding body in the trait, resolve and type-check that body,
and preserve the existing native execution contract. Example target output
(other members and the helper's existing bounds are omitted):

```text
pub trait List<T0>: core::ops::Index<usize, Output = T0>
    + core::iter::Iterable<Item = T0> {
    fn sorted(self) -> List<T0> where T0: core::cmp::Ord {
        __default_List_sorted::<Self, T0, <Self as core::iter::Iterable>::Iter>(self)
    }
}

// Private native entry declaration; the algorithm remains registered Rust code.
fn __default_List_sorted<T0, T1, T2>(receiver: T0) -> List<T1>
    where T0: core::iter::Iterable<Item = T1, Iter = T2> + List<T1>,
          T1: core::cmp::Ord, T2: core::iter::Iterator<Item = T1>;
```

The helper remains necessary as a typed native call target in this design. This
task exposes its relationship to the trait; it does not remove native registration,
move the algorithm into Kagari or require every concrete type to repeat it.
Required methods stay bodyless; explicit implementations still override defaults.

## Verified owners and constraints

| Owner | Current responsibility / required change |
| --- | --- |
| [stdlib list registration](../crates/kagari-stdlib/src/catalog/list_methods.rs) | `default` creates the private function and assigns `NativeDefaultApplication`; keep this single authored relationship. |
| [callable model](../crates/kagari-types/src/callable.rs) | `NativeDefaultApplication { declaration, arguments }` identifies a native template and type arguments in the trait/method binder, including `Self`. |
| [source renderer](../crates/kagari-hir/src/native/render.rs) | `Renderer::function` currently emits `;` for every function; render forwarding bodies for registered default methods. |
| [native source importer](../crates/kagari-hir/src/native/api.rs) | Validates non-trivia source correspondence; currently overwrites `has_default` from metadata. Require the parsed body to agree instead. |
| [body checking](../crates/kagari-hir/src/typeck/check.rs) | Already traverses functions with bodies, including trait defaults. Check generated calls in the declaring trait context. |
| [compiler default selection](../crates/kagari-compiler/src/source/lower/instances/defaults.rs) | Currently constructs the native import from metadata alone; require checked forwarding-call evidence for source compilation. |
| [interface emission](../crates/kagari-compiler/src/bytecode/interfaces.rs) | Uses symbolic native defaults for shared interface entries; preserve concrete overrides and generic receiver/iterator substitution. |
| [source-free default proof](../crates/kagari-contract/src/types/proofs/defaults.rs) | Validates native applications without HIR/source. Keep this boundary and registration authentication. |
| [SDK preparation/cache](../crates/kagari-embed/src/engine/declarations.rs) | Currently checks signatures before publishing generated files; also validate generated default bodies before admitting executable checked inputs. |

The language's [default-method rules](spec/traits.md#default-methods) require
default bodies to be checked in their declaring context even when unused.
Native algorithms, borrow/root cleanup, callbacks, cancellation and concrete
storage overrides retain their existing behavior.

## Representation and authority

1. **One registration recipe.** Keep `CallableImplementation::NativeDefault` and
   `NativeDefaultApplication` as the portable recipe needed by source-free linking.
   Derive generated text from it; do not add a second handwritten body string or
   independent target/generic mapping to `FnDecl`. Keep helper declarations private.
2. **A real parsed body.** Generate exactly one tail call, forwarding all value
   parameters in declaration order, including `self`, with explicit template type
   arguments. Use owner-aware type spelling for trait/method binders and qualified
   associated projections. Unit-returning defaults also use a tail call. Resolve
   helpers in the declaring provider, using a qualified path when required;
   never resolve them relative to the implementing module.
3. **Source agreement.** Retain full non-trivia correspondence validation, including
   the body. Compare syntax-derived `has_default` with registration; a missing,
   changed or extra default body is an error, not something metadata silently fixes.
   Only the helper gets the executable Rust entry binding. The method retains its
   default recipe/provenance, not a direct native entry pretending to be its body.
4. **Checked evidence, not another signature model.** Add
   `native_default_calls: HashMap<FunctionId, ExprId>` to `TypeTable`, mapping the
   generated method to its checked tail call. Existing `ResolvedCall` holds the
   canonical target, type arguments and applied signature. Populate this map only
   after ordinary resolution/type checking and recipe agreement succeed; expose a
   read-only getter. No marker based solely on registration or source spelling.
5. **A precise forwarding contract.** Verify the single-call shape, resolved target,
   ordered parameter bindings (once each), template arguments after normalization,
   result type and bounds. The callee must be the authenticated native template,
   not the trait method itself. Reject extra statements, reordered/duplicated
   arguments, arbitrary expressions and mismatched registered targets. This proof
   permits direct native lowering without introducing a new script call frame.
6. **Local identity lifetime.** The new table references the same lowering as its
   `FunctionId`/`ExprId`. Include it in body reuse/remapping and source-fact comparison;
   declaration/signature-only results must not claim body validation. Definition
   mapping continues through existing `ResolvedCall` records. Drop failed or stale
   proof entries rather than carrying IDs into a fresh arena.

Keeping the portable recipe is not a second independently authored implementation:
the source is its checked projection. Source compilation must consume checked call
facts; source-free installation must independently validate the executable contract
against the installed registration. Neither path may trust a name prefix as proof.

## Execution phases

| Phase | Changes and acceptance |
| --- | --- |
| ND01: render and import | Render bodies and accurate method/body source ranges; keep helper declarations and required/override methods distinct. Validate text-to-registration correspondence. Stop assigning default availability to bodyless trait methods. Update the native-view fixture, including method-local generics and associated iterator spelling. |
| ND02: check and publish | Resolve/check generated bodies and publish the checked-call map only on success. Cover `Self`, outer/method generic binders, associated projections, callback arguments and method bounds. SDK/provider preparation checks these bodies even if no script calls them; cache successful validation by the existing immutable provider/dependency inputs. Keep `signatures()` body-free: add preparation at the orchestration layer, not hidden work inside signature queries. |
| ND03: consume checked calls | Route source native-default lowering through the checked call and its declaring module. Reuse existing instance/native-import machinery after substituting receiver and method parameters. Update ABI/default materialization consumers and interface slots consistently. Retain source-free symbolic recipe validation; never fall back to unchecked metadata when source proof is absent. |
| ND04: integrate and document | Update affected native/default/navigation fixtures and docs, inspect regenerated declarations, remove obsolete assumptions and run focused acceptance. Refresh disposable generated caches only where needed. No format/ABI version bump or old-view reader is required for this unpublished change. |

ND01-ND03 form one integration checkpoint: do not leave a render-only change
published as complete. If an intermediate check fails, record the command/cause
and owning phase here; resolve all carried errors before that checkpoint commits.
Do not alter ordinary source default-method dispatch or rewrite native algorithms.

## Acceptance and focused validation

Use existing contract owners; extend their fixtures instead of adding a parallel
test family. Run only a few tests relevant to the current phase, once after the
coherent change. The following selectors are the initial acceptance set, not a
command list to repeat after every edit:

```text
cargo test -p kagari-hir native_view_
cargo test -p kagari-hir native_and_script_defaults_keep_source_identity_and_override_policy
cargo test -p kagari-embed --test list_algorithms custom_container_reuses_native_defaults_with_linear_read_traversal
cargo test -p kagari-embed --test list_algorithms generic_custom_receiver_uses_its_associated_iterator_in_default_calls
cargo test -p kagari-embed --test default_methods explicit_override_takes_precedence_over_the_default_body
```

- Extend native-view correspondence coverage for body removal/target/argument
  tampering; reuse existing trivia acceptance. Validate an unused malformed default
  at provider preparation and ensure cancelled preparation publishes no success.
- Update `native_defaults_do_not_create_script_implementation_bodies`: the trait
  now owns a checked forwarding body; implementing types still need no synthesized
  HIR body. Consolidate this assertion into the existing default-ownership fixture
  if it no longer establishes a separate contract.
- Existing list fixtures already round-trip artifacts and execute under GC checks.
  Verify both static and interface calls use defaults/overrides correctly. Add a
  row only if the relevant binder or dispatch combination is actually uncovered.
- Default-method navigation opens the trait method and visible body; navigation
  from its helper call resolves the private native declaration. Changed bodies
  invalidate dependent cached proofs; retained snapshots keep their old valid IDs.
- Preserve once-only, left-to-right argument evaluation, callback/trap effects,
  roots/borrow cleanup, cancellation, override policy and call-depth behavior.
  Direct lowering of the proved forwarding call avoids an extra script frame.
- Run changed-file formatting, `uv run --locked scripts/check_structure.py`,
  relevant documentation checks and `git diff --check` at integration. No local
  workspace tests, complete crate suites or feature/backend matrices; CI owns them.

Do not claim source-free/no-source-feature acceptance solely from a round-trip
test built with source support. GitHub CI owns that feature boundary and broader
runtime acceptance; report its status separately. Do not add dependencies from
contract/runtime back to syntax or HIR to implement validation.

## Progress

- [ ] ND01: Render and validate explicit forwarding bodies.
- [ ] ND02: Check bodies and publish reusable checked-call evidence.
- [ ] ND03: Consume checked calls in source compilation; preserve source-free linking.
- [ ] ND04: Complete focused acceptance, navigation and documentation.

Planning checkpoint: source consumers and existing contract fixtures inspected;
implementation and tests have not been run. No intermediate build errors are known.
