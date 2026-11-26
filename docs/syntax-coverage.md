# Syntax Coverage Audit

`docs/kagari.ebnf` describes the intended source grammar. The hand-written
parser and lexer may lag behind it. This audit makes that difference visible
without treating a successfully parsed example as proof of complete grammar
coverage.

Run the audit from the repository root:

```sh
cargo test -p kagari-syntax grammar_coverage -- --nocapture
```

The audit maintains three inventories:

- [`syntax-coverage.tsv`](syntax-coverage.tsv) has one row for every EBNF rule.
  `covered` means at least one named example parses cleanly; `partial` means a
  witness exists but a documented form is missing; `missing` points to a
  rejected-source regression case; `unverified` means there is no focused
  witness yet. A `covered` rule can still have untested alternatives.
- [`syntax-branches.tsv`](syntax-branches.tsv) records each top-level `|`
  alternative of source-language rules. `witnessed` rows name a parse-clean
  example, `missing` rows name a rejection case, and `unverified` rows require
  review. The production text is checked against the EBNF, so reordering or
  changing an alternative requires updating its row.
- [`syntax-quantifiers.tsv`](syntax-quantifiers.tsv) numbers every source-language
  optional or repeated position (`?`, `*`, `+`). It is a change-review gate;
  the current audit does not claim that both sides of each optional or every
  repetition count have test witnesses.

Module-level transparent type aliases have parse witnesses in
[grammar-witnesses.kgr](../examples/syntax/grammar-witnesses.kgr), including
visibility, attributes, generics, either where-clause position, compound targets
and inline-module placement. They require `=` and remain distinct from trait/impl
associated types and explicit Native declarations. Formatter and editor tests cover
layout and lexical structure; parse coverage does not establish alias expansion,
constraint checking or semantic navigation.

Type-parameterized associated declarations and definitions, including their
input/output bounds and qualified member arguments, have an executable witness
in [generic-associated-types.kgr](../examples/syntax/generic-associated-types.kgr).

Async function/closure/block and postfix-await grammar has a parse witness in
[async.kgr](../examples/syntax/async.kgr). Executable acceptance belongs to the SDK
async contracts: explicit closure inference, ordinary-closure rejection, nested
calls, generic Fn coercion and ordinary for-body suspension. The
[async_tasks script](../crates/kagari/examples/async_tasks/script.kgr) also runs
as a decoded artifact in a standalone consumer without source compilation, including
an async block constructed by an ordinary closure factory. Block capture, return,
propagation, loop boundaries, coldness and nested Future outputs have focused
syntax/HIR/SDK coverage. No async iterator or custom awaitable grammar is implied.

Named block-local functions have an executable witness in
[callables.kgr](../examples/syntax/callables.kgr): forward declaration lookup,
explicit parameters and a returned local callable. Focused syntax/HIR tests cover
nested owners, attributes/async declarations, malformed recovery, shadowing and
capture rejection. Formatter/lint/navigation, source-free artifacts, async execution,
error traces, GC/cancellation/depth cleanup and retained reload generations have
separate focused tests. Parse coverage alone does not establish those contracts.

Module-only `static val`/`static var` declarations have parse witnesses in
[grammar-witnesses.kgr](../examples/syntax/grammar-witnesses.kgr), including visibility
and inline-module placement. Focused rejection cases cover local/associated forms,
missing type/initializer and declaration patterns. The
[persistent-state host example](../crates/kagari/examples/persistent_static.rs)
executes initialization and reload; grammar coverage alone does not establish these
runtime semantics. HIR navigation/completion/usage facts and initializer lints use
the same checked body owners; editor semantic LSP features remain separately deferred.

The ordinary source rules and top-level alternatives have parse-clean example
witnesses. Explicit Native declarations are additionally checked by the
[syntax declaration fixtures](../crates/kagari-syntax/tests/fixtures/declarations/)
and their structural AST tests. Their inventory rows remain partial/unverified
for the example-only audit: those fixtures are not executable project-source
examples. This is parser coverage, not an assertion that every form has linked
runtime behavior. [`grammar-witnesses.kgr`](../examples/syntax/grammar-witnesses.kgr)
is parser-only because it combines unrelated grammar forms. Inline modules and
wildcard imports also have executable examples and artifact tests in
`cargo test -p kagari --lib -- runtime::tests::language::syntax_examples::`
and `cargo test -p kagari --lib -- engine::tests::source_modules::`.

The tests also extract quoted source terminals from the EBNF and compare them
with the lexer. The current unrecognized terminal baseline is empty.
Negative source cases keep remaining gaps visible until implemented. If a gap becomes accepted,
its negative case fails so the inventory and positive example must be updated
together.

