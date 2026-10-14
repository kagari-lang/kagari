# Cranelift synchronous execution expansion plan

Status: planning complete; J01-J06 implementation has not started. The user
requested this execution plan, not implementation activation. The
[roadmap](implementation-roadmap.md#cranelift-synchronous-execution) owns queue
placement. This document owns scope, phase order, design decisions, acceptance
and the progress ledger. Proposed contracts below are not existing APIs.

## Outcome and scope

Compile most existing synchronous Kagari business code through Cranelift:
parameters, locals, control flow, numeric operations, script calls, package calls,
objects, collections, closures, interfaces and installed host operations.
Coverage means executing the relevant script bodies as machine code, with explicit
runtime helpers for operations whose semantics belong to runtime or libraries.
Successful interpreter fallback does not establish native coverage.

Use the existing checked source -> verified MIR pipeline. Compile control flow and
scalar operations directly; reuse runtime-owned allocation, access, calls, roots
and failure services. Do not implement another type checker, bytecode dispatcher
or collection library inside the backend. Helpers must perform named, checked
operations rather than interpret arbitrary instructions one at a time.

The target is the synchronous behavior already specified by the language, not
new syntax or previously unimplemented source semantics. In particular, generic
templates use the current checked specialization/shared-body model; no runtime
source specialization is introduced.

Exclude async factory/resume execution and suspension, native debugger callbacks,
automatic hotness/first-call scheduling, LLVM, AOT, persisted machine code,
speculative optimization, deoptimization and cross-function inlining. Async code
and execution with an observer keep explicit pre-entry interpreter selection.
Synchronous callbacks reached from async execution must preserve the existing
session contracts; they do not authorize native suspension.

Library-owned collection migration (NC), host-registration unification (HR/SA21),
SDK/CLI renaming and compatible-reload redesign remain separate tasks. Consume
their current contracts, and refresh affected boundaries if those tasks land first.
This plan makes no numerical speedup or compilation-latency promise.

## Baseline and architectural diagnosis

Planning baseline: `e9f359d4`, inspected on 2026-10-14. No implementation changes
or new Cargo validation were performed while preparing this plan.

| Owner | Current evidence and intended disposition |
| --- | --- |
| [MIR instructions](../crates/kagari-mir/src/instruction.rs) | Already express locals, CFG terminators, calls, aggregates, paths, interfaces and closures. Consume checked facts instead of reinterpreting source syntax. |
| [Backend input](../crates/kagari-codegen/src/lib.rs) | Supplies a module seal, function analysis and helper links. Add verified program-scoped call evidence before compiling cross-module targets. |
| [Cranelift emission](../crates/kagari-codegen-cranelift/src/emit.rs) | Accepts zero-argument, single-block scalar functions. Replace this restriction incrementally while preserving verified origins and code ownership. |
| [Native ABI](../crates/kagari-abi/src/native_call.rs) | Only runtime/result pointers and Unit/Bool/i32 transport. Replace coherently with an explicit argument/result/frame contract. Runtime Rust Value layout must stay private. |
| [Native invocation](../crates/kagari-runtime/src/backend/native.rs) | Owns installation, runtime frame admission, status decoding and retained versions. Extend these owners rather than bypass them with raw generated calls. |
| [Execution calls](../crates/kagari-runtime/src/frame/calls.rs), [native host calls](../crates/kagari-runtime/src/frame/native.rs) | Prepared targets and rooted host arguments already exist, but several entrypoints are coupled to the executing bytecode instruction. Factor shared validated operations; do not have JIT helpers fake an interpreter PC to select authority. |
| [GC roots](../crates/kagari-runtime/src/gc/roots.rs) | Runtime owns registered slots and leases. Reuse this ownership for native root frames; empty scalar stack maps do not prove heap-reference support. |
| [SDK preparation](../crates/kagari-embed/src/program/native.rs), [VM selection](../crates/kagari-vm/src/vm/native.rs) | Explicit preparation, per-program product caching and pre-entry fallback exist. Extend argument transport and function-boundary selection without runtime compilation. |
| [Native tests](../crates/kagari-embed/tests/cranelift_preparation.rs) | Assert real native execution, artifact correspondence, lifetime and reload. Extend existing semantic owners instead of creating a duplicate syntax smoke suite. |

The narrow backend is an intentional baseline, not a demonstrated miscompile.
Missing capability is mainly native execution machinery. Shared runtime checks
are semantic costs; copying or skipping them is not an optimization. Existing
interpreter cursor optimizations remain valid only inside their closed borrow
scope and cannot be carried across collecting helpers or host reentry.

For example, a damage function loops over units, reads armor, calculates damage,
updates health and notifies a host callback. Today its entry falls back as soon as
unsupported MIR is found. Under this plan its loop and arithmetic are generated
code; checked object/host services perform reads, mutation and notification.
Arguments remain rooted across notification, a callback observes the committed
health change, and an error preserves earlier effects without replaying the loop.
Both routes retain the caller's exact dependency generations.

## Required design contracts

### ABI, admission and errors

J01 records the exact physical ABI in kagari-abi: argument count/representation,
result ownership, context/frame handles, status and error retrieval. Use explicit
physical descriptors and runtime-owned handles or slots for reference-bearing
values; never embed Rust Value, references, enum layouts or mutable heap pointers
as an undocumented machine-code contract. Reserve no speculative alternate ABI.
Scalar registers may remain unboxed inside a compiled function.

SDK, VM, direct invocation and typed host-call entrypoints must agree on parameter
admission, semantic types, heap ownership and return rooting. Generated code and
helpers must not unwind through C frames. Runtime retains the structured error
and trace while status transport indicates success/failure; all exits release the
owned frame and roots exactly once. Polling preserves sticky cancellation and
trap origins; do not reintroduce instruction charging or generic permissions.

Replace the old zero-argument API and affected consumers directly. Retain ABI
identifiers but do not routinely bump them for this unpublished migration.
Invalidate affected development artifacts and batch fixture regeneration at
coherent boundaries. No compatibility alias or second invocation implementation.

### Control flow and numeric semantics

Lower verified MIR blocks and locals using Cranelift block parameters/variables.
No additional shared SSA IR is required. Preserve checked arithmetic, conversion
rules, signed division/remainder edge cases, shifts, floating-point comparisons
and once-only left-to-right evaluation. Hardware instructions must not introduce
an OS trap, different conversion result or changed failure order.

Initially retain existing logical polling points and origins through CFG lowering,
including loop backedges and returns. Helpers and long native library operations
must keep cooperative cancellation. Poll removal/coalescing is separate measured
work and requires proof of unchanged required safepoints and observable behavior.

Cranelift's [FunctionBuilder documentation](https://docs.wasmtime.dev/api/cranelift_frontend/struct.FunctionBuilder.html)
describes variable SSA construction and block parameters. Use those facilities
against the repository's pinned version, not an assumed latest API. Its stack-map
facilities do not supply Kagari's root ownership, host borrowing or reload rules.

### Calls, program evidence and execution selection

J03 extends the checked handoff with program-scoped target/signature evidence or
sealed call-site descriptors installed by runtime. Codegen cannot recover targets
by name, trust unchecked module offsets, or infer types from bytecode/source.
Descriptors bind the caller's concrete function and logical call site, argument
and result contracts, supplying dependency and generation. Installation rejects
foreign, stale or mismatched evidence before entry.

Use a shared runtime call transition/trampoline first. Runtime owns admission,
frames, arguments, dependency selection, call depth, results and cleanup; the VM
owns the interpreter executor and execution driver. Keep runtime independent of
the VM crate. Refactor bytecode-specific wrappers onto shared services only where
needed; do not expose private interpreter internals as a new backend interface.

Preparation selects and installs reachable synchronous functions, handling cycles
with a finite visited worklist and explicit bounds. Per-runtime installed handles
stay separate from reusable compiled products. No compilation occurs in the
execution loop. Preserve complete backend/configuration/version cache identity;
do not cache transient errors as unsupported decisions.

The execution driver may select native or interpreter at a callee boundary and
return to its caller, including host callbacks. This is normal mixed execution,
not permission to restart a partially executed caller. A compiler/link error is
an error. Unsupported callee selection must occur before that callee's effects.
An observer conservatively selects interpretation for the entire execution.
Unimplemented scheduling policies must be rejected explicitly rather than treated
as automatic compilation; preserve explicit Enabled/Disabled entry behavior.

Before enabling recursion, settle machine-stack use. Prefer a runtime-owned call
driver/trampoline so script depth does not imply unbounded C-stack recursion. If
a bounded native stack chain is selected, prove the bound and its interaction
with configured script depth, mixed calls and synchronous host reentry on each
supported platform. A script depth check alone is not a native-stack proof.

### GC roots and operation helpers

Use runtime-registered native root frames as the first implementation. J04 defines
the mapping from MIR locals/temporaries to root slots and from sealed safepoints
to initialized live values. A collecting/reentrant helper may run only after all
necessary references are published. Never register uninitialized bits or retain
frame/heap borrows across a collecting call or host callback.

New objects and returned references stay protected continuously from allocation
through destination publication. Transfer result protection before releasing the
callee's roots. Clear slots consistently with the existing lifetime contract;
avoid retaining dead objects for an entire long-running function without evidence.
Validate runtime identity/generation on handle admission. Cleanup covers success,
trap, cancellation, depth failure and host reentry. Physical machine-stack scanning
is not required for this batch and cannot be silently substituted for this protocol.

Provide focused helper families for allocation, fields/indexing, module state,
enum payloads, interfaces, closures, installed calls and typed host paths. Reuse
the existing operation owner; remove any superseded duplicated implementations.
Complex numeric or library operations may use helpers where required to preserve
semantics, while simple arithmetic and CFGs remain generated code.

Treat calls as conservative memory/GC barriers unless checked evidence proves
otherwise. Existing false effect bits do not establish transitive absence of
allocation, mutation or callbacks. Preserve bounds/type/access checks, readonly
alias behavior, mutation commits, collection revisions and pinned stored calls.

## Coverage contract

J01 inventories every current MIR instruction, terminator, call-target and value
representation in this table's owning categories. Each receives a phase and a
status: direct native, runtime helper, explicit mixed callee, or deferred fallback.
Record concrete uncovered cases here; do not create a parallel progress queue.
No percentage or overall Native report substitutes for this inventory.

| User-visible category | Required coverage | Owner |
| --- | --- | --- |
| Functions and scalar values | Parameters/results; Unit, Bool, I32, I64, U64, F32, F64 and checked conversions/operations; uninhabited exits preserve failure semantics | J01-J02 |
| Local control flow | Bindings, mutation, blocks, if, loops, break/continue, return and short-circuit logic | J02 |
| Script calls and packages | Static/associated calls, recursion, concrete generic specializations and verified cross-package targets | J03 |
| Data and state | Strings, tuples, arrays/repetition, ranges, structures, enums, module slots, field/index access and assignment | J04 |
| Patterns and propagation | Existing destructuring/match lowering, enum payload admission, Option/Result propagation and early-return cleanup | J02/J04 |
| Callable objects and polymorphism | Captured/mutable closure cells, function values, interface construction/upcasts/dispatch, shared generic environments and checked operation witnesses | J05 |
| Collections and iteration | Existing Vec/Map/Set contracts, iteration protocols, adapters, selected callbacks and range/collection for loops | J04-J05 |
| Host integration | Installed functions/types, typed paths/views, scoped borrows, stored callbacks, reflection within declared metadata and synchronous reentry | J05 |
| Async and debugging | Explicit whole-entry fallback for deferred async operations and observed execution; no partial native suspension | Deferred |

Syntax already lowered to existing operations needs no backend-specific parser
rule. Known source-lowering gaps, including the shared generic Add issue recorded
in the roadmap, must be reproduced before classification. Repair a bounded gap
needed by this matrix at its semantic owner, or explicitly record a scope decision;
do not silently count it as native coverage or broaden into a general compiler rewrite.

## Execution phases

Execute in order. Each phase is a coherent buildable checkpoint; split commits by
responsibility when useful, retaining a named owner for carried errors. Default
to buildable commits. If an unavoidable atomic API migration crosses crates,
record the affected commands, diagnostics and resolution checkpoint here, disclose
the break in commit bodies, and resolve it before starting the next phase.

### J01: ABI and explicit preparation

- Refresh the baseline inventory, runtime frame ownership and policy behavior.
- Record exact scalar/reference transport, error, frame and helper contracts here
  before implementation. Implement scalar parameters/results across backend,
  runtime, VM and SDK; define reference admission for J04 without accepting it early.
- Replace obsolete entry consumers and keep preparation/cache ownership explicit.
- Prove zero/multiple parameters, representation rejection, stale/foreign handles,
  errors without C unwinding, source/artifact correspondence and explicit policy.
- Exit: supported parameterized scalar functions really execute natively; legacy
  zero-argument scalar behavior and pre-entry fallback remain correct.

### J02: scalar operations and CFGs

- Implement block lowering, locals and terminators; complete existing scalar
  numeric families and conversions through direct emission or justified helpers.
- Test input-dependent branches, loops, early returns and short-circuit effects.
  Constant folding must not erase the operation under test.
- Prove overflow/division/conversion boundaries, floating-point special cases,
  cancellation in loops and exact trap locations against interpreter semantics.
- Exit: a parameterized scalar calculation with mutation and loops executes fully
  natively without per-instruction interpreter dispatch.

### J03: checked calls and mixed execution

- Implement program call evidence, shared call transitions, bounded recursive
  preparation, code retention and execution-driver selection.
- Support same-module, cross-module/package and recursive scalar script calls;
  ensure specialized generic bodies use the same route.
- Prove native-to-native, native-to-interpreter and interpreter-to-native calls,
  depth exhaustion, cancellation, error propagation and old/new reload generations.
- Exit: all in-scope scalar callees can be asserted native; explicitly unsupported
  callees run once through the documented mixed-call boundary.

### J04: rooted values and aggregates

- Implement native root publication and protected arguments/results, then string,
  object, enum, range, array, module-state and access helpers.
- Extend general entry/call transport to admitted reference-bearing values.
- Exercise allocation and return handoff with forced collection, roots live across
  collecting calls, alias mutation, bounds/access traps and cancellation cleanup.
- Exit: object-bearing functions, matching and propagation execute natively;
  the same cases pass through encoded artifacts and retain exact generations.

### J05: closures, interfaces and host/library integration

- Extend prepared calls and helpers to closure captures/cells, interface dispatch,
  shared generic environments, installed native entries and typed host paths.
- Reuse collection algorithms, iteration/default methods and retained selections.
  Adapt ScriptInvoker/typed callback selection through the common driver.
- Prove scoped borrow rejection, reentry, once-only mutation, callback errors,
  structural revision checks and callbacks/interfaces surviving reload.
- Exit: a representative cross-package collection-processing operation with a
  closure, interface call and host mutation executes its synchronous script bodies
  natively. Library helper calls are identified separately from interpreted callees.

### J06: acceptance and documentation consolidation

- Close the coverage inventory using existing semantic suites and a small set of
  distinct missing boundary cases. Require actual native selection for all claimed
  script bodies, including nested calls and both source/encoded routes.
- Keep fallback tests separate and prove no replay of completed effects.
- Measure compilation/preparation, cold invocation, repeated execution and retained
  code/root memory on fixed representative workloads; record environment and limits.
  A performance miss triggers diagnosis, not an unbounded optimization phase.
- Update current JIT/backend, embedding and architecture contracts only as features
  become implemented; remove obsolete scalar-only statements and stale charging text.
- Move remaining bounded gaps/CI acceptance to roadmap/review, move lasting contracts
  to specifications and remove this completed plan. Git history retains the ledger.

## Validation and checkpoints

Reuse contract owners: backend scalar emission, SDK cranelift_preparation and
native_preparation, numeric_operations, existing language contracts, GC/reentry,
package inputs, interfaces and reload. Select only the relevant test names/targets
per phase; do not run this entire list locally as a disguised workspace sweep.
Add cases only for uncovered semantics or native boundaries, not each new helper.

Representative commands, to narrow according to the actual change:

```text
cargo test -p kagari-codegen-cranelift --lib <affected_filter>
cargo test -p kagari-embed --test cranelift_preparation <affected_filter>
cargo test -p kagari-embed --test native_preparation <affected_filter>
cargo test -p kagari-runtime --lib <affected_filter>
cargo check -p kagari-embed --no-default-features --features native --lib
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
git diff --check
```

Use --no-fail-fast when running multiple selected targets. At each implementation
checkpoint manually review imports/reexports, semantic ownership, unsafe blocks,
root/borrow lifetimes, cancellation, LOC and migration removals. Use normal Cargo
profiles, target directory and default parallelism. Run scoped Clippy when relevant.
Documentation-only checkpoints need link/content/diff checks, not Cargo builds.

CI owns full workspace tests/Clippy, the complete source/native/source-free feature
matrix and optional CLI JIT tests. Existing CI currently uses Ubuntu; J06 must
explicitly establish native coverage on Windows x86_64 and Linux x86_64, and record
other target support as unverified unless exercised. Do not claim host ISA discovery
alone establishes portability. Native-only production dependencies must remain
free of source analysis; use the existing feature-consumer checks in CI.

For native acceptance, assert execution selection for the tested function and
relevant script callees using bounded test diagnostics, not a new public profiling
subsystem. Report helper use and deliberate mixed execution separately. Use
parameterized/non-foldable input or disabled MIR optimization where needed, plus
representative optimized execution, so constant folding cannot fake support.
Compare values, error kind/origin, completed effects, roots/frame cleanup and
generation behavior, including forced GC and cancellation. Unrun CI is pending,
not a passing local check; unresolved required rows prevent completion.

## Progress ledger

- [x] Record bounded plan, baseline evidence, phase order and ownership.
- [ ] J01: exact ABI/inventory decisions and scalar argument entry acceptance.
- [ ] J02: scalar numeric/control-flow coverage and focused validation.
- [ ] J03: checked calls, preparation closure and mixed-execution acceptance.
- [ ] J04: rooted values, aggregates and lifecycle acceptance.
- [ ] J05: closures/interfaces/host/library coverage and reentry acceptance.
- [ ] J06: inventory closure, focused final validation, measurements and docs.
- [ ] CI: complete feature/backend and declared platform coverage.

Carried implementation errors: none; implementation has not started. No speedup
measurements or new test results are claimed. Exact physical ABI and call-driver
mechanics are J01/J03 design checkpoints under the contracts above, not reasons to
silently reduce coverage. Record newly demonstrated blockers and bounded scope
decisions here. Implementation commits use Conventional Commits and a
`Plan-Phase: J01` (or the corresponding phase) trailer.
