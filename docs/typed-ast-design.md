# Strongly Typed AST Replacement

Status: TA1–TA3 integrated on 2026-11-05; TA4 local acceptance in progress.
The production parser and consumers use the strongly typed AST. This document
owns its finite migration, performance contract and progress ledger; the
[roadmap](implementation-roadmap.md) owns activation. It does not activate a wider
HIR/compiler/runtime rewrite or the GC integration plan.

Execution branch: `refactor/typed-ast`. TA1 through TA4 execute continuously under
the current goal, with complete CI explicitly handed off after local acceptance.

## 1. Problem and scope

Replace Rowan-backed positional AST views with a directly constructed, strongly
typed, lossless and recoverable AST. A reader must discover a node's fields and
alternatives from Rust declarations, without reconstructing parser emission order
from accessor implementations and tree diagrams. Preserve language behavior,
source provenance, formatter guarantees and the checked compilation boundary.

Explicit non-goals: incremental lexing/reparsing, edit-stable node identities,
red/green trees, cross-revision subtree sharing, new LSP features, new language
syntax, semantic type checking inside the parser, executable ABI/version changes,
runtime optimization and a general purpose AST construction/serialization API.
Complete parsing after a source change is acceptable. Existing reuse of an
unchanged physical parse and sharing among inline modules remain required; these
are current ownership contracts, not incremental parsing work.

### Evidence from the current implementation

| Evidence | Architectural consequence |
| --- | --- |
| [AST wrappers](../crates/kagari-syntax/src/ast/mod.rs) contain only `SyntaxNode`; `BinaryExpr::rhs()` filters expression children and selects the second | Field roles are encoded separately in parser emission, accessors and documentation |
| The same module documents that `if {}` can expose its block as `condition()` | Recovery can shift positional roles; adding a typed wrapper does not establish field identity |
| [Type lowering](../crates/kagari-hir/src/lower/ty.rs) repeatedly probes alternate forms of `TypeRef` | Syntax alternatives need explicit variants; HIR should consume the parsed form rather than rediscover it |
| [Lowering context](../crates/kagari-hir/src/lower/context.rs) traverses descendant tokens for a trimmed span | Source sites need a direct contract, including name versus whole-expression sites |
| [Expression lowering](../crates/kagari-hir/src/lower/expr/mod.rs) inspects raw children for interpolation | Ordered text/hole parts are missing from the typed interface |
| [Attribute lowering](../crates/kagari-hir/src/lower/mod.rs) traverses descendants and reads Rowan parents | The declaration/member that owns an attribute must expose it directly |
| [Source preparation](../crates/kagari-hir/src/analysis/source_preparation.rs) shares physical parses and stores logical module locators | A replacement must retain module views without reparsing or deep-cloning the physical AST |
| [Formatter](../crates/kagari-format/src/syntax.rs) uses concrete structure and validates reparsed output | Replacing the tree requires a typed formatting traversal and an equivalent preservation check, not deleting structure validation |

These establish a structural maintainability problem and plausible repeated-read
costs. They do not establish that Rowan dominates compilation time. The timing
baseline below measures that separately; no runtime speedup is inferred.

## 2. References and deliberate differences

Reviewed on 2026-11-04. Public documentation versions are recorded as served; these
are design references, not dependency additions or claims of comparative speed.

