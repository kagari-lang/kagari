# Kagari Trait System Draft

This document defines the intended direction for the Kagari trait system.
It is a design draft, not a finalized implementation contract.

The main goal is to preserve useful abstraction mechanisms from Rust-like languages without importing Rust's lifetime-driven complexity.

Reflection direction is drafted separately in [reflection.md](/Users/mikai/CLionProjects/kagari/docs/spec/reflection.md).
Security direction is drafted separately in [security.md](/Users/mikai/CLionProjects/kagari/docs/spec/security.md).
Host interop direction is drafted separately in [host-interop.md](/Users/mikai/CLionProjects/kagari/docs/spec/host-interop.md).
Runtime model direction is drafted separately in [runtime.md](/Users/mikai/CLionProjects/kagari/docs/spec/runtime.md).

## Design Goals

- support static polymorphism through generic trait bounds
- support dynamic polymorphism through `dyn Trait`
- support runtime downcast through concrete type identity
- avoid lifetime parameters and borrow-driven object-safety complexity
- keep the implementation model compatible with a GC-backed runtime

## Non-Goals

The first trait version should not attempt to reproduce all of Rust's trait features.

In particular, this draft does not aim to support:

- lifetime-parameterized traits
- generalized associated types
- specialization
- negative impls
- auto traits
- full Rust-style coherence and orphan behavior
- trait-object support for every possible method shape

## Core Model

Kagari should treat traits as two related but distinct mechanisms:

1. static trait constraints
2. dynamic interface objects

This split is the key simplification.

Static trait constraints are used by generic functions and generic types during type checking.
Dynamic interface objects are used for runtime dispatch and downcast.

These two layers should share trait declarations, but they should not be forced into the same implementation model.

## Static Traits

Static trait use is the default and should be the first implementation target.

Example:

```kagari
trait Display {
    fn to_string(self) -> String;
}

fn show<T>(value: T) -> String
where T: Display
{
    value.to_string()
}
```

The important properties are:

- trait bounds participate in name resolution and type checking
- method lookup may be resolved statically for generic code
- no runtime trait object is needed in the common generic case
- no downcast is involved

## Trait Declarations

A reasonable initial surface syntax is:

```kagari
trait TraitName<T1, T2> {
    fn method(self, x: T1) -> T2;
}
```

Possible future grammar shape:

```ebnf
trait_item       ::= visibility? trait_decl ;

trait_decl       ::= "trait" IDENT generic_param_clause? "{" trait_member* "}" ;

trait_member     ::= method_sig ";" ;

method_sig       ::= "fn" IDENT generic_param_clause? "(" method_param_list? ")" return_type? where_clause? ;
```

The initial version should keep trait members small:

- methods only
- no associated consts
- no associated types in v1 unless there is a concrete need

## Trait Implementation

Trait implementation should be distinct from inherent `impl`.

Proposed surface syntax:

```kagari
impl Display for Int {
    fn to_string(self) -> String {
        ...
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

Proposed grammar shape:

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

Trait bounds should initially support:

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

The first version should limit bounds to simple trait references:

```kagari
where T: Display + Clone
```

Avoid more advanced forms at first:

- higher-rank bounds
- equality constraints
- projection-heavy associated type constraints

## Dynamic Trait Objects

`dyn Trait` should be treated as a runtime interface object, not as a borrow-dependent type form.

Example:

```kagari
let d: dyn Display = button;
d.to_string();
```

A `dyn Trait` value should conceptually carry:

- a handle to the underlying object
- the concrete runtime type id
- a dispatch table for the trait's methods

This is intentionally closer to a GC-friendly interface object than to Rust's exact trait-object model.

## Runtime Representation

A useful conceptual representation is:

```text
DynObject {
  data_handle: ValueHandle,
  concrete_type_id: TypeId,
  vtable: TraitVTableId
}
```

The actual runtime layout may differ, but the semantic model should preserve these three capabilities:

- dynamic dispatch
- runtime type identity
- safe downcast checks

## Downcast

Downcast should be defined in terms of concrete runtime type identity, not generic trait reasoning.

Example:

```kagari
if let p = x.downcast<Player>() {
    ...
}

if x.is<Player>() {
    ...
}
```

The recommended model is:

- every runtime heap object has a concrete type id
- `dyn Trait` preserves that concrete type id
- `downcast<T>` succeeds when the stored concrete type id matches `T`
- `is<T>` is a non-consuming boolean check over the same rule

This is much simpler than attempting to infer downcast through trait structure.

## Object Safety

The first version of `dyn Trait` should impose a deliberately small object-safety rule set.

Initially allow only methods that:

- do not return `Self`
- do not mention unconstrained generic method parameters
- do not require monomorphization at the call site

The first version should probably reject `dyn Trait` for traits whose methods require static knowledge of the concrete type.

This means some traits can exist purely for static generic use and still not be dyn-compatible.

## Receiver Model

Kagari already has a non-Rust reference model, so receiver forms should stay aligned with the rest of the language.

Recommended receiver forms:

- `self`
- `mut self`
- `ref self`

Suggested interpretation:

- `self` receives the ordinary value
- `mut self` allows the local receiver binding to be rebound
- `ref self` aliases the caller-visible receiver slot, following Kagari's `ref` rules

This is intentionally not Rust borrowing.

## Relationship Between Traits and Downcast

Traits should not be the mechanism that determines whether downcast is possible.

Instead:

- traits describe callable capability sets
- dynamic objects carry runtime concrete type identity
- downcast works because concrete type identity is preserved

This avoids conflating:

- compile-time capability reasoning
- runtime type tests

## Recommended v1 Feature Set

The first usable trait version should include:

- trait declarations with methods
- trait impls for concrete types
- generic trait bounds through `where`
- static method lookup through bounds
- `dyn Trait`
- `is<T>`
- `downcast<T>`

## Recommended v1 Exclusions

The first usable trait version should exclude:

- associated types unless they become clearly necessary
- trait inheritance with complex conflict rules
- trait upcasting
- specialization
- default trait methods if implementation bandwidth is limited
- trait objects for non-object-safe traits

## Type-Checking Guidance

Type checking can stay manageable if trait resolution is kept intentionally simple.

Early trait resolution rules should prefer:

- explicit impl lookup by concrete type
- explicit bound lookup by generic parameter
- no overlapping impls in v1
- clear ambiguity errors rather than aggressive inference

If there are multiple plausible impl candidates, the compiler should reject the program instead of attempting overly clever selection.

## Coherence Guidance

Kagari does not need Rust's full coherence model in the first version.

A practical first rule is:

- within one compilation world, there must be at most one visible impl of a given trait for a given concrete type

This rule is simple enough to understand and simple enough to enforce.

If host integration later requires looser behavior, that can be designed separately.

## Host Interoperability

Traits and host object integration should remain separate concerns.

A host object may:

- implement script-visible traits
- be wrapped as a `dyn Trait`
- participate in `is<T>` and `downcast<T>` if the runtime assigns it a stable concrete type identity

But host borrowing rules should not leak into the script trait model.

## Recommended Implementation Order

If implemented incrementally, the recommended order is:

1. trait declarations
2. trait impls for concrete script types
3. generic bounds and static resolution
4. inherent impl and trait impl disambiguation
5. `dyn Trait`
6. `is<T>` and `downcast<T>`

This order keeps the system useful early without forcing runtime object machinery before static trait checking is stable.
