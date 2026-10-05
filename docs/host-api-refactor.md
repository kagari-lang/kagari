# Host API Unification Refactor Plan

Status: queued, documentation only. This plan gives embedding applications one
consistent path for registering interfaces, preparing code, loading scripts,
calling functions and publishing reloads. It composes existing execution boundaries
and the queued interoperability work rather than replacing them with another VM.
API names and examples are proposals, not currently callable interfaces.

The outer-tuple argument convention below is explicitly confirmed by the user.
The object model and latest-version function handles are the proposed design to
review at activation. This document authorizes neither implementation nor commits.
Existing specifications remain authoritative until implementation updates them.

The later [runtime ownership and host object design](runtime-ownership-and-host-api-design.md)
owns automatic roots, common typed calls/conversion, object access and the Send
runtime contract. HA consumes that foundation. Its Function below remains a
proposed logical latest-version entry; GO PinnedFunction handles retain one checked
version and do not activate HA's update policy.

The later [package proposal](package-design.md) refines package identity and inputs.
The [update model](update-model-design.md) owns the compatible-update versus state-
replacement boundary and refines the compatibility gates below. These are contract
handoffs, not circular requirements to finish both implementations first.

## Dependencies and scope

Build on current [native registration](spec/standard-declarations.md) and
[execution control](spec/execution.md). The queued
[runtime ownership design](runtime-ownership-and-host-api-design.md) supplies
conversion/root/call contracts; [Rust interoperability](rust-interop-design.md)
extends them with DTO derives, Serde and external opaque ownership.
The [roadmap](implementation-roadmap.md) owns activation and scheduling.

This track owns the coherent host-facing facade, default workflows, call argument
adapter and reload-aware entry handles. GO owns common conversion/retention and
object APIs; Rust interop owns DTO/Serde/opaque extensions; execution policy owns
resource semantics; native providers own linked invocation. Reuse those
implementations and contracts.

Synchronous acceptance does not require implementing async. The
[async execution](async-execution-design.md) and [host task scope](host-task-scope-design.md)
proposals remain responsible for task ownership, suspension and scheduling. They
must reuse the final loading, type and version model when integrated. This plan does
not silently add another prerequisite to their already queued execution order.

## Current API inventory

This inventory describes the existing SDK concepts. Re-audit concrete names and
behavior at activation; it is not a current build report or the proposed facade.

| Area | Current surface and limitation |
| --- | --- |
| Source preparation | [Engine source APIs](../crates/kagari-embed/src/engine/source.rs) expose source mutation, overlays, analysis queries, `CheckedModule`, artifact emission and `compile_to_artifact` |
| Program preparation | [PreparedProgram](../crates/kagari-embed/src/program/mod.rs) validates artifacts/native input before loading; it uses `Rc` and an interior-mutable native cache, not a thread-transferable public program |
| Runtime facade | [KagariRuntime](../crates/kagari-embed/src/runtime.rs) exposes registration, load/reload, raw runtime access and `execute`/`execute_prepared`; both execution methods reject nonempty argument slices |
| Call configuration | [ExecutionContext](../crates/kagari-embed/src/context.rs) combines language/authority, resources, host exposure, JIT, tracing, deterministic inputs and panic policy; some JIT modes are declared but rejected by ordinary execution |
| Native preparation | [Native APIs](../crates/kagari-embed/src/program/native.rs) expose backend preparation and runtime installation separately from ordinary execution |
| Errors | [Embedding errors](../crates/kagari-embed/src/error.rs) classify several stages, while preparation and registration also return other error types |
| Reload | [Runtime loading](../crates/kagari-runtime/src/loading.rs) stages, validates and publishes candidates; the SDK returns a new version-bound `LoadedModule`, not a stable latest-version logical entry |

The common workflow exposes `CheckedModule`, artifact construction,
`PreparedProgram::from_artifact`, `LoadedModule`, raw `Value` and `ExecutionReport`.
Some compilation steps are already combined; the problem is their presence in the
normal embedding workflow, not the existence of separate internal phases. An
in-memory artifact object is not necessarily a serialized byte round trip.

