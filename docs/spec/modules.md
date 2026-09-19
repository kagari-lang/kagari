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

### Source import analysis

An analysis snapshot resolves imports against its registered source files, the
standard library and immutable host declarations. Hosts bind a source name to a
package/module identity before analysis. For example, a file bound to package
`app`, path `library` is imported as `use app::library;`, or a public function as
`use app::library::value;`. Aliases use `as`. Disk, supplied text and editor overlay
content all enter through the source database; the active overlay wins for its
source name. Analysis does not discover or read unregistered disk dependencies.

Source imports retain module identity, file identity and document revision.
Only public items enter an imported namespace. Ambiguity between a module and an
item, or between source, standard and host namespaces, is an error. Resolution
does not choose a fallback namespace. Definition queries can follow source facade
re-exports even when a function body contains errors.

The snapshot exposes its import graph and a cancellable dependency-first order.
Cycle diagnostics identify imports within the cyclic component; an importer of
that component is rejected without itself being labelled cyclic. Unrelated cycles
do not prevent compilation of an independent root. Lifecycle semantics belong to
[module-activation.md](module-activation.md).

Declarations are collected for every module before signatures are checked.
Local functions, constants, module declarations, structs, enums and traits share
one module-level namespace. Repeated names, including collisions between kinds, produce
`KG_RESOLVE_DUPLICATE_DECLARATION` on each subsequent declaration in source order.
All declarations retain distinct identities for tooling, but an ambiguous name has
no selected target. Calls, annotations, constructors, trait bounds and impl headers
consume the same declaration table; none chooses the first or last declaration.
Unrelated declarations and function bodies remain queryable, while the module
cannot enter code generation. Introducing or removing a collision invalidates
dependent semantic queries without changing an existing snapshot.

Import aliases enter that same declaration table. An invalid import blocks implicit
host or standard-library fallback; a duplicate alias has no target in any of its
import entries, including the first. Qualified expression paths resolve their root
through lexical bindings before namespace lookup. A parameter or local named `api`
therefore shadows `use std::math as api;` for `api::clamp(...)`, and normal lookup
resumes outside its scope. Standard-library call targets are resolver facts shared
by type checking and navigation, without a second textual lookup in the checker.

The runtime-helper prelude (`print`, `type_of`, `get_field`, `set_field`,
`set_index`) is resolved after explicit declarations, imports, lexical bindings
and host declarations. Each helper has an explicit resolver target consumed by
the checker; spelling is interpreted only during name resolution. Argument errors
retain that target, and declaration changes invalidate cached calls. These helpers
and other named function items are not first-class values in the current checked
subset. Bare function, module or type names produce `KG_TYPE_INVALID_VALUE_TARGET`
in HIR while retaining their resolution for tooling; unknown names still produce
`KG_RESOLVE_UNKNOWN_NAME`. Enum unit-variant values retain their constructor rules.

Public struct, enum and trait types can be used in parameter, return, field and
local annotations via direct imports and qualified namespace aliases. Type
facades and aliases of re-exported source modules retain the final declaration
identity. Generic parameters shadow imported type names within their binder.
Type navigation reports the defining file and revision, including inside array
and tuple annotations. Import or visibility changes invalidate those targets and
dependent signatures; an old snapshot retains its old declarations.

Function signatures are checked for every module before any function body is
checked. Imported calls use these signatures, including through public source
facades; they do not reinterpret dependency syntax. Parameter/return types retain
nominal declaration identity, including when two modules declare the same name.
Signature queries retain errors independently of body and constant diagnostics.
An invalid dependency body does not erase its usable function signatures.

When only user or impl function bodies change, signature queries reuse the checked
result if the declaration surface, module identity, imported type bindings and host
declarations still match. Reuse remaps source-local type references and diagnostic
ranges into the new lowering, including for erroneous signatures; it does not keep
old local binding identities alive. Changes to dependencies, signatures or field
contracts invalidate the affected queries. Cancelled or older analyses cannot
replace a newer cached revision. Query reuse remains conservative: declarations
and signatures have independent cached snapshot queries. A function body can be
queried by its nominal declaration identity, resolving/checking only that function
and its module constants. Full analysis uses the same resolver and checker in batch
form. Both paths reuse unchanged function facts where possible; source-local IDs
are remapped, and same-named impl methods match by their own function identity.

