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

All inventoried EBNF rules and top-level alternatives have parse-clean
witnesses. This is parser coverage, not an assertion that every form has linked
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
particular branch. The parser's Rowan CST, semantic analysis, artifact loading,
and runtime execution require their own tests; this audit measures their
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
String syntax remains unchanged in TL01: no byte/raw/multiline additions and no
removal of `f` interpolation.

Interpolated strings and their lexer modes have an executable witness in
[string-interpolation.kgr](../examples/syntax/string-interpolation.kgr). Focused
parser tests cover nested interpolation, Unicode escapes, malformed input and
nesting/cancellation limits. Expression holes retain their original source spans.
