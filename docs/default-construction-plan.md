# Default and struct update execution plan

## Status and authority

Status: active; implementation and one coherent commit per phase authorized on
2026-10-17. DF01-DF04 are complete; DF05 final handoff is next. Remote publication is not authorized. The [roadmap](implementation-roadmap.md#default-and-struct-update)
owns activation and queue placement. This is the single execution plan for
[SA26](review.md#sa26-add-ordinary-default-construction-and-struct-update-expressions),
including DF01-DF05, design decisions, focused acceptance and the progress ledger.
Examples below describe the target, not currently supported standard APIs.

Language behavior remains owned by [syntax](spec/syntax.md),
[traits](spec/traits.md), [values](spec/value-semantics.md) and
[native declarations](spec/standard-declarations.md). Update those owners as each
vertical slice becomes executable; do not mark the whole feature implemented early.

## Outcome and finite scope

Deliver an ordinary core-library trait and independent struct update expressions:

```text
trait Default {
    fn default() -> Self;
}

struct Player { var health: i32, val items: Vec<i32> }
impl Default for Player {
    fn default() -> Self { Player { health: 100, items: Vec::new() } }
}

val first = Player::default();
val inferred: Player = Default::default();
val changed = Player { health: 200, ..Default::default() };
val updated = Player { health: 200, ..first };
fn make<T: Default>() -> T { T::default() }
```

Included: ordinary trait registration/publication and prelude, a finite standard
implementation inventory, contextual static associated calls, struct expression
syntax/AST/HIR/checking/lowering, source tools and editor grammar/highlighting,
representative source-free execution, cleanup and generation-pinned calls.

Excluded: derive, enum default attributes/automatic selection, field declaration
initializers, new default keywords/protocols, blanket implementations, general
trait-solver expansion, SA22 interface-construction witnesses, SA25 authoring
redesign, collection storage migration, host/view update syntax, Rust moves/borrows,
new native instructions and broader native coverage. Existing native default-body
recipes are not a universal Default constructor. No new executable format/runtime
ABI identifier or compatibility reader is required by this design.

The previously observed SA16/SA17 editor gap was independently fixed by the merged
`890b6a78`/`cf419275` work before activation. Reuse its declaration/rest grammar and
tests. SA26's own expression syntax must ship with editor support; passing selected
SA26 checks must not be presented as complete editor-language parity.

## Baseline and ownership inventory

Planning baseline: clean `66bafba7`, inspected on 2026-10-17, after the editor tooling
merge. Source and existing tests were inspected; no builds, behavior tests or npm
installation ran for this planning task. The earlier editor parse probe could not
run because the local npm tree-sitter dependency was absent; this is a tooling
prerequisite, not a newly reproduced SA26 parser failure. Reconfirm owners at DF01.

| Owner | Current mechanism and required change |
| --- | --- |
| [Registration key](../crates/kagari-stdlib/src/catalog/key.rs), [construction traits](../crates/kagari-stdlib/src/catalog/construction.rs), [assembly](../crates/kagari-stdlib/src/catalog/mod.rs) | No Default registration exists. Add a private registration key and required static method through ordinary contracts, without a language role or conversion adapter. |
| [Namespaces](../crates/kagari-stdlib/src/namespaces.rs), [partition](../crates/kagari-stdlib/src/catalog/partition.rs), [documentation](../crates/kagari-stdlib/src/catalog/documentation.rs) | Own canonical trait placement, std facade/prelude exports, native function placement and exhaustive API documentation. Update together, including inventory assertions. |
| [Construction implementations](../crates/kagari-stdlib/src/catalog/construction_defaults.rs), [native bindings](../crates/kagari-stdlib/src/bindings.rs) | Reuse explicit implementation registration, checked result types and allocation helpers. Do not route constructors through a method-name heuristic or an arbitrary zeroed-value helper. |
| [Trait-call sites](../crates/kagari-hir/src/typeck/body/trait_calls.rs), [method checking](../crates/kagari-hir/src/typeck/body/methods.rs), [inference](../crates/kagari-hir/src/typeck/inference.rs) | Concrete/qualified/generic static calls already have paths. Trait-qualified owners are excluded before result context can infer Self. Introduce general unresolved-Self static-call handling, then reuse checked selection and call facts. |
| [Compiler calls](../crates/kagari-compiler/src/source/lower/expr/calls.rs), [applied methods](../crates/kagari-compiler/src/source/lower/expr/operators.rs) | Already consume checked receiver types for static trait dispatch. Preserve this handoff; no runtime trait search or Default spelling recognition. |
| [Expression parser](../crates/kagari-syntax/src/parser/grammar/expr.rs), [AST](../crates/kagari-syntax/src/ast/expr.rs), [syntax kinds](../crates/kagari-syntax/src/kind.rs) | Struct bodies currently contain explicit named/shorthand fields only. Add an identifiable terminal update-base child and accessor, distinct from a field initializer or range expression. |
| [HIR expressions](../crates/kagari-hir/src/hir/expr/mod.rs), [lowering](../crates/kagari-hir/src/lower/expr/mod.rs), [resolver](../crates/kagari-hir/src/resolver/resolve.rs) | Add optional base expression and visit it after explicit fields, including names/captures and malformed-source recovery. |
| [Constructor typing](../crates/kagari-hir/src/typeck/body/constructors.rs), [type facts](../crates/kagari-hir/src/typeck/table.rs) | Inference currently finalizes arguments after explicit fields and then requires every field. Include base constraints before finalization; retain explicit-field facts and separately record checked omitted fields. |
| [Completion](../crates/kagari-hir/src/typeck/completion.rs), [constant checking](../crates/kagari-hir/src/typeck/check/constants.rs), [definition mapping](../crates/kagari-hir/src/typeck/table/mapping.rs) | Add base traversal and omitted-field identity mapping/visitation. Preserve early exits, cancellation and rejection of nonconstant expressions. |
| [Aggregate MIR lowering](../crates/kagari-compiler/src/source/lower/expr/aggregates.rs) | Evaluate explicit fields, evaluate base, read checked omitted fields, then emit existing complete MakeStruct. Bytecode slot ordering must not reorder evaluation. |
| [Formatter](../crates/kagari-format/src/syntax.rs), [navigation](../crates/kagari-hir/src/analysis/mod.rs), [usage](../crates/kagari-hir/src/analysis/usage.rs) | Preserve explicit-label source alignment, base-expression navigation/usage/captures, comments and formatting idempotence. Do not invent source labels for omitted fields. |
| [Tree-sitter](../editors/tree-sitter-kagari/grammar.js), [Zed queries](../editors/zed/languages/kagari/highlights.scm), [TextMate](../editors/vscode/syntaxes/kagari.tmLanguage.json) | Tree-sitter struct expressions lack a base. Update grammar/generated products and recovery/capture tests; verify TextMate's existing lexical rules without semantic name guesses. |

## Semantic decisions

### Ordinary core-library Default

Canonical declaration: `core::default::Default`, exposed through the ordinary
prelude and existing std facade publication. One declaration identity, not a second
std trait. Its sole required method has no receiver/value arguments and returns
trait-owned Self. It is static-only, not an interface value type. Do not extend
Protocol, LangRole, the language-trait inventory or implicit conversion adapters.

`Type::default()`, `<Type as Default>::default()` and `T::default()` select ordinary
implementations. Local names, imported aliases, shadowing and ambiguity obey existing
rules. A same-spelled application trait does not gain a standard-library identity.
A default is implementation-defined, not automatically zeroed or recursively built.
No user struct receives an implicit implementation.

Initial standard implementation inventory, frozen for this task:

| Types | Result | Bounds / identity |
| --- | --- | --- |
| Unit, bool | `()`, `false` | No bounds. |
| Existing ten integer types, f32, f64 | Correctly typed zero; positive float zero | No new scalar types. |
| String | Empty String | Existing String semantics/allocation ownership. |
| Vec<T> | Fresh empty vector on every call | No T: Default requirement. |
| Option<T> | None | No T: Default requirement; checked ordinary enum identity. |

Defer tuples, builtin arrays, maps/sets, ranges, Result, other automatic enum
choices, host/scoped values and all other standard implementations. This explicit
inventory is not a promise of Rust's complete implementor set. User-authored impls
on currently admissible targets still follow ordinary trait rules.

### Contextual static associated calls

`val p: Player = Default::default()` first resolves the actual trait declaration,
uses the declared method result and the expected Player type to constrain Self,
then proves Player implements that applied trait and records the normal checked
static call. The expected type is a constraint, not implementation evidence.

Apply the same mechanism to an unrelated user trait such as
`trait Factory { fn make() -> Self; }`; no Default-specific inference branch.
Support existing expected-type boundaries: annotated bindings, assignment targets,
function parameters/results, declared struct field values and the update base.
Generic expected T requires its matching bound. Explicit type/trait applications,
method generic arguments and ordinary qualified disambiguation remain supported.
Do not choose the sole installed implementation when Self remains undetermined.
Missing context, incompatible context, absent bounds and ambiguity produce ordinary
source diagnostics. Never and error recovery must not manufacture implementation
proofs or hide independent argument/signature errors.

Use the existing bounded inference/selection architecture. DF01 freezes the exact
Self constraint representation and diagnostic owner; broader inverse projection
solving or unrestricted implementation enumeration is outside this task. Existing
concrete/generic static calls must not regress while adding trait-qualified calls.

### Struct update syntax and typing

Accept `S { field: expression, shorthand, ..base }` and `S { ..base }`. There is
one base, at the end, with an expression operand. Follow Rust 1.90's expression
grammar: no comma after the base. This differs deliberately from the existing
pattern rest marker, which has no operand and permits a trailing comma.
Reject missing operands, repeated/nonterminal bases and `..=` as an update marker;
preserve following source during recovery. No new range-token meaning elsewhere.

The base must produce the same nominal owned struct and identical instantiated
type arguments. No structural compatibility, interface adaptation, host reference
projection or type-changing update. An owned struct returned by a checked host
operation is still eligible. Without a base, current completeness checks remain.
With a base, every field must be accessible, including omitted fields and the
all-overridden case; update syntax cannot bypass private construction boundaries.
Duplicate/unknown fields and ordinary type/bound errors remain errors.

Seed constructor inference from explicit type arguments/available expected context,
check explicit fields in source order, then infer the base under the partially
instantiated target. A normally completing typed base may constrain remaining
arguments; finalize only afterward. Never invent a concrete argument merely to
make Default applicable. If constraints remain insufficient, diagnose them rather
than retrying effects or selecting an arbitrary implementation. A noncompleting
field/base supplies no produced-value constraint; retain current completion-aware
validation and independent source diagnostics.

Proposed shared model, to confirm at DF01:

```text
ExprKind::StructInit { path, explicit_type, fields, base: Option<ExprId> }
ResolvedStructInit { structure, fields, base_fields: Vec<DefinitionId> }
```

`fields` remains explicit-only/source-ordered; checked fields preserve recovery
holes and label navigation correspondence. `base_fields` contains omitted checked
field identities in declaration order. Do not duplicate the base ExprId in the
checked record. A written marker with missing operand lowers to a recoverable
missing expression, not to an absent base. Update mapping/restoration and source
examples coherently; retain no obsolete parallel construction representation.

### Evaluation, sharing and cleanup

For `Player { health: record(200), ..make_player() }`:

1. Evaluate record(200) once, then make_player() once and in full.
2. Read only omitted fields from the resulting base, after base evaluation.
3. Allocate a new outer Player using explicit values plus those checked reads.

Even if every field is explicit, evaluate the base and preserve all its effects.
Do not lower this as per-field calls to Default or an in-place mutation. Do not
read omitted fields before explicit/base effects: aliases may change their state.
The result's health is independent of the base's health, but an inherited items
collection retains shared identity. The base remains usable; no implicit Clone,
move, freeze or deep copy. Field val/var does not prevent ordinary construction.

If a required operand returns/traps/cancels, later evaluation and final construction
are skipped. Preserve completed effects, explicit values live across the base call,
GC/reentry safety, depth accounting and cleanup on every exit. Use existing MIR
ReadAggregateField/MakeStruct and verifier contracts, not a new update opcode or
runtime fallback constructor. Static/generic calls retain supplying generations
across reload. Unsupported native programs fall back before entry; this task does
not claim new actual-native object/call support.

## Mature references and deliberate adaptations

Consulted Rust **1.90.0** primary documentation during planning:

- [Default](https://doc.rust-lang.org/1.90.0/std/default/trait.Default.html): ordinary
  required associated function, contextual construction and deliberate impls.
- [Struct expressions](https://doc.rust-lang.org/1.90.0/reference/expressions/struct-expr.html#functional-update-syntax):
  terminal base, same struct type, omitted members and all-field visibility.
- [Operand evaluation](https://doc.rust-lang.org/1.90.0/reference/expressions.html#evaluation-order-of-operands):
  operands execute left-to-right before applying construction effects.

Reuse that model rather than inventing a compiler-recognized Default constructor.
Rust's Sized, moves, partial invalidation, Copy/Drop restrictions and borrowing
assume a different ownership model. Kagari keeps ordinary typed GC values, a fresh
outer struct, shallow member sharing, checked host boundaries and pinned code.
The bounded initial impl inventory and existing inference limits are explicit
scope choices, not claims of identical Rust capability or measured speedups.

## Phase order and acceptance

### DF01 — Freeze contracts and direct-consumer inventory

Reconfirm the baseline/owners, static-call inference and update model, standard impl
inventory, exact recovery/visibility/generic/completion rules and editor prerequisites.
Freeze focused tests by semantic owner below and identify distinct missing cases.
Review all StructInit consumers and selected-call facts, remaps and native naming.
This is a documentation/design checkpoint; do not publish incomplete source APIs.
Acceptance: required choices and buildable vertical-slice boundaries are closed,
with concrete failures/deviations recorded here before implementation.

### DF02 — Ordinary Default declaration and contextual factory calls

Register/document/publish the required core trait and prelude together. Implement
ordinary trait-qualified Self inference and checked call selection without spelling
special cases; use source-authored Player/Factory implementations for this phase.
Exercise concrete, qualified, generic and expected-result calls through executable
lowering, not HIR alone. Keep missing context/bounds, shadowing and static-only
interface rejection. Acceptance: focused registration, typing and source/artifact
execution pass; no declarations with missing native implementations are installed.

### DF03 — Explicit standard Default implementations

Register and bind the finite inventory together, with explicit namespace ownership,
checked callable/result facts and complete API docs. Check typed values, fresh Vec
identity, and Vec/Option element types lacking Default. Preserve allocation/roots,
ordinary generic dispatch and source-free callable validation. Acceptance: required
impls execute and malformed/missing calls remain rejected; no fake universal default
body, zeroed storage, name heuristics or unnecessary element bounds.

### DF04 — Struct update vertical slice and editor support

Migrate parser/AST/HIR, resolver/captures, constructor inference/checking, completion,
constant validation, facts/remaps, compiler extraction/construction and direct tools
in one coherent slice. Include formatter, EBNF/coverage and Tree-sitter/generated
parser/query/lexical-adapter checks in this checkpoint. Add a runnable update example.
Acceptance: required update forms compile/execute with ordinary contextual calls;
nominal/generic/access/recovery errors remain rejected; ignored overrides do not
skip base effects. Editor parsing/highlighting/recovery supports the new expressions.

### DF05 — Focused integration, products and handoff

Complete effects/alias timing, divergence/trap/cancellation/root cleanup, navigation,
reuse and pinned Default-call evidence at existing owners. Add a used update/default
entry to the shared feature artifact, preserve unrelated fixture/native contracts,
refresh the disposable product once, and execute without a source frontend.
Reconcile fixture documentation with actual entry contracts rather than assuming
its historical scalar-main description is still accurate.

Run required local acceptance, retaining unaffected passing evidence. Complete
matrix/feature/platform testing remains CI-only. Move lasting contracts to their
spec/architecture/editor owners, remove the resolved SA26 review entry and completed
plan, update roadmap/index, and keep phase ledger/results in Git history. No carried
integration error, silently deferred required surface or performance claim.

## Focused verification and build policy

The following are inspected existing anchors, not newly executed results. DF01
selects a small subset per phase and adds cases only for distinct missing contracts;
do not run this entire list repeatedly or reconstruct a full workspace sweep.

| Contract | Existing focused anchors / owner |
| --- | --- |
| Registration/prelude/static calls | stdlib `canonical_foundation_contracts_survive_scoped_publication_and_installation`, `standard_documentation_covers_the_registered_api_and_examples_analyze`; compiler `foundational_static_methods_and_generic_bounds_compile`; SDK `static_calls_share_ordinary_trait_dispatch`, `explicit_prelude_requires_imports_and_local_names_shadow_defaults` |
| Syntax/recovery | syntax `tests::parser::struct_expr`, `tests::grammar_coverage`; add update operand/placement/recovery assertions beside constructor cases |
| Constructor typing | HIR `reports_invalid_struct_initializers`, `explicit_struct_arguments_check_identity_arity_bounds_and_fields`, `constructor_fields_share_partial_argument_context_and_reject_unseeded_members`, `struct_fields_infer_and_check_only_normally_produced_values` |
| Visibility/tooling/reuse | HIR `private_foreign_fields_are_rejected_but_public_fields_remain_accessible`, `struct_initializer_labels_follow_checked_field_targets_after_errors`, `independent_body_queries_rebase_constructor_targets_and_invalidate_payload_changes`; existing formatter constructor/comment idempotence and lint `usage_tracks_reads_shadowing_captures_and_places` |
| Checked executable facts | compiler `struct_initializers_require_every_slot_once_and_the_declared_representation`, `field_operands_require_an_existing_owner_slot_type_and_write_permission`, `bytecode_initializers_use_layout_order_after_source_order_evaluation` |
| Observable execution | SDK `inference_order_does_not_change_evaluation_order`, `returning_expressions_stop_aggregate_construction`; extend existing runtime/language_contract effect/trap/cancellation cases and generic reload owner with selected Default calls |

Distinct new coverage: renamed Factory inference; missing/ambiguous Self or bound;
standard defaults/fresh containers; base-only/all-overridden construction; base
nominal/generic/private-field rejection; alias mutation observed at omitted-field
read time; fresh outer/shared inner identity; early exits before/in base; pending
explicit objects across collecting base calls; retained Default selection on reload.
Do not treat existing tests named "default" as evidence for the new Default trait.

Source-free checkpoint commands, from repository root:

```text
cargo run -p kagari --example regenerate_feature_artifact
cargo test -p kagari --test artifact_features portable_fixture_matches_source_emission
cargo test -p kagari --no-default-features --test artifact_features portable_artifact_executes_without_source_compilation
```

Extend existing fixture assertions to actually call the new entry; emission alone
is not execution coverage. Preserve malformed field/slot/type/selected-call rejection.
Do not bump versions or regenerate products after every declaration edit.

Editor checks follow [editor ownership/workflow](../editors/README.md#syntax-change-workflow).
From `editors/`, install pinned dependencies once when needed, then use affected
corpus/capture tests and generated consistency checks. At the coherent grammar
checkpoint use `npm run generate`, `npm run build:grammar`, `npm run test:grammar`,
`npm run check:generated`, `npm run test:zed` and relevant TextMate/shared checks.
Stage/build the Zed WASM through existing scripts and inspect representative valid,
incomplete and interpolation-nested updates in an actual dev extension when the
host prerequisites are available. Record unavailable host evidence explicitly;
query/native tests alone do not prove installed rendering. Full inventory and
cross-platform/editor-host matrices remain CI-owned. No npm install was run here.

At every implementation checkpoint review changed handwritten Rust imports,
visibility, ownership, functions, re-exports, macro scopes and effective LOC. Use
ordinary modules, no compatibility forwards or blanket structure exceptions.
Required local checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Temporary edits may break compilation within DF02/DF04's coherent migration, but
checkpoint commits must build and pass their selected tests. Record actual failed
commands, cause and repair owner here; do not retain duplicate models/stubs to hide
integration errors. After repairs rerun failing/affected cases only. Use default
Cargo profiles/target/parallelism, bounded commands and uv for Python. No local full
workspace test or exhaustive feature/backend sweep. Later authorized phase commits
use Conventional Commits and `Plan-Phase: DF0N`; mark actual breaking APIs explicitly.

## Progress ledger

- [x] DF01: freeze contracts, consumer inventory and focused acceptance selection.
- [x] DF02: ordinary declaration and general contextual static-call inference.
- [x] DF03: finite standard implementation inventory and bindings.
- [x] DF04: struct update vertical slice, source tools and editor syntax.
- [ ] DF05: focused boundary acceptance, disposable products and documentation handoff.

### DF01 — activation and acceptance freeze

Activated at `f9f1c74d`; the existing planning edits belong to this task. That
baseline includes merged destructuring editor support and the repaired scalar
feature-artifact main. Preserve both while adding a separately executed update entry.
The inventory above remains current: trait-qualified owners are rejected by the
static-call site builder, StructInit has only explicit fields, and Tree-sitter
struct expressions lack update bases. Node 22.21.1/npm 10.9.4 are available; editor
node_modules is absent and will be installed from the lock at DF04.

Frozen model: a distinct syntax base node/accessor; HIR base Option<ExprId> with
Some(Missing) for a written incomplete operand; ResolvedStructInit.base_fields
stores only omitted definition identities. Keep label facts explicit-only. The
contextual factory path introduces a trait-owned Self inference constraint from
the declared result and ordinary expected type, resolves that receiver before
existing bounded implementation selection, and records ordinary static call facts.
It never enumerates implementations to invent Self, constructs an interface, or
recognizes Default by spelling. If the existing solver cannot invert a result
projection, diagnose unresolved Self rather than expanding the solver here.
Required direct-Self factory surfaces and existing static calls remain acceptance.
The initial impl inventory and all-field/base-order/value rules above are frozen.

Focused phase selection (only affected/failing checks are repeated):

- DF02: stdlib registration/documentation inventory; HIR contextual factory positive,
  generic/bound/ambiguity/shadowing/static-only rejection cases at the trait owner;
  existing compiler foundational_static_methods_and_generic_bounds_compile; SDK
  static_calls_share_ordinary_trait_dispatch extended with Default/custom Factory
  through source/artifact routes; existing explicit-prelude shadowing contract.
- DF03: stdlib explicit installation/binding checks and SDK standard-default value,
  fresh Vec and no-element-bound cases. Reuse DF02's unchanged dispatch evidence;
  refresh source-free products only at DF05.
- DF04: syntax tests::parser::struct_expr and tests::grammar_coverage; HIR update
  nominal/generic/private/completion cases plus selected constructor-context,
  navigation/reuse anchors; checked compiler extraction/slot-order cases; formatter
  constructor/comment round-trip and existing usage/lint test; SDK runnable update
  example; selected editor grammar/capture/incremental recovery and generated checks.
- DF05: extend existing conformance effects/trap/cancellation/GC cases and retained
  generic reload owner with actual selected Default calls; source-free feature entry,
  canonical emission, both existing scalar native-artifact tests, and selected
  malformed field/local/callable rejection. Full matrix/inventory remains CI-only.

DF01 is documentation-only. Content/link/diff checks precede its commit; no Cargo
build/test is needed. No new carried implementation failures or performance claims.
Required local implementation checks remain as specified above.

### DF02 — ordinary declaration and contextual factory calls

Registered required static Default under core::default with ordinary std/prelude
publication and documentation. Language roles remain unchanged (26); the ordinary
foundation inventory is now 41. Contextual trait-qualified factories constrain a
trait-owned Self variable from declared results, then prove the receiver through
existing bounded selection and publish normal static call facts only on success.
Custom Factory, aliases, structural results, generic bounds, missing/ambiguous
context and static-only admission are covered without spelling-based behavior.

Passed: 2 selected stdlib registration/documentation contracts; 29 HIR trait and
inference/constructor/namespace regressions; compiler foundational static calls;
SDK static_calls_share_ordinary_trait_dispatch (source/artifact/artifact-JIT, forced
GC) and explicit prelude/shadowing. SDK covers concrete, qualified, generic,
return/parameter/field/assignment and annotated factory/default calls. Structure
checked 1,299 Rust files, zero violations/exceptions; formatting, workspace all-target
Clippy and diff checks passed. Manual import/visibility/LOC review added no
exceptions. No carried failures. Standard Default implementations remain DF03;
updates/editors and combined source-free/reload acceptance remain DF04/DF05.
Complete feature/backend/platform tests are CI-only and unrun locally.

### DF03 — finite standard implementations

Registered/bound Unit, bool, the ten integers, f32/f64, String, Vec<T> and Option<T>
through explicit ordinary native entries and namespace ownership. No universal
recipe, new language role or element Default bound. Updated implementation docs
and registration assertions. SDK checks every value/width, positive float zero,
fresh independent Vec storage and None with a non-Default element type under
forced GC through source/artifact/artifact-JIT. Existing constructor rejection
coverage now also rejects implicit user-struct and tuple defaults.

Passed 3 stdlib registration tests, the API documentation contract and both SDK
value/invalid-construction filters. Structure checked 1,300 Rust files with zero
violations/exceptions; formatting, workspace all-target Clippy and diff checks
passed. Manual imports/ownership/visibility review added no exceptions. No failures
remain. Source-free fixture regeneration is intentionally batched at DF05; complete
feature/backend/platform acceptance remains CI-only.

### DF04 — struct update, tools and checked static-bound integration

Implemented the dedicated terminal base node/accessor, HIR base/missing recovery,
constructor/context/completion/access checks, explicit-only navigation facts and
omitted identity remaps, and existing MIR field reads/fresh construction. The base
executes even with all fields overridden. Normalized associated projections on both
target and base boundaries, including lowering through the existing planner.
Source tools and grammar inventories agree; the runnable generic update example
checks order, once-only calls, alias read timing, outer independence and inner sharing.

Selected checks passed: 8 constructor parser cases, 5 pattern-rest regressions;
11 HIR update cases and 8 frozen constructor/visibility/reuse/dispatch regressions;
6 compiler lowering/verification cases with affected projection filters rerun after
repair; SDK example source/artifact execution, 2 formatter checks and usage/lint.
All 5 grammar-inventory checks were completed (3 failing cases passed after repair).
Within the already changed inventories, reconciled existing omitted explicit_enum_path
rule/branch and optional method async numbering. No assertions were weakened:
explicit_enum_path has no example witness and is honestly marked unverified.

Integration acceptance exposed a required DF02 boundary gap: an operation owner T
was conflated with a value-level self in shared generic T::default(). Expanded this
checkpoint coherently at contract/MIR/runtime owners: derive has_receiver from the
checked declaration, preserve scope checks on the owner type, validate imported
full signatures, and forward static arguments without a fabricated receiver.
No compiler special case, new opcode or ABI bump. The retained generic reload test
now passes, plus 7 contract interface, 24 VM interface and 5 runtime application
tests, including malformed static calls, explicit Self-typed arguments, artifact
round-trip and GC cleanup. DF05's SDK acceptance edits remain separate from this
production checkpoint. This necessary boundary repair is an explicit scope
clarification, not native-backend expansion or unrelated solver work.

Other repaired intermediate failures were the syntax/HIR handoff, a test-only Span
API mistake, premature inference errors on returning bases and unnormalized projected
applications. No carried implementation/build/test failures remain. Investigative
probes also encountered existing projected closure-argument and generic-closure
limitations outside required update/default surfaces; equivalent projected base
lowering was verified through checked Fn::call and generic functions without
weakening assertions or changing those unrelated mechanisms.

Editor acceptance passed: pinned npm installation, generated consistency (6 exact
hashes/LF), grammar corpus plus 38 native tests, Zed 21 passed/1 optional WASM test
skipped, TextMate 17, shared regions 12, and direct parsing of the runnable example.
Staging succeeded. Actual build:zed was attempted, then pinned official SDK 34
acquisition twice; curl 56 connection failures prevented completion. SDK 29 was not
substituted. Actual WASM loading/rendering is unverified; complete host/platform
acceptance stays pending rather than inferred from native tests.

Manual structure/ownership/import/LOC review split a growing compiler test at its
semantic boundary; no exceptions/re-exports added. Structure checked 1,302 Rust
files, zero violations/exceptions; formatting, workspace all-target Clippy and diff
checks passed. Remaining DF05 work is acceptance/product/documentation handoff;
full workspace/feature/backend/editor inventory remains CI-only.
