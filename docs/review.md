# Repository review

Keep ongoing repository review findings in this document rather than creating
dated review files. Entries record observed behavior and possible follow-ups;
they do not activate implementation work.

SA1-SA3 were originally inspected against `c2f87c89`. SA2/SA3 and SA8 are now
implemented and locally accepted; their full CI acceptance remains pending.

## SA1 Inline modules are parsed again

[prepare_declarations](../crates/kagari-hir/src/analysis/declaration_queries.rs)
copies the enclosing source, masks bytes outside each inline module body and
parses that virtual file again. The parent AST already contains the module body;
[lower_module_decl](../crates/kagari-hir/src/lower/item.rs) only records its header.
This repeats lexing/parsing and allocates a full source buffer per child.

Consider lowering existing `ModuleBlock::items()` with an independent child
context. Preserve module ownership, visibility, stable child identities, original
UTF-8/CRLF offsets and diagnostic attribution. Reuse the existing inline-module
navigation tests. Keep declaration, signature and body checking separate.

## SA2 Import resolution rescans every module

Implemented in NR04 and locally accepted in NR05 under the
[name-resolution plan](name-resolution-plan.md). Full CI acceptance is pending.

The old builder rescanned all modules after each propagation step. The
[solver](../crates/kagari-hir/src/imports/solver.rs) now seeds declarations once,
replaces affected module contributions and schedules reverse namespace observers.
Observations include absent/pending names, namespace prefixes and glob membership;
watchers remain distinct from linking dependencies. New revisions rebuild from
current inputs, preserving removal, retargeting, access and ambiguity changes.

