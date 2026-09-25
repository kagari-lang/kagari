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

The tests also extract quoted source terminals from the EBNF and compare them
with the lexer. The current unrecognized terminal baseline is `@`.
Negative source cases keep remaining gaps, including attributes and pattern alternatives,
visible until implemented. If a gap becomes accepted,
its negative case fails so the inventory and positive example must be updated
together.

When changing syntax, update the EBNF, add a focused parser case or executable
example, and then update the affected inventory rows. Keep `unverified` when
the available example only happens to parse but does not demonstrate the
particular branch. The parser's Rowan CST, semantic analysis, artifact loading,
and runtime execution require their own tests; this audit measures their
source-grammar inputs only.