The [module activation contract](spec/module-activation.md) already guarantees
validation before publication, no implicit execution on load, candidate isolation
and root-pinned dependency versions. Preserve these foundations.

## Public object model

| Object | Host meaning | Ownership boundary |
| --- | --- | --- |
| `Engine` | Prepare code from source or an artifact | Compilation/preparation settings, declarations and relevant caches; not Actor business state |
| `Program` | Prepared executable code and its dependency closure | Independent of a specific runtime's heap, installed callbacks and object handles |
| `Runtime` | One independent execution environment | Heap, host bindings, installations, execution protection and runtime-local native links |
| `Script` | A stable logical installation in one runtime | Selects the active program generation for new root calls |
| `Function` | A checked exported callable identity within a Script | Retains runtime/installation identity and expected contract, not an unchecked slot number |

The first four objects describe the workflow; `Function` is an entry handle, not
another execution environment. A Script represents an installed program closure,
not necessarily one source file. Source/module identity, an installation label and
a program generation are separate concepts. Labels must not rewrite compiled
module identities or make incompatible code look reload-compatible.

Runtime mutation and execution stay explicit at the call site. Function handles
must not require a long-lived mutable borrow that prevents later calls or reloads.
No hidden global runtime, mandatory mutex, or implicit multi-threaded VM is added.
Raw runtime escape hatches are advanced facilities, not prerequisites for ordinary
registration, calls, cancellation or reload. They cannot bypass handle validation.

## Ordinary host workflow

The binding definitions and `DamageRequest`/`DamageResult` below come from the
Rust interop plan. All snippets are target API sketches; builder names may change.

```rust
let engine = Engine::builder()
    .api(host_bindings.declarations())
    .build()?;

let program = engine.compile(Source::file("scripts/battle.kgr"))?;

let mut runtime = engine.runtime()
    .bindings(host_bindings)
    .build()?;

let script = runtime.load(&program)?;
let calculate = runtime.function(&script, "calculate")?;
let result: DamageResult = runtime.call(&calculate, (request,))?;
```

Host API definitions are authored once. Offline declarations are a projection of
the same binding descriptor, not a second signature catalog. Separate Actors can
install different callback captures and objects satisfying the same interface.
Do not store every Actor's host state inside a shared Engine for convenience.

Registration follows the Rust interop module naming rules. Runtime construction or
installation validates the declared interface against supplied implementations.
Linking a Program validates its actual requirements before publication. Script code
cannot add trusted bindings by spelling a declaration or attribute.

## Source and artifact preparation

Source text, files and prebuilt artifacts converge on Program:

```rust
let from_text = engine.compile(Source::text("battle.kgr", text))?;
let from_file = engine.compile(Source::file("scripts/battle.kgr"))?;
let from_artifact = engine.prepare(&artifact_bytes)?;
```

All use the same subsequent load/call/reload workflow. Artifact-only builds retain
preparation and execution without linking source analysis. Source builds can export
deployment artifacts through an explicit build API. In-process compilation should
not require encoding and decoding bytes just to obtain a Program, but must retain
equivalent verification and native-input correspondence checks.

Preserve explicit source names for diagnostics and logical module identities for
imports. Multi-file compilation uses a coherent source snapshot and a declared
resolver/source set; it must not hide arbitrary filesystem/network discovery.
Compilation checks the complete reachable dependency closure, including failures
outside the root file. User-supplied paths and artifact lengths remain validated.

Source overlays and semantic queries belong to an explicit tooling/workspace
surface. Normal file/text compilation must not unexpectedly compile an unrelated
editor overlay. An advanced snapshot-based compile entry can deliberately select
that overlay. This is a behavior change from the current shared source database
shortcut and requires tests and specification updates, not merely a rename.

Compilation/preparation does not execute script functions or Native handlers.
Load only links and publishes validated code; it does not call `main` or infer an
initializer. Optional `load_source` convenience, if justified, composes the same
compile/load path and must document that it performs synchronous preparation.

Compilation can be separated from the Actor's message processing, but this does
not make today's Engine or PreparedProgram `Send`/`Sync`. A cross-thread workflow
must use an explicitly transferable artifact or a separately proven shareable code
representation. Do not add unsafe thread traits to the existing `Rc`/`RefCell`
model. Keep verification of transferred input and account for preparation latency.

