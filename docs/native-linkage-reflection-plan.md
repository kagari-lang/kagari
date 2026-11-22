# Declaration-linked Native helpers

**Status: NL01 implemented; NL02–NL03 pending.** The [roadmap](implementation-roadmap.md#native-model-replacement)
locates this follow-up to the existing [Native model](native-model-design.md#1-inputs-and-ownership).
Planning baseline: `afe502b8a`.

## Outcome and scope

Make Range and Result helpers ordinary private Native declarations and bind their
implementations through the verified executable contract. Remove
[`BuiltinFunction::from_name`](../crates/kagari-hir/src/builtin.rs) and its
expression-name fallback. HIR resolves and checks declarations; linking checks
that the implementation matches. No Rust implementation is needed to analyze a
call, and no source is needed to link a verified compiled product.

Reflection is in scope only to retire the old script helpers and their compiler
special cases. Static fields, indices, typed Host paths and debugger inspection
remain independent. The [script reflection design](spec/reflection.md#optional-script-reflection),
including whether to offer a `std::reflect` module and the shape of any API, is
not part of this plan. Moving basic `String` methods is a separate consumer of
the binding route; any `String.len` fast path requires separate measurements.
Standard-library bundle selection and collection provider restoration are out of scope.

## Evidence and ownership

| Layer | Current evidence | Intended responsibility |
| --- | --- | --- |
| Core source and Native module declarations | [Packaged core inputs](../crates/kagari-hir/src/analysis/core_source.rs), [Native syntax](spec/standard-declarations.md#explicit-native-syntax-and-fields) | One canonical declaration per function. Core owns its private operations. |
| HIR | [Declaration preparation](../crates/kagari-hir/src/analysis/declarations.rs), [name fallback](../crates/kagari-hir/src/resolver/resolve.rs), [helper checking](../crates/kagari-hir/src/typeck/body/host_access.rs) | Resolve ordinary names and signatures, including generic arguments and visibility. Do not consult an installed Rust body or add another catalog for these names. |
| Compiler and executable contracts | [Native imports](../crates/kagari-contract/src/native_import.rs), [Range and Result operation validation](../crates/kagari-contract/src/operations.rs) | Emit the selected declaration, applied signature and dependency in the normal verified call contract. Preserve portable identities, not frontend-local IDs, field-name arguments selected by HIR, or Rust function pointers. |
| Runtime installation and linking | [Native registry](../crates/kagari-runtime/src/native/registry.rs), [verified authored catalog projection](../crates/kagari-runtime/src/native/catalog/authored.rs) | Bind implementations against verified declarations without publishing a second module owner. The existing public authoring projection is not authority for private functions; use their verified executable contracts at the linking boundary, not broader source exports. |
| Runtime inspection | [Reflection kernels](../crates/kagari-runtime/src/reflection.rs), [public field lookup](../crates/kagari-runtime/src/native/objects/fields.rs), [reflection model](spec/reflection.md) | Keep protected access and validation still used by Host/debugger or ordinary typed operations. Retire script-only helper paths with no remaining consumer. |

Two axes are independent: **where the declaration comes from** and **who supplies
the implementation**. Structured `ModuleDecl`, external declaration documents and
ordinary project/core source converge on the same static model. A Native declaration
can check and compile without its implementation. Loading must validate a real
binding before publication. Tooling origins and debug source are not execution
authority. Do not turn a registered binding into a second declaration source.

For example, [`Range<T>::start_bound`](../library/core/ops.kgr) used to call
`__range_start_bound(self)` without a declaration. It now calls private
`range_start`, so HIR checks the argument and gets `Bound<T>` from the signature.
The compiler records the dependency and the loader checks the Runtime binding
against it. Calling `start_bound` still produces the same lower bound. A user
function with the same spelling in another module is a different declaration and
does not select the core implementation.

## Implementation phases

Each phase leaves an independently checked build. Use existing modules and
scoped contracts first; no new general provider framework or alternate HIR model.

### NL01 — Bind an existing authored Native declaration

Connect the missing implementation-only route in Runtime/SDK setup using one
private Range projection as the witness. Its checked declaration remains in
`core::ops`. Load the Native declaration from the verified executable contract;
match the fixed foundation binding by canonical owner/member identity and complete
applicable signature, policies and operation contract. A product claiming a path
or containing internally consistent records does not by itself authorize a core
binding. Keep independent checks in the provider/linker; do not trust caller-supplied
intrinsic tags, broaden `with_verified_modules` into a private export surface, or
install a second `core::ops` declaration input. Run the foundation binding route
with no optional stdlib. Source-free consumers use compiled declarations and
ordinary binding setup, not a frontend or a separately maintained declaration file.

**Gate:** HIR resolves the authored declaration without an executable binding.
The correct source and decoded-product paths execute; missing, duplicate, foreign
or mismatched binding evidence rejects before publication. A same-named user
function remains ordinary and an external caller cannot import the private helper.

### NL02 — Migrate Range and Result helpers

Use private typed Native declarations in `core::ops` and `core::result`; reuse the
existing runtime kernels. Range contracts cover all current range shapes and keep
the endpoint type exact, including the result-context element of `RangeFull`.
Result contracts preserve the canonical `Result` and `!` residual constraints,
reporting `Err` carrier validation and owner/caller authorization in source and
portable verification. Prefer signatures that express the two distinct transfer
routes (making a residual versus forwarding a converted error). Keep the one
ordinary `From` evaluation, original detached origin and stack, and existing
trap/evaluation order. Remove each magic-name branch with its last consumer; do
not retain a compatibility fallback.

**Gate:** Existing Range endpoint and Result origin tests still distinguish
inclusivity, unbounded ends and converted errors. An independently decoded invalid
operation/import cannot weaken current access or type checks. Check execution
without optional stdlib. Retire obsolete executable forms only after checking
consumers; do not remove the separate range-construction contract.

### NL03 — Retire the legacy script helpers and name fallback

Audit consumers of `type_of`, `get_field`, `set_field` and `set_index`. Migrate
static uses to ordinary typed field/index operations where equivalent. Preserve
once-only evaluation, mutation, result values and trap order: old `set_index` on
a tuple returns a replacement tuple, not an in-place write. Retire tests dedicated
to removed helper semantics after preserving independent assertions for surviving
contracts. Record any genuine dynamic consumer without an equivalent and its
lost capability; do not replace it with a misleading static rewrite or an ad hoc
reflection API.

Remove the name fallback, special HIR literal/context rules and obsolete
executable helper paths with no remaining consumer. Keep existing protected
Host/debugger inspection, typed access and their validation. No replacement
script reflection module or compatibility alias ships in this phase.

**Gate:** `BuiltinFunction::from_name` and its call-site fallback are gone. Calls
without declarations use ordinary unresolved-name diagnostics; user declarations
with the former helper names resolve normally. Ordinary typed access and retained
Host/debugger contracts still work. Refresh affected unpublished artifacts without
ABI identifier bumps and check retained source-free and lifecycle coverage. Move
lasting behavior to its spec/architecture owner and retire this plan when the
gates close.

## Validation and progress

NL01 validates the private Range template against an independent Runtime-owned
contract, reserves its binding identity against registration, and checks caller
ownership and exact semantic call slots on the portable route. The ordinary
registry remains the binding authority for non-foundation declarations. Local
checks passed: HIR private import/same-named function, compiler forged Range
operands, Runtime authored Range bounds from compiled and decoded products
(including a foreign import denial), workspace all-target Clippy, Rust format,
structure and diff checks. Core fixture regeneration is batched with the remaining
portable changes. No full workspace/backend matrix was run; that is CI scope.

Subsequent checkpoints select focused owner tests and directly affected
integration routes as well as the workspace lint, format, structure and diff
checks. Preserve independently useful source-free, GC/borrowing, cancellation
and reload assertions.
