# kagari-syntax: From Source Text to a Recoverable Syntax Tree

This is an architecture review sample for the current implementation, inspected
at commit `6c19c00`. It explains the crate's behavior and boundaries before any
proposed redesign. The [workspace architecture](../architecture.md) describes
the wider pipeline; the [syntax specification](../spec/syntax.md) and
[grammar](../kagari.ebnf) define the language.

## 1. Purpose and place in the system

`kagari-syntax` converts one source file into a tree that preserves its text,
together with syntax diagnostics. It also provides structured views for reading
declarations, expressions, statements and type spellings from that tree.

Its output answers **what was written and how it is grouped**. HIR subsequently
determines what names mean, whether types agree and whether the program can be
compiled. A diagnostic-free syntax tree is not permission to execute code.

```mermaid
flowchart LR
    Source["Caller-owned source file"] -->|"text, mode, limits, cancellation"| Syntax
    Syntax["kagari-syntax"] -->|"tree and diagnostics"| HIR["kagari-hir: analysis and tooling"]
    Syntax -->|"declaration tree and diagnostics"| Build["kagari-abi build tooling"]
    HIR -->|"checked source meaning"| Compiler["kagari-compiler"]
    Build -->|"generated standard declaration metadata"| ABI["kagari-abi"]
```

Arrows show data flow, not Cargo dependencies. The ABI consumer runs at build
time; executable runtime contracts do not invoke the parser.

| Owned here | Owned elsewhere |
| --- | --- |
| Recognizing tokens and their byte ranges | Reading files, source identity and revisions |
| Grammar, grouping and operator precedence | Module discovery, import resolution and name binding |
| Preserving whitespace, comments and erroneous text | Type checking, generic solving and constant evaluation |
| Syntax diagnostics, recovery and parser limits | Deciding whether analysis is valid for code generation |
| Structured access to syntax nodes | MIR, bytecode, execution, GC and hot reload |

## 2. Inputs and outputs

The caller supplies a `kagari_common::SourceFile`. Its text is already valid UTF-8
because it is a Rust string. No grammar or semantic validity is assumed.
Token and diagnostic spans use byte offsets into that text. Syntax does not
attach file/revision identities to its result; the caller keeps that association.

| Input | Meaning |
| --- | --- |
| Source text | One complete file per parse, including unfinished or malformed source |
| Mode | Ordinary source, or offline declarations with additional declaration forms |
| `ParseLimits` | Diagnostic, recursive grammar nesting and completed tree depth limits |
| `CancellationToken` | A caller-controlled request to abandon work |

| Output | Guarantee and limitation |
| --- | --- |
| Token buffer | Token kinds and source ranges, including trivia, unknown tokens and EOF; no name or type facts |
| `Parse` | Owns a Rowan green tree and diagnostics; can represent erroneous or limit-exhausted input |
| AST view | Structured access to the same tree; optional children may be missing after recovery |
| `Cancelled` | No parse result is published by the controlled parse entrypoint |

The public entrypoints express different acceptance policies:

| Entrypoint | Result policy |
| --- | --- |
| `lex` / `lex_with_cancellation` | Tokenize only; the controlled version can return `Cancelled` |
| `parse` | Ordinary source with default limits and a fresh cancellation token; returns tree plus diagnostics |
| `parse_with_cancellation` / `parse_with_limits` | Ordinary source with caller cancellation, optionally caller limits |
| `parse_declarations` | Same parser with declaration mode enabled, caller limits and cancellation |
| `parse_module` | Convenience wrapper: returns the AST only when diagnostics are empty; otherwise returns diagnostics |

Despite its name, `parse_module` does not load or link modules. It uses default
limits and does not accept caller cancellation.

Declaration mode accepts forms such as `pub fn len<T>(value: [T]) -> usize;`
and opaque top-level type declarations used by the standard library. It shares
the lexer, tree model and recovery machinery with ordinary source parsing.
Its result still carries no executable validation seal.

## 3. Internal responsibilities

The implementation has four working parts and a shared syntax vocabulary.

| Part | Responsibility | Handoff |
| --- | --- | --- |
| Lexer | Scan characters, recognize literals/comments and track interpolation modes | Ordered token buffer |
| Parser core | Own cursor, tree builder, diagnostics, limits and recovery primitives | Controlled operations for grammar handlers |
| Grammar handlers | Recognize items, types, statements and expressions; construct their grouping | Nodes and tokens written directly into the tree builder |
| AST views | Expose accessors such as function name, parameters and body | Optional children and iterators over syntax nodes |
| Token/tree vocabulary | Define lexical kinds, tree kinds and their mapping | Shared representation used by the other parts |

