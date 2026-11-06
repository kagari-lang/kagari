# Syntax fuzzing

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
