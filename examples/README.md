# Kagari Examples

These files show syntax that currently passes semantic checking and executes.
From the repository root, run any standalone `.kgr` file with
`cargo run -p kagari-cli -- run <path>`. The CLI exits successfully when its
assertions pass; the `syntax_examples` integration test also checks return values
through source and encoded-artifact loading:

```sh
cargo test -p kagari-embed --test syntax_examples
```

| Executable feature | Small example or existing showcase | Expected result |
| --- | --- | --- |
| `val`, `var`, `while`, `loop`, `if`, `continue`, `break` and loop values | [control-flow.kgr](syntax/control-flow.kgr) | `42` |
| `for` over Array, Map, Set and String | [for-collections.kgr](syntax/for-collections.kgr) | `42` |
| Struct fields, shorthand initialization, inherent methods, enum payloads, tuples and arrays | [data-model.kgr](syntax/data-model.kgr) | `42` |
| Arithmetic (including `%`), comparison, unary and short-circuit logical operators | [expressions.kgr](syntax/expressions.kgr) | `42` |
| Literal, wildcard, binding, nested tuple, struct and enum `match` patterns | [match.kgr](syntax/match.kgr) | `42` |
| Generic function `where` bound and trait call | [where-bounds.kgr](syntax/where-bounds.kgr) | `42` |
| Aliased module import | [import-alias.kgr](syntax/import-alias.kgr) | `42` |
| Concrete interface value and dynamic method dispatch | [interface-dispatch.kgr](interface-dispatch.kgr) | `42` |
| Generic trait method | [generic-trait-methods.kgr](generic-trait-methods.kgr) | `42` |
| Functions, `const`, generics, structs, enums, arrays, tuples, fields, indexes, assignment and core standard modules | [standard-library.kgr](standard-library.kgr) | `(3, true, 2, true, 2, true, 12)` |

The [imported-traits](imported-traits/) files show public declarations, grouped
imports and cross-module trait implementations. Their module identities are
bound by the `source_modules` integration test, which executes them from source
and artifacts. [host-trait-bound.kgr](host-trait-bound.kgr) likewise requires the
host declarations installed by the `offline_nominal` integration test. They are
not standalone CLI programs.

Closure expressions remain a syntax and runtime gap. Parser tests document
syntax-only forms; the grammar alone does not establish runtime support.
