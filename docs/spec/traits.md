# Kagari Trait and Interface System

This document defines Kagari traits and their use as interface value types.

The main goal is to preserve useful abstraction mechanisms from Rust-like languages while keeping the script-facing model closer to Kotlin interfaces than Rust trait objects.

Reflection rules are defined separately in [reflection.md](reflection.md).
Security rules are defined separately in [security.md](security.md).
Host interop rules are defined separately in [host-interop.md](host-interop.md).
Runtime model rules are defined separately in [runtime.md](runtime.md).

## Design Goals

- support static polymorphism through generic trait bounds
- allow trait names to be used directly as interface value types
- support runtime interface dispatch without script-visible Rust borrowing concepts
- support runtime downcast through concrete type identity
- avoid lifetime parameters and borrow-driven object-safety complexity
- keep the implementation model compatible with a GC-backed runtime

## Scope Exclusions

Kagari traits do not reproduce all of Rust's trait features.
The current trait scope excludes:

- script-level `dyn` trait-object syntax
- Rust-style trait object syntax such as borrowed or boxed `dyn` trait objects
- lifetime-parameterized traits
- generic associated types (GAT), including lifetime-parameterized forms
- associated consts
- specialization
- negative impls
- auto traits
- full Rust-style coherence and orphan behavior
- higher-rank bounds
- higher-kinded type parameters and projection-heavy solving beyond ordinary associated types

## Core Model

Kagari traits serve two script-facing purposes:

1. static trait constraints for generic code
2. ordinary interface value types for dynamic dispatch

There is no script-visible split between a trait name and a separate `dyn` trait-object type.
A trait name can be used directly as a type when a value is handled through that interface.

Example:

```kagari
trait Display {
    fn to_string(self) -> String;
}

fn show(value: Display) -> String {
    value.to_string()
}
```

This is an interface call.
The runtime may represent it internally with a value handle, concrete type id, and vtable, but script authors do not write or reason about Rust trait objects.

## Static Trait Bounds

Static trait bounds are used by generic functions and generic types during type checking.

Example:

```kagari
fn show_static<T>(value: T) -> String
where T: Display
{
    value.to_string()
}
```

The important properties are:

- trait bounds participate in name resolution and type checking
- method lookup may be resolved statically for generic code
- no runtime interface object is required in the common static generic case
- no downcast is involved

Bounds resolve in their declaring generic scope. A `where` predicate must name
a generic parameter or an associated projection rooted in a generic parameter;
an unknown name or an unrelated concrete type is an invalid target.
Methods inherit their impl's inline and `where` constraints. A method's own
constraints apply only within that method. If a method declares a parameter with
the same name as an inherited parameter, its uses and bounds refer to the method
parameter; the outer parameter's constraints are not combined with it.
An implicit receiver can still contain the outer parameter and retains its bounds.
Parameter equality uses the declaring owner and position, so same-spelled inner
and outer parameters cannot exchange values merely because their names match.
Implicit `Self` is identified by its trait; substitution in impl signatures and
trait calls replaces only that trait's `Self` throughout composite types.

The current foundation implementation records standard constraint identities and
user trait declaration targets in HIR. It retains navigation in invalid bounds
and reports inherited invalid references once. Applied bounds such as `Show<i32>`
retain their trait declaration and ordered type arguments in HIR and ABI. Local impl
headers may apply a generic trait to concrete types or impl parameters; the
checked method contract substitutes those arguments, validates declared
trait-parameter bounds and retains them in the
interface-table ABI. Private generic
functions now infer argument types and compile reachable concrete instances,
including calls through existing concrete trait implementations. Instances are
deduplicated by declaration and arguments, with configurable growth limits.
Public functions require concrete signatures. Generic impl methods specialize
at reachable concrete receivers. Dynamic interface tables likewise specialize
reachable impl templates by declaration and concrete type arguments.
Static trait-method calls infer method-local generic arguments from their
arguments and expected result, check those arguments against the method's
bounds, and specialize reachable local or dependency-defined implementations.
The method's arguments follow the implementation's type arguments in the
concrete instance key; interface values still reject generic methods.

