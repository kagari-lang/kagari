# Kagari Examples

These files show syntax that currently passes semantic checking and executes.
From the repository root, run any standalone `.kgr` file with
`cargo run -p kagari-cli -- run <path>`. The CLI exits successfully when its
assertions pass. Ordinary `syntax/*.kgr` programs returning `42` are discovered as
independent SDK library tests, which check source and encoded-artifact execution.
Use a filename filter for a focused run, or `--list` to inspect the group:

```sh
cargo test -p kagari --lib -- runtime::tests::language::syntax_examples::source_examples::path_01__UP__UP_examples_syntax_array_operations_kgr
cargo test -p kagari --lib -- --list runtime::tests::language::syntax_examples::source_examples::
```

The [testing guide](../docs/testing.md#directory-discovered-syntax-examples) defines
admission, the five different-contract exclusions and the small explicit group for
root examples/special execution routes. Add ordinary syntax examples without a
Rust registration row; do not assume declaration-only, async, grammar-witness or
different-result files share this execution contract.

Source tools can inspect an example without executing it:

```sh
cargo run -p kagari-cli -- fmt examples/syntax/string-interpolation.kgr --check
cargo run -p kagari-cli -- check examples/syntax/string-interpolation.kgr
cargo run -p kagari-cli -- lint examples/syntax/string-interpolation.kgr
```

Formatting check may report layout differences; lint warnings are optional and do
not mean a language error. See [tool configuration and rules](../docs/spec/toolchain-tools.md).
The formatter's full grammar matrix consumes the syntax fixtures without rewriting
them and runs in CI.

The [error-stack example](error-stack.kgr) intentionally returns Err: running
`cargo run -p kagari-cli -- run examples/error-stack.kgr` prints the original
`read_settings` stack and exits with status 1. See [the reporting contract](../docs/spec/error-reporting.md).

| Executable feature | Small example or existing showcase | Expected result |
| --- | --- | --- |
| Fixed-width bitwise operations and compound assignment | [bitwise.kgr](syntax/bitwise.kgr) | `42` |
| Explicit casts and built-in numeric From/TryFrom | [numeric-conversions.kgr](syntax/numeric-conversions.kgr) | `42` |
| 6502 address arithmetic, stack/zero-page wrap, branches and ADC/SBC flags | [6502-numeric.kgr](6502-numeric.kgr) | `42` |
| Script Vec3/rays, analytic sphere hits/misses, normalization and local fixed-seed SplitMix64 sampling (64 samples) | [numeric-sampling.kgr](numeric-sampling.kgr) | `42`; public `sampling_checksum()` returns `0x396d503bb05c761a` |
| Standard `PartialEq/Eq/Hash`, `Debug/Display`, generic bounds and structural/identity keys | [standard-traits.kgr](syntax/standard-traits.kgr) | `42` |
| Custom/native iterators, associated Item and generic for loops | [iterators.kgr](syntax/iterators.kgr) | `42` |
| Explicit and fallible conversion protocols, derived reverse calls | [conversions.kgr](syntax/conversions.kgr) | `42` |
| `Ordering`, `PartialOrd`/`Ord`, custom and generic comparisons | [ordering.kgr](syntax/ordering.kgr) | `42` |
| `Add`/`Sub`/`Mul`/`Div`/`Rem`, `Neg`/`Not`, different operand and result types | [operators.kgr](syntax/operators.kgr) | `42` |
| Read-only `Index`, generic access and shared returned objects | [index.kgr](syntax/index.kgr) | `42` |
| `Option`/`Result` constructors, propagation, lazy combinations, flattening and transposition | [result-option.kgr](syntax/result-option.kgr) | `42` |
| `val`, `var`, `while`, `loop`, `if`, `continue`, `break` and loop values | [control-flow.kgr](syntax/control-flow.kgr) | `42` |
| Block expressions, independent block statements, and block `match` arms | [blocks.kgr](syntax/blocks.kgr) | `42` |
| Array initialization, bulk copying, prepared sorting/filtering, lazy chunks and immediate range removal | [array-operations.kgr](syntax/array-operations.kgr) | `42` |
| Lazy integer ranges, open bounds and range-based copying | [ranges.kgr](syntax/ranges.kgr) | `42` |
| `for` over Map entries and Set elements | [for-collections.kgr](syntax/for-collections.kgr) | `42` |
| Lazy adapters, short-circuit terminals, FromIterator, collect, partition and group_by | [collection-pipelines.kgr](syntax/collection-pipelines.kgr) | `42` |
| Nested iteration, aggregation, ordered extrema and fallible collection | [iterator-extensions.kgr](syntax/iterator-extensions.kgr) | `42` |
| Struct fields, shorthand initialization, inherent methods, enum payloads, tuples, arrays and assignment targets | [data-model.kgr](syntax/data-model.kgr) | `42` |
| Arithmetic (including `%`), comparison, unary and short-circuit logical operators | [expressions.kgr](syntax/expressions.kgr) | `42` |
| Identifier-only String interpolation, Debug, literal braces and ordered intermediate strings | [string-interpolation.kgr](syntax/string-interpolation.kgr) | `42` |
| Numeric bases and separators, exponents, cooked/raw multiline strings, continuations and nested block comments | [literals-and-comments.kgr](syntax/literals-and-comments.kgr) | `42` |
| Unicode scalar characters, escapes, constants and character patterns | [char-literals.kgr](syntax/char-literals.kgr) | `()` |
| Byte characters and cooked/raw multiline byte arrays | [byte-literals.kgr](syntax/byte-literals.kgr) | `1` |
| Literal, wildcard, binding, nested tuple, struct and enum `match` patterns | [match.kgr](syntax/match.kgr) | `42` |
| Guarded `match` arms and pattern-bound guard names | [match-guards.kgr](syntax/match-guards.kgr) | `42` |
| `if val` and `while val` binding conditions | [binding-conditions.kgr](syntax/binding-conditions.kgr) | `42` |
| Tool metadata attributes on items, fields and methods | [attributes.kgr](syntax/attributes.kgr) | `42` |
| `|` alternatives, shared bindings, inclusive and exclusive range patterns, constant bounds | [pattern-alternatives.kgr](syntax/pattern-alternatives.kgr) | `42` |
| Result error conversion through From with preserved diagnostic origins | [error-conversion.kgr](syntax/error-conversion.kgr) | `42` |
| Body inference, local placeholders, explicit call type arguments and numeric suffixes | [type-inference.kgr](syntax/type-inference.kgr) | `42` |
| Generic function `where` bound and trait call | [where-bounds.kgr](syntax/where-bounds.kgr) | `42` |
| Lexical closures, mutable captures, nested closures, function types and higher-order calls | [closures.kgr](syntax/closures.kgr) | `42` |
| Aliased module import | [import-alias.kgr](syntax/import-alias.kgr) | `42` |
| Inline module body and wildcard import | [inline-modules.kgr](syntax/inline-modules.kgr) | `42` |
| Private field with parent-visible module, function and method | [visibility.kgr](syntax/visibility.kgr) | `42` |
| Concrete interface value and dynamic method dispatch | [interface-dispatch.kgr](interface-dispatch.kgr) | `42` |
| Associated types, equality bindings, projection bounds and static/dynamic calls | [associated-types.kgr](syntax/associated-types.kgr) | `42` |
| Type-parameterized associated types, input/output bounds, inheritance and qualified projections | [generic-associated-types.kgr](syntax/generic-associated-types.kgr) | `42` |
| Scalar associated constants, defaults, overrides and qualified static access | [associated-constants.kgr](syntax/associated-constants.kgr) | `42` |
| Generic impls converted to concrete dynamic interfaces and table reuse | [generic-interfaces.kgr](syntax/generic-interfaces.kgr) | `42` |
| Supertraits, inherited projections, diamond deduplication and interface upcasting | [trait-inheritance.kgr](syntax/trait-inheritance.kgr) | `42` |
| Default method fallback, override precedence, generic impls and static/dynamic calls | [default-methods.kgr](syntax/default-methods.kgr) | `42` |
| Generic trait method | [generic-trait-methods.kgr](generic-trait-methods.kgr) | `42` |
| Functions, `const`, generics, structs, enums, arrays, tuples, fields, indexes, assignment and core standard modules | [standard-library.kgr](standard-library.kgr) | `(3, true, 2, true, 2, true, 12)` |

The [imported-traits](imported-traits/) files show public declarations, grouped
imports and cross-module trait implementations. Their module identities are
bound by the `source_modules` integration test, which executes them from source
and artifacts. [host-trait-bound.kgr](host-trait-bound.kgr) likewise requires the
host declarations installed by the `offline_nominal` integration test. They are
not standalone CLI programs.

[host-interfaces.kgr](host-interfaces.kgr) demonstrates associated outputs on a
host type, qualified projections and static/dynamic interface calls returning
`42`. Run `cargo run -p kagari --example host_interfaces` to install its
offline declarations, compile without callbacks, then bind and execute. The
`host_interfaces` integration test also covers encoded artifacts, JIT-enabled
execution, GC, reentry, permissions and hot reload.

Rust embedding examples use common declarations, explicit `bind` and installation:

| Boundary | Command | Result |
| --- | --- | --- |
| One method set for runtime-owned Opaque and scoped Ref/RefMut; checked read/edit/take | `cargo run -p kagari --example scoped_host` | Both calls return hp=70; take returns hp=71 and old aliases reject access |
| DTO derive with a retained Opaque field | `cargo run -p kagari --example value_interop` | `Applied(Response { amount: 40 })` |
| Optional schema-backed Serde | `cargo run -p kagari --example serde_interop --no-default-features --features source,serde` | `Applied(7, 20)` |
| Cold declarations, encoded contracts and installed callbacks | `cargo run -p kagari-runtime --example offline_host` | Immutable configuration returns 42 |

The [embedding guide](../docs/spec/embedding-api.md#typed-native-registration)
describes supported schemas and payload capabilities. The
[host interop contract](../docs/spec/host-interop.md) defines scope/loan and explicit
Kagari trait registration; Rust methods/traits are never exposed by DTO registration.

The examples cover executable forms; parser-only recovery cases remain in the
syntax crate tests. The [grammar-witnesses.kgr](syntax/grammar-witnesses.kgr)
file combines grammar branches for the syntax audit and is not a standalone
executable program.
The [syntax coverage audit](../docs/syntax-coverage.md) compares these witnesses
with EBNF rules and records forms that are still missing or unverified.

- `syntax/callables.kgr`: unified `Fn` bounds, contextual closure inference, and stateful callable objects used as callbacks.

## Packages and workspaces

The [package workspace](package-workspace/README.md) contains two source packages,
explicit workspace dependency inheritance, an external child module and multiple
public host entries. Its README gives CLI check/build/run commands and a Rust host
that invokes several exports from the same jointly compiled Program.

## Persistent state across hot reload

Run the [persistent_static host](../crates/kagari/examples/persistent_static.rs):

```sh
cargo run -p kagari --example persistent_static --no-default-features --features source
```

The example increments a `static var` counter, mutates an object stored in
`static val`, retains a callback, and publishes new code with changed initializers.
After reload the counter continues at 3 rather than restarting at 101. The retained
callback runs v1 code against the current counter (13), while an ordinary v2 call
returns 23. An untouched static first accessed through v2 still uses v1's recipe
(10); a fresh installation uses v2's recipe (20).

`const` remains a compile-time value. `static` is explicitly typed, lazy runtime
storage local to an installation; `val` prevents rebinding, not object mutation.
Compatible reload shares cells but never retargets old code. Initializers cannot
start async work, recursive initialization fails permanently, and compatible
reload cannot delete/rename a static or change its storage contract. This is not
disk persistence or automatic snapshot/state migration. See the
[module guide](../docs/spec/modules.md#persistent-module-statics) and
[activation rules](../docs/spec/module-activation.md).

## Host-driven async tasks

Run the deterministic [async_tasks host](../crates/kagari/examples/async_tasks/main.rs):

```text
cargo run -p kagari --example async_tasks --no-default-features --features source
```

Its [script](../crates/kagari/examples/async_tasks/script.kgr) uses a synchronous
handler to admit an async closure, then returns before IO starts. Each ordinary
`for` iteration awaits an RPC and a database request. Two public native providers
share the typed completion protocol. The host drives bounded slices and can run
another handler while the Task waits. Notifications record readiness without
executing script. A successful report returns `300`; a second Task is cancelled by
scope close, cleanup completes without a remote reply, and a late reply is stale.
The demo needs no network, timed sleeps, Actor framework or Tokio executor.

To emit the same portable artifact and verify the standalone source-free host:

```text
cargo run -p kagari --example async_tasks --no-default-features --features source -- target/fixtures/async_tasks.kbc
uv run python scripts/check_features.py --async-only
```

The focused command checks only the artifact-only async consumer and its dependency
graph. The complete feature/backend matrix remains a GitHub CI responsibility.