Declaration collection owns module names, imports, named definitions, fields and
generic parameters. It does not resolve body expressions or create local bindings.
Signature queries consume those declarations and resolve imported types before
checking parameter, return, field and impl contracts. Neither query evaluates
constants. Full analysis consumes these same results, then resolves body scopes
and adds analysis-scoped parameter/local identities. Declaration-only snapshots
never acquire those local identities when a full analysis subsequently runs.

Parse and declaration diagnostics are available from declaration queries;
signature queries additionally report signature errors. Body name/type errors and
constant-evaluation failures belong to full analysis. Queries may return usable
facts with errors. Only complete checked analysis may cross into code generation.

Struct construction and field access use a checked aggregate catalog for the root's
reachable modules. Both local and imported targets carry nominal declaration IDs;
field contracts include their declaring struct, declaration-order slot, type,
writeability and source location. Nested imported fields obey the same `val`/`var`
and initializer checks as local fields. A field change invalidates dependent bodies
even when exported function signatures still name the same nominal type. Incomplete
member access retains the receiver type for queries. IR/bytecode layouts and runtime
field slots use these identities and permissions; executable bundle ownership and
cross-module method/enum operations still require further linking work.

`AnalysisSnapshot::check_program(root, cancel)` checks the entire reachable source
closure and returns an immutable `CheckedProgram`. Its members are ordered before
their dependents, with each diamond dependency included once. Errors in any reachable
body or constant reject compilation even when the imported item is unused; tooling
can still query the partial snapshot. Diagnostics retain the owning file/revision.
Function targets from another document revision do not resolve in the checked
program. Unrelated broken modules do not prevent compiling a valid closure.

`lower_program_to_ir` preserves module boundaries, initializers and dependency edges.
Imported calls carry nominal declaration contracts; whole-program IR verification
binds each contract to a module/function slot and checks its signature. Public
facades resolve to the final defining module. Per-module HIR IDs and same-spelled
functions cannot substitute for these link identities. Generic-instance and generated
instruction budgets are shared across the whole closure, with cancellation checks.

Current implementation boundary: source imports support graph, definition and
function signature queries, imported type annotations and call checking. Applied
user types, namespace-facade calls, foreign trait constraints/implementations,
cross-module method/enum operations remain pending. Valid source closures compile
to BytecodeProgram, including dependency initializers and module/function call slots.
Initialization follows the verified dependency-first order, visits shared dependencies
once, and caches failure per runtime and execution version. No completed side effect
is rolled back. Loaded members share one immutable program version; cross-module calls
resolve within it even after a newer root version is published. Active retention on any
member keeps all instances in that version. Candidate isolation and initialization
before publication remain outstanding; the current reload API still publishes before
ordinary lazy initialization and does not satisfy the full activation contract.
The current reload validator requires the same logical member set and checks public
ABI and typed-path fingerprints for each member. It rejects stale root handles and
changed member contracts before publication; automatic state migration is absent.

### Module contents

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

The current scalar evaluator handles the implemented literal types (`bool`,
`i32`, `f32`, and `String`). Analysis owns evaluated const facts; code generation
does not evaluate initializers again. Arithmetic follows [value semantics](value-semantics.md):
overflow or integer division by zero produces a source diagnostic and prevents
code generation. Short-circuit branches are evaluated only when selected, but
all branches must still satisfy const-safe syntax and type rules. Invalid
constants do not discard unrelated function/type facts.

V1 exclusions:

- tuples
- arrays
- structs
- enums
- any future type lowered as a GC handle or other heap-backed runtime object

This keeps `const` aligned with Kagari's ordinary runtime value model.
Physical heap allocation does not determine value semantics. Aggregate consts
remain outside this scalar evaluator even when the aggregate has value semantics.

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

V1 rejects cyclic imports before initialization. No partially initialized module
is exposed. [Module activation](module-activation.md) defines dependency ordering,
failure caching, explicit retries, and candidate isolation.

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
- reload initializes isolated candidates before publishing, as defined in
  [module activation](module-activation.md)
- old initialized instances remain valid while reachable
- failed initialization is cached; ordinary imports never implicitly retry it

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
