# MIR and Crate Architecture Refactor

Status: A00-A02 complete; A03 runtime/artifact integration is in progress; workspace build is broken.

This is the active architecture execution plan linked from
[implementation-roadmap.md](implementation-roadmap.md). It follows the completed
[foundation refactor](foundation-refactor.md) and subsequent language extensions.
The value, failure, module, host, and security specifications remain authoritative
for observable behavior. This document owns the new crate boundaries and migration
order; older descriptions of the monolithic IR crate and bytecode-fed native
backend are the implementation being replaced, not alternative target designs.

## Objective and Scope

First correct the existing project's source structure and pass every CI check.
This is the mandatory A00 prerequisite, not work deferred to final integration.
Do not begin MIR or crate-boundary migration until A00 has passed its exit gate.

Evolve the existing typed IR into Kagari MIR and establish one verified execution
contract for bytecode generation and native compilation. Preserve the implemented
language, interpreter, embedding APIs' capabilities, artifact execution, debugging,
GC, host reentry, and hot reload while replacing their internal boundaries.

The exit workspace contains the thirteen active crates listed below. Remove
`kagari-ir` and replace `kagari-jit-cranelift` with
`kagari-codegen-cranelift`. Do not keep forwarding crates or renamed API aliases.

The existing Cranelift subset must work through the new boundary. Expanding JIT
coverage beyond that subset is a subsequent track, prioritized before LLVM.
`kagari-codegen-llvm` is a late future extension: do not create a placeholder crate,
add LLVM dependencies, or implement LLVM/JIT/AOT tooling in this track.

## Execution Rules

- Work through A00-A05 in order. A00 establishes a clean, passing starting point
  before any architectural migration. Each phase is a substantial unit,
  not a queue of one-file checkpoints. Several coherent commits may share a phase.
- Replace old models directly. No compatibility wrappers, deprecated re-exports,
  dual lowering paths, old artifact readers, or migration adapters.
- A00 cleanup commits may contain unresolved findings, but A00 cannot close and
  A01 cannot start until every CI check passes. Do not carry pre-existing failures
  from A00 into the migration phases.
- After A00 passes, intermediate commits and A01-A04 phase transitions may fail
  compilation and tests. Do not build temporary compatibility scaffolding to make
  a migration checkpoint green. This allowance does not waive the A00 exit gate.
- Record broken commands, representative diagnostics, their cause, and the next
  owning phase in the progress ledger. Distinguish expected unfinished wiring
  from unexpected failures. Continue work that resolves them instead of treating
  planned intermediate errors as a reason to stop for approval.
- A completed intermediate phase means its stated ownership changes are done;
  it does not claim downstream integration is working. Keep outstanding errors
  visible until resolved. Never hide them by deleting useful tests, suppressing
  diagnostics, returning fake success, or leaving production `todo!()` stubs.
- Run focused checks when the affected units can build. Attempt relevant checks
  at phase boundaries and record unavailable validation honestly. Run
  `git diff --check` for every checkpoint. Full workspace and structure checks are
  mandatory at both A00 and A05; neither may close with known failures.
- Use Conventional Commits with `Architecture-Step: Axx` trailers. Mark breaking
  public API/format changes with `!` and describe them. If a commit leaves the
  build broken, record that in its body and the ledger.
- Update this checklist, affected specifications, examples, and the roadmap as
  their implementation changes. English is used for source comments and docs.
- Preserve ABI, schema, version, bounds, handle ownership, and permission checks.
  No compatibility support does not mean trusting unvalidated runtime inputs.
- Use the workspace's O1 development/test profile, default target directory,
  and default Cargo parallelism. Do not create per-phase target directories or
  restore `-j 2` without a measured reason. Do not routinely clean caches; measure
  build and test execution time separately and record the cache state.

## Target Crates and Ownership

| Crate | Owned responsibilities | Must not own |
| --- | --- | --- |
| `kagari-common` | Source identities/database, spans, diagnostics, cancellation, foundational limits and shared primitives | MIR, bytecode, runtime state, backend frameworks |
| `kagari-syntax` | Tokens, lexer, parser, Rowan CST, AST views and syntax recovery | Type checking or execution |
| `kagari-hir` | Recoverable semantic analysis, resolution, inference, traits, visibility, checked results and tool queries | Runtime layouts or backend lowering |
| `kagari-abi` | Concrete executable type identities, signatures, logical layout contracts, target layout descriptors, intrinsic/helper identities and signatures, version/fingerprint definitions, backend-neutral native invocation/lifetime contracts | HIR types, GC implementation, machine-code builders or mutable runtime state |
| `kagari-mir` | Concrete typed CFG, operations, effects, origins, verification, dataflow, public optimization passes and complete backend handoff; portable MIR encoding/decoding when requested | Source parsing, name/type resolution, HIR queries or runtime ownership |
| `kagari-compiler` | Compilation orchestration, bounded reachable monomorphization, HIR-to-MIR and MIR-to-bytecode lowering, link-description construction | Executing scripts, runtime installation or backend-specific IR |
| `kagari-bytecode` | Interpreter instruction/program model, bytecode validation, bounded canonical codec and `.kbc` envelope | HIR/MIR lowering, source compilation, GC or native compilation |
| `kagari-codegen` | Native compilation interface, target/options, capability checks, diagnostics and compilation products, consuming verified MIR and explicit link/ABI descriptions | Script invocation, mutable runtime state or concrete backend dependencies |
| `kagari-codegen-cranelift` | MIR-to-CLIF translation, Cranelift-specific legalization, code generation and code-memory ownership | Independent language semantics, heap, host policy or frame driver |
| `kagari-runtime` | Values, heap/GC/roots, object mutation, host bindings, authority, budgets, sessions, linked immutable versions, native installation/invocation services and reload | Parsing, semantic analysis, MIR optimization or backend compiler implementation |
| `kagari-vm` | Bytecode execution, the existing frame driver, interpreter debugging and native-call integration | Another heap, source compilation or dependency on a concrete native backend |
| `kagari-embed` | Host-facing SDK, feature selection, compile/load/execute/reload orchestration and backend registration | Duplicate lowering, verifier or execution semantics |
| `kagari-cli` | Arguments, filesystem IO, command orchestration and presentation | Compiler or runtime rules |

Future only: `kagari-codegen-llvm` implements the same native compilation
interface. LLVM does not define a different Kagari language profile or runtime.

Keep GC, host interop, reload, optimization passes and bytecode encoding as modules
inside their owning crates. Do not introduce separate GC, optimizer, artifact,
linker or standard-library crates merely to mirror this table's sub-responsibilities.

### Dependency Constraints

Arrows below mean production dependencies; common/ABI edges are abbreviated:

```text
syntax -> common
hir -> abi, syntax, common
abi -> common
mir -> abi, common
bytecode -> abi, common
compiler core -> mir, bytecode, abi
compiler source feature -> hir, syntax
codegen -> mir, abi
codegen-cranelift -> codegen, mir, abi, Cranelift libraries
runtime -> bytecode, abi, common
vm -> runtime, bytecode, abi
embed -> runtime, vm, bytecode; optional compiler/codegen/backend features
cli -> embed and explicitly enabled tooling facilities
```

- MIR has no production dependency on HIR or syntax. Replace embedded HIR
  `TypeId`, generic binders and builtin identities with concrete executable
  identities or lower-level shared primitives as appropriate.
- Runtime, bytecode and VM have no transitive production dependency on the source
  frontend, MIR, codegen or concrete backend libraries. Source-driven unit-test
  fixtures may have dev-dependencies; verify production graphs separately.
- Native backends compile without `Runtime`, `LoadedModule`, Rust `Value`, source
  text or HIR. They receive immutable link descriptions and helper declarations.
- Codegen may use MIR internally, but runtime installation consumes only
  backend-neutral descriptors and ownership handles. Place this small execution
  contract in ABI; do not make runtime depend on codegen to store native code.
- The compiler's MIR-to-bytecode path must be usable without its source feature.
  This allows artifact consistency checks without dragging in HIR or syntax.
- `stdlib/*.kgr` remains the authoritative source API surface. HIR owns semantic
  declaration processing; ABI owns stable execution identities/contracts;
  runtime owns implementations. Generate or validate those mappings rather than
  duplicating standard signatures across crates. Retain offline host declarations.

## Starting Implementation and Migration Map

This is the planning inventory; refine it during A01 without preserving obsolete
package boundaries merely because a file currently lives there.

| Current location | Destination |
| --- | --- |
| `kagari-ir/src/module/{abi,layout,types,numeric,...}` | Separate shared executable contracts into ABI from MIR-specific values/operations |
| `kagari-ir/src/module/{function,instruction,verify,...}` | MIR representation, verifier and analyses |
| `kagari-ir/src/lower` and source-facing `program.rs` orchestration | Compiler source lowering and monomorphization |
| `kagari-ir/src/bytecode/lower.rs` and compilation/link construction | Frontend-free compiler core |
| `kagari-ir/src/bytecode/{instruction,module,verifier,artifact,...}` | Bytecode model, validation and codec; separate shared ABI facts |
| `kagari-runtime/src/backend.rs` | Codegen compilation API versus ABI/runtime installation and invocation contracts |
| `kagari-runtime/src/jit_abi.rs` | ABI declarations versus runtime helper implementations |
| `kagari-jit-cranelift` | `kagari-codegen-cranelift`, switched from bytecode input to MIR |
| Compile orchestration embedded in `kagari-embed` | Compiler implementation behind the SDK facade |
| HIR builtin surfaces re-exported through IR to runtime | Source declaration processing in HIR, shared execution contracts in ABI |

Preserve logical identities through these moves. Files with mixed responsibilities
must be split; moving an entire mixed module into `common` or ABI does not satisfy
the dependency boundary.

### A01 frozen migration inventory (base `4d82fcb`, 2026-09-28)

The A00 exit revision is the comparison baseline. The following inventory includes
all current execution/compiler owners; directories include their child modules
and tests. Moves must preserve behavior and update consumers directly.

