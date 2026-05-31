# Kagari Module Execution Specification

This document defines the module execution model for Kagari.

Syntax is defined in [syntax.md](syntax.md).
Execution pipeline behavior is defined in [execution.md](execution.md).
Runtime behavior is defined in [runtime.md](runtime.md).

## Design Goals

- allow script files to contain top-level executable code
- avoid forcing a Rust-style explicit `main` function for every script
- ensure imported modules are not re-executed on every import
- support embeddable scripting, configuration scripts, and hotfix scripts
- keep module execution semantics compatible with hot reload and caching

## Module Model

Each source file is compiled as a module.

Each module has:

- declarations such as functions, structs, and enums
- module items such as `const`
- public module interface
- an implicit module initialization function
- an optional module initialization result

Conceptually:

```text
source file
  -> module metadata
  -> declarations
  -> implicit fn __module_init__()
```

Top-level executable statements are lowered into that implicit initialization function.

## Module Initialization Result

The implicit module initialization function may produce a result value.

Rule:

- if top-level code ends in a tail expression, that expression becomes the module initialization result
- if there is no tail expression, the result is `()`
- source code does not need to spell a trailing `()` expression

This is especially useful for single-file script execution.

For example:

```kagari
val x = 1;
val y = 2;
x + y
```

Conceptually:

```text
implicit fn __module_init__() -> i32:
  x = 1
  y = 2
  return x + y
```

## Relationship Between Imports and Module Results

Rule:

- direct script execution may expose the module initialization result as the script result
- `import` produces a module instance or module namespace view
- the module initialization result may be stored as part of the module instance
- the initialization result does not replace the module's public interface

This keeps the system consistent:

- single-file scripts can naturally return a value
- imported modules still behave like modules
- public items remain accessible through the module instance

In other words:

- module result is an execution result
- public items are the module interface

These coexist rather than compete.

## Module Scope Kinds

Kagari distinguishes three different top-level concepts:

1. top-level executable statements
2. private module bindings created during module initialization
3. public module items

These are separate semantic categories.

### Top-Level `val` and `var`

Top-level `val` and `var` are part of module initialization code.

They are:

- runtime bindings
- private to the defining module
- not part of the module's public interface
- not the same thing as closure capture

They are intended for module startup logic such as:

```kagari
val config = load_config();
host.log(config);
```

These bindings belong to module initialization semantics, not to the module public-interface model.

### `const`

`const` represents a compile-time constant item.

Properties:

- compile-time evaluable
- may be made public with `pub`
- no runtime initialization step
- suitable for inlining and constant propagation
- must produce a `const-safe` value

Example:

```kagari
pub const VERSION: i32 = 1;
```

The key rule is not "borrow-checked immutability".
Kagari does not rely on a Rust-style borrow system for this.

Instead, the rule is:

- a `const` initializer must be evaluable at compile time
- the resulting value must belong to a `const-safe` value type family
- the resulting value must not require heap-backed runtime identity

V1 `const-safe` types:

- builtin scalar types such as `()`, `bool`, `i32`, `i64`, `f32`, `f64`, and `String`

V1 exclusions:

- tuples
- arrays
- structs
- enums
- any future type lowered as a GC handle or other heap-backed runtime object

This keeps `const` aligned with Kagari's ordinary runtime value model.
Kagari currently treats heap-backed values as identity-bearing runtime objects, so allowing them in `const` would require a separate frozen-object model.

In other words, `const` in v1 is a compile-time by-value constant, not a shared read-only object.

### `const` Write Restrictions

The language defines `const` restrictions at the item boundary, not by object-graph freezing.

For a `const` item itself, the following operations are rejected:

- reassignment
- reflection-based write
- passing the value to APIs that require mutable access

Copies of a `const` value are ordinary runtime values.
If a `const` scalar is copied into another binding or container, later writes affect the destination storage, not the original `const` item.
This keeps `const` semantics simple without introducing provenance tracking or deep-freeze rules.

### Module Storage

The v1 module model does not include a script-visible `static` item.

Mutable module-level storage has non-trivial hot reload semantics around epochs, old closures, module namespace values, persistence, and migration.
Until those rules are specified, v1 keeps module-level mutable storage out of the surface language.

Scripts that need durable or cross-reload mutable state use:

- host-owned state exposed through typed handles or typed path mutation
- persisted script-owned state with explicit migration rules
- future versioned module state, if a later spec adds it

