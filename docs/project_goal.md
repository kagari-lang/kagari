

# Kagari Project Goal: Hot-Reload-First Game Server Scripting Specification

## 1. Purpose

This document records the Kagari language goals and specification boundaries.

Kagari is a strongly typed, GC-backed, hot-reload-first scripting language designed to work closely with Rust host applications, especially game servers. Its user experience is closer to a Kotlin-style business language with Rust-inspired syntax than to a simplified Rust clone. This document defines language design rules that guide the type system, VM, GC, host ABI, and compiler/runtime implementation.

Kagari borrows Rust's clarity, expression style, enum/trait ergonomics, and type safety, but it does not reproduce Rust's lifetime model, borrow checker, multi-threading model, `dyn Trait` split, or full trait-system complexity.

Kagari is optimized for writing readable, safe, reloadable game logic and game-domain models.

Typical use cases include:

- skill logic
- buff logic
- combat formulas
- game-domain structs, enums, interfaces, and services
- quest progress rules
- activity/event rules
- item usage logic
- NPC interaction logic
- GM/server-side hotfix logic
- small pieces of business logic that must be patched without rebuilding the Rust server

The specification keeps the language comfortable for game server development while keeping the compiler, runtime, and hot reload model realistic to implement.

## 2. Current Repository Context

The repository has a basic language skeleton with separated crates for syntax, semantic analysis, IR, runtime, VM, and CLI. The specification works with that structure instead of replacing it.

Existing architecture:

- `kagari-syntax` owns lexer, parser, and AST-level syntax.
- `kagari-hir` / semantic layers own resolved names, typed constructs, and language meaning.
- `kagari-ir` receives already-checked semantic information and lowers it into execution-friendly forms.
- `kagari-runtime` owns runtime abstractions, GC placeholders, host interoperability boundaries, and hot reload metadata.
- `kagari-vm` owns the interpreter / execution layer.
- `kagari-cli` remains a thin entry point for driving the pipeline.

The README already positions Kagari as:

- strongly typed
- embeddable
- Rust-inspired in syntax and expression style
- GC-backed
- hot-reload-oriented
- not directly exposing Rust's lifetime and borrow-checking model to script authors
- designed around clear host/runtime boundaries
- able to use compile-time metadata for generated registration, routing, validation, and tooling

The specification refines that architecture into detailed design rules.

## 3. Deliverables

The documentation set lives under `docs/spec/`.

Specification files:

```text
docs/spec/README.md
docs/spec/design-goals.md
docs/spec/type-system.md
docs/spec/memory-management.md
docs/spec/generics-and-traits.md
docs/spec/writeability-and-references.md
docs/spec/compile-time-metadata.md
docs/spec/host-interop.md
docs/spec/typed-path-mutation.md
docs/spec/hot-reload.md
docs/spec/game-server-scripting.md
docs/spec/non-goals.md
```

The exact file names may vary to fit repository style, while preserving the same conceptual split.

The documentation is detailed enough that future compiler/runtime implementation tasks can use it as a source of truth.

Compiler/runtime behavior is governed by follow-up implementation tasks. This document focuses on specification and design boundaries.

## 4. High-Level Design Thesis

Kagari is designed around this thesis:

> Kagari is not a Rust replacement. Kagari is a strongly typed, hot-reload-first, Kotlin-style scripting language with Rust-inspired syntax for expressing game-domain models and server business logic.

This implies:

- Rust owns infrastructure: networking, scheduling, persistence, deployment, observability, and host services.
- Authoritative game state may live in Rust, in Kagari, or across both layers depending on the server architecture.
- Kagari owns game-domain modeling, business behavior, formulas, handlers, and mutation rules regardless of where the underlying state is stored.
- Kagari scripts may define ordinary script-owned objects, services, traits, generic helpers, rule configuration, temporary data, and derived state.
- Kagari-owned objects are managed by Kagari's GC.
- Rust host objects are not owned by Kagari's GC.
- Host data access must go through controlled handles, host APIs, typed host-backed views, or typed path mutation.
- Typed path mutation is the bridge between natural Kagari field syntax and Rust-owned state when the embedding application chooses to keep state in Rust.
- Hot reload is a first-class design constraint, not an afterthought.
- Language complexity is justified by game-logic ergonomics, not by type-theory completeness.
- Rust-like syntax must not imply Rust-like ownership, borrowing, lifetimes, or `dyn Trait` user concepts.

Kagari supports compile-time metadata and compiler-assisted code generation, but it does not require a script-visible runtime reflection system. Attributes may be used by the compiler, build tools, or host integration layers to generate code such as handler registries, message dispatch maps, persistence schemas, editor metadata, or validation tables.

## 5. Core Principles

The specification follows these principles.

### 5.1 Strongly Typed, But Not Rust-Complex

Kagari is statically typed enough to catch most game-logic mistakes before a script is loaded.

The language supports:

- primitive types
- structs
- enums
- pattern matching
- functions
- methods
- modules
- first-order generics for business modeling
- Kotlin-style nominal traits/interfaces with Rust-like declaration syntax
- host-defined opaque reference types
- typed field access
- typed function calls
- compile-time attributes for generated registration, routing, persistence schemas, and tooling metadata

The type system stays deliberately constrained.

Kagari does not attempt to support the full complexity of Rust's type system.

