# Repository review

Keep ongoing repository review findings in this document rather than creating
dated review files. Entries record observed behavior and possible follow-ups;
they do not activate implementation work.

SA1-SA3 were inspected against `c2f87c89`. Performance impact is unmeasured.

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

[ModuleGraph::build](../crates/kagari-hir/src/imports/builder.rs) resolves all modules
against the previous catalog, rebuilds the catalog and compares it after every
round. Chained public globs need information propagation, but unrelated and stable
modules also repeat this work.

Consider a module work queue with deduplicated entries, reverse dependencies and
updates to only the affected catalog entries. Track unresolved lookups too, so
newly available names trigger retries. Preserve target, visibility and ambiguity
changes, including removals across source revisions. Where dependencies are known,
topological ordering can handle acyclic chains; strongly connected components
(SCCs) can restrict iteration to cyclic groups. Name-level propagation and query
caching are further options, not prerequisites. Measure chains, fan-out and cycles
before choosing a broader design or claiming a speedup.

## SA3 Iteration exhaustion is not distinguished from convergence

The same loop permits at most `2N + 2` rounds for `N` modules and exits early when
[catalog equality](../crates/kagari-hir/src/imports/builder.rs) finds unchanged
candidates and re-export targets. Exhausting the limit proceeds to graph construction without
an explicit non-convergence check; the code does not establish why this bound is
sufficient. No failing input has been reproduced.

Establish the bound or use a termination argument for the chosen solver, and
handle exhaustion explicitly rather than treating it as convergence. Cover long
alias/glob chains, valid cycles, unresolved cycles and cancellation.

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

SA1-SA3 remain separate, unactivated tasks. The solver scheduling and iteration
bound are unchanged; no performance improvement has been measured.

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
Consider splitting aggregate cases for selective execution and reusing immutable
test setup where safe. Preserve fresh-snapshot, runtime isolation and source-free
coverage. Test-harness optimization remains unactivated.
