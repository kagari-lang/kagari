# Kagari Module Execution Draft

This document describes a proposed module execution model for Kagari.
It is a design draft, not a finalized implementation contract.

Syntax direction is drafted separately in [syntax.md](/Users/mikai/CLionProjects/kagari/docs/spec/syntax.md).
Execution pipeline direction is drafted separately in [execution.md](/Users/mikai/CLionProjects/kagari/docs/spec/execution.md).
Runtime model direction is drafted separately in [runtime.md](/Users/mikai/CLionProjects/kagari/docs/spec/runtime.md).

## Design Goals

- allow script files to contain top-level executable code
- avoid forcing a Rust-style explicit `main` function for every script
- ensure imported modules are not re-executed on every import
- support embeddable scripting, configuration scripts, and hotfix scripts
- keep module execution semantics compatible with hot reload and caching

## Recommended Model

Each source file should be compiled as a module.

Each module should have:

- declarations such as functions, structs, and enums
- module items such as `const` and `static`
- exports
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

The implicit module initialization function should be allowed to produce a result value.

Recommended rule:

- if top-level code ends in a tail expression, that expression becomes the module initialization result
- if there is no tail expression, the result is `unit`

This is especially useful for single-file script execution.

For example:

```kagari
let x = 1;
let y = 2;
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

The recommended rule is:

- direct script execution may expose the module initialization result as the script result
- `import` should still produce a module instance or module namespace view
- the module initialization result may be stored as part of the module instance
- the initialization result should not replace the export model

This keeps the system consistent:

- single-file scripts can naturally return a value
- imported modules still behave like modules
- exports remain accessible through the module instance

In other words:

- module result is an execution result
- exports are the module interface

These should coexist rather than compete.

## Module Scope Kinds

Kagari should distinguish three different top-level concepts:

1. top-level executable statements
2. private module bindings created during module initialization
3. exportable module items

These should not be conflated.

### Top-Level `let`

Top-level `let` and `let mut` should be treated as part of module initialization code.

They are:

- runtime bindings
- private to the defining module
- not directly exportable
- not the same thing as closure capture

They are intended for module startup logic such as:

```kagari
let config = load_config();
host.log(config);
```

The recommended direction is that these bindings belong to module initialization semantics, not to the exported item model.

### `const`

`const` should represent a compile-time constant item.

Recommended properties:

- compile-time evaluable
- may be exported with `pub`
- no runtime initialization step
- suitable for inlining and constant propagation
- must produce a `const-safe` value

Example:

```kagari
pub const VERSION: i32 = 1;
```

The key rule is not "borrow-checked immutability".
Kagari does not rely on a Rust-style borrow system for this.

Instead, the rule should be:

- a `const` initializer must be evaluable at compile time
- the resulting value must belong to a `const-safe` value type family
- the resulting value must not require heap-backed runtime identity

Recommended v1 `const-safe` types:

- builtin scalar types such as `unit`, `bool`, `i32`, `i64`, `f32`, `f64`, and `str`

Recommended v1 exclusions:

- tuples
- arrays
- structs
- enums
- any future type lowered as a GC handle or other heap-backed runtime object

This keeps `const` aligned with Kagari's ordinary runtime value model.
Kagari currently treats heap-backed values as identity-bearing runtime objects, so allowing them in `const` would require a separate frozen-object model.

In other words, `const` in v1 is a compile-time by-value constant, not a shared read-only object.

### `const` Write Restrictions

The language should define `const` restrictions at the item boundary, not by object-graph freezing.

For a `const` item itself, the following operations should be rejected:

- reassignment
- reflection-based write
- passing the value to APIs that require mutable access

Copies of a `const` value are ordinary runtime values.
If a `const` scalar is copied into another binding or container, later writes affect the destination storage, not the original `const` item.
This keeps `const` semantics simple without introducing provenance tracking or deep-freeze rules.

### `static` and `static mut`

`static` and `static mut` should represent module-level storage items.

Recommended properties:

- stable module-level storage slot
- runtime initialization allowed
- may be exported with `pub`
- `mut` controls whether rebinding is allowed, matching the language's ordinary mutability model

Examples:

```kagari
pub static CONFIG = load_config();
pub static mut COUNTER: i32 = 0;
```

This means Kagari can distinguish cleanly between:

- `const`: compile-time value
- `static`: module storage
- top-level `let`: private module initialization binding

## Why Kagari Should Allow Top-Level Code

Kagari is being shaped as a scripting and embedding language, not as a strict systems language.

Top-level code is useful for:

- configuration scripts
- plugin scripts
- startup glue code
- hotfix scripts
- small utility scripts

Requiring an explicit `main` for every file would make those use cases more awkward without solving the real import problem.

The import problem should be solved by module loading rules, not by forbidding top-level execution.

## Implicit Module Initialization

The recommended rule is:

- top-level statements are legal
- they execute through an implicit module initialization function
- that function runs at most once per loaded module instance

For example:

```kagari
let version = 1;
host.log("loading script");

