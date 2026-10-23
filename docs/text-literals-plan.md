# Text literals, Unicode characters and String traversal

## Status and authority

Planning baseline: `1b5da5df` (2026-10-23). Implementation and per-phase commits
are authorized. TL01 is implemented with focused local acceptance; TL02 is next.
Publication and pushes remain outside this task.
The user has selected identifier-only interpolation, Rust-style raw/multiline
literal spelling, byte literals and a distinct Unicode `char` type.

This document owns the finite TL01-TL05 sequence, decisions and progress ledger.
[Project goals](project_goal.md), [architecture](architecture.md) and
[language specifications](README.md#language-and-execution-specifications) retain
their existing authority. The contracts below are the implementation target, not
claims that these features already work. Update the relevant specifications as
each vertical slice lands; the [roadmap](implementation-roadmap.md#text-literals-and-string-traversal)
only records activation and remaining scope.

## Outcome and scope

Provide one coherent literal model, a scalar `char` that survives every checked
boundary, and explicit lazy String traversal. Preserve static typing, managed
ownership, left-to-right effects, source-free execution and pinned reload.

| Source form | Result | Escapes | Interpolation | Physical newlines |
| --- | --- | --- | --- | --- |
| `'a'` | `char` | Text/character escapes | No | Rejected |
| `b'A'` | `u8` | Byte/character escapes | No | Rejected |
| `"..."` | `String` | Text escapes | Names, optionally `:?` | Retained |
| `r"..."`, `r#"..."#` | `String` | None | No | Retained |
| `b"..."` | `[u8]` | Byte escapes | No | Retained |
| `br"..."`, `br#"..."#` | `[u8]` | None | No | Retained |

No Rust `&str`, borrowed byte slices or const-generic array type is introduced.
Byte strings construct ordinary fixed-length Kagari arrays, not immutable String
objects or a new byte-buffer family.

Non-goals: expression interpolation compatibility, `format`/dynamic templates,
triple quotes, `fr`/`rb` prefixes, C strings, lifetimes/raw identifiers, automatic
dedent, grapheme/locale/normalization APIs, direct String indexing or implicit
String iteration, character arithmetic or `Range<char>` iteration, a general
UTF-8 conversion library, Future trait work, NC/RL migrations, native backend
expansion and routine ABI/version bumps. Existing async blocks remain unchanged.

## Inspected architecture and concrete gaps

| Owner | Baseline and required responsibility |
| --- | --- |
| [Shared literal decoding](../crates/kagari-source/src/literal.rs) | Currently decodes ordinary strings, rejects physical newlines and lacks hexadecimal byte escapes and Rust Unicode-escape underscores. Own shared decoding/scalar validation here, not separate lexer/HIR interpretations. |
| [Lexer](../crates/kagari-syntax/src/lexer.rs), [expression grammar](../crates/kagari-syntax/src/parser/grammar/expr.rs) | `f` starts interpolation, holes parse arbitrary expressions, and newline recovery assumes single-line strings. Replace these rules coherently while preserving physical source offsets. |
| [HIR expression lowering](../crates/kagari-hir/src/lower/expr/mod.rs) | Text pieces currently collapse braces and synthesize quoted literals. Replace synthetic re-quoting where necessary; raw delimiters, escapes and braces must be decoded exactly once. |
| [Scalar types](../crates/kagari-types/src/scalar.rs), [builtin surface](../crates/kagari-types/src/surface.rs), [contract representation](../crates/kagari-contract/src/representation.rs) | Add semantic Char independently of IntegerType. Contract representation remains the semantic-to-physical owner; current `u32` uses physical I64, so spelling alone cannot determine layout. |
| [HIR scalar facts](../crates/kagari-hir/src/typeck/scalar.rs), [constant evaluation](../crates/kagari-hir/src/typeck/const_eval.rs) | Carry validated character constants, fixed-type byte characters and decoded literal facts into checked lowering. No syntax/type reconstruction in executable consumers. |
| [MIR instructions](../crates/kagari-mir/src/instruction.rs), [bytecode instructions](../crates/kagari-bytecode/src/instruction.rs) | Preserve scalar domains through constants, folds, pools, codecs and source-free verification. Byte payloads are not UTF-8 String payloads. |
| [Runtime scalar values](../crates/kagari-runtime/src/value/scalar.rs), [value checks](../crates/kagari-runtime/src/value_check.rs) | Char/u32 compatibility must not follow a shared machine representation. Audit slots, fields, arguments, returns and generic containers, not just literal construction. |
| [Native scalar conversion](../crates/kagari-runtime/src/native/conversion/scalars.rs) | Rust `char` is absent; add typed conversion and consistent optional Serde schema/serialization support. Host state stays outside the script heap. |
| [String declarations](../crates/kagari-stdlib/src/foundation/string.rs), [String cursors](../crates/kagari-runtime/src/gc/string_iter.rs) | Public traversal methods are absent. Bytes already declares semantic `u8` with its correct physical I64 representation; preserve that contract. CharIndices allocates one-scalar Strings and must change to Char. |
| [Iteration declarations](../crates/kagari-stdlib/src/foundation/iteration/cursor.rs), [operation contracts](../crates/kagari-contract/src/operations.rs) | Reuse cursor dispatch, GC tracing and commit rules. Add Chars and update item contracts; do not restore unrelated dormant String cursor APIs. |
| [Editor grammar](../editors/tree-sitter-kagari/grammar.js), [syntax inventory](syntax-coverage.md), [site](../site/README.md) | Each syntax checkpoint includes its affected adapters, fixtures, examples and consumer migration. Generated parsers are products of the grammar, not a second semantic authority. |

The architecture already has the necessary ownership boundaries. This work extends
those owners, rather than introducing a general literal framework or a second
formatting/iteration engine. It does require a cross-layer Char slice: adding a
source token backed by unchecked integers would leave the central safety problem
unresolved.

## Frozen target contracts

### Character domain and operations

- `char` is one Unicode scalar: U+0000 through U+10FFFF excluding U+D800 through
  U+DFFF. `'🙂'` is one char; a base letter plus a combining mark is two scalars
  and cannot occupy one character literal. No grapheme-cluster promise is made.
- Character literals decode to exactly one scalar. Byte-character literals decode
  to exactly one byte and have fixed type `u8`, independent of integer inference.
  Both reject empty/multiple values and direct LF, CR or TAB; escapes can express
  these values. Apostrophe escapes work in character literals.
- Char supports Copy/Clone according to existing scalar conventions, Eq/PartialEq,
  Ord/PartialOrd ordered by scalar value, Hash, Display, Debug and Default (NUL).
  Display produces the scalar's UTF-8 text; Debug produces a single-quoted escaped
  representation using the shared character-escape policy. Equal chars hash equally.
- Provide `From<char>` for `u32` and `String`, and `TryFrom<u32>` for `char`, with
  existing Into/TryInto adapters. Frozen error surface:
  `core::char::CharTryFromError::InvalidScalar`, covering both surrogates and values
  above U+10FFFF. This is a character-domain error, not a numeric truncation.
  The exact declaration is recorded in the builtins specification.
- No implicit char/integer conversion, arithmetic, bitwise operations, numeric
  parsing, Sum/Product or increment. Do not broaden generic numeric builtin proofs
  to make Char pass. No new `as` conversion rule is introduced.
- Support char constants, associated constants, equality/order comparisons and
  character literal/range patterns through existing checked pattern machinery.
  A range pattern does not authorize iterable `Range<char>` construction.
- Rust `char` round-trips through typed calls, fields and generic containers;
  Rust `u32` does not masquerade as Char. Optional Serde handles char explicitly.
  Persistent statics retain ordinary exact-type/reload rules; replacing char with
  u32 is not a compatible state change merely because storage widths agree.

Keep an explicit Char domain/tag through representation and value validation;
reuse the existing scalar bank where possible. Do not create a second register
bank or enlarge compact Value without a concrete layout need. Validate scalars at
untrusted artifact and host boundaries; preserve established trusted execution
checks rather than adding per-use decoding. Reject surrogates, out-of-range and
noncanonical oversized payloads in both portable and in-memory validation.

### Escape, raw delimiter and multiline rules

Use the Rust 1.90 lexical rules identified below, with Kagari result types:

- Cooked text supports the applicable quote/backslash escapes, `\n`, `\r`, `\t`,
  `\0`, ASCII `\x00` through `\x7F`, and `\u{...}` Unicode scalars. Unicode escapes
  contain one to six hex digits; underscores may follow digits, including the
  final digit, but cannot be the first item. Reject invalid scalar values.
- Cooked byte forms permit `\x00` through `\xFF`; Unicode escapes are forbidden.
  Unescaped byte text must be ASCII. `b"中文"` is an error, not implicit UTF-8
  encoding. `br"\xFF"` contains four ASCII bytes, not the byte FF.
- Raw strings use zero through 255 hashes and close at a quote followed by the
  required hashes; any further hashes belong to subsequent source tokens.
  Backslashes and braces have no special meaning inside them. A raw byte string
  applies the same delimiter rule plus ASCII validation.
- Cooked and raw strings/byte strings can span source lines. Retain indentation
  and initial/final newline; do not dedent. Normalize source CRLF to LF in the
  decoded value without rewriting the source buffer or its byte spans. Reject
  standalone CR in literal text, except as continuation whitespace below.
- In cooked strings/byte strings, backslash immediately followed by LF or CRLF is
  a continuation. Remove it and subsequent ASCII spaces, tabs, LF and CR up to
  the next non-whitespace character. Raw strings have no continuation processing.
  Ordinary `\n` escapes still produce a newline rather than starting continuation.
- Lex interpolation delimiters from source, not from decoded text: `\u{7B}`
  produces a literal opening brace and cannot start a hole. Avoid double decoding
  during HIR lowering, concatenation and formatter round-tripping.
- Bounded delimiter scanning, cancellation and diagnostics still apply to very
  long literals. A newline no longer proves a string is unterminated. Diagnose
  the actual unmatched delimiter; do not promise recovery to the next declaration
  when its text may legally belong to an unterminated multiline string.

### Identifier-only interpolation

Ordinary String literals replace the old `f"..."` form. A hole is exactly
`{name}` or `{name:?}` with no internal whitespace or other format options;
`name` is a normal value identifier, not an arbitrary expression or qualified
path. Normal name resolution admits locals, parameters, constants, accessible
statics and imported aliases. Display and Debug requirements remain statically
checked at each occurrence.

```text
val name = player.name;
val score = compute_score();
"Player {name}: {score} ({score:?})"
r#"Literal {name}; quotes: "hello""#
"Literal braces: {{ and }}"
```

Reject `{player.name}`, `{players[index]}`, `{get_player()}`, `{module::VALUE}`,
operators and blocks, with a diagnostic suggesting an outer binding. `{{` and
`}}` mean literal braces; a lone unmatched brace is invalid. Do not add `\{`,
`${...}`, `fr` or a permanent `f` compatibility reader. Raw and byte strings never
interpolate.

Evaluation remains occurrence-by-occurrence, left-to-right: read the first name,
format it once, then read/format the next name. Reading a static can initialize
it; user Display/Debug can mutate state or trap. Repeated names are not silently
captured once at string entry. Keep completed side effects on failure.

For example, in `"{first}{STATE}"`, formatting `first` can change STATE; the
second occurrence observes the subsequent value. Eagerly collecting all argument
values before formatting would violate this contract. The same rule governs
migration from old expression holes: moving all expressions into preceding
bindings is not automatically semantics-preserving. When an old expression must
run after earlier formatting, use ordered intermediate rendered strings or
explicit statements that preserve that order. Review branch/argument evaluation
as well as apparent textual order; do not perform a blanket regex rewrite.

No-hole cooked/raw strings remain literals in attributes, patterns and whatever
constant positions already permit String. Holes do not introduce runtime
formatting into const evaluation or literal-only metadata; reject them where the
existing context requires a literal. Preserve decoded-value matching for escaped
braces and raw strings. This plan does not expand String const capabilities.

### Fresh byte storage

Each evaluation of `b"..."` or `br"..."` constructs a fresh mutable `[u8]`, including
empty literals. Copying that returned array uses Kagari's existing shared-object
semantics; evaluating the literal again does not share its storage.

```text
val a = b"AB";
val alias = a;
val b = b"AB";
a[0] = b'Z';
// alias observes ZB; b still contains AB.
```

Start from checked array construction and its allocation/error rules. Immutable
payload pooling is allowed, but the runtime's cached String-constant materializer
must not cache mutable byte-array results. Do not add a specialized allocation
opcode/cache without a concrete need. Byte strings do not become scalar constants
or constant patterns simply because their source is a literal; retain existing
array const/pattern restrictions. Arbitrary payload bytes need no UTF-8 validity.

### Explicit lazy String traversal

| Method | Item | Meaning |
| --- | --- | --- |
| `text.chars()` | `char` | Unicode scalar order |
| `text.char_indices()` | `(u64, char)` | UTF-8 byte offset and scalar |
| `text.bytes()` | `u8` | UTF-8 encoded bytes |

For `"aé🙂"`, chars yields three scalars, char_indices yields offsets 0, 1, 3,
and bytes yields seven bytes. Combining marks remain separate items. `len`,
`find` and `slice` keep their current byte-based contracts. No `for x in text`
or integer String indexing is added; use an explicit traversal method.

Reuse Iterator/Iterable adapters and existing cursor state ownership. Copies of
one cursor share progress; a new method call creates independent progress. Keep
the source String traced while a cursor survives GC or async suspension. Never
retain a Rust string borrow across allocation, callback/reentry or suspension.
Preserve preview/allocation/publication/commit ordering so a failed item
publication does not advance the cursor. Chars and CharIndices no longer require
allocating a one-character String; tuple/cursor allocation remains governed by
ordinary runtime rules. This is an ownership requirement, not a measured speed claim.

## Reference mechanisms and deliberate differences

- [Rust 1.90 Reference: tokens](https://doc.rust-lang.org/1.90.0/reference/tokens.html)
  supplies character/byte validity, hexadecimal/Unicode escapes, raw delimiter
  matching, CRLF and continuation rules. Reuse that finite lexical model, not Rust
  borrowing, array-length types or a parser for the whole Rust language.
- [RFC 2795: implicit identifiers](https://rust-lang.github.io/rfcs/2795-format-args-implicit-identifiers.html)
  motivates name-only capture and ordinary scope lookup without embedding a full
  expression grammar inside format text. Kagari deliberately differs: ordinary
  strings interpolate, no format macro is required, and occurrence-by-occurrence
  evaluation follows Kagari's observable formatting contract.
- Kagari's existing checked scalar pipeline and GC cursor machinery own executable
  semantics. Rust string iterators borrow source storage; Kagari cursors instead
  trace managed source objects. Preserve physical source offsets while decoding
  CRLF rather than replacing the source buffer. Mutable byte arrays need fresh
  construction even if Rust can use immutable static byte-literal storage.

No new cache, interpreter dispatch architecture or performance target is proposed.
If implementation exposes a boundary defect, diagnose and record it here before
expanding scope; a local workaround must not create duplicate semantic owners.

## Execution sequence

Each phase is a coherent, buildable checkpoint. Enum/type-model changes and their
consumers land together, not as intentionally broken public scaffolding. No
compatibility aliases, dual readers or production feature guards for migration.
Use Conventional Commits, `!` for breaking surface changes and `Plan-Phase: TLxx`
trailers. Each completed implementation phase is committed separately.

### TL01 — Char vertical slice

1. Freeze the minimal char/protocol/conversion/error surface above in the specs.
2. Extend shared decoding, tokens/AST/HIR, builtin registration, constant and
   pattern checking, checked MIR lowering and both portable codecs/verifiers.
3. Implement explicit runtime Char values/domain checks, equality/order/hash,
   formatting/default/conversions and Rust typed/optional Serde integration.
   Review public/associated constant validators and generic builtin proofs.
4. Update affected syntax tools and examples for character literals. Keep current
   String syntax until TL02; do not advertise raw/multiline strings prematurely.
5. Keep unsupported JIT/AOT cases on established pre-entry fallback/rejection
   paths. Never reinterpret Char as a supported i32 body to claim native support.

Acceptance: scalar endpoints/NUL/supplementary values, invalid scalar and char/u32
negatives, const/pattern behavior, generic/field/container/host round-trips,
source-free constants and invalid artifacts. Include a representative persistent
static type/reload case and selected backend capability rejection/fallback case.

### TL02 — Ordinary interpolation, raw and multiline String cutover

1. Implement the shared literal-kind/delimiter/escape handling needed for String;
   replace f-mode arbitrary expressions with identifier-only ordinary-string holes.
   Carry decoded text exactly once into HIR and preserve source spans.
2. Preserve interleaved read/format semantics, Display/Debug checks, literal-only
   contexts and useful malformed-delimiter/hole diagnostics.
3. Migrate affected `.kgr`, embedded Rust test sources, examples, docs and site
   snippets. Classify outer Rust `format!`/raw strings separately from Kagari text.
   Review effectful holes manually; remove old acceptance instead of retaining f.
4. Update EBNF, syntax inventories, formatter, analyzer spans, Tree-sitter scanner/
   grammar/generated products, Zed queries and TextMate grammar as applicable.
   Formatter output must preserve literal values and be idempotent: do not indent
   multiline contents or change raw closing delimiters unsafely.

Acceptance: name resolution and trait errors, rejected expressions/old prefixes,
escaped/decoded braces, multiline/raw delimiter boundaries, CRLF/continuations,
Unicode escapes, ordered static reads/formatting/traps and unchanged source spans.
All affected old-syntax fixtures migrate in this checkpoint, not in TL05.

### TL03 — Byte character and byte-string slice

1. Add b/br recognition using the shared delimiter/decoder mechanisms from TL02;
   enforce ASCII source text, byte escapes and single-byte character cardinality.
2. Carry `b'A'` as checked u8 and byte strings as checked array construction.
   Update constant payload/codec handling only where the chosen lowering needs it;
   keep byte payloads distinct from text and mutable results distinct from pools.
3. Update affected editor/tooling support, literal docs and examples together.

Acceptance: ASCII/escape limits, forbidden Unicode, raw backslashes, embedded NUL
and FF, multiline/CRLF, fixed u8 typing, source-free execution and repeated-literal
freshness including empty arrays. Test shared aliases separately from fresh
literal evaluations; neither behavior may be optimized away.

### TL04 — String traversal on shared cursor machinery

1. Add Chars and revise CharIndices item contracts across declarations, operation
   validation, compiler consumption and runtime publication. Expose and verify
   Bytes' existing semantic u8 contract without changing its I64 representation.
2. Register the three explicit methods through installed standard-library owners;
   integrate with existing Iterable adapters without eager intermediate Lists.
3. Replace single-scalar String production, retain tracing and preview/commit
   semantics, and document byte versus scalar positions.

Acceptance: `aé🙂`, empty/NUL/combining-mark inputs, Item typing, adapter composition,
shared/independent progress, exhaustion, GC retention and a representative retained
cursor across suspension. Exercise failed publication/cleanup at the existing
runtime contract owner rather than adding a second test harness.

### TL05 — Focused integration and documentation handoff

1. Audit changed responsibilities, boundary validation and consumer migrations;
   remove obsolete helpers and duplicate tests only where their contracts are
   actually replaced. Regenerate impacted disposable fixtures at this checkpoint
   if not needed earlier; never keep required source-free coverage stale.
2. Run the selected integration/regression set and lightweight acceptance checks.
   Record actual commands/results and unavailable editor/native tooling distinctly.
3. Consolidate lasting behavior in syntax/types/stdlib/execution/artifact/embedding
   specs, architecture where ownership changed, editor guide and site examples.
   Move unresolved out-of-scope findings to the existing review/roadmap owner.
4. Retire this plan once implementation/local acceptance are complete and ongoing
   CI acceptance is recorded in the roadmap. Git history retains the phase ledger.

No full-workspace local test run, full feature/backend sweep, publication or native
coverage expansion is part of TL05.

## Validation selection and build policy

Reuse existing contract owners; add cases only for uncovered rules/boundaries.
Choose exact package/target/test-name filters after inspecting the phase's tests,
record them in the ledger, and use `--no-fail-fast` for multiple selected targets.
Do not invent test names or treat compile failure/skipped cases as passing tests.

| Changed boundary | Existing owner / selected evidence |
| --- | --- |
| Literal grammar/decoding | Source literal tests; syntax lexer/interpolation tests; malformed input, delimiter bounds and original spans |
| Type/const/protocol contracts | HIR scalar/const/associated-constant tests; contract primitive proof tests; SDK language standard-traits cases |
| Executable serialization | MIR wire tests; bytecode canonical/identity/verification tests; a frontend-free artifact consumer |
| Runtime values/embedding | SDK typed-call and language associated-constant/array cases; native scalar and optional native-schema Serde tests |
| String/cursors | SDK string-methods/iteration-traits cases; runtime String cursor publication and GC tests |
| Source tools | Focused formatter/analyzer cases; Tree-sitter corpus/generated consistency and native Node capture tests; affected Zed/TextMate assets |
| Integration | Representative literal/interpolation execution and static reload; pre-entry native fallback/rejection; changed site/source-example checks |

At implementation checkpoints run affected tests plus:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Review imports, ownership, re-exports, effective LOC and manual macro tokens as
required by repository rules. No blanket structural exceptions. Use workspace
profiles, default target/parallelism and ignored `target/` for temporary products.
Do not bump format/ABI identifiers routinely for this unpublished change; refresh
affected development artifacts instead. Retain meaningful source-free coverage.

Full-workspace tests and the exhaustive feature/backend/platform matrix are
**CI-only**. Local focused success does not establish CI acceptance. Previously
passing checks remain evidence for unaffected work; rerun failing/affected cases
after batched fixes rather than restarting the entire validation sequence.

Known baseline tooling limits: the Windows native Tree-sitter Node addon is absent
for Node 24.15.0; the observed node-gyp 11.5/VS 18 combination is unsupported. The
observed local WASI SDK is 25 while editor checks require 34. Recheck availability
when TL02 starts; do not claim CLI corpus success proves native Node/WASM tests.
Record blocked local acceptance and required CI evidence; do not silently replace
pinned tooling or mark blocked tests passed.

## Progress ledger

- [x] Inspect baseline owners and record selected scope, target contracts and phases.
- [x] TL01: Char vertical slice and focused acceptance.
- [ ] TL02: String syntax/interpolation cutover and consumer/tool migration.
- [ ] TL03: Byte literals and fresh array semantics.
- [ ] TL04: Lazy String traversal.
- [ ] TL05: Focused integration, lasting documentation and plan retirement.

Planning checkpoint: documentation only; no implementation or phase commit made.
Focused content review and checks of all 26 local links/anchors in this plan and
its roadmap entry passed. Both changed documents retain CRLF; whitespace checks
and `git diff --check` passed. Review confirmed that Bytes' physical I64 is the
existing correct representation of semantic u8, not a defect to migrate.
Rust builds, tests and Clippy were not run for this documentation-only task.
Full CI acceptance remains unrun locally.

### TL01 checkpoint

Implemented distinct Char throughout source facts, types, physical contracts,
MIR/bytecode, runtime storage/GC, typed host/Serde and standard-library protocols.
Canonical constants validate the Unicode domain; comparison/range-pattern lowering
uses Char rather than numeric storage. Declared char conversions no longer enter
the numeric From specialization. Added character editor grammar and lasting specs.

Focused local acceptance passed:

- Source/syntax/types character and adjacent token/interpolation cases; HIR and
  contract `cargo test -p kagari-hir -p kagari-contract --lib char --no-fail-fast`
  (7 tests). MIR/bytecode char codecs, pool identity and disassembly cases passed.
- Compiler `portable_program_codec_rebuilds_seals_and_preserves_canonical_lowering`
  and Cranelift unsupported-before-entry checks passed, including char range facts.
- `cargo test -p kagari --lib char_ --no-fail-fast`: 4 tests, covering protocols,
  conversions, rejected numeric capabilities and persistent static char state/type
  rejection across source and artifact reload. Source/artifact/JIT-fallback routes
  in the standard-trait owner retain separate boundary coverage.
- `cargo test -p kagari-runtime --features serde --lib --test native_schema char_
  --no-fail-fast`: 8 tests covering storage/domain checks and host/Serde containers.
- Analyzer character editing and stdio Unicode fixture passed; formatter character
  tests (2), syntax audits (5), Tree-sitter character corpus (15) and affected
  string/comment cases (7), TextMate/manifest tests (20), generated consistency and
  CLI execution of Zed queries passed. Native Node/WASM acceptance remains blocked
  by the baseline toolchain limitations above, not inferred from CLI checks.
- Structure, workspace formatting, workspace/all-target Clippy and diff checks
  passed after integration repairs. Scoped IDE build requests timed out; successful
  Cargo tests/checks provide build evidence instead.

Initial parallel builds caused lock waits; selected tests were completed serially.
An anonymous-symbol LNK2019 in bytecode was resolved by cleaning only that package
and rerunning the affected tests. No linker source workaround or broad clean was
used. No selected Rust integration failure remains. Exhaustive feature/backend/
platform/workspace tests remain CI-only. No measured performance claim is made.
