# Modules and imports

A source file defines a module; [packages and workspaces](packages.md) organize their source acquisition and dependency closures. `mod child { ... }` defines a child module in the same physical file; `mod child;` refers to a separately registered child source. The two forms cannot define the same identity. A module contains declarations (`use`, `mod`, `fn`, `const`, `static`, `struct`, `enum`, `type`, `trait`, and `impl`) only. Statements and expressions execute in functions or lazily invoked static initializers. There is no implicit module entry or module result.

The source database binds each registered source to a package and module identity. Disk, supplied source, and editor overlays use the same identity and analysis path; an active overlay replaces its base source. Package acquisition discovers declared source files before analysis; analysis does not discover unregistered files. A package root has an empty module path. A standalone source has its own package identity derived from its source name. Source locations retain the physical file and revision, including for inline modules.

Imports bind names and create dependency edges for linking. `use path::item`, aliases, grouped imports, and globs resolve against source, standard-library, and declared host namespaces. `crate` starts at the unnamed package root, `self` at the current module, and `super` at its parent; traversing above the root fails. Source dependency aliases belong to the importing package and name only its direct dependencies. Merely registering another source package or sharing workspace membership grants no import access. Installed native aliases remain explicit compiler inputs and cannot be shadowed by source dependency aliases. A public re-export forwards the resolved target. Local declarations and explicit imports take precedence over globs; conflicting glob names are diagnosed. Import resolution never runs code or initializes an instance.

Names occupy independent **Type** and **Value** spaces. Types, transparent type
aliases, traits, type parameters, `Self`, modules and package aliases belong to Type.
Functions, constants, statics, locals, parameters and existing enum constructors
belong to Value. Type annotations,
trait bounds, struct literals and struct patterns select Type; expressions, calls
and enum patterns select Value. A struct does not introduce a positional constructor.
Every nonterminal component of `a::b` selects Type and must denote a namespace;
a Value named `a` does not hide a module named `a`.

A named `use` leaf imports both available categories under its alias. Missing one
category is valid when the other resolves; missing both is an unresolved import.
An inaccessible or ambiguous category is an error even when the other succeeds.
Precedence (declarations/explicit imports, globs, implicit prelude/package names)
and conflicts apply independently in each category. Duplicate strong bindings in
one category are errors even when their targets match. Equal-target glob bindings
coalesce while retaining their provenance. A value named `Option` does not shadow
the prelude type `Option`. Public members derive from the resolved binding table,
including valid re-exports and globs.

```kagari
mod api {
    pub struct Token { pub val value: i32 }
    pub fn Token() -> i32 { 40 }
}
use self::api::Token as Item;
fn main() -> i32 {
    val instance: Item = Item { value: 2 };
    Item() + instance.value
}
```

The checked closure contains every registered module in the selected package and every declared dependency package, including unused modules, uncalled function bodies, static initializer bodies and dependencies used only for trait implementations. Each dependency contributes its own transitive closure. Unrelated packages remain outside that closure. Its order is stable but carries no initialization meaning. Cyclic module references are legal; recursive calls remain subject to runtime call-depth limits and cancellation. The checked program resolves calls and types using declaration identities, then links to slots within one program version. Errors in any reachable module prevent code generation, while analysis remains queryable for tools.

`const` is an immutable compile-time value evaluated within the restricted const-safe language. Statics are runtime storage with independent lazy initialization, not constants or a cached module result. Loading verifies and links the complete program before publishing a handle. A call names a function entry explicitly; the CLI invokes `main`. A missing or ambiguous entry fails before executing script instructions.

Hot reload prepares and validates a candidate program, stages generation bindings to compatible existing static cells and private new cells, rechecks the expected base version and bindings, then publishes it. The ordinary reload API does not execute candidate functions. Hosts that explicitly evaluate a candidate before publication must use the restricted candidate session; its effects are limited as described in [module activation](module-activation.md). A failed candidate leaves the active entry intact. Created computations and admitted frames/queues retain exact bodies, while ordinary inner calls select current implementations. Publication requires no active synchronous script/callback/reentry scope; parked work may remain.