Struct and enum declarations retain generic binders and inline bounds in checked
signatures. Type applications such as `Cell<i32>` resolve those binders, check
arity, and preserve declaration identity through imports and facades. Independent
signature queries check applied bounds in parameters, returns, fields and payloads
after constructing the shared aggregate catalog. Body analysis consumes those
signatures and checks local annotations and constructors.
Constructors infer arguments from field or variant payload values. Field access
substitutes the receiver's concrete arguments. For example:

```kagari
struct Cell<T> { var value: T }
enum Packet<T> { Data(T) }
fn read<T>(cell: Cell<T>) -> T { cell.value }
fn main() -> Packet<i32> { Packet::Data(read(Cell { value: 7 })) }
```

Reachable aggregate instances use layouts keyed by declaration plus arguments.
They share the configurable instantiation budget with function instances, including
across module boundaries. Recursive growth is rejected before execution. Explicit
constructor arguments and contextual inference can supply parameters absent from
fields/payloads; arguments that remain unknown produce a diagnostic. Public generic type templates are permitted, while public
function entries still require concrete signatures.

## Ordinary Associated Types

An associated type is an output of a trait implementation. It is identified by
its owning trait declaration and member, separately from the trait's input type
arguments. Two implementations with the same receiver and trait inputs cannot be
distinguished by assigning different associated outputs.

```kagari
trait Reader {
    type Item;
    fn read(self) -> Self::Item;
}

struct Number { val value: i32 }
impl Reader for Number {
    type Item = i32;
    fn read(self) -> Self::Item { self.value }
}

fn read<R: Reader>(reader: R) -> R::Item { reader.read() }
fn read_integer<R: Reader<Item = i32>>(reader: R) -> i32 { reader.read() }
fn read_qualified<R: Reader>(reader: R) -> <R as Reader>::Item { reader.read() }
fn read_interface(reader: Reader<Item = i32>) -> i32 { reader.read() }
```

`Self::Item` refers to the current trait's associated member. `R::Item` requires
one applicable trait bound declaring that name. If multiple bounds declare it,
use `<R as Reader>::Item`. A qualified projection must be justified by the
receiver's bounds or a valid concrete implementation; spelling a binding in the
qualified trait does not create a new equality proof.

Trait references accept associated equality bindings after positional arguments,
for example `Reader<i32, Item = String>`. Unknown, duplicate, or out-of-order
bindings are errors. A bound may leave outputs unspecified. A trait used as a
value type must bind every associated type; its method parameters and results
are checked after replacing those projections. Different output bindings denote
different interface types and cannot be exchanged implicitly.

Declarations can constrain outputs, as in `type Item: Display;`. Generic functions
can add projection constraints, as in `where R::Item: Display`. These constraints
are checked against implementation definitions and concrete call arguments, and
are available for static method lookup on the projected value. Generic impls may
define outputs in terms of their own type parameters.

Projection templates remain in checked signatures and canonical ABI metadata.
Reachable static calls normalize them to concrete types during monomorphization;
no runtime generic specialization or runtime projection lookup is introduced.
Cross-module analysis and artifact loading use declaration identity, including
for same-spelled members on unrelated traits. Loading rechecks associated schemas,
method contracts and output bounds before execution.

