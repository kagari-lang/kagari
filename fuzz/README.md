# Fuzzing

`syntax` exercises the public `kagari-syntax` parser and AST consumers. The
workspace library contains the oracle; its optional `fuzzing` feature adds the
libFuzzer executable. Ordinary workspace tests and Clippy cover corpus replay
without requiring a sanitizer or linking libFuzzer.

## Contracts

Each input is plain UTF-8 source, replayed in Source and Declarations modes with
five limit configurations: defaults, independently reduced diagnostics, nesting
and tree depth, and all three reduced together. Small limits (including zero)
derive from source length and endpoint bytes, preserving verbatim source replay.
Each accepted input therefore runs ten parses. Independent limits exercise stop
paths that another small budget could otherwise mask. Invalid UTF-8
and inputs above 128 KiB are skipped because source acquisition accepts text and
parser limits do not bound input size. No production checks are disabled.

The oracle checks lexical partitioning and exact source reconstruction, UTF-8
boundaries for token/node/diagnostic/missing-role sites, diagnostic budgets,
source-order grammar token preservation, and retained inline-module access after
dropping the parse/root handles. It traverses recovered trees and lets all trees
drop normally, covering construction, traversal and destruction failures.
Grammar traversal must match the complete lexical `(kind, start, end)` sequence,
including trivia and the single EOF record. Identical spellings at different
positions cannot substitute for one another. Combined operators are expanded into
their contiguous physical lexical pieces; missing slots and empty recovered
children consume no tokens.

An explicit stack over `elements()` supplies the expected preorder for
`descendants()` and `visit()`. Source-derived pruning checks that rejecting a
subtree still visits its siblings. Logical-module visits exclude nested bodies
while retaining their declarations and attributes, including after parent handles
are dropped. These paths share AST field definitions: agreement checks traversal
contracts, not independent grammar correctness.
The recovered tree need not obey a strict final depth cap: active outer scopes
can still close after the growth limit stops parsing.

Syntax diagnostics are expected outcomes. A panic, failed invariant, timeout or
sanitizer finding is a failure. Successful parsing is not proof of correct
precedence or field-role semantics; existing grammar tests remain authoritative.
HIR lowering, type checking and formatter round trips are outside this target.
Cancellation remains covered by the syntax crate's deterministic checkpoint tests.

## Replay without nightly

Run from the repository root:

```text
cargo test --locked -p kagari-syntax-fuzz --lib
```

This discovers every file in `fuzz/seeds/syntax`, also checks empty input, and
generates depth/chain and 16-bit/32-bit lexical-coordinate boundary cases, including
exact 65,535/65,536/65,537-byte sources with Unicode and trailing tokens/comments.
Seeds cover legal expressions, declarations, inline modules, Unicode, recovery
and unfinished strings. They are starting points, not an exhaustive grammar list.

## Coverage-guided run