Public source functions may be generic templates. Source calls specialize them
under ordinary type/bound checking. Public-path declaration discovery also records
generic declarations without executable applications, including for source updates.
Typed host binding separately requires existing executable evidence and never
specializes missing applications at runtime; see the
[relative host-path contract](embedding-api.md#installed-programs-and-public-paths).
Public re-export aliases retain canonical declarations. Checked
Programs keep a finite public namespace graph so cyclic module aliases do not require
infinite path expansion. Private, package-restricted and implicit prelude names are
absent from this graph. The root's zero-argument, non-generic `main` is recorded
separately for CLI execution and does not become a public name merely by convention.

## Transparent alias names

A [transparent type alias](syntax.md#transparent-type-aliases) is a source declaration
in the Type namespace. It participates in forward lookup, named/grouped/glob
imports and public re-exports under the ordinary namespace rules. A `use ... as`
rename forwards that declaration; it is not itself a type-argument application.
Navigation from an alias use, including an imported or re-exported name, identifies
the alias declaration. Its target path and generic parameters retain their own
source navigation. The alias's source identity is not a distinct runtime type.

An alias does not create a synthetic import namespace for the target's members.
`use Alias::Variant;` is unsupported, even for a closed enum alias. Use the
canonical enum's existing variant import or a supported alias-qualified
constructor/pattern path; expression member lookup does not create import entries.

For example:

```text
mod models {
    pub struct Cell<T> { pub val value: T }
    pub type IntCell = Cell<i32>;
}
mod api { pub use super::models::IntCell as Counter; }
use self::api::Counter;
struct Cell { val unrelated: bool }
fn make() -> Counter { Counter { value: 42 } }
```

`Counter` normalizes to `models::Cell<i32>`; the importing module's unrelated
`Cell` cannot change its meaning. Navigation from `Counter` reaches `IntCell`.
Checking includes unused alias declarations in the selected module closure, so an
invalid target or alias cycle is an error even when no function uses it. Legal
cycles in the module/import graph do not permit recursive type-alias expansion.

Alias visibility cannot expose a less visible target, including a private type
nested in a generic application or another target type. For example,
`struct Hidden {} pub type Public = Hidden;` is rejected, as is
`pub type Public = [Hidden];`. Re-exporting an alias does not widen the alias's
visibility or bypass target/member privacy. A public target declared inside a
private module may still be exposed through a valid public alias, under the same
item-versus-containing-path distinction as ordinary re-exports.

Normalized source uses retain the target's existing lowering and runtime contracts.
[Public type exports](artifacts.md#public-type-exports) owns their portable host
records and compiler-driven layout roots; [object binding](embedding-api.md#public-object-types-and-binding-errors)
owns the finite supported host applications. Source alias support is not unrestricted
host reflection or runtime specialization.

## Persistent module statics

```kagari
static val INITIAL_COUNT: i32 = 1;
pub static var COUNT: i32 = INITIAL_COUNT;
```

A static declares one name in the Value namespace. It requires an explicit closed
semantic type and an initializer expression; it is allowed only at module scope.
There are no local, associated, generic or thread-local statics, destructuring
static declarations, or default initialization. Ordinary visibility and import
rules apply. Imports and re-exports alias the original declaration, not a copy of
its storage.

`static val` forbids rebinding the cell; it does not freeze a referenced object or
prevent mutation through another writable alias. `static var` permits checked
replacement after initialization. Values retain ordinary identity, aliasing and
durable-storage rules; storing them does not deep-copy or reset them.

Each installation owns a cell for the logical module/declaration identity.
Repeated imports in that installation share it. Independently loaded roots and
separate runtimes do not, even when they share immutable prepared code. Compatible
generations bind to the same cell. Created closures/Futures and admitted computations
keep their selected bodies, while ordinary calls use current implementations. See [activation and reload](module-activation.md)
for fixed initializer recipes and publication, [assignment ordering](value-semantics.md#evaluation-and-assignment)
for access effects, and [runtime ownership](runtime.md#persistent-static-cells)
for retention. Persistent means continuity within an installation, not persistence
to disk or automatic state snapshot/restore.

## Visibility

Visibility is checked during import, qualified lookup, field access, struct construction, and inherent method calls. `pub` permits access from other module trees. An unmarked item or field is private to its owning module and its descendants. `pub(super)` permits access from the parent module and its descendants. `pub(crate)` permits access from every module with the same package identity and rejects access from other packages. A private inherent method is available within its defining module tree. Every module and item component of a direct path must be accessible. Re-exports may expose a public item through a private module, but cannot widen the visibility of the item or module they alias. Globs omit members that cannot be re-exported at the glob's visibility. `pub(in path)` is not part of this revision.
