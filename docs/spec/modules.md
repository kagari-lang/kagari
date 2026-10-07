# Modules and imports

A source file defines a module. `mod child { ... }` defines a child module in the same physical file; `mod child;` refers to a separately registered child source. The two forms cannot define the same identity. A module contains declarations (`use`, `mod`, `fn`, `const`, `struct`, `enum`, `trait`, and `impl`) only. Statements and expressions execute inside an explicitly called function. There is no implicit module entry or module result. `static` is not part of this revision.

The source database binds each registered source to a package and module identity. Disk, supplied source, and editor overlays use the same identity and analysis path; an active overlay replaces its base source. Analysis does not discover unregistered files. Source locations retain the physical file and revision, including for inline modules.

Imports bind names and create dependency edges for linking. `use path::item`, aliases, grouped imports, and globs resolve against source, standard-library, and declared host namespaces. `self`, `super`, and `crate` are path roots. A public re-export forwards the resolved target. Local declarations and explicit imports take precedence over globs; conflicting glob names are diagnosed. Import resolution never runs code or initializes an instance.

Names occupy independent **Type** and **Value** spaces. Types, traits, type parameters,
`Self`, modules and package aliases belong to Type. Functions, constants, locals,
parameters and existing enum constructors belong to Value. Type annotations,
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

The graph contains every module reachable from the selected root, including unused imports. Its order is stable but carries no initialization meaning. Cyclic module references are legal; recursive calls remain subject to runtime call-depth limits and cancellation. The checked program resolves calls and types using declaration identities, then links to slots within one program version. Errors in any reachable module prevent code generation, while analysis remains queryable for tools.

`const` is an immutable compile-time value evaluated within the restricted const-safe language. A module instance has no initialization state or cached result. Loading verifies and links the complete program before publishing a handle. A call names a function entry explicitly; the CLI invokes `main`. A missing or ambiguous entry fails before executing script instructions.

Hot reload prepares and validates a candidate program, stages isolated runtime instances, rechecks the expected base version and bindings, then publishes it. The ordinary reload API does not execute candidate functions. Hosts that explicitly evaluate a candidate before publication must use the restricted candidate session; its effects are limited as described in [module activation](module-activation.md). A failed candidate leaves the active entry intact. Calls already in progress retain their pinned program and dependency versions.

Visibility is checked during import, qualified lookup, field access, struct construction, and inherent method calls. `pub` permits access from other module trees. An unmarked item or field is private to its owning module and its descendants. `pub(super)` permits access from the parent module and its descendants. A private inherent method is available within its defining module tree. Every module and item component of a direct path must be accessible. Re-exports may expose a public item through a private module, but cannot widen the visibility of the item or module they alias. Globs omit members that cannot be re-exported at the glob's visibility. `pub(crate)` and `pub(in path)` are not part of this revision.