Top-level `val` and `var` remain private module initialization logic, not durable module storage.

## Why Kagari Should Allow Top-Level Code

Kagari is being shaped as a scripting and embedding language, not as a strict systems language.

Top-level code is useful for:

- configuration scripts
- plugin scripts
- startup glue code
- hotfix scripts
- small utility scripts

Requiring an explicit `main` for every file would make those use cases more awkward without solving the real import problem.

The import problem is solved by module loading rules, not by forbidding top-level execution.

## Implicit Module Initialization

Rule:

- top-level statements are legal
- they execute through an implicit module initialization function
- that function runs at most once per loaded module instance

For example:

```kagari
val version = 1;
host.log("loading script");

fn greet() -> String {
    "hello"
}
```

Conceptually becomes:

```text
module public interface:
  greet

implicit fn __module_init__():
  version = 1
  host.log("loading script")
```

The exact lowering strategy may vary, but the semantic model matches this behavior.
The important point is that `version` in this example is a private top-level initialization binding, not a public item.

## Import Execution Rule

Import rule:

- the first successful import of a module executes its initialization function
- later imports of the same loaded module return the cached module instance
- later imports do not re-run top-level code

This gives the expected behavior for script modules:

- initialization side effects happen once
- initialization result is computed once
- public bindings remain available
- repeated imports are cheap and predictable

## Module Lifecycle

The runtime tracks module state explicitly.

A practical model is:

```text
Uninitialized
Initializing
Initialized
Failed
```

Lifecycle behavior:

1. module is loaded in `Uninitialized`
2. first import moves it to `Initializing`
3. initialization function executes
4. success moves it to `Initialized`
5. failure moves it to `Failed`

If a module is already `Initialized`, imports return the cached instance without re-running initialization.

## Circular Imports

Circular imports are handled through module state, not by banning module execution.

If module `A` imports `B` while `B` imports `A`:

- the second access sees that `A` is already `Initializing`
- the runtime returns the in-progress module instance
- reads of bindings that are not yet initialized trap or observe an explicitly uninitialized state according to the runtime's partial-initialization policy

The runtime model reserves an explicit partial-initialization state for this case.

## Relationship to `main`

Kagari does not require a language-level `main` function in every file.

Instead:

- a file may act as a module with top-level initialization
- a host or CLI may optionally choose to call a public `main`

This means:

- `main` is a host or application convention
- not a mandatory language construct

For example, a CLI could define:

1. load entry module
2. run its implicit initialization function
3. if public `main` exists, call it

This keeps the language flexible while still supporting executable entrypoints.

## Top-Level Restrictions

Allowing top-level code does not mean every statement form is accepted at module scope.

Restrictions:

- allow `val`
- allow `var`
- allow expression statements
- allow top-level initialization expressions
- disallow `return`
- disallow `break`
- disallow `continue`

Top-level `if` and `match` follow the normal statement rules.
They require no special treatment when they lower into module initialization code.

## Public Module Interface

Public-interface rule:

- `pub fn` makes a function item public
- `pub const` makes a compile-time constant item public
- top-level `val` and `var` cannot be public module items

This avoids introducing forms such as `pub val x = 1;` and keeps module visibility tied to item declarations rather than statement syntax.

## Runtime Contract

The runtime exposes module loading in terms of module instances rather than raw source files.

A loaded module instance conceptually includes:

- module identity
- epoch or reload generation
- public bindings
- initialization state
- optional module initialization result
- bytecode for the implicit module init function
- bytecode for declared functions

This fits naturally with Kagari's bytecode-first execution model.

## Hot Reload Interaction

Hot reload creates a new module instance or a new module epoch.

Rule:

- imports are cached per module instance or per epoch
- reloading a module invalidates the prior initialized instance
- the new instance runs its initialization function again

This keeps module execution predictable across reloads.

## Summary

The Kagari module model is:

- allow top-level executable code
- compile that code into an implicit module initialization function
- allow that implicit initialization function to return a module result
- treat top-level `val` and `var` as private module initialization bindings, not as public items
- use `const` for compile-time constants
- defer script-visible mutable module storage to a later design
- cache initialized module instances
- do not re-execute a module on repeated import
- treat `main` as an optional host-side convention

This gives Kagari a scripting-friendly module system without sacrificing predictable execution behavior.