| Current files / public entrypoints | New owner / required separation |
| --- | --- |
| HIR `types.rs::BuiltinType`, builtin `surface.rs::StandardIntrinsic` and standard enum/trait identities | ABI scalar and standard execution identities; HIR imports shared identities while keeping semantic interpretation |
| HIR `build.rs`, `build/{api,implementations}.rs`, `builtin/{surface,declarations,traits,numeric}.rs`, `stdlib/*.kgr` | Generated declaration descriptors and execution mappings must agree with the authoritative stdlib; source semantic processing stays in HIR, no HIR dependency from ABI |
| IR `module/abi/{mod,wire,verify}.rs` | ABI signatures, nominal identities, bounded wire types, portable trait/layout checks; HIR conversions and source signature construction move to compiler source |
| IR `module/{types,numeric,layout,contracts,host}.rs` and shared operation identities in `instruction.rs` | ABI physical/semantic execution contracts and helper signatures; source conversions move to compiler; MIR-specific operand checking stays MIR |
| IR `module/{function,instruction,ids,verify}.rs` and `module/verify/*` | MIR CFG, concrete instances, operations, verifier; replace HIR type slots and instance arguments with concrete ABI types |
| IR `lower/*`, `program.rs::lower_program_to_ir` | Compiler source lowering, bounded monomorphization and source link construction |
| IR `program.rs::{VerifiedIrProgram,verify_program}`, `bytecode/lower.rs` and `lower/debug.rs` | MIR verified program/link facts versus frontend-free compiler bytecode emission |
| IR `bytecode/{instruction,module,program,access,trait_bounds,verifier,artifact}.rs` and children | Bytecode model, linked verification, resource/integrity checks and bounded codec; replace frontend trait/type queries with portable ABI checks |
| IR `decode_limits.rs` | Shared ABI wire bounds used by MIR/bytecode codecs without a reverse dependency |
| IR `tests/*` and inline ABI/artifact suites | Compiler integration tests for source-driven behavior; contract/codec tests remain with their owner, without production frontend edges |
| Runtime `backend.rs` | ABI native descriptors/ownership, codegen compile interface and runtime installation/invocation; remove backend invocation from compiler traits |
| Runtime `jit_abi.rs` | ABI scalar calling representation/helper declarations versus runtime callback implementations |
| `kagari-jit-cranelift/src/lib.rs` | Rename to codegen-cranelift and migrate scalar compilation from bytecode to verified MIR in A04; native invocation belongs to runtime |
| Embed `lib.rs::{check,compile,compile_program,compile_artifact}` and compile options/errors | Compiler source orchestration behind the SDK; runtime loading/execution and feature/backend selection remain SDK responsibilities |
| Runtime/VM/SDK/CLI/examples/tests imports and manifests | Direct owner imports, thirteen workspace packages, updated feature graph and no old IR facade |

The baseline production graph has IR -> HIR; runtime -> IR; VM -> runtime + IR;
Cranelift -> runtime + IR; embed -> HIR + syntax + IR + runtime + VM. These are
migration inputs, not accepted final edges. HIR's build currently parses stdlib
through syntax/common and emits `standard_api.rs` into `OUT_DIR`. The extraction
must keep one generated declaration mapping, preserve offline host declarations,
and distinguish build/dev dependencies from production graph checks.

A01 exits when real ABI/MIR/compiler/bytecode/codegen ownership is present
and source-analysis types are removed from execution contracts. A02 owns completion
of compiler analyses/lowering; A03 owns SDK feature and artifact wiring; A04 owns
Cranelift translation and code-memory lifecycle. Intermediate errors are recorded
at each checkpoint rather than repaired with compatibility facades.

## Shared MIR and Execution Contract

MIR is an evolution of the existing typed IR, not another representation inserted
between HIR and the old IR. Its required contract is:

- Concrete semantic types remain available independently of VM slot representation.
  Preserve integer width/signedness, numeric policies, concrete generic instances,
  nominal layouts, collection access contracts and interface/closure signatures.
- Explicit CFG and once-only evaluation preserve short circuiting, propagation,
  compound assignment, mutation commit order, trap order and completed effects.
- Calls distinguish static, interface, closure, host and helper targets. Backend
  lowering performs no fallback name lookup or fresh trait/type inference.
- Effects describe reads, writes, allocation, traps and calls that can reenter or
  affect aliases. A readonly view is not proof that its referent cannot change.
- Verification covers operand/call/layout contracts, CFG and initialization;
  analyses add program-point value/root liveness, debug availability, safepoints
  and logical budget placement. A sealed handoff is constructible only after
  required verification and analysis. Transformations invalidate/recompute facts.
- Start with the existing local/temp CFG form; strict SSA is not an entry gate.
  Implement bounded constant/branch simplification and dead pure operation cleanup
  where justified, retaining trap and budget behavior. Machine-level optimization
  and register allocation belong to the native frameworks.
- MIR names logical roots; native lowering maps them to machine locations. Keep
  the nonmoving collector and explicit rooted host handles. Moving/generational GC
  is not part of this track.
- Logical budget charges are independent of physical opcode count. Initially
  preserve the established accounting and failure points, represented explicitly.
  Do not silently redefine budgets when simplifying or lowering instructions.
  Budget batching requires separate evidence of preserved partial-progress and
  termination semantics, not merely equivalent results without limits.
- Origins, lexical scopes and inline/call provenance must survive lowering.
  Preserve original error origins and tracing; optimized-out locals must not be
  reported as if a guessed value were still available.
- Execution versions fix layout, calls and dependency identities. Native code and
  captured values retain their versions. Code caches are per immutable version
  and target/ABI/options, not duplicated for every runtime.

The runtime ABI defines both marshalled public boundaries and the representation
contracts needed for future typed native calls. It does not require every native
temporary to be a tagged VM `Value`. Full compact container storage, aggressive
unboxing, escape analysis and specialized native calling conventions are later
performance work, not required to finish this refactor.

## Artifact and Feature Boundary

Support three reviewable configurations:

1. Bytecode-only embedding: load validated `.kbc` and execute without compiler,
   HIR, syntax, MIR or native backend production dependencies.
2. Source-enabled embedding: enable the compiler source feature and the existing
   compile/check/tooling capabilities.
3. Native-enabled embedding: enable codegen plus a registered backend. Native
   compilation of artifacts must work without source text or frontend dependencies.

Compiler-produced artifacts intended for later native compilation carry a bounded,
versioned portable MIR section with the necessary concrete metadata. MIR owns its
codec; the `.kbc` envelope treats it as an optional section without a MIR dependency.
Do not serialize arena addresses, Rust pointers or analysis-snapshot-local IDs.
The section is a compiler representation, not trusted executable proof.

Bytecode-only loading validates envelope limits, integrity and bytecode contracts;
native preparation additionally decodes/verifies MIR, recomputes necessary analyses
and checks it against the bytecode/manifest before installing native code. Use the
frontend-free MIR-to-bytecode lowering to establish canonical correspondence;
matching hashes of independently supplied payloads alone do not prove semantic
equivalence. Run this preparation before executing user effects, not halfway
through a running function. Explicit bytecode-only exports may omit native input;
report their native compilation ineligibility honestly.

Raise affected artifact, ABI and interface versions when their contracts change.
Reject older versions before execution; do not reserve an arbitrary version number
in this plan or add a compatibility decoder. In-memory and serialized native input
must refer to the same concrete functions, layouts and dependency identities.

## Ordered Phases

### A00 — Correct Existing Structure and Pass CI

- Start with the full [structure audit](structure-checks.md) and resolve every
  existing finding: production wildcard imports, verbose paths at use sites,
  repeated parent traversal, misplaced re-exports and oversized source/test files.
- Treat LOC and re-export placement as design defaults. Where a cohesive file or
  deliberate API boundary is demonstrably preferable, record a reviewed, bounded
  exception with concrete evidence under the structure-check policy. Do not split
  code or remove a useful re-export merely to satisfy a number or filename rule.
- Replace globs with explicit imports, import from actual owners, and keep only
  intentional facade exports. Split oversized files by responsibility using normal
  module boundaries. Do not hide code in macros, string fixtures or ignored files,
  add forwarding layers, widen visibility, or weaken checks to reduce findings.
- Keep the current crate graph and execution model during this cleanup. Preserve
  language behavior and useful test coverage; update consumers directly where
  import/module paths change. MIR extraction and new crate ownership start at A01.
- Run checker regression tests and all commands used by the `structure` and
  `rust` CI jobs, plus `git diff --check`, on the completed cleanup revision:

  ```text
  uv run --locked scripts/check_structure.py --self-test
  uv run --locked scripts/check_structure.py
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  git diff --check
  ```

- Fix any formatting, lint, build or test failures uncovered by these gates.
  Record the validated revision, commands and results in the ledger. Record hosted
  CI status when available; do not report pending or failed CI as passing or reuse
  results from an earlier revision with different code.

Phase exit: every finding is fixed or covered by a justified, narrowly bounded
exception; the checker, checker tests and every CI gate pass for the cleanup
revision. No pre-existing failure is deferred. Only then may A01
start. Commit coherent cleanup checkpoints with `Architecture-Step: A00` trailers.

### A01 — Extract Contracts and Establish Crate Ownership

- Freeze a migration inventory of current files, public entrypoints, generated
  standard declarations, build scripts and dependency edges in this document.
- Extract ABI/executable identities and split the existing IR crate into MIR,
  compiler and bytecode ownership. Move files and callers directly; update workspace
  manifests and lockfile. Do not preserve the old IR crate as a facade.
- Establish the codegen boundary and target Cranelift crate name. Confirm codec,
  native ownership and feature placement against the dependency constraints above.
- Remove source-analysis types from execution contracts. Keep physical layout
  descriptors separate from semantic types and backend-specific builder types.

Phase exit: the thirteen-crate ownership is represented by real migrated code,
not empty placeholders; the old package names are removed. A dependency inventory
and explicit remaining wiring errors are recorded. The workspace may be broken.

### A02 — Complete MIR, Analysis and Compiler Lowering

- Move source orchestration and bounded monomorphization to compiler; preserve
  recoverable HIR/tool queries and permit codegen only from checked semantics.
- Implement the concrete MIR, operation/effect and verification contracts above.
  Build liveness, safepoint, debug and budget analyses and a sealed backend handoff.
