# Standard library declaration sources

The standard library's public API is described by versioned Kagari declaration
sources. All source comments, documentation and examples are written in English.
The declaration source owns public signatures, documentation, method views and
source locations. Runtime code owns representation and execution contracts.

## Declaration mode

`parse_declarations` is an explicit, cancellable parser entry with ordinary parser
limits. It permits top-level `fn ...;` signatures and opaque `pub type Name<T>;` declarations. Ordinary source parsing still
requires a body. Parsing an interface does not grant code-generation authority.
Standard library sources are installed by the engine, not discovered from user
imports or recognized by a user-controlled file extension.

Outer `///` comments belong to the immediately following declaration. They retain
Markdown including fenced Kagari examples. The CST remains lossless. Existing
`#[intrinsic(...)]` and `#[method(...)]` attribute syntax is used for interface metadata;
no second attribute syntax is introduced.

The implementation sequence and acceptance status are tracked in
[the implementation roadmap](../implementation-roadmap.md#standard-library-declaration-sources).


## Public functions and method views

The HIR build reads the bundled `.kgr` files with the declaration parser and
compiles their AST signatures into immutable metadata. Generated output is a
build artifact, not a second handwritten API definition. Unknown intrinsic IDs,
duplicate bindings/exports/method views, missing documentation and function bodies
fail the standard-library build. The engine bundles the exact parsed source text.

`#[method(name)]` exposes a view using the first parameter as receiver. Function
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

## Native types and standard protocols

The same files declare Option, Result, Ordering, Array, Map, Set, String and Cursor.
Native declarations bind existing engine representations; they do not define empty
script structs. Enum variant order and payload counts are checked against the
runtime discriminant contract. Primitive scalar representations remain engine-owned.

All 21 standard traits derive their public contracts from these sources, including
supertraits, generic parameters, methods, associated types and associated bounds.
Trait solving and native implementations remain engine code. Declaration identities
and member locations refer to the bundled source text, not placeholder spans.

Documentation examples for functions, types and traits execute both directly and
after artifact serialization. Nested members document their role within the enclosing
protocol; protocol examples demonstrate the complete use rather than duplicating
the same example on every associated type.

## Tool queries

`AnalysisSnapshot::source` reads ordinary analyzed files and the exact bundled SDK
sources by file identity. `definition_at` and `declaration` route standard targets
to their real identifier ranges. `FileAnalysis::standard_api_at` provides the
written declaration and Markdown, `standard_signature_at` instantiates native
function signatures from checked call arguments, and `standard_method_completions`
uses the recovered receiver type even in incomplete code. Method signatures omit
the receiver parameter. These queries do not register or execute host functions.

Standard trait methods and associated types have ordinary declaration identities.
The catalog is immutable across snapshots. User declarations take precedence over
unqualified native names; navigation follows resolution, not a text-name heuristic.
Native method candidates are one input to completion; lexical trait completion and
the LSP transport remain separate tool work.