The parser uses recursive descent with expression precedence levels and iterative
operator/postfix chains. It builds the concrete syntax tree (CST) directly; there
is no separate parser-event replay stage. The CST includes punctuation, comments,
whitespace and error nodes. The abstract syntax tree (AST) API is a set of views
over that CST, not a second independently allocated semantic tree.

Two decisions illustrate why the lexer and parser have separate responsibilities:

- String interpolation needs lexical modes for text and expression holes; the
  lexer keeps those modes on an explicit stack.
- Adjacent angle tokens can mean a shift in an expression or generic delimiters
  in a type. Expression parsing joins the relevant adjacent tokens; type parsing
  keeps delimiters separate. Neither requires resolving a type or a name.

## 4. Main flow

```mermaid
flowchart TD
    Input["Source file, mode, limits, cancellation"] --> Lex["Lex the complete text"]
    Lex --> Tokens["Tokens: kinds and byte ranges"]
    Tokens --> Init["Create per-call parser and SourceFile root"]
    Init --> Read["Read next item and nested grammar"]
    Read --> Choice{"Grammar outcome"}
    Choice -->|"recognized"| Build["Append nodes and original token text"]
    Choice -->|"syntax error"| Recover["Record diagnostic; recover with available structure"]
    Choice -->|"limit exhausted"| Suffix["Stop grammar work; retain remaining tokens in an Error node"]
    Build --> More{"More input?"}
    Recover --> More
    More -->|"yes"| Read
    More -->|"no"| Finish["Close root and finish tree"]
    Suffix --> Finish
    Finish --> Check{"Final cancellation check"}
    Check -->|"clear"| Result["Parse: tree and diagnostics"]
    Check -->|"cancelled"| Cancel["Return Cancelled; discard result"]
    Lex -.->|"cancellation observed"| Cancel
    Read -.->|"cancellation observed: unwind and finish"| Check
    Result --> View["Create AST views when requested"]
```

Solid arrows show the normal and recoverable flow. Dashed arrows summarize
cancellation: lexing can exit early, while parsing stops grammar work and closes
its builder before the entrypoint rejects the result. Limits retain a lossless
suffix; cancellation does not promise a partial tree to the caller.

For example, consider:

```kagari
fn total() -> i32 {
    1 + 2 * 3
}
```

1. Lexing identifies keywords, identifiers, numbers, punctuation and trivia,
   retaining their source ranges.
2. Item parsing recognizes the function, return type spelling and body.
3. Expression parsing groups the body as `1 + (2 * 3)` according to precedence.
4. The returned tree still reproduces the original spacing and line breaks.
5. AST accessors expose the function and its expression structure to HIR.

Syntax does not compute `7`, prove that `i32` is valid or check the return type.
If the final `}` is missing, parsing can still return the function structure and
an expected-block-end diagnostic. Tolerant analysis can inspect that partial
structure; a strict caller must reject its diagnostics.

## 5. State, ownership and lifetime

```mermaid
flowchart LR
    Caller["Caller"] -->|"owns"| Source["SourceFile and text"]
    Caller -->|"controls"| Cancel["Cancellation token"]
    Parser["Per-call Parser"] -.->|"temporarily borrows"| Source
    Parser -->|"owns"| Work["tokens, cursor, builder, diagnostics, limits and counters"]
    Parser -->|"moves finished tree and diagnostics into"| Parse["Parse result"]
    Parse -->|"retains"| Tree["immutable green tree storage"]
    AST["AST view"] -->|"retains through Rowan node handles"| Tree
```

Arrows here mean ownership or retention, not control flow. Parser mutation is
local to a call and uses `&mut Parser`. Its token buffer, cursor and builder are
not shared with callers. No host callback or execution reentry occurs during
parsing, and the crate's handwritten production source has no explicit `Rc` or
`RefCell` usage. Rowan manages tree sharing internally; this is not a claim that
the dependency contains no interior mutability.

The result does not borrow the input string. AST views can remain alive after
the `Parse` wrapper or original `SourceFile` is dropped. File identity and
revision bookkeeping still require caller-owned information. Dropping temporary
parser state or the final retained tree releases ordinary Rust-owned resources;
there are no script roots, external registrations or commit/rollback actions.

Syntax itself keeps no cross-call cache and exposes no incremental reparse
operation. HIR's analysis database owns reuse decisions and can reuse a prior
parse for an unchanged file revision. A newly parsed file goes through full
lexing and parsing.

## 6. Failure and resource boundaries