Generics and traits are not optional advanced features in Kagari. They are required for comfortable game-server modeling. The constraint is not "avoid generics and traits"; the constraint is "avoid Rust's advanced trait machinery."

Pattern matching is expressive enough for game logic without becoming Rust's full pattern system. Kagari supports enum/struct/tuple destructuring, guards, or-patterns such as `Fire | Ice`, and range patterns such as `1..=10` or `"a".."m"`. Range pattern bounds are literals or compile-time constants, not arbitrary runtime expressions.

### 5.2 Compile-Time Metadata, Not Runtime Reflection

Kagari supports compiler-visible metadata. This metadata is useful for code generation and host integration.

Examples:

```kagari
@handler(LoginRequest)
pub fn on_login(ctx: GameCtx, req: LoginRequest) -> Result<()> {
    ...
}

@persist("player")
pub struct PlayerState {
    id: PlayerId
    level: i32
}
```

A compiler or build step may use this metadata to generate:

- handler registration tables
- message dispatch maps
- persistence schemas
- host binding descriptors
- editor and GM tooling metadata
- hot reload ABI fingerprints
- validation tables

This is not the same as runtime reflection. Ordinary Kagari scripts do not depend on `type_of`, string-based field lookup, dynamic method invocation, or reflection-based mutation. Game logic uses normal typed fields, traits, functions, and generated registration code.

Kagari may keep internal type metadata for the compiler, VM, GC, persistence, and hot reload validation, but that metadata does not automatically become a script-visible reflection API.

### 5.3 High-Performance, Simple GC for Script-Owned Data

Kagari is a GC language, and its GC is designed for an embedded, single-threaded script VM rather than for a general-purpose multi-threaded runtime.

The target GC model is:

- per-VM / per-isolate
- single-threaded
- precise tracing, not conservative stack scanning
- optimized for many short-lived script allocations
- explicit about roots
- independent from Rust host object ownership
- high-performance without requiring concurrent GC threads
- simple enough to reason about during hot reload and host calls

This means:

- script-created objects can be GC-managed
- script authors do not manage memory manually
- there is no Rust-style ownership model exposed to script authors
- there is no script-level borrow checker
- script-owned values and host-owned values must remain explicitly different
- each VM owns its own script heap
- different Rust worker threads may run different Kagari VM instances, but they must not share one mutable script heap

GC only owns Kagari script data. It must not pretend to own Rust host references and must not scan the Rust object graph.

Host values such as `PlayerRef`, `EntityRef`, `BattleRef`, `WorldRef`, or lower-level `HostObjectId` values are treated as opaque, non-owning handles from the GC's point of view.

The high-performance target comes from simple structural choices:

- fast allocation paths, preferably bump-pointer or region-style allocation for young/temporary objects
- a generational or nursery-oriented design for common short-lived script values
- a stable old-object representation that keeps host interop and debugging simple
- explicit root handles for host-retained Kagari values
- simple write barriers only where needed for generational or incremental correctness
- incremental work scheduling within the single VM thread, not concurrent GC worker threads

The GC design does not make script execution multi-threaded. Multi-threading belongs to the Rust host. Kagari may run many VM isolates across host threads, but a single Kagari heap is not mutated concurrently by multiple script threads.

The design also avoids finalizer-driven business logic. Game state cleanup, network resources, database handles, timers, and native resources are controlled by the Rust host, not by unpredictable GC finalization.

The specification documents GC as part of Kagari's runtime contract, because it affects:

- script object lifetime
- host root registration
- hot reload module lifetime
- closure/upvalue lifetime
- module global lifetime
- safe handling of host references
- memory pressure behavior in long-running game servers

### 5.4 Rust-Owned and Kagari-Owned Models Remain Explicit

Kagari does not assume that every application is a game server or that all important state must always live in Rust.

The more general rule is:

> Kagari makes ownership boundaries explicit. Rust-owned state, host-owned infrastructure state, and Kagari-owned GC-managed script data remain clearly distinguishable.

In many embedded use cases, the host application will own some external state and expose controlled handles to Kagari. This is common for networking, persistence, logging, service registries, editor asset databases, workflow engines, and other infrastructure owned by a Rust core.

For game servers, where authoritative state lives is an architectural choice. Some servers may keep player/entity/battle/world state in Rust and expose typed views to Kagari. Other servers may keep more domain state in Kagari-owned objects and ask Rust mainly to persist, schedule, and host services. Kagari models both sides clearly.

Kagari must support both models because both are useful:

1. **Rust-owned domain state** is useful when the host wants authoritative control over persistence, concurrency, dirty tracking, replication, or validation.
2. **Kagari-owned domain data** is useful for temporary objects, rule configuration, generated state, helper collections, test fixtures, and script-level abstractions.

Examples of script-owned data:

- temporary lists, maps, strings, structs, and enums
- local calculation results
- game-domain helper objects such as formulas, rule objects, temporary effects, activity descriptors, and script services
- rule configuration loaded into a module
- script-defined helper data structures
- short-lived objects created during script execution
- module-level constants and immutable tables, if allowed by the module system

Examples of host-owned data:

- network sessions, timers, database connections, service handles, and persistence adapters
- game server player/entity/battle/world/inventory/quest state
- editor documents and asset databases
- simulation world state owned by a Rust engine
- workflow engine state owned by the host
- application resources such as files, sockets, database connections, timers, and service handles
- any data whose lifetime, synchronization, persistence, or security policy is controlled by the host application

Kagari models both sides explicitly:

1. **Script-owned values** are normal Kagari values and may be managed by Kagari's GC.
2. **Host-owned values** are represented by typed handles, typed host-backed views, host APIs, or typed path mutation.

For host-owned values, scripts operate through application-defined types such as:

- `PlayerRef`
- `EntityRef`
- `BattleRef`
- `DocumentRef`
- `AssetRef`
- `WorkflowRef`
- `ResourceRef`
- `HostObjectId`
- `GameCtx`, `ToolCtx`, `EditorCtx`, or other host-defined context types

These handles are safe, typed views into host-owned state. They are not raw pointers, and they do not imply that Kagari's GC owns the referenced host object.

This broader principle keeps Kagari useful beyond game servers while preserving the same safety model:

- scripts may freely create and manipulate Kagari-owned data
- host resources remain under host control
- host APIs define what scripts are allowed to read or mutate
- host references do not become long-lived unmanaged Rust references inside the script VM
- typed host-backed views and typed path mutation provide ergonomic field-style access without giving scripts raw nested host references

Game servers remain the primary motivating domain. The important architectural point is not that one side must own all state, but that both ownership models are explicit and that crossing the Rust/Kagari boundary is typed, validated, and hot-reload-aware.

### 5.5 Hot Reload First

Hot reload shapes the language design from the beginning.

Kagari uses `pub` as the module visibility marker. Public items form the script module boundary, including hot reload entry points visible to the host. Kagari does not add a separate `export` keyword unless a later design needs to distinguish host ABI exposure from normal module visibility.

The specification defines rules around:

- module epochs
- public script entry points
- ABI stability
- concrete public function signatures
- module versioning
- compatibility checks
- reload validation
- script state restrictions
- safe transition between old and new code

Hot reload prefers stateless or near-stateless script modules for pure rule code.

When Kagari owns persistent game-domain state, that state is explicit, versioned, and migratable. Scripts do not rely on hidden persistent mutable globals, background threads, open handles, or persisted closures.

### 5.6 Game Logic Comfort Over Type-System Power

Kagari makes common game logic easy to write:

```kagari
fn on_cast(ctx: BattleCtx, caster: EntityRef, target: EntityRef, skill: SkillId) -> Result<()> {
    val damage = caster.combat.attack * 120 / 100

    target.combat.hp -= damage
    target.buffs[BuffId::Burning].round = 3

    ctx.emit(SkillHit { caster, target, damage })
    return ok
}
```

The language prioritizes this kind of clarity over advanced type-level abstraction.

## 6. Type System Scope

`docs/spec/type-system.md` describes the type system.

The target design supports these concepts conceptually:

- `bool`
- signed integers such as `i32`, `i64`
- floating point types such as `f32`, `f64`
- `String`
- `()`
- tuple types
- array types
- collection types such as `Vec<T>` and `HashMap<K, V>`
- struct types
- enum types
- function types if needed by closures
- opaque host reference types
- `Option<T>`
- `Result<T, E>`

The type system distinguishes:

- value types owned by Kagari
- GC-managed reference types owned by Kagari
- host reference types owned by Rust
- temporary call-frame-scoped host handles

The type system rejects obvious mistakes before runtime:

- assigning `String` to `i32`
- writing to read-only host fields
- calling missing methods
- accessing missing fields
- passing `PlayerRef` where `EntityRef` is required
- writing to a `const` item
- exposing a hot reload entry point with unsupported generic or unstable ABI types

## 7. Memory Management and GC Design

`docs/spec/memory-management.md` defines memory management.

The GC specification describes Kagari's target memory model, not a staged implementation roadmap.

Kagari's GC is a high-performance but simple collector for single-threaded script isolates embedded in a Rust host.

The memory model is:

```text
Rust Host
  - owns threads, scheduling, services, networking, persistence, and infrastructure resources
  - may own selected domain state if the embedding application chooses that architecture
  - may run multiple Kagari VM isolates on different host threads
  - may keep Kagari values alive only through explicit runtime roots

Kagari VM Isolate
  - owns one script heap
  - runs script code on one thread at a time
  - performs GC for script-owned objects only
  - never traces into the Rust host object graph
```

The GC is precise. It traces known Kagari roots instead of scanning arbitrary native stack memory.

Required root sources include:

- VM value stack
- call frames
- local variables and temporaries visible to the VM
- closures and upvalues
- module objects and public module bindings
- constant-pool values that are GC-owned
- explicit host roots
- temporary runtime roots used during allocation, host calls, compilation, or module loading

Host-retained Kagari values must be registered through explicit root handles. Rust values stored in normal Rust variables do not keep Kagari objects alive unless they are registered with the runtime.

The GC distinguishes these categories:

- immediate scalar values such as integers, floats, booleans, and small enum tags
- GC-managed Kagari objects such as strings, lists, maps, structs, closures, and module objects
- opaque host handles such as `PlayerRef`, `EntityRef`, `BattleRef`, or `HostObjectId`
- temporary call-frame-scoped host handles

The high-performance target prioritizes allocation throughput and predictable pauses without adding multi-threaded GC complexity.