When changing syntax, update the EBNF, reuse or update an existing parser case or
executable example, and update the affected inventory rows. Add a witness only
for a previously uncovered grammar rule or alternative. Keep `unverified` when
the available example only happens to parse but does not demonstrate the
particular branch. The parser's typed AST/lossless lexical table, semantic
analysis, artifact loading and runtime execution require their own tests; this audit measures their
source-grammar inputs only.

Character literals have a source witness in
[char-literals.kgr](../examples/syntax/char-literals.kgr), including supplementary
scalars, escaped controls, ASCII hexadecimal escapes, Unicode-escape underscores,
and character/range patterns. These lexical additions do not add source-language
quantifier positions; `syntax-quantifiers.tsv` is unchanged. The shared decoder in
`crates/kagari-source/src/literal.rs` owns exactly-one-scalar and scalar-range
validation. Tree-sitter/TextMate recognize lexical shape for editing, not the
complete semantic domain (for example, `\\u{D800}` still needs a compiler error).
Formatter coverage checks spelling preservation and idempotence; analyzer coverage
only establishes document synchronization, not semantic highlighting or diagnostics.
Ordinary String interpolation has an executable witness in
[string-interpolation.kgr](../examples/syntax/string-interpolation.kgr). A hole is
exactly `{name}` or `{name:?}`; expressions, whitespace, comments, other format
options and the old `f` prefix are rejected. Doubled braces are literal text;
Unicode-escaped braces never become holes. The source-language quantifier ledger
requires at least one hole for `interpolated_string`; no-hole forms remain literals.

[literals-and-comments.kgr](../examples/syntax/literals-and-comments.kgr) witnesses
raw Strings, multiline contents, continuations and extended text escapes. Raw
strings use 0..255 hashes and do not interpret escapes or holes. Literal CRLF
normalizes only in decoded values: retained source text and physical byte/UTF-16
positions do not change. Formatter tests preserve complete literal spelling and
idempotence even when layout requests a different newline style. Analyzer tests
retain malformed edits and old snapshots without claiming semantic diagnostics.
Editor lexical shape checks are not scalar validation.

[byte-literals.kgr](../examples/syntax/byte-literals.kgr) witnesses `b'A'` (`u8`),
`b"..."` and `br#"..."#` (`[u8]`). Direct byte contents are ASCII; cooked byte
escapes allow `\\x00` through `\\xFF` but reject `\\u{...}`. Braces never open
interpolation in byte forms. Raw byte strings share the 0..255 hash delimiter
rule and retain literal backslashes. Multiline/CRLF and cooked continuations
share the String rules above. These lexical rules add no source-language
quantifier positions. Formatter tests retain byte spelling across newline options;
analyzer synchronization retains even invalid non-ASCII byte edits verbatim.
Tree-sitter/TextMate tests distinguish byte regions, escapes and following code.
Expression `literal` includes byte strings; `scalar_literal` excludes them in
patterns and attributes, including nested attribute arrays. Byte characters remain
scalar literals, witnessed by singleton/or patterns; this does not extend the
existing i32/char range-pattern domains to u8. Tree-sitter negative cases cover
cooked/raw byte strings in patterns and direct/nested attribute values.
Fresh array allocation and fixed `u8` typing require runtime/type tests, not merely
this parse witness.

## Comparison with stable Rust syntax

