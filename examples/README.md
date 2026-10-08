# Kagari Examples

These files show syntax that currently passes semantic checking and executes.
From the repository root, run any standalone `.kgr` file with
`cargo run -p kagari-cli -- run <path>`. The CLI exits successfully when its
assertions pass; the `syntax_examples` integration test also checks return values
through source and encoded-artifact loading:

```sh
cargo test -p kagari-embed --test syntax_examples
```

The [error-stack example](error-stack.kgr) intentionally returns Err: running
`cargo run -p kagari-cli -- run examples/error-stack.kgr` prints the original
`read_settings` stack and exits with status 1. See [the reporting contract](../docs/spec/error-reporting.md).

| Executable feature | Small example or existing showcase | Expected result |
| --- | --- | --- |
| Fixed-width bitwise operations and compound assignment | [bitwise.kgr](syntax/bitwise.kgr) | `42` |
| Explicit casts and built-in numeric From/TryFrom | [numeric-conversions.kgr](syntax/numeric-conversions.kgr) | `42` |
| 6502 address arithmetic, stack/zero-page wrap, branches and ADC/SBC flags | [6502-numeric.kgr](6502-numeric.kgr) | `42` |
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
| `for` over Array, Map, Set and String | [for-collections.kgr](syntax/for-collections.kgr) | `42` |
| Lazy adapters, short-circuit terminals, FromIterator, collect, partition and group_by | [collection-pipelines.kgr](syntax/collection-pipelines.kgr) | `42` |
| Nested iteration, aggregation, ordered extrema and fallible collection | [iterator-extensions.kgr](syntax/iterator-extensions.kgr) | `42` |
| Struct fields, shorthand initialization, inherent methods, enum payloads, tuples, arrays and assignment targets | [data-model.kgr](syntax/data-model.kgr) | `42` |
| Arithmetic (including `%`), comparison, unary and short-circuit logical operators | [expressions.kgr](syntax/expressions.kgr) | `42` |
| String interpolation, joining, Unicode queries, lazy splitting and typed parsing | [string-interpolation.kgr](syntax/string-interpolation.kgr) | `42` |
| Numeric bases and separators, exponents, escaped strings, and nested block comments | [literals-and-comments.kgr](syntax/literals-and-comments.kgr) | `42` |
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
`42`. Run `cargo run -p kagari-embed --example host_interfaces` to install its
offline declarations, compile without callbacks, then bind and execute. The
`host_interfaces` integration test also covers encoded artifacts, JIT-enabled
execution, GC, reentry, permissions and hot reload.

The examples cover executable forms; parser-only recovery cases remain in the
syntax crate tests. The [grammar-witnesses.kgr](syntax/grammar-witnesses.kgr)
file combines grammar branches for the syntax audit and is not a standalone
executable program.
The [syntax coverage audit](../docs/syntax-coverage.md) compares these witnesses
with EBNF rules and records forms that are still missing or unverified.

- `syntax/callables.kgr`: unified `Fn` bounds, contextual closure inference, and stateful callable objects used as callbacks.

## Host-driven async tasks

Run the deterministic [async_tasks host](../crates/kagari-embed/examples/async_tasks/main.rs):

```text
cargo run -p kagari-embed --example async_tasks --no-default-features --features source
```

Its [script](../crates/kagari-embed/examples/async_tasks/script.kgr) uses a synchronous
handler to admit an async closure, then returns before IO starts. Each ordinary
`for` iteration awaits an RPC and a database request. Two public native providers
share the typed completion protocol. The host drives bounded slices and can run
another handler while the Task waits. Notifications record readiness without
executing script. A successful report returns `300`; a second Task is cancelled by
scope close, cleanup completes without a remote reply, and a late reply is stale.
The demo needs no network, timed sleeps, Actor framework or Tokio executor.

To emit the same portable artifact and verify the standalone source-free host:

```text
cargo run -p kagari-embed --example async_tasks --no-default-features --features source -- target/fixtures/async_tasks.kbc
uv run python scripts/check_features.py --async-only
```

The focused command checks only the artifact-only async consumer and its dependency
graph. The complete feature/backend matrix remains a GitHub CI responsibility.
