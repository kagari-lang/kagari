# Kagari Baseline JIT Specification

The JIT is an optional function compiler. Language semantics, verified MIR and the
shared runtime contracts govern both it and the bytecode interpreter. Native code
must preserve results, traps, logical budgets, completed effects and version identity.
The exact compilation/installation API is specified in
[codegen-backend.md](codegen-backend.md).

## Input and Ownership

`kagari-codegen-cranelift` implements the compilation-only `CodegenBackend` trait in
`kagari-codegen`. It accepts a function selected from verified MIR, its sealed
analysis facts and explicit ABI/link descriptions. It does not consume bytecode or
source and cannot infer types or resolve names again. Cranelift objects remain
private to that crate.

Compiler core lowers the same MIR to bytecode. For serialized native input, the SDK
verifies portable MIR and canonical correspondence to the bytecode before linking.
Invalid bytecode, malformed MIR, mismatched versions or inconsistent metadata are
errors; none authorize fallback to unvalidated execution.

The backend owns emission, legalization, target configuration and executable pages.
Runtime owns installation, invocation, frames, GC, capabilities, budgets, generations
and reload. VM owns interpreter execution and selection from a prepared entry.
SDK owns preparation/cache orchestration. The CLI selects this path only when built
with its `jit` feature and invoked with `--jit`.

## Supported Native Subset

The implemented backend supports zero-argument functions with a single return block
and Unit/Bool/i32 values: constants, moves, logical budget checkpoints, checked i32
add/subtract/multiply/negation, boolean not, supported equality/order comparisons and
return. Native tests explicitly assert native entry/status; interpreter success alone
is not evidence that a function compiled.

Locals, control flow, calls, GC-bearing values, other numeric families and
remainder/division currently use interpreter fallback. Extending this coverage is
separate follow-up work. LLVM, AOT, optimizing tiers, speculative inlining,
deoptimization, tracing JIT and persisted machine code remain outside this baseline.

## Runtime ABI and Safepoints

The scalar entry accepts opaque runtime and result pointers and returns an ABI status.
Result tags/fields and helper signatures live in `kagari-abi`; runtime Rust `Value`
layout is not a machine-code contract. Backend implementation is trusted unsafe host
code and must retain callable pages/links and avoid unwinding across the C ABI.

Every logical instruction and return visits the declared step helper with the sealed
logical offset. Runtime records the origin, observes terminal state/cancellation,
collects at the safepoint and charges the verified point before the operation runs.
Optimized MIR retains logical charges. Overflow and resource exhaustion must agree
with the interpreter in trap order, source location and completed effects.

Current scalar safepoints have empty root maps. Future GC-bearing support requires
physical maps implementing MIR's logical liveness/root facts. Missing native support
for a valid root contract selects fallback before entry. Host calls and typed paths
must preserve capability/exposure checks, scoped borrows, no-escape rules, generation
validation and mutation commit guarantees through shared runtime services.

## Preparation and Fallback

Hosts explicitly call `KagariRuntime::prepare_native`, then `execute_prepared`.
Preparation performs no script execution. It checks the exact loaded/prepared version
and policy, builds links, compiles or reuses a cached product, and installs it for the
runtime. Source and artifact routes share this flow. `execute` remains interpreter-only.

Missing portable MIR, unsupported valid operations, disabled JIT policy and an
attached execution observer select pre-entry interpreter fallback. Compiler/link
errors, invalid inputs, stale/foreign handles and failures after native entry remain
errors. Native execution never restarts in the interpreter after partial work.
Execution reports expose Native or InterpreterFallback with diagnostics.

`JitPolicy` includes future scheduling choices, but automatic compile-on-load,
first-call and threshold scheduling are not implemented. Hosts schedule explicit
preparation. Policy cannot change source-visible semantics or grant capabilities.

## Debugging

The baseline debugger is interpreter-first. Native execution with an attached
observer falls back before entering code. Line tables, spans and flags claiming safe
callbacks alone are insufficient: native observer callbacks and live value locations
must be implemented and tested before debugged execution can run natively.

## Code Lifetime and Reload

Each compilation product owns its code independently of the compiler object and
later compilations. An installed native handle binds that product to an immutable
loaded function and its retained dependency closure. It keeps old pages and versions
alive across publication, active calls and backend/prepared-program destruction.

Reload validates a candidate and publishes a new epoch without retargeting old
handles. Calls through the latest published entry use the new version; explicit
retained old handles execute their original version. Mixing a prepared native entry
with another function/version is rejected. Failed reload leaves current entrypoints
unchanged. Interpreter cache invalidation is separate from native page ownership.

## Acceptance

- Native-supported cases match interpreter values, traps, origins and every relevant
  logical budget boundary, for source and encoded artifacts.
- Unsupported and observer/policy cases select fallback before any native effects.
- Native failures preserve traces and do not restart execution.
- GC, cancellation, budget exhaustion and faults release roots and frames.
- Old/new native entries retain their exact code and dependency versions across reload.
- Native-only artifact consumers compile and run without frontend dependencies.
- Workspace checks and optional feature tests pass; follow-up coverage expansion must
  preserve the same runtime safety and semantic boundaries.
