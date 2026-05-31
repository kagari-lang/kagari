# Codex Goal Guide for Kagari

This document contains the single Goal Mode prompt and operating rules for executing `docs/implementation-roadmap.md`.

The roadmap is authoritative.
Codex should advance one roadmap step at a time, verify that step, and commit immediately after the step is complete.
Milestones remain acceptance groupings, not commit boundaries.

## Operating Rules

- Execute only by `docs/implementation-roadmap.md`.
- Treat `docs/spec/`, `docs/kagari.ebnf`, and `docs/architecture.md` as source of truth.
- Do not use current code behavior as the standard when it conflicts with specs.
- Break compatibility freely when current code is wrong.
- Prefer structural rewrites over compatibility shims.
- Keep crate boundaries clean and named around stable Kagari concepts.
- Do not preserve old syntax or runtime behavior as deprecated aliases unless a spec explicitly requires it.
- Add tests before or alongside behavior changes.
- Keep each step small enough to leave a reviewable working tree.
- Commit after every fully verified roadmap step with that step's exact conventional commit message.
- Include the step's `Roadmap-Step: Mx.y` trailer in every step commit.
- Do not combine multiple steps in one commit unless the roadmap explicitly marks them as one step.
- Do not advance to the next step while the current step has uncommitted changes.

## Reference Documents

Codex must read these before starting or resuming execution:

- `docs/architecture.md`
- `docs/implementation-roadmap.md`
- `docs/kagari.ebnf`
- `docs/spec/syntax.md`
- `docs/spec/builtins.md`
- `docs/spec/modules.md`
- `docs/spec/module-loading.md`
- `docs/spec/traits.md`
- `docs/spec/reflection.md`
- `docs/spec/typed-path-mutation.md`
- `docs/spec/bytecode.md`
- `docs/spec/artifacts.md`
- `docs/spec/debugger.md`
- `docs/spec/runtime.md`
- `docs/spec/host-interop.md`
- `docs/spec/embedding-api.md`
- `docs/spec/security.md`
- `docs/spec/execution.md`
- `docs/spec/codegen-backend.md`
- `docs/spec/jit.md`

## Unified Goal Prompt

Use this prompt when starting a long-running Codex Goal Mode session:

```text
/goal Implement Kagari according to docs/implementation-roadmap.md.

Reference documents:
- docs/architecture.md
- docs/implementation-roadmap.md
- docs/kagari.ebnf
- all files under docs/spec/

Execution rules:
- Advance strictly by roadmap step, not by whole milestone.
- Milestones are acceptance groupings; steps are the commit units.
- Start by inspecting git status and recent commits.
- Identify the first incomplete roadmap step.
- Execute exactly that step.
- Use docs/spec/, docs/kagari.ebnf, and docs/architecture.md as the source of truth.
- Do not treat the current Rust implementation as authoritative when it conflicts with the specs.
- Break compatibility freely when existing code has the wrong design.
- Remove incorrect legacy behavior instead of supporting both old and new behavior.
- Keep the codebase structural: clear crate boundaries, coherent modules, stable names, and no compatibility hacks.
- Add or update tests for every behavior change.
- Before committing the step, run the relevant package tests from the current milestone plus git diff --check.
- Run the milestone verification commands when the step completes a milestone.
- Commit immediately after each fully verified step using the exact conventional commit message listed for that step.
- Add the exact `Roadmap-Step: Mx.y` trailer listed for that step to the commit body.
- Do not proceed to the next step until the current step is verified and committed.
- Keep the working tree small and reviewable at all times.

Status reporting:
- Report the current milestone and step.
- List completed work, remaining work, verification results, and the commit hash after each step commit.
- If blocked, report the blocking condition and the smallest concrete decision needed.
```

## Resume Prompt

Use this prompt when resuming an interrupted Goal Mode session:

```text
/goal Resume execution of docs/implementation-roadmap.md.

First inspect git status, recent commits, and the roadmap.
Find the first incomplete step by checking roadmap step ids and `Roadmap-Step: Mx.y` commit trailers in git log.
Do not repeat completed step commits.
Continue with exactly one roadmap step at a time.
Verify and commit each completed step with its required conventional commit message.
Specs remain authoritative over the current implementation.
```

## Status Report Format

Codex should report progress using this format:

```text
Milestone: <number and title>
Step: <step id and title>
Status: <not started | in progress | blocked | verified | committed>
Completed:
- ...
Remaining:
- ...
Verification:
- ...
Commit:
- <hash and message, or not committed yet>
Trailer:
- Roadmap-Step: <Mx.y, or not committed yet>
```