## Confirmed argument and return rules

The outermost Rust tuple always denotes the argument list. Every element is exactly
one script argument and uses the interop plan's ordinary value conversion.

| Rust expression passed as args | Script arguments |
| --- | --- |
| `()` | No arguments |
| `(request,)` | One struct argument |
| `(a, b)` | Two arguments |
| `((a, b),)` | One tuple-valued argument |
| `((),)` | One unit-valued argument |
| `((a, b), enabled)` | One tuple argument followed by one boolean argument |

Conceptually the normal call API is:

```rust
fn call<R: FromKagari>(
    &mut self,
    function: &Function,
    args: impl IntoArgs,
) -> Result<R>;
```

`IntoArgs` is a dedicated argument-list adapter for `()` and supported tuple
arities. Its tuple elements require `IntoKagari`. Do not add a blanket single-value
implementation for all `T: IntoKagari`, a `single(...)` wrapper, recursive argument
flattening, or a heuristic based on the target signature. A Rust tuple converted
through `IntoKagari` remains one script tuple; only the explicit outer argument
adapter expands a tuple into parameters. Document the supported tuple arity and
test its maximum boundary when implementing it.

These examples should fail to compile as normal argument lists:

```rust
runtime.call::<DamageResult>(&calculate, request)?;
runtime.call::<()>(&takes_array, vec![1_i32, 2])?;
```

Their accepted forms are `(request,)` and `(vec![1_i32, 2],)`. Dynamic callers can
use a separately named checked dynamic API, not a vector overload whose meaning
changes between an array value and a parameter list.

Kagari returns one value. A tuple return is converted as one tuple through
`FromKagari`; there is no Lua-style multiple-return expansion or `FromKagariMulti`.
Unit converts to `()`. A script business `Result<T, E>` remains data inside the SDK
call result; runtime errors remain a distinct outer error channel.

## Function resolution and call validation

Ordinary lookup resolves an exported callable and records its declaration contract:

```rust
let calculate = runtime.function(&script, "calculate")?;
let result = runtime.call::<DamageResult>(&calculate, (request,))?;
```

Rust infers parameter types from the outer tuple and the return type from `R` or
the receiving variable. It does not prove compatibility with separately loaded
script code at Rust compilation time. The runtime validates arity, parameter
representations and expected return type before executing the target function.
Normal calls must reject private/non-callable declarations and foreign handles.

Select and pin a coherent program generation before boundary conversion. Check the
host call signature, convert/root inputs, invoke, then convert the rooted result.
A static return-type mismatch must fail before script effects; a data-dependent
output conversion failure can occur afterward and cannot undo completed effects.
Custom host conversion code must obey the same reentry and lifetime contracts.

Signature/entry caches may be reused only with the matching runtime installation,
program generation, binding revision and Rust-side mapping. Reload must invalidate
or safely re-resolve cached slots. An optional explicit typed binding may validate
a declared Rust function signature during startup; it is not mandatory boilerplate
for every lookup. Its exact API is a review gate.

Direct calls return the business value, not a mandatory `ExecutionReport`. Optional
observers/reporting provide execution statistics. Advanced dynamic results must
retain script objects through checked rooted handles; exposing a raw unrooted GC
value as a safe long-lived result is not acceptable.

## Reload and version selection

The proposed default is a stable Script and a latest-version Function entry for
new root calls:

```rust
let next = engine.compile(Source::file("scripts/battle.kgr"))?;
runtime.reload(&script, &next)?;
let result: DamageResult = runtime.call(&calculate, (request,))?;
```

Review and confirm this policy at activation. It is not a claim about the current
version-bound LoadedModule API. The intended rules are:

1. Compile/prepare the replacement without changing active code.
2. Validate the target installation, expected base generation, public contracts,
   dependency closure and required host bindings; stage a candidate privately.
3. Publish only after all checks succeed, atomically for that runtime installation.
4. New root calls through existing logical handles select the published generation.
5. Existing root executions retain their full dependency version set until finished.

