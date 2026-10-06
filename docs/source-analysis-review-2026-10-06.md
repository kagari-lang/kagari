# Source analysis review

Reviewed on 2026-10-06 against `c2f87c89`. These are open findings and proposed
follow-ups, not an active implementation plan. Performance impact is unmeasured.

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

[ModuleGraph::build](../crates/kagari-hir/src/imports/mod.rs) resolves all modules
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
[same_members](../crates/kagari-hir/src/imports/catalog.rs) finds unchanged members
and re-export targets. Exhausting the limit proceeds to graph construction without
an explicit non-convergence check; the code does not establish why this bound is
sufficient. No failing input has been reproduced.

Establish the bound or use a termination argument for the chosen solver, and
handle exhaustion explicitly rather than treating it as convergence. Cover long
alias/glob chains, valid cycles, unresolved cycles and cancellation.