Every trait impl must define each declared output exactly once. Defaults,
recursive definitions, associated types in inherent impls, GAT and associated
consts are outside this checkpoint. Host implementations declare their ordinary
associated outputs in the same offline interface used for runtime registration;
see [Host Associated Outputs and Interfaces](#host-associated-outputs-and-interfaces).

See [associated-types.kgr](../../examples/syntax/associated-types.kgr) for a
runnable example returning `42`.

## Interface Value Types

A trait name used as a value type denotes an interface value.

Example:

```kagari
val effect: SkillEffect = BurnEffect { rounds: 3 }
effect.apply(ctx, caster, target)
```

Conceptually, an interface value carries:

- a handle to the underlying value
- the concrete runtime type id
- the interface trait id
- a dispatch table for interface methods

This is closer to Kotlin interface values than to Rust's borrow-dependent trait object model.
The value may refer to a GC-managed script object, a boxed script value, or a host-backed value depending on the runtime representation.

## Runtime Representation

A useful internal representation is:

```text
InterfaceObject {
  data: ValueHandle,
  concrete_type_id: TypeId,
  trait_id: TraitId,
  vtable_id: TraitVTableId
}
```

The runtime layout is implementation-defined, but the semantic model preserves:

- dynamic method dispatch
- concrete runtime type identity
- retained identity for future `is<T>` and `downcast<T>` checks

These script-level runtime type tests are planned; they are not executable yet.

This representation is internal.
This representation does not introduce script-visible `dyn` syntax, lifetime parameters, `Sized` rules, or Rust object-safety terminology.

## Trait Declarations

Trait declarations use this surface syntax:

```kagari
trait TraitName<T1, T2> {
    fn method(self, x: T1) -> T2;
}
```

Grammar shape:

```ebnf
trait_item       ::= visibility? trait_decl ;

trait_decl       ::= "trait" IDENT generic_param_clause? supertrait_clause? "{" trait_member* "}" ;
supertrait_clause ::= ":" type_bound_list ;

trait_member     ::= attribute* trait_method | associated_type_decl ;
trait_method     ::= method_sig (";" | block) ;
associated_type_decl ::= "type" IDENT (":" type_bound_list)? ";" ;

method_sig       ::= "fn" IDENT generic_param_clause? "(" method_param_list? ")" return_type? where_clause? ;
```

Trait members are limited to:

- methods; bodies are parsed, but do not yet supply omitted impl methods
- ordinary associated types, optionally constrained by trait or standard bounds
- no associated consts or generic associated types

## Trait Implementation

Trait implementation is distinct from inherent `impl`.

Example:

```kagari
impl Display for Player {
    fn to_string(self) -> String {
        self.name
    }
}
```

Generic implementation:

```kagari
impl<T> Display for Vec<T>
where T: Display
{
    fn to_string(self) -> String {
        ...
    }
}
```

Grammar shape:

```ebnf
impl_block        ::= inherent_impl
                    | trait_impl ;

inherent_impl     ::= "impl" generic_param_clause? type where_clause? "{" impl_item* "}" ;

trait_impl        ::= "impl" generic_param_clause? trait_ref "for" type where_clause? "{" impl_item* "}" ;

trait_ref         ::= path generic_args? ;
```

This keeps the language model clear:

- `impl Type { ... }` means inherent methods
- `impl Trait for Type { ... }` means trait implementation

## Generic Bounds

Trait bounds support:

- direct type parameter bounds in parameter lists
- trailing `where`

Example:

```kagari
fn sort<T>(xs: Vec<T>)
where T: Ord
{
    ...
}
```

Bounds are simple trait references:

```kagari
where T: Display + Clone
```

Ordinary associated projections and associated equality bindings are supported.
Higher-rank bounds, arbitrary type equalities and implicit type-level computation
remain outside the implemented scope.

## Interface Compatibility Rules

Not every trait method shape is suitable for interface dispatch.
The language describes this as interface compatibility rather than Rust object safety.

Interface-callable methods must:

- not return `Self`
- not take `Self` in parameters other than the receiver
- not require method-level generic instantiation at the call site
- not mention unconstrained generic method parameters
- have parameter and return types representable in the runtime value model

Traits may still contain methods that are useful for static generic bounds but are not callable through an interface value.
The compiler rejects use of a trait as an interface type when the trait contains methods that cannot be dispatched dynamically.

## Generic Methods

Trait methods and inherent methods may have generic parameters in the syntax.

Generic methods are primarily a static dispatch feature.
They are not callable through an interface value unless the runtime provides an explicit specialization or adapter mechanism.

This keeps interface dispatch simple and avoids hidden runtime monomorphization.

## Receiver Model

Kagari has a non-Rust reference model, so method receivers are simple and do not imply Rust-style borrowing.

Receiver form:

- `self`

Receiver semantics:

- `self` receives the ordinary value
- receiver passing uses the ordinary parameter value model

This is intentionally not Rust borrowing.

## Planned Downcast

The following syntax and behavior are a design target, not current executable
features. Downcast is defined in terms of concrete runtime type identity, not generic trait reasoning.

Example:

```kagari
if val p = x.downcast<Player>() {
    ...
}

if x.is<Player>() {
    ...
}
```

Downcast model:

- every runtime heap object or host-registered value has a concrete type id
- interface values preserve the concrete type id
- `downcast<T>` succeeds when the stored concrete type id matches `T`
- `is<T>` is a non-consuming boolean check over the same rule

This is much simpler than attempting to infer downcast through trait structure.

## Relationship Between Traits and Downcast

Traits are not the mechanism that determines downcast validity.

Instead:

- traits describe callable capability sets
- interface values carry runtime concrete type identity
- downcast works because concrete type identity is preserved

This avoids conflating:

- compile-time capability reasoning
- runtime type tests

## Type-Checking Guidance

Trait resolution is intentionally simple.

Trait resolution uses:

- explicit impl lookup by concrete type
- explicit bound lookup by generic parameter
- no overlapping impls
- clear ambiguity errors rather than aggressive inference

If there are multiple plausible impl candidates, the compiler rejects the program instead of selecting one implicitly.

## Coherence Guidance

Kagari does not use Rust's full coherence model.

Coherence rule:

- within one compilation world, there must be at most one visible impl of a given trait for a given concrete type

This rule is simple enough to understand and enforce.
Looser host integration behavior requires a separate language or host-ABI extension.

## Host Interoperability

Traits and host object integration are separate concerns.

A host object may:

- implement script-visible traits
- be viewed through a trait/interface value
- participate in `is<T>` and `downcast<T>` if the runtime assigns it a stable concrete type identity

Host borrowing rules do not leak into the script trait model.
If a host-backed value is exposed through an interface, method calls must still respect host registration, capability, path mutation, and call-boundary rules.
Concrete host types with a checked applied trait table satisfy matching static
trait bounds. Ordered arguments distinguish `Readable<i32>` from
`Readable<bool>` on the same host type. A specialized bound call selects the
mapped host method by declaration identity and uses the normal host call
contract. Host declaration arguments and associated outputs use `HostValueType`.
Durable host root handles can also be converted to concrete interface values.

## Implemented Feature Set

The current trait system includes:

- trait declarations with methods
- concrete and generic trait impls, specialized at compile time
- generic trait bounds through `where`
- static method lookup through bounds
- trait names usable directly as interface value types
- interface dispatch through runtime vtables
- ordinary associated types, equality bindings and qualified projections
- concrete interface instances from generic implementations, including dependencies

## Remaining Scope

The current trait system excludes:

- script-level `dyn` trait-object syntax
- associated consts
- specialization
- default trait methods
- interface dispatch for non-interface-compatible methods

## Checked and Executable Contracts

### Checked method contracts

Semantic analysis stores trait and method declarations in the shared nominal
catalog. A method contract contains its owning declaration, ordered parameters,
generic parameter identities and bounds, return type, and source declaration.
Function signatures own the checked constraint map, keyed by the parameter's
declaring owner and position. Signature validation, function-body environments,
call checking and the method catalog consume that map. Inline, `where`, and
inherited impl bounds are assembled once during signature analysis, before any
function body runs; method shadowing does not change the receiver's outer binder.
Semantic nominal types pair their declaration identity with ordered type
arguments; the type kind also participates in identity. Substitution retains the
declaration and recursively replaces arguments by their parameter owner/position.
Local impl headers and bounds resolve applied trait types with checked argument
arity. Executable interface values consume these same checked contracts.
An imported trait can be used in a bound or implemented locally. Its method
identities come from the defining module; after shared signatures are available,
the local implementation is checked against that module's trait contract.
Whole-program loading also checks the imported interface table against the
dependency's public ABI before execution.
Trait methods compare local generic binders by position after substituting
trait arguments and `Self`; binder spelling does not affect impl matching.
The same substitution applies to method bounds in the public interface table,
including type arguments nested inside applied trait constraints.
Semantic checking applies it to private trait implementations as well, before
any executable interface table is generated.
An applied generic trait can be an interface type when its methods meet the
interface rules; the trait's own type parameters do not count as method-local
generic parameters. Interface method-local generics remain unsupported.
Local and imported interface annotations use these same contracts for argument
checking, Self substitution, and definition queries. Invalid parameter types retain
Error facts without discarding later parameters or unrelated declarations.

Two distinct bounds offering the same method name make an unqualified call
ambiguous (`KG_TYPE_AMBIGUOUS_METHOD`); bound order never selects a winner.
Repeating the same bound does not create another candidate. Duplicate method
declarations within a trait or impl produce `KG_RESOLVE_DUPLICATE_METHOD`.
Ambiguous calls have no selected method target and cannot enter code generation.

The runtime represents script-backed interface values as generation-checked
GC objects. Construction requires a verified implementation table and resolved
method slots; the object retains its concrete payload and the linked dependency
version until collection. Forged, stale or foreign handles are rejected. The
method bindings follow trait declaration order regardless of implementation
source order, and ordinal lookup checks the exact applied interface identity.
Source calls on interface values use a verified trait method slot and enter the
receiver's pinned implementation version through the explicit frame stack.
The construction entry accepts verified concrete script table instances,
including instances of generic impls and checked host bridge tables.

Verified bytecode can now allocate the same interface object with
`MakeInterface`, using an implementation table slot resolved from the typed IR
declaration identity and a pinned module slot. The source compiler emits this
instruction when a concrete expression is used where an interface is expected
and a unique implementation template is available. HIR records the selected
declaration and inferred arguments; IR lowering specializes that selection rather
than repeating name or implementation lookup. Whole-program verification
resolves imported implementations through the dependency graph.

An embedding path that already has a linked implementation can create and
retain such a value explicitly:

```rust,ignore
let value = vm.runtime().make_interface(&loaded_impl, table_index, concrete_value)?;
let rooted = vm.runtime().root_value(value).expect("valid runtime-owned value");
let result = vm.invoke_interface_method(&rooted.value(), &method_id, &[])?;
```

The root must remain alive while the host retains the value. The table index
belongs to `loaded_impl`; another runtime cannot use that linked module.
The invocation boundary checks concrete parameter and result ABI types,
including nominal script identity, before accepting an embedding call.

Concrete implementations defined in a dependency are visible to bound-call
resolution through the checked implementation catalog. Their methods link by
declaration identity and signature to the defining module. Multiple matching
concrete implementations reject the whole reachable closure during signature
checking, including when unused. Generic templates use the same conservative
overlap rule across modules as within one module; distinct concrete trait
applications stay independent. Reachable methods in generic dependency
implementations are specialized for the receiver's concrete type arguments and
linked to the defining module by instance identity. Further trait extensions are
tracked in the [implementation roadmap](../implementation-roadmap.md).

### Generic Implementation Interface Instances

A conversion from an applied script type can select a generic impl template,
including inside a generic function whose receiver shape identifies that template:

```kagari
trait Reader { type Item; fn read(self) -> Self::Item; }
struct Holder<T> { val value: T }
impl<T> Reader for Holder<T> {
    type Item = T;
    fn read(self) -> Self::Item { self.value }
}
fn boxed<T>(value: Holder<T>) -> Reader<Item = T> { value }
fn main() -> i32 { boxed(Holder { value: 42 }).read() }
```

The caller's bounds must justify the selected impl's constraints. Concrete
instantiation rechecks those constraints; an unconstrained type parameter does
not prove a required bound. Method-local generics remain incompatible with
interface values.

The instance key is the impl declaration plus its ordered concrete arguments.
Repeated conversions reuse one table. Conversion makes all its interface methods
reachable, even if a particular caller only uses one method. Dependency-defined
instances and their method slots are emitted in the impl's owning module. Table
instances share the configurable generic-instantiation budget with functions and
aggregate layouts; there is no runtime method specialization.

Artifact metadata retains the generic template and the concrete arguments of each
linked table. Verification rejects duplicate instances, invalid arguments or
bounds, missing methods and slots pointing at another instance. Generic template
records with no arguments cannot be used as executable table slots. Runtime
construction checks the concrete receiver contract and pins the implementation
version using the existing interface object ownership model.

See [generic-interfaces.kgr](../../examples/syntax/generic-interfaces.kgr), which
returns `42` from both numeric and string interface instances.

### Host Associated Outputs and Interfaces

An offline `HostTraitImplementationDeclaration` contains the trait identity,
ordered concrete inputs, an `associated_types` list of `HostAssociatedTypeBinding`
records, and the method mapping. Each output is keyed by its trait-owned member
identity and has a portable `HostValueType`. Every declared output must appear
exactly once, even when no method is called. Signature analysis and artifact
verification check output bounds and substitute outputs, trait inputs and `Self`
into the full method contract. Host projections normalize at compile time.

A host receiver can satisfy a static generic bound or be converted to a fully
bound interface such as `Reader<Item = i32>`. The compiler emits a concrete
interface table and ordinary IR forwarding functions. Each forwarding function
calls the mapped host method through the normal verified host boundary; there is
no separate runtime dispatcher or runtime generic specialization. Loading checks
the mapping and forwarding code before execution. Host effects, exposure,
capabilities, budget and call-scoped receiver borrowing remain enforced.

The interface payload may retain a registered durable `HostRoot`, whose registry
ownership, concrete type and schema are checked. Borrow tokens and path views
cannot be retained in an interface. Boxing does not transfer ownership of the
underlying host object to the script GC. An embedding must explicitly root the
interface while retaining it. Interface objects and their calls pin the linked
execution version, including across synchronous host reentry and hot reload.
Tuple method parameters and results validate any nested host roots as well.

See [host-interfaces.kgr](../../examples/host-interfaces.kgr) and run
`cargo run -p kagari-embed --example host_interfaces` for offline compilation
followed by runtime binding and static/dynamic calls returning `42`.

### Remaining Execution Work

Runtime downcasting remains separate work. Default method fallback,
associated consts and type-parameterized GAT follow as individual checkpoints.

### Trait Inheritance and Upcasting

`trait Child<T>: Parent<T> + Other` declares required parent contracts. Implementing
`Child` requires a separate implementation of each parent under the implementation's
bounds. It does not generate parent implementations or merge their declarations.
The import graph and the inheritance graph remain separate; cycles in inheritance
are rejected even when no method is called. Traversal is cancellable and bounded
to 64 declaration levels and 4,096 applied nodes.

Child bounds expose transitive parent methods and associated projections.
For example, `trait Child: Reader<Item = i32>` permits `T::Item` and
`<T as Reader>::Item` under `T: Child`, and permits `Self::Item` in child methods.
The same applied parent reached through a diamond is deduplicated by declaration
and arguments. Distinct declarations with the same member name remain ambiguous.
An implementation defines only its own trait's associated outputs and methods.

A dynamic child interface must satisfy the interface compatibility rules for
every parent, including complete associated output bindings. Parent arguments
cannot depend on erased `Self`. A child whose parent has an unbound output can
still be used as a static bound; it cannot be used as an interface value.
Inherited output equality bindings currently belong in the parent clause;
binding an inherited output directly on a child type is not yet supported.

An expected parent type converts a child interface to a parent view. Calls to
inherited parent methods use that same view. Both views retain the same concrete
payload and original execution family. Parent method tables, including generic
instances and host bridges, are compiled and verified before execution. There
is no runtime generic specialization. GC, rooted host retention, host permissions
and old-version calls use the existing interface ownership contracts.

See [trait-inheritance.kgr](../../examples/syntax/trait-inheritance.kgr), which
returns `42` through static calls, inherited projections and dynamic parent views.
