# MIR and Crate Architecture Refactor

Status: planned; no implementation phase is complete.

This is the active architecture execution plan linked from
[implementation-roadmap.md](implementation-roadmap.md). It follows the completed
[foundation refactor](foundation-refactor.md) and subsequent language extensions.
The value, failure, module, host, and security specifications remain authoritative
for observable behavior. This document owns the new crate boundaries and migration
order; older descriptions of the monolithic IR crate and bytecode-fed native
backend are the implementation being replaced, not alternative target designs.

## Objective and Scope

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

- Work through A01-A05 in order. Each phase is a substantial architectural unit,
  not a queue of one-file checkpoints. Several coherent commits may share a phase.
- Replace old models directly. No compatibility wrappers, deprecated re-exports,
  dual lowering paths, old artifact readers, or migration adapters.
- Intermediate commits and A01-A04 phase transitions may fail compilation and
  tests. A green workspace is not a prerequisite for a structural move or commit.
  Do not build temporary compatibility scaffolding to make a checkpoint green.
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
  `git diff --check` for every checkpoint. Full workspace checks are mandatory at
  A05, when no known build or test failures may remain.
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
hir -> syntax, common
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

- [ ] A01 — Contracts and crate ownership.
- [ ] A02 — MIR, analyses and compiler lowering.
- [ ] A03 — Runtime, artifacts and embedding.
- [ ] A04 — Existing Cranelift backend migration.
- [ ] A05 — Integration, audit and baseline.

Current state: planning only. No code migration has started.

| Phase | Commits / completed work | Checks and results | Known errors / next owner |
| --- | --- | --- | --- |
| A01 | Not started | Not run | None recorded |
| A02 | Not started | Not run | None recorded |
| A03 | Not started | Not run | None recorded |
| A04 | Not started | Not run | None recorded |
| A05 | Not started | Not run | None recorded |

Update this ledger at every checkpoint with reproducible commands and concise
diagnostics. Keep build state separate from scope completion. Resume by inspecting
the working tree, ledger and `Architecture-Step` commit trailers, then continue
the first unfinished phase or its explicitly carried integration work.

## Goal Prompt

```text
/goal Implement docs/mir-architecture-refactor.md through A01-A05 in order.
Reach the documented thirteen-crate architecture, preserving Kagari semantics
and migrating the existing Cranelift subset to verified MIR. LLVM and expanded
JIT coverage are deferred. Do not add compatibility layers or obsolete readers.
Intermediate phases/commits may fail compilation or tests: record errors and
their owning follow-up phase, then continue the direct migration. Commit coherent
work using Conventional Commits with Architecture-Step: Axx trailers and keep
the progress ledger current. Use the default Cargo target directory/parallelism
and the configured O1 profile. Complete the goal only after A05 resolves all
carried errors and passes the documented final checks and acceptance matrix.
```

Writing this plan does not itself start a goal or authorize unrelated language
features. Future work prioritizes Cranelift control flow, calls and measured hot
paths; LLVM, advanced native optimization, compact storage, async, moving GC and
speculative deoptimization require their own approved scopes.