- Migrate interpreter lowering to consume that handoff. Share semantic facts rather
  than recovering them from bytecode order or re-reading syntax in a backend.
- Add bounded public passes with explicit analysis invalidation and verification.
  Keep the frontend-free lowering core separable from compiler source support.

Phase exit: the source-to-verified-MIR-to-bytecode path and MIR contract tests exist;
the compiler no longer depends on the old IR shape. Runtime/native integration
errors may remain and must have identified owners in A03/A04.

### A03 — Reconnect Runtime, Artifacts and Embedding

- Adapt runtime/VM to ABI and bytecode contracts while preserving the single frame
  driver, execution sessions, host reentry, roots, mutation guarantees and reload.
- Separate immutable code/layout/debug/version state from per-runtime mutable state.
  Define native installation and invocation ownership without importing codegen.
- Implement the bounded portable native-input section, consistency verification,
  version rejection and load-before-execute boundary described above.
- Split source and execution features in embed/compiler; retain runtime-only and
  artifact-native use without transitive frontend dependencies. Update CLI and host
  examples to current APIs instead of adding old-name wrappers.

Phase exit: source and bytecode execution use the new crate graph and artifact
format; dependency/feature configurations can be inspected. Native compiler wiring
may still fail until A04. Record actual validation, including unavailable checks.

### A04 — Move Existing Cranelift Execution to the MIR Boundary

- Split native compilation from installation/invocation. Cranelift consumes the
  verified handoff and explicit ABI/link data, never a `BytecodeFunction` as its
  semantic input and never HIR/source/runtime state for semantic recovery.
- Preserve the currently supported native subset, helper behavior, error/source
  mapping, budget accounting and interpreter fallback. Select support before
  executing a function; never restart partially executed effects in the interpreter.
- Retain native code memory and its exact execution/dependency versions. Preserve
  debugger policy and source/artifact behavior without introducing a second runtime.
- Check the backend interface for future LLVM needs using concrete contracts; do
  not add LLVM or expand JIT coverage just to demonstrate that it could be added.

Phase exit: existing genuinely native cases execute natively through MIR; unsupported
cases fall back before entry. Tests distinguish native execution from fallback, so
passing every test through the interpreter does not count as completing this phase.

### A05 — Integrate, Audit and Establish the New Baseline

- Resolve every carried build/test error. Remove old imports, duplicate fact tables,
  hidden frontend dependencies, stale generated bindings, obsolete codecs and
  transitional production stubs. Retain behavior tests and rewrite structural tests
  to assert the new architecture.