| Condition | Observable behavior | Caller responsibility |
| --- | --- | --- |
| Unknown token or malformed grammar | Diagnostics and recoverable structure; unknown input can appear in error nodes | Preserve diagnostics; tolerate missing children in tooling |
| Diagnostic budget exhausted | Stop recovery and preserve the unparsed suffix; add a limit diagnostic | Treat this as incomplete analysis |
| Nesting or tree depth exhausted | Stop further grammar growth, record the limit and preserve remaining text | Reject executable use; adjust policy only deliberately |
| Cancellation observed | Controlled entrypoint returns `Err(Cancelled)` | Discard the attempt rather than publish it as a successful analysis |
| Internal invariant violation | Some paths still use `expect`; raw Rowan kind conversion uses `unsafe` | Do not interpret syntax recovery as a universal panic/safety guarantee |

Defaults are 256 ordinary diagnostics, 64 simultaneously active recursive
grammar entries and a completed CST depth threshold of 128. A limit diagnostic
is additional to the ordinary diagnostic budget. A zero diagnostic budget still
allows valid source. Tree depth is checked when nodes complete, so the threshold
is a stop condition, not a promise that every returned recovery tree fits that
exact depth.

These limits are not a total memory or CPU budget. Lexing materializes the entire
token buffer before parser limits apply, and retaining an unparsed suffix still
requires storage proportional to that input. Raising recursion limits can also
weaken stack protection. Source-size admission policy belongs at the caller
boundary; this crate currently has no explicit source-byte or token-count limit.

## 7. Architecture assessment

**The current responsibility split is coherent for source parsing.** Local
mutable parser state, immutable output storage and separate semantic analysis
give it a much narrower ownership problem than the execution runtime. Sharing
one grammar implementation between tooling and compilation also avoids two
different definitions of ordinary source syntax.

The following are review questions, not implemented changes or claims that every
listed concern has a demonstrated failing program:

| Observation | Architectural consequence | Review direction |
| --- | --- | --- |
| `Parse` can contain errors and does not encode the selected parse mode | Consumers must retain diagnostics and know whether they requested offline declarations | Check that every path to executable compilation applies the appropriate semantic gate |
| Public token and Rowan node APIs expose more than `Parse` and AST views | Callers can become coupled to representation; raw kind conversion assumes valid Kagari kinds | Review which construction APIs need to be public and how their invariants are enforced |
| Parser limits begin after complete tokenization | Deep input is bounded more directly than very wide input | Review source admission across callers before claiming bounded memory use |
| Grammar recovery is spread across item/type/statement/expression handlers | A new grammar branch can affect progress, diagnostics and text preservation | Keep malformed-input, cancellation and lossless recovery checks alongside grammar changes |

There is no evidence here that replacing the parser with a new abstraction would
improve its boundary. The next architecture walkthrough should follow its output
into HIR: how partial syntax becomes recoverable facts, and where those facts
become checked input for code generation.

## 8. Evidence and maintenance

These links allow implementation verification without making source reading a
prerequisite for the preceding explanation.

| Contract | Evidence |
| --- | --- |
| Entrypoints, modes, limits and result ownership | [Parser facade](../../crates/kagari-syntax/src/parser/mod.rs) |
| Tokenization and interpolation state | [Lexer](../../crates/kagari-syntax/src/lexer.rs) |
| Recovery, cancellation polling and depth accounting | [Parser core](../../crates/kagari-syntax/src/parser/core.rs) |
| Syntax views and Rowan bridge | [AST views](../../crates/kagari-syntax/src/ast/mod.rs), [node adapter](../../crates/kagari-syntax/src/syntax_node.rs) |
| Partial trees and syntax diagnostics | [Error tests](../../crates/kagari-syntax/src/tests/parser/errors.rs) |
| Lossless limits and cancellation | [Limit tests](../../crates/kagari-syntax/src/tests/limits.rs), [cancellation tests](../../crates/kagari-syntax/src/tests/cancellation.rs) |
| Offline declarations | [Declaration tests](../../crates/kagari-syntax/src/tests/declarations.rs), [ABI build consumer](../../crates/kagari-abi/build/main.rs) |
| Caller-owned parse reuse and HIR lowering | [Analysis queries](../../crates/kagari-hir/src/analysis/declaration_queries.rs) |
| Grammar coverage and its limits | [Coverage audit](../syntax-coverage.md) |

For changes to syntax behavior, run `cargo test -p kagari-syntax`; grammar changes
also require the coverage workflow linked above. This documentation sample was
checked against implementation and existing test assertions; it does not report
a new test run or a full panic/soundness audit.
