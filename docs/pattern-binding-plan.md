# Struct patterns and destructuring declarations execution plan

## Status and authority

Status: active; implementation and one commit per phase authorized on 2026-10-17.
PB01-PB02 are complete; PB03 is next.
The [roadmap](implementation-roadmap.md#review-follow-up-order) owns activation and
queue placement. This is the single execution plan for
[SA16](review.md#sa16-struct-patterns-should-require-explicit-omission-with-) and
[SA17](review.md#sa17-ordinary-valvar-declarations-should-support-destructuring-patterns).
It owns PB01-PB04, their decisions, acceptance and progress ledger. The user's
subsequent authorization activates implementation and coherent phase commits.

[Syntax](spec/syntax.md), [value semantics](spec/value-semantics.md) and
[host access](spec/host-interop.md) remain the current contracts until implementation
updates their owners. Examples below are proposed behavior, not accepted syntax.

## Outcome and finite scope

Implement explicit omission in named-field struct patterns, then use the shared
pattern model for ordinary local declarations:

```text
struct Point { val x: i32, val y: i32 }

val Point { x, .. } = point;
var (left, right): (i32, i32) = make_pair();
val (id, Point { x: coordinate, .. }) = make_result();
```

One task does not mean two simultaneous semantic implementations. SA16 establishes
the struct-pattern rule for all existing contexts before SA17 accepts declarations
using it. Simple `val n = 1;` becomes the same pattern-based declaration, not a
compatibility path retained beside destructuring.

Included: parser/AST, HIR and source maps, lexical resolution, typing, checked
extraction, mutable captures, navigation/debug locals, usage/lint facts, formatting,
grammar coverage and representative source-free execution.

Excluded: struct-update expressions and Default (SA26), named local functions
(SA14), declaration self-ID cleanup (SA11), slice/tuple rest patterns, declaration
`else`, per-leaf `mut`/writeability, delayed initialization, new enum exhaustiveness
analysis, Rust moves/borrows, direct host/view destructuring, new native-backend
coverage and unrelated performance or storage migrations. SA15/SA9/SA1 are not
mandatory predecessors; their proposed queue order does not block this task.

## Baseline and implementation inventory

Planning baseline: clean `0ae1d1b4`, inspected on 2026-10-17. Source and existing
tests were inspected; no builds, behavior tests or performance probes ran for this
planning task. Reconfirm affected owners in PB01 before editing.

| Owner | Current evidence and required migration |
| --- | --- |
| [Binding parser](../crates/kagari-syntax/src/parser/grammar/stmt.rs), [statement AST](../crates/kagari-syntax/src/ast/stmt.rs) | A declaration owns one name; replace that child/accessor with a pattern. Preserve annotation/initializer parsing and malformed-source recovery. |
| [Pattern parser](../crates/kagari-syntax/src/parser/grammar/expr.rs), [pattern AST](../crates/kagari-syntax/src/ast/expr.rs) | Reuse tuple/struct/name structure. AST range detection currently treats any direct `..` as a range; a struct rest token must not trigger that predicate. |
| [HIR statements](../crates/kagari-hir/src/hir/stmt.rs), [patterns](../crates/kagari-hir/src/hir/pattern.rs), [lowering](../crates/kagari-hir/src/lower/stmt.rs) | Replace statement-level `name`/`local` with `PatternId`; leaf patterns retain actual binding identities. Add struct rest presence and preserve physical source sites. |
| [Resolution](../crates/kagari-hir/src/resolver/resolve.rs), [resolved patterns](../crates/kagari-hir/src/resolver/resolved.rs) | Ordinary initializers resolve before binding publication. Existing pattern resolution can interpret bare names as variants; use an explicit declaration context. Current irrefutability is conservative structural classification, not full type checking or coverage analysis. |
| [Pattern typing](../crates/kagari-hir/src/typeck/body/patterns.rs), [statement typing](../crates/kagari-hir/src/typeck/body/statements.rs) | Reuse field identities, access, generic substitution and duplicate/unknown-field checks. Add completeness, declaration-wide duplicate bindings and leaf writeability; preserve whole-value contextual inference/coercion and non-completing initializer handling. |
| [Pattern execution lowering](../crates/kagari-compiler/src/source/lower/expr/patterns.rs), [statement lowering](../crates/kagari-compiler/src/source/lower/stmt.rs), [function state](../crates/kagari-compiler/src/source/lower/state.rs) | Existing lowering projects tuple/struct children before visiting `_`. Reuse checked extraction while skipping ignored projections; migrate captured-`var` discovery and cell creation from one local to all declaration leaves. |
| [Usage facts](../crates/kagari-hir/src/analysis/usage.rs), [formatter](../crates/kagari-format/src/syntax.rs) | Usage currently reads one statement local. Produce initialized/mutable/read/write/capture facts per real leaf. Reuse CST formatting for pattern braces and verify comments/idempotence. |
| [Grammar](kagari.ebnf), [syntax guide](architecture/syntax.md), [HIR guide](architecture/hir.md) | Update grammar, AST/HIR examples, ownership and coverage products together; do not document queued behavior as already supported. |

PB01 also searches all Binding/Struct-pattern consumers: arena and snapshot remaps,
completion analysis, semantic diagnostics, closure capture, lint, source navigation,
debug scopes and tests inspecting the old model. No forwarding accessors or duplicate
name/local fields may conceal unmigrated consumers.

## Shared semantic contract

### Struct patterns

For `Point { x, y }`:

| Pattern | Result |
| --- | --- |
| `Point { x, y }` | Accept; bind both fields. |
| `Point { x: n, y: _ }` | Accept; bind n and do not read y. |
| `Point { x, .. }` | Accept; bind x and ignore unlisted fields. |
| `Point { .. }` | Accept; introduce no bindings or field reads. |
| `Point { x }` | Reject: y is missing without an explicit rest marker. |

Permit at most one rest marker at the end; a trailing comma follows the existing
field-list convention. Rest may omit zero fields. Record its presence on the whole
struct pattern, not a fake field or wildcard child. Completeness compares resolved
field identities against the entire nominal declaration, including inaccessible
fields; defaults do not make fields optional. An inaccessible field can be omitted
with rest, but explicitly listing it still requires access even with `_`.

Apply these rules recursively in match, for, binding conditions and ordinary
declarations. Preserve shorthand, unknown/duplicate-field errors and nominal/generic
identity. Existing sources relying on implicit omission must add `..` or list all
fields; migrate their fixtures rather than preserving the old rule.

### Declaration model and name resolution

The intended HIR shape is:

```text
Binding {
    pattern: PatternId,
    writeability: Writeability,
    ty: Option<TypeRefId>,
    initializer: ExprId,
}

val Point { x: n, .. } = make_point();
Binding.pattern -> Struct { path: "Point", fields: [x -> Name(n, local_n)], has_rest: true }
```

Field names are illustrative; checked field identities remain in the existing
semantic tables. A simple declaration uses `Name`, and `_` uses `Wildcard`.

Context policy frozen in PB01: bare names and shorthand fields
introduce locals throughout ordinary declaration patterns, preserving existing
ordinary-name shadowing. Thus `val None = value;` binds a local, as does the None
leaf in `val (None, n) = pair;`. Explicit constructor syntax still resolves a
constructor and is subject to refutability checking. Existing match/conditional
constructor precedence remains unchanged. Do not accidentally select variants but
bind identically positioned constants, or expand constant-pattern support here.

`val _ = work();` evaluates work once but introduces no referenceable `_` local.
All declarations require an initializer; missing syntax is recovery, not a new
uninitialized-variable feature. An annotation describes the whole initializer and
provides its inference/coercion context; leaf types come from checked projections.
Do not insert additional per-leaf conversions during extraction.

Ordinary declarations require a type-valid, conservatively irrefutable pattern.
Reject literal/range/variant forms, including single-variant enums; no new coverage
engine proves exhaustive alternatives or ranges. Use a no-top-level-alternative
declaration grammar, following Rust's PatternNoTopAlt model. Nested/grouped existing
alternatives may only be accepted under existing binding-consistency and conservative
irrefutability rules; they must not introduce a new runtime match-failure path.

### Evaluation, publication and sharing

1. Resolve and evaluate the complete initializer in the old lexical environment,
   exactly once. For `val (x, y) = (x + 1, 3);`, RHS x denotes the previous binding.
2. Perform only required extraction, depth-first in pattern source order. For
   `var Point { y: b, x: a } = make_point();`, extract y before x, not declaration
   field order. Direct `_` and omitted fields cause no projections. Reuse shared
   traversal rules for nested subpatterns that require neither a binding nor a
   runtime decision; do not skip initializer effects or static access validation.
3. Publish all new leaf bindings after declaration completion. No sibling leaf
   becomes visible during the initializer, and debug scopes must not expose partial
   declarations. Preserve completed effects on a later trap/cancellation; temporary
   values and capture cells follow existing root and cleanup ownership.

The outer val/var applies to every actual binding. Var permits rebinding extracted
locals, not writing back into the matched object. Captured var leaves use the same
shared-cell model as current simple declarations. Scalars copy values; object
members retain shared identity. Val and destructuring do not freeze, deep-copy,
move out of, or implicitly Clone the source object.

### Host boundary and contextual limits

Initial scope destructures owned tuples and `TypeId::Struct` values, including
owned values obtained through a checked host initializer. Direct patterns over
`Host`, scoped `Ref`/`RefMut`, path views and interface values remain unsupported;
being able to read a field expression does not imply struct-pattern eligibility.
This limits decomposition, not Name/wildcard bindings: existing admissible simple
host/scoped bindings must remain valid under the unified model. PB01 freezes this
boundary for implementation and its diagnostics/tests.
No host data is silently snapshotted into GC storage to make a pattern succeed.

Checked host operations in an initializer retain declared access, borrow/liveness
validation, once-only evaluation, trap order and cleanup. If future direct host/view
patterns are requested, they require an explicit scope decision and shared checked
projection contracts, not ad hoc reads in lowering. Broader host adaptation is not
a prerequisite for delivering these two review items.

## Mature reference and Kagari differences

Primary references consulted: Rust **1.90.0** [struct and wildcard patterns](https://doc.rust-lang.org/1.90.0/reference/patterns.html),
[let statements](https://doc.rust-lang.org/1.90.0/reference/statements.html#let-statements)
and [binding scopes](https://doc.rust-lang.org/1.90.0/reference/names/scopes.html#pattern-binding-scopes).
Adopt explicit struct completeness, one pattern-based declaration, whole-value
annotations, no top-level alternative and post-statement binding scope.

Do not import Rust moves, partial moves, borrow/match ergonomics, per-leaf mut or
its complete exhaustiveness analysis. Kagari deliberately preserves ordinary
bare-name declaration shadowing, declaration-wide writeability, conservative
refutability and shared-GC/checked-host semantics. Declaration name policy and host
eligibility are explicit adaptations, not claims of identical Rust behavior.

## Phase order and acceptance

### PB01 — Freeze the shared contract and consumer inventory

Confirm baseline owners and choose the exact AST rest/name representation,
declaration-context resolution, conservative pattern admission and host boundary
above. Record any departure here before editing; do not silently enlarge the task.
Select a finite affected test set and uncovered cases by semantic owner. Reuse
existing checked field/projection and local/capture machinery rather than adding
another resolver, extractor or binding table. Acceptance: decisions and buildable
checkpoint boundaries are closed before PB02 production edits.

### PB02 — SA16 shared struct-pattern foundation

Implement rest parsing, AST range/rest discrimination, HIR presence and recursive
field completeness together. Migrate existing pattern consumers, syntax examples,
formatter/grammar coverage and affected source fixtures. Carry rest through snapshot
and identity remaps without losing spans. Accept only when existing match/for/
conditional patterns and representative execution obey the same rule. Do not expose
SA17 declarations while their typing or execution path is incomplete.

### PB03 — SA17 unified declarations and all direct consumers

Replace statement name/local with a pattern and migrate parsing, resolution,
annotation inference/coercion, leaf types/writeability, duplicate diagnostics,
completion, captures/cells, source maps, usage facts, navigation/debug scopes and
checked execution in one coherent migration. Reuse pattern extraction through its
semantic owner; do not call a general match builder and add a supposedly unreachable
runtime failure trap. Skip ignored projections without bypassing static validation.

Internal edits may temporarily fail to build while the representation changes, but
all affected callers and focused checks must build/pass before the checkpoint.
Record failures and their owning repair here; do not defer broken integration to
PB04 or add compatibility fields/accessors to create a superficial green build.

### PB04 — Focused integration and handoff

Validate combined nesting, nominal/generic fields, scope/writeability, shared
identity, mutable captures, recovery/navigation and exact evaluation behavior at
existing owners. Refresh the disposable source-free fixture once at this coherent
checkpoint and execute representative accepted/rejected contracts without a source
frontend. Verify invalid/incompatible field or local facts are not trusted merely
because syntax was accepted. Native-unsupported patterns select pre-entry fallback;
this plan does not require new native instructions.

Finish required local structure/format/Clippy/diff checks. Full workspace tests and
the exhaustive feature/backend/platform matrix remain CI-only. State local results
and pending CI separately. Move lasting contracts/examples to their spec/architecture
owners, remove resolved SA16/SA17 findings and this completed plan, and leave Git
history to retain the completed ledger. No performance claim or routine ABI/format
version bump; regenerate affected development products instead.

## Focused verification

Reuse tests by grammar, static semantics, observable behavior and core boundaries.
Add only distinct uncovered rules at the existing owners, not a second smoke suite.

| Contract group | Selected coverage |
| --- | --- |
| Syntax/recovery | Complete/rest/empty/nested structs; duplicate/nonterminal rest; rest versus range AST; tuple/struct declarations, annotations, missing initializer and following-item recovery. |
| Static semantics | Missing/unknown/duplicate/private fields; generic field substitution; declaration-name policy; whole-value annotation/coercion; duplicate names; conservative refutability and unsupported host/view patterns. |
| Observable declarations | Once-only initializer; source-order extraction; no ignored reads; old-name shadowing; val/var rebinding; shared nested objects; non-completing initializer and effects/root cleanup on traps/cancellation. |
| Captures/tooling | Mutable captured leaves share cells; scope exit hides all leaves; nested declaration navigation, usage/lint facts and format idempotence; retained snapshot/arena identity. |
| Executable boundaries | Representative source/serialized/source-free execution, checked generic/field identities and malformed-product rejection; preserve existing pinned reload contracts when captures retain values. |

Existing focused anchors, not a command to sweep every package after each edit:

```text
cargo test -p kagari-syntax --lib tests::parser::match_expr
cargo test -p kagari-syntax --lib tests::parser::statements
cargo test -p kagari-hir --lib resolves_named_match_pattern_bindings_inside_arm
cargo test -p kagari-hir --lib records_named_match_pattern_binding_type
cargo test -p kagari-format --lib declarations_types_attributes_and_patterns_have_canonical_layout
cargo test -p kagari-lint --lib usage_tracks_reads_shadowing_captures_and_places
cargo test -p kagari --lib constructors_patterns_and_nested_payloads_execute
cargo test -p kagari-vm --lib closures_keep_captured_objects_and_cells_alive_across_collection
cargo test -p kagari-vm --lib debug_session_hides_inner_binding_after_lexical_scope
```

PB01 records exact filters for new distinct cases and the representative source-free
consumer before implementation. Use package/target/name filters and `--no-fail-fast`
for multiple selected targets. Do not repeat unchanged passing checks for a fresh
transcript or treat a failed build/zero matched tests as a pass. Required local
implementation checks follow [repository policy](../AGENTS.md#verification-and-tooling):

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Review production imports/re-exports, macro scopes, visibility, effective LOC and
responsibility boundaries manually. Split growing owners coherently, without
blanket exceptions. Planning/documentation-only checkpoints need content/link/diff
checks, not Cargo builds. Commit each authorized phase coherently with a
`Plan-Phase: PB0N` trailer; PB02 and PB03 must not carry broken integration.

## Progress ledger

- [x] PB01: freeze decisions, consumer inventory and focused acceptance selection.
- [x] PB02: SA16 shared pattern rule and fixture migration.
- [ ] PB03: SA17 unified declarations and all direct consumers.
- [ ] PB04: focused integration, actual acceptance and documentation handoff.

### PB01 — contract and acceptance freeze

Activated at `0ae1d1b4` with the existing planning edits; those edits belong to this
task. User authorization is implementation plus one commit per phase, not remote
publication. SA16/SA17 scope and the Rust 1.90 references above remain unchanged.

Decisions: use an AST `has_rest()` accessor and HIR `Struct.has_rest: bool`; keep
rest out of PatternField and range recognition. Binding owns one PatternId plus
writeability, optional whole-value annotation and initializer. All bare declaration
names/shorthand bind, including names shadowing prelude constructors; explicit
constructor forms remain refutable. Top-level alternatives are excluded, nested
alternatives reuse existing binding consistency and conservative irrefutability.
Wildcard is not a local. Host/view decomposition remains excluded, with existing
simple bindings and checked initializer access preserved. Do not introduce a second
binding table, general exhaustiveness engine or runtime match-failure trap for an
accepted declaration. Typed field facts, GC roots and captured-var cells retain
their existing owners.

The consumer search confirms the inventory above plus HIR analysis arena/owner
fixtures, completion, usage and local navigation. Most Struct consumers already
ignore additional fields; its constructor, checker and model docs need explicit
rest migration. Syntax and HIR form the PB02 build boundary. PB03 must migrate
syntax/HIR/compiler/tooling together before its checkpoint; no compatibility fields.

Frozen focused selection (reuse passing unaffected evidence):

- PB02: syntax `tests::parser::match_expr` and `tests::grammar_coverage`; new HIR
  `tests::patterns::struct_pattern_` cases for completeness, generic/private/nested
  fields; existing `records_named_match_pattern_binding_type`; formatter
  `declarations_types_attributes_and_patterns_have_canonical_layout`; SDK
  `struct_pattern_rest_executes_in_existing_contexts` at the language syntax owner.
- PB03: syntax `tests::parser::statements`; HIR `tests::patterns::declaration_pattern_`,
  `tests::lower`, `tests::resolver`, `analysis::arena_tests` and `analysis::owner_tests`
  for changed models/identities; existing formatter binding/pattern cases and lint
  `usage_tracks_reads_shadowing_captures_and_places`; SDK
  `declaration_patterns_preserve_scope_aliases_and_captures`; VM
  `closures_keep_captured_objects_and_cells_alive_across_collection` and
  `debug_session_hides_inner_binding_after_lexical_scope`. New tests live beside
  existing contract owners, not separate smoke suites.
- PB04: extend the existing feature-artifact producer with a used representative
  declaration/rest case, generate once through `regenerate_feature_artifact`, then
  run `cargo test -p kagari --no-default-features --test artifact_features portable_artifact_executes_without_source_compilation`.
  Add checked-lowering coverage for ignored projections at its owner and an
  initializer-effect/trap-cleanup contract in the existing language conformance
  harness. Native-unsupported syntax requires pre-entry fallback, not new native
  instructions. Keep format idempotence, navigation/usage and malformed-source facts
  within the selected contract groups; full workspace/platform/backend CI is unrun.

This phase changes documentation only. Local documentation file links, whitespace
and diff checks are required before its commit; no Cargo build is needed. No new
intermediate implementation errors or performance claims exist at PB01.

### PB02 — explicit struct-pattern omission

Implemented one terminal rest token and AST has_rest; range_inclusive excludes
struct rest. HIR retains the boolean, and recursive checking compares valid resolved
field identities against the entire nominal declaration, including inaccessible
fields. Duplicate/unknown/access errors still reject; rest does not create locals.
Ordinary declarations remain unchanged for PB03. Grammar/specification, branch and
quantifier inventories, the existing match example and formatter coverage agree.

All frozen PB02 commands passed: 5 match-parser cases, 5 grammar-inventory cases,
6 HIR completeness/access/generic cases, the existing named-pattern type-fact test,
formatter canonical/idempotent patterns and source/artifact execution through match,
for, if-val and while-val (19 tests). Structure checked 1,289 Rust files with zero
violations/exceptions; formatting, workspace all-target Clippy with denied warnings
and diff checks passed. Manual ownership/import/LOC review introduced no visibility
or re-export exceptions. No intermediate build errors remain. Full workspace and
backend/platform CI are not run locally; source-free combined acceptance stays in
PB04. This checkpoint intentionally rejects omitted fields without rest; existing
code must list those fields or add `..`.
