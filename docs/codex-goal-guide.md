# Codex Goal Guide for Kagari

Repository-wide engineering and agent workflow rules live in
[AGENTS.md](../AGENTS.md). This guide describes goal execution and resumption;
the active plan owns phase-specific acceptance and intermediate-error rules.

The [implementation roadmap](implementation-roadmap.md) points to the active
[MIR and crate architecture refactor](mir-architecture-refactor.md).
The completed [foundation checkpoints](foundation-refactor.md) and subsequent
language extensions are historical implementation records. Their current semantic
contracts in `docs/spec/` remain authoritative; do not restart R01-R18.

## Operating Rules

- Read the active plan and its progress ledger, relevant specifications, and the
  working-tree diff before continuing. Follow A00-A05 in order.
- Complete A00 structure cleanup and pass every CI gate before starting A01 or
  changing MIR/crate boundaries. Existing structure, lint, build and test failures
  belong to A00, not deferred final-integration work.
- LOC and re-export placement allow narrow exceptions backed by concrete design
  evidence. Review and record them under the structure-check policy; do not force
  harmful splits or add bulk exemptions for existing findings.
- Replace obsolete crates, APIs and formats directly. No compatibility aliases,
  forwarding crates, parallel semantic implementations or artifact upgrades.
  Preserve runtime ABI/schema/version/permission validation.
- After A00 passes, A01-A04 phases and commits may fail compilation or tests.
  Record the command, representative diagnostics, cause and owning follow-up phase in the
  plan's ledger and disclose broken builds in commit bodies. Continue the planned
  migration without building compatibility scaffolding or asking for permission
  solely because intermediate wiring is incomplete.
- Run focused verification where possible and `git diff --check` at checkpoints.
  Use Conventional Commits with `Architecture-Step: Axx` trailers; several commits
  may belong to one phase. Keep scope completion separate from build status.
- Use the configured O1 profile, default target directory and default Cargo
  parallelism. Do not create another target directory for each checkpoint.
- At A05, resolve all carried errors, run the acceptance/feature matrix and pass
  `uv run --locked scripts/check_structure.py`,
  `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, and `git diff --check`.
- Record durable measurement summaries with cache state and separate build/test
  times. Target logs are disposable; they must not be the only progress record.
- Migrate the existing Cranelift subset. Expanded JIT coverage is the next native
  priority; LLVM remains a late independent track and is not a completion gate.

## Goal Prompt

The complete goal prompt and acceptance criteria live in
[the execution plan](mir-architecture-refactor.md#goal-prompt). A short entry is:

```text
/goal Implement docs/mir-architecture-refactor.md through A00-A05.
Finish A00 source structure cleanup and pass every CI gate before starting A01.
Follow its crate boundaries, no-compatibility policy, intermediate-error rules,
Conventional Commit checkpoints and final acceptance matrix. Keep its progress
ledger current. LLVM and expanded JIT coverage remain deferred.
```

To resume, inspect the working tree, A checklist, known-error ledger and recent
`Architecture-Step` trailers. Continue unfinished work without replaying completed
phases. Do not mark the goal complete while final integration errors remain.