Ordinary static calls, Native reentry and future async resumes retain their pinned
execution context, never resolve a latest-version entry midway through execution.
Retained trait objects and closures explicitly enter their own pinned implementation
context as specified by the update model; this does not create fresh root protection.
A public root-call API must not silently reset budgets/versions on reentry; Native
callbacks use the dedicated checked call context.

Compatible reload preserves published callable identities and contracts. Removing
an entry or incompatibly changing its argument/return representation must reject
ordinary reload, not leave a previously acquired Function silently pointing at a
different function. The update model proposes freezing all existing named contracts,
allowing new types/definitions, and rejecting new trait impls for old concrete types.
Finalize its complete descriptor policy before implementation, including private
named definitions, opaque/value types and dependent modules.

Any failed preparation, linking, compatibility check or stale-base publication
leaves the active version usable. Existing candidate isolation, generation and
reference validation remain mandatory. Ordinary reload runs no candidate code.
Advanced staged validation is a separate opt-in API. State replacement is a distinct
host lifecycle: quiesce writers, export explicit data, restore a fresh environment
and switch routing. It never happens automatically when compatible reload fails and
cannot promise rollback of arbitrary host effects. Old runtime/function handles do
not transfer to that new environment.

Explicit old-version snapshots may be exposed for advanced use. They must be named
as pinned versions rather than hidden behind the ordinary Function handle. Old
generations remain alive while pinned by executions or retained objects/handles,
and are reclaimed after those owners release them. Cross-Actor publication is a
host coordination problem, not a global atomicity promise of `reload`.

Finalize installation lifetime at activation: dropping a handle must not
accidentally cancel work, while any explicit unload must reject subsequent root
calls and specify treatment of pinned work. Neither a recycled label nor a slot
index may revive an invalid handle. Do not add unload/cancellation behavior merely
as an incidental consequence of this facade refactor.

## Configuration and backend policy

Place options at the lifetime where they belong:

| Configuration | Responsibility |
| --- | --- |
| Engine/build | Source resolution, compiler limits, artifact preparation and backend configuration |
| Runtime | Host implementations, call-depth limits, installed backend policy and diagnostics |
| Call | Root cancellation and genuinely call-specific deterministic inputs |

Normal calls use runtime defaults. A `call_with` variant may take focused
`CallOptions`; it must distinguish an absent override from an explicit setting.
Do not recreate ExecutionContext's per-call permission/JIT/host-exposure bundle or
reset nested call-depth/cancellation state. Preserve installation-based access;
host services own admission and deadlines, without generic runtime work quotas.

Interpreter and native execution use one call surface and identical boundary
validation. Backend preparation, helper linking, cache ownership and optional
prewarming remain explicit internally or through advanced APIs. Do not expose
strategy variants that silently do nothing. Define fallback behavior for unsupported
functions separately from genuine native compilation/validation errors. Hot-path
compilation latency must be controlled by the selected, actually implemented policy.

## Errors and diagnostics

Use one SDK error family for normal prepare/load/call/reload operations while
preserving structured causes and useful stage distinctions. Cover source diagnostics,
artifact validation, host binding mismatch, reload conflict, boundary conversion,
script traps, cancellation and protection exhaustion. Preserve file/module origin,
revision-aware spans, error codes and script/Native traces as applicable.

Do not flatten everything into a string or report a script business `Err` as a trap.
Document the distinction between a pre-execution failure and a failure after
completed script/host effects. A host panic policy must have implemented behavior;
it cannot promise recovery from aborts or leave a corrupted runtime executable.
Reporting/statistics are optional and must not require precise hot-path charging.

## Tooling and future asynchronous calls

Move source editing, overlays and semantic queries behind a cohesive tooling
surface rather than making them prerequisites for loading a script. Preserve query
snapshot ownership and diagnostics; do not delete valuable analysis APIs merely to
make the ordinary facade small. Backend and tooling APIs retain their feature gates.

Future async entry points reuse Script/Function identity, outer-tuple arguments,
conversion and root version rules. They create scope-owned executions through the
async design, not a second loader or a second host interface catalog. Synchronous
`call` must not block the Actor thread waiting for external RPC; reject incompatible
async entry usage before starting it. Async admission establishes version ownership
according to the async task contract; later polls/resumes cannot switch generations.

