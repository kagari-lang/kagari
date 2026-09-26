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

Completed language extension: generic implementation interface tables, keyed by
impl declaration and concrete arguments, with shared instantiation limits and
cross-module method reachability. See the [trait contract](spec/traits.md#generic-implementation-interface-instances)
and [runnable example](../examples/syntax/generic-interfaces.kgr). Artifact format
44 and runtime ABI v44 reject prior formats without migration.

Completed language extension: host associated-output declarations and dynamic
interfaces, using offline contracts and verified IR forwarding functions through
the existing host boundary. See the [trait contract](spec/traits.md#host-associated-outputs-and-interfaces)
and [embedding example](../crates/kagari-embed/examples/host_interfaces.rs).
Integration tests cover source/artifact execution, JIT-enabled execution,
cross-module generic inputs, GC, reentry, traps and retained reload versions.
Artifact format 45, runtime ABI v45 and KHI v10 reject previous products.

Completed language extension: bounded, acyclic trait inheritance, transitive
static bounds, inherited associated projections and dynamic interface upcasting.
Diamond paths deduplicate the same applied declaration; unrelated same-named
methods are ambiguous. Source, artifact and JIT fallback tests cover cross-module
generic parents, invalid graphs, GC and retained reload versions. See the
[trait contract](spec/traits.md#trait-inheritance-and-upcasting) and
[runnable example](../examples/syntax/trait-inheritance.kgr). Artifact format 46
and runtime ABI v46 reject previous products; KHI remains v10.

Completed language extension: checked default method bodies, explicit override
precedence and fallback for script impls, including generic impls, method-local
generics, associated outputs, closures and imported private helpers. Defaults
are specialized in the impl's module while retaining the trait's checked name
resolution and original debug source. See the [trait contract](spec/traits.md#default-methods)
and [runnable example](../examples/syntax/default-methods.kgr). Artifact format 47
and runtime ABI v47 reject previous products; KHI remains v10. Host mappings
continue to explicitly provide every declared method.

The next trait checkpoints are associated consts, then type-parameterized GAT.
A trait declaring associated consts or GAT, and every
trait inheriting it, will be usable only for static dispatch. Each checkpoint updates executable examples,
semantic and artifact validation, and commits separately. Lifetimes, script-level
`dyn`, higher-kinded type parameters, specialization, negative impls, auto traits
and advanced coherence/solver behavior are outside this sequence.
