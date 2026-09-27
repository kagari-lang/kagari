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
`#[intrinsic(...)]` binds native execution. The installed numeric declarations
use `#[numeric(...)]` to pair an integer operation with their concrete impl target;
it does not grant user declarations intrinsic behavior. Instance methods are declared with
`self` inside an inherent or native trait `impl`; there is no method-alias attribute.

The implementation sequence and acceptance status are tracked in
[the implementation roadmap](../implementation-roadmap.md#standard-library-declaration-sources).


## Public functions and method views

The HIR build reads the bundled `.kgr` files with the declaration parser and
compiles their AST signatures into immutable metadata. Generated output is a
build artifact, not a second handwritten API definition. Unknown intrinsic IDs,
duplicate bindings/exports/method views, missing documentation and function bodies
fail the standard-library build. The engine bundles the exact parsed source text.

Generic inherent blocks own their receiver parameters, for example
`impl<T> ArrayList<T> { pub fn get(self, index: usize) -> Option<T>; }`.
Associated constructors omit `self`. Method-specific generic parameters extend
the impl parameters; `Self` resolves to the impl target. Concrete targets such as
`impl ArrayList<String>` restrict methods to that receiver shape. Metadata and source
identities are generated from these declarations, including read-only versus
mutable receiver access. Method syntax and qualified calls such as
`ArrayList::get(values, index)` share the same checked signature. Old module-level
method functions are removed. Genuine free functions remain at module scope.
Native `Iterable`, `OrderedNumber` and `SignedNumber` constraints retain their
existing restricted meanings. They do not grant arbitrary Iterator or operator
implementations access to native helpers.

Public documentation follows the [rustdoc writing guidance](https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html):
a concise summary, behavior and boundary details, applicable Panics sections and
executable Examples. Kagari's Panics sections describe script traps, not Rust
unwinding. Examples run through source compilation and encoded artifact loading.
Numeric literals use context before falling back to i32 or f64; suffixes such as
`1usize` and `1.0f32` select an explicit primitive type. These rules do not introduce
implicit numeric conversions. See the [literal rules](syntax.md#literals).

## Native types and standard protocols

The same files declare Option, Result, Ordering, Array, Map, Set, String and Iter.
Native declarations bind existing engine representations; they do not define empty
script structs. Enum variant order and payload counts are checked against the
runtime discriminant contract. Primitive scalar representations remain engine-owned.

All standard traits derive their public contracts from these sources, including
supertraits, generic parameters, methods, associated types and associated bounds.
Trait solving and native implementations remain engine code. Declaration identities
and member locations refer to the bundled source text, not placeholder spans.
Method-local generic parameters and their bounds retain method-owned identities
through analysis and portable ABI validation. Engine-supplied iterator defaults are
marked with intrinsic attributes; user implementations need only supply required
methods and may provide ordinary explicit overrides of default methods. The fixed
`List::join` and `Iterator::join` traversal operations are exceptions: their declared
string-item constraints are checked at each call, and implementations cannot override
them. They lower at the call site and do not require a virtual method binding. Runtime
interface tables retain vacant slots for omitted engine operations so later declared
method ordinals remain stable.

`iter.kgr` explicitly declares `impl<T> Iterator for Iter<T>`, including
`type Item = T` and `#[intrinsic(IterNext)] fn next(self) -> Option<T>;`.
The build validates this implementation against the sealed native stepping ABI.
Its receiver, generic parameter, associated type and method metadata come from
the declaration. Native iterator trait resolution reads that associated type
mapping. `map`, `filter`, `collect` and other defaults remain on `Iterator`.
The identity `Iterable` implementation remains derived from `Iterator`.
This declaration does not grant user code access to intrinsic bindings.

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

`builtin::declarations::implementations` exposes explicit native implementations
for a checked receiver, with source identities distinct from the trait's members.
Completion on `Iter<T>` includes the declared `next` implementation and inherited
`Iterator` methods. Ordinary trait-call navigation still identifies the protocol
member; the implementation catalog provides the concrete implementation location.

Native method declarations retain where predicates and qualified associated-type
projections. For example, Iterator::min requires Self::Item: Ord; this obligation
is checked at call sites and retained in portable method contracts.


Collection SDK files explicitly declare Iterable and FromIterator implementations
for ArrayList, LinkedHashMap and LinkedHashSet; String declares Iterable.
The same files declare List/MutableList, Map/MutableMap and Set/MutableSet
traits and native impl witnesses. Their inherited members and source locations
are available to semantic queries, including read-only member completion.
Result and Option declare their conditional FromIterator implementations. Generic
arguments, key constraints, destination bounds, associated outputs and member
locations come from those declarations. The build validates the supported native
signatures and intrinsic bindings. Static constructors do not appear as instance
completion candidates. Iterable inheritance through a generic associated Iter
retains the originating Item equality constraints.

## String query semantics

String search and slicing use UTF-8 byte offsets. `find`/`rfind` return the first/last
substring offset or `None`; empty patterns match zero/the byte length respectively.
`strip_prefix` and `strip_suffix` remove exactly one match, returning `Some` even
for an empty pattern. Trimming uses Unicode White_Space; start/end variants retain
whitespace on the opposite end. No operation normalizes Unicode or mutates its
receiver. Returned strings own their contents; optional results are ordinary
GC-rooted Option values.
