# Standard library declaration sources

The standard library's public API is described by versioned Kagari declaration
sources. All source comments, documentation and examples are written in English.
The declaration source owns public signatures, documentation, method views and
source locations. Runtime code owns representation and execution contracts.

## Declaration mode

`parse_declarations` is an explicit, cancellable parser entry with ordinary parser
limits. It permits top-level `fn ...;` signatures. Ordinary source parsing still
requires a body. Parsing an interface does not grant code-generation authority.
Standard library sources are installed by the engine, not discovered from user
imports or recognized by a user-controlled file extension.

Outer `///` comments belong to the immediately following declaration. They retain
Markdown including fenced Kagari examples. The CST remains lossless. Existing
`@intrinsic(...)` and `@method(...)` attribute syntax is used for interface metadata;
no second attribute syntax is introduced.

The implementation sequence and acceptance status are tracked in
[the implementation roadmap](../implementation-roadmap.md#standard-library-declaration-sources).


## Public functions and method views

The HIR build reads the bundled `.kgr` files with the declaration parser and
compiles their AST signatures into immutable metadata. Generated output is a
build artifact, not a second handwritten API definition. Unknown intrinsic IDs,
duplicate bindings/exports/method views, missing documentation and function bodies
fail the standard-library build. The engine bundles the exact parsed source text.

`@method(name)` exposes a view using the first parameter as receiver. Function
and method calls share parameter types, generic constraints and result types.
Native `Iterable`, `OrderedNumber` and `SignedNumber` constraints retain their
existing restricted meanings. They do not grant arbitrary Iterator or operator
implementations access to native helpers.

Public documentation follows the [rustdoc writing guidance](https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html):
a concise summary, behavior and boundary details, applicable Panics sections and
executable Examples. Kagari's Panics sections describe script traps, not Rust
unwinding. Examples run through source compilation and encoded artifact loading.
The numeric examples reflect current literal typing: unsuffixed integers are i32,
unsuffixed floats are f32; usize values may come from lengths and f64 values from
typed host bindings. Declaration migration does not introduce numeric casts.
