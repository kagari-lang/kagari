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
| Block expressions, independent block statements, and block `match` arms | [blocks.kgr](syntax/blocks.kgr) | `42` |
| Half-open and inclusive integer ranges | [ranges.kgr](syntax/ranges.kgr) | `42` |
| `for` over Array, Map, Set and String | [for-collections.kgr](syntax/for-collections.kgr) | `42` |
| Struct fields, shorthand initialization, inherent methods, enum payloads, tuples, arrays and assignment targets | [data-model.kgr](syntax/data-model.kgr) | `42` |
| Arithmetic (including `%`), comparison, unary and short-circuit logical operators | [expressions.kgr](syntax/expressions.kgr) | `42` |
| Numeric bases and separators, exponents, escaped strings, and nested block comments | [literals-and-comments.kgr](syntax/literals-and-comments.kgr) | `42` |
| Literal, wildcard, binding, nested tuple, struct and enum `match` patterns | [match.kgr](syntax/match.kgr) | `42` |
| Guarded `match` arms and pattern-bound guard names | [match-guards.kgr](syntax/match-guards.kgr) | `42` |
| `if val` and `while val` binding conditions | [binding-conditions.kgr](syntax/binding-conditions.kgr) | `42` |
| Tool metadata attributes on items, fields and methods | [attributes.kgr](syntax/attributes.kgr) | `42` |
| `|` alternatives, shared bindings, inclusive and exclusive range patterns, constant bounds | [pattern-alternatives.kgr](syntax/pattern-alternatives.kgr) | `42` |
| Generic function `where` bound and trait call | [where-bounds.kgr](syntax/where-bounds.kgr) | `42` |
| Lexical closures, mutable captures, nested closures, function types and higher-order calls | [closures.kgr](syntax/closures.kgr) | `42` |
| Aliased module import | [import-alias.kgr](syntax/import-alias.kgr) | `42` |
| Inline module body and wildcard import | [inline-modules.kgr](syntax/inline-modules.kgr) | `42` |
| Concrete interface value and dynamic method dispatch | [interface-dispatch.kgr](interface-dispatch.kgr) | `42` |
| Generic trait method | [generic-trait-methods.kgr](generic-trait-methods.kgr) | `42` |
| Functions, `const`, generics, structs, enums, arrays, tuples, fields, indexes, assignment and core standard modules | [standard-library.kgr](standard-library.kgr) | `(3, true, 2, true, 2, true, 12)` |

The [imported-traits](imported-traits/) files show public declarations, grouped
imports and cross-module trait implementations. Their module identities are
bound by the `source_modules` integration test, which executes them from source
and artifacts. [host-trait-bound.kgr](host-trait-bound.kgr) likewise requires the
host declarations installed by the `offline_nominal` integration test. They are
not standalone CLI programs.

The examples cover executable forms; parser-only recovery cases remain in the
syntax crate tests. The [grammar-witnesses.kgr](syntax/grammar-witnesses.kgr)
file combines grammar branches for the syntax audit and is not a standalone
executable program.
The [syntax coverage audit](../docs/syntax-coverage.md) compares these witnesses
with EBNF rules and records forms that are still missing or unverified.
