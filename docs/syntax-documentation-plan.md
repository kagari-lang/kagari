# Syntax Documentation Completion Plan

Status: planned; documentation implementation has not started. The
[roadmap](implementation-roadmap.md#syntax-documentation-completion) owns activation
and progress. This plan can be executed without the originating conversation.

## Goal and scope

A reader opening any production module in `kagari-syntax` should understand its
role, recognize the source construct represented by each type or variant, and
know how to inspect its data without reconstructing the contract from method bodies.
Cover the entire crate, including public APIs and important private parser state.
Repository documentation and source comments are written in English.

Deliver Rustdoc beside the code, with shared explanations in the existing
[syntax architecture](architecture/syntax.md). Keep language rules in the
[syntax specification](spec/syntax.md) and [grammar](kagari.ebnf). Do not duplicate
the architecture document as a second tutorial or create a separate node catalog.
Preserve behavior, visibility, enum discriminants and data layout. The only planned
code change is forwarding documentation attributes through `ast_node!`.

## Documentation standard

Follow the [Rustdoc writing guide](https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html):
start with a short summary, then explain useful behavior and show an example.
Use `//!` for crate/module orientation and `///` for items, fields and variants.
Use the [Rust API documentation guidelines](https://rust-lang.github.io/api-guidelines/documentation.html)
for relevant examples, cross-links and error contracts; examples may be shared by
linking to the owning type or operation. The standard library's
[`Ordering`](https://doc.rust-lang.org/std/cmp/enum.Ordering.html) illustrates concise
enum/variant descriptions, and
[`std::env::args`](https://doc.rust-lang.org/std/env/fn.args.html) illustrates a summary,
behavior details, panic conditions and usage.

Apply these concrete rules:

- Document every public module, type/alias, trait, function, inherent method,
  field and enum variant, including macro-generated AST types. Trait implementations
  can inherit the trait contract; explain only implementation-specific behavior.
- Put exact spelling first for punctuation and keywords: `+`, `::`, `fn`, etc.
  For identifiers/literals, give a representative source example. For tree kinds,
  identify the construct and link its AST wrapper where one exists. For sentinels
  and recovery kinds, explain their actual role and lack of fixed source spelling.
- Explain representation: what is stored, units/ranges, ownership or borrowing,
  and links to related structures. Distinguish logical child structure from Rust
  fields. State whether an accessor selects direct children or searches descendants.
- Describe `None`, empty iterators, ordering, filtering and relevant defaults.
  Distinguish legally omitted syntax from missing parts caused by error recovery.
  Do not promise that malformed trees retain all positional child roles.
- Use `# Examples`, `# Errors`, `# Panics` and `# Safety` when applicable, without
  empty boilerplate sections. Distinguish diagnostics inside `Parse` from a returned
  cancellation error. Document caller-reachable panics accurately; internal
  invariant assertions are not automatically normal caller error conditions.
- Use intra-doc links for symbols. Rust usage examples should compile and exercise
  a meaningful result, using `?` where appropriate. Mark Kagari snippets and tree
  sketches as `text` so Rustdoc does not compile them as Rust. Do not hide broken
  examples with `ignore` or use `no_run` for examples that can run normally.
- Keep simple entries to one sentence; expand only where structure or behavior
  needs explanation. Avoid restating names/signatures, adding an example per trivial
  getter, or copying the same storage explanation onto every AST wrapper.
- Comment private state, algorithms and invariants needed to follow the parser;
  do not mechanically narrate every statement or test helper.

## Coverage and required content

Paths below are relative to `crates/kagari-syntax/src/`.

| Files | Required documentation |
| --- | --- |
| `lib.rs` | Crate purpose; source -> tokens -> CST -> AST views; a minimal parse/inspect example; entrypoint links; buffer aliases and inline-capacity versus limit distinction. |
| `kind.rs`, `token.rs` | Every variant's spelling/example/role; lexical kinds versus tree kinds; `Token.kind` and byte-span meaning; trivia; EOF/unknown/error distinctions; conversion. Explain parser-combined shift kinds rather than implying every syntax kind has a lexer equivalent. |
| `syntax_node.rs` | Rowan language marker, node/token/element/children aliases; green storage versus navigable handles; ranges and retention; root construction and valid raw-kind assumptions. |
| `ast/mod.rs`, `ast/traits.rs`, `ast/support.rs` | One-field wrappers over the same CST; kind-based casting is not validation or reparsing; clone/retention model; direct-child/token selection; documentation extraction behavior. Forward macro doc attributes onto generated types. |
| `ast/expr.rs` | Every wrapper and `Expr` variant; source form, child shape and accessor mapping. Cover paths/generics/qualified members, calls, block tails, conditional bindings, patterns, ranges, closures and interpolation, not only simple expressions. |
| `ast/item.rs`, `ast/misc.rs` | Every declaration/helper wrapper, `Item`, visibility and writeability variants; attributes, inline/external modules, nested use trees/aliases/globs, parameters, fields, variants, generics and bounds. Distinguish source paths from resolved identities. |
| `ast/stmt.rs`, `ast/ty.rs` | Every statement/type wrapper and `Stmt` variant; binding/assignment forms, statement versus expression forms, optional components; type spelling versus resolved types. |
| `lexer.rs` | Both entrypoints, source byte offsets, retained trivia, EOF, malformed token handling and cancellation. Explain interpolation mode stack/brace depth and contextual delimiter treatment near the implementation. |
| `parser/mod.rs` | `Parse`, limits and fields/defaults, all entrypoints; tolerant versus strict results, declaration mode, source association, cancellation and exhausted-limit behavior. |
| `parser/core.rs`, `parser/grammar/{mod,expr,item,stmt,types}.rs` | Module orientation; `Parser`/`Checkpoint` state and important field invariants; cursor/trivia handling, checkpoint wrapping, precedence/associativity, contextual restrictions, recovery progress and nesting/tree-depth accounting. Link representative handlers to AST outputs. |

Use current grammar handlers and focused existing tests to verify claims. If a
kind is unused or a behavior is surprising, describe the verified state; do not
invent intended semantics. Record material discrepancies in the existing review
document, leaving behavioral fixes outside this documentation task.

## Representative source comments

For enum variants, make the symbol visible in both source and IDE hover:

```rust
/// The `::` path separator.
ColonColon,
/// An identifier, such as `total`.
Ident,
/// The `{` opening an expression hole in an interpolated string.
FormatOpen,
```

After adding macro attribute forwarding, place wrapper docs inside the invocation
so they attach to the generated type:

```rust
ast_node!(
    /// A binary operator expression, such as `total + 1`.
    ///
    /// Its direct children contain the left expression, operator token and right
    /// expression, with trivia retained. [`Self::lhs`] and [`Self::rhs`] select
    /// the first and second expression children; [`Self::operator`] reads the token.
    /// Missing operands can occur in recovered syntax.
    BinaryExpr, BinaryExpr
);
```

Add small `text` tree sketches to complex nodes where the prose is insufficient.
In the shared AST explanation, explicitly show that `BinaryExpr` stores only
`syntax: SyntaxNode`; `lhs`/`rhs` are views, not stored fields. Include one complete
and one incomplete source example to demonstrate diagnostics plus partial access.

## Execution order and acceptance

| Phase | Deliverable and acceptance |
| --- | --- |
| SD01 | Crate/module orientation and shared CST/AST storage model; macro attribute forwarding; verify a generated node's documentation renders on its type. |
| SD02 | Complete `SyntaxKind`, `TokenKind`, token fields and Rowan vocabulary. Every symbol is readable at its declaration; contextual and synthetic kinds are explained. |
| SD03 | Complete all five AST category files plus trait/support APIs. Every wrapper has a source example and logical shape; every accessor states meaningful selection/absence behavior. |
| SD04 | Complete lexer, parse APIs, limits, parser state and grammar orientation. Readers can follow source-to-tree construction and recovery from the documented entrypoints. |
| SD05 | Audit the coverage table, build the crate's docs, run the small set of documentation examples, inspect representative rendered pages and reconcile the architecture links. |

Use a few shared runnable examples: lex and inspect spans; parse and inspect a
function/expression; inspect incomplete source with diagnostics. Link simple
accessors to these examples rather than multiplying near-identical doctests.

Batch validation after coherent changes; do not rebuild documentation after each
comment. At SD05 run once, fixing and rerunning only affected failures:

```text
cargo rustdoc -p kagari-syntax --lib -- --document-private-items -D missing_docs -D rustdoc::broken_intra_doc_links
cargo test -p kagari-syntax --doc
```

The missing-docs lint checks public coverage, not private-state explanations or
documentation quality. Manually audit the table and inspect generated `SyntaxKind`,
`BinaryExpr`, `PathExpr`, `Parse` and `Parser` pages, including links and code blocks.
Run formatting on changed Rust files and the repository structure checker for the
macro checkpoint. Use content/local-link checks and `git diff --check` at checkpoints.
No workspace tests, full crate unit suite or feature matrix is required locally;
GitHub CI owns broad testing. Planning-only changes require no Cargo build.
