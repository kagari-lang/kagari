# HIR Documentation Completion Plan

Status: HD01-HD08 complete within their scopes. The whole-crate missing-docs
audit has pre-existing imports documentation gaps recorded under HD08 below. The
[roadmap](implementation-roadmap.md#hir-documentation-completion) owns activation,
phase order and progress. This plan is executable without the originating conversation.

## Goal and boundaries

Document the entire production surface of `kagari-hir` so a reader can follow source
meaning from lowering through checked analysis without reconstructing storage,
identity or phase contracts from implementation bodies. Explain the current code,
including recoverable analysis and tooling queries, not a proposed architecture.

Deliver English Rustdoc beside the owning code and a short reading guide at
`docs/architecture/hir.md`, linked from `docs/architecture.md` and crate docs.
Reuse the [syntax documentation standard](syntax-documentation-plan.md#documentation-standard):
short summaries, useful examples, intra-doc links and applicable error/panic contracts.
Use storage diagrams and lookup tables as well as node shapes; HIR is not one tree.
Keep language rules in the [module](spec/modules.md), [syntax](spec/syntax.md),
[trait](spec/traits.md) and other relevant specifications.

This is a documentation task. Preserve behavior, visibility, IDs, data layout,
query scheduling and cache policy. Minimal macro changes to attach documentation
to generated IDs/methods are allowed; changing their implementation is not.
Do not implement inline-module reuse, import scheduling, identity interning or
other optimizations. Record newly verified discrepancies concisely in
[the existing review](review.md), checking for an existing entry first.
Compiler/MIR/runtime implementation documentation and rewriting other crates are
outside scope; link their owning symbols where a HIR handoff needs explanation.

## Documentation contract

- Cover every public module, type/alias, trait, function, inherent method, field
  and enum variant, including macro-generated items. Trait implementations may
  inherit their trait contract. Give private production modules an orientation;
  explain important private state and algorithms, not every helper statement.
- For each core structure explain what it represents, what it physically stores,
  who owns it, which operation produces it and which operation consumes it. Link
  to concrete symbols. Keep trivial entries short and share common explanations.
- For each ID family provide an allocation -> storage -> lookup example, its
  validity scope and relevant checks. Distinguish plain indices, arena/owner IDs,
  member slots, source-unit references and scoped definition identities. Do not
  claim all IDs have the same checks or remain valid after a source edit.
- Show actual Rust fields separately from conceptual relationships. Label arrows
  as stored IDs, borrowed views, shared `Arc` ownership or computed lookups.
  Mark illustrative indices and collapsed fields; never imply an allocation order
  or field that the implementation does not guarantee.
- Expand each data model from a concrete source fragment into its actual fields.
  Show every field, including empty buffers and optional values; identify source
  syntax, synthesized metadata and ID target storage. Add contrasting examples
  for required/default/defined members, generic input/output constraints and
  materially different forms. Label context-dependent fragments and distinguish
  recoverable/unpopulated models from currently accepted language behavior.
- Every HIR node family must have a source example and annotated storage/ID diagram.
  Cover materially different forms: declarations versus bodies, expression versus
  place, block statements versus tail expression, binding patterns, calls,
  generics and qualified paths. Simple variants may link to a shared family
  diagram with a concise local description; do not invent a runtime node for syntax.
- For semantic tables identify key, value, producer stage, consumer and absence
  meaning. Distinguish missing facts, deliberate omission, error recovery and
  invariant violations; do not interpret every `None` as a failed analysis.
- For query APIs explain inputs, result/diagnostics, dependencies, cache granularity,
  invalidation/reuse conditions and cancellation where applicable. Distinguish
  conceptual phases from actual eager/lazy calls; show the real callers.
- Document byte ranges and physical versus logical source association. Explain
  internal assertions and caller-reachable panics accurately; do not describe an
  unchecked ID constructor as validation or an invariant panic as a syntax error.
- Use `text` fences for Kagari and diagrams. Rust examples must compile and assert
  meaningful results; share setup and examples rather than adding one per getter.
  Do not use `ignore` to conceal broken examples or `no_run` for runnable examples.

## Required coverage

Paths are relative to `crates/kagari-hir/src/`. Nested production modules belong
to their listed family even when not individually named below.

| Owner | Required content and concrete anchors |
| --- | --- |
| `lib.rs`, `hir/mod.rs`, `lower/mod.rs` | Crate map; `LoweredModule`, `AnalyzedModule`, `AnalysisResult`, `CheckedAnalysis`; distinguish stored HIR, semantic side tables and validated output. Explain retained source, diagnostics and `DefinitionPath`/`DefinitionId` parameterization. |
| `hir/ids.rs`, `hir/body.rs`, `hir/item/` | `Module` declaration collections and `Body` vectors; `HirArenaId`, `HirOwner`, `BodyOwner`; all ID families and buffer aliases. `Body` can store nodes from multiple owners. Show allocation and checked lookup, including field/variant owner plus slot and parameters/locals at their actual owners. |
| `hir/expr/`, `hir/stmt.rs`, `hir/place.rs`, `hir/pattern.rs`, `hir/ty.rs`, `hir/writeability.rs` | All node kinds and payloads; links between IDs; literals/operators; missing nodes; condition, closure, match and propagation forms; expression/place distinction and writeability. Type spelling is not a resolved semantic type. |
| `source_map.rs`, `lower/` | AST-to-HIR construction, ID allocation and owner switching, import-tree flattening, expression/place lowering, source sites and recovery. Show which syntax is retained, transformed or omitted; explain inline-module handling as implemented. |
| `imports/`, `resolver/` | `SourceUnit`, `SourceDeclRef`, `NamespaceId`, `ImportDirective`, `ModuleImportFacts`, `NameEntry`, `NameTable`, `NamespaceCatalog`, `ModuleGraph`, `ResolvedNames`, lexical scopes. Directive vs binding vs namespace vs target; visibility, alias/glob provenance, candidate precedence, ambiguity, unresolved states, re-exports, fixed-point scheduling/termination and cache inputs. |
| `declarations.rs`, `declarations/`, `callable.rs`, `callable/`, `types.rs`, `types/` | Declaration and binding identity, callable signatures, semantic type variants, substitution and nominal/associated types. Explicitly distinguish `TypeRefId` from the semantic `TypeId` enum; explain portable paths versus scoped definition-table references and mapping failures. |
| `typeck/`, `aggregates/` | `ModuleSignatures`, `TypedModule`, `TypeTable`, `ResolvedCall`, `CallTarget`, `AggregateCatalog`; signature/body division, constraints/inference, calls/member selection, generic bounds and implementations, scalar/constant facts, coercion, iteration and propagation. Explain temporary inference state versus published facts and body/signature reuse. |
| `host.rs`, `host/`, `native.rs`, `native/`, `language/`, `builtin/` | Host declarations/IDs and origin, native registration/rendering, validated language roles and bounded builtin bridges. Explain how these provide inputs to ordinary analysis; do not imply HIR executes Rust functions or duplicate external registration specifications. |
| `analysis/`, `program.rs` | `AnalysisDatabase`, `AnalysisSnapshot`, `DeclarationSnapshot`, `SignatureSnapshot`, `FunctionAnalysis`, `CheckedProgram`; preparation, publication and query callers, revisions/ownership/caches, navigation/completion and diagnostic queries. Checked dependency closure and compiler handoff; HIR checking is not MIR verification or execution. |

At HD01 inventory production files and macro-generated APIs against this table.
Extend the appropriate row for overlooked files, without creating a second catalog
or expanding into other crates. Existing useful docs should be retained and corrected.

## Diagrams and recurring examples

The reading guide must contain three linked views: physical storage and IDs,
semantic tables keyed by those IDs, and query dependencies/publication boundaries.
Detailed node and algorithm explanations stay beside their owning code.

Use these two examples throughout; verify them with current APIs and existing
fixtures before publishing. Register the same foundation inputs used by analysis
tests where needed; do not bypass required registration to shorten an example.

**A. Local data and arithmetic**

```text
fn add(x: i32) -> i32 {
    val y = x + 1;
    y
}
```

Trace parameter, local, block, statement and expression IDs, the source map,
lexical resolution, inferred/declared types and checked output. At the binary node,
include at least this level of detail (indices are illustrative):

```text
Body.exprs                         // physical rows, other fields collapsed
+-- e0 -> (owner, Name { name: "x", ... })
+-- e1 -> (owner, Literal(...))
`-- e2 -> (owner, Binary { lhs: e0, op: ..., rhs: e1 })

ExprId = { arena, owner, index }    // e0/e1/e2 abbreviate complete IDs
Body::expr(e2) -> arena check -> vector[index] -> owner check -> ExprData
ResolvedNames.exprs[e0] -> parameter binding
TypeTable.exprs[e2]     -> semantic type
SourceMap              -> corresponding source byte range
```

Replace schematic payloads with verified variants in the final Rustdoc. Show the
actual SourceMap accessor and its source association. Do not embed resolved names
or types in `ExprData` diagrams: they are independent facts keyed by IDs.

**B. Cross-module lookup and calls**

Bind three source units in package `pkg` to logical paths `math`, `api` and `app`:

```text
// math
pub fn sum(x: i32) -> i32 { x + 1 }
// api
pub use pkg::math::*;
// app
use pkg::api::sum as add;
fn main() -> i32 { add(41) }
```

Show the source-database bindings explicitly; filenames alone do not define this
package layout. Trace the real directive, local name `add`, candidates/provenance,
namespace lookup, canonical `SourceDeclRef` for `math::sum`, resolved expression,
selected call/signature and checked dependency closure. Add a compact qualified-path
variation to explain why entering a nested namespace creates no synthetic import.
Distinguish glob expansion/re-export propagation from lookup at the use site.

Use small local examples for generic/trait calls, incomplete syntax or source edits
when these two do not expose the relevant contract. Reuse existing contract fixtures;
do not grow a new tutorial or test suite for every feature. For cache documentation,
contrast a body-only edit with a signature/export edit and show actual invalidation
checks, not a promised minimal recomputation algorithm.

## Execution and acceptance

Implement in order. Each phase updates its roadmap checkbox and records only
material gaps or carried errors there; this plan owns requirements, not a second
progress log. Use coherent `docs(hir): ...` commits with a `HIR-Docs-Phase: HD0N`
trailer (list phases together when a checkpoint combines them).

| Phase | Deliverable and acceptance |
| --- | --- |
| HD01 | Inventory and crate/reading-guide orientation; document `Module`, `Body`, IDs and source-map storage. Readers can locate an expression, declaration, parameter/local and member from an ID, and explain arena/owner/source-unit/definition identity differences. Add macro doc forwarding only if necessary. |
| HD02 | Complete HIR node families and lowering contracts. Example A has annotated storage diagrams, source ranges and AST-to-HIR transformations. Every node kind has a useful description; absence/recovery and expression/place differences are explicit. |
| HD03 | Complete imports, namespaces and lexical resolution. Example B follows actual tables and lookup APIs to the original declaration. Document current fixed-point bounds, ambiguity, visibility and provenance; do not describe proposed work queues as implemented. |
| HD04 | Complete declarations, callable/type models, aggregates and type checking. Show signature/body inputs and outputs and key/value/absence contracts for semantic tables. Trace a call from name resolution to `ResolvedCall`, and explain inference state versus published facts. |
| HD05 | Complete host/native/language/builtin documentation. Readers can follow registered inputs, generated declarations, language-role validation and the existing syntax bridges into ordinary analysis. No external API redesign. |
| HD06 | Complete analysis/query/cache and checked-program documentation. Link real entrypoints to declaration/signature/body queries; show retained snapshots, edit/reuse behavior, diagnostics/cancellation and checked handoff. Finish the two cross-linked example traces. |
| HD07 | Audit all coverage rows, rendered diagrams/links and public docs; perform focused validation below; reconcile architecture and roadmap. No unexplained required coverage or carried documentation failures remain. |
| HD08 | User-requested follow-up: revise every model family under `hir/` with source-to-field examples and reference targets; update the reading guide. Cover all record fields/enum payloads, distinguish absence/recovery/synthetic metadata and current unused surfaces. Preserve executable definitions; use focused documentation/render/link/structure checks. |

## Focused validation

Inspect implementation and existing tests before asserting behavior, especially
`tests/lower.rs`, `tests/resolver.rs`, `tests/recovery.rs`, import namespace/provenance
tests and analysis arena/owner/identity/query tests. Reading these is not an
instruction to run their suites. No failing test is required for comment edits.

Batch the final documentation build after HD01-HD06:

```text
cargo rustdoc -p kagari-hir --lib -- --document-private-items -D missing_docs -D rustdoc::broken_intra_doc_links
```

Run only a few runnable documentation examples covering lowering/storage, analysis
and cross-module lookup. Use filtered `cargo test -p kagari-hir --doc <filter>`
commands and record the actual filters/counts. If the entire doctest inventory is
already only those few examples, one `cargo test -p kagari-hir --doc` is sufficient.
Do not add structural mirror tests, a new snapshot framework or a permanent dump
tool. Temporary probes and generated output belong under ignored `target/`.

Inspect rendered `Body`, `ExprId`, `ExprKind`, `SourceDeclRef`, `NameTable`,
`TypeTable`, `AnalysisSnapshot` and `CheckedProgram` pages. Check diagrams against
allocators/accessors and table writers, not merely whether Markdown renders.
The missing-docs lint cannot verify private-state coverage or explanation quality.

Run changed-file Rust formatting checks, the structure checker once at the coherent
Rust documentation checkpoint, and local-link/content plus `git diff --check` checks.
Rerun successful builds/tests only after relevant changes or concrete failures.
Planning-only changes need link/content and diff checks, no Cargo invocation.
No local workspace tests, complete HIR unit suite, full Clippy run or feature/backend
matrix: GitHub CI owns broad testing. Report actual local results and CI status
separately. Preserve unrelated work; do not fix behavioral defects as documentation.

## Acceptance record

HD01-HD07 are complete. All 125 production modules have orientation documentation;
strict Rustdoc covers the public surface, including generated IDs. The
[reading guide](architecture/hir.md) contains storage, semantic-table and query
views, the local arithmetic trace and the three-module glob/alias trace.
Required rendered pages were inspected, including their diagrams, table formatting
and identity lookup links. Existing SA1-SA3 behavior is documented without changing
it; no new architectural discrepancy requires a separate review entry.

- Strict Rustdoc passes without warnings, missing public docs or broken intra-doc links.
- Three runnable doctests pass: `lower::lower_module`, `analyze_source` and
  `program::AnalysisSnapshot::check_program`. The first run exposed trailing syntax
  trivia in a span assertion; the example now documents that range, and only
  `lower::lower_module` was rerun after correction (one passed, two filtered out).
- Changed-file formatting, local document links and `git diff --check` pass.
  The structure checker inspected 921 Rust files: zero violations or exceptions.
- Token comparison against the pre-task source confirms comment/format-only changes
  in 96 Rust files, except the permitted ID macro documentation-attribute forwarding.
  Existing user edits were preserved and excluded from the documentation commit.

No local full test suite, Clippy or feature/backend matrix was run. GitHub CI has
not been run or observed for this local documentation checkpoint. No carried
documentation failures remain.

## HD08 source-to-field follow-up acceptance (2026-10-10)

At the user's request, all 17 files under `src/hir/` now explain the data models
through concrete syntax-to-field mappings. Record/enum examples identify generated
identities, ID target storage, empty buffers, optional syntax, synthetic name nodes
and recovery. Trait declarations versus impl definitions, member input/output
bounds, name qualification versus call arguments and expression/place/pattern
ownership are explicit. Unpopulated method registries/ADT links and rejected type
defaults are distinguished from accepted language behavior. Offline native type
examples explicitly require declaration parsing, not ordinary script parsing.

The reading guide indexes these mappings and traces associated constants/types
and method links. No model cleanup, local-function feature or stable-ID redesign
was activated. Review findings SA11-SA14 remain deferred. Executable Rust token
comparison against the pre-HD08 files passes after normalizing an optional
trailing field comma introduced by formatting; layouts, visibility and behavior
are unchanged. Existing parser and review-document edits were preserved.

Validation:

- `cargo rustdoc -p kagari-hir --lib -- --document-private-items -D rustdoc::broken_intra_doc_links -D rustdoc::invalid_html_tags` passes. Generated HTML for 16 representative model pages was inspected for example content and consistent table columns, including the escaped pattern-alternative pipe.
- `cargo test -p kagari-hir --doc` passes all three existing runnable examples.
- A temporary probe under ignored `target/` verifies 15 documented fragments with the correct parser mode and checks representative associated-member/default and qualified/member/generic-call mappings against lowering. This validates syntax/storage, not full semantic acceptance of every contextual fragment. No permanent tests or dump framework were added.
- `cargo fmt -p kagari-hir -- --check`, the structure checker (1003 Rust files; zero violations/exceptions), Markdown links/anchors, CRLF preservation and `git diff --check` pass.

The initial whole-crate strict command with `-D missing_docs` fails on unchanged
imports APIs, including `imports/bindings.rs::PerNamespace`, `LookupOutcome`
variants and `imports/solver.rs` work/limit records. Newly introduced Rustdoc link
and HTML markup errors were corrected. The remaining missing-docs findings are
outside HD08's `hir/` model scope and are not suppressed or claimed as passing.
Follow-up owner: imports/namespace/solver documentation at its next documentation
checkpoint. The earlier HD01-HD07 acceptance record is historical, not evidence
that the current whole-crate missing-docs audit passes. No workspace suite,
Clippy or CI matrix was run for this documentation-only follow-up.
