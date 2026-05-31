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
The initial trait scope excludes:

- script-level `dyn Trait` syntax
- Rust-style trait object syntax such as `&dyn Trait` or `Box<dyn Trait>`
- lifetime-parameterized traits
- generalized associated types
- associated types
- associated consts
- specialization
- negative impls
- auto traits
- full Rust-style coherence and orphan behavior
- higher-rank bounds
- projection-heavy associated type constraints

## Core Model

Kagari traits serve two script-facing purposes:

1. static trait constraints for generic code
2. ordinary interface value types for dynamic dispatch

There is no script-visible split between `Trait` and `dyn Trait`.
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
- safe `is<T>` checks
- safe `downcast<T>` checks

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

trait_decl       ::= "trait" IDENT generic_param_clause? "{" trait_member* "}" ;

trait_member     ::= method_sig ";" ;

method_sig       ::= "fn" IDENT generic_param_clause? "(" method_param_list? ")" return_type? where_clause? ;
```

Trait members are limited to:

- methods only
- no associated consts
- no associated types

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

The initial trait-bound scope excludes:

- higher-rank bounds
- equality constraints
- associated type projections
- implicit type-level computation

## Interface Compatibility Rules

Not every trait method shape is suitable for interface dispatch.
The language describes this as interface compatibility rather than Rust object safety.

Interface-callable methods must:

- not return `Self`
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

## Downcast

Downcast is defined in terms of concrete runtime type identity, not generic trait reasoning.

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

## Initial Feature Set

The initial trait system includes:

- trait declarations with methods
- trait impls for concrete types
- generic trait bounds through `where`
- static method lookup through bounds
- trait names usable directly as interface value types
- interface dispatch through runtime vtables
- `is<T>`
- `downcast<T>`

## Initial Scope Exclusions

The initial trait system excludes:

- script-level `dyn Trait`
- associated types
- associated consts
- trait inheritance with complex conflict rules
- trait upcasting
- specialization
- default trait methods
- interface dispatch for non-interface-compatible methods

## Implementation Phases

The implementation can be staged in this order:

1. trait declarations
2. trait impls for concrete script types
3. generic bounds and static resolution
4. inherent impl and trait impl disambiguation
5. runtime interface value representation
6. interface dispatch through vtables
7. `is<T>` and `downcast<T>`

This order keeps static trait checking independent from runtime interface machinery until interface value representation is implemented.