- Audit handwritten Rust against [AGENTS.md](../AGENTS.md#imports-and-module-paths):
  explicit production imports/re-exports, no deep parent traversal, readable paths
  at use sites and proper module ownership. Review test-only classification and
  macro-generated code in context; a successful build is not this audit.
- Pass the [strict structure checker](structure-checks.md) again to catch migration
  regressions. A00 owns pre-existing cleanup; A05 owns final migration clearance.
  Revalidate documented exceptions against the new ownership boundaries. No
  grandfathering baseline or blanket suppression is permitted at any phase.
- Finish specifications, crate/layout documentation, CLI, SDK examples and feature
  recipes. Keep the roadmap and goal guide pointing to one active plan.
- Run the final acceptance matrix and workspace gates. Record performance and
  artifact/cache size measurements separately from correctness results.
- Record a final completion audit and a follow-up Cranelift JIT scope. LLVM remains
  a later independent track, not an unchecked phase keeping this goal alive.

Phase exit: the complete target architecture is usable, all required gates pass,
no known integration errors remain, and no compatibility path is retained.

## Acceptance and Measurement

Required behavioral coverage:

- Correct functions remain queryable beside erroneous/incomplete source; compile
  and execute paths reject unchecked HIR. Source identity and diagnostics survive.
- Concrete generic types, narrow integers, custom operators/equality, interfaces,
  closures and collection callbacks preserve source and artifact behavior.
- Once-only evaluation, alias mutation, checked overflow, failed compound writes,
  traps and budget exhaustion preserve ordering and already-completed effects.
- GC at supported boundaries preserves roots through calls and host reentry;
  stale/foreign handles remain rejected and terminal paths release resources.
- Reload failure leaves current entrypoints untouched; old interpreted/native
  calls retain their exact dependency versions and code-memory owners.
- Malformed bytecode/MIR sections, old formats and inconsistent executable metadata
  are rejected at their validation boundaries before the relevant code executes.
- Native-supported cases compare interpreter/Cranelift results, effects, traps and
  logical budgets. Unsupported/debug-policy cases verify pre-entry fallback.
- Source-enabled, bytecode-only and artifact-native configurations prove the
  advertised dependency graphs with production `cargo tree`/metadata inspection
  and standalone consumer smoke checks; workspace dev-feature unification alone
  is not evidence of a frontend-free build.

Final commands, using the default target directory and default parallelism:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Also validate documented optional-feature combinations. Tests and examples must
cover actual native execution as well as interpreter fallback. Use existing
conformance suites and add tests for the new boundaries; do not impose a frozen
test count or write tests that only reproduce implementation details.

Record the toolchain, machine, profile, feature set, parallelism and cache state.
Measure compile time, repeat-test time, source compilation, MIR analysis, artifact
size/load time, shared-code memory across runtimes, native compilation and call
overhead. Preserve baseline GC/reload checks. Record material regressions and their
cause; do not claim that crate splitting inherently improves build performance.
Store durable summaries in docs; ignored target logs are disposable evidence and
must not be the sole record needed to resume the goal.

## Progress Ledger

- [x] A00 — Existing structure cleanup and passing CI prerequisite.
- [x] A01 — Contracts and crate ownership.
- [x] A02 — MIR, analyses and compiler lowering.
- [ ] A03 — Runtime, artifacts and embedding.
- [ ] A04 — Existing Cranelift backend migration.
- [ ] A05 — Integration, audit and baseline.

Current state: A00 passed at `4d82fcb`, A01 completed at `3227b0a`, and A02
completes with the bounded public MIR passes checkpoint below. Source compilation,
verified MIR analyses and frontend-free bytecode lowering pass their scoped checks.
The workspace remains intentionally broken at the recorded A03 VM and A04 native
backend boundaries; the runtime suite is restored by the A03 checkpoint below.
Resume A03 integration; A05 final acceptance
has not run and the overall refactor is not complete.

Pre-migration structural audit (2026-09-27):
`uv run --locked scripts/check_structure.py --json` scanned 375 Rust files and
exited 1 with 2,975 findings: 2,833 qualified paths, 92 wildcard imports,
22 re-export locations, 9 repeated parent traversals and 19 oversized files.
There were no parse errors. This is a debt record, not a suppression baseline.
A00 owns all these findings and must fix or justify them before A01. No exceptions
have been added for the existing findings. CI reports the
outstanding violations as failures; acceptance of a failing cleanup checkpoint
does not authorize starting the MIR refactor with failing gates.

| Phase | Commits / completed work | Checks and results | Known errors / next owner |
| --- | --- | --- | --- |
| A00 | `ec0bf1a`, `2cf5fb3`, `957b691`, `ed10ba2`, `c14e5bd`, `05c4e0f`, and the production responsibility checkpoint below | 32 checker tests; 432 files, zero findings/exceptions; workspace clippy and all 1,315 tests/doc tests; fmt and diff checks pass | None; hosted CI not queried for local commits |
| A01 | Complete at `3227b0a`; thirteen owners, portable ABI and linked validation, source-free execution dependencies | 418 targeted ABI/bytecode/HIR/MIR tests pass; dependency inventory and exit evidence below | A02-A04 own the recorded downstream migration work |
| A02 | Complete with bounded public passes; `0cb6570` concrete source handoff, `c5ecb17` sealed analyses, `74168b1` portable origins, `5ded21e` logical charges | 254 focused tests and 2 seal doc tests; scoped source/core clippy, structure, fmt and diff checks pass | A03/A04 own artifact/VM/native integration and obsolete runtime backend fixtures; optimized execution parity follows restored VM wiring |
| A03 | Runtime fixture/import ownership restored; compilation mocks removed from runtime tests | 182 runtime tests plus one seal doc test; runtime all-target clippy, structure, fmt and diff pass | Native installation/invocation, artifact MIR preparation/correspondence, SDK feature separation and VM integration remain |
| A04 | Not started | Not run | None recorded |
| A05 | Not started | Not run | None recorded |

### A00 checkpoint: foundation imports (2026-09-27)

Checkpoint commit subject: `refactor: make foundation imports and facades explicit`.
Changed common, syntax and existing Cranelift modules only; crate ownership and
execution contracts are unchanged. Qualified numeric variants replace local globs.
The existing host-interface API now lives in `host_interface/mod.rs`; the small
AST node macro lives at the AST facade with explicit consumer imports. No new
forwarding surface, visibility expansion or structure exception was introduced.

Validation on this checkpoint's Rust sources:

- `uv run --locked scripts/check_structure.py --self-test`: 32 tests pass.
- `uv run --locked scripts/check_structure.py --json`: exit 1, 2,845 findings
  (2,710 qualified paths, 89 globs, 18 re-export locations, 9 parent traversals,
  19 oversized files); none in common, syntax or Cranelift. No parse errors or
  exceptions. Remaining findings are owned by A00, not deferred to A05.
- `cargo clippy -p kagari-common -p kagari-syntax -p kagari-jit-cranelift
  --all-targets -- -D warnings`: pass.
- `cargo test -p kagari-common -p kagari-syntax -p kagari-jit-cranelift`:
  107 tests pass; doc tests pass.
- `cargo fmt --all -- --check` and `git diff --check`: pass.
- Full workspace clippy/tests and hosted CI are not yet validated for A00.

### A00 checkpoint: behavioral test modules (2026-09-27)

Checkpoint commit subject: `refactor(test): split suites by semantic responsibility`.
Split eight oversized test suites into ordinary child modules covering generic
context/recovery/completion, bytecode lowering/validation/artifacts/identities,
offline host contracts, source imports/implementations, typed paths and VM
frames/debugging/helpers. Shared fixtures remain in the enclosing test module.
Integration targets use Cargo's normal `tests/<suite>/main.rs` layout.
A syntax-token comparison against `ec0bf1a` preserved all 340 function bodies,
including assertions, except relocated fixture paths and one direct owner import.
No coverage was removed or assertions weakened.

Validation:

- `cargo test --workspace`: 1315 tests pass, including documentation examples
  and Cranelift execution; doc tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo fmt --all -- --check` and `git diff --check`: pass.
- `uv run --locked scripts/check_structure.py --json`: exit 1, 2,837 findings;
  all eight test LOC findings resolved. Eleven oversized production files remain,
  alongside 2,710 qualified paths, 89 globs, 18 re-export locations and 9 parent
  traversals. No exceptions or parse errors. A00 owns these remaining findings.
- Hosted CI status is unavailable; local workspace success does not close A00
  while the structure audit fails.

Environment: Rust 1.98.1 (`48a229cea`, LLVM 22.1.8), aarch64-apple-darwin,
MacBookPro18,2, 32 GiB RAM, 10 logical CPUs; workspace O1 dev/test profile,
default Cargo parallelism and target directory, warm incremental cache. These
are correctness runs; no performance comparison is claimed.

### A00 checkpoint: explicit enum dispatch (2026-09-27)

Checkpoint commit subject: `refactor: qualify semantic and execution enum variants`.
Removed 45 function-local enum wildcard imports from HIR numeric/inference rules,
IR lowering and verification, runtime helper dispatch/value semantics and VM
collection dispatch. Variants retain their owning enum names; grouped enum uses
in collection lowering now declare module-scope dependencies. The audit still
tracks parent-module globs and remaining local-import/path cleanup within A00.

Validation:

- `cargo check --workspace --all-targets`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo test --workspace`: 1,315 tests pass; doc tests pass.
- `cargo fmt --all -- --check` and `git diff --check`: pass.
- Token comparison confirms the edits preserve operands and control flow after
  normalizing enum qualification, imports and rustfmt's match-arm wrapping.
- `uv run --locked scripts/check_structure.py --json`: exit 1, 2,792 findings
  (2,710 qualified paths, 44 globs, 18 re-export locations, 9 parent traversals,
  11 oversized files). No exceptions or parse errors. A01 remains gated.

### A00 checkpoint: explicit module dependencies (2026-09-28)

Checkpoint commit subject: `refactor: import module dependencies explicitly`.
Replaced use-site qualified paths with explicit imports or short module names
across all production crates, build scripts and SDK examples. Removed repeated
parent traversal. Reviewed same-named HIR/IR/bytecode operations and source types;
keep distinct aliases where contracts differ and reuse imports of the same owner.
No crate graph, public visibility, ABI version or execution policy changed.
The CLI's optional backend import retains its JIT feature guard.

Validation:

- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo test --workspace`: 1,315 tests pass; doc tests pass.
- `cargo check -p kagari-cli --features jit`: pass (default CLI also covered).
- `cargo fmt --all -- --check` and `git diff --check`: pass.
- `uv run --locked scripts/check_structure.py --json`: exit 1, 71 findings in
  402 files: 42 parent/module wildcard imports, 18 re-export locations and 11
  oversized files. Zero qualified paths, parent traversals, parse errors or
  exceptions. The earlier enum audit ran before the final two grouped-enum fixes;
  this fresh audit includes those fixes and supersedes that intermediate count.
- Remaining A00 work also includes moving unjustified function-local imports to
  module scope while reviewing ownership, followed by the complete A00 exit gates.

### A00 checkpoint: dependency ownership (2026-09-28)

Checkpoint commit subject: `refactor: replace inherited and local production imports`.
Removed all remaining production globs, importing from concrete owners without
adding forwarding exports or widening visibility. Moved 120 function-local
production imports to module scope and replaced opaque local aliases with owner
names or existing meaningful aliases. Test-only dependencies are imported in their
test modules. Hoisting one import exposed an equivalent nested-if simplification;
clippy's let-chain rewrite preserves its short-circuit evaluation order.

Validation:

- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo test -p kagari-hir -p kagari-ir -p kagari-runtime -p kagari-vm`:
  810 tests pass; doc tests pass. Full workspace tests last passed at `ed10ba2`;
  A00 exit will rerun all gates on the final cleanup revision.
- Manual/syntax review finds no remaining function-local production imports.
- `cargo fmt --all -- --check` and `git diff --check`: pass.
- `uv run --locked scripts/check_structure.py --json`: exit 1, 29 findings:
  18 re-export locations and 11 oversized files. Zero production glob, qualified
  path, repeated-parent, parse-error findings or exceptions. A00 remains active.

### A00 checkpoint: intentional facade exports (2026-09-28)

Checkpoint commit subject: `refactor(runtime)!: consolidate contract facade exports`.
HIR aggregates, analysis and imports, and the IR ABI contract now use ordinary
`mod.rs` roots for their existing APIs and child implementations. This changes no
crate edge or Rust module path. Runtime host declarations, path permissions and
capabilities are re-exported directly by the runtime library facade; implementation
modules import their common-crate owners. Consumers use the intended facade.
The former `runtime::host`/`metadata`/`security` forwarding exports are removed,
not retained as aliases. Host declaration types previously exported only below
`host` are relocated to the runtime root; implementation visibility is unchanged.
A01 will replace the current IR/HIR execution-contract ownership as planned.

Validation: workspace clippy with all targets and denied warnings passes;
`cargo test -p kagari-runtime --lib` passes 71 tests; fmt and diff checks pass.
The strict structure audit reports only 11 effective-LOC findings, with no
exceptions. Full A00 workspace and optional-feature acceptance is still pending.

### A00 checkpoint: production responsibility splits (2026-09-28)

Checkpoint commit subject: `refactor: separate semantic and execution responsibilities`.
The remaining oversized HIR, IR and runtime modules are separated with ordinary
Rust child modules. HIR body checking separates calls, places, patterns, methods,
constructors, host access and statements; declaration validation separates constant
initializers from trait surfaces. IR lowering separates calls, patterns and aggregate
construction, bytecode debug metadata and operation verification. Artifact resource
limits have one focused owner. Runtime separates loading/publication, authority,
object construction, array/map/set operations, host registration/borrows and standard
string/math implementations. Inline runtime, artifact and ABI tests have dedicated
modules. No execution contract, crate edge or public API is changed in this checkpoint.

Private helpers remain bounded to their original owner module; pre-existing
restricted visibility retains the same effective scope after relocation. Review
accounted for all 802 affected function bodies: only repaired scope paths, explicit
imports and rustfmt trailing-comma changes differ. No fixture strings or assertions
were weakened. Large exhaustive dispatch matches remain in focused handlers; A02
owns the lowering redesign and must review call policy as MIR contracts change.
This is architectural follow-up, not an exception to the passing structure rules.

Validation on the final cleanup source revision (this checkpoint):

- `uv run --locked scripts/check_structure.py --self-test`: 32 pass.
- `uv run --locked scripts/check_structure.py`: 432 files, zero violations and
  zero exceptions.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo test --workspace`: 1,315 tests pass, including doc tests.
- `git diff --check`: pass; no function-local production imports remain.

These are the commands used by both hosted CI jobs, executed locally on the
configured O1 profiles with the default target and parallelism. The warm incremental
cache/toolchain are unchanged from the recorded environment. Hosted CI status was
not queried for these local commits; no remote success is claimed. A00 has no
carried build/test/structure errors. The commit subject and `Architecture-Step: A00`
trailer identify this validated cleanup revision without a self-referential hash.

### A01 checkpoint: shared execution identities (2026-09-28)

Checkpoint commit subject: `refactor(abi)!: extract shared execution identities`.
Added the real `kagari-abi` crate with scalar identities, standard intrinsic IDs,
physical value representations and bounded decoder primitives. Their original enum
ordering, serialization derives, numeric layout rules and decoder limits are
preserved. The HIR build still generates standard API declarations from `stdlib`;
those generated declarations now bind the ABI intrinsic IDs directly. Semantic
queries and inference remain in HIR. This shared ABI edge is independent of source
analysis: `cargo tree -p kagari-abi --edges normal` contains common/serde and their
support libraries, with no HIR, syntax, MIR, runtime or native backend dependency.

Removed the old defining types and forwarding exports. Consumers, examples and
tests import `kagari_abi::{scalar,standard,representation}` directly. Checked source
types become physical representations through the existing concrete ABI conversion;
`ValueType` no longer accepts HIR `TypeId`. Decoder functions are public at the ABI
boundary because the future bytecode/MIR codecs share the same bounded contract.
No artifact version changes are needed for this move: encoded identities and
representations have not changed. The ABI's bincode dev dependency tests decoder
bounds; its production codec implementation depends only on serde/common.

Breaking Rust API: `BuiltinType`, `StandardIntrinsic` and `ValueType` are imported
from their ABI owner modules; their old HIR/IR locations are removed. No aliases
preserve the superseded locations.

A01 is not complete: the workspace currently has ten crates. Logical ABI contracts,
standard declaration descriptors and native ownership still need extraction; MIR,
compiler, bytecode and codegen must replace the remaining mixed IR/runtime owners.
The bytecode verifier still uses existing HIR trait/type queries, which A01 owns
removing through portable ABI contracts. This checkpoint does not claim the final
frontend-free runtime graph or any A02-A05 acceptance.

Validation: workspace/all-target clippy with denied warnings passes; structure
passes for 435 Rust files with no violations/exceptions.
`cargo test -p kagari-abi -p kagari-hir -p kagari-ir -p kagari-runtime -p kagari-vm`
passes 810 tests, including doc tests.
Formatting and diff checks pass. No build errors are carried from this checkpoint.

### A01 checkpoint: compiler and execution crate ownership (2026-09-28)

Checkpoint commit subject: `refactor(architecture)!: separate compiler and execution crates`.
The workspace now has all thirteen target package names with migrated implementation
or concrete contracts. `kagari-ir` is removed, and `kagari-jit-cranelift` is renamed
`kagari-codegen-cranelift`; neither remains as a facade. MIR owns CFG functions,
instructions, IDs and module/program verification. Compiler source owns lowering,
instance planning and source orchestration; compiler core owns bytecode emission
and debug lowering. Bytecode owns the interpreter model, linked verifier and codec.
ABI owns logical types/layouts, shared operations/effects, executable IDs, slot
semantics and native call/installation descriptors. Source type encoding and scalar
constant conversion are ordinary compiler functions rather than methods/foreign
trait implementations on execution types.

MIR function and source-call identities now carry `ConcreteFunctionIdentity` and
ABI arguments instead of HIR arguments. There is no renamed `FunctionInstance`
alias. The source planner still needs its checked-to-concrete handoff repaired;
use a private source request key only where semantic instantiation requires it,
then construct the concrete identity at the MIR boundary. Do not put HIR types back
into MIR to satisfy those callers.

Codegen now owns the compile-only trait and diagnostics. Its input is a selected
function in verified MIR plus an explicit immutable native helper description.
ABI pairs native artifacts with an owning memory handle; the native scalar call
signature uses an opaque context pointer rather than `Runtime`. Runtime retains
helper implementations, value decoding and invocation errors/debug capability
checks. A03 must install/invoke those products with pinned execution versions;
A04 must make Cranelift return actual memory-owning MIR compilation products.
The current backend implementation still contains its old bytecode/runtime calls
and therefore cannot yet implement the new interface.

The compiler `source` feature gates HIR/syntax and source-only bincode use; core
bytecode emission is outside that feature. Manifests omit frontend edges from ABI,
MIR, bytecode and runtime, and omit runtime/bytecode edges from native codegen.
Those declarations are **not** evidence that the graph builds: unresolved old
queries intentionally expose work still needed at those boundaries. No compatibility
aliases, frontend dependency cycle, validation bypass or success stub was added.
The artifact producer fingerprint now names `kagari-compiler`; portable MIR and
version changes remain A03 work.

Validation and carried errors:

- `cargo metadata --format-version 1 --no-deps`: resolves exactly thirteen workspace
  packages with the intended names. Manifests and lockfile are updated.
- `uv run --locked scripts/check_structure.py`: 447 files, zero violations and zero
  exceptions. Module/re-export ownership and moved macro paths were reviewed;
  new cross-crate contract visibility still needs compilation-driven review.
- `cargo fmt --all -- --check` and `git diff --check`: pass.
- Syntax inventory preserves all 1,312 `#[test]` functions by name. Source-driven
  ABI corruption/conversion suites moved into compiler tests; codec/contract unit
  tests remain with their owners. This is coverage inventory, not test execution.
- `cargo check --workspace --all-targets`: fails (exit 101), with 45 ABI library
  errors and 47 library-test errors at the recorded attempt. Representative
  diagnostics: E0432/E0433 for missing `kagari_hir` in ABI host/trait verification,
  standard contracts and wire types; E0599 for removed `from_checked_type` methods
  still used by runtime-side semantic checks. A01 owns replacing these with
  bounded portable ABI substitution/validation and generated standard descriptors.
  Do not restore a production HIR dependency to ABI. This first failing crate
  prevents downstream compilation, so later compiler/native failures are not yet
  exhaustively enumerated.
- Tests and clippy are unavailable for this revision until those build errors are
  fixed. Previous A00 and `1838e57` results do not validate this migration revision.

Next work by owner: A01 removes remaining HIR queries from ABI, MIR linked
verification and bytecode access/trait validation, reviews required public contract
APIs, and relocates any remaining source-only conversion fixtures. A02 repairs
concrete instance handoff, link lowering and the verified-analysis seal, removes
source text from backend debug inputs, and supplies liveness/safepoint/budget facts.
A03 reconnects SDK feature selection, native product installation/invocation and
portable artifacts. A04 replaces Cranelift's removed runtime/bytecode inputs and
preserves its scalar behavior with actual code-memory ownership. A05 remains the
full integration/feature/behavior gate. A01's checklist stays open.

### A01 checkpoint: portable standard declaration contracts (2026-09-28)

Moved the bundled standard declaration generator and its descriptor types/tables
from HIR to ABI. HIR now supplies source-specific extension traits that interpret
those shared descriptors as source declarations, inferred types and signatures.
Consumers import descriptors from their actual ABI owner; the former ABI enum and
native-default forwarding re-exports are removed. Associated member identity
construction now belongs to `kagari-common::identity`.

ABI standard trait templates and numeric intrinsic signatures expand directly from
the generated descriptors, retaining binder ownership, receiver/associated-type
projections, default slots, canonical associated member order and normalized bounds.
The scalar conversion matrix and parser/error identities are shared ABI policy;
HIR wraps their results in source types. Source binary-operator conversion moved
to compiler lowering. Runtime's remaining standard-surface HIR import is removed.
The declaration parser is an ABI **build dependency** only; normal production
ABI dependencies remain common, serde, bincode and smallvec. This does not yet
establish a compiling source-free execution graph.

Validation and carried errors:

- The relocated generator builds successfully. Its `standard_api.rs` is byte-for-byte
  identical to both retained prior HIR generator outputs (SHA-256
  `a7904b5cbd2388d1590f21141bfd8ebb48cd2c415e74d1eaa13406ba8a3bdcf6`).
- `cargo test -p kagari-common`: 22 tests pass, including identity decoding bounds.
- `uv run --locked scripts/check_structure.py`: 452 Rust files, zero violations,
  zero exceptions. Moved macro paths, module ownership and descriptor imports were
  also reviewed. `cargo fmt --all -- --check` and `git diff --check` pass.
- Test inventory retains all 1,312 prior test functions and adds two portable
  standard-template tests (1,314 total). The two new tests cannot run yet because
  ABI still fails to compile; inventory is not behavioral execution evidence.
- `cargo check -p kagari-abi --lib`: fails (exit 101), with 20 errors at this
  checkpoint. Remaining E0432/E0433 HIR references are in host validation and
  ABI type/interface validation; E0599 references still call the removed
  `from_checked_type` during substitution/projection normalization. No errors
  are reported in the new descriptor expansion or numeric modules at this attempt.
  Downstream HIR/compiler consumers still need compilation-driven extension-trait
  import cleanup once ABI builds. Workspace tests and clippy remain unavailable.

A01 remains open. Its next unit replaces ABI/HIR round trips with bounded portable
substitution and projection normalization, including associated families and
linked method-contract proofs, then replaces host/native-interface HIR queries.
Keep cancellation, depth/node limits, binder identities and linked validation;
do not bypass those checks or restore HIR dependencies to executable crates.
The previously recorded A02-A05 work and carried integration errors remain owned
by those phases.

### A01 checkpoint: source-free ABI validation (2026-09-28)

ABI now builds without HIR imports or checked-source type conversions. Its
substitution engine replaces one binder layer, preserves foreign method/family
binders, substitutes receiver owners explicitly, and bounds cancellation, identity
paths, traversal work, output depth and output node counts. It shares the existing
64-depth/4,096-node wire limits. Concrete interface instantiation rejects remaining
symbolic types; associated family templates retain their own parameters and bounds.

Portable projection normalization resolves embedded and applied associated outputs
and accepts a checked dependency lookup. Cyclic and oversized expansions fail
explicitly. Portable implementation matching checks repeated binder consistency,
requested associated outputs and family application. Module-local method checks
use the current implementation's outputs and may defer dependency projections;
linked method checks require a supplied complete resolver and compare normalized
signatures and bounds. A new test demonstrates that local deferral does not make
a corrupted dependency-supplied result pass the linked comparison.

Host trait bridges now match portable host declarations directly. Intrinsic host
standard constraints are shared ABI policy. Native collection, indexing and
iteration capabilities are validated without source analysis, with generated
standard-implementation fixtures checking their associated outputs. Source-only
ABI-to-HIR conversion lives in compiler lowering, and its existing source callers
were updated. Removed the old HIR implementation-signature reconstruction entrypoint.
MIR/bytecode consumers now use public ABI validation entrypoints from their actual
owners; internal helpers and generated descriptor expansion remain private.

Validation and carried errors:

- `cargo test -p kagari-abi -p kagari-hir`: 19 ABI and 353 HIR tests pass. After
  tightening matching-input bounds, `cargo test -p kagari-abi` again passes all
  19 tests. This includes generated standard contracts, wire corruption checks,
  alpha-renaming, repeated binders, associated families, cancellation, depth/node
  expansion limits, projection cycles, native access invariance, host constraints
  and dependency-projection comparison. Existing HIR semantic tests are preserved.
- `cargo clippy -p kagari-abi -p kagari-hir --all-targets -- -D warnings`: passes.
  Fixed the source descriptor extension-trait imports and unused imports exposed
  once ABI built; no frontend dependency was restored to execution crates.
- `uv run --locked scripts/check_structure.py`: 456 Rust files, zero findings and
  zero exceptions. Reviewed new module boundaries, cross-crate API visibility,
  production imports and bounded transformation control flow. Formatting and
  `git diff --check` pass. Test inventory retains every prior test and adds twelve
  behavioral/boundary cases (1,326 `#[test]` functions total).
- `cargo check --workspace --all-targets`: still fails (exit 101), now past ABI and
  HIR. The latest attempt reports 11 MIR errors and 64 bytecode library errors
  (65 for its test target). Representative failures: E0432/E0433 for old HIR
  imports in MIR program inheritance and bytecode access/trait verification;
  E0599 for removed checked-type/signature conversions; stale ABI helper owners,
  restricted visibility and old module paths from extraction. These are carried
  migration errors, not successful workspace validation. Compiler/runtime/native
  integration failures remain incompletely enumerated behind these crates.

The previous 20 ABI compilation errors are resolved. A01 remains open because
MIR and bytecode still query source analysis. The next substantial unit replaces
`kagari-bytecode::trait_bounds`' source catalog with portable linked proofs:
implementation uniqueness/ownership, intrinsic/native capabilities, associated
families and bounds, supertrait closure, and complete projection normalization.
Use the new ABI matching/substitution operations, retain proof/cancellation budgets,
and supply that resolver to linked method checks. Share portable ancestry with MIR
rather than restoring HIR. Bytecode access and descriptor argument matching also
need portable operations. A02 still owns concrete source-request handoff and MIR
analysis seals; the recorded A03-A05 work and final behavior matrix remain open.

### A01 checkpoint: portable ancestry and access validation (2026-09-28)

MIR now builds without HIR. Shared ABI trait ancestry substitutes applied generic
arguments, associated outputs and receivers, preserves parent order, deduplicates
diamonds and rejects declaration cycles, missing contracts, invalid arity and
bounded-work exhaustion. MIR program upcast checks use this operation with lookup
restricted to the permitted dependency closure. Concrete interface requests use
portable arguments directly.

Bytecode access validation now consumes portable collection capabilities, scalar
bounds, enum payloads and standard declaration arguments. Collection weakening
changes only outer access; element types remain invariant. Standard argument
binding preserves the first known binding and left-to-right traversal, leaving
operand access checks to validate subsequent occurrences. Unknown element types
do not erase a known storage constructor's mutability. Array repetition retains
its value-only rules and rejects shared payloads. ABI owns integer range bounds,
which source scalar validation also uses.

Validation and carried errors:

- `cargo test -p kagari-abi -p kagari-hir -p kagari-mir`: 26 ABI unit tests,
  353 HIR unit tests and both MIR compile-fail seal doctests pass (381 total).
  Seven new tests cover ancestry substitution, diamonds, expanding cycles,
  generated standard parents, cancellation, access invariance, shared repetition
  payloads and partial standard bindings. Test inventory retains all prior cases
  and now contains 1,333 `#[test]` functions.
- `cargo clippy -p kagari-abi -p kagari-hir -p kagari-mir --all-targets -- -D warnings`:
  passes. Structure check covers 460 Rust files with zero findings/exceptions.
  Reviewed production imports, module ownership, new API visibility and bounded
  walks. Formatting and diff checks pass.
- `cargo check --workspace --all-targets`: still fails (exit 101). Bytecode now
  reports 31 errors in each library/test target, concentrated in its remaining
  HIR-based linked trait catalog and removed checked-type conversions (A01).
  With MIR compiling, Cranelift reports 20 library errors and 44 test-target
  errors: obsolete bytecode/runtime imports, the removed invocation method,
  incompatible compile interface, and old function/product fields. These are
  owned by the planned A04 verified-MIR backend rewrite; do not restore its old
  dependencies. Compiler/runtime integration remains incompletely enumerated
  behind these failures. Workspace tests and clippy have not passed.

A01 remains open. Continue replacing bytecode's linked trait proofs with portable
implementation matching, intrinsic capabilities, complete projection normalization
and associated bounds. Preserve ownership, uniqueness, cancellation and proof
limits. The previous MIR compilation errors are resolved; MIR analysis seals and
concrete source lowering remain A02 work. A03-A05 acceptance remains outstanding.

### A01 checkpoint: portable standard implementation proofs (2026-09-28)

Added portable matching for engine-owned standard implementation descriptors.
Receiver matching binds only declared parameters, retains invariant element types
and permits readonly storage only for declared readonly capabilities. Applied
matching also binds trait-only parameters, rechecks repeated occurrences and
compares requested associated outputs. Matching and proving remain separate:
instantiated declaration bounds are returned as explicit portable obligations.
This retains Eq/Hash requirements on collection keys and nested FromIterator
requirements on Option/Result payloads rather than accepting a matching shape as
a complete proof.

Portable intrinsic rules now describe numeric and callable/operator outputs,
parsing and checked conversion errors, reverse conversion obligations, generated
collection/iteration contracts and Iterator's identity Iterable obligation.
Associated-output lookup uses applied trait inputs, including parameters absent
from the receiver. Reverse conversion preserves the canonical Error identity;
identity Iterable validates its Iter binding and carries the requested Item into
the required Iterator contract. Bounded type operations and cancellation precede
artifact-supplied type comparisons and expansion.

Native interface capability validation now consumes these descriptor/operation
contracts, replacing its separate hardcoded storage/output matrix. Its documented
local responsibility remains dispatch shape and outputs; the existing linked
validator still owns nominal key proofs against the dependency closure. No HIR
dependency or partial linked-proof success was introduced.

Validation:

- `cargo test -p kagari-abi -p kagari-hir -p kagari-mir`: 32 ABI and 353 HIR unit
  tests plus both MIR seal doctests pass. After tightening parsing-output arity
  and adding trait-only/repeated-parameter cases, `cargo test -p kagari-abi` passes
  all 33 tests. The seven added cases cover generated implementations, declared
  bounds, associated-output corruption, readonly access, RangeFull's trait-only
  parameter, conflicting Result error inputs, operators/callables, reversed
  conversions, lifted collection obligations and identity iteration/cancellation.
- Targeted ABI/HIR/MIR all-target clippy passes with warnings denied; final ABI
  clippy also passes after the last changes. Structure check covers 464 Rust
  files, zero findings/exceptions. Production imports, ownership, public helper
  boundaries and bounded matching were reviewed. Formatting and diff checks pass.
  Test inventory retains every prior case and contains 1,340 test functions.
- The previous workspace failure remains carried: bytecode's HIR-based linked
  catalog (31 library/test-target errors, A01) and the old Cranelift implementation
  (20 library / 44 test-target errors, A04). These consumers were not changed in
  this checkpoint, so the unchanged workspace failure was not repeated. Full
  workspace tests/clippy remain unverified.

A01 remains open. The next unit must consume these explicit intrinsic obligations
in the complete portable linked catalog, including script and host matching,
nominal override ownership, uniqueness, enum structural defaults, associated
family normalization and generic assumptions. Intrinsic matching alone does not
complete linked validation. A02-A05 work and final acceptance remain outstanding.

### A01 exit: portable linked validation and dependency audit (2026-09-28)

Bytecode now builds without HIR. Its linked trait validator borrows portable ABI
implementation tables, host declarations, trait contracts and concrete enum
layouts directly. It no longer reconstructs source implementation signatures or
queries source types. The shared ABI proof catalog retains the 4,096 implementation,
100,000 match-work and 64-depth limits, cancellation, duplicate declaration
rejection and conflicting enum-layout rejection. Recursive generic obligations
share a search budget; growing proofs fail explicitly. Structural enum defaults
allow recursive payloads while still rejecting non-hashable members.

Linked validation covers implementation uniqueness, nominal override ownership,
explicit PartialEq/Eq prerequisites, default suppression after custom equality,
conversion/iteration obligations, host candidates, supertrait closure, ordinary
associated outputs, family binders and input/output bounds. Projection normalization
uses portable matching and resolves only a unique implementation output; cycles
fail. Method validation receives the complete linked resolver. Unused trait graphs
and family constructor references, required dynamic parent tables and instantiated
interface uniqueness remain checked. New direct linked-validator tests reject
corrupt outputs, missing parent implementations and invalid family applications.

Removed the obsolete HIR executable-signature constructor and concrete enum
payload cache/branches. Source search tests construct their own source catalog
fixtures and keep their original assertions. Also corrected the remaining runtime
range-helper/artifact imports and the duplicate compiler bytecode-emitter import.
These are direct ownership fixes, without restoring forbidden dependencies.

A01's thirteen real crates and codec/native contract ownership are established.
The current normal dependency inventory (excluding build/dev edges) is:

| Owner | Internal production dependencies |
| --- | --- |
| common | none |
| syntax | common |
| hir | abi, syntax, common |
| abi | common |
| mir | abi, common |
| bytecode | abi, common |
| compiler | abi, bytecode, mir, common; hir/syntax behind source |
| codegen | abi, mir |
| codegen-cranelift | abi, codegen, mir |
| runtime | abi, bytecode, common |
| vm | abi, bytecode, runtime, common |
| embed | abi, bytecode, codegen, compiler, hir, mir, runtime, syntax, vm, common |
| cli | codegen-cranelift, embed, runtime, syntax, vm, common |

The metadata audit confirms exactly thirteen workspace packages and no transitive
normal source edge from ABI/MIR/bytecode/runtime/VM, nor a MIR/codegen/backend edge
from bytecode/runtime/VM. ABI's stdlib generation has separate build dependencies;
source-driven fixtures have dev dependencies. HIR legitimately consumes shared ABI
identities. SDK feature selection and CLI facade cleanup remain A03 work, as scoped
by the frozen inventory. Cranelift's old implementation is still broken against
its already-correct compile-only dependency boundary; A04 owns that translation.

Validation and carried integration failures:

- `cargo test -p kagari-abi -p kagari-bytecode -p kagari-hir -p kagari-mir`: 38 ABI,
  25 bytecode and 353 HIR unit tests plus two MIR seal doctests pass (418 total).
  Final ABI/bytecode tests pass again after public-helper/input preflight cleanup.
  Seven tests added; every previous test retained, 1,347 test functions total.
- `cargo clippy -p kagari-abi -p kagari-bytecode -p kagari-hir -p kagari-mir --all-targets -- -D warnings`:
  passes. `cargo clippy -p kagari-runtime --lib -- -D warnings` also passes.
- `cargo check -p kagari-compiler --no-default-features --lib`: passes, proving
  the source-free compiler core builds. `cargo check -p kagari-runtime --lib`
  passes. The previously carried 31 bytecode errors and two newly exposed runtime
  import errors are resolved.
- `uv run --locked scripts/check_structure.py`: 472 Rust files, zero findings and
  zero exceptions. Reviewed production imports, module/visibility boundaries,
  bounded recursive control flow, host ownership and macro/test scopes. Formatting
  and `git diff --check` pass.
- `cargo check --workspace --all-targets`: fails in Cranelift, 20 library and
  44 test-target errors, including removed bytecode/runtime imports, obsolete
  invocation API and old MIR/native-product fields (A04).
- `cargo check -p kagari-compiler --lib`: fails with 17 errors and 43 warnings.
  Source planning still puts HIR TypeId arguments into ConcreteFunctionIdentity,
  then attempts HIR substitutions using ABI arguments (E0308/E0277/E0271/E0599).
  A02 must introduce source-only request keys and a concrete lowering handoff;
  do not make executable identities contain HIR again.
- `cargo check -p kagari-compiler -p kagari-vm --lib`: also exposes five VM errors
  for obsolete codegen imports and a removed runtime BackendDiagnosticKind (A03).
  Runtime/VM behavioral suites and SDK/CLI integration remain unverified behind
  compiler/backend errors. Full workspace clippy/tests are not passing evidence.

A01 scope is complete, with build state explicitly broken in the owning follow-up
phases. Continue A02: fix concrete source request handoff, complete MIR analyses,
seals/debug/budget contracts and source-to-MIR-to-bytecode lowering, preserving the
existing semantic test inventory. A03/A04 retain the recorded integration work;
A05 must pass the entire acceptance matrix and final gates before goal completion.

### A02 checkpoint: concrete source instance handoff (2026-09-28)

Compiler specialization now uses a private source InstanceKey containing checked
HIR types. MIR function identities and interface-instance requests are emitted as
ConcreteFunctionIdentity with ABI arguments. Emission clones under the existing
source type depth/node limits and rejects non-concrete arguments before encoding.
Source call contracts encode their checked arguments directly; cross-module
interface demand discovery keeps portable arguments. Imported concrete requests
are converted back to checked types only at the compiler source planner's intake,
without introducing a source dependency in MIR or executable contracts.

Updated relocated compiler tests and examples to import their actual ABI, MIR,
bytecode and compiler owners. Program lowering diagnostics use SourceProgramError;
MIR verification diagnostics retain ProgramErrorKind. Corruption tests now mutate
portable argument types directly, keeping their original rejection assertions.
Source integration tests and examples explicitly require the source feature,
allowing all targets of the compiler core to build without frontend support.
Removed unused extraction imports and simplified the emitter's already-portable
argument clone; no semantic coverage was removed or assertions weakened.

Validation:

- `cargo test -p kagari-compiler`: all 123 unit and eight source-program integration
  tests pass (131 total). This restores the source-to-MIR-to-bytecode test path,
  including monomorphization/deduplication, growing recursion and shared limits,
  imported generic methods, layout/ABI/host/interface verification, associated
  families, malformed artifact rejection, once-only evaluation and trap ordering.
- `cargo clippy -p kagari-compiler --all-targets -- -D warnings`: passes. The prior
  17 compiler library errors and 43 warnings are resolved, as are relocated test
  and example import/type errors discovered once the library built.
- `cargo check -p kagari-compiler --no-default-features --all-targets` and matching
  all-target clippy with warnings denied: pass. Source fixtures remain available
  and executed with the default source feature; the core has no frontend edge.
- Both `cargo run -p kagari-compiler --example applied_traits` and `--example layouts`
  pass, reporting validated applied interfaces, concrete aggregate fields and
  stable executable declaration identities.
- Structure check: 472 Rust files, zero findings/exceptions. Reviewed changed
  production imports, private key ownership, conversion bounds and macro paths.
  Formatting and diff checks pass. Test inventory remains 1,347, none removed.

A02 remains open: verification currently seals the existing typed CFG, but the
required program-point liveness/root, safepoint, debug availability and explicit
logical-budget analyses and transformation invalidation are not complete. Continue
with the full sealed backend handoff, portable origin metadata and bounded public
passes. Existing cross-module/source tests now provide executable regression
coverage while completing that contract. The recorded five VM native-integration
errors (A03) and Cranelift errors (A04) remain; unchanged downstream failures were
not repeated at this checkpoint. SDK feature/artifact integration, the A05 behavior
matrix and full workspace gates remain unverified.

### A02 checkpoint: sealed program-point analyses (2026-09-28)

Checkpoint subject: `refactor(mir): seal liveness and debugger availability`.

`VerifiedMirModule` now owns revision-bound function/block/point facts in addition
to the checked CFG. Forward definite initialization feeds lexical debugger
availability. Backward non-SSA liveness includes debugger reads and names logical
heap roots before each instruction and terminator. Runtime-service and control-flow
safepoints are classified from MIR, including allocation and potentially reentrant
calls. A call's operands remain roots while its newly defined result is unavailable
until successful completion. Unreachable points expose no live/debug values.
`BackendFunctionInput` exposes these facts from the same immutable seal. Extracting
raw MIR discards the analyses; reverification recomputes them. The two seal
compile-fail examples now test real exported types rather than unresolved paths.

Operand definitions/uses and successor inventories moved to their instruction and
terminator owners, with exhaustive enum matching. Verification and analyses share
these inventories. The analyses use packed slot sets and worklists, with a module
budget of 64 MiB for conservatively estimated fact/scratch storage and 100 million
work units, plus cooperative cancellation during both fixed points and lexical
scope traversal. The prior definite-initialization matrix limit remains checked.

Compiler debug emission consumes MIR availability and MIR operation/control-flow
kinds instead of reconstructing lexical availability from flattened bytecode.
Locals are emitted only where definitely initialized and in scope; parameter debug
records are now explicitly required, closing a forged-metadata case that otherwise
could omit a parameter and substitute an unscoped local. Interpreter root storage
remains its existing conservative function-wide layout; precise native stack-map
mapping belongs to A04 and must consume the sealed logical roots.

Validation (default Cargo target/parallelism and configured O1 profiles):

- `cargo test -p kagari-mir -p kagari-codegen -p kagari-compiler`: 140 tests pass
  (131 compiler unit, eight program integration, one MIR budget), plus two seal
  compile-fail doc tests. Nine new tests cover call operand/result roots, debugger
  retention, semantic liveness across loop backedges without debugger visibility,
  last uses, branch initialization, unreachable code, mutation/reverification,
  metadata rejection, memory limits, work limits and cancellation. Existing tests
  and assertions remain intact.
- `cargo clippy -p kagari-mir -p kagari-codegen -p kagari-compiler --all-targets -- -D warnings`
  and `cargo clippy -p kagari-compiler --no-default-features --all-targets -- -D warnings`:
  pass. The lowering core remains frontend-free.
- `uv run --locked scripts/check_structure.py`: 476 Rust files, zero findings and
  zero exceptions. Reviewed private analysis ownership, instruction inventories,
  production imports, visibility and the separate debug/fixed-point/root stages.
- `cargo fmt --all -- --check` and `git diff --check`: pass. Temporary logs are
  under `target/a02-analysis-*.log`.

A02 remains open. Next represent portable source positions/origins without retaining
`SourceFile`, add explicit logical budget charges that preserve existing failure
points, and implement bounded public simplification/cleanup passes with verification
and analysis invalidation. No budget semantics or optimized native execution are
claimed by this checkpoint. The previously recorded VM errors (A03), Cranelift
errors (A04), feature/artifact integration and A05 acceptance remain outstanding;
unchanged downstream failures were not rerun. The workspace is still broken at
those integration boundaries.

### A02 checkpoint: portable debug origins (2026-09-28)

Checkpoint subject: `refactor(mir)!: replace source files with portable origins`.

MIR no longer retains `SourceFile`, source text, line indexes or process-local file
handles. Its debug owner now contains a serializable `SourceOrigin` with URI,
source byte length and sorted positions for referenced span endpoints. Source
lowering captures one-based UTF-8 line/column coordinates before releasing the
frontend. Normal functions, closures, protocol/callable/iterator adapters and host
interface forwarding functions use the same capture routine. Function/local/capture
ranges and instruction/terminator origins remain attached to the MIR; synthetic
zero-width origins are recorded explicitly. Position capture is cancellable.

MIR verification bounds origin-table storage and checks ordered unique offsets,
coordinate consistency, zero-origin coordinates, span order, source bounds and
position coverage. Arithmetic rejects extreme offsets without overflow. An offset
without a line position (such as the LF byte in CRLF) retains absent coordinates.
MIR without a source origin retains its spans but does not invent a source URI or
line table coordinates. Bytecode debug lowering consumes the stored coordinates;
no execution/backend component needs source text to reconstruct these facts.

This breaks the raw MIR debug API: `MirFunctionDebugMetadata.source` is now
`Option<SourceOrigin>`, and debug record ownership moved from `function` to `debug`.
All consumers import the actual owner; the existing explicit crate-root facade
remains explicit, with no forwarding module or compatibility model. The executable
artifact format is unchanged here; A03 still owns the verified-MIR artifact section
and canonical bytecode/MIR correspondence.

Validation (default Cargo target/parallelism and configured O1 profiles):

- `cargo test -p kagari-mir -p kagari-compiler -p kagari-codegen`: 146 tests pass
  (135 compiler unit, ten program integration, one MIR budget), plus two seal doc
  tests. Six new tests prove source-file release via a weak reference, origin codec
  round trips, exact Unicode/CRLF coordinate preservation, malformed origin/range
  rejection, absent-origin behavior, capture cancellation, inherited default-body
  definition origins and inline-module physical offsets. No tests were removed.
- Source-enabled all-target clippy for MIR/compiler/codegen and frontend-free
  compiler all-target clippy with `-D warnings`: pass. The latter uses
  `cargo clippy -p kagari-compiler --no-default-features --all-targets -- -D warnings`.
- `uv run --locked scripts/check_structure.py`: 480 Rust files, zero findings and
  zero exceptions. Reviewed debug ownership, explicit imports, origin validation,
  serialization boundaries and source retention. Formatting and diff checks pass.
  Logs are under `target/a02-origins-*.log`.

A02 remains open. Continue with explicit logical budget charges that preserve the
existing interpreter's failure points, then bounded public constant/branch
simplification and dead pure-operation cleanup with re-verification/re-analysis.
Do not conflate the completed origin/analysis contracts with completed budget or
optimization semantics. The existing A03 VM and A04 Cranelift integration errors
remain unchanged and were not rerun. Artifact/feature reconnection and A05's full
behavior matrix and workspace gates remain outstanding.

### A02 checkpoint: explicit logical budget points (2026-09-28)

Checkpoint subject: `refactor(execution)!: preserve explicit logical budget points`.

The shared ABI defines `LogicalBudgetCharge::Step`: one pre-operation instruction
charge, with no zero-cost or batched representation. Every sealed MIR point owns
its charge and canonical logical offset. MIR owns entry-first emission order;
compiler jump offsets and charge emission consume the sealed facts. Native codegen
can now use these offsets without inspecting bytecode or counting machine
instructions. Every logical point is a GC/cancellation/budget safepoint, including
pure operations; runtime operations additionally retain their operand roots while
calling runtime services.

MIR and bytecode now provide `BudgetCheckpoint`, which replaces a removed pure
operation while preserving its origin and charge. It performs no value operation;
it still runs the ordinary pre-operation checks. Compiler lowering emits an
explicit instruction-budget table and the verifier requires exact point coverage.
The bounded codec and artifact metadata limits cover this table, and malformed
charge variants cannot decode. Artifact format and runtime ABI advance to **102**;
previous formats remain rejected, with no compatibility reader.

The interpreter frame fetch returns the operation and its checked charge together.
VM dispatch consumes that charge at the existing point: after the pre-instruction
GC/observer work and fetch, before execution. Native budget helpers validate the
active frame and logical offset, obtain the same metadata charge, publish the
failure origin, then check GC/resources. Missing frames/offsets quarantine rather
than silently accepting an unvalidated native charge. Native point mutation moved
from diagnostic snapshots into frame ownership. The public runtime entrypoint is
now `consume_logical_charge`; callers and authored bytecode fixtures were updated
directly. Dynamic host/collection resource costs retain their existing contracts.

Validation (default Cargo target/parallelism and configured O1 profiles):

- `cargo test -p kagari-abi -p kagari-bytecode -p kagari-mir -p kagari-compiler -p kagari-codegen`:
  212 tests pass (38 ABI, 25 bytecode, 138 compiler unit, ten program integration,
  one MIR budget), plus two seal doc tests. New compiler tests cover checkpoint
  charge/origin preservation and artifact round trips, missing/extra/invalid
  charges, and nonzero-entry logical offsets matching bytecode emission.
- `cargo test -p kagari-runtime --test execution_frames --test execution_sessions --test host_scopes`:
  32 tests pass. Three new tests exercise native budget exhaustion at the exact
  logical offset with cleanup, invalid native frame/offset rejection, and frame
  fetch charge delivery. Existing session/reentry/host quota coverage passes.
  These moved integration tests had stale `crate::` imports; they now import their
  actual runtime/ABI owners without adding dependencies or weakening assertions.
- All-target clippy with `-D warnings` passes for ABI/bytecode/MIR/compiler/codegen;
  frontend-free compiler all-target clippy also passes. Runtime clippy passes with
  `--lib --test execution_frames --test execution_sessions --test host_scopes`.
- Structure audit: 482 Rust files, zero findings/exceptions; formatting and diff
  checks pass. Reviewed enum inventories, explicit imports, point ordering, charge
  ownership, artifact bounds and the pre-operation failure ordering. A duplicated
  test operand inventory now uses the exhaustive MIR use/definition APIs; the
  original type-layout assertions remain.

Carried integration evidence: `cargo check -p kagari-vm` still reports the same five
A03 errors: removed `kagari_codegen` imports in `error.rs`/`vm.rs` and the old runtime
`BackendDiagnosticKind` path. The broad runtime test attempt additionally exposed
18 name-resolution errors in `crates/kagari-runtime/src/tests.rs`: its old compiler
backend mock still names `CodegenBackend`, `BackendFunctionInput`,
`BackendCompileError` and obsolete native product types through the runtime facade.
A03 must replace those compilation/invocation fixtures at the intended owners;
A04 owns the real Cranelift implementation. Do not reintroduce a runtime-to-codegen
edge to make these fixtures compile. Logs are `target/a02-budget-tests.log`,
`target/a02-budget-vm-check.log`, and the passing `target/a02-budget-*-tests.log`
and clippy logs. Full VM/native behavior remains unverified until that integration.

A02 remains open for bounded public constant/branch simplification and dead pure
operation cleanup, retaining these logical checkpoints and re-verifying/recomputing
all facts after transformation. The checkpoint mechanism is tested, but no public
optimization pipeline is implemented yet. A03 artifact/MIR correspondence and
feature wiring, A04 native migration and A05 acceptance remain outstanding.

### A02 exit: bounded public MIR passes (2026-09-28)

Added `kagari_mir::passes::optimize`, consuming a verified module and returning a
new seal plus pass statistics. Per-block scalar propagation folds successful
integer/boolean operations using the shared checked arithmetic contracts and
selects constant branches. A backward sweep removes dead pure temporary
producers using sealed cross-block live-outs. Removed operations become budget
checkpoints: blocks, logical offsets, charges, origins and scopes retain their
identity. Both transformation stages reverify and recompute analyses; cancellation
or the public work limit returns no partial result. Non-SSA writes kill facts,
calls clear propagation state, and heap reads, allocations and trapping operations
remain executable. Floating-point arithmetic is deliberately left to execution.

Corrected two effect declarations before relying on purity: raw unsigned arithmetic
can trap, and unchecked casts still validate their source numeric domain. Source
lowering exposes the same frontend-free pipeline through optional
`MirLoweringOptions.optimization`; its default preserves the diagnostic lowering
form. No alternate interpreter or source dependency was introduced into MIR.

Validation:

- `cargo test -p kagari-abi -p kagari-bytecode -p kagari-mir -p kagari-compiler
  -p kagari-codegen`: 222 tests and two seal doc tests pass. Ten new pass tests
  cover integer widths/shift rules, overflow/division/shift traps, unsigned effects,
  cast/float preservation, non-SSA invalidation, call barriers, heap reads,
  recomputed reachability, unchanged origins/charges and cancellation/work limits.
- `cargo test -p kagari-runtime --test execution_frames --test execution_sessions
  --test host_scopes`: 32 tests pass, retaining frame, budget, reentry and root
  coverage. Optimized execution parity must be added once A03 restores VM wiring.
- All-target clippy with denied warnings passes for ABI, bytecode, MIR, compiler
  and codegen; `cargo clippy -p kagari-compiler --no-default-features --all-targets
  -- -D warnings` also passes. MIR still depends only on ABI/common and generic
  support crates; the compiler's optional source layer owns HIR/syntax.
- `uv run --locked scripts/check_structure.py`: 487 Rust files, zero findings or
  exceptions. Reviewed changed imports, ownership, visibility and pass state/work
  bounds. `cargo fmt --all -- --check` and `git diff --check` pass. Logs reside in
  `target/a02-passes-*.log`; no structural debt or suppressed checks were added.

A02 meets its exit gate: checked source-to-MIR-to-bytecode lowering, concrete
operation contracts, sealed program-point analyses, portable origins, explicit
budgets and public bounded transformations exist with contract tests. Runtime
integration remains separate: the previously recorded five VM errors and eighteen
obsolete runtime backend fixture errors belong to A03; Cranelift compilation and
invocation migration belongs to A04. These unchanged failures were not rerun here.
Workspace tests/clippy and the final behavior matrix are not passing evidence.
Continue A03 native installation/invocation ownership, portable MIR artifact
correspondence, load-before-execute preparation and embedding feature separation.

### A03 checkpoint: restore runtime integration coverage (2026-09-28)

Replaced the obsolete compiler backend mock in runtime unit tests with an ABI
executable descriptor fixture. The test still checks registration, epoch retention
and invalid-function rejection, and now checks complete metadata preservation.
Compilation diagnostics and invocation belong to the subsequent codegen/VM
integration tests, not a runtime-to-codegen dependency. The fake compiler had no
independent semantic coverage and no tests were removed.

The first full `cargo test -p kagari-runtime` attempt then exposed stale `crate::`
imports in separate runtime integration-test/example targets. Corrected those to
their actual `kagari_runtime` owner and updated the host commit quarantine test to
consume an explicit logical budget charge. Source strings, fixtures and behavioral
assertions were retained; no production compatibility aliases were introduced.

Validation:

- `cargo test -p kagari-runtime`: all 182 unit/integration tests and one sealed
  module doc test pass, including host borrow validation, typed path commit order,
  GC ownership, sessions, cancellation and reload. This resolves the eighteen
  previously carried unit-test name-resolution errors and newly exposed fixture
  import errors; they are no longer outstanding A03 debt.
- `cargo clippy -p kagari-runtime --all-targets -- -D warnings`: passes, including
  examples. `cargo tree -p kagari-runtime --edges normal --prefix none` confirms
  production Kagari dependencies are ABI, bytecode and common only.
- Structure: 487 Rust files, zero violations/exceptions. Reviewed the affected
  imports and fixture ownership; formatting and diff checks pass. Logs:
  `target/a03-runtime-tests.log`, `target/a03-runtime-clippy.log` and
  `target/a03-runtime-dependencies.log`.

A03 is not complete. Native products still need runtime-owned installation and
invocation with retained code memory and execution versions; the present registry
only stores descriptors. Next replace VM-owned backend compilation with SDK
preparation, add the portable MIR section/canonical correspondence checks, and
separate embedding features. The recorded VM compiler imports and A04 Cranelift
errors still break the workspace; unchanged failures were not rerun here. A05
must validate optimized/unoptimized execution parity and the full feature matrix.

Update this ledger at every checkpoint with reproducible commands and concise
diagnostics. Keep build state separate from scope completion. Resume by inspecting
the working tree, ledger and `Architecture-Step` commit trailers, then continue
the first unfinished phase or its explicitly carried integration work.

## Goal Prompt

```text
/goal Implement docs/mir-architecture-refactor.md through A00-A05 in order.
First finish A00: correct the existing source structure and pass the checker,
checker tests, formatting, workspace clippy, workspace tests and diff checks.
Allow narrowly bounded LOC/re-export exceptions only with concrete design evidence;
do not add a grandfathering baseline or force unhelpful splits to satisfy a metric.
Do not start A01 or any MIR/crate migration before every A00 CI gate passes.
Reach the documented thirteen-crate architecture, preserving Kagari semantics
and migrating the existing Cranelift subset to verified MIR. LLVM and expanded
JIT coverage are deferred. Do not add compatibility layers or obsolete readers.
After A00 passes, A01-A04 phases/commits may fail compilation or tests: record
errors and their owning follow-up phase, then continue the direct migration.
Commit coherent work using Conventional Commits with Architecture-Step: Axx trailers and keep
the progress ledger current. Use the default Cargo target directory/parallelism
and the configured O1 profile. Complete the goal only after A05 resolves all
carried errors and passes the documented final checks and acceptance matrix.
```

Writing this plan does not itself start a goal or authorize unrelated language
features. Future work prioritizes Cranelift control flow, calls and measured hot
paths; LLVM, advanced native optimization, compact storage, async, moving GC and
speculative deoptimization require their own approved scopes.
