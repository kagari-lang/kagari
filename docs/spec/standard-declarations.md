# Standard library declaration sources

The standard library's public API is described by versioned Kagari declaration
sources. All source comments, documentation and examples are written in English.
The declaration source owns public signatures, documentation, method views and
source locations. Runtime code owns representation and execution contracts.

The NR reset currently reinstalls ArrayList new/len/push/from_fn and List len/get.
Other standard implementation APIs are temporarily removed; their specified
semantics and tests remain NR04 restoration obligations. See the
[active plan](../native-provider-refactor.md#reset-execution-checkpoint-2026-10-01).

## Declaration mode

`parse_declarations` is an explicit, cancellable parser entry with ordinary parser
limits. It permits top-level `fn ...;` signatures and opaque `pub type Name<T>;` declarations. Ordinary source parsing still
requires a body. Parsing an interface does not grant code-generation authority.
Standard library sources are installed by the engine, not discovered from user
imports or recognized by a user-controlled file extension.

`stdlib/std.kgr` declares the public namespace through ordinary re-exports.
`stdlib/prelude.kgr` declares the types and protocols available without an explicit
import. HIR installs the `std` package alias and those prelude bindings only from
the prepared engine package. A local declaration, explicit import or glob binding
takes precedence over an implicit prelude binding; an ambiguous explicit import
does not fall back to the prelude. Standard modules, functions, types and traits
are ordinary source import targets. Option and Result re-export their variants,
and the prelude imports those source declarations. Constructor and pattern facts
retain the variant identity, checked payload types and the owning enum's native
representation hook. Checked callable applications carry the same identities and
native/script implementation selections into portable execution contracts.

Functions and methods returning unit omit the return annotation, for example
`fn clear(self);`. Callback function types still spell out `-> ()`, as in
`fn for_each(self, callback: fn(Self::Item) -> ());`.
Non-returning operations use `-> !`, including `std::debug::panic`. Checked
signatures and native execution contracts retain Never rather than Unit.

Outer `///` comments belong to the immediately following declaration. They retain
Markdown including fenced Kagari examples. The CST remains lossless. `#[native(binding)]` resolves an installed provider descriptor. The old intrinsic,
numeric, radix, protocol and default marker families are removed. User annotations
cannot install handlers or acquire provider authority. Instance methods are declared with
`self` inside an inherent or native trait `impl`; there is no method-alias attribute.

The implementation sequence and acceptance status are tracked in
[the implementation roadmap](../implementation-roadmap.md#standard-library-and-hir-integration-interim-checkpoint).


## Public functions and method views

`kagari-stdlib` reads the installed `.kgr` manifest with the declaration parser
and retains the exact text, syntax trees, annotations, documentation and declaration
coordinates. HIR imports that package through ordinary declaration collection and
checks signatures, bounds and installed native bindings. Unknown binding IDs or a
native declaration with a script body are errors. Ordinary script bodies are
retained for checking and execution. Public declarations must be documented;
documentation examples remain part of acceptance. The ABI does not parse sources
or generate source descriptors. Consumer migration and validation status are
tracked in the [active plan](../stdlib-hir-refactor.md#progress-ledger).

Generic inherent blocks own their receiver parameters, for example
`impl<T> ArrayList<T> { pub fn get(self, index: usize) -> Option<T>; }`.
Associated constructors omit `self`. Method-specific generic parameters extend
the impl parameters; `Self` resolves to the impl target. Concrete targets such as
`impl ArrayList<String>` restrict methods to that receiver shape. Metadata and source
identities are derived by HIR from these declarations, including read-only versus
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
Method-local generic parameters and enclosing impl binders are retained in ABI
signature checks. Native read access comes from provider metadata. List retains its
Index supertrait; its len entry is native and get is an ordinary script body.
Selected default callable/witness metadata remains NR02 work, rather than the old
method-specific traversal and conversion catalogs.

Installed native defaults that forbid replacement explicitly carry
`#[method_policy(Final)]` in their declaration. Unannotated defaults remain
overridable. HIR records this policy independently of the native binding, and
portable callable declarations preserve it for interface validation. Installation
rejects a final required method; user-written policy attributes do not acquire
installed declaration authority.

`iter.kgr` retains required iterator/aggregation declarations. The old Iter impl
and native adapter defaults are removed. Restoring them requires provider-owned
persistent traced state and checked stepping entries under NR03/NR04. Identity
Iterable remains a language protocol rule, not native binding authority.

Documentation examples for functions, types and traits execute both directly and
after artifact serialization. Nested members document their role within the enclosing
protocol; protocol examples demonstrate the complete use rather than duplicating
the same example on every associated type.

## Checked callable execution

Required declarations have no body or executable target. Script declarations retain
ordinary checked bodies; native declarations have provider bindings and no synthetic
script body. Engine functions, native impl methods and native defaults carry concrete
callable identities, checked substitutions, full parameter/result types, bounds and
selected protocol applications. Compiler lowering encodes these facts without
looking up declaration syntax or expanding public standard algorithms.

Portable linking validates each binding against its carried declaration and trusted
operation contract, including associated outputs, generic method arguments, selected
private dependencies and runtime/binding versions. Host bindings additionally retain
their offline host declaration and authority contract. A user declaration with a
native-looking attribute cannot claim installed engine provenance.

Runtime continuations own callback-heavy standard algorithms, buffered construction
and all lazy adapter steps. They use the caller's frames, roots and resource scope;
selected script implementations and explicit overrides use ordinary linked calls.
Readonly native receiver applications may weaken only outer storage access for read
capabilities, preserving invariant nested payload types. Mutable providers cannot
accept readonly access. The final ownership map is in the
[integration plan](../stdlib-hir-refactor.md#final-implementation-ownership-audit).

## Tool queries

`AnalysisSnapshot::source` reads ordinary analyzed files and the installed standard
package retained by that snapshot. `definition_at` and `declaration` identify source
declarations by their real identifier ranges. `AnalysisSnapshot::documentation_at`
provides the resolved declaration, written syntax and Markdown for both user and
standard source. `DeclarationSnapshot::documentation` accepts a declaration identity
without checking bodies. Neither query falls back to a process-global source catalog;
old snapshots retain their original source revision and documentation.

`DeclarationSnapshot::files` enumerates user files, inline modules and the installed
package within the same snapshot. Declaration inventories and executable Markdown
examples use these files and declaration IDs, rather than an ABI source table.
Trait and impl associated types participate in the declaration index with separate
owner identities, exact name ranges and their own written documentation.

`FileAnalysis::call_signature_at` projects the selected local/imported function,
trait method or offline host declaration into a declaration ID, named parameter
types and result type. It applies checked call substitutions; it does not read a
standard signature catalog or replace declared types with the types of erroneous
arguments. Method syntax omits the receiver parameter, and missing arguments do
not remove declared parameters. The query does not register or execute callbacks.

`FileAnalysis::method_completions` returns declaration IDs and names for source
methods, including incomplete member expressions. Inherent receiver matching and
trait interface selection share the HIR call-checking path. Known receiver and
method bounds filter candidates; unsupplied method type arguments remain open.
Explicit implementations retain their declaration IDs, while inherited defaults
refer to the trait declaration. The query does not load a separate standard method
table. The [integration ledger](../stdlib-hir-refactor.md#progress-ledger) records validation
of the shared semantic and executable boundaries.

Standard trait methods and associated types have ordinary declaration identities.
Installed collection and iteration implementations are selected from checked HIR
impl patterns, bounds and associated types. Native selection additionally requires
installed-package provenance; a user implementation of the same trait keeps its
ordinary dispatch. Readonly native storage may satisfy read/iteration capabilities,
but it cannot satisfy mutable capability implementations.
Each snapshot owns immutable declaration facts. User declarations take precedence over
unqualified native names; navigation follows resolution, not a text-name heuristic.
Native method candidates are one input to completion; lexical trait completion and
the LSP transport remain separate tool work.

The analyzed `AggregateCatalog` exposes checked implementations and selects native
applications for a receiver, with source identities distinct from the trait's members.
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
locations come from those declarations. HIR validates the supported native
signatures and installed bindings. Static constructors do not appear as instance
completion candidates. Iterable inheritance through a generic associated Iter
retains the originating Item equality constraints.

Option and Result FromIterator providers execute through rooted native traversal.
They stop at the first None or Err, close guarded native iterators and skip the
inner destination constructor on failure. Result lifting preserves the original
error object and its origin. On success, including empty input, they construct the
selected inner destination once from the prepared ArrayList and then wrap it.
Nested lifting repeats this contract with bounded, checked destination applications.
Script constructors and source methods run on the caller's ordinary execution
frames; native Array/Map/Set constructors share their existing traversal and key
lookup implementations. Portable linking verifies source item equalities, concrete
constructor method arguments, implementation and method bounds, private dependency
closure and any key witnesses before loading. Each original logical operation keeps
its instruction charge and completed side effects across cancellation and traps.

## String query semantics

String search and slicing use UTF-8 byte offsets. `find`/`rfind` return the first/last
substring offset or `None`; empty patterns match zero/the byte length respectively.
`strip_prefix` and `strip_suffix` remove exactly one match, returning `Some` even
for an empty pattern. Trimming uses Unicode White_Space; start/end variants retain
whitespace on the opposite end. No operation normalizes Unicode or mutates its
receiver. Returned strings own their contents; optional results are ordinary
GC-rooted Option values.

String splitting returns lazy, fused `Iter<String>` values. `split` retains empty
fields and matches non-overlapping string separators. The empty separator yields
both empty edge fields and one field per Unicode scalar. `splitn(n, separator)`
yields at most n fields, with the unsplit remainder in the last field; zero yields
none. `split_once`/`rsplit_once` exclude the first/last matching separator and return
None if absent. `split_whitespace` coalesces Unicode whitespace without empty
fields. `lines` recognizes LF and CRLF, retains lone CR and omits an extra field
after a final terminator. Empty input has no lines. Traversals retain their source
and version; each step prepares its result before advancing the shared cursor.

String replacement matches non-overlapping patterns from left to right; the empty
pattern matches each scalar boundary, including both ends. `replacen` limits the
number of replacements. `repeat` checks the output size before allocation; empty
input stays empty for any count. ASCII casing only changes ASCII letters. Unicode
casing uses context-sensitive, locale-independent Unicode mappings and may change
length. `bytes` yields u8 values; `char_indices` yields (byte offset, one-scalar
String). `is_char_boundary` accepts the start/end and rejects out-of-range offsets.

## Option and Result combinations

`unwrap_or_else`, `or_else`, and `map_or_else` invoke only the selected callback,
once. `map_or` evaluates its fallback eagerly like ordinary arguments. Predicates
run only on the requested variant. Option filter retains the original shared
payload when accepted; zip combines two present payloads without deep copying.
`flatten` removes one layer; Result flatten requires the same error type on both
layers. `transpose` exchanges Option and Result. Forwarded Err values preserve
the original trace through flatten and transpose; errors returned by recovery
callbacks retain their own origins. `ok`/`err` discard the opposite variant and
project the payload into Option (which has no error-trace metadata). All callbacks
execute on ordinary VM frames with session budgets, roots and trap cleanup.

## String parsing

`FromStr` has associated type `Err` and static `from_str(String)` returning
`Result<Self, Self::Err>`. String.parse selects this protocol from the expected
type or explicit type argument. Numeric and boolean implementations are declared
in the bundled standard package; user types provide ordinary implementations with their own errors.
All built-in parsers consume the complete input without trimming.

The public parse call carries its checked FromStr witness, exact associated error
type and selected concrete method into the native runtime entry. Numeric providers
reuse the Rust parser; user methods execute on ordinary generation-pinned frames.
Generic associated errors resolve from the carried dependency closure. The entry
returns the original Result, preserving its error origin and logical charge schedule.

Integers use decimal unless from_str_radix specifies 2..=36; plus is accepted,
minus only for signed targets. Prefixes, underscores and whitespace are rejected.
Overflow returns ParseError::OutOfRange. Invalid radix returns InvalidRadix
(instead of Rust's radix panic); empty input returns Empty and invalid digits or
signs return InvalidDigit. Bool accepts exactly true/false. Floating parsing uses
decimal/exponent syntax and case-insensitive NaN/inf/infinity with optional sign;
overflow produces infinity. Invalid boolean/float syntax returns InvalidSyntax.
These are business Result errors carrying their creation trace, not VM traps.

## Equality assertions

`std::debug::assert_eq<T: PartialEq>` evaluates both arguments and its message once,
in order, before comparing them. Its checked native entry delegates to the selected
primitive, script, composed or declared collection-interface equality. Script and
composed callbacks use ordinary execution frames, roots and session limits. The
final assertion uses the existing Rust assertion helper on the original second
charge; failed assertions and callback traps retain their caller and script origins.
