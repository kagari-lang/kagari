# Kagari Project Goal

Kagari is a statically typed, GC-backed, hot-reload-first scripting language for
Rust-hosted applications. Game-server business logic is the motivating domain:
skills, combat formulas, quests, activities, protocol handlers and live rule changes.
The same typed host boundary also supports tools and simulations.

Rust owns infrastructure such as IO, scheduling, persistence and concurrency.
Kagari expresses domain models and behavior with Rust-inspired syntax and
Kotlin-like shared-object ergonomics. Complexity needs a concrete product use;
resembling Rust does not require its ownership/lifetime model.

## Product priorities

1. Correct static typing and explicit observable execution/failure semantics.
2. Safe, practical embedding and clear host/script ownership.
3. Hot reload that preserves active state and generation-pinned calls on failure.
4. Recoverable analysis, diagnostics, navigation and interpreter-first debugging.
5. Maintainable boundaries and measured performance across actual workloads.

Generics and nominal traits are ordinary modeling tools, not optional advanced
features. Structs, enums, matching, closures, associated outputs and interface
values follow [traits](spec/traits.md) and [syntax](spec/syntax.md). Those detailed
specifications own supported features; this overview is not a second type-system
checklist. A trait name is an interface value type without script-visible `dyn`.

## Ownership and state

Applications may keep authoritative domain state in Rust, Kagari or both. Script
objects belong to the per-isolate precise tracing GC. Rust host state/resources
remain host-owned and are accessed through installed typed interfaces, opaque
handles and checked paths. Script values do not retain unrestricted Rust borrows.
The GC does not scan Rust object graphs; retained script values use explicit roots.

One script heap is driven on one host thread. Independent isolates may run on
different host threads; shared mutable heap access and script threading are outside
the current model. Host services own resource lifecycle, synchronization and IO.

The active [runtime ownership and host object design](runtime-ownership-and-host-api-design.md)
targets a Send runtime that can move between host workers while retaining exclusive
execution, centralized ownership and automatic host-value retention. This is a
phased replacement of the fixed-thread implementation, not concurrent script
heap access or a claim that current runtime values are transferable.

`val` prevents slot rebinding, `var` permits writes and `const` denotes a checked
compile-time value. Readonly collection views are shallow; aliases may mutate the
same referent. Typed host mutation validates receiver/member/index contracts and
borrow scope while retaining once-only evaluation and completed effects.

## Reload and executable boundaries

Compile and validate candidates before atomic publication. Retain old code,
metadata and dependencies while values/calls still require them. Public contracts,
declared access and host schemas constrain compatibility. Durable state is explicit;
hidden mutable module globals and implicit state migration are not assumed.
[Loading/activation](spec/module-activation.md) defines current behavior; the
[update proposal](update-model-design.md) covers later compatible updates and
explicit state replacement.

Verified MIR supplies interpreter/native lowering facts. Serialized programs are
bounded and validated before execution, without a source frontend. Backends share
language semantics and runtime safety boundaries; wider JIT support must be proven
through actual native execution. Interpreter fallback is selected before entry.

## Host integration and tooling

Installation determines available APIs. Visibility, writeability, scoped borrows,
roots and generation checks remain mandatory. Scripts are primarily trusted;
cooperative cancellation and call-depth protection do not promise hard preemption,
process isolation or generic CPU/memory quotas. Hosts own admission and deadlines.

Compile-time metadata can support registrations, routing, schemas and editor data.
Ordinary game logic uses typed calls/fields rather than reflective string lookup.
Explicit reflection is limited by declared metadata and access adapters.
Tooling should report errors on incomplete source and preserve useful source/debug
origins without making source analysis a runtime dependency.

## Scope boundaries

The current direction does not include script-visible Rust lifetimes/borrow
checking, raw references, Send/Sync, concurrent script execution, higher-kinded
types, unrestricted specialization/overlapping impls, monkey-patching or direct
host IO that bypasses installed services. Async, package distribution, a new
embedding facade and state replacement remain separately queued designs.
Associated types and shared generic interface calls already exist and must not
be excluded by an outdated product overview.

## Document authority

- [Architecture](architecture.md): crate/data ownership and execution boundaries.
- [Specifications](README.md#language-and-execution-specifications): language,
  failure, host, artifact, debugger and backend behavior.
- [Roadmap](implementation-roadmap.md): pending scope, sequencing and open decisions.
- [Grammar](kagari.ebnf) and [syntax audit](syntax-coverage.md): syntax and parser coverage.
- [Measurements](performance-baseline.md): reproducible observations and their limits.
- [AGENTS.md](../AGENTS.md): engineering, structure, verification and commit rules.

The project is unpublished and in early development. Replace obsolete internal
models directly, keep required validation, and add compatibility commitments only
for an actual release/consumer or an explicit user requirement.