On the bounded 48-module named chain, module visits fell from 2,304 to 86. Adding
96 unrelated modules now adds 96 visits (182 total), compared with 6,912 total
visits previously. Diamond and removed-seed workloads show small timing regressions;
closure/proof overhead and measurement limits are documented in the
[measurement ledger](implementation-roadmap.md#name-resolution-sa8-sa2-sa3-ci-pending).
This demonstrates reduced unrelated rescan work, not universal speedup.

## SA3 Iteration exhaustion is not distinguished from convergence

Implemented in NR04; focused failure/publication acceptance passes. Full CI remains
pending. The old `2N + 2` loop could publish its last draft without convergence.

The solver now distinguishes cancellation, work exhaustion and non-convergence.
Queue exhaustion triggers pending-dependency closure; successful publication also
requires settled directives and binding tiers. Category-qualified acyclic proofs
prevent a named/glob cycle from manufacturing an order-dependent target. An exact,
eight-state history detects repetition and a separate input-sized work bound
limits remaining work. No mathematical completeness claim is made for that bound.

The core failure fixture forces both work limits, exercises a non-convergent
cycle and cancellation, checks old cache/snapshot retention and then successful
recovery. Existing cycle tests cover seeded circulation, unseeded unresolved
aliases, cross-category dependencies and source input order. Failures propagate
through HIR and SDK preparation; no partial graph is admitted to checked execution.

## SA4 Import records also represent namespace lookup state

Resolved by the [IR01-IR03 execution plan](import-resolution-plan.md).
Validation and phase commits are recorded in the
[roadmap ledger](implementation-roadmap.md#import-and-namespace-resolution-ir01-ir03-complete).

The previous `ResolvedImport` model combined named bindings, glob roots, module
headers, package/prelude bindings and empty-alias namespace entries. Import-vector
positions served as declaration and namespace identities.

The implementation now separates real directives and their source provenance from
named, tiered candidates and snapshot-qualified targets. A shared
[namespace catalog](../crates/kagari-hir/src/imports/catalog.rs) resolves nested
members with importer visibility and explicit lookup states. Qualified source
units include arena identity; aliases/re-exports share canonical declarations.
Imported signature consumers, compiler handoff, navigation, dependencies and
snapshot reuse use the new boundary. Deep aliases, grouped paths, strong/glob
precedence, unused imports, stale arenas, retained snapshots and source-free/native
execution are covered by behavioral acceptance. See
[implemented ownership](architecture.md#source-namespaces-and-import-ownership).

SA1 remains separate and unactivated. SA2/SA3 were subsequently implemented by
NR04; their bounded measurements and CI status are recorded above.

## SA5 Incremental reuse misses transitive namespace changes (P1)

Resolved; validation is recorded in the [SA5 follow-up ledger](implementation-roadmap.md#import-and-namespace-resolution-ir01-ir03-complete).

Reproduced against `c3294fad`. Signature reuse in
[check_signatures](../crates/kagari-hir/src/lib.rs), full-file/body reuse in
[snapshot](../crates/kagari-hir/src/analysis/mod.rs) and function-body reuse in
[body](../crates/kagari-hir/src/analysis/body_queries.rs) previously compared local
import facts and canonical imported contracts without comparing reachable namespace
bindings. Declaration reuse already performed that comparison, but later reuse
could still retain stale signatures, name resolution or checked body facts.

Setup: `library` declares the items; `exports` publicly aliases them; unchanged
`facade` contains `pub use pkg::exports::*;`; `root` imports
`use pkg::facade as m;`. Change only the aliases in `exports`:

- Rename `value as old` to `value as new`: incremental analysis still accepts
  `m::old()` and retains its navigation target; fresh analysis reports `UnknownName`.
- Swap `number as selected, flag as spare` to `flag as selected, number as spare`,
  where returns are `i32` and `bool`. Add a comment to `root` to exercise body
  remapping: incremental analysis reuses the body of
  `fn main() -> i32 { m::selected() }` and misses the return-type error.
- Swap `A as Selected, B as Spare` to `B as Selected, A as Spare`: incremental
  signature analysis still types `fn accept(x: m::Selected) {}` as `A`; fresh
  analysis returns `B`.

All affected reuse paths now compare reachable namespace bindings. Retaining a
complete file requires exact tables; reusing checked facts permits only the current
module's arena rebasing through the existing remappers, including validated local
variant owner/slot identities. Foreign namespace targets retain complete identities.
[Five regression tests](../crates/kagari-hir/src/imports/cache_tests.rs) cover the
three reproductions, single-function reuse, old snapshots, unrelated file sharing
and local nominal namespace remapping. All five tests pass, as do the HIR suite,
workspace tests completed in segments and the source-free/native feature matrix.

## SA6 Generated cache names differed from analyzed paths on Windows

Resolved at the SDK's [cache publication boundary](../crates/kagari-embed/src/engine/declarations.rs).
Windows canonicalization produced `\\?\F:\...`, which was published verbatim while
the source database normalized separators and drive spelling. The analyzed name
lost the usable drive prefix and differed from the published declaration source.
Four existing `registration_sources` tests failed their absolute-path assertion.

Publication now converts canonical drive paths to ordinary paths and applies the
same source-name normalization as analysis. The unchanged six-test suite passes,
including actual file reads, matching source views, content validation and retained
snapshot navigation. This bounded integration correction preserves all assertions.

## SA7 Routine fixes trigger expensive aggregate test suites

SA5's five focused regressions executed in 1.07s, while unrelated aggregate
language-contract, syntax-example and VM suites each took several minutes in that
validation run. These are observed suite timings, not isolated benchmarks.
Some runners serialize many scenarios inside one test and repeatedly create fresh
analysis/standard-library environments; Cargo cannot schedule those cases separately.

Routine follow-ups now use focused validation under [AGENTS.md](../AGENTS.md).
The [numeric fixture follow-up](implementation-roadmap.md#focused-test-harness-optimization-sa7-numeric-fixtures-complete)
retains all 90 cases and both artifact routes, batches 90 compilations into 4,
and installs/loads 8 runtimes instead of 180. Only scalar locals are shared within
each route; a successful call after every case checks trap cleanup. The four
numeric tests pass in an observed 4.51s; timing conditions and limitations are in
the ledger. Source-free validation and route isolation remain covered.

SDK/other aggregate runners and selectively executing their individual cases
remain separate follow-ups. Reuse immutable setup where safe and preserve tests
that specifically require fresh snapshots or independent mutable runtimes.

## SA8 Separate type/value lookup and unify export information

Implemented by NR01-NR03 and locally accepted with the NR04 solver integration;
full CI acceptance remains pending. The [execution plan](name-resolution-plan.md)
and [roadmap ledger](implementation-roadmap.md#name-resolution-sa8-sa2-sa3-ci-pending)
retain contracts, measurements and phase validation.

Name tables, directive outcomes, host queries and semantic lookup now select Type
or Value explicitly, with independent conflicts and `strong > glob > implicit`
precedence. Prefixes select Type; terminal use sites select their syntactic category.
One ordinary use leaf can introduce both bindings. Dual imports expose both tooling
targets, and a single-target query does not choose an arbitrary winner. Cache
comparison/remapping preserves both categories and qualified declaration ownership.

HIR `Module.exports`, `Export` and `ExportItem` were removed. Resolved bindings
provide public namespace members, including glob re-exports. Portable authored
`ModuleDecl.exports` uses category-aware `ExportName` keys; generated native aliases
retain their declared category and independent native ABI symbol validation.

Existing language/import/native/query/cache fixtures were consolidated around
these contracts. Focused SDK validation covers direct and encoded source-free
execution; the selected Cranelift preparation used `InterpreterFallback`. One
new normal test covers solver failure/publication as a distinct core boundary.
Macros, new constructors, SA1 and SA9 remain outside this implementation.

## SA9 Module paths are copied into internal graph keys and references

[ModuleIdentity](../crates/kagari-common/src/identity.rs) is a portable package/path
value containing owned strings. It is also embedded in `SourceUnit`, binding owners,
namespace keys and dependency sets. Cloning these identities copies strings; hashing
and ordering examine path content. NR04 removed whole-catalog rounds, but retained
portable keys in namespace watchers and derivation sets. This establishes remaining
representation overhead, not a measured bottleneck.

Separate portable module identity from a compact, context-owned module handle.
Intern each identity once; use handles for catalog keys, owners and dependencies,
and borrow the portable identity for diagnostics, registration and artifact boundaries.
The existing [definition table](../crates/kagari-common/src/identity/table.rs) already
interns module roots and shares their paths; investigate extending/reusing that
owner before introducing a second interner. Do not reuse HIR `ModuleId`: it addresses
a local module declaration, not a module in the analysis universe.

The design must define handle ownership and retained-snapshot validity, remap across
independent contexts, retain source revision/arena checks, and preserve deterministic
artifact ordering independently of allocation order. Parent/package/visibility checks
need table queries in place of editing copied path vectors. `Arc<ModuleIdentity>`
alone reduces clone costs but still compares/hashes content with ordinary traits.

Rust reference:

- The resolver's [`Module<'ra>`](https://doc.rust-lang.org/stable/nightly-rustc/rustc_resolve/struct.Module.html)
  wraps `Interned<'ra, ModuleData<'ra>>`: a `Copy` handle with identity-based
  equality, while module data is stored separately. It does not copy a full
  string path into every module reference.
- Definitions, including modules, use [`DefId`](https://doc.rust-lang.org/stable/nightly-rustc/rustc_hir/def_id/struct.DefId.html),
  a `Copy` pair of `CrateNum` and `DefIndex`, for compiler queries.
- Session-local numeric IDs can change after source edits. The
  [incremental cache](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html)
  stores `DefPathHash` and remaps it to the current session's `DefId` when loading.

Apply the same separation in Kagari: portable `ModuleIdentity` at boundaries,
shared identity data in the context, compact handles inside the graph. This does
not require introducing Rust's crate model or adopting its arena lifetimes.

Keep this a separate follow-up from SA8 and SA2/SA3. Measure graph build allocations
and time before/after on the same module workload, separating compilation time;
reuse identity/snapshot and import tests for focused validation. Code migration is
not activated by this review.

## SA10 Generated trait declarations hide native default bodies

Resolved by [ND01-ND04](native-default-bodies-plan.md): generated traits own real
forwarding bodies, parsed `has_default` must agree with registration, and source
compilation consumes checked calls. SDK preparation checks unused defaults before
publishing their source files. Private native helpers and source-free validated
recipes remain; direct dispatch adds no script frame. Broader CI acceptance is
reported separately in the plan.
