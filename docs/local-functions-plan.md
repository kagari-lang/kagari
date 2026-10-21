# Named local functions execution plan

Status: active; implementation authorized on 2026-10-21. The user selected noncapturing
local functions on 2026-10-21, including forward references and recursion with
outer data passed explicitly. The rules below are the accepted execution contract. This plan owns LF01-LF06, acceptance and the progress ledger;
the [roadmap](implementation-roadmap.md#review-follow-up-order) owns activation.
Grammar, typing and executable support are implemented; tooling/lifecycle acceptance remains active.

## Outcome and scope

Implement [SA14](review.md#sa14-function-bodies-cannot-declare-named-local-functions):
named functions declared inside callable bodies and their nested blocks, with
ordinary checked calls and function values, lexical visibility and no implicit
environment capture. The requested example must execute to `42`:

```kagari
fn outer() -> i32 {
    fn inner(x: i32) -> i32 { x + 1 }
    inner(41)
}
```

Include declarations inside methods, closure blocks and other local functions;
own generic parameters/bounds; explicit async functions under the existing async
contract; source diagnostics, navigation, formatting and lint integration. Preserve
module functions, methods, closures, checked execution and generation pinning.

Exclude local ADTs/traits/impls/imports, capturing named functions, new const
evaluation capabilities, tail-call optimization, general incremental compilation,
new backend capabilities and SA11 self-ID removal. A function declaration in a
constant initializer is outside this delivery and must receive a diagnostic.
Keep existing format/runtime identifiers; update affected development artifacts
only if their contracts actually change. No performance target is asserted.

## Language contract

| Area | Rule |
| --- | --- |
| Declaration | A block statement contains an ordinary `fn` or `async fn` definition with a required body. No trailing semicolon is required. Parameter types and omitted-result-as-unit rules match module functions. |
| Visibility | A name is visible throughout its declaring block and descendants, including before its textual declaration. It is unavailable outside that block, through module paths, imports or exports. `pub` and restricted visibility on local functions are errors. |
| Recursion | Self recursion and mutual recursion in the same block are supported. Collect all local headers before resolving bodies. Calls retain normal call-depth limits and cancellation behavior. |
| Duplicate names | Two local function declarations with the same name in one block are errors. A declaration in an inner block may shadow an outer function or module name. There is no new overloading rule. |
| Value shadowing | Parameters and `val`/`var` bindings keep their existing lexical rules. A block-wide function binding is installed first; a same-block value binding shadows it after that binding's initializer, regardless of the function declaration's textual position. A nearer block wins over an outer block. |
| No capture | A local function cannot read or write enclosing parameters, locals, pattern bindings or method `self`. Lookup must diagnose an inaccessible enclosing value rather than silently select a same-named module item. Pass required values as parameters or use an existing closure. |
| Generic boundary | Outer function/impl/trait generic parameters and contextual `Self` are not inherited. A local function can declare its own generics and bounds, including a binder with the same spelling as an outer binder. Callers may pass their own type arguments to that independent declaration. |
| Static names | Module declarations/imports and lexically visible named functions remain available across the local-function boundary. The barrier applies to enclosing value bindings and generic context, not to static callable declarations. |
| Function values | A checked, concrete local function can be passed, stored and returned through the ordinary function type. Generic function values follow existing specialization rules; no polymorphic first-class values are added. Leaving the declaring block does not invalidate a returned callable. |
| Execution | A declaration has no runtime initialization or body execution at its statement position. Only calls execute its body. Return, propagation, loop control and await obey the innermost callable's rules; control flow cannot jump into an enclosing callable. |
| Closures | A closure inside a local function may capture that local function's parameters/locals. Neither such a closure nor another nested function may bypass the enclosing local-function capture barrier. A closure outside it may reference the local function as an ordinary callable. |
| Attributes | Reuse applicable function attributes and lexical lint policy. Reject attributes requiring module export/registration or another unsupported target; do not silently ignore them or publish local declarations. |

For example, this proposal supports a forward call and explicit state passing:

```kagari
fn outer(base: i32) -> i32 {
    val result = add(base, 1);
    fn add(value: i32, delta: i32) -> i32 { value + delta }
    result
}
```

Changing `add` to read `base` directly is an error, even though its source is
nested inside `outer`. Declaring `add<T>(value: T) -> T` is allowed; referring to
an undeclared outer `T` inside `add` is not. These rules keep local `fn` as a static
callable declaration and leave captured state under the existing closure owner.

## Existing implementation and required ownership changes

The inspection baseline is `ef2f887d`. These are architectural integration points,
not evidence of a current accepted-program bug:

- [Block parsing](../crates/kagari-syntax/src/parser/grammar/stmt.rs),
  [AST statements](../crates/kagari-syntax/src/ast/stmt.rs) and
  [HIR statements](../crates/kagari-hir/src/hir/stmt.rs) lack local declarations.
  Reuse the existing function parser and `FnDef`; retain malformed local functions
  as recoverable nodes without consuming the next sibling or block tail.
- [Function lowering](../crates/kagari-hir/src/lower/item.rs) allocates the
  source-map ID, lowers the whole body, then returns the record for insertion.
  Recursively appending a child during that interval would put the child in its
  parent's vector slot. Reserve IDs and storage slots together before descending,
  then fill those exact slots. Any incomplete builder state stays private and
  cannot become a published `LoweredModule`.
- [Function records](../crates/kagari-hir/src/hir/item/function.rs) share
  `Module.functions`, but storage does not imply module membership. Add an explicit
  local origin and a link to its declaring lexical block/enclosing body. Keep
  `StmtKind` linked to the child's `FunctionId`; child signatures/bodies receive
  `BodyOwner::Function(child)`. Keep function rows, source-map rows and IDs aligned,
  including methods that contain locals. Reserve block ownership before children
  where required; avoid a second function store or fake module items.
- [Declarations](../crates/kagari-hir/src/declarations.rs) currently give ordinary
  user functions module-root identities. Intern locals beneath their enclosing
  callable definition, with deterministic per-name occurrences in lexical source
  order to distinguish same-named declarations in separate blocks. A function
  inside a closure belongs to its nearest named callable for identity ancestry;
  its actual lexical block remains explicit. Nested locals use their local parent.
  These are compilation identities, not an edit-stable public naming scheme.
  Preserve complete source-unit/revision/arena checks and portable identity
  validation. Internal definitions must not become importable namespace members.
- [Body resolution](../crates/kagari-hir/src/resolver/resolve.rs) currently visits
  statements sequentially, while [selected-body resolution](../crates/kagari-hir/src/resolver/collect.rs)
  starts each function independently. Build a shared lexical declaration index,
  predeclare block functions, and reconstruct the required static ancestor scopes
  for a selected local body. Do not type-check the enclosing body just to resolve
  its child. Resolve every body once; crossing a function boundary must reset
  dynamic bindings, closure capture state and control-flow context appropriately.
- Signature checking must collect all local callable signatures before checking
  bodies, using only each declaration's own generic environment. Reuse ordinary
  checked call facts, generic instantiation and diagnostic owners. Invalid unused
  local bodies still prevent complete-program acceptance.
- [Signature reuse](../crates/kagari-hir/src/typeck/signature_reuse.rs) currently
  erases outer function bodies, and [body reuse](../crates/kagari-hir/src/typeck/reuse.rs)
  assumes the existing declaration structure. A local signature edit inside an
  erased parent body must invalidate dependent facts. In this delivery, reject
  changed-revision signature/body reuse for a logical module containing local
  functions unless its new lexical declaration environment is explicitly proved
  equivalent. Keep exact-revision reuse and existing reuse for unaffected modules.
  This bounded conservative rule must land before local facts can be published;
  finer reuse remains R8, not a second caching project.
- [Compiler entry selection](../crates/kagari-compiler/src/source/lower/mod.rs)
  and instance planning must consume checked local identities for direct calls,
  function values, generic instances and async factories. Locals are internal
  callables reached through the existing planner, not additional exports. The
  declaration statement emits no executable operation. Backends consume checked
  facts without name resolution, capture inference or new calling conventions.

Retain `Function.id` and other self IDs throughout SA14. Record the resulting
allocation/ownership interface for SA11 to review independently. Review visitors,
selected-body queries, attribute collection and debug traversal so that child
bodies are neither omitted nor accidentally treated as part of their parent's
return, capture, reachability or usage analysis.

## Mature implementation references

Consulted on 2026-10-21. Rust Reference and rustc development-guide links are
living documentation, not pinned implementation revisions. The Kotlin functions
page is dated 2026-09-24.

- [Rust items](https://doc.rust-lang.org/reference/items.html) and
  [scope rules](https://doc.rust-lang.org/reference/names/scopes.html) establish
  block-wide item visibility, declaration-order independence, value-binding
  barriers and independent inner generic parameters. Adopt those mechanisms for
  the selected noncapturing model. Kagari still uses its own shared-object,
  checked-call, async and GC contracts; Rust ownership/lifetimes are not imported.
- [rustc HIR storage](https://rustc-dev-guide.rust-lang.org/hir.html) separates
  parent links from out-of-band item/body records. Apply the ownership principle
  to Kagari's existing function table, source maps and body owners; do not copy
  rustc's query engine or replace all ID families.
- [Kotlin local functions](https://kotlinlang.org/docs/functions.html#local-functions)
  demonstrate the alternative of local functions that access enclosing variables.
  The user selected the noncapturing model. No new environment initialization,
  recursive closure group or hidden captured-parameter convention is needed for
  named declarations. Existing closure runtime ownership remains unchanged.

## Execution phases and commits

Execute in order after implementation is authorized. Each phase is a coherent
checkpoint with its own Conventional Commit and a `Plan-Phase: LF0N` trailer.
Use `!` and explain downstream impact when an exposed Rust model changes.

### LF01 Contract and integration inventory

Freeze the proposed semantic table in this plan, map the existing contract-test
owners, and specify local origin/block links, slot reservation and definition
occurrence ordering with one parent/two-child example. Inspect generic methods,
closures, async functions, selected-body queries and signature reuse before coding.
Add the local-function syntax/specification changes alongside implementation in
LF02-LF04; LF01 must not claim the feature already exists.

Acceptance: every semantic choice has an owner and an expected success/error;
the phase ledger records the selected tests and any concrete design adjustment.
Documentation checks suffice. Suggested commit:
`docs(hir): Define local function contracts and integration gates`.

### LF02 Grammar and declaration ownership

Extend block grammar, AST statements, HIR local origin/statement links and lowering.
Implement private slot reservation/finalization, declaration identities and block
membership without adding local functions to module items or export scopes.
Update grammar/specs as staged support and preserve parser budgets, cancellation,
physical offsets and recovery. Adapt all affected exhaustive matches coherently.

Acceptance: focused parser/HIR tests establish nested ownership, method and sibling
allocation, same-named functions in different blocks and recovery after malformed
declarations. Downstream stages not yet implemented must return an explicit staged
unsupported diagnostic/error; no checked executable can silently omit a local.
Suggested commit: `feat(hir)!: Represent block-local function declarations`.

### LF03 Lexical resolution and typing

Implement block predeclaration, lookup/shadowing, capture and generic barriers,
selected-body scope reconstruction, independent signatures and recursive calls.
Integrate navigation identities and the conservative reuse rule before publication.
Check local bodies even when the enclosing code is unreachable or the local is
unused. Preserve cancellation without publishing partially resolved facts.

Acceptance: forward calls, mutual recursion, independent generics, closure boundary
cases and forbidden capture have correct types/diagnostics. Editing a local name,
signature or body cannot reuse stale facts; retained snapshots still navigate to
their original source. Compiler lowering retains its explicit unsupported gate
until LF04. Suggested commit: `feat(hir): Check lexical local functions`.

### LF04 Executable calls and function values

Extend existing instance planning and lowering for local direct calls, generic
instances, escaping function values and explicit async factories. Reuse ordinary
MIR/function-value/runtime paths and remove all staged unsupported gates. Keep
internal identity metadata separate from source namespace and export metadata.

Acceptance: the requested example returns `42`; forward/mutual-recursive calls and
escaped local callables execute correctly. A representative program serializes,
loads without source and executes with internal calls intact and locals absent
from exports. Exercise ordinary generic and async paths with focused cases.
Suggested commit: `feat(compiler): Execute checked local function calls`.

### LF05 Tooling and lifecycle contracts

Finish formatter/comment attachment, lint ownership and suppression, declaration
navigation, scope completion and debug function/source sites. Verify local calls
through the existing root/call owner: recursion depth, cooperative cancellation,
trap cleanup, GC of callable values and retained old generations after reload.
Extend the current lifecycle owners only for distinct local-function boundaries;
reuse existing module-function and closure regression cases.

Acceptance: diagnostics/navigation target the right declaration after shadowing
and source edits; formatting is idempotent; a retained callable still runs its old
local implementation after reload while new calls use the new implementation.
Source-free loading preserves the same behavior and does not require HIR.
Suggested commit: `test(language): Verify local function tooling and lifecycle`;
use `fix`/`feat` instead if substantive implementation work dominates the phase.

### LF06 Local acceptance and documentation closure

Resolve carried errors and finish the selected tests, structure review and required
Clippy/lightweight checks. Update current syntax, EBNF, syntax coverage, HIR/syntax
architecture and an existing function example. Record pending exhaustive CI in the
roadmap separately from passing local validation. Move any unresolved bounded
follow-up to its existing review owner. Remove the resolved SA14 finding and this
completed plan; Git history retains the ledger. Leave SA11 queued and unstarted.

Acceptance: no staged rejection, temporary representation bridge, known integration
failure or missing selected test remains. Suggested commit:
`docs(language): Publish local function contracts and retire plan`.

## Validation and intermediate build policy

Keep implementation checkpoints buildable. Internal API edits may span their
direct consumers in one phase; do not make standalone commits with fake records,
unchecked facts or production `todo!()` paths. LF02-LF03 explicitly expose staged
support only, with real rejection at the unimplemented downstream boundary. Remove
that rejection in LF04. Unexpected failures must be recorded with reproduction,
representative diagnostic, cause and owning phase; disclose broken builds in commit
bodies rather than presenting them as passing checkpoints.

| Boundary | Selected local evidence |
| --- | --- |
| Grammar and HIR | One focused group for local declarations, recovery/limits, block tails, Unicode/CRLF sites and parent/child storage alignment. |
| Resolution and typing | Forward/self/mutual calls; duplicate/shadowing rules; rejected outer values and generic context; own generics; methods/closures/async; no module lookup/export leakage. |
| Query correctness | A selected local body resolves with ancestor declaration scopes; edits to nested signatures invalidate reuse; old/new snapshots remain distinct; cancellation does not publish partial results. |
| Execution and artifacts | Basic `42`, recursion, returned function, generic instantiation and async result; representative encoded source-free execution with private internal targets. |
| Runtime boundaries | Retained callable across reload, managed arguments/results through GC and traps, recursion depth and cancellation using existing harnesses. |
| Source tools | Formatting round trip/idempotence, scope/name navigation and lint handling with a local shadow or nested unused function. |

Use the existing `kagari-syntax`, `kagari-hir`, `kagari-compiler`, `kagari-format`,
`kagari-lint`, `kagari-analyzer` and SDK `kagari` test owners where affected. Select
test-name/target filters in each phase, not whole package suites. Add cases only
where the current semantic owner lacks a distinct contract. Local validation is
representative, not the sum of all package tests; avoid duplicating a full sweep.
Use `--no-fail-fast` for multiple selected test targets and record skipped tests.

At each implementation checkpoint, review changed handwritten Rust ownership,
imports/re-exports, effective LOC, paths and mixed responsibilities, and run:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Reuse passing evidence for unchanged code. Documentation-only checkpoints use
content/link/diff checks. Full-workspace tests and complete feature/backend/platform
validation remain CI-only. Representative local backend tests may use already
supported capabilities; unsupported native capabilities must keep their existing
diagnostic/fallback contract. Never report unrun CI as passed.

## Progress ledger

- Planning baseline: `ef2f887d`; working tree was clean before this document.
- User decision: noncapturing local functions with forward references/recursion;
  outer data is passed explicitly. Goal execution is authorized.
- Current carried implementation errors: none. LF01-LF04 implementation and selected checks pass.
- Structural debt: no new exception requested. LF02 reviews function/block storage;
  LF03 owns bounded conservative reuse; finer invalidation remains R8.
- [x] LF01 Contract and integration inventory.
- [x] LF02 Grammar and declaration ownership.
- [x] LF03 Lexical resolution and typing.
- [x] LF04 Executable calls and function values.
- [ ] LF05 Tooling and lifecycle contracts.
- [ ] LF06 Local acceptance and documentation closure.

During execution, append each phase's changed contracts, actual selected commands,
results, carried failures and commit reference here. Do not create a parallel ledger.

### LF01 checkpoint

Activated from `bd10e121` with a clean working tree. The language table is frozen
for this execution. Function origin is `Local { block: BlockId }`; the block's
owner identifies the enclosing callable (or a rejected constant context).
`StmtKind::Function(FunctionId)` links the declaration without visiting its body.
Private lowering slots use `Option<Function>` until finalization; published HIR
contains only complete records. Blocks reserve their final row before children.
For `outer { fn a() {} fn b() {} }`, allocation reserves outer slot 0, a slot 1,
b slot 2, and finalizes exactly those rows despite completing children first.
Methods use the same allocator. Semantic identities are outer/a/0 and outer/b/0;
two a declarations in separate blocks receive occurrences 0 and 1 beneath outer.
No local enters `Module.items` or a namespace/export table.

The signature checker already visits the shared function table before checking
bodies. Local generic contexts can reuse that owner, while resolution needs
static lexical ancestry for independently selected bodies. Existing body-elision
reuse must reject modules with local declarations on either side of an edit.

Selected owners: syntax parser/AST tests; HIR lowering/resolver/typeck and analysis
body-query tests; compiler source-program/artifact tests; SDK runtime embedding
and async integration tests; formatter/lint/analyzer contract tests. Add focused
local-function groups at these owners and select filters per checkpoint. Existing
ordinary body-edit reuse and closure capture cases provide the small regression
set; complete backend/platform testing remains CI-owned.

Validation: document content/link review and `git diff --check` passed. No build
was required. No carried failure or structural exception.

### LF02 checkpoint

Implemented shared FnDef block parsing/AST statements, explicit local origin and
statement links, private function reservations, reserved block rows, nested
semantic definition ancestry and module/export exclusion. Methods use the same
function allocator. Recoverable syntax retains following declarations and tails.
Semantic checking rejects locals with a staged LF03 diagnostic; compiler statement
lowering has a staged LF04 error. These are removed by their owning phases.

Passed: `cargo test -p kagari-hir --lib local_function_rows_preserve_owners_and_module_membership`
and `cargo test -p kagari-syntax --lib local_function_syntax_preserves_recovery_and_offsets`.
The first establishes parent/child/method storage and generic ownership; the second
covers attributes/async headers, Unicode/CRLF source sites and malformed sibling
recovery. Structure check (1404 files), fmt, workspace all-target Clippy and diff
checks pass. An initial completion-visitor return-type mismatch and a Clippy
needless-return finding were repaired; no carried build failure. No structural
exception. Full CI remains pending.

### LF03 integration decisions

Lexical ancestry is recorded by the existing resolver as declaration-time active
scope snapshots after block predeclaration. A selected local body resolves only
its ancestor chain plus itself, once per body; it does not type-check ancestors
or resolve unrelated functions. This avoids a parallel lexical walker/index while
retaining shadowing positions, closure parameters and pattern scopes. Crossing
owners blocks value capture, including through nested closures; static local
function bindings remain visible. Navigation filters the same owner boundary.

Inspection and execution of the typing tests corrected an initial assumption:
named declarations previously supported direct calls but were not first-class
values (closures already were). The authorized escaping-local contract therefore
adds local-function-value specialization using existing inference/bounds and
checked callable-application facts. LF04 uses MakeClosure with no captures and
the existing function target. Module/native/method name-as-value support is not
expanded. No new runtime environment or calling convention is introduced.

### LF03 checkpoint

Implemented block predeclaration, independent local signatures, duplicate/visibility
and constant-context diagnostics, noncapturing lookup/visible-scope boundaries,
selected ancestor resolution, callable-value specialization and conservative reuse.
Declaration navigation retains static function definitions rather than creating
binding IDs for them. The compiler rejects modules containing locals until LF04.

Passed HIR tests: `local_function` selected five tests (the callable-contract test
was rerun separately after its fix), plus
`body_edits_reuse_other_bodies_but_signatures_invalidate_them`,
`declaration_pattern_read_only_and_mutable_leaves_keep_usage_and_captures`, and
`snapshots_reuse_unchanged_files_and_cancellation_does_not_publish`.
They cover forward/mutual calls, generic and async headers, local function values,
methods/closures, capture rejection, duplicate names, export-scope isolation,
selected-body queries, nested edits, retained navigation and ordinary regressions.
An initial binding collector assumption that every lexical binding was a runtime
local caused a panic; static function bindings now keep their declaration IDs.
The function-value test exposed the missing capability recorded above; it is now
checked through existing specialization. No remaining test/build failure.

Structure check (1406 files), fmt, workspace all-target Clippy and diff checks
pass. No structural exception; exhaustive CI remains pending.

### LF04 checkpoint

Removed staged executable gates. Direct calls reuse ordinary instance planning;
local function values use checked applications and MakeClosure with no captures.
Declarations emit no instruction. Imported trait default bodies retain authored
source/binder ownership while placing local executable identities in the receiving
module. A planner-owned mapping from authored declaration to receiving identity
lasts for that planner only; it ensures recursive function-value references reuse
one placement, independently of caller substitutions. Generic instance keys and
limits remain unchanged. No artifact/ABI version bump or verifier weakening.

Passed `cargo test -p kagari --lib local_functions_execute_from_artifacts_without_export_leakage`
(source-free artifact round trip, forward/mutual/generic/value/closure/method calls,
async execution and export exclusion) and
`cargo test -p kagari-compiler --lib local_function_values_in_imported_default_methods_keep_source_ownership`
(imported default, recursive generic function-value reference, MIR/bytecode round
trips and verification). The imported case initially failed InvalidInstance;
receiving placement now follows the existing imported-body ownership contract.
`cargo test -p kagari-syntax --lib async_callable_syntax_contract` passes after
restricting block-head async declaration parsing to async followed by fn; async
closures keep their expression path. Structure check, fmt, workspace all-target
Clippy and diff checks pass after formatting repair. No carried error or structural
exception. Full backend/platform coverage remains CI-only.
