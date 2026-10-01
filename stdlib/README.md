# Kagari standard library

These declaration files document the standard library bundled with the engine.
They contain the public signatures used by compilation and tooling. Read the
`///` comments above a declaration for its behavior, constraints and examples.
Runtime-native functions intentionally have no Kagari body. The installed
`kagari-stdlib` package prepares the exact sources for ordinary HIR checking and
tool queries. Checked native calls carry provider/signature/witness contracts into
artifacts; Rust runtime helpers and continuations own their execution.

## Modules

| Module | Contents |
| --- | --- |
| [array](array.kgr) | Shared arrays, indexed access, insertion, removal and string joining |
| [map](map.kgr) | Hash maps, key lookup and shallow snapshots |
| [set](set.kgr) | Hash sets, membership and set algebra |
| [string](string.kgr) | Immutable UTF-8 strings, byte ranges and scalar iteration |
| [option](option.kgr) | Optional values, mapping, fallbacks and conversion to Result |
| [result](result.kgr) | Recoverable errors, propagation and preserved error origins |
| [iter](iter.kgr) | Iterator, Iterable, FromIterator, Sum/Product and lazy pipelines |
| [math](math.kgr) | Checked numeric helpers and floating-point operations |
| [debug](debug.kgr) | Assertions, traps and host-routed logging |
| [cmp](cmp.kgr) | PartialEq, Eq, PartialOrd, Ord and Ordering |
| [hash](hash.kgr) | Hash and the equality/hash contract |
| [fmt](fmt.kgr) | Debug and Display |
| [ops](ops.kgr) | Arithmetic, unary and read-only indexing protocols |
| [convert](convert.kgr) | From, Into, TryFrom and TryInto |

## Reading examples

Examples use Kagari syntax. A snippet without `fn main` can be placed inside a
`main` function. A snippet containing `fn main` is a complete program. Standard
methods live in `impl` blocks: `values.get(index)` calls the same declaration as
`ArrayList::get(values, index)`. Constructors such as `ArrayList::new()` omit `self`.
Generic impl parameters describe the receiver; method generics describe additional
types introduced by that operation. Free functions such as `std::math::min` remain
at module scope.

Native collection Iterable/FromIterator implementations are declared in their
respective files. Result and Option declare fallible collection implementations.
The concrete iterator also declares its protocol explicitly:
`impl<T> Iterator for Iter<T>` supplies `Item` and the native `next` method.
Its `map`, `filter` and `collect` methods are defaults declared on `Iterator`.

Numeric literals use contextual types and otherwise default to `i32` or `f64`.
Suffixes select a primitive type explicitly, for example `1usize` and `1.0f32`.
Indices and lengths use `usize`; examples can also obtain these from lengths.
Logging requires a host log binding and permission to call it.

A `# Panics` section describes script traps. It does not mean Rust unwinding or a
catchable business error. Recoverable absence and errors use `Option` and `Result`.
`kgr,should_panic` examples intentionally trap. All fenced API examples are checked
by `cargo test -p kagari-embed --test standard_declarations`, through both direct
compilation and serialized artifact loading.

## Shared semantics

Mutable containers and structs share object references. Passing, returning or
collecting their elements does not deep-copy object graphs. Failed preparation
leaves mutation targets unchanged. Committed updates and completed earlier side
effects remain visible when later execution fails.
Do not structurally mutate a guarded collection during iteration.

Hash keys must keep their equality and hash stable while stored. Equal keys must
have equal hashes; matching hashes alone do not imply equality. Iteration order
is not a public sorting guarantee. String offsets count UTF-8 bytes unless an API
explicitly says otherwise.

See [value semantics](../docs/spec/value-semantics.md),
[standard protocols](../docs/spec/traits.md), and the
[declaration architecture](../docs/spec/standard-declarations.md) for full contracts.

## Constructing text

Use `f"name={name}"` for Display formatting and `f"value={value:?}"` for Debug.
Use `{{` and `}}` for literal braces. Expressions are evaluated once, in order;
formatting failures propagate normally. Join already formatted strings with
`["red", "green"].join(", ")`. Two strings can still be joined with `concat`.

Read-only `List<String>` supports the same join call, including String-key map
snapshots. String-yielding iterators join their remaining items through the first
None; format other values explicitly with `.map(|x| f"{x}").join(", ")`.
See [the runnable example](../examples/syntax/string-interpolation.kgr).

Collection interfaces (`List`/`MutableList`, `Map`/`MutableMap`, `Set`/`MutableSet`)
are declared alongside their native impl witnesses. Constructors belong to
`ArrayList`, `LinkedHashMap`, and `LinkedHashSet`. `[T]` is the List interface;
array literals create ArrayList storage. See the collection-access specification.
