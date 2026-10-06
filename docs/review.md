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