| Reference | Mechanism adopted | Ownership and differing assumptions |
| --- | --- | --- |
| [syn `ExprBinary`](https://docs.rs/syn/latest/syn/struct.ExprBinary.html), documentation served as syn 3.0.3 | Concrete Rust fields and enums describe syntax directly | Its token-stream/procedural-macro use does not provide Kagari's source-trivia and incomplete-file contract; do not copy its acceptance policy |
| [Oxc `BinaryExpression`](https://docs.rs/oxc_ast/latest/oxc_ast/ast/struct.BinaryExpression.html), oxc_ast 0.155.0; [parser architecture](https://oxc.rs/docs/contribute/parser.html) | Direct typed AST construction; arena allocation is a possible storage choice | Its arena lifetimes and JavaScript/TypeScript model are not Kagari's retained source/module ownership; start with ordinary Rust ownership, measure before adding an allocator |
| [Roslyn syntax model](https://learn.microsoft.com/en-us/dotnet/csharp/roslyn-sdk/work-with-syntax), current SDK guide | Named syntax fields, punctuation, trivia, missing and skipped input | Retain these contracts, not CLR node allocation or incremental red/green machinery; use Kagari's UTF-8 byte coordinates and shared source storage |
| [rustc HIR guide](https://rustc-dev-guide.rust-lang.org/hir.html), current guide | Separate source grammar from analysis-oriented owned bodies/IDs | rustc's documented lowering follows expansion and resolution; Kagari's lowering precedes name resolution/type checking. Do not move those phases merely to imitate rustc |

A Kagari-specific node model is necessary because the grammar, recovered shapes,
declaration-only mode and source identity/projection contracts are language-owned.
No custom allocator, universal node database or schema language is justified by
this replacement. Handwritten Rust node definitions are the single structural
owner; EBNF remains the language grammar, not a second node-layout specification.

## 3. Ownership and data model

```text
SourceFile: shared physical text, revision, line index and provenance
    |
    v
Parse: Arc<ParsedFile>
    +-- retained SourceFile handle (shares existing text/line storage)
    +-- ParseMode and diagnostics
    +-- ordered lexical records, including trivia and unknown input
    +-- typed module bodies, containing typed items/statements/expressions/types
    `-- private, parse-local module IDs for independently retained logical views
             |
             v
    HIR lowering: new semantic IDs/bodies + SourceMap
```

`Parse` retains the source needed to interpret ranges, without copying the text or
creating a second line index. Use existing `SourceFile` storage and provenance;
do not introduce another source/revision authority. `ParseMode::{Source,
Declarations}` is explicit and survives retention. A declaration parse cannot
silently become executable source acceptance.

Use ordinary owned enums/structs, `Box` only at recursive edges and `Vec` for lists
as the initial representation. No `Arc` per expression, reference-counted child
graph or semantic IDs in syntax. Module bodies live in a private typed module
table because current analysis retains root and inline modules independently.
`ModuleDef.body` names a required module body, not an untyped children list.
A retained module view pairs `Arc<ParsedFile>` with its private module ID; readers
borrow the actual typed fields. IDs are not interchangeable with HIR IDs and are
not valid in another parse. Other syntax nodes need no owning handle of their own.
This directly replaces current independent Rowan handles; migrate their consumers
instead of preserving a parallel handle API.

The following is schematic Rust, not a proposed compatibility facade:

```text
Expr = Binary(BinaryExpr) | Call(CallExpr) | If(IfExpr) | ... | Error(ErrorExpr)

BinaryExpr {
    site: SyntaxSite,
    lhs: Required<Box<Expr>>,
    operator: Required<OperatorToken<BinaryOp>>,
    rhs: Required<Box<Expr>>,
}

CallExpr {
    site: SyntaxSite,
    callee: Required<Box<Expr>>,
    generic_args: Option<GenericArgList>,
    l_paren: Required<Token<LParen>>,
    arguments: SeparatedList<Expr, Comma>,
    r_paren: Required<Token<RParen>>,
}

IfExpr {
    condition: Required<Condition>,
    then_body: Required<Block>,
    else_clause: Option<ElseClause>,
    ...
}
Condition = Expression(Box<Expr>) | Binding(BindingCondition)
Required<T> = Present(T) | Missing { at: ByteOffset }
```

Constructors and mutation belong to the parser; consumers receive immutable
borrows and exhaustively match alternatives. A typed operator includes its actual
token site. Missing operators lower to explicit missing/error structure, never
an invented successful `Add` merely because it is a convenient default.
`SeparatedList` retains element order, separator positions, missing elements and
an optional trailing separator. `f(a,,b,)` must preserve both the missing argument
and trailing comma; collecting only recognized expressions is insufficient.

Do not expose `syntax().children().filter_map(...).nth(...)` as the implementation
of a field. Generic visitors traverse typed fields in authored order; they are
derived behavior, not another stored tree. Read-only `NodeRef` enums/enter-leave
events may support existing generic consumers. Do not recreate a universal
parent/children storage layer merely to avoid migrating those consumers.

## 4. Lossless text, punctuation and recovery

Retain the lexer sequence as a single ordered table. Each lexical record contains
kind and a half-open byte span into the retained source. Whitespace, line/block/doc
comments, unknown lexemes and the zero-width EOF remain represented. No per-token
owned spelling, copied comment strings or independently stored CST are needed.
Real lexical ranges partition the source exactly once; missing syntax consumes no
bytes. Token references and ranges are private parse-local values. Public typed
access resolves them through the owning parse, not arbitrary caller-supplied text.

Typed AST fields retain keywords/delimiters/separators explicitly. One grammatical
operator may refer to adjacent lexical pieces: the current lexer keeps angle
tokens separate, and parsing distinguishes shifts from nested generic closers.
An operator reference can cover those contiguous pieces without rewriting or
duplicating the lexical stream. This preserves source spelling and precedence.

Leading/trailing trivia ranges may index the same lexical table; they never own a
second copy of trivia. Freeze a deterministic rule: trivia through the first line
ending after a real token is trailing; subsequent trivia belongs to the next
token. Consume block comments atomically even if they contain newlines. The first
token owns initial trivia, EOF owns otherwise unattached final trivia, and a
trivia-only file is valid. Formatting's leading/trailing/dangling semantic
classification remains with the existing formatter, independent of storage.
Inner/outer documentation remains distinguishable from ordinary comments.

`SyntaxSite` supplies body span, full span and lexical coverage with constant-time
boundary access. Exact field layout is measured in TA1; avoid storing redundant
copies of every derivable range. Body span excludes outer trivia but includes
internal trivia/errors; full span includes assigned outer trivia. An empty or
missing component has a precise zero-width anchor. Name/segment/operator/member
sites remain distinct from the whole construct. Convert to physical `FileSpan`
through `SourceFile::span`, preserving copied-source projections; line/UTF-16
conversion stays in the existing source layer.

Recovery is parser-owned and produces three distinct states:

| Input | Required result |
| --- | --- |
| `if {}` | Missing condition, present then block; the block never becomes the condition |
| `if ready {}` | Present condition/body and legitimately absent else clause |
| `if ready {} else` | Present else clause containing a missing body |
| `f(a @@@, b)` | Recognized arguments plus ordered unexpected lexical range at the recovery boundary; original text retained |
| `f(a` | Missing right delimiter at the recovery anchor; round-trip does not synthesize `)` |
| Diagnostic/depth exhaustion | Limit diagnostic, usable recognized prefix and explicit unparsed suffix through EOF |

Each skipped lexical range has one syntax owner and insertion position among that
owner's grammatical fields/list entries. It is an explicit unexpected-content
record; it does not turn an otherwise recognized argument into another argument.
An entirely unrecognized expression uses `Expr::Error` instead. Keep error payloads
in a sparse per-parse table, referenced by an optional private recovery ID on the
owning site's metadata. Valid nodes allocate no recovery vector. The parser
assigns the owner and field/list position; consumers do not infer them from spans.

Keep existing diagnostic kinds, useful ranges, recovery progress, cancellation
polling and parser limits. Any intentional improved recovery changes require
reviewed fixture updates; do not retain the old positional bug as a public API.
Depth accounting must also bound recursive destruction: check admission before
attaching an arbitrarily deep iterative postfix/binary chain, preserve its suffix
as error input, and test drop after limit exhaustion. Full parsing is not a reason
to weaken cancellation or source-size policy.

## 5. Required handoff to HIR and other consumers

| Consumer need | AST contract | Remains outside syntax |
| --- | --- | --- |
| Expressions, statements and assignment places | Explicit grouping, operators, binding patterns, writeability spelling and once-only authored child order | Place classification, desugaring, mutation/effect semantics and HIR allocation |
| Types/generics/traits | `TypeSyntax` variants for named/applied/grouped/tuple/function/qualified/projection forms; ordered positional and associated arguments with individual sites | Resolved types, binders, trait selection and validation of argument ordering |
| Paths, calls and method-like syntax | Path segments with sites, explicit generic application and receiver/member/callee distinctions | Name resolution, implicit receiver handling, overload/interface selection |
| Literals and interpolation | Literal kind/raw spelling/site; ordered `Text` and `Hole` parts, debug marker and delimiters | Numeric fitting and value typing; reuse the existing source literal-decoding owner rather than another decoder |
| Attributes/docs/visibility | Attached to their actual declaration/member, retaining unknown or unsupported syntax for diagnosis | Attribute eligibility, registration metadata and visibility checking |
| Functions, local functions, closures, async blocks | Distinct variants, explicit bodies/signatures, absent body versus declaration terminator | Capture analysis, callable/body ownership, inferred returns and Future typing |
| Modules and imports | Inline/external module form, physical body extent, module docs and structured use tree with alias/glob sites | File acquisition, logical module identity, import flattening and dependency resolution |
| Diagnostics/navigation | Full/body/name/member/keyword sites and explicit missing anchors; source identity comes from owning parse | SourceMap IDs, inferred/generated sites and UI coordinate conversion |

Keep AST and HIR separate. AST preserves written parentheses, punctuation and
recovered grammar; HIR owns analysis-friendly arenas, body ownership, desugaring,
source maps and later resolved/checked facts. The existing HIR `ExprKind` is
already stored structure, so adding a third intermediate AST between CST and HIR
would duplicate work. Remove the CST once the replacement is integrated.

Audit all current consumers, not only `lower/expr`: syntax/parser tests, formatter,
HIR lowering, declaration-only native ingestion, analysis documentation/navigation,
package module discovery and analyzer syntax adapters. Compiler/runtime consumers
must still depend on checked facts, not the new syntax representation.

Inline child modules share the exact same physical parse and source text. Keep
logical diagnostics projected for presentation while the complete physical error
set still rejects executable compilation. A child view must survive dropping the
caller's root handle without reparse, source substring copying or deep AST clone.
This is a required retention test and a benchmark follow-up, not optional LSP work.

For formatting, replace CST-kind dispatch with typed visitors and preserve
comment attachment/layout ownership. Validation compares ordered non-whitespace
lexical contents (comments included) and a canonical grammar-shape traversal of
the typed AST before/after formatting. The shape distinguishes grouping,
operators, generic application and argument order; matching token text alone is
insufficient. No CST mirror may survive just to perform that check.

## 6. Performance contract

Performance is measured from the current implementation before replacement. The
benchmark lives in the HIR example solely to access the public syntax and HIR
entrypoints without adding HIR to syntax dependencies or widening private APIs:
[driver](../crates/kagari-hir/examples/ast_baseline/main.rs),
[workloads](../crates/kagari-hir/examples/ast_baseline/workloads.rs),
[process runner](../scripts/benchmark_ast.py).

### Frozen workload recipes

| Row | Source/work amount | Purpose |
| --- | --- | --- |
| small | One generated function, 77 bytes | Fixed per-file overhead |
| medium | 256 distinct functions, 20,114 bytes | Routine expression/control/call parsing |
| large | 4,096 distinct functions, 326,570 bytes | Scaling and destruction |
| trivia | Same 256 bodies plus Unicode docs, nested comments and CRLF, 34,450 bytes | Lossless storage without skipping trivia work |
| syntax_corpus | Twelve existing syntax examples, separate files, 9,268 bytes after CRLF-to-LF normalization | Generics, traits, patterns, imports, interpolation, inline modules and updates |
| recovery | 64 broken functions, 2,038 bytes, currently 128 diagnostics | Recovery cost and useful prefix/sibling preservation |
| limits | 160 nested parentheses plus a following function, 352 bytes | Default parser-limit behavior and safe teardown |
| declarations | 256 generic Native signatures, 8,338 bytes | Separate declaration-mode parsing |

The generated routine bodies use unresolved `helper` calls deliberately: this is
syntax and structural HIR work, not semantic checking. The twelve example paths
are fixed in the workload owner. Fixture bytes, recipe, parse modes, limits and
logical checksums must match across a pair. The report records a length-delimited
BLAKE3 input hash, file/byte/token counts and diagnostics/function/field evidence.
Do not silently regenerate a faster corpus or loosen assertions during migration.

### Measured intervals

| Stage | Timed work | Excluded work / limitation |
| --- | --- | --- |
| `lex_drop` | Full lexing including trivia/EOF and token-buffer destruction | Source creation and file IO |
| `parse_drop` | Public parse including lexing, all tree/diagnostic construction and final destruction | Source creation, correctness walks, HIR |
| `parse_and_drop_split` | Separate construction and final-owner drop intervals | Extra timer reads; diagnostic attribution, not a replacement for `parse_drop` |
| `field_read` | Repeated lhs/operator/rhs, callee/arguments and condition/then/else reads of preselected nodes | Discovery, parsing and parent traversal; never label it complete AST traversal |
| `parse_hir_drop` | Public `lower_module`: fresh parse, structural root-module lowering, SourceMap and intermediate/final destruction | Type checking, complete package/inline-child preparation and executable lowering |

HIR timing runs only on valid ordinary-source rows. Declaration-mode/recovered
HIR are covered by focused correctness tests later; the public convenience API
cannot isolate already-parsed lowering, so do not subtract parse time to invent a
HIR-only measurement. Likewise, `parse_and_drop_split` construction includes
lexing; subtracting the independent lexer row does not measure parser-only cost.
The current benchmark does not measure peak/retained bytes
or allocation counts; those require the separate TA1 instrumentation described
below and are **unmeasured**, not zero or passing.

Measure seven fresh processes for baseline characterization. Candidate acceptance
uses seven alternating baseline/candidate process pairs, three warmup batches and
eleven measured batches per stage/process. Batch size targets 20 ms (100 ms for
field reads, frozen in TA1) using an
untimed probe, bounded to 1..100,000 iterations. Record actual iterations; a sample
is a batch mean, not individual-operation latency. Report median of process
medians, process min/max, ns/file or ns/workload and MiB/s for parse rows.
Paired results use the median of per-pair candidate/baseline ratios, not a ratio
of pooled medians. Eleven batch means do not establish p99 latency.

Use workspace release, default features/build parallelism and one measurement
thread. Load sources once, warm allocator/code/data, and always construct a fresh
parse; no parse cache in the timed loop. Compilation, source/line-index setup,
input loading and correctness verification are outside timing. Do not overlap
compilation, tests or another benchmark with measurement. Archive executable
SHA-256, lockfile/harness/input hashes, revision plus dirty state, toolchain,
machine, environment, sampling and cache state. Preserve the baseline executable
before rebuilding; ignored raw reports are supplemental to the recorded results.

### Acceptance budgets fixed before replacement

These are engineering budgets, not predicted speedups or mature-runtime claims.
For each row let B be its paired baseline time, and C the candidate time.

| Metric | Required budget | Decision rule |
| --- | --- | --- |
| Parse lifecycle, all eight rows | Median paired C/B <= 1.05 | No per-row regression hidden by an aggregate score |
| Primary parse improvement, medium/large/trivia/syntax_corpus | Median paired C/B <= 0.80 on each row | Required for this goal's performance completion; structural completion alone does not close the goal |
| Parse-to-HIR lifecycle, five valid ordinary rows | Median paired C/B <= 1.05 | A parser win must not move its cost to lowering; report any gains separately |
| Field reads, five valid ordinary rows | Median paired C/B <= 1.00 | Direct fields must not regress the same logical reads; no promised numeric speedup |
| Split construction/drop and lexer control | Report each interval and C/B | Investigate attribution changes; lifecycle budgets remain authoritative |
| Peak and retained additional heap bytes | Candidate <= B + max(0.10 B, 4096 bytes), separately per row | TA1 freezes measured B before implementing the replacement; includes lexical tables, nodes, diagnostics and retained parse/module-view backing, excludes already-owned source bytes identically |
| Allocation requests / requested bytes | Record separately for parsing and final teardown; no unbounded growth across repeated lifecycles | Do not substitute RSS for owned allocations or hide retained arenas; no allocation-count speed claim |
| Correctness | Exact source reconstruction, stable authored field roles, semantic/gate/retention tests pass | Any failure blocks acceptance regardless of timing |

If process-median spread exceeds 10% of its median, mark that timing row noisy and
repeat matched processes before making an acceptance claim. Ratios within two
percentage points of a threshold require a second complete paired run. A failed
performance target requires checking field/storage/ownership decisions before
adding caches or specialized paths. Do not change thresholds after seeing results.

### Reproduction

```text
cargo build --release -p kagari-hir --example ast_baseline
target/release/examples/ast_baseline --check
uv run python scripts/benchmark_ast.py
uv run python scripts/benchmark_ast.py --baseline-executable <frozen-baseline-executable>
```

On Windows the executable has `.exe`. The runner archives raw JSON and a copy of
the candidate image under `target/ast-baseline/<UTC stamp>/`. It validates expected
rows/stages/sample counts and paired workload evidence; it reports timing only,
not a full acceptance verdict. Run from the same checkout's source corpus. The
current source root is compiled into the example; archival comparison requires
that root and the frozen fixture hashes, not just moving an executable elsewhere.
Paired runs require the baseline's adjacent `metadata.json` and reject a changed
image or differing toolchain/profile/environment before measuring. Archive the
exact `Cargo.lock` too. The sole permitted dependency delta is removal of Rowan
from `kagari-syntax` and `kagari-format`, together with newly unreferenced packages
in Rowan's dependency closure. Compare resolved dependency identities so Cargo's
equivalent version-qualified spelling is accepted. All retained package versions,
checksums, sources and other dependency edges must remain unchanged. Record both
lock hashes and the removed-package list. Early TA0 archives can recover their
lockfile from the recorded revision only when the exact byte hash matches.
The seven-process rule is an acceptance requirement; `--runs 1` is only a smoke
check and cannot establish acceptance.

## 7. Finite migration and validation

### Decisions fixed for continuous execution

| Question | Default decision; TA1 responsibility |
| --- | --- |
| Storage and ownership | Direct Rust structs/enums, boxed recursive edges, vector lists, one retained parse owner and a typed module-body table. Measure size/allocation costs before considering an arena; no separate node database or per-expression Arc |
| Source sites | Start with explicit body `Span`, lexical coverage and optional recovery ID. Derive full extent from lexical boundaries/trivia, using the body span as the empty anchor. TA1 may compact this layout while preserving constant-time field/site access |
| Recovery/list representation | Required versus optional fields remain distinct. Each separated-list entry stores `Required<T>` and an optional required separator; this distinguishes a missing item, missing comma and final trailing comma. Sparse unexpected-content records retain their owner and insertion position |
| SourceMap migration | Inventory uses of existing full `syntax_span` versus trimmed `token_span`; map each to its corresponding new site. Do not silently replace all sites with trimmed spans or lose copied-source projections. Preserve contract assertions; intentional recovery improvements need specific fixture explanations |
| Traversal and inline roots | Borrow typed fields; source-order visitors for generic consumers; immutable parse/module view for independent retention. No universal parent table. HIR lowers attached attributes with the known owner instead of searching parents |
| Limits and destruction | Preserve limit values and resource meaning, independently of allocation layout. TA1 fixes grammar-depth accounting for removed wrappers, tests iterative chains and bounded teardown, and records any deliberate recovery-fixture difference |
| Declaration mode and literals | Store parse mode; preserve ordinary executable acceptance gates. Keep raw spelling and existing literal decoding ownership; no new parser-side semantic inference |
| Production migration | Replace the implementation directly and adapt every consumer. Remove `rowan` from both syntax and format manifests and the lockfile; no compatibility crate, copied Rowan implementation or permanent generic CST |

No unresolved product decision requires pausing between phases. Routine field
names, module splits and allocation sizing are implementation decisions within
these contracts; record material refinements here and continue. A new language
feature, incremental parser or broader semantic/runtime redesign is outside scope.

### Baseline readiness before replacing production storage

TA1 must complete these prerequisites while the old parser still builds:

1. Retain the TA0 source revision, harness, executable/metadata/lockfile and frozen
   workload bytes. The commit introducing this file is the reproducible checkpoint;
   the measurements above that commit's parent remain historical evidence. If an
   ignored image is missing, rebuild the checkpoint in an isolated checkout and
   make fresh matched measurements; do not compare a new binary with old timings.
   Fixture paths are currently resolved from the compiled checkout root: keep
   that root available, or add an explicit corpus-root input to both harnesses
   before freezing a replacement baseline. Never take fixtures from two revisions.
2. Add diagnostic-only owned-allocation accounting to the harness. For each row,
   measure allocation/reallocation requests and requested bytes, peak live bytes,
   retained bytes after parsing, retained bytes with only an inline module view,
   and net bytes after releasing all parse/view owners. Exclude already-owned
   source backing identically, but include new lexical/node/diagnostic storage.
   Account realloc growth/shrink and teardown, assert zero net owned teardown,
   warm measurement infrastructure first, and use separate diagnostic runs from
   ordinary timing. No zero/unimplemented counter can pass memory acceptance.
3. Add one fixed inline-module retention workload, using existing source-preparation
   contracts. Run it on the old model before migration; record its bytes/hash and
   baseline alongside the eight frozen timing rows. This planned ownership probe
   does not alter those eight workloads or their thresholds.
4. Stabilize or explicitly diagnose the noisy auxiliary rows. Required field-read
   acceptance cannot be waived because its baseline was noisy. Measure spread of
   both images; repeats near a threshold must independently pass rather than be
   pooled to hide a failed run. Freeze any necessary sampling adjustment on both
   images and keep the original evidence. Report diagnostic-only noise separately.
5. Adapt benchmark accessors to the new API at cutover without changing the timed
   work. Preselection stays outside field timing; production traversal remains
   inside parse-to-HIR timing. Keep frozen field/diagnostic evidence. Recovery
   improvements are proven on separate semantic fixtures, not by weakening the
   frozen workload checks. Retain exact before/after harness source references.

The existing script reports measurements, not acceptance. TA1 also adds a
fail-closed acceptance report covering seven pairs, expected stages, both-image
noise, every timing budget and measured memory rows. Missing measurements,
unresolved noise on a required row or a failing threshold mean incomplete
acceptance; they must never default to success. This is benchmark work on the
existing owner, not another progress or test framework.

| Phase | Scope and exit |
| --- | --- |
| TA0: design and baseline | Inspect consumers, freeze this contract, add and run the current timing harness. No parser replacement |
| TA1: storage/recovery proof | Finalize concrete node layouts and parse/module ownership; measure baseline memory; prove direct binary/call/if and missing/skipped/trivia representation with existing fixtures. This is a bounded test prototype, not a second production parser |
| TA2: coherent syntax cutover | Implement the complete typed node families and parser construction, migrate syntax tests, remove Rowan-backed storage/views and temporary proof scaffolding. Default limits, cancellation and declaration-mode contracts build and pass |
| TA3: consumer integration | Migrate HIR/source preparation/native declarations, formatter, package and analyzer callers. Remove all raw-node dependencies/temporary adapters; preserve physical diagnostics, provenance and shared inline parses |
| TA4: local acceptance and CI handoff | Run focused contracts, required structural/fmt/Clippy checks and paired performance/memory checks; document unmet targets and leave exhaustive matrix to CI |

TA2/TA3 are one coordinated API migration: do not maintain two production semantic
implementations for hypothetical compatibility. Intermediate commits, if requested,
may have downstream compile failures only when their exact commands/diagnostics
and TA3 owner are recorded here. Attempt focused syntax checks once that unit
builds; do not repeatedly rerun known unchanged consumer failures. Integration
must be fully restored before final implementation acceptance.

For one goal, phases are internal milestones, not separate branches, PRs or human
approval stops. Work on `refactor/typed-ast`; retain finite failing-command notes
while the API changes, then resolve them through TA3. A successful syntax-only
build is not a reason to stop with consumers broken. Keep progress here rather
than creating per-phase plans. Checkpoints need no full-workspace test sweep.

Use this consumer order within TA3: HIR lowering and physical source preparation;
declaration-only Native ingestion; formatter and its preservation validator;
package discovery and remaining analysis/navigation/adapters; test/benchmark
consumers and dependency cleanup. An adapter used during editing must be removed
by this same integration checkpoint, not left for an unspecified future phase.

Select local tests by boundary: syntax precedence/typed recovery plus limits and
cancellation; HIR lowering sites/interpolation/generics/local functions; shared
physical inline parse/error projection; ordinary versus Native declaration gates;
formatter exact output and lexical/grammar preservation; one representative
package/analyzer navigation path. Reuse existing tests, adding only missing
distinct contracts such as stable recovered field roles. Full workspace and
feature/backend/platform test matrices remain CI-only.

Start from these focused local selections; adjust names only when their owning
tests move, preserving the exercised boundary. This is not an exhaustive package
sweep:

```text
cargo test -p kagari-syntax --lib -- tests::limits tests::cancellation tests::declarations tests::parser::postfix tests::parser::errors
cargo test -p kagari-hir --lib -- tests::lower analysis::source_preparation::tests
cargo test -p kagari-format --lib -- adjacency_interpolation_comments_and_boundaries_are_idempotent preserves_crlf_and_canonical_empty_output rejects_malformed_source
uv run python -m unittest discover -s scripts/tests -p test_ast_benchmark.py
```

Add the smallest existing pattern/type/interpolation contract selection affected
by TA2, one package inline-discovery case and one analysis navigation case during
TA3. Record exact selections and outcomes here; the table of consumer needs
determines coverage, not a target test count. Avoid re-running unrelated contracts.

Required implementation checks: `uv run --locked scripts/check_structure.py`,
`cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check`.
Benchmark tooling tests must reject missing/nonfinite evidence and mismatched
workloads; a malformed report cannot become a passing performance result.

### Goal completion and handoff

The goal's local completion requires all of the following: one production typed
AST/parser, all consumers integrated, no Rowan dependency or compatibility CST,
all selected semantic/ownership tests and local checks passing, all required
timing/memory budgets met (including the four 20% reductions), and current
architecture/performance documentation updated. Missing memory evidence or a
performance miss is not a completed goal. Diagnose and continue within this scope;
do not silently relax thresholds, replace corpora or stop at structural completion.

Complete feature/backend/platform tests stay in CI. The goal closes at **local
implementation acceptance and an explicit CI handoff**, not a claim that unrun CI
passed. List the pending CI matrix and any external status in the final report.
Publication/merging is separate from the local implementation goal.

Suggested goal objective (used only when the user starts the goal):

```text
Implement TA1-TA4 from docs/typed-ast-design.md on refactor/typed-ast in one
continuous run. Preserve the frozen baseline before cutover, replace Rowan with
the direct typed AST, migrate all consumers, and satisfy the document's local
correctness, structural, timing and memory acceptance criteria. Continue across
internal milestones; maintain the single plan ledger. Finish with the integrated
changes committed using Conventional Commits, measured results, and an explicit
handoff of the exhaustive CI matrix. Incremental parsing and broader HIR/runtime
redesign are out of scope.
```

On completion, move lasting syntax/HIR/formatter contracts and measured conclusions
to their existing architecture/performance owners, retain unresolved work in the
roadmap and remove this completed plan. Do not leave a parallel progress queue.

## 8. Progress and current evidence

- [x] Inspect current syntax, HIR lowering, physical source reuse and formatter contracts.
- [x] Define the strong AST, recovery, source ownership and next-layer requirements.
- [x] Add a reproducible current-implementation timing harness with fixed workloads.
- [x] Complete seven-process baseline, A/A repeat, evidence review and tooling validation.
- [x] Prepare the single-branch goal contract, fixed decisions, baseline prerequisites and completion gate.
- [x] TA1 memory baseline and bounded representation proof.
- [x] TA2/TA3 implementation and coherent consumer migration.
- [ ] TA4 paired performance and integration acceptance; exhaustive CI remains pending.

### TA1 execution ledger (2026-11-05)

- Production remains Rowan-backed while baseline instrumentation is frozen.
  The diagnostic allocator forwards to `System`, counts successful allocation and
  reallocation requests/bytes and frees, accounts growth/shrink, and reports live
  and peak requested bytes. Thread-local accounting is disabled in ordinary timing.
  Source construction, probe vectors and JSON reporting are outside the window.
  Each of nine rows repeats four complete lifecycles, requiring identical counts
  and zero net owned bytes after releasing every parse/view.
- The additional ownership workload is 32 inline modules with four frozen
  trivia-bearing routine functions each; the retained first child exposes four
  items after the caller drops every root parse. This adds no timing row.
- On Apple M1 Max / aarch64 macOS, rustc 1.99.1 (`58cf8f9c3`), workspace release,
  default features/parallelism, one measurement thread, warmed resident inputs,
  the first seven-process archive is `20261104T160538438452Z` (host UTC stamp).
  No builds/tests overlapped measurements. Required field-read process spread was
  19.3% for medium. A pre-cutover sampling adjustment increases only field-read
  batches to 100 ms on both images. The next seven-process characterization
  (`20261104T161030135308Z`) still has one medium field-read outlier (18.4% spread);
  this is unresolved noise, not acceptance. Seven A/A pairs in
  `20261104T161411058915Z` stabilize both images' field reads: candidate/baseline
  spreads are 5.5/1.7%, 4.1/1.4%, 3.4/2.1%, 3.4/2.1% and 3.9/3.2% for the
  five valid rows. Parse lifecycle ratios range from 0.9986 to 1.0141. One large
  baseline parse/HIR row remains noisy (10.6%); split-drop auxiliary spreads also
  remain visible and cannot establish attribution. TA4 must repeat contemporary
  pairs until every required row meets its noise rule. A/A naturally fails the
  replacement's 20% improvement gate and is not candidate acceptance.
  The final frozen baseline image SHA-256 is
  `270fd10917c9f2592f1f41c8add05d1d6c7ccd7ee2099400a5434c8cdbe8557f`.
  Its archive includes exact input bytes and harness sources. The inline probe
  is 17,558 bytes with length-delimited BLAKE3
  `3125435cdfe3c659afecef0da93e5dbe944d66a9da28e3da9026bb7c5d49b2fb`.
- Additional heap baseline (bytes; source backing excluded):

  | Row | Peak during parsing | Retained after parsing | Child-only retained | Parse requests |
  | --- | ---: | ---: | ---: | ---: |
  | small | 4592 | 3456 | n/a | 88 |
  | medium | 764768 | 333936 | n/a | 6235 |
  | large | 12207968 | 5310576 | n/a | 98415 |
  | trivia | 805864 | 350456 | n/a | 6239 |
  | syntax_corpus | 164888 | 141168 | n/a | 3418 |
  | recovery | 113304 | 54952 | n/a | 764 |
  | limits | 32848 | 7888 | n/a | 77 |
  | declarations | 321664 | 87440 | n/a | 2629 |
  | inline_retention | 572936 | 174488 | 174064 | 3103 |

- The bounded, test-only representation proof constructs binary/call/if fields
  without a second parser. Missing condition/operand/delimiter anchors, a missing
  argument, trailing comma and sparse skipped input are independent fields. Its
  32-bit token-reference layout measures site 12 B, expression 16 B, required
  expression 16 B, separated entry 24 B, binary 56 B, call 72 B and if 56 B.
  Boxed recursive alternatives avoid expanding every literal to the largest
  expression payload. Production layout and retained memory still require TA2/TA4
  measurement; this proof is not production parser or recovery acceptance.
- The runner now archives the exact harness sources, executable, lockfile and
  frozen input bytes, and adds fail-closed timing/memory acceptance. It checks
  seven pairs, both-image process spread, all budgets, independent near-threshold
  repeats, workload equality and nonzero consistent counters. Missing data,
  leaked backing and absent view retention cannot pass. Auxiliary spread remains
  separately visible. Thirteen tooling tests and two layout/recovery proofs pass;
  structure, workspace all-target Clippy, formatting and diff checks pass at the
  instrumentation checkpoint. No production consumer/build failures are carried.
- TA2 source ownership refinement: ordinary nodes are owned structs/enums, exposed
  through immutable borrows. A single generic borrowed carrier pairs a node with
  its parse for spelling/site access; it owns no `Arc`, text or tree cursor.
  Only retained module views clone the parse owner. Read-only expression/statement
  alternatives and visitors are derived from typed fields, with no second stored
  grammar tree or parent table. Module declarations name private body-table IDs.
  Recursive expression alternatives are boxed to keep simple literals compact;
  admission must bound completed-chain depth before attaching each new wrapper.
- TA2 in progress after baseline checkpoint `e1b38ebb`: production parser/storage
  now construct typed expression, statement, pattern, type, attribute, signature,
  declaration and module-body fields. `cargo check -p kagari-syntax --lib` passes
  while borrowed access/derived traversal are being completed. It currently emits
  unused-import/docs and not-yet-read traversal/recovery-field warnings; these are
  carried integration work, not accepted warnings. Syntax tests still use the old
  API and have not passed this cutover. The TA1 test-only proof is still present
  until those contracts migrate to their production owners.
- Attempted consumer boundary: `cargo check -p kagari-hir --lib` fails on seven
  removed imports (`ast::traits`, `TypeAliasDef`, `ty::TypeRef`). This is expected
  TA3 API migration debt; all raw traversal, lowering sites and borrowed signatures
  must migrate before repeating this boundary. No downstream acceptance is claimed.
- The initial HIR import failures are resolved. HIR now borrows typed fields,
  exhaustively lowers `TypeView`, reads constant-time sites, lowers ordered string
  parts, and gathers attributes from their known owner without parent traversal.
  Shared physical source preparation, declaration correspondence and package module
  discovery use the retained module table. `cargo check -p kagari-hir --lib` and
  `cargo check -p kagari-package --lib` pass without warnings. Analyzer has only a
  binary target (`cargo check -p kagari-analyzer --lib` is inapplicable); its binary
  integration remains part of TA3. Existing syntax/HIR unit test APIs and formatter
  are still being migrated; no complete integration claim is made.
- Five production contract tests pass via `cargo test -p kagari-syntax --test
  typed_fields`: recovered field roles/else presence, missing list entries versus
  unexpected separator-boundary input/trailing commas, trivia/source sharing,
  child-view lifetime/foreign locator rejection and iterative chain exhaustion/drop.
  The old TA1 proof remains pending removal after all syntax tests migrate.
- Candidate harness `--check` matches **all** frozen identities/evidence, including
  corpus checksum 4360, corpus function count 29, recovery diagnostics 128/checksum
  896 and limits diagnostics 1. The unchanged workloads have not been replaced.
- First candidate allocation diagnosis (not acceptance): retained bytes are 2735,
  421703, 6730820, 430919, 129314, 65485, 5934, 198745 and 225029 for small,
  medium, large, trivia, corpus, recovery, limits, declarations and inline retention.
  All four repeated lifecycles release to zero. Peaks are below the baseline, but
  medium/large/trivia/recovery/declarations/inline retained budgets fail. Ordinary
  struct/enumeration layout embeds large absent optional payloads; the declaration
  row also loses Rowan's shared repeated signature nodes while retaining separate
  physical sites as required. Before optimization, quantify actual field layouts
  and compact the typed ownership model; do not add caching/interned node graphs,
  relax memory budgets or change the source corpus. TA4 owns this unresolved gate.

### Integrated implementation and storage diagnosis (2026-11-05)

- All production consumers compile through typed borrows: HIR lowering, source
  preparation/reuse, native interface admission, package discovery, lint and
  formatter. Rowan dependencies, green/raw construction and the bounded test
  prototype have been removed. The physical diagnostic gate remains authoritative.
- Named `Required` slots preserve malformed roles. The resource frame accumulates
  real child depths, including signatures and iterative wrappers; all six existing
  resource contract tests pass. Five selected recovery failures were resolved
  through the intended typed model, including rejected entries after struct update
  and duplicate pattern-rest delimiters. Obsolete positional/CST assertions now
  inspect typed missing slots and explicit fields; following siblings/tails remain
  asserted. The type-alias model retains leading/trailing `where` placement.
- Measured layout identified absent optional payloads, vector headers and the
  lexical full-extent table as retained costs. Ordinary boxes isolate rare clauses;
  separated lists are immutable slices; absent attached attributes cost one pointer.
  Sites use ordered lexical coordinates or a private reversed-pair sparse recovery
  reference (8 bytes). Real token indices use nonzero encoding (4 bytes).
- Lexical starts use a source-length-proven 16/32-bit choice. A sparse immutable
  exception hash table retains non-default trivia splits with expected constant-time
  reads; no body/site getter scans tokens or text. Both storage widths retain UTF-8
  byte semantics and the existing source backing/projections. This replaces the
  first candidate's 4-byte boundary on every token; no node interning/cache or
  custom allocator was added.
- Comparative layout references: syn ItemFn documentation served 3.0.9 uses a boxed
  block; TypeParam 3.0.6 owns punctuation/bounds; ReturnType 3.0.9 explicitly models
  absence versus arrow/type. These support isolating payloads without copying syn's
  token-stream/trivia semantics. See [ItemFn](https://docs.rs/syn/latest/syn/struct.ItemFn.html),
  [TypeParam](https://docs.rs/syn/latest/syn/struct.TypeParam.html),
  [ReturnType](https://docs.rs/syn/latest/syn/enum.ReturnType.html).
  [OpenJDK JDK 9 release notes, JEP 254](https://www.oracle.com/java/technologies/javase/9-relnotes.html)
  document content-proven storage width behind one logical API. Kagari applies
  that mechanism only to immutable numeric offsets, not character encoding or
  JVM allocation; source text stays UTF-8 and all external coordinates stay bytes.
  No speed result is inferred from these references.
- Focused local evidence: 35/40 selected initial syntax contracts passed; after
  batched repairs the five failing cases passed. Twenty-eight of 29 additional
  grammar/attribute contracts passed, with the remaining full-physical attribute
  traversal corrected and passing separately. Six production typed-field tests
  pass, including both offset widths. HIR lowering/source preparation had 12/13
  pass initially; the exact attribute site was corrected and passes. Eight native
  admission/inline/attribute checks, seven documentation/location queries, eight
  lint fixture/policy checks and one package external/inline visibility check pass.
  The analyzer `navigation` filter selected zero tests and is not evidence; actual
  HIR navigation cases are selected separately. Formatter's 18 preservation checks
  and representative expected-output cases pass, including both alias placements.
- Required structure, formatting and workspace/all-target Clippy are exercised at
  this integration boundary; final outcome is recorded below. Unchanged passing
  contract selections are retained as evidence. Complete grammar inventories,
  workspace tests, feature/backend/platform coverage remain CI-only.
- Memory after compaction has all nine retained/peak rows inside the frozen budget;
  all four repeated lifecycles return to zero. Exact final values and contemporary
  seven-pair timing/noise acceptance remain TA4 work; isolated diagnostic runs do
  not establish timing acceptance.

### TA4 first paired outcome and noise diagnosis

The seven-pair image comparison at UTC 20261104T181937587757Z passed every
numerical time/memory budget but failed the unchanged 10% process-spread gate
for `small/field_read`. That candidate row runs at about 17 ns/read and reaches
the frozen 100,000-iteration cap, giving batches of about 1.7 ms; process medians
varied 16.78–19.29 ns. This is unresolved measurement noise, not acceptance.
No timing thresholds, input bytes, warmups/samples or iteration cap were changed.
A new complete seven-pair comparison uses identical inherited application,
throughput/latency tier-zero policy for both images, with idle sleep inhibited:
`/usr/sbin/taskpolicy -a -t 0 -l 0 /usr/bin/caffeinate -i uv run python scripts/benchmark_ast.py --baseline-executable target/ast-baseline/20261104T161411058915Z/ast_baseline`.
The macOS taskpolicy(8) manual states that children inherit these policies.
This isolates a process scheduling condition without modifying either timed image.
The first report remains failed evidence; the new complete result is pending.

Remaining focused selections now pass: seven cancellation/postfix/error tests,
two explicit HIR annotation/alias navigation tests, 22 formatter comment/layout/
preservation cases (one matching complete-inventory case remained CI-ignored),
two syntax Rustdoc examples and 13 fail-closed benchmark-tool tests. The broad
analyzer navigation-name filter selected zero tests; HIR owns the actual analysis
navigation implementation and the two named cases establish that boundary.
Workspace all-target Clippy, structure (1812 files, zero exceptions), format and
diff checks passed. No full workspace test or complete feature matrix ran locally.

### Cancellation review before final image freeze

The scheduling-policy rerun was interrupted after three complete pairs and is
not acceptance evidence. Manual review identified an asynchronous race in the
new `take`: its assertion consulted the cancellation-masked logical EOF after a
branch had admitted a real token. It now checks physical lexical EOF, completing
the admitted read before the controlled entrypoint rejects cancellation. No host
callback or partial success is introduced. The eight selected cancellation and
resource tests pass again. The final image must be rebuilt and measured from a
fresh complete seven-pair run; earlier image results cannot accept this change.

### Baseline measured on 2026-11-04

Implementation revision `d799160945faa9707d3d1bcd44e64321a8c2250a` plus this
uncommitted benchmark/documentation change; no production parser/HIR changes.
Windows 11 `10.0.26300`, Intel Core i9-12900K, 24 logical processors,
68,447,940,608 bytes installed RAM. rustc 1.99.1
`58cf8f9c35b355199727627d07ae34a4e2846d67`, LLVM 23.1.1,
`x86_64-pc-windows-msvc`; cargo 1.99.1 `5f94df478`.
Workspace release/default features/default build parallelism, single measurement
thread, resident inputs and warmed allocation/code/data. CPU affinity/frequency
were not fixed. `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `CARGO_BUILD_JOBS` and
`RUSTC_WRAPPER` were unset. Source setup, compilation and checks were excluded;
no build/test/other benchmark was run concurrently with timed processes.

The first build took 41.05 s; the import-only rebuild took 1.17 s. Neither is a
parser measurement. Seven initial characterization processes were followed by
seven alternating A/A process pairs (14 processes), with three warmups and eleven
samples per stage per process. A/A compares equivalent parser/HIR implementations;
the example changed only an explicit import to satisfy structure checks. It is
measurement calibration, not a typed-AST speed comparison.

All values below are microseconds per entire workload, median of seven candidate
process medians from the A/A repeat. The corpus row is the sum for twelve separate
files, not one file's latency. Split intervals are independently timed and must
not be added to reconstruct the separately measured lifecycle column.

| Workload | Lex + drop | Parse + drop | Split construction | Split drop | Parse + HIR + drop | Field reads |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| small | 0.756 | 7.375 | 6.242 | 1.188 | 26.362 | 0.931 |
| medium | 345.905* | 1255.371 | 951.105* | 146.411* | 5432.800 | 240.820 |
| large | 4315.360 | 17792.850 | 15937.650 | 1990.450 | 94054.900 | 4207.040* |
| trivia | 208.422 | 1061.006 | 926.117 | 122.291 | 5315.750 | 242.477 |
| syntax_corpus | 77.128 | 482.661 | 416.720 | 66.244 | 1781.283 | 40.490 |
| recovery | 17.517 | 98.215 | 84.222 | 13.791 | unmeasured | unmeasured |
| limits | 2.384 | 17.944 | 15.174 | 2.891 | unmeasured | unmeasured |
| declarations | 82.483 | 298.253 | 267.239 | 30.334 | unmeasured | unmeasured |

`*` remains noisy: medium lex/split-construction/split-drop process spread is
52.3%/11.5%/15.9%; large field-read spread is 10.3%. The corresponding medium
split-drop A/A ratio is 1.0895. These auxiliary rows cannot support a performance
acceptance claim until matched repetition/measurement diagnosis resolves the
variation. The initial run also had noisy small HIR/field-read measurements;
those stabilized in the repeat. Do not attribute the variation to a specific
allocator, CPU scheduling or Rowan mechanism without further evidence.

| Workload | Parse throughput (MiB/s) | Parse process spread | Median paired A/A parse ratio |
| --- | ---: | ---: | ---: |
| small | 9.96 | 4.6% | 0.9853 |
| medium | 15.28 | 2.7% | 1.0071 |
| large | 17.50 | 4.0% | 0.9989 |
| trivia | 30.97 | 6.0% | 0.9978 |
| syntax_corpus | 18.31 | 3.0% | 1.0125 |
| recovery | 19.79 | 2.2% | 0.9911 |
| limits | 18.71 | 0.9% | 0.9984 |
| declarations | 26.66 | 3.9% | 0.9864 |

All eight primary lifecycle calibration ratios are within 1.5% of parity. This
supports using paired lifecycle measurements on this machine; it establishes
neither a replacement speedup nor cross-machine absolute latency promises.
The primary 20% improvement targets correspond roughly to 1.004 ms medium,
14.234 ms large, 0.849 ms trivia and 0.386 ms corpus on this reference run.
Acceptance still uses contemporaneous paired ratios, not these rounded numbers.

For resumption, the first report identifier is `20261104T153110941006Z` and the
A/A repeat is `20261104T153828541299Z`. They are optional raw evidence under the
ignored output directory, not documentation dependencies. The recorded image
SHA-256 values are:

```text
initial: 41750b47ea208b35f211d1025b6fc452acd32a76e23f3facdb3a4b3af4e8e067
repeat:  4f82651ed36d399124fbe0de6336dd611e062a63f82cf966022a6e5a9c3605ab
lock:    0971f2db665c28e817f434d05c8480afc393e44f8cdae6eaeb3a77a4b4c9ed02
```

Frozen length-delimited input BLAKE3 hashes:

```text
small          669549a370d6e209ba0dbb72e6b51eceede737246d3ff85d7a5db37594492bd1
medium         af7413102e1b5c17c7d52864469f82a618c6f77474d5370dde8ec429f3b69a01
large          c1ae1c000158fbd75daf74df8bbd22ec53527661d49214070c83a3f1ae7e22e4
trivia         d851a4cfeebc30d42f18a178c127713ff0bf69b01bdaa5004dd890f1860a5055
syntax_corpus  44aeb6877a0b8c35b926f449e61dfcfd7e7420e4b47f7952b95cb138d83035aa
recovery       f1667a1b1211e31b3d5f8f2c888c61acf360c2cf0d3df6a0cfeb91123dd0060e
limits         9feb535f2264a812ffa02fd8a427c128cc6d26f146b8d5bdd91675388ce50b1d
declarations   9d58d0c4a6e74b24af28be51b7f8418d3a42f713ba78f70c8f9dc8de47cff10c
```

### Actual validation and remaining limits

- The release example's `--check` and all 21 measured processes pass lexical
  coverage, exact tree/source round-trip, diagnostic expectations and logical
  field/function evidence before and after measurement. Generated valid function
  counts are 1/256/4096/256, with field checksum 107 per function; the corpus
  has 29 lowered root-module functions and checksum 4360. Recovery/limit counts
  are 128/1. These bounded checks do not replace language conformance tests.
- Five tooling tests pass: missing rows/stages/samples, nonfinite/nonpositive
  timings, changed input/evidence, per-pair normalization and absence of a speedup
  claim for unpaired characterization. Command:
  `uv run python -m unittest discover -s scripts/tests -p test_ast_benchmark.py`.
- Structure checks pass (1793 Rust files, zero violations/exceptions), formatting,
  workspace all-target Clippy, local documentation links and diff checks pass.
  One initial import-style finding in the new example was fixed; no outstanding
  build/test failure remains. Only measurement tooling and documentation changed,
  so production grammar/HIR tests were not redundantly rerun.
- Peak/retained memory, allocation counts, isolated already-parsed HIR lowering,
  package-wide analysis, cold-start performance and tail latency are unmeasured.
  TA1 owns memory instrumentation and its pre-replacement baseline. No typed-AST
  implementation/performance acceptance or complete CI result is claimed.

### Single-branch readiness review

The preparation review fixed two execution gaps: the improvement target is now
explicitly part of goal completion, and deleting Rowan no longer conflicts with
the benchmark's dependency-equivalence check. The runner archives lockfile bytes
and permits only the audited removal described above. Five additional dependency
tests cover Rowan/orphan removal, equivalent dependency spellings, retained
package/edge changes, still-referenced packages and unrelated removals; all ten
tooling tests pass. The actual historical baseline lockfile was recovered and its
recorded SHA-256 verified. These changes affect report admission, not timed Rust
work; the existing timing/Clippy/structure/fmt evidence remains applicable.

Remaining implementation decisions are bounded TA1 layout/measurement choices,
with defaults and required evidence listed in section 7. They do not require
another design-only checkpoint before the user starts the goal. Memory accounting,
inline retention measurements and the acceptance reporter are explicitly assigned
to TA1 before the production representation changes. No implementation phase has
been marked complete by this preparation commit.
