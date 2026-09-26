# Kagari Implementation Roadmap

[Foundation refactor](foundation-refactor.md) is the sole active R01–R18 execution plan. Its three semantic contracts and checkpoint status define the behavior to implement and verify. Each completed checkpoint requires a Conventional Commit with a `Roadmap-Step: Rxx` trailer. The [performance baseline](performance-baseline.md) records R18 measurements.

The former M1–M11 milestone queue is historical and has been removed from this document. Git history retains its original scope and commits; those milestones do not prescribe current APIs, compatibility branches, artifact formats, or acceptance criteria.

After the foundation track, plan separate work for complete LSP/editor integration, a full incremental dependency database, async and cross-thread execution policy, incremental or generational GC, complete event replay and persistent state migration, advanced JIT optimization, and further standard-library coverage. These tracks reuse the foundation contracts without reopening their semantics.

Completed language extension: ordinary associated types, equality bindings,
projection bounds and qualified projections, preserving the existing static and
dynamic interface model. The [trait contract](spec/traits.md#ordinary-associated-types),
[runnable example](../examples/syntax/associated-types.kgr) and associated-type
integration tests define this checkpoint. GAT, associated consts and advanced
coherence/solver features remain separate future work.