Design rules:

- use per-isolate heaps to avoid synchronization in the common path
- use fast allocation for temporary objects, ideally bump-pointer or region-style allocation
- optimize for short-lived script values through a nursery or generational strategy
- keep old or long-lived objects in a representation that is easy to trace, debug, and expose through stable handles
- schedule incremental GC work on the VM thread when useful, using allocation or tick budgets
- avoid concurrent GC worker threads in the language runtime
- avoid sharing mutable GC objects across VM isolates

The spec does not mandate exact implementation names such as mark-sweep, mark-region, copying nursery, Immix-style blocks, or handles. It defines the intended tradeoff:

> Kagari uses a simple, precise tracing design with fast allocation and good behavior for short-lived objects, while avoiding the complexity of a fully concurrent, shared-heap garbage collector.

The GC must not manage Rust host state.

For example, this Kagari value is GC-managed:

```kagari
val names = ["alice", "bob", "carol"]
```

But this value is only an opaque host handle:

```kagari
fn on_login(ctx: GameCtx, player: PlayerRef) {
    player.login_count += 1
}
```

In the second example, `player` may be represented in Kagari as a typed handle, but the actual player object, its inventory, its database identity, and its dirty tracking state remain owned by Rust.

Typed path mutation avoids unnecessary GC pressure. A deep assignment such as:

```kagari
player.inventory.items[item_id].count -= 1
```

does not allocate long-lived proxy objects for `inventory`, `items`, or `item`. It lowers to a typed path mutation that the host/runtime can execute directly.

The GC specification also states what Kagari avoids:

- no script-visible manual memory management
- no script-visible Rust ownership model
- no conservative native stack scanning
- no tracing into arbitrary Rust object graphs
- no shared mutable script heap across host threads
- no script-level threads in the GC model
- no finalizer-driven gameplay or resource cleanup
- no reliance on GC timing for business logic
- no requirement that host references be GC-owned

The GC is compatible with hot reload.

Module objects, closures, and constants from old module epochs remain alive while they are still reachable from active call frames, closures, epoch-bound runtime objects, or explicit roots. Once an old epoch is no longer reachable, normal GC can reclaim its script-owned objects.

Script-visible `static` module storage is deferred until its hot reload semantics are explicitly designed.
Persistent script state does not store module namespace views.

The hot reload system does not forcibly destroy old module objects. It publishes new epochs and allows ordinary reachability to determine when old script objects can be collected.

## 8. Generics Design

`docs/spec/generics-and-traits.md` defines a constrained generics model.

Kagari supports **first-order generics**.

Allowed examples:

```kagari
struct Pair<A, B> {
    first: A
    second: B
}

enum Option<T> {
    Some(T)
    None
}

enum Result<T, E> {
    Ok(T)
    Err(E)
}

fn first<T>(items: Vec<T>) -> Option<T> {
    if items.is_empty() {
        return None
    }
    return Some(items[0])
}
```

Generic support includes:

- generic structs
- generic enums
- generic functions
- generic methods
- generic traits in the form `Trait<T>`
- simple trait bounds such as `T: Eq`
- multiple simple bounds such as `T: Eq + Hash`

Generic support excludes from the core design:

- higher-kinded types
- generic associated types
- associated types
- const generics
- lifetime parameters
- type-level computation
- specialization
- negative impls
- auto traits
- complex blanket impls
- overlapping impls
- conditional impls written by users unless a later spec explicitly allows a constrained form

Rule:

> Kagari supports business-level generics, not type-gymnastics-level generics.

Good use cases:

```kagari
fn clamp<T: Ord>(value: T, min: T, max: T) -> T
fn contains<T: Eq>(items: Vec<T>, value: T) -> bool
fn pick_random<T>(items: Vec<T>) -> Option<T>
```

Bad use cases that Kagari does not optimize for:

```kagari
fn map_container<F<_>, A, B>(container: F<A>, f: Fn(A) -> B) -> F<B>
trait Functor<F<_>>
```

## 9. Trait Design

Kagari traits are Kotlin-style nominal interfaces with Rust-like syntax.

Traits are a core game-domain modeling feature. They are usable both as generic bounds and as ordinary interface types. Kagari does not expose Rust's `dyn Trait` distinction to script authors.

Allowed conceptual examples:

```kagari
trait Damageable {
    fn hp(self) -> i32
    fn set_hp(self, value: i32)

    fn damage(self, amount: i32) {
        val next = max(0, self.hp() - amount)
        self.set_hp(next)
    }
}

impl Damageable for EntityRef {
    fn hp(self) -> i32 {
        return self.combat.hp
    }

    fn set_hp(self, value: i32) {
        self.combat.hp = value
    }
}
```

Traits support:

- required methods
- default methods
- simple trait bounds
- simple supertraits if needed
- `impl Trait for Type`
- generic traits in the form `trait Repository<T>`
- using a trait name directly as an interface type, such as `Vec<SkillEffect>`

Traits avoid:

- associated types
- GAT
- HKT
- specialization
- negative impls
- auto traits
- complex blanket impls
- Rust object-safety terminology
- explicit `dyn Trait` syntax
- trait upcasting as a user-facing feature
- advanced coherence rules

Use generic traits instead of associated types.

Instead of this Rust-like style:

```kagari
trait Iterator {
    type Item
    fn next(self) -> Option<Self::Item>
}
```

Kagari style:

```kagari
trait Iterator<T> {
    fn next(self) -> Option<T>
}
```

Instead of:

```kagari
trait Repository {
    type Entity
    fn get(self, id: i64) -> Option<Self::Entity>
}
```

Kagari style:

```kagari
trait Repository<E> {
    fn get(self, id: i64) -> Option<E>
}
```

## 10. Trait Values and Interface Dispatch

Kagari does not copy Rust's trait object model.

There is no script-level `dyn Trait` syntax. A trait name can be used directly as an interface type:

```kagari
trait SkillEffect {
    fn apply(self, ctx: BattleCtx, caster: EntityRef, target: EntityRef) -> Result<()>
}

val effects: Vec<SkillEffect> = [
    DamageEffect { amount: 100 },
    HealEffect { amount: 50 },
    AddBuffEffect { buff: BuffId::Burning, rounds: 3 },
]

for effect in effects {
    effect.apply(ctx, caster, target)
}
```

This is dynamic interface dispatch in the ordinary language model, similar to Kotlin interfaces. Script authors do not need to think about Rust trait objects, `Box<dyn Trait>`, `&dyn Trait`, `Sized`, object safety, or lifetimes.

### 10.1 Generic Bounds and Interface Types

Kagari distinguishes two uses of the same trait declaration:

1. **Generic bounds** are for type-checked reusable code.

   ```kagari
   fn apply_damage<T: Damageable>(target: T, amount: i32) {
       target.damage(amount)
   }
   ```

2. **Interface types** are for heterogeneity and runtime dispatch.

   ```kagari
   fn run_effects(ctx: BattleCtx, caster: EntityRef, target: EntityRef, effects: Vec<SkillEffect>) -> Result<()> {
       for effect in effects {
           effect.apply(ctx, caster, target)
       }
       return ok
   }
   ```

The syntax stays simple: `SkillEffect` means the trait/interface type, not `dyn SkillEffect`.

### 10.2 Runtime Representation

Internally, an interface value may carry:

```text
InterfaceValue {
    value: ValueHandle,
    trait_id: TraitId,
    impl_id: ImplId,
    dispatch_table: TraitDispatchTable,
}
```

This representation is an implementation model, not a source-language concept.

`value` may refer to:

- a GC-managed Kagari object
- a Kagari value boxed into a GC object when necessary
- an opaque host handle such as `HostObjectId`

The interface value does not own Rust host state. If the underlying value is host-owned, the interface value only carries a handle and dispatch metadata.

### 10.3 Interface-Callable Trait Methods

A method is callable through a trait/interface type only if it has a stable runtime dispatch shape.

A method is interface-callable if:

- the method has a receiver such as `self`
- the method has no method-level generic type parameters
- the method does not require compile-time specialization
- the method does not return `Self` through the interface
- the method does not take `Self` as a normal parameter, except for the receiver
- the method does not depend on associated types
- the method does not depend on hidden generic parameters
- the method has a stable ABI after the concrete type is erased

Allowed example:

```kagari
trait SkillEffect {
    fn apply(self, ctx: BattleCtx, caster: EntityRef, target: EntityRef) -> Result<()>
}
```

Not interface-callable:

```kagari
trait CloneLike {
    fn clone(self) -> Self
}
```

The problem is not GC or memory safety. The problem is that `Self` means the hidden concrete type, and that type is no longer statically known through an interface-typed value.

Use an explicit interface return type instead:

```kagari
trait CloneEffect {
    fn clone_effect(self) -> SkillEffect
}
```

Not interface-callable:

```kagari
trait Mapper {
    fn map<T>(self, value: T) -> T
}
```

Generic methods require compile-time instantiation or a more complex runtime generic calling convention, so they are not callable through an interface-typed value.

### 10.4 Generic Traits as Interface Types

Generic traits may be used as interface types only when all generic arguments are known in the type.

Allowed conceptually:

```kagari
val source: Source<Item> = item_source
val handler: Handler<Event, Result<()>> = event_handler
```

Here `Item`, `Event`, and `Result<()>` are part of the interface type.

Not allowed:

```kagari
val source: Source<_> = item_source
```

Kagari avoids implicit existential type parameters in interface values. Hidden generic parameters make hot reload compatibility, runtime dispatch, and error messages harder to reason about.

### 10.5 Default Methods

Default trait methods can be supported, and the interface dispatch model defines how they are represented.

Rule:

- required methods appear as dispatch-table slots
- default methods may be lowered into ordinary helper functions or dispatch-table thunks
- default methods callable through an interface type must still satisfy the same interface-callable rules

This avoids treating default methods as a special separate dispatch mechanism.

### 10.6 Enums Remain Useful for Closed Sets

For many scenarios, enums are still a good choice when the set of variants is known:

```kagari
enum SkillEffectKind {
    Damage(DamageEffect)
    Heal(HealEffect)
    AddBuff(AddBuffEffect)
}
```

Enums are easier to serialize, inspect, version, and exhaustively match.

Trait/interface values are more appropriate when the implementation set is open-ended, for example:

- skill effects and buff effects defined across multiple script modules
- game-domain services with interchangeable implementations
- plugin-style tool APIs
- editor extensions
- workflow steps
- host-registered capabilities
- heterogeneous collections whose variants cannot be known by the library author