fn greet() -> str {
    "hello"
}
```

Conceptually becomes:

```text
module exports:
  greet

implicit fn __module_init__():
  version = 1
  host.log("loading script")
```

The exact lowering strategy may vary, but the semantic model should match this behavior.
The important point is that `version` in this example is a private top-level initialization binding, not an exported item.

## Import Execution Rule

The recommended import rule is:

- the first successful import of a module executes its initialization function
- later imports of the same loaded module return the cached module instance
- later imports do not re-run top-level code

This gives the expected behavior for script modules:

- initialization side effects happen once
- initialization result is computed once
- exported bindings remain available
- repeated imports are cheap and predictable

## Module Lifecycle

The runtime should track module state explicitly.

A practical model is:

```text
Uninitialized
Initializing
Initialized
Failed
```

Recommended behavior:

1. module is loaded in `Uninitialized`
2. first import moves it to `Initializing`
3. initialization function executes
4. success moves it to `Initialized`
5. failure moves it to `Failed`

If a module is already `Initialized`, imports return the cached instance without re-running initialization.

## Circular Imports

Circular imports should be handled through module state, not by banning module execution.

If module `A` imports `B` while `B` imports `A`:

- the second access sees that `A` is already `Initializing`
- the runtime returns the in-progress module instance
- reads of bindings that are not yet initialized may trap or observe an explicitly uninitialized state

The exact behavior for partially initialized bindings can be finalized later, but the runtime model should be prepared for it.

## Relationship to `main`

Kagari should not require a language-level `main` function in every file.

Instead:

- a file may act as a module with top-level initialization
- a host or CLI may optionally choose to call an exported `main`

This means:

- `main` is a host or application convention
- not a mandatory language construct

For example, a CLI could define:

1. load entry module
2. run its implicit initialization function
3. if exported `main` exists, call it

This keeps the language flexible while still supporting executable entrypoints.

## Top-Level Restrictions

Allowing top-level code does not mean every statement form should be accepted at module scope.

Recommended restrictions:

- allow `let`
- allow `let mut`
- allow expression statements
- allow top-level initialization expressions
- disallow `return`
- disallow `break`
- disallow `continue`

Whether top-level `if` or `match` is allowed can be decided by normal statement rules.
They do not need special treatment if they simply lower into module initialization code.

## Export Model

The recommended export rule is:

- `pub fn` exports a function item
- `pub const` exports a compile-time constant item
- `pub static` and `pub static mut` export module storage items
- top-level `let` and `let mut` are not exportable

This avoids introducing forms such as `pub let x = 1;` and keeps exportability tied to item declarations rather than statement syntax.

## Recommended Runtime Contract

The runtime should expose module loading in terms of module instances rather than raw source files.

A loaded module instance should conceptually include:

- module identity
- epoch or reload generation
- exported bindings
- initialization state
- optional module initialization result
- bytecode for the implicit module init function
- bytecode for declared functions

This fits naturally with Kagari's bytecode-first execution model.

## Hot Reload Interaction

Hot reload should create a new module instance or a new module epoch.

Recommended rule:

- imports are cached per module instance or per epoch
- reloading a module invalidates the prior initialized instance
- the new instance runs its initialization function again

This keeps module execution predictable across reloads.

## Recommended Direction

The recommended Kagari module model is:

- allow top-level executable code
- compile that code into an implicit module initialization function
- allow that implicit initialization function to return a module result
- treat top-level `let` as private module initialization binding, not as an exportable item
- use `const`, `static`, and `static mut` for exportable module-level data
- cache initialized module instances
- do not re-execute a module on repeated import
- treat `main` as an optional host-side convention

This gives Kagari a scripting-friendly module system without sacrificing predictable execution behavior.