This is a review of script-facing constructs, **not** a Rust compatibility goal or an
implementation checklist. The comparison includes the P0/P1 follow-up to `341662b37` and uses the [Rust 1.90 Reference](https://doc.rust-lang.org/1.90.0/reference/),
Edition 2024, as used by the [Kagari syntax spec](spec/syntax.md). Rust compiler source alone
is not evidence of stable syntax. Exclude nightly features and Rust-specific ownership,
borrows, lifetimes, raw pointers, unsafe layout/FFI and threading contracts. Compare language
forms, not standard-library API coverage. The EBNF describes intended syntax; HIR admission
and execution are separate. Equivalent spellings and deliberate differences are not gaps.

### Declaration and type gaps

- **Where clauses:** ordinary struct, enum and trait headers accept trailing predicates, as do
  functions, methods, impls and aliases. Trait headers place `where` after any supertraits.
  A bound on `Box<T>` is not a bound on `T`; the [header fixture](../crates/kagari-hir/tests/fixtures/typeck/traits/header-predicates.kgr)
  exercises both concrete application and inherited predicates. Source publication preserves them.
- **Data declarations:** named-field and empty braced structs exist, but not tuple structs
  (`struct PlayerId(i32);`) or unit structs (`struct Ready;`). Enum variants can be unit,
  positional or [named-field](../examples/syntax/named-variants.kgr); explicit discriminant
  numbers (`Ready = 1`) are not supported.
- **Generics and interfaces:** no default type parameter (`T = DefaultType`), const generic
  parameter/argument, array length in the type (`[T; N]`), or `impl Trait` parameter/return
  syntax. A named generic covers the usual parameter case. An opaque return means *one hidden
  concrete type*; it is not the same as returning an interface value. Associated types,
  type-parameterized associated types (GATs), qualified projections, associated output equality
  and supertraits already exist. `R: Reader` plus `where R::Item: Eq` can express a constraint
  without Rust's inline `Reader<Item: Eq>` spelling. See [trait contracts](spec/traits.md).
- **Members and constants:** inherent and trait-associated constants exist. A qualified
  `<Limits as Trait>::MAX` selects the trait member, independently of `Limits::MAX`. Ordinary async functions, closures and
  blocks exist; general async trait methods do not have an executable contract. Module `const`
  is not general Rust const evaluation: no user `const fn`, inline `const { ... }` block or
  general compile-time evaluation of aggregates. These need semantics, not just parser tokens.

### Statements, expressions and patterns

- **Binding and early exit:** `if val Some(x) = e` and `while val ...` cover one pattern test.
  `val PAT = e else { ... };` and `var` support an early exit when the pattern fails; the
  failure block must not fall through. No Edition 2024 let-chain (`if let ... && let ...`).
  `(x, y) = pair` assigning to *existing* slots is distinct from binding and remains unsupported.
- **Control flow:** blocks, `if`, `match` and expression-position `loop` can yield values;
  `val n = loop { break 42; };` is not missing. No loop labels, labeled blocks or targeted
  `break`/`continue`. `return`, `break`, `continue` and assignment are statements, not general
  expressions: write `None => { return 0; }`, not `None => return 0`. Named local functions
  exist; there is no general block-local item scope for `struct`, `type`, `use` or `impl`.
- **Patterns:** tuple/field destructuring, alternatives, guards, record rest (`Point { x, .. }`),
  [positional rest and signed literal patterns](../examples/syntax/positional-patterns.kgr),
  and two-ended ranges exist. `whole @ Some(_)`, array patterns and one-ended range patterns
  remain separate candidates. Ordinary function/closure parameters are names, not general patterns; destructure
  inside the body. Closure parameters support type annotations but not `|x| -> R { ... }`.
  See the [pattern grammar](../crates/kagari-syntax/src/parser/grammar/patterns.rs).
- **Exhaustiveness is not syntax.** Kagari rejects `match s { State::Ready => 1 }` for
  `enum State { Ready, Waiting }` during type checking. Its [coverage checker](../crates/kagari-hir/src/typeck/body/joins/exhaustiveness.rs)
  also distinguishes payload cases and guards; this is not general exhaustiveness checking
  for every possible scrutinee type.

### Modules, names and metaprogramming

- `mod`, grouped/glob imports, `as` aliases, `pub use`, `pub(super)` and `pub(crate)` exist.
  No `pub(in ancestor_path)` restriction; private items already cover the meaning of `pub(self)`.
  Identifiers are ASCII; no Unicode identifiers or Rust raw identifiers such as `r#type`.
  Built-in integers stop at 64 bits: no `i128`/`u128` or pointer-sized integer types.
- Outer attributes and compiler-recognized roles are not a user macro system. No `macro_rules!`,
  general `name!(...)` expansion, user procedural macros/derive, general Rust-style conditional
  compilation or inner `#![...]` attributes. These are independent tooling/expansion decisions,
  not requirements for the existing Native declaration or Host provider boundary.

### Similar-looking features not to count as missing

- `val`/`var` control rebinding, not deep object immutability. `[T]` is a GC array whose length
  is fixed at allocation, not a Rust slice. `[value; n]` need not encode `n` in the type.
  Tuple member access is `pair[0]` rather than `pair.0`; struct field shorthand and `..base`
  updates already exist. Bitwise operators, shifts and `as` scalar conversion also exist.
- Bare trait names can denote interface values; scripts do not need Rust's `dyn` spelling.
  Kagari's `fn(A) -> R` can carry a closure environment, unlike a Rust function pointer.
  `?`, `.await`, and `for` exist; the applicable protocols/providers differ. In particular an
  unavailable [bundled range iterator](spec/builtins.md) is not a missing `for` production.
- Same syntax need not mean same evaluation: Kagari evaluates the root/index of `a[i()] = rhs()`
  before the RHS; Rust ordinary assignment evaluates the RHS first. Preserve the documented
  [mutation contract](spec/value-semantics.md) rather than treating this as a parser fix.

Rust references by topic: [items and generics](https://doc.rust-lang.org/1.90.0/reference/items/generics.html),
[types](https://doc.rust-lang.org/1.90.0/reference/types.html),
[statements](https://doc.rust-lang.org/1.90.0/reference/statements.html),
[expressions](https://doc.rust-lang.org/1.90.0/reference/expressions.html),
[patterns](https://doc.rust-lang.org/1.90.0/reference/patterns.html) and
[visibility](https://doc.rust-lang.org/1.90.0/reference/visibility-and-privacy.html).
