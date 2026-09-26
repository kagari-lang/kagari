# Kagari standard library

These declaration files document the standard library bundled with the engine.
They contain the public signatures used by compilation and tooling. Read the
`///` comments above a declaration for its behavior, constraints and examples.
Runtime-native functions intentionally have no Kagari body.

## Modules

| Module | Contents |
| --- | --- |
| [array](array.kgr) | Shared arrays, indexed access, insertion and removal |
| [map](map.kgr) | Hash maps, key lookup and shallow snapshots |
| [set](set.kgr) | Hash sets, membership and set algebra |
| [string](string.kgr) | Immutable UTF-8 strings, byte ranges and scalar iteration |
| [option](option.kgr) | Optional values, mapping, fallbacks and conversion to Result |
| [result](result.kgr) | Recoverable errors, propagation and preserved error origins |
| [iter](iter.kgr) | Iterator, IntoIterator, native Cursor and collection helpers |
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
functions can be called by qualified path, such as `std::array::get(values, index)`;
functions marked `#[method(get)]` also support `values.get(index)`.

Integer literals currently have type `i32`, while indices and lengths use `usize`.
Examples obtain `usize` values from collection or string lengths. Functions taking
`f64` show a typed function parameter because unsuffixed float literals are `f32`.
Logging requires a host log binding and permission to call it.

A `# Panics` section describes script traps. It does not mean Rust unwinding or a
catchable business error. Recoverable absence and errors use `Option` and `Result`.
`kgr,should_panic` examples intentionally trap. All fenced API examples are checked
by `cargo test -p kagari-embed --test standard_declarations`, through both direct
compilation and serialized artifact loading.

## Shared semantics

Mutable containers and structs share object references. Passing, returning or
collecting their elements does not deep-copy object graphs. Failed standard
mutations leave their target unchanged; completed earlier side effects remain.
Do not structurally mutate a guarded collection during iteration.

Hash keys must keep their equality and hash stable while stored. Equal keys must
have equal hashes; matching hashes alone do not imply equality. Iteration order
is not a public sorting guarantee. String offsets count UTF-8 bytes unless an API
explicitly says otherwise.

See [value semantics](../docs/spec/value-semantics.md),
[standard protocols](../docs/spec/traits.md), and the
[declaration architecture](../docs/spec/standard-declarations.md) for full contracts.