No Tokio dependency is introduced in the core SDK by this facade plan. GO owns the
Send runtime and callback/payload transfer requirements; it does not require Sync
for exclusively accessed runtime state. Background workers may deliver owned
validated completion data; only the owning scheduler drives script execution.

## Implementation phases

All phases are unstarted. Re-audit predecessor APIs before choosing changes and
reuse accepted GO typed call/root contracts rather than rebuilding them.

- [ ] HA00: Confirm latest-version entry policy, public compatibility rules,
  handle lifetimes, tuple arity, configuration precedence, error/panic behavior and
  supported backend strategies. Establish source/artifact and external-consumer tests.
- [ ] HA01: Introduce the cohesive Engine/Program preparation surface and shared
  binding descriptors. Separate normal sources from tooling snapshots; retain
  artifact-only support and verification without mandatory serialization round trips.
- [ ] HA02: Introduce runtime-owned Script/Function identities, ordinary loading and
  IntoArgs-based typed calls using RI conversions. Implement nonempty arguments,
  preflight signature checks, rooted outputs and focused call options.
- [ ] HA03: Integrate UP's compatible publication through stable logical handles and
  generation-aware entry caching. Preserve staged validation, candidate cleanup,
  dependency pinning and reentry; keep state replacement a separate lifecycle API.
- [ ] HA04: Unify normal errors and backend call dispatch, finish the tooling/advanced
  API boundaries, and migrate SDK examples, CLI and embedding consumers. Remove
  superseded public wrappers rather than keeping compatibility-only duplicates.
- [ ] HA05: Update embedding, loading, activation and execution specifications and
  cross-check queued async/interop designs. Run final feature/behavior acceptance.

HA does not repeat RI's converter/derive work or EP's permission/budget migration.
Any new generic execution capability belongs in its actual owning layer, not in an
SDK workaround. The default checkpoint policy is buildable changes; exceptions need
explicit activation-time approval and a ledger owner. Record checks and failures
here. Authorized future commits follow repository Conventional Commit rules and
breaking-change notes; this documentation task creates no commit.

## Acceptance and validation

Required behavioral coverage:

- Text, file, explicit snapshot and artifact inputs converge on equivalent checked
  execution, with multi-module errors retaining their own source locations.
- Artifact-only builds load and execute without source analysis; invalid artifacts
  fail before linking or effects. Native-enabled builds reject native-input
  correspondence mismatches; bytecode-only builds retain the specified opaque
  payload envelope/bounds checks without pretending to verify MIR semantics.
- One Program can be installed with independent host state in multiple runtimes;
  foreign Script/Function/value handles are rejected.
- Zero, one, multiple, tuple-valued, unit-valued and mixed opaque/value arguments
  obey the outer-tuple rule. Include compile-fail tests for bare structs/vectors,
  tuple arity limits and unsupported mappings; tuple results never spread.
- Argument and expected return signature mismatches precede script effects; data
  conversion failures preserve cleanup and already-completed effects correctly.
- Existing Function handles call the new generation after compatible publication;
  pinned roots/reentry remain old. Failed/incompatible/stale reload leaves old
  calls usable; publication cannot mix dependency generations or cached slots.
- Load/reload performs no implicit script initialization. Candidate validation
  retains existing isolation and cleanup tests.
- Runtime defaults and call overrides are deterministic; nested work cannot reset
  root protection. Cancellation, traps and GC leave no leaked temporary roots.
- Interpreter and supported native paths share the public call API and results;
  unsupported-backend fallback does not conceal invalid code or runtime failures.
- Main examples require only the embedding facade and application bindings, not
  raw Value construction, manual sessions, LoadedModule internals or Native entries.

Run relevant predecessor and SDK feature matrices, including source-free and native
configurations, external-consumer examples and compile-fail cases. Final checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Measure repeated calls and reload cache invalidation separately from compilation.
Record toolchain, machine, profile, features, default Cargo parallelism, cache state
and workload. Preserve conclusions here and temporary output under ignored `target/`.
No performance or working-build claim is made by this design-only document.