### 10.7 Hot Reload Rules

Trait/interface values must be compatible with hot reload.

The hot reload rules define:

- whether an interface value created by an old module epoch keeps using the old implementation table
- whether new interface values use the latest published implementation table
- how implementation ABI fingerprints are compared during reload validation
- how removed or changed trait methods affect existing interface values
- whether interface values may cross module epoch boundaries

Rule:

> An interface value carries enough implementation identity to keep existing calls stable. Reloading a module publishes new implementation tables for new values, while old values remain valid until they become unreachable.

This matches the broader hot reload model: old reachable script objects remain alive until ordinary GC can reclaim them.

### 10.8 Non-Goals for Trait Values

Kagari avoids:

- Rust-style object safety terminology as the main user-facing model
- explicit `dyn Trait`
- dynamic calls to generic methods
- dynamic dispatch involving associated types
- trait upcasting as a user-facing feature
- dynamic downcasting as a common programming pattern
- reflection-heavy method lookup for every interface call
- implicit runtime specialization
- hidden generic parameters in interface values

The key rule is:

> GC removes the need for Rust-style ownership and lifetime restrictions, but interface dispatch still needs a small set of rules for which methods can be called after the concrete type is erased.

## 11. Binding and Field Writeability Model

`docs/spec/writeability-and-references.md` defines binding and field writeability.

Kagari does not expose Rust's borrow checker.

Kagari object fields and referenced objects are mutable according to their type, field policy, and host policy. Script authors do not mark ordinary object references as writable before changing game-domain state.

Local bindings use `val` when the binding cannot be rebound and `var` when the binding itself may be rebound.
Function parameters are not rebindable in v1.
Fields use the same rule: `val field: T` is read-only after initialization, and `var field: T` is writable.
This keeps ordinary data modeling conservative while still making mutable game state explicit.

`const` means:

- this item is a compile-time constant value
- the const item itself cannot be reassigned
- the const item is not module storage

Ordinary mutable bindings do not mean:

- Rust-style exclusive borrow
- lifetime-tracked `&mut T`
- a guarantee that no other alias exists in the entire runtime
- Rust-style exclusive access

Example:

```kagari
fn damage(target: EntityRef, amount: i32) {
    target.combat.hp -= amount
}
```

This is allowed because modifying `target.combat.hp` changes the referenced object or typed path. It does not rebind the local `target` binding.
It is still subject to field-level write policy; `combat` and `hp` must be writable where the assignment occurs.

Rebinding a local binding requires `var`:

```kagari
fn choose_target(target: EntityRef, fallback: EntityRef) -> EntityRef {
    var selected = target
    selected = fallback
    return selected
}
```

Assigning directly to a function parameter is rejected in v1.

Runtime safety is still enforced by the host boundary.

## 12. Compile-Time Metadata and Code Generation

`docs/spec/compile-time-metadata.md` defines compile-time metadata.

Kagari supports attributes as compile-time metadata for compiler passes, build tooling, generated code, and host integration.

This feature exists for cases such as:

- collect all message handlers and generate a dispatch map
- collect all persisted state types and generate persistence schemas
- collect all public hot reload entries and generate ABI fingerprints
- collect host binding declarations and generate registration descriptors
- generate editor, inspector, or GM tooling metadata
- run static validation over annotated APIs

Example:

```kagari
@handler(LoginRequest)
pub fn handle_login(ctx: GameCtx, req: LoginRequest) -> Result<()> {
    ...
}

@handler(BattleCommand)
pub fn handle_battle(ctx: GameCtx, cmd: BattleCommand) -> Result<()> {
    ...
}
```

The compiler or build step may generate a conceptual table:

```text
LoginRequest  -> handle_login
BattleCommand -> handle_battle
```

This avoids forcing scripts to manually maintain stringly typed maps while still keeping dispatch strongly typed and visible to tooling.

This does not imply script-visible runtime reflection. Kagari avoids:

- `type_of(value)` as a normal scripting primitive
- string-based field lookup in ordinary game logic
- reflection-based field writes
- runtime dynamic method invocation
- using reflection as the trait dispatch mechanism
- compile-time metaprogramming powerful enough to become a separate macro language

The intended model is:

```text
source attributes
  -> compiler metadata
  -> generated registration / schema / validation data
  -> ordinary typed runtime code
```

## 13. Host Interoperability

`docs/spec/host-interop.md` defines host interoperability.

The host interop design establishes the boundary between Kagari and Rust.

Rules:

- Kagari GC owns script-created objects only.
- Rust owns host infrastructure state and any game state the embedding application chooses to keep on the host side.
- Host references must be explicit opaque values.
- Host-owned mutable access must be scoped to a call frame, typed path operation, or host-controlled execution boundary.
- Scripts do not retain host mutable references beyond the call in which they are valid.
- Host APIs validate arguments and reject illegal access.
- Host APIs record dirty paths and emit events.
- Host bindings expose typed views over Rust-owned structs without exposing raw Rust references.
- The frontend does not depend directly on runtime implementation details.

Example host-facing script:

```kagari
pub fn on_item_use(ctx: GameCtx, player: PlayerRef, item_id: ItemId) -> Result<()> {
    player.inventory.items[item_id].count -= 1
    player.combat.hp += 100
    ctx.emit(ItemUsed { player, item_id })
    return ok
}
```

