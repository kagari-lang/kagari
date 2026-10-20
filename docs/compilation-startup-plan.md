# Fast compilation and eager startup execution plan

## Status and authority

**Planning only; implementation has not started.** Requested after the browser
latency investigation at `98f5c39d9d29449b3856bb155f677c48d37aee99` on 2026-10-20.
This document owns CS01-CS06, their decisions, acceptance and progress. The
[roadmap](implementation-roadmap.md#fast-compilation-and-eager-startup) owns
activation; [measurements](performance-baseline.md#browser-compilation-latency-2026-10-20)
own the existing evidence. Writing this plan does not authorize implementation,
commits, pushes, deployment or remote configuration changes.

Follow [project goals](project_goal.md), [architecture](architecture.md), the
[artifact contract](spec/artifacts.md), [loading contract](spec/module-loading.md)
and [repository rules](../AGENTS.md). This plan does not change language semantics.
When implementation is authorized, execute coherent buildable phases in order.
Commit phases only when authorized, with Conventional Commits and a
`Plan-Phase: CSxx` trailer; explain breaking API changes in the commit body.

## Resuming on another machine

This plan and its linked baseline are the durable handoff; previous chat history,
ignored `target/` probes and the adjacent Vela checkout are not required. Transfer
all four documentation changes together: this file, `docs/README.md`,
`docs/implementation-roadmap.md` and `docs/performance-baseline.md`. Uncommitted or
untracked files do not arrive through a normal clone/pull; use an explicitly
authorized commit/transfer before switching machines. This note does not authorize
committing or pushing them.

At resumption:

1. Inspect repository rules, `git status`, relevant diffs and current HEAD. The
   investigation revision above is a reference, not an instruction to reset the
   checkout. Preserve intervening work and recheck affected owners if they changed.
2. Read this plan, its linked measurement section and the progress ledger. Start
   at the first incomplete phase after implementation is authorized; currently
   that is CS01. Record API decisions and carried errors here, not in a new queue.
3. Use repository-managed toolchain/dependency files and the linked site/browser
   instructions. Install the required WASM target and matching wasm-bindgen CLI
   when browser validation is due; do not depend on tools previously installed
   under another machine's ignored `target/` directory.
4. Existing evidence is sufficient to start implementation. At the final performance
   comparison, a changed machine requires a matched pre-change release baseline and
   post-change measurement on that machine, with environment and product revisions
   recorded. Do not report a ratio between different machines as an optimization
   result or silently redefine the latency goals.
5. Keep local tests focused and report unrun CI separately. Phase commits require
   authorization; push, deployment and remote settings remain separately authorized.

The deferred cross-process disk-cache direction is recorded under
[package follow-ups](implementation-roadmap.md#package-acceptance-and-follow-ups).
It is not a prerequisite or an extra phase of this plan.

## Outcome and finite scope

Optimize the complete development loop: source edit -> useful diagnostics ->
prepared executable -> first test/script entry. This matters for human and
AI-driven repeated compilation, not just Playground presentation.

Required outcomes:

1. Source execution does not construct a mutable deployment envelope merely to
   feed it back through external-artifact preparation. Artifact import/export
   remain distinct operations sharing the same executable contract implementation.
2. Establish each invariant at its owning boundary, retain immutable evidence,
   and validate only new obligations introduced by transformations. This means
   no repeated whole-program proof after an unchanged sealed handoff, not one
   giant function or a literal single traversal of every representation.
3. Eagerly finish static executable validation and required host linking for the
   selected program/dependency closure before admitting script execution. Do not
   move verification, source compilation or dependency loading to first invocation.
4. Make individual verification operations efficient before adding caches. Shared
   program metadata is prepared once with visibility-aware views, not rebuilt for
   every module or instruction.
5. Default development execution uses the interpreter and bounded, inexpensive
   lowering. Heavy optimization/AOT is explicit, never a prerequisite for testing.
   Development and deployment preserve identical typing and safety guarantees.

Not included: a full incremental compiler, persistent compilation daemon/cache,
new script test runner, general JIT/AOT expansion, interpreter dispatch redesign,
Java-style lazy class loading, parallel compiler redesign, new type interning
system, automatic artifact pruning, or a new generic proof-certificate format.
Existing explicit native execution policies remain supported; this plan does not
silently disable JIT requested by an embedding application.

The [review](review.md) retains R7 specialization-round work, R8 semantic reuse,
R16 flow scheduling and identity-lifetime/reachability follow-ups. Inspect overlap
and retain sound foundations, but do not mark those findings fixed or activate
all of them through this plan. Large-project incremental reuse is a later step;
this work must not require recompilation of unchanged code as an architectural
principle, nor promise to implement reuse it does not actually provide.

## Evidence and current ownership

The existing release WASM minimal workload is `fn main()->i32{42}`. Median fresh
session construction plus compile/load was 3646.5 ms; recompilation on a retained
session was 2631.5 ms. One separate 2676.5 ms compile profile attributed about
56.3% to bytecode verification and 12.7% to MIR verification. Trait verification
appeared on three boundaries, approximately 477/492/505 ms. These are sampled
attributions, not exact phase timers or call/allocation counts.

`Ty::within_wire_limits` constructs wire nodes to answer a boolean predicate.
Allocation/free self-time is substantial. Carrying standard-library contracts is
not itself a defect; repeated processing of those contracts is the concern.
Release alone improved fresh setup/compile by 25.2%, not enough to fix latency.
No matched native-target or Vela timing has been measured. Remote download delay
is separate, and the live Pages binary revision/profile was not established.
Existing evidence suffices to begin corrections; another broad benchmark campaign
is not a prerequisite.

| Owner | Current responsibility and problem |
| --- | --- |
| [SDK source compilation](../crates/kagari/src/engine/source.rs) | `emit_bytecode` lowers MIR/bytecode, optionally encodes portable MIR, imports identities and constructs `KbcArtifact`; `compile_to_artifact` is also used for immediate execution. |
| [Compiler bytecode lowering](../crates/kagari-compiler/src/bytecode.rs) | Checks the lowered graph but returns editable records, losing reusable verification evidence. |
| [Bytecode evidence](../crates/kagari-bytecode/src/program/verified.rs) | Existing `VerifiedBytecodeProgram` owns exact scoped records, export targets and suspension evidence. Mutation/extraction discards the seal. Reuse this owner. |
| [Artifact envelope](../crates/kagari-bytecode/src/artifact.rs) | Construction creates verification metadata; consuming preparation verifies externally mutable input again. Serialized flags/hashes do not establish trust. |
| [SDK preparation](../crates/kagari/src/program/mod.rs) | Imports artifact identities, verifies the envelope and optional native input, then constructs `PreparedProgram`. |
| [Runtime entry](../crates/kagari/src/runtime.rs) | Installs/binds a prepared program against the current runtime; stages reload candidates before publication. This is not redundant compilation. |
| [Type traversal](../crates/kagari-types/src/ty/wire.rs), [matching](../crates/kagari-types/src/ty/matching.rs) | Limit predicates allocate wire products; matching performs expensive checks before some obvious mismatches. Preserve validation even when reordering work. |
| [Program verification](../crates/kagari-bytecode/src/program.rs), [trait proofs](../crates/kagari-bytecode/src/trait_bounds.rs) | Rebuild module dependency closures and closure-wide declarations, receiver owners and proof catalogs. |
| [Contract proofs](../crates/kagari-contract/src/types/proofs/mod.rs) | Shared semantic proof owner; keep trait semantics here rather than inventing a bytecode-only solver. |
| [Browser adapter](../crates/kagari-web/src/lib.rs) | Fresh analysis session -> artifact -> preparation -> transactional load. Source execution does not write/read a `.kbc` file, but crosses its mutable object boundary. |
| [CLI](../crates/kagari-cli/src/main.rs), [package CLI](../crates/kagari-cli/src/project.rs) | Standalone source run uses an in-memory artifact; package run additionally publishes serialized output today. |

Native installations, standard authored modules and native-only HIR baselines
already have preparation/reuse mechanisms. Do not add a duplicate global stdlib
cache or claim all native source bodies are rechecked on every compilation.

## Intended architecture

### Two inputs, one executable model

```text
Source / captured package inputs
  -> checked source -> verified MIR -> checked bytecode generation
  -> VerifiedBytecodeProgram (+ matching verified MIR when requested)
  -> PreparedProgram -> runtime binding -> execution

External KBC bytes / editable artifact records
  -> bounded decode / envelope and compatibility checks
  -> executable validation (+ native-input correspondence when enabled)
  -> VerifiedBytecodeProgram -> PreparedProgram -> runtime binding -> execution

Export request
  -> checked compilation result -> explicit portable artifact encoding
```

Exact new API names are decided in CS01. Keep one canonical preparation model,
not a new parallel verified wrapper or a `trusted: bool` shortcut. Source-disabled
builds must still prepare and execute artifacts without frontend dependencies.

The in-process compilation owner may combine bytecode and MIR only when it can
establish that they come from the same lowering generation. Two independently
valid but unrelated programs are not a valid native-preparation pair. Do not
encode/decode MIR merely to establish this correspondence. External portable MIR
retains its independent validation, including canonical correspondence and origins.

Retain exact definition scopes through sealing. Perform necessary identity mapping
before sealing where possible. If a sealed representation must be remapped, the
owning layer must explicitly check the mapping and translate all dependent evidence;
an arbitrary import or matching hash does not preserve a seal automatically.
Dropping the source engine must not invalidate prepared program IDs or retain its
entire source/query history. Runtime-local identity adoption remains necessary.

### Verification obligation matrix

| Boundary | Establish here | Retain or check later |
| --- | --- | --- |
| HIR/source | Names, static types, generic requirements, selected calls/witnesses, source diagnostics | Backends consume checked facts; no syntax resolution in executable consumers. |
| MIR | Typed CFG, operation contracts, control/effect and cleanup invariants | Retain the verified generation; bytecode still checks its own lowering obligations. |
| Bytecode | Bounded shapes/IDs, declared dependency access, ABI/layouts, trait/associated-output contracts, instruction operands/targets, initialization, roots, suspension and exports | Seal exact data and derived facts; preparation does not rerun the same graph proof. |
| External envelope | Decode/resource bounds, versions, integrity, metadata agreement and optional compiler payload | Execute shared semantic checks before sealing; never trust stored verification flags. |
| Runtime binding | Current installed host signatures/access, runtime identities, module generations and call targets | Keep runtime-specific linking and transactional publication. |
| Execution | Dynamic bounds/numerics, host borrows, object ownership, roots, cancellation, traps and depth | Never replace dynamic checks with a static verification claim. |

Example: for a call requiring `T: Eq`, source checking selects the implementation;
lowering carries its executable contract; bytecode validation checks the emitted
call and contract agree. Preparation retains this result. Runtime binding still
checks the actual installed provider. An imported artifact must establish the same
executable fact independently, without reconstructing source inference.

### Efficient eager verification

- Use a bounded read-only type traversal shared with the codec's structural rules.
  The boolean predicate must not materialize a `Vec<Node>` or associated-binding
  output vectors. Bounded traversal scratch is allowed; do not promise zero heap
  allocations by replacing it with unbounded recursion.
- Preserve depth/node/arity limits, illegal associated bindings, canonical wire
  ordering, identity conversion collision detection and cancellation. Codec-only
  ordering work remains in encoding; shared shape rules must not diverge.
- Separate catalog structure validation from module-scoped proof queries. Build
  program-owned declaration/layout/implementation indexes once, then use explicit
  dependency-scope views for each module. An implementation in an unrelated module
  must never satisfy a bound just because it is in a global index.
- Validate malformed unused declarations/projections as required today. Early
  candidate rejection is allowed only after its admission invariants are known;
  an unrelated candidate must not become a hiding place for invalid input.
- Reuse the existing proof semantics and budgets. If memoization is concretely
  justified, scope it to this immutable verification operation and key it by exact
  identities, receiver/substitution, assumptions and dependency visibility. Do not
  retain a process-wide cache across source edits, installation changes or reload.
- Inventory graph-wide, declaration-local and function-local obligations before
  fusing loops. Build indexes once, avoid repeated linear searches, and retain
  clear ownership rather than creating one monolithic verifier.
- Propagate caller cancellation through newly structured bounded work. Do not add
  fresh uncancelled tokens as a convenience. Preserve existing rejection categories
  and deterministic traversal where observable diagnostics depend on order.

Completion is eager for the selected executable closure, not for every project
file that could ever be imported. There is no lazy verification on first call.
Existing runtime allocation or concrete layout materialization is not automatically
source compilation; audit cold paths so no static proof work is hidden there.

### Development versus deployment

Immediate execution retains debug origins, bindings, safe points and suspension
facts. Cheap semantics-preserving lowering remains enabled; required validation
never depends on an optimization mode. Export-only fingerprints, portable MIR
encoding and deployment metadata are not mandatory costs on this path. Explicit
AOT/native consumers retain their existing capabilities and checks.

For package commands, separate acquisition/lockfile policy from artifact production:
`build`/explicit export still publish artifacts; source-only `run` should prepare
captured inputs directly rather than publish an incidental KBC. CS01 must enumerate
and document the changed output/storage contract, and preserve root selection,
source provenance and package diagnostics. Do not silently remove explicit output
requests or mutate lockfile behavior as a side effect of this change.

Keep the current bounded browser analysis-session lifetime. Permanent engine reuse
is not part of this plan: shared append-only identities require a separate lifetime
solution. Correct obvious duplicate preparation only when it follows the existing
installation owner and can be independently validated. Never use reuse as a
substitute for making fresh compilation efficient.

## References and deliberate adaptations

Primary references reviewed on 2026-10-20:

- Lua 5.4.9: [`luaY_parser`](https://www.lua.org/source/5.4/lparser.c.html#luaY_parser)
  constructs an in-memory closure/prototype, and
  [`luaB_dofile`](https://www.lua.org/source/5.4/lbaselib.c.html#luaB_dofile)
  loads and invokes code without requiring a dump/reload roundtrip. This supports
  CS01/CS02's direct compilation-result handoff and CS05's source entrypoints, not
  removal of Kagari's static typing, trait, GC or host validation. The linked 5.4
  source tree may advance; the version reviewed here was 5.4.9. No comparative
  latency or expected speedup is established by this source inspection.
- wasmparser 0.240.0 (the wasm-tools/Wasmtime ecosystem):
  [`FuncToValidate<T>`](https://docs.rs/wasmparser/0.240.0/wasmparser/struct.FuncToValidate.html)
  carries module resources, function identity/type and enabled features into a
  function validator;
  [`FuncValidator<T>`](https://docs.rs/wasmparser/0.240.0/wasmparser/struct.FuncValidator.html)
  separates function-local validation state from module resources. Its
  `define_locals()` and `op()` APIs allow validation of already-parsed inputs,
  rather than requiring a second parsing pass. `into_allocations()` and the next
  `into_validator()` reuse
  [scratch allocations](https://docs.rs/wasmparser/0.240.0/wasmparser/struct.FuncValidatorAllocations.html),
  not verification conclusions. CS03/CS04 should adopt shared resource ownership,
  scoped local state and avoidance of repeated preparation where applicable.
  Scratch reuse is optional and must have a concrete allocation justification;
  do not introduce a process-wide pool or expand into a parallel verifier project.
  WASM's structured control flow and type rules differ from Kagari's CFG, trait
  proofs and GC obligations: this is not evidence that every Kagari check can be
  done in one pass or that module-global indexes may bypass dependency visibility.
- [Rust MIR development guide](https://rustc-dev-guide.rust-lang.org/mir/index.html)
  and [MIR passes](https://rustc-dev-guide.rust-lang.org/mir/passes.html)
  (rolling documentation, accessed on the date above): explicit types, per-body CFG
  and separate build/transform/dataflow responsibilities. CS01/CS02 adopt typed
  downstream facts and focused pass ownership, not Rust's borrowing, LLVM pipeline
  or compile latency. Rust's `Steal` query ownership can avoid cloning but imposes
  ordering constraints; copying that mechanism is not proposed. Kagari must retain
  its GC, host and reload contracts.
- [JVMS SE 25, sections 4.7.4 and 4.10](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-4.html#jvms-4.10):
  supplied stack-map facts are checked against instruction transitions and incoming
  control flow, not blindly trusted. Adopt explicit checkable facts and separation
  of structural and semantic obligations, not JVM generic erasure, relaxed interface
  assignability or lazy class loading. JVM stack maps are not Kagari GC root maps.
- The existing [review's primary references](review.md#primary-implementation-references)
  cover Rust 1.90.0 instance collection and context-owned identities. R7 worklists
  and persistent incremental dependency tracking remain separately scoped.

Supplementary local comparison: Vela revision `5c511b811`, inspected 2026-10-20.
Its `vela_playground_wasm/src/lib.rs` calls `Engine::compile_source` then
`Runtime::new_compiled`; private `CompiledProgram` fields retain bytecode, verified
MIR and binding data, while a separate portable-program linker handles imports.
It still verifies bytecode and linked images, recreates its engine per Run and
merges standard registries. Its page initializes WASM before reporting ready and
its Pages recipe uses release. These observations are recorded here so the plan
does not require the adjacent checkout. Vela is dynamically typed without Kagari's
full generic/trait proof obligations; its reported fast UI is not a matched timing
result or a reason to remove Kagari checks.

The design therefore adapts existing Kagari seals and proof owners. No new universal
IR, JVM-style loader or separate trusted bytecode semantics is justified.

## Phases and acceptance

### CS01 — Freeze obligations and consumer migration

- Complete the matrix above against concrete MIR/bytecode/preparation call sites,
  including hidden export metadata and native correspondence checks.
- Specify the canonical source-to-prepared API, paired MIR/bytecode provenance,
  definition-scope ownership and mutation invalidation rules.
- Classify callers as immediate execution, export, external import or deliberate
  tampering tests. Include standalone/package CLI, browser, SDK examples, debugger,
  reload, shared test routes and AOT consumers.
- Finalize package run output behavior and a finite list of affected tests here.
  Select one existing representative trait/host workload for timing alongside 42.

Acceptance: every removed/reused check has an owner and evidence lifetime; no
validation bypass, public forgeable seal or lost native/debug capability. Existing
measurements are sufficient; no broad new profiling milestone is required.

### CS02 — Retain verified compilation and prepare directly

- Make bytecode lowering retain its checked graph through the existing seal.
- Introduce the canonical SDK source/checked-package preparation route and matching
  in-memory native input ownership; keep artifact export/import explicit.
- Derive export verification metadata from evidence when the exact sealed program
  is retained. Editable/imported records still require complete verification.
- Update direct compiler/SDK consumers coherently; do not keep compatibility
  forwarders or an intermediate unvalidated production path.

Acceptance: a source program reaches `PreparedProgram` without KBC construction,
portable MIR roundtrip or a repeated full bytecode graph proof. A prepared result
survives source-session disposal, supports independent runtimes and preserves
native preparation where requested. Forged/mismatched artifacts still fail.

### CS03 — Remove allocation-heavy type validation

- Replace wire-output construction in limit predicates with shared bounded shape
  traversal; preserve the real encoder's ordering and identity conversion checks.
- Remove identity substitutions that only recreate an unchanged type for validation
  when the shared validator can establish the exact same invariant.
- Reorder matching only behind validated-input boundaries; review all consumers of
  changed type helpers, not merely the sampled trait call site.

Acceptance: existing valid/invalid limit and matching contracts remain identical;
limit-only checks do not construct wire products. Focused allocation/work evidence
shows the eliminated work without turning private layout details into test policy.

### CS04 — Program-owned verification preparation

- Separate graph/index preparation, structural declaration validation, scoped trait
  queries and function verification, retaining the shared contract proof owner.
- Reuse declaration/receiver/implementation preparation across module verification;
  preserve dependency visibility and exact generic assumptions in every query.
- Remove superseded closure-wide setup and repeated validation of unchanged shared
  inputs. Keep unavoidable obligation-dependent work explicit and bounded.
- Review MIR and native handoffs for repeated semantic setup on the changed path;
  do not expand into R7/R16 algorithms without an explicit scope decision.

Acceptance: adding modules sharing one dependency does not rebuild that dependency's
structural proof catalog per module. Local obligations still run, invisible impls
are rejected, unused malformed contracts are rejected, cancellation remains bounded,
and source and external artifact paths use the same executable semantics. Existing
trait tests supply a fixed closure rather than exercising graph visibility: add a
focused graph-level contract case proving that a reachable implementation satisfies
a bound, an unrelated implementation cannot satisfy it, and an invisible competing
implementation does not introduce ambiguity.

### CS05 — Immediate-run consumers and browser products

- Migrate browser source Run, CLI source/package Run and ordinary source test helpers
  to the direct route. Retain distinct artifact roundtrip/source-free test routes.
- Preserve failed-candidate rollback, busy handling, cancellation/reset cleanup,
  rooted results, debugger metadata, host binding and pinned reload.
- Keep package build/export and AOT production explicit; update affected SDK, CLI,
  package and embedding documentation/examples with intentional API/output changes.
- Use workspace release for published WASM products, updating exact-product recipes,
  provenance and affected orchestration tests together. Rust release optimization
  of the tool is distinct from heavy optimization of each Kagari script.
- Keep Worker isolation and fresh analysis lifetimes. Preloading, CDN changes and
  permanent engine reuse are not prerequisites or substitutes for compiler fixes.

Acceptance: real source routes no longer indirectly invoke artifact preparation;
real external artifacts remain independently checked and executable. Source and
source-free browser products retain separate dependency graphs and coherent hashes.
Local product verification does not authorize a push or Pages deployment.

### CS06 — Focused correctness, latency and handoff

- Run the selected integration set and outstanding required lightweight checks.
  Regenerate affected disposable fixtures once at this coherent checkpoint.
- Compare fresh and repeated source execution, external artifact loading and a
  small representative growth workload under the measurement policy below.
- Audit before-entry versus first-call work, source-session disposal and repeated
  failure/reset cleanup; report retained memory separately from allocation traffic.
- Move lasting contracts to architecture/spec owners and results to the performance
  baseline. Reconcile roadmap/review links; remove this plan after completion and
  move any pending CI or unresolved goals to their lasting owner.

Acceptance: all in-scope integration failures resolved; structural and performance
results reported independently from unrun CI. A missed goal requires a concrete
remaining diagnosis, not a completion claim or silently expanded optimization queue.

## Verification and performance acceptance

### Focused correctness selection

The following is a phase-selection menu, not a suite to run at every checkpoint.
Use existing tests; add cases only for distinct uncovered boundaries. These commands
are source-inspected examples, not results executed while writing this plan.

| Phase / contract | Existing focused command or owner |
| --- | --- |
| CS02 seals and mutation | `cargo test -p kagari-bytecode --lib artifact::identity_tests` |
| CS02 native input | `cargo test -p kagari --lib source_exports_matching_native_input_or_explicit_bytecode_only_artifacts`; `mismatched_artifact_native_input_is_rejected_before_loading` under the same package/target |
| CS03 type boundaries | `cargo test -p kagari-types --lib ty::wire::tests`; select affected matching/substitution cases after API review |
| CS04 associated outputs/projections | `cargo test -p kagari-bytecode --lib trait_bounds::tests`; add the distinct graph-visibility case specified in CS04 |
| CS04 lowered call contracts | `cargo test -p kagari-compiler --lib tests::bytecode::host_contracts` |
| Bytecode flow/roots regression | Select affected cases from `cargo test -p kagari-compiler --lib tests::bytecode::validation` |
| Runtime host binding | `cargo test -p kagari --lib offline_host_facades_preserve_linking_and_backend_call_traces` |
| Reload | `cargo test -p kagari --lib prepared_reload_preserves_abi_validation_and_the_previous_version_on_failure` |
| Package provenance | `cargo test -p kagari --lib captured_packages_share_the_sdk_pipeline_and_retain_generations` |
| Package CLI | `cargo test -p kagari-cli --bin kg project_builds_select_roots_and_store_independent_artifacts`; adjust run assertions without removing build/export coverage |
| Browser lifecycle | `cargo test -p kagari-web --lib exclusive_run_cancellation_reset_and_failed_candidates_preserve_state` |
| Source origins | `cargo test -p kagari-web --lib source_diagnostics_retain_origin_and_utf16_ranges` |
| Debugger | `cargo test -p kagari-vm --lib optimized_breakpoints_preserve_branch_paths_and_binding_state` |

Source-free acceptance must run in a separate Cargo feature invocation. Produce the
fixture with `cargo run -p kagari --example regenerate_feature_artifact`, then select:

```text
cargo test -p kagari --no-default-features --test artifact_features portable_artifact_executes_without_source_compilation
cargo test -p kagari --no-default-features --test artifact_features source_free_native_imports_reject_forged_signatures
cargo test -p kagari --no-default-features --features native --test artifact_features real_cranelift_compiles_portable_artifact_without_source
```

Preserve opaque-MIR bounds versus native-MIR interpretation feature coverage if that
boundary changes. Fixture production with source enabled is not source-free proof.
Native-host web unit tests do not establish browser acceptance: use the existing
[product harness](../site/scripts/wasm-products.mjs),
[Worker harness](../site/scripts/wasm-worker.mjs) and
[Playground browser harness](../site/scripts/playground-browser.mjs) with their
maintained [site instructions](../site/README.md), selecting affected cases.
Do not invent new broad browser/platform matrices for this task.

Each Rust implementation checkpoint requires:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Review imports/re-exports, exact seal constructors, visibility, identity lifetimes,
effective LOC and responsibility boundaries manually. No blanket structure
exceptions. Documentation-only steps need content/link/diff checks, not builds.
Full-workspace tests and complete feature/backend/platform matrices remain CI-only.
Use `--no-fail-fast` for multiple selected test targets; repair/rerun only failing
or newly affected cases. Do not repeat unchanged known failures without a repair.

### Work and latency gates

Hard architectural acceptance:

- Direct source preparation does not enter mutable artifact preparation or encode
  portable MIR for an interpreter-only request.
- One full bytecode validation establishes the seal for a newly generated graph;
  unchanged handoffs do not repeat it. Transformation/linking obligations remain.
- Limit predicates do not materialize wire products; shared declaration setup is
  program-owned rather than repeated for each module. Demonstrate actual work with
  narrowly scoped test instrumentation or profiles, not only renamed functions.
- Static verification/linking completes before execution admission. Loading a
  prepared program into another runtime still checks that runtime's host bindings.

Proposed local latency goals, not predicted or measured outcomes: for the existing
minimal workload on the recorded machine/browser with release WASM already
initialized, target **at most 500 ms median source compile/prepare/load** and
**at most 1500 ms median fresh-session construction plus compile/prepare/load**.
The latter prevents hiding costs in initialization; retained-session timings must
not be the only evidence. These are development feedback goals, not universal
hardware/remote-network guarantees. An instant/sub-100-ms result is not promised.

Use the documented fresh/retained-session method with at least five samples per
condition at final comparison; retain every sample and separate any explicit warmup.
Recreate a matched baseline if toolchain, workload or environment has materially
changed. Keep engine installation, source features and script optimization settings
comparable. Measure execution/reset separately and verify exact results/effects.
Record medians and sample ranges; do not add overlapping sampled costs.

Add a small native-release comparison of the same SDK operations, separating engine
creation, compilation/preparation, runtime linking and first call. Native speed is
currently unknown; do not extrapolate WASM percentages. Source/bytecode and optional
native-backend timings must be labelled separately.

After the fixes, use a bounded growth probe (for example 1, 8 and 32 modules sharing
one dependency, plus one trait-heavy fixture) to expose repeated shared setup.
Record functions, instructions, declarations, obligations and type sizes, not only
source LOC. Do not claim universal linear verification: dependency edges, generic
instantiations and local dataflow have real costs. Record body-only versus public
signature edit costs descriptively without claiming new incremental reuse.

A missed latency goal triggers one bounded architectural reassessment and related
repair batch within this scope. Report the remaining cause and request a scope
change if needed; do not relax checks, silently lower the goal, endlessly add phases
or treat a partial speedup as completion. Run full timing comparisons at coherent
boundaries, not after every edit. Keep probes/logs under ignored `target/`, with
methods, results, environment/profile/features/cache state and limitations retained
in tracked documentation. No new measurements are required before CS02 begins.

## Progress ledger

| Phase | Status | Evidence / next action |
| --- | --- | --- |
| Plan | Drafted | Browser evidence, Lua/wasmparser/Rust/JVMS mechanisms, Vela comparison and cross-machine handoff recorded; no implementation or performance improvement claimed. |
| CS01 | Not started | Freeze exact API/provenance rules, obligation inventory and package run output contract. |
| CS02 | Not started | Direct sealed compilation/preparation. |
| CS03 | Not started | Shared read-only type validation. |
| CS04 | Not started | Scoped program-level verification preparation. |
| CS05 | Not started | Immediate-run consumers and release browser products. |
| CS06 | Not started | Focused integration, measured goals and documentation handoff. |

No new build/test failures were established by this documentation task; no code
validation was run. Existing CI/performance findings remain with their current
owners. Update this ledger with actual commands, selected-scope rationale, remaining
CI scope and any carried errors when implementation changes it.