Install a nightly toolchain and `cargo install cargo-fuzz --version 0.13.2 --locked`.
See the [official setup guide](https://rust-fuzz.github.io/book/cargo-fuzz/setup.html)
for the C++ compiler and platform sanitizer requirements. Keep the repository's
default toolchain unchanged; select nightly only for the fuzz command.

On Windows, use the x64 Native Tools prompt or put the MSVC x64 directory
containing `clang_rt.asan_dynamic-x86_64.dll` on this shell's `PATH`. A successful
build followed by `STATUS_DLL_NOT_FOUND` means the sanitizer runtime cannot be
loaded; fix the runtime search path before treating the campaign as executed.

Create `target/fuzz-corpus/syntax`, then run from the repository root:

```text
cargo +nightly fuzz run syntax --features fuzzing target/fuzz-corpus/syntax fuzz/seeds/syntax -- -max_total_time=60 -max_len=131072 -timeout=10 -rss_limit_mb=2048
```

The first corpus directory receives generated inputs; checked-in seeds stay
unchanged. Cargo uses the default workspace `target` directory. Crash artifacts
go to ignored `fuzz/artifacts/syntax`. The runner's size, timeout and memory limits
are separate from parser limits. The bounded CI job uses the same target with
AddressSanitizer; this is a smoke campaign, not exhaustive fuzz acceptance.

Replay a saved failure and minimize it with the same feature:

```text
cargo +nightly fuzz run syntax --features fuzzing fuzz/artifacts/syntax/CRASH_FILE
cargo +nightly fuzz tmin syntax --features fuzzing fuzz/artifacts/syntax/CRASH_FILE
```

Investigate whether the parser or the oracle violated its documented contract.
After fixing a confirmed defect, retain a minimized seed or add coverage to its
existing semantic test owner; avoid duplicate tests of the same boundary. Keep
generated corpora, coverage and campaign logs out of Git.

## LSP framing and lifecycle targets

The optional `lsp` feature adds the analyzer's narrow `verification` entrypoints.
It does not replace production protocol checks or require libFuzzer for ordinary
corpus replay:

```text
cargo test --locked -p kagari-syntax-fuzz --features lsp --lib lsp_tests
```

`lsp_wire` accepts raw bytes up to 64 KiB. It compares fragmented and contiguous
reads through the production codec, checks terminal framing failures and exact
body-admission lengths, and validates encoded field presence and bounded encoding.
An independently constructed Content-Length frame must preserve every arbitrary
body byte, including invalid UTF-8, and the following frame; sampled EOF cuts
must agree with known boundaries. The raw parser stops after 256 complete frames,
and fragment schedules use at most 256 chunks. Literal envelope/header unit tests
remain the independent oracle for specific rejection codes and message shapes.
This target does not claim to exercise OS writes or actor transport cleanup;
process tests own partial-write/disconnect behavior.

`lsp_documents` exercises production full-sync document ownership and source
coordinates against independent plain strings. Two bytes select per-file and
total byte limits, followed by at most 96 records: operation, URI selector,
signed version byte, payload length and UTF-8-lossy text. The three documents
include a percent-escaped local path and its alias, an untitled URI and an opaque
URI. Operations cover open/duplicate open, full/multiple/
empty changes, rejected range/range-length changes, close/reopen and wrong
language. Source text is capped at 128 bytes per file; larger payloads exercise
unavailable synchronization without allocating multi-megabyte inputs.

The model checks exact text, protocol versions, globally increasing open
incarnations, stable identity within an incarnation and fresh identity on reopen,
independent source revisions, byte accounting and atomic rejection. Closed buffer
metadata must be forgotten. Unavailable open inputs carry no fabricated source;
the real actor/acquisition test owns their disk-shadow behavior.
Up to eight distinct retained source snapshots remain exact after later updates
and close/reopen. A scalar-walking oracle computes UTF-8/UTF-16 positions and
lines for LF, CRLF and lone CR, rejects split scalars/CRLF and out-of-bounds
positions, and checks selected range endpoints. It never uses the production
line index to derive an expected coordinate. Incremental/ranged synchronization
remains deferred to LSP08; here ranged changes must be rejected atomically.

`lsp_session` drives bounded actions against the actual request and output state
owners and an independent lifecycle ledger. Its contract is terminal-response
uniqueness, wire-ID reuse, request retirement and response/output-credit ownership.
The first byte selects a request capacity of one through eight; subsequent
three-byte records select an operation, ID/serial and value, up to 128 operations.
Admission, cancellation, completion, frame grants, valid/forged acknowledgements
and control-output saturation are followed by a drain proving final credit release.
The same bounded records also drive the production `JobCredit`, diagnostic
publisher and shared writer-credit owner. A separate two-context/two-URI model
tracks retained job IDs/tokens, current generations, open incarnations/versions,
root contributions, last-granted sets, replaceable intents and reliable output.
Its fourteen actions include edit/close/reopen, job admission/cancellation,
matching/forged completion, fresh/stale contribution replacement, shutdown,
reliable queue fill, writer grant/acknowledgement and contribution clearing.
Only a matching completion releases worker credit. Cancelled old work can finish
successfully, but cannot publish into a newer generation.
Exact duplicates merge; differing root records retain their provenance; version
negotiation affects wire output without weakening generation checks. Fairness
retains its eight-frame debt when a full reliable queue temporarily makes a
publication ineligible. Final draining proves job/output/publication retirement.

This target exercises the actual synchronous state owners, not a toy server.
The T1 diagnostic test separately holds a real HIR worker's completed result,
admits a newer edit through the real codec/actor/acquisition path, releases the
old completion and verifies current versioned diagnostics, repair and clearing.
It complements fuzzed owner orders without claiming exhaustive thread schedules.

Raw `.bin` wire seeds retain CRLF and invalid-byte witnesses without Git text
conversion. Curated seeds live under `fuzz/seeds/lsp_wire` and
`fuzz/seeds/lsp_session` and `fuzz/seeds/lsp_documents`; generated corpora and
artifacts stay ignored. Run each
bounded ASan campaign after preparing the corresponding directory under
`target/fuzz-corpus`:

```text
cargo +nightly fuzz run lsp_wire --features fuzzing,lsp target/fuzz-corpus/lsp_wire fuzz/seeds/lsp_wire -- -max_total_time=60 -max_len=65536 -timeout=10 -rss_limit_mb=2048
cargo +nightly fuzz run lsp_session --features fuzzing,lsp target/fuzz-corpus/lsp_session fuzz/seeds/lsp_session -- -max_total_time=60 -max_len=65536 -timeout=10 -rss_limit_mb=2048
cargo +nightly fuzz run lsp_documents --features fuzzing,lsp target/fuzz-corpus/lsp_documents fuzz/seeds/lsp_documents -- -max_total_time=60 -max_len=65536 -timeout=10 -rss_limit_mb=2048
```

The CI LSP matrix uses these commands. Compilation time is separate from the
60-second campaign, and the existing Windows sanitizer-runtime setup applies.
No panic recovery, validation bypass or timing-based success oracle is used.
Retain each confirmed finding at its lowest affected owner and replay its actual
transport/actor sequence when the defect involves those integration boundaries.