Even though this looks like direct nested field access, Kagari does not hold nested Rust `&mut` references internally. `player`, `player.inventory`, and `player.inventory.items[item_id]` may be represented as typed host handles or host-backed path views.

## 14. Typed Path Mutation

`docs/spec/typed-path-mutation.md` defines typed path mutation.

This is one of the most important Kagari design features for architectures that keep authoritative state in Rust while writing business logic in Kagari.

Kagari allows natural deep field access syntax:

```kagari
target.combat.hp -= damage
player.inventory.items[item_id].count -= 1
player.quest.active[quest_id].progress = 3
```

But the semantic model is not:

```text
Take &mut player
then take &mut player.inventory
then take &mut player.inventory.items[item_id]
then take &mut count
```

Instead, the semantic model is:

```text
Compile the assignment target into a typed path.
Ask the Rust host/runtime to apply a checked mutation to that path.
```

Nested host-backed values assigned to locals do not become Rust references. They are script-visible typed views or path handles:

```kagari
val combat = player.combat
val inventory = player.inventory

combat.hp -= damage
inventory.items[item_id].count -= 1
```

Conceptually:

```text
combat    = View(root = player, path = combat)
inventory = View(root = player, path = inventory)

ModifyPath(root = player, path = combat.hp, op = SubAssign, value = damage)
ModifyPath(root = player, path = inventory.items[item_id].count, op = SubAssign, value = 1)
```

This lets Kagari scripts freely keep multiple views into the same Rust-owned object graph without exposing Rust's aliasing and borrow rules.

Conceptual lowering:

```kagari
player.inventory.items[item_id].count -= 1
```

becomes:

```text
ModifyPath(
    root = player,
    path = inventory.items[item_id].count,
    op = SubAssign,
    value = 1,
)
```

Another example:

```kagari
target.buffs[BuffId::Burning].round = 3
```

becomes:

```text
SetPath(
    root = target,
    path = buffs[BuffId::Burning].round,
    value = 3,
)
```

Typed path mutation supports:

- static field existence checks
- static type checks
- field access and mutation policy checks
- read-only field rejection
- host authorization hooks
- dirty path recording
- precise persistence update generation
- event dispatch hooks
- safer hot reload compatibility

This design is especially important for game servers because the host often needs to know exactly what changed.

Example effects:

```text
modified:
- combat.hp
- inventory.items[item_id].count
- quest.active[quest_id].progress
```

Potential Rust-side consequences:

```text
mark dirty field
append game event
generate MongoDB update
sync client state
write audit/debug logs
run validation hooks
```

Typed path mutation is a scripting ergonomics feature, not a promise to expose raw nested host references.

## 15. Local Variables and Nested Views

The typed path design defines what happens when a nested object is assigned to a local variable.

For example:

```kagari
val combat = player.combat
combat.hp -= 100
```

Rule:

> Host-backed nested values may be assigned to locals as typed views or path handles, not as Rust references.

Allowed:

```kagari
player.combat.hp -= 100
```

Also allowed:

```kagari
val combat = player.combat
val inventory = player.inventory

combat.hp -= 100
inventory.gold += 10
```

The local values above preserve enough root and path identity for reads and writes to lower into checked host operations.

Not the semantic model:

```text
combat = &mut player.combat
inventory = &mut player.inventory
```

Reason:

- raw detached Rust references complicate lifetime rules
- detached mutable proxies complicate hot reload
- detached mutable proxies complicate dirty tracking
- detached mutable proxies make aliasing behavior harder to explain

The language allows ergonomic local views, but those views remain Kagari values with host-checked read/write behavior.

## 16. Game Server Scripting Model

`docs/spec/game-server-scripting.md` defines the game server scripting model.

Kagari is used in a game server with explicit Rust/Kagari responsibilities.

Rust responsibilities:

- networking
- scheduling
- persistence
- actor/service lifecycle
- multi-threading if needed
- database access
- long-lived player/entity/battle/world state, when the server chooses a Rust-owned state model
- persistence snapshots and storage for game state
- dirty tracking
- event dispatch
- logging/tracing
- host API implementation
- typed path access adapters for Rust-owned state, when Rust owns that state
- reload coordination
- rollback/gray release strategy

Kagari responsibilities:

- game-domain modeling
- script-visible models over Rust-owned state when state is host-owned
- script-owned state when the server chooses a Kagari-owned state model
- script-owned temporary data, helper objects, and rule configuration
- game rules
- combat formulas
- skill effects
- buff effects
- item usage logic
- quest logic
- activity rules
- small hotfix logic
- readable business behavior

Kagari is not responsible for:

- server threading
- raw database access
- direct network IO
- long-running background tasks
- unrestricted filesystem IO
- bypassing persistence, validation, or security boundaries owned by the host

Script style:

```kagari
pub fn on_cast(ctx: BattleCtx, caster: EntityRef, target: EntityRef, skill: SkillId) -> Result<()> {
    val atk = caster.combat.attack
    val damage = atk * skill.power / 100

    target.combat.hp -= damage

    if skill.has_buff(BuffId::Burning) {
        target.buffs[BuffId::Burning].round = 3
    }

    ctx.emit(SkillHit { caster, target, skill, damage })
    return ok
}
```

