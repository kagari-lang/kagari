# Codex Goal Guide for Kagari

The [implementation roadmap](implementation-roadmap.md) points to the active
[foundation checkpoints](foundation-refactor.md). R01–R18 and the three contracts
in `docs/spec/` govern the foundation work. Earlier M1–M11 milestones are historical.

## Operating Rules

- Read `docs/foundation-refactor.md`, the relevant `docs/spec/` files, and
  `docs/kagari.ebnf` before changing a behavior.
- Work through incomplete R checkpoints in order. Keep each change reviewable.
- Remove superseded behavior directly; do not add compatibility aliases or
  artifact upgrades. Preserve runtime ABI, schema, version, and permission checks.
- Verify each checkpoint with focused tests and `git diff --check`, then make a
  Conventional Commit with a `Roadmap-Step: Rxx` trailer. Multiple commits may
  share a checkpoint when a coherent change needs its own review boundary.
- At the end, run `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, and `git diff --check`.
- Record reproducible measurements for compiler time, edit reanalysis, shared
  code, call overhead, and baseline GC pauses in
  [performance-baseline.md](performance-baseline.md).

## Goal Prompt

```text
/goal Implement the incomplete R checkpoints in docs/foundation-refactor.md.
Use the three semantic contracts and relevant docs/spec/ files as authority.
Verify and commit each checkpoint using a Conventional Commit and a
Roadmap-Step: Rxx trailer. Finish with the documented workspace checks.
```

To resume, inspect the working tree, the R checklist, and recent commit trailers.
Continue the first incomplete checkpoint without repeating completed work.