The script expresses the rule. Depending on the chosen architecture, it may operate over Rust-owned state through typed handles and typed path mutation, or over Kagari-owned state managed by the script runtime. The host enforces infrastructure, persistence, scheduling, and security boundaries.

## 17. Hot Reload Specification

`docs/spec/hot-reload.md` defines hot reload.

The hot reload design covers:

- module identity
- module epoch
- loaded module metadata
- public entry signatures
- ABI fingerprints
- validation before publish
- failure handling
- compatibility rules
- state migration, if any
- restrictions on global state
- restrictions on closures/coroutines, if such features exist later
- call-frame behavior during reload

Rules:

- Public hot reload entry points use concrete types.
- Public hot reload entry points are not generic.
- Existing active calls finish with the module version they started with.
- New calls use the latest successfully published module epoch.
- Failed reload validation does not replace the currently active module.
- script-visible `static` module storage is deferred until its hot reload semantics are explicitly designed
- module namespace views are not first-class durable references
- Mutable state that must survive reloads is host-owned, persisted, or explicitly versioned with migration rules.
- Script-owned persistent state requires explicit migration rules.

Good public entry function:

```kagari
pub fn on_cast(ctx: BattleCtx, caster: EntityRef, target: EntityRef, skill: SkillId) -> Result<()>
```

Bad public entry function:

```kagari
pub fn on_event<T: GameEvent>(ctx: GameCtx, event: T)
```

Generic functions are allowed internally, but public hot reload ABI remains concrete.

## 18. Non-Goals

`docs/spec/non-goals.md` defines non-goals.

The document explicitly says that core Kagari does not support:

- Rust lifetimes
- Rust borrow checker as a script-visible feature
- raw Rust references exposed directly to scripts
- user-visible ownership transfer rules like Rust
- multi-threading in the script language
- concurrent mutation of one script heap from multiple script threads
- shared mutable GC objects across VM isolates
- `Send` / `Sync` style type system
- HKT
- GAT
- associated types
- const generics
- specialization
- negative impls
- auto traits
- complex blanket impls
- overlapping trait impls
- trait-level type computation
- explicit `dyn Trait`
- generic public hot reload entry points
- script-visible runtime reflection
- reflection-based field mutation
- dynamic method invocation through reflection
- script-owned long-lived mutable host references
- direct database access from scripts
- direct network IO from scripts
- bypassing host persistence, scheduling, validation, or security boundaries

This list exists to keep Kagari focused.

Excluded features may be reconsidered later only if a concrete game-server use case justifies the implementation cost.

## 19. Implementation Guidance

The design informs future implementation.

Compiler phases treat deep assignment targets as `Place` or `TypedPath` nodes after semantic analysis.

Conceptual pipeline:

```text
AST place expression
  -> resolved HIR place
  -> typed place / typed path
  -> IR path mutation instruction
  -> VM/runtime host path mutation call
```

Example IR concepts:

```text
ReadPath(root, path)
SetPath(root, path, value)
ModifyPath(root, path, op, value)
CallHostFunction(symbol, args)
EmitHostEvent(event)
```

The exact IR names may differ, but the semantic boundary is stable.

## 20. Documentation Style Requirements

Write the spec documents in clear English.

Use a practical, engineering-oriented style.

Use:

- concrete examples
- explicit allowed/disallowed rules
- small code snippets
- short rationale sections
- game-server scenarios

Avoid:

- academic type theory language unless necessary
- vague statements such as "support Rust-like generics"
- promising features without constraints
- implementation-specific details without stable semantic constraints

Each document answers:

- What problem does this part solve?
- What does Kagari support?
- What does Kagari intentionally avoid?
- Why is this design good for hot reload and game servers?
- What do future implementation tasks preserve?

## 21. Specification Coverage

The specification covers:

- `docs/spec/`
- The specification documents listed above or an equivalent structure
- The docs clearly define Kagari as a strongly typed, GC-backed, hot-reload-first Kotlin-style scripting language with Rust-inspired syntax for Rust-hosted game servers.
- The docs clearly explain first-order generics and Kotlin-style nominal traits/interfaces as core modeling features.
- The docs explicitly reject HKT, GAT, associated types, specialization, negative impls, auto traits, complex blanket impls, and explicit `dyn Trait`.
- The docs explain that `val` declares non-rebindable slots, `var` declares rebindable or writable slots, and `const` is the immutable compile-time value form.
- The docs explain compile-time metadata and generated registration/schema/validation data without requiring script-visible runtime reflection.
- The docs explain host-owned vs script-owned data.
- The docs define Kagari's GC as a per-VM, single-threaded, precise tracing collector for script-owned objects.
- The docs explain that high GC performance comes from per-isolate heaps, fast allocation, generational or nursery-oriented behavior, explicit roots, and simple runtime barriers rather than concurrent GC threads.
- The docs explain that Rust host objects are opaque handles from the GC's point of view and must not be traced as part of the Kagari heap.
- The docs explain typed path mutation for deep field assignment.
- The docs explain why deep mutation does not require nested Rust `&mut` references.
- The docs define hot reload constraints around concrete public entry points and module epochs.
- The docs include game-server examples.
- The docs avoid implementation changes unless strictly necessary.
- The README may link to the spec index.
